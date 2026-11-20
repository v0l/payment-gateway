import { execSync } from 'node:child_process';
import { createHmac } from 'node:crypto';
import { readFileSync, mkdirSync } from 'node:fs';
import os from 'node:os';
import { join } from 'node:path';
import { chromium, type Frame, type Page } from 'playwright-core';
import { admin, call, check, databaseName, failures, gateway, startGateway } from './gateway';

const env = Object.fromEntries(
  readFileSync(join(os.homedir(), '.config/payment-gateway/revolut-sandbox.env'), 'utf8')
    .split('\n')
    .filter(l => l.includes('='))
    .map(l => [l.slice(0, l.indexOf('=')).trim(), l.slice(l.indexOf('=') + 1).trim()]),
);
const REVOLUT = 'https://sandbox-merchant.revolut.com';
const shots = process.env.SHOT_DIR ?? join(os.tmpdir(), 'revolut-inline');
mkdirSync(shots, { recursive: true });

const SECRET = 'x'.repeat(16);
const PLAIN_CARD = '4929420573595709';
const THREE_DS_CARD = process.env.REVOLUT_3DS_CARD ?? '5555550079553105';

async function cancelOrder(id: string): Promise<void> {
  await fetch(`${REVOLUT}/api/orders/${id}/cancel`, {
    method: 'POST',
    headers: { authorization: `Bearer ${env.REVOLUT_SANDBOX_TOKEN}`, 'revolut-api-version': '2024-09-01' },
  });
}

async function cardFrame(page: Page): Promise<Frame> {
  for (let i = 0; i < 60; i++) {
    const f = page.frames().find(x => x.url().includes('card-field'));
    if (f && (await f.locator('input[name="number"]').count())) return f;
    await page.waitForTimeout(500);
  }
  throw new Error('card field never appeared');
}

function latestOrder(): string {
  return execSync(`docker exec pgw-test-db mariadb -N -uroot -ptest ${databaseName} -e "select lookup from payment_attempt order by id desc limit 1"`).toString().trim();
}

async function orderCompleted(id: string): Promise<void> {
  for (let i = 0; i < 60; i++) {
    const o = await fetch(`${REVOLUT}/api/orders/${id}`, { headers: { authorization: `Bearer ${env.REVOLUT_SANDBOX_TOKEN}`, 'revolut-api-version': '2024-09-01' } }).then(r => r.json());
    if (o.state === 'completed') return;
    await new Promise(r => setTimeout(r, 1000));
  }
}

async function challenge(page: Page): Promise<boolean> {
  for (let i = 0; i < 240; i++) {
    const f = page.frames().find(x => x.url().includes('/challenge'));
    if (f) {
      const btn = f.getByRole('button', { name: 'Continue' });
      await btn.waitFor({ state: 'visible' });
      await btn.click();
      return true;
    }
    await page.waitForTimeout(500);
  }
  return false;
}

