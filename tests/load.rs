mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::api::{self, AppState};
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::ratelimit::{RateConfig, RateLimits, Rule};
use payment_gateway::service::ProviderSink;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_f64(name: &str) -> Option<f64> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn creation_and_settlement_throughput_meets_the_target() {
    let payments = env_usize("LOAD_PAYMENTS", 60);
    let create_concurrency = env_usize("LOAD_CREATE_CONCURRENCY", 32);
    let settle_concurrency = env_usize("LOAD_SETTLE_CONCURRENCY", 50);

    let h = harness().await;
    let t = tenant(&h).await;
    let big = Rule::new(1_000_000, 1_000_000.0);
    let mut state = AppState::new(h.service.clone(), ADMIN.into(), true, None);
    state.rate_limits = Arc::new(RateLimits::new(RateConfig {
        api_key: big,
        api_ip: big,
        pay_ip: big,
        pay_attempt_ip: big,
        signup_ip: big,
        login_ip: big,
        hook_ip: big,
        reset_ip: big,
        reset_email: big,
        token_confirm_ip: big,
    }));
    let h = Harness {
        app: api::router(state),
        ..h
    };
    let ln = provider(&h, &t, "lightning", json!({}), None).await;

    let gate = Arc::new(Semaphore::new(create_concurrency));
    let started = Instant::now();
    let mut tasks = vec![];
    for n in 0..payments {
        let permit = gate.clone().acquire_owned().await.unwrap();
        let (app, key) = (h.app.clone(), t.key.clone());
        tasks.push(tokio::spawn(async move {
            let begin = Instant::now();
            let (s, p) = call(
                &app,
                "POST",
                "/v1/payments",
                Some(&key),
                json!({"reference": format!("load-{n}"), "amount": 1000 + n as u64, "currency": "EUR"}),
            )
            .await;
            assert_eq!(s, StatusCode::CREATED, "{p}");
            let id = p["id"].as_str().unwrap().to_string();
            let (s, r) = call(
                &app,
                "POST",
                &format!("/v1/payments/{id}/attempts"),
                Some(&key),
                json!({"method": "lightning"}),
            )
            .await;
            assert_eq!(s, StatusCode::OK, "{r}");
            drop(permit);
            (id, begin.elapsed().as_secs_f64() * 1000.0)
        }));
    }
    let mut ids = vec![];
    let mut latencies = vec![];
    for task in tasks {
        let (id, ms) = task.await.unwrap();
        ids.push(id);
        latencies.push(ms);
    }
    let create_secs = started.elapsed().as_secs_f64();
    let creations_per_sec = payments as f64 / create_secs;
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let lookups: Vec<String> = sqlx::query_scalar("select lookup from payment_attempt")
        .fetch_all(&h.pool)
        .await
        .unwrap();
    assert_eq!(lookups.len(), payments);

    let gate = Arc::new(Semaphore::new(settle_concurrency));
    let started = Instant::now();
    let mut tasks = vec![];
    for lookup in lookups {
        let permit = gate.clone().acquire_owned().await.unwrap();
        let sink = ProviderSink {
            service: h.service.clone(),
            provider_id: ln,
        };
        tasks.push(tokio::spawn(async move {
            sink.event(
                ProviderEvent::Paid {
                    lookup: lookup.clone(),
                    reference: lookup,
                    amount: None,
                    confirmed: true,
                    saved: None,
                },
                None,
            )
            .await
            .unwrap();
            drop(permit);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let settle_secs = started.elapsed().as_secs_f64();
    let settlements_per_sec = payments as f64 / settle_secs;

    for id in &ids {
        assert_eq!(get_payment(&h, &t, id).await["status"], "paid");
    }
    let findings = payment_gateway::ops::integrity::check(&h.pool)
        .await
        .unwrap();
    assert!(findings.is_empty(), "{findings:?}");

    println!(
        "LOAD payments={payments} create_concurrency={create_concurrency} settle_concurrency={settle_concurrency}"
    );
    println!(
        "LOAD creations_per_sec={creations_per_sec:.1} create_p50_ms={:.1} create_p95_ms={:.1} create_p99_ms={:.1}",
        percentile(&latencies, 0.5),
        percentile(&latencies, 0.95),
        percentile(&latencies, 0.99)
    );
    println!("LOAD settlements_per_sec={settlements_per_sec:.1}");

    if let Some(min) = env_f64("LOAD_MIN_CREATIONS_PER_SEC") {
        assert!(creations_per_sec >= min, "{creations_per_sec} < {min}");
    }
    if let Some(min) = env_f64("LOAD_MIN_SETTLEMENTS_PER_SEC") {
        assert!(settlements_per_sec >= min, "{settlements_per_sec} < {min}");
    }
}
