# Monitoring and alerts

## Endpoints

- `GET /health`: liveness, always cheap.
- `GET /ready`: readiness, checks the database.
- `GET /metrics`: Prometheus text. Send `Authorization: Bearer` with the admin token or `METRICS_TOKEN`.
- `GET /status`: operator status as JSON, or HTML with `Accept: text/html`, with the admin token. Shows the version, uptime, database state, the lease holder and the webhook backlog.

Logs are one JSON object per line with `ts`, `level`, `msg`, `request_id` and `tenant_id`. Send `x-request-id` and it is echoed back. Secrets are redacted and webhook bodies are never logged.

## Metrics

`gateway_http_requests_total`, `gateway_http_request_duration_seconds`, `gateway_settlements_total`, `gateway_settlement_duration_seconds`, `gateway_webhook_deliveries_total{outcome}`, `gateway_webhook_pending`, `gateway_webhook_oldest_pending_age_seconds`, `gateway_provider_calls_total{method,op,outcome}`, `gateway_provider_call_duration_seconds`, `gateway_watcher_up{provider}`, `gateway_leader` and `gateway_db_pool_connections{state}`.

## Suggested alerts

| Alert | Condition | Why |
| --- | --- | --- |
| No leader | `sum(gateway_leader) == 0` for 2 minutes | Nothing settles or delivers without the worker lease. |
| Two leaders | `sum(gateway_leader) > 1` for 2 minutes | Lease problem, check the database clock. |
| Watcher down | `gateway_watcher_up == 0` for 5 minutes | A provider's settlement stream is not running. |
| Webhook backlog | `gateway_webhook_oldest_pending_age_seconds > 900` | Customers are not being told about payments. |
| Provider errors | rising `gateway_provider_calls_total{outcome="error"}` | A provider or credential is failing. |
| Not ready | `/ready` failing for 1 minute | Database unreachable. |
| Pool exhausted | `gateway_db_pool_connections{state="idle"} == 0` for 5 minutes | Raise `DB_MAX_CONNECTIONS` or find the slow query. |

Check the exact label values on your own `/metrics` output before pasting these into a rule file.
