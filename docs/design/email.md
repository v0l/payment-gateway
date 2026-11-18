# Email guide

Templates are in `src/mailer/templates.rs`, the shared layout in `src/mailer/layout.rs`. Every mail has an HTML body and a plain text twin. Rendered samples: run `cargo run --example render_emails docs/design/emails`; screenshots are in `docs/design/screenshots/`.

## Layout

- Nested tables with `role="presentation"`. A 560 px card (`width:100%;max-width:560px`) centred on a page coloured cell. No flexbox, grid, web fonts or media dependent layout, so Outlook, Gmail, Apple Mail and mobile clients render it the same way.
- All visual styling is inline. The only `<style>` block carries dark mode and a small mobile padding rule, which clients that strip it can lose safely.
- System font stack, 15 px body, 20 px heading, 1.55 line height. Link and button colour is the accent `#0b6572`.
- Structure: hidden preheader, heading, paragraphs, one button (bulletproof: a table cell with `bgcolor` and a padded link), the same link printed as selectable text, then a muted footnote.
- Escape every interpolated value (project names, emails, details).

## Plain text twin

Heading, a blank line, paragraphs separated by blank lines, the link on its own line, then the footnote. No markup, no tracking.

## No remote assets, no tracking

No `<img>`, no `url()`, no remote CSS, no tracking pixels, no link rewriting. Links go straight to the console. A test (`tests/mail_templates.rs`) fails if `<img`, `src=`, `url(` or `<script` appear.

## Dark mode

`color-scheme` and `supported-color-schemes` meta plus `@media (prefers-color-scheme:dark)` override classes (`pg-bg`, `pg-card`, `pg-text`, `pg-muted`, `pg-btn`, `pg-link`, `pg-code`) using the dark tokens (`#0c1312`, `#141d1c`, `#e4ecea`, accent `#52c3d1`). Inline colours are the light values so clients that ignore dark mode stay readable, and clients that force their own dark mode invert a layout that already has tolerable contrast.

## Templates

| Mail | Subject | Button | Footnote |
| --- | --- | --- | --- |
| Invite | You have been invited | Accept the invite | none |
| Password reset | Reset your password | Choose a new password | If this was not you, ignore this email. Your password stays as it is. |
| Email change | Confirm your new email address | Confirm email address | If this was not you, ignore this email and nothing changes. |
| Email changed (to the old address) | Your account email was changed | none | none |
| Project notification | [Project] Event | none | At most one email per event type per project per hour, and how to turn them off. |

There is no login challenge mail today; a future one follows the password reset pattern with the code in a monospace block.

## Voice

Same as the [design language](README.md): plain, specific, no apology, no em or en dashes. State when a link expires and that it works once. Always say what to do if the person did not ask for the mail.
