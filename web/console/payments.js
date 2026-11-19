"use strict";
var STATUSES = ["", "pending", "paid", "expired", "cancelled", "refunded", "partially_refunded"];

function newPayment(pid) {
  projectShell(pid, "payments", function (p, body) {
    var ref = el("input", { value: "order-" + Math.floor(Date.now() / 1000) });
    var amount = el("input", { type: "number", value: "1500", min: "1" });
    var cur = el("select", {}, ["EUR", "USD", "GBP", "BTC"].map(function (c) { return el("option", { value: c, text: c }); }));
    var desc = el("input", { placeholder: "Shown to the customer on the pay page" });
    var out = el("div"); var btn = el("button", { class: "primary", text: "Create payment" });
    btn.addEventListener("click", function () {
      busy(btn, function () { return api("POST", "/projects/" + pid + "/payments", { reference: ref.value, amount: Number(amount.value), currency: cur.value, description: desc.value || null }); })
        .then(function (r) { go("#/p/" + pid + "/payments/" + r.id); }).catch(function (e) { msg(out, e.message, true); });
    });
    body.appendChild(el("div", { class: "card form" }, [field("Reference", ref), field("Currency", cur), field("Amount in minor units (cents, or msats for BTC)", amount), field("Description", desc), el("div", { class: "row" }, [btn, el("a", { href: "#/p/" + pid + "/payments", text: "Cancel" })]), out]));
  }, { sub: "Creates a pay link you can send to a customer. Your app usually does this through the API." });
}

function deliveryOpen() { return !!document.querySelector(".delivery-open"); }
