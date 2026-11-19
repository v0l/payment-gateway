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
        el("p", { class: "standing" }, [status(bad ? "failed" : "completed", bad ? "Action needed" : "Up to date"), STANDING_TEXT[a.standing]]),
        el("p", { class: "muted", text: "Monthly price " + fmtAmount(a.plan.monthly_price, a.plan.currency) + ". This period: " + a.usage_count + " settled payments, " + fmtAmount(a.usage_volume, a.plan.currency) + " volume, estimated usage fee " + fmtAmount(a.estimated_usage_fee, a.plan.currency) + ". Credit: " + fmtAmount(a.credit, a.plan.currency) + "." })
      ]));
      if (!b.invoices.length) { body.appendChild(section("Invoices", [el("p", { class: "muted", text: "No invoices yet. The first one is made at the end of this billing period." })])); return; }
      var head = ["Invoice", "Period", "Lines", ["Total", "num"], "Status", "Paid with", ""];
      body.appendChild(section("Invoices", [table(head, b.invoices.map(function (i) {
        var lines = i.lines.map(function (l) { return l.name + " x" + l.quantity; }).join("; ");
        var links = [];
        if (i.pay_url) links.push(el("a", { href: i.pay_url, target: "_blank", rel: "noopener", text: "Pay" }));
        if (i.receipt_url) links.push(el("a", { href: i.receipt_url, target: "_blank", rel: "noopener", text: "Receipt" }));
        var st = i.past_due ? status("failed", "Past due") : status(INV_PILL[i.status] || i.status, i.status === "open" ? "Open" : i.status === "paid" ? "Paid" : "Waived");
        return el("tr", {}, [el("td", { class: "mono", text: String(i.id) }), el("td", { class: "muted", text: fmtDay(new Date(i.period_start * 1000).toISOString()) + " to " + fmtDay(new Date(i.period_end * 1000).toISOString()) }), el("td", { class: "trunc", text: lines }), el("td", { class: "num" }, [moneyEl(i.total, i.currency)]), el("td", {}, [st]), el("td", { class: "muted", text: methodLabel(i.payment_method) }), el("td", {}, links)]);
      }))]));
    }).catch(function (e) { body.appendChild(el("div", { class: "err", role: "alert", text: e.message })); });
  }, { sub: "What this project pays the platform." });
}

function platformPlans() {
  var list = el("div"); var out = el("div");
  function load() {
    api("GET", "/admin/platform/plans").then(function (rows) {
      var head = ["Id", "Name", "Mode", "Monthly price", "Included volume", "Fee, basis points", "Base fee", "Active", ""];
      list.replaceChildren(el("div", { class: "plan-editor" }, [table(head, rows.map(function (r) {
        var inputs = {
          name: el("input", { value: r.name, "aria-label": "Name" }),
          monthly_price: el("input", { type: "number", value: String(r.monthly_price), "aria-label": "Monthly price" }),
          included_volume: el("input", { type: "number", value: String(r.included_volume), "aria-label": "Included volume" }),
          fee_bps: el("input", { type: "number", value: String(r.fee_bps), "aria-label": "Fee in basis points" }),
          fee_base: el("input", { type: "number", value: String(r.fee_base), "aria-label": "Base fee" })
        };
        var active = el("input", { type: "checkbox", "aria-label": "Active" }); active.checked = r.active;
        var save = el("button", { class: "primary", text: "Save" });
        save.addEventListener("click", function () {
          busy(save, function () {
            return api("POST", "/admin/platform/plans/" + r.id, { name: inputs.name.value, live: r.live, currency: r.currency, monthly_price: Number(inputs.monthly_price.value), included_volume: Number(inputs.included_volume.value), fee_bps: Number(inputs.fee_bps.value), fee_base: Number(inputs.fee_base.value), limits: r.limits, active: active.checked });
          }).then(function () { msg(out, "Plan saved", false); load(); }).catch(function (e) { msg(out, e.message, true); });
        });
        return el("tr", {}, [el("td", { class: "mono", text: String(r.id) }), el("td", {}, [inputs.name]), el("td", { class: "muted", text: r.live ? "Live" : "Test" }), el("td", {}, [inputs.monthly_price]), el("td", {}, [inputs.included_volume]), el("td", {}, [inputs.fee_bps]), el("td", {}, [inputs.fee_base]), el("td", {}, [active]), el("td", {}, [save])]);
      }))]));
    }).catch(function (e) { msg(list, e.message, true); });
  }
  operatorPage("platform-plans", "Platform plans", "Prices are in minor units of the plan currency. Zero means free.", el("div", {}, [out, list]));
  load();
}
