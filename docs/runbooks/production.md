# Production: pay.apexstrata.ie

Production runs beside the sandbox in its own namespace `payment-gateway-prod` on axst-k8s. The sandbox (`payment-gateway`, `pay-sandbox.apexstrata.ie`) is not touched. The first tenant is YALR (issue #854).

Manifests live in `v0l-k8s/payment-gateway-prod/` (branch `payment-gateway-prod`). Scripts live in `scripts/prod/` and every one of them is a dry run unless called with `--apply`.

## What runs

| Part | Choice |
| --- | --- |
| Image | `registry.v0l.io/payment-gateway:c99ddb3@sha256:ef132755...ce7cad9e`, the digest the sandbox runs today (master at c99ddb3). Change the tag and digest in `payment-gateway-prod.yml` if #799 merges first |
| Database | own MariaDB 12.3 Deployment `mysql`, PVC `mysql-data` 10Gi on StorageClass `cephfs` (covered by `cephfs-backup` too), user `gateway` |
| Ingress | `pay.apexstrata.ie`, ingress-nginx, `letsencrypt-prod`. A second Ingress answers `/admin`, `/metrics` and `/status` for 127.0.0.1 only, so the public host cannot reach them. Operators use `kubectl port-forward` |
| Lightning | YALR's own provider on the cluster LND `lnd.bitcoin:10009`, not custody, not a shared provider |
| On-chain | off. The gateway's on-chain path needs an LND wallet and the cluster LND is also LNVPS's wallet, so no on-chain provider is added. Add one later with the same macaroon method |
| Cards | Revolut live with YALR's merchant key (Secret `yalr/yalr-revolut`, key `api-key`, identical to LNVPS's `revolut.token`) |
| Platform billing (#757) | all plan prices stay zero, no `fee_rate`, `fee_bps` or `fee_base` anywhere. Providers are created with `fee_rate: 0.0` |
| Mock provider, regtest sandbox | not set. `GATEWAY_ALLOW_MOCK` and `SANDBOX_*` are absent from the manifest |

## Secrets

Generated locally by `scripts/prod/create-secrets.sh --apply` into `~/.config/payment-gateway-prod/` (mode 700, files 600, never overwritten once present), then created in the cluster. Nothing secret is in git. `deploy/prod-secrets.example` and `v0l-k8s/payment-gateway-prod/secrets.example` show the shape.

| Where | What |
| --- | --- |
| `~/.config/payment-gateway-prod/encryption-key` | `GATEWAY_ENCRYPTION_KEY`, 64 hex |
| `~/.config/payment-gateway-prod/admin-token` | `GATEWAY_ADMIN_TOKEN` |
| `~/.config/payment-gateway-prod/{metrics-token,mysql-root-password,mysql-gateway-password,restic-password}` | the rest |
| `~/.config/payment-gateway-prod/tenant-yalr.json` | `api_key` and `webhook_secret` for YALR, shown once by the gateway |
| `~/.config/payment-gateway-prod/lnd-gateway-macaroon.hex` | the baked macaroon |
| k8s `payment-gateway-prod/gateway` | `database-url admin-token encryption-key metrics-token smtp-url` |
| k8s `payment-gateway-prod/mysql` | `root-password gateway-password` |
| k8s `payment-gateway-prod/payment-gateway-backup` | `db-user db-password restic-repository restic-password aws-access-key-id aws-secret-access-key` |
| k8s `payment-gateway-prod/lnd` | `tls.cert gateway.macaroon` |
| k8s `payment-gateway-prod/ghcr` | copy of `payment-gateway/ghcr` for the registry pull |

### Escrow

Without the encryption key the sealed provider configs and webhook secrets in every backup are unreadable (`docs/runbooks/restore.md`, Lost encryption key). Before go-live the user stores two copies off the cluster:

1. The password manager vault: `encryption-key` and `restic-password`, as two separate entries.
2. A sealed paper copy of `encryption-key` held by a second person.

The restic password is the master for the backups in B2 bucket `k8s-axst`, path `payment-gateway-prod`. It must not live only in the cluster. The B2 credentials are the existing infra ones from `~/.config/backup/b2.env`.

## Apply order

Every step below changes the cluster. Run from this repo with `kubectl` context `axst-k8s`. Only `payment-gateway-prod` is touched, plus read-only `get secret` on `yalr`, `payment-gateway` and `bitcoin`.

1. DNS (Cloudflare, zone apexstrata.ie): `pay` CNAME `backend3.v0l.io`, same as `pay-sandbox`. The cert cannot issue before this resolves.
2. Namespace and workloads:

```sh
cd ~/git/v0l-k8s && git checkout payment-gateway-prod
kubectl --context axst-k8s apply -f payment-gateway-prod/00-namespace.yml
scripts/prod/create-secrets.sh            # dry run
scripts/prod/create-secrets.sh --apply
```

3. Escrow the encryption key (above) before anything is stored with it.
4. Backup image (the CronJob and drill use it):

```sh
scripts/prod/build-backup-image.sh --apply
```

5. LND macaroon and Secret (needs `ssh -i ~/.ssh/agents debian@10.97.0.40`, see below):

```sh
scripts/prod/bake-lnd-macaroon.sh --apply
```

6. Workloads, ingress and backup:

```sh
kubectl --context axst-k8s apply -f ~/git/v0l-k8s/payment-gateway-prod/payment-gateway-prod.yml
kubectl --context axst-k8s apply -f ~/git/v0l-k8s/payment-gateway-prod/backup.yml
kubectl --context axst-k8s -n payment-gateway-prod rollout status deploy/mysql deploy/payment-gateway
kubectl --context axst-k8s -n payment-gateway-prod get certificate
curl -fsS https://pay.apexstrata.ie/ready
```

7. Tenant and providers (the Revolut step registers a live webhook at Revolut, so it needs the user's approval):

```sh
scripts/prod/bootstrap.sh tenant --apply
scripts/prod/bootstrap.sh revolut --apply
scripts/prod/bootstrap.sh operator you@example.com --apply
```

8. Go-live checks (spends 10 sats of real money, creates and cancels a live Revolut order):

```sh
WEBHOOK_TEST_URL=https://<request catcher you can read> scripts/prod/golive.sh --apply
```

9. Give YALR #854 the `api_key` and `webhook_secret` from `tenant-yalr.json` and set its webhook URL with `scripts/prod/bootstrap.sh webhook https://<yalr>/... --apply`.

## Lightning on the shared LND

The cluster LND serves LNVPS and snort too and is addressed as `lnd.bitcoin:10009`. The gateway gets its own macaroon, not `admin.macaroon`:

```sh
lncli --network=mainnet bakemacaroon --root_key_id 853 invoices:read invoices:write offchain:read offchain:write
```

`scripts/prod/bake-lnd-macaroon.sh` runs this on the bitcoin VM (`sudo docker exec bitcoin-lnd-1 ...`), converts the hex to a binary file and creates Secret `lnd` with the TLS certificate copied from `yalr/lnd`. Do not let LND regenerate that certificate: its SANs are only `localhost` and `lnd`, and every Secret copy goes stale if it changes (`bitcoin-archive-vms.md`). The gateway dials `https://lnd.bitcoin:10009` and trusts the pinned certificate, so the SAN is not checked.

- `invoices:*` covers AddInvoice, LookupInvoice, SubscribeInvoices and CancelInvoice. `offchain:*` covers SendPaymentV2 and TrackPaymentV2 for refunds and payouts. The macaroon cannot send on-chain and cannot read the wallet.
- Revoke it any time with `lncli deletemacaroonid 853`. That also kills any other macaroon baked with root key 853 and nothing else.
- Invoices must not collide with LNVPS: the gateway invoice memo is the payment `description`, or `Payment <reference>` when omitted. YALR must send a description starting `YALR` and the go-live check uses `apexstrata-pay golive check`. Payment hashes are unique per invoice, so there is no technical collision: the gateway only reacts to hashes it created itself, and settlements of LNVPS invoices on the same stream are ignored.
- On first start the invoice watcher subscribes from settle index 0 and replays LND's settled history once. Those hashes are unknown to the gateway and are dropped, so expect a burst of debug lines and a short delay before the first `gateway_watcher_up` goes to 1.
- Refunds and payouts spend from the shared node. YALR payouts stay off until #854 decides on them. Leave `GATEWAY_PAYOUT_LND_*` unset, so withdrawals stay queued.

## Revolut live webhook

Nothing is created by hand. When the Revolut provider exists the worker registers `https://pay.apexstrata.ie/hook/<tenant>/<provider>` at Revolut within a tick (`src/providers/revolut.rs`, `setup`), stores the signing secret sealed in the database and never writes it to a log. Only an exact URL match is replaced, so the existing YALR and LNVPS webhooks are never listed for deletion or changed. Check the result:

```sh
kubectl --context axst-k8s -n payment-gateway-prod logs deploy/payment-gateway | grep -i webhook
```

`golive.sh` uses its own tenant `golive-check`, so its webhook URL (`/hook/<check tenant>/...`) is different from YALR's. It deletes only that URL at Revolut at the end.

## Operator access

`/admin`, `/metrics` and `/status` are not reachable on the public host. Use:

```sh
kubectl --context axst-k8s -n payment-gateway-prod port-forward svc/payment-gateway 18081:8080
curl -H "Authorization: Bearer $(cat ~/.config/payment-gateway-prod/admin-token)" http://127.0.0.1:18081/status
```

The production manifest does not set `GATEWAY_OPERATOR_2FA_GRACE`, so the first operator must enrol 2FA before the console lets them in.

## Backups

CronJob `payment-gateway-backup` runs at 02:17 UTC with `scripts/backup.sh`: `mariadb-dump --single-transaction` to a temp file, then `restic backup` into `s3:.../k8s-axst/payment-gateway-prod`, then `restic forget --prune` keeping 14 daily, 8 weekly, 12 monthly. A failed dump never reaches restic (`tests/backup_script.rs`). The repository is created on the first run.

Restore drill (never over the live database): Job `restore-drill-job.yml` restores the newest snapshot into `gateway_drill`; `golive.sh` runs it, compares table counts, runs `payment-gateway check-integrity` against the restored copy and drops it. By hand:

```sh
kubectl --context axst-k8s -n payment-gateway-prod create job pgw-backup-now --from=cronjob/payment-gateway-backup
kubectl --context axst-k8s -n payment-gateway-prod apply -f ~/git/v0l-k8s/payment-gateway-prod/restore-drill-job.yml
kubectl --context axst-k8s -n payment-gateway-prod logs job/payment-gateway-restore-drill
```

Disaster recovery is `docs/runbooks/restore.md`. Repeat the drill monthly and after any key rotation.

## Go-live checks

`scripts/prod/golive.sh --apply` stops at the first failure:

1. `/health` and `/ready` answer 200 on the public host and `/admin/tenants` is closed to the world.
2. Tenant `golive-check` with the test webhook and the Lightning provider.
3. A 10 sat (10000 msat) mainnet invoice, paid from the NWC wallet in `~/.agents/memory/wallet.json` through `npx @getalby/cli pay-invoice`, must reach `paid`. Set `PAY_CMD` to pay differently, for example `PAY_CMD="npx -y @getalby/cli ..."` with another wallet.
4. A `payment.paid` delivery to `WEBHOOK_TEST_URL` is marked delivered.
5. A Revolut live order (1.00 EUR) is created, its `checkout_url` is checked, the payment is cancelled and the test webhook is deleted at Revolut.
6. A backup Job runs, the restore drill restores it into `gateway_drill`, table and payment counts and `check-integrity` are checked, and the scratch database is dropped.
7. The `golive-check` tenant is disabled.

## Rollback

Everything is in one namespace. `kubectl delete ns payment-gateway-prod` removes it. Remove the Revolut webhook for YALR's tenant at Revolut first if it was created, and revoke the macaroon with `lncli deletemacaroonid 853`. The backups in B2 stay.
