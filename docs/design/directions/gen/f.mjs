import { writeFileSync } from "node:fs";
import { payments, detail, eur, sats, queue, fyi, health, wallets, available, settling, paidToday } from "./data2.mjs";
import { statusWord, methods } from "./data.mjs";

const out = new URL("../round2/f/", import.meta.url).pathname;

const css = `
@font-face{font-family:SS3;font-weight:400;src:url(fonts/ss3-400.woff2) format("woff2")}
@font-face{font-family:SS3;font-weight:600;src:url(fonts/ss3-600.woff2) format("woff2")}
@font-face{font-family:SS3;font-weight:700;src:url(fonts/ss3-700.woff2) format("woff2")}
:root{--canvas:#f6f7f8;--surface:#fff;--ink:#23252b;--muted:#6c707a;--faint:#9a9ea7;--line:#e3e5e8;--hover:#f2f4f7;--accent:#2f5bd3;--bad:#c42b1c;--badb:#fdecea;--warn:#9a6700;--warnb:#fff4d6;--ok:#1a7f37;--okb:#e6f4ea}
*{box-sizing:border-box}
body{margin:0;background:var(--canvas);color:var(--ink);font:400 14px/1.45 SS3,system-ui,sans-serif;font-variant-numeric:tabular-nums}
a{color:inherit;text-decoration:none}
:focus-visible{outline:2px solid var(--accent);outline-offset:1px}
.hdr{height:48px;background:var(--surface);border-bottom:1px solid var(--line);display:flex;align-items:center;gap:22px;padding:0 20px;position:sticky;top:0;z-index:2}
.hdr b{font-weight:700;white-space:nowrap}
.hdr b span{font-weight:400;color:var(--muted);margin-left:8px}
.hdr nav{display:flex;gap:2px;overflow-x:auto;scrollbar-width:none}
.hdr nav a{padding:5px 10px;border-radius:5px;color:var(--muted);white-space:nowrap}
.hdr nav a:hover{background:var(--hover);color:var(--ink)}
.hdr nav a[aria-current]{color:var(--ink);background:var(--hover);font-weight:600}
.hdr nav a i{font-style:normal;color:var(--bad);font-weight:700;margin-left:5px}
.find{margin-left:auto;min-width:260px;height:30px;border:1px solid var(--line);border-radius:6px;display:flex;align-items:center;padding:0 10px;color:var(--faint);gap:8px;justify-content:space-between}
.find kbd{font:inherit;font-size:12px;color:var(--muted);border:1px solid var(--line);border-radius:4px;padding:0 5px}
.wrap{display:grid;grid-template-columns:minmax(0,1fr) 300px;gap:0;min-height:calc(100vh - 48px)}
.main{padding:20px 24px 60px;min-width:0}
.rail{background:var(--surface);border-left:1px solid var(--line);padding:20px}
h1{font-size:20px;font-weight:700;margin:0;letter-spacing:-.005em}
h2{font-size:14px;font-weight:700;margin:0}
.hrow{display:flex;align-items:baseline;gap:12px;margin-bottom:14px;flex-wrap:wrap}
.hrow .m{color:var(--muted)}
.hrow .sp{flex:1}
.btn{height:28px;padding:0 10px;border:1px solid var(--line);border-radius:6px;background:var(--surface);display:inline-flex;align-items:center;font-weight:600;font-size:13px;white-space:nowrap}
.btn.pri{background:var(--accent);border-color:var(--accent);color:#fff}
.sheet{background:var(--surface);border:1px solid var(--line);border-radius:8px;overflow:hidden}
.bulk{display:flex;align-items:center;gap:8px;padding:8px 12px;border-bottom:1px solid var(--line);background:#fafbfc;color:var(--muted);font-size:13px}
.bulk .sp{flex:1}
table{width:100%;border-collapse:collapse}
th{font-weight:600;color:var(--muted);font-size:12.5px;text-align:left;padding:6px 10px;border-bottom:1px solid var(--line);white-space:nowrap;background:#fafbfc}
td{padding:0 10px;height:36px;border-bottom:1px solid var(--line);white-space:nowrap}
tbody tr:hover td{background:var(--hover)}
td.cb,th.cb{width:34px;padding-right:0}
.box{display:inline-block;width:14px;height:14px;border:1.5px solid #b9bdc5;border-radius:3px;vertical-align:middle}
.box.on{background:var(--accent);border-color:var(--accent)}
.r{text-align:right}
td.amt{text-align:right;font-weight:600}
td.trunc{max-width:260px;overflow:hidden;text-overflow:ellipsis}
td.menu,th.menu{width:36px;text-align:center;color:var(--faint);position:sticky;right:0;background:var(--surface)}
th.menu{background:#fafbfc}
.grp td{height:34px;background:#fafbfc;font-weight:700}
.grp td span{font-weight:400;color:var(--muted);margin-left:8px}
.bar{display:inline-block;width:3px;height:14px;border-radius:2px;vertical-align:-2px;margin-right:8px;background:var(--faint)}
.bar.bad{background:var(--bad)}.bar.warn{background:#d39200}
.due.soon{color:var(--bad);font-weight:600}
.act{color:var(--accent);font-weight:600}
.m{color:var(--muted)}
.tag{display:inline-block;font-size:12.5px;font-weight:600;padding:0 6px;border-radius:4px;line-height:20px;background:#eef0f3;color:var(--muted)}
.tag.paid{background:var(--okb);color:var(--ok)}.tag.pending{background:var(--warnb);color:var(--warn)}.tag.failed,.tag.disputed,.tag.refunded{background:var(--badb);color:var(--bad)}
.fold td{color:var(--muted)}
.fyi{margin-top:24px}
.fyi ul{list-style:none;margin:8px 0 0;padding:0}
.fyi li{display:grid;grid-template-columns:96px minmax(0,1fr);gap:10px;padding:7px 0;border-bottom:1px solid var(--line)}
.fyi time{color:var(--muted)}
.rail section{margin-bottom:26px}
.rail h2{margin-bottom:8px}
.kv div{display:flex;justify-content:space-between;gap:10px;padding:5px 0;border-bottom:1px solid var(--line)}
.kv dt{color:var(--muted)}.kv dd{margin:0;font-weight:600;text-align:right}
.hl{list-style:none;margin:0;padding:0}
.hl li{display:grid;grid-template-columns:12px minmax(0,1fr);gap:4px 8px;padding:6px 0;border-bottom:1px solid var(--line)}
.hl i{width:8px;height:8px;border-radius:50%;background:var(--ok);margin-top:6px}
.hl i.warn{background:#d39200}
.hl small{grid-column:2;color:var(--muted);font-size:13px}
.views{display:flex;gap:2px;border-bottom:1px solid var(--line);margin-bottom:10px;overflow-x:auto;scrollbar-width:none}
.views a{padding:7px 12px 9px;color:var(--muted);white-space:nowrap;border-bottom:2px solid transparent;margin-bottom:-1px}
.views a[aria-current]{color:var(--ink);font-weight:600;border-color:var(--ink)}
.views a span{color:var(--faint);margin-left:5px}
.fbar{display:flex;gap:6px;align-items:center;margin-bottom:10px;flex-wrap:wrap}
.chip{height:26px;padding:0 9px;border:1px dashed #c8ccd3;border-radius:6px;display:inline-flex;align-items:center;color:var(--muted);font-size:13px}
.chip.set{border-style:solid;color:var(--ink);background:var(--surface)}
.chip.set b{font-weight:600;margin-left:4px}
.tot{margin-left:auto;color:var(--muted);font-size:13px}
.scroll{overflow-x:auto}
.mono{font-family:ui-monospace,"DejaVu Sans Mono",monospace;font-size:12.5px}
.detail{display:grid;grid-template-columns:180px minmax(0,1fr) 280px;gap:28px;padding:20px 24px 60px;align-items:start}
.toc{position:sticky;top:68px;font-size:13.5px}
.toc a{display:block;padding:4px 10px;border-left:2px solid var(--line);color:var(--muted)}
.toc a[aria-current]{border-color:var(--ink);color:var(--ink);font-weight:600}
.crumbs{color:var(--muted);margin-bottom:6px}
.dhead{display:flex;align-items:center;gap:12px;flex-wrap:wrap;margin-bottom:4px}
.dhead .amt{font-size:26px;font-weight:700;letter-spacing:-.01em}
.dsub{color:var(--muted);margin-bottom:16px}
.dsec{margin-bottom:18px}
.dsec>h2{margin:0 0 8px}
.facts{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:0 28px}
.facts div{display:grid;grid-template-columns:130px minmax(0,1fr);gap:8px;padding:6px 0;border-bottom:1px solid var(--line)}
.facts dt{color:var(--muted)}.facts dd{margin:0;overflow-wrap:anywhere}
.masked{color:var(--muted);letter-spacing:.04em}
.mob{display:none}
@media(max-width:1100px){.detail{grid-template-columns:minmax(0,1fr) 260px}.toc{display:none}}
@media(max-width:860px){
.hdr{gap:12px;padding:0 14px}.hdr nav,.find{display:none}.hdr .mb{margin-left:auto;color:var(--muted)}
.wrap{grid-template-columns:minmax(0,1fr)}
.rail{border-left:0;border-top:1px solid var(--line)}
.main{padding:16px 12px 40px}
.desk{display:none}
.mob{display:block}
.qlist{list-style:none;margin:0;padding:0}
.qlist li{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:2px 10px;padding:10px 12px;border-bottom:1px solid var(--line)}
.qlist li.g{display:block;background:#fafbfc;font-weight:700;padding:8px 12px}
.qlist .s{color:var(--muted);font-size:13px;grid-column:1/-1}
.qlist b{font-weight:600}
.qlist .amt{font-weight:600;text-align:right}
.detail{grid-template-columns:minmax(0,1fr);padding:16px 12px 40px}
.facts{grid-template-columns:minmax(0,1fr)}
.facts div{grid-template-columns:110px minmax(0,1fr)}
.tot{display:none}
.detail td{white-space:normal;height:auto;padding-top:7px;padding-bottom:7px}
.detail td.trunc{max-width:none;overflow-wrap:anywhere}
.detail .hm{display:none}
}
`;

