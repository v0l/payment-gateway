# Retention and personal data

## Retention

The server purges on start and every 6 hours when any of these is set (whole days):

```
GATEWAY_RETAIN_DELIVERY_BODY_DAYS=90      # empties the body of delivered or abandoned webhooks
GATEWAY_RETAIN_UNPAID_PAYMENT_DAYS=180    # deletes expired or cancelled payments that never took money
GATEWAY_RETAIN_SESSION_DAYS=7             # deletes console sessions this long past expiry
```

Run once by hand with `payment-gateway purge` and the same variables. Paid, refunded and partially refunded payments, their attempts, refunds and receipts, and open payments are never touched. Payments with any paid attempt, confirmed receipt or refund are skipped even if their status is expired. Undelivered webhooks keep their body. The audit log and ledger are never purged.

## Customer data requests

```
payment-gateway privacy-export <project id> --email person@example.com
payment-gateway privacy-export <project id> --customer-ref cust_42
payment-gateway privacy-delete <project id> --email person@example.com
```

Export prints the matching payments (with `customer_ref`, `customer_email`, `metadata`) and saved methods as JSON. Delete sets `customer_email`, `customer_ref` and `metadata` to null on that project's matching payments, empties their finished webhook bodies, and (for `--customer-ref`) deletes saved methods. Amounts, status, attempts and refunds stay, so accounting is unchanged. Other projects are never touched. Backups taken earlier still hold the data until they age out.

## Audit

Every operator command writes an audit event as actor `system`: `keys.rotated`, `retention.purged` (with counts), `privacy.exported` and `privacy.deleted` (project and subject kind, never the subject value). Audit rows are never purged. `privacy-delete` also replaces the customer's email and reference with `[removed]` inside the `details` of that project's audit events.
