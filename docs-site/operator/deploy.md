# Deploying

The gateway is one binary and a MariaDB or MySQL database. The container image is built from the `Dockerfile` in the repository.

## Environment

```sh
GATEWAY_ENCRYPTION_KEY=$(openssl rand -hex 32)
GATEWAY_ADMIN_TOKEN=$(openssl rand -hex 24)
DATABASE_URL=mysql://gateway:password@db/gateway
GATEWAY_PUBLIC_URL=https://pay.example.com
GATEWAY_LISTEN=0.0.0.0:8080
```

`GATEWAY_ENCRYPTION_KEY` seals provider credentials and webhook secrets in the database. Keep a copy offline and separate from database backups. If every copy is lost those secrets cannot be read and must be entered again. The variables `DATABASE_URL`, `GATEWAY_ENCRYPTION_KEY`, `GATEWAY_ADMIN_TOKEN` and `METRICS_TOKEN` also accept a `_FILE` variant that names a mounted secret file.

All variables are validated at start and every problem is reported at once with exit code 2. The full table is in `docs/reference.md` in the repository.

`GATEWAY_ALLOW_MOCK` is for tests only. Never set it in production.

## Database

Create an empty database and a user with full rights on it. Migrations run on start and are forward only. Use MariaDB 11.4 or a compatible MySQL. Size the pool with `DB_MAX_CONNECTIONS` (default 10) across all replicas.

## Ingress

Put a TLS terminating proxy in front and forward to `GATEWAY_LISTEN`. Requirements:

- The proxy must append the client address to `X-Forwarded-For`. The gateway trusts the last hop for rate limiting, so a proxy that passes a client supplied header through unchanged weakens the limits.
- `GATEWAY_PUBLIC_URL` must be the public https address. It is used for pay pages and for provider webhook URLs.
- Allow server sent events on `/pay/{id}/events` (no response buffering).
- Route `/health` to liveness probes and `/ready` to readiness probes.

## One worker, many replicas

Every replica serves the API. Background work (provider watchers, the expiry sweep, the webhook sender, reconciliation and payouts) runs only on the replica holding the `workers` lease in the `worker_lease` table. The lease is renewed every third of `LEASE_TTL_SECS` (default 30) and taken over after it expires, so a rolling deploy with several replicas is safe. Never run two gateways against one database with different encryption keys.

On SIGTERM the process stops accepting, finishes in-flight requests and settlements, releases the lease and exits. Give the pod at least `SHUTDOWN_GRACE_SECS` (default 25) plus a margin.

## First account

Create the first operator with the admin token, then use the console:

```sh
curl -X POST "$GW/admin/invites" \
  -H "Authorization: Bearer $GATEWAY_ADMIN_TOKEN" -H "Content-Type: application/json" \
  -d '{"email":"you@example.com","operator":true}'
```

Open the returned `invite_url`, choose a password and enrol two factor authentication. Set `GATEWAY_OPERATOR_2FA_GRACE=1` only while bootstrapping.

## Pay page checks

`web/scripts/e2e/` holds a Playwright and axe-core run for the pay page, written in TypeScript and run by bun. It starts a gateway on a fresh database with the mock provider and scans the page in light, dark and 360 px layouts for serious accessibility problems, horizontal scroll and small tap targets. Run `bun install` in `web/`, build the binary, then:

```sh
TEST_DATABASE_URL=mysql://root:test@127.0.0.1:33907/ CHROME=/usr/bin/google-chrome bun scripts/e2e/pay.ts
```
