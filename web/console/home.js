"use strict";
var WALLET_SWATCH = ["w1", "w2", "w3", "w4", "w5"];

function todayChart(series) {
  var w = 1000, h = 150;
  var max = Math.max.apply(null, series.today.concat(series.last_week).concat([1]));
  function pts(arr) {
    return arr.map(function (v, i) { return ((i + 1) / 24 * w).toFixed(1) + "," + (h - 4 - (v / max) * (h - 14)).toFixed(1); });
  }
  var svg = svgEl("svg", { viewBox: "0 0 " + w + " " + h, preserveAspectRatio: "none", class: "chart-svg", role: "img", "aria-label": "Paid so far today " + fmtAmount(series.today_total, series.currency) + ", against " + fmtAmount(series.last_week_same_time, series.currency) + " by this time last week" });
  svg.appendChild(svgEl("polyline", { points: "0," + (h - 4) + " " + pts(series.last_week).join(" "), class: "chart-week", "vector-effect": "non-scaling-stroke" }));
  var today = pts(series.today);
  svg.appendChild(svgEl("polyline", { points: "0," + (h - 4) + " " + today.join(" "), class: "chart-today", "vector-effect": "non-scaling-stroke" }));
  var last = today[today.length - 1].split(",");
  svg.appendChild(svgEl("line", { x1: last[0], x2: last[0], y1: 0, y2: h, class: "chart-now", "vector-effect": "non-scaling-stroke" }));
  svg.appendChild(svgEl("circle", { cx: last[0], cy: last[1], r: 3.5, class: "chart-dot" }));
  return el("figure", { class: "chart" }, [svg,
    el("div", { class: "legend" }, [el("span", { text: "Today" }), el("span", { class: "prev", text: "Same day last week" })]),
    el("div", { class: "chart-axis" }, ["00:00", "06:00", "12:00", "18:00", "24:00"].map(function (t) { return el("span", { text: t }); }))]);
}

function deltaText(now, then) {
  if (!then) return now ? el("span", { class: "up", text: "new" }) : el("span");
  var pct = (now - then) / then * 100;
  var s = (pct >= 0 ? "+" : "\u2212") + Math.abs(pct).toFixed(1) + "%";
  return el("span", { class: pct >= 0 ? "up" : "down", text: s });
}

function todayBlock(r, currency) {
  var s = r.series.filter(function (x) { return x.currency === currency; })[0];
  if (!s) {
    return el("section", { class: "sec today-sec" }, [el("h2", { text: "Today" }), el("div", { class: "today" }, [el("span", { class: "muted", text: "Paid so far" }), bigMoney(0, currency || "EUR")]),
      el("p", { class: "muted", text: "No payments today or on the same day last week yet." })]);
  }
  var picker = r.series.length > 1 ? chips(r.series.map(function (x) { return [x.currency, x.currency]; }), currency, function (c) { homeCurrency = c; drawToday(); }, "Currency") : null;
  return el("section", { class: "sec today-sec" }, [
    el("h2", { text: "Today" }),
    el("div", { class: "today" }, [el("span", { class: "muted", text: "Paid so far" }), bigMoney(s.today_total, s.currency),
      el("span", { class: "muted", text: fmtAmount(s.last_week_same_time, s.currency) + " by this time last week" }), deltaText(s.today_total, s.last_week_same_time), picker]),
    todayChart(s)
  ]);
}

var homeCurrency = null;
var drawToday = function () {};

function feedItem(pid, it) {
  var amount = it.amount !== null && it.amount !== undefined && it.currency ? fmtAmount(it.amount, it.currency) + " " : "";
  var title = it.kind === "payment" || it.kind === "refund" || it.kind === "payout" || it.kind === "dispute" ? amount + it.title.toLowerCase() : it.title;
  title = title.charAt(0).toUpperCase() + title.slice(1);
  var what = [it.customer, it.reference, it.detail].filter(Boolean).join(", ");
  var li = el("li", { class: it.payment_id ? "click" : "", tabindex: it.payment_id ? "0" : null }, [
    el("time", { datetime: it.at, text: fmtWhen(it.at) }),
    el("span", { class: "dot " + it.tone, "aria-hidden": "true" }),
    el("b", { text: title }),
    el("span", { class: "meth", text: it.kind === "webhook" ? "Webhook" : it.kind === "payout" ? "Payout" : methodName(it.method) }),
    el("span", { class: "what", text: what })
  ]);
  if (it.payment_id) {
    var open = function () { go("#/p/" + pid + "/payments/" + it.payment_id); };
    li.addEventListener("click", open);
    li.addEventListener("keydown", function (e) { if (e.key === "Enter") open(); });
  }
  return li;
}

