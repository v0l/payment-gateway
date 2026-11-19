import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../web/tokens.css", import.meta.url), "utf8");

function block(start) {
  const i = css.indexOf(start);
  const open = css.indexOf("{", i);
  let depth = 0;
  for (let j = open; j < css.length; j++) {
    if (css[j] === "{") depth++;
    if (css[j] === "}" && --depth === 0) return css.slice(open + 1, j);
  }
  return "";
}

function vars(text) {
  const out = {};
  for (const m of text.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) out[m[1]] = m[2];
  return out;
}

const light = vars(block(":root, [data-theme=\"light\"] {"));
const dark = { ...light, ...vars(block('[data-theme="dark"] {')) };

function lum(hex) {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
}
function ratio(a, b) {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

const pairs = [
  ["text", "bg", 4.5],
  ["text", "card", 4.5],
  ["muted", "bg", 4.5],
  ["muted", "card", 4.5],
  ["muted", "code", 4.5],
  ["accent", "bg", 4.5],
  ["accent", "card", 4.5],
  ["accent-text", "accent", 4.5],
  ["ok", "card", 4.5],
  ["ok", "ok-bg", 4.5],
  ["warn", "card", 4.5],
  ["warn", "warn-bg", 4.5],
  ["bad", "card", 4.5],
  ["bad", "bad-bg", 4.5],
  ["info", "card", 4.5],
  ["info", "info-bg", 4.5],
  ["line-strong", "card", 3],
  ["line-strong", "bg", 3],
  ["focus", "card", 3],
  ["faint", "card", 3],
  ["muted", "rail", 4.5],
  ["muted", "hover", 4.5],
  ["accent", "accent-soft", 4.5],
  ["text", "rail", 4.5],
];

let failed = 0;
const rows = [];
for (const [name, theme] of [["light", light], ["dark", dark]]) {
  for (const [fg, bg, min] of pairs) {
    const r = ratio(theme[fg], theme[bg]);
    const pass = r >= min;
    if (!pass) failed++;
    rows.push(`| ${name} | ${fg} on ${bg} | ${theme[fg]} on ${theme[bg]} | ${r.toFixed(2)} | ${min} | ${pass ? "pass" : "FAIL"} |`);
  }
}
console.log("| Theme | Pair | Colours | Ratio | Needs | Result |");
console.log("| --- | --- | --- | --- | --- | --- |");
console.log(rows.join("\n"));
process.exit(failed ? 1 : 0);
