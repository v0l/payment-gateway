import { writeFileSync } from "node:fs";
import { payments, detail, month, eur, sats, today, paidToday, hourlyToday, hourlyLastWeek, monthFigures, activity, money } from "./data2.mjs";
import { statusWord } from "./data.mjs";

const out = new URL("../round2/d/", import.meta.url).pathname;

const css = `
@font-face{font-family:Geist;font-weight:400;src:url(fonts/geist-400.woff2) format("woff2")}
@font-face{font-family:Geist;font-weight:500;src:url(fonts/geist-500.woff2) format("woff2")}
@font-face{font-family:Geist;font-weight:600;src:url(fonts/geist-600.woff2) format("woff2")}
:root{--bg:#fff;--wash:#f7f7f8;--line:#e4e4e7;--line2:#d4d4d8;--ink:#1f1f24;--muted:#6b6b76;--faint:#a1a1aa;--ok:#1d7a46;--okb:#e7f3eb;--warn:#a35a00;--warnb:#fbf0df;--bad:#c3302f;--badb:#fbe9e8;--offb:#f0f0f2}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font:400 14px/1.5 Geist,system-ui,sans-serif;font-variant-numeric:tabular-nums}
a{color:inherit;text-decoration:none}
button{font:inherit;color:inherit}
:focus-visible{outline:2px solid #2563eb;outline-offset:2px}
small{font-size:.72em}
.top{border-bottom:1px solid var(--line)}
.bar{max-width:1320px;margin:0 auto;padding:0 32px;height:56px;display:flex;align-items:center;gap:16px}
.proj{display:flex;align-items:center;gap:10px;font-weight:600}
.proj i{width:22px;height:22px;border-radius:6px;background:var(--ink);display:block}
.mode{font-size:12px;font-weight:500;color:var(--ok);background:var(--okb);padding:1px 8px;border-radius:4px}
.search{margin-left:auto;width:340px;height:34px;border:1px solid var(--line);border-radius:8px;display:flex;align-items:center;justify-content:space-between;padding:0 10px;color:var(--faint);background:var(--bg)}
.search kbd{font:inherit;font-size:12px;border:1px solid var(--line);border-radius:4px;padding:0 6px;color:var(--muted)}
.me{width:30px;height:30px;border-radius:50%;background:var(--offb);display:grid;place-items:center;font-size:12px;font-weight:600;color:var(--muted)}
.tabs{max-width:1320px;margin:0 auto;padding:0 24px;display:flex;gap:4px;overflow-x:auto;scrollbar-width:none}
.tabs a{padding:10px 10px 12px;color:var(--muted);white-space:nowrap;border-bottom:2px solid transparent;margin-bottom:-1px}
.tabs a:hover{color:var(--ink)}
.tabs a[aria-current]{color:var(--ink);border-color:var(--ink);font-weight:500}
main{max-width:1320px;margin:0 auto;padding:32px 32px 80px}
h1{font-size:24px;font-weight:600;letter-spacing:-.015em;margin:0;line-height:1.25}
h2{font-size:15px;font-weight:600;margin:0}
.muted{color:var(--muted)}
.head{display:flex;align-items:flex-end;justify-content:space-between;gap:16px;margin-bottom:24px}
.btn{height:34px;padding:0 14px;border-radius:8px;border:1px solid var(--line2);background:var(--bg);font-weight:500;display:inline-flex;align-items:center;cursor:pointer}
.btn.primary{background:var(--ink);border-color:var(--ink);color:#fff}
.btns{display:flex;gap:8px}
.cols{display:grid;grid-template-columns:minmax(0,1fr) 360px;gap:56px}
.today{display:flex;align-items:baseline;gap:14px;flex-wrap:wrap}
.today .fig{font-size:30px;font-weight:600;letter-spacing:-.02em}
.up{color:var(--ok);font-weight:500}.down{color:var(--bad);font-weight:500}
.chart{display:block;width:100%;height:150px;margin:14px 0 4px}
.axis{display:flex;justify-content:space-between;color:var(--faint);font-size:12px;padding-bottom:28px;border-bottom:1px solid var(--line)}
.legend{display:flex;gap:18px;font-size:12.5px;color:var(--muted);margin-top:6px}
.legend span::before{content:"";display:inline-block;width:14px;height:0;border-top:2px solid var(--ink);vertical-align:middle;margin-right:6px}
.legend span.prev::before{border-top:2px dashed var(--faint)}
.sec{display:flex;align-items:center;justify-content:space-between;margin:28px 0 6px}
.seg{display:flex;border:1px solid var(--line);border-radius:8px;padding:2px}
.seg a{padding:3px 10px;border-radius:6px;color:var(--muted);font-size:13px}
.seg a[aria-current]{background:var(--offb);color:var(--ink);font-weight:500}
.feed{list-style:none;margin:0;padding:0}
.feed li{display:grid;grid-template-columns:52px 14px minmax(0,1fr) auto;gap:4px 12px;padding:12px 0;border-bottom:1px solid var(--line);align-items:baseline}
.feed time{color:var(--muted);font-size:13px}
.dot{width:8px;height:8px;border-radius:50%;background:var(--faint);display:inline-block;transform:translateY(-1px)}
.dot.paid{background:var(--ok)}.dot.pending{background:#d4890b}.dot.failed{background:var(--bad)}
.feed b{font-weight:500}
.feed .sub{grid-column:3;color:var(--muted);font-size:13px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.feed .meth{color:var(--muted);font-size:13px;grid-row:1;grid-column:4}
.side h2{margin-bottom:2px}
.figs{margin:12px 0 36px}
.figs div{display:grid;grid-template-columns:1fr auto;gap:0 12px;padding:11px 0;border-bottom:1px solid var(--line)}
.figs div:first-child{border-top:1px solid var(--line)}
.figs dt{color:var(--muted)}
.figs dd{margin:0;font-weight:600;text-align:right}
.figs .prev{grid-column:1/-1;font-size:12.5px;color:var(--faint);text-align:right}
.figs .prev span{margin-left:6px}
.todo{list-style:none;margin:10px 0 0;padding:0}
.todo li{padding:12px 0;border-bottom:1px solid var(--line)}
.todo li:first-child{border-top:1px solid var(--line)}
.todo .k{font-size:12.5px;font-weight:500}
.todo .k.bad{color:var(--bad)}.todo .k.warn{color:var(--warn)}
.todo p{margin:2px 0 6px}
.todo a{font-weight:500;text-decoration:underline;text-underline-offset:3px;text-decoration-color:var(--line2)}
.filters{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:12px}
.filters .btn{font-weight:400;color:var(--muted);height:32px}
.filters .btn b{color:var(--ink);font-weight:500;margin-left:5px}
.filters .q{flex:1;min-width:200px;max-width:320px;justify-content:flex-start;color:var(--faint)}
.summary{color:var(--muted);margin:0 0 8px}
.summary b{color:var(--ink);font-weight:500}
.split{display:grid;grid-template-columns:minmax(0,1fr) 380px;gap:0;border-top:1px solid var(--line)}
table{width:100%;border-collapse:collapse}
th{text-align:left;font-weight:500;color:var(--muted);font-size:13px;padding:10px 12px;border-bottom:1px solid var(--line);white-space:nowrap}
td{padding:11px 12px;border-bottom:1px solid var(--line);white-space:nowrap}
td.amt,th.amt{text-align:right;padding-right:20px}
td.amt{font-weight:600}
td.amt span{color:var(--faint);font-weight:400;margin-left:4px;font-size:12.5px}
td.desc{max-width:240px;overflow:hidden;text-overflow:ellipsis}
tbody tr:hover{background:var(--wash)}
tbody tr.sel{background:var(--wash);box-shadow:inset 2px 0 var(--ink)}
.pill{display:inline-block;font-size:12.5px;font-weight:500;padding:1px 8px;border-radius:4px;background:var(--offb);color:var(--muted)}
.pill.paid{background:var(--okb);color:var(--ok)}.pill.pending{background:var(--warnb);color:var(--warn)}.pill.failed,.pill.disputed,.pill.refunded{background:var(--badb);color:var(--bad)}
.panel{border-left:1px solid var(--line);padding:20px 24px}
.panel header{display:flex;justify-content:space-between;color:var(--muted);font-size:13px}
.panel .amt{font-size:28px;font-weight:600;letter-spacing:-.02em;margin:8px 0 2px}
.kv{margin:16px 0 0}
.kv div{display:grid;grid-template-columns:120px minmax(0,1fr);gap:12px;padding:8px 0;border-bottom:1px solid var(--line)}
.kv dt{color:var(--muted)}.kv dd{margin:0;overflow-wrap:anywhere}
.steps{list-style:none;margin:18px 0 0;padding:0}
.steps li{position:relative;padding:0 0 16px 22px}
.steps li::before{content:"";position:absolute;left:3px;top:7px;width:8px;height:8px;border-radius:50%;background:var(--ok)}
.steps li::after{content:"";position:absolute;left:6.5px;top:17px;bottom:-1px;width:1px;background:var(--line2)}
.steps li:last-child::after{display:none}
.steps li.grey::before{background:var(--faint)}
.steps small{display:block;font-size:12.5px;color:var(--muted)}
.link{font-weight:500;text-decoration:underline;text-underline-offset:3px;text-decoration-color:var(--line2)}
.crumb{color:var(--muted);margin-bottom:10px;display:block}
.dhead{display:flex;justify-content:space-between;align-items:flex-start;gap:20px;flex-wrap:wrap}
.dhead .amt{font-size:32px;font-weight:600;letter-spacing:-.02em;line-height:1.1;display:flex;align-items:center;gap:12px}
.dhead .amt span.cur{color:var(--faint);font-weight:400;font-size:20px}
.strip{display:flex;margin:24px 0 36px;border-top:1px solid var(--line);border-bottom:1px solid var(--line)}
.strip div{padding:12px 24px 12px 0;margin-right:24px;border-right:1px solid var(--line)}
.strip div:last-child{border:0}
.strip dt{color:var(--muted);font-size:13px}.strip dd{margin:0;font-weight:500}
.dcols{display:grid;grid-template-columns:minmax(0,1fr) 340px;gap:56px}
.dcols section{margin-bottom:36px}
.dcols h2{margin-bottom:12px}
.bd{max-width:440px}
.bd div{display:flex;justify-content:space-between;padding:7px 0;border-bottom:1px solid var(--line)}
.bd div.tot{font-weight:600;border-bottom:0;border-top:1px solid var(--ink)}
.json{background:var(--wash);border:1px solid var(--line);border-radius:8px;padding:12px 14px;font:12.5px/1.6 ui-monospace,"DejaVu Sans Mono",monospace;color:#3f3f46;white-space:pre;overflow:auto;margin:0}
.mob{display:none}
@media(max-width:860px){
.bar{padding:0 16px;height:52px}.search{width:auto;margin-left:auto;border:0;padding:0}.search span,.search kbd{display:none}.search::after{content:"Search";color:var(--muted)}
.tabs{padding:0 8px}
main{padding:20px 16px 60px}
.cols,.dcols,.split{grid-template-columns:minmax(0,1fr)}
.cols{gap:8px}
.panel{display:none}
.head{flex-wrap:wrap;align-items:flex-start}
.feed li{grid-template-columns:44px minmax(0,1fr) auto;gap:2px 10px}
.feed .dot{display:none}.feed .sub{grid-column:2/-1}.feed .meth{grid-column:3}
.desk{display:none}
.mob{display:block}
.rows{list-style:none;margin:0;padding:0}
.rows li{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:2px 12px;padding:12px 0;border-bottom:1px solid var(--line)}
.rows .a{font-weight:600;text-align:right}
.rows .d{color:var(--muted);font-size:13px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.strip{flex-wrap:wrap}.strip div{border:0;padding:8px 20px 8px 0;margin:0}
.filters .q{max-width:none;flex-basis:100%;white-space:nowrap;overflow:hidden}
}
`;

