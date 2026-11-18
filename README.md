# payment-gateway

A self contained payment gateway. Apps ask it to collect money (Lightning, on-chain Bitcoin, cards, bank, PayPal) and it tells them when it is paid, with optional custody and payouts. App logic such as subscriptions and credit stays in the app.

- Documentation, quickstarts and the operator guide: the gateway serves them at `/`, `/guide/` and `/operator/`. The sources are in [docs-site/](docs-site/). Regenerate the pages with `node docs-site/build.mjs`.
- API reference: `/docs`, generated from `GET /v1/openapi.json`.
- Full configuration table, endpoints and operations notes: [docs/reference.md](docs/reference.md).
- Design: [DESIGN.md](DESIGN.md). API changes: [docs/CHANGELOG.md](docs/CHANGELOG.md).

## Run

```
GATEWAY_ENCRYPTION_KEY=$(openssl rand -hex 32)
GATEWAY_ADMIN_TOKEN=$(openssl rand -hex 24)
DATABASE_URL=mysql://user:pass@host/gateway
GATEWAY_PUBLIC_URL=https://pay.example.com
cargo run --release
```

## Test

```
docker run -d --name pgw-test-db -e MARIADB_ROOT_PASSWORD=test -p 127.0.0.1:33907:3306 mariadb:11.4
export TEST_DATABASE_URL=mysql://root:test@127.0.0.1:33907/
CARGO_BUILD_JOBS=2 cargo test
```
