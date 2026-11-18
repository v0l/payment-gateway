use super::Message;

const FONT: &str = "system-ui,-apple-system,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif";
const MONO: &str = "ui-monospace,SFMono-Regular,Menlo,Consolas,monospace";

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub struct Mail<'a> {
    pub to: &'a str,
    pub subject: String,
    pub preheader: &'a str,
    pub heading: &'a str,
    pub paragraphs: Vec<String>,
    pub action: Option<(&'a str, &'a str)>,
    pub footnote: Option<&'a str>,
}

const STYLE: &str = "\
:root{color-scheme:light dark;supported-color-schemes:light dark}\
@media (prefers-color-scheme:dark){\
.pg-bg{background:#0c1312!important}\
.pg-card{background:#141d1c!important;border-color:#2a3836!important}\
.pg-text{color:#e4ecea!important}\
.pg-muted{color:#9bacaa!important}\
.pg-btn{background:#52c3d1!important;color:#052226!important}\
.pg-btn-cell{background:#52c3d1!important}\
.pg-link{color:#52c3d1!important}\
.pg-code{background:#1d2928!important}}\
@media (max-width:480px){.pg-pad{padding:20px 16px!important}}";

impl Mail<'_> {
    pub fn render(self) -> Message {
        let mut text = format!("{}\n\n{}", self.heading, self.paragraphs.join("\n\n"));
        if let Some((_, url)) = self.action {
            text.push_str(&format!("\n\n{url}"));
        }
        if let Some(note) = self.footnote {
            text.push_str(&format!("\n\n{note}"));
        }
        let mut body = String::new();
        for p in &self.paragraphs {
            body.push_str(&format!(
                "<p class=\"pg-text\" style=\"margin:0 0 16px;font-size:15px;line-height:1.55;color:#121c1b\">{}</p>",
                escape(p)
            ));
        }
        if let Some((label, url)) = self.action {
            body.push_str(&format!(
                "<table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\" border=\"0\" style=\"margin:8px 0 20px\"><tr>\
                 <td class=\"pg-btn-cell\" bgcolor=\"#0b6572\" style=\"border-radius:8px;background:#0b6572\">\
                 <a class=\"pg-btn\" href=\"{url}\" style=\"display:inline-block;padding:12px 22px;font-size:15px;font-weight:600;color:#ffffff;text-decoration:none;border-radius:8px;font-family:{FONT}\">{label}</a>\
                 </td></tr></table>\
                 <p class=\"pg-muted\" style=\"margin:0 0 4px;font-size:13px;line-height:1.5;color:#566361\">If the button does not work, copy this link into your browser:</p>\
                 <p class=\"pg-code\" style=\"margin:0 0 16px;padding:10px 12px;background:#e8eeec;border-radius:8px;font-family:{MONO};font-size:12px;line-height:1.5;word-break:break-all\"><a class=\"pg-link\" href=\"{url}\" style=\"color:#0b6572\">{url}</a></p>",
                url = escape(url),
                label = escape(label),
            ));
        }
        if let Some(note) = self.footnote {
            body.push_str(&format!(
                "<p class=\"pg-muted\" style=\"margin:0;font-size:13px;line-height:1.5;color:#566361\">{}</p>",
                escape(note)
            ));
        }
        let html = format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
             <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
             <meta name=\"color-scheme\" content=\"light dark\"><meta name=\"supported-color-schemes\" content=\"light dark\">\
             <title>{title}</title><style>{STYLE}</style></head>\
             <body class=\"pg-bg\" style=\"margin:0;padding:0;background:#f2f5f4\">\
             <div style=\"display:none;max-height:0;overflow:hidden;opacity:0\">{pre}</div>\
             <table role=\"presentation\" class=\"pg-bg\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" border=\"0\" bgcolor=\"#f2f5f4\" style=\"background:#f2f5f4\"><tr><td align=\"center\" style=\"padding:24px 12px\">\
             <table role=\"presentation\" class=\"pg-card\" width=\"560\" cellpadding=\"0\" cellspacing=\"0\" border=\"0\" bgcolor=\"#ffffff\" style=\"width:100%;max-width:560px;background:#ffffff;border:1px solid #d5ddda;border-radius:12px;font-family:{FONT}\"><tr>\
             <td class=\"pg-pad\" style=\"padding:28px 32px\">\
             <h1 class=\"pg-text\" style=\"margin:0 0 16px;font-size:20px;line-height:1.3;font-weight:700;color:#121c1b\">{heading}</h1>\
             {body}\
             </td></tr></table></td></tr></table></body></html>",
            title = escape(&self.subject),
            pre = escape(self.preheader),
            heading = escape(self.heading),
        );
        Message {
            to: self.to.to_string(),
            subject: self.subject,
            text,
            html,
        }
    }
}
