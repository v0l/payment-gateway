# Key rotation

Sealed values (webhook secrets, provider configs, and any column added to `SEALED_COLUMNS` in `src/ops/rotate.rs`) are stored as `pgw1:<kid>:<nonce><ciphertext>`. Values written before key ids existed have no prefix and are opened by trying each key.

## Configure

```
GATEWAY_ENCRYPTION_KEYS=k2:<64 hex>,k1:<64 hex>
```

Newest first. The service seals with the first key and opens with any listed key. `GATEWAY_ENCRYPTION_KEY=<64 hex>` still works and is treated as one key with id `default`.

## Rotate

1. Take a backup (`docs/runbooks/restore.md`) and confirm it restores.
2. Generate the new key: `openssl rand -hex 32`. Store it in the secret manager and in escrow before using it.
3. Put it first in `GATEWAY_ENCRYPTION_KEYS`, keep every older key after it, redeploy.
4. Run `payment-gateway rotate-keys` with the same environment and `DATABASE_URL`. It walks every registered sealed column in batches, re-seals values not yet under the newest key, and prints how many values each key still covers. It is safe to stop and run again: each row is updated only if it still holds the value that was read.
5. When every line reports only the new key id, run `payment-gateway check-integrity`. It fails if any sealed value cannot be opened.
6. Keep the old key until the last backup that needs it has aged out (backups are sealed with whatever key was current when written). Then drop it from the list.

## Suspected key leak

Rotate immediately (steps 2 to 4), then also regenerate the provider credentials and webhook secrets that were sealed under the leaked key, because an old backup still opens with it.

## Adding a sealed column

Add it to `SEALED_COLUMNS`. The test `every_binary_column_is_registered_as_sealed_or_known_plain` fails for an unregistered binary column.
