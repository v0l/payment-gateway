use super::ApiError;
use axum::Json;
use axum::http::{HeaderName, HeaderValue};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

pub const NEXT_CURSOR: &str = "x-next-cursor";
pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 100;

pub fn limit(raw: Option<&String>) -> Result<u32, ApiError> {
    match raw {
        None => Ok(DEFAULT_LIMIT),
        Some(v) => match v.parse::<u32>() {
            Ok(n) if (1..=MAX_LIMIT).contains(&n) => Ok(n),
            _ => Err(
                ApiError::bad_request(format!("limit must be between 1 and {MAX_LIMIT}"))
                    .with_field("limit"),
            ),
        },
    }
}

pub fn encode(position: &str) -> String {
    hex::encode(position.as_bytes())
}

pub fn decode(cursor: &str) -> Result<String, ApiError> {
    hex::decode(cursor)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or_else(|| ApiError::bad_request("invalid cursor").with_field("cursor"))
}

pub fn decode_id(cursor: Option<&String>) -> Result<Option<u64>, ApiError> {
    match cursor {
        None => Ok(None),
        Some(c) => decode(c)?
            .parse()
            .map(Some)
            .map_err(|_| ApiError::bad_request("invalid cursor").with_field("cursor")),
    }
}

pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<String>,
}

impl<T> Page<T> {
    pub fn cut(mut rows: Vec<T>, limit: u32, position: impl Fn(&T) -> String) -> Self {
        let more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next = if more {
            rows.last().map(|r| encode(&position(r)))
        } else {
            None
        };
        Self { items: rows, next }
    }
}

impl<T: Serialize> IntoResponse for Page<T> {
    fn into_response(self) -> Response {
        let next = self.next;
        let mut res = Json(self.items).into_response();
        if let Some(c) = next
            && let Ok(v) = HeaderValue::from_str(&c)
        {
            res.headers_mut()
                .insert(HeaderName::from_static(NEXT_CURSOR), v);
        }
        res
    }
}
