"use strict";
function withdrawalPill(w) {
  var label = w.status + (w.purpose === "verify" ? " (test)" : "");
  return el("span", { class: "pill " + w.status, text: label });
}

function payoutErr(box) { return function (e) { box.replaceChildren(el("div", { class: "err", role: "alert", text: e.message })); }; }

function payouts(pid) {
  projectShell(pid, "payouts", function (p, body) {
    var alertBox = el("div"); var out = el("div", {}, [el("div", { class: "skeleton" }), el("div", { class: "skeleton" })]);
    body.appendChild(alertBox); body.appendChild(out);
    var fail = payoutErr(alertBox);
    function load() { api("GET", "/projects/" + pid + "/payouts").then(draw).catch(fail); }
    function act(method, path, payload, done) {
      alertBox.replaceChildren();
      return api(method, path, payload).then(function (r) { if (done) toast(done); load(); return r; }).catch(fail);
    }
    function draw(r) {
      var lim = r.limits; var kids = [];
      var left = Math.max(lim.daily_max - lim.used_24h, 0);
      kids.push(el("div", { class: "grid" }, [
        el("div", { class: "stat" }, [el("div", { class: "n" }, [moneyEl(left, "BTC")]), el("div", { class: "l", text: "left to withdraw in the next 24 hours" })]),
        el("div", { class: "stat" }, [el("div", { class: "n" }, [moneyEl(lim.min_amount, "BTC")]), el("div", { class: "l", text: "minimum withdrawal" })]),
        el("div", { class: "stat" }, [el("div", { class: "n" }, [moneyEl(lim.auto_threshold, "BTC")]), el("div", { class: "l", text: p.trusted ? "sent without review up to this amount" : "every withdrawal is reviewed by an operator first" })])
      ]));
      if (!r.node_ready) {
        kids.push(el("div", { class: "banner warn", role: "status" }, [el("div", {}, [el("b", { text: "Payouts are not switched on for this gateway." }), "Requests wait in the queue until the operator connects a payout node."])]));
      }

      kids.push(el("h3", { text: "Destinations" }));
      kids.push(el("p", { class: "muted", text: "Verify a new destination with a small test payout, then wait " + lim.destination_delay_hours + " hours before you can withdraw to it." }));
      if (!r.destinations.length) {
        kids.push(el("div", { class: "empty" }, [el("b", { text: "No destinations yet." }), "Add an on-chain address, Lightning address or LNURL-pay below, or pay a bolt11 invoice per withdrawal."]));
      } else {
        kids.push(el("table", {}, [el("tr", {}, ["Label", "Destination", "Type", "Status", ""].map(function (h) { return el("th", { text: h }); }))].concat(r.destinations.map(function (d) {
          var status = d.usable ? el("span", { class: "pill ok", text: "ready" }) : d.verified ? el("span", { class: "pill pending", text: "waiting" }) : el("span", { class: "pill pending", text: "unverified" });
          var note = d.verified && !d.usable ? el("div", { class: "muted", text: "Usable from " + new Date(d.usable_from).toLocaleString() }) : null;
          var actions = [];
          if (!d.verified) {
            actions.push(el("button", { text: "Send test payout", onclick: function () { act("POST", "/projects/" + pid + "/payout-destinations/" + d.id + "/verify", {}, "Test payout queued"); } }));
          }
          actions.push(el("button", { text: "Remove", onclick: function () {
            confirmDialog({ title: "Remove this destination?", body: "You can no longer withdraw to " + d.label + ". Withdrawals already sent are not affected.", keep: "Keep destination", confirm: "Remove destination", danger: true }).then(function (v) {
              if (v) act("DELETE", "/projects/" + pid + "/payout-destinations/" + d.id, undefined, "Destination removed");
            });
          } }));
          var cells = [el("td", { text: d.label }), el("td", { class: "mono", text: d.masked }), el("td", { class: "muted", text: d.kind }), el("td", {}, [status, note]), el("td", {}, [el("div", { class: "row" }, actions)])];
          var rows = [el("tr", {}, cells)];
          if (d.challenge) {
            var sig = el("input", { placeholder: "Signed message (base64)", "aria-label": "Signed message" });
            rows.push(el("tr", {}, [el("td", { colspan: "5" }, [el("div", { class: "muted", text: "Or prove you own a legacy address by signing this message:" }), el("div", { class: "mono-wrap", text: d.challenge }),
              el("div", { class: "row" }, [sig, el("button", { text: "Verify signature", onclick: function () { act("POST", "/projects/" + pid + "/payout-destinations/" + d.id + "/verify", { signature: sig.value }, "Destination verified"); } })])])]));
          }
          return rows;
        }).reduce(function (a, b) { return a.concat(b); }, []))));
      }
      var kind = el("select", { "aria-label": "Type" }, [["onchain", "On-chain address"], ["lightning_address", "Lightning address"], ["lnurl", "LNURL-pay"]].map(function (k) { return el("option", { value: k[0], text: k[1] }); }));
      var label = el("input", { placeholder: "Label, for example Cold wallet", "aria-label": "Label" });
      var value = el("input", { placeholder: "Address", "aria-label": "Address" });
      kids.push(el("div", { class: "row" }, [kind, label, value, el("button", { text: "Add destination", onclick: function () {
        act("POST", "/projects/" + pid + "/payout-destinations", { kind: kind.value, label: label.value, value: value.value }, "Destination added");
      } })]));

      kids.push(el("h3", { text: "Request a withdrawal" }));
      var usable = r.destinations.filter(function (d) { return d.usable; });
      var dest = el("select", { "aria-label": "Pay to" }, [el("option", { value: "", text: "A bolt11 invoice" })].concat(usable.map(function (d) { return el("option", { value: d.id, text: d.label + " (" + d.masked + ")" }); })));
      var sats = el("input", { type: "number", min: "1", placeholder: "Amount in sats", "aria-label": "Amount in sats" });
      var inv = el("input", { placeholder: "bolt11 invoice for exactly this amount", "aria-label": "bolt11 invoice" });
      var send = el("button", { class: "primary", text: "Request withdrawal" });
      send.addEventListener("click", function () {
        var payload = { amount: Math.round(Number(sats.value) * 1000) };
        if (dest.value) { payload.destination_id = Number(dest.value); } else { payload.invoice = inv.value; }
        busy(send, function () { return act("POST", "/projects/" + pid + "/withdrawals", payload, "Withdrawal requested"); });
      });
      kids.push(el("div", { class: "row" }, [dest, sats, inv, send]));
      kids.push(el("p", { class: "muted", text: "A network fee reserve is held with the amount and the unused part is returned when the payment completes." }));

      kids.push(el("h3", { text: "History" }));
      if (!r.withdrawals.length) {
        kids.push(el("div", { class: "empty" }, [el("b", { text: "No withdrawals yet." }), "Request one above once you have a balance."]));
      } else {
        kids.push(el("table", {}, [el("tr", {}, ["When", "Amount", "To", "Status", ""].map(function (h) { return el("th", { text: h }); }))].concat(r.withdrawals.map(function (w) {
          return el("tr", {}, [el("td", { class: "muted", text: ago(w.created), title: w.created }), el("td", {}, [moneyEl(w.amount, w.currency, w.fee)]),
            el("td", { class: "mono", text: w.destination_label }), el("td", {}, [withdrawalPill(w), w.error ? el("div", { class: "muted", text: w.error }) : null]),
            el("td", {}, [w.status === "requested" ? el("button", { text: "Cancel withdrawal", onclick: function () {
              confirmDialog({ title: "Cancel this withdrawal?", body: "The amount returns to your balance. Nothing has been sent.", keep: "Keep withdrawal", confirm: "Cancel withdrawal", danger: true }).then(function (v) {
                if (v) act("POST", "/projects/" + pid + "/withdrawals/" + w.id + "/cancel", {}, "Withdrawal cancelled");
              });
            } }) : null])]);
        }))));
      }
      out.replaceChildren.apply(out, kids);
    }
    load();
  });
}

