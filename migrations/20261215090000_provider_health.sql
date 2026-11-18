ALTER TABLE payment ADD COLUMN dispute_status VARCHAR(16) NULL;

ALTER TABLE provider_config
    ADD COLUMN auto_disabled_at TIMESTAMP NULL DEFAULT NULL;

CREATE TABLE provider_health (
    provider_config_id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
    window_start TIMESTAMP NOT NULL,
    successes BIGINT UNSIGNED NOT NULL DEFAULT 0,
    errors BIGINT UNSIGNED NOT NULL DEFAULT 0,
    consecutive_failures INT UNSIGNED NOT NULL DEFAULT 0,
    last_success_at TIMESTAMP NULL DEFAULT NULL,
    last_error_at TIMESTAMP NULL DEFAULT NULL,
    last_error VARCHAR(400) NULL,
    CONSTRAINT fk_provider_health_config FOREIGN KEY (provider_config_id) REFERENCES provider_config (id) ON DELETE CASCADE
);

CREATE TABLE provider_event (
    provider_config_id BIGINT UNSIGNED NOT NULL,
    event_id VARCHAR(190) NOT NULL,
    created TIMESTAMP NOT NULL,
    PRIMARY KEY (provider_config_id, event_id),
    KEY ix_provider_event_created (created),
    CONSTRAINT fk_provider_event_config FOREIGN KEY (provider_config_id) REFERENCES provider_config (id) ON DELETE CASCADE
);
