"use strict";
var SVG_NS = "http://www.w3.org/2000/svg";

function svgEl(tag, attrs) {
  var n = document.createElementNS(SVG_NS, tag);
  Object.keys(attrs || {}).forEach(function (k) { n.setAttribute(k, attrs[k]); });
  return n;
}
