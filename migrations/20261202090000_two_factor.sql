ALTER TABLE account ADD COLUMN totp_secret VARBINARY(255) NULL;
ALTER TABLE account ADD COLUMN totp_enabled BIT(1) NOT NULL DEFAULT 0;
ALTER TABLE account ADD COLUMN totp_last_step BIGINT UNSIGNED NULL;

ALTER TABLE tenant ADD COLUMN require_2fa BIT(1) NOT NULL DEFAULT 0;

CREATE TABLE account_recovery_code (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    account_id BIGINT UNSIGNED NOT NULL,
    code_hash CHAR(64) NOT NULL,
    created TIMESTAMP NOT NULL,
    used_at TIMESTAMP NULL DEFAULT NULL,
    CONSTRAINT fk_recovery_account FOREIGN KEY (account_id) REFERENCES account (id),
    UNIQUE KEY ux_recovery_hash (code_hash),
    KEY ix_recovery_account (account_id)
);

CREATE TABLE login_challenge (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    account_id BIGINT UNSIGNED NOT NULL,
    token_hash CHAR(64) NOT NULL,
    created TIMESTAMP NOT NULL,
    expires TIMESTAMP NOT NULL,
    CONSTRAINT fk_challenge_account FOREIGN KEY (account_id) REFERENCES account (id),
    UNIQUE KEY ux_challenge_token (token_hash),
    KEY ix_challenge_account (account_id)
);
