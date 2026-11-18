use super::*;
use crate::db::SearchFilter;

const STATUSES: &[&str] = &[
    "pending",
    "paid",
    "expired",
    "cancelled",
    "refunded",
    "partially_refunded",
];

fn parse_when(v: &str, end_of_day: bool) -> Option<DateTime<Utc>> {
    if let Ok(t) = DateTime::parse_from_rfc3339(v) {
        return Some(t.with_timezone(&Utc));
    }
    let day = chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").ok()?;
    let start = day.and_hms_opt(0, 0, 0)?.and_utc();
    Some(if end_of_day {
        start + Duration::days(1)
    } else {
        start
    })
}

fn nonempty<'a>(q: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    q.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

pub(super) fn parse_filter(q: &HashMap<String, String>) -> Result<SearchFilter, ApiError> {
    let mut f = SearchFilter::default();
    if let Some(v) = nonempty(q, "status") {
        if !STATUSES.contains(&v) {
            return Err(ApiError::bad_request("unknown status").with_field("status"));
        }
        f.status = Some(v.to_string());
    }
    if let Some(v) = nonempty(q, "method") {
        if !METHODS.contains(&v) {
            return Err(ApiError::bad_request("unknown method").with_field("method"));
        }
        f.method = Some(v.to_string());
    }
    f.query = nonempty(q, "q").map(str::to_string);
    f.customer = nonempty(q, "customer").map(str::to_string);
    f.currency = nonempty(q, "currency").map(|c| c.to_uppercase());
    if let Some(v) = nonempty(q, "from") {
        f.created_from = Some(
            parse_when(v, false)
                .ok_or_else(|| ApiError::bad_request("from must be a date").with_field("from"))?,
        );
    }
    if let Some(v) = nonempty(q, "to") {
        f.created_to = Some(
            parse_when(v, true)
                .ok_or_else(|| ApiError::bad_request("to must be a date").with_field("to"))?,
        );
    }
    for (key, slot) in [
        ("min_amount", &mut f.min_amount),
        ("max_amount", &mut f.max_amount),
    ] {
        if let Some(v) = nonempty(q, key) {
            *slot = Some(v.parse::<u64>().map_err(|_| {
                ApiError::bad_request(format!("{key} must be a whole number of minor units"))
                    .with_field(key)
            })?);
        }
    }
    if let (Some(lo), Some(hi)) = (f.min_amount, f.max_amount)
        && lo > hi
    {
        return Err(
            ApiError::bad_request("min_amount is above max_amount").with_field("min_amount")
        );
    }
    if let (Some(a), Some(b)) = (f.created_from, f.created_to)
        && a >= b
    {
        return Err(ApiError::bad_request("from is after to").with_field("from"));
    }
    f.before = nonempty(q, "before")
        .map(parse_cursor)
        .transpose()
        .map_err(|_| ApiError::bad_request("before must be a cursor from a previous page"))?;
    Ok(f)
}
