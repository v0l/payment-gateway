import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from 'playwright-core';

const mode = process.argv[2] === 'old' ? 'old' : 'new';
const base = process.env.SITE_URL ?? 'http://127.0.0.1:18862';
const out = process.env.SHOTS_DIR ?? `/tmp/sandbox-${mode}`;
const baseline = process.env.BASELINE_REV ?? '18d88ce';
const root = join(import.meta.dirname, '..', '..');
mkdirSync(out, { recursive: true });

const old = (path: string) => execFileSync('git', ['show', `${baseline}:${path}`], { cwd: root }).toString();

const node = (pub: string) => ({ pubkey: pub.repeat(66).slice(0, 66), synced: true, height: 210, onchain_sats: 1_000_000, unconfirmed_sats: 5000, active_channels: 1 });
const status = {
  ready: true,
  chain: { ok: true, value: { height: 210, miner_sats: 5_000_000_000 } },
  gateway: { ok: true, value: node('a') },
  customer: { ok: true, value: node('b') },
  channels: { ok: true, value: [{ active: true, capacity_sats: 500_000, local_sats: 250_000, remote_sats: 250_000 }] },
};
const payments = [
  { id: 'aaaaaaaa11112222', reference: 'sandbox-order', amount: 1500, currency: 'EUR', status: 'pending', pay_url: '/pay/aaaaaaaa', attempts: [{ method: 'lightning', status: 'pending', details: { bolt11: `lnbcrt${'q'.repeat(60)}` } }] },
  { id: 'bbbbbbbb33334444', reference: 'paid-order', amount: 900, currency: 'EUR', status: 'paid', pay_url: '/pay/bbbbbbbb', attempts: [{ method: 'onchain', status: 'paid', details: { address: 'bcrt1qexampleaddress' } }] },
];
const hooks = [{ received: '2026-11-20T12:00:00Z', event_type: 'payment.paid', payment_id: 'bbbbbbbb33334444', status: '200', signature_valid: true, body: '{"type":"payment.paid"}' }];

const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/usr/bin/google-chrome', args: ['--no-sandbox'] });
const errors: string[] = [];
try {
  for (const width of [1440, 390]) {
    const ctx = await browser.newContext({ viewport: { width, height: 900 }, locale: 'en-IE', timezoneId: 'Europe/Dublin', colorScheme: 'light', bypassCSP: true });
    await ctx.addInitScript(() => sessionStorage.setItem('pgw-admin', 'token'));
    const page = await ctx.newPage();
    page.on('pageerror', e => errors.push(`${width} ${e.message}`));
    page.on('console', m => {
      if (m.type() === 'error') errors.push(`${width} ${m.text()}`);
    });
    await page.route('**/sandbox/api/status', r => r.fulfill({ json: status }));
    await page.route('**/sandbox/api/payments', r => r.fulfill({ json: payments }));
    await page.route('**/sandbox/api/hooks', r => r.fulfill({ json: hooks }));
    if (mode === 'old') {
      await page.route('**/sandbox', r => r.fulfill({ contentType: 'text/html', body: old('web/sandbox.html') }));
      await page.route('**/sandbox/app.js', r => r.fulfill({ contentType: 'text/javascript', body: old('web/sandbox.js') }));
      await page.route('**/sandbox/app.css', r => r.fulfill({ contentType: 'text/css', body: old('web/sandbox.css') }));
      await page.route('**/assets/tokens.css', r => r.fulfill({ contentType: 'text/css', body: old('web/tokens.css') }));
    } else {
      await page.route('**/sandbox', r => r.fulfill({ contentType: 'text/html', body: readFileSync(join(root, 'web', 'dist', 'sandbox.html'), 'utf8') }));
    }
    await page.goto(`${base}/sandbox`);
    await page.waitForSelector('#payments table');
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(300);
    await page.screenshot({ path: join(out, `sandbox-${width}.png`), fullPage: true });
    await ctx.close();
  }
} finally {
  await browser.close();
}
console.log(errors.length ? `errors:\n${errors.join('\n')}` : 'no console errors');
process.exit(errors.length ? 1 : 0);
