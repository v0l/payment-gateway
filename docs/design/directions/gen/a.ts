import { writeFileSync } from "node:fs";
import { project, payments, detail, month, attention, methods, eur, sats, esc, statusWord } from "./data";

const out = new URL("../a/", import.meta.url).pathname;

const css = `
@font-face{font-family:Literata;font-weight:400;src:url(fonts/literata-400.woff2) format("woff2")}
@font-face{font-family:Literata;font-weight:400;font-style:italic;src:url(fonts/literata-400i.woff2) format("woff2")}
@font-face{font-family:Literata;font-weight:600;src:url(fonts/literata-600.woff2) format("woff2")}
:root{--desk:#d8dbd5;--paper:#fffefb;--ink:#1d2420;--soft:#5b665f;--rule:#9aa59d;--hair:#dfe3dc;--paid:#1f6b4a;--loss:#8f2d2d;--wait:#8a5a00;--mark:#f2e27a}
*{box-sizing:border-box}
html{background:var(--desk)}
body{margin:0;color:var(--ink);font:400 17px/1.6 Literata,Georgia,serif;font-variant-numeric:lining-nums tabular-nums}
a{color:inherit}
:focus-visible{outline:3px solid var(--ink);outline-offset:2px}
.bar{display:flex;align-items:baseline;gap:32px;max-width:1040px;margin:0 auto;padding:22px 24px 0}
.bar b{font-weight:600;font-size:18px;white-space:nowrap}
.bar nav{display:flex;gap:22px;overflow-x:auto;flex:1;scrollbar-width:none}
.bar nav a{text-decoration:none;color:var(--soft);padding-bottom:3px;white-space:nowrap}
.bar nav a[aria-current]{color:var(--ink);border-bottom:2px solid var(--ink)}
.bar .who{display:none}
.sheet{max-width:1040px;margin:18px auto 64px;background:var(--paper);padding:56px 64px 72px;box-shadow:0 1px 0 var(--rule),0 24px 40px -30px rgba(29,36,32,.4);display:grid;grid-template-columns:minmax(0,1fr) 240px;column-gap:56px}
.sheet>*{min-width:0}.sheet>header,.sheet>.full{grid-column:1/-1}
h1{font-weight:600;font-size:40px;line-height:1.15;letter-spacing:-.01em;margin:0 0 8px;max-width:20ch}
h2{font-weight:600;font-size:20px;margin:48px 0 12px}
.dateline{color:var(--soft);font-style:italic;margin:0 0 32px}
.lede{font-size:24px;line-height:1.5;margin:0 0 8px;max-width:34em}
.lede em{font-style:normal;background:linear-gradient(transparent 62%,var(--mark) 62%)}
.ledger{width:100%;border-collapse:collapse;margin:8px 0}
.ledger td{padding:7px 0;vertical-align:baseline}
.ledger td:first-child{width:99%}
.ledger td:first-child span{background:var(--paper);padding-right:10px;position:relative}
.ledger tr td:first-child{background:radial-gradient(circle,var(--rule) .8px,transparent 1.2px) 0 .95em/6px 4px repeat-x}
.ledger td:last-child{text-align:right;white-space:nowrap;padding-left:10px;background:var(--paper)}
.ledger .sub td:first-child span{color:var(--soft);padding-left:24px}
.ledger .total td{border-top:2px solid var(--ink);padding-top:12px;font-weight:600}
.ledger .total td:first-child{background:none}
.neg{color:var(--loss)}.pos{color:var(--paid)}.wait{color:var(--wait)}
.margin{font-size:15px;line-height:1.5;color:var(--soft);padding-top:8px}
.margin h3{font-size:15px;font-weight:600;margin:0 0 4px;color:var(--ink)}
.note{border-left:3px solid var(--ink);padding:2px 0 2px 14px;margin:0 0 22px}
.note.loss{border-color:var(--loss)}.note.wait{border-color:var(--wait)}
.note a{color:var(--ink)}
.strip{display:block;width:100%;height:56px;margin:14px 0 4px}
.strip rect{fill:var(--ink)}
.strip .peak{fill:var(--paid)}
.axis{display:flex;justify-content:space-between;font-size:14px;color:var(--soft)}
.sentence{font-size:18px;margin:0 0 28px;max-width:40em}
.sentence select,.sentence button{font:inherit;color:var(--ink);background:none;border:0;border-bottom:2px solid var(--ink);padding:0 2px;border-radius:0;appearance:none;-webkit-appearance:none;cursor:pointer}
.sentence select{padding-right:2px}
.day{display:flex;justify-content:space-between;align-items:baseline;margin:36px 0 4px;border-bottom:2px solid var(--ink);padding-bottom:4px}
.day b{font-size:18px;font-weight:600}
.day span{color:var(--soft)}
.lines{list-style:none;margin:0;padding:0}
.lines li{display:grid;grid-template-columns:56px minmax(0,1fr) 110px 100px;gap:0 16px;align-items:baseline;padding:11px 0;border-bottom:1px solid var(--hair)}
.lines li:hover{background:#f6f7f2}
.lines time{color:var(--soft);font-size:15px}
.lines .what{min-width:0}
.lines .what a{text-decoration:none}
.lines .what a:hover{text-decoration:underline}
.lines .what small{display:block;color:var(--soft);font-size:14px;line-height:1.4}
.lines .st{font-style:italic;font-size:15px}
.lines .amt{text-align:right}
.lines .amt small{display:block;color:var(--soft);font-size:13px}
.st.paid{color:var(--paid)}.st.pending{color:var(--wait)}.st.expired{color:var(--soft)}.st.failed,.st.disputed{color:var(--loss)}.st.refunded{color:var(--loss)}
.big{font-size:64px;line-height:1.05;font-weight:600;letter-spacing:-.02em;margin:8px 0 4px}
.receipt dl{margin:24px 0;display:grid;grid-template-columns:150px 1fr;gap:0}
.receipt dt{color:var(--soft);padding:8px 0;border-bottom:1px solid var(--hair)}
.receipt dd{margin:0;padding:8px 0;border-bottom:1px solid var(--hair);overflow-wrap:anywhere}
.log{list-style:none;margin:0;padding:0;font-size:15px}
.log li{position:relative;padding:0 0 18px 0}
.log time{display:block;color:var(--soft);font-size:13px}
.stamp{display:inline-block;border:2px solid var(--paid);color:var(--paid);padding:2px 12px;transform:rotate(-3deg);font-weight:600;margin-left:8px;vertical-align:middle;font-size:18px}
.actions{display:flex;flex-wrap:wrap;gap:8px 24px;margin:28px 0 0}
.actions a{font-weight:600}
code,.mono{font-family:ui-monospace,"DejaVu Sans Mono",monospace;font-size:.82em}
@media(max-width:820px){
.bar{flex-wrap:wrap;gap:10px 20px;padding:16px 16px 0}.bar .who{display:none}.bar nav{flex-basis:100%;order:3}
.sheet{grid-template-columns:minmax(0,1fr);padding:28px 18px 48px;margin-top:12px}
h1{font-size:30px}.lede{font-size:20px}.big{font-size:44px}
.margin{margin-top:36px;border-top:2px solid var(--ink);padding-top:16px}
.lines li{grid-template-columns:1fr auto;gap:2px 12px}.lines time{display:none}.lines .st{grid-column:1}.lines .amt{grid-row:1;grid-column:2}
.receipt dl{grid-template-columns:110px 1fr}
.sentence{font-size:16px}
}
@media(prefers-reduced-motion:reduce){*{transition:none!important}}
`;

