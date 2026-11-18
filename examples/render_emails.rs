use payment_gateway::mailer::templates;
use std::fs;

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "emails".into());
    fs::create_dir_all(&dir).unwrap();
    let base = "https://pay.example.com/console";
    let mails = [
        (
            "password-reset",
            templates::password_reset("you@example.com", &format!("{base}/reset/9f2c1d")),
        ),
        (
            "invite-project",
            templates::invite(
                "you@example.com",
                &format!("{base}/invite/9f2c1d"),
                Some("Coffee shop"),
                7,
            ),
        ),
        (
            "invite-account",
            templates::invite("you@example.com", &format!("{base}/invite/9f2c1d"), None, 7),
        ),
        (
            "email-change",
            templates::email_change("you@example.com", &format!("{base}/email/9f2c1d")),
        ),
        (
            "email-changed",
            templates::email_changed("you@example.com", "new@example.com"),
        ),
        (
            "event",
            templates::event(
                "you@example.com",
                "Coffee shop",
                "Payment failed",
                "Payment pay_8f3k2 for 12.50 EUR failed on Stripe.",
            ),
        ),
    ];
    for (name, m) in mails {
        fs::write(format!("{dir}/{name}.html"), &m.html).unwrap();
        fs::write(format!("{dir}/{name}.txt"), &m.text).unwrap();
    }
}
