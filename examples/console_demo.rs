use chrono::{Duration, Utc};
use payment_gateway::accounts;
use payment_gateway::api::{self, AppState};
use payment_gateway::crypto::Sealer;
use payment_gateway::db;
use payment_gateway::ledger::{self, AccountKind, AccountRef, Entry, Posting, TxKind};
use payment_gateway::mailer::DisabledMailer;
use payment_gateway::providers::{
    DefaultFactory, DisputeState, EventSink, ProviderEvent, Registry,
};
use payment_gateway::rates::StaticRates;
use payment_gateway::service::{ProviderSink, Service};
use serde_json::{Value, json};
use std::sync::Arc;

const ADMIN: &str = "demo-admin-token-demo-admin-token";
const PASSWORD: &str = "correct horse battery";

struct Demo {
    service: Arc<Service>,
    app: axum::Router,
    pool: sqlx::MySqlPool,
}

impl Demo {
    async fn call(&self, method: &str, path: &str, key: Option<&str>, body: Value) -> Value {
        use tower::ServiceExt;
        let mut req = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json");
        if let Some(k) = key {
            req = req.header("authorization", format!("Bearer {k}"));
        }
        let body = if body.is_null() {
            axum::body::Body::empty()
        } else {
            axum::body::Body::from(body.to_string())
        };
        let resp = self
            .app
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = http_body_util::BodyExt::collect(resp.into_body())
            .await
            .unwrap()
            .to_bytes();
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert!(status.is_success(), "{method} {path}: {status} {v}");
        v
    }

    async fn pay(&self, tenant: &db::Tenant, body: Value) -> String {
        let input: payment_gateway::model::CreatePayment = serde_json::from_value(body).unwrap();
        let (row, _) = self
            .service
            .create_payment(tenant, input, None)
            .await
            .unwrap();
        hex::encode(&row.id)
    }

    async fn attempt(&self, tenant: &db::Tenant, id: &str, body: Value) -> Value {
        let input: payment_gateway::model::CreateAttempt = serde_json::from_value(body).unwrap();
        let pid = db::parse_payment_id(id).unwrap();
        let (_, a) = self
            .service
            .create_attempt(tenant, &pid, input)
            .await
            .unwrap();
        serde_json::to_value(payment_gateway::model::AttemptView::from(&a)).unwrap()
    }

    async fn event(&self, provider_id: u64, ev: ProviderEvent) {
        ProviderSink {
            service: self.service.clone(),
            provider_id,
        }
        .event(ev, None)
        .await
        .unwrap();
    }

    async fn shift(&self, id: &str, minutes: i64) {
        let pid = db::parse_payment_id(id).unwrap();
        let p = &pid[..];
        let run = |sql: &'static str| sqlx::query(sql);
        run("update payment set created = created - interval ? minute, expires = expires - interval ? minute, paid_at = paid_at - interval ? minute where id = ?")
            .bind(minutes).bind(minutes).bind(minutes).bind(p)
            .execute(&self.pool).await.unwrap();
        run("update payment_attempt set created = created - interval ? minute, expires = expires - interval ? minute, paid_at = paid_at - interval ? minute where payment_id = ?")
            .bind(minutes).bind(minutes).bind(minutes).bind(p)
            .execute(&self.pool).await.unwrap();
        run("update gateway_event set created = created - interval ? minute where payment_id = ?")
            .bind(minutes)
            .bind(p)
            .execute(&self.pool)
            .await
            .unwrap();
        run("update webhook_delivery set created = created - interval ? minute, next_attempt = next_attempt - interval ? minute where payment_id = ?")
            .bind(minutes).bind(minutes).bind(p)
            .execute(&self.pool).await.unwrap();
        run("update attempt_receipt r join payment_attempt a on a.id = r.attempt_id set r.created = r.created - interval ? minute where a.payment_id = ?")
            .bind(minutes).bind(p)
            .execute(&self.pool).await.unwrap();
        run("update refund set created = created - interval ? minute where payment_id = ?")
            .bind(minutes)
            .bind(p)
            .execute(&self.pool)
            .await
            .unwrap();
        run("update ledger_entry e join ledger_transaction t on t.id = e.transaction_id join payment_attempt a on t.reference_kind = 'attempt' and a.id = cast(t.reference_id as unsigned) set e.created = e.created - interval ? minute where a.payment_id = ?")
            .bind(minutes).bind(p)
            .execute(&self.pool).await.unwrap();
    }
}

struct Item {
    desc: &'static str,
    cents: u64,
    method: &'static str,
    customer: &'static str,
    minutes_ago: i64,
    outcome: &'static str,
}

