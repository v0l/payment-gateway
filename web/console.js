(function () {
  "use strict";
  var root = document.getElementById("root");
  var me = null;
  var csrf = "";
  var timer = null;

  function el(tag, attrs, kids) {
    var n = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      var v = attrs[k];
      if (v === null || v === undefined || v === false) return;
      if (k === "text") n.textContent = v;
      else if (k === "class") n.className = v;
      else if (k === "value") n.value = v;
      else if (k.indexOf("on") === 0) n.addEventListener(k.slice(2), v);
      else n.setAttribute(k, v === true ? "" : v);
    });
    (kids || []).forEach(function (c) {
      if (c === null || c === undefined || c === false) return;
      n.appendChild(typeof c === "string" || typeof c === "number" ? document.createTextNode(String(c)) : c);
    });
    return n;
  }

  function api(method, path, body) {
    var opts = { method: method, headers: {}, credentials: "same-origin" };
    if (csrf) opts.headers["x-csrf-token"] = csrf;
    if (body !== undefined) { opts.headers["content-type"] = "application/json"; opts.body = JSON.stringify(body); }
    return fetch("/console/api" + path, opts).then(function (r) {
      return r.json().catch(function () { return {}; }).then(function (b) {
        if (r.status === 401 && path !== "/login" && path !== "/signup") { me = null; csrf = ""; route(); }
        if (!r.ok) { var e = new Error(b.error || "Request failed"); e.status = r.status; throw e; }
        return b;
      });
    });
  }

  function fmtAmount(amount, currency) {
    if (currency === "BTC") return Math.ceil(amount / 1000).toLocaleString() + " sats";
    var scale = 100, code = currency;
    if (currency.length === 4 && currency[0] === "M") { scale = 100000; code = currency.slice(1); }
    try { return new Intl.NumberFormat(undefined, { style: "currency", currency: code }).format(amount / scale); }
    catch (e) { return (amount / scale).toFixed(2) + " " + currency; }
  }
  function ago(iso) {
    if (!iso) return "";
    var s = Math.round((Date.now() - new Date(iso).getTime()) / 1000);
    if (s < 60) return s + "s ago"; if (s < 3600) return Math.round(s / 60) + "m ago";
    if (s < 86400) return Math.round(s / 3600) + "h ago"; return Math.round(s / 86400) + "d ago";
  }
  function pill(text, cls) { return el("span", { class: "pill " + (cls || text), text: text }); }
  function msg(box, text, bad) { box.replaceChildren(el("div", { class: bad ? "err" : "ok-msg", text: text })); }
  function secretBox(title, value) {
    return el("div", { class: "secret" }, [el("b", { text: title + " (shown once, copy it now)" }), el("span", { class: "mono", text: value })]);
  }
  function field(label, input) { return el("div", { class: "field" }, [el("label", { text: label }), input]); }
  function go(hash) { location.hash = hash; }
  function stopTimer() { if (timer) { clearInterval(timer); timer = null; } }
  function busy(btn, fn) {
    btn.disabled = true;
    return Promise.resolve().then(fn).then(function (v) { btn.disabled = false; return v; }, function (e) { btn.disabled = false; throw e; });
  }

  function authPage(title, sub, fields, label, onsubmit) {
    var err = el("div", { class: "err" });
    var btn = el("button", { class: "primary", type: "submit", text: label });
    var form = el("form", { class: "card" }, fields.concat([btn, err]));
    form.addEventListener("submit", function (ev) {
      ev.preventDefault(); err.textContent = "";
      busy(btn, function () { return onsubmit(); }).catch(function (e) { err.textContent = e.message; });
    });
    root.replaceChildren(el("div", { class: "auth" }, [el("h1", { text: title }), el("p", { class: "muted", text: sub }), form]));
  }

  function loginPage() {
    var email = el("input", { type: "email", autocomplete: "username", required: true });
    var pw = el("input", { type: "password", autocomplete: "current-password", required: true });
    authPage("Payment gateway", "Sign in to your console. Accounts are by invite.", [field("Email", email), field("Password", pw)], "Sign in", function () {
      return api("POST", "/login", { email: email.value, password: pw.value }).then(function (m) { me = m; csrf = m.csrf; go("#/"); route(); });
    });
  }

  function invitePage(token) {
    api("GET", "/invite/" + encodeURIComponent(token)).then(function (info) {
      var pw = el("input", { type: "password", autocomplete: "new-password", required: true, minlength: 10 });
      var sub = "Create a password for " + info.email + (info.project ? " to join " + info.project : "") + ".";
      authPage("Accept invite", sub, [field("Password (10+ characters)", pw)], "Create account", function () {
        return api("POST", "/signup", { token: token, password: pw.value }).then(function (m) { me = m; csrf = m.csrf; location.replace("/console#/"); });
      });
    }).catch(function () {
      root.replaceChildren(el("div", { class: "auth" }, [el("h1", { text: "Invite not valid" }), el("p", { class: "muted", text: "It may have expired or already been used. Ask for a new one." })]));
    });
  }

  function layout(active, projectId, content) {
    var navs = [el("h1", { text: "Payment gateway" })];
    var link = function (text, hash, key) { return el("a", { href: hash, class: key === active ? "on" : "", text: text }); };
    me.projects.forEach(function (p) {
      var open = String(p.id) === String(projectId);
      navs.push(el("div", { class: "sep", text: p.name }));
      if (open) {
        [["Overview", "overview"], ["Payments", "payments"], ["Providers", "providers"], ["Webhooks", "webhooks"], ["Team", "team"], ["Settings", "settings"]].forEach(function (x) {
          navs.push(link(x[0], "#/p/" + p.id + "/" + x[1], x[1]));
        });
      } else { navs.push(link("Open", "#/p/" + p.id + "/overview", "")); }
    });
    navs.push(link("+ New project", "#/new", "new"));
    if (me.is_operator) { navs.push(el("div", { class: "sep", text: "Operator" })); navs.push(link("Accounts", "#/admin", "admin")); }
    navs.push(el("div", { class: "foot muted" }, [me.email, el("br"), el("a", { text: "Account", href: "#/account" }), " ", el("a", { text: "Sign out", onclick: function () {
      api("POST", "/logout", {}).then(function () { me = null; csrf = ""; route(); });
    } })]));
    root.replaceChildren(el("div", { class: "shell" }, [el("nav", {}, navs), el("main", {}, [content])]));
  }

  function newProject() {
    var name = el("input", { placeholder: "My shop" });
    var out = el("div");
    var btn = el("button", { class: "primary", text: "Create project" });
    btn.addEventListener("click", function () {
      busy(btn, function () { return api("POST", "/projects", { name: name.value }); }).then(function (r) {
        out.replaceChildren(secretBox("API key", r.api_key), secretBox("Webhook signing secret", r.webhook_secret),
          el("p", {}, [el("a", { text: "Go to project", href: "#/p/" + r.id + "/overview" })]));
        return api("GET", "/me").then(function (m) { me = m; });
      }).catch(function (e) { msg(out, e.message, true); });
    });
    layout("new", null, el("div", {}, [el("h2", { text: "New project" }), el("p", { class: "sub", text: "A project has its own API key, providers and webhook." }),
      el("div", { class: "card" }, [field("Name", name), btn, out])]));
  }

  function tabs(pid, active) {
    return el("div", { class: "tabs" }, [["overview", "Overview"], ["payments", "Payments"], ["providers", "Providers"], ["webhooks", "Webhooks"], ["team", "Team"], ["settings", "Settings"]].map(function (t) {
      return el("a", { href: "#/p/" + pid + "/" + t[0], class: t[0] === active ? "on" : "", text: t[1] });
    }));
  }

  function projectShell(pid, tab, build) {
    api("GET", "/projects/" + pid).then(function (p) {
      var body = el("div", {}, [el("h2", { text: p.display_name || p.name }), el("p", { class: "sub" }, [p.role + (p.trusted ? "" : " ") , p.trusted ? "" : pill("test limits", "pending")]), tabs(pid, tab)]);
      layout(tab, pid, body);
      build(p, body);
    }).catch(function (e) { layout("", pid, el("div", { class: "err", text: e.message })); });
  }

  function overview(pid) {
    projectShell(pid, "overview", function (p, body) {
      api("GET", "/projects/" + pid + "/stats?days=30").then(function (s) {
        var count = function (st) { return s.by_status.filter(function (r) { return r.status === st; }).reduce(function (a, r) { return a + r.count; }, 0); };
        var revenue = {};
        s.paid_by_day.forEach(function (d) { revenue[d.currency] = (revenue[d.currency] || 0) + d.total; });
        var revCards = Object.keys(revenue).map(function (c) { return el("div", { class: "stat" }, [el("div", { class: "n", text: fmtAmount(revenue[c], c) }), el("div", { class: "l", text: "paid, 30 days" })]); });
        body.appendChild(el("div", { class: "grid" }, revCards.concat([
          el("div", { class: "stat" }, [el("div", { class: "n", text: count("paid") }), el("div", { class: "l", text: "paid payments" })]),
          el("div", { class: "stat" }, [el("div", { class: "n", text: count("pending") }), el("div", { class: "l", text: "pending" })]),
          el("div", { class: "stat" }, [el("div", { class: "n", text: count("expired") + count("cancelled") }), el("div", { class: "l", text: "expired or cancelled" })]),
          el("div", { class: "stat" }, [el("div", { class: "n", text: s.pending_deliveries }), el("div", { class: "l", text: "webhooks waiting" })])
        ])));
        var max = Math.max.apply(null, [1].concat(s.paid_by_day.map(function (d) { return d.count; })));
        body.appendChild(el("h3", { text: "Paid per day" }));
        body.appendChild(el("div", { class: "bars" }, s.paid_by_day.map(function (d) {
          var b = el("div", { title: d.day + ": " + d.count + " (" + d.currency + ")" }); b.style.height = Math.max(4, Math.round(d.count / max * 100)) + "%"; return b;
        })));
        body.appendChild(el("h3", { text: "By method" }));
        body.appendChild(s.by_method.length ? el("table", {}, s.by_method.map(function (m) { return el("tr", {}, [el("td", { text: m.method }), el("td", { text: m.count })]); })) : el("p", { class: "muted", text: "No paid attempts yet." }));
        if (!p.trusted) body.appendChild(el("p", { class: "muted", text: "This project is in test mode. Ask an operator to lift the limits to take live payments." }));
      });
    });
  }

  var STATUSES = ["", "pending", "paid", "expired", "cancelled", "refunded", "partially_refunded"];

  function payments(pid) {
    projectShell(pid, "payments", function (p, body) {
      var q = el("input", { placeholder: "Search reference or id" });
      var st = el("select", {}, STATUSES.map(function (s) { return el("option", { value: s, text: s || "any status" }); }));
      var list = el("div"); var more = el("div");
      var rows = []; var next = null;
      function load(reset) {
        if (reset) { rows = []; next = null; }
        var qs = "?status=" + encodeURIComponent(st.value) + "&q=" + encodeURIComponent(q.value) + (next ? "&before=" + encodeURIComponent(next) : "");
        api("GET", "/projects/" + pid + "/payments" + qs).then(function (r) {
          rows = rows.concat(r.payments); next = r.next; draw();
        }).catch(function (e) { msg(list, e.message, true); });
      }
      function draw() {
        list.replaceChildren(rows.length ? el("table", {}, [el("tr", {}, ["Id", "Reference", "Amount", "Status", "Method", "Created"].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (r) {
          var paid = r.attempts.filter(function (a) { return a.status === "paid"; })[0];
          var tr = el("tr", { class: "click", onclick: function () { go("#/p/" + pid + "/payments/" + r.id); } }, [
            el("td", { class: "mono", text: r.id.slice(0, 8) }), el("td", { text: r.reference }), el("td", { text: fmtAmount(r.amount, r.currency) }),
            el("td", {}, [pill(r.status)]), el("td", { text: paid ? paid.method : "" }), el("td", { class: "muted", text: ago(r.created) })]);
          return tr;
        }))) : el("p", { class: "muted", text: "No payments match." }));
        more.replaceChildren(next ? el("button", { text: "Load more", onclick: function () { load(false); } }) : null);
      }
      q.addEventListener("keydown", function (e) { if (e.key === "Enter") load(true); });
      st.addEventListener("change", function () { load(true); });
      body.appendChild(el("div", { class: "row" }, [q, st, el("button", { text: "Search", onclick: function () { load(true); } }), el("button", { class: "primary", text: "New payment", onclick: function () { go("#/p/" + pid + "/payments/new"); } })]));
      body.appendChild(list); body.appendChild(more);
      load(true);
    });
  }

  function newPayment(pid) {
    projectShell(pid, "payments", function (p, body) {
      var ref = el("input", { value: "order-" + Math.floor(Date.now() / 1000) });
      var amount = el("input", { type: "number", value: "1500", min: "1" });
      var cur = el("select", {}, ["EUR", "USD", "GBP", "BTC"].map(function (c) { return el("option", { value: c, text: c }); }));
      var desc = el("input", { placeholder: "Optional description" });
      var out = el("div"); var btn = el("button", { class: "primary", text: "Create" });
      btn.addEventListener("click", function () {
        busy(btn, function () { return api("POST", "/projects/" + pid + "/payments", { reference: ref.value, amount: Number(amount.value), currency: cur.value, description: desc.value || null }); })
          .then(function (r) { go("#/p/" + pid + "/payments/" + r.id); }).catch(function (e) { msg(out, e.message, true); });
      });
      body.appendChild(el("div", { class: "card" }, [field("Reference", ref), field("Amount (minor units: cents, or msats for BTC)", amount), field("Currency", cur), field("Description", desc), btn, out]));
    });
  }

  function paymentDetail(pid, id) {
    projectShell(pid, "payments", function (p, body) {
      var box = el("div"); body.appendChild(box);
      function load() {
        api("GET", "/projects/" + pid + "/payments/" + id).then(function (d) {
          var pay = d.payment;
          var actions = el("div", { class: "row" });
          var out = el("div");
          if (pay.status === "pending") {
            actions.appendChild(el("button", { class: "danger", text: "Cancel payment", onclick: function () {
              api("POST", "/projects/" + pid + "/payments/" + id + "/cancel", {}).then(load).catch(function (e) { msg(out, e.message, true); });
            } }));
          }
          if (pay.status === "paid" || pay.status === "partially_refunded") {
            var amt = el("input", { type: "number", placeholder: "Amount (blank = all remaining)" });
            var dest = el("input", { placeholder: "Lightning invoice or address (for BTC payments)" });
            actions.appendChild(amt); actions.appendChild(dest);
            actions.appendChild(el("button", { class: "danger", text: "Refund", onclick: function () {
              if (!confirm("Refund this payment?")) return;
              api("POST", "/projects/" + pid + "/payments/" + id + "/refund", { amount: amt.value ? Number(amt.value) : null, destination: dest.value || null })
                .then(function (r) { msg(out, "Refund " + r.status + (r.error ? ": " + r.error : ""), r.status === "failed"); load(); }).catch(function (e) { msg(out, e.message, true); });
            } }));
          }
          box.replaceChildren(
            el("div", { class: "card" }, [el("dl", { class: "kv" }, [
              el("dt", { text: "Id" }), el("dd", { class: "mono", text: pay.id }),
              el("dt", { text: "Reference" }), el("dd", { text: pay.reference }),
              el("dt", { text: "Amount" }), el("dd", { text: fmtAmount(pay.amount, pay.currency) }),
              el("dt", { text: "Status" }), el("dd", {}, [pill(pay.status)]),
              el("dt", { text: "Created" }), el("dd", { text: new Date(pay.created).toLocaleString() }),
              el("dt", { text: "Expires" }), el("dd", { text: new Date(pay.expires).toLocaleString() }),
              el("dt", { text: "Paid" }), el("dd", { text: pay.paid_at ? new Date(pay.paid_at).toLocaleString() : "" }),
              el("dt", { text: "Customer" }), el("dd", { text: pay.customer_ref || "" }),
              el("dt", { text: "Pay page" }), el("dd", {}, [el("a", { href: pay.pay_url, target: "_blank", rel: "noopener", text: pay.pay_url })]),
              el("dt", { text: "Metadata" }), el("dd", { class: "mono", text: pay.metadata ? JSON.stringify(pay.metadata) : "" })
            ]), actions, out]),
            el("h3", { text: "Attempts" }),
            pay.attempts.length ? el("table", {}, [el("tr", {}, ["Method", "Status", "Charge", "Fee", "Details"].map(function (h) { return el("th", { text: h }); }))].concat(pay.attempts.map(function (a) {
              var det = a.details || {};
              return el("tr", {}, [el("td", { text: a.method + (a.option ? " " + a.option : "") }), el("td", {}, [pill(a.status)]), el("td", { text: fmtAmount(a.amount, a.currency) }),
                el("td", { text: a.processing_fee ? fmtAmount(a.processing_fee, pay.currency) : "" }),
                el("td", { class: "mono", text: det.bolt11 ? det.bolt11.slice(0, 36) + "..." : det.address || det.checkout_url || det.deposit_address || "" })]);
            }))) : el("p", { class: "muted", text: "No attempts yet." }),
            el("h3", { text: "Receipts" }),
            d.receipts.length ? el("table", {}, d.receipts.map(function (r) { return el("tr", {}, [el("td", { class: "mono", text: r.ref }), el("td", { text: r.amount }), el("td", {}, [pill(r.confirmed ? "confirmed" : "unconfirmed", r.confirmed ? "ok" : "pending")])]); })) : el("p", { class: "muted", text: "None." }),
            el("h3", { text: "Refunds" }),
            pay.refunds.length ? el("table", {}, pay.refunds.map(function (r) { return el("tr", {}, [el("td", { text: fmtAmount(r.amount, pay.currency) }), el("td", {}, [pill(r.status)]), el("td", { class: "muted", text: r.error || "" })]); })) : el("p", { class: "muted", text: "None." }),
            el("h3", { text: "Webhooks sent" }),
            d.deliveries.length ? el("table", {}, d.deliveries.map(function (x) {
              return el("tr", {}, [el("td", { text: x.event_type }), el("td", {}, [pill(x.delivered_at ? "delivered" : x.abandoned_at ? "failed" : "waiting", x.delivered_at ? "ok" : x.abandoned_at ? "bad" : "pending")]), el("td", { text: x.attempts + " tries" }), el("td", { class: "muted", text: x.last_error || "" }),
                el("td", {}, [el("a", { text: "replay", onclick: function () { api("POST", "/projects/" + pid + "/deliveries/" + x.id + "/replay", {}).then(load); } })])]);
            })) : el("p", { class: "muted", text: "None." })
          );
        }).catch(function (e) { box.replaceChildren(el("div", { class: "err", text: e.message })); });
      }
      load();
      stopTimer(); timer = setInterval(load, 5000);
    });
  }

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
              out.replaceChildren(el("div", { class: "ok-msg", text: "Saved." }), hook ? el("p", { class: "muted" }, ["Webhook URL for this provider: ", el("span", { class: "mono", text: hook })]) : null);
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

  function webhooks(pid) {
    projectShell(pid, "webhooks", function (p, body) {
      var only = el("input", { type: "checkbox" });
      var list = el("div");
      function load() {
        api("GET", "/projects/" + pid + "/deliveries" + (only.checked ? "?undelivered=1" : "")).then(function (rows) {
          list.replaceChildren(rows.length ? el("table", {}, [el("tr", {}, ["Time", "Event", "State", "Tries", "Error", ""].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (d) {
            return el("tr", {}, [el("td", { class: "muted", text: ago(d.created) }), el("td", { text: d.event_type }),
              el("td", {}, [pill(d.delivered_at ? "delivered" : d.abandoned_at ? "failed" : "waiting", d.delivered_at ? "ok" : d.abandoned_at ? "bad" : "pending")]),
              el("td", { text: d.attempts }), el("td", { class: "muted", text: d.last_error || "" }),
              el("td", {}, [el("details", {}, [el("summary", { text: "body" }), el("pre", { text: JSON.stringify(JSON.parse(d.body), null, 2) })]), el("a", { text: "replay", onclick: function () { api("POST", "/projects/" + pid + "/deliveries/" + d.id + "/replay", {}).then(load); } })])]);
          }))) : el("p", { class: "muted", text: "No webhooks yet." }));
        });
      }
      only.addEventListener("change", load);
      body.appendChild(el("div", { class: "row" }, [el("label", {}, [only, " only undelivered"])]));
      body.appendChild(list); load();
      p.webhook_url ? body.appendChild(el("p", { class: "muted" }, ["Sending to ", el("span", { class: "mono", text: p.webhook_url })])) : body.appendChild(el("p", { class: "err", text: "No webhook url set. Add one under Settings." }));
    });
  }

  function team(pid) {
    projectShell(pid, "team", function (p, body) {
      var list = el("div"); var out = el("div");
      function load() {
        api("GET", "/projects/" + pid + "/members").then(function (rows) {
          list.replaceChildren(el("table", {}, rows.map(function (m) {
            return el("tr", {}, [el("td", { text: m.email }), el("td", {}, [pill(m.role, "ok")]), el("td", {}, [m.account_id === me.id ? null : el("a", { text: "remove", onclick: function () {
              if (!confirm("Remove " + m.email + "?")) return;
              api("DELETE", "/projects/" + pid + "/members/" + m.account_id).then(load).catch(function (e) { msg(out, e.message, true); });
            } })])]);
          })));
        });
      }
      body.appendChild(list);
      if (p.role !== "viewer") {
        var email = el("input", { type: "email", placeholder: "teammate@example.com" });
        var role = el("select", {}, ["viewer", "admin", "owner"].map(function (r) { return el("option", { value: r, text: r }); }));
        body.appendChild(el("h3", { text: "Invite a teammate" }));
        body.appendChild(el("div", { class: "row" }, [email, role, el("button", { class: "primary", text: "Create invite link", onclick: function () {
          api("POST", "/projects/" + pid + "/invites", { email: email.value, role: role.value }).then(function (r) {
            out.replaceChildren(secretBox("Invite link for " + r.email + ", valid " + r.expires_in_days + " days", r.invite_url));
          }).catch(function (e) { msg(out, e.message, true); });
        } })]));
        body.appendChild(out);
      }
      load();
    });
  }

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

  function account() {
    var cur = el("input", { type: "password", autocomplete: "current-password" });
    var nw = el("input", { type: "password", autocomplete: "new-password" });
    var out = el("div"); var btn = el("button", { class: "primary", text: "Change password" });
    btn.addEventListener("click", function () {
      busy(btn, function () { return api("POST", "/password", { current: cur.value, new: nw.value }); }).then(function () { cur.value = ""; nw.value = ""; msg(out, "Password changed. Other sessions were signed out."); }).catch(function (e) { msg(out, e.message, true); });
    });
    layout("account", null, el("div", {}, [el("h2", { text: "Account" }), el("p", { class: "sub", text: me.email }), el("div", { class: "card" }, [field("Current password", cur), field("New password (10+ characters)", nw), btn, out])]));
  }

  function admin() {
    var box = el("div"); var out = el("div");
    function load() {
      api("GET", "/admin/accounts").then(function (rows) {
        box.replaceChildren(el("table", {}, [el("tr", {}, ["Email", "Role", "Last login", "Status", ""].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (a) {
          return el("tr", {}, [el("td", { text: a.email }), el("td", { text: a.is_operator ? "operator" : "customer" }), el("td", { class: "muted", text: ago(a.last_login) }),
            el("td", {}, [pill(a.disabled ? "disabled" : "active", a.disabled ? "bad" : "ok")]),
            el("td", {}, [a.id === me.id ? null : el("a", { text: a.disabled ? "enable" : "disable", onclick: function () { api("POST", "/admin/accounts/" + a.id + "/disabled", { enabled: a.disabled }).then(load); } })])]);
        }))));
      });
    }
    var email = el("input", { type: "email", placeholder: "customer@example.com" });
    var op = el("input", { type: "checkbox" });
    layout("admin", null, el("div", {}, [el("h2", { text: "Accounts" }), el("p", { class: "sub", text: "Invite people, disable accounts, lift test limits per project from its settings." }),
      el("div", { class: "row" }, [email, el("label", {}, [op, " operator"]), el("button", { class: "primary", text: "Create invite", onclick: function () {
        api("POST", "/admin/invites", { email: email.value, operator: op.checked }).then(function (r) { out.replaceChildren(secretBox("Invite link for " + r.email, r.invite_url)); load(); }).catch(function (e) { msg(out, e.message, true); });
      } })]), out, box]));
    load();
  }

  function route() {
    stopTimer();
    var h = location.hash.replace(/^#/, "") || "/";
    var m = h.match(/^\/invite\/(.+)$/) || location.pathname.match(/^\/console\/invite\/(.+)$/);
    if (m && !me) { return invitePage(m[1]); }
    if (!me) { return loginPage(); }
    var parts = h.split("/").filter(Boolean);
    if (parts.length === 0) { return me.projects.length ? go("#/p/" + me.projects[0].id + "/overview") : newProject(); }
    if (parts[0] === "new") return newProject();
    if (parts[0] === "account") return account();
    if (parts[0] === "admin" && me.is_operator) return admin();
    if (parts[0] === "p" && parts[1]) {
      var pid = parts[1], tab = parts[2] || "overview";
      if (tab === "overview") return overview(pid);
      if (tab === "payments") { if (parts[3] === "new") return newPayment(pid); if (parts[3]) return paymentDetail(pid, parts[3]); return payments(pid); }
      if (tab === "providers") { if (parts[3]) return providerForm(pid, parts[3]); return providers(pid); }
      if (tab === "webhooks") return webhooks(pid);
      if (tab === "team") return team(pid);
      if (tab === "settings") return settings(pid);
    }
    go("#/");
  }

  window.addEventListener("hashchange", route);
  api("GET", "/me").then(function (m) { me = m; csrf = m.csrf; route(); }).catch(function () { me = null; route(); });
})();
