(function () {
  "use strict";
  var parts = location.pathname.split("/").filter(Boolean);
  var embed = parts[0] === "embed";
  var id = parts[1];
  var app = document.getElementById("app");
  var state = null;
  var busy = false;
  var errorText = "";
  var timer = null;
  var announced = {};
  var chosenOption = {};
  if (embed) document.body.classList.add("embed");

  function el(tag, attrs, children) {
    var n = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      if (attrs[k] === null || attrs[k] === undefined) return;
      if (k === "text") n.textContent = attrs[k];
      else if (k === "class") n.className = attrs[k];
      else if (k.indexOf("on") === 0) n.addEventListener(k.slice(2), attrs[k]);
      else n.setAttribute(k, attrs[k]);
    });
    (children || []).forEach(function (c) {
      if (c) n.appendChild(typeof c === "string" ? document.createTextNode(c) : c);
    });
    return n;
  }

  function money(amount, currency) {
    if (currency === "BTC") {
      var sats = Math.ceil(amount / 1000);
      return sats.toLocaleString() + " sats";
    }
    var scale = 100;
    var code = currency;
    if (currency.length === 4 && currency[0] === "M") {
      scale = 100000;
      code = currency.slice(1);
    }
    try {
      return new Intl.NumberFormat(undefined, { style: "currency", currency: code }).format(amount / scale);
    } catch (e) {
      return (amount / scale).toFixed(2) + " " + currency;
    }
  }

  function post(type, extra) {
    if (!embed || window.parent === window || !state) return;
    var msg = Object.assign({ source: "payment-gateway", type: "pgw:" + type, payment_id: state.id }, extra || {});
    (state.allowed_origins || []).forEach(function (o) {
      try { window.parent.postMessage(msg, o); } catch (e) {}
    });
  }

  function announce(type, extra) {
    var key = type + (extra && extra.status ? extra.status : "");
    if (announced[key]) return;
    announced[key] = true;
    post(type, extra);
  }

  function api(path, opts) {
    return fetch(path, opts).then(function (r) {
      return r.json().catch(function () { return {}; }).then(function (b) {
        if (!r.ok) throw new Error(b.error || "Request failed");
        return b;
      });
    });
  }

  function load() {
    return api("/pay/" + id + "/state").then(function (s) {
      state = s;
      render();
    }).catch(function (e) {
      app.replaceChildren(el("div", { class: "card" }, [el("p", { class: "error", text: e.message })]));
    });
  }

  function applyAccent() {
    var c = state.tenant.accent_color;
    if (c && /^#[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$/.test(c)) {
      document.documentElement.style.setProperty("--accent", c);
    }
  }

  function brand() {
    var kids = [];
    var logo = state.tenant.logo_url;
    if (logo && /^https:\/\//.test(logo)) {
      kids.push(el("img", { src: logo, alt: "", referrerpolicy: "no-referrer" }));
    }
    kids.push(el("span", { class: "name", text: state.tenant.name }));
    return el("header", { class: "brand" }, kids);
  }

  function copyRow(label, value) {
    var btn = el("button", {
      type: "button",
      text: "Copy",
      onclick: function () {
        if (navigator.clipboard) {
          navigator.clipboard.writeText(value).then(function () {
            btn.textContent = "Copied";
            setTimeout(function () { btn.textContent = "Copy"; }, 1500);
          });
        }
      }
    });
    return el("div", { class: "field" }, [
      el("label", { text: label }),
      el("div", { class: "copy" }, [el("code", { text: value, title: value }), btn])
    ]);
  }

  function pendingAttempt() {
    var list = (state.attempts || []).filter(function (a) { return a.status === "pending"; });
    return list.length ? list[list.length - 1] : null;
  }

  function attemptView(a) {
    var d = a.details || {};
    var kids = [];
    if (d.checkout_url) {
      var link = el("a", {
        class: "primary",
        href: d.checkout_url,
        target: embed ? "_top" : "_self",
        rel: "noopener",
        text: "Continue to payment"
      });
      link.className = "primary";
      link.style.cssText = "display:block;padding:12px 14px;border-radius:10px;background:var(--accent);color:#fff;text-align:center;font-weight:600;text-decoration:none";
      kids.push(link);
      kids.push(el("p", { class: "muted", text: "You will be taken to our payment provider to finish." }));
    }
    if (d.bolt11) {
      kids.push(el("a", { href: "lightning:" + d.bolt11 }, [
        el("img", { class: "qr", src: "/pay/" + id + "/qr/" + a.id, alt: "Lightning invoice QR code" })
      ]));
      kids.push(copyRow("Lightning invoice", d.bolt11));
    }
    if (d.address) {
      kids.push(el("img", { class: "qr", src: "/pay/" + id + "/qr/" + a.id, alt: "Bitcoin payment QR code" }));
      kids.push(copyRow("Bitcoin address", d.address));
      kids.push(copyRow("Amount (sats)", String(d.amount_sats)));
    }
    if (d.deposit_address) {
      kids.push(copyRow("Send " + (d.coin || "") + " to", d.deposit_address));
      kids.push(copyRow("Amount (" + (d.coin || "") + ")", String(d.deposit_amount)));
      kids.push(el("p", { class: "muted", text: "Send exactly this amount. It converts to Bitcoin automatically." }));
    }
    kids.push(el("p", { class: "muted" }, [
      el("span", { class: "spinner" }),
      "Waiting for payment"
    ]));
    kids.push(el("p", { class: "muted", text: "Total " + money(a.amount, a.currency) }));
    return kids;
  }

  function choose(m, option) {
    if (busy) return;
    busy = true;
    errorText = "";
    render();
    api("/pay/" + id + "/attempts", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ provider_id: m.provider_id, option: option || null })
    }).then(function (s) {
      state = s;
    }).catch(function (e) {
      errorText = e.message;
    }).then(function () {
      busy = false;
      render();
    });
  }

  function methodLabel(m) {
    return { lightning: "Lightning", onchain: "Bitcoin", stripe: "Card", revolut: "Card (Revolut)", paypal: "PayPal", fixedfloat: "Other crypto" }[m] || m;
  }

  function methodList() {
    var ul = el("ul", { class: "methods" });
    state.methods.forEach(function (m) {
      if (m.options && m.options.length) {
        var sel = el("select", { "aria-label": "Coin", style: "font:inherit;padding:6px;border-radius:8px" },
          m.options.map(function (o) { return el("option", { value: o, text: o }); }));
        sel.value = chosenOption[m.provider_id] || m.options[0];
        sel.addEventListener("change", function () { chosenOption[m.provider_id] = sel.value; });
        var go = el("button", { class: "method", type: "button", onclick: function () { choose(m, sel.value); } }, [
          el("span", { text: methodLabel(m.method) }), sel
        ]);
        ul.appendChild(el("li", {}, [go]));
        return;
      }
      var fee = m.fee > 0 ? el("span", { class: "fee", text: "+" + money(m.fee, m.currency) + " fee" }) : null;
      ul.appendChild(el("li", {}, [
        el("button", { class: "method", type: "button", disabled: busy ? "disabled" : null, onclick: function () { choose(m); } }, [
          el("span", { text: methodLabel(m.method) }), fee
        ])
      ]));
    });
    return ul;
  }

  function goBack() {
    var u = state.return_url;
    if (!u) return;
    if (embed) {
      window.open(u, "_top");
    } else {
      location.assign(u);
    }
  }

  function terminal() {
    var s = state.status;
    var ok = s === "paid" || s === "refunded" || s === "partially_refunded";
    var title = ok ? "Payment received" : s === "expired" ? "This payment has expired" : s === "cancelled" ? "This payment was cancelled" : "Payment " + s;
    var kids = [el("div", { class: "status " + (ok ? "ok" : "bad") }, [
      el("div", { class: "icon", "aria-hidden": "true", text: ok ? "\u2713" : "\u2717" }),
      el("h2", { text: title })
    ])];
    if (state.return_url) {
      kids.push(el("button", { class: "primary", type: "button", text: ok ? "Continue" : "Back", onclick: goBack }));
    }
    return kids;
  }

  function render() {
    document.title = "Pay " + state.tenant.name;
    applyAccent();
    var card = el("div", { class: "card" }, [
      brand(),
      el("div", { class: "amount", text: money(state.amount, state.currency) }),
      state.description ? el("p", { class: "desc", text: state.description }) : null
    ]);
    if (state.status !== "pending") {
      terminal().forEach(function (n) { card.appendChild(n); });
      announce(state.status === "paid" ? "paid" : state.status, { status: state.status });
      if (!embed && state.status === "paid" && state.return_url && !card.dataset.redirected) {
        setTimeout(goBack, 2500);
      }
    } else {
      var a = pendingAttempt();
      if (a) {
        attemptView(a).forEach(function (n) { card.appendChild(n); });
        if (state.methods.length > 1 || (state.methods.length === 1 && state.methods[0].options.length)) {
          card.appendChild(el("p", {}, [el("button", { class: "link", type: "button", text: "Use a different method", onclick: function () {
            state.attempts = (state.attempts || []).filter(function (x) { return x.id !== a.id; });
            render();
          } })]));
        }
      } else if (state.methods.length) {
        card.appendChild(el("p", { class: "muted", text: "Choose how to pay" }));
        card.appendChild(methodList());
      } else {
        card.appendChild(el("p", { class: "error", text: "No payment methods are available." }));
      }
    }
    if (errorText) card.appendChild(el("p", { class: "error", role: "alert", text: errorText }));
    app.replaceChildren(card);
    announce("ready");
    resize();
  }

  function resize() {
    post("resize", { height: Math.ceil(document.documentElement.getBoundingClientRect().height) });
  }

  function refresh() {
    if (timer) return;
    timer = setTimeout(function () {
      timer = null;
      api("/pay/" + id + "/state").then(function (s) {
        var showing = pendingAttempt();
        var hidden = showing && !(s.attempts || []).some(function (x) { return x.id === showing.id && x.status === "pending"; });
        state = s;
        if (hidden || !busy) render();
      }).catch(function () {});
    }, 150);
  }

  function listen() {
    if (!window.EventSource) {
      setInterval(refresh, 4000);
      return;
    }
    var es = new EventSource("/pay/" + id + "/events");
    es.addEventListener("change", refresh);
    setInterval(refresh, 20000);
  }

  window.addEventListener("message", function () {});
  load().then(listen);
})();
