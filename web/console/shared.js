"use strict";
function feeText(bps, base) {
  return (bps / 100) + "%" + (base ? " + " + Math.ceil(base / 1000) + " sats" : "");
}

function sharedMethods(pid, container) {
  var box = el("div");
  container.appendChild(el("h3", { text: "Platform managed methods" }));
  container.appendChild(el("p", { class: "muted", text: "Use the platform's own Lightning and on-chain wallets. Payments are held as a balance for you, minus the fee shown." }));
  container.appendChild(box);
  function load() {
    api("GET", "/projects/" + pid + "/shared").then(function (rows) {
      if (!rows.length) { box.replaceChildren(el("p", { class: "muted", text: "No platform managed methods are available to this project." })); return; }
      var head = ["Name", "Method", "Network", "Platform fee", ""];
      box.replaceChildren(el("table", {}, [el("tr", {}, head.map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (r) {
        var btn = el("button", { class: r.enabled ? "" : "primary", text: r.enabled ? "Disable" : "Enable", onclick: function () {
          api("POST", "/projects/" + pid + "/shared/" + r.id + "/enabled", { enabled: !r.enabled }).then(function () { load(); }).catch(function (e) { msg(box, e.message, true); });
        } });
        return el("tr", {}, [el("td", {}, [r.name, " ", pill("platform managed", "ok")]), el("td", { text: r.method }), el("td", { text: r.network }), el("td", { text: feeText(r.fee_bps, r.fee_base_msat) }), el("td", {}, [btn])]);
      }))));
    }).catch(function (e) { msg(box, e.message, true); });
  }
  load();
}

function sharedAdmin() {
  var body = el("div");
  layout("shared", null, body);
  (function () {
    body.appendChild(el("h2", { text: "Shared providers" }));
    var list = el("div"); var out = el("div");
    function load() {
      api("GET", "/admin/shared").then(function (rows) {
        var head = ["Name", "Method", "Network", "Fee", "Test mode", "Live", "Status", ""];
        list.replaceChildren(rows.length ? el("table", {}, [el("tr", {}, head.map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (r) {
          return el("tr", {}, [el("td", { text: r.name }), el("td", { text: r.method + "/" + r.provider_type }), el("td", { text: r.network }),
            el("td", { text: feeText(r.fee_bps, r.fee_base_msat) }), el("td", { text: r.allow_untrusted ? "yes" : "no" }), el("td", { text: r.allow_trusted ? "yes" : "no" }),
            el("td", {}, [pill(r.enabled ? "enabled" : "disabled", r.enabled ? "ok" : "bad")]),
            el("td", {}, [el("a", { text: r.enabled ? "disable" : "enable", onclick: function () { api("POST", "/admin/shared/" + r.id, { enabled: !r.enabled }).then(load); } }), " ",
              el("a", { text: "fee override", onclick: function () { feeOverride(r); } })])]);
        }))) : el("p", { class: "muted", text: "No shared providers yet." }));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    function feeOverride(r) {
      var tid = prompt("Project id"); if (!tid) return;
      var bps = prompt("Fee in basis points (blank for default)", ""); var base = prompt("Base fee in msats (blank for default)", "");
      api("POST", "/admin/shared/" + r.id + "/projects/" + encodeURIComponent(tid) + "/fee", { fee_bps: bps === "" ? null : Number(bps), fee_base_msat: base === "" ? null : Number(base) })
        .then(function () { msg(out, "Override saved.", false); }).catch(function (e) { msg(out, e.message, true); });
    }
    var name = el("input", { placeholder: "Name" });
    var kind = el("select", {}, [el("option", { value: "lightning", text: "lightning" }), el("option", { value: "onchain", text: "onchain" })]);
    var network = el("select", {}, ["mainnet", "testnet", "signet", "regtest"].map(function (n) { return el("option", { value: n, text: n }); }));
    var url = el("input", { placeholder: "https://host:10009" });
    var cert = el("textarea", { placeholder: "tls.cert (PEM)" });
    var mac = el("textarea", { placeholder: "Macaroon (hex)" });
    var bps = el("input", { type: "number", placeholder: "Fee, basis points" });
    var base = el("input", { type: "number", placeholder: "Base fee, msats" });
    var untrusted = el("input", { type: "checkbox" });
    var add = el("button", { class: "primary", text: "Add shared provider" });
    add.addEventListener("click", function () {
      var cfg = { url: url.value.trim(), cert: cert.value.trim(), macaroon: mac.value.trim() };
      busy(add, function () { return api("POST", "/admin/shared", { name: name.value, method: kind.value, provider_type: "lnd", network: network.value, config: cfg, fee_bps: Number(bps.value || 0), fee_base_msat: Number(base.value || 0), allow_untrusted: untrusted.checked }); })
        .then(function () { cert.value = ""; mac.value = ""; load(); }).catch(function (e) { msg(out, e.message, true); });
    });
    body.appendChild(list);
    body.appendChild(el("div", { class: "card" }, [el("h3", { text: "Add a shared provider" }), field("Name", name), field("Method", kind), field("Network", network), field("LND url", url), field("tls.cert", cert), field("Macaroon", mac), field("Platform fee (basis points)", bps), field("Base fee (msats)", base), field("Allow test mode projects", untrusted), add]));
    body.appendChild(out);
    load();
  })();
}
