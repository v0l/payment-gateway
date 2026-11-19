import { gzipSync } from 'node:zlib';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const budgets: Record<string, number> = { pay: 14 * 1024 };

const dist = join(import.meta.dirname, '..', 'dist');
const manifest = JSON.parse(readFileSync(join(dist, 'manifest.json'), 'utf8')) as Record<string, { file: string; isEntry?: boolean; imports?: string[] }>;

let failed = false;
for (const [name, limit] of Object.entries(budgets)) {
  const entry = manifest[`${name}.html`];
  if (!entry) throw new Error(`no entry ${name}`);
  const files = new Set<string>([entry.file]);
  for (const i of entry.imports ?? []) files.add(manifest[i].file);
  const size = [...files].reduce((n, f) => n + gzipSync(readFileSync(join(dist, f))).length, 0);
  const ok = size <= limit;
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name} script ${size} bytes gzip, budget ${limit}`);
  if (!ok) failed = true;
}
if (readdirSync(dist).length === 0) failed = true;
if (failed) process.exit(1);
