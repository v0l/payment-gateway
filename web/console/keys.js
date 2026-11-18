"use strict";
function apiKeys(pid) {
  projectShell(pid, "keys", function (p, body) {
    var out = el("div");
    var list = el("div");
    var scopes = ["payments:read", "payments:write", "refunds:write", "saved:write", "admin"];
    var name = el("input", { placeholder: "Key name" });
    var expires = el("input", { type: "date" });
    var boxes = scopes.map(function (s) { return el("input", { type: "checkbox", value: s }); });
    function load() {
      api("GET", "/projects/" + pid + "/keys").then(function (rows) {
        list.replaceChildren(el("table", {}, [el("tr", {}, ["Name", "Key", "Scopes", "Last used", "Expires", ""].map(function (h) { return el("th", { text: h }); }))].concat(rows.map(function (k) {
          var state = k.revoked_at ? "revoked" : (k.expires && new Date(k.expires) < new Date() ? "expired" : "");
          return el("tr", {}, [el("td", { text: k.name }), el("td", { class: "mono", text: k.prefix + "..." }),
            el("td", { text: k.scopes.join(", ") }), el("td", { class: "muted", text: k.last_used ? ago(k.last_used) : "never" }),
            el("td", { class: "muted", text: k.expires ? new Date(k.expires).toLocaleDateString() : "never" }),
            el("td", {}, [state ? pill(state, "bad") : el("a", { text: "revoke", onclick: function () {
              if (!confirm("Requests using this key stop working immediately. Continue?")) return;
              api("POST", "/projects/" + pid + "/keys/" + k.id + "/revoke", {}).then(load).catch(function (e) { msg(out, e.message, true); });
            } })])]);
        }))));
      });
    }
    var create = el("button", { text: "Create key", onclick: function () {
      var chosen = boxes.filter(function (b) { return b.checked; }).map(function (b) { return b.value; });
      var req = { name: name.value, scopes: chosen };
      if (expires.value) req.expires = new Date(expires.value + "T23:59:59Z").toISOString();
      api("POST", "/projects/" + pid + "/keys", req).then(function (r) {
        out.replaceChildren(secretBox("New API key (" + r.name + "), shown once", r.api_key));
        name.value = ""; load();
      }).catch(function (e) { msg(out, e.message, true); });
    } });
    body.appendChild(el("p", { class: "muted", text: p.trusted ? "Keys created here are live keys (pgw_live_)." : "Keys created here are test keys (pgw_test_) until the project goes live." }));
    body.appendChild(el("div", { class: "row" }, [name, expires].concat(boxes.map(function (b, i) { return el("label", {}, [b, " " + scopes[i]]); })).concat([create])));
    body.appendChild(out); body.appendChild(list); load();
  });
}
