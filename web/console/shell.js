"use strict";
var TAB_GROUPS = [
  ["overview", "Home", [["overview", "Home"]]],
  ["payments", "Payments", [["payments", "Payments"]]],
  ["customers", "Customers", [["customers", "Customers"]]],
  ["billing", "Billing", [["subscriptions", "Subscriptions"], ["plans", "Plans"], ["billing", "Platform bill"]]],
  ["balance", "Balance", [["balance", "Balance"], ["payouts", "Payouts"]]],
  ["developers", "Developers", [["providers", "Payment methods"], ["keys", "API keys"], ["webhooks", "Webhooks"]]],
  ["settings", "Settings", [["settings", "Settings"], ["team", "Team"], ["activity", "Activity"]]]
];
var OPERATOR_TABS = [["tenants", "Projects"], ["admin", "Accounts"], ["shared", "Shared providers"], ["platform-plans", "Platform plans"], ["treasury", "Treasury"], ["payout-queue", "Payout queue"], ["audit", "Audit"]];
var attentionCount = {};
var searchOpen = false;

function groupOf(tab) {
  for (var i = 0; i < TAB_GROUPS.length; i++) {
    if (TAB_GROUPS[i][2].some(function (s) { return s[0] === tab; })) return TAB_GROUPS[i];
  }
  return null;
}

function menuButton(label, items, opts) {
  var list = el("div", { class: "pop", role: "menu", hidden: true });
  var btn = el("button", { class: "bar-btn" + (opts && opts.cls ? " " + opts.cls : ""), type: "button", "aria-haspopup": "menu", "aria-expanded": "false", "aria-label": opts && opts.label ? opts.label : null }, label);
  function close() { list.hidden = true; btn.setAttribute("aria-expanded", "false"); document.removeEventListener("click", outside, true); document.removeEventListener("keydown", onKey); }
  function outside(e) { if (!wrap.contains(e.target)) close(); }
  function onKey(e) {
    if (e.key === "Escape") { close(); btn.focus(); }
    var links = Array.prototype.slice.call(list.querySelectorAll("a,button"));
    var i = links.indexOf(document.activeElement);
    if (e.key === "ArrowDown") { e.preventDefault(); (links[i + 1] || links[0]).focus(); }
    if (e.key === "ArrowUp") { e.preventDefault(); (links[i - 1] || links[links.length - 1]).focus(); }
  }
  btn.addEventListener("click", function () {
    if (!list.hidden) return close();
    list.hidden = false; btn.setAttribute("aria-expanded", "true");
    document.addEventListener("click", outside, true); document.addEventListener("keydown", onKey);
    var first = list.querySelector("a,button"); if (first) first.focus();
  });
  items.forEach(function (it) {
    if (it.sep) { list.appendChild(el("div", { class: "pop-sep", role: "separator" })); return; }
    if (it.head) { list.appendChild(el("div", { class: "pop-head", text: it.head })); return; }
    var node = it.href ? el("a", { href: it.href, role: "menuitem", class: it.on ? "on" : "", "aria-current": it.on ? "true" : null }, it.kids || [it.text]) :
      el("button", { type: "button", role: "menuitem", text: it.text, onclick: function () { close(); it.onclick(); } });
    node.addEventListener("click", close);
    list.appendChild(node);
  });
  var wrap = el("div", { class: "menu-wrap" }, [btn, list]);
  return wrap;
}

function projectSwitcher(projectId) {
  var current = me.projects.filter(function (p) { return String(p.id) === String(projectId); })[0];
  var items = [{ head: "Projects" }].concat(me.projects.map(function (p) {
    return { href: "#/p/" + p.id + "/overview", on: current && p.id === current.id, kids: [el("span", { text: p.name }), el("small", { text: p.role })] };
  }));
  items.push({ sep: true }, { href: "#/new", text: "New project" });
  var label = [el("span", { class: "mark", "aria-hidden": "true" }), el("b", { text: current ? current.name : "Payment gateway" }), el("span", { class: "chev", "aria-hidden": "true" })];
  return menuButton(label, items, { cls: "switch", label: "Switch project, current " + (current ? current.name : "none") });
}

function accountMenu() {
  var initials = me.email.slice(0, 2).toUpperCase();
  return menuButton([el("span", { class: "avatar", text: initials })], [
    { head: me.email },
    { href: "#/account", text: "Account and security" },
    { text: "Sign out", onclick: function () { api("POST", "/logout", {}).then(function () { me = null; csrf = ""; route(); }); } }
  ], { cls: "acct", label: "Account menu for " + me.email });
}

