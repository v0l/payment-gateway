"use strict";
var root = document.getElementById("root");
var me = null;
var csrf = "";
var timer = null;

function el(tag, attrs, kids) {
  var n = document.createElement(tag);
  Object.keys(attrs || {}).forEach(function (k) {
    var v = attrs[k];
    if (v === null || v === undefined || v === false) return;
    if (k === "text") n.textContent = v;
    else if (k === "class") n.className = v;
    else if (k === "value") n.value = v;
    else if (k.indexOf("on") === 0) n.addEventListener(k.slice(2), v);
    else n.setAttribute(k, v === true ? "" : v);
  });
  (kids || []).forEach(function (c) {
    if (c === null || c === undefined || c === false) return;
    n.appendChild(typeof c === "string" || typeof c === "number" ? document.createTextNode(String(c)) : c);
  });
  return n;
}

function api(method, path, body) {
  var opts = { method: method, headers: {}, credentials: "same-origin" };
  if (csrf) opts.headers["x-csrf-token"] = csrf;
  if (body !== undefined) { opts.headers["content-type"] = "application/json"; opts.body = JSON.stringify(body); }
  return fetch("/console/api" + path, opts).then(function (r) {
    return r.json().catch(function () { return {}; }).then(function (b) {
      if (r.status === 401 && path !== "/login" && path !== "/login/2fa" && path !== "/signup") { me = null; csrf = ""; route(); }
      if (!r.ok) { var e = new Error(b.error || "Request failed"); e.status = r.status; throw e; }
      return b;
    });
  });
}

function fmtAmount(amount, currency) {
  if (currency === "BTC") return Math.ceil(amount / 1000).toLocaleString() + " sats";
  var scale = 100, code = currency;
  if (currency.length === 4 && currency[0] === "M") { scale = 100000; code = currency.slice(1); }
  try { return new Intl.NumberFormat(undefined, { style: "currency", currency: code }).format(amount / scale); }
  catch (e) { return (amount / scale).toFixed(2) + " " + currency; }
}
function ago(iso) {
  if (!iso) return "";
  var s = Math.round((Date.now() - new Date(iso).getTime()) / 1000);
  if (s < 60) return s + "s ago"; if (s < 3600) return Math.round(s / 60) + "m ago";
  if (s < 86400) return Math.round(s / 3600) + "h ago"; return Math.round(s / 86400) + "d ago";
}
function pill(text, cls) { return el("span", { class: "pill " + (cls || text), text: text }); }
function msg(box, text, bad) { box.replaceChildren(el("div", { class: bad ? "err" : "ok-msg", text: text })); }
function secretBox(title, value) {
  return el("div", { class: "secret" }, [el("b", { text: title + " (shown once, copy it now)" }), el("span", { class: "mono", text: value })]);
}
function moneyEl(amount, currency, fee) {
  var kids = [fmtAmount(amount, currency)];
  if (fee) { kids.push(el("small", { text: "+" + fmtAmount(fee, currency) + " fee" })); }
  return el("span", { class: "money", title: currency === "BTC" ? amount.toLocaleString() + " msats" : null }, kids);
}
function confirmDialog(o) {
  return new Promise(function (resolve) {
    var prev = document.activeElement;
    var keep = el("button", { text: o.keep });
    var go = el("button", { class: o.danger ? "danger" : "primary", text: o.confirm });
    var extra = o.input ? (o.inputType === "text" ? el("input", { type: "text", "aria-label": o.input, placeholder: o.input }) : el("textarea", { rows: "2", "aria-label": o.input, placeholder: o.input })) : null;
    var scrim = el("div", { class: "dialog-scrim modal" }, [el("div", { class: "dialog", role: "alertdialog", "aria-modal": "true", "aria-label": o.title }, [
      el("h4", { text: o.title }), el("p", { text: o.body }), extra, el("div", { class: "row" }, [keep, go])])]);
    function close(v) { document.removeEventListener("keydown", onKey); scrim.remove(); if (prev && prev.focus) prev.focus(); resolve(v); }
    function onKey(e) { if (e.key === "Escape") close(null); }
    keep.addEventListener("click", function () { close(null); });
    go.addEventListener("click", function () { close({ text: extra ? extra.value : "" }); });
    document.addEventListener("keydown", onKey);
    document.body.appendChild(scrim);
    keep.focus();
  });
}
function toast(text) {
  var t = el("div", { class: "toast", role: "status", text: text });
  document.body.appendChild(t);
  setTimeout(function () { t.remove(); }, 5000);
}
var fieldSeq = 0;
function field(label, input) {
  if (!input.id) input.id = "f" + (++fieldSeq);
  return el("div", { class: "field" }, [el("label", { text: label, for: input.id }), input]);
}
function go(hash) { location.hash = hash; }
function stopTimer() { if (timer) { clearInterval(timer); timer = null; } }
function busy(btn, fn) {
  btn.disabled = true;
  return Promise.resolve().then(fn).then(function (v) { btn.disabled = false; return v; }, function (e) { btn.disabled = false; throw e; });
}

