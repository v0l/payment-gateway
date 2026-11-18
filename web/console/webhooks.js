"use strict";
function webhooks(pid) {
  projectShell(pid, "webhooks", function (p, body) {
    var only = el("input", { type: "checkbox" });
    var list = el("div"); var detailBox = el("div"); var toolOut = el("div");
    function load() {
      api("GET", "/projects/" + pid + "/deliveries" + (only.checked ? "?undelivered=1" : "")).then(function (rows) {
        list.replaceChildren(rows.length ? el("table", {}, [el("tr", {}, ["Time", "Event", "State", "Tries", "Error", ""].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (d) {
          return el("tr", {}, [el("td", { class: "muted", text: ago(d.created) }), el("td", { text: d.event_type }),
            el("td", {}, [pill(d.delivered_at ? "delivered" : d.abandoned_at ? "failed" : "waiting", d.delivered_at ? "ok" : d.abandoned_at ? "bad" : "pending")]),
            el("td", { text: d.attempts }), el("td", { class: "muted", text: d.last_error || "" }),
            el("td", {}, [el("details", {}, [el("summary", { text: "body" }), el("pre", { text: JSON.stringify(JSON.parse(d.body), null, 2) })]), el("a", { text: "replay", onclick: function () { api("POST", "/projects/" + pid + "/deliveries/" + d.id + "/replay", {}).then(load); } }), " ", el("a", { text: "detail", onclick: function () { deliveryDetail(pid, d.id, detailBox); } })])]);
        }))) : el("p", { class: "muted", text: "No webhooks yet." }));
      });
    }
    only.addEventListener("change", load);
    body.appendChild(el("div", { class: "row toolbar-row" }, [el("label", { class: "check" }, [only, "Only undelivered"]), el("button", { text: "Resend all failed", disabled: p.role === "viewer", onclick: function () {
      api("POST", "/projects/" + pid + "/deliveries-resend-failed", {}).then(function (r) { msg(toolOut, r.queued + " queued for delivery."); load(); }).catch(function (e) { msg(toolOut, e.message, true); });
    } })]));
    body.appendChild(toolOut); body.appendChild(list); body.appendChild(detailBox); load();
    body.appendChild(el("h3", { text: "Endpoint health" }));
    var healthBox = el("div"); body.appendChild(healthBox);
    api("GET", "/projects/" + pid + "/webhook-health").then(function (h) {
      healthBox.replaceChildren(h.endpoints.length ? el("table", {}, [el("tr", {}, ["URL", "State", "Sent 7d", "Failed 7d", "Last success", "Last failure"].map(function (x) { return el("th", { text: x }); }))].concat(h.endpoints.map(function (e) {
        return el("tr", {}, [el("td", { class: "mono", text: e.url }), el("td", {}, [pill(e.state, e.state === "healthy" ? "ok" : e.state === "failing" ? "bad" : "pending")]), el("td", { text: e.deliveries_7d }), el("td", { text: e.failed_7d }), el("td", { class: "muted", text: e.last_success ? ago(e.last_success) : "" }), el("td", { class: "muted", text: e.last_failure ? ago(e.last_failure) : "" })]);
      }))) : el("p", { class: "muted", text: "No endpoints." }));
    }).catch(function (e) { msg(healthBox, e.message, true); });
    body.appendChild(signatureHelper(pid));
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


function signatureHelper(pid) {
  var bodyIn = el("textarea", { rows: "4", placeholder: "Raw request body exactly as received", "aria-label": "Request body" });
  var secret = el("input", { placeholder: "Signing secret (whsec_...)", "aria-label": "Signing secret", autocomplete: "off" });
  var sig = el("input", { placeholder: "x-gateway-signature header value", "aria-label": "Signature header" });
  var out = el("div");
  return el("div", {}, [el("h3", { text: "Verify a signature" }), el("div", { class: "card" }, [field("Body", bodyIn), field("Secret", secret), field("Signature header", sig),
    el("button", { text: "Verify", onclick: function () {
      api("POST", "/projects/" + pid + "/webhook-verify", { body: bodyIn.value, secret: secret.value, signature: sig.value.trim() }).then(function (r) {
        msg(out, r.valid ? "Signature is valid. Age " + r.age_seconds + " s." : "Signature does not match.", !r.valid);
      }).catch(function (e) { msg(out, e.message, true); });
    } }), out])]);
}
