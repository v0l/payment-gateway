"use strict";
function refundFlow(pid, id, pay, amountText, destination, out, reload) {
  var amount = amountText ? Number(amountText) : null;
  var qs = [];
  if (amount) qs.push("amount=" + amount);
  if (destination) qs.push("destination=" + encodeURIComponent(destination));
  return api("GET", "/projects/" + pid + "/payments/" + id + "/refund-preview" + (qs.length ? "?" + qs.join("&") : "")).then(function (pv) {
    if (pv.destination_required && !destination) throw new Error("A destination is needed for " + pv.method + " refunds.");
    var lines = "Refund " + fmtAmount(pv.amount, pv.currency) + " of " + fmtAmount(pv.refundable, pv.currency) + " refundable (" + (pv.full ? "full" : "partial") + "). " +
      "Sent to the customer: " + fmtAmount(pv.charge, pv.charge_currency) + ". " +
      (pv.fee_kept ? "Processing fee kept: " + fmtAmount(pv.fee_kept, pv.charge_currency) + ". " : "") +
      "Left refundable afterwards: " + fmtAmount(pv.remaining_after, pv.currency) + ".";
    return confirmDialog({ title: "Confirm refund", body: lines, keep: "Do not refund", confirm: "Refund", danger: true }).then(function (r) {
      if (!r) return;
      return api("POST", "/projects/" + pid + "/payments/" + id + "/refund", { amount: amount, destination: destination || null, attempt_id: pv.attempt_id })
        .then(function (res) { msg(out, "Refund " + res.status + (res.error ? ": " + res.error : ""), res.status === "failed"); reload(); });
    });
  }).catch(function (e) { msg(out, e.message, true); });
}

function paymentTools(pid, id, pay, role, out, reload) {
  var box = el("div");
  var canWrite = role !== "viewer";
  var base = "/projects/" + pid + "/payments/" + id;
  var buttons = el("div", { class: "row" });
  buttons.appendChild(el("button", { text: "Resend webhook", disabled: !canWrite, onclick: function () {
    api("POST", base + "/resend-webhook", {}).then(function () { msg(out, "Webhook queued."); reload(); }).catch(function (e) { msg(out, e.message, true); });
  } }));
  if ((pay.status === "pending" || pay.status === "expired") && canWrite) {
    buttons.appendChild(el("button", { text: "Mark as paid", onclick: function () {
      confirmDialog({ title: "Mark as paid", body: "Settles this payment without a provider confirmation. The paid webhook carries a manual flag and the reason below.", input: "Reason", keep: "Cancel", confirm: "Mark paid", danger: true }).then(function (r) {
        if (!r) return;
        api("POST", base + "/mark-paid", { reason: r.text }).then(reload).catch(function (e) { msg(out, e.message, true); });
      });
    } }));
  }
  var notes = el("div", { class: "notes" }); var note = el("textarea", { rows: "2", placeholder: "Internal note for your team, not shown to the customer", "aria-label": "Internal note" });
  function loadNotes() {
    api("GET", base + "/notes").then(function (r) {
      notes.replaceChildren.apply(notes, r.notes.length ? r.notes.map(function (n) { return el("p", {}, [el("span", { class: "muted", text: fmtFull(n.created) + "  " }), n.body]); }) : [el("p", { class: "muted", text: "No notes yet." })]);
    });
  }
  box.appendChild(notes);
  if (canWrite) box.appendChild(el("div", { class: "row" }, [note, el("button", { text: "Add note", onclick: function () {
    api("POST", base + "/notes", { body: note.value }).then(function () { note.value = ""; loadNotes(); }).catch(function (e) { msg(out, e.message, true); });
  } })]));
  box.appendChild(buttons);
  loadNotes();
  return box;
}

function deliveryDetail(pid, did, holder) {
  api("GET", "/projects/" + pid + "/deliveries/" + did).then(function (d) {
    var resp = d.response || {};
    holder.replaceChildren(el("div", { class: "card delivery-open" }, [el("dl", { class: "kv" }, [
      el("dt", { text: "Request" }), el("dd", { class: "mono", text: d.request.method + " " + d.request.url }),
      el("dt", { text: "Response status" }), el("dd", { text: resp.status === null || resp.status === undefined ? "none" : String(resp.status) }),
      el("dt", { text: "Duration" }), el("dd", { text: resp.duration_ms === null || resp.duration_ms === undefined ? "" : resp.duration_ms + " ms" }),
      el("dt", { text: "Time to deliver" }), el("dd", { text: d.time_to_deliver_ms === null || d.time_to_deliver_ms === undefined ? "not delivered" : d.time_to_deliver_ms + " ms" }),
      el("dt", { text: "Error" }), el("dd", { text: d.last_error || "" })]),
      el("h3", { text: "Request body" }), el("pre", { text: d.request.body }),
      el("h3", { text: "Response snippet" }), el("pre", { text: resp.body || "" })]));
  }).catch(function (e) { msg(holder, e.message, true); });
}