var STATUS_WORDS = { pending: "Waiting", paid: "Paid", expired: "Expired", cancelled: "Cancelled", refunded: "Refunded", partially_refunded: "Part refunded", failed: "Failed", disputed: "Disputed", completed: "Completed", requested: "Requested", approved: "Approved", sending: "Sending", rejected: "Rejected", delivered: "Delivered", waiting: "Waiting", retrying: "Retrying", processing: "Processing", active: "Active", past_due: "Past due", trialing: "Trial", incomplete: "Incomplete", paused: "Paused" };
var STATUS_TONE = { pending: "warn", paid: "ok", completed: "ok", delivered: "ok", active: "ok", expired: "neutral", cancelled: "neutral", rejected: "bad", refunded: "bad", partially_refunded: "warn", failed: "bad", disputed: "bad", refund_failed: "bad", requested: "warn", approved: "info", sending: "info", waiting: "warn", retrying: "warn", processing: "info", past_due: "bad", trialing: "info", incomplete: "warn", paused: "neutral" };
var METHOD_NAMES = { lightning: "Lightning", onchain: "On-chain", stripe: "Card", revolut: "Revolut", paypal: "PayPal", fixedfloat: "Other coins", manual: "Manual", bitcoin: "Bitcoin" };

function statusWord(s) { return STATUS_WORDS[s] || String(s || "").replace(/_/g, " "); }
function status(s, word) { return el("span", { class: "pill " + (STATUS_TONE[s] || s), text: word || statusWord(s) }); }
function methodName(m) { return METHOD_NAMES[m] || m || ""; }
function shownAttempt(p) {
  var a = p.attempts || [];
  return a.filter(function (x) { return x.status === "paid"; })[0] || a[a.length - 1] || null;
}
function moneyParts(amount, currency) {
  var s = fmtAmount(amount, currency);
  var m = s.match(/^(.*?)([.,]\d{2})(\D*)$/);
  return m ? [m[1], m[2] + m[3]] : [s, ""];
}
function bigMoney(amount, currency, cls) {
  var parts = moneyParts(amount, currency);
  return el("span", { class: "big-money " + (cls || "") }, [parts[0], parts[1] ? el("small", { text: parts[1] }) : null]);
}
function signedMoney(amount, currency) {
  var s = fmtAmount(Math.abs(amount), currency);
  return el("span", { class: "money " + (amount < 0 ? "out" : "in"), text: (amount < 0 ? "\u2212" : "+") + s });
}
function sats(msat) { return fmtAmount(msat, "BTC"); }
function fmtTime(iso) { return iso ? new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : ""; }
function fmtDay(iso) { return iso ? new Date(iso).toLocaleDateString([], { day: "numeric", month: "short" }) : ""; }
function fmtWhen(iso) {
  if (!iso) return "";
  var d = new Date(iso); var now = new Date();
  if (d.toDateString() === now.toDateString()) return fmtTime(iso);
  var y = new Date(now); y.setDate(now.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return "Yesterday " + fmtTime(iso);
  return fmtDay(iso) + ", " + fmtTime(iso);
}
function fmtWhenAfter(label, iso) {
  var w = fmtWhen(iso);
  if (!label) return w;
  if (!w) return label;
  return label + " " + (w.indexOf("Yesterday") === 0 ? "yesterday" + w.slice(9) : w);
}
function fmtFull(iso) { return iso ? new Date(iso).toLocaleString([], { day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" }) : ""; }
function tzOffset() { return -new Date().getTimezoneOffset(); }
function clickRow(attrs, cells, open) {
  attrs = attrs || {};
  attrs.class = (attrs.class ? attrs.class + " " : "") + "click";
  attrs.tabindex = "0";
  attrs.onclick = function (e) { if (e.target.closest("a,button,input,label")) return; open(e); };
  attrs.onkeydown = function (e) { if (e.key === "Enter" && e.target === e.currentTarget) open(e); };
  return el("tr", attrs, cells);
}
function table(head, rows, opts) {
  var label = opts && opts.label ? opts.label : (head ? head.map(function (h) { return typeof h === "string" ? h : h[0]; }).filter(Boolean).slice(0, 3).join(", ") + " table" : "Table");
  return el("div", { class: "table-wrap" + (opts && opts.cls ? " " + opts.cls : ""), tabindex: "0", role: "region", "aria-label": label }, [el("table", {}, [
    head ? el("thead", {}, [el("tr", {}, head.map(function (h) { return typeof h === "string" ? el("th", { text: h }) : el("th", { class: h[1] }, [h[0]]); }))]) : null,
    el("tbody", {}, rows)
  ])]);
}
function section(title, kids, opts) {
  return el("section", { class: "sec" + (opts && opts.cls ? " " + opts.cls : ""), id: opts && opts.id ? opts.id : null }, [el("div", { class: "sec-head" }, [el("h2", { text: title }), opts && opts.aside ? opts.aside : null])].concat(kids));
}
function kv(pairs) {
  return el("dl", { class: "kv" }, pairs.filter(Boolean).map(function (p) { return el("div", {}, [el("dt", { text: p[0] }), el("dd", { class: p[2] || null }, [p[1] === null || p[1] === undefined || p[1] === "" ? el("span", { class: "muted", text: "None" }) : p[1]])]); }));
}
function chips(options, current, onpick, label) {
  var box = el("div", { class: "chips", role: "group", "aria-label": label });
  function draw() {
    box.replaceChildren.apply(box, options.map(function (o) {
      return el("button", { type: "button", class: o[0] === current ? "on" : "", "aria-pressed": o[0] === current ? "true" : "false", onclick: function () { current = o[0]; draw(); onpick(o[0]); } }, [o[1], o[2] !== undefined ? el("span", { class: "n", text: String(o[2]) }) : null]);
    }));
  }
  draw();
  return box;
}
function copyText(text, label) {
  if (navigator.clipboard) navigator.clipboard.writeText(text).then(function () { toast((label || "Copied") + "."); });
}

