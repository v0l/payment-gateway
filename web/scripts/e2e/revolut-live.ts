import { execSync, spawn } from 'node:child_process';
import { createHmac } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import { join } from 'node:path';
import { chromium, type Browser, type Frame } from 'playwright-core';

const root = join(import.meta.dirname, '..', '..', '..');
const out = process.env.LIVE_OUT ?? join(os.tmpdir(), 'revolut-live');
mkdirSync(out, { recursive: true });

const env: Record<string, string> = Object.fromEntries(
  readFileSync(join(os.homedir(), '.config/payment-gateway/revolut-sandbox.env'), 'utf8')
    .split('\n')
    .filter(l => l.includes('='))
    .map(l => [l.slice(0, l.indexOf('=')).trim(), l.slice(l.indexOf('=') + 1).trim()]),
);
const TOKEN = env.REVOLUT_SANDBOX_TOKEN;
const PUBLIC_KEY = env.REVOLUT_SANDBOX_PUBLIC_KEY;
const REVOLUT = 'https://sandbox-merchant.revolut.com';
const ADMIN = 'live-admin-token-live-admin-token';
const PORT = 18760;
const gateway = `http://127.0.0.1:${PORT}`;
const database = process.env.TEST_DATABASE_URL;
if (!database) throw new Error('set TEST_DATABASE_URL');

interface Reply {
  status: number;
  json: Record<string, any> | null;
}

