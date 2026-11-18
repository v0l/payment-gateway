"use strict";
var FILTER_KEYS = ["q", "status", "method", "customer", "from", "to", "min_amount", "max_amount"];
var METHOD_OPTIONS = ["", "lightning", "onchain", "stripe", "revolut", "paypal", "fixedfloat"];

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

function payments(pid) {
  projectShell(pid, "payments", function (p, body) {
    var inputs = {
      q: el("input", { placeholder: "Reference or id", "aria-label": "Reference or id" }),
      status: el("select", { "aria-label": "Status" }, STATUSES.map(function (s) { return el("option", { value: s, text: s || "any status" }); })),
      method: el("select", { "aria-label": "Method" }, METHOD_OPTIONS.map(function (s) { return el("option", { value: s, text: s || "any method" }); })),
      customer: el("input", { placeholder: "Customer ref or email", "aria-label": "Customer" }),
      from: el("input", { type: "date", "aria-label": "From date" }),
      to: el("input", { type: "date", "aria-label": "To date" }),
      min_amount: el("input", { type: "number", min: "0", placeholder: "Min amount (minor units)", "aria-label": "Minimum amount" }),
      max_amount: el("input", { type: "number", min: "0", placeholder: "Max amount (minor units)", "aria-label": "Maximum amount" })
    };
    var list = el("div", { class: "list-area" }); var more = el("div"); var out = el("div");
    var viewSelect = el("select", { "aria-label": "Saved views" });
    var rows = []; var next = null; var views = [];
    function values() {
      var v = {};
      FILTER_KEYS.forEach(function (k) { v[k] = inputs[k].value.trim(); });
      return v;
    }
    function load(reset) {
      if (reset) { rows = []; next = null; }
      api("GET", "/projects/" + pid + "/payments" + filterQuery(values(), next ? "before=" + encodeURIComponent(next) : "")).then(function (r) {
        rows = rows.concat(r.payments); next = r.next; draw();
      }).catch(function (e) { msg(list, e.message, true); });
    }
    function draw() {
      list.replaceChildren(rows.length ? el("table", {}, [el("tr", {}, ["Id", "Reference", "Amount", "Status", "Method", "Created"].map(function (h) { return el("th", { class: h === "Amount" ? "num" : "", text: h }); }))].concat(rows.map(function (r) {
        var shown = r.attempts.filter(function (a) { return a.status === "paid"; })[0] || r.attempts[r.attempts.length - 1];
        return el("tr", { class: "click", tabindex: "0", onclick: function () { go("#/p/" + pid + "/payments/" + r.id); }, onkeydown: function (e) { if (e.key === "Enter") go("#/p/" + pid + "/payments/" + r.id); } }, [
          el("td", { class: "mono", text: r.id.slice(0, 8) }), el("td", { text: r.reference }), el("td", { class: "num" }, [moneyEl(r.amount, r.currency)]),
          el("td", {}, [pill(r.status)]), el("td", { text: shown ? shown.method : "-" }), el("td", { class: "muted", text: ago(r.created) })]);
      }))) : el("p", { class: "muted", text: "No payments match these filters. Clear a filter or widen the dates." }));
      more.replaceChildren(next ? el("button", { text: "Load more", onclick: function () { load(false); } }) : el("span", { class: "muted", text: rows.length ? "No more payments" : "" }));
    }
    function loadViews() {
      api("GET", "/projects/" + pid + "/saved-views").then(function (r) {
        views = r.views;
        viewSelect.replaceChildren.apply(viewSelect, [el("option", { value: "", text: "Saved views" })].concat(views.map(function (v) { return el("option", { value: String(v.id), text: v.name }); })));
      });
    }
    viewSelect.addEventListener("change", function () {
      var v = views.filter(function (x) { return String(x.id) === viewSelect.value; })[0];
      if (!v) return;
      FILTER_KEYS.forEach(function (k) { inputs[k].value = v.filter[k] || ""; });
      load(true);
    });
    FILTER_KEYS.forEach(function (k) {
      inputs[k].addEventListener(inputs[k].tagName === "SELECT" ? "change" : "keydown", function (e) { if (e.type === "change" || e.key === "Enter") load(true); });
    });
    var viewName = el("input", { placeholder: "View name", "aria-label": "View name" });
    var canWrite = p.role !== "viewer";
    body.appendChild(el("div", { class: "filters" }, FILTER_KEYS.map(function (k) { return inputs[k]; })));
    body.appendChild(el("div", { class: "row" }, [
      el("button", { text: "Search", onclick: function () { load(true); } }),
      el("button", { text: "Clear", onclick: function () { FILTER_KEYS.forEach(function (k) { inputs[k].value = ""; }); viewSelect.value = ""; load(true); } }),
      el("button", { text: "Export CSV", onclick: function () { downloadCsv(pid, values(), out); } }),
      viewSelect, viewName,
      el("button", { text: "Save view", onclick: function () {
        var f = {}; var v = values(); Object.keys(v).forEach(function (k) { if (v[k]) f[k] = v[k]; });
        api("POST", "/projects/" + pid + "/saved-views", { name: viewName.value, filter: f }).then(function () { viewName.value = ""; msg(out, "View saved."); loadViews(); }).catch(function (e) { msg(out, e.message, true); });
      } }),
      el("button", { class: "danger", text: "Cancel stale pending", disabled: !canWrite, onclick: function () { bulkCancelDialog(pid, out, function () { load(true); }); } }),
      el("button", { class: "primary", text: "New payment", onclick: function () { go("#/p/" + pid + "/payments/new"); } })
    ]));
    body.appendChild(out); body.appendChild(list); body.appendChild(more);
    loadViews(); load(true);
  });
}

function bulkCancelDialog(pid, out, done) {
  var hours = "24";
  confirmDialog({ title: "Cancel stale pending payments", body: "Cancels every pending payment older than " + hours + " hours (up to 200 at a time). This cannot be undone.", keep: "Keep them", confirm: "Cancel payments", danger: true }).then(function (r) {
    if (!r) return;
    api("POST", "/projects/" + pid + "/payments-bulk-cancel", { older_than_hours: Number(hours) }).then(function (res) {
      msg(out, res.cancelled + " cancelled, " + res.skipped + " skipped" + (res.more ? ". Run again for more." : "."));
      done();
    }).catch(function (e) { msg(out, e.message, true); });
  });
}
