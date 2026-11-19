"use strict";
function customers(pid) {
  projectShell(pid, "customers", function (p, body) {
    var q = el("input", { type: "search", placeholder: "Customer ref starts with", "aria-label": "Search customers" });
    var list = el("div", { class: "list-area" }, [skeletonRows()]);
    function load() {
      api("GET", "/projects/" + pid + "/customers" + (q.value.trim() ? "?q=" + encodeURIComponent(q.value.trim()) : "")).then(function (r) {
        list.replaceChildren(r.customers.length ? table(["Customer", ["Payments", "num"], ["Paid", "num"], "Last payment"], r.customers.map(function (c) {
          return clickRow({}, [el("td", { text: c.customer_ref }), el("td", { class: "num", text: String(c.payments) }), el("td", { class: "num", text: String(c.paid) }), el("td", { class: "muted", text: fmtWhen(c.last_payment) })], function () { go("#/p/" + pid + "/customers/" + encodeURIComponent(c.customer_ref)); });
        })) : el("div", { class: "empty" }, [el("b", { text: "No customers yet." }), el("p", { class: "muted", text: "Pass customer_ref when your app creates a payment and customers appear here." })]));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    q.addEventListener("keydown", function (e) { if (e.key === "Enter") load(); });
    body.appendChild(el("div", { class: "filterbar" }, [q, el("button", { text: "Search", onclick: load })]));
    body.appendChild(list); load();
  }, { sub: "Everyone who paid you, by the customer_ref your app sends." });
}

function customerDetail(pid, ref) {
  projectShell(pid, "customers", function (p, body) {
    var box = el("div", {}, [skeletonRows()]); body.appendChild(box);
    body.querySelector(".page-head h1").textContent = ref;
    function load() {
      api("GET", "/projects/" + pid + "/customers/" + encodeURIComponent(ref)).then(function (c) {
        var out = el("div");
        var totals = Object.keys(c.paid_totals).map(function (k) { return fmtAmount(c.paid_totals[k], k); }).join(", ");
        box.replaceChildren(
          section("Customer", [kv([["Customer ref", c.customer_ref], ["Email", c.email], ["Paid in total", totals || "Nothing yet"]])]),
          section("Payments", [c.payments.length ? table([["Amount", "num"], "Status", "Reference", "Created"], c.payments.map(function (x) {
            return clickRow({}, [el("td", { class: "amt", text: fmtAmount(x.amount, x.currency) }), el("td", {}, [status(x.status)]), el("td", { text: x.reference }), el("td", { class: "muted", text: fmtWhen(x.created) })], function () { go("#/p/" + pid + "/payments/" + x.id); });
          })) : el("p", { class: "muted", text: "No payments." })]),
          section("Saved cards", [c.saved_methods.length ? table(["Card", "Expires", ""], c.saved_methods.map(function (m) {
            return el("tr", {}, [el("td", { text: (m.brand || "Card") + " ending " + (m.last4 || "????") }), el("td", { class: "muted", text: m.exp_month ? m.exp_month + "/" + m.exp_year : "" }),
              el("td", { class: "num" }, [el("button", { class: "link", type: "button", text: "Delete", disabled: p.role === "viewer", onclick: function () {
                confirmDialog({ title: "Delete saved card", body: "Delete " + (m.brand || "card") + " ending " + (m.last4 || "????") + "? The customer will have to enter it again.", keep: "Keep it", confirm: "Delete", danger: true }).then(function (r) {
                  if (!r) return;
                  api("DELETE", "/projects/" + pid + "/customers/" + encodeURIComponent(ref) + "/saved-methods/" + m.id).then(function () { toast("Saved card deleted."); load(); }).catch(function (e) { msg(out, e.message, true); });
                });
              } })])]);
          })) : el("p", { class: "muted", text: "No saved cards." })]),
          section("Subscriptions", [c.subscriptions.length ? table(["Plan", ["Amount", "num"], "Status", "Next charge"], c.subscriptions.map(function (x) {
            return clickRow({}, [el("td", { text: x.plan_name || "Plan " + x.plan_id }), el("td", { class: "num", text: x.amount === null ? "" : fmtAmount(x.amount, x.currency) }), el("td", {}, [status(x.status, x.cancel_at_period_end ? statusWord(x.status) + ", ends" : null)]), el("td", { class: "muted", text: x.next_charge ? fmtDay(x.next_charge) : "" })], function () { go("#/p/" + pid + "/subscriptions/" + x.id); });
          })) : el("p", { class: "muted", text: "No subscriptions." })]),
          out);
      }).catch(function (e) { msg(box, e.message, true); });
    }
    load();
  });
}
