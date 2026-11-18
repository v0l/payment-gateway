"use strict";
function dashboardCards(s) {
  var cards = [];
  var totals = {};
  s.volume.forEach(function (v) { totals[v.currency] = (totals[v.currency] || 0) + v.volume; });
  Object.keys(totals).forEach(function (c) {
    cards.push(el("div", { class: "stat" }, [el("div", { class: "n", text: fmtAmount(totals[c], c) }), el("div", { class: "l", text: "paid volume " + c })]));
  });
  cards.push(el("div", { class: "stat" }, [el("div", { class: "n", text: s.success_rate === null ? "-" : Math.round(s.success_rate * 100) + "%" }), el("div", { class: "l", text: "success rate (" + s.paid + " paid, " + s.failed + " expired or cancelled)" })]));
  cards.push(el("div", { class: "stat" }, [el("div", { class: "n", text: s.avg_settlement_seconds === null ? "-" : fmtDuration(s.avg_settlement_seconds) }), el("div", { class: "l", text: "average time to settle" })]));
  s.pending.forEach(function (p) {
    cards.push(el("div", { class: "stat" }, [el("div", { class: "n", text: fmtAmount(p.value, p.currency) }), el("div", { class: "l", text: p.payments + " pending " + p.currency })]));
  });
  cards.push(el("div", { class: "stat" }, [el("div", { class: "n", text: s.failed_webhooks }), el("div", { class: "l", text: "failed webhooks" })]));
  return cards;
}

function fmtDuration(sec) {
  if (sec < 90) return Math.round(sec) + " s";
  if (sec < 5400) return Math.round(sec / 60) + " min";
  if (sec < 172800) return (sec / 3600).toFixed(1) + " h";
  return (sec / 86400).toFixed(1) + " d";
}

function overview(pid) {
  projectShell(pid, "overview", function (p, body) {
    var area = el("div", { class: "dash-area" });
    var switcher = el("div", { class: "seg", role: "group", "aria-label": "Period" });
    var current = 30;
    function load() {
      switcher.replaceChildren.apply(switcher, [7, 30, 90].map(function (d) {
        return el("button", { class: d === current ? "on" : "", "aria-pressed": d === current ? "true" : "false", text: d + " days", onclick: function () { current = d; load(); } });
      }));
      api("GET", "/projects/" + pid + "/dashboard?days=" + current).then(function (s) {
        var byMethod = el("table", {}, [el("tr", {}, ["Currency", "Method", "Payments", "Volume"].map(function (h) { return el("th", { text: h }); }))].concat(s.volume.map(function (v) {
          return el("tr", {}, [el("td", { text: v.currency }), el("td", { text: v.method }), el("td", { text: v.payments }), el("td", { text: fmtAmount(v.volume, v.currency) })]);
        })));
        var top = el("table", {}, [el("tr", {}, ["Customer", "Payments", "Volume"].map(function (h) { return el("th", { text: h }); }))].concat(s.top_customers.map(function (c) {
          return el("tr", { class: "click", tabindex: "0", onclick: function () { go("#/p/" + pid + "/customers/" + encodeURIComponent(c.customer_ref)); } }, [el("td", { text: c.customer_ref }), el("td", { text: c.payments }), el("td", { text: fmtAmount(c.volume, c.currency) })]);
        })));
        area.replaceChildren(el("div", { class: "grid" }, dashboardCards(s)),
          el("h3", { text: "Volume by currency and method" }), s.volume.length ? byMethod : el("p", { class: "muted", text: "No paid payments in this period." }),
          el("h3", { text: "Top customers" }), s.top_customers.length ? top : el("p", { class: "muted", text: "No customers yet." }));
      }).catch(function (e) { msg(area, e.message, true); });
    }
    body.appendChild(switcher); body.appendChild(area);
    if (!p.trusted) body.appendChild(el("p", { class: "muted", text: "This project is in test mode. Ask an operator to lift the limits to take live payments." }));
    load();
  });
}