function activityFeed(pid) {
  var list = el("ul", { class: "feed" });
  var more = el("div", { class: "more" });
  var kind = "all"; var next = null;
  function load(append) {
    api("GET", "/projects/" + pid + "/feed?kind=" + kind + (append && next ? "&before=" + encodeURIComponent(next) : "")).then(function (r) {
      var items = r.items.map(function (it) { return feedItem(pid, it); });
      if (append) items.forEach(function (n) { list.appendChild(n); }); else list.replaceChildren.apply(list, items);
      if (!append && !items.length) list.replaceChildren(el("li", { class: "muted" }, [el("span", { text: kind === "all" ? "Nothing has happened yet. New payments, refunds and webhook problems show up here." : "Nothing of this kind yet." })]));
      next = r.next;
      more.replaceChildren(next ? el("button", { class: "small", text: "Show older", onclick: function () { load(true); } }) : el("span"));
    }).catch(function (e) { msg(list, e.message, true); });
  }
  load(false);
  return section("Activity", [list, more], { aside: chips([["all", "All"], ["payments", "Payments"], ["refunds", "Refunds and disputes"], ["payouts", "Payouts"], ["webhooks", "Webhooks"]], kind, function (k) { kind = k; load(false); }, "Show") });
}

function queueAction(pid, p, row, out, reload) {
  var label = row.action_label;
  if (row.action === "open" || row.action === "mark_paid" && !row.bulk) return el("a", { href: "#/p/" + pid + "/payments/" + row.payment_id, text: label });
  if (row.action === "providers") return el("a", { href: "#/p/" + pid + "/providers", text: label });
  if (row.action === "payouts") return el("a", { href: "#/p/" + pid + "/payouts", text: label });
  if (row.action === "subscription") return el("a", { href: "#/p/" + pid + "/subscriptions/" + row.key.split(":")[1], text: label });
  if (row.action === "copy_link") return el("button", { class: "link", type: "button", text: label, onclick: function () { copyText(location.origin + "/pay/" + row.payment_id, "Pay link copied"); } });
  if (row.action === "resend_failed") return el("button", { class: "link", type: "button", text: label, disabled: p.role === "viewer", onclick: function (e) {
    busy(e.target, function () { return api("POST", "/projects/" + pid + "/deliveries-resend-failed", {}); }).then(function (r) { toast(r.queued + " webhooks queued again."); reload(); }).catch(function (x) { msg(out, x.message, true); });
  } });
  if (row.action === "mark_paid") return el("a", { href: "#/p/" + pid + "/payments/" + row.payment_id, text: label });
  return el("a", { href: "#/p/" + pid + "/payments/" + row.payment_id, text: label });
}

