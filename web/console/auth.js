"use strict";
function authPage(title, sub, fields, label, onsubmit) {
  var err = el("div", { class: "err" });
  var btn = el("button", { class: "primary", type: "submit", text: label });
  var form = el("form", { class: "card" }, fields.concat([btn, err]));
  form.addEventListener("submit", function (ev) {
    ev.preventDefault(); err.textContent = "";
    busy(btn, function () { return onsubmit(); }).catch(function (e) { err.textContent = e.message; });
  });
  root.replaceChildren(el("div", { class: "auth" }, [el("h1", { text: title }), el("p", { class: "muted", text: sub }), form]));
}

function loginPage() {
  var email = el("input", { type: "email", autocomplete: "username", required: true });
  var pw = el("input", { type: "password", autocomplete: "current-password", required: true });
  authPage("Payment gateway", "Sign in to your console. Accounts are by invite.", [field("Email", email), field("Password", pw)], "Sign in", function () {
    return api("POST", "/login", { email: email.value, password: pw.value }).then(function (m) { me = m; csrf = m.csrf; go("#/"); route(); });
  });
}

function invitePage(token) {
  api("GET", "/invite/" + encodeURIComponent(token)).then(function (info) {
    var pw = el("input", { type: "password", autocomplete: "new-password", required: true, minlength: 10 });
    var sub = "Create a password for " + info.email + (info.project ? " to join " + info.project : "") + ".";
    authPage("Accept invite", sub, [field("Password (10+ characters)", pw)], "Create account", function () {
      return api("POST", "/signup", { token: token, password: pw.value }).then(function (m) { me = m; csrf = m.csrf; location.replace("/console#/"); });
    });
  }).catch(function () {
    root.replaceChildren(el("div", { class: "auth" }, [el("h1", { text: "Invite not valid" }), el("p", { class: "muted", text: "It may have expired or already been used. Ask for a new one." })]));
  });
}