const child = await startGateway();
let providerId = 0;
const orders: string[] = [];
const urls = new Set<string>();
try {
  const tenant = await admin<{ id: number; api_key: string }>('POST', '/admin/tenants', { name: 'Inline Shop', webhook_url: 'http://127.0.0.1:9/hook' });
  const provider = await admin<{ id: number }>('POST', `/admin/tenants/${tenant.id}/providers`, {
    method: 'revolut',
    provider_type: 'revolut',
    name: 'sandbox',
    config: { url: REVOLUT, token: env.REVOLUT_SANDBOX_TOKEN, api_version: '2024-09-01', public_key: env.REVOLUT_SANDBOX_PUBLIC_KEY, webhook_secret: SECRET },
  });
  providerId = provider.id;
  await admin('POST', `/admin/tenants/${tenant.id}/settings`, { allowed_origins: [] });
  const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROME_PATH });

  const run = async (name: string, card: string, opts: { save?: boolean; viewport?: { width: number; height: number };  }) => {
    const ref = `${name}-${Date.now()}`;
    const payment = await call<{ id: string }>(tenant.api_key, 'POST', '/v1/payments', {
      reference: ref,
      amount: 1500,
      currency: 'EUR',
      customer_ref: ref,
      customer_email: `${ref}@example.com`,
    });
    const page = await browser.newPage({ viewport: opts.viewport ?? { width: 1000, height: 900 } });
    page.on('framenavigated', f => {
      if (f === page.mainFrame()) urls.add(f.url());
    });
    await page.goto(`${gateway}/pay/${payment.id}`);
    if (opts.save !== false) await page.getByLabel('Save this card for future payments').check();
    await page.locator('button.method').first().click();
    const frame = await cardFrame(page);
    await page.getByLabel('Name on card').fill('Test Person');
    await frame.locator('input[name="number"]').fill(card);
    await frame.locator('input[name="expiry"]').fill('12/30');
    await frame.locator('input[name="code"]').fill('123');
    await frame.locator('input[name="postcode"]').fill('SW1A 1AA');
    await page.screenshot({ path: join(shots, `${name}-filled.png`), fullPage: true });
    await page.waitForTimeout(1500);
    orders.push(latestOrder());
    await page.getByRole('button', { name: 'Pay by card' }).click();
    if (name === '3ds') {
      check('3ds challenge shown inside the page', await challenge(page));
      check('3ds never left the pay page', [...urls].every(u => u.startsWith(gateway)), [...urls].join(','));
      await page.close();
      return;
    }
    let paid = false;
    for (let i = 0; i < 120 && !paid; i++) {
      paid = await page.getByText('Payment seen on the network').count().then(c => c > 0);
      if (!paid) await page.waitForTimeout(1000);
    }
    if (!paid) {
      const o = await fetch(`${REVOLUT}/api/orders/${orders[orders.length - 1]}`, { headers: { authorization: `Bearer ${env.REVOLUT_SANDBOX_TOKEN}`, 'revolut-api-version': '2024-09-01' } }).then(r => r.json());
      console.log(name, 'order', o.state, JSON.stringify(o.payments?.map((p: { state: string }) => p.state) ?? null));
      const fr = await cardFrame(page);
      console.log(name, 'field', (await fr.locator('body').innerText().catch(() => '')).replace(/\n/g, ' | ').slice(0, 200));
      console.log(name, 'button', await page.locator('button.primary').first().innerText().catch(() => ''));
    }
    await page.screenshot({ path: join(shots, `${name}-after.png`), fullPage: true });
    check(`${name} success shown inline`, paid);
    check(`${name} never left the pay page`, [...urls].every(u => u.startsWith(gateway)), [...urls].join(','));
    if (opts.viewport) {
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
      check(`${name} has no horizontal scroll`, !overflow);
    }
    if (opts.save !== false) {
      const body = JSON.stringify({ event: 'ORDER_COMPLETED', order_id: orders[orders.length - 1], merchant_order_ext_ref: null });
      await orderCompleted(orders[orders.length - 1]);
      const ts = Date.now().toString();
      const sig = `v1=${createHmac('sha256', SECRET).update(`v1.${ts}.${body}`).digest('hex')}`;
      await fetch(`${gateway}/hook/${tenant.id}/${providerId}`, { method: 'POST', headers: { 'content-type': 'application/json', 'revolut-signature': sig, 'revolut-request-timestamp': ts }, body });
      let saved: unknown[] = [];
      for (let i = 0; i < 10 && !saved.length; i++) {
        saved = await call<unknown[]>(tenant.api_key, 'GET', `/v1/saved-methods?customer_ref=${ref}`).catch(() => []);
        if (!saved.length) await page.waitForTimeout(1000);
      }
      check(`${name} card listed under customer_ref`, saved.length > 0);
    }
    await page.close();
  };


  await run('plain', PLAIN_CARD, { save: false });
  await run('3ds', THREE_DS_CARD, { save: false });
  await run('save', PLAIN_CARD, {});
  await run('mobile', PLAIN_CARD, { viewport: { width: 390, height: 844 }, save: false });

  await browser.close();
} finally {
  for (const o of orders) await cancelOrder(o);
  child.kill();
}
if (failures.length) {
  console.log(`${failures.length} failed`);
  process.exit(1);
}
