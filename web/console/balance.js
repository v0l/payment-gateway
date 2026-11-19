"use strict";
var WALLET_CLASSES = ["w1", "w2", "w3", "w4", "w5"];

function statementLabel(e) {
  if (e.transaction_kind === "payment_received") return e.payment_description ? "Payment, " + e.payment_description : "Payment received";
  if (e.transaction_kind === "refund") return e.payment_description ? "Refund of " + e.payment_description : "Refund";
  if (e.transaction_kind === "payout_requested") return "Payout requested";
  if (e.transaction_kind === "payout_cancelled") return "Payout returned";
  if (e.transaction_kind === "payout_completed") return "Payout sent";
  if (e.transaction_kind === "fee") return "Platform fee";
  if (e.transaction_kind === "adjustment") return e.reference_kind === "refund" ? "Refund reversed" : "Adjustment";
  return statusWord(e.transaction_kind);
}

function statementTable(pid, rows) {
  if (!rows.length) return el("div", { class: "empty" }, [el("b", { text: "No balance changes yet." }), el("p", { class: "muted", text: "Payments through a platform payment method add to this balance. Your own wallets and card providers pay you directly." })]);
  return table(["When", "Entry", ["Amount", "num"], ["Balance", "num"]], rows.map(function (e) {
    var cells = [el("td", { class: "muted", text: fmtWhen(e.created) }),
      el("td", { class: "two" }, [statementLabel(e), el("span", { class: "sub2", text: [e.customer_ref, methodName(e.method)].filter(Boolean).join(", ") })]),
      el("td", { class: "num" }, [signedMoney(e.amount, e.currency)]),
      el("td", { class: "num", text: fmtAmount(e.balance_after, e.currency) })];
    return e.payment_id ? clickRow({}, cells, function () { go("#/p/" + pid + "/payments/" + e.payment_id); }) : el("tr", {}, cells);
  }));
}

function balance(pid) {
  projectShell(pid, "balance", function (p, body) {
    var hero = el("div", { class: "balance-hero" }, [skeletonRows()]);
    var listBox = el("div"); var more = el("div", { class: "more" });
    var aside = el("aside", {}, [skeletonRows()]);
    var rows = [];
    body.appendChild(hero);
    body.appendChild(el("div", { class: "balance-grid" }, [section("Balance changes", [el("p", { class: "muted", text: "Every payment, refund and payout that moved your platform balance, with the balance after it." }), listBox, more]), aside]));
    function loadStatement(append) {
      var before = append && rows.length ? rows[rows.length - 1].id : null;
      api("GET", "/projects/" + pid + "/statement?limit=50" + (before ? "&before=" + before : "")).then(function (page) {
        rows = append ? rows.concat(page) : page;
        listBox.replaceChildren(statementTable(pid, rows));
        more.replaceChildren(page.length === 50 ? el("button", { text: "Older", onclick: function () { loadStatement(true); } }) : el("span"));
      }).catch(function (e) { msg(listBox, e.message, true); });
    }
    Promise.all([api("GET", "/projects/" + pid + "/home?tz=" + tzOffset()), api("GET", "/projects/" + pid + "/payouts").catch(function () { return null; })]).then(function (r) {
      var f = r[0].funds; var po = r[1];
      var total = f.available + f.settling + f.in_payout;
      var withAmount = f.wallets.filter(function (w) { return w.available !== null || w.settling; });
      var comp = el("div", { class: "comp", role: "img", "aria-label": "Balance by wallet" }, withAmount.map(function (w, i) {
        var n = el("i", { class: WALLET_CLASSES[i % 5], title: w.name });
        n.style.flexGrow = String(Math.max(1, (w.available || 0) + w.settling));
        return n;
      }));
      var walletRows = f.wallets.map(function (w, i) {
        return el("tr", {}, [el("td", { class: "two" }, [el("span", { class: "sw " + WALLET_CLASSES[i % 5], "aria-hidden": "true" }), w.name, el("span", { class: "sub2", text: methodName(w.method) + ", settles " + w.settles })]),
          el("td", { class: "num two nowrap" }, [el("b", { text: w.available === null ? (w.settling ? sats(w.settling) : "None held") : sats(w.available) }), el("span", { class: "sub2", text: w.available === null ? (w.settling ? "settling, then in your own wallet" : "Paid to your own wallet") : (w.settling ? sats(w.settling) + " settling" : "Nothing settling") })])]);
      });
      hero.replaceChildren(
        el("div", {}, [el("h1", { text: "Available to withdraw" }), bigMoney(f.available, "BTC"),
          el("div", { class: "balmeta" }, [el("span", {}, [el("b", { text: sats(f.settling) }), " settling"]), el("span", {}, [el("b", { text: sats(f.in_payout) }), " in payouts on the way"]), el("span", {}, [el("b", { text: sats(total) }), " total"])]),
          comp,
          el("div", { class: "actions" }, [el("a", { class: "btn primary", href: "#/p/" + pid + "/payouts", text: "Withdraw" }), el("a", { class: "btn", href: "#/p/" + pid + "/payments", text: "See payments" })])]),
        f.wallets.length ? table(["Wallet", ["Available", "num"]], walletRows, { cls: "plain" }) : el("div", { class: "empty" }, [el("b", { text: "No payment methods yet." }), el("a", { href: "#/p/" + pid + "/providers", text: "Add a payment method" })])
      );
      var kids = [el("h2", { text: "Payouts" })];
      if (po) {
        var lim = po.limits;
        var left = Math.max(lim.daily_max - lim.used_24h, 0);
        var next = po.withdrawals.filter(function (w) { return ["requested", "approved", "sending"].indexOf(w.status) >= 0; })[0];
        kids.push(el("div", { class: "payout-next" }, next ? [bigMoney(next.amount, next.currency), el("p", { text: statusWord(next.status) + " to " + next.destination_label + ", requested " + fmtWhen(next.created) })] :
          [el("p", { text: "No payout on the way. You can withdraw up to " + sats(Math.min(left, Math.max(f.available, 0))) + " now." })]));
        var recent = po.withdrawals.filter(function (w) { return w.purpose !== "verify"; }).slice(0, 4);
        kids.push(el("h2", { text: "Recent payouts" }));
        kids.push(recent.length ? el("ul", { class: "rules" }, recent.map(function (w) { return el("li", {}, [el("b", { text: fmtDay(w.finished_at || w.created) }), el("span", { text: sats(w.amount) + ", " + statusWord(w.status).toLowerCase() })]); })) : el("p", { class: "muted", text: "None yet." }));
        kids.push(el("h2", { text: "Limits" }));
        kids.push(el("ul", { class: "rules" }, [el("li", {}, [el("b", { text: "Left in the next 24 hours" }), el("span", { text: sats(left) })]), el("li", {}, [el("b", { text: "Smallest payout" }), el("span", { text: sats(lim.min_amount) })]), el("li", {}, [el("b", { text: "Sent without review up to" }), el("span", { text: p.trusted ? sats(lim.auto_threshold) : "Every payout is reviewed" })])]));
      }
      kids.push(el("h2", { text: "When money becomes available" }));
      kids.push(el("ul", { class: "rules" }, [["Lightning", "Instantly"], ["On-chain", "After confirmations"], ["Card and PayPal", "On the provider's payout schedule"], ["Platform methods", "In this balance straight away"]].map(function (x) { return el("li", {}, [el("b", { text: x[0] }), el("span", { text: x[1] })]); })));
      aside.replaceChildren.apply(aside, kids);
    }).catch(function (e) { msg(hero, e.message, true); });
    loadStatement(false);
  }, { bare: true });
}
