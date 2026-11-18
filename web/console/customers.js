"use strict";
function customers(pid) {
  projectShell(pid, "customers", function (p, body) {
    var q = el("input", { placeholder: "Customer ref starts with", "aria-label": "Search customers" });
    var list = el("div", { class: "list-area" });
    function load() {
      api("GET", "/projects/" + pid + "/customers" + (q.value.trim() ? "?q=" + encodeURIComponent(q.value.trim()) : "")).then(function (r) {
        list.replaceChildren(r.customers.length ? el("table", {}, [el("tr", {}, ["Customer", "Payments", "Paid", "Last payment"].map(function (h) { return el("th", { text: h }); }))].concat(r.customers.map(function (c) {
          var open = function () { go("#/p/" + pid + "/customers/" + encodeURIComponent(c.customer_ref)); };
          return el("tr", { class: "click", tabindex: "0", onclick: open, onkeydown: function (e) { if (e.key === "Enter") open(); } }, [el("td", { text: c.customer_ref }), el("td", { text: c.payments }), el("td", { text: c.paid }), el("td", { class: "muted", text: ago(c.last_payment) })]);
        }))) : el("p", { class: "muted", text: "No customers yet. Pass customer_ref when creating payments." }));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    q.addEventListener("keydown", function (e) { if (e.key === "Enter") load(); });
    body.appendChild(el("div", { class: "row" }, [q, el("button", { text: "Search", onclick: load })]));
    body.appendChild(list); load();
  });
}

function customerDetail(pid, ref) {
  projectShell(pid, "customers", function (p, body) {
    var box = el("div"); body.appendChild(box);
    function load() {
      api("GET", "/projects/" + pid + "/customers/" + encodeURIComponent(ref)).then(function (c) {
        var out = el("div");
        var totals = Object.keys(c.paid_totals).map(function (k) { return fmtAmount(c.paid_totals[k], k); }).join(", ");
        box.replaceChildren(
          el("div", { class: "card" }, [el("dl", { class: "kv" }, [el("dt", { text: "Customer" }), el("dd", { text: c.customer_ref }), el("dt", { text: "Email" }), el("dd", { text: c.email || "" }), el("dt", { text: "Paid in total" }), el("dd", { text: totals || "nothing yet" })])]),
          el("h3", { text: "Payments" }),
          c.payments.length ? el("table", {}, [el("tr", {}, ["Id", "Reference", "Amount", "Status", "Created"].map(function (h) { return el("th", { text: h }); }))].concat(c.payments.map(function (x) {
            var open = function () { go("#/p/" + pid + "/payments/" + x.id); };
            return el("tr", { class: "click", tabindex: "0", onclick: open, onkeydown: function (e) { if (e.key === "Enter") open(); } }, [el("td", { class: "mono", text: x.id.slice(0, 8) }), el("td", { text: x.reference }), el("td", { text: fmtAmount(x.amount, x.currency) }), el("td", {}, [pill(x.status)]), el("td", { class: "muted", text: ago(x.created) })]);
          }))) : el("p", { class: "muted", text: "No payments." }),
          el("h3", { text: "Saved methods" }),
          c.saved_methods.length ? el("table", {}, c.saved_methods.map(function (m) {
            return el("tr", {}, [el("td", { text: (m.brand || "card") + " ending " + (m.last4 || "????") }), el("td", { class: "muted", text: m.exp_month ? m.exp_month + "/" + m.exp_year : "" }),
              el("td", {}, [el("button", { class: "danger", text: "Delete", disabled: p.role === "viewer", onclick: function () {
                confirmDialog({ title: "Delete saved method", body: "Delete " + (m.brand || "card") + " ending " + (m.last4 || "????") + "? The customer will have to enter it again.", keep: "Keep it", confirm: "Delete", danger: true }).then(function (r) {
                  if (!r) return;
                  api("DELETE", "/projects/" + pid + "/customers/" + encodeURIComponent(ref) + "/saved-methods/" + m.id).then(function () { toast("Saved method deleted."); load(); }).catch(function (e) { msg(out, e.message, true); });
                });
              } })])]);
          })) : el("p", { class: "muted", text: "No saved methods." }),
          el("h3", { text: "Subscriptions" }),
          c.subscriptions.length ? el("p", { text: String(c.subscriptions.length) }) : el("p", { class: "muted", text: "No subscriptions." }),
          out);
      }).catch(function (e) { msg(box, e.message, true); });
    }
    load();
  });
}
