"use strict";
function account() {
  var cur = el("input", { type: "password", autocomplete: "current-password" });
  var nw = el("input", { type: "password", autocomplete: "new-password" });
  var out = el("div"); var btn = el("button", { class: "primary", text: "Change password" });
  btn.addEventListener("click", function () {
    busy(btn, function () { return api("POST", "/password", { current: cur.value, new: nw.value }); }).then(function () { cur.value = ""; nw.value = ""; msg(out, "Password changed. Other sessions were signed out."); }).catch(function (e) { msg(out, e.message, true); });
  });
  var cards = [field("Current password", cur), field("New password (10+ characters)", nw), btn, out];
  var sections = [el("h2", { text: "Account" }), el("p", { class: "sub", text: me.email }), el("div", { class: "card" }, cards)];
  if (me.mail_enabled) {
    var pwNow = el("input", { type: "password", autocomplete: "current-password" });
    var newEmail = el("input", { type: "email", autocomplete: "off" });
    var emailOut = el("div"); var emailBtn = el("button", { class: "primary", text: "Send confirmation" });
    emailBtn.addEventListener("click", function () {
      busy(emailBtn, function () { return api("POST", "/email", { password: pwNow.value, new_email: newEmail.value }); }).then(function (r) { pwNow.value = ""; msg(emailOut, "Confirmation sent to " + r.email + ". The change applies once you open the link."); }).catch(function (e) { msg(emailOut, e.message, true); });
    });
    sections.push(el("h3", { text: "Change email" }));
    sections.push(el("div", { class: "card" }, [field("New email", newEmail), field("Current password", pwNow), emailBtn, emailOut]));
    var box = el("input", { type: "checkbox" }); box.checked = me.notify;
    var notifyOut = el("div");
    box.addEventListener("change", function () {
      api("POST", "/notifications", { enabled: box.checked }).then(function (r) { me.notify = r.notify; msg(notifyOut, r.notify ? "Notifications on." : "Notifications off."); }).catch(function (e) { box.checked = me.notify; msg(notifyOut, e.message, true); });
    });
    sections.push(el("h3", { text: "Notifications" }));
    sections.push(el("div", { class: "card" }, [el("label", {}, [box, el("span", { text: " Email me about failed webhooks, failing providers and failed refunds on projects I own" })]), notifyOut]));
  }
  layout("account", null, el("div", {}, sections));
}

