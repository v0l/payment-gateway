"use strict";
(function () {
  var root = document.getElementById("root");

  function el(tag, attrs, kids) {
    var n = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      var v = attrs[k];
      if (v === null || v === undefined || v === false) return;
      if (k === "text") n.textContent = v;
      else if (k === "class") n.className = v;
      else n.setAttribute(k, v === true ? "" : v);
    });
    (kids || []).forEach(function (c) {
      if (c === null || c === undefined || c === false) return;
      n.appendChild(typeof c === "string" ? document.createTextNode(c) : c);
    });
    return n;
  }
  function pill(text, cls) { return el("span", { class: "pill " + (cls || text), text: text }); }
  function section(title, kids) { return el("section", {}, [el("h3", { text: title })].concat(kids)); }

  var TOKENS = ["bg", "card", "code", "text", "muted", "line", "line-strong", "accent", "ok", "warn", "bad", "info"];
  var PAYMENT = ["pending", "paid", "expired", "cancelled", "failed", "disputed", "partially_refunded", "refunded"];
  var ATTEMPT = ["pending", "paid", "expired", "cancelled", "failed"];
  var REFUND = ["pending", "completed", "refund_failed"];
  var WITHDRAWAL = ["requested", "processing", "completed", "failed", "cancelled"];
  var WEBHOOK = ["delivered", "waiting", "retrying", "failed"];

  function swatches() {
    return el("div", { class: "swatches" }, TOKENS.map(function (t) {
      return el("div", { class: "swatch" }, [el("i", { class: "sw-" + t }), el("span", { text: "--" + t })]);
    }));
  }

  function buttons() {
    return el("div", { class: "row" }, [
      el("button", { class: "primary", text: "Save changes" }),
      el("button", { text: "Cancel" }),
      el("button", { class: "danger", text: "Delete project" }),
      el("button", { class: "primary", disabled: true, text: "Saving" })
    ]);
  }

  function inputs() {
    var bad = el("div", { class: "field invalid" }, [
      el("label", { text: "Webhook URL" }),
      el("input", { value: "http://localhost/hook", "aria-invalid": "true" }),
      el("div", { class: "hint", text: "Use an https URL that is reachable from the internet." })
    ]);
    return el("div", {}, [
      el("div", { class: "field" }, [el("label", { text: "Project name" }), el("input", { placeholder: "Coffee shop" }), el("div", { class: "hint", text: "Shown to your customers on the pay page." })]),
      bad,
      el("div", { class: "field" }, [el("label", { text: "Currency" }), el("select", {}, [el("option", { text: "EUR" }), el("option", { text: "USD" })])])
    ]);
  }

  function tables() {
    var rows = [["pay_8f3k2", "paid", "12.50", "EUR"], ["pay_1d9ac", "pending", "4.00", "EUR"], ["pay_77hq0", "expired", "99.00", "EUR"]];
    return el("div", {}, [
      el("table", {}, [
        el("thead", {}, [el("tr", {}, ["Payment", "Status", "Amount"].map(function (h) { return el("th", { text: h }); }))]),
        el("tbody", {}, rows.map(function (r) {
          return el("tr", { class: "click" }, [el("td", { class: "mono", text: r[0] }), el("td", {}, [pill(r[1])]), el("td", { class: "money", text: r[2] + " " + r[3] })]);
        }))
      ]),
      el("div", { class: "pager" }, [el("span", { class: "muted", text: "1 to 3 of 3" }), el("button", { disabled: true, text: "Previous" }), el("button", { disabled: true, text: "Next" })])
    ]);
  }

  function tabs() {
    return el("div", { class: "tabs" }, ["Overview", "Payments", "Providers", "Settings"].map(function (t, i) {
      return el("a", { href: "#", class: i === 1 ? "on" : "", text: t });
    }));
  }

  function badges() {
    function group(name, list) {
      return el("div", { class: "row" }, [el("span", { class: "muted", text: name })].concat(list.map(function (s) { return pill(s); })));
    }
    return el("div", {}, [group("Payment", PAYMENT), group("Attempt", ATTEMPT), group("Refund", REFUND), group("Withdrawal", WITHDRAWAL), group("Webhook", WEBHOOK)]);
  }

  function money() {
    return el("dl", { class: "kv" }, [
      el("dt", { text: "Fiat" }), el("dd", { class: "money", text: "\u20ac1,234.50" }),
      el("dt", { text: "Sats" }), el("dd", { class: "money", text: "250,000 sats" }),
      el("dt", { text: "Msats" }), el("dd", { class: "money", text: "250,000,000 msats" }),
      el("dt", { text: "Fee" }), el("dd", { class: "money", text: "\u20ac0.25", "data-minor": "25" })
    ]);
  }

  function copyable() {
    return el("div", {}, [
      el("div", { class: "copyable" }, [el("code", { text: "pk_live_4f9c0a77d2e1" }), el("button", { text: "Copy" })]),
      el("div", { class: "secret" }, [el("b", { text: "Webhook secret (shown once, copy it now)" }), el("span", { class: "mono", text: "whsec_9b1d52c0e7a4f3318" })])
    ]);
  }

  function qr() {
    var svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 9 9");
    svg.setAttribute("role", "img");
    svg.setAttribute("aria-label", "Example QR code");
    var cells = "111111101100010111010001101110101011101110101110101000100101111110111111100000000010101010111011101001001";
    for (var i = 0; i < 81; i++) {
      if (cells[i % cells.length] === "1") {
        var r = document.createElementNS("http://www.w3.org/2000/svg", "rect");
        r.setAttribute("x", i % 9); r.setAttribute("y", Math.floor(i / 9));
        r.setAttribute("width", 1); r.setAttribute("height", 1); r.setAttribute("fill", "#000000");
        svg.appendChild(r);
      }
    }
    return el("div", { class: "qr-block" }, [svg]);
  }

  function states() {
    return el("div", {}, [
      el("div", { class: "empty" }, [el("b", { text: "No payments yet" }), el("p", { class: "muted", text: "Create a payment from your app or use the console to send a test payment." }), el("button", { class: "primary", text: "Create test payment" })]),
      el("div", {}, [el("div", { class: "skeleton" }), el("div", { class: "skeleton" })]),
      el("p", { class: "err", role: "alert", text: "Could not load payments. Check your connection and try again." })
    ]);
  }

  function banners() {
    return el("div", {}, [
      el("div", { class: "banner info" }, [el("div", {}, [el("b", { text: "Test limits apply" }), "Payments above 100 EUR need operator approval."])]),
      el("div", { class: "banner ok" }, [el("div", {}, [el("b", { text: "Webhook secret rotated" }), "The old secret stops working in 24 hours."])]),
      el("div", { class: "banner warn" }, [el("div", {}, [el("b", { text: "Two factor is off" }), "Turn it on to protect withdrawals."])]),
      el("div", { class: "banner bad" }, [el("div", {}, [el("b", { text: "Provider rejected the key" }), "Check the API key for Stripe and save again."])]),
      el("p", {}, [el("span", { class: "toast", role: "status" }, ["Copied", el("button", { text: "Undo" })])])
    ]);
  }

  function dialog() {
    return el("div", { class: "dialog-scrim" }, [el("div", { class: "dialog", role: "alertdialog", "aria-modal": "true" }, [
      el("h4", { text: "Delete the Stripe provider?" }),
      el("p", { class: "muted", text: "Payments in progress keep their current attempt. New payments cannot use Stripe." }),
      el("div", { class: "row" }, [el("button", { text: "Keep provider" }), el("button", { class: "danger", text: "Delete provider" })])
    ])]);
  }

  function recovery() {
    return el("div", {}, [
      el("div", { class: "banner warn" }, [el("div", {}, [el("b", { text: "Save these recovery codes" }), "Each code works once. They are not shown again."])]),
      el("ul", { class: "codes" }, ["4f7k-9q2m", "x8d3-a1vz", "m2pw-6t0c", "h9ye-r5nb", "z3ju-8k4s", "c7fq-1w6d"].map(function (c) { return el("li", { text: c }); })),
      el("div", { class: "row" }, [el("button", { text: "Copy codes" }), el("button", { text: "Download" }), el("button", { class: "primary", text: "I saved them" })])
    ]);
  }

  function panel(theme) {
    var p = el("div", { class: "theme-panel", "data-theme": theme }, [
      el("h2", { text: theme === "light" ? "Light" : "Dark" }),
      el("p", { class: "sub", text: "Every component the console and pay page use." }),
      section("Colour", [swatches()]),
      section("Type", [el("div", {}, [el("div", { class: "stat" }, [el("div", { class: "n", text: "Balance 1,234.50" }), el("div", { class: "l", text: "Small label" })]), el("p", { text: "Body text at 14px. Plain words, short sentences." }), el("p", { class: "muted", text: "Muted text for hints and metadata." }), el("p", { class: "mono", text: "pay_8f3k2 monospace for ids" })])]),
      section("Buttons", [buttons()]),
      section("Inputs and validation", [inputs()]),
      section("Tabs", [tabs()]),
      section("Tables and paging", [el("div", { class: "card" }, [tables()])]),
      section("Status pills", [badges()]),
      section("Money", [money()]),
      section("Copyable ids and secrets", [copyable()]),
      section("QR block", [qr()]),
      section("Empty, loading and error", [states()]),
      section("Banners and toasts", [banners()]),
      section("Confirm dialog", [dialog()]),
      section("2FA and recovery codes", [recovery()])
    ]);
    return p;
  }

  root.appendChild(panel("light"));
  root.appendChild(panel("dark"));
  var force = new URLSearchParams(location.search).get("theme");
  if (force === "light" || force === "dark") {
    root.querySelectorAll(".theme-panel").forEach(function (p) {
      if (p.getAttribute("data-theme") !== force) p.remove();
    });
  }
})();
