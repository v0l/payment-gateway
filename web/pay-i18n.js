(function (root, factory) {
  "use strict";
  var api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.PayI18n = api;
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  var LANGUAGES = ["en", "es", "fr", "de", "pt", "it", "ga"];
  var NAMES = { en: "English", es: "Español", fr: "Français", de: "Deutsch", pt: "Português", it: "Italiano", ga: "Gaeilge" };
  var RTL = ["ar", "he", "fa", "ur"];

  function normalise(tag) {
    if (!tag) return null;
    var base = String(tag).toLowerCase().split(/[-_]/)[0];
    return LANGUAGES.indexOf(base) >= 0 ? base : null;
  }

  function pickLanguage(query, tenantDefault, browserLanguages) {
    var fromQuery = normalise(query);
    if (fromQuery) return fromQuery;
    var list = browserLanguages || [];
    for (var i = 0; i < list.length; i++) {
      var hit = normalise(list[i]);
      if (hit) return hit;
    }
    return normalise(tenantDefault) || "en";
  }

  function direction(lang) {
    return RTL.indexOf(String(lang).split("-")[0]) >= 0 ? "rtl" : "ltr";
  }

  function translate(dict, fallback, key, vars) {
    var text = dict && dict[key] !== undefined ? dict[key] : fallback && fallback[key] !== undefined ? fallback[key] : key;
    return text.replace(/\{(\w+)\}/g, function (m, name) {
      return vars && vars[name] !== undefined ? String(vars[name]) : m;
    });
  }

  function formatMoney(amount, currency, lang) {
    if (currency === "BTC") {
      var sats = Math.ceil(amount / 1000);
      return new Intl.NumberFormat(lang).format(sats) + " sats";
    }
    var scale = 100;
    var code = currency;
    if (currency.length === 4 && currency[0] === "M") {
      scale = 100000;
      code = currency.slice(1);
    }
    try {
      var digits = scale === 100 ? undefined : 3;
      return new Intl.NumberFormat(lang, {
        style: "currency",
        currency: code,
        minimumFractionDigits: digits,
        maximumFractionDigits: digits
      }).format(amount / scale);
    } catch (e) {
      return new Intl.NumberFormat(lang, { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(amount / scale) + " " + currency;
    }
  }

  function formatCountdown(ms) {
    var total = Math.max(0, Math.floor(ms / 1000));
    var h = Math.floor(total / 3600);
    var m = Math.floor((total % 3600) / 60);
    var s = total % 60;
    var two = function (n) { return (n < 10 ? "0" : "") + n; };
    return h > 0 ? h + ":" + two(m) + ":" + two(s) : m + ":" + two(s);
  }

  return {
    LANGUAGES: LANGUAGES,
    NAMES: NAMES,
    normalise: normalise,
    pickLanguage: pickLanguage,
    direction: direction,
    translate: translate,
    formatMoney: formatMoney,
    formatCountdown: formatCountdown
  };
});
