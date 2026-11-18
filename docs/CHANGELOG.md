# API changelog

## Versioning

The API is versioned by path prefix. Inside `/v1` changes are additive only: new endpoints, new optional request fields, new response fields, new event types, new scopes. A change that removes or renames something, tightens validation or changes meaning ships under a new prefix (`/v2`) and `/v1` keeps working for a published deprecation period. Clients must ignore response fields and event types they do not know.

## v1 (current)

- Platform billing events `platform.invoice.created`, `platform.invoice.paid`, `platform.invoice.past_due` and `platform.invoice.waived`, carrying `data.invoice` and a null payment. Additive; endpoint filters such as `platform.*` work.
- Landing page at `/`, documentation at `/guide/`, operator guide at `/operator/` and draft legal templates at `/legal/`, all static and served from the binary. No API change.
- Recurring billing: `/v1/plans` and `/v1/subscriptions` (create, list, get, pause, resume, cancel, change plan with optional proration) and the events `subscription.created`, `invoice.paid`, `invoice.failed`, `invoice.payment_required`, `subscription.past_due`, `subscription.cancelled`, `subscription.paused`, `subscription.resumed`, `subscription.updated`. Subscription events carry `data.subscription` and a null `data.payment` when there is no payment. Purely additive.
- OpenAPI 3.1 description at `GET /v1/openapi.json` (no auth) and a rendered reference at `/docs`.
- Scoped API keys per project: `payments:read`, `payments:write`, `refunds:write`, `saved:write`, `admin`. The existing project key is now a `default` key with all scopes. Keys can expire, be revoked and show when they were last used. Keys are stored as a SHA-256 hash.
- Several webhook endpoints per project with event filters, per-endpoint signing secrets, enable and disable, test events and delivery stats. Each project's existing `webhook_url` and signing secret became its first (default) endpoint, matching all events. All deliveries now go through endpoints, so there is exactly one delivery per event per endpoint. The `webhook_url` field in the settings API and `webhook_secret` rotation still work for one release by reading and writing that default endpoint; use the endpoints API instead. The field is removed in the next release.
- New event types `withdrawal.requested`, `withdrawal.completed` and `withdrawal.failed`. They carry `data.withdrawal` instead of `data.payment`, follow endpoint filters (`withdrawal.*`) and appear in `GET /v1/events`. `payment_id` on the delivery and event rows can now be empty.
- Cursor pagination on list endpoints: `limit` (1 to 100) and `cursor` query parameters, next cursor in the `X-Next-Cursor` response header. Responses stay bare arrays.
- New list endpoints: `GET /v1/refunds`, `GET /v1/events` (pull feed of the same events webhooks carry).
- `GET /v1/payments` filters: `status`, `reference`, `customer_ref`, `created_from`, `created_to`. `reference` is now optional on create (one is generated).
- Errors: `error` stays a string. A new `error_detail` object carries `code`, `message` and optional `field`. Existing clients that read `error` keep working. New clients should read `error_detail.code`.
- Error migration plan: `error` is a string in all `/v1` releases, so existing clients are not broken. In `/v2`, `error` becomes the object `{code, message, field?}` (today's `error_detail`) and the string moves to the deprecated `error_message` for one release. Until then `error_detail` is the stable way to read the structured form.
- `reference` on `POST /v1/payments` is optional. When omitted the gateway generates `pay_<random>`. Sending one behaves as before, including idempotency checks that compare it.
