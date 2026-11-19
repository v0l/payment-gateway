import type { PayState } from './types';

function luminance(hex: string): number {
  let h = hex.replace('#', '');
  if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
  const [r, g, b] = [0, 2, 4].map(i => {
    const v = parseInt(h.slice(i, i + 2), 16) / 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const x = luminance(a);
  const y = luminance(b);
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

function readableOn(hex: string): string {
  return luminance(hex) > 0.179 ? '#000000' : '#ffffff';
}

export function isDark(s: PayState): boolean {
  const theme = s.branding?.theme ?? 'auto';
  if (theme === 'dark') return true;
  if (theme === 'light') return false;
  return !!window.matchMedia?.('(prefers-color-scheme: dark)').matches;
}

export function applyBranding(s: PayState): void {
  const b = s.branding ?? {};
  const root = document.documentElement;
  if (b.theme === 'light' || b.theme === 'dark') root.setAttribute('data-theme', b.theme);
  else root.removeAttribute('data-theme');
  root.setAttribute('data-radius', b.radius ?? 'md');
  root.setAttribute('data-font', b.font ?? 'system');
  const c = isDark(s) && b.accent_dark ? b.accent_dark : s.tenant.accent_color;
  const card = getComputedStyle(root).getPropertyValue('--card').trim();
  root.style.removeProperty('--accent');
  root.style.removeProperty('--accent-text');
  if (c && /^#[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$/.test(c) && (!/^#[0-9a-fA-F]{6}$/.test(card) || contrast(c, card) >= 4.5)) {
    root.style.setProperty('--accent', c);
    root.style.setProperty('--accent-text', readableOn(c));
  }
}
