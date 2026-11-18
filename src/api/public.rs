use super::{ApiError, AppState};
use crate::db::{self, Tenant, parse_payment_id};
use crate::model::{AttemptView, CreateAttempt};
use crate::service::MethodChoice;
use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;

const PAY_HTML: &str = include_str!("../../web/pay.html");
const PAY_JS: &str = include_str!("../../web/pay.js");
const PAY_CSS: &str = include_str!("../../web/pay.css");
const TOKENS_CSS: &str = include_str!("../../web/tokens.css");
const GATEWAY_JS: &str = include_str!("../../web/gateway.js");

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pay/{id}", get(pay_page))
        .route("/embed/{id}", get(embed_page))
        .route("/pay/{id}/state", get(state))
        .route("/pay/{id}/return", get(provider_return))
        .route("/pay/{id}/attempts", post(create_attempt))
        .route("/pay/{id}/events", get(events))
        .route("/pay/{id}/qr/{attempt}", get(qr))
        .route(
            "/assets/pay.js",
            get(|| async { asset(PAY_JS, "text/javascript") }),
        )
        .route(
            "/assets/pay.css",
            get(|| async { asset(PAY_CSS, "text/css") }),
        )
        .route(
            "/assets/tokens.css",
            get(|| async { asset(TOKENS_CSS, "text/css") }),
        )
        .route("/gateway.js", get(|| async { gateway_js() }))
        .route("/hook/{tenant}/{provider}", post(hook))
}

fn asset(body: &'static str, content_type: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        body,
    )
        .into_response()
}

fn gateway_js() -> Response {
    let mut r = asset(GATEWAY_JS, "text/javascript");
    r.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    r
}

pub(super) fn page_headers(frame_ancestors: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    let csp = format!(
        "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' https: data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors {frame_ancestors}"
    );
    h.insert(header::CONTENT_SECURITY_POLICY, csp.parse().unwrap());
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if frame_ancestors == "'none'" {
        h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    }
    h
}