function attentionQueue(pid, p, onCount) {
  var box = el("div", { class: "queue" }, [skeletonRows()]);
  var out = el("div");
  var selected = {};
  function load() {
    api("GET", "/projects/" + pid + "/attention").then(function (r) {
      attentionCount[pid] = r.open;
      onCount(r.open);
      selected = {};
      var waiting = r.waiting.map(function (w) { return w.payments + " for " + fmtAmount(w.volume, w.currency); }).join(" and ");
      if (!r.groups.length) {
        box.replaceChildren(el("div", { class: "quiet", role: "status" }, [el("span", { class: "dot", "aria-hidden": "true" }),
          el("span", { text: "Nothing needs you right now." + (waiting ? " " + waiting + " waiting for customers to pay." : "") })]));
        return;
      }
      var bulkBar = el("div", { class: "bulkbar", hidden: true });
      var canWrite = p.role !== "viewer";
      function drawBulk() {
        var ids = Object.keys(selected);
        bulkBar.hidden = !ids.length;
        if (!ids.length) return;
        var kinds = {}; ids.forEach(function (k) { kinds[selected[k]] = (kinds[selected[k]] || 0) + 1; });
        var acts = [];
        if (kinds.mark_paid) acts.push(el("button", { class: "small", type: "button", text: "Accept " + kinds.mark_paid + " as paid", disabled: !canWrite || !r.manual_paid_allowed, title: r.manual_paid_allowed ? null : "Turn on manual payment handling in Settings first", onclick: function (e) { bulk(e.target, "mark_paid"); } }));
        if (kinds.cancel) acts.push(el("button", { class: "small", type: "button", text: "Cancel " + kinds.cancel, disabled: !canWrite, onclick: function (e) { bulk(e.target, "cancel"); } }));
        bulkBar.replaceChildren.apply(bulkBar, [el("span", { text: ids.length + " selected" }), el("span", { class: "sp" })].concat(acts));
      }
      function bulk(btn, action) {
        var ids = Object.keys(selected).filter(function (k) { return selected[k] === action; });
        var title = action === "cancel" ? "Cancel " + ids.length + " payments" : "Accept " + ids.length + " payments as paid";
        var body = action === "cancel" ? "The customers can no longer pay them. This cannot be undone." : "They are marked paid without a provider confirmation. Your webhook gets a manual flag and the reason.";
        confirmDialog({ title: title, body: body, keep: "Go back", confirm: action === "cancel" ? "Cancel payments" : "Mark paid", danger: true, input: action === "mark_paid" ? "Reason, for example seen in the bank statement" : null }).then(function (v) {
          if (!v) return;
          busy(btn, function () { return api("POST", "/projects/" + pid + "/attention/bulk", { action: action, payments: ids, reason: v.text }); }).then(function (res) {
            toast(res.done.length + " done" + (res.failed.length ? ", " + res.failed.length + " could not be changed" : "") + ".");
            load();
          }).catch(function (e) { msg(out, e.message, true); });
        });
      }
      var rows = [];
      r.groups.forEach(function (g) {
        var groupBox = g.rows.some(function (x) { return x.bulk; }) && canWrite ? el("input", { type: "checkbox", "aria-label": "Select all in " + g.title, onchange: function (e) {
          g.rows.forEach(function (x) { if (!x.bulk) return; if (e.target.checked) selected[x.payment_id] = x.bulk; else delete selected[x.payment_id]; });
          box.querySelectorAll("input[data-group='" + g.kind + "']").forEach(function (c) { c.checked = e.target.checked; });
          drawBulk();
        } }) : null;
        rows.push(el("tr", { class: "group" }, [el("td", { class: "cb" }, [groupBox]), el("td", { colspan: "6" }, [el("span", { class: "tone " + g.tone, "aria-hidden": "true" }), g.title, el("span", { class: "muted", text: String(g.rows.length) })])]));
        g.rows.forEach(function (x) {
          var cb = x.bulk && canWrite ? el("input", { type: "checkbox", "data-group": g.kind, "aria-label": "Select " + x.title, onchange: function (e) { if (e.target.checked) selected[x.payment_id] = x.bulk; else delete selected[x.payment_id]; drawBulk(); } }) : null;
          var cells = [el("td", { class: "cb" }, [cb]),
            el("td", { class: "two" }, [el("b", { text: x.title }), el("span", { class: "sub2", text: [x.who, methodName(x.method)].filter(Boolean).join(", ") })]),
            el("td", { class: "why", text: x.why }),
            el("td", { class: "due hide-m" + (x.due_soon ? " soon" : ""), text: fmtWhenAfter(x.due, x.at) }),
            el("td", { class: "amt" }, [x.amount !== null && x.currency ? fmtAmount(x.amount, x.currency) : ""]),
            el("td", {}, [queueAction(pid, p, x, out, load)])];
          rows.push(x.payment_id ? clickRow({}, cells, function () { go("#/p/" + pid + "/payments/" + x.payment_id); }) : el("tr", {}, cells));
        });
      });
      if (waiting) rows.push(el("tr", { class: "fold" }, [el("td", { class: "cb" }), el("td", { colspan: "5" }, [waiting + " waiting for customers to pay. Nothing to do yet. ", el("a", { href: "#/p/" + pid + "/payments", text: "Show them" })])]));
      var t = el("div", { class: "table-wrap", tabindex: "0", role: "region", "aria-label": "Needs attention" }, [bulkBar, el("table", {}, [el("thead", {}, [el("tr", {}, [el("th", { class: "cb" }), el("th", { text: "Payment" }), el("th", { text: "Why it is here" }), el("th", { class: "hide-m", text: "When" }), el("th", { class: "num", text: "Amount" }), el("th", { text: "Next step" })])]), el("tbody", {}, rows)])]);
      var mob = el("ul", { class: "queue-mob" }, [].concat.apply([], r.groups.map(function (g) {
        return [el("li", { class: "g" }, [el("span", { class: "tone " + g.tone, "aria-hidden": "true" }), g.title + " ", el("span", { class: "muted", text: String(g.rows.length) })])].concat(g.rows.map(function (x) {
          return el("li", {}, [el("b", { text: x.title }), el("span", { class: "a", text: x.amount !== null && x.currency ? fmtAmount(x.amount, x.currency) : "" }),
            el("span", { class: "d", text: x.why }), el("span", { class: "d", text: [x.who, fmtWhenAfter(x.due, x.at)].filter(Boolean).join(", ") }), el("span", { class: "n" }, [queueAction(pid, p, x, out, load)])]);
        }));
      })).concat(waiting ? [el("li", { class: "d" }, [waiting + " waiting for customers to pay. ", el("a", { href: "#/p/" + pid + "/payments", text: "Show them" })])] : []));
      box.replaceChildren(t, mob);
    }).catch(function (e) { msg(box, e.message, true); });
  }
  load();
  return { node: el("div", {}, [box, out]), reload: load };
}

