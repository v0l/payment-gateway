import { writeFileSync } from "node:fs";
import { project, payments, detail, month, attention, methods, eur, sats, esc, statusWord } from "./data.mjs";

const out = new URL("../b/", import.meta.url).pathname;

const css = `
@font-face{font-family:JB;font-weight:400;src:url(fonts/jb-400.woff2) format("woff2")}
@font-face{font-family:JB;font-weight:700;src:url(fonts/jb-700.woff2) format("woff2")}
:root{--bg:#1d2433;--panel:#252f44;--line:#36435e;--text:#cfd8e6;--dim:#7d8aa3;--amber:#f5b941;--mint:#8fd6b4;--coral:#ef7b7b;--sky:#7fb4f0}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:400 14px/1.55 JB,ui-monospace,monospace;display:flex;flex-direction:column;min-height:100vh}
a{color:var(--sky);text-decoration:none}a:hover{text-decoration:underline}
:focus-visible{outline:2px solid var(--amber);outline-offset:1px}
.top{display:flex;gap:16px;align-items:center;padding:10px 20px;border-bottom:1px solid var(--line);background:var(--panel)}
.crumb{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.crumb b{color:var(--amber);font-weight:700}
.crumb i{color:var(--dim);font-style:normal;padding:0 6px}
.k{flex:1;max-width:520px;margin-left:auto;display:flex;justify-content:space-between;align-items:center;border:1px solid var(--line);background:var(--bg);padding:5px 12px;color:var(--dim)}
.k kbd{font:inherit;color:var(--text);border:1px solid var(--line);padding:0 6px}
.k span::before{content:"> ";color:var(--amber)}
.wrap{flex:1;display:grid}
.dim{color:var(--dim)}.ok{color:var(--mint)}.warn{color:var(--amber)}.bad{color:var(--coral)}
.status{display:flex;gap:0;border-top:1px solid var(--line);background:var(--panel);font-size:13px;overflow:hidden;white-space:nowrap}
.status span{padding:4px 14px;border-right:1px solid var(--line)}
.status .mode{background:var(--amber);color:var(--bg);font-weight:700}
.status .sp{flex:1;border:0}
h1,h2{font-size:14px;margin:0;font-weight:700}
.pane{border-right:1px solid var(--line);min-width:0;overflow:hidden}
.pane:last-child{border:0}
.ph{padding:8px 20px;border-bottom:1px solid var(--line);color:var(--dim);display:flex;justify-content:space-between;gap:12px}
.ph b{color:var(--text)}
.tail{list-style:none;margin:0;padding:6px 0}
.tail li{display:grid;grid-template-columns:62px 118px minmax(0,1fr) 108px;gap:12px;padding:3px 20px;white-space:nowrap}
.tail li>*{overflow:hidden;text-overflow:ellipsis}
.tail li:hover,.tail li.sel{background:var(--panel)}
.tail li.sel{box-shadow:inset 3px 0 var(--amber)}
.tail .r{text-align:right}
.spark{font-size:20px;letter-spacing:1px;line-height:1;color:var(--mint)}
.kv{display:grid;grid-template-columns:132px minmax(0,1fr);gap:2px 16px;padding:12px 20px;margin:0}
.kv dt{color:var(--dim)}.kv dd{margin:0;overflow-wrap:anywhere}
.m{display:grid;grid-template-columns:repeat(5,1fr);gap:0;border-bottom:1px solid var(--line)}
.m div{padding:12px 20px;border-right:1px solid var(--line)}
.m div:last-child{border:0}
.m b{display:block;font-size:22px;font-weight:700;color:var(--text)}
.bars{padding:10px 20px}
.bars div{display:grid;grid-template-columns:110px minmax(0,1fr) 110px;gap:12px}
.bars .b{color:var(--sky);overflow:hidden;white-space:nowrap}
.ov{grid-template-columns:minmax(0,1.55fr) minmax(0,1fr)}
.ov .ask li{padding:10px 20px;border-bottom:1px solid var(--line);list-style:none}
.ov ul{margin:0;padding:0}
.ov .ask .t{color:var(--amber);font-weight:700}
.split{grid-template-columns:minmax(0,1.5fr) minmax(0,1fr)}
.filters{display:flex;gap:8px;padding:8px 20px;border-bottom:1px solid var(--line);flex-wrap:wrap}
.filters span{border:1px solid var(--line);padding:0 10px;color:var(--dim)}
.filters span b{color:var(--text);font-weight:400}
.json{margin:0;padding:12px 20px;color:var(--dim);overflow:hidden;font:inherit;white-space:pre-wrap;overflow-wrap:anywhere}
.json em{font-style:normal;color:var(--mint)}.json strong{font-weight:400;color:var(--amber)}
.ev{list-style:none;margin:0;padding:6px 0}
.ev li{display:grid;grid-template-columns:92px 74px 150px minmax(0,1fr);gap:12px;padding:3px 20px}
.ev li>:last-child{overflow-wrap:anywhere}
.tl{display:block;height:6px;background:var(--line);margin:4px 20px 14px;position:relative}
.tl i{position:absolute;top:-3px;width:2px;height:12px;background:var(--amber)}
.detail{grid-template-columns:minmax(0,1fr) minmax(0,1fr)}
.hd{padding:16px 20px;border-bottom:1px solid var(--line);display:flex;gap:16px;align-items:baseline;flex-wrap:wrap}
.hd .amt{font-size:30px;font-weight:700;color:var(--text);line-height:1.1}
.pill{padding:0 8px;background:var(--mint);color:var(--bg);font-weight:700}
.pill.warn{background:var(--amber)}.pill.bad{background:var(--coral)}.pill.dim{background:var(--line);color:var(--text)}
@media(max-width:820px){
body{font-size:13px}
.k{display:none}.top{padding:8px 14px}
.ov,.split,.detail{grid-template-columns:minmax(0,1fr)}
.pane{border-right:0;border-bottom:1px solid var(--line)}
.m{grid-template-columns:repeat(2,1fr)}.m div{border-bottom:1px solid var(--line)}
.tail li{grid-template-columns:48px minmax(0,1fr) 84px;gap:8px;padding:3px 14px}
.tail li .hide{display:none}
.ph,.kv,.filters,.hd,.bars,.json{padding-left:14px;padding-right:14px}
.ev li{grid-template-columns:84px minmax(0,1fr);gap:8px}.ev li .hide{display:none}
.kv{grid-template-columns:96px minmax(0,1fr)}
.bars div{grid-template-columns:84px minmax(0,1fr) 84px;gap:6px}
.status span:nth-child(n+4){display:none}
}
`;

