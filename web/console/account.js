"use strict";
function account() {
  var cur = el("input", { type: "password", autocomplete: "current-password" });
  var nw = el("input", { type: "password", autocomplete: "new-password" });
  var out = el("div"); var btn = el("button", { class: "primary", text: "Change password" });
  btn.addEventListener("click", function () {
    busy(btn, function () { return api("POST", "/password", { current: cur.value, new: nw.value }); }).then(function () { cur.value = ""; nw.value = ""; msg(out, "Password changed. Other sessions were signed out."); }).catch(function (e) { msg(out, e.message, true); });
  });
  var cards = [field("Current password", cur), field("New password, 10 or more characters", nw), el("div", { class: "row" }, [btn]), out];
  var sections = [section("Password", [el("div", { class: "card form" }, cards)])];
  if (me.mail_enabled) {
    var pwNow = el("input", { type: "password", autocomplete: "current-password" });
    var newEmail = el("input", { type: "email", autocomplete: "off" });
    var emailOut = el("div"); var emailBtn = el("button", { class: "primary", text: "Send confirmation" });
    emailBtn.addEventListener("click", function () {
      busy(emailBtn, function () { return api("POST", "/email", { password: pwNow.value, new_email: newEmail.value }); }).then(function (r) { pwNow.value = ""; msg(emailOut, "Confirmation sent to " + r.email + ". The change applies once you open the link."); }).catch(function (e) { msg(emailOut, e.message, true); });
    });
    sections.push(section("Email", [el("div", { class: "card form" }, [field("New email", newEmail), field("Current password", pwNow), el("div", { class: "row" }, [emailBtn]), emailOut])]));
    var box = el("input", { type: "checkbox" }); box.checked = me.notify;
    var notifyOut = el("div");
    box.addEventListener("change", function () {
      api("POST", "/notifications", { enabled: box.checked }).then(function (r) { me.notify = r.notify; msg(notifyOut, r.notify ? "Notifications on." : "Notifications off."); }).catch(function (e) { box.checked = me.notify; msg(notifyOut, e.message, true); });
    });
    sections.push(section("Notifications", [el("div", { class: "card" }, [el("label", { class: "check" }, [box, el("span", { text: "Email me about failed webhooks, failing payment methods and failed refunds on projects I own" })]), notifyOut])]));
  }
  var tf = el("div", { class: "card" });
  sections.push(section("Two factor authentication", [tf]));
  operatorPage("account", "Account", me.email, el("div", {}, sections));
  twoFactorCard(tf);
}

function recoveryList(codes) {
  return el("div", { class: "secret" }, [el("b", { text: "Recovery codes (shown once, store them somewhere safe)" })].concat(codes.map(function (c) { return el("span", { class: "mono", text: c }); })));
}

function twoFactorCard(box) {
  api("GET", "/2fa").then(function (st) {
    if (st.enabled) return twoFactorEnabled(box, st);
    var start = el("button", { class: "primary", text: "Set up authenticator app" });
    var out = el("div");
    start.addEventListener("click", function () {
      busy(start, function () { return api("POST", "/2fa/setup", {}); }).then(function (r) {
        var code = el("input", { type: "text", inputmode: "numeric", autocomplete: "one-time-code", maxlength: 8 });
        var confirm = el("button", { class: "primary", text: "Activate" });
        var res = el("div");
        confirm.addEventListener("click", function () {
          busy(confirm, function () { return api("POST", "/2fa/activate", { code: code.value }); }).then(function (a) {
            me.totp_enabled = true;
            box.replaceChildren(el("p", { class: "ok-msg", text: "Two factor is on." }), recoveryList(a.recovery_codes));
          }).catch(function (e) { msg(res, e.message, true); });
        });
        box.replaceChildren(
          el("p", { text: "Scan this QR code with your authenticator app, or enter the key by hand." }),
          el("img", { src: "data:image/svg+xml;utf8," + encodeURIComponent(r.qr_svg), alt: "Authenticator QR code", width: 220, height: 220 }),
          el("div", { class: "secret" }, [el("b", { text: "Key" }), el("span", { class: "mono", text: r.secret })]),
          field("Code from the app", code), confirm, res);
      }).catch(function (e) { msg(out, e.message, true); });
    });
    box.replaceChildren(el("p", { class: "muted", text: "Off. Accounts that refund payments or rotate keys should turn this on." + (st.operator_grace ? " Operators will be required to enable it." : "") }), start, out);
  }).catch(function (e) { msg(box, e.message, true); });
}

function twoFactorEnabled(box, st) {
  var pw = el("input", { type: "password", autocomplete: "current-password" });
  var code = el("input", { type: "text", inputmode: "numeric", autocomplete: "one-time-code", maxlength: 8 });
  var out = el("div");
  var regen = el("button", { text: "New recovery codes" });
  var off = el("button", { text: "Turn off two factor" });
  var body = function () { return { password: pw.value, code: code.value }; };
  regen.addEventListener("click", function () {
    busy(regen, function () { return api("POST", "/2fa/recovery-codes", body()); }).then(function (r) { out.replaceChildren(recoveryList(r.recovery_codes)); }).catch(function (e) { msg(out, e.message, true); });
  });
  off.addEventListener("click", function () {
    confirmDialog({ title: "Turn off two factor", body: "Signing in will only need your password.", keep: "Keep it on", confirm: "Turn off", danger: true }).then(function (r) {
      if (r) busy(off, function () { return api("POST", "/2fa/disable", body()); }).then(function () { me.totp_enabled = false; twoFactorCard(box); }).catch(function (e) { msg(out, e.message, true); });
    });
  });
  box.replaceChildren(el("p", { class: "ok-msg", text: "On. " + st.recovery_codes_remaining + " recovery codes left." }),
    field("Password", pw), field("Current code", code), el("div", { class: "row" }, [regen, off]), out);
}