function fundsBlock(pid, f) {
  var kids = [el("h2", { text: "Funds" })];
  if (!f.wallets.length) {
    kids.push(el("p", { class: "hint", text: "Add a payment method to start taking money." }), el("a", { href: "#/p/" + pid + "/providers", text: "Add a payment method" }));
    return el("section", { class: "funds" }, kids);
  }
  kids.push(el("p", { class: "hint", text: "Held for you by the platform, ready to withdraw" }), bigMoney(f.available, "BTC"));
  kids.push(el("div", { class: "line" }, [el("span", { text: "Settling" }), el("span", { text: f.settling ? sats(f.settling) : "Nothing" })]));
  if (f.in_payout) kids.push(el("div", { class: "line" }, [el("span", { text: "In payouts on the way" }), el("span", { text: sats(f.in_payout) })]));
  var lp = f.latest_payout;
  kids.push(el("div", { class: "line" }, [el("span", { text: "Latest payout" }), el("span", { text: lp ? fmtAmount(lp.amount, lp.currency) + ", " + statusWord(lp.status).toLowerCase() + " " + fmtDay(lp.finished_at || lp.created) : "None yet" })]));
  kids.push(el("div", { class: "wallets" }, f.wallets.map(function (w, i) {
    var amount = w.available === null ? (w.settling ? sats(w.settling) + " settling" : (w.currency === "BTC" ? "In your wallet" : "Paid out by provider")) : sats(w.available);
    return el("div", { class: "w" }, [el("i", { class: WALLET_SWATCH[i % 5], "aria-hidden": "true" }), el("b", { text: w.name }), el("span", { class: "money", text: amount }),
      el("small", { text: methodName(w.method) + ", settles " + w.settles + (w.available !== null && w.settling ? ", " + sats(w.settling) + " settling" : "") })]);
  })));
  kids.push(el("a", { href: "#/p/" + pid + "/balance", text: "Open balance" }));
  return el("section", { class: "funds" }, kids);
}

