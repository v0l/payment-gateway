"use strict";
function feeMoney(bps, base) {
  var parts = [el("span", { class: "money", text: (bps / 100) + "%" })];
  if (base) parts.push(" ", el("span", { class: "money" }, ["+ " + base.toLocaleString() + " msats", el("small", { text: "fee" })]));
  return el("span", {}, parts);
}

function skeletonRows() {
  return el("div", {}, [el("div", { class: "skeleton" }), el("div", { class: "skeleton" }), el("div", { class: "skeleton" })]);
}

function emptyState(title, hint) {
  return el("div", { class: "empty" }, [el("b", { text: title }), el("p", { class: "muted", text: hint })]);
}

function confirmChoice(title, body, safeLabel, dangerLabel, onConfirm) {
  var safe = el("button", { text: safeLabel });
  var danger = el("button", { class: "danger", text: dangerLabel });
  var scrim = el("div", { class: "dialog-scrim modal" }, [el("div", { class: "dialog", role: "alertdialog", "aria-modal": "true", "aria-label": title }, [
    el("h4", { text: title }), el("p", { class: "muted", text: body }), el("div", { class: "row" }, [safe, danger])
  ])]);
  function close() { document.removeEventListener("keydown", onKey); scrim.remove(); }
  function onKey(e) { if (e.key === "Escape") close(); }
  safe.addEventListener("click", close);
  danger.addEventListener("click", function () { close(); onConfirm(); });
  document.addEventListener("keydown", onKey);
  document.body.appendChild(scrim);
  safe.focus();
}

function sharedMethods(pid, container) {
  var box = el("div", {}, [skeletonRows()]);
  container.appendChild(section("Platform payment methods", [el("p", { class: "muted", text: "Use the platform's own Lightning and on-chain wallets. Payments are held as a balance for you, minus the fee shown." }), box]));
  function toggle(r, btn) {
    busy(btn, function () { return api("POST", "/projects/" + pid + "/shared/" + r.id + "/enabled", { enabled: !r.enabled }); })
      .then(function () { load(); }).catch(function (e) { msg(box, e.message, true); });
  }
  function load() {
    api("GET", "/projects/" + pid + "/shared").then(function (rows) {
      if (!rows.length) { box.replaceChildren(emptyState("No platform payment methods offered yet.", "An operator can make Lightning and on-chain wallets available to this project.")); return; }
      var head = ["Name", "Method", "Network", "Platform fee", "Status", ""];
      box.replaceChildren(table(head, rows.map(function (r) {
        var btn = el("button", { class: r.enabled ? "small" : "small primary", text: r.enabled ? "Turn off" : "Turn on" });
        btn.addEventListener("click", function () {
          if (!r.enabled) return toggle(r, btn);
          confirmChoice("Disable " + r.name + "?", "New payments can no longer use this method. Payments already started and your balance stay as they are.", "Keep method", "Disable method", function () { toggle(r, btn); });
        });
        return el("tr", {}, [el("td", { text: r.name }), el("td", { class: "muted", text: methodName(r.method) }), el("td", { class: "muted", text: r.network }),
          el("td", {}, [feeMoney(r.fee_bps, r.fee_base_msat)]), el("td", {}, [status(r.enabled ? "active" : "expired", r.enabled ? "On" : "Off")]), el("td", { class: "num" }, [btn])]);
      })));
    }).catch(function (e) { msg(box, e.message, true); });
  }
  load();
}

