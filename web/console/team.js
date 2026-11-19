"use strict";
function team(pid) {
  projectShell(pid, "team", function (p, body) {
    var list = el("div", {}, [skeletonRows()]); var out = el("div");
    function load() {
      api("GET", "/projects/" + pid + "/members").then(function (rows) {
        list.replaceChildren(table(["Member", "Role", ""], rows.map(function (m) {
          return el("tr", {}, [el("td", { text: m.email }), el("td", {}, [status("active", m.role.charAt(0).toUpperCase() + m.role.slice(1))]), el("td", { class: "num" }, [m.account_id === me.id || p.role !== "owner" ? null : el("button", { class: "link", type: "button", text: "Remove", onclick: function () {
            confirmDialog({ title: "Remove " + m.email, body: "They lose access to this project straight away.", keep: "Keep member", confirm: "Remove member", danger: true }).then(function (r) {
              if (r) api("DELETE", "/projects/" + pid + "/members/" + m.account_id).then(function () { toast("Member removed."); load(); }).catch(function (e) { msg(out, e.message, true); });
            });
          } })])]);
        })));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    body.appendChild(section("Members", [list]));
    if (p.role !== "viewer") {
      var email = el("input", { type: "email", placeholder: "teammate@example.com", "aria-label": "Email" });
      var role = el("select", { "aria-label": "Role" }, [["viewer", "Viewer, can read"], ["admin", "Admin, can take actions"], ["owner", "Owner, can change everything"]].map(function (r) { return el("option", { value: r[0], text: r[1] }); }));
      body.appendChild(section("Invite a teammate", [el("div", { class: "row" }, [email, role, el("button", { class: "primary", text: "Create invite link", onclick: function () {
        api("POST", "/projects/" + pid + "/invites", { email: email.value, role: role.value }).then(function (r) {
          out.replaceChildren(secretBox("Invite link for " + r.email + ", valid " + r.expires_in_days + " days", r.invite_url));
        }).catch(function (e) { msg(out, e.message, true); });
      } })]), out]));
    }
    load();
  }, { sub: "People who can see or work on this project." });
}
