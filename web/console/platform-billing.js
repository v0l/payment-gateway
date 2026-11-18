"use strict";
var INV_PILL = { open: "pending", paid: "ok", waived: "cancelled" };
var STANDING_TEXT = { current: "Up to date", warned: "Past due: pay soon to avoid restrictions", restricted: "Restricted: only refunds and withdrawals work", disabled: "New payments disabled. Refunds and withdrawals still work" };
function methodLabel(m) { return m === "lightning" ? "Lightning" : m === "stripe" || m === "revolut" ? "Card" : m || ""; }

function platformBilling(pid) {
  projectShell(pid, "billing", function (p, body) {
    api("GET", "/projects/" + pid + "/platform-billing").then(function (b) {
      var a = b.account;
      var bad = a.standing !== "current";
      body.appendChild(el("div", { class: "card" }, [
        el("h3", { text: a.plan.name + " plan" }),
        el("p", {}, [pill(a.standing, bad ? "failed" : "ok"), " " + STANDING_TEXT[a.standing]]),
        el("p", { class: "muted", text: "Monthly price " + fmtAmount(a.plan.monthly_price, a.plan.currency) + ". This period: " + a.usage_count + " settled payments, " + fmtAmount(a.usage_volume, a.plan.currency) + " volume, estimated usage fee " + fmtAmount(a.estimated_usage_fee, a.plan.currency) + ". Credit: " + fmtAmount(a.credit, a.plan.currency) + "." })
      ]));
      body.appendChild(el("h3", { text: "Invoices" }));
      if (!b.invoices.length) { body.appendChild(el("p", { class: "muted", text: "No invoices yet." })); return; }
      var head = ["Id", "Period", "Lines", "Total", "Status", "Method", ""];
      body.appendChild(el("table", {}, [el("tr", {}, head.map(function (h) { return el("th", { text: h }); }))].concat(b.invoices.map(function (i) {
        var lines = i.lines.map(function (l) { return l.name + " x" + l.quantity; }).join("; ");
        var links = [];
        if (i.pay_url) links.push(el("a", { href: i.pay_url, target: "_blank", rel: "noopener", text: "Pay" }));
        if (i.receipt_url) links.push(el("a", { href: i.receipt_url, target: "_blank", rel: "noopener", text: "Receipt" }));
        var status = i.past_due ? pill("past due", "failed") : pill(i.status, INV_PILL[i.status] || i.status);
        return el("tr", {}, [el("td", { class: "mono", text: String(i.id) }), el("td", { text: new Date(i.period_start * 1000).toLocaleDateString() + " to " + new Date(i.period_end * 1000).toLocaleDateString() }), el("td", { text: lines }), el("td", {}, [moneyEl(i.total, i.currency)]), el("td", {}, [status]), el("td", { text: methodLabel(i.payment_method) }), el("td", {}, links)]);
      }))));
    }).catch(function (e) { body.appendChild(el("div", { class: "err", text: e.message })); });
  });
}

function platformPlans() {
  var list = el("div"); var out = el("div");
  function load() {
    api("GET", "/admin/platform/plans").then(function (rows) {
      var head = ["Id", "Name", "Mode", "Monthly price", "Included volume", "Fee bps", "Base fee", "Active", ""];
      list.replaceChildren(el("div", { class: "plan-editor" }, [el("table", {}, [el("tr", {}, head.map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (r) {
        var inputs = {
          name: el("input", { value: r.name }),
          monthly_price: el("input", { type: "number", value: String(r.monthly_price) }),
          included_volume: el("input", { type: "number", value: String(r.included_volume) }),
          fee_bps: el("input", { type: "number", value: String(r.fee_bps) }),
          fee_base: el("input", { type: "number", value: String(r.fee_base) })
        };
        var active = el("input", { type: "checkbox" }); active.checked = r.active;
        var save = el("button", { class: "primary", text: "Save" });
        save.addEventListener("click", function () {
          busy(save, function () {
            return api("POST", "/admin/platform/plans/" + r.id, { name: inputs.name.value, live: r.live, currency: r.currency, monthly_price: Number(inputs.monthly_price.value), included_volume: Number(inputs.included_volume.value), fee_bps: Number(inputs.fee_bps.value), fee_base: Number(inputs.fee_base.value), limits: r.limits, active: active.checked });
          }).then(function () { msg(out, "Plan saved", false); load(); }).catch(function (e) { msg(out, e.message, true); });
        });
        return el("tr", {}, [el("td", { class: "mono", text: String(r.id) }), el("td", {}, [inputs.name]), el("td", { text: r.live ? "live" : "test" }), el("td", {}, [inputs.monthly_price]), el("td", {}, [inputs.included_volume]), el("td", {}, [inputs.fee_bps]), el("td", {}, [inputs.fee_base]), el("td", {}, [active]), el("td", {}, [save])]);
      })))]));
    }).catch(function (e) { msg(list, e.message, true); });
  }
  layout("platform-plans", null, el("div", {}, [el("h2", { text: "Platform plans" }), el("p", { class: "sub", text: "Prices are in minor units of the plan currency. Zero means free." }), out, list]));
  load();
}
