import { mkdirSync, readFileSync, readdirSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { render } from 'preact-render-to-string';
import { Document, DocsShell, type Assets, type NavSection } from '../src/site/layout';
import { Landing, LANDING_DESCRIPTION } from '../src/site/Landing';
import { renderMarkdown } from '../src/site/markdown';
import { referenceMarkdown } from '../src/site/reference-md';

const web = join(import.meta.dirname, '..');
const docs = join(web, '..', 'docs-site');
const dist = join(web, 'dist');
const out = join(dist, 'site');
const check = process.argv.includes('--check');

interface ManifestEntry {
  file: string;
  css?: string[];
}
const manifest = JSON.parse(readFileSync(join(dist, 'manifest.json'), 'utf8')) as Record<string, ManifestEntry>;

const entry = (key: string): ManifestEntry => {
  const e = manifest[key];
  if (!e) throw new Error(`manifest has no ${key}, run vite build first`);
  return e;
};

const siteEntry = entry('src/site/site.ts');
const siteStyles = (siteEntry.css ?? []).map(f => `/static/${f}`);
const docsEntry = entry('src/site/docs.tsx');
const staticPath = (f: string) => `/static/${f}`;

const nav = (JSON.parse(readFileSync(join(docs, 'nav.json'), 'utf8')) as { sections: NavSection[] }).sections;

function expandIncludes(md: string): string {
  return md.replace(/^@include (\S+) (\S+)$/gm, (_, file: string, lang: string) => {
    const body = readFileSync(join(docs, file), 'utf8').replace(/\n$/, '');
    return `\`\`\`${lang} sample\n${body}\n\`\`\``;
  });
}

const problems: string[] = [];

function write(path: string, html: string): void {
  const text = `<!doctype html>${html}\n`;
  if (check) {
    const current = existsSync(path) ? readFileSync(path, 'utf8') : '';
    if (current !== text) problems.push(path);
    return;
  }
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text);
}

const baseAssets: Assets = { styles: siteStyles };
const titleOf = (t: string) => `${t} | Payment Gateway`;

write(
  join(out, 'landing.html'),
  render(
    <Document title="Payment Gateway" description={LANDING_DESCRIPTION} assets={baseAssets}>
      <Landing />
    </Document>,
  ),
);

const known = new Set<string>();
for (const section of nav) {
  for (const p of section.pages) {
    known.add(`${section.id}/${p.slug}.md`);
    const md = p.slug === 'reference' && section.id === 'guide' ? referenceMarkdown(docs) : readFileSync(join(docs, section.id, `${p.slug}.md`), 'utf8');
    const body = renderMarkdown(expandIncludes(md));
    write(
      join(out, `${section.id}-${p.slug}.html`),
      render(
        <Document title={titleOf(p.title)} assets={baseAssets}>
          <DocsShell nav={nav} section={section.id} slug={p.slug} active={section.id} banner={section.banner}>
            <article class="doc" dangerouslySetInnerHTML={{ __html: body }} />
          </DocsShell>
        </Document>,
      ),
    );
  }
}
for (const section of nav) {
  const dir = join(docs, section.id);
  if (!existsSync(dir)) continue;
  for (const f of readdirSync(dir)) {
    if (f.endsWith('.md') && !known.has(`${section.id}/${f}`)) problems.push(`${section.id}/${f} is not in nav.json`);
  }
}

write(
  join(out, 'docs.html'),
  render(
    <Document title={titleOf('API reference')} assets={{ styles: [...siteStyles, ...(docsEntry.css ?? []).map(staticPath)], script: staticPath(docsEntry.file) }}>
      <DocsShell nav={nav} section="guide" slug="reference" active="docs">
        <div id="reference">
          <div class="search">
            <label class="sr-only" for="ref-search">Search the API reference</label>
            <input id="ref-search" type="search" placeholder="Search endpoints, for example refund or subscription" autocomplete="off" />
            <p class="count" id="ref-count" role="status" />
          </div>
          <article class="doc" id="root">
            <h1>API reference</h1>
            <p>Loading the API description.</p>
          </article>
        </div>
      </DocsShell>
    </Document>,
  ),
);

if (problems.length) {
  console.error(`stale or unlisted:\n${problems.join('\n')}`);
  process.exit(1);
}
console.log(`${check ? 'checked' : 'wrote'} ${known.size + 2} site pages`);
