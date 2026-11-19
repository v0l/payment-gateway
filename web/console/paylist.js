"use strict";
var FILTER_KEYS = ["q", "status", "method", "customer", "from", "to", "min_amount", "max_amount"];
var METHOD_OPTIONS = ["", "lightning", "onchain", "stripe", "revolut", "paypal", "fixedfloat"];
var SETTLE_DAYS = { stripe: 7, paypal: 7, revolut: 1 };

function filterQuery(values, extra) {
  var parts = [];
  FILTER_KEYS.forEach(function (k) {
    if (values[k]) parts.push(k + "=" + encodeURIComponent(values[k]));
  });
  if (extra) parts.push(extra);
  return parts.length ? "?" + parts.join("&") : "";
}

function downloadCsv(pid, values, out) {
  return fetch("/console/api/projects/" + pid + "/payments-export" + filterQuery(values), { credentials: "same-origin", headers: { "x-csrf-token": csrf } })
    .then(function (r) {
      if (!r.ok) throw new Error("Export failed");
      return r.blob();
    }).then(function (blob) {
      var a = el("a", { href: URL.createObjectURL(blob), download: "payments-" + pid + ".csv" });
      document.body.appendChild(a); a.click(); a.remove();
    }).catch(function (e) { msg(out, e.message, true); });
}

function availableWhen(p) {
  if (p.status === "pending") return el("span", { text: "When paid" });
  if (p.status !== "paid" && p.status !== "partially_refunded") return el("span", { text: "" });
  var a = shownAttempt(p);
  if (!a) return el("span", { text: "Manual" });
  if (a.platform_managed) return el("span", { text: "In your balance" });
  var days = SETTLE_DAYS[a.method];
  if (!days) return el("span", { text: "In your wallet" });
  var at = new Date(a.paid_at || p.paid_at);
  at.setDate(at.getDate() + days);
  return at <= new Date() ? el("span", { text: "Paid out by " + methodName(a.method) }) : el("span", { text: "From " + fmtDay(at.toISOString()) });
}

function paymentPanel(pid, p) {
  var a = shownAttempt(p);
  var steps = [];
  if (p.paid_at) steps.push(el("li", {}, ["Paid" + (a ? " over " + methodName(a.method) : ""), el("small", { text: fmtFull(p.paid_at) })]));
  (p.refunds || []).forEach(function (r) { steps.push(el("li", { class: r.status === "failed" ? "bad" : "warn" }, ["Refund " + fmtAmount(r.amount, p.currency) + " " + statusWord(r.status).toLowerCase(), el("small", { text: fmtFull(r.created) })])); });
  if (p.dispute_status) steps.push(el("li", { class: "bad" }, ["Dispute " + p.dispute_status]));
  if (!p.paid_at && p.status === "pending") steps.push(el("li", { class: "warn" }, ["Waiting for the customer", el("small", { text: "Expires " + fmtFull(p.expires) })]));
  steps.push(el("li", { class: "grey" }, ["Created", el("small", { text: fmtFull(p.created) })]));
  return el("aside", { class: "panel", "aria-label": "Selected payment" }, [
    el("div", { class: "panel-head" }, [el("span", { class: "mono", text: p.id.slice(0, 16) }), el("a", { href: "#/p/" + pid + "/payments/" + p.id, text: "Open payment" })]),
    el("div", {}, [bigMoney(p.amount, p.currency), status(p.dispute_status === "open" ? "disputed" : p.status)]),
    el("div", { class: "muted", text: p.description || p.reference }),
    el("ol", { class: "steps" }, steps),
    kv([["Customer", p.customer_ref], ["Method", a ? methodName(a.method) : null], ["Reference", p.reference], ["Available", availableWhen(p)]])
  ]);
}

