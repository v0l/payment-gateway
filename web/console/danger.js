"use strict";
function dangerZone(pid, p) {
  var out = el("div"); var box = el("div", { class: "card" });
  var token = el("input", { placeholder: "Deletion token", "aria-label": "Deletion token", autocomplete: "off" });
  function draw(s) {
    var kids = [];
    kids.push(el("div", { class: "row" }, [el("button", { class: "danger", text: p.enabled === false ? "Project disabled" : "Disable project", disabled: p.enabled === false, onclick: function () {
      confirmDialog({ title: "Disable project", body: "API keys stop working and no new payments are accepted. You can enable it again from the operator area or by calling the settings endpoint.", keep: "Keep enabled", confirm: "Disable", danger: true }).then(function (r) {
        if (!r) return;
        api("POST", "/projects/" + pid + "/settings/enabled", { enabled: false }).then(function () { toast("Project disabled."); go("#/"); location.reload(); }).catch(function (e) { msg(out, e.message, true); });
      });
    } })]));
    kids.push(el("p", { class: "muted", text: "Deleting a project needs a token and a " + s.cooling_off_hours + " hour cooling-off period. It is blocked while there are pending payments or a balance." }));
    if (s.delete_requested_at) {
      kids.push(el("p", { text: "Deletion requested " + fmtWhen(s.delete_requested_at) + ". Paste the token to confirm once the cooling-off has passed." }));
      kids.push(el("div", { class: "row" }, [token, el("button", { class: "danger", text: "Delete project", onclick: function () {
        confirmDialog({ title: "Delete project", body: "This disables the project, revokes its keys and removes its members. It cannot be undone.", keep: "Keep project", confirm: "Delete forever", danger: true }).then(function (r) {
          if (!r) return;
          api("POST", "/projects/" + pid + "/delete-confirm", { token: token.value }).then(function () { toast("Project deleted."); api("GET", "/me").then(function (m) { me = m; go("#/"); route(); }); }).catch(function (e) { msg(out, e.message, true); });
        });
      } }), el("button", { text: "Cancel request", onclick: function () { api("DELETE", "/projects/" + pid + "/delete-request").then(load); } })]));
    } else {
      kids.push(el("button", { class: "danger", text: "Request deletion", onclick: function () {
        api("POST", "/projects/" + pid + "/delete-request", {}).then(function (r) { out.replaceChildren(secretBox("Deletion token", r.token)); load(true); }).catch(function (e) { msg(out, e.message, true); });
      } }));
    }
    box.replaceChildren.apply(box, kids);
  }
  function load(keepOut) {
    if (!keepOut) out.replaceChildren();
    api("GET", "/projects/" + pid + "/settings").then(draw).catch(function (e) { msg(box, e.message, true); });
  }
  load();
  return section("Disable or delete", [box, out]);
}

function projectConfigPanels(pid, p) {
  var wrap = el("div");
  api("GET", "/projects/" + pid + "/settings").then(function (s) {
    var legal = { name: el("input", { value: s.legal_name || "" }), address: el("textarea", { rows: "3" }), vat_id: el("input", { value: s.legal_vat_id || "" }), email: el("input", { value: s.legal_email || "" }) };
    legal.address.value = s.legal_address || "";
    var legalOut = el("div");
    var save = el("button", { class: "primary", text: "Save legal details", disabled: p.role === "viewer", onclick: function () {
      var body = {}; Object.keys(legal).forEach(function (k) { body[k] = legal[k].value; });
      api("POST", "/projects/" + pid + "/settings/legal", body).then(function () { msg(legalOut, "Saved."); }).catch(function (e) { msg(legalOut, e.message, true); });
    } });
    var kinds = [["webhook_abandoned", "A webhook delivery is abandoned"], ["provider_failing", "A provider keeps failing"], ["refund_failed", "A refund fails"]];
    var boxes = {}; var notifOut = el("div");
    var notifRows = kinds.map(function (k) { var c = el("input", { type: "checkbox" }); c.checked = !!s["notify_" + k[0]]; boxes[k[0]] = c; return el("div", {}, [el("label", { class: "check" }, [c, k[1]])]); });
    var saveNotif = el("button", { text: "Save notifications", disabled: p.role === "viewer", onclick: function () {
      var body = {}; kinds.forEach(function (k) { body[k[0]] = boxes[k[0]].checked; });
      api("POST", "/projects/" + pid + "/settings/notifications", body).then(function () { msg(notifOut, "Saved."); }).catch(function (e) { msg(notifOut, e.message, true); });
    } });
    var manual = el("input", { type: "checkbox" }); manual.checked = !!s.allow_manual_paid; manual.disabled = p.role !== "owner";
    var manualOut = el("div");
    manual.addEventListener("change", function () {
      api("POST", "/projects/" + pid + "/manual-paid", { enabled: manual.checked }).then(function () { msg(manualOut, "Saved."); }).catch(function (e) { manual.checked = !manual.checked; msg(manualOut, e.message, true); });
    });
    wrap.replaceChildren(
      section("Legal details for receipts", [el("div", { class: "card form" }, [field("Legal name", legal.name), field("Address", legal.address), field("VAT id", legal.vat_id), field("Contact email", legal.email), el("div", { class: "row" }, [save]), legalOut])]),
      section("Notifications", [el("div", { class: "card" }, notifRows.concat([el("div", { class: "row" }, [saveNotif]), notifOut]))]),
      section("Manual payment handling", [el("div", { class: "card" }, [el("label", { class: "check" }, [manual, "Let admins mark waiting payments as paid, with a reason. Needed for Accept in the attention queue."]), manualOut])]));
  }).catch(function (e) { msg(wrap, e.message, true); });
  return wrap;
}
