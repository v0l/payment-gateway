# API changelog

## Versioning

The API is versioned by path prefix. Inside `/v1` changes are additive only: new endpoints, new optional request fields, new response fields, new event types, new scopes. A change that removes or renames something, tightens validation or changes meaning ships under a new prefix (`/v2`) and `/v1` keeps working for a published deprecation period. Clients must ignore response fields and event types they do not know.

## v1 (current)

- OpenAPI 3.1 description at `GET /v1/openapi.json` (no auth) and a rendered reference at `/docs`.
- Scoped API keys per project: `payments:read`, `payments:write`, `refunds:write`, `saved:write`, `admin`. The existing project key is now a `default` key with all scopes. Keys can expire, be revoked and show when they were last used. Keys are stored as a SHA-256 hash.
- Several webhook endpoints per project with event filters, per-endpoint signing secrets, enable and disable, test events and delivery stats. The single legacy webhook URL keeps working unchanged.
- Cursor pagination on list endpoints: `limit` (1 to 100) and `cursor` query parameters, next cursor in the `X-Next-Cursor` response header. Responses stay bare arrays.
- New list endpoints: `GET /v1/refunds`, `GET /v1/events` (pull feed of the same events webhooks carry).
- `GET /v1/payments` filters: `status`, `reference`, `customer_ref`, `created_from`, `created_to`. `reference` is now optional on create (one is generated).
- Errors: `error` stays a string. A new `error_detail` object carries `code`, `message` and optional `field`. Existing clients that read `error` keep working. New clients should read `error_detail.code`.
