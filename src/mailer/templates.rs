use super::Message;
use super::layout::Mail;

pub fn password_reset(to: &str, link: &str) -> Message {
    Mail {
        to,
        subject: "Reset your password".into(),
        preheader: "Choose a new password. The link works once and expires in 1 hour.",
        heading: "Reset your password",
        paragraphs: vec![
            "Someone asked to reset the password for this account. Open the link below to choose a new one. It works once and expires in 1 hour.".into(),
        ],
        action: Some(("Choose a new password", link)),
        footnote: Some("If this was not you, ignore this email. Your password stays as it is."),
    }
    .render()
}

pub fn invite(to: &str, link: &str, project: Option<&str>, days: i64) -> Message {
    let what = match project {
        Some(p) => format!("You have been invited to join the project {p}."),
        None => "You have been invited to create an account.".to_string(),
    };
    Mail {
        to,
        subject: "You have been invited".into(),
        preheader: "Open the link to set a password and join.",
        heading: "You have been invited",
        paragraphs: vec![
            what,
            format!("Open the link below to set a password. It is valid for {days} days."),
        ],
        action: Some(("Accept the invite", link)),
        footnote: None,
    }
    .render()
}

pub fn email_change(to: &str, link: &str) -> Message {
    Mail {
        to,
        subject: "Confirm your new email address".into(),
        preheader: "Confirm this address for your account.",
        heading: "Confirm your new email address",
        paragraphs: vec![
            "Open the link below to confirm this address for your account. It works once and expires in 1 hour.".into(),
        ],
        action: Some(("Confirm email address", link)),
        footnote: Some("If this was not you, ignore this email and nothing changes."),
    }
    .render()
}

pub fn email_changed(to: &str, new_email: &str) -> Message {
    Mail {
        to,
        subject: "Your account email was changed".into(),
        preheader: "The email address for your account changed.",
        heading: "Your account email was changed",
        paragraphs: vec![format!(
            "The email address for your account was changed to {new_email}. If this was not you, contact the operator."
        )],
        action: None,
        footnote: None,
    }
    .render()
}

pub fn event(to: &str, project: &str, subject: &str, detail: &str) -> Message {
    Mail {
        to,
        subject: format!("[{project}] {subject}"),
        preheader: detail,
        heading: subject,
        paragraphs: vec![format!("Project {project}: {subject}."), detail.to_string()],
        action: None,
        footnote: Some(
            "You get at most one email per event type per project per hour. You can turn these off on your account page.",
        ),
    }
    .render()
}

pub fn receipt(to: &str, seller: &str, receipt_url: &str) -> Message {
    Mail {
        to,
        subject: format!("Your receipt from {seller}"),
        preheader: "View and print your receipt.",
        heading: "Your receipt",
        paragraphs: vec![format!("Thank you for your payment to {seller}.")],
        action: Some(("View receipt", receipt_url)),
        footnote: None,
    }
    .render()
}