const tabs = ["Overview", "Payments", "Customers", "Subscriptions", "Balance", "Payouts", "Providers", "Developers", "Settings"];
function shell(title, current, body) {
  const t = tabs.map(n => `<a href="${n === "Overview" ? "overview" : n === "Payments" ? "payments" : "#"}.html"${n === current ? ' aria-current="page"' : ""}>${n}</a>`).join("");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${title}</title><link rel="stylesheet" href="style.css"></head><body>
<header class="top"><div class="bar"><a class="proj" href="overview.html"><i></i>Northwind Hosting</a><span class="mode">Live</span><div class="search"><span>Search payments, customers, keys</span><kbd>/</kbd></div><span class="me">SK</span></div>
<nav class="tabs" aria-label="Project">${t}</nav></header><main>${body}</main></body></html>`;
}

function chart() {
  const W = 800, H = 150, max = 70000;
  const pt = (arr) => arr.map((v, i) => `${((i / 23) * W).toFixed(1)},${(H - 4 - (v / max) * (H - 12)).toFixed(1)}`).join(" ");
  const last = hourlyToday.length - 1;
  const nx = ((last / 23) * W).toFixed(1), ny = (H - 4 - (hourlyToday[last] / max) * (H - 12)).toFixed(1);
  return `<svg class="chart" viewBox="0 0 ${W} ${H}" preserveAspectRatio="none" role="img" aria-label="Paid so far today against last Wednesday by hour">
<polyline points="${pt(hourlyLastWeek)}" fill="none" stroke="#a1a1aa" stroke-width="1.5" stroke-dasharray="4 4" vector-effect="non-scaling-stroke"/>
<polyline points="${pt(hourlyToday)}" fill="none" stroke="#1f1f24" stroke-width="2" vector-effect="non-scaling-stroke"/>
<line x1="${nx}" x2="${nx}" y1="0" y2="${H}" stroke="#e4e4e7" vector-effect="non-scaling-stroke"/>
<circle cx="${nx}" cy="${ny}" r="3.5" fill="#1f1f24"/></svg>`;
}

const st = { paid: "paid", pending: "pending", failed: "failed", muted: "" };

function overview() {
  const feed = activity.map(a => `<li><time>${a.t}</time><span class="dot ${st[a.kind]}"></span><b>${a.title}</b><span class="meth">${a.method}</span><span class="sub">${a.who}, ${a.what}</span></li>`).join("");
  const figs = monthFigures.map(f => `<div><dt>${f.label}</dt><dd>${f.value}</dd><span class="prev">${f.prev}<span class="${f.delta.startsWith("-") ? (f.label === "Refunded" ? "up" : "down") : "up"}">${f.delta}</span></span></div>`).join("");
  return `<div class="head"><div><h1>Today</h1><span class="muted">Wednesday 18 November, live mode</span></div><div class="btns desk"><a class="btn" href="#">Create payment link</a></div></div>
<div class="cols"><div>
<div class="today"><span class="muted">Paid so far</span><span class="fig">${money(paidToday)}</span><span class="muted">${eur(59000)} by this time last Wednesday</span><span class="up">+4.7%</span></div>
${chart()}<div class="legend"><span>Today</span><span class="prev">Last Wednesday</span></div><div class="axis"><span>00:00</span><span>06:00</span><span>12:00</span><span>18:00</span><span>23:59</span></div>
<div class="sec"><h2>Activity</h2><div class="seg"><a href="#" aria-current="true">All</a><a href="#">Payments</a><a href="#">Refunds</a><a href="#">Webhooks</a></div></div>
<ul class="feed">${feed}</ul>
<p style="margin:14px 0 0"><a class="link" href="payments.html">See all payments</a></p></div>
<aside class="side"><h2>This month</h2><span class="muted">1 to 18 November, against the same days in October</span><dl class="figs">${figs}</dl>
<h2>Needs action</h2><ul class="todo">
<li><span class="k bad">Dispute, respond by 24 Nov</span><p>${eur(14900)} from cus_yuki.t says the dedicated host was not received.</p><a href="payment.html">Respond</a></li>
<li><span class="k bad">Declined</span><p>${eur(2500)} top-up from cus_omar.f. The customer has not retried.</p><a href="#">Send a new link</a></li>
<li><span class="k warn">Waiting, 1 of 3 confirmations</span><p>${eur(5400)} on-chain from cus_elsa.v, seen at 18:44.</p><a href="#">View payment</a></li></ul></aside></div>`;
}

function paymentsPage() {
  const rows = payments.map((p, i) => `<tr${i === 0 ? ' class="sel"' : ""}><td class="amt">${eur(p.cents)}<span>EUR</span></td><td><span class="pill ${p.status}">${statusWord[p.status]}</span></td><td class="desc">${p.desc}</td><td class="muted">${p.customer}</td><td class="muted">${({ lightning: "Lightning", onchain: "On-chain", card: "Card", paypal: "PayPal", bank: "Bank transfer" })[p.method]}</td><td class="muted">${p.day === "Today" ? "18 Nov" : "17 Nov"}, ${p.time}</td></tr>`).join("");
  const mrows = payments.slice(0, 12).map(p => `<li><a href="payment.html"><b style="font-weight:500">${p.desc}</b></a><span class="a">${eur(p.cents)}</span><span class="d">${p.customer}, ${p.time}</span><span><span class="pill ${p.status}">${statusWord[p.status]}</span></span></li>`).join("");
  const d = detail;
  return `<div class="head"><h1>Payments</h1><div class="btns"><a class="btn" href="#">Export</a><a class="btn primary" href="#">Create payment</a></div></div>
<div class="filters"><span class="btn q">Search by ID, customer or description</span><span class="btn">Status<b>All</b></span><span class="btn">Method<b>All</b></span><span class="btn">Date<b>Last 7 days</b></span><span class="btn desk">Amount<b>Any</b></span></div>
<p class="summary"><b>291</b> payments this month. <b>${eur(81600)}</b> paid, <b>${eur(7200)}</b> waiting, <b>${eur(14900)}</b> refunded in this view.</p>
<div class="split desk"><table><thead><tr><th class="amt">Amount</th><th>Status</th><th>Description</th><th>Customer</th><th>Method</th><th>Date</th></tr></thead><tbody>${rows}</tbody></table>
<aside class="panel"><header><span>${d.id}</span><span>Close</span></header><div class="amt">${eur(d.cents)} <span class="pill paid" style="vertical-align:middle">Paid</span></div><span class="muted">${d.desc}</span>
<ol class="steps"><li>Paid over Lightning<small>18 Nov, 21:46:11, 2 min after creation</small></li><li>Webhooks delivered<small>2 of 2, slowest 214 ms</small></li><li class="grey">Created by API key pk_live_4f9a<small>18 Nov, 21:44:02</small></li></ol>
<dl class="kv"><div><dt>Customer</dt><dd>${d.customer}</dd></div><div><dt>Amount in sats</dt><dd>${sats(d.cents)}</dd></div><div><dt>Order</dt><dd>${d.metadata.order}</dd></div></dl>
<p style="margin:18px 0 0"><a class="link" href="payment.html">Open payment</a></p></aside></div>
<ul class="rows mob">${mrows}</ul>`;
}

function paymentPage() {
  const d = detail;
  const steps = [...d.events].reverse().map(e => `<li${e.state === "open" ? ' class="grey"' : ""}>${e.note}<small>18 Nov, ${e.t}</small></li>`).join("");
  const wh = d.webhooks.map(w => `<tr><td>${w.url}</td><td class="muted">${w.event}</td><td><span class="pill paid">${w.code}</span></td><td class="muted">${w.ms} ms</td><td class="muted">${w.at}</td></tr>`).join("");
  const json = `{
  "id": "${d.id}",
  "amount": ${d.cents},
  "currency": "EUR",
  "status": "paid",
  "method": "lightning",
  "paid_at": "2026-11-18T21:46:11Z",
  "metadata": {
    "order": "${d.metadata.order}",
    "plan": "${d.metadata.plan}"
  }
}`;
  return `<a class="crumb" href="payments.html">Payments</a>
<div class="dhead"><div><div class="amt">${eur(d.cents)}<span class="cur">EUR</span><span class="pill paid">Paid</span></div><div class="muted" style="margin-top:4px">${d.desc}</div></div><div class="btns"><a class="btn" href="#">Copy pay link</a><a class="btn" href="#">Refund</a></div></div>
<dl class="strip"><div><dt>Paid</dt><dd>18 Nov, 21:46</dd></div><div><dt>Customer</dt><dd>${d.customer}</dd></div><div><dt>Method</dt><dd>Lightning</dd></div><div><dt>Time to pay</dt><dd>2 min 10 s</dd></div><div><dt>Fee</dt><dd>${eur(0)}</dd></div></dl>
<div class="dcols"><div>
<section><h2>Timeline</h2><ol class="steps" style="margin-top:4px">${steps}</ol></section>
<section><h2>Breakdown</h2><div class="bd"><div><span>VPS Dublin-3, 1 month</span><span>${eur(1800)}</span></div><div><span class="muted">Network fee paid by you</span><span>${eur(0)}</span></div><div><span class="muted">Received in sats</span><span>${sats(d.cents)}</span></div><div class="tot"><span>Net</span><span>${eur(1800)}</span></div></div></section>
<section><h2>Webhook deliveries</h2><div style="overflow-x:auto"><table><thead><tr><th>Endpoint</th><th>Event</th><th>Response</th><th>Time</th><th>Sent</th></tr></thead><tbody>${wh}</tbody></table></div></section></div>
<aside><section><h2>Details</h2><dl class="kv" style="margin-top:0"><div><dt>ID</dt><dd>${d.id}</dd></div><div><dt>Invoice</dt><dd>${d.invoiceRef}</dd></div><div><dt>Node</dt><dd>${d.node}</dd></div><div><dt>Created by</dt><dd>API key pk_live_4f9a</dd></div><div><dt>Order</dt><dd>${d.metadata.order}</dd></div><div><dt>Plan</dt><dd>${d.metadata.plan}</dd></div></dl></section>
<section><h2>API object</h2><pre class="json">${json}</pre></section></aside></div>`;
}

writeFileSync(out + "style.css", css);
writeFileSync(out + "overview.html", shell("Overview", "Overview", overview()));
writeFileSync(out + "payments.html", shell("Payments", "Payments", paymentsPage()));
writeFileSync(out + "payment.html", shell("Payment pay_7Q4M2X", "Payments", paymentPage()));
