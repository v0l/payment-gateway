import { createServer, type Server } from 'node:http';
import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium, type Browser, type BrowserContextOptions, type Page } from 'playwright-core';
import { PORT, call, check, failures, gateway, startGateway, admin } from './gateway';

const web = join(import.meta.dirname, '..', '..');
const root = join(web, '..');
const axeSource = readFileSync(join(web, 'node_modules', 'axe-core', 'axe.min.js'), 'utf8');
const shots = join(root, 'docs/design/screenshots');
mkdirSync(shots, { recursive: true });

const HOST_PORT = PORT + 1;
const hostOrigin = `http://localhost:${HOST_PORT}`;

function hostServer(): Promise<Server> {
  const page = `<!doctype html><html><body><button id="open">pay</button><div id="pay"></div>
<script src="${gateway}/gateway.js"></script></body></html>`;
  const server = createServer((_req, res) => {
    res.setHeader('content-type', 'text/html');
    res.end(page);
  });
  return new Promise(resolve => server.listen(HOST_PORT, '127.0.0.1', () => resolve(server)));
}

interface AxeViolation {
  id: string;
  impact: string;
  nodes: { target: unknown }[];
}
interface AxeWindow {
  axe: { run(doc: Document, opts: object): Promise<{ violations: AxeViolation[] }> };
}

async function axeScan(page: Page, label: string): Promise<void> {
  await page.evaluate(axeSource);
  const result = await page.evaluate(() => (window as unknown as AxeWindow).axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'wcag21aa', 'best-practice'] }));
  const serious = result.violations.filter(v => v.impact === 'serious' || v.impact === 'critical');
  check(`axe ${label}`, serious.length === 0, JSON.stringify(serious.map(v => [v.id, v.nodes.map(n => n.target)])));
}

async function noHorizontalScroll(page: Page, label: string): Promise<void> {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
  check(`no horizontal scroll ${label}`, !overflow);
}

interface Tenant {
  id: number;
  api_key: string;
}

async function setup(): Promise<Tenant> {
  const tenant = await admin<Tenant>('POST', '/admin/tenants', { name: 'E2E Shop', webhook_url: 'http://127.0.0.1:9/hook' });
  await admin('POST', `/admin/tenants/${tenant.id}/settings`, { allowed_origins: [hostOrigin] });
  await admin('POST', `/admin/tenants/${tenant.id}/branding`, {
    thanks_text: 'Thanks, your order is on its way.',
    footer_links: [{ label: 'Terms', url: 'https://shop.example/terms' }],
  });
  for (const method of ['lightning', 'onchain']) {
    await admin('POST', `/admin/tenants/${tenant.id}/providers`, { method, provider_type: 'mock', name: method, config: { min_confirmations: 3 } });
  }
  return tenant;
}

const newPayment = (tenant: Tenant, ref: string, extra: object = {}) =>
  call<{ id: string }>(tenant.api_key, 'POST', '/v1/payments', { reference: ref, amount: 1500, currency: 'EUR', ...extra });

type Events = { events: string[]; handle: { close(): void } };

async function embedChecks(browser: Browser, tenant: Tenant): Promise<void> {
  const ctx = await browser.newContext({ viewport: { width: 900, height: 800 } });
  const page = await ctx.newPage();
  await page.goto(`${hostOrigin}/`);
  const pay = await newPayment(tenant, 'embed');
  await page.evaluate(
    ({ gateway, id }) => {
      const w = window as unknown as Events & { PaymentGateway: { open(o: object): { close(): void } } };
      w.events = [];
      w.handle = w.PaymentGateway.open({
        gateway,
        paymentId: id,
        container: '#pay',
        onReady: () => w.events.push('ready'),
        onPaid: () => w.events.push('paid'),
        onError: (e: { code: string }) => w.events.push(`error:${e.code}`),
        onClosed: () => w.events.push('closed'),
      });
    },
    { gateway, id: pay.id },
  );
  await page.waitForFunction(() => (window as unknown as Events).events.includes('ready'), null, { timeout: 15000 });
  check('embed reports ready from the gateway origin', true);
  await page.waitForFunction(() => document.querySelector('iframe')!.style.height.endsWith('px'));
  check('embed resizes its frame', true);

  await page.evaluate(id => {
    window.postMessage({ source: 'payment-gateway', type: 'pgw:paid', payment_id: id }, '*');
  }, pay.id);
  await page.waitForTimeout(300);
  check('message from the wrong origin or source is ignored', !(await page.evaluate(() => (window as unknown as Events).events.includes('paid'))));

  const unlisted = await browser.newContext();
  const rogue = await unlisted.newPage();
  await rogue.setContent(`<iframe id="f" src="${gateway}/embed/${pay.id}"></iframe>`);
  await rogue.waitForTimeout(800);
  const framed = await rogue.evaluate(() => {
    const f = document.getElementById('f') as HTMLIFrameElement;
    try {
      return !!(f.contentDocument && f.contentDocument.body && f.contentDocument.body.children.length > 0);
    } catch {
      return false;
    }
  });
  check('an unlisted origin cannot frame the embed or read it', !framed);
  await unlisted.close();

  await page.evaluate(() => (window as unknown as Events).handle.close());
  check('close fires onClosed and removes the frame', (await page.evaluate(() => (window as unknown as Events).events.includes('closed'))) && (await page.locator('iframe').count()) === 0);

  const pay2 = await newPayment(tenant, 'modal');
  await page.evaluate(
    ({ gateway, id }) => {
      const w = window as unknown as Events & { PaymentGateway: { open(o: object): { close(): void } } };
      w.events = [];
      w.handle = w.PaymentGateway.open({ gateway, paymentId: id, mode: 'modal', onReady: () => w.events.push('ready'), onClosed: () => w.events.push('closed') });
    },
    { gateway, id: pay2.id },
  );
  await page.waitForFunction(() => (window as unknown as Events).events.includes('ready'), null, { timeout: 15000 });
  check('modal has dialog role', (await page.locator('[role=dialog][aria-modal=true]').count()) === 1);
  await page.screenshot({ path: join(shots, 'embed-modal.png') });
  await page.keyboard.press('Escape');
  check('escape closes the modal', (await page.evaluate(() => (window as unknown as Events).events.includes('closed'))) && (await page.locator('[role=dialog]').count()) === 0);
  await ctx.close();
}

