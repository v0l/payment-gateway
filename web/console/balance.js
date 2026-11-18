"use strict";
function balance(pid) {
  projectShell(pid, "balance", function (p, body) {
    var cards = el("div", { class: "grid" });
    var list = el("div"); var more = el("div");
    var rows = [];
    body.appendChild(cards);
    body.appendChild(el("h3", { text: "History" }));
    body.appendChild(list);
    body.appendChild(more);
    api("GET", "/projects/" + pid + "/balance").then(function (r) {
      var held = r.balances.filter(function (b) { return b.kind === "tenant_balance"; });
      if (!held.length) { cards.appendChild(el("p", { class: "muted", text: "No funds held for this project." })); }
      held.forEach(function (b) {
        cards.appendChild(el("div", { class: "stat" }, [el("div", { class: "n", text: fmtAmount(b.balance, b.currency) }), el("div", { class: "l", text: "owed to you, " + b.currency })]));
      });
    }).catch(function (e) { msg(cards, e.message, true); });
    function load(append) {
      var before = append && rows.length ? rows[rows.length - 1].id : null;
      api("GET", "/projects/" + pid + "/ledger?limit=50" + (before ? "&before=" + before : "")).then(function (page) {
        rows = append ? rows.concat(page) : page;
        if (!rows.length) { list.replaceChildren(el("p", { class: "muted", text: "No ledger entries yet." })); more.replaceChildren(); return; }
        var head = ["When", "Account", "Type", "Amount", "Related"];
        list.replaceChildren(el("table", {}, [el("tr", {}, head.map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (e) {
          var related = e.reference_kind ? e.reference_kind + " " + (e.reference_id || "") : "";
          return el("tr", {}, [el("td", { class: "muted", text: ago(e.created), title: e.created }), el("td", { text: e.account_kind }), el("td", {}, [pill(e.transaction_kind, "ok")]), el("td", { text: (e.amount > 0 ? "+" : "") + fmtAmount(e.amount, e.currency) }), el("td", { class: "mono", text: related })]);
        }))));
        more.replaceChildren(page.length === 50 ? el("button", { text: "Older", onclick: function () { load(true); } }) : "");
      }).catch(function (e) { msg(list, e.message, true); });
    }
    load(false);
  });
}
