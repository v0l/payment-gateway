CREATE TABLE platform_plan (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    live BIT(1) NOT NULL DEFAULT 0,
    monthly_price BIGINT UNSIGNED NOT NULL DEFAULT 0,
    currency VARCHAR(8) NOT NULL DEFAULT 'EUR',
    included_volume BIGINT UNSIGNED NOT NULL DEFAULT 0,
    fee_bps INT UNSIGNED NOT NULL DEFAULT 0,
    fee_base BIGINT UNSIGNED NOT NULL DEFAULT 0,
    limit_payments_per_day BIGINT UNSIGNED NULL,
    limit_pending_payments BIGINT UNSIGNED NULL,
    limit_amount_per_payment BIGINT UNSIGNED NULL,
    limit_volume_per_day BIGINT UNSIGNED NULL,
    limit_providers BIGINT UNSIGNED NULL,
    limit_webhook_endpoints BIGINT UNSIGNED NULL,
    limit_attempts_per_payment BIGINT UNSIGNED NULL,
    active BIT(1) NOT NULL DEFAULT 1,
    created TIMESTAMP NOT NULL,
    UNIQUE KEY ux_platform_plan_name (name)
);

INSERT INTO platform_plan (id, name, live, created) VALUES
    (1, 'Test', 0, CURRENT_TIMESTAMP),
    (2, 'Live', 1, CURRENT_TIMESTAMP);

CREATE TABLE platform_setting (
    id TINYINT UNSIGNED NOT NULL PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NULL
);

INSERT INTO platform_setting (id, tenant_id) VALUES (1, NULL);

CREATE TABLE platform_account (
    tenant_id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
    plan_id BIGINT UNSIGNED NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'current',
    status_since TIMESTAMP NULL DEFAULT NULL,
    period_start TIMESTAMP NOT NULL,
    period_end TIMESTAMP NOT NULL,
    period_fee BIGINT UNSIGNED NOT NULL DEFAULT 0,
    credit BIGINT UNSIGNED NOT NULL DEFAULT 0,
    created TIMESTAMP NOT NULL,
    CONSTRAINT fk_platform_account_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    CONSTRAINT fk_platform_account_plan FOREIGN KEY (plan_id) REFERENCES platform_plan (id),
    KEY ix_platform_account_period (period_end)
);

CREATE TABLE platform_invoice (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    uniq_key VARCHAR(64) NOT NULL,
    kind VARCHAR(16) NOT NULL,
    plan_id BIGINT UNSIGNED NOT NULL,
    period_start TIMESTAMP NOT NULL,
    period_end TIMESTAMP NOT NULL,
    currency VARCHAR(8) NOT NULL,
    usage_count BIGINT UNSIGNED NOT NULL DEFAULT 0,
    usage_volume BIGINT UNSIGNED NOT NULL DEFAULT 0,
    line_items TEXT NOT NULL,
    subtotal BIGINT UNSIGNED NOT NULL,
    credit_applied BIGINT UNSIGNED NOT NULL DEFAULT 0,
    total BIGINT UNSIGNED NOT NULL,
    status VARCHAR(8) NOT NULL,
    reason VARCHAR(255) NULL,
    payment_id BINARY(32) NULL,
    pay_attempts INT UNSIGNED NOT NULL DEFAULT 0,
    past_due_notified BIT(1) NOT NULL DEFAULT 0,
    due_at TIMESTAMP NOT NULL,
    paid_at TIMESTAMP NULL DEFAULT NULL,
    created TIMESTAMP NOT NULL,
    CONSTRAINT fk_platform_invoice_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    UNIQUE KEY ux_platform_invoice_key (uniq_key),
    KEY ix_platform_invoice_tenant (tenant_id, id),
    KEY ix_platform_invoice_payment (payment_id),
    KEY ix_platform_invoice_status (status, due_at)
);
