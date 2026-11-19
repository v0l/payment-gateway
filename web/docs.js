"use strict";
(function () {
  function el(tag, attrs, kids) {
    var e = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      if (k === "text") e.textContent = attrs[k];
      else e.setAttribute(k, attrs[k]);
    });
    (kids || []).forEach(function (c) { e.appendChild(typeof c === "string" ? document.createTextNode(c) : c); });
    return e;
  }
  function params(op) {
    var rows = (op.parameters || []).map(function (p) {
      return el("tr", {}, [el("td", {}, [el("code", { text: p.name })]), el("td", { text: p.in }), el("td", { text: p.required ? "required" : "optional" })]);
    });
    return rows.length ? el("table", {}, [el("thead", {}, [el("tr", {}, ["Name", "In", "Required"].map(function (h) { return el("th", { scope: "col", text: h }); }))]), el("tbody", {}, rows)]) : null;
  }
  function render(spec) {
    var root = document.getElementById("root");
    var boxes = [];
    root.replaceChildren(el("h1", { text: spec.info.title }), el("p", { text: "Version " + spec.info.version + ". " + spec.info.description }));
    root.appendChild(el("p", { text: "Authenticate with an Authorization: Bearer header carrying a project API key. List endpoints accept limit (1 to 100) and cursor, and return the next cursor in the X-Next-Cursor header." }));
    Object.keys(spec.paths).forEach(function (path) {
      Object.keys(spec.paths[path]).forEach(function (method) {
        var op = spec.paths[path][method];
        var full = "/v1" + path;
        var box = el("section", { class: "op", "aria-label": method.toUpperCase() + " " + full }, [
          el("div", { class: "line" }, [el("span", { class: "method " + method, text: method.toUpperCase() }), el("span", { class: "path", text: full })]),
          el("p", { class: "summary", text: op.summary }),
        ]);
        if (op["x-required-scope"]) box.appendChild(el("p", { class: "scope", text: "Scope: " + op["x-required-scope"] }));
        var t = params(op);
        if (t) box.appendChild(t);
        box.setAttribute("data-text", (method + " " + full + " " + (op.summary || "")).toLowerCase());
        boxes.push(box);
        root.appendChild(box);
      });
    });
    root.appendChild(el("p", {}, [el("a", { href: "/v1/openapi.json", text: "OpenAPI document (JSON)" })]));
    var input = document.getElementById("ref-search");
    var count = document.getElementById("ref-count");
    function filter() {
      var terms = input.value.toLowerCase().split(/\s+/).filter(Boolean);
      var shown = 0;
      boxes.forEach(function (b) {
        var text = b.getAttribute("data-text");
        var ok = terms.every(function (t) { return text.indexOf(t) >= 0; });
        b.hidden = !ok;
        if (ok) shown++;
      });
      count.textContent = terms.length ? shown + " of " + boxes.length + " endpoints" : "";
    }
    input.addEventListener("input", filter);
  }
  fetch("/v1/openapi.json").then(function (r) { return r.json(); }).then(render).catch(function (e) {
    document.getElementById("root").textContent = "Could not load the API description: " + e.message;
  });
})();
