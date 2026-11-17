(function () {
  "use strict";
  var $ = function (id) { return document.getElementById(id); };
  var token = sessionStorage.getItem("pgw-admin") || "";
  var timer = null;

  function el(tag, attrs, kids) {
    var n = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      var v = attrs[k];
      if (v === null || v === undefined) return;
      if (k === "text") n.textContent = v;
      else if (k === "class") n.className = v;
      else if (k.indexOf("on") === 0) n.addEventListener(k.slice(2), v);
      else n.setAttribute(k, v);
    });
    (kids || []).forEach(function (c) { if (c) n.appendChild(typeof c === "string" ? document.createTextNode(c) : c); });
    return n;
  }

  function api(path, body) {
    var opts = { headers: { authorization: "Bearer " + token } };
    if (body !== undefined) {
      opts.method = "POST";
      opts.headers["content-type"] = "application/json";
      opts.body = JSON.stringify(body);
    }
    return fetch(path, opts).then(function (r) {
      if (r.status === 401) { signOut(); throw new Error("unauthorized"); }
      return r.json().catch(function () { return {}; }).then(function (b) {
        if (!r.ok) throw new Error(b.error || ("HTTP " + r.status));
        return b;
      });
    });
  }

  function kv(rows) {
    var dl = el("dl", { class: "kv" });
    rows.forEach(function (r) {
      dl.appendChild(el("dt", { text: r[0] }));
      dl.appendChild(el("dd", { class: r[2] ? "mono" : "", text: String(r[1]) }));
    });
    return dl;
  }

  function part(box, p, render) {
    if (!p.ok) { box.replaceChildren(el("span", { class: "err", text: p.error })); return; }
    box.replaceChildren(render(p.value));
  }

  function sats(n) { return Number(n).toLocaleString() + " sats"; }

  function renderStatus(s) {
    var ready = $("ready");
    ready.textContent = s.ready ? "ready" : "not ready";
    ready.className = "badge " + (s.ready ? "ok" : "bad");
    part($("chain-body"), s.chain, function (c) { return kv([["Height", c.height], ["Miner wallet", sats(c.miner_sats)]]); });
    var node = function (n) {
      return kv([["Pubkey", n.pubkey.slice(0, 20) + "...", true], ["Synced", n.synced ? "yes" : "no"], ["Height", n.height],
        ["On-chain", sats(n.onchain_sats) + (n.unconfirmed_sats ? " (+" + sats(n.unconfirmed_sats) + " pending)" : "")],
        ["Active channels", n.active_channels]]);
    };
    part($("gw-body"), s.gateway, node);
    part($("cu-body"), s.customer, node);
    var ch = $("channels");
    if (s.channels.ok) {
      var rows = s.channels.value.map(function (c) {
        return el("tr", {}, [el("td", { text: c.active ? "active" : "pending" }), el("td", { text: sats(c.capacity_sats) }),
          el("td", { text: sats(c.local_sats) }), el("td", { text: sats(c.remote_sats) })]);
      });
      ch.replaceChildren(rows.length ? el("table", {}, [
        el("tr", {}, [el("th", { text: "Channel" }), el("th", { text: "Capacity" }), el("th", { text: "Customer" }), el("th", { text: "Gateway" })])
      ].concat(rows)) : el("span", { class: "muted", text: "No channels yet" }));
    }
  }

  function msg(id, text, bad) {
    var n = $(id);
    n.textContent = text;
    n.className = bad ? "err" : "muted";
  }

  function wrap(btn, fn) {
    btn.addEventListener("click", function () {
      btn.disabled = true;
      Promise.resolve().then(fn).catch(function (e) { alert(e.message); }).then(function () { btn.disabled = false; refresh(); });
    });
  }

  function paymentRow(p) {
    var attempts = (p.attempts || []).map(function (a) {
      var d = a.details || {};
      var text = a.method + " " + a.status;
      return el("div", {}, [el("span", { class: "pill " + a.status, text: text }), " ",
        d.bolt11 ? el("code", { class: "mono", text: d.bolt11.slice(0, 40) + "...", title: d.bolt11 }) : null,
        d.address ? el("code", { class: "mono", text: d.address }) : null]);
    });
    var actions = el("div", { class: "row" });
    if (p.status === "pending") {
      ["lightning", "onchain"].forEach(function (m) {
        actions.appendChild(el("button", { class: "ghost", text: "Use " + m, onclick: function () {
          api("/sandbox/api/payments/" + p.id + "/attempt", { method: m }).then(refresh).catch(function (e) { alert(e.message); });
        } }));
      });
      actions.appendChild(el("button", { text: "Pay from customer", onclick: function () {
        api("/sandbox/api/payments/" + p.id + "/pay", {}).then(function (r) { msg("p-msg", r.message); refresh(); })
          .catch(function (e) { msg("p-msg", e.message, true); });
      } }));
    }
    actions.appendChild(el("a", { href: p.pay_url, target: "_blank", rel: "noopener", text: "Open pay page" }));
    return el("tr", {}, [
      el("td", { class: "mono", text: p.id.slice(0, 8) }),
      el("td", { text: p.reference }),
      el("td", { text: p.amount + " " + p.currency }),
      el("td", {}, [el("span", { class: "pill " + p.status, text: p.status })]),
      el("td", {}, attempts.concat([actions]))
    ]);
  }

  function renderPayments(list) {
    $("payments").replaceChildren(list.length ? el("table", {}, [
      el("tr", {}, ["Id", "Reference", "Amount", "Status", "Attempts and actions"].map(function (h) { return el("th", { text: h }); }))
    ].concat(list.map(paymentRow))) : el("span", { class: "muted", text: "None yet" }));
  }

  function renderHooks(list) {
    $("hooks").replaceChildren(list.length ? el("table", {}, [
      el("tr", {}, ["Time", "Event", "Payment", "Status", "Signature", "Body"].map(function (h) { return el("th", { text: h }); }))
    ].concat(list.map(function (h) {
      return el("tr", {}, [
        el("td", { text: new Date(h.received).toLocaleTimeString() }),
        el("td", { text: h.event_type }),
        el("td", { class: "mono", text: h.payment_id.slice(0, 8) }),
        el("td", { text: h.status }),
        el("td", {}, [el("span", { class: "pill " + (h.signature_valid ? "paid" : "failed"), text: h.signature_valid ? "valid" : "invalid" })]),
        el("td", {}, [el("details", {}, [el("summary", { text: "json" }), el("pre", { class: "mono", text: JSON.stringify(JSON.parse(h.body || "{}"), null, 2) })])])
      ]);
    }))) : el("span", { class: "muted", text: "None yet. Pay a payment and the signed event shows up here." }));
  }

  function refresh() {
    if (!token) return Promise.resolve();
    return Promise.all([api("/sandbox/api/status"), api("/sandbox/api/payments"), api("/sandbox/api/hooks")]).then(function (r) {
      renderStatus(r[0]); renderPayments(r[1]); renderHooks(r[2]);
    }).catch(function () {});
  }

  function show() {
    $("login").hidden = !!token;
    $("console").hidden = !token;
    if (token) { refresh(); clearInterval(timer); timer = setInterval(refresh, 3000); }
  }

  function signOut() { token = ""; sessionStorage.removeItem("pgw-admin"); clearInterval(timer); show(); }

  $("signin").addEventListener("click", function () {
    var t = $("token").value.trim();
    fetch("/sandbox/api/status", { headers: { authorization: "Bearer " + t } }).then(function (r) {
      if (r.status === 401) { $("login-error").textContent = "Wrong token"; return; }
      token = t; sessionStorage.setItem("pgw-admin", t); $("token").value = ""; show();
    });
  });
  $("signout").addEventListener("click", signOut);

  wrap($("mine"), function () { return api("/sandbox/api/mine", { blocks: Number($("mine-n").value) }); });
  wrap($("fund-gw-btn"), function () { return api("/sandbox/api/fund", { target: "gateway", sats: Number($("fund-gw").value) }); });
  wrap($("fund-cu-btn"), function () { return api("/sandbox/api/fund", { target: "customer", sats: Number($("fund-cu").value) }); });
  wrap($("faucet-btn"), function () {
    return api("/sandbox/api/faucet", { address: $("faucet-addr").value, sats: Number($("faucet-sats").value) })
      .then(function (r) { msg("tool-msg", "sent " + r.txid); }).catch(function (e) { msg("tool-msg", e.message, true); });
  });
  wrap($("pay-bolt-btn"), function () {
    return api("/sandbox/api/pay-invoice", { bolt11: $("pay-bolt").value })
      .then(function () { msg("tool-msg", "invoice paid"); }).catch(function (e) { msg("tool-msg", e.message, true); });
  });
  wrap($("bootstrap"), function () {
    var log = $("setup-log"); log.hidden = false; log.textContent = "Working. This can take a minute.\n";
    return api("/sandbox/api/bootstrap", {}).then(function (r) {
      log.textContent = (r.log || []).join("\n") + (r.ok ? "\nDone." : "\nFAILED: " + r.error);
    }).catch(function (e) { log.textContent += "\nFAILED: " + e.message; });
  });
  wrap($("rotate"), function () {
    return api("/sandbox/api/rotate-key", {}).then(function (r) {
      var log = $("setup-log"); log.hidden = false; log.textContent = "New sandbox API key (shown once):\n" + r.api_key;
    });
  });
  wrap($("p-create"), function () {
    return api("/sandbox/api/payments", { reference: $("p-ref").value, amount: Number($("p-amount").value), currency: $("p-cur").value })
      .then(function () { msg("p-msg", "created"); }).catch(function (e) { msg("p-msg", e.message, true); });
  });

  show();
})();
