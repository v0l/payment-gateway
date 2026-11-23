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
        method: "post",
        path: "/withdrawals",
        summary: "Create a withdrawal (payout)",
        scope: "withdrawals:write",
        list: false,
        body: Some("CreateWithdrawal"),
        params: &[],
    },
    Op {
        method: "get",
        path: "/withdrawals",
        summary: "List withdrawals",
        scope: "withdrawals:read",
        list: true,
        body: None,
        params: &["status"],
    },
    Op {
        method: "get",
        path: "/withdrawals/{id}",
        summary: "Get a withdrawal",
        scope: "withdrawals:read",
        list: false,
        body: None,
        params: &[],
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
        method: "post",
        path: "/saved-methods",
        summary: "Import an existing provider saved card",
        scope: "saved:write",
        list: false,
        body: Some("ImportSavedMethod"),
        params: &[],
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
    Op {
        method: "post",
        path: "/plans",
        summary: "Create a billing plan",
        scope: "payments:write",
        list: false,
        body: Some("CreatePlan"),
        params: &[],
    },
    Op {
        method: "get",
        path: "/plans",
        summary: "List billing plans",
        scope: "payments:read",
        list: true,
        body: None,
        params: &[],
    },
    Op {
        method: "get",
        path: "/plans/{id}",
        summary: "Get a billing plan",
        scope: "payments:read",
        list: false,
        body: None,
        params: &[],
    },
    Op {
        method: "post",
        path: "/plans/{id}/archive",
        summary: "Archive a billing plan",
        scope: "payments:write",
        list: false,
        body: None,
        params: &[],
    },
    Op {
        method: "post",
        path: "/subscriptions",
        summary: "Create a subscription",
        scope: "payments:write",
        list: false,
        body: Some("CreateSubscription"),
        params: &[],
    },
    Op {
        method: "get",
        path: "/subscriptions",
        summary: "List subscriptions",
        scope: "payments:read",
        list: true,
        body: None,
        params: &["customer_ref", "status"],
    },
    Op {
        method: "get",
        path: "/subscriptions/{id}",
        summary: "Get a subscription with its billing history",
        scope: "payments:read",
        list: false,
        body: None,
        params: &[],
    },
    Op {
        method: "post",
        path: "/subscriptions/{id}/pause",
        summary: "Pause a subscription",
        scope: "payments:write",
        list: false,
        body: None,
        params: &[],
    },
    Op {
        method: "post",
        path: "/subscriptions/{id}/resume",
        summary: "Resume a subscription",
        scope: "payments:write",
        list: false,
        body: None,
        params: &[],
    },
    Op {
        method: "post",
        path: "/subscriptions/{id}/cancel",
        summary: "Cancel a subscription now or at period end",
        scope: "payments:write",
        list: false,
        body: Some("CancelSubscription"),
        params: &[],
    },
    Op {
        method: "post",
        path: "/subscriptions/{id}/change-plan",
        summary: "Change plan, optionally prorating",
        scope: "payments:write",
        list: false,
        body: Some("ChangePlan"),
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
    let payment_op = op.path.starts_with("/payments") && !op.path.ends_with("/refund");
    let withdrawal_op = op.path.starts_with("/withdrawals");
    let item = if withdrawal_op {
        json!({"$ref": "#/components/schemas/Withdrawal"})
    } else if payment_op {
        json!({"$ref": "#/components/schemas/Payment"})
    } else if op.path == "/saved-methods" {
        json!({"$ref": "#/components/schemas/SavedMethod"})
    } else if op.path == "/methods" {
        json!({"$ref": "#/components/schemas/Method"})
    } else {
        json!({"type": "object"})
    };
    let mut ok = json!({"description": "Success", "content": {"application/json": {"schema": if op.list { json!({"type": "array", "items": item}) } else { item }}}});
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

pub const EVENT_TYPES: &[&str] = &[
    "payment.paid",
    "payment.expired",
    "payment.cancelled",
    "payment.late_payment",
    "attempt.failed",
    "refund.completed",
    "refund.failed",
    "payment.disputed",
    "payment.dispute_closed",
    "withdrawal.requested",
    "withdrawal.completed",
    "withdrawal.failed",
    "withdrawal.paid",
];

pub const PROVIDER_TYPES: &[&str] = &[
    "lightning/lnd",
    "onchain/lnd",
    "stripe/stripe",
    "revolut/revolut",
    "paypal/paypal",
    "fixedfloat/fixedfloat",
];

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
        "x-console": {"POST /console/api/projects/{id}/providers/{pid}/test": {"summary": "Test a provider's credentials without creating a payment", "auth": "console session, project admin", "response": {"$ref": "#/components/schemas/ProviderTestResult"}}},
        "components": {
            "securitySchemes": {"bearerAuth": {"type": "http", "scheme": "bearer", "description": "Project API key. Keys carry scopes: payments:write, payments:read, refunds:write, saved:write, withdrawals:read, withdrawals:write, admin."}},
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
                "Method": {"type": "object", "properties": {
                    "id": {"type": "integer"},
                    "platform_managed": {"type": "boolean", "description": "True when the method runs on a shared platform wallet and settles into the project balance minus the platform fee."},
                    "method": {"type": "string"},
                    "name": {"type": "string"},
                    "fee_rate": {"type": ["number", "null"]},
                    "fee_base": {"type": ["integer", "null"]},
                    "fee_currency": {"type": ["string", "null"]}
                }},
                "Attempt": {"type": "object", "properties": {
                    "id": {"type": "integer"},
                    "method": {"type": "string"},
                    "status": {"type": "string"},
                    "amount": {"type": "integer", "description": "Charged amount in minor units (msats for BTC)."},
                    "platform_managed": {"type": "boolean"},
                    "platform_fee": {"type": ["integer", "null"], "description": "Platform fee in msats, set when a platform managed attempt is paid. Rounded up."},
                    "net_credited": {"type": ["integer", "null"], "description": "Msats credited to the project balance, set when a platform managed attempt is paid. Never more than was received."}
                }},
                "LineItemInput": {"type": "object", "required": ["name", "unit_amount"], "properties": {
                    "name": {"type": "string", "maxLength": 127},
                    "description": {"type": "string", "maxLength": 255},
                    "quantity": {"type": "integer", "minimum": 1, "maximum": 1000000, "default": 1},
                    "unit_amount": {"type": "integer", "description": "Minor units"},
                    "tax_rate": {"type": "number", "description": "Percent, 0 to 100, at most 2 decimals. Exclusive with tax_amount."},
                    "tax_amount": {"type": "integer", "description": "Minor units. Exclusive with tax_rate."},
                    "sku": {"type": "string", "maxLength": 64}
                }},
                "ImportSavedMethod": {"type": "object", "required": ["customer_ref", "provider_id", "external_customer_id", "external_id"], "description": "Registers a card that already exists at the provider. Idempotent on provider_id and external_id: repeating the call returns the same saved method. The external ids are never returned.", "properties": {
                    "customer_ref": {"type": "string", "maxLength": 128},
                    "provider_id": {"type": "integer", "description": "One of your enabled card providers"},
                    "external_customer_id": {"type": "string", "maxLength": 255, "description": "The provider's customer id"},
                    "external_id": {"type": "string", "maxLength": 255, "description": "The provider's payment method id"},
                    "brand": {"type": "string", "maxLength": 32},
                    "last4": {"type": "string", "maxLength": 8},
                    "exp_month": {"type": "integer", "minimum": 1, "maximum": 12},
                    "exp_year": {"type": "integer"}
                }},
                "SavedMethod": {"type": "object", "properties": {
                    "id": {"type": "integer"}, "customer_ref": {"type": "string"}, "provider_id": {"type": "integer"},
                    "brand": {"type": ["string", "null"]}, "last4": {"type": ["string", "null"]},
                    "exp_month": {"type": ["integer", "null"]}, "exp_year": {"type": ["integer", "null"]}
                }},
                "DiscountInput": {"type": "object", "description": "Exactly one of amount or percent.", "properties": {
                    "amount": {"type": "integer", "description": "Minor units"},
                    "percent": {"type": "number", "description": "0 to 100, at most 2 decimals. Rounded half up on the subtotal."},
                    "code": {"type": "string", "maxLength": 64}
                }},
                "LineItem": {"type": "object", "properties": {
                    "name": {"type": "string"}, "description": {"type": ["string", "null"]}, "sku": {"type": ["string", "null"]},
                    "quantity": {"type": "integer"}, "unit_amount": {"type": "integer"},
                    "amount": {"type": "integer", "description": "quantity times unit_amount"},
                    "discount": {"type": "integer", "description": "This line's share of the discount"},
                    "tax_rate": {"type": ["number", "null"]}, "tax": {"type": "integer"},
                    "total": {"type": "integer", "description": "amount minus discount plus tax"}
                }},
                "Payment": {"type": "object", "description": "Payment view, also the webhook data.payment object.", "properties": {
                    "id": {"type": "string"}, "reference": {"type": "string"},
                    "amount": {"type": "integer", "description": "Total in minor units: subtotal minus discount plus tax"},
                    "tax": {"type": "integer"}, "currency": {"type": "string"}, "status": {"type": "string"},
                    "line_items": {"type": "array", "items": {"$ref": "#/components/schemas/LineItem"}},
                    "subtotal": {"type": ["integer", "null"], "description": "Sum of line amounts, null without line items"},
                    "discount": {"type": ["object", "null"], "properties": {"amount": {"type": "integer"}, "percent": {"type": ["number", "null"]}, "code": {"type": ["string", "null"]}}},
                    "disputed": {"type": "boolean", "default": false, "description": "True while a chargeback or dispute is open. The status itself does not change."},
                    "dispute_status": {"type": ["string", "null"], "enum": ["open", "won", "lost", null], "description": "Latest dispute state, null when the payment was never disputed."},
                    "receipt_url": {"type": "string", "description": "Public HTML receipt, available once paid"},
                    "pay_url": {"type": "string"},
                    "attempts": {"type": "array", "items": {"$ref": "#/components/schemas/Attempt"}}
                }},
                "CreatePayment": {"type": "object", "required": ["currency"], "description": "Send amount, or line_items to have the server compute it. With line_items, amount and tax are optional and must equal the computed values when sent.", "properties": {
                    "reference": {"type": "string", "description": "Optional. Your own identifier."},
                    "amount": {"type": "integer", "description": "Minor units. Optional with line_items."},
                    "currency": {"type": "string"},
                    "tax": {"type": "integer"},
                    "line_items": {"type": "array", "maxItems": 100, "items": {"$ref": "#/components/schemas/LineItemInput"}},
                    "discount": {"$ref": "#/components/schemas/DiscountInput"},
                    "description": {"type": "string"},
                    "customer_ref": {"type": "string"},
                    "customer_email": {"type": "string"},
                    "metadata": {"type": "object"},
                    "methods": {"type": "array", "items": {"type": "string"}},
                    "success_url": {"type": "string"},
                    "cancel_url": {"type": "string"}
                }},
                "ProviderType": {"type": "string", "enum": PROVIDER_TYPES, "description": "method/provider_type pairs a project can configure in the console. Config fields per type are in the README."},
                "ProviderTestResult": {"type": "object", "required": ["ok", "message"], "properties": {"ok": {"type": "boolean"}, "message": {"type": "string", "description": "Fixed success text or a sanitized error. Never contains credentials."}}},
                "EventType": {"type": "string", "enum": EVENT_TYPES, "description": "Webhook and /v1/events types. Endpoint filters accept these, a prefix wildcard such as withdrawal.*, or *. withdrawal.* events carry data.withdrawal instead of data.payment."},
                "CreateAttempt": {"type": "object", "properties": {"method": {"type": "string"}, "option": {"type": "string"}, "save_method": {"type": "boolean"}}},
                "CreatePlan": {"type": "object", "required": ["name", "amount", "currency"], "properties": {
                    "name": {"type": "string"}, "description": {"type": "string"},
                    "amount": {"type": "integer", "description": "Minor units per period"},
                    "currency": {"type": "string"},
                    "interval": {"type": "string", "enum": ["day", "week", "month", "year"], "default": "month"},
                    "interval_count": {"type": "integer", "default": 1},
                    "trial_days": {"type": "integer", "default": 0},
                    "grace_days": {"type": "integer", "default": 3, "description": "Days a customer has to pay a period's invoice when the method cannot be charged automatically"}
                }},
                "CreateSubscription": {"type": "object", "required": ["customer_ref", "plan_id"], "properties": {
                    "customer_ref": {"type": "string"}, "plan_id": {"type": "integer"},
                    "saved_method_id": {"type": "integer", "description": "Omit to get a first payment (payment_id, pay_url) that saves the card and activates the subscription when it settles."}
                }},
                "CancelSubscription": {"type": "object", "properties": {"at_period_end": {"type": "boolean", "default": false}}},
                "ChangePlan": {"type": "object", "required": ["plan_id"], "properties": {"plan_id": {"type": "integer"}, "proration": {"type": "string", "enum": ["none", "prorate"], "default": "none"}}},
                "CreateWithdrawal": {"type": "object", "required": ["amount"], "description": "Send the Idempotency-Key header (required). With provider_id the payout is paid from that project LND wallet to destination (bolt11 invoice, lightning address, lnurl or on-chain address) and does not touch the custody balance. Without it the payout is paid from the custody balance under the payout limits and approval rules, and destination must be a bolt11 invoice (or send destination_id of a verified saved destination).", "properties": {
                    "amount": {"type": "integer", "description": "Msats, a whole number of sats."},
                    "currency": {"type": "string", "enum": ["BTC"], "default": "BTC"},
                    "destination": {"type": "string"},
                    "destination_id": {"type": "integer", "description": "Custody only: a verified saved destination."},
                    "provider_id": {"type": "integer", "description": "One of the project's own LND providers, from GET /methods."}
                }},
                "Withdrawal": {"type": "object", "description": "Withdrawal view, also the webhook data.withdrawal object.", "properties": {
                    "id": {"type": "integer"},
                    "amount": {"type": "integer"}, "currency": {"type": "string"},
                    "status": {"type": "string", "enum": ["requested", "approved", "sending", "completed", "failed", "rejected", "cancelled"]},
                    "destination_kind": {"type": "string"}, "destination_label": {"type": "string", "description": "Masked destination."},
                    "fee": {"type": ["integer", "null"]}, "max_fee": {"type": "integer"},
                    "external_id": {"type": ["string", "null"], "description": "Payment hash or txid once paid."},
                    "error": {"type": ["string", "null"]},
                    "idempotency_key": {"type": ["string", "null"]},
                    "provider_id": {"type": ["integer", "null"]},
                    "created": {"type": "string"}, "finished_at": {"type": ["string", "null"]}
                }},
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

    #[test]
    fn payment_schema_carries_the_platform_fee_and_net() {
        let s = spec();
        let attempt = &s["components"]["schemas"]["Attempt"]["properties"];
        for field in ["platform_managed", "platform_fee", "net_credited"] {
            assert!(attempt[field].is_object(), "{field}");
        }
        assert!(s["components"]["schemas"]["Method"]["properties"]["platform_managed"].is_object());
        assert_eq!(
            s["paths"]["/payments/{id}"]["get"]["responses"]["200"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/Payment"
        );
    }
}
