"use strict";
function webhooks(pid) {
  projectShell(pid, "webhooks", function (p, body) {
    var only = el("input", { type: "checkbox" });
    var list = el("div");
    function load() {
      api("GET", "/projects/" + pid + "/deliveries" + (only.checked ? "?undelivered=1" : "")).then(function (rows) {
        list.replaceChildren(rows.length ? el("table", {}, [el("tr", {}, ["Time", "Event", "State", "Tries", "Error", ""].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (d) {
          return el("tr", {}, [el("td", { class: "muted", text: ago(d.created) }), el("td", { text: d.event_type }),
            el("td", {}, [pill(d.delivered_at ? "delivered" : d.abandoned_at ? "failed" : "waiting", d.delivered_at ? "ok" : d.abandoned_at ? "bad" : "pending")]),
            el("td", { text: d.attempts }), el("td", { class: "muted", text: d.last_error || "" }),
            el("td", {}, [el("details", {}, [el("summary", { text: "body" }), el("pre", { text: JSON.stringify(JSON.parse(d.body), null, 2) })]), el("a", { text: "replay", onclick: function () { api("POST", "/projects/" + pid + "/deliveries/" + d.id + "/replay", {}).then(load); } })])]);
        }))) : el("p", { class: "muted", text: "No webhooks yet." }));
      });
    }
    only.addEventListener("change", load);
    body.appendChild(el("div", { class: "row" }, [el("label", {}, [only, " only undelivered"])]));
    body.appendChild(list); load();
    p.webhook_url ? body.appendChild(el("p", { class: "muted" }, ["Sending to ", el("span", { class: "mono", text: p.webhook_url })])) : body.appendChild(el("p", { class: "err", text: "No webhook url set. Add one under Settings." }));
  });
}

