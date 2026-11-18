ALTER TABLE webhook_delivery
    ADD COLUMN last_status INT NULL,
    ADD COLUMN last_response VARCHAR(512) NULL,
    ADD COLUMN last_duration_ms INT NULL,
    ADD COLUMN last_attempt_at TIMESTAMP NULL DEFAULT NULL;

CREATE TABLE payment_note (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    payment_id BINARY(32) NOT NULL,
    account_id BIGINT UNSIGNED NOT NULL,
    body VARCHAR(2000) NOT NULL,
    created TIMESTAMP NOT NULL,
    CONSTRAINT fk_payment_note_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    KEY ix_payment_note_payment (tenant_id, payment_id, id)
);

CREATE TABLE saved_view (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    account_id BIGINT UNSIGNED NOT NULL,
    name VARCHAR(64) NOT NULL,
    filter TEXT NOT NULL,
    created TIMESTAMP NOT NULL,
    CONSTRAINT fk_saved_view_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    UNIQUE KEY ux_saved_view_name (tenant_id, account_id, name)
);

CREATE TABLE project_config (
    tenant_id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
    allow_manual_paid BIT(1) NOT NULL DEFAULT 0,
    legal_name VARCHAR(128) NULL,
    legal_address VARCHAR(512) NULL,
    legal_vat_id VARCHAR(64) NULL,
    legal_email VARCHAR(255) NULL,
    notify_webhook_abandoned BIT(1) NOT NULL DEFAULT 1,
    notify_provider_failing BIT(1) NOT NULL DEFAULT 1,
    notify_refund_failed BIT(1) NOT NULL DEFAULT 1,
    delete_token_hash CHAR(64) NULL,
    delete_requested_at TIMESTAMP NULL DEFAULT NULL,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    CONSTRAINT fk_project_config_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id)
);