function sharedAdmin() {
  var body = el("div");
  operatorPage("shared", "Shared providers", "Platform wallets that projects can turn on. The platform holds the funds and keeps the fee.", body);
  var list = el("div", {}, [skeletonRows()]); var out = el("div");
  var overrideBox = el("div");

  function load() {
    api("GET", "/admin/shared").then(function (rows) {
      if (!rows.length) { list.replaceChildren(emptyState("No shared providers yet.", "Add a Lightning or on-chain LND wallet below.")); return; }
      var head = ["Name", "Method", "Network", "Fee", "Test mode", "Live", "Status", ""];
      list.replaceChildren(table(head, rows.map(function (r) {
        var toggleBtn = el("button", { text: r.enabled ? "Disable provider" : "Enable provider" });
        toggleBtn.addEventListener("click", function () {
          function run() { busy(toggleBtn, function () { return api("POST", "/admin/shared/" + r.id, { enabled: !r.enabled }); }).then(load).catch(function (e) { msg(out, e.message, true); }); }
          if (!r.enabled) return run();
          confirmChoice("Disable " + r.name + "?", "Projects can no longer start payments on it. Existing balances and refunds are not affected.", "Keep provider", "Disable provider", run);
        });
        var feeBtn = el("button", { text: "Override fee", onclick: function () { feeOverride(r); } });
        return el("tr", {}, [el("td", { text: r.name }), el("td", { class: "muted", text: methodName(r.method) + ", " + r.provider_type }), el("td", { class: "muted", text: r.network }),
          el("td", {}, [feeMoney(r.fee_bps, r.fee_base_msat)]),
          el("td", {}, [status(r.allow_untrusted ? "active" : "expired", r.allow_untrusted ? "Allowed" : "Blocked")]),
          el("td", {}, [status(r.allow_trusted ? "active" : "expired", r.allow_trusted ? "Allowed" : "Blocked")]),
          el("td", {}, [status(r.enabled ? "active" : "expired", r.enabled ? "On" : "Off")]),
          el("td", { class: "num" }, [toggleBtn, " ", feeBtn])]);
      })));
    }).catch(function (e) { msg(list, e.message, true); });
  }

  function feeOverride(r) {
    var project = el("input", { type: "number", min: "1", "aria-label": "Project id" });
    var bps = el("input", { type: "number", min: "0", "aria-label": "Fee in basis points" });
    var base = el("input", { type: "number", min: "0", "aria-label": "Base fee in msats" });
    var save = el("button", { class: "primary", text: "Save override" });
    var cancel = el("button", { text: "Cancel", onclick: function () { overrideBox.replaceChildren(); } });
    save.addEventListener("click", function () {
      if (!project.value) { msg(out, "Enter the project id the override is for.", true); return; }
      busy(save, function () {
        return api("POST", "/admin/shared/" + r.id + "/projects/" + encodeURIComponent(project.value) + "/fee", {
          fee_bps: bps.value === "" ? null : Number(bps.value), fee_base_msat: base.value === "" ? null : Number(base.value)
        });
      }).then(function () { overrideBox.replaceChildren(); msg(out, "Override saved for project " + project.value + ".", false); }).catch(function (e) { msg(out, e.message, true); });
    });
    overrideBox.replaceChildren(el("div", { class: "card" }, [
      el("h3", { text: "Fee override for " + r.name }),
      el("p", { class: "muted", text: "Leave a fee blank to use the provider default. Leave both blank to remove the override." }),
      field("Project id", project), field("Fee (basis points)", bps), field("Base fee (msats)", base),
      el("div", { class: "row" }, [save, cancel])
    ]));
    project.focus();
  }

  var name = el("input", { placeholder: "Platform Lightning" });
  var kind = el("select", {}, [el("option", { value: "lightning", text: "Lightning" }), el("option", { value: "onchain", text: "On-chain" })]);
  var network = el("select", {}, ["mainnet", "testnet", "signet", "regtest"].map(function (n) { return el("option", { value: n, text: n }); }));
  var url = el("input", { placeholder: "https://host:10009" });
  var cert = el("textarea", { placeholder: "tls.cert (PEM)" });
  var mac = el("textarea", { placeholder: "Macaroon (hex)" });
  var bps = el("input", { type: "number", min: "0", placeholder: "100" });
  var base = el("input", { type: "number", min: "0", placeholder: "0" });
  var untrusted = el("input", { type: "checkbox" });
  var add = el("button", { class: "primary", text: "Add shared provider" });
  add.addEventListener("click", function () {
    var cfg = { url: url.value.trim(), cert: cert.value.trim(), macaroon: mac.value.trim() };
    busy(add, function () {
      return api("POST", "/admin/shared", { name: name.value, method: kind.value, provider_type: "lnd", network: network.value, config: cfg, fee_bps: Number(bps.value || 0), fee_base_msat: Number(base.value || 0), allow_untrusted: untrusted.checked });
    }).then(function () { cert.value = ""; mac.value = ""; msg(out, "Shared provider added.", false); load(); }).catch(function (e) { msg(out, e.message, true); });
  });
  body.appendChild(section("Providers", [list, overrideBox]));
  body.appendChild(section("Add a shared provider", [el("div", { class: "card" }, [
    el("div", { class: "banner warn", role: "note" }, [el("div", {}, [el("b", { text: "Test mode projects stay off mainnet" }), "Only providers on testnet, signet or regtest can be allowed for test mode projects."])]),
    el("div", { class: "form" }, [field("Name", name), field("Method", kind), field("Network", network), field("LND url", url), field("tls.cert", cert), field("Macaroon", mac),
      field("Platform fee (basis points)", bps), field("Base fee (msats)", base), el("label", { class: "check" }, [untrusted, "Allow test mode projects"]), el("div", { class: "row" }, [add])])
  ])]));
  body.appendChild(out);
  load();
}
