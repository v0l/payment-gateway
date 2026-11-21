# Backup and restore

## Back up

`scripts/backup.sh` dumps the gateway database with `mariadb-dump --single-transaction` (a consistent snapshot, no table locks) either to a restic repository or to a gzip file.

```
DB_NAME=gateway DB_HOST=db DB_USER=backup DB_PASSWORD=... BACKUP_DIR=/backups BACKUP_KEEP=28 scripts/backup.sh
DB_NAME=gateway ... RESTIC_REPOSITORY=s3:... RESTIC_PASSWORD=... scripts/backup.sh
```

A CronJob proposal is in `deploy/backup-cronjob.yaml` with its image in `deploy/Dockerfile.backup`. Nothing is applied by this repo.

The dump contains sealed values as ciphertext. It is useless without the encryption key that sealed them, and the key must not be stored with the backup.

## Restore drill

Run monthly and after every rotation. Never restore over the live database.

```
RESTORE_OVERWRITE=1 DB_HOST=... DB_USER=... DB_PASSWORD=... \
GATEWAY_BIN=payment-gateway GATEWAY_ENCRYPTION_KEYS=... \
scripts/restore.sh /backups/gateway-<stamp>.sql.gz gateway_drill
```

For restic use `restic:latest` and set `DB_SOURCE_NAME=gateway`. With `GATEWAY_BIN` set, the script runs `payment-gateway check-integrity` on the result. With `RESTORE_REQUIRE_INTEGRITY=1` (set in the prod drill Job) it refuses to run without `GATEWAY_BIN` and an encryption key, prints table and migration counts, and exits non-zero on any finding or unreadable sealed value. It checks that paid payments have a paid attempt, refunds do not exceed the paid amount, nothing is orphaned, and every sealed value opens with the configured keys. Drop the scratch database afterwards. The automated version of this drill is `restore_drill_*` in `tests/ops.rs`.

## Disaster recovery

1. Stop the gateway. Do not run two replicas against one database.
2. Provision a database and restore the newest good backup into it: `scripts/restore.sh <dump> gateway`.
3. Set `GATEWAY_ENCRYPTION_KEYS` from escrow. The list must include the key id the restored values carry (`payment-gateway check-integrity` reports unreadable values).
4. Run `payment-gateway check-integrity`, then start the gateway.
5. Payments paid between the backup and the failure are missing. On start the provider watchers resume from their cursors and reconcile pending attempts. Reconcile paid events against the provider dashboards (Stripe, Revolut, LND invoices) and replay webhooks with `POST /admin/deliveries/{id}/replay` or by creating them from the provider side.
6. Take a fresh backup.

## Lost encryption key

Provider configs and webhook secrets are unrecoverable. Payments, attempts, refunds and receipts stay readable. Recreate each project's provider credentials and webhook secret through the console or admin API.
