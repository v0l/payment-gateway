use super::*;
use crate::ledger;

const MAX_TZ_OFFSET_MIN: i64 = 14 * 60;

pub(super) fn tz_offset(q: &HashMap<String, String>) -> Result<i64, ApiError> {
    match q.get("tz").map(|v| v.trim()).filter(|v| !v.is_empty()) {
        None => Ok(0),
        Some(v) => v
            .parse::<i64>()
            .ok()
            .filter(|m| m.abs() <= MAX_TZ_OFFSET_MIN)
            .ok_or_else(|| {
                ApiError::bad_request("tz must be minutes east of UTC, -840 to 840")
                    .with_field("tz")
            }),
    }
}

pub(super) fn local_midnight(now: DateTime<Utc>, tz_minutes: i64) -> DateTime<Utc> {
    let local = now + Duration::minutes(tz_minutes);
    let day = local.date_naive().and_hms_opt(0, 0, 0).expect("midnight");
    day.and_utc() - Duration::minutes(tz_minutes)
}

fn month_start(now: DateTime<Utc>, tz_minutes: i64) -> DateTime<Utc> {
    use chrono::Datelike;
    let local = now + Duration::minutes(tz_minutes);
    let first = local
        .date_naive()
        .with_day(1)
        .expect("first of month")
        .and_hms_opt(0, 0, 0)
        .expect("midnight");
    first.and_utc() - Duration::minutes(tz_minutes)
}

fn previous_month_start(start: DateTime<Utc>, tz_minutes: i64) -> DateTime<Utc> {
    month_start(start - Duration::days(1), tz_minutes)
}

#[derive(Serialize)]
struct Series {
    currency: String,
    today: Vec<i64>,
    last_week: Vec<i64>,
    today_total: i64,
    today_payments: i64,
    last_week_same_time: i64,
    last_week_total: i64,
}

fn cumulative(rows: &[db::HourlyVolume], currency: &str, hours: usize) -> Vec<i64> {
    let mut per = [0i64; 24];
    for r in rows.iter().filter(|r| r.currency == currency) {
        if (0..24).contains(&r.slot) {
            per[r.slot as usize] += r.volume;
        }
    }
    let mut out = Vec::with_capacity(hours);
    let mut sum = 0;
    for v in per.iter().take(hours) {
        sum += v;
        out.push(sum);
    }
    out
}

#[derive(Serialize)]
struct Figure {
    currency: String,
    paid: i64,
    paid_prev: i64,
    payments: i64,
    payments_prev: i64,
    refunded: i64,
    refunded_prev: i64,
    success_rate: Option<f64>,
    success_rate_prev: Option<f64>,
}

#[derive(Serialize)]
struct Wallet {
    key: String,
    name: String,
    method: String,
    currency: String,
    available: Option<i64>,
    settling: i64,
    settles: &'static str,
    platform_managed: bool,
}

#[derive(Serialize)]
struct Health {
    id: u64,
    name: String,
    method: String,
    state: &'static str,
    note: String,
    platform_managed: bool,
}

fn settles(method: &str) -> &'static str {
    match method {
        "lightning" => "instantly",
        "onchain" => "after confirmations",
        "fixedfloat" => "after the swap confirms",
        _ => "on the provider's payout schedule",
    }
}

fn rate(paid: i64, failed: i64) -> Option<f64> {
    (paid + failed > 0).then(|| paid as f64 / (paid + failed) as f64)
}

fn health_of(p: &db::ProviderStatusRow, now: DateTime<Utc>) -> (&'static str, String) {
    if p.auto_disabled_at.is_some() {
        return ("bad", "switched off after repeated errors".into());
    }
    if !p.enabled {
        return ("off", "disabled".into());
    }
    let fails = p.consecutive_failures.unwrap_or(0);
    if fails > 0 {
        return (
            "warn",
            format!(
                "{fails} failed call{} in a row",
                if fails == 1 { "" } else { "s" }
            ),
        );
    }
    match p.last_success_at {
        Some(t) => ("ok", format!("working, last call {}", since(now, t))),
        None => ("ok", "connected, no calls yet".into()),
    }
}

fn since(now: DateTime<Utc>, t: DateTime<Utc>) -> String {
    let m = (now - t).num_minutes().max(0);
    if m < 1 {
        "just now".into()
    } else if m < 60 {
        format!("{m} min ago")
    } else if m < 48 * 60 {
        format!("{} h ago", m / 60)
    } else {
        format!("{} days ago", m / 1440)
    }
}

