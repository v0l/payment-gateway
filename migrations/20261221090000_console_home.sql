ALTER TABLE payment ADD KEY ix_payment_tenant_paid (tenant_id, paid_at);
ALTER TABLE payment ADD KEY ix_payment_tenant_status (tenant_id, status);
ALTER TABLE payment ADD KEY ix_payment_tenant_dispute (tenant_id, dispute_status);
ALTER TABLE webhook_delivery ADD KEY ix_webhook_delivery_tenant_created (tenant_id, created);
ALTER TABLE refund ADD KEY ix_refund_status (status, created);
ALTER TABLE gateway_event ADD KEY ix_gateway_event_payment (payment_id, id);
