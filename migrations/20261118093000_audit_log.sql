CREATE TABLE audit_event (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NULL,
    actor_kind VARCHAR(16) NOT NULL,
    actor_id VARCHAR(64) NULL,
    action VARCHAR(64) NOT NULL,
    target_kind VARCHAR(32) NULL,
    target_id VARCHAR(128) NULL,
    ip VARCHAR(64) NULL,
    user_agent VARCHAR(255) NULL,
    details TEXT NULL,
    created TIMESTAMP(3) NOT NULL,
    KEY ix_audit_tenant_created (tenant_id, created),
    KEY ix_audit_action_created (action, created)
);
