use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub const LANGUAGES: [&str; 7] = ["en", "es", "fr", "de", "pt", "it", "ga"];
pub const THEMES: [&str; 3] = ["auto", "light", "dark"];
pub const RADII: [&str; 4] = ["none", "sm", "md", "lg"];
pub const FONTS: [&str; 4] = ["system", "serif", "mono", "rounded"];
const MAX_LINKS: usize = 3;
const MAX_THANKS: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FooterLink {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Branding {
    #[serde(default)]
    pub default_lang: Option<String>,
    #[serde(default = "auto")]
    pub theme: String,
    #[serde(default)]
    pub accent_dark: Option<String>,
    #[serde(default)]
    pub logo_dark_url: Option<String>,
    #[serde(default = "md")]
    pub radius: String,
    #[serde(default = "system")]
    pub font: String,
    #[serde(default)]
    pub thanks_text: Option<String>,
    #[serde(default)]
    pub footer_links: Vec<FooterLink>,
    #[serde(default = "yes")]
    pub show_powered_by: bool,
}

fn auto() -> String {
    "auto".into()
}
fn md() -> String {
    "md".into()
}
fn system() -> String {
    "system".into()
}
fn yes() -> bool {
    true
}

impl Default for Branding {
    fn default() -> Self {
        Self {
            default_lang: None,
            theme: auto(),
            accent_dark: None,
            logo_dark_url: None,
            radius: md(),
            font: system(),
            thanks_text: None,
            footer_links: vec![],
            show_powered_by: true,
        }
    }
}

fn https(u: &str) -> bool {
    u.len() <= 512
        && url::Url::parse(u)
            .map(|p| p.scheme() == "https" && p.host().is_some())
            .unwrap_or(false)
}

fn blank_to_none(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

impl Branding {
    pub fn validated(mut self) -> Result<Self> {
        self.default_lang = blank_to_none(self.default_lang).map(|l| l.to_lowercase());
        if let Some(l) = &self.default_lang
            && !LANGUAGES.contains(&l.as_str())
        {
            bail!("default_lang must be one of {}", LANGUAGES.join(", "));
        }
        if !THEMES.contains(&self.theme.as_str()) {
            bail!("theme must be one of {}", THEMES.join(", "));
        }
        if !RADII.contains(&self.radius.as_str()) {
            bail!("radius must be one of {}", RADII.join(", "));
        }
        if !FONTS.contains(&self.font.as_str()) {
            bail!("font must be one of {}", FONTS.join(", "));
        }
        self.accent_dark = blank_to_none(self.accent_dark);
        if self
            .accent_dark
            .as_deref()
            .is_some_and(|c| !crate::validate::valid_color(c))
        {
            bail!("accent_dark must be #rgb or #rrggbb");
        }
        self.logo_dark_url = blank_to_none(self.logo_dark_url);
        if self.logo_dark_url.as_deref().is_some_and(|u| !https(u)) {
            bail!("logo_dark_url must be an https url");
        }
        self.thanks_text = blank_to_none(self.thanks_text);
        if self
            .thanks_text
            .as_deref()
            .is_some_and(|t| t.chars().count() > MAX_THANKS)
        {
            bail!("thanks_text is longer than {MAX_THANKS} characters");
        }
        if self.footer_links.len() > MAX_LINKS {
            bail!("at most {MAX_LINKS} footer links");
        }
        for l in &mut self.footer_links {
            l.label = l.label.trim().to_string();
            if l.label.is_empty() || l.label.chars().count() > 40 {
                bail!("footer link labels are 1-40 characters");
            }
            if !https(&l.url) {
                bail!("footer link {} must be an https url", l.label);
            }
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(label: &str, url: &str) -> FooterLink {
        FooterLink {
            label: label.into(),
            url: url.into(),
        }
    }

    #[test]
    fn defaults_are_valid() {
        assert!(Branding::default().validated().is_ok());
    }

    #[test]
    fn only_safe_tokens_are_accepted() {
        let bad = [
            Branding {
                default_lang: Some("xx".into()),
                ..Default::default()
            },
            Branding {
                theme: "neon".into(),
                ..Default::default()
            },
            Branding {
                radius: "9px; background:url(x)".into(),
                ..Default::default()
            },
            Branding {
                font: "Comic Sans".into(),
                ..Default::default()
            },
            Branding {
                accent_dark: Some("red".into()),
                ..Default::default()
            },
            Branding {
                logo_dark_url: Some("http://x.example/l.png".into()),
                ..Default::default()
            },
            Branding {
                footer_links: vec![link("Terms", "javascript:alert(1)")],
                ..Default::default()
            },
            Branding {
                footer_links: vec![link("a", "https://a.example"); 4],
                ..Default::default()
            },
            Branding {
                thanks_text: Some("x".repeat(501)),
                ..Default::default()
            },
        ];
        for b in bad {
            assert!(b.clone().validated().is_err(), "{b:?}");
        }
    }

    #[test]
    fn good_input_is_normalised() {
        let b = Branding {
            default_lang: Some(" GA ".into()),
            thanks_text: Some("   ".into()),
            footer_links: vec![link(" Terms ", "https://a.example/t")],
            ..Default::default()
        }
        .validated()
        .unwrap();
        assert_eq!(b.default_lang.as_deref(), Some("ga"));
        assert_eq!(b.thanks_text, None);
        assert_eq!(b.footer_links[0].label, "Terms");
    }
}
