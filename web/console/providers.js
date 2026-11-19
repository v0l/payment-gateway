"use strict";
var PROVIDER_TEMPLATES = {
  "lightning/lnd": { fields: [["url", "LND gRPC url (https://host:10009)"], ["cert", "tls.cert (PEM)", "area"], ["macaroon", "Macaroon (hex)", "area"]], note: "Use a macaroon limited to invoices and offchain payments." },
  "onchain/lnd": { fields: [["url", "LND gRPC url (https://host:10009)"], ["cert", "tls.cert (PEM)", "area"], ["macaroon", "Macaroon (hex)", "area"], ["address_type", "Address type (p2tr, p2wkh, np2wkh)"], ["min_confirmations", "Confirmations needed (1-100)"]], note: "" },
  "onchain/xpub": { label: "Bitcoin on-chain (your xpub)", fields: [["xpub", "Extended public key (xpub, ypub, zpub) or output descriptor", "area"], ["min_confirmations", "Confirmations needed (0-100, default 1)"], ["esplora_url", "Esplora url (optional, leave blank for the gateway default)"]], note: "The gateway only sees public keys. Payments go straight to your wallet, and refunds are done by hand from it." },
  "stripe/stripe": { fields: [["secret_key", "Secret key (sk_live_...)"], ["webhook_secret", "Webhook signing secret (whsec_...)"]], note: "Add the webhook URL shown after saving to Stripe, for checkout.session events." },
  "revolut/revolut": { fields: [["token", "Merchant API key"], ["api_version", "API version (2024-09-01)"]], note: "The webhook is registered for you." },
  "paypal/paypal": { fields: [["client_id", "Client ID"], ["client_secret", "Client secret"], ["mode", "Mode (sandbox or live)"], ["webhook_id", "Webhook ID (from the PayPal developer dashboard)"]], note: "Subscribe the webhook URL shown after saving to CHECKOUT.ORDER.APPROVED, PAYMENT.CAPTURE.COMPLETED, PAYMENT.CAPTURE.REFUNDED and CUSTOMER.DISPUTE events." },
  "fixedfloat/fixedfloat": { fields: [["api_key", "API key"], ["api_secret", "API secret"], ["coins", "Coins, comma separated (XMR,LTC)"]], note: "Needs an on-chain provider in the same project." }
};

function healthText(r) {
  var h = r.health;
  if (r.auto_disabled_at) return "Switched off after repeated errors";
  if (!h) return "No calls yet";
  return h.successes + " worked, " + h.errors + " failed" + (h.last_error ? ". Last error: " + h.last_error : "");
}

function testProvider(pid, id, box) {
  var out = el("div");
  box.insertBefore(out, box.firstChild);
  api("POST", "/projects/" + pid + "/providers/" + id + "/test").then(function (r) { msg(out, r.message, !r.ok); }).catch(function (e) { msg(out, e.message, true); });
}

