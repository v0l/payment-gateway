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
      default_cancel_url: el("input", { value: p.default_cancel_url || "" })
    };
    f.allowed_origins.value = (p.allowed_origins || []).join("\n");
    var out = el("div"); var btn = el("button", { class: "primary", text: "Save settings" });
    btn.addEventListener("click", function () {
      var payload = {};
      Object.keys(f).forEach(function (k) { payload[k] = f[k].value.trim() || null; });
      payload.allowed_origins = f.allowed_origins.value.split(/\s+/).filter(Boolean);
      busy(btn, function () { return api("POST", "/projects/" + pid + "/settings", payload); }).then(function () { msg(out, "Saved."); }).catch(function (e) { msg(out, e.message, true); });
    });
    body.appendChild(el("div", { class: "card" }, [field("Webhook URL (https)", f.webhook_url), field("Display name on pay page", f.display_name), field("Logo URL", f.logo_url), field("Accent colour", f.accent_color),
      field("Allowed embed origins", f.allowed_origins), field("Default success URL", f.default_success_url), field("Default cancel URL", f.default_cancel_url), btn, out]));
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

