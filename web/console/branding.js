"use strict";
function renderBranding(pid, box) {
  api("GET", "/projects/" + pid + "/branding").then(function (b) {
    function select(values, current) {
      var s = el("select", {}, values.map(function (v) { return el("option", { value: v, text: v }); }));
      s.value = current;
      return s;
    }
    var f = {
      default_lang: select(["en", "es", "fr", "de", "pt", "it", "ga"], b.default_lang || "en"),
      theme: select(["auto", "light", "dark"], b.theme),
      radius: select(["none", "sm", "md", "lg"], b.radius),
      font: select(["system", "serif", "mono", "rounded"], b.font),
      accent_dark: el("input", { value: b.accent_dark || "", placeholder: "#52c3d1" }),
      logo_dark_url: el("input", { value: b.logo_dark_url || "", placeholder: "https://..." }),
      thanks_text: el("textarea", { placeholder: "Shown after a successful payment" })
    };
    f.thanks_text.value = b.thanks_text || "";
    var links = el("textarea", { placeholder: "One per line: Label | https://..." });
    links.value = (b.footer_links || []).map(function (l) { return l.label + " | " + l.url; }).join("\n");
    var powered = el("input", { type: "checkbox" });
    powered.checked = b.show_powered_by !== false;
    powered.disabled = !b.can_hide_powered_by;
    var out = el("div");
    var save = el("button", { class: "primary", text: "Save branding" });
    save.addEventListener("click", function () {
      var payload = {};
      ["default_lang", "theme", "radius", "font"].forEach(function (k) { payload[k] = f[k].value; });
      ["accent_dark", "logo_dark_url", "thanks_text"].forEach(function (k) { payload[k] = f[k].value.trim() || null; });
      payload.footer_links = links.value.split("\n").map(function (l) { return l.trim(); }).filter(Boolean).map(function (l) {
        var i = l.indexOf("|");
        return { label: l.slice(0, i).trim(), url: l.slice(i + 1).trim() };
      });
      if (b.can_hide_powered_by) payload.show_powered_by = powered.checked;
      busy(save, function () { return api("POST", "/projects/" + pid + "/branding", payload); }).then(function () { msg(out, "Saved."); }).catch(function (e) { msg(out, e.message, true); });
    });
    var poweredField = el("div", { class: "field" }, [el("label", { class: "check" }, [powered, "Show Powered by on the pay page"])]);
    box.classList.add("form");
    box.replaceChildren(field("Default language", f.default_lang), field("Theme", f.theme), field("Corner radius", f.radius), field("Font", f.font),
      field("Accent colour in dark mode", f.accent_dark), field("Logo URL for dark mode", f.logo_dark_url), field("Thank-you text", f.thanks_text),
      field("Footer links", links), poweredField, el("div", { class: "row" }, [save]), out);
  }).catch(function (e) { box.replaceChildren(el("div", { class: "err", role: "alert", text: e.message })); });
}
