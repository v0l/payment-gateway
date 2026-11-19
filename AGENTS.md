# Agent guide

Rust service (axum, sqlx on MySQL/MariaDB). Read `DESIGN.md` for the model and `README.md` for the API. This file is how to work in the repo.

## Layout

- `src/api/`: `admin.rs` (admin token API), `tenant.rs` (`/v1`, API key auth), `public.rs` (pay page, embed, SSE, provider webhooks), `console.rs` (`/console/api`, session auth), `sandbox.rs` (regtest tools, only when `SANDBOX_*` env is set)
- `src/service.rs`: payments, attempts, settlement, expiry, refunds, webhook delivery. All money state changes go through here, inside one DB transaction with the payment row locked.
- `src/providers/`: one file per provider behind the `Provider` trait (`mod.rs`). Mock provider is `mock.rs`, used by almost every test.
- `src/db.rs`, `migrations/`: queries and schema. Migrations are forward only. Never edit an applied migration. Add a new file named `<UTC timestamp YYYYMMDDHHMMSS>_<name>.sql` (for example `20261118093000_audit_log.sql`), never the next small number: several branches add migrations in parallel and numbered ones collide. Existing `0001`-`0004` stay as they are and sort first.
- `src/validate.rs`, `src/ssrf.rs`: customer supplied config and URL checks. Anything a customer can type that makes the server open a connection or read a file must go through these.
- `src/accounts.rs`, `src/limiter.rs`, `src/crypto.rs`: accounts and sessions, login limiter, sealing of secrets at rest.
- `web/`: bun + Vite + Preact + TypeScript (strict), no plain JS. Entries: `src/pay`, `src/receipt`, `src/gateway` (the embed script), `src/console` (preact-iso routes), `src/sandbox`, `src/styleguide`, `src/site` (landing, guide, operator, legal and the API reference). `docs-site/*.md` is rendered at build time by `scripts/prerender.tsx` into `dist/site/*.html` with one shared layout (`src/site/layout.tsx`); there are no generated HTML files in git other than `dist`. Shared CSS: `src/tokens.css` (the only colour literals), `src/shared/shell.css`, `src/components.css`. API types are generated from `docs-site/openapi.json` into `src/api/schema.d.ts` by `bun run gen:api`.
- Web dev loop: `cd web && bun install`, start a gateway (`scripts/start-test-gateway.sh` or `cargo run -- serve`), then `GATEWAY_URL=http://127.0.0.1:8080 bun run dev`. Vite proxies `/pay`, `/embed`, `/v1`, `/console/api` and `/receipt` to it. The Rust binary embeds `web/dist` with rust-embed (`src/web.rs`). `dist` is committed so `cargo build` and `cargo test` need no bun; run `bun run build` after any change in `web/` and commit the result. CI fails when `dist` or the generated API types are stale. The pay bundle has a gzip budget in `web/scripts/budget.ts`. The Dockerfile has a bun stage that rebuilds `dist`.
- `tests/`: integration tests over a real database. `tests/common/mod.rs` has the harness.
- `regtest/compose.yaml`: bitcoind and two LND nodes for the real lightning and on-chain test.

## Build and test

```
docker run -d --name pgw-test-db -e MARIADB_ROOT_PASSWORD=test -p 127.0.0.1:33907:3306 mariadb:11.4
export TEST_DATABASE_URL=mysql://root:test@127.0.0.1:33907/
CARGO_BUILD_JOBS=2 cargo test
CARGO_BUILD_JOBS=2 cargo clippy --all-targets
cargo fmt
(cd web && bun run check && bun test && bun run build)
```

- Always set `CARGO_BUILD_JOBS=2`. A default build uses 9 GB or more and several agents run at once. When using the bash tool pass `memory_mb: 0` for cargo.
- If the test container is not running, start it. Each test creates its own database, so tests never collide.
- Real LND test: `docker compose -f regtest/compose.yaml up -d`, copy certs and macaroons as `tests/regtest_e2e.rs` describes, run `REGTEST=1 cargo test --test regtest_e2e`, then `docker compose down -v`.
- Build needs `protoc`.

## Rules

- A bug is not fixed without a regression test that failed first. A feature is not done without tests for the happy path, the failure paths and tenant isolation.
- Do not add code comments. Names carry the meaning.
- Commit messages: a short subject, no body unless it records something the diff cannot show, no trailers.
- No em or en dashes in docs, UI text, or commit messages.
- Money is integer minor units (msats for BTC). Never use floats for amounts that are stored or compared.
- Settlement must stay exactly once: lock the payment row, check state, write, emit the webhook row in the same transaction. Do not run a query on the pool while holding a transaction that locks a row it needs.
- Secrets are sealed at rest and never returned by any endpoint or written to logs. Add a test that a new secret field is not echoed.
- Customer facing features must work for an untrusted project (see `validate.rs`). Operator only behaviour is gated on `trusted` or `is_operator`.
- Every tenant scoped query takes the tenant id. Add a test where tenant B cannot read or change tenant A's data.
- Several agents work at once in separate worktrees. Keep your change set focused, rebase on the default branch before you finish, and resolve conflicts yourself. Prefer adding a new module over growing a shared file.
- Keep files under about 600 lines. Split by concern instead of growing `service.rs`, `db.rs` and the console modules.

## Definition of done

1. `cargo test` passes in full, `cargo clippy --all-targets` has no warnings, `cargo fmt` is clean, in `web/`: `bun run check`, `bun test`, `bun run build` (commit `dist`), and `bun run e2e` for UI changes (needs `TEST_DATABASE_URL`, `CHROME` and a built `payment-gateway` binary).
2. New behaviour is covered by tests as above. For UI changes, also run the binary and check the page in a headless browser.
3. `README.md` or `DESIGN.md` updated when public behaviour changes.
4. One focused commit series on your branch. Move the issue to review with a short note on how you verified it.

## Do not

- Do not deploy, push images, or touch the cluster. Deploys are done by the maintainer.
- Do not run the regtest stack on the real cluster ports, or leave containers running.
- Do not commit secrets or `.env` files.
