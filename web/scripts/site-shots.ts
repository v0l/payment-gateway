import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from 'playwright-core';

const base = process.env.SITE_URL ?? 'http://127.0.0.1:18861';
const out = process.env.SHOTS_DIR ?? '/tmp/site-shots';
const only = process.env.ONLY ? process.env.ONLY.split(',') : null;
mkdirSync(out, { recursive: true });

const pages: [string, string][] = [
  ['landing', '/'],
  ['guide-index', '/guide/'],
  ['guide-webhooks', '/guide/webhooks'],
  ['guide-errors', '/guide/errors'],
  ['guide-quickstart-api', '/guide/quickstart-api'],
  ['guide-reference', '/guide/reference'],
  ['operator-index', '/operator/'],
  ['operator-treasury', '/operator/treasury'],
  ['legal-terms', '/legal/terms'],
  ['legal-index', '/legal/'],
  ['docs', '/docs'],
  ['styleguide', '/console/styleguide'],
];

const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/usr/bin/google-chrome', args: ['--no-sandbox'] });
const errors: string[] = [];
try {
  for (const width of [1440, 390]) {
    const ctx = await browser.newContext({ viewport: { width, height: width > 600 ? 900 : 844 }, locale: 'en-IE', colorScheme: 'light', isMobile: width < 600 });
    const page = await ctx.newPage();
    page.on('pageerror', e => errors.push(`${width} ${e.message}`));
    page.on('console', m => {
      if (m.type() === 'error') errors.push(`${width} ${m.text()}`);
    });
    for (const [name, path] of pages) {
      if (only && !only.includes(name)) continue;
      await page.goto(base + path);
      await page.waitForLoadState('networkidle');
      await page.evaluate(() => document.fonts.ready);
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(out, `${name}-${width}.png`), fullPage: true });
    }
    await ctx.close();
  }
} finally {
  await browser.close();
}
console.log(errors.length ? `errors:\n${errors.join('\n')}` : 'no console errors');
process.exit(errors.length ? 1 : 0);
