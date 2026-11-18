use super::*;
use crate::db::SearchFilter;
use axum::body::{Body, Bytes};

const EXPORT_PAGE: u32 = 500;
const HEADER: [&str; 12] = [
    "id",
    "reference",
    "status",
    "currency",
    "amount",
    "tax",
    "created",
    "paid_at",
    "customer_ref",
    "customer_email",
    "description",
    "methods",
];

pub fn csv_cell(value: &str) -> String {
    let dangerous = value
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r'));
    let text = if dangerous {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

pub fn csv_row(cells: &[String]) -> String {
    let mut line = cells
        .iter()
        .map(|c| csv_cell(c))
        .collect::<Vec<_>>()
        .join(",");
    line.push('\n');
    line
}

fn payment_cells(p: &db::PaymentRow) -> Vec<String> {
    vec![
        hex::encode(&p.id),
        p.reference.clone(),
        p.status.clone(),
        p.currency.clone(),
        p.amount.to_string(),
        p.tax.to_string(),
        p.created.to_rfc3339(),
        p.paid_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
        p.customer_ref.clone().unwrap_or_default(),
        p.customer_email.clone().unwrap_or_default(),
        p.description.clone().unwrap_or_default(),
        p.methods.clone().unwrap_or_default(),
    ]
}

struct ExportState {
    pool: sqlx::MySqlPool,
    tenant: u64,
    filter: SearchFilter,
    first: bool,
    done: bool,
}

async fn next_chunk(mut st: ExportState) -> Option<(Result<Bytes, std::io::Error>, ExportState)> {
    if st.done {
        return None;
    }
    let mut out = String::new();
    if st.first {
        out.push_str(&csv_row(&HEADER.map(String::from)));
        st.first = false;
    }
    let rows = match db::search_payments(&st.pool, st.tenant, &st.filter, EXPORT_PAGE).await {
        Ok(r) => r,
        Err(e) => {
            st.done = true;
            return Some((Err(std::io::Error::other(e.to_string())), st));
        }
    };
    for p in &rows {
        out.push_str(&csv_row(&payment_cells(p)));
    }
    match rows.last() {
        Some(last) if rows.len() as u32 == EXPORT_PAGE => {
            st.filter.before = Some((last.created, last.id.clone()));
        }
        _ => st.done = true,
    }
    Some((Ok(Bytes::from(out)), st))
}

pub(super) async fn export_payments(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    sess.project(&s, id, Role::Viewer).await?;
    let mut filter = parse_filter(&q)?;
    filter.before = None;
    sess.event(Action::PaymentsExported)
        .tenant(id)
        .details(json!({"filter": q}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    let state = ExportState {
        pool: s.pool().clone(),
        tenant: id,
        filter,
        first: true,
        done: false,
    };
    let body = Body::from_stream(futures::stream::unfold(state, next_chunk));
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"payments-{id}.csv\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        body,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_that_start_like_formulas_are_neutralised() {
        for bad in ["=1+1", "+1", "-1", "@SUM(A1)", "\t=1", "\r=1"] {
            let cell = csv_cell(bad);
            assert!(
                cell.starts_with('\'') || cell.starts_with("\"'"),
                "{bad:?} -> {cell:?}"
            );
        }
        assert_eq!(csv_cell("plain"), "plain");
        assert_eq!(csv_cell("a=b"), "a=b");
    }

    #[test]
    fn cells_with_separators_are_quoted() {
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("two\nlines"), "\"two\nlines\"");
        assert_eq!(csv_cell("=HYPERLINK(\"x\",\"y\")"), "\"'=HYPERLINK(\"\"x\"\",\"\"y\"\")\"");
    }
}
