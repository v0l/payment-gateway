import { readFileSync } from 'node:fs';
import os from 'node:os';
import { join } from 'node:path';
import { chromium } from 'playwright-core';

const env = Object.fromEntries(
  readFileSync(join(os.homedir(), '.config/payment-gateway/revolut-sandbox.env'), 'utf8')
    .split('\n')
    .filter(l => l.includes('='))
    .map(l => [l.slice(0, l.indexOf('=')).trim(), l.slice(l.indexOf('=') + 1).trim()]),
);
const REVOLUT = 'https://sandbox-merchant.revolut.com';
const headers = { authorization: `Bearer ${env.REVOLUT_SANDBOX_TOKEN}`, 'revolut-api-version': '2024-09-01', 'content-type': 'application/json' };

const order = (await fetch(`${REVOLUT}/api/orders`, { method: 'POST', headers, body: JSON.stringify({ amount: 100, currency: 'EUR' }) }).then(r => r.json())) as { id: string; checkout_url: string };
const browser = await chromium.launch({ headless: true, executablePath: process.env.CHROME_PATH });
let result = 'SANDBOX_NO_SUBMIT';
try {
  const page = await browser.newPage({ viewport: { width: Number(process.env.VW ?? 1000), height: 900 } });
  const submitted = new Promise<string>(resolve => {
    page.on('response', async r => {
      if (!r.url().endsWith('/payments') || r.request().method() !== 'POST') return;
      const body = await r.text().catch(() => '');
      const errorId = /"errorId":"([^"]+)"/.exec(body)?.[1] ?? '';
      resolve(r.ok() ? 'SANDBOX_OK' : `SANDBOX_${r.status()} ${errorId}`);
    });
  });
  await page.goto(order.checkout_url);
  await page.getByRole('button', { name: 'Pay with card' }).click({ timeout: 30000 });
  const deadline = Date.now() + 30000;
  let field = page.frames().find(f => f.url().includes('card-field'));
  while (!field && Date.now() < deadline) {
    await page.waitForTimeout(500);
    field = page.frames().find(f => f.url().includes('card-field'));
  }
  if (!field) throw new Error('card field never appeared');
  await field.locator('input[name="number"]').fill('4929420573595709');
  await field.locator('input[name="expiry"]').fill('12/30');
  await field.locator('input[name="code"]').fill('123');
  await page.getByPlaceholder('Cardholder name').fill('Probe Person');
  await page.getByPlaceholder('Email address').fill('probe@example.com');
  await page.locator('button[type=submit][form]').click();
  result = await Promise.race([submitted, page.waitForTimeout(30000).then(() => 'SANDBOX_NO_SUBMIT')]);
  if (result === 'SANDBOX_OK') {
    result = 'SANDBOX_STUCK_PENDING';
    for (let i = 0; i < 60; i++) {
      const o = (await fetch(`${REVOLUT}/api/orders/${order.id}`, { headers }).then(r => r.json())) as { state: string };
      if (o.state === 'completed' || o.state === 'authorised') {
        result = 'SANDBOX_OK';
        break;
      }
      await page.waitForTimeout(2000);
    }
  }
} catch (e) {
  result = `SANDBOX_PROBE_ERROR ${(e as Error).message.split('\n')[0]}`;
  await browser.contexts()[0]?.pages()[0]?.screenshot({ path: '/tmp/sandbox-probe-error.png' }).catch(() => {});
} finally {
  await browser.close();
  const o = (await fetch(`${REVOLUT}/api/orders/${order.id}`, { headers }).then(r => r.json())) as { state: string };
  if (o.state !== 'completed') await fetch(`${REVOLUT}/api/orders/${order.id}/cancel`, { method: 'POST', headers });
}
console.log(new Date().toISOString(), result);
