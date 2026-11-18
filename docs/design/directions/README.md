# Console redesign directions

Static mockups with the same Northwind Hosting data. Regenerate with `node gen/a.mjs` (and b, c), then `sh gen/shoot.sh`. Screenshots are `{a,b,c}-{overview,payments,payment}-{1440,390}.png`. Fonts are self hosted under each direction's `fonts/`.

## A: Statement

A printed account statement. Top text tabs, one paper sheet on a grey desk, no cards or tiles. The overview opens with a sentence ("was paid EUR 14,236.40 across 291 payments") and a dotted-leader ledger that sums to what can be withdrawn. Needs-a-decision items sit in the margin as notes. Payments are grouped by day with the filter written as a sentence. Payment detail is a receipt with a stamp and a margin log.
Palette: desk #d8dbd5, paper #fffefb, ink #1d2420, paid #1f6b4a, loss #8f2d2d, wait #8a5a00, mark #f2e27a. Type: Literata only.

## B: Terminal

A keyboard-first operator view. No sidebar and no tab bar: a breadcrumb, a command palette field and a status line with key hints. The overview is a live event tail beside a "needs you" queue. Payments is a list with an inspector pane and raw JSON. Payment detail is a two pane page with a millisecond timeline, and webhook deliveries.
Palette: bg #1d2433, panel #252f44, text #cfd8e6, dim #7d8aa3, amber #f5b941, mint #8fd6b4, coral #ef7b7b, sky #7fb4f0. Type: JetBrains Mono only.

## C: Lanes

A triage board where the payment method is the colour system. Pill nav on a dark hub bar (bottom tab bar on phones). The overview opens with what needs you, then payments by state in columns, then one swimlane per method. Payments are grouped into method bands with an amount-length bar. Payment detail opens with a journey line and a method coloured amount block.
Palette: ground #eceef6, ink #181540, lightning #f7c52b, on-chain #ff6b3d, card #3f5bff, PayPal #0aa5c9, bank #1fa971, alert #c4234a. Type: Schibsted Grotesk.
