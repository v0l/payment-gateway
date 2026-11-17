CREATE TABLE tenant (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    api_key_hash CHAR(64) NOT NULL,
    webhook_url VARCHAR(512) NULL,
    webhook_secret VARBINARY(256) NOT NULL,
    enabled BIT(1) NOT NULL DEFAULT 1,
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY ux_tenant_name (name),
    UNIQUE KEY ux_tenant_api_key_hash (api_key_hash)
);

CREATE TABLE provider_config (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    method VARCHAR(32) NOT NULL,
    provider_type VARCHAR(32) NOT NULL,
    name VARCHAR(128) NOT NULL,
    enabled BIT(1) NOT NULL DEFAULT 1,
    config VARBINARY(8192) NOT NULL,
    fee_rate DOUBLE NULL,
    fee_base BIGINT UNSIGNED NULL,
    fee_currency VARCHAR(8) NULL,
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_provider_config_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    UNIQUE KEY ux_provider_config_name (tenant_id, name)
);
