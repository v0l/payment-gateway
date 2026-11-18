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
