# Design language

One language for the console, the hosted pay page and email. Tokens live in `web/tokens.css`. The living reference is `/console/styleguide` (static, CSP safe, shows every component in light and dark). Surface guides: [console](console.md), [pay page](pay-page.md), [email](email.md).

## Console identity (v3)

Subject: a payment gateway console for operators and merchant developers. They read amounts, statuses and ids all day, in sats, EUR and BTC.

Direction: a ledger book bound in deep navy. The memorable thing is the overview: one large paid-volume figure over a real daily volume chart, with a quiet ledger strip of figures below. Everything else stays disciplined.

### Palette

| Name | Light | Dark | Role |
| --- | --- | --- | --- |
| Paper | `#eef1f6` | `#0a1120` | page |
| Ledger | `#ffffff` | `#111b2e` | cards, inputs |
| Ink | `#0f1a2e` | `#e5ebf7` | text |
| Cobalt | `#2440d4` | `#8aa0ff` | action, links, chart bars |
| Navy | `#0c1530` | `#060a16` | navigation rail |
| Brass | `#ffb454` | `#ffb454` | current location mark in the rail, today's bar |

State colours (green, amber, red, teal for info) are unchanged in role. Cobalt on navy replaces the teal accent, which read as the generic fintech default.

### Type

- Bricolage Grotesque (variable, self-hosted) for everything: headings are bold with tight tracking (-0.03em), body at 14 px. Its slightly quirky grotesque shapes carry the personality, so no second display face.
- IBM Plex Mono (self-hosted, 400 and 500) for ids, keys and codes only.
- Scale: 11, 12, 14, 16, 20, 24, 34 (figures), 40, hero figure `clamp(36px, 7vw, 56px)`.

### Layout

```
+---------+--------------------------------------------+
| navy    |  Project name (30px)                        |
| rail    |  tabs: underline only, wrap, no scroll      |
| 236px   |  [7 | 30 | 90 days]                          |
|         |  +--------------------------------------+   |
|         |  | 61,000 sats            [BTC|EUR]     |   |
|         |  | paid across 7 payments                |   |
|         |  | ||  |  ||||     |   daily bars      |   |
|         |  | ====== method share bar ======        |   |
|         |  +--------------------------------------+   |
|         |  | pending | success | settle | webhooks |   |
|         |  Top customers (ruled table, left aligned)  |
+---------+--------------------------------------------+
```

Left aligned throughout, amounts right aligned in tables. Under 800 px the rail becomes a single scrolling strip above the content.

### Defaults avoided

- Cream paper with a serif and a clay accent: used cool blue grey paper and a grotesque instead.
- Identical rounded stat cards with soft grey shadows: the figures sit in one ruled strip, and the hero is a single large panel. No shadows on cards.
- Uppercase muted section labels: headings are sentence case 16 px bold.
- Monospace for small data labels: mono is only for ids and keys.
- A tracked caps eyebrow, numbered markers, dash-joined meta strings and arrows on links: none.
- Pills are 3 px rectangles with a status dot, not capsules, so state does not rely on colour alone.

### Critique notes

The first pass left the rail as a centred column at 390 px and an empty grey gap in the ledger strip; both were fixed (rail strip, hairline shadows instead of a grey gap). The chart uses the brass bar for today so the latest day is findable without a legend.

## Principles

The product handles other people's money, so it should feel like a good bank statement: calm, exact and quiet.

1. **Numbers first.** Amounts, statuses and ids are the content. They use tabular figures, never wrap mid-value and never share a line with decoration.
2. **One memorable thing.** The identity is a cobalt accent and a navy rail on cool blue greys (paper in light, ink in dark). Everything else stays neutral. Colour means something: teal is the action, green/amber/red/blue are states. Nothing is coloured for decoration.
3. **Say what happened.** Every state names its cause and the next step. Errors do not apologise.
4. **No surprises with money.** Destructive and irreversible actions ask first, name what will be lost, and use the same verb on the button as in the dialog title.
5. **Works everywhere.** 360 px wide, keyboard only, light or dark, reduced motion, no external fonts, scripts or images.
6. **Borders over shadows.** Structure comes from 1 px lines. Shadow is reserved for things that float (dialogs, toasts) plus a faint lift on cards.

