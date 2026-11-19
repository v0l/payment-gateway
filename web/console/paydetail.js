"use strict";
var EVENT_WORDS = {
  "payment.paid": "Paid", "payment.late_payment": "Paid after it closed", "payment.expired": "Expired unpaid", "payment.cancelled": "Cancelled",
  "payment.disputed": "Dispute opened", "payment.dispute_closed": "Dispute closed", "payment.marked_paid": "Marked paid", "payment.note_added": "Note added",
  "attempt.failed": "Attempt declined", "refund.requested": "Refund requested", "refund.completed": "Refund completed", "refund.failed": "Refund failed"
};

function timeline(pay, events, deliveries) {
  var items = [{ at: pay.created, text: "Created", sub: pay.reference, tone: "grey" }];
  pay.attempts.forEach(function (a) {
    items.push({ at: a.paid_at || a.expires, text: methodName(a.method) + " attempt " + statusWord(a.status).toLowerCase(), sub: a.details && (a.details.bolt11 ? a.details.bolt11.slice(0, 32) + "\u2026" : a.details.address || a.details.checkout_url || ""), tone: a.status === "paid" ? "" : a.status === "failed" ? "bad" : "grey", at_hint: a.paid_at ? null : "expires" });
  });
  events.forEach(function (e) {
    var d = e.detail || {};
    var extra = d.reason || d.error || (d.manual ? "Marked by hand" : "") || "";
    items.push({ at: e.created, text: EVENT_WORDS[e.type] || e.type, sub: extra, tone: /failed|disput|expired|cancel/.test(e.type) ? (e.type === "payment.dispute_closed" && d.dispute === "won" ? "" : "bad") : "" });
  });
  deliveries.forEach(function (d) {
    items.push({ at: d.delivered_at || d.last_attempt_at || d.created, text: "Webhook " + d.event_type + (d.delivered_at ? " delivered" : d.abandoned_at ? " given up" : " retrying"), sub: (d.url || "") + (d.last_status ? ", answered " + d.last_status : "") + (d.last_duration_ms ? " in " + d.last_duration_ms + " ms" : ""), tone: d.delivered_at ? "" : "bad" });
  });
  items.forEach(function (it, i) { it.n = i; });
  items.sort(function (a, b) { return (new Date(b.at) - new Date(a.at)) || (b.n - a.n); });
  return el("ol", { class: "steps" }, items.map(function (i) {
    return el("li", { class: i.tone }, [i.text, el("small", { text: (i.at_hint ? "Expires " : "") + fmtFull(i.at) + (i.sub ? ", " + i.sub : "") })]);
  }));
}

function whereMoneyWent(pay) {
  var a = shownAttempt(pay);
  var rows = [];
  var net = pay.amount;
  rows.push(el("tr", {}, [el("td", { text: "Customer paid" }), el("td", { class: "num muted", text: a && a.currency !== pay.currency ? (a.currency === "BTC" ? sats(a.amount) : fmtAmount(a.amount, a.currency)) : "" }), el("td", { class: "num", text: fmtAmount(pay.amount, pay.currency) })]));
  if (a && a.processing_fee) rows.push(el("tr", {}, [el("td", { text: "Processing fee added to the customer's total" }), el("td"), el("td", { class: "num muted", text: fmtAmount(a.processing_fee, pay.currency) })]));
  if (a && a.platform_fee) {
    rows.push(el("tr", {}, [el("td", { text: "Platform fee" }), el("td", { class: "num muted", text: sats(a.platform_fee) }), el("td")]));
  }
  (pay.refunds || []).filter(function (r) { return r.status !== "failed"; }).forEach(function (r) {
    net -= r.amount;
    rows.push(el("tr", {}, [el("td", { text: "Refund, " + statusWord(r.status).toLowerCase() }), el("td"), el("td", { class: "num" }, [signedMoney(-r.amount, pay.currency)])]));
  });
  var where = !a ? "Not paid yet" : a.platform_managed ? "Added to your balance with the platform" : a.method === "lightning" || a.method === "onchain" ? "Paid into your own " + methodName(a.method) + " wallet" : "Held by " + methodName(a.method) + " and paid out on its schedule";
  if (pay.status === "pending" || pay.status === "expired" || pay.status === "cancelled") {
    return section("Where the money went", [el("p", { class: "muted", text: pay.status === "pending" ? "Nothing has been paid yet. The customer has until " + fmtFull(pay.expires) + "." : "This payment was not paid, so no money moved." })], { cls: "flow" });
  }
  rows.push(el("tr", { class: "total" }, [el("td", { text: where }), el("td", { class: "num muted", text: a && a.net_credited ? sats(a.net_credited) : "" }), el("td", { class: "num", text: fmtAmount(net, pay.currency) })]));
  return section("Where the money went", [el("div", { class: "table-wrap plain" }, [el("table", {}, [el("tbody", {}, rows)])])], { cls: "flow", id: "money" });
}

