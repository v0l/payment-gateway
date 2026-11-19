"use strict";
function admin() {
  var box = el("div", {}, [skeletonRows()]); var out = el("div");
  function load() {
    api("GET", "/admin/accounts").then(function (rows) {
      box.replaceChildren(table(["Email", "Role", "Last sign-in", "Status", ""], rows.map(function (a) {
        return el("tr", {}, [el("td", { text: a.email }), el("td", { class: "muted", text: a.is_operator ? "Operator" : "Customer" }), el("td", { class: "muted", text: a.last_login ? fmtWhen(a.last_login) : "Never" }),
          el("td", {}, [status(a.disabled ? "failed" : "active", a.disabled ? "Disabled" : "Active")]),
          el("td", { class: "num" }, [a.id === me.id ? null : el("button", { class: "link", type: "button", text: a.disabled ? "Enable" : "Disable", onclick: function () { api("POST", "/admin/accounts/" + a.id + "/disabled", { enabled: a.disabled }).then(load).catch(function (e) { msg(out, e.message, true); }); } })])]);
      })));
    }).catch(function (e) { msg(box, e.message, true); });
  }
  var health = el("div", {}, [skeletonRows()]);
  api("GET", "/admin/providers").then(function (rows) {
    health.replaceChildren(rows.length ? table(["Project", "Payment method", "Status", ["Errors, 24 h", "num"], ["In a row", "num"], "Last error"], rows.map(function (r) {
      return el("tr", {}, [el("td", { class: "muted", text: String(r.tenant_id) }), el("td", { text: r.name + ", " + methodName(r.method) }),
        el("td", {}, [status(r.auto_disabled_at ? "failed" : r.enabled ? "active" : "expired", r.auto_disabled_at ? "Switched off" : r.enabled ? "On" : "Off")]),
        el("td", { class: "num", text: (r.errors || 0) + " of " + ((r.errors || 0) + (r.successes || 0)) }), el("td", { class: "num", text: String(r.consecutive_failures || 0) }), el("td", { class: "muted trunc", text: r.last_error || "" })]);
    })) : el("p", { class: "muted", text: "No payment methods configured yet." }));
  }).catch(function (e) { msg(health, e.message, true); });
  var email = el("input", { type: "email", placeholder: "customer@example.com", "aria-label": "Email" });
  var op = el("input", { type: "checkbox" });
  operatorPage("admin", "Accounts", "Invite people and disable accounts. Lift test limits from a project's settings.", el("div", {}, [
    section("Invite someone", [el("div", { class: "row" }, [email, el("label", { class: "check" }, [op, "Operator"]), el("button", { class: "primary", text: "Create invite", onclick: function () {
      api("POST", "/admin/invites", { email: email.value, operator: op.checked }).then(function (r) { out.replaceChildren(secretBox("Invite link for " + r.email, r.invite_url)); load(); }).catch(function (e) { msg(out, e.message, true); });
    } })]), out]),
    section("All accounts", [box]),
    section("Payment method health", [health])]));
  load();
}