Not used on purpose: gradients, glass, all caps labels beyond the small section headings in the console nav, emoji as icons, animated entrances.

## Tokens

All tokens are CSS custom properties in `web/tokens.css`. Dark values apply with `prefers-color-scheme: dark`, or with `data-theme="dark"` / `data-theme="light"` on any element to force one (the styleguide uses this). Never write a colour literal in `console.css`, `pay.css` or `components.css`; a test enforces it.

### Colour

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `--bg` | `#eef1f6` | `#0a1120` | page |
| `--card` | `#ffffff` | `#111b2e` | cards, inputs, dialogs |
| `--code` | `#e4e9f1` | `#1a2640` | code, hover rows, secret boxes |
| `--text` | `#0f1a2e` | `#e5ebf7` | body |
| `--muted` | `#52607a` | `#9aa8c4` | hints, metadata, table heads |
| `--line` | `#d6ddea` | `#243250` | decorative dividers |
| `--line-strong` | `#7384a3` | `#5f7196` | borders of controls (3:1 needed) |
| `--accent` / `--accent-text` | `#2440d4` / `#ffffff` | `#8aa0ff` / `#0a1230` | primary action, links, focus |
| `--ok`, `--ok-bg` | `#1a6b39`, `#e3f3e8` | `#62c786`, `#12301e` | paid, completed, delivered |
| `--warn`, `--warn-bg` | `#86500a`, `#fbf0d9` | `#e5b45a`, `#36280c` | pending, needs attention, one time secrets |
| `--bad`, `--bad-bg` | `#b3261e`, `#fbe6e4` | `#f0877f`, `#3a1714` | failed, expired, destructive |
| `--info`, `--info-bg` | `#0b6572`, `#dcf0f2` | `#52c3d1`, `#0e2d33` | informational, processing, refunded |

Rule: never convey state by colour alone. A pill always carries its status word.

### Contrast (WCAG AA)

`node scripts/contrast.mjs` reads `tokens.css`, checks every pair below in both themes and exits non zero on failure. Text pairs need 4.5:1, control borders and focus ring 3:1. Current result: 38 of 38 pass.

Run the script for the current ratios; every pair passes in both themes.

### Type

- Bricolage Grotesque (variable, self-hosted) for everything: headings are bold with tight tracking (-0.03em), body at 14 px. Its slightly quirky grotesque shapes carry the personality, so no second display face.
- IBM Plex Mono (self-hosted, 400 and 500) for ids, keys and codes only.
- Scale: 11, 12, 14, 16, 20, 24, 34 (figures), 40, hero figure `clamp(36px, 7vw, 56px)`.

### Layout

```
+---------+--------------------------------------------+
| navy    |  Project name (30px)                        |
| rail    |  tabs: underline only, wrap, no scroll      |
| 236px   |  [7 | 30 | 90 days]                          |
|         |  +--------------------------------------+   |
|         |  | 61,000 sats            [BTC|EUR]     |   |
|         |  | paid across 7 payments                |   |
|         |  | ||  |  ||||     |   daily bars      |   |
|         |  | ====== method share bar ======        |   |
|         |  +--------------------------------------+   |
|         |  | pending | success | settle | webhooks |   |
|         |  Top customers (ruled table, left aligned)  |
+---------+--------------------------------------------+
```

Left aligned throughout, amounts right aligned in tables. Under 800 px the rail becomes a single scrolling strip above the content.

### Defaults avoided

- Cream paper with a serif and a clay accent: used cool blue grey paper and a grotesque instead.
- Identical rounded stat cards with soft grey shadows: the figures sit in one ruled strip, and the hero is a single large panel. No shadows on cards.
- Uppercase muted section labels: headings are sentence case 16 px bold.
- Monospace for small data labels: mono is only for ids and keys.
- A tracked caps eyebrow, numbered markers, dash-joined meta strings and arrows on links: none.
- Pills are 3 px rectangles with a status dot, not capsules, so state does not rely on colour alone.

