use anyhow::{Result, bail};
use payment_gateway::api::{self, AppState};
use payment_gateway::audit::Actor;
use payment_gateway::config::{Config, encryption_keys_from_env};
use payment_gateway::crypto::Sealer;
use payment_gateway::db;
use payment_gateway::ops::{self, privacy::Subject, retention::RetentionPolicy};
use payment_gateway::providers::{DefaultFactory, Registry};
use payment_gateway::rates::HttpRates;
use payment_gateway::service::Service;
use payment_gateway::worker;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
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

async fn serve() -> Result<()> {
    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
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
    let service = Service::with_mailer(
        pool,
        sealer,
        registry,
        Arc::new(HttpRates::mempool()),
        config.public_url.clone(),
        mailer.unwrap_or_else(|| Arc::new(payment_gateway::mailer::DisabledMailer)),
    );
    let sandbox = match payment_gateway::sandbox::SandboxConfig::from_env()? {
        Some(cfg) => Some(payment_gateway::sandbox::Sandbox::from_config(
            cfg,
            service.clone(),
        )?),
        None => None,
    };
    let _workers = worker::spawn(service.clone(), Duration::from_secs(5));
    let mut state = AppState::new(service, config.admin_token, config.allow_mock, sandbox);
    state.operator_2fa_grace = std::env::var("GATEWAY_OPERATOR_2FA_GRACE").is_ok_and(|v| v == "1");
    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    log::info!("listening on {}", config.listen);
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
