# Backups and key rotation

## Backups

`scripts/backup.sh` dumps the database with `mariadb-dump --single-transaction` to a restic repository or a gzip file. A CronJob proposal is in `deploy/backup-cronjob.yaml`. The dump holds sealed values as ciphertext, so it is useless without the encryption key. Never store the key with the backup.

```sh
DB_NAME=gateway DB_HOST=db DB_USER=backup DB_PASSWORD=... BACKUP_DIR=/backups BACKUP_KEEP=28 scripts/backup.sh
```

Run a restore drill monthly and after every key rotation, into a scratch database, with `scripts/restore.sh`. With `GATEWAY_BIN` set it runs `payment-gateway check-integrity` on the result. The full procedure, including disaster recovery, is in `docs/runbooks/restore.md` in the repository.

## Key rotation

Set the keys newest first:

```sh
GATEWAY_ENCRYPTION_KEYS=k2:<64 hex>,k1:<64 hex>
```

1. Take a backup and confirm it restores.
2. Generate the new key with `openssl rand -hex 32` and escrow it before use.
3. Put it first in `GATEWAY_ENCRYPTION_KEYS`, keep older keys after it, and redeploy.
4. Run `payment-gateway rotate-keys` with the same environment. It re-seals values in batches and is safe to stop and run again.
5. When every line reports only the new key id, run `payment-gateway check-integrity`.
6. Keep the old key until the last backup that needs it has aged out.

After a suspected leak, rotate at once and also regenerate provider credentials and webhook secrets, since an old backup still opens with the leaked key. See `docs/runbooks/key-rotation.md`.

## Retention

`payment-gateway purge` removes old data according to the `GATEWAY_RETAIN_*` variables, and `privacy-export` and `privacy-delete` serve personal data requests. See `docs/runbooks/retention-and-privacy.md`.