function monthBlock(m) {
  var figs = [];
  m.figures.forEach(function (f) {
    var c = f.currency;
    figs.push(["Paid, " + c, fmtAmount(f.paid, c), fmtAmount(f.paid_prev, c), deltaText(f.paid, f.paid_prev)]);
    figs.push(["Paid payments, " + c, String(f.payments), String(f.payments_prev), deltaText(f.payments, f.payments_prev)]);
    if (f.success_rate !== null) figs.push(["Success rate, " + c, Math.round(f.success_rate * 1000) / 10 + "%", f.success_rate_prev === null ? "none" : Math.round(f.success_rate_prev * 1000) / 10 + "%", el("span")]);
    if (f.refunded || f.refunded_prev) figs.push(["Refunded, " + c, fmtAmount(f.refunded, c), fmtAmount(f.refunded_prev, c), el("span")]);
  });
  var start = new Date(m.start);
  return el("section", {}, [el("h2", { text: "This month" }), el("p", { class: "hint", text: "1 to " + new Date().getDate() + " " + start.toLocaleDateString([], { month: "long" }) + ", against the same days last month" }),
    figs.length ? el("dl", { class: "figs" }, figs.map(function (x) { return el("div", {}, [el("dt", { text: x[0] }), el("dd", {}, [x[1], el("span", { class: "prev" }, [x[2], x[3]])])]); })) : el("p", { class: "muted", text: "No payments this month or last month yet." })]);
}

function healthBlock(pid, list) {
  return el("section", {}, [el("h2", { text: "Payment methods" }),
    list.length ? el("ul", { class: "health" }, list.map(function (h) {
      return el("li", {}, [el("i", { class: h.state, "aria-hidden": "true" }), el("span", { text: h.name + (h.platform_managed ? " (platform)" : "") }), el("small", { text: methodName(h.method) + ", " + h.note })]);
    })) : el("p", { class: "muted" }, ["None set up. ", el("a", { href: "#/p/" + pid + "/providers", text: "Add one" })])]);
}

function overview(pid) {
  projectShell(pid, "overview", function (p, body) {
    var todayBox = el("div"); var rail = el("aside", { class: "rail", "aria-label": "Funds and figures" }, [skeletonRows()]);
    var queue = attentionQueue(pid, p, function (n) {
      var tab = document.querySelector(".tabs a:first-child");
      if (!tab) return;
      var badge = tab.querySelector(".count");
      if (n && !badge) tab.appendChild(el("span", { class: "count", text: String(n), "aria-label": n + " open items" }));
      else if (badge && n) badge.textContent = String(n);
      else if (badge) badge.remove();
    });
    var main = el("div", {}, [queue.node, todayBox, activityFeed(pid)]);
    body.appendChild(pageHead("Needs attention", "Payments and problems that need someone, grouped by what to do."));
    body.appendChild(el("div", { class: "home" }, [main, rail]));
    if (!p.trusted) body.insertBefore(el("p", { class: "banner info", role: "note" }, [el("span", { text: "This project is in test mode. Ask an operator to lift the limits to take live payments." })]), body.firstChild);
    api("GET", "/projects/" + pid + "/home?tz=" + tzOffset()).then(function (r) {
      homeCurrency = homeCurrency && r.series.some(function (s) { return s.currency === homeCurrency; }) ? homeCurrency : (r.series[0] ? r.series[0].currency : null);
      drawToday = function () { todayBox.replaceChildren(todayBlock(r, homeCurrency)); };
      drawToday();
      rail.replaceChildren(fundsBlock(pid, r.funds), monthBlock(r.month), healthBlock(pid, r.health));
    }).catch(function (e) { msg(rail, e.message, true); });
  }, { bare: true });
}

function dashboardFigures(s) {
  var out = [];
  s.pending.forEach(function (p) { out.push(figure(fmtAmount(p.value, p.currency), p.payments + " pending in " + p.currency)); });
  out.push(figure(s.success_rate === null ? "None" : Math.round(s.success_rate * 100) + "%", s.paid + " paid, " + s.failed + " expired or cancelled"));
  out.push(figure(String(s.failed_webhooks), "failed webhook deliveries", s.failed_webhooks ? "bad" : ""));
  return out;
}

function figure(value, label, tone) {
  return el("div", { class: "fig" + (tone ? " " + tone : "") }, [el("div", { class: "fig-n money", text: value }), el("div", { class: "fig-l", text: label })]);
}
