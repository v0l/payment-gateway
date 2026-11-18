"use strict";
var STATUSES = ["", "pending", "paid", "expired", "cancelled", "refunded", "partially_refunded"];

function payments(pid) {
  projectShell(pid, "payments", function (p, body) {
    var q = el("input", { placeholder: "Search reference or id" });
    var st = el("select", {}, STATUSES.map(function (s) { return el("option", { value: s, text: s || "any status" }); }));
    var list = el("div"); var more = el("div");
    var rows = []; var next = null;
    function load(reset) {
      if (reset) { rows = []; next = null; }
      var qs = "?status=" + encodeURIComponent(st.value) + "&q=" + encodeURIComponent(q.value) + (next ? "&before=" + encodeURIComponent(next) : "");
      api("GET", "/projects/" + pid + "/payments" + qs).then(function (r) {
        rows = rows.concat(r.payments); next = r.next; draw();
      }).catch(function (e) { msg(list, e.message, true); });
    }
    function draw() {
      list.replaceChildren(rows.length ? el("table", {}, [el("tr", {}, ["Id", "Reference", "Amount", "Status", "Method", "Created"].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (r) {
        var paid = r.attempts.filter(function (a) { return a.status === "paid"; })[0];
        var tr = el("tr", { class: "click", onclick: function () { go("#/p/" + pid + "/payments/" + r.id); } }, [
          el("td", { class: "mono", text: r.id.slice(0, 8) }), el("td", { text: r.reference }), el("td", { text: fmtAmount(r.amount, r.currency) }),
          el("td", {}, [pill(r.status)]), el("td", { text: paid ? paid.method : "" }), el("td", { class: "muted", text: ago(r.created) })]);
        return tr;
      }))) : el("p", { class: "muted", text: "No payments match." }));
      more.replaceChildren(next ? el("button", { text: "Load more", onclick: function () { load(false); } }) : null);
    }
    q.addEventListener("keydown", function (e) { if (e.key === "Enter") load(true); });
    st.addEventListener("change", function () { load(true); });
    body.appendChild(el("div", { class: "row" }, [q, st, el("button", { text: "Search", onclick: function () { load(true); } }), el("button", { class: "primary", text: "New payment", onclick: function () { go("#/p/" + pid + "/payments/new"); } })]));
    body.appendChild(list); body.appendChild(more);
    load(true);
  });
}

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

function paymentDetail(pid, id) {
  projectShell(pid, "payments", function (p, body) {
    var box = el("div"); body.appendChild(box);
    function load() {
      api("GET", "/projects/" + pid + "/payments/" + id).then(function (d) {
        var pay = d.payment;
        var actions = el("div", { class: "row" });
        var out = el("div");
        if (pay.status === "pending") {
          actions.appendChild(el("button", { class: "danger", text: "Cancel payment", onclick: function () {
            api("POST", "/projects/" + pid + "/payments/" + id + "/cancel", {}).then(load).catch(function (e) { msg(out, e.message, true); });
          } }));
        }
        if (pay.status === "paid" || pay.status === "partially_refunded") {
          var amt = el("input", { type: "number", placeholder: "Amount (blank = all remaining)" });
          var dest = el("input", { placeholder: "Lightning invoice or address (for BTC payments)" });
          actions.appendChild(amt); actions.appendChild(dest);
          actions.appendChild(el("button", { class: "danger", text: "Refund", onclick: function () {
            if (!confirm("Refund this payment?")) return;
            api("POST", "/projects/" + pid + "/payments/" + id + "/refund", { amount: amt.value ? Number(amt.value) : null, destination: dest.value || null })
              .then(function (r) { msg(out, "Refund " + r.status + (r.error ? ": " + r.error : ""), r.status === "failed"); load(); }).catch(function (e) { msg(out, e.message, true); });
          } }));
        }
        box.replaceChildren(
          el("div", { class: "card" }, [el("dl", { class: "kv" }, [
            el("dt", { text: "Id" }), el("dd", { class: "mono", text: pay.id }),
            el("dt", { text: "Reference" }), el("dd", { text: pay.reference }),
            el("dt", { text: "Amount" }), el("dd", { text: fmtAmount(pay.amount, pay.currency) }),
            el("dt", { text: "Status" }), el("dd", {}, [pill(pay.status)]),
            el("dt", { text: "Created" }), el("dd", { text: new Date(pay.created).toLocaleString() }),
            el("dt", { text: "Expires" }), el("dd", { text: new Date(pay.expires).toLocaleString() }),
            el("dt", { text: "Paid" }), el("dd", { text: pay.paid_at ? new Date(pay.paid_at).toLocaleString() : "" }),
            el("dt", { text: "Customer" }), el("dd", { text: pay.customer_ref || "" }),
            el("dt", { text: "Pay page" }), el("dd", {}, [el("a", { href: pay.pay_url, target: "_blank", rel: "noopener", text: pay.pay_url })]),
            el("dt", { text: "Metadata" }), el("dd", { class: "mono", text: pay.metadata ? JSON.stringify(pay.metadata) : "" })
          ]), actions, out]),
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
              el("td", {}, [el("a", { text: "replay", onclick: function () { api("POST", "/projects/" + pid + "/deliveries/" + x.id + "/replay", {}).then(load); } })])]);
          })) : el("p", { class: "muted", text: "None." })
        );
      }).catch(function (e) { box.replaceChildren(el("div", { class: "err", text: e.message })); });
    }
    load();
    stopTimer(); timer = setInterval(load, 5000);
  });
}

