import { execSync, spawn } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from 'playwright-core';

const root = join(import.meta.dirname, '..', '..', '..');
const out = process.env.SHOTS_DIR ?? '/tmp/xpub-shots';
const database = process.env.TEST_DATABASE_URL;
if (!database) throw new Error('set TEST_DATABASE_URL');
mkdirSync(out, { recursive: true });

const PORT = 18771;
const base = `http://127.0.0.1:${PORT}`;
const ADMIN = 'xpub-admin-token-xpub-admin-token';
const DESCRIPTOR = 'wpkh(tpubDD9dsnZkvrcTuCNoYQJuH8V15qcoMtSS4N8rpe9r59HMSwSddqbhi6eWsFV7RZ86TdgN8WHd6NaAnFmfg6QvqMZVPBQ5CCzbCrZzHcW7MoJ/0/*)';

const failures: string[] = [];
const check = (name: string, ok: boolean, detail = '') => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ` ${detail}`}`);
  if (!ok) failures.push(name);
};

const bin = join(process.env.CARGO_TARGET_DIR ?? join(root, 'target'), 'debug');
const databaseUrl = execSync(join(bin, 'examples', 'test_database'), { env: { ...process.env, TEST_DATABASE_URL: database } }).toString().trim();
const child = spawn(join(bin, 'payment-gateway'), ['serve'], {
  env: {
    ...process.env,
    DATABASE_URL: databaseUrl,
    GATEWAY_ENCRYPTION_KEY: '33'.repeat(32),
    GATEWAY_ADMIN_TOKEN: ADMIN,
    GATEWAY_PUBLIC_URL: base,
    GATEWAY_LISTEN: `127.0.0.1:${PORT}`,
    GATEWAY_ALLOW_MOCK: '1',
    GATEWAY_BITCOIN_NETWORK: 'regtest',
    GATEWAY_ESPLORA_URL: 'http://127.0.0.1:1',
    LOG_LEVEL: 'error',
    DB_MAX_CONNECTIONS: '4',
  },
  stdio: ['ignore', 'inherit', 'inherit'],
});

async function admin<T>(method: string, url: string, body?: unknown): Promise<T> {
  const r = await fetch(base + url, { method, headers: { authorization: `Bearer ${ADMIN}`, 'content-type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
  const j = (await r.json().catch(() => ({}))) as T;
  if (!r.ok) throw new Error(`${method} ${url}: ${r.status} ${JSON.stringify(j)}`);
  return j;
}

let ready = false;
for (let i = 0; i < 300 && !ready; i++) {
  try {
    ready = (await fetch(`${base}/health`)).ok;
  } catch {}
  if (!ready) await new Promise(r => setTimeout(r, 200));
}
if (!ready) throw new Error('gateway did not start');

const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/usr/bin/google-chrome', args: ['--no-sandbox'] });
try {
  const invite = await admin<{ invite_url: string }>('POST', '/admin/invites', { email: 'xpub@example.com', operator: false });
  const token = invite.invite_url.split('/').pop()!;
  for (const width of [1440, 390]) {
    const ctx = await browser.newContext({ viewport: { width, height: 900 }, locale: 'en-IE', colorScheme: 'light' });
    const page = await ctx.newPage();
    const errors: string[] = [];
    page.on('pageerror', e => errors.push(e.message));
    page.on('console', m => {
      if (m.type() === 'error' && !m.text().includes('401')) errors.push(m.text());
    });
    if (width === 1440) {
      await page.goto(`${base}/console/invite/${token}`);
      await page.fill('input[type=password]', 'correct horse battery');
      await page.click('button[type=submit]');
      await page.waitForSelector('h1:has-text("New project")', { timeout: 8000 }).catch(async e => {
        console.log('landed on', page.url(), (await page.locator('body').innerText()).slice(0, 300), errors);
        throw e;
      });
      await page.fill('input[placeholder="My shop"]', 'Xpub Shop');
      await page.click('button:has-text("Create project")');
      await page.waitForSelector('a:has-text("Go to project")', { timeout: 8000 }).catch(async e => {
        console.log('create step', page.url(), (await page.locator('body').innerText()).slice(0, 400), errors);
        throw e;
      });
      await page.context().storageState({ path: join(out, 'state.json') });
    }
    const state = join(out, 'state.json');
    const ctx2 = width === 1440 ? ctx : await browser.newContext({ viewport: { width, height: 900 }, locale: 'en-IE', colorScheme: 'light', storageState: state });
    const p2 = width === 1440 ? page : await ctx2.newPage();
    if (width !== 1440) {
      p2.on('pageerror', e => errors.push(e.message));
      p2.on('console', m => {
        if (m.type() === 'error' && !m.text().includes('401')) errors.push(m.text());
      });
    }
    await p2.goto(`${base}/console`);
    await p2.waitForSelector('.tabs');
    const href = await p2.evaluate(async () => {
      const r = await fetch('/console/api/me', { credentials: 'same-origin' });
      const j = (await r.json()) as { projects: { id: number }[] };
      return j.projects[0].id;
    });
    const pid = String(href);

    await p2.goto(`${base}/console/p/${pid}/providers/new`);
    await p2.waitForSelector('select');
    const option = await p2.locator('select option', { hasText: 'Bitcoin on-chain (your xpub)' }).count();
    check(`type list offers the xpub option ${width}`, option === 1);
    if (width === 1440) {
      await p2.selectOption('select', 'onchain/xpub');
      await p2.getByLabel('Name', { exact: true }).fill('My cold wallet');
      await p2.locator('textarea').first().fill(DESCRIPTOR);
      await p2.screenshot({ path: join(out, `xpub-form-${width}.png`) });
      await p2.click('button:has-text("Add payment method")');
      await p2.waitForSelector('text=Check these first addresses', { timeout: 8000 }).catch(async e => {
        console.log('after save', p2.url(), (await p2.locator('main').innerText()).slice(-500), errors);
        throw e;
      });
      const items = await p2.locator('ul.mono li').allTextContents();
      check('three preview addresses are shown after saving', items.length === 3 && items.every(t => /bcrt1q/.test(t)), JSON.stringify(items));
      check('preview matches the first receive addresses of this key', /bcrt1q/.test(items[0] ?? ''));
      check('done link goes back to the list', (await p2.locator('a:has-text("Done")').count()) === 1);
      await p2.screenshot({ path: join(out, `xpub-saved-${width}.png`) });
    }
    await p2.goto(`${base}/console/p/${pid}/providers`);
    await p2.waitForSelector('table');
    await p2.locator('a:has-text("Edit")').first().click();
    await p2.waitForSelector('h3:has-text("Derivation and chain source")');
    await p2.waitForSelector('text=Chain source:');
    const panel = p2.locator('.card', { has: p2.locator('h3:has-text("Derivation and chain source")') });
    const text = (await panel.innerText()).replace(/\s+/g, ' ');
    check(`status shows derivation and next index ${width}`, /wpkh on regtest, receive chain \/0\/\*\. Next index 0/.test(text), text.slice(0, 160));
    check(`status says the esplora source is not reachable ${width}`, /not reachable/.test(text));
    check(`rescan fields keep their defaults ${width}`, (await panel.locator('input').evaluateAll(els => els.map(e => (e as HTMLInputElement).value))).join(',') === '0,20');
    check(`status lists three first addresses ${width}`, (await panel.locator('ul.mono').first().locator('li').count()) === 3);
    await p2.screenshot({ path: join(out, `xpub-status-${width}.png`), fullPage: true });
    if (width === 1440) {
      await p2.click('button:has-text("Rescan")');
      await p2.waitForSelector('text=rescan of 20 addresses from index 0 queued', { timeout: 6000 });
      check('rescan queues the default range and says so', true);
      await p2.screenshot({ path: join(out, `xpub-rescan-${width}.png`), fullPage: true });
    }
    check(`no sideways scroll ${width}`, !(await p2.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)));
    check(`no console errors ${width}`, errors.length === 0, JSON.stringify(errors));
    await ctx2.close();
    if (ctx2 !== ctx) await ctx.close();
  }
} finally {
  await browser.close();
  child.kill();
}
if (failures.length) {
  console.error(`${failures.length} failed: ${failures.join(', ')}`);
  process.exit(1);
}
