ALTER TABLE account ADD COLUMN notify BIT(1) NOT NULL DEFAULT 1;

CREATE TABLE account_token (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    account_id BIGINT UNSIGNED NOT NULL,
    kind VARCHAR(16) NOT NULL,
    token_hash CHAR(64) NOT NULL,
    new_email VARCHAR(255) NULL,
    created TIMESTAMP NOT NULL,
    expires TIMESTAMP NOT NULL,
    used_at TIMESTAMP NULL DEFAULT NULL,
    CONSTRAINT fk_account_token_account FOREIGN KEY (account_id) REFERENCES account (id),
    UNIQUE KEY ux_account_token_hash (token_hash),
    KEY ix_account_token_account (account_id, kind)
);

CREATE TABLE notification_throttle (
    tenant_id BIGINT UNSIGNED NOT NULL,
    kind VARCHAR(32) NOT NULL,
    sent_at TIMESTAMP NOT NULL,
    PRIMARY KEY (tenant_id, kind)
);
