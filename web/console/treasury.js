"use strict";
var STUCK_LABELS = { settled_without_credit: "Settled, no ledger credit", withdrawal_sending: "Withdrawal stuck sending", unconfirmed_receipt: "On-chain receipt unconfirmed", paid_without_webhook: "Paid, webhook not delivered" };
function treasuryTable(heads, rows) {
  return table(heads, rows);
}
function treasury() {
  var out = el("div");
  var body = el("div");
  function load() {
    api("GET", "/admin/treasury").then(function (t) {
      var alerts = (t.latest && t.latest.alerts) || [];
      var kids = [];
      if (alerts.length) { kids.push(el("div", { class: "banner bad", role: "alert" }, [el("div", {}, alerts.map(function (a) { return el("div", { text: a }); }))])); }
      kids.push(el("div", { class: "ledger" }, [figure(t.assets === null ? "No run yet" : sats(t.assets), "assets at the last run"), figure(sats(t.liabilities), "owed to projects and payouts"),
        figure(t.difference === null ? "None" : sats(t.difference), "difference", t.difference !== null && t.difference + t.tolerance_msat < 0 ? "bad" : ""), figure(sats(t.fees_earned), "fees earned"), figure(sats(t.pending_payouts), "payouts on the way")]));
      kids.push(el("h3", { text: "Wallets" }));
      kids.push(treasuryTable(["Wallet", "Kind", "Balance", "Error"], (t.wallets || []).map(function (w) {
        return el("tr", {}, [el("td", { class: "mono", text: w.identity }), el("td", { text: w.kind }), el("td", { text: sats(w.msat) }), el("td", { class: "muted", text: w.error || "" })]);
      })));
      kids.push(el("h3", { text: "Stuck money" }));
      kids.push(t.stuck.length ? treasuryTable(["Problem", "Project", "Reference", "Since", ""], t.stuck.map(function (i) {
        return el("tr", {}, [el("td", { text: STUCK_LABELS[i.kind] || i.kind }), el("td", { text: String(i.tenant_id) }), el("td", { class: "mono", text: i.reference }), el("td", { class: "muted", text: fmtWhen(i.since) }), el("td", {}, [el("a", { href: i.link, text: "Open" })])]);
      })) : el("p", { class: "muted", text: "Nothing stuck." }));
      kids.push(el("h3", { text: "Balances per project" }));
      kids.push(treasuryTable(["Project", "Balance", "Pending payout"], t.tenants.map(function (r) {
        return el("tr", {}, [el("td", {}, [el("a", { href: "#/p/" + r.tenant_id + "/balance", text: r.name })]), el("td", { text: sats(r.balance) }), el("td", { text: sats(r.pending_payout) })]);
      })));
      kids.push(el("h3", { text: "Reconciliation history" }));
      kids.push(treasuryTable(["When", "Trigger", "Assets", "Liabilities", "Difference", "Drift", "Unbalanced", "Stuck", "Status"], t.runs.map(function (r) {
        return el("tr", {}, [el("td", { class: "muted", text: fmtWhen(r.created) }), el("td", { text: r.trigger_kind }), el("td", { text: sats(r.assets) }), el("td", { text: sats(r.liabilities) }), el("td", { text: sats(r.difference) }),
          el("td", { text: String(r.drift_accounts) }), el("td", { text: String(r.unbalanced_transactions) }), el("td", { text: String(r.stuck_count) }), el("td", {}, [status(r.status === "ok" ? "completed" : "failed", r.status === "ok" ? "OK" : "Alert")])]);
      })));
      body.replaceChildren.apply(body, kids);
    }).catch(function (e) { msg(body, e.message, true); });
  }
  operatorPage("treasury", "Treasury", "What the platform holds against what it owes. Runs hourly. Nothing here moves money or fixes balances.", el("div", {}, [
    el("div", { class: "row" }, [el("button", { class: "primary", text: "Run reconciliation now", onclick: function () {
      api("POST", "/admin/treasury/run", {}).then(function () { load(); }).catch(function (e) { msg(out, e.message, true); });
    } })]), out, body]));
  load();
}
function treasuryBanner(host) {
  api("GET", "/admin/treasury/banner").then(function (r) {
    if (r.alerts && r.alerts.length) {
      host.replaceChildren(el("div", { class: "banner bad", role: "alert" }, [el("div", {}, [el("b", { text: "Treasury alert" }), r.alerts.join(". ") + ". ", el("a", { href: "#/treasury", text: "Open treasury" })])]));
    }
  }).catch(function () {});
}
