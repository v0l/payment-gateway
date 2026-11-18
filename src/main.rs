use anyhow::{Result, bail};
use payment_gateway::api::{self, AppState};
use payment_gateway::audit::Actor;
use payment_gateway::config::{Config, encryption_keys_from_env};
use payment_gateway::crypto::Sealer;
use payment_gateway::db;
use payment_gateway::lease::{Lease, SystemClock, WORKERS_LEASE};
use payment_gateway::logging;
use payment_gateway::ops::{self, privacy::Subject, retention::RetentionPolicy};
use payment_gateway::providers::{DefaultFactory, Registry};
use payment_gateway::rates::HttpRates;
use payment_gateway::service::Service;
use payment_gateway::worker;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    logging::init(
        &std::env::var("LOG_LEVEL")
            .or_else(|_| std::env::var("RUST_LOG"))
            .unwrap_or_else(|_| "info".into()),
        std::env::var("LOG_FORMAT").map_or(true, |f| f != "text"),
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => serve().await,
        Some(cmd) => run_command(cmd, &args[1..]).await,
    }
}

async fn run_command(cmd: &str, args: &[String]) -> Result<()> {
    let url = std::env::var("DATABASE_URL")?;
    let pool = db::connect(&url).await?;
    match cmd {
        "rotate-keys" => {
            let sealer = Sealer::with_keys(&encryption_keys_from_env()?)?;
            let report = ops::rotate::rotate_keys(
                &pool,
                &sealer,
                &ops::rotate::RotateOptions::default(),
                Actor::System,
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            for (col, kid, n) in ops::rotate::sealed_kids(&pool).await? {
                println!("{col} key {kid}: {n}");
            }
        }
        "purge" => {
            let policy = RetentionPolicy::from_env()?;
            if policy.is_empty() {
                bail!("set at least one GATEWAY_RETAIN_* variable");
            }
            let report =
                ops::retention::purge(&pool, &policy, chrono::Utc::now(), Actor::System).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        "check-integrity" => {
            let findings = ops::integrity::check(&pool).await?;
            let mut bad = findings.len() as i64;
            println!("{}", serde_json::to_string_pretty(&findings)?);
            if let Ok(keys) = encryption_keys_from_env() {
                let sealer = Sealer::with_keys(&keys)?;
                let unreadable = ops::integrity::unreadable_sealed_values(&pool, &sealer).await?;
                println!("unreadable sealed values: {unreadable}");
                bad += unreadable;
            }
            if bad != 0 {
                bail!("integrity check failed");
            }
        }
        "privacy-export" | "privacy-delete" => {
            let (tenant, subject) = privacy_args(args)?;
            if cmd == "privacy-export" {
                let e = ops::privacy::export(&pool, tenant, &subject, Actor::System).await?;
                println!("{}", serde_json::to_string_pretty(&e)?);
            } else {
                let r = ops::privacy::delete(&pool, tenant, &subject, Actor::System).await?;
                println!("{}", serde_json::to_string_pretty(&r)?);
            }
        }
        other => bail!(
            "unknown command {other}: serve, rotate-keys, purge, check-integrity, privacy-export, privacy-delete"
        ),
    }
    Ok(())
}

fn privacy_args(args: &[String]) -> Result<(u64, Subject)> {
    let usage = "usage: <tenant id> --email <email> | --customer-ref <ref>";
    let [tenant, flag, value] = args else {
        bail!(usage);
    };
    let tenant = tenant.parse()?;
    let subject = match flag.as_str() {
        "--email" => Subject::Email(value.clone()),
        "--customer-ref" => Subject::CustomerRef(value.clone()),
        _ => bail!(usage),
    };
    Ok((tenant, subject))
}

async fn termination() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => futures::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = futures::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}

async fn serve() -> Result<()> {
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(2);
        }
    };
    logging::init(&config.log_level, config.log_json);
    logging::register_secret(&config.admin_token);
    if let Some(t) = &config.metrics_token {
        logging::register_secret(t);
    }
    for (_, key) in &config.encryption_keys {
        logging::register_secret(&hex::encode(key));
    }
    let pool = db::connect_with(
        &config.database_url,
        config.db_max_connections,
        config.db_acquire_timeout,
    )
    .await?;
    let sealer = Sealer::with_keys(&config.encryption_keys)?;
    let registry = Registry::new(
        pool.clone(),
        sealer.clone(),
        Arc::new(DefaultFactory::new(config.allow_mock)),
    );
    let retention = RetentionPolicy::from_env()?;
    if !retention.is_empty() {
        let pool = pool.clone();
        tokio::spawn(async move {
            loop {
                match ops::retention::purge(&pool, &retention, chrono::Utc::now(), Actor::System)
                    .await
                {
                    Ok(r) => log::info!("retention purge: {r:?}"),
                    Err(e) => log::error!("retention purge failed: {e:#}"),
                }
                tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
            }
        });
    }
    let mailer = payment_gateway::mailer::from_env()?;
    let lease = Arc::new(Lease::new(
        pool.clone(),
        WORKERS_LEASE,
        &config.instance_id,
        config.lease_ttl,
        Arc::new(SystemClock),
    ));
    let service = Service::with_mailer(
        pool,
        sealer,
        registry,
        Arc::new(HttpRates::mempool()),
        config.public_url.clone(),
        mailer.unwrap_or_else(|| Arc::new(payment_gateway::mailer::DisabledMailer)),
    );
    let sat_per_vbyte = config.payout_policy.onchain_sat_per_vbyte;
    service.payouts.set_policy(config.payout_policy.clone());
    if let Some(lnd) = &config.payout_lnd {
        let node = payment_gateway::payouts::LndPayoutNode::connect(
            &lnd.url,
            std::path::Path::new(&lnd.cert_path),
            std::path::Path::new(&lnd.macaroon_path),
            sat_per_vbyte,
        )
        .await?;
        service.payouts.set_node(Some(Arc::new(node)));
    }
    let sandbox = match payment_gateway::sandbox::SandboxConfig::from_env()? {
        Some(cfg) => Some(payment_gateway::sandbox::Sandbox::from_config(
            cfg,
            service.clone(),
        )?),
        None => None,
    };
    let gateway_ops = api::ops::Ops {
        metrics_token: config.metrics_token.clone(),
        body_limit: config.body_limit,
        request_timeout: config.request_timeout,
        instance: config.instance_id.clone(),
        ..Default::default()
    };
    let _ = gateway_ops.lease.set(lease.clone());
    let mut state = AppState::new(
        service.clone(),
        config.admin_token.clone(),
        config.allow_mock,
        sandbox,
    )
    .with_ops(gateway_ops);
    state.operator_2fa_grace = config.operator_2fa_grace;
    let shutdown_state = state.ops.clone();

    let leader = tokio::spawn(worker::lead(
        service.clone(),
        config.worker_tick,
        lease,
        shutdown_state.shutdown.subscribe(),
    ));

    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    log::info!("listening on {} as {}", config.listen, config.instance_id);
    let drain = shutdown_state.clone();
    let serve = axum::serve(listener, api::router(state)).with_graceful_shutdown(async move {
        termination().await;
        log::info!("shutdown requested, draining");
        drain.begin_shutdown();
    });
    let serve = tokio::spawn(async move { serve.await });
    let mut shutdown = shutdown_state.shutdown.subscribe();
    let _ = shutdown.wait_for(|down| *down).await;

    let grace = config.shutdown_grace;
    if tokio::time::timeout(grace, async {
        let _ = serve.await;
        let _ = leader.await;
    })
    .await
    .is_err()
    {
        log::error!("shutdown grace of {grace:?} exceeded, exiting");
    }
    log::info!("stopped");
    Ok(())
}
