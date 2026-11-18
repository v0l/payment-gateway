use super::*;
use crate::api_keys::{key_mode, key_prefix, new_scoped_key, parse_scopes};

#[derive(Serialize)]
pub(super) struct KeyView {
    id: u64,
    name: String,
    prefix: String,
    scopes: Vec<String>,
    mode: Option<String>,
    expires: Option<DateTime<Utc>>,
    last_used: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created: DateTime<Utc>,
}

impl From<db::ApiKeyRow> for KeyView {
    fn from(k: db::ApiKeyRow) -> Self {
        Self {
            id: k.id,
            name: k.name,
            prefix: k.key_prefix,
            scopes: crate::api_keys::Scopes::parse(&k.scopes).list().to_vec(),
            mode: k.mode,
            expires: k.expires,
            last_used: k.last_used,
            revoked_at: k.revoked_at,
            created: k.created,
        }
    }
}

pub(super) async fn list_keys(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<KeyView>> {
    sess.project(&s, id, Role::Admin).await?;
    Ok(Json(
        db::list_api_keys(s.pool(), id)
            .await?
            .into_iter()
            .map(KeyView::from)
            .collect(),
    ))
}

#[derive(Deserialize)]
pub(super) struct NewKey {
    name: String,
    scopes: Vec<String>,
    expires: Option<DateTime<Utc>>,
}

pub(super) async fn create_key(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<NewKey>,
) -> ApiResult<Value> {
    let (tenant, _) = sess.project(&s, id, Role::Admin).await?;
    let name = req.name.trim().to_string();
    if name.is_empty() || name.len() > 64 {
        return Err(ApiError::bad_request("name must be 1-64 characters").with_field("name"));
    }
    let scopes =
        parse_scopes(&req.scopes).map_err(|m| ApiError::bad_request(m).with_field("scopes"))?;
    if req.expires.is_some_and(|e| e <= Utc::now()) {
        return Err(ApiError::bad_request("expires must be in the future").with_field("expires"));
    }
    let mode = key_mode(tenant.trusted);
    let key = new_scoped_key(mode);
    let scope_list = scopes.join(",");
    let key_id = db::insert_api_key(
        s.pool(),
        &db::NewApiKey {
            tenant_id: id,
            name: &name,
            key_hash: &hash_api_key(&key),
            key_prefix: &key_prefix(&key),
            scopes: &scope_list,
            mode,
            expires: req.expires,
        },
    )
    .await
    .map_err(|e| match e.downcast_ref::<sqlx::Error>() {
        Some(sqlx::Error::Database(d)) if d.is_unique_violation() => {
            ApiError::new(StatusCode::CONFLICT, "a key with that name exists".into())
                .with_field("name")
        }
        _ => e.into(),
    })?;
    sess.event(Action::ApiKeyCreated)
        .tenant(id)
        .target("api_key", key_id)
        .details(json!({"name": name, "scopes": scopes}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(
        json!({"id": key_id, "name": name, "scopes": scopes, "mode": mode, "api_key": key}),
    ))
}

pub(super) async fn revoke_key(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, key_id)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    if !db::revoke_api_key(s.pool(), id, key_id, Utc::now()).await? {
        return Err(ApiError::not_found());
    }
    sess.event(Action::ApiKeyRevoked)
        .tenant(id)
        .target("api_key", key_id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"revoked": true})))
}
