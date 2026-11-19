"use strict";
function activityTable(rows, withProject) {
  var head = ["When", "Who", "Action", "Target", "IP"];
  if (withProject) head.splice(1, 0, "Project");
  if (!rows.length) return el("div", { class: "empty" }, [el("b", { text: "Nothing recorded yet." }), el("p", { class: "muted", text: "Sign-ins, key changes, refunds and settings changes are recorded here." })]);
  return table(head, rows.map(function (e) {
    var actor = e.actor_email || (e.actor_kind + (e.actor_id ? " " + e.actor_id : ""));
    var target = e.target_kind ? e.target_kind + " " + (e.target_id || "") : "";
    var cells = [el("td", { class: "muted", text: fmtWhen(e.created), title: e.created }), el("td", { text: actor }), el("td", { text: e.action }), el("td", { class: "mono trunc", text: target }), el("td", { class: "muted", text: e.ip || "" })];
    if (withProject) cells.splice(1, 0, el("td", { class: "muted", text: e.tenant_id === null ? "Platform" : String(e.tenant_id) }));
    return el("tr", { title: e.details ? JSON.stringify(e.details) : "" }, cells);
  }));
}

function activityView(path, withProject, body) {
  var action = el("input", { type: "search", placeholder: "Action, for example provider", "aria-label": "Action" });
  var actor = el("input", { type: "search", placeholder: "Who, email or id", "aria-label": "Who" });
  var list = el("div", {}, [skeletonRows()]); var more = el("div", { class: "more" });
  var rows = [];
  function query(before) {
    var q = ["limit=50"];
    if (action.value.trim()) q.push("action=" + encodeURIComponent(action.value.trim()));
    if (actor.value.trim()) q.push("actor=" + encodeURIComponent(actor.value.trim()));
    if (before) q.push("before=" + before);
    return path + "?" + q.join("&");
  }
  function load(append) {
    var before = append && rows.length ? rows[rows.length - 1].id : null;
    api("GET", query(before)).then(function (page) {
      rows = append ? rows.concat(page) : page;
      list.replaceChildren(activityTable(rows, withProject));
      more.replaceChildren(page.length === 50 ? el("button", { text: "Older", onclick: function () { load(true); } }) : el("span"));
    }).catch(function (e) { msg(list, e.message, true); });
  }
  [action, actor].forEach(function (i) { i.addEventListener("keydown", function (e) { if (e.key === "Enter") load(false); }); });
  body.appendChild(el("div", { class: "filterbar" }, [action, actor, el("button", { text: "Filter", onclick: function () { load(false); } })]));
  body.appendChild(list);
  body.appendChild(more);
  load(false);
}

function activity(pid) {
  projectShell(pid, "activity", function (p, body) {
    activityView("/projects/" + pid + "/activity", false, body);
  }, { sub: "Who did what in this project. Kept for audit." });
}

function auditLog() {
  var body = el("div");
  operatorPage("audit", "Audit log", "Every sensitive action across the platform.", body);
  activityView("/admin/audit", true, body);
}
