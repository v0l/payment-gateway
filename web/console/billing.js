"use strict";
var SUB_PILL = { active: "ok", trialing: "info", incomplete: "pending", past_due: "failed", paused: "neutral", cancelled: "cancelled" };
function subPill(status) { return pill(status, SUB_PILL[status] || status); }
function when(iso) { return iso ? new Date(iso).toLocaleString() : ""; }
function planLabel(p) { return fmtAmount(p.amount, p.currency) + " / " + (p.interval_count > 1 ? p.interval_count + " " : "") + p.interval; }

function plans(pid) {
  projectShell(pid, "plans", function (p, body) {
    var list = el("div"); var out = el("div");
    var name = el("input", { placeholder: "Pro" });
    var amount = el("input", { type: "number", placeholder: "1500" });
    var cur = el("input", { value: "EUR" });
    var unit = el("select", {}, ["month", "year", "week", "day"].map(function (u) { return el("option", { value: u, text: u }); }));
    var count = el("input", { type: "number", value: "1" });
    var trial = el("input", { type: "number", value: "0" });
    var grace = el("input", { type: "number", value: "3" });
    var btn = el("button", { class: "primary", text: "Create plan" });
    btn.addEventListener("click", function () {
      busy(btn, function () {
        return api("POST", "/projects/" + pid + "/plans", { name: name.value, amount: Number(amount.value), currency: cur.value, interval: unit.value, interval_count: Number(count.value), trial_days: Number(trial.value), grace_days: Number(grace.value) });
      }).then(function () { name.value = ""; msg(out, "Plan created", false); load(); }).catch(function (e) { msg(out, e.message, true); });
    });
    body.appendChild(el("div", { class: "card" }, [field("Name", name), field("Amount (minor units)", amount), field("Currency", cur), field("Interval", unit), field("Every N intervals", count), field("Trial days", trial), field("Grace days for pay-link methods", grace), btn, out]));
    body.appendChild(list);
    function load() {
      api("GET", "/projects/" + pid + "/plans").then(function (rows) {
        if (!rows.length) { list.replaceChildren(el("p", { class: "muted", text: "No plans yet." })); return; }
        var head = ["Id", "Name", "Price", "Trial", "Grace", "Status", ""];
        list.replaceChildren(el("table", {}, [el("tr", {}, head.map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (r) {
          var act = r.active ? el("button", { text: "Archive", onclick: function () {
            api("POST", "/projects/" + pid + "/plans/" + r.id + "/archive", {}).then(load).catch(function (e) { msg(out, e.message, true); });
          } }) : "";
          return el("tr", {}, [el("td", { class: "mono", text: String(r.id) }), el("td", { text: r.name }), el("td", { text: planLabel(r) }), el("td", { text: r.trial_days + "d" }), el("td", { text: r.grace_days + "d" }), el("td", {}, [pill(r.active ? "active" : "archived", r.active ? "ok" : "cancelled")]), el("td", {}, [act])]);
        }))));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    load();
  });
}

function subscriptions(pid) {
  projectShell(pid, "subscriptions", function (p, body) {
    var list = el("div"); var out = el("div");
    var status = el("select", {}, ["", "incomplete", "trialing", "active", "past_due", "paused", "cancelled"].map(function (s) { return el("option", { value: s, text: s || "any status" }); }));
    var customer = el("input", { placeholder: "customer_ref" });
    var planId = el("input", { type: "number", placeholder: "plan id" });
    var saved = el("input", { type: "number", placeholder: "saved method id (optional)" });
    var btn = el("button", { class: "primary", text: "Subscribe" });
    btn.addEventListener("click", function () {
      var b = { customer_ref: customer.value, plan_id: Number(planId.value) };
      if (saved.value) b.saved_method_id = Number(saved.value);
      busy(btn, function () { return api("POST", "/projects/" + pid + "/subscriptions", b); })
        .then(function (r) { go("#/p/" + pid + "/subscriptions/" + r.id); }).catch(function (e) { msg(out, e.message, true); });
    });
    body.appendChild(el("div", { class: "card" }, [field("Customer", customer), field("Plan", planId), field("Saved method", saved), btn, out]));
    body.appendChild(el("div", { class: "row" }, [status]));
    body.appendChild(list);
    status.addEventListener("change", load);
    function load() {
      api("GET", "/projects/" + pid + "/subscriptions" + (status.value ? "?status=" + status.value : "")).then(function (rows) {
        if (!rows.length) { list.replaceChildren(el("p", { class: "muted", text: "No subscriptions." })); return; }
        var head = ["Id", "Customer", "Status", "Period ends", "Next charge"];
        list.replaceChildren(el("table", {}, [el("tr", {}, head.map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (r) {
          return el("tr", { class: "click", onclick: function () { go("#/p/" + pid + "/subscriptions/" + r.id); } }, [el("td", { class: "mono", text: String(r.id) }), el("td", { text: r.customer_ref }), el("td", {}, [subPill(r.status)]), el("td", { text: when(r.current_period_end) }), el("td", { text: when(r.next_charge) })]);
        }))));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    load();
  });
}

function subscriptionDetail(pid, id) {
  projectShell(pid, "subscriptions", function (p, body) {
    var box = el("div"); body.appendChild(box);
    function load() {
      api("GET", "/projects/" + pid + "/subscriptions/" + id).then(function (s) {
        var out = el("div");
        var run = function (action, payload) {
          return api("POST", "/projects/" + pid + "/subscriptions/" + id + "/" + action, payload || {}).then(load).catch(function (e) { msg(out, e.message, true); });
        };
        var actions = el("div", { class: "row" });
        if (s.status === "active" || s.status === "trialing") { actions.appendChild(el("button", { text: "Pause", onclick: function () { run("pause"); } })); }
        if (s.status === "paused") { actions.appendChild(el("button", { text: "Resume", onclick: function () { run("resume"); } })); }
        if (s.status !== "cancelled") {
          if (!s.cancel_at_period_end) { actions.appendChild(el("button", { text: "Cancel at period end", onclick: function () { run("cancel", { at_period_end: true }); } })); }
          actions.appendChild(el("button", { class: "danger", text: "Cancel now", onclick: function () { if (confirm("Cancel this subscription now?")) run("cancel", {}); } }));
        }
        if (s.status === "active" || s.status === "trialing") {
          var np = el("input", { type: "number", placeholder: "new plan id" });
          var pr = el("select", {}, [el("option", { value: "none", text: "No proration" }), el("option", { value: "prorate", text: "Prorate remaining days" })]);
          actions.appendChild(np); actions.appendChild(pr);
          actions.appendChild(el("button", { text: "Change plan", onclick: function () { run("change-plan", { plan_id: Number(np.value), proration: pr.value }); } }));
        }
        var head = ["Period", "From", "To", "Amount", "Status", "Failures", "Payment"];
        var hist = s.periods.length ? el("table", {}, [el("tr", {}, head.map(function (h) { return el("th", { text: h }); }))].concat(s.periods.map(function (r) {
          var link = r.payment_id ? el("a", { href: "#/p/" + pid + "/payments/" + r.payment_id, class: "mono", text: r.payment_id.slice(0, 10) }) : "";
          return el("tr", {}, [el("td", { text: String(r.index) }), el("td", { text: when(r.period_start) }), el("td", { text: when(r.period_end) }), el("td", { text: fmtAmount(r.amount, r.currency) }), el("td", {}, [pill(r.status, r.status === "paid" ? "paid" : r.status === "void" ? "cancelled" : "pending")]), el("td", { text: String(r.failures) }), el("td", {}, [link])]);
        }))) : el("p", { class: "muted", text: "No invoices yet." });
        box.replaceChildren(
          el("div", { class: "card" }, [el("dl", { class: "kv" }, [
            el("dt", { text: "Customer" }), el("dd", { text: s.customer_ref }),
            el("dt", { text: "Status" }), el("dd", {}, [subPill(s.status)]),
            el("dt", { text: "Plan" }), el("dd", { text: String(s.plan_id) }),
            el("dt", { text: "Current period" }), el("dd", { text: when(s.current_period_start) + " to " + when(s.current_period_end) }),
            el("dt", { text: "Next charge" }), el("dd", { text: when(s.next_charge) || "none" }),
            el("dt", { text: "Cancels at period end" }), el("dd", { text: s.cancel_at_period_end ? "yes" : "no" }),
            el("dt", { text: "Credit" }), el("dd", { text: String(s.credit) }),
            el("dt", { text: "Pay link" }), el("dd", { class: "mono", text: s.pay_url || "" })
          ]), actions, out]),
          el("h3", { text: "History" }), hist
        );
      }).catch(function (e) { msg(box, e.message, true); });
    }
    load();
  });
}
