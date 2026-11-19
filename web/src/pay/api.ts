import { formatCountdown, formatMoney, pickLanguage, direction, type Language } from './format';
import type { Attempt, PayState, Progress } from './types';

export type Dict = Record<string, string>;

export class ApiError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(path, init);
  const body = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw new ApiError(body.error ?? 'request failed', r.status);
  return body as T;
}

export async function loadDict(code: string): Promise<Dict> {
  const r = await fetch(`/static/i18n/${code}.json`);
  return (await r.json()) as Dict;
}

export function translate(dict: Dict | null, fallback: Dict | null, key: string, vars?: Record<string, string | number>): string {
  const text = dict?.[key] ?? fallback?.[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (m, name: string) => (vars && vars[name] !== undefined ? String(vars[name]) : m));
}

export function chooseLanguage(tenantDefault: string | null | undefined): Language {
  const q = new URLSearchParams(location.search).get('lang');
  return pickLanguage(q, tenantDefault, navigator.languages ?? [navigator.language]);
}

export function applyLanguage(code: Language): void {
  document.documentElement.lang = code;
  document.documentElement.dir = direction(code);
}

export const money = formatMoney;
export const countdown = formatCountdown;

export function pendingAttempt(s: PayState): Attempt | null {
  const list = (s.attempts ?? []).filter(a => a.status === 'pending');
  return list.length ? list[list.length - 1] : null;
}

export function lastEnded(s: PayState): Attempt | null {
  const list = (s.attempts ?? []).filter(a => a.status === 'expired' || a.status === 'failed');
  return list.length ? list[list.length - 1] : null;
}

export function progressOf(a: Attempt): Progress {
  return a.progress ?? { required: 0, received: 0, confirmed: 0, min_confirmations: 1 };
}

export function isOk(status: string): boolean {
  return status === 'paid' || status === 'refunded' || status === 'partially_refunded';
}