const ITEMS: &[Item] = &[
    Item {
        desc: "VPS Dublin-3, 1 month",
        cents: 1800,
        method: "lightning",
        customer: "cus_amelia.k",
        minutes_ago: 14,
        outcome: "paid",
    },
    Item {
        desc: "VPS Frankfurt-1, 12 months",
        cents: 19200,
        method: "onchain",
        customer: "cus_tomasz.w",
        minutes_ago: 29,
        outcome: "paid",
    },
    Item {
        desc: "Credit top-up",
        cents: 5000,
        method: "platform",
        customer: "cus_priya.n",
        minutes_ago: 48,
        outcome: "paid",
    },
    Item {
        desc: "VPS Dublin-3, 1 month",
        cents: 1800,
        method: "lightning",
        customer: "cus_liam.o",
        minutes_ago: 62,
        outcome: "pending",
    },
    Item {
        desc: "IPv4 /29 block",
        cents: 4500,
        method: "platform",
        customer: "cus_noor.s",
        minutes_ago: 80,
        outcome: "paid",
    },
    Item {
        desc: "Credit top-up",
        cents: 10000,
        method: "platform",
        customer: "cus_jonas.b",
        minutes_ago: 115,
        outcome: "paid",
    },
    Item {
        desc: "VPS Dublin-3, 1 month",
        cents: 1800,
        method: "lightning",
        customer: "cus_hana.m",
        minutes_ago: 128,
        outcome: "expired",
    },
    Item {
        desc: "Dedicated host, 1 month",
        cents: 14900,
        method: "platform",
        customer: "cus_ravi.d",
        minutes_ago: 160,
        outcome: "refunded",
    },
    Item {
        desc: "VPS Frankfurt-2, 3 months",
        cents: 5400,
        method: "onchain",
        customer: "cus_elsa.v",
        minutes_ago: 196,
        outcome: "confirming",
    },
    Item {
        desc: "Credit top-up",
        cents: 2500,
        method: "lightning",
        customer: "cus_omar.f",
        minutes_ago: 250,
        outcome: "declined",
    },
    Item {
        desc: "VPS Dublin-3, 1 month",
        cents: 1800,
        method: "platform",
        customer: "cus_sofia.r",
        minutes_ago: 335,
        outcome: "paid",
    },
    Item {
        desc: "Reverse DNS add-on",
        cents: 300,
        method: "lightning",
        customer: "cus_amelia.k",
        minutes_ago: 408,
        outcome: "paid",
    },
    Item {
        desc: "Dedicated host, 1 month",
        cents: 14900,
        method: "platform",
        customer: "cus_yuki.t",
        minutes_ago: 612,
        outcome: "disputed",
    },
    Item {
        desc: "VPS Frankfurt-2, 3 months",
        cents: 5400,
        method: "onchain",
        customer: "cus_bram.d",
        minutes_ago: 400,
        outcome: "short",
    },
    Item {
        desc: "VPS Dublin-3, 1 month",
        cents: 1800,
        method: "lightning",
        customer: "cus_lena.k",
        minutes_ago: 520,
        outcome: "late",
    },
    Item {
        desc: "VPS Frankfurt-1, 12 months",
        cents: 19200,
        method: "platform",
        customer: "cus_marta.l",
        minutes_ago: 770,
        outcome: "paid",
    },
];

