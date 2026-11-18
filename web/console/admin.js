"use strict";
function admin() {
  var box = el("div"); var out = el("div");
  function load() {
    api("GET", "/admin/accounts").then(function (rows) {
      box.replaceChildren(el("table", {}, [el("tr", {}, ["Email", "Role", "Last login", "Status", ""].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (a) {
        return el("tr", {}, [el("td", { text: a.email }), el("td", { text: a.is_operator ? "operator" : "customer" }), el("td", { class: "muted", text: ago(a.last_login) }),
          el("td", {}, [pill(a.disabled ? "disabled" : "active", a.disabled ? "bad" : "ok")]),
          el("td", {}, [a.id === me.id ? null : el("a", { text: a.disabled ? "enable" : "disable", onclick: function () { api("POST", "/admin/accounts/" + a.id + "/disabled", { enabled: a.disabled }).then(load); } })])]);
      }))));
    });
  }
  var health = el("div");
  api("GET", "/admin/providers").then(function (rows) {
    health.replaceChildren(el("table", {}, [el("tr", {}, ["Project", "Provider", "Status", "Errors (24h)", "In a row", "Last error"].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (r) {
      return el("tr", {}, [el("td", { text: String(r.tenant_id) }), el("td", { text: r.name + " (" + r.method + ")" }),
        el("td", {}, [pill(r.auto_disabled_at ? "auto-disabled" : r.enabled ? "enabled" : "disabled", r.enabled ? "ok" : "bad")]),
        el("td", { text: (r.errors || 0) + " / " + ((r.errors || 0) + (r.successes || 0)) }), el("td", { text: String(r.consecutive_failures || 0) }), el("td", { class: "muted", text: r.last_error || "" })]);
    }))));
  });
  var email = el("input", { type: "email", placeholder: "customer@example.com" });
  var op = el("input", { type: "checkbox" });
  layout("admin", null, el("div", {}, [el("h2", { text: "Accounts" }), el("p", { class: "sub", text: "Invite people, disable accounts, lift test limits per project from its settings." }),
    el("div", { class: "row" }, [email, el("label", {}, [op, " operator"]), el("button", { class: "primary", text: "Create invite", onclick: function () {
      api("POST", "/admin/invites", { email: email.value, operator: op.checked }).then(function (r) { out.replaceChildren(secretBox("Invite link for " + r.email, r.invite_url)); load(); }).catch(function (e) { msg(out, e.message, true); });
    } })]), out, box, el("h2", { text: "Provider health" }), health]));
  load();
}

