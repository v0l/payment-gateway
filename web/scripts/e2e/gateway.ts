import { execSync, spawn, type ChildProcess } from 'node:child_process';
import { join } from 'node:path';

const root = join(import.meta.dirname, '..', '..', '..');

export const ADMIN = 'e2e-admin-token-e2e-admin-token';
export const PORT = Number(process.env.E2E_PORT ?? 18746);
export const gateway = `http://127.0.0.1:${PORT}`;
const database = process.env.E2E_DATABASE_URL ?? process.env.TEST_DATABASE_URL;
if (!database) throw new Error('set TEST_DATABASE_URL (or E2E_DATABASE_URL) to a MariaDB server url');

export const failures: string[] = [];
export function check(name: string, ok: boolean, detail = ''): void {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ` ${detail}`}`);
  if (!ok) failures.push(name);
}

export async function call<T>(key: string, method: string, url: string, body?: unknown): Promise<T> {
  const r = await fetch(gateway + url, {
    method,
    headers: { authorization: `Bearer ${key}`, 'content-type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  });
  const json = (await r.json().catch(() => ({}))) as T;
  if (!r.ok) throw new Error(`${method} ${url}: ${r.status} ${JSON.stringify(json)}`);
  return json;
}

export const admin = <T>(method: string, url: string, body?: unknown) => call<T>(ADMIN, method, url, body);

export interface Running {
  child: ChildProcess;
  stop(): void;
}

export async function startGateway(): Promise<ChildProcess> {
  const bin = join(process.env.CARGO_TARGET_DIR ?? join(root, 'target'), 'debug');
  const databaseUrl = execSync(join(bin, 'examples', 'test_database'), { env: { ...process.env, TEST_DATABASE_URL: database } }).toString().trim();
  const child = spawn(join(bin, 'payment-gateway'), [], {
    env: {
      ...process.env,
      DATABASE_URL: databaseUrl,
      GATEWAY_ENCRYPTION_KEY: '11'.repeat(32),
      GATEWAY_ADMIN_TOKEN: ADMIN,
      GATEWAY_PUBLIC_URL: gateway,
      GATEWAY_LISTEN: `127.0.0.1:${PORT}`,
      GATEWAY_ALLOW_MOCK: '1',
      LOG_LEVEL: 'error',
      DB_MAX_CONNECTIONS: '4',
    },
    stdio: ['ignore', 'inherit', 'inherit'],
  });
  for (let i = 0; i < 600; i++) {
    try {
      const r = await fetch(`${gateway}/favicon.svg`);
      if (r.ok) return child;
    } catch {}
    await new Promise(r => setTimeout(r, 200));
  }
  child.kill();
  throw new Error('gateway did not start');
}
