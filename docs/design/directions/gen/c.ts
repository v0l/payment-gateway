import { writeFileSync } from "node:fs";
import { project, payments, detail, month, attention, methods, eur, sats, esc, statusWord } from "./data";

const out = new URL("../c/", import.meta.url).pathname;

const css = `
@font-face{font-family:SG;font-weight:500;src:url(fonts/sg-500.woff2) format("woff2")}
@font-face{font-family:SG;font-weight:800;src:url(fonts/sg-800.woff2) format("woff2")}
@font-face{font-family:JB;font-weight:400;src:url(fonts/jb-400.woff2) format("woff2")}
:root{--ground:#eceef6;--ink:#181540;--soft:#5c5a85;--white:#fff;--lightning:#f7c52b;--onchain:#ff6b3d;--card:#3f5bff;--paypal:#0aa5c9;--bank:#1fa971;--bad:#c4234a}
*{box-sizing:border-box}
body{margin:0;background:var(--ground);color:var(--ink);font:500 15px/1.45 SG,system-ui,sans-serif}
a{color:inherit}
:focus-visible{outline:3px solid var(--ink);outline-offset:2px}
.hub{background:var(--ink);color:#fff;display:flex;align-items:center;gap:6px;padding:0 24px;height:60px}
.hub b{font-weight:800;font-size:17px;margin-right:20px;white-space:nowrap}
.hub a{text-decoration:none;padding:8px 14px;border-radius:99px;color:#c9c8ee;white-space:nowrap}
.hub a[aria-current]{background:var(--lightning);color:var(--ink);font-weight:800}
.hub .sp{flex:1}
.hub .who{color:#9c9bd0;font-size:13px}
.page{max-width:1360px;margin:0 auto;padding:28px 32px 80px}
h1{font-weight:800;font-size:44px;line-height:1;letter-spacing:-.03em;margin:0 0 6px}
.sub{color:var(--soft);margin:0 0 24px}
.tri{display:grid;grid-template-columns:repeat(3,1fr);gap:0;border:3px solid var(--ink);margin-bottom:32px;background:#fff}
.tri a{display:block;padding:16px 20px;text-decoration:none;border-right:3px solid var(--ink)}
.tri a:last-child{border:0}
.tri .tag{display:inline-block;font-weight:800;color:#fff;background:var(--bad);padding:1px 10px;border-radius:99px;font-size:13px;margin-bottom:6px}
.tri .tag.w{background:var(--lightning);color:var(--ink)}
.tri b{display:block;font-weight:800;font-size:17px;line-height:1.25}
.tri span{color:var(--soft);font-size:14px;display:block;margin-top:4px}
.tri em{font-style:normal;font-weight:800;text-decoration:underline;text-underline-offset:3px;display:block;margin-top:8px}
h2{font-weight:800;font-size:22px;letter-spacing:-.02em;margin:0 0 12px}
.pipe{display:grid;grid-template-columns:repeat(5,minmax(0,1fr));gap:12px;margin-bottom:36px}
.col{background:#fff;padding:12px;min-height:290px;border-top:8px solid var(--ink)}
.col.created{border-color:#b5b4d6}.col.waiting{border-color:var(--lightning)}.col.paid{border-color:var(--bank)}.col.refunded{border-color:var(--bad)}.col.expired{border-color:#8a89ad}
.col h3{margin:0;font-weight:800;font-size:15px;display:flex;justify-content:space-between}
.col .tot{font-size:26px;font-weight:800;letter-spacing:-.02em;margin:2px 0 10px}
.chip{display:grid;grid-template-columns:8px 1fr auto;gap:8px;align-items:center;padding:6px 0;border-top:1px solid #e3e4f0;font-size:13px;text-decoration:none}
.chip i{width:8px;height:22px;border-radius:2px;display:block}
.chip span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.m-lightning{background:var(--lightning)}.m-onchain{background:var(--onchain)}.m-card{background:var(--card)}.m-paypal{background:var(--paypal)}.m-bank{background:var(--bank)}
.lanes{background:#fff;padding:8px 20px}
.lane{display:grid;grid-template-columns:130px minmax(0,1fr) 190px;gap:16px;align-items:center;padding:10px 0;border-bottom:1px solid #e3e4f0}
.lane:last-child{border:0}
.lane b{font-weight:800}
.lane .trk{display:flex;gap:2px;align-items:flex-end;height:36px}
.lane .trk i{flex:1;display:block}
.lane .n{text-align:right}
.lane .n b{font-size:18px}
.filters{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:20px}
.filters a{border:3px solid var(--ink);padding:5px 14px;border-radius:99px;text-decoration:none;font-weight:800;background:#fff}
.filters a.on{background:var(--ink);color:#fff}
.band{background:#fff;margin-bottom:20px;display:grid;grid-template-columns:150px minmax(0,1fr)}
.band>header{padding:16px;color:var(--ink);display:flex;flex-direction:column;justify-content:space-between}
.band>header b{font-weight:800;font-size:19px;line-height:1.1}
.band>header span{font-size:14px}
.band.card>header{color:#fff}
.band.paypal>header,.band.onchain>header{color:var(--ink)}
.rows{list-style:none;margin:0;padding:0}
.rows li{display:grid;grid-template-columns:minmax(0,1.4fr) 110px minmax(120px,1fr) 110px;gap:16px;align-items:center;padding:10px 18px;border-bottom:1px solid #e3e4f0}
.rows li:last-child{border:0}
.rows li:hover{background:#f6f7fc}
.rows a{text-decoration:none;font-weight:800}
.rows small{display:block;color:var(--soft);font-weight:500;font-size:13px}
.len{height:10px;background:var(--ink);border-radius:0 5px 5px 0}
.rows .amt{text-align:right;font-weight:800;font-size:17px}
.badge{font-weight:800;font-size:13px;padding:2px 10px;border-radius:99px;border:2px solid var(--ink);display:inline-block;justify-self:start}
.badge.paid{background:var(--bank);border-color:var(--bank);color:#fff}
.badge.pending{background:var(--lightning);border-color:var(--lightning)}
.badge.failed,.badge.disputed,.badge.refunded{background:var(--bad);border-color:var(--bad);color:#fff}
.badge.expired{border-color:#a9a8cc;color:var(--soft)}
.journey{display:grid;grid-template-columns:repeat(5,1fr);margin:24px 0 28px}
.journey div{position:relative;padding-top:30px;font-size:14px;color:var(--soft)}
.journey div::before{content:"";position:absolute;left:0;right:0;top:11px;height:6px;background:var(--bank)}
.journey div::after{content:"";position:absolute;left:0;top:2px;width:24px;height:24px;border-radius:50%;background:var(--bank);border:4px solid var(--ground)}
.journey div.todo::before{background:#c9cbe0}.journey div.todo::after{background:#c9cbe0}
.journey b{display:block;color:var(--ink);font-weight:800}
.block{display:grid;grid-template-columns:minmax(0,1.2fr) minmax(0,1fr);gap:0;margin-bottom:28px}
.pay{background:var(--lightning);padding:28px 28px 24px;display:flex;flex-direction:column;justify-content:space-between;gap:30px}
.pay .amt{font-size:84px;font-weight:800;letter-spacing:-.05em;line-height:.9}
.pay .meth{font-weight:800;font-size:20px}
.pay .badge{background:var(--ink);color:#fff;border-color:var(--ink);font-size:15px;padding:3px 14px}
.facts{background:#fff;padding:8px 24px}
.facts div{display:grid;grid-template-columns:120px minmax(0,1fr);gap:12px;padding:9px 0;border-bottom:1px solid #e3e4f0;overflow-wrap:anywhere}
.facts div:last-child{border:0}
.facts dt{color:var(--soft)}.facts dd{margin:0}
.two{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px}
.sheet{background:#fff;padding:18px 20px}
.sheet li{list-style:none;padding:8px 0;border-bottom:1px solid #e3e4f0;font-size:14px;overflow-wrap:anywhere}
.sheet ul{margin:0;padding:0}
.sheet li:last-child{border:0}
.sheet time{font-family:JB,monospace;font-size:12px;color:var(--soft);margin-right:8px}
.act{display:flex;gap:10px;flex-wrap:wrap;margin:0 0 24px}
.act a{background:var(--ink);color:#fff;padding:8px 18px;border-radius:99px;text-decoration:none;font-weight:800}
.act a.o{background:none;color:var(--ink);border:3px solid var(--ink);padding:5px 18px}
.tabbar{display:none}
@media(max-width:820px){
.hub{height:52px;padding:0 16px}.hub a,.hub .sp,.hub .who{display:none}.hub b{margin:0}
.page{padding:20px 16px 96px}h1{font-size:34px}
.tri{grid-template-columns:1fr}.tri a{border-right:0;border-bottom:3px solid var(--ink)}
.pipe{display:flex;overflow-x:auto;scroll-snap-type:x mandatory;margin:0 -16px 28px;padding:0 16px}
.col{flex:0 0 76%;scroll-snap-align:start;min-height:240px}
.lane{grid-template-columns:90px minmax(0,1fr) 78px;gap:8px}.lane .n span{display:none}
.band{grid-template-columns:1fr}.band>header{flex-direction:row;align-items:baseline}
.rows li{grid-template-columns:minmax(0,1fr) auto;gap:4px 12px;padding:10px 14px}.len{display:none}.rows li .badge{grid-column:1;grid-row:2}.rows .amt{grid-row:1;grid-column:2}
.journey{grid-template-columns:repeat(5,1fr);font-size:11px}.journey b{font-size:12px}
.block,.two{grid-template-columns:1fr}.pay .amt{font-size:64px}
.facts div{grid-template-columns:96px 1fr}
.tabbar{display:grid;grid-template-columns:repeat(4,1fr);position:fixed;left:0;right:0;bottom:0;background:var(--ink);padding:8px 8px 10px}
.tabbar a{color:#c9c8ee;text-align:center;text-decoration:none;font-size:12px;padding:6px 0;border-radius:99px}
.tabbar a[aria-current]{background:var(--lightning);color:var(--ink);font-weight:800}
}
`;

