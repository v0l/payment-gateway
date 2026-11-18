ALTER TABLE tenant ADD COLUMN trusted BIT(1) NOT NULL DEFAULT 1;

CREATE TABLE account (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    email VARCHAR(255) NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    is_operator BIT(1) NOT NULL DEFAULT 0,
    disabled BIT(1) NOT NULL DEFAULT 0,
    created TIMESTAMP NOT NULL,
    last_login TIMESTAMP NULL DEFAULT NULL,
    UNIQUE KEY ux_account_email (email)
);

CREATE TABLE account_session (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    account_id BIGINT UNSIGNED NOT NULL,
    token_hash CHAR(64) NOT NULL,
    csrf CHAR(32) NOT NULL,
    created TIMESTAMP NOT NULL,
    expires TIMESTAMP NOT NULL,
    CONSTRAINT fk_session_account FOREIGN KEY (account_id) REFERENCES account (id),
    UNIQUE KEY ux_session_token (token_hash),
    KEY ix_session_account (account_id)
);

CREATE TABLE project_member (
    tenant_id BIGINT UNSIGNED NOT NULL,
    account_id BIGINT UNSIGNED NOT NULL,
    role VARCHAR(16) NOT NULL,
    created TIMESTAMP NOT NULL,
    PRIMARY KEY (tenant_id, account_id),
    CONSTRAINT fk_member_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    CONSTRAINT fk_member_account FOREIGN KEY (account_id) REFERENCES account (id)
);

CREATE TABLE invite (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    token_hash CHAR(64) NOT NULL,
    email VARCHAR(255) NOT NULL,
    tenant_id BIGINT UNSIGNED NULL,
    role VARCHAR(16) NULL,
    operator BIT(1) NOT NULL DEFAULT 0,
    created_by BIGINT UNSIGNED NULL,
    created TIMESTAMP NOT NULL,
    expires TIMESTAMP NOT NULL,
    used_at TIMESTAMP NULL DEFAULT NULL,
    UNIQUE KEY ux_invite_token (token_hash),
    KEY ix_invite_tenant (tenant_id)
);
