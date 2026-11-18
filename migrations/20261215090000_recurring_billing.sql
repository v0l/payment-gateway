CREATE TABLE billing_plan (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    name VARCHAR(128) NOT NULL,
    description VARCHAR(255) NULL,
    amount BIGINT UNSIGNED NOT NULL,
    currency VARCHAR(8) NOT NULL,
    interval_unit VARCHAR(8) NOT NULL,
    interval_count INT UNSIGNED NOT NULL DEFAULT 1,
    trial_days INT UNSIGNED NOT NULL DEFAULT 0,
    grace_days INT UNSIGNED NOT NULL DEFAULT 3,
    active BIT(1) NOT NULL DEFAULT 1,
    created TIMESTAMP NOT NULL,
    CONSTRAINT fk_billing_plan_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    KEY ix_billing_plan_tenant (tenant_id, id)
);

CREATE TABLE subscription (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    tenant_id BIGINT UNSIGNED NOT NULL,
    customer_ref VARCHAR(128) NOT NULL,
    plan_id BIGINT UNSIGNED NOT NULL,
    saved_method_id BIGINT UNSIGNED NULL,
    status VARCHAR(16) NOT NULL,
    period_index INT NOT NULL DEFAULT -1,
    anchor TIMESTAMP NOT NULL,
    anchor_index INT NOT NULL DEFAULT 0,
    current_period_start TIMESTAMP NOT NULL,
    current_period_end TIMESTAMP NOT NULL,
    trial_end TIMESTAMP NULL DEFAULT NULL,
    next_charge TIMESTAMP NULL DEFAULT NULL,
    cancel_at_period_end BIT(1) NOT NULL DEFAULT 0,
    adjustment BIGINT NOT NULL DEFAULT 0,
    setup_payment_id BINARY(32) NULL,
    claimed_until TIMESTAMP NULL DEFAULT NULL,
    paused_at TIMESTAMP NULL DEFAULT NULL,
    cancelled_at TIMESTAMP NULL DEFAULT NULL,
    cancel_reason VARCHAR(32) NULL,
    created TIMESTAMP NOT NULL,
    CONSTRAINT fk_subscription_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id),
    CONSTRAINT fk_subscription_plan FOREIGN KEY (plan_id) REFERENCES billing_plan (id),
    KEY ix_subscription_tenant (tenant_id, id),
    KEY ix_subscription_customer (tenant_id, customer_ref),
    KEY ix_subscription_due (status, next_charge),
    KEY ix_subscription_setup (setup_payment_id)
);

CREATE TABLE subscription_period (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
    subscription_id BIGINT UNSIGNED NOT NULL,
    period_index INT NOT NULL,
    period_start TIMESTAMP NOT NULL,
    period_end TIMESTAMP NOT NULL,
    amount BIGINT UNSIGNED NOT NULL,
    currency VARCHAR(8) NOT NULL,
    status VARCHAR(8) NOT NULL,
    attempts INT UNSIGNED NOT NULL DEFAULT 0,
    failures INT UNSIGNED NOT NULL DEFAULT 0,
    payment_id BINARY(32) NULL,
    created TIMESTAMP NOT NULL,
    paid_at TIMESTAMP NULL DEFAULT NULL,
    CONSTRAINT fk_period_subscription FOREIGN KEY (subscription_id) REFERENCES subscription (id),
    UNIQUE KEY ux_period_subscription_index (subscription_id, period_index),
    KEY ix_period_payment (payment_id)
);

ALTER TABLE gateway_event MODIFY payment_id BINARY(32) NULL;

ALTER TABLE webhook_delivery MODIFY payment_id BINARY(32) NULL;