### Critique notes

The first pass left the rail as a centred column at 390 px and an empty grey gap in the ledger strip; both were fixed (rail strip, hairline shadows instead of a grey gap). The chart uses the brass bar for today so the latest day is findable without a legend.

## Principles

The product handles other people's money, so it should feel like a good bank statement: calm, exact and quiet.

1. **Numbers first.** Amounts, statuses and ids are the content. They use tabular figures, never wrap mid-value and never share a line with decoration.
2. **One memorable thing.** The identity is a cobalt accent and a navy rail on cool blue greys (paper in light, ink in dark). Everything else stays neutral. Colour means something: teal is the action, green/amber/red/blue are states. Nothing is coloured for decoration.
3. **Say what happened.** Every state names its cause and the next step. Errors do not apologise.
4. **No surprises with money.** Destructive and irreversible actions ask first, name what will be lost, and use the same verb on the button as in the dialog title.
5. **Works everywhere.** 360 px wide, keyboard only, light or dark, reduced motion, no external fonts, scripts or images.
6. **Borders over shadows.** Structure comes from 1 px lines. Shadow is reserved for things that float (dialogs, toasts) plus a faint lift on cards.

Not used on purpose: gradients, glass, all caps labels beyond the small section headings in the console nav, emoji as icons, animated entrances.

## Tokens

All tokens are CSS custom properties in `web/tokens.css`. Dark values apply with `prefers-color-scheme: dark`, or with `data-theme="dark"` / `data-theme="light"` on any element to force one (the styleguide uses this). Never write a colour literal in `console.css`, `pay.css` or `components.css`; a test enforces it.

### Colour

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `--bg` | `#eef1f6` | `#0a1120` | page |
| `--card` | `#ffffff` | `#111b2e` | cards, inputs, dialogs |
| `--code` | `#e4e9f1` | `#1a2640` | code, hover rows, secret boxes |
| `--text` | `#0f1a2e` | `#e5ebf7` | body |
| `--muted` | `#52607a` | `#9aa8c4` | hints, metadata, table heads |
| `--line` | `#d6ddea` | `#243250` | decorative dividers |
| `--line-strong` | `#7384a3` | `#5f7196` | borders of controls (3:1 needed) |
| `--accent` / `--accent-text` | `#2440d4` / `#ffffff` | `#8aa0ff` / `#0a1230` | primary action, links, focus |
| `--ok`, `--ok-bg` | `#1a6b39`, `#e3f3e8` | `#62c786`, `#12301e` | paid, completed, delivered |
| `--warn`, `--warn-bg` | `#86500a`, `#fbf0d9` | `#e5b45a`, `#36280c` | pending, needs attention, one time secrets |
| `--bad`, `--bad-bg` | `#b3261e`, `#fbe6e4` | `#f0877f`, `#3a1714` | failed, expired, destructive |
| `--info`, `--info-bg` | `#0b6572`, `#dcf0f2` | `#52c3d1`, `#0e2d33` | informational, processing, refunded |

Rule: never convey state by colour alone. A pill always carries its status word.

### Contrast (WCAG AA)

`node scripts/contrast.mjs` reads `tokens.css`, checks every pair below in both themes and exits non zero on failure. Text pairs need 4.5:1, control borders and focus ring 3:1. Current result: 38 of 38 pass.

| Pair | Light | Dark |
| --- | --- | --- |
| text on bg / card | 15.85 / 17.39 | 15.65 / 14.31 |
| muted on bg / card / code | 5.71 / 6.26 / 5.33 | 7.94 / 7.26 / 6.34 |
| accent on bg / card | 6.14 / 6.73 | 9.02 / 8.25 |
| accent-text on accent | 6.73 | 7.98 |
| ok on card / ok-bg | 6.55 / 5.69 | 8.21 / 6.84 |
| warn on card / warn-bg | 6.63 / 5.87 | 9.01 / 7.51 |
| bad on card / bad-bg | 6.54 / 5.46 | 6.94 / 6.46 |
| info on card / info-bg | 6.71 / 5.67 | 7.71 / 6.77 |
| line-strong on card / bg (3:1) | 3.70 / 3.37 | 3.44 / 3.76 |
| focus on card (3:1) | 6.73 | 8.25 |

