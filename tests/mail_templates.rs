use payment_gateway::mailer::Message;
use payment_gateway::mailer::templates;

fn all() -> Vec<Message> {
    vec![
        templates::password_reset(
            "a@example.com",
            "https://pay.example.com/console/reset/tok123",
        ),
        templates::invite(
            "a@example.com",
            "https://pay.example.com/console/invite/tok123",
            Some("Coffee <shop>"),
            7,
        ),
        templates::invite(
            "a@example.com",
            "https://pay.example.com/console/invite/tok123",
            None,
            7,
        ),
        templates::email_change(
            "a@example.com",
            "https://pay.example.com/console/email/tok123",
        ),
        templates::email_changed("a@example.com", "new@example.com"),
        templates::event(
            "a@example.com",
            "Coffee shop",
            "Payment failed",
            "Payment pay_8f3k2 for 12.50 EUR failed.",
        ),
    ]
}

#[test]
fn every_template_has_a_plain_text_twin_and_no_remote_assets() {
    for m in all() {
        assert!(!m.text.trim().is_empty(), "{}", m.subject);
        assert!(!m.html.contains("<img"), "{}", m.subject);
        assert!(!m.html.contains("src="), "{}", m.subject);
        assert!(!m.html.contains("url("), "{}", m.subject);
        assert!(!m.html.contains("<script"), "{}", m.subject);
    }
}

#[test]
fn templates_use_table_layout_dark_mode_and_a_hidden_preheader() {
    for m in all() {
        assert!(m.html.contains("role=\"presentation\""), "{}", m.subject);
        assert!(
            m.html.contains("prefers-color-scheme:dark"),
            "{}",
            m.subject
        );
        assert!(m.html.contains("color-scheme"), "{}", m.subject);
        assert!(
            m.html.contains("display:none;max-height:0"),
            "{}",
            m.subject
        );
        assert!(m.html.contains("name=\"viewport\""), "{}", m.subject);
    }
}

#[test]
fn templates_escape_user_supplied_text() {
    let m = templates::invite(
        "a@example.com",
        "https://x.example/console/invite/t",
        Some("<b>x</b>"),
        7,
    );
    assert!(!m.html.contains("<b>x</b>"));
    assert!(m.html.contains("&lt;b&gt;x&lt;/b&gt;"));
    let e = templates::email_changed("a@example.com", "<i>@example.com");
    assert!(!e.html.contains("<i>@"));
}

#[test]
fn templates_never_use_dashes_as_punctuation() {
    for m in all() {
        assert!(!m.text.contains('\u{2014}') && !m.text.contains('\u{2013}'));
        assert!(!m.html.contains('\u{2014}') && !m.html.contains('\u{2013}'));
        assert!(!m.html.contains("&mdash;") && !m.html.contains("&ndash;"));
    }
}

#[test]
fn action_mails_carry_the_link_as_button_and_plain_text() {
    let url = "https://pay.example.com/console/reset/tok123";
    let m = templates::password_reset("a@example.com", url);
    assert!(m.text.ends_with(url) || m.text.contains(url));
    assert!(m.html.matches(url).count() >= 2);
}