#[tokio::main]
async fn main() {
    let url = std::env::var("DEMO_DATABASE_URL").expect("DEMO_DATABASE_URL");
    let listen = std::env::var("DEMO_LISTEN").unwrap_or_else(|_| "127.0.0.1:18760".into());
    let public = format!("http://{listen}");
    let pool = db::connect(&url).await.unwrap();
    let sealer = Sealer::new(&[7u8; 32]);
    let factory = Arc::new(DefaultFactory::new(true));
    let registry = Registry::new(pool.clone(), sealer.clone(), factory.clone());
    let service = Service::with_mailer(
        pool.clone(),
        sealer,
        registry,
        Arc::new(StaticRates::new(&[("EUR", 89_900.0), ("USD", 97_000.0)])),
        public.clone(),
        Arc::new(DisabledMailer),
    );
    let state = AppState::new(service.clone(), ADMIN.into(), true, None);
    let app = api::router(state);
    let d = Demo {
        service: service.clone(),
        app: app.clone(),
        pool: pool.clone(),
    };

    let hash = accounts::hash_password(PASSWORD).unwrap();
    let owner = accounts::insert_account(&pool, "sam@northwind.example", &hash, true)
        .await
        .unwrap();
    sqlx::query("update account set totp_enabled = 0 where id = ?")
        .bind(owner)
        .execute(&pool)
        .await
        .unwrap();
    let t = d
        .call(
            "POST",
            "/admin/tenants",
            Some(ADMIN),
            json!({"name": "Northwind Hosting"}),
        )
        .await;
    let tid = t["id"].as_u64().unwrap();
    let key = t["api_key"].as_str().unwrap().to_string();
    accounts::add_member(&pool, tid, owner, accounts::Role::Owner)
        .await
        .unwrap();
    db::set_tenant_trusted(&pool, tid, true).await.unwrap();
    let ln = d
        .call("POST", &format!("/admin/tenants/{tid}/providers"), Some(ADMIN), json!({"method": "lightning", "provider_type": "mock", "name": "Dublin Lightning node", "config": {}}))
        .await["id"]
        .as_u64()
        .unwrap();
    let oc = d
        .call("POST", &format!("/admin/tenants/{tid}/providers"), Some(ADMIN), json!({"method": "onchain", "provider_type": "mock", "name": "Cold wallet", "config": {"min_confirmations": 3}}))
        .await["id"]
        .as_u64()
        .unwrap();
    let shared = d
        .call("POST", "/admin/shared", Some(ADMIN), json!({"name": "Platform Lightning", "method": "lightning", "provider_type": "mock", "network": "mainnet", "config": {}, "fee_bps": 100, "fee_base_msat": 0, "allow_untrusted": false}))
        .await["id"]
        .as_u64()
        .unwrap();
    let tenant = db::get_tenant(&pool, tid).await.unwrap().unwrap();
    let platform_config = service.enable_shared(&tenant, shared).await.unwrap();
    let shared_key = payment_gateway::shared::shared_key(shared);
    let receiver = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hook_url = format!("http://{}/hooks/gateway", receiver.local_addr().unwrap());
    tokio::spawn(async move {
        let app = axum::Router::new().fallback(|| async { axum::http::StatusCode::OK });
        axum::serve(receiver, app).await.unwrap();
    });
    let sealed = service.sealer.seal(b"whsec_demo").unwrap();
    let created_ep = Utc::now() - Duration::days(60);
    for url in [
        hook_url.as_str(),
        "https://ops.northwind.example/hooks/audit",
    ] {
        let id = db::insert_endpoint(&pool, tid, url, &sealed, None, None)
            .await
            .unwrap();
        sqlx::query("update webhook_endpoint set created = ? where id = ?")
            .bind(created_ep)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }
    db::set_manual_paid_allowed(&pool, tid, true).await.unwrap();

    for (i, it) in ITEMS.iter().enumerate() {
        let body = json!({"reference": format!("INV-{}", 2291 - i), "amount": it.cents, "currency": "EUR", "description": it.desc, "customer_ref": it.customer, "customer_email": format!("{}@northwind.example", it.customer.trim_start_matches("cus_")), "metadata": {"order": format!("VPS-{}", 88213 - i as u64 * 7)}});
        let id = d.pay(&tenant, body).await;
        let method = if it.method == "platform" {
            "lightning"
        } else {
            it.method
        };
        let provider = match it.method {
            "platform" => shared_key,
            "onchain" => oc,
            _ => ln,
        };
        let mut attempt_body = json!({"method": method});
        attempt_body["provider_id"] = json!(if it.method == "platform" {
            platform_config
        } else {
            provider
        });
        let a = d.attempt(&tenant, &id, attempt_body).await;
        let lookup = if method == "onchain" {
            a["details"]["address"].as_str().unwrap().to_string()
        } else {
            a["details"]["payment_hash"].as_str().unwrap().to_string()
        };
        let need = a["amount"].as_u64().unwrap();
        let paid = |amount: Option<u64>, confirmed: bool| ProviderEvent::Paid {
            lookup: lookup.clone(),
            reference: format!("{:016x}{:048x}", i as u64 + 1, (i as u64 + 7) * 2654435761),
            amount,
            confirmed,
            saved: None,
        };
        match it.outcome {
            "paid" => d.event(provider, paid(None, true)).await,
            "refunded" => {
                d.event(provider, paid(None, true)).await;
                d.call(
                    "POST",
                    &format!("/v1/payments/{id}/refund"),
                    Some(&key),
                    json!({"destination": "lnbc1demo"}),
                )
                .await;
            }
            "disputed" => {
                d.event(provider, paid(None, true)).await;
                d.event(
                    provider,
                    ProviderEvent::Dispute {
                        lookup: lookup.clone(),
                        state: DisputeState::Open,
                        reason: Some("Customer says the host was not delivered".into()),
                    },
                )
                .await;
            }
            "confirming" => d.event(provider, paid(None, false)).await,
            "short" => d.event(provider, paid(Some(need / 10 * 7), true)).await,
            "declined" => {
                d.event(
                    provider,
                    ProviderEvent::Failed {
                        lookup: lookup.clone(),
                        reason: "insufficient funds".into(),
                    },
                )
                .await
            }
            "late" => {
                d.call(
                    "POST",
                    &format!("/v1/payments/{id}/cancel"),
                    Some(&key),
                    Value::Null,
                )
                .await;
                d.event(provider, paid(None, true)).await;
            }
            "expired" => {
                sqlx::query("update payment set status = 'expired' where id = ?")
                    .bind(&db::parse_payment_id(&id).unwrap()[..])
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            _ => {}
        }
        d.shift(&id, it.minutes_ago).await;
    }

    for day in 1..=40i64 {
        let n = 3 + (day * 7 % 5);
        for k in 0..n {
            let cents = [1800u64, 4500, 5000, 19200, 300, 1800, 10000][((day + k) % 7) as usize];
            let id = d.pay(&tenant, json!({"reference": format!("H-{day}-{k}"), "amount": cents, "currency": "EUR", "description": "VPS renewal", "customer_ref": format!("cus_hist{}", (day + k) % 23)})).await;
            let a = d
                .attempt(
                    &tenant,
                    &id,
                    json!({"method": "lightning", "provider_id": ln}),
                )
                .await;
            let lookup = a["details"]["payment_hash"].as_str().unwrap().to_string();
            d.event(
                ln,
                ProviderEvent::Paid {
                    lookup,
                    reference: format!("h{day}{k}"),
                    amount: None,
                    confirmed: true,
                    saved: None,
                },
            )
            .await;
            d.shift(&id, day * 1440 - 60 * (k + 2) - 300).await;
        }
    }

    sqlx::query("update webhook_delivery d join webhook_endpoint e on e.id = d.endpoint_id set d.delivered_at = null, d.attempts = 4, d.last_status = 503, d.last_error = 'answered 503 Service Unavailable', d.last_duration_ms = 30012, d.last_attempt_at = d.created + interval 40 minute, d.next_attempt = now() + interval 20 minute where e.url like 'https://ops.%' and d.created > now() - interval 3 hour")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("update webhook_delivery d join webhook_endpoint e on e.id = d.endpoint_id set d.delivered_at = coalesce(d.delivered_at, d.created + interval 1 second), d.last_status = 200, d.last_duration_ms = 214, d.attempts = greatest(d.attempts, 1) where e.url not like 'https://ops.%'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("update webhook_delivery d join webhook_endpoint e on e.id = d.endpoint_id set d.delivered_at = coalesce(d.delivered_at, d.created + interval 1 second), d.last_status = 200, d.last_duration_ms = 96 where e.url like 'https://ops.%' and d.last_error is null")
        .execute(&pool)
        .await
        .unwrap();

    let mut tx = pool.begin().await.unwrap();
    let posting = Posting::new(
        TxKind::Adjustment,
        vec![
            Entry::new(
                AccountRef::tenant(tid, AccountKind::TenantBalance, "BTC"),
                2_350_000_000,
            ),
            Entry::new(
                AccountRef::platform(AccountKind::PlatformWallet, "BTC"),
                -2_350_000_000,
            ),
        ],
    );
    ledger::post(&mut tx, &posting, &format!("demo:opening:{tid}"))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("update ledger_entry e join ledger_transaction t on t.id = e.transaction_id set e.created = now() - interval 2 day, t.created = now() - interval 2 day where t.idempotency_key like 'demo:opening:%'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("insert into withdrawal (tenant_id, amount, currency, purpose, max_fee, fee, destination_kind, destination, destination_label, status, created, finished_at) values (?, 1800000000, 'BTC', 'payout', 0, 1200, 'lightning_address', x'00', 'treasury@northwind.example', 'completed', now() - interval 6 day, now() - interval 6 day), (?, 1650000000, 'BTC', 'payout', 0, 1100, 'lightning_address', x'00', 'treasury@northwind.example', 'completed', now() - interval 13 day, now() - interval 13 day)")
        .bind(tid)
        .bind(tid)
        .execute(&pool)
        .await
        .unwrap();
    let other = d
        .call(
            "POST",
            "/admin/tenants",
            Some(ADMIN),
            json!({"name": "Apex Strata"}),
        )
        .await;
    accounts::add_member(
        &pool,
        other["id"].as_u64().unwrap(),
        owner,
        accounts::Role::Owner,
    )
    .await
    .unwrap();
    println!("PROJECT={tid}");
    println!("LISTEN={listen}");
    let listener = tokio::net::TcpListener::bind(&listen).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