pub(super) async fn load(s: &AppState, raw: &str) -> Result<(db::PaymentRow, Tenant), ApiError> {
    let id = parse_payment_id(raw).ok_or_else(ApiError::not_found)?;
    let p = db::get_payment(s.pool(), &id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let tenant = s.service.tenant(p.tenant_id).await?;
    if !tenant.enabled {
        return Err(ApiError::not_found());
    }
    Ok((p, tenant))
}

async fn pay_page(State(s): State<AppState>, Path(id): Path<String>) -> Result<Response, ApiError> {
    load(&s, &id).await?;
    Ok((StatusCode::OK, page_headers("'none'"), PAY_HTML).into_response())
}

async fn embed_page(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let (_, tenant) = load(&s, &id).await?;
    let origins = tenant.origins();
    let ancestors = if origins.is_empty() {
        "'none'".to_string()
    } else {
        origins.join(" ")
    };
    Ok((StatusCode::OK, page_headers(&ancestors), PAY_HTML).into_response())
}

#[derive(Serialize)]
struct PublicTenant {
    name: String,
    logo_url: Option<String>,
    accent_color: Option<String>,
}

#[derive(Serialize)]
struct PublicState {
    id: String,
    status: String,
    amount: u64,
    currency: String,
    description: Option<String>,
    expires: chrono::DateTime<chrono::Utc>,
    tenant: PublicTenant,
    allowed_origins: Vec<String>,
    line_items: Vec<crate::invoice::LineView>,
    subtotal: Option<u64>,
    discount: Option<crate::invoice::DiscountView>,
    tax: u64,
    methods: Vec<MethodChoice>,
    attempts: Vec<AttemptView>,
    return_url: Option<String>,
}

fn public_attempt(a: &db::AttemptRow) -> AttemptView {
    let mut v: AttemptView = a.into();
    if let Value::Object(map) = &mut v.details {
        map.remove("order_token");
    }
    v
}

async fn build_state(
    s: &AppState,
    p: &db::PaymentRow,
    tenant: &Tenant,
) -> Result<PublicState, ApiError> {
    let attempts = db::attempts_for_payment(s.pool(), &p.pid()).await?;
    let methods = if p.status == "pending" && p.expires > chrono::Utc::now() {
        s.service.available_methods(tenant, p).await?
    } else {
        vec![]
    };
    let breakdown = p.breakdown();
    Ok(PublicState {
        line_items: breakdown
            .as_ref()
            .map(|b| b.lines.iter().map(crate::invoice::Line::view).collect())
            .unwrap_or_default(),
        subtotal: breakdown.as_ref().map(|b| b.subtotal),
        discount: breakdown
            .as_ref()
            .and_then(|b| b.discount.as_ref().map(|d| d.view())),
        tax: p.tax,
        id: hex::encode(&p.id),
        status: p.status.clone(),
        amount: p.amount,
        currency: p.currency.clone(),
        description: p.description.clone(),
        expires: p.expires,
        tenant: PublicTenant {
            name: tenant
                .display_name
                .clone()
                .unwrap_or_else(|| tenant.name.clone()),
            logo_url: tenant.logo_url.clone(),
            accent_color: tenant.accent_color.clone(),
        },
        allowed_origins: tenant.origins(),
        methods,
        attempts: attempts.iter().map(public_attempt).collect(),
        return_url: s.service.signed_return_url(tenant, p),
    })
}

#[derive(Deserialize)]
struct ReturnQuery {
    token: Option<String>,
}

async fn provider_return(
    State(s): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<ReturnQuery>,
) -> Result<Response, ApiError> {
    let (p, tenant) = load(&s, &id).await?;
    if let Some(token) = q.token.filter(|t| t.len() <= 64) {
        s.service.capture_return(&p.pid(), &token).await?;
    }
    let p = db::get_payment(s.pool(), &p.pid())
        .await?
        .ok_or_else(ApiError::not_found)?;
    let target = s
        .service
        .signed_return_url(&tenant, &p)
        .unwrap_or_else(|| s.service.pay_url(&p.pid()));
    Ok((StatusCode::SEE_OTHER, [(header::LOCATION, target)]).into_response())
}

async fn state(State(s): State<AppState>, Path(id): Path<String>) -> Result<Response, ApiError> {
    let (p, tenant) = load(&s, &id).await?;
    let body = build_state(&s, &p, &tenant).await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

#[derive(Deserialize)]
struct PublicAttempt {
    provider_id: u64,
    option: Option<String>,
}

async fn create_attempt(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<PublicAttempt>,
) -> Result<Response, ApiError> {
    let (p, tenant) = load(&s, &id).await?;
    let cfg = db::get_provider_config(s.pool(), input.provider_id)
        .await?
        .filter(|c| c.tenant_id == tenant.id && c.enabled)
        .ok_or_else(ApiError::not_found)?;
    s.service
        .create_attempt(
            &tenant,
            &p.pid(),
            CreateAttempt {
                method: cfg.method,
                provider_id: Some(cfg.id),
                option: input.option,
                ..Default::default()
            },
        )
        .await?;
    let p = db::get_payment(s.pool(), &p.pid())
        .await?
        .ok_or_else(ApiError::not_found)?;
    let body = build_state(&s, &p, &tenant).await?;
    Ok(Json(body).into_response())
}

async fn events(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let (p, _) = load(&s, &id).await?;
    let wanted = p.pid();
    let rx = s.service.changes.subscribe();
    let initial =
        futures::stream::once(async { Ok(Event::default().event("change").data("init")) });
    let live = BroadcastStream::new(rx).filter_map(move |msg| async move {
        match msg {
            Ok(changed) if changed == wanted => {
                Some(Ok(Event::default().event("change").data("update")))
            }
            Ok(_) => None,
            Err(_) => Some(Ok(Event::default().event("change").data("resync"))),
        }
    });
    let mut shutdown = s.ops.shutdown.subscribe();
    let stop = async move {
        let _ = shutdown.wait_for(|down| *down).await;
    };
    Ok(Sse::new(initial.chain(live).take_until(stop)).keep_alive(KeepAlive::default()))
}

async fn qr(
    State(s): State<AppState>,
    Path((id, attempt)): Path<(String, u64)>,
) -> Result<Response, ApiError> {
    let (p, _) = load(&s, &id).await?;
    let a = db::get_attempt(s.pool(), attempt)
        .await?
        .filter(|a| a.payment_id == p.id)
        .ok_or_else(ApiError::not_found)?;
    let details: Value = serde_json::from_str(&a.details).unwrap_or(Value::Null);
    let data = details["bolt11"]
        .as_str()
        .map(|b| format!("lightning:{}", b.to_uppercase()))
        .or_else(|| details["uri"].as_str().map(str::to_string))
        .or_else(|| details["deposit_address"].as_str().map(str::to_string))
        .ok_or_else(ApiError::not_found)?;
    let code =
        qrcode::QrCode::new(data.as_bytes()).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(260, 260)
        .quiet_zone(true)
        .build();
    Ok((
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        svg,
    )
        .into_response())
}

pub const PROVIDER_HOOK_MAX_BYTES: usize = 256 * 1024;

async fn hook(
    State(s): State<AppState>,
    Path((tenant, provider)): Path<(u64, u64)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    if body.len() > PROVIDER_HOOK_MAX_BYTES {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "webhook body too large".into()));
    }
    let cfg = db::get_provider_config(s.pool(), provider)
        .await?
        .filter(|c| c.tenant_id == tenant && c.enabled)
        .ok_or_else(ApiError::not_found)?;
    crate::logging::set_tenant(tenant);
    let handler = s.service.registry.get(cfg.id).await?;
    let map: HashMap<String, String> = headers
        .iter()
        .filter_map(|(k, v)| Some((k.as_str().to_lowercase(), v.to_str().ok()?.to_string())))
        .collect();
    let events = handler.handle_webhook(&map, &body).await.map_err(|e| {
        log::warn!(
            "webhook for provider {} rejected: {}",
            cfg.id,
            crate::logsafe::clean(&format!("{e:#}"))
        );
        ApiError::bad_request("invalid webhook")
    })?;
    let event_id = handler.event_id(&body);
    if let Some(id) = &event_id
        && !db::claim_provider_event(s.pool(), cfg.id, id).await?
    {
        return Ok(Json(json!({ "ok": true, "duplicate": true })));
    }
    for ev in events {
        if let Err(e) = s.service.apply_event(cfg.id, ev).await {
            if let Some(id) = &event_id {
                let _ = db::release_provider_event(s.pool(), cfg.id, id).await;
            }
            return Err(e.into());
        }
    }
    Ok(Json(json!({ "ok": true })))
}
