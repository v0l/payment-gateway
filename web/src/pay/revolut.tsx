import { useState } from 'preact/hooks';
import type { Widget } from './types';

type RevolutInstance = {
  payWithPopup(opts: Record<string, unknown>): void;
};
type RevolutCheckout = (token: string, mode: 'sandbox' | 'prod') => Promise<RevolutInstance>;

declare global {
  interface Window {
    RevolutCheckout?: RevolutCheckout;
  }
}

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

function RevolutCard(props: { w: Widget; t: (key: string) => string }) {
  const { w, t } = props;
  const [note, setNote] = useState('');
  const [disabled, setDisabled] = useState(false);
  const start = async () => {
    setDisabled(true);
    try {
      const RC = await loadRevolutScript(w.mode);
      const instance = await RC(w.token, w.mode === 'sandbox' ? 'sandbox' : 'prod');
      const opts: Record<string, unknown> = {
        savePaymentMethodFor: 'merchant',
        onSuccess: () => setNote(t('detected')),
        onError: () => {
          setDisabled(false);
          setNote(t('card_widget_failed'));
        },
        onCancel: () => setDisabled(false),
      };
      if (w.email) opts.email = w.email;
      instance.payWithPopup(opts);
    } catch {
      setDisabled(false);
      setNote(t('card_widget_failed'));
    }
  };
  return (
    <div class="revolut-card">
      <button type="button" class="secondary" disabled={disabled} onClick={start}>{t('pay_with_card_save')}</button>
      <p class="muted" aria-live="polite">{note}</p>
    </div>
  );
}

export function revolutCardButton(w: Widget, t: (key: string) => string) {
  return <RevolutCard w={w} t={t} />;
}