const nav = [["Home", "overview", 6], ["Payments", "payments"], ["Customers"], ["Subscriptions"], ["Balance"], ["Payouts"], ["Providers"], ["Developers"], ["Settings"]];
function shell(title, current, body) {
  const n = nav.map(([l, h, c]) => `<a href="${h || "#"}.html"${l === current ? ' aria-current="page"' : ""}>${l}${c ? `<i>${c}</i>` : ""}</a>`).join("");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${title}</title><link rel="stylesheet" href="style.css"></head><body>
<header class="hdr"><b>Northwind Hosting<span>Live</span></b><nav aria-label="Project">${n}</nav><div class="find">Find a payment, customer or key<kbd>Ctrl K</kbd></div><span class="mb mob">Menu</span></header>${body}</body></html>`;
}

function overview() {
  const count = queue.reduce((a, g) => a + g.rows.length, 0);
  const body = queue.map(g => `<tr class="grp"><td class="cb"><span class="box"></span></td><td colspan="6"><span class="bar ${g.tone}"></span>${g.group}<span>${g.rows.length}</span></td><td class="menu"></td></tr>` + g.rows.map((r, i) => `<tr><td class="cb"><span class="box${g.group === "Paid the wrong amount or late" ? " on" : ""}"></span></td><td class="trunc"><a href="payment.html"><b style="font-weight:600">${r.desc}</b></a> <span class="m">${r.who}</span></td><td class="m">${r.method}</td><td class="trunc">${r.reason}</td><td class="due${r.dueSoon ? " soon" : " m"}">${r.due}</td><td class="amt">${r.cents == null ? "" : eur(r.cents)}</td><td><a class="act" href="#">${r.act}</a></td><td class="menu">\u22ef</td></tr>`).join("")).join("");
  const mob = queue.map(g => `<li class="g"><span class="bar ${g.tone}"></span>${g.group}</li>` + g.rows.map(r => `<li><b>${r.desc}</b><span class="amt">${r.cents == null ? "" : eur(r.cents)}</span><span class="s">${r.reason}. ${r.due}.</span><a class="act" href="#">${r.act}</a></li>`).join("")).join("");
  const fy = fyi.map(f => `<li><time>${f.t}</time><span>${f.text}</span></li>`).join("");
  const hl = health.map(h => `<li><i class="${h.state}"></i><span>${h.name}</span><small>${h.note}</small></li>`).join("");
  return `<div class="wrap"><div class="main">
<div class="hrow"><h1>Needs attention</h1><span class="m">${count} items, oldest first within each group</span><span class="sp"></span><a class="btn desk" href="#">Assign</a><a class="btn desk" href="#">Mark handled</a></div>
<div class="sheet desk"><div class="bulk"><span class="box on"></span>2 selected<span class="sp"></span><a class="btn" href="#">Accept both</a><a class="btn" href="#">Request the rest</a></div>
<div class="scroll"><table><thead><tr><th class="cb"></th><th>Payment</th><th>Method</th><th>Why it is here</th><th>Due</th><th class="r">Amount</th><th>Next step</th><th class="menu"></th></tr></thead><tbody>${body}
<tr class="fold"><td class="cb"></td><td colspan="7">9 payments waiting for the customer to pay, ${eur(142000)}. These need nothing from you yet. <a class="act" href="#">Show</a></td></tr></tbody></table></div></div>
<div class="sheet mob"><ul class="qlist">${mob}</ul></div>
<section class="fyi"><h2>For your information</h2><ul>${fy}</ul></section></div>
<aside class="rail"><section><h2>Funds</h2><dl class="kv"><div><dt>Available</dt><dd>${eur(available)}</dd></div><div><dt>Settling</dt><dd>${eur(settling)}</dd></div><div><dt>Next payout</dt><dd>${eur(2000000)}, Fri</dd></div></dl></section>
<section><h2>Today</h2><dl class="kv"><div><dt>Paid</dt><dd>${eur(paidToday)}</dd></div><div><dt>Payments</dt><dd>14</dd></div><div><dt>Success rate</dt><dd>85.7%</dd></div><div><dt>Webhooks delivered</dt><dd>41 of 45</dd></div></dl></section>
<section><h2>Providers</h2><ul class="hl">${hl}</ul></section></aside></div>`;
}

function paymentsPage() {
  const rows = payments.map(p => `<tr><td class="cb"><span class="box"></span></td><td><a href="payment.html">${p.id}</a></td><td class="trunc">${p.desc}</td><td class="m">${p.customer}</td><td class="m">${methods[p.method]}</td><td><span class="tag ${p.status}">${statusWord[p.status]}</span></td><td class="amt">${eur(p.cents)}</td><td class="r m">${sats(p.cents)}</td><td class="m">${p.day === "Today" ? "18 Nov" : "17 Nov"} ${p.time}</td><td class="menu">\u22ef</td></tr>`).join("");
  const mob = payments.slice(0, 14).map(p => `<li><b>${p.desc}</b><span class="amt">${eur(p.cents)}</span><span class="s">${p.id}, ${p.customer}, ${p.time}</span><span><span class="tag ${p.status}">${statusWord[p.status]}</span></span></li>`).join("");
  return `<div class="main" style="max-width:none">
<div class="hrow"><h1>Payments</h1><span class="sp"></span><a class="btn desk" href="#">Export</a><a class="btn pri" href="#">New payment</a></div>
<nav class="views" aria-label="Saved views"><a href="#" aria-current="true">All<span>291</span></a><a href="#">Waiting<span>9</span></a><a href="#">Unusual<span>3</span></a><a href="#">Refunded<span>6</span></a><a href="#">Disputed<span>1</span></a><a href="#">Large, over \u20ac100<span>38</span></a><a href="#">Add view</a></nav>
<div class="fbar"><span class="chip set">Date<b>Last 7 days</b></span><span class="chip">Method</span><span class="chip">Customer</span><span class="chip">Amount</span><span class="chip">Metadata</span><span class="tot">18 of 291, ${eur(81600)} paid</span></div>
<div class="sheet desk"><div class="scroll"><table><thead><tr><th class="cb"><span class="box"></span></th><th>ID</th><th>Description</th><th>Customer</th><th>Method</th><th>Status</th><th class="r">Amount</th><th class="r">Sats</th><th>Created</th><th class="menu"></th></tr></thead><tbody>${rows}</tbody></table></div></div>
<div class="sheet mob"><ul class="qlist">${mob}</ul></div></div>`;
}

function paymentPage() {
  const d = detail;
  const life = [["Open", "21:44:02", "Created by API key pk_live_4f9a"], ["Paid", "21:46:11", "Lightning attempt att_91K2 settled"], ["Webhooks sent", "21:46:12", "2 of 2 endpoints answered 200"]];
  const lifeRows = life.map(l => `<tr><td><span class="tag ${l[0] === "Open" ? "" : "paid"}">${l[0]}</span></td><td class="m hm">18 Nov ${l[1]}</td><td>${l[2]}</td></tr>`).join("");
  const ev = d.events.map(e => `<tr><td class="m">${e.t}</td><td class="mono hm">${e.name}</td><td class="trunc" style="max-width:340px">${e.note}</td></tr>`).join("");
  const wh = d.webhooks.map(w => `<tr><td class="trunc">${w.url}</td><td>${w.event}</td><td><span class="tag paid">${w.code}</span></td><td class="r m">${w.ms} ms</td><td class="m">${w.at}</td><td class="menu">\u22ef</td></tr>`).join("");
  const toc = ["Summary", "Lifecycle", "Attempts", "Events", "Webhooks", "Cost", "Customer", "Metadata"].map((s, i) => `<a href="#${s.toLowerCase()}"${i === 0 ? ' aria-current="true"' : ""}>${s}</a>`).join("");
  return `<div class="detail"><nav class="toc" aria-label="On this page">${toc}</nav><div>
<div class="crumbs"><a href="payments.html">Payments</a> / <span>${d.id}</span></div>
<div class="dhead" id="summary"><span class="amt">${eur(d.cents)}</span><span class="tag paid">Paid</span><span class="sp" style="flex:1"></span><a class="btn desk" href="#">Copy link</a><a class="btn desk" href="#">Resend webhooks</a><a class="btn" href="#">Refund</a></div>
<div class="dsub">${d.desc}, ${d.customer}, paid over Lightning on 18 Nov at 21:46</div>
<section class="dsec" id="lifecycle"><h2>Lifecycle</h2><div class="sheet"><table><tbody>${lifeRows}</tbody></table></div></section>
<section class="dsec" id="attempts"><h2>Attempts</h2><div class="sheet scroll"><table><thead><tr><th>Attempt</th><th>Method</th><th>Status</th><th class="hm">Reference</th><th class="r">Amount</th></tr></thead><tbody><tr><td>att_91K2</td><td>Lightning</td><td><span class="tag paid">Paid</span></td><td class="trunc hm">lnbc200160n1p3x...</td><td class="amt">${sats(d.cents)} sats</td></tr></tbody></table></div></section>
<section class="dsec" id="events"><h2>Events</h2><div class="sheet scroll"><table><thead><tr><th>Time</th><th class="hm">Event</th><th>Detail</th></tr></thead><tbody>${ev}</tbody></table></div></section>
<section class="dsec" id="webhooks"><h2>Webhooks</h2><div class="sheet scroll"><table><thead><tr><th>Endpoint</th><th>Event</th><th>Response</th><th class="r">Time</th><th>Sent</th><th class="menu"></th></tr></thead><tbody>${wh}</tbody></table></div></section>
<section class="dsec" id="cost"><h2>Cost</h2><dl class="facts"><div><dt>Amount</dt><dd>${eur(d.cents)}</dd></div><div><dt>Routing fee</dt><dd>0 sats</dd></div><div><dt>Gateway fee</dt><dd>${eur(0)}</dd></div><div><dt>Net</dt><dd><b>${eur(d.cents)}</b></dd></div></dl></section>
<section class="dsec" id="customer"><h2>Customer</h2><dl class="facts"><div><dt>Customer</dt><dd>${d.customer}</dd></div><div><dt>Email</dt><dd class="masked">am\u2022\u2022\u2022\u2022@northwind.example</dd></div><div><dt>Pay page from</dt><dd>Dublin, IE</dd></div><div><dt>IP address</dt><dd class="masked">Hidden for your role</dd></div></dl></section>
<section class="dsec" id="metadata"><h2>Metadata</h2><dl class="facts"><div><dt>order</dt><dd>${d.metadata.order}</dd></div><div><dt>plan</dt><dd>${d.metadata.plan}</dd></div></dl></section></div>
<aside class="rail" style="border:1px solid var(--line);border-radius:8px"><section><h2>Properties</h2><dl class="kv"><div><dt>ID</dt><dd>${d.id}</dd></div><div><dt>Invoice</dt><dd>${d.invoiceRef}</dd></div><div><dt>Node</dt><dd>${d.node}</dd></div><div><dt>Created</dt><dd>21:44:02</dd></div><div><dt>Time to pay</dt><dd>2 min 9 s</dd></div><div><dt>Expires</dt><dd>Not applicable</dd></div></dl></section>
<section><h2>Related</h2><dl class="kv"><div><dt>Other payments by customer</dt><dd><a class="act" href="#">4</a></dd></div><div><dt>Subscription</dt><dd><a class="act" href="#">sub_2KQ8</a></dd></div></dl></section>
<section style="margin:0"><h2>Notes</h2><p class="m" style="margin:0">Add a note for your team. Notes are not shown to the customer.</p></section></aside></div>`;
}

writeFileSync(out + "style.css", css);
writeFileSync(out + "overview.html", shell("Needs attention", "Home", overview()));
writeFileSync(out + "payments.html", shell("Payments", "Payments", paymentsPage()));
writeFileSync(out + "payment.html", shell("Payment pay_7Q4M2X", "Payments", paymentPage()));
