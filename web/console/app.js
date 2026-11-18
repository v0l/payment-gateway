"use strict";
function route() {
  stopTimer();
  var h = location.hash.replace(/^#/, "") || "/";
  var m = h.match(/^\/invite\/(.+)$/) || location.pathname.match(/^\/console\/invite\/(.+)$/);
  if (m && !me) { return invitePage(m[1]); }
  var rc = location.pathname.match(/^\/console\/reset\/(.+)$/);
  if (rc) { return resetConfirmPage(rc[1]); }
  var ec = location.pathname.match(/^\/console\/confirm-email\/(.+)$/);
  if (ec) { return confirmEmailPage(ec[1]); }
  if (!me && h === "/reset") { return resetRequestPage(); }
  if (!me) { return loginPage(); }
  var parts = h.split("/").filter(Boolean);
  if (parts.length === 0) { return me.projects.length ? go("#/p/" + me.projects[0].id + "/overview") : newProject(); }
  if (parts[0] === "new") return newProject();
  if (parts[0] === "account") return account();
  if (parts[0] === "admin" && me.is_operator) return admin();
  if (parts[0] === "audit" && me.is_operator) return auditLog();
  if (parts[0] === "p" && parts[1]) {
    var pid = parts[1], tab = parts[2] || "overview";
    if (tab === "overview") return overview(pid);
    if (tab === "payments") { if (parts[3] === "new") return newPayment(pid); if (parts[3]) return paymentDetail(pid, parts[3]); return payments(pid); }
    if (tab === "providers") { if (parts[3]) return providerForm(pid, parts[3]); return providers(pid); }
    if (tab === "webhooks") return webhooks(pid);
    if (tab === "balance") return balance(pid);
    if (tab === "team") return team(pid);
    if (tab === "activity") return activity(pid);
    if (tab === "settings") return settings(pid);
  }
  go("#/");
}

window.addEventListener("hashchange", route);
api("GET", "/me").then(function (m) { me = m; csrf = m.csrf; route(); }).catch(function () { me = null; route(); });
