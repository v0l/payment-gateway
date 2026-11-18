"use strict";
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

