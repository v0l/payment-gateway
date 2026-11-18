# Pay page guide

The hosted page `/pay/{id}` and the embed `/embed/{id}` (see `gateway.js`). They share `web/pay.css` and `web/tokens.css`. Read the [design language](README.md) first.

## Shape

One card, 460 px max, centred. From top: brand row (tenant logo and name), amount (32 px, bold, tabular), description (muted), then the state area. The page background and card follow the visitor's light or dark preference. The embed has a transparent background, 8 px padding and reports its height to the parent with `pgw:resize`.

## Tenant branding limits

A tenant may change only:

| Setting | Limit |
| --- | --- |
| Accent colour | 3 or 6 digit hex. The page ignores it when it has less than 4.5:1 contrast against the card in the visitor's current theme, and picks black or white button text from the accent's luminance. |
| Logo | `https://` URL, shown at 28 px high, max 120 px wide, `referrerpolicy="no-referrer"`, empty alt. |
| Name | Plain text. |

A tenant may not change: fonts, radius, spacing, status colours, copy, layout, or inject CSS or HTML. The accent affects the primary button, links, focus ring, spinner and method hover border only. If a tenant needs a font or radius token later, add it to this table first and validate it in `validate.rs`.

## States

- **Choose.** "Choose how to pay", then one full width button per method: method name left, fee right ("+0.25 EUR fee"). Methods with options (other crypto) include a coin select inside the row.
- **Wait.** The chosen method's details: QR on a white square, then copyable fields (invoice, address, amount). A spinner line "Waiting for payment" and the total. For card and bank, a primary "Continue to payment" button and one line saying the person will go to the provider. "Use a different method" is a text button below.
- **Success.** Check mark in `--ok`, "Payment received", and "Continue" if there is a return URL (redirects after 2.5 s on the full page, never in the embed). Receipts follow the same card: amount, description, reference and time, with the status word beside the icon.
- **Failure and expiry.** Cross in `--bad`, title says what happened ("This payment has expired", "This payment was cancelled"), and "Back" if there is a return URL. Say what to do next when there is something: "Ask the shop for a new payment link."
- **Error.** Inline `error` text with `role="alert"` under the card content. The state area stays usable.
- **Loading.** "Loading" text in the card shell.

## Content rules

Copy is neutral, second person, no jargon: "Lightning invoice", "Bitcoin address", "Send exactly this amount." Status words are never colour only: icon plus title. No em or en dashes.

## Accessibility

- `main` has `aria-live="polite"` so state changes are announced. Do not move focus on polling updates.
- Every control is a real button or link, reachable by keyboard, with the global 2 px focus ring.
- Copy buttons change text to "Copied" (announced by the live region).
- QR images carry alt text and the same payload is available as copyable text.
- Touch targets are at least 44 px high on method and primary buttons.
- Works from 320 px wide, 200% zoom, light and dark, and reduced motion (spinner stops).
- The page sets no inline styles and loads nothing outside `'self'` except the tenant logo (`https:` images).