const child = await startGateway();
const host = await hostServer();
const browser = await chromium.launch({ executablePath: process.env.CHROME || undefined, args: ['--no-sandbox'] });
try {
  const tenant = await setup();
  const variants: [string, BrowserContextOptions][] = [
    ['light', { colorScheme: 'light', viewport: { width: 1024, height: 800 } }],
    ['dark', { colorScheme: 'dark', viewport: { width: 1024, height: 800 } }],
    ['mobile', { colorScheme: 'light', viewport: { width: 360, height: 740 }, isMobile: true }],
  ];
  for (const [name, opts] of variants) {
    const ctx = await browser.newContext({ ...opts, locale: 'en-IE' });
    const page = await ctx.newPage();
    const pay = await newPayment(tenant, `choose-${name}`);
    await page.goto(`${gateway}/pay/${pay.id}`);
    await page.waitForSelector('button.method');
    check(`html lang set ${name}`, (await page.getAttribute('html', 'lang')) === 'en');
    await axeScan(page, `${name} method list`);
    await noHorizontalScroll(page, `${name} method list`);
    await page.screenshot({ path: join(shots, `pay-methods-${name}.png`) });

    await page.click('button.method >> text=Lightning');
    await page.waitForSelector('img.qr');
    const alt = await page.getAttribute('img.qr', 'alt');
    check(`qr has alt text ${name}`, !!alt && alt.length > 5);
    check(`open in wallet link ${name}`, (await page.locator("a.primary[href^='lightning:']").count()) === 1);
    await axeScan(page, `${name} lightning`);
    await noHorizontalScroll(page, `${name} lightning`);
    const tap = await page.evaluate(() => Math.min(...[...document.querySelectorAll('button, a.primary, select')].map(e => e.getBoundingClientRect().height)));
    check(`tap targets at least 44px ${name}`, tap >= 44, String(tap));
    await page.screenshot({ path: join(shots, `pay-lightning-${name}.png`) });
    await ctx.close();
  }

  const ctx = await browser.newContext({ viewport: { width: 360, height: 740 }, locale: 'de-DE' });
  const page = await ctx.newPage();
  const pay = await newPayment(tenant, 'lang');
  await page.goto(`${gateway}/pay/${pay.id}`);
  await page.waitForSelector('button.method');
  check('browser language picks german', (await page.getAttribute('html', 'lang')) === 'de');
  check('german amount format', ((await page.textContent('.amount')) ?? '').includes('15,00'));
  await page.goto(`${gateway}/pay/${pay.id}?lang=ga`);
  await page.waitForSelector('button.method');
  check('lang query overrides browser', (await page.getAttribute('html', 'lang')) === 'ga');
  await page.goto(`${gateway}/pay/${pay.id}?lang=fr`);
  await page.waitForSelector('button.method');
  await page.click('button.method >> nth=1');
  await page.waitForSelector('img.qr');
  await page.focus('select#lang');
  await page.keyboard.press('Tab');
  await page.screenshot({ path: join(shots, 'pay-onchain-fr-mobile.png') });
  check('onchain shows address', (await page.locator('code').count()) >= 2);
  await ctx.close();

  await embedChecks(browser, tenant);
} finally {
  await browser.close();
  host.close();
  child.kill();
}
if (failures.length) {
  console.error(`${failures.length} failed: ${failures.join(', ')}`);
  process.exit(1);
}
