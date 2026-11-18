ALTER TABLE tenant
    ADD COLUMN limit_payments_per_day BIGINT UNSIGNED NULL,
    ADD COLUMN limit_pending_payments BIGINT UNSIGNED NULL,
    ADD COLUMN limit_amount_per_payment BIGINT UNSIGNED NULL,
    ADD COLUMN limit_volume_per_day BIGINT UNSIGNED NULL,
    ADD COLUMN limit_providers BIGINT UNSIGNED NULL,
    ADD COLUMN limit_webhook_endpoints BIGINT UNSIGNED NULL,
    ADD COLUMN limit_attempts_per_payment BIGINT UNSIGNED NULL;

ALTER TABLE payment ADD KEY ix_payment_tenant_created (tenant_id, created);
