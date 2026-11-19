CREATE TABLE xpub_state (
    provider_config_id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
    next_index INT UNSIGNED NOT NULL DEFAULT 0,
    rescan_from INT UNSIGNED NULL,
    rescan_count INT UNSIGNED NULL,
    CONSTRAINT fk_xpub_state_provider FOREIGN KEY (provider_config_id) REFERENCES provider_config (id)
);

CREATE TABLE xpub_address (
    provider_config_id BIGINT UNSIGNED NOT NULL,
    idx INT UNSIGNED NOT NULL,
    address VARCHAR(100) NOT NULL,
    issued TIMESTAMP(3) NOT NULL,
    PRIMARY KEY (provider_config_id, idx),
    KEY ix_xpub_address_address (provider_config_id, address),
    CONSTRAINT fk_xpub_address_provider FOREIGN KEY (provider_config_id) REFERENCES provider_config (id)
);
