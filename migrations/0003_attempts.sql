ALTER TABLE tenant
    ADD COLUMN display_name VARCHAR(128) NULL,
    ADD COLUMN logo_url VARCHAR(512) NULL,
    ADD COLUMN accent_color VARCHAR(16) NULL,
    ADD COLUMN allowed_origins TEXT NULL,
    ADD COLUMN default_success_url VARCHAR(512) NULL,
    ADD COLUMN default_cancel_url VARCHAR(512) NULL;

CREATE TABLE payment_attempt (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    payment_id BINARY(32) NOT NULL,
    provider_config_id BIGINT UNSIGNED NOT NULL,
    watch_provider_id BIGINT UNSIGNED NOT NULL,
    method VARCHAR(32) NOT NULL,
    option_key VARCHAR(64) NOT NULL DEFAULT '',
    base_amount BIGINT UNSIGNED NOT NULL,
    processing_fee BIGINT UNSIGNED NOT NULL DEFAULT 0,
    currency VARCHAR(8) NOT NULL,
    charge_amount BIGINT UNSIGNED NOT NULL,
    charge_currency VARCHAR(8) NOT NULL,
    rate DOUBLE NOT NULL DEFAULT 1,
    lookup VARCHAR(255) NOT NULL,
    external_id VARCHAR(255) NULL,
    details TEXT NOT NULL,
    status VARCHAR(16) NOT NULL,
    created TIMESTAMP NOT NULL,
    expires TIMESTAMP NOT NULL,
    paid_at TIMESTAMP NULL DEFAULT NULL,
    save_method BIT(1) NOT NULL DEFAULT 0,
    CONSTRAINT fk_attempt_payment FOREIGN KEY (payment_id) REFERENCES payment (id),
    CONSTRAINT fk_attempt_provider FOREIGN KEY (provider_config_id) REFERENCES provider_config (id),
    KEY ix_attempt_payment (payment_id),
    KEY ix_attempt_lookup (watch_provider_id, lookup),
    KEY ix_attempt_status_expires (status, expires)
);

CREATE TABLE attempt_receipt (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    attempt_id BIGINT UNSIGNED NOT NULL,
    ref VARCHAR(255) NOT NULL,
    amount BIGINT UNSIGNED NOT NULL,
    confirmed BIT(1) NOT NULL DEFAULT 0,
    created TIMESTAMP NOT NULL,
    CONSTRAINT fk_receipt_attempt FOREIGN KEY (attempt_id) REFERENCES payment_attempt (id),
    UNIQUE KEY ux_receipt_ref (attempt_id, ref)
);

CREATE TABLE provider_cursor (
    provider_config_id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
    cursor_value VARCHAR(255) NOT NULL,
    updated TIMESTAMP NOT NULL,
    CONSTRAINT fk_cursor_provider FOREIGN KEY (provider_config_id) REFERENCES provider_config (id)
);

CREATE TABLE saved_method (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    customer_ref VARCHAR(128) NOT NULL,
    provider_config_id BIGINT UNSIGNED NOT NULL,
    external_customer_id VARCHAR(255) NOT NULL,
    external_id VARCHAR(255) NOT NULL,
    brand VARCHAR(32) NULL,
    last4 VARCHAR(8) NULL,
    exp_month SMALLINT UNSIGNED NULL,
    exp_year SMALLINT UNSIGNED NULL,
    enabled BIT(1) NOT NULL DEFAULT 1,
    created TIMESTAMP NOT NULL,
    CONSTRAINT fk_saved_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    CONSTRAINT fk_saved_provider FOREIGN KEY (provider_config_id) REFERENCES provider_config (id),
    UNIQUE KEY ux_saved_external (provider_config_id, external_id),
    KEY ix_saved_customer (tenant_id, customer_ref)
);

CREATE TABLE refund (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    payment_id BINARY(32) NOT NULL,
    attempt_id BIGINT UNSIGNED NOT NULL,
    amount BIGINT UNSIGNED NOT NULL,
    charge_amount BIGINT UNSIGNED NOT NULL,
    currency VARCHAR(8) NOT NULL,
    destination VARCHAR(2048) NULL,
    status VARCHAR(24) NOT NULL,
    external_id VARCHAR(255) NULL,
    error VARCHAR(512) NULL,
    created TIMESTAMP NOT NULL,
    CONSTRAINT fk_refund_payment FOREIGN KEY (payment_id) REFERENCES payment (id),
    CONSTRAINT fk_refund_attempt FOREIGN KEY (attempt_id) REFERENCES payment_attempt (id),
    KEY ix_refund_payment (payment_id)
);