function payments(pid) {
  projectShell(pid, "payments", function (p, body) {
    var inputs = {
      q: el("input", { type: "search", placeholder: "Search by reference or payment id", "aria-label": "Reference or id" }),
      status: el("select", { "aria-label": "Status" }, STATUSES.map(function (s) { return el("option", { value: s, text: s ? statusWord(s) : "Any status" }); })),
      method: el("select", { "aria-label": "Method" }, METHOD_OPTIONS.map(function (s) { return el("option", { value: s, text: s ? methodName(s) : "Any method" }); })),
      customer: el("input", { placeholder: "Customer ref or email", "aria-label": "Customer" }),
      from: el("input", { type: "date", "aria-label": "From date" }),
      to: el("input", { type: "date", "aria-label": "To date" }),
      min_amount: el("input", { type: "number", min: "0", placeholder: "Min, minor units", "aria-label": "Minimum amount in minor units" }),
      max_amount: el("input", { type: "number", min: "0", placeholder: "Max, minor units", "aria-label": "Maximum amount in minor units" })
    };
    var views = el("div", { class: "views", role: "group", "aria-label": "Views" });
    var split = el("div", { class: "split" });
    var mob = el("ul", { class: "mob-list" });
    var more = el("div", { class: "more" }); var out = el("div"); var total = el("span", { class: "total" });
    var rows = []; var next = null; var selected = null; var current = "all"; var saved = [];
    function values() {
      var v = {};
      FILTER_KEYS.forEach(function (k) { v[k] = inputs[k].value.trim(); });
      return v;
    }
    var BUILTIN = [["all", "All", {}], ["pending", "Waiting", { status: "pending" }], ["paid", "Paid", { status: "paid" }], ["refunded", "Refunded", { status: "refunded" }], ["expired", "Expired", { status: "expired" }], ["last_7_days", "Last 7 days", { from: new Date(Date.now() - 6 * 864e5).toISOString().slice(0, 10) }]];
    function drawViews(counts) {
      var tabs = BUILTIN.map(function (b) { return [b[0], b[1], counts ? counts[b[0]] : undefined, b[2]]; });
      saved.forEach(function (v) { tabs.push(["saved:" + v.id, v.name, undefined, v.filter]); });
      views.replaceChildren.apply(views, tabs.map(function (t) {
        return el("button", { type: "button", class: t[0] === current ? "on" : "", "aria-pressed": t[0] === current ? "true" : "false", onclick: function () {
          current = t[0];
          FILTER_KEYS.forEach(function (k) { inputs[k].value = t[3][k] || ""; });
          drawViews(counts); load(true);
        } }, [t[1], t[2] !== undefined ? el("span", { class: "n", text: Number(t[2]).toLocaleString() }) : null]);
      }).concat([el("button", { type: "button", class: "", text: "Save this view", onclick: saveView })]));
    }
    function saveView() {
      confirmDialog({ title: "Save this view", body: "Saves the current filters as a tab on this page, for you.", input: "Name", inputType: "text", keep: "Cancel", confirm: "Save view" }).then(function (r) {
        if (!r || !r.text.trim()) return;
        var f = {}; var v = values(); Object.keys(v).forEach(function (k) { if (v[k]) f[k] = v[k]; });
        api("POST", "/projects/" + pid + "/saved-views", { name: r.text.trim(), filter: f }).then(function () { toast("View saved."); loadViews(); }).catch(function (e) { msg(out, e.message, true); });
      });
    }
    function loadViews() {
      Promise.all([api("GET", "/projects/" + pid + "/payment-view-counts").catch(function () { return null; }), api("GET", "/projects/" + pid + "/saved-views").catch(function () { return { views: [] }; })]).then(function (r) {
        saved = r[1].views; drawViews(r[0]);
      });
    }
    function select(row) {
      selected = row;
      split.querySelectorAll("tr.sel").forEach(function (t) { t.classList.remove("sel"); });
      var tr = split.querySelector("tr[data-id='" + row.id + "']");
      if (tr) tr.classList.add("sel");
      var old = split.querySelector(".panel");
      var panel = paymentPanel(pid, row);
      if (old) old.replaceWith(panel); else split.appendChild(panel);
    }
    function draw() {
      if (!rows.length) {
        split.replaceChildren(el("div", { class: "empty" }, [el("b", { text: "No payments match." }), el("p", { class: "muted", text: "Clear a filter or widen the dates. New payments appear here as soon as your app creates them." })]));
        mob.replaceChildren();
        more.replaceChildren();
        return;
      }
      var trs = rows.map(function (r) {
        var a = shownAttempt(r);
        return clickRow({ "data-id": r.id }, [
          el("td", { class: "amt" }, [fmtAmount(r.amount, r.currency), el("small", { text: r.currency })]),
          el("td", {}, [status(r.dispute_status === "open" ? "disputed" : r.status)]),
          el("td", { class: "trunc" }, [r.description || r.reference]),
          el("td", { class: "muted trunc", text: r.customer_ref || "" }),
          el("td", { class: "muted", text: a ? methodName(a.method) : "" }),
          el("td", { class: "muted" }, [availableWhen(r)]),
          el("td", { class: "muted", text: fmtWhen(r.created) })
        ], function () { select(r); });
      });
      var t = table(["Amount", "Status", "Description", "Customer", "Method", "Available", "Created"].map(function (h) { return h === "Amount" ? [h, "num"] : h; }), trs, { cls: "plain" });
      split.replaceChildren(t);
      select(selected && rows.some(function (r) { return r.id === selected.id; }) ? rows.filter(function (r) { return r.id === selected.id; })[0] : rows[0]);
      mob.replaceChildren.apply(mob, rows.map(function (r) {
        var li = el("li", { tabindex: "0" }, [el("b", { text: r.description || r.reference }), el("span", { class: "a", text: fmtAmount(r.amount, r.currency) }),
          el("span", { class: "d", text: [r.customer_ref, fmtWhen(r.created)].filter(Boolean).join(", ") }), el("span", {}, [status(r.dispute_status === "open" ? "disputed" : r.status)])]);
        li.addEventListener("click", function () { go("#/p/" + pid + "/payments/" + r.id); });
        li.addEventListener("keydown", function (e) { if (e.key === "Enter") go("#/p/" + pid + "/payments/" + r.id); });
        return li;
      }));
      more.replaceChildren(next ? el("button", { text: "Load more", onclick: function () { load(false); } }) : el("span", { class: "muted", text: "No more payments" }));
    }
    function load(reset) {
      if (reset) { rows = []; next = null; }
      api("GET", "/projects/" + pid + "/payments" + filterQuery(values(), next ? "before=" + encodeURIComponent(next) : "")).then(function (r) {
        rows = rows.concat(r.payments); next = r.next;
        var paid = {}; rows.forEach(function (x) { if (x.status === "paid") paid[x.currency] = (paid[x.currency] || 0) + x.amount; });
        total.textContent = rows.length + (next ? "+" : "") + " shown" + Object.keys(paid).map(function (c) { return ", " + fmtAmount(paid[c], c) + " paid"; }).join("");
        draw();
      }).catch(function (e) { msg(split, e.message, true); });
    }
    FILTER_KEYS.forEach(function (k) {
      inputs[k].addEventListener(inputs[k].tagName === "SELECT" || inputs[k].type === "date" ? "change" : "keydown", function (e) { if (e.type === "change" || e.key === "Enter") { current = "custom"; drawViews(null); loadViews(); load(true); } });
    });
    var moreFilters = el("details", { class: "hide-s" }, [el("summary", { text: "More filters" }), el("div", { class: "filterbar" }, [inputs.customer, inputs.min_amount, inputs.max_amount, el("button", { class: "small", text: "Apply", onclick: function () { load(true); } })])]);
    body.querySelector(".page-head").appendChild(el("div", { class: "actions" }, [
      el("button", { text: "Export CSV", onclick: function () { downloadCsv(pid, values(), out); } }),
      el("button", { text: "Cancel stale", disabled: p.role === "viewer", title: "Cancel waiting payments older than 24 hours", onclick: function () { bulkCancelDialog(pid, out, function () { load(true); loadViews(); }); } }),
      el("button", { class: "primary", text: "New payment", disabled: p.role === "viewer", onclick: function () { go("#/p/" + pid + "/payments/new"); } })
    ]));
    body.appendChild(views);
    body.appendChild(el("div", { class: "filterbar" }, [inputs.q, inputs.status, inputs.method, inputs.from, inputs.to, el("button", { class: "link", type: "button", text: "Clear", onclick: function () { FILTER_KEYS.forEach(function (k) { inputs[k].value = ""; }); current = "all"; loadViews(); load(true); } }), total]));
    body.appendChild(moreFilters);
    body.appendChild(out); body.appendChild(split); body.appendChild(mob); body.appendChild(more);
    drawViews(null); loadViews(); load(true);
  }, { sub: "Every payment your app created, newest first. Select a row to see it here." });
}

function bulkCancelDialog(pid, out, done) {
  var hours = "24";
  confirmDialog({ title: "Cancel stale waiting payments", body: "Cancels every waiting payment older than " + hours + " hours (up to 200 at a time). Customers can no longer pay them. This cannot be undone.", keep: "Keep them", confirm: "Cancel payments", danger: true }).then(function (r) {
    if (!r) return;
    api("POST", "/projects/" + pid + "/payments-bulk-cancel", { older_than_hours: Number(hours) }).then(function (res) {
      msg(out, res.cancelled + " cancelled, " + res.skipped + " skipped" + (res.more ? ". Run again for more." : "."));
      done();
    }).catch(function (e) { msg(out, e.message, true); });
  });
}
