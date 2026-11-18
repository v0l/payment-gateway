"use strict";
var SVG_NS = "http://www.w3.org/2000/svg";

function svgEl(tag, attrs) {
  var n = document.createElementNS(SVG_NS, tag);
  Object.keys(attrs || {}).forEach(function (k) { n.setAttribute(k, attrs[k]); });
  return n;
}

function dayList(days) {
  var out = [];
  var now = new Date();
  for (var i = days - 1; i >= 0; i--) {
    var d = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() - i));
    out.push(d.toISOString().slice(0, 10));
  }
  return out;
}

function dailySeries(daily, currency, days) {
  var by = {};
  daily.forEach(function (d) { if (d.currency === currency) by[d.day] = d; });
  return dayList(days).map(function (day) {
    var d = by[day];
    return { day: day, volume: d ? d.volume : 0, payments: d ? d.payments : 0 };
  });
}

function shortDay(day) {
  return new Date(day + "T00:00:00Z").toLocaleDateString(undefined, { day: "numeric", month: "short", timeZone: "UTC" });
}

function volumeChart(series, currency) {
  var max = series.reduce(function (m, s) { return Math.max(m, s.volume); }, 0);
  var w = 1000, h = 200, gap = 3;
  var slot = w / series.length;
  var svg = svgEl("svg", { viewBox: "0 0 " + w + " " + h, preserveAspectRatio: "none", class: "chart-svg", role: "img", "aria-label": "Paid volume per day in " + currency });
  [0.5, 1].forEach(function (f) { svg.appendChild(svgEl("line", { x1: 0, x2: w, y1: h - f * (h - 8), y2: h - f * (h - 8), class: "chart-grid", "vector-effect": "non-scaling-stroke" })); });
  series.forEach(function (s, i) {
    var bh = max ? Math.max(s.volume ? 3 : 0, (s.volume / max) * (h - 8)) : 0;
    var bar = svgEl("rect", { x: i * slot + gap / 2, y: h - bh, width: Math.max(1, slot - gap), height: bh, class: "chart-bar" + (i === series.length - 1 ? " latest" : "") });
    var tip = svgEl("title");
    tip.textContent = shortDay(s.day) + ": " + fmtAmount(s.volume, currency) + " from " + s.payments + (s.payments === 1 ? " payment" : " payments");
    bar.appendChild(tip);
    svg.appendChild(bar);
  });
  svg.appendChild(svgEl("line", { x1: 0, x2: w, y1: h, y2: h, class: "chart-base", "vector-effect": "non-scaling-stroke" }));
  var mid = series[Math.floor(series.length / 2)];
  return el("figure", { class: "chart" }, [
    el("div", { class: "chart-scale" }, [el("span", { class: "money", text: max ? fmtAmount(max, currency) + " peak day" : "No volume yet" })]),
    svg,
    el("div", { class: "chart-axis" }, [el("span", { text: shortDay(series[0].day) }), el("span", { text: shortDay(mid.day) }), el("span", { text: "Today" })])
  ]);
}

function methodShares(rows, currency) {
  var total = 0;
  var by = {};
  rows.filter(function (r) { return r.currency === currency; }).forEach(function (r) { by[r.method] = (by[r.method] || 0) + r.volume; total += r.volume; });
  var names = Object.keys(by).sort(function (a, b) { return by[b] - by[a]; });
  if (!total) return el("p", { class: "muted", text: "No paid payments in this period." });
  var bar = el("div", { class: "share-bar", role: "img", "aria-label": "Share of volume by method" }, names.map(function (n, i) {
    return el("i", { class: "s" + (i % 4), title: n + " " + Math.round(by[n] / total * 100) + "%" });
  }));
  bar.childNodes.forEach(function (c, i) { c.style.flexGrow = String(by[names[i]]); });
  var legend = el("ul", { class: "share-legend" }, names.map(function (n, i) {
    return el("li", {}, [el("span", { class: "swatch-dot s" + (i % 4) }), el("span", { text: n }), el("span", { class: "money muted", text: fmtAmount(by[n], currency) + " (" + Math.round(by[n] / total * 100) + "%)" })]);
  }));
  return el("div", {}, [bar, legend]);
}