function operatorMenu(active) {
  return menuButton([el("span", { class: "hide-s", text: "Operator" }), el("span", { class: "chev", "aria-hidden": "true" })], [{ head: "Platform" }].concat(OPERATOR_TABS.map(function (t) {
    return { href: "#/" + t[0], text: t[1], on: t[0] === active };
  })), { cls: active && OPERATOR_TABS.some(function (t) { return t[0] === active; }) ? "on" : "", label: "Operator menu" });
}

function searchBox(projectId) {
  var input = el("input", { type: "search", class: "search-input", placeholder: projectId ? "Find a payment, customer or reference" : "Search", "aria-label": "Search payments", autocomplete: "off" });
  var results = el("div", { class: "search-results", role: "listbox", hidden: true });
  var kbd = el("kbd", { text: "Ctrl K" });
  var timerId = null;
  function hide() { results.hidden = true; results.replaceChildren(); }
  function open(hash) { hide(); input.value = ""; input.blur(); go(hash); }
  function run() {
    var q = input.value.trim();
    if (q.length < 2) return hide();
    var jobs = [];
    if (projectId) jobs.push(api("GET", "/projects/" + projectId + "/payments?q=" + encodeURIComponent(q)).then(function (r) { return r.payments.slice(0, 6).map(function (p) { return { label: (p.description || p.reference), sub: fmtAmount(p.amount, p.currency) + ", " + statusWord(p.status) + ", " + p.id.slice(0, 10), hash: "#/p/" + projectId + "/payments/" + p.id }; }); }).catch(function () { return []; }));
    if (projectId) jobs.push(api("GET", "/projects/" + projectId + "/customers?q=" + encodeURIComponent(q)).then(function (r) { return r.customers.slice(0, 4).map(function (c) { return { label: c.customer_ref, sub: c.payments + " payments", hash: "#/p/" + projectId + "/customers/" + encodeURIComponent(c.customer_ref) }; }); }).catch(function () { return []; }));
    if (me.is_operator && q.length >= 6) jobs.push(api("GET", "/operator/search?q=" + encodeURIComponent(q)).then(function (r) { return r.hits.map(function (h) { return { label: h.reference, sub: "Project " + h.tenant_id + ", " + h.matched + " match", hash: "#/p/" + h.tenant_id + "/payments/" + h.payment_id }; }); }).catch(function () { return []; }));
    Promise.all(jobs).then(function (lists) {
      var all = [].concat.apply([], lists);
      results.hidden = false;
      results.replaceChildren.apply(results, all.length ? all.map(function (x, i) {
        return el("a", { href: x.hash, role: "option", id: "sr-" + i, onclick: function (e) { e.preventDefault(); open(x.hash); } }, [el("span", { text: x.label }), el("small", { text: x.sub })]);
      }) : [el("p", { class: "muted", text: "Nothing matches \u201c" + q + "\u201d." })]);
    });
  }
  input.addEventListener("input", function () { clearTimeout(timerId); timerId = setTimeout(run, 180); });
  input.addEventListener("keydown", function (e) {
    var opts = Array.prototype.slice.call(results.querySelectorAll("a"));
    var i = opts.indexOf(document.activeElement);
    if (e.key === "Escape") { hide(); input.blur(); }
    if (e.key === "ArrowDown" && opts.length) { e.preventDefault(); opts[0].focus(); }
    if (e.key === "Enter" && opts.length) { e.preventDefault(); open(opts[0].getAttribute("href")); }
    if (i >= 0) return;
  });
  results.addEventListener("keydown", function (e) {
    var opts = Array.prototype.slice.call(results.querySelectorAll("a"));
    var i = opts.indexOf(document.activeElement);
    if (e.key === "ArrowDown") { e.preventDefault(); (opts[i + 1] || opts[0]).focus(); }
    if (e.key === "ArrowUp") { e.preventDefault(); if (i <= 0) input.focus(); else opts[i - 1].focus(); }
    if (e.key === "Escape") { hide(); input.focus(); }
  });
  input.addEventListener("blur", function () { setTimeout(function () { if (!results.contains(document.activeElement)) hide(); }, 150); });
  return el("div", { class: "search", role: "search" }, [input, kbd, results]);
}

