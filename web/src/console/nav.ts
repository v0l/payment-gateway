const PREFIX = '/console';

export function href(path: string): string {
  return PREFIX + path;
}

export function go(path: string, replace = false): void {
  const url = href(path);
  if (replace) history.replaceState(null, '', url);
  else history.pushState(null, '', url);
  dispatchEvent(new PopStateEvent('popstate'));
}

export function legacyHashToPath(): void {
  const h = location.hash;
  if (h.startsWith('#/')) {
    history.replaceState(null, '', PREFIX + h.slice(1));
  }
}

export function enc(s: string): string {
  return encodeURIComponent(s);
}
