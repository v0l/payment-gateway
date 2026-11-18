"use strict";
function newProject() {
  var name = el("input", { placeholder: "My shop" });
  var out = el("div");
  var btn = el("button", { class: "primary", text: "Create project" });
  btn.addEventListener("click", function () {
    busy(btn, function () { return api("POST", "/projects", { name: name.value }); }).then(function (r) {
      out.replaceChildren(secretBox("API key", r.api_key), secretBox("Webhook signing secret", r.webhook_secret),
        el("p", {}, [el("a", { text: "Go to project", href: "#/p/" + r.id + "/overview" })]));
      return api("GET", "/me").then(function (m) { me = m; });
    }).catch(function (e) { msg(out, e.message, true); });
  });
  layout("new", null, el("div", {}, [el("h2", { text: "New project" }), el("p", { class: "sub", text: "A project has its own API key, providers and webhook." }),
    el("div", { class: "card" }, [field("Name", name), btn, out])]));
}

function overview(pid) {
  projectShell(pid, "overview", function (p, body) {
    api("GET", "/projects/" + pid + "/stats?days=30").then(function (s) {
      var count = function (st) { return s.by_status.filter(function (r) { return r.status === st; }).reduce(function (a, r) { return a + r.count; }, 0); };
      var revenue = {};
      s.paid_by_day.forEach(function (d) { revenue[d.currency] = (revenue[d.currency] || 0) + d.total; });
      var revCards = Object.keys(revenue).map(function (c) { return el("div", { class: "stat" }, [el("div", { class: "n", text: fmtAmount(revenue[c], c) }), el("div", { class: "l", text: "paid, 30 days" })]); });
      body.appendChild(el("div", { class: "grid" }, revCards.concat([
        el("div", { class: "stat" }, [el("div", { class: "n", text: count("paid") }), el("div", { class: "l", text: "paid payments" })]),
        el("div", { class: "stat" }, [el("div", { class: "n", text: count("pending") }), el("div", { class: "l", text: "pending" })]),
        el("div", { class: "stat" }, [el("div", { class: "n", text: count("expired") + count("cancelled") }), el("div", { class: "l", text: "expired or cancelled" })]),
        el("div", { class: "stat" }, [el("div", { class: "n", text: s.pending_deliveries }), el("div", { class: "l", text: "webhooks waiting" })])
      ])));
      var max = Math.max.apply(null, [1].concat(s.paid_by_day.map(function (d) { return d.count; })));
      body.appendChild(el("h3", { text: "Paid per day" }));
      body.appendChild(el("div", { class: "bars" }, s.paid_by_day.map(function (d) {
        var b = el("div", { title: d.day + ": " + d.count + " (" + d.currency + ")" }); b.style.height = Math.max(4, Math.round(d.count / max * 100)) + "%"; return b;
      })));
      body.appendChild(el("h3", { text: "By method" }));
      body.appendChild(s.by_method.length ? el("table", {}, s.by_method.map(function (m) { return el("tr", {}, [el("td", { text: m.method }), el("td", { text: m.count })]); })) : el("p", { class: "muted", text: "No paid attempts yet." }));
      if (!p.trusted) body.appendChild(el("p", { class: "muted", text: "This project is in test mode. Ask an operator to lift the limits to take live payments." }));
    });
  });
}