function onShortcut(e) {
  var t = e.target;
  var typing = t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
  if ((e.key === "k" && (e.ctrlKey || e.metaKey)) || (e.key === "/" && !typing)) {
    var s = document.querySelector(".search-input");
    if (s) { e.preventDefault(); s.focus(); s.select(); }
  }
}
document.addEventListener("keydown", onShortcut);

function tabBar(active, projectId) {
  if (!projectId) return null;
  var group = groupOf(active);
  var count = attentionCount[projectId];
  var tabs = el("nav", { class: "tabs", "aria-label": "Project" }, TAB_GROUPS.map(function (g) {
    var on = group && g[0] === group[0];
    var kids = [g[1]];
    if (g[0] === "overview" && count) kids.push(el("span", { class: "count", text: String(count), "aria-label": count + " open items" }));
    return el("a", { href: "#/p/" + projectId + "/" + g[2][0][0], class: on ? "on" : "", "aria-current": on ? "page" : null }, kids);
  }));
  var sub = group && group[2].length > 1 ? el("nav", { class: "subtabs", "aria-label": group[1] }, group[2].map(function (s) {
    return el("a", { href: "#/p/" + projectId + "/" + s[0], class: s[0] === active ? "on" : "", "aria-current": s[0] === active ? "page" : null, text: s[1] });
  })) : null;
  return el("div", { class: "tabrow" }, [tabs, sub]);
}

function refreshAttentionCount(projectId) {
  api("GET", "/projects/" + projectId + "/attention").then(function (r) {
    attentionCount[projectId] = r.open;
    var tab = document.querySelector(".tabs a:first-child");
    if (!tab) return;
    var badge = tab.querySelector(".count");
    if (r.open && !badge) tab.appendChild(el("span", { class: "count", text: String(r.open), "aria-label": r.open + " open items" }));
    else if (badge && r.open) badge.textContent = String(r.open);
    else if (badge) badge.remove();
  }).catch(function () {});
}

function layout(active, projectId, content, opts) {
  var bar = el("header", { class: "top" }, [el("div", { class: "top-in" }, [
    projectSwitcher(projectId),
    projectId && opts && opts.project && !opts.project.trusted ? el("span", { class: "mode test", text: "Test mode" }) : (projectId ? el("span", { class: "mode", text: "Live" }) : null),
    searchBox(projectId),
    me.is_operator ? operatorMenu(active) : null,
    accountMenu()
  ])]);
  var banner = el("div", { class: "banners" });
  var main = el("main", { id: "main", class: opts && opts.wide ? "wide" : "" }, [banner, content]);
  root.replaceChildren.apply(root, [el("a", { class: "skip", href: "#main", text: "Skip to content", onclick: function (e) { e.preventDefault(); main.focus(); } }), bar, tabBar(active, projectId), main].filter(Boolean));
  main.setAttribute("tabindex", "-1");
  if (projectId && attentionCount[projectId] === undefined) refreshAttentionCount(projectId);
  if (me.is_operator && typeof treasuryBanner === "function") { treasuryBanner(banner); }
  window.scrollTo(0, 0);
}

function pageHead(title, sub, actions) {
  return el("div", { class: "page-head" }, [el("div", {}, [el("h1", { text: title }), sub ? el("p", { class: "sub" }, Array.isArray(sub) ? sub : [sub]) : null]), actions && actions.length ? el("div", { class: "actions" }, actions) : null]);
}

var TAB_TITLES = { overview: "Home", payments: "Payments", customers: "Customers", subscriptions: "Subscriptions", plans: "Plans", billing: "Platform bill", balance: "Balance", payouts: "Payouts", providers: "Payment methods", keys: "API keys", webhooks: "Webhooks", settings: "Settings", team: "Team", activity: "Activity" };

function projectShell(pid, tab, build, opts) {
  api("GET", "/projects/" + pid).then(function (p) {
    var body = el("div", { class: "page" });
    var bare = opts && opts.bare;
    if (!bare) body.appendChild(pageHead(TAB_TITLES[tab] || tab, opts && opts.sub ? opts.sub : null, opts && opts.actions ? opts.actions(p) : null));
    layout(tab, pid, body, { project: p, wide: opts && opts.wide });
    document.title = (TAB_TITLES[tab] || "Console") + ", " + (p.display_name || p.name);
    build(p, body);
  }).catch(function (e) { layout("", pid, el("div", { class: "page" }, [el("div", { class: "err", role: "alert", text: e.message })])); });
}

function operatorPage(active, title, sub, content) {
  document.title = title + ", operator";
  layout(active, null, el("div", { class: "page" }, [pageHead(title, sub), content]));
}
