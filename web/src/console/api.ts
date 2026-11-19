import { useEffect, useState } from 'preact/hooks';

export interface Project {
  id: number;
  name: string;
  role: 'owner' | 'admin' | 'viewer';
  trusted?: boolean;
  enabled?: boolean;
  display_name?: string | null;
  webhook_url?: string | null;
  logo_url?: string | null;
  accent_color?: string | null;
  allowed_origins?: string[];
  default_success_url?: string | null;
  default_cancel_url?: string | null;
  legal_name?: string | null;
  address?: string | null;
  tax_id?: string | null;
  support_email?: string | null;
  require_2fa?: boolean;
}

export interface Me {
  id: number;
  email: string;
  is_operator: boolean;
  csrf: string;
  projects: Project[];
  mail_enabled?: boolean;
  notify?: boolean;
  totp_enabled?: boolean;
  two_factor?: boolean;
  challenge?: string;
}

export class ApiError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

interface Session {
  me: Me | null;
  csrf: string;
  ready: boolean;
}

const session: Session = { me: null, csrf: '', ready: false };
const listeners = new Set<() => void>();

export function getSession(): Session {
  return session;
}

export function setSession(me: Me | null): void {
  session.me = me;
  session.csrf = me?.csrf ?? '';
  session.ready = true;
  listeners.forEach(l => l());
}

export function useSession(): Session {
  const [, tick] = useState(0);
  useEffect(() => {
    const l = () => tick(n => n + 1);
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);
  return session;
}

const OPEN_PATHS = ['/login', '/login/2fa', '/signup'];

export async function api<T = unknown>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {};
  if (session.csrf) headers['x-csrf-token'] = session.csrf;
  const init: RequestInit = { method, headers, credentials: 'same-origin' };
  if (body !== undefined) {
    headers['content-type'] = 'application/json';
    init.body = JSON.stringify(body);
  }
  const r = await fetch(`/console/api${path}`, init);
  const b = (await r.json().catch(() => ({}))) as { error?: string };
  if (r.status === 401 && !OPEN_PATHS.includes(path)) setSession(null);
  if (!r.ok) throw new ApiError(b.error ?? 'Request failed', r.status);
  return b as T;
}

export async function downloadBlob(path: string): Promise<Blob> {
  const r = await fetch(`/console/api${path}`, { credentials: 'same-origin', headers: { 'x-csrf-token': session.csrf } });
  if (!r.ok) throw new Error('Export failed');
  return r.blob();
}

export function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export interface Loaded<T> {
  data: T | null;
  error: string;
  reload: () => void;
  set: (v: T | null) => void;
}

export function useLoad<T>(fn: () => Promise<T>, deps: unknown[]): Loaded<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState('');
  const [n, setN] = useState(0);
  useEffect(() => {
    let live = true;
    setError('');
    fn().then(
      v => {
        if (live) setData(v);
      },
      e => {
        if (live) setError(errText(e));
      },
    );
    return () => {
      live = false;
    };
  }, [...deps, n]);
  return { data, error, reload: () => setN(x => x + 1), set: setData };
}
