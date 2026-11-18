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
  var email = el("input", { type: "email", placeholder: "customer@example.com" });
  var op = el("input", { type: "checkbox" });
  layout("admin", null, el("div", {}, [el("h2", { text: "Accounts" }), el("p", { class: "sub", text: "Invite people, disable accounts, lift test limits per project from its settings." }),
    el("div", { class: "row" }, [email, el("label", {}, [op, " operator"]), el("button", { class: "primary", text: "Create invite", onclick: function () {
      api("POST", "/admin/invites", { email: email.value, operator: op.checked }).then(function (r) { out.replaceChildren(secretBox("Invite link for " + r.email, r.invite_url)); load(); }).catch(function (e) { msg(out, e.message, true); });
    } })]), out, box]));
  load();
}

