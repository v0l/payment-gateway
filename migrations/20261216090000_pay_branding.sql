CREATE TABLE tenant_branding (
    tenant_id BIGINT UNSIGNED NOT NULL PRIMARY KEY,
    default_lang VARCHAR(8) NULL,
    theme VARCHAR(8) NOT NULL DEFAULT 'auto',
    accent_dark VARCHAR(16) NULL,
    logo_dark_url VARCHAR(512) NULL,
    radius VARCHAR(8) NOT NULL DEFAULT 'md',
    font VARCHAR(16) NOT NULL DEFAULT 'system',
    thanks_text VARCHAR(500) NULL,
    footer_links TEXT NULL,
    show_powered_by BIT(1) NOT NULL DEFAULT 1,
    updated TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    CONSTRAINT fk_branding_tenant FOREIGN KEY (tenant_id) REFERENCES tenant (id)
);
