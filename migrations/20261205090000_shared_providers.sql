CREATE TABLE shared_provider (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    method VARCHAR(32) NOT NULL,
    provider_type VARCHAR(32) NOT NULL,
    network VARCHAR(16) NOT NULL,
    config VARBINARY(8192) NOT NULL,
    enabled BIT(1) NOT NULL DEFAULT 1,
    fee_bps INT UNSIGNED NOT NULL DEFAULT 0,
    fee_base_msat BIGINT UNSIGNED NOT NULL DEFAULT 0,
    allow_untrusted BIT(1) NOT NULL DEFAULT 0,
    allow_trusted BIT(1) NOT NULL DEFAULT 1,
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY ux_shared_provider_name (name)
);

CREATE TABLE shared_provider_override (
    tenant_id BIGINT UNSIGNED NOT NULL,
    shared_provider_id BIGINT UNSIGNED NOT NULL,
    fee_bps INT UNSIGNED NULL,
    fee_base_msat BIGINT UNSIGNED NULL,
    PRIMARY KEY (tenant_id, shared_provider_id),
    CONSTRAINT fk_shared_override_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    CONSTRAINT fk_shared_override_provider FOREIGN KEY (shared_provider_id) REFERENCES shared_provider (id)
);

ALTER TABLE provider_config
    ADD COLUMN shared_provider_id BIGINT UNSIGNED NULL,
    ADD CONSTRAINT fk_provider_config_shared FOREIGN KEY (shared_provider_id) REFERENCES shared_provider (id),
    ADD UNIQUE KEY ux_provider_config_shared (tenant_id, shared_provider_id);

ALTER TABLE payment_attempt
    ADD COLUMN shared_provider_id BIGINT UNSIGNED NULL,
    ADD COLUMN platform_fee_bps INT UNSIGNED NULL,
    ADD COLUMN platform_fee_base BIGINT UNSIGNED NULL,
    ADD COLUMN platform_fee BIGINT UNSIGNED NULL,
    ADD COLUMN platform_net BIGINT UNSIGNED NULL;

ALTER TABLE provider_cursor DROP FOREIGN KEY fk_cursor_provider;
