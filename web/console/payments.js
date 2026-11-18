"use strict";
var STATUSES = ["", "pending", "paid", "expired", "cancelled", "refunded", "partially_refunded"];

function newPayment(pid) {
  projectShell(pid, "payments", function (p, body) {
    var ref = el("input", { value: "order-" + Math.floor(Date.now() / 1000) });
    var amount = el("input", { type: "number", value: "1500", min: "1" });
    var cur = el("select", {}, ["EUR", "USD", "GBP", "BTC"].map(function (c) { return el("option", { value: c, text: c }); }));
    var desc = el("input", { placeholder: "Optional description" });
    var out = el("div"); var btn = el("button", { class: "primary", text: "Create" });
    btn.addEventListener("click", function () {
      busy(btn, function () { return api("POST", "/projects/" + pid + "/payments", { reference: ref.value, amount: Number(amount.value), currency: cur.value, description: desc.value || null }); })
        .then(function (r) { go("#/p/" + pid + "/payments/" + r.id); }).catch(function (e) { msg(out, e.message, true); });
    });
    body.appendChild(el("div", { class: "card" }, [field("Reference", ref), field("Amount (minor units: cents, or msats for BTC)", amount), field("Currency", cur), field("Description", desc), btn, out]));
  });
}

function deliveryOpen() { return !!document.querySelector(".delivery-open"); }

