"use strict";
function newProject() {
  var name = el("input", { placeholder: "My shop" });
  var out = el("div");
  var btn = el("button", { class: "primary", text: "Create project" });
  btn.addEventListener("click", function () {
    busy(btn, function () { return api("POST", "/projects", { name: name.value }); }).then(function (r) {
      out.replaceChildren(secretBox("API key", r.api_key), secretBox("Webhook signing secret", r.webhook_secret),
        el("p", {}, [el("a", { text: "Go to project", href: "#/p/" + r.id + "/overview" })]));
      return api("GET", "/me").then(function (m) { me = m; });
    }).catch(function (e) { msg(out, e.message, true); });
  });
  operatorPage("new", "New project", "A project has its own API key, payment methods and webhooks.", el("div", { class: "card" }, [field("Name", name), el("div", { class: "row" }, [btn]), out]));
}
