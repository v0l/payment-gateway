CREATE TABLE payout_destination (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    kind VARCHAR(24) NOT NULL,
    label VARCHAR(128) NOT NULL,
    value VARBINARY(2048) NOT NULL,
    value_hash CHAR(64) NOT NULL,
    verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_by BIGINT UNSIGNED NULL,
    usable_from TIMESTAMP(3) NOT NULL,
    verified_at TIMESTAMP(3) NULL DEFAULT NULL,
    disabled BOOLEAN NOT NULL DEFAULT FALSE,
    created TIMESTAMP(3) NOT NULL,
    CONSTRAINT fk_payout_destination_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    UNIQUE KEY ux_payout_destination_value (tenant_id, value_hash),
    KEY ix_payout_destination_tenant (tenant_id, id)
);

CREATE TABLE payout_limit (
    tenant_id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
    daily_max BIGINT UNSIGNED NULL,
    auto_threshold BIGINT UNSIGNED NULL,
    CONSTRAINT fk_payout_limit_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id)
);

CREATE TABLE withdrawal (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    amount BIGINT UNSIGNED NOT NULL,
    currency VARCHAR(8) NOT NULL,
    purpose VARCHAR(16) NOT NULL DEFAULT 'payout',
    max_fee BIGINT UNSIGNED NOT NULL DEFAULT 0,
    fee BIGINT UNSIGNED NULL,
    destination_kind VARCHAR(24) NOT NULL,
    destination_id BIGINT UNSIGNED NULL,
    destination VARBINARY(2048) NOT NULL,
    destination_label VARCHAR(128) NOT NULL,
    invoice VARBINARY(2048) NULL,
    payment_hash CHAR(64) NULL,
    status VARCHAR(16) NOT NULL,
    external_id VARCHAR(128) NULL,
    error VARCHAR(512) NULL,
    attempts INT UNSIGNED NOT NULL DEFAULT 0,
    requested_by BIGINT UNSIGNED NULL,
    approved_by BIGINT UNSIGNED NULL,
    auto_approved BOOLEAN NOT NULL DEFAULT FALSE,
    created TIMESTAMP(3) NOT NULL,
    approved_at TIMESTAMP(3) NULL DEFAULT NULL,
    sending_since TIMESTAMP(3) NULL DEFAULT NULL,
    claimed_at TIMESTAMP(3) NULL DEFAULT NULL,
    finished_at TIMESTAMP(3) NULL DEFAULT NULL,
    CONSTRAINT fk_withdrawal_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    UNIQUE KEY ux_withdrawal_payment_hash (payment_hash),
    KEY ix_withdrawal_status (status, id),
    KEY ix_withdrawal_tenant (tenant_id, id),
    KEY ix_withdrawal_tenant_created (tenant_id, created)
);

ALTER TABLE webhook_delivery DROP FOREIGN KEY fk_webhook_delivery_payment;
ALTER TABLE webhook_delivery MODIFY payment_id BINARY(32) NULL;
ALTER TABLE webhook_delivery ADD CONSTRAINT fk_webhook_delivery_payment FOREIGN KEY (payment_id) REFERENCES payment (id);
