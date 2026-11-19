"use strict";
function authPage(title, sub, fields, label, onsubmit) {
  var err = el("div", { class: "err", role: "alert" });
  var btn = el("button", { class: "primary", type: "submit", text: label });
  var form = el("form", { class: "card" }, fields.concat([el("div", { class: "row" }, [btn]), err]));
  form.addEventListener("submit", function (ev) {
    ev.preventDefault(); err.textContent = "";
    busy(btn, function () { return onsubmit(); }).catch(function (e) { err.textContent = e.message; });
  });
  authScreen([el("h1", { text: title }), el("p", { class: "muted", text: sub }), form]);
  var first = form.querySelector("input"); if (first) first.focus();
}

function authScreen(kids) {
  document.title = "Console";
  root.replaceChildren(el("main", { class: "auth", id: "main" }, [el("div", { class: "brand" }, [el("span", { class: "mark", "aria-hidden": "true" }), "Payment gateway"])].concat(kids)));
}

function loginPage() {
  var email = el("input", { type: "email", autocomplete: "username", required: true });
  var pw = el("input", { type: "password", autocomplete: "current-password", required: true });
  authPage("Payment gateway", "Sign in to your console. Accounts are by invite.", [field("Email", email), field("Password", pw)], "Sign in", function () {
    return api("POST", "/login", { email: email.value, password: pw.value }).then(function (m) {
      if (m.two_factor) return secondStepPage(m.challenge);
      me = m; csrf = m.csrf; go("#/"); route();
    });
  });
  root.querySelector(".auth").appendChild(el("p", { class: "muted" }, [el("a", { text: "Forgot your password?", href: "#/reset" })]));
}

function resetRequestPage() {
  var email = el("input", { type: "email", autocomplete: "username", required: true });
  authPage("Reset password", "Enter your email and we will send a reset link if an account exists.", [field("Email", email)], "Send reset link", function () {
    return api("POST", "/reset", { email: email.value }).then(function () {
      authScreen([el("h1", { text: "Check your email" }), el("p", { class: "muted", text: "If an account exists for that address, a reset link is on its way. It works once and expires in 1 hour." }), el("p", {}, [el("a", { text: "Back to sign in", href: "#/" })])]);
    });
  });
}

function resetConfirmPage(token) {
  var pw = el("input", { type: "password", autocomplete: "new-password", required: true, minlength: 10 });
  authPage("Choose a new password", "You will be signed out everywhere and can sign in with the new password.", [field("New password (10+ characters)", pw)], "Set password", function () {
    return api("POST", "/reset/confirm", { token: token, password: pw.value }).then(function () { location.replace("/console#/"); });
  });
}

function confirmEmailPage(token) {
  authScreen([el("h1", { text: "Confirming email" })]);
  api("POST", "/email/confirm", { token: token }).then(function (r) {
    authScreen([el("h1", { text: "Email confirmed" }), el("p", { class: "muted", text: "Your sign in email is now " + r.email + "." }), el("p", {}, [el("a", { text: "Continue", href: "/console#/" })])]);
  }).catch(function () {
    authScreen([el("h1", { text: "Link not valid" }), el("p", { class: "muted", text: "It may have expired or already been used." })]);
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
    authScreen([el("h1", { text: "Invite not valid" }), el("p", { class: "muted", text: "It may have expired or already been used. Ask for a new one." })]);
  });
}


function secondStepPage(challenge) {
  var code = el("input", { type: "text", inputmode: "numeric", autocomplete: "one-time-code", maxlength: 8, required: true });
  authPage("Two factor code", "Enter the 6 digit code from your authenticator app.", [field("Code", code)], "Verify", function () {
    return api("POST", "/login/2fa", { challenge: challenge, code: code.value }).then(function (m) { me = m; csrf = m.csrf; go("#/"); route(); });
  });
  root.querySelector(".auth").appendChild(el("p", { class: "muted" }, [el("a", { href: "#", text: "Use a recovery code instead", onclick: function (ev) { ev.preventDefault(); recoveryStepPage(challenge); } })]));
}

function recoveryStepPage(challenge) {
  var code = el("input", { type: "text", autocomplete: "off", required: true, placeholder: "xxxx-xxxx-xxxx-xxxx" });
  authPage("Recovery code", "Each recovery code works once.", [field("Recovery code", code)], "Verify", function () {
    return api("POST", "/login/2fa", { challenge: challenge, recovery_code: code.value }).then(function (m) { me = m; csrf = m.csrf; go("#/"); route(); });
  });
}
