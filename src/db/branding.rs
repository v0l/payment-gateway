use super::*;
use crate::branding::{Branding, FooterLink};

#[derive(FromRow)]
struct BrandingRow {
    default_lang: Option<String>,
    theme: String,
    accent_dark: Option<String>,
    logo_dark_url: Option<String>,
    radius: String,
    font: String,
    thanks_text: Option<String>,
    footer_links: Option<String>,
    show_powered_by: bool,
}

impl From<BrandingRow> for Branding {
    fn from(r: BrandingRow) -> Self {
        let footer_links: Vec<FooterLink> = r
            .footer_links
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();
        Self {
            default_lang: r.default_lang,
            theme: r.theme,
            accent_dark: r.accent_dark,
            logo_dark_url: r.logo_dark_url,
            radius: r.radius,
            font: r.font,
            thanks_text: r.thanks_text,
            footer_links,
            show_powered_by: r.show_powered_by,
        }
    }
}

pub async fn get_branding(pool: &MySqlPool, tenant_id: u64) -> Result<Branding> {
    let row: Option<BrandingRow> = sqlx::query_as(
        "select default_lang, theme, accent_dark, logo_dark_url, radius, font, thanks_text, footer_links, show_powered_by from tenant_branding where tenant_id = ?",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into).unwrap_or_default())
}

pub async fn set_branding(pool: &MySqlPool, tenant_id: u64, b: &Branding) -> Result<()> {
    sqlx::query(
        "insert into tenant_branding (tenant_id, default_lang, theme, accent_dark, logo_dark_url, radius, font, thanks_text, footer_links, show_powered_by) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) on duplicate key update default_lang = values(default_lang), theme = values(theme), accent_dark = values(accent_dark), logo_dark_url = values(logo_dark_url), radius = values(radius), font = values(font), thanks_text = values(thanks_text), footer_links = values(footer_links), show_powered_by = values(show_powered_by)",
    )
    .bind(tenant_id)
    .bind(&b.default_lang)
    .bind(&b.theme)
    .bind(&b.accent_dark)
    .bind(&b.logo_dark_url)
    .bind(&b.radius)
    .bind(&b.font)
    .bind(&b.thanks_text)
    .bind(serde_json::to_string(&b.footer_links)?)
    .bind(b.show_powered_by)
    .execute(pool)
    .await?;
    Ok(())
}