async function revolut(method: string, p: string, body?: unknown): Promise<Reply> {
  const r = await fetch(REVOLUT + p, {
    method,
    headers: { authorization: `Bearer ${TOKEN}`, 'revolut-api-version': '2024-09-01', 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await r.text();
  let json: Record<string, any> | null = null;
  try {
    json = JSON.parse(text);
  } catch {}
  return { status: r.status, json };
}

async function api<T = any>(key: string, method: string, url: string, body?: unknown): Promise<T> {
  const r = await fetch(gateway + url, {
    method,
    headers: { authorization: `Bearer ${key}`, 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  });
  const json = (await r.json().catch(() => ({}))) as T;
  if (!r.ok) throw new Error(`${method} ${url}: ${r.status} ${JSON.stringify(json)}`);
  return json;
}

const save = (name: string, value: unknown) => writeFileSync(join(out, name), JSON.stringify(value, null, 2));
const sleep = (ms: number) => new Promise(r => setTimeout(r, ms));

const dbName = `pgw_live_${Date.now()}`;
const base = database.replace(/\/[^/]*$/, '/');
execSync(`docker exec pgw-test-db mariadb -uroot -ptest -e "CREATE DATABASE ${dbName}"`);
const child = spawn(join(process.env.CARGO_TARGET_DIR ?? join(root, 'target'), 'debug/payment-gateway'), [], {
  env: {
    ...process.env,
    DATABASE_URL: base + dbName,
    GATEWAY_ENCRYPTION_KEY: '22'.repeat(32),
    GATEWAY_ADMIN_TOKEN: ADMIN,
    GATEWAY_PUBLIC_URL: gateway,
    GATEWAY_LISTEN: `127.0.0.1:${PORT}`,
    LOG_LEVEL: 'warn',
    LOG_FORMAT: 'text',
    DB_MAX_CONNECTIONS: '4',
  },
  stdio: ['ignore', 'inherit', 'inherit'],
});
let hookId: string | null = null;
let browser: Browser | null = null;
const cleanup = { orders: [] as string[], customers: [] as string[] };
const lastAttemptLookup = () => execSync(`docker exec pgw-test-db mariadb -N -uroot -ptest ${dbName} -e "select lookup from payment_attempt order by id desc limit 1"`).toString().trim();

try {
  for (let i = 0; i < 300; i++) {
    try {
      if ((await fetch(`${gateway}/favicon.svg`)).ok) break;
    } catch {}
    await sleep(200);
  }
  const hook = await revolut('POST', '/api/1.0/webhooks', { url: 'https://example.invalid/live-test-hook', events: ['ORDER_COMPLETED', 'ORDER_FAILED', 'ORDER_CANCELLED'] });
  hookId = hook.json!.id;
  const secret: string = hook.json!.signing_secret;

  const tenant = await api<{ id: number; api_key: string }>(ADMIN, 'POST', '/admin/tenants', { name: 'Live Revolut', webhook_url: 'http://127.0.0.1:9/hook' });
  const prov = await api<{ id: number }>(ADMIN, 'POST', `/admin/tenants/${tenant.id}/providers`, {
    method: 'revolut',
    provider_type: 'revolut',
    name: 'sandbox',
    config: { url: REVOLUT, token: TOKEN, api_version: '2024-09-01', public_key: PUBLIC_KEY, webhook_secret: secret },
  });
  console.log('provider', JSON.stringify({ id: prov.id }));

  const sendHook = async (event: string, orderId: string) => {
    const body = JSON.stringify({ event, order_id: orderId, merchant_order_ext_ref: null });
    const ts = Date.now().toString();
    const sig = `v1=${createHmac('sha256', secret).update(`v1.${ts}.${body}`).digest('hex')}`;
    const r = await fetch(`${gateway}/hook/${tenant.id}/${prov.id}`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'revolut-signature': sig, 'revolut-request-timestamp': ts },
      body,
    });
    console.log('hook', event, r.status);
  };

  const customerRef = `live-${Date.now()}`;
  const payment = await api<{ id: string }>(tenant.api_key, 'POST', '/v1/payments', {
    reference: customerRef,
    amount: 1500,
    currency: 'EUR',
    customer_ref: customerRef,
    customer_email: `${customerRef}@example.com`,
  });
  const view = await api<{ attempts: { details?: { widget?: unknown } }[] }>(tenant.api_key, 'POST', `/v1/payments/${payment.id}/attempts`, { method: 'revolut', save_method: true });
  const attempt = view.attempts[0];
  console.log('attempt keys:', Object.keys(attempt).join(','), 'widget:', Boolean(attempt.details?.widget));
  const orderId = lastAttemptLookup();

  browser = await chromium.launch({ headless: true, executablePath: process.env.CHROME_PATH });
  const page = await browser.newPage({ viewport: { width: 1000, height: 900 } });
  page.on('console', m => {
    if (m.type() === 'error') console.log('page error:', m.text().slice(0, 200));
  });
  await page.goto(`${gateway}/pay/${payment.id}`);
  await page.getByText('Revolut', { exact: false }).first().click({ timeout: 5000 }).catch(() => {});
  await page.getByRole('button', { name: 'Pay by card and save it' }).waitFor({ timeout: 15000 });
  await page.screenshot({ path: join(out, '1-pay.png') });
  const popupPromise = page.context().waitForEvent('page', { timeout: 8000 }).catch(() => null);
  await page.getByRole('button', { name: 'Pay by card and save it' }).click();
  await page.waitForTimeout(6000);
  await page.screenshot({ path: join(out, '2-widget.png') });
  const popup = await popupPromise;
  console.log('frames:', page.frames().map(f => new URL(f.url() || 'about:blank').host).join(','), 'popup:', Boolean(popup));
  const target = popup ?? page;
  const frames = target.frames().filter(f => f !== target.mainFrame());
  console.log('target frames:', frames.map(f => f.url().slice(0, 80)).join(' | '));

  const card: Frame | undefined = frames.find(f => f.url().includes('card-popup'));
  let filled = false;
  if (card) {
    await card.locator('input[name="number"]').fill('4929420573595709');
    await card.locator('input[name="expiry"]').fill('12/30');
    await card.locator('input[name="code"]').fill('123');
    await card.locator('input[name="postcode"]').fill('D01 F5P2');
    await card.locator('input[name="customer.name"]').fill('Test Person');
    const box = card.locator('input[name="save-card-for-customer"]');
    if (!(await box.isChecked())) await box.check({ force: true }).catch(() => {});
    console.log('save box checked:', await box.isChecked());
    filled = true;
  }
  await target.screenshot({ path: join(out, '3-filled.png') });
  console.log('filled:', filled);
  if (filled && card) {
    await card.locator('button[type=submit]').click();
    await page.waitForTimeout(15000);
    await page.screenshot({ path: join(out, '4-after.png') });
  }

  const order = (await revolut('GET', `/api/orders/${orderId}`)).json!;
  console.log('revolut order state:', order.state, 'save_payment_method_for:', order.save_payment_method_for, 'customer:', order.customer && 'present');
  save('order_after_widget.json', order);
  if (order.state === 'completed' || order.state === 'authorised') await sendHook('ORDER_COMPLETED', orderId);
  await sleep(1500);
  const paid = await api<{ status: string }>(tenant.api_key, 'GET', `/v1/payments/${payment.id}`);
  console.log('gateway payment status:', paid.status);
  const saved = await api<{ id: number }[]>(tenant.api_key, 'GET', `/v1/saved-methods?customer_ref=${customerRef}`);
  console.log('gateway saved-methods:', JSON.stringify(saved));
  const custId: string | undefined = order.customer?.id;
  if (custId) {
    cleanup.customers.push(custId);
    const pm = await revolut('GET', `/api/customers/${custId}/payment-methods`);
    console.log('revolut payment-methods:', JSON.stringify(pm.json));
    save('customer_payment_methods.json', pm.json);
  }
  cleanup.orders.push(orderId);

  if (saved.length) {
    const charge = await api(tenant.api_key, 'POST', `/v1/saved-methods/${saved[0].id}/charge`, { reference: `${customerRef}-2`, amount: 700, currency: 'EUR' });
    console.log('charge status:', charge.status, JSON.stringify(charge).slice(0, 300));
    save('charge_response.json', charge);
    const chargeOrder = lastAttemptLookup();
    if (chargeOrder) {
      cleanup.orders.push(chargeOrder);
      save('charge_order.json', (await revolut('GET', `/api/orders/${chargeOrder}`)).json);
    }
  }
} catch (e) {
  console.log('ERROR', e instanceof Error ? e.message : String(e));
} finally {
  if (browser) await browser.close();
  child.kill();
  if (hookId) console.log('webhook cleanup', (await revolut('DELETE', `/api/1.0/webhooks/${hookId}`)).status);
  for (const c of cleanup.customers) console.log('customer delete', (await revolut('DELETE', `/api/customers/${c}`)).status);
  for (const o of cleanup.orders) console.log('order cancel', (await revolut('POST', `/api/orders/${o}/cancel`)).status);
  execSync(`docker exec pgw-test-db mariadb -uroot -ptest -e "DROP DATABASE ${dbName}"`);
}
