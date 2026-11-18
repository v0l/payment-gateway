use serde_json::{Map, Value, json};

struct Op {
    method: &'static str,
    path: &'static str,
    summary: &'static str,
    scope: &'static str,
    list: bool,
    body: Option<&'static str>,
    params: &'static [&'static str],
}

const OPS: &[Op] = &[
    Op {
        method: "get",
        path: "/methods",
        summary: "List enabled payment methods",
        scope: "payments:read",
        list: true,
        body: None,
        params: &[],
    },
    Op {
        method: "get",
        path: "/rates",
        summary: "Current BTC rates",
        scope: "payments:read",
        list: false,
        body: None,
        params: &[],
    },
    Op {
        method: "post",
        path: "/payments",
        summary: "Create a payment",
        scope: "payments:write",
        list: false,
        body: Some("CreatePayment"),
        params: &[],
    },
    Op {
        method: "get",
        path: "/payments",
        summary: "List payments",
        scope: "payments:read",
        list: true,
        body: None,
        params: &[
            "status",
            "reference",
            "customer_ref",
            "created_from",
            "created_to",
        ],
    },
    Op {
        method: "get",
        path: "/payments/{id}",
        summary: "Get a payment",
        scope: "payments:read",
        list: false,
        body: None,
        params: &[],
    },
    Op {
        method: "post",
        path: "/payments/{id}/attempts",
        summary: "Start a payment attempt",
        scope: "payments:write",
        list: false,
        body: Some("CreateAttempt"),
        params: &[],
    },
    Op {
        method: "post",
        path: "/payments/{id}/cancel",
        summary: "Cancel a payment",
        scope: "payments:write",
        list: false,
        body: None,
        params: &[],
    },
    Op {
        method: "post",
        path: "/payments/{id}/refund",
        summary: "Refund a payment",
        scope: "refunds:write",
        list: false,
        body: Some("RefundInput"),
        params: &[],
    },
    Op {
        method: "get",
        path: "/refunds",
        summary: "List refunds",
        scope: "payments:read",
        list: true,
        body: None,
        params: &["payment_id", "status"],
    },
    Op {
        method: "get",
        path: "/events",
        summary: "Pull the event feed, oldest first",
        scope: "payments:read",
        list: true,
        body: None,
        params: &["type"],
    },
    Op {
        method: "get",
        path: "/saved-methods",
        summary: "List saved payment methods",
        scope: "saved:write or payments:read",
        list: true,
        body: None,
        params: &["customer_ref"],
    },
    Op {
        method: "delete",
        path: "/saved-methods/{id}",
        summary: "Delete a saved payment method",
        scope: "saved:write",
        list: false,
        body: None,
        params: &[],
    },
    Op {
        method: "post",
        path: "/saved-methods/{id}/charge",
        summary: "Charge a saved payment method",
        scope: "saved:write",
        list: false,
        body: Some("CreatePayment"),
        params: &[],
    },
];

fn path_params(path: &str) -> Vec<Value> {
    path.split('/')
        .filter_map(|seg| seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
        .map(|name| json!({"name": name, "in": "path", "required": true, "schema": {"type": "string"}}))
        .collect()
}

fn query(name: &str) -> Value {
    json!({"name": name, "in": "query", "required": false, "schema": {"type": "string"}})
}

fn operation(op: &Op) -> Value {
    let mut params = path_params(op.path);
    if op.list {
        params.push(json!({"name": "limit", "in": "query", "required": false, "schema": {"type": "integer", "minimum": 1, "maximum": 100, "default": 50}}));
        params.push(query("cursor"));
    }
    params.extend(op.params.iter().map(|p| query(p)));
    let mut o = Map::new();
    o.insert("summary".into(), json!(op.summary));
    o.insert(
        "description".into(),
        json!(format!("Requires scope: {}.", op.scope)),
    );
    o.insert("security".into(), json!([{"bearerAuth": []}]));
    o.insert("x-required-scope".into(), json!(op.scope));
    o.insert("parameters".into(), Value::Array(params));
    if let Some(schema) = op.body {
        o.insert("requestBody".into(), json!({"required": true, "content": {"application/json": {"schema": {"$ref": format!("#/components/schemas/{schema}")}}}}));
    }
    let mut ok = json!({"description": "Success", "content": {"application/json": {"schema": {"type": if op.list { "array" } else { "object" }}}}});
    if op.list {
        ok["headers"] = json!({"X-Next-Cursor": {"description": "Pass as cursor to fetch the next page. Absent on the last page.", "schema": {"type": "string"}}});
    }
    let err = |d: &str| json!({"description": d, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Error"}}}});
    o.insert(
        "responses".into(),
        json!({
            "200": ok,
            "400": err("Invalid request"),
            "401": err("Missing, unknown, revoked or expired key"),
            "403": err("Key lacks the required scope, or a limit was hit"),
            "404": err("Not found"),
            "429": err("Rate limited"),
        }),
    );
    Value::Object(o)
}

pub fn spec() -> Value {
    let mut paths: Map<String, Value> = Map::new();
    for op in OPS {
        let entry = paths
            .entry(op.path.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("path object");
        entry.insert(op.method.into(), operation(op));
    }
    paths.insert(
        "/openapi.json".into(),
        json!({"get": {"summary": "This document", "security": [], "responses": {"200": {"description": "OpenAPI 3.1 document"}}}}),
    );
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Payment gateway API",
            "version": "1.0.0",
            "description": "Changes inside /v1 are additive. Breaking changes ship under a new version prefix. See docs/CHANGELOG.md."
        },
        "servers": [{"url": "/v1"}],
        "paths": Value::Object(paths),
        "components": {
            "securitySchemes": {"bearerAuth": {"type": "http", "scheme": "bearer", "description": "Project API key. Keys carry scopes: payments:write, payments:read, refunds:write, saved:write, admin."}},
            "schemas": {
                "Error": {
                    "type": "object",
                    "required": ["error"],
                    "properties": {
                        "error": {"type": "string", "description": "Message string. Stays a string throughout /v1. In /v2 this key becomes the object now found in error_detail and the string moves to a deprecated error_message for one release. Read error_detail.code in new code."},
                        "error_detail": {
                            "type": "object",
                            "required": ["code", "message"],
                            "properties": {"code": {"type": "string"}, "message": {"type": "string"}, "field": {"type": "string"}}
                        }
                    }
                },
                "CreatePayment": {"type": "object", "required": ["amount", "currency"], "properties": {
                    "reference": {"type": "string", "description": "Optional. Your own identifier."},
                    "amount": {"type": "integer", "description": "Minor units"},
                    "currency": {"type": "string"},
                    "tax": {"type": "integer"},
                    "description": {"type": "string"},
                    "customer_ref": {"type": "string"},
                    "customer_email": {"type": "string"},
                    "metadata": {"type": "object"},
                    "methods": {"type": "array", "items": {"type": "string"}},
                    "success_url": {"type": "string"},
                    "cancel_url": {"type": "string"}
                }},
                "CreateAttempt": {"type": "object", "properties": {"method": {"type": "string"}, "option": {"type": "string"}, "save_method": {"type": "boolean"}}},
                "RefundInput": {"type": "object", "properties": {"amount": {"type": "integer", "description": "Minor units. Omit for the full remaining amount."}, "destination": {"type": "string"}}}
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_is_openapi_31_with_every_op() {
        let s = spec();
        assert_eq!(s["openapi"], "3.1.0");
        for op in OPS {
            assert!(s["paths"][op.path][op.method].is_object(), "{}", op.path);
        }
    }
}
