"use strict";
function payouts(pid) {
  projectShell(pid, "payouts", function (p, body) {
    var out = el("div"); var box = el("div");
    body.appendChild(out); body.appendChild(box);
    function load() {
      api("GET", "/projects/" + pid + "/payouts").then(function (r) { draw(r); }).catch(function (e) { msg(box, e.message, true); });
    }
    function draw(r) {
      var lim = r.limits;
      var kids = [];
      kids.push(el("div", { class: "grid" }, [
        el("div", { class: "stat" }, [el("div", { class: "n", text: fmtAmount(lim.daily_max - lim.used_24h > 0 ? lim.daily_max - lim.used_24h : 0, "BTC") }), el("div", { class: "l", text: "left to withdraw today" })]),
        el("div", { class: "stat" }, [el("div", { class: "n", text: fmtAmount(lim.min_amount, "BTC") }), el("div", { class: "l", text: "minimum withdrawal" })]),
        el("div", { class: "stat" }, [el("div", { class: "n", text: fmtAmount(lim.auto_threshold, "BTC") }), el("div", { class: "l", text: p.trusted ? "sent without review up to" : "reviewed by an operator first" })])
      ]));
      if (!r.node_ready) { kids.push(el("p", { class: "muted", text: "Payouts are not enabled on this gateway yet. Requests wait until they are." })); }

      kids.push(el("h3", { text: "Destinations" }));
      kids.push(el("p", { class: "muted", text: "A new destination must be verified and waits " + lim.destination_delay_hours + " hours before it can be used." }));
      if (!r.destinations.length) { kids.push(el("p", { class: "muted", text: "No destinations yet." })); }
      r.destinations.forEach(function (d) {
        var actions = [];
        if (!d.verified) {
          actions.push(el("a", { text: "send test payout", onclick: function () {
            api("POST", "/projects/" + pid + "/payout-destinations/" + d.id + "/verify", {}).then(function () { load(); }).catch(function (e) { msg(box, e.message, true); });
          } }));
          if (d.challenge) {
            var sig = el("input", { placeholder: "signed message (base64)" });
            actions.push(sig);
            actions.push(el("a", { text: "verify signature", onclick: function () {
              api("POST", "/projects/" + pid + "/payout-destinations/" + d.id + "/verify", { signature: sig.value }).then(function () { load(); }).catch(function (e) { msg(box, e.message, true); });
            } }));
            actions.push(el("div", { class: "muted mono", text: "Sign: " + d.challenge }));
          }
        }
        actions.push(el("a", { text: "remove", onclick: function () {
          api("DELETE", "/projects/" + pid + "/payout-destinations/" + d.id).then(load).catch(function (e) { msg(box, e.message, true); });
        } }));
        kids.push(el("div", { class: "row" }, [el("b", { text: d.label }), el("span", { class: "mono", text: d.masked }), pill(d.kind, "ok"),
          pill(d.usable ? "ready" : (d.verified ? "waiting until " + new Date(d.usable_from).toLocaleString() : "unverified"), d.usable ? "ok" : "pending")].concat(actions)));
      });
      var kind = el("select", {}, [["onchain", "On-chain address"], ["lightning_address", "Lightning address"], ["lnurl", "LNURL-pay"]].map(function (k) { return el("option", { value: k[0], text: k[1] }); }));
      var label = el("input", { placeholder: "Label" }); var value = el("input", { placeholder: "Address" });
      kids.push(el("div", { class: "row" }, [kind, label, value, el("button", { text: "Add destination", onclick: function () {
        api("POST", "/projects/" + pid + "/payout-destinations", { kind: kind.value, label: label.value, value: value.value }).then(load).catch(function (e) { msg(box, e.message, true); });
      } })]));

      kids.push(el("h3", { text: "Request a withdrawal" }));
      var dest = el("select", {}, [el("option", { value: "", text: "Pay a bolt11 invoice" })].concat(r.destinations.filter(function (d) { return d.usable; }).map(function (d) { return el("option", { value: d.id, text: d.label + " (" + d.masked + ")" }); })));
      var sats = el("input", { type: "number", min: "1", placeholder: "Amount in sats" });
      var inv = el("input", { placeholder: "bolt11 invoice (when no destination is chosen)" });
      kids.push(el("div", { class: "row" }, [dest, sats, inv, el("button", { class: "primary", text: "Withdraw", onclick: function () {
        var body = { amount: Math.round(Number(sats.value) * 1000) };
        if (dest.value) { body.destination_id = Number(dest.value); } else { body.invoice = inv.value; }
        api("POST", "/projects/" + pid + "/withdrawals", body).then(load).catch(function (e) { msg(box, e.message, true); });
      } })]));

      kids.push(el("h3", { text: "History" }));
      if (!r.withdrawals.length) { kids.push(el("p", { class: "muted", text: "No withdrawals yet." })); }
      else {
        var head = ["When", "Amount", "To", "Status", "Fee", ""];
        kids.push(el("table", {}, [el("tr", {}, head.map(function (h) { return el("th", { text: h }); }))].concat(r.withdrawals.map(function (w) {
          var status = w.status + (w.purpose === "verify" ? " (test)" : "");
          return el("tr", {}, [el("td", { class: "muted", text: ago(w.created), title: w.created }), el("td", { text: fmtAmount(w.amount, w.currency) }),
            el("td", { class: "mono", text: w.destination_label }), el("td", {}, [pill(status, w.status === "failed" || w.status === "rejected" ? "bad" : (w.status === "completed" ? "ok" : "pending")), w.error ? el("div", { class: "muted", text: w.error }) : null]),
            el("td", { text: w.fee === null ? "" : fmtAmount(w.fee, w.currency) }),
            el("td", {}, [w.status === "requested" ? el("a", { text: "cancel", onclick: function () {
              api("POST", "/projects/" + pid + "/withdrawals/" + w.id + "/cancel", {}).then(load).catch(function (e) { msg(box, e.message, true); });
            } }) : null])]);
        }))));
      }
      out.replaceChildren.apply(out, kids);
    }
    load();
  });
}

