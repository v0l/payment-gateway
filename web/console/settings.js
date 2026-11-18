"use strict";
function settings(pid) {
  projectShell(pid, "settings", function (p, body) {
    var f = {
      webhook_url: el("input", { value: p.webhook_url || "", placeholder: "https://example.com/hooks/payments" }),
      display_name: el("input", { value: p.display_name || "" }),
      logo_url: el("input", { value: p.logo_url || "", placeholder: "https://..." }),
      accent_color: el("input", { value: p.accent_color || "", placeholder: "#5b3df5" }),
      allowed_origins: el("textarea", { placeholder: "One per line. Sites allowed to embed the pay page." }),
      default_success_url: el("input", { value: p.default_success_url || "" }),
      default_cancel_url: el("input", { value: p.default_cancel_url || "" }),
      legal_name: el("input", { value: p.legal_name || "", placeholder: "Seller name on receipts" }),
      address: el("textarea", { placeholder: "Seller address on receipts" }),
      tax_id: el("input", { value: p.tax_id || "", placeholder: "VAT or tax ID" }),
      support_email: el("input", { value: p.support_email || "", placeholder: "support@example.com" })
    };
    f.address.value = p.address || "";
    f.allowed_origins.value = (p.allowed_origins || []).join("\n");
    var out = el("div"); var btn = el("button", { class: "primary", text: "Save settings" });
    btn.addEventListener("click", function () {
      var payload = {};
      Object.keys(f).forEach(function (k) { payload[k] = f[k].value.trim() || null; });
      payload.allowed_origins = f.allowed_origins.value.split(/\s+/).filter(Boolean);
      busy(btn, function () { return api("POST", "/projects/" + pid + "/settings", payload); }).then(function () { msg(out, "Saved."); }).catch(function (e) { msg(out, e.message, true); });
    });
    body.appendChild(el("div", { class: "card" }, [field("Webhook URL (https)", f.webhook_url), field("Display name on pay page", f.display_name), field("Logo URL", f.logo_url), field("Accent colour", f.accent_color),
      field("Allowed embed origins", f.allowed_origins), field("Default success URL", f.default_success_url), field("Default cancel URL", f.default_cancel_url),
      field("Legal name (receipts)", f.legal_name), field("Address (receipts)", f.address), field("Tax ID (receipts)", f.tax_id), field("Support email (receipts)", f.support_email), btn, out]));
    var brandingBox = el("div", { class: "card" });
    body.appendChild(el("h3", { text: "Pay page branding" }));
    body.appendChild(brandingBox);
    renderBranding(pid, brandingBox);
    var limitsBox = el("div", { class: "card" });
    body.appendChild(el("h3", { text: "Limits and usage" }));
    body.appendChild(limitsBox);
    renderLimits(pid, limitsBox);
    if (p.role === "owner") {
      var req = el("input", { type: "checkbox" });
      req.checked = !!p.require_2fa;
      var reqOut = el("div");
      req.addEventListener("change", function () {
        api("POST", "/projects/" + pid + "/require-2fa", { required: req.checked }).then(function () { msg(reqOut, "Saved."); }).catch(function (e) { req.checked = !req.checked; msg(reqOut, e.message, true); });
      });
      body.appendChild(el("h3", { text: "Security" }));
      body.appendChild(el("div", { class: "card" }, [el("label", {}, [req, " Require two factor for members of this project"]), reqOut]));
    }
    var keyOut = el("div");
    var rows = [el("button", { text: "Rotate API key", onclick: function () {
      if (!confirm("The old key stops working immediately. Continue?")) return;
      api("POST", "/projects/" + pid + "/rotate-key", {}).then(function (r) { keyOut.replaceChildren(secretBox("New API key", r.api_key)); }).catch(function (e) { msg(keyOut, e.message, true); });
    } })];
    if (p.role === "owner") rows.push(el("button", { text: "Rotate webhook signing secret", onclick: function () {
      if (!confirm("Update your receiver with the new secret straight away. Continue?")) return;
      api("POST", "/projects/" + pid + "/rotate-webhook-secret", {}).then(function (r) { keyOut.replaceChildren(secretBox("New webhook secret", r.webhook_secret)); }).catch(function (e) { msg(keyOut, e.message, true); });
    } }));
    if (me.is_operator) {
      var trustOut = el("div");
      body.appendChild(el("h3", { text: "Operator" }));
      body.appendChild(el("div", { class: "row" }, [el("button", { text: p.trusted ? "Move back to test limits" : "Lift test limits (go live)", onclick: function () {
        api("POST", "/admin/projects/" + pid + "/trusted", { trusted: !p.trusted }).then(function () { msg(trustOut, "Updated. Reload to see it."); }).catch(function (e) { msg(trustOut, e.message, true); });
      } })]));
      body.appendChild(trustOut);
    }
    body.appendChild(el("h3", { text: "Keys" }));
    body.appendChild(el("div", { class: "row" }, rows)); body.appendChild(keyOut);
  });
}


var LIMIT_ROWS = [
  ["payments_per_day", "Payments per 24 hours", "payments_per_day"],
  ["pending_payments", "Open pending payments", "pending_payments"],
  ["amount_per_payment", "Max amount per payment (EUR cents)", null],
  ["volume_per_day", "Volume per 24 hours (EUR cents)", "volume_per_day"],
  ["providers", "Providers", "providers"],
  ["webhook_endpoints", "Webhook endpoints", "webhook_endpoints"],
  ["attempts_per_payment", "Attempts per payment", null]
];
function renderLimits(pid, box) {
  api("GET", "/projects/" + pid + "/limits").then(function (v) {
    var inputs = {}; var out = el("div");
    var rows = LIMIT_ROWS.map(function (r) {
      var used = r[2] ? String(v.usage[r[2]]) : "";
      var cell;
      if (me.is_operator) {
        cell = el("input", { value: String(v.limits[r[0]]), type: "number", min: "0" });
        inputs[r[0]] = cell;
      } else {
        cell = el("span", { text: String(v.limits[r[0]]) });
      }
      return el("tr", {}, [el("td", { text: r[1] }), el("td", { text: used }), el("td", {}, [cell]), el("td", { text: String(v.defaults[r[0]]) })]);
    });
    var head = el("tr", {}, ["Limit", "Used", "Limit", "Default"].map(function (t) { return el("th", { text: t }); }));
    box.replaceChildren(el("table", {}, [head].concat(rows)), out);
    if (!me.is_operator) return;
    var save = el("button", { class: "primary", text: "Save limits" });
    save.addEventListener("click", function () {
      var payload = {};
      Object.keys(inputs).forEach(function (k) {
        var n = parseInt(inputs[k].value, 10);
        payload[k] = n === v.defaults[k] ? null : n;
      });
      busy(save, function () { return api("POST", "/admin/projects/" + pid + "/limits", payload); }).then(function () { msg(out, "Saved."); }).catch(function (e) { msg(out, e.message, true); });
    });
    box.appendChild(save);
  }).catch(function (e) { box.replaceChildren(el("div", { text: e.message })); });
}