function paymentDetail(pid, id) {
  projectShell(pid, "payments", function (p, body) {
    var box = el("div"); body.appendChild(box);
    function load() {
      api("GET", "/projects/" + pid + "/payments/" + id).then(function (d) {
        var pay = d.payment;
        var actions = el("div", { class: "row" });
        var out = el("div"); var deliveryHolder = el("div");
        if (pay.status === "pending") {
          actions.appendChild(el("button", { class: "danger", text: "Cancel payment", onclick: function () {
            api("POST", "/projects/" + pid + "/payments/" + id + "/cancel", {}).then(load).catch(function (e) { msg(out, e.message, true); });
          } }));
        }
        if (pay.status === "paid" || pay.status === "partially_refunded") {
          var amt = el("input", { type: "number", placeholder: "Amount (blank = all remaining)", "aria-label": "Refund amount" });
          var dest = el("input", { placeholder: "Lightning invoice or address (for BTC payments)", "aria-label": "Refund destination" });
          actions.appendChild(amt); actions.appendChild(dest);
          actions.appendChild(el("button", { class: "danger", text: "Refund", onclick: function () {
            refundFlow(pid, id, pay, amt.value, dest.value.trim(), out, load);
          } }));
        }
        box.replaceChildren(
          el("div", { class: "card" }, [el("dl", { class: "kv" }, [
            el("dt", { text: "Id" }), el("dd", { class: "mono", text: pay.id }),
            el("dt", { text: "Reference" }), el("dd", { text: pay.reference }),
            el("dt", { text: "Amount" }), el("dd", { text: fmtAmount(pay.amount, pay.currency) }),
            el("dt", { text: "Status" }), el("dd", {}, [pill(pay.status), pay.dispute_status ? " " : "", pay.dispute_status ? pill(pay.dispute_status === "open" ? "disputed" : "dispute " + pay.dispute_status, pay.dispute_status === "won" ? "ok" : "bad") : ""]),
            el("dt", { text: "Created" }), el("dd", { text: new Date(pay.created).toLocaleString() }),
            el("dt", { text: "Expires" }), el("dd", { text: new Date(pay.expires).toLocaleString() }),
            el("dt", { text: "Paid" }), el("dd", { text: pay.paid_at ? new Date(pay.paid_at).toLocaleString() : "" }),
            el("dt", { text: "Customer" }), el("dd", { text: pay.customer_ref || "" }),
            el("dt", { text: "Receipt" }), el("dd", {}, [pay.receipt_url ? el("a", { href: pay.receipt_url, target: "_blank", rel: "noopener", text: pay.receipt_url }) : null]),
            el("dt", { text: "Pay page" }), el("dd", {}, [el("a", { href: pay.pay_url, target: "_blank", rel: "noopener", text: pay.pay_url })]),
            el("dt", { text: "Metadata" }), el("dd", { class: "mono", text: pay.metadata ? JSON.stringify(pay.metadata) : "" })
          ]), actions, out]),
          (pay.line_items && pay.line_items.length) ? el("h3", { text: "Line items" }) : null,
          (pay.line_items && pay.line_items.length) ? el("table", {}, [el("tr", {}, ["Item", "Qty", "Unit", "Discount", "Tax", "Total"].map(function (h) { return el("th", { text: h }); }))].concat(pay.line_items.map(function (l) {
            return el("tr", {}, [el("td", { text: l.name + (l.sku ? " (" + l.sku + ")" : "") }), el("td", { text: String(l.quantity) }), el("td", { text: fmtAmount(l.unit_amount, pay.currency) }),
              el("td", { text: l.discount ? fmtAmount(l.discount, pay.currency) : "" }), el("td", { text: l.tax ? fmtAmount(l.tax, pay.currency) : "" }), el("td", { text: fmtAmount(l.total, pay.currency) })]);
          }))) : null,
          paymentTools(pid, id, pay, p.role, out, load),
          el("h3", { text: "Attempts" }),
          pay.attempts.length ? el("table", {}, [el("tr", {}, ["Method", "Status", "Charge", "Fee", "Details"].map(function (h) { return el("th", { text: h }); }))].concat(pay.attempts.map(function (a) {
            var det = a.details || {};
            return el("tr", {}, [el("td", { text: a.method + (a.option ? " " + a.option : "") }), el("td", {}, [pill(a.status)]), el("td", { text: fmtAmount(a.amount, a.currency) }),
              el("td", { text: a.processing_fee ? fmtAmount(a.processing_fee, pay.currency) : "" }),
              el("td", { class: "mono", text: det.bolt11 ? det.bolt11.slice(0, 36) + "..." : det.address || det.checkout_url || det.deposit_address || "" })]);
          }))) : el("p", { class: "muted", text: "No attempts yet." }),
          el("h3", { text: "Receipts" }),
          d.receipts.length ? el("table", {}, d.receipts.map(function (r) { return el("tr", {}, [el("td", { class: "mono", text: r.ref }), el("td", { text: r.amount }), el("td", {}, [pill(r.confirmed ? "confirmed" : "unconfirmed", r.confirmed ? "ok" : "pending")])]); })) : el("p", { class: "muted", text: "None." }),
          el("h3", { text: "Refunds" }),
          pay.refunds.length ? el("table", {}, pay.refunds.map(function (r) { return el("tr", {}, [el("td", { text: fmtAmount(r.amount, pay.currency) }), el("td", {}, [pill(r.status)]), el("td", { class: "muted", text: r.error || "" })]); })) : el("p", { class: "muted", text: "None." }),
          el("h3", { text: "Webhooks sent" }),
          d.deliveries.length ? el("table", {}, d.deliveries.map(function (x) {
            return el("tr", {}, [el("td", { text: x.event_type }), el("td", {}, [pill(x.delivered_at ? "delivered" : x.abandoned_at ? "failed" : "waiting", x.delivered_at ? "ok" : x.abandoned_at ? "bad" : "pending")]), el("td", { text: x.attempts + " tries" }), el("td", { class: "muted", text: x.last_error || "" }),
              el("td", {}, [el("a", { text: "replay", onclick: function () { api("POST", "/projects/" + pid + "/deliveries/" + x.id + "/replay", {}).then(load); } }), " ", el("a", { text: "detail", onclick: function () { deliveryDetail(pid, x.id, deliveryHolder); } })])]);
          })) : el("p", { class: "muted", text: "None." }),
          deliveryHolder
        );
      }).catch(function (e) { box.replaceChildren(el("div", { class: "err", text: e.message })); });
    }
    load();
    stopTimer(); timer = setInterval(function () { var a = document.activeElement; if (a && box.contains(a) && /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName)) return; if (deliveryOpen()) return; load(); }, 5000);
  });
}

