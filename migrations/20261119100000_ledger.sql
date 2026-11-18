CREATE TABLE ledger_account (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NULL,
    scope BIGINT UNSIGNED AS (IFNULL(tenant_id, 0)) STORED,
    kind VARCHAR(32) NOT NULL,
    currency VARCHAR(8) NOT NULL,
    balance BIGINT NOT NULL DEFAULT 0,
    allow_negative BOOLEAN NOT NULL DEFAULT FALSE,
    created TIMESTAMP(3) NOT NULL,
    UNIQUE KEY ux_ledger_account (scope, kind, currency),
    KEY ix_ledger_account_tenant (tenant_id)
);

CREATE TABLE ledger_transaction (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    kind VARCHAR(32) NOT NULL,
    reference_kind VARCHAR(32) NULL,
    reference_id VARCHAR(128) NULL,
    idempotency_key VARCHAR(191) NOT NULL,
    created TIMESTAMP(3) NOT NULL,
    UNIQUE KEY ux_ledger_tx_key (idempotency_key),
    KEY ix_ledger_tx_reference (reference_kind, reference_id)
);

CREATE TABLE ledger_entry (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    transaction_id BIGINT UNSIGNED NOT NULL,
    account_id BIGINT UNSIGNED NOT NULL,
    amount BIGINT NOT NULL,
    currency VARCHAR(8) NOT NULL,
    created TIMESTAMP(3) NOT NULL,
    KEY ix_ledger_entry_account (account_id, id),
    KEY ix_ledger_entry_tx (transaction_id),
    CONSTRAINT fk_ledger_entry_tx FOREIGN KEY (transaction_id) REFERENCES ledger_transaction (id),
    CONSTRAINT fk_ledger_entry_account FOREIGN KEY (account_id) REFERENCES ledger_account (id)
);
