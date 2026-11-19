"use strict";
function webhooks(pid) {
  projectShell(pid, "webhooks", function (p, body) {
    var canWrite = p.role !== "viewer";
    var undeliveredOnly = false;
    var list = el("div", {}, [skeletonRows()]); var detailBox = el("div"); var toolOut = el("div");
    function load() {
      api("GET", "/projects/" + pid + "/deliveries" + (undeliveredOnly ? "?undelivered=1" : "")).then(function (rows) {
        list.replaceChildren(rows.length ? table(["Sent", "Event", "State", ["Tries", "num"], "Error", ""], rows.map(function (d) {
          var state = d.delivered_at ? "delivered" : d.abandoned_at ? "failed" : "waiting";
          return el("tr", {}, [el("td", { class: "muted", text: fmtWhen(d.created) }), el("td", { text: d.event_type }),
            el("td", {}, [status(state, d.delivered_at ? "Delivered" : d.abandoned_at ? "Gave up" : "Retrying")]),
            el("td", { class: "num", text: String(d.attempts) }), el("td", { class: "muted trunc", text: d.last_error || "" }),
            el("td", { class: "num" }, [el("button", { class: "link", type: "button", text: "Details", onclick: function () { deliveryDetail(pid, d.id, detailBox); detailBox.scrollIntoView({ block: "nearest" }); } }), " ",
              el("button", { class: "link", type: "button", text: "Send again", disabled: !canWrite, onclick: function () { api("POST", "/projects/" + pid + "/deliveries/" + d.id + "/replay", {}).then(function () { toast("Webhook queued."); load(); }); } })])]);
        })) : el("div", { class: "empty" }, [el("b", { text: undeliveredOnly ? "Everything was delivered." : "No webhooks sent yet." }), el("p", { class: "muted", text: "Events are sent here when payments change." })]));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    var filter = chips([["all", "All"], ["undelivered", "Not delivered"]], "all", function (v) { undeliveredOnly = v === "undelivered"; load(); }, "Show");
    var resend = el("button", { text: "Send all failed again", disabled: !canWrite, onclick: function () {
      api("POST", "/projects/" + pid + "/deliveries-resend-failed", {}).then(function (r) { msg(toolOut, r.queued + " queued for delivery."); load(); }).catch(function (e) { msg(toolOut, e.message, true); });
    } });

    var healthBox = el("div", {}, [skeletonRows()]);
    api("GET", "/projects/" + pid + "/webhook-health").then(function (h) {
      healthBox.replaceChildren(h.endpoints.length ? table(["Endpoint", "State", ["Sent, 7 days", "num"], ["Failed", "num"], "Last success", "Last failure"], h.endpoints.map(function (e) {
        return el("tr", {}, [el("td", { class: "trunc", text: e.url }), el("td", {}, [status(e.state === "healthy" ? "completed" : e.state === "failing" ? "failed" : "pending", e.state.charAt(0).toUpperCase() + e.state.slice(1))]), el("td", { class: "num", text: String(e.deliveries_7d) }), el("td", { class: "num", text: String(e.failed_7d) }), el("td", { class: "muted", text: e.last_success ? fmtWhen(e.last_success) : "" }), el("td", { class: "muted", text: e.last_failure ? fmtWhen(e.last_failure) : "" })]);
      })) : el("p", { class: "muted", text: "No endpoints yet." }));
    }).catch(function (e) { msg(healthBox, e.message, true); });

    var epOut = el("div"), epList = el("div");
    var epUrl = el("input", { placeholder: "https://example.com/hook", "aria-label": "Endpoint URL" });
    var epEvents = el("input", { placeholder: "Events, comma separated. Empty for all", "aria-label": "Events" });
    function loadEndpoints() {
      api("GET", "/projects/" + pid + "/webhook-endpoints").then(function (rows) {
        epList.replaceChildren(rows.length ? table(["Endpoint", "Events", "State", ["Delivered", "num"], ["Failed", "num"], ["Waiting", "num"], ""], rows.map(function (e) {
          var base = "/projects/" + pid + "/webhook-endpoints/" + e.id;
          return el("tr", {}, [el("td", { class: "trunc", text: e.url }), el("td", { class: "muted trunc", text: e.events.length ? e.events.join(", ") : "All" }),
            el("td", {}, [status(e.enabled ? "active" : "expired", e.enabled ? "On" : "Off")]),
            el("td", { class: "num", text: String(e.delivered) }), el("td", { class: "num", text: String(e.failed) }), el("td", { class: "num", text: String(e.pending) }),
            el("td", { class: "num" }, [
              el("button", { class: "link", type: "button", text: "Send test", disabled: !canWrite, onclick: function () { api("POST", base + "/test", {}).then(function (r) { msg(epOut, r.ok ? "Test event delivered." : "Test failed: " + r.error, !r.ok); }); } }), " ",
              el("button", { class: "link", type: "button", text: e.enabled ? "Turn off" : "Turn on", disabled: !canWrite, onclick: function () { api("POST", base, { enabled: !e.enabled }).then(loadEndpoints); } }), " ",
              el("button", { class: "link", type: "button", text: "New secret", disabled: !canWrite, onclick: function () { api("POST", base + "/rotate-secret", {}).then(function (r) { epOut.replaceChildren(secretBox("New signing secret", r.secret)); }).catch(function (x) { msg(epOut, x.message, true); }); } }), " ",
              el("button", { class: "link", type: "button", text: "Delete", disabled: !canWrite, onclick: function () {
                confirmDialog({ title: "Delete this endpoint", body: "Events stop going to " + e.url + ".", keep: "Keep it", confirm: "Delete endpoint", danger: true }).then(function (v) { if (v) api("DELETE", base).then(loadEndpoints); });
              } })])]);
        })) : el("p", { class: "muted", text: "No endpoints yet. Add one below." }));
      }).catch(function (e) { msg(epList, e.message, true); });
    }
    body.appendChild(section("Deliveries", [toolOut, list, detailBox], { aside: el("div", { class: "actions" }, [filter, resend]) }));
    body.appendChild(section("Endpoints", [p.webhook_url ? el("p", { class: "muted" }, ["Default endpoint from Settings: ", el("span", { class: "mono", text: p.webhook_url })]) : el("p", { class: "muted", text: "No default endpoint is set in Settings." }), epList, el("div", { class: "row" }, [epUrl, epEvents, el("button", { text: "Add endpoint", disabled: !canWrite, onclick: function () {
      var events = epEvents.value.split(",").map(function (x) { return x.trim(); }).filter(Boolean);
      api("POST", "/projects/" + pid + "/webhook-endpoints", { url: epUrl.value, events: events }).then(function (r) {
        epOut.replaceChildren(secretBox("Signing secret", r.secret)); epUrl.value = ""; loadEndpoints();
      }).catch(function (e) { msg(epOut, e.message, true); });
    } })]), epOut]));
    body.appendChild(section("Endpoint health", [healthBox]));
    body.appendChild(signatureHelper(pid));
    load(); loadEndpoints();
  }, { sub: "Events sent to your servers when payments change." });
}

function signatureHelper(pid) {
  var bodyIn = el("textarea", { rows: "4", placeholder: "Raw request body exactly as received", "aria-label": "Request body" });
  var secret = el("input", { placeholder: "Signing secret (whsec_...)", "aria-label": "Signing secret", autocomplete: "off" });
  var sig = el("input", { placeholder: "x-gateway-signature header value", "aria-label": "Signature header" });
  var out = el("div");
  return section("Check a signature", [el("div", { class: "card form" }, [field("Body", bodyIn), field("Secret", secret), field("Signature header", sig),
    el("div", { class: "row" }, [el("button", { text: "Check signature", onclick: function () {
      api("POST", "/projects/" + pid + "/webhook-verify", { body: bodyIn.value, secret: secret.value, signature: sig.value.trim() }).then(function (r) {
        msg(out, r.valid ? "The signature is valid. Age " + r.age_seconds + " s." : "The signature does not match.", !r.valid);
      }).catch(function (e) { msg(out, e.message, true); });
    } })]), out])]);
}
