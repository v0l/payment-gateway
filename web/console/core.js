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
      if (r.status === 401 && path !== "/login" && path !== "/signup") { me = null; csrf = ""; route(); }
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
function field(label, input) { return el("div", { class: "field" }, [el("label", { text: label }), input]); }
function go(hash) { location.hash = hash; }
function stopTimer() { if (timer) { clearInterval(timer); timer = null; } }
function busy(btn, fn) {
  btn.disabled = true;
  return Promise.resolve().then(fn).then(function (v) { btn.disabled = false; return v; }, function (e) { btn.disabled = false; throw e; });
}

function layout(active, projectId, content) {
  var navs = [el("h1", { text: "Payment gateway" })];
  var link = function (text, hash, key) { return el("a", { href: hash, class: key === active ? "on" : "", text: text }); };
  me.projects.forEach(function (p) {
    var open = String(p.id) === String(projectId);
    navs.push(el("div", { class: "sep", text: p.name }));
    if (open) {
      [["Overview", "overview"], ["Payments", "payments"], ["Providers", "providers"], ["Webhooks", "webhooks"], ["Team", "team"], ["Settings", "settings"]].forEach(function (x) {
        navs.push(link(x[0], "#/p/" + p.id + "/" + x[1], x[1]));
      });
    } else { navs.push(link("Open", "#/p/" + p.id + "/overview", "")); }
  });
  navs.push(link("+ New project", "#/new", "new"));
  if (me.is_operator) { navs.push(el("div", { class: "sep", text: "Operator" })); navs.push(link("Accounts", "#/admin", "admin")); }
  navs.push(el("div", { class: "foot muted" }, [me.email, el("br"), el("a", { text: "Account", href: "#/account" }), " ", el("a", { text: "Sign out", onclick: function () {
    api("POST", "/logout", {}).then(function () { me = null; csrf = ""; route(); });
  } })]));
  root.replaceChildren(el("div", { class: "shell" }, [el("nav", {}, navs), el("main", {}, [content])]));
}

function tabs(pid, active) {
  return el("div", { class: "tabs" }, [["overview", "Overview"], ["payments", "Payments"], ["providers", "Providers"], ["webhooks", "Webhooks"], ["team", "Team"], ["settings", "Settings"]].map(function (t) {
    return el("a", { href: "#/p/" + pid + "/" + t[0], class: t[0] === active ? "on" : "", text: t[1] });
  }));
}

function projectShell(pid, tab, build) {
  api("GET", "/projects/" + pid).then(function (p) {
    var body = el("div", {}, [el("h2", { text: p.display_name || p.name }), el("p", { class: "sub" }, [p.role + (p.trusted ? "" : " ") , p.trusted ? "" : pill("test limits", "pending")]), tabs(pid, tab)]);
    layout(tab, pid, body);
    build(p, body);
  }).catch(function (e) { layout("", pid, el("div", { class: "err", text: e.message })); });
}

