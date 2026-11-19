import { writeFileSync } from "node:fs";
import { payments, detail, eur, sats, wallets, available, settling, payouts, ledger, openingToday, money, signed } from "./data2";
import { statusWord, methods } from "./data";

const out = new URL("../round2/e/", import.meta.url).pathname;

const css = `
@font-face{font-family:Plex;font-weight:300;src:url(fonts/plex-300.woff2) format("woff2")}
@font-face{font-family:Plex;font-weight:400;src:url(fonts/plex-400.woff2) format("woff2")}
@font-face{font-family:Plex;font-weight:500;src:url(fonts/plex-500.woff2) format("woff2")}
@font-face{font-family:Plex;font-weight:600;src:url(fonts/plex-600.woff2) format("woff2")}
:root{--page:#fbfbfc;--side:#f1f2f5;--ink:#1e2130;--muted:#646a7d;--faint:#9ba0ae;--line:#e3e5ea;--blue:#1f3a8a;--b2:#3b5bb5;--b3:#7d95d6;--b4:#b7c5ea;--b5:#dfe5f6;--ok:#16794a;--warn:#9a5b00;--bad:#b42318}
*{box-sizing:border-box}
body{margin:0;background:var(--page);color:var(--ink);font:400 15px/1.5 Plex,system-ui,sans-serif;font-variant-numeric:tabular-nums;display:grid;grid-template-columns:248px minmax(0,1fr);min-height:100vh}
a{color:inherit;text-decoration:none}
:focus-visible{outline:2px solid var(--b2);outline-offset:2px}
small{font-size:.62em;font-weight:inherit}
.side{background:var(--side);padding:18px 14px;display:flex;flex-direction:column;gap:2px;border-right:1px solid var(--line)}
.org{display:flex;align-items:center;gap:10px;font-weight:600;padding:6px 10px 16px}
.org i{width:26px;height:26px;border-radius:50%;background:var(--blue);display:block}
.move{background:var(--blue);color:#fff;border-radius:8px;padding:9px 12px;font-weight:500;text-align:center;margin:0 4px 18px}
.side nav a{display:flex;justify-content:space-between;padding:7px 10px;border-radius:6px;color:#3b4052}
.side nav a:hover{background:#e6e8ee}
.side nav a[aria-current]{background:#fff;color:var(--ink);font-weight:500;box-shadow:0 0 0 1px var(--line)}
.side nav a span{color:var(--faint)}
.side h3{font-size:13px;font-weight:500;color:var(--muted);margin:22px 10px 6px}
.wl a{display:grid;grid-template-columns:1fr auto;padding:6px 10px;border-radius:6px;font-size:14px}
.wl a:hover{background:#e6e8ee}
.wl b{font-weight:400}.wl span{color:var(--muted)}
.side .me{margin-top:auto;padding:12px 10px 0;color:var(--muted);font-size:14px;border-top:1px solid var(--line)}
main{padding:36px 56px 80px;max-width:1180px;width:100%}
.topline{display:flex;justify-content:space-between;align-items:center;margin-bottom:28px;color:var(--muted)}
.acts{display:flex;gap:8px}
.btn{border:1px solid var(--line);background:#fff;border-radius:8px;padding:6px 14px;font-weight:500;color:var(--ink);font-size:14px}
.btn.p{background:var(--blue);border-color:var(--blue);color:#fff}
h1{font-size:15px;font-weight:500;color:var(--muted);margin:0}
.bal{font-size:56px;font-weight:300;letter-spacing:-.02em;line-height:1.1;margin:4px 0 6px}
.bal small{color:var(--muted)}
.balmeta{display:flex;gap:28px;color:var(--muted);flex-wrap:wrap}
.balmeta b{color:var(--ink);font-weight:500}
.hero{display:grid;grid-template-columns:minmax(0,1fr) 440px;gap:56px;margin-bottom:48px;align-items:start}
.comp{display:flex;height:10px;border-radius:5px;overflow:hidden;margin:28px 0 0;gap:2px}
.comp i{display:block}
.wt td{padding:9px 10px;font-size:14px}
.wt td b{font-weight:500;font-size:15px}
.sw{display:inline-block;width:10px;height:10px;border-radius:2px;margin-right:10px;vertical-align:0}
.grid{display:grid;grid-template-columns:minmax(0,1fr) 300px;gap:56px}
h2{font-size:17px;font-weight:500;margin:0 0 4px}
.hint{color:var(--muted);margin:0 0 14px;font-size:14px}
table{width:100%;border-collapse:collapse}
th{font-weight:400;color:var(--muted);font-size:13.5px;text-align:left;padding:8px 10px;border-bottom:1px solid var(--line)}
td{padding:11px 10px;border-bottom:1px solid var(--line);vertical-align:top}
th:first-child,td:first-child{padding-left:0}
th:last-child,td:last-child{padding-right:0}
.r{text-align:right;white-space:nowrap}
.in{color:var(--ok)}.outv{color:var(--bad)}
td .sub{display:block;color:var(--muted);font-size:13.5px}
.open td{color:var(--muted);border-bottom:0}
.payout{border-left:2px solid var(--blue);padding:2px 0 2px 18px;margin-bottom:28px}
.payout .amt{font-size:28px;font-weight:300;letter-spacing:-.01em;line-height:1.2}
.payout p{margin:4px 0 0;color:var(--muted);font-size:14px}
.payout a{color:var(--blue);font-weight:500}
.past{list-style:none;margin:0 0 28px;padding:0;font-size:14px}
.past li{display:flex;justify-content:space-between;padding:7px 0;border-bottom:1px solid var(--line)}
.past span{color:var(--muted)}
.rules{font-size:14px;color:var(--muted);margin:0;padding:0;list-style:none}
.rules li{padding:6px 0;border-bottom:1px solid var(--line);display:flex;justify-content:space-between;gap:12px}
.rules b{font-weight:400;color:var(--ink)}
.status{font-size:14px}
.status.paid{color:var(--ok)}.status.pending{color:var(--warn)}.status.expired{color:var(--faint)}.status.failed,.status.refunded,.status.disputed{color:var(--bad)}
.day td{background:none;color:var(--muted);font-size:13.5px;padding-top:22px;border-bottom:1px solid var(--ink)}
.day td b{color:var(--ink);font-weight:500;font-size:15px;margin-right:10px}
.chips{display:flex;gap:6px;flex-wrap:wrap;margin:0 0 18px}
.chips a{border:1px solid var(--line);background:#fff;border-radius:999px;padding:4px 12px;font-size:14px;color:#3b4052}
.chips a[aria-current]{background:var(--ink);border-color:var(--ink);color:#fff}
.sumline{display:flex;gap:28px;margin:0 0 20px;flex-wrap:wrap;color:var(--muted)}
.sumline b{display:block;color:var(--ink);font-weight:300;font-size:26px;line-height:1.2}
.when{color:var(--muted);font-size:14px}
.detail{max-width:760px}
.back{color:var(--muted);font-size:14px}
.dh{margin:14px 0 6px;display:flex;align-items:baseline;gap:16px;flex-wrap:wrap}
.dh .bal{font-size:48px;margin:0}
.flow{margin:28px 0 40px}
.flow td{font-size:15px}
.flow .tot td{border-bottom:0;border-top:1px solid var(--ink);font-weight:500}
.cols2{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:48px}
.hist{list-style:none;margin:0;padding:0;font-size:14px}
.hist li{display:grid;grid-template-columns:70px minmax(0,1fr);gap:12px;padding:8px 0;border-bottom:1px solid var(--line)}
.hist span{overflow-wrap:anywhere}
.hist time{color:var(--muted)}
.dl div{display:grid;grid-template-columns:120px minmax(0,1fr);gap:12px;padding:8px 0;border-bottom:1px solid var(--line);font-size:14px}
.dl dt{color:var(--muted)}.dl dd{margin:0;overflow-wrap:anywhere}
.mtop{display:none}
@media(max-width:860px){
body{display:block}
.side{display:none}
.mtop{display:flex;justify-content:space-between;align-items:center;padding:14px 16px;border-bottom:1px solid var(--line);background:var(--side)}
.mtop b{font-weight:600}.mtop span{color:var(--muted);font-size:14px}
main{padding:22px 16px 60px}
.topline{margin-bottom:18px}.topline .acts .btn:not(.p){display:none}
.bal{font-size:42px}
.hero{grid-template-columns:minmax(0,1fr);gap:28px;margin-bottom:36px}
.sumline{gap:8px 20px}.sumline b{font-size:21px}
.grid,.cols2{grid-template-columns:minmax(0,1fr)}
.grid{gap:8px}
.hide{display:none}
.dh .bal{font-size:40px}
}
`;

