use super::Message;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn build(to: &str, subject: &str, lines: &[String], link: Option<&str>) -> Message {
    let mut text = lines.join("\n\n");
    let mut html =
        String::from("<!doctype html><html><body><div style=\"font-family:sans-serif\">");
    for l in lines {
        html.push_str(&format!("<p>{}</p>", escape(l)));
    }
    if let Some(link) = link {
        text.push_str(&format!("\n\n{link}"));
        html.push_str(&format!("<p><a href=\"{0}\">{0}</a></p>", escape(link)));
    }
    html.push_str("</div></body></html>");
    Message {
        to: to.to_string(),
        subject: subject.to_string(),
        text,
        html,
    }
}

pub fn password_reset(to: &str, link: &str) -> Message {
    build(
        to,
        "Reset your password",
        &[
            "Someone asked to reset the password for this account. Open the link below to choose a new one. It works once and expires in 1 hour.".into(),
            "If this was not you, ignore this email. Your password stays as it is.".into(),
        ],
        Some(link),
    )
}

pub fn invite(to: &str, link: &str, project: Option<&str>, days: i64) -> Message {
    let what = match project {
        Some(p) => format!("You have been invited to join the project {p}."),
        None => "You have been invited to create an account.".to_string(),
    };
    build(
        to,
        "You have been invited",
        &[
            what,
            format!("Open the link below to set a password. It is valid for {days} days."),
        ],
        Some(link),
    )
}

pub fn email_change(to: &str, link: &str) -> Message {
    build(
        to,
        "Confirm your new email address",
        &[
            "Open the link below to confirm this address for your account. It works once and expires in 1 hour.".into(),
            "If this was not you, ignore this email and nothing changes.".into(),
        ],
        Some(link),
    )
}

pub fn email_changed(to: &str, new_email: &str) -> Message {
    build(
        to,
        "Your account email was changed",
        &[format!(
            "The email address for your account was changed to {new_email}. If this was not you, contact the operator."
        )],
        None,
    )
}

pub fn event(to: &str, project: &str, subject: &str, detail: &str) -> Message {
    build(
        to,
        &format!("[{project}] {subject}"),
        &[
            format!("Project {project}: {subject}."),
            detail.to_string(),
            "You get at most one email per event type per project per hour. You can turn these off on your account page.".into(),
        ],
        None,
    )
}
