# Error reference

Every error response has the same shape. `error` is a string and stays a string for all of `/v1`. New code should read `error_detail`.

```json
{
  "error": "amount must be positive",
  "error_detail": {"code": "invalid_request", "message": "amount must be positive", "field": "amount"}
}
```

| Status | Code | Meaning |
| --- | --- | --- |
| 400 | `invalid_request` | The body or a parameter is invalid. `field` names it when known. |
| 401 | `unauthorized` | Missing, unknown, revoked or expired API key. |
| 403 | `forbidden` | The action is not allowed for this project. |
| 403 | `insufficient_scope` | The key lacks the scope. The body carries `scope`. |
| 403 | `limit_exceeded` | A usage limit was hit. The body carries `limit`, `current` and `max`. |
| 404 | `not_found` | No such object, or it belongs to another project. |
| 409 | `conflict` | The object is in the wrong state, or an idempotency key was reused with different parameters. |
| 413 | `payload_too_large` | The body is over 1 MiB. |
| 429 | `rate_limited` | Too many requests. Wait for the `Retry-After` header. |
| 502 | `upstream_error` | A provider failed. Retry later. |
| 500 | `internal_error` | Unexpected failure. Retry with the same idempotency key. |

Retry 429, 502 and 5xx with backoff. Do not retry other 4xx without changing the request.