### Type

Self-hosted fonts served from `/console/fonts/` (CSP `font-src 'self'`, no CDNs), with system fallbacks.

- `--font-sans`: Bricolage Grotesque, then `system-ui`.
- `--font-mono`: Plex Mono, then `ui-monospace`, for ids, keys, invoices, codes and addresses only.

Scale: `--fs-xs` 11, `--fs-sm` 12, `--fs-base` 14, `--fs-md` 16, `--fs-lg` 20, `--fs-xl` 24, `--fs-display` 32. Line height 1.5 for text, 1.25 for headings. Weights: 400 body, 600 emphasis and buttons, 700 headings and amounts. Amounts use `font-variant-numeric: tabular-nums`. Keep lines under 75 characters. The pay page body is 16 px, the console 14 px.

### Space, radius, shadow

- Space: 4, 8, 12, 16, 24, 32, 48 (`--space-1` to `--space-7`).
- Radius: 4 chips and skeletons, 8 controls, 10 pay page method buttons, 12 cards and dialogs, 14 pay page card, pill for badges. Use the smaller radius for the smaller thing.
- Shadow: `--shadow-1` cards (barely visible), `--shadow-2` toasts, `--shadow-3` dialogs.

### Motion

`--dur-fast` 120 ms for hover and focus, `--dur` 200 ms for open and close, easing `--ease`. Motion answers an action; nothing animates on load. The spinner and the loading skeleton are the only looping animations. `tokens.css` contains a global `prefers-reduced-motion` rule that makes all animations and transitions instant. Do not add JS driven motion without checking `matchMedia("(prefers-reduced-motion: reduce)")`.

### Breakpoints

- 360 px: the minimum. Nothing may scroll the page sideways. Wide tables scroll inside their container.
- 800 px: the console nav collapses above the content (tablet and below).
- 1100 px: console content max width.
- Pay page is a single 460 px column at every width.

Focus: every interactive element shows a 2 px `--focus` outline with 2 px offset on `:focus-visible`. Never remove it.

## Components

Classes are in `console.css` (existing) and `components.css` (new). See the styleguide for each.

- **Buttons.** One `primary` per view (the thing most people do next). Default for secondary. `danger` for destructive, never filled red. Disabled while a request runs and the label changes to the progress form ("Saving"). Labels are verb plus object: "Save changes", "Create project", not "Submit" or "OK".
- **Inputs and validation.** Label above, always visible, sentence case. Hint below in `--muted`. Validate on submit and on blur, not on every key. An invalid field gets `field invalid`, a red border, `aria-invalid="true"` and a hint that says how to fix it ("Use an https URL that is reachable from the internet."). Do not clear what the person typed.
- **Tables and paging.** Left aligned text, amounts right aligned where columns are numeric, muted header 12 px, row dividers only. Clickable rows get `click` and must also contain a link for keyboard use. Paging is Previous / Next with a "1 to 25 of 140" count, aligned right. Wide tables scroll inside `main`.
- **Tabs.** `tabs` with the current tab marked `on`. Tabs switch views within one project and keep the URL hash. At most 8. See the console guide.
- **Badges and status pills.** `pill <status>`. Always lower case, the raw status word with underscores shown as written by the API. Tone follows what the status means for the person, not whether the payment completed. Red is only for something that broke or needs action. A customer letting a payment lapse or a merchant cancelling is an ordinary outcome and stays neutral.

  | Tone | Class | Payment | Attempt | Refund | Withdrawal | Webhook |
  | --- | --- | --- | --- | --- | --- | --- |
  | green | `ok` | paid | paid | completed | completed | delivered |
  | amber | `pending` | pending, partially_refunded | pending | pending | requested | waiting, retrying |
  | blue | `info` | refunded | none | none | processing | none |
  | neutral | `expired`, `cancelled` | expired, cancelled | expired, cancelled | none | cancelled | none |
  | red | `failed`, `disputed`, `refund_failed`, `bad` | failed, disputed | failed | refund_failed | failed | failed (abandoned) |

  Subscriptions: active green, trialing blue, incomplete amber, past_due red, paused and cancelled neutral. Invoice periods: paid green, open amber, void neutral.

  Unknown statuses fall back to a neutral outlined pill.
