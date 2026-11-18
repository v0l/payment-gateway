"use strict";
var PROVIDER_TEMPLATES = {
  "lightning/lnd": { fields: [["url", "LND gRPC url (https://host:10009)"], ["cert", "tls.cert (PEM)", "area"], ["macaroon", "Macaroon (hex)", "area"]], note: "Use a macaroon limited to invoices and offchain payments." },
  "onchain/lnd": { fields: [["url", "LND gRPC url (https://host:10009)"], ["cert", "tls.cert (PEM)", "area"], ["macaroon", "Macaroon (hex)", "area"], ["address_type", "Address type (p2tr, p2wkh, np2wkh)"], ["min_confirmations", "Confirmations needed (1-100)"]], note: "" },
  "stripe/stripe": { fields: [["secret_key", "Secret key (sk_live_...)"], ["webhook_secret", "Webhook signing secret (whsec_...)"]], note: "Add the webhook URL shown after saving to Stripe, for checkout.session events." },
  "revolut/revolut": { fields: [["token", "Merchant API key"], ["api_version", "API version (2024-09-01)"]], note: "The webhook is registered for you." },
  "fixedfloat/fixedfloat": { fields: [["api_key", "API key"], ["api_secret", "API secret"], ["coins", "Coins, comma separated (XMR,LTC)"]], note: "Needs an on-chain provider in the same project." }
};

function providers(pid) {
  projectShell(pid, "providers", function (p, body) {
    var list = el("div"); body.appendChild(list);
    function load() {
      api("GET", "/projects/" + pid + "/providers").then(function (rows) {
        list.replaceChildren(rows.length ? el("table", {}, [el("tr", {}, ["Name", "Method", "Fee", "Status", ""].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (r) {
          return el("tr", {}, [el("td", { text: r.name }), el("td", { text: r.method + "/" + r.provider_type }),
            el("td", { class: "muted", text: (r.fee_rate ? r.fee_rate + "%" : "") + (r.fee_base ? " + " + r.fee_base + " " + (r.fee_currency || "") : "") }),
            el("td", {}, [pill(r.enabled ? "enabled" : "disabled", r.enabled ? "ok" : "bad")]),
            el("td", {}, [el("a", { text: r.enabled ? "disable" : "enable", onclick: function () { api("POST", "/projects/" + pid + "/providers/" + r.id + "/enabled", { enabled: !r.enabled }).then(load); } }), " ",
              el("a", { text: "edit", onclick: function () { go("#/p/" + pid + "/providers/" + r.id); } })])]);
        }))) : el("p", { class: "muted", text: "No providers yet. Add one to take payments." }));
      });
    }
    body.appendChild(el("div", { class: "row" }, [el("button", { class: "primary", text: "Add provider", onclick: function () { go("#/p/" + pid + "/providers/new"); } })]));
    load();
    sharedMethods(pid, body);
  });
}

function providerForm(pid, id) {
  projectShell(pid, "providers", function (p, body) {
    var existing = null;
    var start = id === "new" ? Promise.resolve(null) : api("GET", "/projects/" + pid + "/providers").then(function (rows) { return rows.filter(function (r) { return String(r.id) === String(id); })[0] || null; });
    start.then(function (row) {
      existing = row;
      var keys = Object.keys(PROVIDER_TEMPLATES).filter(function (k) { return k !== "mock"; });
      var sel = el("select", {}, keys.map(function (k) { return el("option", { value: k, text: k }); }));
      if (row) { sel.value = row.method + "/" + row.provider_type; sel.disabled = true; }
      var name = el("input", { value: row ? row.name : "" });
      var rate = el("input", { type: "number", step: "0.01", placeholder: "e.g. 1.5", value: row && row.fee_rate !== null ? row.fee_rate : "" });
      var base = el("input", { type: "number", placeholder: "minor units", value: row && row.fee_base !== null ? row.fee_base : "" });
      var fcur = el("input", { placeholder: "EUR", value: row && row.fee_currency ? row.fee_currency : "" });
      var cfgBox = el("div"); var inputs = {};
      var out = el("div"); var btn = el("button", { class: "primary", text: row ? "Save" : "Add provider" });
      function drawConfig() {
        var t = PROVIDER_TEMPLATES[sel.value]; inputs = {};
        cfgBox.replaceChildren.apply(cfgBox, t.fields.map(function (f) {
          var cur = row && row.config ? row.config[f[0]] : null;
          var input = f[2] === "area" ? el("textarea", { placeholder: cur === "set" ? "Saved. Leave as is to keep it." : "" }) : el("input", { placeholder: cur === "set" ? "Saved. Leave blank to keep it." : "", value: cur && cur !== "set" ? String(cur) : "" });
          inputs[f[0]] = input; return field(f[1], input);
        }).concat(t.note ? [el("p", { class: "muted", text: t.note })] : []));
      }
      sel.addEventListener("change", drawConfig); drawConfig();
      btn.addEventListener("click", function () {
        var parts = sel.value.split("/"); var cfg = {};
        Object.keys(inputs).forEach(function (k) {
          var v = inputs[k].value.trim();
          if (v === "") { if (row && row.config && row.config[k] === "set") cfg[k] = "set"; return; }
          if (k === "coins") cfg[k] = v.split(",").map(function (c) { return c.trim().toUpperCase(); }).filter(Boolean);
          else if (k === "min_confirmations") cfg[k] = Number(v);
          else cfg[k] = v;
        });
        var payload = { method: parts[0], provider_type: parts[1], name: name.value, config: cfg,
          fee_rate: rate.value === "" ? null : Number(rate.value), fee_base: base.value === "" ? null : Number(base.value), fee_currency: fcur.value || null };
        busy(btn, function () { return row ? api("POST", "/projects/" + pid + "/providers/" + row.id, payload) : api("POST", "/projects/" + pid + "/providers", payload); })
          .then(function (r) {
            var hook = (r && r.hook_url) || (row && row.hook_url);
            out.replaceChildren(el("div", { class: "ok-msg", text: "Saved." }), hook ? el("p", { class: "muted" }, ["Webhook URL for this provider: ", el("span", { class: "mono", text: hook })]) : "");
            if (!row) setTimeout(function () { go("#/p/" + pid + "/providers"); }, 1200);
          }).catch(function (e) { msg(out, e.message, true); });
      });
      var del = row ? el("button", { class: "danger", text: "Delete", onclick: function () {
        if (!confirm("Delete this provider?")) return;
        api("DELETE", "/projects/" + pid + "/providers/" + row.id).then(function () { go("#/p/" + pid + "/providers"); }).catch(function (e) { msg(out, e.message, true); });
      } }) : null;
      body.appendChild(el("div", { class: "card" }, [field("Provider", sel), field("Name", name), cfgBox,
        el("h3", { text: "Processing fee added to the customer total" }), el("div", { class: "row" }, [rate, base, fcur]),
        el("div", { class: "row" }, [btn, del]), out]));
    });
  });
}

