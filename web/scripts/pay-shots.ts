import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { chromium, type Page } from 'playwright-core';

const base = process.env.DEMO_URL ?? 'http://127.0.0.1:18861';
const admin = process.env.DEMO_ADMIN ?? 'demo-admin-token-demo-admin-token';
const out = process.env.SHOTS_DIR ?? '/tmp/shots';
mkdirSync(out, { recursive: true });

async function call<T>(method: string, url: string, token: string, body?: unknown): Promise<T> {
  const r = await fetch(base + url, {
    method,
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  });
  const json = (await r.json().catch(() => ({}))) as T;
  if (!r.ok) throw new Error(`${method} ${url}: ${r.status} ${JSON.stringify(json)}`);
  return json;
}

interface Tenant {
  id: number;
  api_key: string;
}
interface Payment {
  id: string;
}

const tenant = await call<Tenant>('POST', '/admin/tenants', admin, { name: 'Shots Shop ' + Date.now() });
await call('POST', `/admin/tenants/${tenant.id}/branding`, admin, {
  thanks_text: 'Thanks, your order is on its way.',
  footer_links: [{ label: 'Terms', url: 'https://shop.example/terms' }],
});
for (const method of ['lightning', 'onchain']) {
  await call('POST', `/admin/tenants/${tenant.id}/providers`, admin, { method, provider_type: 'mock', name: method, config: { min_confirmations: 3 } });
}
const newPayment = (ref: string, extra: object = {}) =>
  call<Payment>('POST', '/v1/payments', tenant.api_key, { reference: ref, amount: 1500, currency: 'EUR', description: 'Order 1042', ...extra });

const freeze = `.countdown, img.qr { visibility: hidden !important } code, .brand .name { color: transparent !important }`;

async function settle(page: Page) {
  await page.waitForSelector('.card', { timeout: 8000 });
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(400);
  await page.addStyleTag({ content: freeze });
}

const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/usr/bin/google-chrome', args: ['--no-sandbox'] });
const errors: string[] = [];
const lineItems = [
  { name: 'Hosting', quantity: 2, unit_amount: 500 },
  { name: 'Setup', quantity: 1, unit_amount: 500 },
];
for (const width of [1440, 390]) {
  const choose = await newPayment('shot-choose');
  const summary = await newPayment('shot-summary', { amount: undefined, line_items: lineItems });
  const pending = await newPayment('shot-ln');
  const onchain = await newPayment('shot-chain');
  const cancelled = await newPayment('shot-cancel');
  await call('POST', `/v1/payments/${cancelled.id}/cancel`, tenant.api_key).catch(() => {});
  const ctx = await browser.newContext({ viewport: { width, height: width > 600 ? 900 : 844 }, locale: 'en-IE', timezoneId: 'Europe/Dublin', colorScheme: 'light', bypassCSP: true });
  const page = await ctx.newPage();
  page.on('pageerror', e => errors.push(`${width} ${e.message}`));
  page.on('console', m => {
    if (m.type() === 'error') errors.push(`${width} ${m.text()}`);
  });
  const shot = async (name: string, url: string, then?: (p: Page) => Promise<void>) => {
    await page.goto(base + url);
    await settle(page);
    if (then) {
      await then(page);
      await page.waitForTimeout(400);
      await page.addStyleTag({ content: freeze });
    }
    await page.screenshot({ path: join(out, `pay-${name}-${width}.png`) });
  };
  await shot('choose', `/pay/${choose.id}`);
  await shot('summary', `/pay/${summary.id}`);
  await shot('choose-fr', `/pay/${choose.id}?lang=fr`);
  await shot('cancelled', `/pay/${cancelled.id}`);
  await shot('lightning', `/pay/${pending.id}`, async p => {
    await p.click('button.method:has-text("Lightning")');
    await p.waitForSelector('.copy');
  });
  await shot('onchain', `/pay/${onchain.id}`, async p => {
    await p.click('button.method:has-text("Bitcoin")');
    await p.waitForSelector('.copy');
  });
  await shot('embed', `/embed/${choose.id}`);
  await ctx.close();
}
await browser.close();
console.log(errors.length ? `errors:\n${errors.join('\n')}` : 'no console errors');
process.exit(errors.length ? 1 : 0);