const nav = ["Overview", "Payments", "Customers", "Subscriptions", "Balance", "Providers", "Keys", "Settings"];
function shell(title: string, current: string, body: string): string {
  const links = nav.map(n => `<a href="${n === "Overview" ? "overview" : n === "Payments" ? "payments" : "#"}.html"${n === current ? ' aria-current="page"' : ""}>${n}</a>`).join("");
  const tabs = ["Overview", "Payments", "Balance", "More"].map(n => `<a href="${n === "Overview" ? "overview" : n === "Payments" ? "payments" : "#"}.html"${n === current ? ' aria-current="page"' : ""}>${n}</a>`).join("");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${title}</title><link rel="stylesheet" href="style.css"></head><body>
<div class="hub"><b>${project.name}</b>${links}<span class="sp"></span><span class="who">sam@northwind.example</span></div>
<main class="page">${body}</main><nav class="tabbar" aria-label="Project">${tabs}</nav></body></html>`;
}

const chip = (p: (typeof payments)[number]) => `<a class="chip" href="payment.html"><i class="m-${p.method}"></i><span>${esc(p.desc)}</span><b>${eur(p.cents)}</b></a>`;
const sum = (rows: (typeof payments)[number][]) => rows.reduce((a: number, p) => a + p.cents, 0);

function overview() {
  const today = payments.filter(p => p.day === "Today");
  const cols: [string, string, (typeof payments)[number][]][] = [
    ["Waiting to be paid", "waiting", payments.filter(p => p.status === "pending")],
    ["Paid today", "paid", today.filter(p => p.status === "paid")],
    ["Refunded", "refunded", payments.filter(p => p.status === "refunded")],
    ["Disputed", "refunded", payments.filter(p => p.status === "disputed")],
    ["Expired or failed", "expired", payments.filter(p => p.status === "expired" || p.status === "failed")],
  ];
  const pipe = cols.map(([t, c, rows]) => `<section class="col ${c}"><h3>${t}<span>${rows.length}</span></h3><div class="tot">${eur(sum(rows))}</div>${rows.slice(0, 5).map(chip).join("")}</section>`).join("");
  const tri = attention.map((a, i) => `<a href="payment.html"><span class="tag ${i === 1 ? "w" : ""}">${a.kind === "dispute" ? "Dispute" : a.kind === "confirm" ? "Waiting" : "Declined"}</span><b>${esc(a.title)}</b><span>${esc(a.body)}</span><em>${a.cta}</em></a>`).join("");
  const lanes = month.share.map(s => {
    const bars = month.daily.map((v, i) => `<i class="m-${s.method}" style="height:${Math.max(3, Math.round((v / 940) * 36 * (s.pct / 46) * (0.75 + ((i * 7 + s.pct) % 5) / 10)))}px"></i>`).join("");
    return `<div class="lane"><b>${methods[s.method]}</b><div class="trk">${bars}</div><div class="n"><b>${eur(s.cents)}</b><span class="soft"> ${s.count} payments</span></div></div>`;
  }).join("");
  return `<h1>Right now</h1><p class="sub">${project.name}, live. ${payments.filter(p => p.status === "pending").length} payments waiting, ${eur(61800)} paid today.</p>
<div class="tri">${tri}</div>
<h2>Where payments are</h2><div class="pipe">${pipe}</div>
<h2>Where the money comes from, 1 to 18 November</h2><div class="lanes">${lanes}</div>`;
}

function paymentsPage() {
  const order = ["lightning", "onchain", "card", "paypal", "bank"];
  const bands = order.map(m => {
    const rows = payments.filter(p => p.method === m);
    const li = rows.map(p => `<li><div><a href="payment.html">${esc(p.desc)}</a><small>${p.id}, ${p.customer}, ${p.day === "Today" ? p.time : "yesterday " + p.time}</small></div><span class="badge ${p.status}">${statusWord[p.status]}</span><div class="len" style="width:${Math.max(4, (p.cents / 19200) * 100)}%"></div><span class="amt">${eur(p.cents)}</span></li>`).join("");
    return `<section class="band ${m}"><header class="m-${m}"><b>${methods[m]}</b><span>${rows.length} payments<br>${eur(sum(rows))}</span></header><ul class="rows">${li}</ul></section>`;
  }).join("");
  return `<h1>Payments</h1><p class="sub">Grouped by how the customer paid. 291 this month.</p>
<div class="filters"><a class="on" href="#">All</a><a href="#">Waiting</a><a href="#">Paid</a><a href="#">Refunded</a><a href="#">Disputed</a><a href="#">Last 7 days</a></div>${bands}`;
}

function paymentPage() {
  const d = detail;
  const steps = ["Created", "Invoice issued", "Customer paying", "Paid", "Webhooks sent"];
  const t = ["21:44:02", "21:44:02", "21:44:40", "21:46:11", "21:46:12"];
  const jr = steps.map((s, i) => `<div><b>${s}</b>${t[i]}</div>`).join("");
  const ev = d.events.map(e => `<li><time>${e.t.slice(0, 8)}</time>${esc(e.note)}</li>`).join("");
  const wh = d.webhooks.map(w => `<li><time>${w.at}</time>${w.event} to ${w.url}, answered ${w.code} in ${w.ms} ms</li>`).join("");
  return `<p class="sub" style="margin:0 0 8px"><a href="payments.html">Payments</a> / ${d.id}</p><h1>${esc(d.desc)}</h1><p class="sub">${d.customer}, order VPS-88213</p>
<div class="journey">${jr}</div>
<div class="block"><div class="pay"><div><span class="badge">Paid</span></div><div><div class="amt">${eur(d.cents)}</div><div class="meth">${sats(d.cents)} sats over Lightning</div></div></div>
<dl class="facts" style="margin:0"><div><dt>Reference</dt><dd>${d.id}</dd></div><div><dt>Invoice</dt><dd>${d.invoiceRef}</dd></div><div><dt>Node</dt><dd>${d.node}</dd></div><div><dt>Customer fee</dt><dd>${eur(0)}</dd></div><div><dt>Created by</dt><dd>API key pk_live_4f9a</dd></div><div><dt>Plan</dt><dd>${d.metadata.plan}</dd></div></dl></div>
<div class="act"><a href="#">Refund</a><a class="o" href="#">Download receipt</a><a class="o" href="#">Copy pay link</a></div>
<div class="two"><section class="sheet"><h2>What happened</h2><ul>${ev}</ul></section><section class="sheet"><h2>Sent to your servers</h2><ul>${wh}</ul></section></div>`;
}

writeFileSync(out + "style.css", css);
writeFileSync(out + "overview.html", shell("Lanes overview", "Overview", overview()));
writeFileSync(out + "payments.html", shell("Payments", "Payments", paymentsPage()));
writeFileSync(out + "payment.html", shell("Payment", "Payments", paymentPage()));
