"use strict";
function account() {
  var cur = el("input", { type: "password", autocomplete: "current-password" });
  var nw = el("input", { type: "password", autocomplete: "new-password" });
  var out = el("div"); var btn = el("button", { class: "primary", text: "Change password" });
  btn.addEventListener("click", function () {
    busy(btn, function () { return api("POST", "/password", { current: cur.value, new: nw.value }); }).then(function () { cur.value = ""; nw.value = ""; msg(out, "Password changed. Other sessions were signed out."); }).catch(function (e) { msg(out, e.message, true); });
  });
  layout("account", null, el("div", {}, [el("h2", { text: "Account" }), el("p", { class: "sub", text: me.email }), el("div", { class: "card" }, [field("Current password", cur), field("New password (10+ characters)", nw), btn, out])]));
}

