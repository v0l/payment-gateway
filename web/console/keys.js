"use strict";
function apiKeys(pid) {
  projectShell(pid, "keys", function (p, body) {
    var out = el("div");
    var list = el("div", {}, [skeletonRows()]);
    var scopes = ["payments:read", "payments:write", "refunds:write", "saved:write", "admin"];
    var name = el("input", { placeholder: "Key name, for example Billing server", "aria-label": "Key name" });
    var expires = el("input", { type: "date", "aria-label": "Expires" });
    var boxes = scopes.map(function (s) { return el("input", { type: "checkbox", value: s }); });
    function load() {
      api("GET", "/projects/" + pid + "/keys").then(function (rows) {
        list.replaceChildren(rows.length ? table(["Name", "Key", "Scopes", "Last used", "Expires", ""], rows.map(function (k) {
          var state = k.revoked_at ? "revoked" : (k.expires && new Date(k.expires) < new Date() ? "expired" : "");
          return el("tr", {}, [el("td", { text: k.name }), el("td", { class: "mono", text: k.prefix + "\u2026" }),
            el("td", { class: "muted", text: k.scopes.join(", ") }), el("td", { class: "muted", text: k.last_used ? fmtWhen(k.last_used) : "Never" }),
            el("td", { class: "muted", text: k.expires ? fmtDay(k.expires) : "Never" }),
            el("td", { class: "num" }, [state ? status(state === "revoked" ? "failed" : "expired", state === "revoked" ? "Revoked" : "Expired") : el("button", { class: "link", type: "button", text: "Revoke", disabled: p.role === "viewer", onclick: function () {
              confirmDialog({ title: "Revoke " + k.name, body: "Requests that use this key stop working immediately.", keep: "Keep key", confirm: "Revoke key", danger: true }).then(function (r) {
                if (r) api("POST", "/projects/" + pid + "/keys/" + k.id + "/revoke", {}).then(function () { toast("Key revoked."); load(); }).catch(function (e) { msg(out, e.message, true); });
              });
            } })])]);
        })) : el("div", { class: "empty" }, [el("b", { text: "No extra keys yet." }), el("p", { class: "muted", text: "Create a key per server so you can revoke one without touching the others." })]));
      }).catch(function (e) { msg(list, e.message, true); });
    }
    var create = el("button", { class: "primary", text: "Create key", disabled: p.role === "viewer", onclick: function () {
      var chosen = boxes.filter(function (b) { return b.checked; }).map(function (b) { return b.value; });
      var req = { name: name.value, scopes: chosen };
      if (expires.value) req.expires = new Date(expires.value + "T23:59:59Z").toISOString();
      api("POST", "/projects/" + pid + "/keys", req).then(function (r) {
        out.replaceChildren(secretBox("New API key (" + r.name + ")", r.api_key));
        name.value = ""; load();
      }).catch(function (e) { msg(out, e.message, true); });
    } });
    body.appendChild(section("Keys", [list]));
    body.appendChild(section("Create a key", [
      el("p", { class: "muted", text: p.trusted ? "Keys created here are live keys (pgw_live_)." : "Keys created here are test keys (pgw_test_) until the project goes live." }),
      el("div", { class: "card form" }, [field("Name", name), field("Expires (optional)", expires),
        el("div", { class: "field" }, [el("label", { text: "Scopes" }), el("div", { class: "row" }, boxes.map(function (b, i) { return el("label", { class: "check" }, [b, scopes[i]]); }))]),
        el("div", { class: "row" }, [create]), out])]));
    load();
  }, { sub: "Keys your servers use to call the API. Each key shows once, when you create it." });
}