- **Money.** Stored and compared as integer minor units; BTC is msats. Never use floats for storing or comparing. Display only: fiat with `Intl.NumberFormat(undefined, {style:"currency"})` so the person's locale decides separators; BTC amounts as whole sats with thousands separators and the word "sats" ("250,000 sats", rounded up from msats on the pay page). Show msats only in the console where sub-sat precision matters, written "250,000,000 msats". Wrap in `.money` (tabular figures, no wrapping). Show the currency code when it is not obvious from the symbol. A fee is shown beside the amount as "+1.50 EUR fee", never folded in silently.
- **Copyable ids and secrets.** Ids are monospace and truncated in the middle only with a full value in `title`. A copy control is a button reading "Copy" that changes to "Copied" for 1.5 s and announces through `aria-live`. Secrets are shown once in the `secret` box (dashed amber border) with the text "(shown once, copy it now)". They are never put in a URL, a log or a toast.
- **QR blocks.** Always on a white square with 6 to 8 px padding, in both themes, so scanners work. Provide `alt` text that names what the code is ("Lightning invoice QR code") and the same payload as a copyable string below it.
- **Empty states.** A bold sentence saying what is missing, one line on how to fix it, and the primary action. "No payments yet. Create a payment from your app or send a test payment."
- **Loading.** Show the shell at once and use `skeleton` rows for lists. Use plain "Loading" text only before the shell exists. Spinners only next to a status line ("Waiting for payment").
- **Error states.** `err` text (console) or a `banner bad` for page level problems. Say what failed and what to do. Include `role="alert"`.
- **Confirm dialogs.** For deleting, rotating keys, disabling 2FA, refunds and any change that cannot be undone. Title is a question using the action's verb ("Delete the Stripe provider?"), body says what is lost and what stays, two buttons: a plain one that names the safe choice ("Keep provider") and a `danger` one repeating the action ("Delete provider"). Focus starts on the safe button. Escape cancels. Never use a bare browser `confirm()` for new work.
- **Banners.** `banner info|ok|warn|bad`, inline at the top of the area they concern, with a bold first line. Not dismissible if they report a standing condition (test limits).
- **Toasts.** Short confirmation of a finished action, using the button's verb in past tense ("Copied", "Saved", "Key rotated"). Bottom left on desktop, full width on mobile, `role="status"`, gone after 5 s unless it has an Undo.
- **2FA and recovery codes.** The setup screen shows the QR block, the secret as copyable text, then a 6 digit field with `inputmode="numeric"` and `autocomplete="one-time-code"`. Recovery codes appear once, in the `codes` grid (monospace, two or three columns), under a `banner warn` that says each code works once and they are not shown again. Offer Copy codes and Download, and require "I saved them" before leaving.

## Voice and tone

- Plain words. "Sign in", not "authenticate". "Payment", not "transaction object". Describe what a thing does for the person, not how it is built: "Notifications", not "webhook config" in customer facing text.
- Sentence case everywhere, including buttons and headings. No exclamation marks.
- Active voice. Buttons are verb plus object. The same action keeps its name through button, dialog and toast.
- Errors state the cause and the fix and do not apologise: "The webhook URL must use https." not "Oops, something went wrong". Pattern: what happened, then what to do. Never blame the person.
- Empty states invite an action. Success messages are one short line.
- Numbers and units are exact ("expires in 1 hour", "valid for 7 days").
- No em dashes or en dashes anywhere a person reads. Use a full stop, comma, colon or parentheses, and a plain hyphen for ranges (1-128 characters). No "not just X, it's Y", no "seamless", "robust", "unlock", "elevate".
- Dates in the console are relative for the last day ("5m ago") and absolute afterwards, always with the time zone in a tooltip.
