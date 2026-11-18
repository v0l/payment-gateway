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
    var epOut = el("div"), epList = el("div");
    var epUrl = el("input", { placeholder: "https://example.com/hook" });
    var epEvents = el("input", { placeholder: "events, comma separated (empty for all)" });
    function loadEndpoints() {
      api("GET", "/projects/" + pid + "/webhook-endpoints").then(function (rows) {
        epList.replaceChildren(rows.length ? el("table", {}, [el("tr", {}, ["URL", "Events", "State", "Delivered", "Failed", "Pending", ""].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (e) {
          var base = "/projects/" + pid + "/webhook-endpoints/" + e.id;
          return el("tr", {}, [el("td", { class: "mono", text: e.url }), el("td", { text: e.events.length ? e.events.join(", ") : "all" }),
            el("td", {}, [pill(e.enabled ? "enabled" : "disabled", e.enabled ? "ok" : "pending")]),
            el("td", { text: e.delivered }), el("td", { text: e.failed }), el("td", { text: e.pending }),
            el("td", {}, [
              el("a", { text: "send test event", onclick: function () { api("POST", base + "/test", {}).then(function (r) { msg(epOut, r.ok ? "Test event delivered." : "Test failed: " + r.error, !r.ok); }); } }), " ",
              el("a", { text: e.enabled ? "disable" : "enable", onclick: function () { api("POST", base, { enabled: !e.enabled }).then(loadEndpoints); } }), " ",
              el("a", { text: "new secret", onclick: function () { api("POST", base + "/rotate-secret", {}).then(function (r) { epOut.replaceChildren(secretBox("New signing secret", r.secret)); }).catch(function (x) { msg(epOut, x.message, true); }); } }), " ",
              el("a", { text: "delete", onclick: function () { if (confirm("Delete this endpoint?")) api("DELETE", base).then(loadEndpoints); } })])]);
        }))) : el("p", { class: "muted", text: "No extra endpoints." }));
      });
    }
    body.appendChild(el("h3", { text: "Endpoints" }));
    body.appendChild(el("div", { class: "row" }, [epUrl, epEvents, el("button", { text: "Add endpoint", onclick: function () {
      var events = epEvents.value.split(",").map(function (x) { return x.trim(); }).filter(Boolean);
      api("POST", "/projects/" + pid + "/webhook-endpoints", { url: epUrl.value, events: events }).then(function (r) {
        epOut.replaceChildren(secretBox("Signing secret, shown once", r.secret)); epUrl.value = ""; loadEndpoints();
      }).catch(function (e) { msg(epOut, e.message, true); });
    } })]));
    body.appendChild(epOut); body.appendChild(epList); loadEndpoints();
    p.webhook_url ? body.appendChild(el("p", { class: "muted" }, ["Sending to ", el("span", { class: "mono", text: p.webhook_url })])) : body.appendChild(el("p", { class: "err", text: "No webhook url set. Add one under Settings." }));
  });
}

