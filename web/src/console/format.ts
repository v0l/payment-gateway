const STATUS_WORDS: Record<string, string> = {
  pending: 'Waiting',
  paid: 'Paid',
  expired: 'Expired',
  cancelled: 'Cancelled',
  refunded: 'Refunded',
  partially_refunded: 'Part refunded',
  failed: 'Failed',
  disputed: 'Disputed',
  completed: 'Completed',
  requested: 'Requested',
  approved: 'Approved',
  sending: 'Sending',
  rejected: 'Rejected',
  delivered: 'Delivered',
  waiting: 'Waiting',
  retrying: 'Retrying',
  processing: 'Processing',
  active: 'Active',
  past_due: 'Past due',
  trialing: 'Trial',
  incomplete: 'Incomplete',
  paused: 'Paused',
};

export const STATUS_TONE: Record<string, string> = {
  pending: 'warn',
  paid: 'ok',
  completed: 'ok',
  delivered: 'ok',
  active: 'ok',
  expired: 'neutral',
  cancelled: 'neutral',
  rejected: 'bad',
  refunded: 'bad',
  partially_refunded: 'warn',
  failed: 'bad',
  disputed: 'bad',
  refund_failed: 'bad',
  requested: 'warn',
  approved: 'info',
  sending: 'info',
  waiting: 'warn',
  retrying: 'warn',
  processing: 'info',
  past_due: 'bad',
  trialing: 'info',
  incomplete: 'warn',
  paused: 'neutral',
};

const METHOD_NAMES: Record<string, string> = {
  lightning: 'Lightning',
  onchain: 'On-chain',
  stripe: 'Card',
  revolut: 'Revolut',
  paypal: 'PayPal',
  fixedfloat: 'Other coins',
  manual: 'Manual',
  bitcoin: 'Bitcoin',
};

export function statusWord(s: string | null | undefined): string {
  return STATUS_WORDS[s ?? ''] ?? String(s ?? '').replace(/_/g, ' ');
}

export function methodName(m: string | null | undefined): string {
  return METHOD_NAMES[m ?? ''] ?? m ?? '';
}

export function fmtAmount(amount: number, currency: string): string {
  if (currency === 'BTC') return `${Math.ceil(amount / 1000).toLocaleString()} sats`;
  let scale = 100;
  let code = currency;
  if (currency.length === 4 && currency[0] === 'M') {
    scale = 100000;
    code = currency.slice(1);
  }
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency: code }).format(amount / scale);
  } catch {
    return `${(amount / scale).toFixed(2)} ${currency}`;
  }
}

export function sats(msat: number): string {
  return fmtAmount(msat, 'BTC');
}

export function moneyParts(amount: number, currency: string): [string, string] {
  const s = fmtAmount(amount, currency);
  const m = s.match(/^(.*?)([.,]\d{2})(\D*)$/);
  return m ? [m[1], m[2] + m[3]] : [s, ''];
}

export function fmtTime(iso: string | null | undefined): string {
  return iso ? new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '';
}

export function fmtDay(iso: string | null | undefined): string {
  return iso ? new Date(iso).toLocaleDateString([], { day: 'numeric', month: 'short' }) : '';
}

export function fmtWhen(iso: string | null | undefined): string {
  if (!iso) return '';
  const d = new Date(iso);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return fmtTime(iso);
  const y = new Date(now);
  y.setDate(now.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return `Yesterday ${fmtTime(iso)}`;
  return `${fmtDay(iso)}, ${fmtTime(iso)}`;
}

export function fmtWhenAfter(label: string | null | undefined, iso: string | null | undefined): string {
  const w = fmtWhen(iso);
  if (!label) return w;
  if (!w) return label;
  return `${label} ${w.indexOf('Yesterday') === 0 ? `yesterday${w.slice(9)}` : w}`;
}

export function fmtFull(iso: string | null | undefined): string {
  return iso
    ? new Date(iso).toLocaleString([], { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit' })
    : '';
}

export function tzOffset(): number {
  return -new Date().getTimezoneOffset();
}

export function when(iso: string | null | undefined): string {
  return iso ? fmtWhen(iso) : '';
}

export interface PayAttempt {
  id: number;
  status: string;
  method: string;
  option?: string | null;
  amount: number;
  base_amount?: number;
  currency: string;
  paid_at?: string | null;
  expires?: string;
  platform_managed?: boolean;
  platform_fee?: number | null;
  net_credited?: number | null;
  processing_fee?: number | null;
  details?: { bolt11?: string; address?: string; checkout_url?: string; deposit_address?: string } | null;
}

export function shownAttempt(p: { attempts?: PayAttempt[] }): PayAttempt | null {
  const a = p.attempts ?? [];
  return a.find(x => x.status === 'paid') ?? a[a.length - 1] ?? null;
}
