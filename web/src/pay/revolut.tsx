import { useEffect, useRef, useState } from 'preact/hooks';
import type { Widget } from './types';

type FieldStatus = { focused: boolean; invalid: boolean; empty: boolean; autofilled: boolean; completed: boolean };
type Callbacks = { onSuccess: () => void; onError: (error: { message?: string }) => void; onCancel?: () => void };
type CardField = { submit(meta?: Record<string, unknown>): void; destroy(): void };
type RevolutInstance = {
  createCardField(opts: Record<string, unknown>): CardField;
};
type RevolutCheckout = (token: string, mode: 'sandbox' | 'prod') => Promise<RevolutInstance>;

declare global {
  interface Window {
    RevolutCheckout?: RevolutCheckout;
  }
}

type T = (key: string, vars?: Record<string, string | number>) => string;

function loadRevolutScript(mode: string): Promise<RevolutCheckout> {
  return new Promise((resolve, reject) => {
    if (window.RevolutCheckout) return resolve(window.RevolutCheckout);
    const s = document.createElement('script');
    s.src = mode === 'sandbox' ? 'https://sandbox-merchant.revolut.com/embed.js' : 'https://merchant.revolut.com/embed.js';
    s.onload = () => (window.RevolutCheckout ? resolve(window.RevolutCheckout) : reject(new Error('no widget')));
    s.onerror = () => reject(new Error('load failed'));
    document.head.appendChild(s);
  });
}

function token(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function fieldStyles() {
  const base = {
    color: token('--text'),
    fontFamily: token('--font-sans'),
    fontSize: token('--fs-md'),
  };
  return {
    default: base,
    focused: base,
    invalid: { ...base, color: token('--bad') },
    empty: base,
    autofilled: base,
    completed: base,
  };
}

export function RevolutCard(props: { w: Widget; locale: string; dark: boolean; t: T; onPaid: () => void }) {
  const { w, t } = props;
  const fieldHost = useRef<HTMLDivElement>(null);
  const card = useRef<CardField | null>(null);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState<Partial<FieldStatus>>({ empty: true });
  const [name, setName] = useState('');
  const [email, setEmail] = useState(w.email ?? '');

  useEffect(() => {
    let cancelled = false;
    const mounted: { destroy(): void }[] = [];
    const callbacks: Callbacks = {
      onSuccess: () => {
        setDone(true);
        setBusy(false);
        props.onPaid();
      },
      onError: () => {
        setBusy(false);
        setError(t('card_widget_failed'));
      },
      onCancel: () => setBusy(false),
    };
    (async () => {
      try {
        const RC = await loadRevolutScript(w.mode);
        const instance = await RC(w.token, w.mode === 'sandbox' ? 'sandbox' : 'prod');
        if (cancelled || !fieldHost.current) return;
        const field = instance.createCardField({
          target: fieldHost.current,
          locale: props.locale,
          theme: props.dark ? 'dark' : 'light',
          styles: fieldStyles(),
          classes: { default: 'rc-field', focused: 'rc-field--focus', invalid: 'rc-field--invalid', empty: 'rc-field--empty', autofilled: 'rc-field--auto', completed: 'rc-field--done' },
          onStatusChange: (s: FieldStatus) => setStatus(s),
          onValidation: (errs: { type?: string }[]) => {
            if (errs.length) setBusy(false);
          },
          ...callbacks,
        });
        card.current = field;
        mounted.push(field);
        setReady(true);
      } catch {
        if (!cancelled) setError(t('card_widget_failed'));
      }
    })();
    return () => {
      cancelled = true;
      for (const m of mounted) {
        try {
          m.destroy();
        } catch {}
      }
    };
  }, [w.token]);

  const submit = (e: Event) => {
    e.preventDefault();
    if (!card.current || busy) return;
    setError('');
    setBusy(true);
    const meta: Record<string, unknown> = { name: name.trim() || undefined, email: email.trim() || undefined };
    if (w.save_method) meta.savePaymentMethodFor = 'merchant';
    card.current.submit(meta);
  };

  if (done) return <p class="notice" role="status">{t('detected')}</p>;

  const stateClass = status.invalid ? ' invalid' : status.focused ? ' focus' : '';
  return (
    <form class="revolut-card" onSubmit={submit} noValidate>
      <div class="field">
        <label for="rc-name">{t('cardholder_name')}</label>
        <input id="rc-name" type="text" autocomplete="cc-name" value={name} onInput={e => setName(e.currentTarget.value)} />
      </div>
      {!w.email && (
        <div class="field">
          <label for="rc-email">{t('email')}</label>
          <input id="rc-email" type="email" autocomplete="email" value={email} onInput={e => setEmail(e.currentTarget.value)} />
        </div>
      )}
      <div class="field">
        <span class="label" id="rc-card-label">{t('card_details')}</span>
        <div class={`rc-host${stateClass}`} aria-labelledby="rc-card-label">
          <div ref={fieldHost} data-testid="revolut-card-field" />
          {!ready && !error && <span class="muted"><span class="spinner" aria-hidden="true" />{t('loading')}</span>}
        </div>
      </div>
      <button type="submit" class="primary" disabled={!ready || busy} data-focus="primary">
        {busy ? t('processing') : t('pay_by_card')}
      </button>
      <p class="error" role="alert">{error}</p>
      {w.checkout_url && (
        <p class="muted trouble">
          {t('having_trouble')} <a href={w.checkout_url} target="_top" rel="noopener">{t('pay_on_revolut')}</a>
        </p>
      )}
    </form>
  );
}
