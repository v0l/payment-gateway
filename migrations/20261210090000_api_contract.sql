CREATE TABLE api_key (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    name VARCHAR(64) NOT NULL,
    key_hash CHAR(64) NOT NULL,
    key_prefix VARCHAR(24) NOT NULL,
    scopes VARCHAR(255) NOT NULL,
    mode VARCHAR(8) NULL,
    expires TIMESTAMP NULL DEFAULT NULL,
    last_used TIMESTAMP NULL DEFAULT NULL,
    revoked_at TIMESTAMP NULL DEFAULT NULL,
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_api_key_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    UNIQUE KEY ux_api_key_hash (key_hash),
    UNIQUE KEY ux_api_key_name (tenant_id, name)
);

INSERT INTO api_key (tenant_id, name, key_hash, key_prefix, scopes, created)
SELECT id, 'default', api_key_hash, 'pgw_', 'admin', created FROM tenant;

CREATE TABLE webhook_endpoint (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    url VARCHAR(512) NOT NULL,
    secret VARBINARY(256) NOT NULL,
    description VARCHAR(128) NULL,
    events VARCHAR(512) NULL,
    enabled BIT(1) NOT NULL DEFAULT 1,
    is_default BIT(1) NOT NULL DEFAULT 0,
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_webhook_endpoint_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    KEY ix_webhook_endpoint_tenant (tenant_id)
);

INSERT INTO webhook_endpoint (tenant_id, url, secret, enabled, is_default)
SELECT id, webhook_url, webhook_secret, 1, 1 FROM tenant WHERE webhook_url IS NOT NULL;

ALTER TABLE tenant DROP COLUMN webhook_url;

CREATE TABLE gateway_event (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    payment_id BINARY(32) NOT NULL,
    event_id CHAR(32) NOT NULL,
    event_type VARCHAR(48) NOT NULL,
    body MEDIUMTEXT NOT NULL,
    created TIMESTAMP NOT NULL,
    UNIQUE KEY ux_gateway_event_id (event_id),
    KEY ix_gateway_event_tenant (tenant_id, id)
);

INSERT INTO gateway_event (tenant_id, payment_id, event_id, event_type, body, created)
SELECT tenant_id, payment_id, event_id, event_type, body, created FROM webhook_delivery ORDER BY id;

ALTER TABLE webhook_delivery
    DROP INDEX ux_webhook_delivery_event,
    ADD COLUMN endpoint_id BIGINT UNSIGNED NULL,
    ADD UNIQUE KEY ux_webhook_delivery_event_endpoint (event_id, endpoint_id),
    ADD KEY ix_webhook_delivery_endpoint (endpoint_id);

UPDATE webhook_delivery d
JOIN webhook_endpoint e ON e.tenant_id = d.tenant_id AND e.is_default = 1
SET d.endpoint_id = e.id
WHERE d.endpoint_id IS NULL;

ALTER TABLE payment ADD KEY ix_payment_tenant_customer (tenant_id, customer_ref);
