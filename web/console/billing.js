"use strict";
function when(iso) { return iso ? fmtWhen(iso) : ""; }
function intervalText(p) { return " every " + (p.interval_count > 1 ? p.interval_count + " " + p.interval + "s" : p.interval); }
function planLabel(p) { return el("span", {}, [moneyEl(p.amount, p.currency), intervalText(p)]); }

function plans(pid) {
  projectShell(pid, "plans", function (p, body) {
    var list = el("div", {}, [skeletonRows()]); var out = el("div");
    var name = el("input", { placeholder: "Pro" });
    var amount = el("input", { type: "number", placeholder: "1500" });
    var cur = el("input", { value: "EUR" });
    var unit = el("select", {}, ["month", "year", "week", "day"].map(function (u) { return el("option", { value: u, text: u.charAt(0).toUpperCase() + u.slice(1) }); }));
    var count = el("input", { type: "number", value: "1" });
    var trial = el("input", { type: "number", value: "0" });
    var grace = el("input", { type: "number", value: "3" });
    var btn = el("button", { class: "primary", text: "Create plan", disabled: p.role === "viewer" });
    btn.addEventListener("click", function () {
      busy(btn, function () {
        return api("POST", "/projects/" + pid + "/plans", { name: name.value, amount: Number(amount.value), currency: cur.value, interval: unit.value, interval_count: Number(count.value), trial_days: Number(trial.value), grace_days: Number(grace.value) });
      }).then(function () { name.value = ""; toast("Plan created."); load(); }).catch(function (e) { msg(out, e.message, true); });
    });
    body.appendChild(section("Plans", [list]));
    body.appendChild(section("Create a plan", [el("div", { class: "card form" }, [field("Name", name), field("Amount in minor units", amount), field("Currency", cur), field("Billed every", unit), field("Number of intervals", count), field("Trial days", trial), field("Grace days for pay-link methods", grace), el("div", { class: "row" }, [btn]), out])]));
    function load() {
      api("GET", "/projects/" + pid + "/plans").then(function (rows) {
        if (!rows.length) { list.replaceChildren(el("div", { class: "empty" }, [el("b", { text: "No plans yet." }), el("p", { class: "muted", text: "Create a plan below, then subscribe customers to it." })])); return; }
        list.replaceChildren(table(["Plan", "Price", ["Trial", "num"], ["Grace", "num"], "Status", ""], rows.map(function (r) {
          var act = r.active && p.role !== "viewer" ? el("button", { class: "link", type: "button", text: "Archive", onclick: function () {
            confirmDialog({ title: "Archive " + r.name, body: "No new subscriptions can use it. Existing subscriptions keep billing.", keep: "Keep it", confirm: "Archive plan" }).then(function (v) {
              if (v) api("POST", "/projects/" + pid + "/plans/" + r.id + "/archive", {}).then(load).catch(function (e) { msg(out, e.message, true); });
            });
          } }) : null;
          return el("tr", {}, [el("td", {}, [el("b", { text: r.name }), el("span", { class: "muted", text: "  #" + r.id })]), el("td", {}, [planLabel(r)]), el("td", { class: "num", text: r.trial_days + " days" }), el("td", { class: "num", text: r.grace_days + " days" }), el("td", {}, [status(r.active ? "active" : "expired", r.active ? "Active" : "Archived")]), el("td", { class: "num" }, [act])]);
        })));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    load();
  }, { sub: "Prices you bill customers again and again." });
}

function subscriptions(pid) {
  projectShell(pid, "subscriptions", function (p, body) {
    var list = el("div", {}, [skeletonRows()]); var out = el("div");
    var current = "";
    var customer = el("input", { placeholder: "customer_ref" });
    var planId = el("select", { "aria-label": "Plan" });
    var saved = el("input", { type: "number", placeholder: "Optional" });
    var btn = el("button", { class: "primary", text: "Subscribe customer", disabled: p.role === "viewer" });
    btn.addEventListener("click", function () {
      var b = { customer_ref: customer.value, plan_id: Number(planId.value) };
      if (saved.value) b.saved_method_id = Number(saved.value);
      busy(btn, function () { return api("POST", "/projects/" + pid + "/subscriptions", b); })
        .then(function (r) { go("#/p/" + pid + "/subscriptions/" + r.id); }).catch(function (e) { msg(out, e.message, true); });
    });
    api("GET", "/projects/" + pid + "/plans").then(function (rows) {
      planId.replaceChildren.apply(planId, rows.filter(function (r) { return r.active; }).map(function (r) { return el("option", { value: String(r.id), text: r.name + ", " + fmtAmount(r.amount, r.currency) + intervalText(r) }); }));
    }).catch(function () {});
    var filter = chips([["", "All"], ["active", "Active"], ["trialing", "Trial"], ["past_due", "Past due"], ["paused", "Paused"], ["cancelled", "Cancelled"]], current, function (v) { current = v; load(); }, "Status");
    body.appendChild(section("Subscriptions", [list], { aside: filter }));
    body.appendChild(section("Subscribe a customer", [el("div", { class: "card form" }, [field("Customer", customer), field("Plan", planId), field("Saved card id", saved), el("div", { class: "row" }, [btn]), out])]));
    function load() {
      api("GET", "/projects/" + pid + "/subscriptions" + (current ? "?status=" + current : "")).then(function (rows) {
        if (!rows.length) { list.replaceChildren(el("div", { class: "empty" }, [el("b", { text: current ? "None with this status." : "No subscriptions yet." }), el("p", { class: "muted", text: "Subscribe a customer to a plan below, or from your app through the API." })])); return; }
        list.replaceChildren(table(["Subscription", "Customer", "Status", "Period ends", "Next charge"], rows.map(function (r) {
          return clickRow({}, [el("td", { class: "mono", text: "#" + r.id }), el("td", { text: r.customer_ref }), el("td", {}, [status(r.status)]), el("td", { class: "muted", text: when(r.current_period_end) }), el("td", { class: "muted", text: when(r.next_charge) || "None" })], function () { go("#/p/" + pid + "/subscriptions/" + r.id); });
        })));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    load();
  }, { sub: "Customers billed on a schedule." });
}

function subscriptionDetail(pid, id) {
  projectShell(pid, "subscriptions", function (p, body) {
    var box = el("div", {}, [skeletonRows()]); body.appendChild(box);
    body.querySelector(".page-head h1").textContent = "Subscription #" + id;
    function load() {
      Promise.all([api("GET", "/projects/" + pid + "/subscriptions/" + id), api("GET", "/projects/" + pid + "/plans")]).then(function (res) {
        var s = res[0], allPlans = res[1];
        var current = allPlans.filter(function (x) { return x.id === s.plan_id; })[0];
        var others = allPlans.filter(function (x) { return x.active && x.id !== s.plan_id; });
        var out = el("div");
        var canWrite = p.role !== "viewer";
        var run = function (action, payload) {
          return api("POST", "/projects/" + pid + "/subscriptions/" + id + "/" + action, payload || {}).then(load).catch(function (e) { msg(out, e.message, true); });
        };
        var actions = el("div", { class: "row" });
        if (canWrite && (s.status === "active" || s.status === "trialing")) { actions.appendChild(el("button", { text: "Pause", onclick: function () { run("pause"); } })); }
        if (canWrite && s.status === "paused") { actions.appendChild(el("button", { text: "Resume", onclick: function () { run("resume"); } })); }
        if (canWrite && s.status !== "cancelled") {
          if (!s.cancel_at_period_end) {
            actions.appendChild(el("button", { text: "Cancel at period end", onclick: function () {
              confirmDialog({ title: "Cancel at the end of this period?", body: "The customer keeps access until " + when(s.current_period_end) + " and is not charged again.", keep: "Keep subscription", confirm: "Cancel at period end", danger: true }).then(function (v) { if (v) run("cancel", { at_period_end: true }); });
            } }));
          }
          actions.appendChild(el("button", { class: "danger", text: "Cancel now", onclick: function () {
            confirmDialog({ title: "Cancel this subscription now?", body: "Billing stops at once and any unpaid invoice is voided.", keep: "Keep subscription", confirm: "Cancel subscription", danger: true }).then(function (v) { if (v) run("cancel", {}); });
          } }));
        }
        var change = null;
        if (canWrite && (s.status === "active" || s.status === "trialing")) {
          var np = el("select", { "aria-label": "New plan" }, [el("option", { value: "", text: others.length ? "Choose a plan" : "No other active plans" })].concat(others.map(function (x) { return el("option", { value: String(x.id), text: x.name + ", " + fmtAmount(x.amount, x.currency) + intervalText(x) }); })));
          var pr = el("select", { "aria-label": "Proration" }, [el("option", { value: "none", text: "No proration" }), el("option", { value: "prorate", text: "Prorate remaining days" })]);
          change = section("Change plan", [el("div", { class: "row" }, [np, pr, el("button", { text: "Change plan", onclick: function () { if (!np.value) { msg(out, "Choose a plan first.", true); return; } run("change-plan", { plan_id: Number(np.value), proration: pr.value }); } })])]);
        }
        var hist = s.periods.length ? table(["Period", "From", "To", ["Amount", "num"], "Status", ["Failures", "num"], "Payment"], s.periods.map(function (r) {
          var link = r.payment_id ? el("a", { href: "#/p/" + pid + "/payments/" + r.payment_id, class: "mono", text: r.payment_id.slice(0, 10) }) : null;
          return el("tr", {}, [el("td", { text: String(r.index) }), el("td", { class: "muted", text: when(r.period_start) }), el("td", { class: "muted", text: when(r.period_end) }), el("td", { class: "num" }, [moneyEl(r.amount, r.currency)]), el("td", {}, [status(r.status === "open" ? "pending" : r.status === "void" ? "cancelled" : r.status, r.status === "void" ? "Void" : r.status === "open" ? "Open" : null)]), el("td", { class: "num", text: String(r.failures) }), el("td", {}, [link])]);
        })) : el("p", { class: "muted", text: "No invoices yet." });
        box.replaceChildren.apply(box, [
          section("Details", [kv([
              ["Customer", el("a", { href: "#/p/" + pid + "/customers/" + encodeURIComponent(s.customer_ref), text: s.customer_ref })],
              ["Status", status(s.status)],
              ["Plan", current ? el("span", {}, [el("a", { href: "#/p/" + pid + "/plans", text: current.name }), ", ", moneyEl(current.amount, current.currency), intervalText(current)]) : el("a", { href: "#/p/" + pid + "/plans", text: "Plan " + s.plan_id })],
              ["Current period", when(s.current_period_start) + " to " + when(s.current_period_end)],
              ["Next charge", when(s.next_charge) || "None"],
              ["Cancels at period end", s.cancel_at_period_end ? "Yes" : "No"],
              ["Credit", moneyEl(s.credit, current ? current.currency : "EUR")],
              s.pay_url ? ["Pay link", el("a", { href: s.pay_url, text: "Open pay link" })] : null
            ]), actions, out]),
          change,
          section("Invoices", [hist])
        ].filter(Boolean));
      }).catch(function (e) { msg(box, e.message, true); });
    }
    load();
  });
}
