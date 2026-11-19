"use strict";
function operatorTenants() {
  var box = el("div", {}, [skeletonRows()]); var healthBox = el("div", { class: "ledger" });
  var term = el("input", { type: "search", placeholder: "Payment id, reference, invoice hash or txid", "aria-label": "Search every project" });
  var results = el("div");
  function search() {
    api("GET", "/operator/search?q=" + encodeURIComponent(term.value.trim())).then(function (r) {
      results.replaceChildren(r.hits.length ? table(["Project", "Payment", "Reference", "Status", "Matched on"], r.hits.map(function (h) {
        return clickRow({}, [el("td", { text: String(h.tenant_id) }), el("td", { class: "mono", text: h.payment_id.slice(0, 12) }), el("td", { text: h.reference }), el("td", {}, [status(h.status)]), el("td", { class: "muted", text: h.matched })], function () { go("#/p/" + h.tenant_id + "/payments/" + h.payment_id); });
      })) : el("p", { class: "muted", text: "Nothing matches." }));
    }).catch(function (e) { msg(results, e.message, true); });
  }
  term.addEventListener("keydown", function (e) { if (e.key === "Enter") search(); });
  api("GET", "/operator/health").then(function (h) {
    healthBox.replaceChildren.apply(healthBox, [[h.tenants, "projects"], [h.tenants_disabled, "disabled"], [h.pending_payments, "waiting payments"], [h.pending_deliveries, "webhooks waiting"], [h.abandoned_deliveries_24h, "webhooks given up, 24 h", h.abandoned_deliveries_24h ? "bad" : ""]].map(function (x) {
      return figure(String(x[0]), x[1], x[2]);
    }));
  }).catch(function (e) { msg(healthBox, e.message, true); });
  api("GET", "/operator/tenants").then(function (r) {
    box.replaceChildren(table(["Project", "Status", ["Payments", "num"], ["Volume, 30 days", "num"], "Daily limit", "Last payment", ""], r.tenants.map(function (t) {
      return el("tr", {}, [el("td", {}, [el("a", { href: "#/p/" + t.id + "/overview", text: t.name })]), el("td", {}, [status(t.enabled ? (t.trusted ? "active" : "pending") : "failed", t.enabled ? (t.trusted ? "Live" : "Test mode") : "Disabled")]),
        el("td", { class: "num", text: String(t.payments) }), el("td", { class: "num", text: fmtAmount(t.volume_30d, "EUR") }), el("td", { class: "muted", text: t.limit_payments_per_day === null ? "Default" : String(t.limit_payments_per_day) }),
        el("td", { class: "muted", text: t.last_payment ? fmtWhen(t.last_payment) : "Never" }), el("td", { class: "num" }, [el("a", { href: "#/support/" + t.id, text: "Support view" })])]);
    })));
  }).catch(function (e) { msg(box, e.message, true); });
  operatorPage("tenants", "Projects", "Every project on the platform.", el("div", {}, [healthBox,
    section("Search every project", [el("div", { class: "filterbar" }, [term, el("button", { text: "Search", onclick: search })]), results]),
    section("All projects", [box])]));
}

function supportView(id) {
  var box = el("div", {}, [skeletonRows()]);
  operatorPage("tenants", "Support view", [el("a", { href: "#/tenants", text: "Projects" }), " / project " + id], box);
  api("GET", "/operator/tenants/" + id + "/support").then(function (r) {
    var s = r.stats;
    document.querySelector(".page-head h1").textContent = r.project.name;
    box.replaceChildren(
      el("div", { class: "banner info", role: "note" }, [el("div", {}, [el("b", { text: "Read-only support view" }), r.members + " members. Opening this view was recorded in the audit log."])]),
      el("div", { class: "ledger" }, dashboardFigures(s)),
      section("Recent payments", [r.recent_payments.length ? table(["Payment", "Reference", ["Amount", "num"], "Status", "Created"], r.recent_payments.map(function (p) { return el("tr", {}, [el("td", { class: "mono", text: p.id.slice(0, 10) }), el("td", { text: p.reference }), el("td", { class: "num", text: fmtAmount(p.amount, p.currency) }), el("td", {}, [status(p.status)]), el("td", { class: "muted", text: fmtWhen(p.created) })]); })) : el("p", { class: "muted", text: "None." })]),
      section("Payment methods", [r.providers.length ? table(["Name", "Method", "Type", "Status"], r.providers.map(function (p) { return el("tr", {}, [el("td", { text: p.name }), el("td", { text: methodName(p.method) }), el("td", { class: "muted", text: p.type }), el("td", {}, [status(p.enabled ? "active" : "expired", p.enabled ? "On" : "Off")])]); })) : el("p", { class: "muted", text: "None." })]),
      section("Webhook endpoints", [r.webhook_endpoints.length ? table(["Endpoint", ["Sent, 7 days", "num"], ["Failed", "num"]], r.webhook_endpoints.map(function (e) { return el("tr", {}, [el("td", { text: e.endpoint_id === null ? "Default" : "Endpoint " + e.endpoint_id }), el("td", { class: "num", text: String(e.recent_total) }), el("td", { class: "num", text: String(e.recent_failed) })]); })) : el("p", { class: "muted", text: "None." })]));
  }).catch(function (e) { box.replaceChildren(el("div", { class: "err", role: "alert", text: e.message })); });
}
