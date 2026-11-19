export const LANGUAGES = ['en', 'es', 'fr', 'de', 'pt', 'it', 'ga'] as const;
export type Language = (typeof LANGUAGES)[number];

export const NAMES: Record<Language, string> = {
  en: 'English',
  es: 'Español',
  fr: 'Français',
  de: 'Deutsch',
  pt: 'Português',
  it: 'Italiano',
  ga: 'Gaeilge',
};

const RTL = ['ar', 'he', 'fa', 'ur'];

export function normalise(tag: string | null | undefined): Language | null {
  if (!tag) return null;
  const base = String(tag).toLowerCase().split(/[-_]/)[0];
  return (LANGUAGES as readonly string[]).includes(base) ? (base as Language) : null;
}

export function pickLanguage(
  query: string | null,
  tenantDefault: string | null | undefined,
  browserLanguages: readonly string[],
): Language {
  const fromQuery = normalise(query);
  if (fromQuery) return fromQuery;
  for (const tag of browserLanguages) {
    const hit = normalise(tag);
    if (hit) return hit;
  }
  return normalise(tenantDefault) ?? 'en';
}

export function direction(lang: string): 'rtl' | 'ltr' {
  return RTL.includes(lang.split('-')[0]) ? 'rtl' : 'ltr';
}

export function formatMoney(amount: number, currency: string, lang: string): string {
  if (currency === 'BTC') {
    return `${new Intl.NumberFormat(lang).format(Math.ceil(amount / 1000))} sats`;
  }
  let scale = 100;
  let code = currency;
  if (currency.length === 4 && currency[0] === 'M') {
    scale = 100000;
    code = currency.slice(1);
  }
  try {
    const digits = scale === 100 ? undefined : 3;
    return new Intl.NumberFormat(lang, {
      style: 'currency',
      currency: code,
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    }).format(amount / scale);
  } catch {
    return `${new Intl.NumberFormat(lang, { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(amount / scale)} ${currency}`;
  }
}

export function formatCountdown(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const two = (n: number) => (n < 10 ? '0' : '') + n;
  return h > 0 ? `${h}:${two(m)}:${two(s)}` : `${m}:${two(s)}`;
}
