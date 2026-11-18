import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const I = require("../../web/pay-i18n.js");

test("language priority is query, browser, project default, english", () => {
  assert.equal(I.pickLanguage("fr", "de", ["es-ES"]), "fr");
  assert.equal(I.pickLanguage(null, "de", ["es-ES"]), "es");
  assert.equal(I.pickLanguage(null, "de", ["ja-JP"]), "de");
  assert.equal(I.pickLanguage(null, null, ["ja-JP"]), "en");
  assert.equal(I.pickLanguage("xx", null, []), "en");
  assert.equal(I.pickLanguage(null, null, ["ga-IE"]), "ga");
});

test("money is formatted for the locale", () => {
  assert.match(I.formatMoney(1500, "EUR", "de"), /15,00/);
  assert.match(I.formatMoney(1500, "EUR", "en-US"), /^€15\.00$/);
  assert.match(I.formatMoney(1234500, "EUR", "fr"), /12\s?345,00/);
  assert.equal(I.formatMoney(5_000_000, "BTC", "en"), "5,000 sats");
  assert.equal(I.formatMoney(5_000_001, "BTC", "en"), "5,001 sats");
});

test("countdown formatting", () => {
  assert.equal(I.formatCountdown(59_000), "0:59");
  assert.equal(I.formatCountdown(61_000), "1:01");
  assert.equal(I.formatCountdown(3_661_000), "1:01:01");
  assert.equal(I.formatCountdown(-5), "0:00");
});

test("translate falls back to english then the key and fills placeholders", () => {
  assert.equal(I.translate({ a: "x {n}" }, { a: "y" }, "a", { n: 2 }), "x 2");
  assert.equal(I.translate({}, { a: "y" }, "a"), "y");
  assert.equal(I.translate({}, {}, "missing"), "missing");
});

test("direction is rtl only for rtl languages", () => {
  assert.equal(I.direction("en"), "ltr");
  assert.equal(I.direction("ar"), "rtl");
});
