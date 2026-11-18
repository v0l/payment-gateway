"use strict";
function operatorTenants() {
  var box = el("div"); var healthBox = el("div", { class: "grid" });
  var term = el("input", { placeholder: "Payment id, reference, invoice hash or txid", "aria-label": "Global search" });
  var results = el("div");
  function search() {
    api("GET", "/operator/search?q=" + encodeURIComponent(term.value.trim())).then(function (r) {
      results.replaceChildren(r.hits.length ? el("table", {}, [el("tr", {}, ["Project", "Payment", "Reference", "Status", "Matched"].map(function (h) { return el("th", { text: h }); }))].concat(r.hits.map(function (h) {
        var open = function () { go("#/p/" + h.tenant_id + "/payments/" + h.payment_id); };
        return el("tr", { class: "click", tabindex: "0", onclick: open, onkeydown: function (e) { if (e.key === "Enter") open(); } }, [el("td", { text: h.tenant_id }), el("td", { class: "mono", text: h.payment_id.slice(0, 12) }), el("td", { text: h.reference }), el("td", {}, [pill(h.status)]), el("td", { text: h.matched })]);
      }))) : el("p", { class: "muted", text: "No match." }));
    }).catch(function (e) { msg(results, e.message, true); });
  }
  term.addEventListener("keydown", function (e) { if (e.key === "Enter") search(); });
  api("GET", "/operator/health").then(function (h) {
    [[h.tenants, "projects"], [h.tenants_disabled, "disabled"], [h.pending_payments, "pending payments"], [h.pending_deliveries, "webhooks waiting"], [h.abandoned_deliveries_24h, "abandoned webhooks, 24h"]].forEach(function (x) {
      healthBox.appendChild(el("div", { class: "stat" }, [el("div", { class: "n", text: x[0] }), el("div", { class: "l", text: x[1] })]));
    });
  }).catch(function (e) { msg(healthBox, e.message, true); });
  api("GET", "/operator/tenants").then(function (r) {
    box.replaceChildren(el("table", {}, [el("tr", {}, ["Project", "Status", "Payments", "Volume 30d", "Daily limit", "Last activity", ""].map(function (h) { return el("th", { text: h }); }))].concat(r.tenants.map(function (t) {
      return el("tr", {}, [el("td", { text: t.name }), el("td", {}, [pill(t.enabled ? (t.trusted ? "live" : "test") : "disabled", t.enabled ? (t.trusted ? "ok" : "pending") : "bad")]),
        el("td", { text: t.payments }), el("td", { class: "num", text: fmtAmount(t.volume_30d, "EUR") }), el("td", { text: t.limit_payments_per_day === null ? "default" : t.limit_payments_per_day }),
        el("td", { class: "muted", text: t.last_payment ? ago(t.last_payment) : "never" }), el("td", {}, [el("a", { href: "#/support/" + t.id, text: "support view" })])]);
    }))));
  }).catch(function (e) { msg(box, e.message, true); });
  layout("tenants", null, el("div", {}, [el("h2", { text: "Projects" }), el("p", { class: "sub", text: "Every project on the platform." }), healthBox,
    el("div", { class: "row" }, [term, el("button", { text: "Search", onclick: search })]), results, el("h3", { text: "All projects" }), box]));
}

function supportView(id) {
  var box = el("div");
  api("GET", "/operator/tenants/" + id + "/support").then(function (r) {
    var s = r.stats;
    box.replaceChildren(
      el("div", { class: "banner info" }, [el("div", {}, [el("b", { text: "Read-only support view" }), "This view was recorded in the audit log."])]),
      el("div", { class: "grid" }, dashboardCards(s)),
      el("h3", { text: "Recent payments" }),
      r.recent_payments.length ? el("table", {}, r.recent_payments.map(function (p) { return el("tr", {}, [el("td", { class: "mono", text: p.id.slice(0, 8) }), el("td", { text: p.reference }), el("td", { text: fmtAmount(p.amount, p.currency) }), el("td", {}, [pill(p.status)]), el("td", { class: "muted", text: ago(p.created) })]); })) : el("p", { class: "muted", text: "None." }),
      el("h3", { text: "Providers" }),
      r.providers.length ? el("table", {}, r.providers.map(function (p) { return el("tr", {}, [el("td", { text: p.name }), el("td", { text: p.method }), el("td", { text: p.type }), el("td", {}, [pill(p.enabled ? "enabled" : "disabled", p.enabled ? "ok" : "pending")])]); })) : el("p", { class: "muted", text: "None." }),
      el("h3", { text: "Webhook endpoints" }),
      r.webhook_endpoints.length ? el("table", {}, r.webhook_endpoints.map(function (e) { return el("tr", {}, [el("td", { text: "endpoint " + (e.endpoint_id === null ? "default" : e.endpoint_id) }), el("td", { text: e.recent_total + " sent, " + e.recent_failed + " failed" })]); })) : el("p", { class: "muted", text: "None." }));
    layout("tenants", null, el("div", {}, [el("h2", { text: r.project.name }), el("p", { class: "sub" }, [r.members + " members. ", el("a", { href: "#/tenants", text: "Back to projects" })]), box]));
  }).catch(function (e) { layout("tenants", null, el("div", { class: "err", text: e.message })); });
}