pub(super) async fn home(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let tz = tz_offset(&q)?;
    let now = s.service.clock.now();
    let today = local_midnight(now, tz);
    let week_ago = today - Duration::days(7);
    let hours_now = ((now - today).num_minutes() / 60 + 1).clamp(1, 24) as usize;
    let today_rows = db::hourly_paid(s.pool(), id, today, today + Duration::days(1)).await?;
    let week_rows = db::hourly_paid(s.pool(), id, week_ago, week_ago + Duration::days(1)).await?;
    let mut currencies: Vec<String> = today_rows
        .iter()
        .chain(week_rows.iter())
        .map(|r| r.currency.clone())
        .collect();
    let m_start = month_start(now, tz);
    let prev_start = previous_month_start(m_start, tz);
    let elapsed = now - m_start;
    let prev_end = (prev_start + elapsed).min(m_start);
    let paid = db::paid_between(s.pool(), id, m_start, now + Duration::seconds(1)).await?;
    let paid_prev = db::paid_between(s.pool(), id, prev_start, prev_end).await?;
    let refunded = db::refunded_between(s.pool(), id, m_start, now + Duration::seconds(1)).await?;
    let refunded_prev = db::refunded_between(s.pool(), id, prev_start, prev_end).await?;
    let outcomes = db::outcomes_between(s.pool(), id, m_start, now + Duration::seconds(1)).await?;
    let outcomes_prev = db::outcomes_between(s.pool(), id, prev_start, prev_end).await?;
    currencies.extend(paid.iter().map(|r| r.currency.clone()));
    currencies.extend(paid_prev.iter().map(|r| r.currency.clone()));
    currencies.sort();
    currencies.dedup();
    let series: Vec<Series> = currencies
        .iter()
        .map(|c| {
            let t = cumulative(&today_rows, c, hours_now);
            let w = cumulative(&week_rows, c, 24);
            Series {
                currency: c.clone(),
                today_total: t.last().copied().unwrap_or(0),
                today_payments: today_rows
                    .iter()
                    .filter(|r| &r.currency == c)
                    .map(|r| r.payments)
                    .sum(),
                last_week_same_time: w.get(hours_now - 1).copied().unwrap_or(0),
                last_week_total: w.last().copied().unwrap_or(0),
                today: t,
                last_week: w,
            }
        })
        .collect();
    let pick = |rows: &[db::CurrencyTotal], c: &str| {
        rows.iter()
            .find(|r| r.currency == c)
            .map(|r| (r.volume, r.payments))
            .unwrap_or((0, 0))
    };
    let pick_rate = |rows: &[db::CreatedOutcome], c: &str| {
        rows.iter()
            .find(|r| r.currency == c)
            .and_then(|r| rate(r.paid, r.failed))
    };
    let figures: Vec<Figure> = currencies
        .iter()
        .map(|c| {
            let (p, n) = pick(&paid, c);
            let (pp, np) = pick(&paid_prev, c);
            Figure {
                currency: c.clone(),
                paid: p,
                paid_prev: pp,
                payments: n,
                payments_prev: np,
                refunded: pick(&refunded, c).0,
                refunded_prev: pick(&refunded_prev, c).0,
                success_rate: pick_rate(&outcomes, c),
                success_rate_prev: pick_rate(&outcomes_prev, c),
            }
        })
        .collect();

    let providers = db::tenant_provider_status(s.pool(), id).await?;
    let incoming = db::unconfirmed_incoming(s.pool(), id).await?;
    let held = ledger::balances(s.pool(), id).await?;
    let held_btc = held
        .iter()
        .find(|b| b.kind == "tenant_balance" && b.currency == "BTC")
        .map(|b| b.balance)
        .unwrap_or(0);
    let pending_payout = held
        .iter()
        .find(|b| b.kind == "pending_payout" && b.currency == "BTC")
        .map(|b| b.balance)
        .unwrap_or(0);
    let mut wallets = vec![];
    let platform_settling: i64 = incoming
        .iter()
        .filter(|i| i.platform_managed)
        .map(|i| i.amount)
        .sum();
    if held_btc != 0 || providers.iter().any(|p| p.platform_managed) {
        wallets.push(Wallet {
            key: "platform".into(),
            name: "Held by the platform".into(),
            method: "bitcoin".into(),
            currency: "BTC".into(),
            available: Some(held_btc),
            settling: platform_settling,
            settles: "instantly for Lightning, after confirmations on-chain",
            platform_managed: true,
        });
    }
    for p in providers
        .iter()
        .filter(|p| !p.platform_managed && p.enabled)
    {
        let settling: i64 = incoming
            .iter()
            .filter(|i| i.provider_config_id == p.id)
            .map(|i| i.amount)
            .sum();
        wallets.push(Wallet {
            key: format!("provider:{}", p.id),
            name: p.name.clone(),
            method: p.method.clone(),
            currency: if crate::model::is_fiat_method(&p.method) {
                "fiat".into()
            } else {
                "BTC".into()
            },
            available: None,
            settling,
            settles: settles(&p.method),
            platform_managed: false,
        });
    }
    let w = db::withdrawal_summary(s.pool(), id).await?;
    let latest = db::withdrawals_for_tenant(s.pool(), id, 1)
        .await?
        .into_iter()
        .next()
        .map(|w| json!({"id": w.id, "amount": w.amount, "currency": w.currency, "status": w.status, "destination": w.destination_label, "created": w.created, "finished_at": w.finished_at}));
    let health: Vec<Health> = providers
        .iter()
        .map(|p| {
            let (state, note) = health_of(p, now);
            Health {
                id: p.id,
                name: p.name.clone(),
                method: p.method.clone(),
                state,
                note,
                platform_managed: p.platform_managed,
            }
        })
        .collect();
    let failing_hooks = db::failing_endpoints(s.pool(), id, now - Duration::days(7))
        .await?
        .len();
    Ok(Json(json!({
        "now": now,
        "today_start": today,
        "hours_elapsed": hours_now,
        "series": series,
        "month": {"start": m_start, "previous_start": prev_start, "previous_end": prev_end, "figures": figures},
        "funds": {
            "available": held_btc,
            "settling": incoming.iter().map(|i| i.amount).sum::<i64>(),
            "in_payout": pending_payout,
            "payouts_in_flight": w.in_flight,
            "latest_payout": latest,
            "wallets": wallets,
        },
        "health": health,
        "failing_webhook_endpoints": failing_hooks,
    })))
}
