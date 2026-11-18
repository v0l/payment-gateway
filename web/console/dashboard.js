"use strict";
function figure(value, label, tone) {
  return el("div", { class: "fig" + (tone ? " " + tone : "") }, [el("div", { class: "fig-n money", text: value }), el("div", { class: "fig-l", text: label })]);
}

function dashboardFigures(s) {
  var out = [];
  s.pending.forEach(function (p) { out.push(figure(fmtAmount(p.value, p.currency), p.payments + " pending in " + p.currency)); });
  out.push(figure(s.success_rate === null ? "-" : Math.round(s.success_rate * 100) + "%", s.paid + " paid, " + s.failed + " expired or cancelled"));
  out.push(figure(s.avg_settlement_seconds === null ? "-" : fmtDuration(s.avg_settlement_seconds), "average time to settle"));
  out.push(figure(String(s.failed_webhooks), "failed webhook deliveries", s.failed_webhooks ? "bad" : ""));
  return out;
}

function dashboardHero(s, days) {
  var currencies = [];
  s.volume.forEach(function (v) { if (currencies.indexOf(v.currency) < 0) currencies.push(v.currency); });
  if (!currencies.length) {
    return el("section", { class: "hero empty-hero" }, [el("h3", { text: "No paid payments in the last " + days + " days" }), el("p", { class: "muted", text: "Create a payment from your app or send a test payment, and its volume shows up here." })]);
  }
  var current = currencies[0];
  var body = el("div");
  var picker = el("div", { class: "seg small", role: "group", "aria-label": "Currency" });
  function draw() {
    var total = s.volume.filter(function (v) { return v.currency === current; }).reduce(function (n, v) { return n + v.volume; }, 0);
    var count = s.volume.filter(function (v) { return v.currency === current; }).reduce(function (n, v) { return n + v.payments; }, 0);
    body.replaceChildren(
      el("div", { class: "hero-total" }, [el("span", { class: "hero-n money", text: fmtAmount(total, current) }), el("span", { class: "hero-l", text: "paid across " + count + (count === 1 ? " payment" : " payments") + " in " + days + " days" })]),
      volumeChart(dailySeries(s.daily, current, days), current),
      methodShares(s.volume, current));
    picker.replaceChildren.apply(picker, currencies.map(function (c) {
      return el("button", { class: c === current ? "on" : "", "aria-pressed": c === current ? "true" : "false", text: c, onclick: function () { current = c; draw(); } });
    }));
  }
  draw();
  return el("section", { class: "hero" }, [currencies.length > 1 ? picker : null, body]);
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
        var top = el("table", { class: "ranked" }, [el("tr", {}, [el("th", { text: "Customer" }), el("th", { class: "num", text: "Payments" }), el("th", { class: "num", text: "Volume" })])].concat(s.top_customers.map(function (c) {
          return el("tr", { class: "click", tabindex: "0", onclick: function () { go("#/p/" + pid + "/customers/" + encodeURIComponent(c.customer_ref)); }, onkeydown: function (e) { if (e.key === "Enter") go("#/p/" + pid + "/customers/" + encodeURIComponent(c.customer_ref)); } }, [el("td", { text: c.customer_ref }), el("td", { class: "num", text: c.payments }), el("td", { class: "num" }, [moneyEl(c.volume, c.currency)])]);
        })));
        area.replaceChildren(dashboardHero(s, current),
          el("div", { class: "ledger" }, dashboardFigures(s)),
          el("h3", { text: "Top customers" }), s.top_customers.length ? top : el("p", { class: "muted", text: "No customers yet. Pass a customer_ref when you create a payment." }));
      }).catch(function (e) { msg(area, e.message, true); });
    }
    body.appendChild(switcher); body.appendChild(area);
    if (!p.trusted) body.appendChild(el("p", { class: "banner info", text: "This project is in test mode. Ask an operator to lift the limits to take live payments." }));
    load();
  });
}