function providers(pid) {
  projectShell(pid, "providers", function (p, body) {
    var list = el("div", {}, [skeletonRows()]);
    var canWrite = p.role !== "viewer";
    function load() {
      api("GET", "/projects/" + pid + "/providers").then(function (rows) {
        list.replaceChildren(rows.length ? table(["Name", "Method", "Fee to customer", "Status", "Last 24 hours", ""], rows.map(function (r) {
          return el("tr", {}, [el("td", { text: r.name }), el("td", { class: "muted", text: methodName(r.method) + ", " + r.provider_type }),
            el("td", { class: "muted", text: (r.fee_rate ? r.fee_rate + "%" : "") + (r.fee_base ? " + " + r.fee_base + " " + (r.fee_currency || "") : "") || "None" }),
            el("td", {}, [status(r.auto_disabled_at ? "failed" : r.enabled ? "active" : "expired", r.auto_disabled_at ? "Switched off" : r.enabled ? "On" : "Off")]),
            el("td", { class: "muted wrap", text: healthText(r) }),
            el("td", { class: "num" }, [el("button", { class: "link", type: "button", text: "Test", disabled: !canWrite, onclick: function () { testProvider(pid, r.id, list); } }), " ", el("button", { class: "link", type: "button", text: r.enabled ? "Turn off" : "Turn on", disabled: !canWrite, onclick: function () { api("POST", "/projects/" + pid + "/providers/" + r.id + "/enabled", { enabled: !r.enabled }).then(load); } }), " ",
              el("a", { href: "#/p/" + pid + "/providers/" + r.id, text: "Edit" })])]);
        })) : el("div", { class: "empty" }, [el("b", { text: "No payment methods of your own yet." }), el("p", { class: "muted", text: "Connect your Lightning node, an on-chain wallet or a card provider to take payments directly." })]));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    body.appendChild(section("Your payment methods", [list], { aside: el("button", { class: "primary", text: "Add payment method", disabled: !canWrite, onclick: function () { go("#/p/" + pid + "/providers/new"); } }) }));
    load();
    var shared = el("div");
    body.appendChild(shared);
    sharedMethods(pid, shared);
  }, { sub: "How customers can pay you. Money from your own methods goes straight to you." });
}

function xpubPanel(pid, row, canWrite) {
  var box = el("div", { class: "card" }, [el("h3", { text: "Derivation and chain source" }), skeletonRows()]);
  var out = el("div");
  function load() {
    api("GET", "/projects/" + pid + "/providers/" + row.id + "/xpub").then(function (x) {
      var from = el("input", { type: "number", min: "0", value: "0", "aria-label": "First index" });
      var count = el("input", { type: "number", min: "1", max: "1000", value: "20", "aria-label": "Number of addresses" });
      var go2 = el("button", { text: "Rescan", disabled: !canWrite, onclick: function () {
        busy(go2, function () { return api("POST", "/projects/" + pid + "/providers/" + row.id + "/rescan", { from: Number(from.value), count: Number(count.value) }); })
          .then(function (r) { msg(out, r.message, false); }).catch(function (e) { msg(out, e.message, true); });
      } });
      box.replaceChildren(el("h3", { text: "Derivation and chain source" }),
        el("p", { class: "muted", text: x.script + " on " + x.network + ", receive chain /0/*. Next index " + x.next_index + ", next address " + x.next_address + "." }),
        el("p", { class: "muted", text: "Chain source: " + x.source + ". " + x.chain.message + (x.chain.ok ? "" : " (not reachable)") }),
        el("h3", { text: "First addresses, check them against your wallet" }),
        el("ul", { class: "mono" }, x.preview.map(function (a, i) { return el("li", { text: i + "  " + a }); })),
        x.issued.length ? el("h3", { text: "Latest addresses handed to customers" }) : "",
        x.issued.length ? el("ul", { class: "mono" }, x.issued.map(function (a) { return el("li", { text: a.index + "  " + a.address }); })) : "",
        el("h3", { text: "Rescan a range" }),
        el("div", { class: "row" }, [field("First index", from), field("Addresses", count), go2]), out);
    }).catch(function (e) { msg(box, e.message, true); });
  }
  load();
  return box;
}

function providerForm(pid, id) {
  projectShell(pid, "providers", function (p, body) {
    var existing = null;
    var start = id === "new" ? Promise.resolve(null) : api("GET", "/projects/" + pid + "/providers").then(function (rows) { return rows.filter(function (r) { return String(r.id) === String(id); })[0] || null; });
    start.then(function (row) {
      existing = row;
      var keys = Object.keys(PROVIDER_TEMPLATES).filter(function (k) { return k !== "mock"; });
      var sel = el("select", {}, keys.map(function (k) { var x = k.split("/"); return el("option", { value: k, text: PROVIDER_TEMPLATES[k].label || methodName(x[0]) + " through " + x[1] }); }));
      if (row) { sel.value = row.method + "/" + row.provider_type; sel.disabled = true; }
      var name = el("input", { value: row ? row.name : "" });
      var rate = el("input", { type: "number", step: "0.01", placeholder: "e.g. 1.5", value: row && row.fee_rate !== null ? row.fee_rate : "" });
      var base = el("input", { type: "number", placeholder: "minor units", value: row && row.fee_base !== null ? row.fee_base : "" });
      var fcur = el("input", { placeholder: "EUR", value: row && row.fee_currency ? row.fee_currency : "" });
      var cfgBox = el("div"); var inputs = {};
      var out = el("div"); var btn = el("button", { class: "primary", text: row ? "Save changes" : "Add payment method" });
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
            var preview = r && r.preview_addresses;
            out.replaceChildren(el("div", { class: "ok-msg", text: "Saved." }), hook ? el("p", { class: "muted" }, ["Webhook URL for this provider: ", el("span", { class: "mono", text: hook })]) : "",
              preview ? el("div", {}, [el("p", { text: "Check these first addresses against your wallet before you take payments:" }), el("ul", { class: "mono" }, preview.map(function (a, i) { return el("li", { text: i + "  " + a }); })), el("a", { href: "#/p/" + pid + "/providers", text: "Done" })]) : "");
            if (!row && !preview) setTimeout(function () { go("#/p/" + pid + "/providers"); }, 1200);
          }).catch(function (e) { msg(out, e.message, true); });
      });
      var test = row ? el("button", { text: "Test connection", onclick: function () {
        busy(test, function () { return api("POST", "/projects/" + pid + "/providers/" + row.id + "/test"); }).then(function (r) { msg(out, r.message, !r.ok); }).catch(function (e) { msg(out, e.message, true); });
      } }) : null;
      var del = row ? el("button", { class: "danger", text: "Delete", onclick: function () {
        confirmDialog({ title: "Delete " + row.name, body: "Customers can no longer pay with it. Payments already made are not affected.", keep: "Keep it", confirm: "Delete", danger: true }).then(function (r) {
          if (r) api("DELETE", "/projects/" + pid + "/providers/" + row.id).then(function () { go("#/p/" + pid + "/providers"); }).catch(function (e) { msg(out, e.message, true); });
        });
      } }) : null;
      [rate, base, fcur].forEach(function (x, i) { x.setAttribute("aria-label", ["Fee percent", "Fixed fee", "Fee currency"][i]); });
      body.querySelector(".page-head h1").textContent = row ? row.name : "Add a payment method";
      body.appendChild(el("div", { class: "card" }, [el("div", { class: "form" }, [field("Type", sel), field("Name", name)]), cfgBox,
        el("h3", { text: "Processing fee added to the customer's total" }), el("div", { class: "row" }, [rate, base, fcur]),
        el("div", { class: "row" }, [btn, test, del, el("a", { href: "#/p/" + pid + "/providers", text: "Back" })]), out]));
      if (row && row.provider_type === "xpub") body.appendChild(xpubPanel(pid, row, p.role !== "viewer"));
    });
  });
}