const tabs = ["Overview", "Payments", "Customers", "Subscriptions", "Balance", "Providers", "Keys", "Settings"];

function shell(title: string, current: string, body: string): string {
  const nav = tabs.map(t => `<a href="${t === "Overview" ? "overview" : t === "Payments" ? "payments" : "#"}.html"${t === current ? " aria-current=\"page\"" : ""}>${t}</a>`).join("");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${title}</title><link rel="stylesheet" href="style.css"></head><body>
<div class="bar"><b>${project.name}</b><nav aria-label="Project">${nav}</nav><span class="who">sam@northwind.example</span></div>
<main class="sheet">${body}</main></body></html>`;
}

function strip() {
  const max = Math.max(...month.daily);
  const w = 100 / month.daily.length;
  const bars = month.daily.map((v, i) => {
    const h = Math.round((v / max) * 54);
    return `<rect class="${v === max ? "peak" : ""}" x="${(i * w + 0.6).toFixed(2)}%" y="${56 - h}" width="${(w - 1.2).toFixed(2)}%" height="${h}"/>`;
  }).join("");
  return `<svg class="strip" role="img" aria-label="Paid volume per day, rising from 1 November to a peak on 18 November" preserveAspectRatio="none">${bars}</svg><div class="axis"><span>1 Nov</span><span>Best day 18 Nov, ${eur(94000)}</span><span>18 Nov</span></div>`;
}

function overview() {
  const closing = month.paidCents - month.refundedCents - month.disputedCents;
  const lines = month.share.map(s => `<tr class="sub"><td><span>${methods[s.method]}, ${s.count} payments</span></td><td>${eur(s.cents)}</td></tr>`).join("");
  const notes = attention.map(a => `<div class="note ${a.kind === "dispute" ? "loss" : a.kind === "failed" ? "loss" : "wait"}"><h3>${esc(a.title)}</h3>${esc(a.body)} <a href="payment.html">${a.cta}</a></div>`).join("");
  return `<header><h1>Statement for ${month.label}</h1><p class="dateline">Live mode, EUR, prepared 18 November at 21:50</p></header>
<div>
<p class="lede">${project.name} was paid <em>${eur(month.paidCents)}</em> across ${month.paidCount} payments, 20% more than the same days in October. ${eur(month.waitingCents)} is still waiting on ${month.waitingCount} unpaid payments.</p>
${strip()}
<h2>The month in figures</h2>
<table class="ledger">
<tr><td><span>Brought forward from 31 October</span></td><td>${eur(month.openingCents)}</td></tr>
<tr><td><span>Paid by customers</span></td><td class="pos">${eur(month.paidCents, { sign: true })}</td></tr>
${lines}
<tr><td><span>Refunded, ${month.refundedCount} payments</span></td><td class="neg">${eur(-month.refundedCents)}</td></tr>
<tr><td><span>Held in a dispute, ${month.disputedCount} payment</span></td><td class="neg">${eur(-month.disputedCents)}</td></tr>
<tr class="total"><td><span>Available to withdraw</span></td><td>${eur(month.openingCents + closing)}</td></tr>
</table>
<p class="dateline" style="margin-top:10px">${sats(month.openingCents + closing)} sats at today's rate. Card and PayPal funds settle after 7 days.</p>
</div>
<aside class="margin"><h3>Needs a decision</h3><p style="margin:0 0 18px">Three items are open.</p>${notes}<h3>Today so far</h3><p style="margin:0">14 payments, ${eur(payments.filter(p => p.day === "Today" && p.status === "paid").reduce((a, p) => a + p.cents, 0))} paid.</p></aside>`;
}

function paymentsPage() {
  const days = ["Today", "Yesterday"];
  const body = days.map(d => {
    const rows = payments.filter(p => p.day === d);
    const total = rows.filter(p => p.status === "paid").reduce((a, p) => a + p.cents, 0);
    const li = rows.map(p => `<li><time>${p.time}</time><div class="what"><a href="payment.html">${esc(p.desc)}</a><small>${p.id} from ${p.customer}, ${methods[p.method]}</small></div><span class="st ${p.status}">${statusWord[p.status]}</span><span class="amt ${p.status === "refunded" ? "neg" : ""}">${eur(p.cents)}<small>${sats(p.cents)} sats</small></span></li>`).join("");
    return `<div class="day"><b>${d}, ${d === "Today" ? "18" : "17"} November</b><span>${rows.length} payments, ${eur(total)} paid</span></div><ul class="lines">${li}</ul>`;
  }).join("");
  return `<header><h1>Payments</h1><p class="dateline">Newest first. 291 this month.</p></header>
<div class="full"><p class="sentence">Showing <select aria-label="Status"><option>every payment</option><option>paid payments</option></select> paid by <select aria-label="Method"><option>any method</option></select> from <select aria-label="Period"><option>the last 7 days</option></select>, matching <button type="button">add a customer or reference</button>.</p>
${body}<p class="dateline" style="margin-top:28px"><a href="#">Show the next 50 payments</a></p></div>`;
}

function paymentPage() {
  const d = detail;
  const log = d.events.map(e => `<li><time>${e.t.slice(0, 8)}</time>${esc(e.note.length > 70 ? e.note.slice(0, 64) + "..." : e.note)}</li>`).join("");
  return `<header><p class="dateline" style="margin:0 0 8px"><a href="payments.html">Payments</a> / ${d.id}</p><h1>Receipt for ${esc(d.desc)}</h1></header>
<div class="receipt"><div class="big">${eur(d.cents)}<span class="stamp">Paid</span></div>
<p class="dateline" style="margin:0">${sats(d.cents)} sats, settled 18 November at 21:46 over Lightning</p>
<dl><dt>Billed to</dt><dd>${d.customer}, amelia.k@northwind.example</dd><dt>For</dt><dd>${esc(d.desc)}, order VPS-88213</dd><dt>Method</dt><dd>Lightning through ${d.node}. No network fee to the customer.</dd><dt>Reference</dt><dd><code>${d.id}</code>, invoice ${d.invoiceRef}</dd><dt>Created by</dt><dd>API key pk_live_4f9a</dd><dt>Webhooks</dt><dd>2 of 2 delivered, the slowest in 214 ms</dd></dl>
<div class="actions"><a href="#">Refund this payment</a><a href="#">Download receipt</a><a href="#">Copy pay link</a></div>
<h2>Sent to your servers</h2>
<table class="ledger">${d.webhooks.map(w => `<tr><td><span>${w.event} to ${w.url}</span></td><td class="pos">${w.code} in ${w.ms} ms</td></tr>`).join("")}</table></div>
<aside class="margin"><h3>What happened</h3><ol class="log" style="list-style:none;padding:0;margin-top:14px">${log}</ol><h3>Metadata</h3><p style="margin:4px 0 0;overflow-wrap:anywhere">order ${d.metadata.order}<br>plan ${d.metadata.plan}</p></aside>`;
}

writeFileSync(out + "style.css", css);
writeFileSync(out + "overview.html", shell("Statement", "Overview", overview()));
writeFileSync(out + "payments.html", shell("Payments", "Payments", paymentsPage()));
writeFileSync(out + "payment.html", shell("Receipt", "Payments", paymentPage()));
