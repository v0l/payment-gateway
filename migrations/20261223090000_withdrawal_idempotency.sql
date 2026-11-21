ALTER TABLE withdrawal ADD COLUMN idempotency_key VARCHAR(128) NULL AFTER tenant_id;
ALTER TABLE withdrawal ADD COLUMN request_hash CHAR(64) NULL AFTER idempotency_key;
ALTER TABLE withdrawal ADD COLUMN provider_config_id BIGINT UNSIGNED NULL AFTER request_hash;
ALTER TABLE withdrawal ADD UNIQUE KEY ux_withdrawal_idempotency (tenant_id, idempotency_key);
