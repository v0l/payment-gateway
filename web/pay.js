(function () {
  "use strict";
  var I = window.PayI18n;
  var parts = location.pathname.split("/").filter(Boolean);
  var embed = parts[0] === "embed";
  var id = parts[1];
  var app = document.getElementById("app");
  var state = null;
  var busy = false;
  var errorKey = "";
  var errorRaw = "";
  var notice = "";
  var timer = null;
  var announced = {};
  var chosenOption = {};
  var dict = null;
  var fallback = null;
  var lang = "en";
  var clockSkew = 0;
  var countdownNode = null;
  var countdownTimer = null;
  var lastAttemptId = null;
  var lastSaid = "";
  var focusKey = null;
  if (embed) document.body.classList.add("embed");

  function t(key, vars) {
    return I.translate(dict, fallback, key, vars);
  }

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
    return I.formatMoney(amount, currency, lang);
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
        if (!r.ok) {
          var err = new Error(b.error || "request failed");
          err.status = r.status;
          throw err;
        }
        return b;
      });
    });
  }

  function loadDict(code) {
    return fetch("/assets/i18n/" + code + ".json").then(function (r) { return r.json(); });
  }

  function chooseLanguage(tenantDefault) {
    var q = new URLSearchParams(location.search).get("lang");
    return I.pickLanguage(q, tenantDefault, navigator.languages || [navigator.language]);
  }

  function setLanguage(code) {
    var tasks = [loadDict(code)];
    if (!fallback) tasks.push(loadDict("en"));
    return Promise.all(tasks).then(function (r) {
      dict = r[0];
      if (r[1]) fallback = r[1];
      lang = code;
      document.documentElement.lang = code;
      document.documentElement.dir = I.direction(code);
    });
  }

  function load() {
    return api("/pay/" + id + "/state").then(function (s) {
      setClock(s);
      state = s;
      return setLanguage(chooseLanguage(s.branding && s.branding.default_lang));
    }).then(function () {
      render();
    }).catch(function (e) {
      var go = dict ? Promise.resolve() : setLanguage(chooseLanguage(null)).catch(function () {});
      go.then(function () {
        app.replaceChildren(el("div", { class: "card" }, [el("p", { class: "error", role: "alert", text: dict ? t("error_load") : e.message })]));
        postError("load_failed", e.message);
      });
    });
  }

  function postError(code, message) {
    if (!embed || window.parent === window) return;
    var origins = (state && state.allowed_origins) || [];
    origins.forEach(function (o) {
      try { window.parent.postMessage({ source: "payment-gateway", type: "pgw:error", payment_id: id, code: code, message: message }, o); } catch (e) {}
    });
  }

  function setClock(s) {
    if (s.server_time) clockSkew = Date.parse(s.server_time) - Date.now();
  }

  function luminance(hex) {
    var h = hex.replace("#", "");
    if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
    var rgb = [0, 2, 4].map(function (i) {
      var v = parseInt(h.slice(i, i + 2), 16) / 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    });
    return 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
  }

  function contrast(a, b) {
    var x = luminance(a), y = luminance(b);
    return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
  }

  function readableOn(hex) {
    return luminance(hex) > 0.179 ? "#000000" : "#ffffff";
  }

  function isDark() {
    var theme = (state.branding && state.branding.theme) || "auto";
    if (theme === "dark") return true;
    if (theme === "light") return false;
    return !!(window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches);
  }

  function applyBranding() {
    var b = state.branding || {};
    var root = document.documentElement;
    if (b.theme === "light" || b.theme === "dark") root.setAttribute("data-theme", b.theme);
    else root.removeAttribute("data-theme");
    root.setAttribute("data-radius", b.radius || "md");
    root.setAttribute("data-font", b.font || "system");
    var dark = isDark();
    var c = dark && b.accent_dark ? b.accent_dark : state.tenant.accent_color;
    var card = getComputedStyle(root).getPropertyValue("--card").trim();
    root.style.removeProperty("--accent");
    root.style.removeProperty("--accent-text");
    if (c && /^#[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$/.test(c) && (!/^#[0-9a-fA-F]{6}$/.test(card) || contrast(c, card) >= 4.5)) {
      root.style.setProperty("--accent", c);
      root.style.setProperty("--accent-text", readableOn(c));
    }
  }

  function brand() {
    var kids = [];
    var b = state.branding || {};
    var logo = isDark() && b.logo_dark_url ? b.logo_dark_url : state.tenant.logo_url;
    if (logo && /^https:\/\//.test(logo)) {
      kids.push(el("img", { src: logo, alt: "", referrerpolicy: "no-referrer" }));
    }
    kids.push(el("span", { class: "name", text: state.tenant.name }));
    return el("header", { class: "brand" }, kids);
  }

  function copyRow(label, value, key) {
    var btn = el("button", {
      type: "button",
      "data-focus": key,
      "aria-label": t("copy_label", { label: label }),
      text: t("copy"),
      onclick: function () {
        var done = function () {
          btn.textContent = t("copied");
          setTimeout(function () { btn.textContent = t("copy"); }, 1500);
        };
        if (navigator.clipboard) navigator.clipboard.writeText(value).then(done);
      }
    });
    var fieldId = "f-" + key;
    return el("div", { class: "field" }, [
      el("label", { for: fieldId, text: label }),
      el("div", { class: "copy" }, [el("code", { id: fieldId, text: value, title: value }), btn])
    ]);
  }

  function pendingAttempt() {
    var list = (state.attempts || []).filter(function (a) { return a.status === "pending"; });
    return list.length ? list[list.length - 1] : null;
  }

  function lastEnded() {
    var list = (state.attempts || []).filter(function (a) { return a.status === "expired" || a.status === "failed"; });
    return list.length ? list[list.length - 1] : null;
  }

  function progressOf(a) {
    return a.progress || { required: 0, received: 0, confirmed: 0, min_confirmations: 1 };
  }

  function expiryMs() {
    return Date.parse(state.expires) - (Date.now() + clockSkew);
  }

  function countdown() {
    countdownNode = el("p", { class: "muted countdown" });
    updateCountdown();
    return countdownNode;
  }

  function updateCountdown() {
    if (!countdownNode || !state) return;
    var ms = expiryMs();
    countdownNode.textContent = ms > 0 ? t("expires_in", { time: I.formatCountdown(ms) }) : t("expired_now");
    countdownNode.classList.toggle("urgent", ms > 0 && ms < 60000);
    if (ms <= 0 && state.status === "pending") refresh();
  }

  function startClock() {
    if (countdownTimer) return;
    countdownTimer = setInterval(updateCountdown, 1000);
  }

  function walletButton(uri, key) {
    return el("a", { class: "primary", href: uri, "data-focus": key, text: t("open_wallet") });
  }

  function onchainStatus(a) {
    var p = progressOf(a);
    var kids = [];
    if (p.received > 0 && p.received < p.required) {
      var remaining = p.required - p.received;
      kids.push(el("p", { class: "notice warn", text: t("underpaid", {
        received: money(p.received, a.currency),
        required: money(p.required, a.currency),
        remaining: money(remaining, a.currency)
      }) }));
    }
    if (p.received > 0) {
      kids.push(el("p", { class: "notice", text: t("detected") }));
      kids.push(el("p", { class: "confirmations", text: t("confirmations", {
        done: p.confirmed >= p.required ? p.min_confirmations : 0,
        total: p.min_confirmations
      }) }));
    }
    return kids;
  }

  function attemptView(a) {
    var d = a.details || {};
    var kids = [];
    if (d.checkout_url) {
      kids.push(el("a", {
        class: "primary",
        href: d.checkout_url,
        target: embed ? "_top" : "_self",
        rel: "noopener",
        "data-focus": "primary",
        text: t("continue_to_provider")
      }));
      kids.push(el("p", { class: "muted", text: t("provider_redirect") }));
    }
    if (d.bolt11) {
      kids.push(el("img", { class: "qr", src: "/pay/" + id + "/qr/" + a.id, alt: t("qr_lightning") }));
      kids.push(walletButton("lightning:" + d.bolt11, "primary"));
      kids.push(copyRow(t("lightning_invoice"), d.bolt11, "invoice"));
    }
    if (d.address) {
      kids.push(onchainStatus(a));
      kids.push(el("img", { class: "qr", src: "/pay/" + id + "/qr/" + a.id, alt: t("qr_bitcoin") }));
      if (d.uri) kids.push(walletButton(d.uri, "primary"));
      kids.push(copyRow(t("bitcoin_address"), d.address, "address"));
      kids.push(copyRow(t("amount_sats"), String(d.amount_sats), "amount"));
    }
    if (d.deposit_address) {
      kids.push(copyRow(t("send_to", { coin: d.coin || "" }), d.deposit_address, "deposit"));
      kids.push(copyRow(t("amount_coin", { coin: d.coin || "" }), String(d.deposit_amount), "deposit-amount"));
      kids.push(el("p", { class: "muted", text: t("send_exact") }));
    }
    kids.push(el("p", { class: "muted" }, [
      el("span", { class: "spinner", "aria-hidden": "true" }),
      t("waiting")
    ]));
    kids.push(countdown());
    kids.push(el("p", { class: "muted", text: t("total", { amount: money(a.amount, a.currency) }) }));
    return kids.reduce(function (all, k) { return all.concat(k); }, []);
  }

  function choose(m, option) {
    if (busy) return;
    busy = true;
    errorKey = "";
    errorRaw = "";
    notice = "";
    render();
    api("/pay/" + id + "/attempts", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ provider_id: m.provider_id, option: option || null })
    }).then(function (s) {
      setClock(s);
      state = s;
      focusKey = "primary";
    }).catch(function (e) {
      errorKey = "error_generic";
      errorRaw = e.status === 409 ? "" : e.message;
      postError("attempt_failed", e.message);
    }).then(function () {
      busy = false;
      render();
    });
  }

  function methodList() {
    var ul = el("ul", { class: "methods" });
    state.methods.forEach(function (m) {
      var label = t("method." + m.method);
      if (m.options && m.options.length) {
        var sel = el("select", { "aria-label": t("coin") },
          m.options.map(function (o) { return el("option", { value: o, text: o }); }));
        sel.value = chosenOption[m.provider_id] || m.options[0];
        sel.addEventListener("change", function () { chosenOption[m.provider_id] = sel.value; });
        var go = el("button", { class: "method", type: "button", disabled: busy ? "disabled" : null, onclick: function () { choose(m, sel.value); } }, [
          el("span", { text: label })
        ]);
        ul.appendChild(el("li", { class: "with-option" }, [go, sel]));
        return;
      }
      var fee = m.fee > 0 ? el("span", { class: "fee", text: t("fee", { amount: money(m.fee, m.currency) }) }) : null;
      ul.appendChild(el("li", {}, [
        el("button", { class: "method", type: "button", disabled: busy ? "disabled" : null, onclick: function () { choose(m); } }, [
          el("span", { text: label }), fee
        ])
      ]));
    });
    return ul;
  }

  function goBack() {
    var u = state.return_url;
    if (!u) return;
    if (embed) window.open(u, "_top");
    else location.assign(u);
  }

  function overpaidNote() {
    var paid = (state.attempts || []).filter(function (a) { return a.status === "paid"; })[0];
    if (!paid) return null;
    var p = progressOf(paid);
    if (p.required > 0 && p.received > p.required) {
      return el("p", { class: "notice warn", text: t("overpaid", {
        extra: money(p.received - p.required, paid.currency),
        name: state.tenant.name
      }) });
    }
    return null;
  }

  function footer() {
    var b = state.branding || {};
    var links = (b.footer_links || []).filter(function (l) { return /^https:\/\//.test(l.url); });
    var kids = [];
    if (links.length) {
      kids.push(el("nav", { class: "footer-links", "aria-label": t("footer_links") }, links.map(function (l) {
        return el("a", { href: l.url, target: embed ? "_blank" : "_self", rel: "noopener noreferrer", text: l.label });
      })));
    }
    if (b.show_powered_by !== false) kids.push(el("p", { class: "powered", text: t("powered_by") }));
    return kids.length ? el("footer", { class: "foot" }, kids) : null;
  }

  function terminalTitle() {
    var s = state.status;
    if (s === "paid" || s === "refunded" || s === "partially_refunded") return t("paid_title");
    if (s === "expired") return t("expired_title");
    if (s === "cancelled") return t("cancelled_title");
    if (s === "failed") return t("failed_title");
    return t("status_other", { status: s });
  }

  function terminal() {
    var s = state.status;
    var ok = s === "paid" || s === "refunded" || s === "partially_refunded";
    var title = terminalTitle();
    var kids = [el("div", { class: "status " + (ok ? "ok" : "bad") }, [
      el("div", { class: "icon", "aria-hidden": "true", text: ok ? "\u2713" : "\u2717" }),
      el("h2", { tabindex: "-1", "data-focus": "title", text: title })
    ])];
    if (ok) {
      var thanks = state.branding && state.branding.thanks_text;
      kids.push(el("p", { class: "thanks", text: thanks || t("thanks_default") }));
      var over = overpaidNote();
      if (over) kids.push(over);
      if (state.receipt_url && /^https:\/\//.test(state.receipt_url)) {
        kids.push(el("a", { class: "secondary", href: state.receipt_url, target: "_blank", rel: "noopener", text: t("receipt") }));
      }
    }
    if (state.return_url) {
      kids.push(el("button", { class: "primary", type: "button", text: ok ? t("continue") : t("back", { name: state.tenant.name }), onclick: goBack }));
    }
    return kids;
  }

  function endedNotice() {
    var a = lastEnded();
    if (!a) return null;
    return el("p", { class: "notice warn", text: t(a.status === "failed" ? "attempt_failed" : "attempt_expired") });
  }

  function languagePicker() {
    var sel = el("select", { id: "lang", "aria-label": t("language") }, I.LANGUAGES.map(function (c) {
      var o = el("option", { value: c, text: I.NAMES[c] });
      if (c === lang) o.setAttribute("selected", "selected");
      return o;
    }));
    sel.value = lang;
    sel.addEventListener("change", function () {
      setLanguage(sel.value).then(function () { focusKey = "lang"; render(); });
    });
    return el("div", { class: "lang" }, [sel]);
  }

  function render() {
    countdownNode = null;
    document.title = t("page_title", { name: state.tenant.name });
    applyBranding();
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
        if (lastAttemptId !== a.id) focusKey = focusKey || "primary";
        lastAttemptId = a.id;
        attemptView(a).forEach(function (n) { card.appendChild(n); });
        if (state.methods.length > 1 || (state.methods.length === 1 && state.methods[0].options.length)) {
          card.appendChild(el("p", {}, [el("button", { class: "link", type: "button", text: t("use_other_method"), onclick: function () {
            state.attempts = (state.attempts || []).filter(function (x) { return x.id !== a.id; });
            focusKey = "methods";
            render();
          } })]));
        }
      } else if (state.methods.length) {
        var ended = endedNotice();
        if (ended) card.appendChild(ended);
        card.appendChild(el("h2", { class: "muted choose", tabindex: "-1", "data-focus": "methods", text: ended ? t("new_request") : t("choose_method") }));
        card.appendChild(methodList());
      } else {
        card.appendChild(el("p", { class: "error", text: t("no_methods") }));
      }
    }
    if (errorKey) card.appendChild(el("p", { class: "error", text: errorRaw ? t(errorKey) + " (" + errorRaw + ")" : t(errorKey) }));
    var nodes = [card];
    var f = footer();
    if (f) nodes.push(f);
    nodes.push(languagePicker());
    app.replaceChildren.apply(app, nodes);
    restoreFocus();
    say();
    announce("ready");
    if (state.status === "pending") startClock();
    resize();
  }

  function liveMessage() {
    if (state.status === "paid") return t("paid_announce");
    if (state.status !== "pending") return terminalTitle();
    var a = pendingAttempt();
    if (a && a.details && a.details.address) {
      var p = progressOf(a);
      if (p.received > 0 && p.received < p.required) return t("underpaid", { received: money(p.received, a.currency), required: money(p.required, a.currency), remaining: money(p.required - p.received, a.currency) });
      if (p.received > 0) return t("detected") + " " + t("confirmations", { done: p.confirmed >= p.required ? p.min_confirmations : 0, total: p.min_confirmations });
    }
    if (!a && lastEnded()) return t(lastEnded().status === "failed" ? "attempt_failed" : "attempt_expired");
    return "";
  }

  function say() {
    var text = liveMessage();
    if (text === lastSaid) return;
    lastSaid = text;
    document.getElementById("live").textContent = text;
  }

  function restoreFocus() {
    if (!focusKey) return;
    var target = app.querySelector('[data-focus="' + focusKey + '"]') || (focusKey === "lang" ? document.getElementById("lang") : null);
    focusKey = null;
    if (target) target.focus({ preventScroll: true });
  }

  function resize() {
    post("resize", { height: Math.ceil(document.documentElement.getBoundingClientRect().height) });
  }

  function watchSize() {
    if (!window.ResizeObserver) return;
    new ResizeObserver(function () { if (state) resize(); }).observe(document.body);
  }

  function refresh() {
    if (timer) return;
    timer = setTimeout(function () {
      timer = null;
      api("/pay/" + id + "/state").then(function (s) {
        setClock(s);
        var showing = pendingAttempt();
        var hidden = showing && !(s.attempts || []).some(function (x) { return x.id === showing.id && x.status === "pending"; });
        var before = state && state.status;
        state = s;
        if (hidden || !busy) {
          if (before !== s.status && s.status !== "pending") focusKey = "title";
          render();
        }
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

  watchSize();
  load().then(listen);
})();