const shades = ["#1f3a8a", "#3b5bb5", "#7d95d6", "#b7c5ea", "#dfe5f6"];
const nav = [["Home", "overview"], ["Payments", "payments"], ["Customers", "#"], ["Subscriptions", "#"], ["Payouts", "#"], ["Providers", "#"], ["Developers", "#"], ["Settings", "#"]];

function shell(title: string, current: string, body: string): string {
  const n = nav.map(([l, h]) => `<a href="${h}.html"${l === current ? ' aria-current="page"' : ""}>${l}${l === "Payments" ? "<span>291</span>" : ""}</a>`).join("");
  const wl = wallets.map(w => `<a href="#"><b>${w.name}</b><span>${eur(w.avail)}</span></a>`).join("");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${title}</title><link rel="stylesheet" href="style.css"></head><body>
<aside class="side"><div class="org"><i></i>Northwind Hosting</div><a class="move" href="#">Move money</a><nav aria-label="Project">${n}</nav><h3>Wallets</h3><div class="wl">${wl}</div><div class="me">sam@northwind.example</div></aside>
<div class="mtop"><b>Northwind Hosting</b><span>Menu</span></div><main>${body}</main></body></html>`;
}

function overview() {
  const total = available + settling;
  const comp = wallets.map((w, i) => `<i style="flex:${w.avail};background:${shades[i]}" title="${w.name}"></i>`).join("");
  const wrows = wallets.map((w, i) => `<tr><td><span class="sw" style="background:${shades[i]}"></span>${w.name}<span class="sub" style="padding-left:20px">${w.where}</span></td><td class="r"><b>${eur(w.avail)}</b><span class="sub">${w.settling ? `${eur(w.settling)} settling` : "Nothing settling"}</span></td></tr>`).join("");
  const rows = ledger.map(r => `<tr><td class="hide">${r.t}</td><td>${r.what}<span class="sub">${r.who}, ${r.wallet}</span></td><td class="r ${r.cents < 0 ? "outv" : "in"}">${signed(r.cents)}</td><td class="r">${eur(r.balance)}</td></tr>`).join("");
  const past = payouts.past.map(p => `<li>${p.date}<span>${eur(p.cents)}, ${p.state.toLowerCase()}</span></li>`).join("");
  return `<div class="topline"><span>Wednesday 18 November, live</span><div class="acts"><a class="btn" href="#">Add funds</a><a class="btn p" href="#">Withdraw</a></div></div>
<div class="hero"><div><h1>Available to withdraw</h1>
<div class="bal">${money(available)}</div>
<div class="balmeta"><span><b>${eur(settling)}</b> settling</span><span><b>${eur(total)}</b> total</span></div>
<div class="balmeta" style="margin-top:4px"><span><b>${sats(available)}</b> sats at 1,112 per euro</span></div>
<div class="comp" role="img" aria-label="Available balance by wallet">${comp}</div></div>
<table class="wt"><thead><tr><th>Wallet</th><th class="r">Available</th></tr></thead><tbody>${wrows}</tbody></table></div>
<div class="grid"><section><h2>Today's balance changes</h2><p class="hint">Every payment, refund and release, with the balance after it.</p>
<table><thead><tr><th class="hide">Time</th><th>Entry</th><th class="r">Amount</th><th class="r">Balance</th></tr></thead><tbody>${rows}<tr class="open"><td class="hide">00:00</td><td>Opening balance</td><td></td><td class="r">${eur(openingToday)}</td></tr></tbody></table></section>
<aside><h2>Next payout</h2><div class="payout"><div class="amt">${eur(payouts.next.cents)}</div><p>${payouts.next.date} to ${payouts.next.to}</p><p>${payouts.next.schedule}. <a href="#">Change</a></p></div>
<h2>Recent payouts</h2><ul class="past" style="margin-top:8px">${past}</ul>
<h2>When money becomes available</h2><ul class="rules" style="margin-top:8px"><li><b>Lightning</b>Instantly</li><li><b>On-chain</b>After 3 confirmations</li><li><b>Bank transfer</b>Next working day</li><li><b>Card, PayPal</b>After 7 days</li></ul></aside></div>`;
}

const when = (p: (typeof payments)[number]): string => {
  if (p.status !== "paid") return p.status === "pending" ? "When paid" : "";
  const m: Record<string, string> = { lightning: "Available", onchain: "Available", bank: "Available 19 Nov", card: "Available 25 Nov", paypal: "Available 25 Nov" };
  return p.day === "Yesterday" && (p.method === "lightning" || p.method === "onchain" || p.method === "bank") ? "Available" : m[p.method];
};

function paymentsPage() {
  const groups = ["Today", "Yesterday"].map(d => {
    const rows = payments.filter(p => p.day === d);
    const paid = rows.filter(p => p.status === "paid").reduce((a, p) => a + p.cents, 0);
    const tr = rows.map(p => `<tr><td><a href="payment.html">${p.desc}</a><span class="sub">${p.customer}, ${methods[p.method]}, ${p.time}</span></td><td><span class="status ${p.status}">${statusWord[p.status]}</span></td><td class="when hide">${when(p)}</td><td class="r">${eur(p.cents)}<span class="sub">${sats(p.cents)} sats</span></td></tr>`).join("");
    return `<tr class="day"><td colspan="4"><b>${d === "Today" ? "Today, 18 November" : "Yesterday, 17 November"}</b>${eur(paid)} paid</td></tr>${tr}`;
  }).join("");
  return `<div class="topline"><span>Payments</span><div class="acts"><a class="btn" href="#">Export CSV</a><a class="btn p" href="#">Request payment</a></div></div>
<div class="sumline"><span><b>${eur(1423640)}</b>paid this month</span><span><b>${eur(142000)}</b>waiting for customers</span><span><b>${eur(38200)}</b>refunded</span></div>
<div class="chips"><a href="#" aria-current="true">All</a><a href="#">Paid</a><a href="#">Waiting</a><a href="#">Refunded</a><a href="#">Disputed</a><a href="#">Lightning</a><a href="#">On-chain</a><a href="#">Card</a><a href="#">More filters</a></div>
<table><thead><tr><th>Payment</th><th>Status</th><th class="hide">In your balance</th><th class="r">Amount</th></tr></thead><tbody>${groups}</tbody></table>`;
}

function paymentPage() {
  const d = detail;
  const hist = d.events.map(e => `<li><time>${e.t.slice(0, 5)}</time><span>${e.note}</span></li>`).join("");
  return `<div class="detail"><a class="back" href="payments.html">Payments</a>
<div class="dh"><div class="bal">${money(d.cents)}</div><span class="status paid">Paid 18 November at 21:46</span></div>
<div style="color:var(--muted)">${d.desc} for ${d.customer}</div>
<div class="acts" style="margin-top:18px"><a class="btn" href="#">Refund</a><a class="btn" href="#">Send receipt</a><a class="btn" href="#">Copy pay link</a></div>
<section class="flow"><h2>Where the money went</h2><p class="hint">This payment is in your Lightning wallet and can be withdrawn now.</p>
<table><tbody><tr><td>Customer paid</td><td class="r">${sats(d.cents)} sats</td><td class="r">${eur(d.cents)}</td></tr>
<tr><td>Routing fee</td><td class="r">0 sats</td><td class="r">${eur(0)}</td></tr>
<tr><td>Gateway fee</td><td class="r">0 sats</td><td class="r">${eur(0)}</td></tr>
<tr class="tot"><td>Added to Lightning wallet</td><td class="r">${sats(d.cents)} sats</td><td class="r">${eur(d.cents)}</td></tr></tbody></table></section>
<div class="cols2"><section><h2>History</h2><ul class="hist" style="margin-top:10px">${hist}</ul></section>
<section><h2>Details</h2><dl class="dl" style="margin-top:10px"><div><dt>Payment ID</dt><dd>${d.id}</dd></div><div><dt>Invoice</dt><dd>${d.invoiceRef}</dd></div><div><dt>Customer email</dt><dd>${d.metadata.customer_email}</dd></div><div><dt>Order</dt><dd>${d.metadata.order}</dd></div><div><dt>Plan</dt><dd>${d.metadata.plan}</dd></div><div><dt>Created by</dt><dd>API key pk_live_4f9a</dd></div><div><dt>Webhooks</dt><dd>2 of 2 delivered</dd></div></dl></section></div></div>`;
}

writeFileSync(out + "style.css", css);
writeFileSync(out + "overview.html", shell("Home", "Home", overview()));
writeFileSync(out + "payments.html", shell("Payments", "Payments", paymentsPage()));
writeFileSync(out + "payment.html", shell("Payment pay_7Q4M2X", "Payments", paymentPage()));