function payoutQueue() {
  var alertBox = el("div"); var box = el("div", {}, [el("div", { class: "skeleton" }), el("div", { class: "skeleton" })]);
  var status = el("select", { "aria-label": "Status" }, ["requested", "approved", "sending", "completed", "failed", "rejected", "cancelled"].map(function (s) { return el("option", { value: s, text: s }); }));
  var fail = payoutErr(alertBox);
  function decide(path, w, project, opts) {
    confirmDialog(opts).then(function (v) {
      if (!v) return;
      alertBox.replaceChildren();
      api("POST", path, opts.input ? { reason: v.text } : {}).then(function () { toast(opts.done); load(); }).catch(fail);
    });
  }
  function load() {
    api("GET", "/admin/withdrawals?status=" + status.value).then(function (rows) {
      if (!rows.length) { box.replaceChildren(el("div", { class: "empty" }, [el("b", { text: "Nothing " + status.value + "." }), "New withdrawals that need review appear here."])); return; }
      box.replaceChildren.apply(box, rows.map(function (q) {
        var w = q.withdrawal;
        var bal = q.balances.map(function (b) { return el("span", {}, [b.kind + " ", moneyEl(b.balance, b.currency), " "]); });
        var acts = [];
        if (w.status === "requested") {
          var amount = fmtAmount(w.amount, w.currency);
          acts.push(el("button", { class: "primary", text: "Approve withdrawal", onclick: function () {
            decide("/admin/withdrawals/" + w.id + "/approve", w, q.project, { title: "Approve this withdrawal?", body: "The worker sends " + amount + " from " + q.project + " to " + w.destination_label + " as soon as it runs. This cannot be undone.", keep: "Keep in queue", confirm: "Approve withdrawal", done: "Withdrawal approved" });
          } }));
          acts.push(el("button", { class: "danger", text: "Reject withdrawal", onclick: function () {
            decide("/admin/withdrawals/" + w.id + "/reject", w, q.project, { title: "Reject this withdrawal?", body: "The reserved " + amount + " returns to " + q.project + " and the project is told it failed.", keep: "Keep in queue", confirm: "Reject withdrawal", danger: true, input: "Reason shown to the project", done: "Withdrawal rejected" });
          } }));
        }
        return el("div", { class: "card" }, [
          el("div", { class: "row" }, [el("b", { text: "#" + w.id + " " + q.project }), el("span", { class: "pill " + (q.trusted ? "ok" : "pending"), text: q.trusted ? "trusted" : "test limits" }), withdrawalPill(w), moneyEl(w.amount, w.currency), el("span", { class: "mono", text: w.destination_kind + " " + w.destination_label })]),
          el("div", { class: "muted" }, [el("span", { text: "Balances: " })].concat(bal.length ? bal : ["none"])),
          el("div", { class: "muted", text: "Completed " + q.completed_count + ", failed or rejected " + q.failed_count + ". Requested " + ago(w.created) + "." }),
          el("div", { class: "muted", text: "Recent: " + (q.recent.map(function (r) { return "#" + r.id + " " + r.status + " " + fmtAmount(r.amount, r.currency); }).join("; ") || "none") }),
          el("div", { class: "row" }, acts)
        ]);
      }));
    }).catch(fail);
  }
  status.addEventListener("change", load);
  layout("payouts", null, el("div", {}, [el("h2", { text: "Payout queue" }), el("p", { class: "sub", text: "Approve or reject withdrawals that need review." }), el("div", { class: "row" }, [status]), alertBox, box]));
  load();
}
