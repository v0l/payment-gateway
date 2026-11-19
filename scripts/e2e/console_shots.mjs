import { mkdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { chromium } from "playwright-core";

const require = createRequire(import.meta.url);
const axeSource = readFileSync(require.resolve("axe-core/axe.min.js"), "utf8");
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../..");
const out = process.env.SHOTS_DIR || path.join(root, "docs/design/screenshots/console-v4");
mkdirSync(out, { recursive: true });
const base = process.env.DEMO_URL || "http://127.0.0.1:18760";
const project = process.env.DEMO_PROJECT || "1";
const only = process.env.ONLY ? process.env.ONLY.split(",") : null;
const widths = (process.env.WIDTHS || "1440,390").split(",").map(Number);

const failures = [];
function check(name, ok, detail = "") {
  if (!ok) { failures.push(name); console.log(`FAIL ${name} ${detail}`); }
}

async function firstPayment(page) {
  return page.evaluate(async (pid) => {
    const r = await fetch(`/console/api/projects/${pid}/payments?q=INV-2291`, { credentials: "same-origin" });
    const j = await r.json();
    return j.payments[0].id;
  }, project);
}

async function firstOf(page, url, pick) {
  return page.evaluate(async ({ url, pick }) => {
    const r = await fetch(url, { credentials: "same-origin" });
    const j = await r.json();
    const list = pick ? j[pick] : j;
    return list && list[0] ? (list[0].customer_ref || list[0].id) : null;
  }, { url, pick });
}

async function run() {
  const browser = await chromium.launch({ executablePath: process.env.CHROME || undefined, args: ["--no-sandbox"] });
  try {
    const login = await browser.newContext({ viewport: { width: 1440, height: 900 }, locale: "en-IE", timezoneId: "Europe/Dublin", colorScheme: "light" });
    const lp = await login.newPage();
    await lp.goto(`${base}/console`);
    await lp.waitForSelector("input[type=email]");
    await lp.screenshot({ path: path.join(out, "login-1440.png") });
    await lp.fill("input[type=email]", "sam@northwind.example");
    await lp.fill("input[type=password]", "correct horse battery");
    await lp.click("button[type=submit]");
    await lp.waitForSelector(".tabs");
    const cookies = await login.cookies();
    const pay = await firstPayment(lp);
    const sub = await firstOf(lp, `/console/api/projects/${project}/subscriptions`, null);
    const pages = [
      ["home", `#/p/${project}/overview`, ".queue table, .queue .quiet"],
      ["payments", `#/p/${project}/payments`, ".split table, .mob-list li"],
      ["payment", `#/p/${project}/payments/${pay}`, ".detail"],
      ["balance", `#/p/${project}/balance`, ".balance-hero .big-money"],
      ["payouts", `#/p/${project}/payouts`, "main h1"],
      ["customers", `#/p/${project}/customers`, "table"],
      ["customer", `#/p/${project}/customers/cus_amelia.k`, "main h1"],
      ["subscriptions", `#/p/${project}/subscriptions`, "main h1"],
      ["plans", `#/p/${project}/plans`, "main h1"],
      ["platform-bill", `#/p/${project}/billing`, "main h1"],
      ["providers", `#/p/${project}/providers`, "main h1"],
      ["keys", `#/p/${project}/keys`, "table"],
      ["webhooks", `#/p/${project}/webhooks`, "table"],
      ["settings", `#/p/${project}/settings`, "main h1"],
      ["team", `#/p/${project}/team`, "table"],
      ["activity", `#/p/${project}/activity`, "table"],
      ["new-payment", `#/p/${project}/payments/new`, "main h1"],
      ["account", `#/account`, "main h1"],
      ["new-project", `#/new`, "main h1"],
      ["op-projects", `#/tenants`, "table"],
      ["op-support", `#/support/${project}`, "main h1"],
      ["op-accounts", `#/admin`, "table"],
      ["op-shared", `#/shared`, "main h1"],
      ["op-platform-plans", `#/platform-plans`, "main h1"],
      ["op-treasury", `#/treasury`, "main h1"],
      ["op-payout-queue", `#/payout-queue`, "main h1"],
      ["op-audit", `#/audit`, "table"],
    ];
    if (sub) pages.push(["subscription", `#/p/${project}/subscriptions/${sub}`, "main h1"]);
    for (const width of widths) {
      const ctx = await browser.newContext({ viewport: { width, height: width > 600 ? 900 : 844 }, locale: "en-IE", timezoneId: "Europe/Dublin", colorScheme: "light", isMobile: width < 600, hasTouch: width < 600 });
      await ctx.addCookies(cookies);
      const page = await ctx.newPage();
      const errors = [];
      page.on("pageerror", (e) => errors.push(e.message));
      page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
      for (const [name, hash, wait] of pages) {
        if (only && !only.includes(name)) continue;
        errors.length = 0;
        await page.goto(`${base}/console${hash}`);
        try { await page.waitForSelector(wait, { timeout: 8000, state: "attached" }); } catch { check(`${name} ${width} loads`, false, wait); }
        await page.waitForTimeout(700);
        await page.evaluate(() => document.fonts.ready);
        const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
        check(`${name} ${width} no sideways scroll`, !overflow);
        check(`${name} ${width} no errors`, errors.length === 0, JSON.stringify(errors));
        const nullText = await page.evaluate(() => /\bnull\b|undefined|NaN|\[object/.test(document.body.innerText));
        check(`${name} ${width} no null text`, !nullText);
        if (width === 1440 || process.env.AXE_ALL) {
          await page.evaluate(axeSource);
          const res = await page.evaluate(() => axe.run(document, { runOnly: ["wcag2a", "wcag2aa", "wcag21aa"] }));
          const serious = res.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
          check(`${name} ${width} axe`, serious.length === 0, JSON.stringify(serious.map((v) => [v.id, v.nodes.slice(0, 3).map((n) => n.target)])));
        }
        await page.screenshot({ path: path.join(out, `${name}-${width}.png`) });
        if (process.env.FULL) await page.screenshot({ path: path.join(out, `${name}-${width}-full.png`), fullPage: true });
      }
      await ctx.close();
    }
  } finally {
    await browser.close();
  }
  console.log(failures.length ? `${failures.length} checks failed` : "all checks passed");
  process.exit(failures.length ? 1 : 0);
}

run().catch((e) => { console.error(e); process.exit(2); });