function shell(title, crumbs, body, status) {
  const c = crumbs.map((x, i) => (i ? `<i>/</i>` : "") + x).join("");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${title}</title><link rel="stylesheet" href="style.css"></head><body>
<div class="top"><div class="crumb"><b>gw</b><i>/</i>${c}</div><div class="k"><span>go to payments, keys, customer...</span><kbd>ctrl k</kbd></div></div>
${body}
<div class="status"><span class="mode">LIVE</span>${status.map(s => `<span>${s}</span>`).join("")}<span class="sp"></span><span class="dim">sam@northwind.example</span></div></body></html>`;
}

const blocks = "\u2581\u2582\u2583\u2584\u2585\u2586\u2587\u2588";
function spark() {
  const max = Math.max(...month.daily), min = Math.min(...month.daily);
  return month.daily.map(v => blocks[Math.min(7, Math.floor(((v - min) / (max - min)) * 7.99))]).join("");
}
const st = s => ({ paid: "ok", pending: "warn", expired: "dim", failed: "bad", refunded: "bad", disputed: "bad" }[s]);
const sym = s => ({ paid: "\u25cf", pending: "\u25cb", expired: "\u00d7", failed: "\u00d7", refunded: "\u21a9", disputed: "!" }[s]);
const row = (p, sel) => `<li${sel ? ' class="sel"' : ""}><span class="dim">${p.time}</span><a href="payment.html" class="hide">${p.id}</a><span>${esc(p.desc)}</span><span class="r ${st(p.status)}">${sym(p.status)} ${eur(p.cents)}</span></li>`;

function overview() {
  const tail = payments.slice(0, 13).map(p => row(p)).join("");
  const bars = month.share.map(s => `<div><span>${methods[s.method]}</span><span class="b">${"\u2588".repeat(Math.round(s.pct / 1.4))}</span><span class="r" style="text-align:right">${s.pct}% ${eur(s.cents)}</span></div>`).join("");
  const ask = attention.map(a => `<li><span class="t">${esc(a.title)}</span><br><span class="dim">${esc(a.body)}</span><br><a href="payment.html">${a.cta.toLowerCase()}</a></li>`).join("");
  return `<div class="wrap ov">
<section class="pane"><div class="ph"><h1>events, live</h1><span class="ok">\u25cf following</span></div>
<div class="m"><div><span class="dim">paid, 18 days</span><b>${eur(month.paidCents)}</b></div><div><span class="dim">waiting</span><b class="warn">${eur(month.waitingCents)}</b></div><div><span class="dim">today</span><b>${eur(61800)}</b></div><div><span class="dim">per day</span><b class="spark">${spark()}</b></div><div><span class="dim">webhooks</span><b class="ok">99.8%</b></div></div>
<ul class="tail">${tail}</ul></section>
<section class="pane"><div class="ph"><h2>needs you <span class="warn">3</span></h2><span>${project.name}</span></div><ul class="ask">${ask}</ul>
<div class="ph" style="border-top:1px solid var(--line)"><h2>by method</h2><span>1-18 nov</span></div><div class="bars">${bars}</div></section></div>`;
}

function paymentsPage() {
  const list = payments.map((p, i) => row(p, i === 0)).join("");
  const d = detail;
  const json = `{\n  <strong>"id"</strong>: <em>"${d.id}"</em>,\n  <strong>"amount"</strong>: ${d.cents},\n  <strong>"currency"</strong>: <em>"EUR"</em>,\n  <strong>"status"</strong>: <em>"paid"</em>,\n  <strong>"method"</strong>: <em>"lightning"</em>,\n  <strong>"metadata"</strong>: {\n    <strong>"order"</strong>: <em>"${d.metadata.order}"</em>\n  }\n}`;
  return `<div class="wrap split">
<section class="pane"><div class="ph"><h1>payments</h1><span>291 in 18 days</span></div>
<div class="filters"><span>status <b>any</b></span><span>method <b>any</b></span><span>since <b>7d</b></span><span>find <b>/ type to search</b></span></div>
<ul class="tail">${list}</ul></section>
<section class="pane"><div class="ph"><b>${d.id}</b><a href="payment.html">open</a></div><div class="hd"><span class="amt">${eur(d.cents)}</span><span class="pill">paid</span></div>
<dl class="kv"><dt>for</dt><dd>${esc(d.desc)}</dd><dt>method</dt><dd>lightning, ${d.node}</dd><dt>sats</dt><dd>${sats(d.cents)}</dd><dt>customer</dt><dd>${d.customer}</dd><dt>created</dt><dd>21:44:02, api key pk_live_4f9a</dd><dt>paid after</dt><dd>2m 9s</dd></dl>
<div class="ph" style="border-top:1px solid var(--line)"><h2>json</h2><span>copy</span></div><pre class="json">${json}</pre></section></div>`;
}

function paymentPage() {
  const d = detail;
  const last = d.events[d.events.length - 1].ms;
  const ticks = d.events.map(e => `<i style="left:${((e.ms / last) * 99).toFixed(1)}%"></i>`).join("");
  const ev = d.events.map(e => `<li><span class="dim">${e.t.slice(0, 8)}</span><span class="dim hide">+${e.ms >= 1000 ? (e.ms / 1000).toFixed(1) + "s" : e.ms + "ms"}</span><span class="${e.state === "paid" ? "ok" : ""} hide">${e.name}</span><span>${esc(e.note)}</span></li>`).join("");
  const wh = d.webhooks.map(w => `<li><span class="dim">${w.at}</span><span class="ok">${w.code}</span><span class="hide">${w.ms}ms</span><span>${w.event} ${w.url}</span></li>`).join("");
  return `<div class="wrap detail">
<section class="pane"><div class="hd"><span class="amt">${eur(d.cents)}</span><span class="pill">paid</span><span class="dim">${sats(d.cents)} sats</span></div>
<dl class="kv"><dt>id</dt><dd>${d.id}</dd><dt>for</dt><dd>${esc(d.desc)}</dd><dt>invoice</dt><dd>${d.invoiceRef}</dd><dt>customer</dt><dd>${d.customer}</dd><dt>method</dt><dd>lightning, ${d.node}</dd><dt>fee</dt><dd>${eur(0)}</dd><dt>order</dt><dd>${d.metadata.order}</dd><dt>plan</dt><dd>${d.metadata.plan}</dd><dt>created by</dt><dd>api key pk_live_4f9a</dd></dl>
<div class="ph" style="border-top:1px solid var(--line)"><h2>actions</h2></div><dl class="kv"><dt>r</dt><dd><a href="#">refund</a></dd><dt>d</dt><dd><a href="#">download receipt</a></dd><dt>c</dt><dd><a href="#">copy pay link</a></dd><dt>w</dt><dd><a href="#">resend webhooks</a></dd></dl></section>
<section class="pane"><div class="ph"><h2>timeline</h2><span>2m 10s end to end</span></div><span class="tl">${ticks}</span><ul class="ev">${ev}</ul>
<div class="ph" style="border-top:1px solid var(--line)"><h2>webhook deliveries</h2><span class="ok">2 of 2</span></div><ul class="ev">${wh}</ul></section></div>`;
}

writeFileSync(out + "style.css", css);
const base = ["overview"];
writeFileSync(out + "overview.html", shell("Terminal overview", [project.name.toLowerCase().replace(" ", "-"), "overview"], overview(), ["291 paid", "3 need you", "rate 1 eur = 1,112 sats", "press ? for keys"]));
writeFileSync(out + "payments.html", shell("Payments", [project.name.toLowerCase().replace(" ", "-"), "payments"], paymentsPage(), ["j/k move", "enter open", "/ search"]));
writeFileSync(out + "payment.html", shell("Payment", [project.name.toLowerCase().replace(" ", "-"), "payments", detail.id], paymentPage(), ["esc back", "r refund", "d receipt"]));
