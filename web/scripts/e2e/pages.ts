import { chromium } from 'playwright-core';
import { check, failures, gateway, startGateway, admin } from './gateway';

interface Tenant {
  id: number;
  api_key: string;
}

const child = await startGateway();
const browser = await chromium.launch({ executablePath: process.env.CHROME || undefined, args: ['--no-sandbox'] });
try {
  const tenant = await admin<Tenant>('POST', '/admin/tenants', { name: 'Pages Shop' });
  await admin('POST', `/admin/tenants/${tenant.id}/providers`, { method: 'lightning', provider_type: 'mock', name: 'ln', config: {} });
  const r = await fetch(`${gateway}/v1/payments`, {
    method: 'POST',
    headers: { authorization: `Bearer ${tenant.api_key}`, 'content-type': 'application/json' },
    body: JSON.stringify({ reference: 'pages-1', amount: 1500, currency: 'EUR' }),
  });
  const payment = (await r.json()) as { id: string };

  const pages: [string, string, string][] = [
    ['landing', '/', 'h1'],
    ['guide', '/guide/', 'h1'],
    ['guide page', '/guide/webhooks', 'article.doc'],
    ['operator', '/operator/', 'h1'],
    ['legal', '/legal/terms', 'article.doc'],
    ['api reference', '/docs', '.op'],
    ['pay page', `/pay/${payment.id}`, 'button.method'],
    ['embed page', `/embed/${payment.id}`, 'button.method'],
    ['console login', '/console', 'input[type=email]'],
    ['console reset', '/console/reset', 'input[type=email]'],
    ['styleguide', '/console/styleguide', '.theme-panel'],
  ];

  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await ctx.newPage();
  const problems: string[] = [];
  page.on('pageerror', e => problems.push(`pageerror ${e.message}`));
  const signedOutProbe = '/console/api/me';
  let probed = false;
  page.on('console', m => {
    if (m.type() !== 'error' && m.type() !== 'warning') return;
    if (probed && m.text().includes('401')) return;
    problems.push(`${m.type()} ${m.text()}`);
  });
  page.on('requestfailed', req => {
    if (req.failure()?.errorText === 'net::ERR_ABORTED' && req.url().endsWith('/events')) return;
    problems.push(`failed ${req.url()}`);
  });
  page.on('response', resp => {
    if (resp.url().endsWith(signedOutProbe) && resp.status() === 401) {
      probed = true;
      return;
    }
    if (resp.status() >= 400) problems.push(`http ${resp.status()} ${resp.url()}`);
  });
  await page.addInitScript(() => {
    document.addEventListener('securitypolicyviolation', e => {
      console.error(`CSP violation ${e.violatedDirective} ${e.blockedURI}`);
    });
  });
  for (const [name, path, selector] of pages) {
    problems.length = 0;
    probed = false;
    await page.goto(gateway + path);
    await page.waitForSelector(selector, { timeout: 10000 });
    await page.waitForLoadState('networkidle');
    check(`${name} loads clean`, problems.length === 0, JSON.stringify(problems));
  }
  await ctx.close();
} finally {
  await browser.close();
  child.kill();
}
if (failures.length) {
  console.error(`${failures.length} failed: ${failures.join(', ')}`);
  process.exit(1);
}
console.log('all pages loaded without console errors or CSP violations');
