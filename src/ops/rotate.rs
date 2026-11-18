use crate::audit::{Action, Actor, Event, Origin};
use crate::crypto::Sealer;
use anyhow::{Context, Result};
use serde::Serialize;
use sqlx::MySqlPool;

pub struct SealedColumn {
    pub table: &'static str,
    pub key: &'static str,
    pub column: &'static str,
}

pub const SEALED_COLUMNS: &[SealedColumn] = &[
    SealedColumn {
        table: "tenant",
        key: "id",
        column: "webhook_secret",
    },
    SealedColumn {
        table: "provider_config",
        key: "id",
        column: "config",
    },
    SealedColumn {
        table: "shared_provider",
        key: "id",
        column: "config",
    },
    SealedColumn {
        table: "account",
        key: "id",
        column: "totp_secret",
    },
];

pub const NOT_SEALED_BINARY_COLUMNS: &[(&str, &str)] = &[
    ("payment", "id"),
    ("payment_attempt", "payment_id"),
    ("refund", "payment_id"),
    ("webhook_delivery", "payment_id"),
];

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct ColumnReport {
    pub table: String,
    pub column: String,
    pub rotated: u64,
    pub already_current: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct RotateReport {
    pub columns: Vec<ColumnReport>,
    pub stopped_early: bool,
}

impl RotateReport {
    pub fn rotated(&self) -> u64 {
        self.columns.iter().map(|c| c.rotated).sum()
    }
}

pub struct RotateOptions {
    pub batch: u32,
    pub max_rows: Option<u64>,
}

impl Default for RotateOptions {
    fn default() -> Self {
        Self {
            batch: 200,
            max_rows: None,
        }
    }
}

pub async fn rotate_keys(
    pool: &MySqlPool,
    sealer: &Sealer,
    opts: &RotateOptions,
    actor: Actor,
) -> Result<RotateReport> {
    let report = rotate_columns(pool, sealer, opts).await?;
    Event::new(Action::KeysRotated, actor)
        .details(serde_json::json!({
            "kid": sealer.current_kid(),
            "rotated": report.rotated(),
            "stopped_early": report.stopped_early,
        }))
        .record(pool, &Origin::default())
        .await;
    Ok(report)
}

async fn rotate_columns(
    pool: &MySqlPool,
    sealer: &Sealer,
    opts: &RotateOptions,
) -> Result<RotateReport> {
    let mut report = RotateReport::default();
    let mut budget = opts.max_rows;
    for col in SEALED_COLUMNS {
        let mut cr = ColumnReport {
            table: col.table.into(),
            column: col.column.into(),
            ..Default::default()
        };
        let mut last = 0u64;
        'column: loop {
            let rows: Vec<(u64, Vec<u8>)> = sqlx::query_as(&format!(
                "select {k}, {c} from {t} where {k} > ? and {c} is not null order by {k} limit ?",
                k = col.key,
                c = col.column,
                t = col.table
            ))
            .bind(last)
            .bind(opts.batch)
            .fetch_all(pool)
            .await?;
            if rows.is_empty() {
                break;
            }
            for (id, sealed) in rows {
                last = id;
                let resealed = sealer
                    .reseal(&sealed)
                    .with_context(|| format!("{}.{} row {id}", col.table, col.column))?;
                let Some(resealed) = resealed else {
                    cr.already_current += 1;
                    continue;
                };
                if budget == Some(0) {
                    report.stopped_early = true;
                    break 'column;
                }
                let r = sqlx::query(&format!(
                    "update {t} set {c} = ? where {k} = ? and {c} = ?",
                    t = col.table,
                    c = col.column,
                    k = col.key
                ))
                .bind(&resealed)
                .bind(id)
                .bind(&sealed)
                .execute(pool)
                .await?;
                cr.rotated += r.rows_affected();
                if let Some(b) = budget.as_mut() {
                    *b -= 1;
                }
            }
        }
        report.columns.push(cr);
        if report.stopped_early {
            break;
        }
    }
    Ok(report)
}

pub async fn sealed_kids(pool: &MySqlPool) -> Result<Vec<(String, String, u64)>> {
    let mut out = Vec::new();
    for col in SEALED_COLUMNS {
        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(&format!(
            "select {c} from {t} where {c} is not null",
            c = col.column,
            t = col.table
        ))
        .fetch_all(pool)
        .await?;
        let mut counts: std::collections::BTreeMap<String, u64> = Default::default();
        for (v,) in rows {
            let kid = Sealer::kid_of(&v).unwrap_or("legacy").to_string();
            *counts.entry(kid).or_default() += 1;
        }
        for (kid, n) in counts {
            out.push((format!("{}.{}", col.table, col.column), kid, n));
        }
    }
    Ok(out)
}