function apiObject(pay) {
  var copy = JSON.parse(JSON.stringify(pay));
  return el("details", {}, [el("summary", { text: "Show the payment as the API returns it" }), el("pre", { text: JSON.stringify(copy, null, 2) })]);
}

function paymentDetail(pid, id) {
  projectShell(pid, "payments", function (p, body) {
    var box = el("div"); body.appendChild(box);
    function load() {
      api("GET", "/projects/" + pid + "/payments/" + id).then(function (d) {
        var pay = d.payment;
        var a = shownAttempt(pay);
        var out = el("div"); var deliveryHolder = el("div");
        var canWrite = p.role !== "viewer";
        var acts = [el("button", { type: "button", text: "Copy pay link", onclick: function () { copyText(pay.pay_url, "Pay link copied"); } })];
        if (pay.status === "pending" && canWrite) acts.push(el("button", { class: "danger", text: "Cancel payment", onclick: function () {
          confirmDialog({ title: "Cancel this payment", body: "The customer can no longer pay it.", keep: "Keep it", confirm: "Cancel payment", danger: true }).then(function (r) {
            if (r) api("POST", "/projects/" + pid + "/payments/" + id + "/cancel", {}).then(load).catch(function (e) { msg(out, e.message, true); });
          });
        } }));
        var refundBox = el("div");
        if ((pay.status === "paid" || pay.status === "partially_refunded") && canWrite) acts.push(el("button", { text: "Refund", onclick: function () {
          var amt = el("input", { type: "number", placeholder: "Amount in minor units, blank for all", "aria-label": "Refund amount" });
          var dest = el("input", { placeholder: "Lightning invoice or address, for bitcoin payments", "aria-label": "Refund destination" });
          refundBox.replaceChildren(el("div", { class: "card" }, [el("h3", { text: "Refund" }), el("div", { class: "row" }, [amt, dest, el("button", { class: "danger", text: "Preview refund", onclick: function () { refundFlow(pid, id, pay, amt.value, dest.value.trim(), out, load); } })])]));
          amt.focus();
        } }));
        var dispute = pay.dispute_status;
        var head = el("div", {}, [
          el("a", { class: "crumb", href: "#/p/" + pid + "/payments", text: "Payments" }),
          el("div", { class: "dhead" }, [bigMoney(pay.amount, pay.currency), el("span", { class: "muted", text: pay.currency }), status(pay.status), dispute ? status(dispute === "open" ? "disputed" : "completed", dispute === "open" ? "Disputed" : "Dispute " + dispute) : null, el("span", { class: "sp" }), el("div", { class: "actions" }, acts)]),
          el("p", { class: "dsub", text: [pay.description || pay.reference, pay.customer_ref, a && pay.paid_at ? "paid over " + methodName(a.method) + " " + fmtFull(pay.paid_at) : "created " + fmtFull(pay.created)].filter(Boolean).join(", ") })
        ]);
        var lines = pay.line_items && pay.line_items.length ? section("Line items", [table(["Item", ["Qty", "num"], ["Unit", "num"], ["Discount", "num"], ["Tax", "num"], ["Total", "num"]], pay.line_items.map(function (l) {
          return el("tr", {}, [el("td", { text: l.name + (l.sku ? " (" + l.sku + ")" : "") }), el("td", { class: "num", text: String(l.quantity) }), el("td", { class: "num", text: fmtAmount(l.unit_amount, pay.currency) }),
            el("td", { class: "num", text: l.discount ? fmtAmount(l.discount, pay.currency) : "" }), el("td", { class: "num", text: l.tax ? fmtAmount(l.tax, pay.currency) : "" }), el("td", { class: "num", text: fmtAmount(l.total, pay.currency) })]);
        }))], { id: "lines" }) : null;
        var attempts = section("Attempts", [pay.attempts.length ? table(["Method", "Status", ["Charged", "num"], ["Fee", "num"], "Reference"], pay.attempts.map(function (x) {
          var det = x.details || {};
          return el("tr", {}, [el("td", { text: methodName(x.method) + (x.option ? " " + x.option : "") + (x.platform_managed ? " (platform)" : "") }), el("td", {}, [status(x.status)]),
            el("td", { class: "num", text: x.currency === "BTC" ? sats(x.amount) : fmtAmount(x.amount, x.currency) }), el("td", { class: "num", text: x.processing_fee ? fmtAmount(x.processing_fee, pay.currency) : "" }),
            el("td", { class: "mono trunc", text: det.bolt11 ? det.bolt11.slice(0, 28) + "\u2026" : det.address || det.checkout_url || det.deposit_address || "" })]);
        })) : el("p", { class: "muted", text: "The customer has not chosen a method yet." })], { id: "attempts" });
        var receipts = d.receipts.length ? section("Received on the network", [table(["Transaction", ["Amount", "num"], "State"], d.receipts.map(function (r) {
          return el("tr", {}, [el("td", { class: "mono trunc", text: r.ref }), el("td", { class: "num", text: sats(r.amount) }), el("td", {}, [status(r.confirmed ? "completed" : "pending", r.confirmed ? "Confirmed" : "Unconfirmed")])]);
        }))], { id: "receipts" }) : null;
        var hooks = section("Webhooks", [d.deliveries.length ? table(["Endpoint", "Event", "Response", ["Tries", "num"], ""], d.deliveries.map(function (x) {
          return el("tr", {}, [el("td", { class: "trunc", text: x.url || "Default endpoint" }), el("td", { text: x.event_type }),
            el("td", {}, [status(x.delivered_at ? "delivered" : x.abandoned_at ? "failed" : "waiting", x.last_status ? String(x.last_status) + (x.last_duration_ms ? ", " + x.last_duration_ms + " ms" : "") : (x.delivered_at ? "Delivered" : x.abandoned_at ? "Gave up" : "Waiting"))]),
            el("td", { class: "num", text: String(x.attempts) }),
            el("td", {}, [el("button", { class: "link", type: "button", text: "Details", onclick: function () { deliveryDetail(pid, x.id, deliveryHolder); } }), " ", el("button", { class: "link", type: "button", text: "Send again", disabled: !canWrite, onclick: function () { api("POST", "/projects/" + pid + "/deliveries/" + x.id + "/replay", {}).then(function () { toast("Webhook queued."); load(); }); } })])]);
        })) : el("p", { class: "muted", text: "No webhook endpoint received an event for this payment." }), deliveryHolder], { id: "webhooks" });
        var cost = section("Cost", [kv([["Charged to the customer", a ? (a.currency === "BTC" ? sats(a.amount) + ", " + fmtAmount(a.base_amount, pay.currency) : fmtAmount(a.amount, a.currency)) : null], ["Processing fee", a && a.processing_fee ? fmtAmount(a.processing_fee, pay.currency) : "None"], ["Platform fee", a && a.platform_fee ? sats(a.platform_fee) : "None"], ["Credited to you", a && a.net_credited ? sats(a.net_credited) : (a && a.status === "paid" ? fmtAmount(pay.amount, pay.currency) : null)]])], { id: "cost" });
        var customer = section("Customer", [kv([["Customer ref", pay.customer_ref ? el("a", { href: "#/p/" + pid + "/customers/" + encodeURIComponent(pay.customer_ref), text: pay.customer_ref }) : null], ["Email", d.customer_email], ["Receipt", pay.receipt_url ? el("a", { href: pay.receipt_url, target: "_blank", rel: "noopener", text: "Open receipt" }) : null]])], { id: "customer" });
        var meta = section("Metadata", [pay.metadata && Object.keys(pay.metadata).length ? kv(Object.keys(pay.metadata).map(function (k) { var v = pay.metadata[k]; return [k, typeof v === "string" ? v : JSON.stringify(v), "mono"]; })) : el("p", { class: "muted", text: "No metadata. Pass metadata when you create the payment to see it here." })], { id: "metadata" });
        var tools = paymentTools(pid, id, pay, p.role, out, load);
        var main = el("div", {}, [head, out, refundBox, whereMoneyWent(pay), section("Timeline", [timeline(pay, d.events || [], d.deliveries)], { id: "timeline" }), lines, attempts, receipts, hooks, cost, customer, meta, section("Notes", [tools], { id: "notes" }), section("API object", [apiObject(pay)], { id: "api" })].filter(Boolean));
        var toc = el("nav", { class: "toc", "aria-label": "On this page" }, [["money", "Where the money went"], ["timeline", "Timeline"], ["attempts", "Attempts"], ["webhooks", "Webhooks"], ["cost", "Cost"], ["customer", "Customer"], ["metadata", "Metadata"], ["notes", "Notes"], ["api", "API object"]].map(function (x) {
          return el("a", { href: "#/p/" + pid + "/payments/" + id, text: x[1], onclick: function (e) { e.preventDefault(); var t = document.getElementById(x[0]); if (t) t.scrollIntoView({ block: "start" }); } });
        }));
        var side = el("aside", { class: "side-card", "aria-label": "Properties" }, [el("h2", { text: "Properties" }), kv([["Payment id", el("span", { class: "mono", text: pay.id.slice(0, 20) + "\u2026", title: pay.id })], ["Reference", pay.reference], ["Created", fmtFull(pay.created)], ["Expires", pay.status === "pending" ? fmtFull(pay.expires) : "Closed"], ["Paid", pay.paid_at ? fmtFull(pay.paid_at) : null], ["Method", a ? methodName(a.method) : null], ["Pay page", el("a", { href: pay.pay_url, target: "_blank", rel: "noopener", text: "Open" })]])]);
        box.replaceChildren(el("div", { class: "detail" }, [toc, main, side]));
      }).catch(function (e) { box.replaceChildren(el("div", { class: "err", role: "alert", text: e.message })); });
    }
    load();
    stopTimer(); timer = setInterval(function () { var a = document.activeElement; if (a && box.contains(a) && /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName)) return; if (deliveryOpen() || box.querySelector("details[open]") || box.querySelector(".card")) return; load(); }, 5000);
  }, { bare: true });
}