function payoutQueue() {
  var box = el("div"); var out = el("div");
  var status = el("select", {}, ["requested", "approved", "sending", "completed", "failed", "rejected", "cancelled"].map(function (s) { return el("option", { value: s, text: s }); }));
  function load() {
    api("GET", "/admin/withdrawals?status=" + status.value).then(function (rows) {
      if (!rows.length) { box.replaceChildren(el("p", { class: "muted", text: "Nothing here." })); return; }
      box.replaceChildren.apply(box, rows.map(function (q) {
        var w = q.withdrawal;
        var bal = q.balances.map(function (b) { return b.kind + " " + fmtAmount(b.balance, b.currency); }).join(", ");
        var acts = [];
        if (w.status === "requested") {
          acts.push(el("button", { class: "primary", text: "Approve", onclick: function () { api("POST", "/admin/withdrawals/" + w.id + "/approve", {}).then(load).catch(function (e) { msg(out, e.message, true); }); } }));
          acts.push(el("button", { text: "Reject", onclick: function () {
            var reason = window.prompt("Reason for rejecting");
            if (reason === null) return;
            api("POST", "/admin/withdrawals/" + w.id + "/reject", { reason: reason }).then(load).catch(function (e) { msg(out, e.message, true); });
          } }));
        }
        return el("div", { class: "card" }, [
          el("div", { class: "row" }, [el("b", { text: "#" + w.id + " " + q.project }), pill(q.trusted ? "trusted" : "new project", q.trusted ? "ok" : "pending"), pill(w.status, w.status), el("span", { text: fmtAmount(w.amount, w.currency) }), el("span", { class: "mono", text: w.destination_kind + " " + w.destination_label })]),
          el("div", { class: "muted", text: "Balances: " + (bal || "none") + ". Completed " + q.completed_count + ", failed or rejected " + q.failed_count + ". Requested " + ago(w.created) + "." }),
          el("div", { class: "muted", text: "Recent: " + q.recent.map(function (r) { return "#" + r.id + " " + r.status + " " + fmtAmount(r.amount, r.currency); }).join("; ") }),
          el("div", { class: "row" }, acts)
        ]);
      }));
    }).catch(function (e) { msg(box, e.message, true); });
  }
  status.addEventListener("change", load);
  layout("payouts", null, el("div", {}, [el("h2", { text: "Payout queue" }), el("p", { class: "sub", text: "Approve or reject withdrawals that need review." }), el("div", { class: "row" }, [status]), out, box]));
  load();
}
