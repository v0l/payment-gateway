import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium, type Page } from 'playwright-core';

const axeSource = readFileSync(join(import.meta.dirname, '..', 'node_modules', 'axe-core', 'axe.min.js'), 'utf8');

const base = process.env.DEMO_URL ?? 'http://127.0.0.1:18761';
const out = process.env.SHOTS_DIR ?? '/tmp/console-shots';
const project = process.env.DEMO_PROJECT ?? '1';
const only = process.env.ONLY ? process.env.ONLY.split(',') : null;
const widths = (process.env.WIDTHS ?? '1440,390').split(',').map(Number);
const axeOn = process.env.AXE === '1';
const frozen = new Date('2026-11-20T12:00:00Z');
mkdirSync(out, { recursive: true });

const failures: string[] = [];
const check = (name: string, ok: boolean, detail = '') => {
  if (!ok) {
    failures.push(name);
    console.log(`FAIL ${name} ${detail}`);
  }
};

async function firstOf(page: Page, url: string, pick: string | null): Promise<string | null> {
  return page.evaluate(
    async ({ url, pick }) => {
      const r = await fetch(url, { credentials: 'same-origin' });
      const j = await r.json();
      const list = pick ? j[pick] : j;
      return list && list[0] ? (list[0].customer_ref ?? list[0].id) : null;
    },
    { url, pick },
  );
}

const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/usr/bin/google-chrome', args: ['--no-sandbox'] });
const login = await browser.newContext({ viewport: { width: 1440, height: 900 }, locale: 'en-IE', timezoneId: 'Europe/Dublin', colorScheme: 'light' });
const lp = await login.newPage();
await lp.goto(`${base}/console`);
await lp.waitForSelector('input[type=email]');
await lp.screenshot({ path: join(out, 'login-1440.png') });
await lp.fill('input[type=email]', 'sam@northwind.example');
await lp.fill('input[type=password]', 'correct horse battery');
await lp.click('button[type=submit]');
await lp.waitForSelector('.tabs');
const cookies = await login.cookies();
const payment = await lp.evaluate(async pid => {
  const r = await fetch(`/console/api/projects/${pid}/payments?q=INV-2291`, { credentials: 'same-origin' });
  const j = await r.json();
  return j.payments[0].id as string;
}, project);
const sub = await firstOf(lp, `/console/api/projects/${project}/subscriptions`, null);
const p = (tab: string) => `#/p/${project}/${tab}`;
const pages: [string, string, string][] = [
  ['home', p('overview'), '.queue table, .queue .quiet'],
  ['payments', p('payments'), '.split table, .mob-list li'],
  ['payment', p(`payments/${payment}`), '.detail'],
  ['balance', p('balance'), '.balance-hero .big-money'],
  ['payouts', p('payouts'), 'main h1'],
  ['customers', p('customers'), 'table'],
  ['customer', p('customers/cus_amelia.k'), 'main h1'],
  ['subscriptions', p('subscriptions'), 'main h1'],
  ['plans', p('plans'), 'main h1'],
  ['platform-bill', p('billing'), 'main h1'],
  ['providers', p('providers'), 'main h1'],
  ['keys', p('keys'), 'table'],
  ['webhooks', p('webhooks'), 'table'],
  ['settings', p('settings'), 'main h1'],
  ['team', p('team'), 'table'],
  ['activity', p('activity'), 'table'],
  ['new-payment', p('payments/new'), 'main h1'],
  ['account', '#/account', 'main h1'],
  ['new-project', '#/new', 'main h1'],
  ['op-projects', '#/tenants', 'table'],
  ['op-support', `#/support/${project}`, 'main h1'],
  ['op-accounts', '#/admin', 'table'],
  ['op-shared', '#/shared', 'main h1'],
  ['op-platform-plans', '#/platform-plans', 'main h1'],
  ['op-treasury', '#/treasury', 'main h1'],
  ['op-payout-queue', '#/payout-queue', 'main h1'],
  ['op-audit', '#/audit', 'table'],
];
if (sub) pages.push(['subscription', p(`subscriptions/${sub}`), 'main h1']);

const maskVolatile = `.countdown, .toast { visibility: hidden !important }`;

for (const width of widths) {
  const ctx = await browser.newContext({
    viewport: { width, height: width > 600 ? 900 : 844 },
    locale: 'en-IE',
    timezoneId: 'Europe/Dublin',
    colorScheme: 'light',
    isMobile: width < 600,
    hasTouch: width < 600,
    bypassCSP: true,
  });
  await ctx.addCookies(cookies);
  const page = await ctx.newPage();
  await page.clock.setFixedTime(frozen);
  const errors: string[] = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', m => {
    if (m.type() === 'error') errors.push(m.text());
  });
  for (const [name, hash, wait] of pages) {
    if (only && !only.includes(name)) continue;
    errors.length = 0;
    await page.goto(`${base}/console${hash}`);
    try {
      await page.waitForSelector(wait, { timeout: 8000, state: 'attached' });
    } catch {
      check(`${name} ${width} loads`, false, wait);
    }
    await page.waitForTimeout(700);
    await page.evaluate(() => document.fonts.ready);
    await page.addStyleTag({ content: maskVolatile });
    await page.evaluate(() => {
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        n.nodeValue = (n.nodeValue ?? '').replace(/\b\d{1,2}:\d{2}(:\d{2})?\b/g, '00:00').replace(/\b\d+[smhd] ago\b/g, '0m ago').replace(/\b[0-9a-f]{12,}\b/g, 'ffffffffffff').replace(/127\.0\.0\.1:\d+/g, '127.0.0.1:0');
      }
    });
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
    check(`${name} ${width} no sideways scroll`, !overflow);
    check(`${name} ${width} no errors`, errors.length === 0, JSON.stringify(errors));
    const nullText = await page.evaluate(() => /\bnull\b|undefined|NaN|\[object/.test(document.body.innerText));
    check(`${name} ${width} no null text`, !nullText);
    if (axeOn && (width === 1440 || process.env.AXE_ALL)) {
      await page.evaluate(axeSource);
      const res = await page.evaluate(() =>
        (window as unknown as { axe: { run(d: Document, o: object): Promise<{ violations: { id: string; impact: string }[] }> } }).axe.run(document, {
          runOnly: ['wcag2a', 'wcag2aa', 'wcag21aa'],
        }),
      );
      const serious = res.violations.filter(v => v.impact === 'serious' || v.impact === 'critical');
      check(`${name} ${width} axe`, serious.length === 0, JSON.stringify(serious.map(v => v.id)));
    }
    await page.screenshot({ path: join(out, `${name}-${width}.png`) });
  }
  await ctx.close();
}
await browser.close();
console.log(failures.length ? `${failures.length} checks failed` : 'all checks passed');
process.exit(failures.length ? 1 : 0);
