"use strict";
function activityTable(rows, withProject) {
  var head = ["When", "Actor", "Action", "Target", "IP"];
  if (withProject) head.splice(1, 0, "Project");
  return el("table", {}, [el("tr", {}, head.map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (e) {
    var actor = e.actor_email || (e.actor_kind + (e.actor_id ? " " + e.actor_id : ""));
    var target = e.target_kind ? e.target_kind + " " + (e.target_id || "") : "";
    var cells = [el("td", { class: "muted", text: ago(e.created), title: e.created }), el("td", { text: actor }), el("td", {}, [pill(e.action, "ok")]), el("td", { class: "mono", text: target }), el("td", { class: "muted", text: e.ip || "" })];
    if (withProject) cells.splice(1, 0, el("td", { text: e.tenant_id === null ? "platform" : String(e.tenant_id) }));
    return el("tr", { title: e.details ? JSON.stringify(e.details) : "" }, cells);
  })));
}

function activityView(path, withProject, body) {
  var action = el("input", { placeholder: "action, e.g. provider" });
  var actor = el("input", { placeholder: "actor email or id" });
  var list = el("div"); var more = el("div");
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
      more.replaceChildren(page.length === 50 ? el("button", { text: "Older", onclick: function () { load(true); } }) : "");
    }).catch(function (e) { msg(list, e.message, true); });
  }
  body.appendChild(el("div", { class: "row" }, [action, actor, el("button", { class: "primary", text: "Filter", onclick: function () { load(false); } })]));
  body.appendChild(list);
  body.appendChild(more);
  load(false);
}

function activity(pid) {
  projectShell(pid, "activity", function (p, body) {
    activityView("/projects/" + pid + "/activity", false, body);
  });
}

function auditLog() {
  var body = el("div", {}, [el("h2", { text: "Audit log" }), el("p", { class: "sub", text: "Every sensitive action across the platform." })]);
  layout("audit", null, body);
  activityView("/admin/audit", true, body);
}
