use super::admin::AdminAuth;
use super::{ApiError, ApiResult, AppState};
use crate::db::parse_payment_id;
use crate::model::CreatePayment;
use crate::sandbox::{ReceivedHook, Sandbox, Status};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

const SANDBOX_HTML: &str = include_str!("../../web/sandbox.html");
const SANDBOX_JS: &str = include_str!("../../web/sandbox.js");
const SANDBOX_CSS: &str = include_str!("../../web/sandbox.css");

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sandbox", get(page))
        .route("/sandbox/app.js", get(script))
        .route("/sandbox/app.css", get(stylesheet))
        .route("/sandbox/hook", post(hook))
        .route("/sandbox/api/status", get(status))
        .route("/sandbox/api/bootstrap", post(bootstrap))
        .route("/sandbox/api/rotate-key", post(rotate_key))
        .route("/sandbox/api/mine", post(mine))
        .route("/sandbox/api/fund", post(fund))
        .route("/sandbox/api/faucet", post(faucet))
        .route("/sandbox/api/pay-invoice", post(pay_invoice))
        .route("/sandbox/api/payments", post(create_payment).get(payments))
        .route("/sandbox/api/payments/{id}/attempt", post(start_attempt))
        .route("/sandbox/api/payments/{id}/pay", post(pay))
        .route("/sandbox/api/hooks", get(hooks))
}

fn sandbox(s: &AppState) -> Result<Arc<Sandbox>, ApiError> {
    s.sandbox.clone().ok_or_else(ApiError::not_found)
}

fn page_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
            .parse()
            .unwrap(),
    );
    h.insert(
        header::CONTENT_TYPE,
        "text/html; charset=utf-8".parse().unwrap(),
    );
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    h.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    h.insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
    h
}

async fn page(State(s): State<AppState>) -> Result<Response, ApiError> {
    sandbox(&s)?;
    Ok((StatusCode::OK, page_headers(), SANDBOX_HTML).into_response())
}

async fn script(State(s): State<AppState>) -> Result<Response, ApiError> {
    sandbox(&s)?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/javascript"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        SANDBOX_JS,
    )
        .into_response())
}

async fn stylesheet(State(s): State<AppState>) -> Result<Response, ApiError> {
    sandbox(&s)?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/css"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        SANDBOX_CSS,
    )
        .into_response())
}

async fn hook(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<Value>, ApiError> {
    let sb = sandbox(&s)?;
    let sig = headers
        .get("x-gateway-signature")
        .and_then(|v| v.to_str().ok());
    sb.record_hook(sig, &body).await;
    Ok(Json(json!({"ok": true})))
}

async fn status(_: AdminAuth, State(s): State<AppState>) -> ApiResult<Status> {
    Ok(Json(sandbox(&s)?.status().await))
}

async fn bootstrap(_: AdminAuth, State(s): State<AppState>) -> ApiResult<Value> {
    let sb = sandbox(&s)?;
    match sb.bootstrap().await {
        Ok(log) => Ok(Json(json!({"ok": true, "log": log}))),
        Err(e) => Ok(Json(json!({"ok": false, "error": format!("{e:#}")}))),
    }
}

async fn rotate_key(_: AdminAuth, State(s): State<AppState>) -> ApiResult<Value> {
    let key = sandbox(&s)?
        .rotate_key()
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(json!({"api_key": key})))
}

#[derive(Deserialize)]
struct MineInput {
    blocks: u32,
}

async fn mine(
    _: AdminAuth,
    State(s): State<AppState>,
    Json(i): Json<MineInput>,
) -> ApiResult<Value> {
    let height = sandbox(&s)?
        .mine(i.blocks)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(json!({"height": height})))
}

#[derive(Deserialize)]
struct FundInput {
    target: String,
    sats: u64,
}

async fn fund(
    _: AdminAuth,
    State(s): State<AppState>,
    Json(i): Json<FundInput>,
) -> ApiResult<Value> {
    let txid = sandbox(&s)?
        .fund(&i.target, i.sats)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(json!({"txid": txid})))
}

#[derive(Deserialize)]
struct FaucetInput {
    address: String,
    sats: u64,
}

async fn faucet(
    _: AdminAuth,
    State(s): State<AppState>,
    Json(i): Json<FaucetInput>,
) -> ApiResult<Value> {
    let txid = sandbox(&s)?
        .faucet(&i.address, i.sats)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(json!({"txid": txid})))
}

#[derive(Deserialize)]
struct InvoiceInput {
    bolt11: String,
}

async fn pay_invoice(
    _: AdminAuth,
    State(s): State<AppState>,
    Json(i): Json<InvoiceInput>,
) -> ApiResult<Value> {
    let preimage = sandbox(&s)?
        .customer_pay_invoice(&i.bolt11)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(Json(json!({"preimage": preimage})))
}

async fn create_payment(
    _: AdminAuth,
    State(s): State<AppState>,
    Json(i): Json<CreatePayment>,
) -> ApiResult<Value> {
    Ok(Json(sandbox(&s)?.create_payment(i).await?))
}

async fn payments(_: AdminAuth, State(s): State<AppState>) -> ApiResult<Vec<Value>> {
    Ok(Json(sandbox(&s)?.recent_payments().await?))
}

#[derive(Deserialize)]
struct AttemptInput {
    method: String,
}

async fn start_attempt(
    _: AdminAuth,
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(i): Json<AttemptInput>,
) -> ApiResult<Value> {
    let pid = parse_payment_id(&id).ok_or_else(ApiError::not_found)?;
    sandbox(&s)?.start_attempt(&pid, &i.method).await?;
    Ok(Json(json!({"ok": true})))
}

async fn pay(_: AdminAuth, State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Value> {
    let pid = parse_payment_id(&id).ok_or_else(ApiError::not_found)?;
    let message = sandbox(&s)?.pay_from_customer(&pid).await?;
    Ok(Json(json!({"message": message})))
}

async fn hooks(_: AdminAuth, State(s): State<AppState>) -> ApiResult<Vec<ReceivedHook>> {
    Ok(Json(sandbox(&s)?.hooks()))
}
