# Security review and threat model

Scope: the gateway binary, its database, the pay page and console front ends, and the provider and webhook integrations. Deployment, cluster and network hardening are the maintainer's and are out of scope here, as are the third party providers themselves.

## Assets

- Customer funds and the payment state machine (settlement must happen exactly once).
- Provider credentials (Stripe and Revolut keys, LND macaroons and certs), sealed at rest.
- Tenant API keys and webhook secrets, stored hashed or sealed.
- Console accounts, sessions and invites.
- Tenant data: payments, customers' saved methods, metadata, delivery history, audit log.
- The operator's own admin token.

## Trust boundaries

| Actor | Trusted for | Not trusted for |
|---|---|---|
| Customer browser | Holding a payment id (capability for `/pay/{id}`) | Anything else. Input is validated, pages are framed-denied, CSP forbids inline script |
| Tenant server | Acting for its own project through `/v1` with its API key | Reading or changing another project. Choosing return URLs outside its `allowed_origins`. Pointing the server at internal hosts (SSRF checks on every URL and provider config) |
| Provider callbacks | Nothing until the signature verifies | Settling another tenant's or another provider's attempt. Callbacks are routed by tenant and provider id and verified with that provider's secret |
| Platform operator | Admin API and operator console routes | Nothing is hidden from the operator. Operator actions are audited |
| Other tenants | Nothing | Every tenant scoped query takes the tenant id. `tests/security.rs` sweeps the `/v1` and `/console/api` routes as a second tenant |

## In scope

Authentication and sessions, CSRF, cross-tenant isolation, SSRF, open redirects, header and CSP policy, input bounds, amount and rate arithmetic, webhook signing and verification, secret handling at rest and in responses and logs, log injection, dependency advisories.

## Out of scope

Host and network hardening, TLS termination, DDoS, the security of Stripe, Revolut, LND and bitcoind themselves, and physical access. Deploys are done by the maintainer.

## Password reset

There is no password reset flow. Accounts are created from operator invites and an operator can issue a new invite. This removes the reset token attack surface. Adding a reset flow requires its own review.

## Findings

| # | Finding | Fix | Test |
|---|---|---|---|
| 1 | SSRF filter missed IPv4-compatible and 6to4 IPv6, Teredo, site-local, NAT64, `192.88.99.0/24` and URLs with userinfo | `src/ssrf.rs` blocks them and resolves IP literals without DNS | unit tests in `src/ssrf.rs`, `untrusted_projects_cannot_point_webhooks_or_providers_at_internal_hosts` |
| 2 | Open redirect: `success_url`, `cancel_url` and project defaults were not checked against `allowed_origins` | Rejected at create and in settings, stored URLs that no longer match fall back to the pay page | `return_urls_outside_allowed_origins_are_refused` |
| 3 | Argon2 relied on implicit defaults, no password policy beyond length | Explicit Argon2id parameters, common and single character passwords refused | unit tests in `src/accounts.rs` |
| 4 | Amount and rate arithmetic used unchecked float casts, no maximum amount | `MAX_AMOUNT`, checked conversion, u128 fee maths, fee rate bounds | `tests/fuzz.rs`, `amounts_beyond_range_and_control_characters_are_rejected` |
| 5 | Control characters in references and descriptions, and in logged upstream errors, webhook errors and audit origin fields | Rejected on input, escaped by `src/logsafe.rs`, stripped in `audit::Origin` | `audit_origin_strips_control_characters`, unit tests in `src/logsafe.rs` |
| 6 | No request body limit | 256 KiB limit on all routers | `oversized_and_deeply_nested_bodies_are_rejected_without_panics` |
| 7 | No `X-Frame-Options` fallback for old browsers | Sent on pay, console and sandbox pages | `html_surfaces_forbid_framing_and_scripts_from_elsewhere` |
| 8 | CSRF token and mock provider secret compared in variable time | `subtle::ConstantTimeEq` | `csrf_token_must_match_exactly` |
| 9 | Provider config key `secret` was not redacted in views | Added to `SECRET_KEYS` | `secrets_are_never_echoed_by_tenant_or_console_endpoints` |
| 10 | Untrusted LND URLs are checked at configuration time but the connection is not pinned to the checked address (DNS rebinding gap) | Fixed: the registry resolves once and connects to the checked IP | `pinned_url_uses_the_checked_address_when_dns_flips` |
| 11 | Ledger balance and entries (console and admin) | Reviewed: project routes check membership, admin routes need the admin token, no change needed | `ledger_balance_and_entries_are_private_to_the_project` |
| 12 | 2FA login challenge and codes across accounts | Reviewed: a challenge only completes with a factor of the account it was issued for, and is single use. 2FA routes need a session and CSRF | `two_factor_challenges_and_codes_cannot_cross_accounts`, `two_factor_routes_need_a_session_and_csrf_and_only_touch_own_account` |
| 13 | `/metrics`, `/status` and `/ready` exposure | Reviewed: metrics and status answer 401 to anonymous calls, tenant keys and console sessions. The metrics token opens only `/metrics`. `/ready` returns only readiness flags | `ops_endpoints_need_the_admin_or_metrics_token_and_never_a_tenant_key` |
| 14 | JSON logs and credentials | Reviewed: API keys, bearer tokens, session cookies and CSRF tokens never appear in request logs, and the redactor masks Authorization and cookie style strings | `json_logs_never_carry_authorization_or_cookie_values` |

The console HTML shell, which now includes the 2FA screens, is served by the same handler and covered by the CSP and frame checks. The sandbox and regtest tooling create payments without return URLs, so the allow-list does not affect them.

Checked and found sound: tenant scoping of every query reached by the isolation sweep, session fixation (login always issues a new token), cookie flags, absence of CORS headers outside `/gateway.js`, webhook routing by tenant and provider, `allowed_origins` header injection.

## Dependency audit

`deny.toml` is checked in CI by the `audit` job (`cargo deny check`). Three advisories are ignored with reasons recorded in the file: `rsa` (sqlx-mysql handshake only), `rustls-pemfile` and `chrono` (unmaintained, no replacement available through our dependencies).
