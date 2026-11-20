import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import { NAMES, LANGUAGES, formatCountdown, type Language } from './format';
import { ApiError, api, applyLanguage, chooseLanguage, isOk, lastEnded, loadDict, money, pendingAttempt, progressOf, translate, type Dict } from './api';
import { applyBranding, isDark } from './branding';
import { RevolutCard } from './revolut';
import type { Attempt, Method, PayState } from './types';

const parts = location.pathname.split('/').filter(Boolean);
const embed = parts[0] === 'embed';
const id = parts[1] ?? '';

type T = (key: string, vars?: Record<string, string | number>) => string;

function postToParent(state: PayState | null, type: string, extra?: object) {
  if (!embed || window.parent === window || !state) return;
  const msg = { source: 'payment-gateway', type: `pgw:${type}`, payment_id: state.id, ...extra };
  for (const o of state.allowed_origins ?? []) {
    try {
      window.parent.postMessage(msg, o);
    } catch {}
  }
}

function postError(state: PayState | null, code: string, message: string) {
  if (!embed || window.parent === window) return;
  for (const o of state?.allowed_origins ?? []) {
    try {
      window.parent.postMessage({ source: 'payment-gateway', type: 'pgw:error', payment_id: id, code, message }, o);
    } catch {}
  }
}

function CopyRow(props: { label: string; value: string; focusKey: string; t: T }) {
  const { t } = props;
  const [done, setDone] = useState(false);
  const fieldId = `f-${props.focusKey}`;
  return (
    <div class="field">
      <label for={fieldId}>{props.label}</label>
      <div class="copy">
        <code id={fieldId} title={props.value}>{props.value}</code>
        <button
          type="button"
          data-focus={props.focusKey}
          aria-label={t('copy_label', { label: props.label })}
          onClick={() => {
            navigator.clipboard?.writeText(props.value).then(() => {
              setDone(true);
              setTimeout(() => setDone(false), 1500);
            });
          }}
        >
          {done ? t('copied') : t('copy')}
        </button>
      </div>
    </div>
  );
}

function Countdown(props: { expires: string; skew: number; pending: boolean; onExpired: () => void; t: T }) {
  const { t } = props;
  const [, tick] = useState(0);
  useEffect(() => {
    const h = setInterval(() => tick(n => n + 1), 1000);
    return () => clearInterval(h);
  }, []);
  const ms = Date.parse(props.expires) - (Date.now() + props.skew);
  useEffect(() => {
    if (ms <= 0 && props.pending) props.onExpired();
  });
  return (
    <p class={`muted countdown${ms > 0 && ms < 60000 ? ' urgent' : ''}`}>
      {ms > 0 ? t('expires_in', { time: formatCountdown(ms) }) : t('expired_now')}
    </p>
  );
}

function OnchainStatus(props: { a: Attempt; lang: string; t: T }) {
  const { a, lang, t } = props;
  const p = progressOf(a);
  const m = (n: number) => money(n, a.currency, lang);
  return (
    <>
      {p.received > 0 && p.received < p.required && (
        <p class="notice warn">{t('underpaid', { received: m(p.received), required: m(p.required), remaining: m(p.required - p.received) })}</p>
      )}
      {p.received > 0 && (
        <>
          <p class="notice">{t('detected')}</p>
          <p class="confirmations">{t('confirmations', { done: p.confirmed >= p.required ? p.min_confirmations : 0, total: p.min_confirmations })}</p>
        </>
      )}
    </>
  );
}

function AttemptView(props: { a: Attempt; state: PayState; skew: number; lang: string; t: T; onExpired: () => void }) {
  const { a, state, lang, t } = props;
  const d = a.details ?? {};
  const qr = `/pay/${id}/qr/${a.id}`;
  const inline = d.widget?.provider === 'revolut';
  return (
    <>
      {inline && d.widget && (
        <RevolutCard w={{ ...d.widget, checkout_url: d.checkout_url }} locale={lang} dark={isDark(state)} t={t} onPaid={props.onExpired} />
      )}
      {!inline && d.checkout_url && (
        <>
          <a class="primary" href={d.checkout_url} target={embed ? '_top' : '_self'} rel="noopener" data-focus="primary">{t('continue_to_provider')}</a>
          <p class="muted">{t('provider_redirect')}</p>
        </>
      )}
      {d.bolt11 && (
        <>
          <img class="qr" src={qr} alt={t('qr_lightning')} />
          <a class="primary" href={`lightning:${d.bolt11}`} data-focus="primary">{t('open_wallet')}</a>
          <CopyRow label={t('lightning_invoice')} value={d.bolt11} focusKey="invoice" t={t} />
        </>
      )}
      {d.address && (
        <>
          <OnchainStatus a={a} lang={lang} t={t} />
          <img class="qr" src={qr} alt={t('qr_bitcoin')} />
          {d.uri && <a class="primary" href={d.uri} data-focus="primary">{t('open_wallet')}</a>}
          <CopyRow label={t('bitcoin_address')} value={d.address} focusKey="address" t={t} />
          <CopyRow label={t('amount_sats')} value={String(d.amount_sats)} focusKey="amount" t={t} />
        </>
      )}
      {d.deposit_address && (
        <>
          <CopyRow label={t('send_to', { coin: d.coin ?? '' })} value={d.deposit_address} focusKey="deposit" t={t} />
          <CopyRow label={t('amount_coin', { coin: d.coin ?? '' })} value={String(d.deposit_amount)} focusKey="deposit-amount" t={t} />
          <p class="muted">{t('send_exact')}</p>
        </>
      )}
      <p class="muted">
        <span class="spinner" aria-hidden="true" />
        {t('waiting')}
      </p>
      <Countdown expires={state.expires} skew={props.skew} pending={state.status === 'pending'} onExpired={props.onExpired} t={t} />
      <p class="muted">{t('total', { amount: money(a.amount, a.currency, lang) })}</p>
    </>
  );
}

function MethodList(props: { methods: Method[]; busy: boolean; lang: string; t: T; onChoose: (m: Method, option?: string, save?: boolean) => void; canSave?: boolean }) {
  const { t } = props;
  const [chosen, setChosen] = useState<Record<number, string>>({});
  const [save, setSave] = useState(false);
  const hasCard = props.methods.some(m => m.method === 'revolut');
  return (
    <>
    <ul class="methods">
      {props.methods.map(m => {
        const label = t(`method.${m.method}`);
        if (m.options?.length) {
          const value = chosen[m.provider_id] ?? m.options[0];
          return (
            <li class="with-option" key={m.provider_id}>
              <button class="method" type="button" disabled={props.busy} onClick={() => props.onChoose(m, value)}>
                <span>{label}</span>
              </button>
              <select aria-label={t('coin')} value={value} onChange={e => setChosen({ ...chosen, [m.provider_id]: e.currentTarget.value })}>
                {m.options.map(o => (
                  <option value={o} key={o}>{o}</option>
                ))}
              </select>
            </li>
          );
        }
        return (
          <li key={m.provider_id}>
            <button class="method" type="button" disabled={props.busy} onClick={() => props.onChoose(m, undefined, props.canSave && m.method === 'revolut' ? save : undefined)}>
              <span>{label}</span>
              {m.fee > 0 && <span class="fee">{t('fee', { amount: money(m.fee, m.currency, props.lang) })}</span>}
            </button>
          </li>
        );
      })}
    </ul>
    {props.canSave && hasCard && (
      <label class="check save-card">
        <input type="checkbox" checked={save} onChange={e => setSave(e.currentTarget.checked)} />
        {t('save_card')}
      </label>
    )}
    </>
  );
}

function Summary(props: { state: PayState; lang: string; t: T }) {
  const { state: s, lang, t } = props;
  if (!s.line_items?.length) return null;
  const m = (n: number) => money(n, s.currency, lang);
  return (
    <details class="summary">
      <summary>{t('order_summary', { count: s.line_items.length })}</summary>
      <table>
        <tbody>
          {s.line_items.map((l, i) => (
            <tr key={i}>
              <td>{(l.quantity > 1 ? `${l.quantity} x ` : '') + l.name}</td>
              <td class="num">{m(l.total)}</td>
            </tr>
          ))}
          {s.discount && (
            <tr>
              <td>{t('discount') + (s.discount.code ? ` ${s.discount.code}` : '')}</td>
              <td class="num">{`-${m(s.discount.amount)}`}</td>
            </tr>
          )}
          {s.tax ? (
            <tr>
              <td>{t('tax')}</td>
              <td class="num">{m(s.tax)}</td>
            </tr>
          ) : null}
          <tr class="total">
            <td>{t('total_label')}</td>
            <td class="num">{m(s.amount)}</td>
          </tr>
        </tbody>
      </table>
    </details>
  );
}

function goBack(s: PayState) {
  const u = s.return_url;
  if (!u) return;
  if (embed) window.open(u, '_top');
  else location.assign(u);
}

function terminalTitle(s: PayState, t: T): string {
  if (isOk(s.status)) return t('paid_title');
  if (s.status === 'expired') return t('expired_title');
  if (s.status === 'cancelled') return t('cancelled_title');
  if (s.status === 'failed') return t('failed_title');
  return t('status_other', { status: s.status });
}

function Terminal(props: { state: PayState; lang: string; t: T }) {
  const { state: s, lang, t } = props;
  const ok = isOk(s.status);
  const paid = (s.attempts ?? []).find(a => a.status === 'paid');
  let overpaid: ComponentChildren = null;
  if (ok && paid) {
    const p = progressOf(paid);
    if (p.required > 0 && p.received > p.required) {
      overpaid = <p class="notice warn">{t('overpaid', { extra: money(p.received - p.required, paid.currency, lang), name: s.tenant.name })}</p>;
    }
  }
  return (
    <>
      <div class={`status ${ok ? 'ok' : 'bad'}`}>
        <div class="icon" aria-hidden="true">{ok ? '\u2713' : '\u2717'}</div>
        <h2 tabIndex={-1} data-focus="title">{terminalTitle(s, t)}</h2>
      </div>
      {ok && <p class="thanks">{s.branding?.thanks_text || t('thanks_default')}</p>}
      {overpaid}
      {ok && s.receipt_url && /^https:\/\//.test(s.receipt_url) && (
        <a class="secondary" href={s.receipt_url} target="_blank" rel="noopener">{t('receipt')}</a>
      )}
      {s.return_url && (
        <button class="primary" type="button" onClick={() => goBack(s)}>
          {ok ? t('continue') : t('back', { name: s.tenant.name })}
        </button>
      )}
    </>
  );
}

function Footer(props: { state: PayState; t: T }) {
  const { state: s, t } = props;
  const b = s.branding ?? {};
  const links = (b.footer_links ?? []).filter(l => /^https:\/\//.test(l.url));
  const powered = b.show_powered_by !== false;
  if (!links.length && !powered) return null;
  return (
    <footer class="foot">
      {links.length > 0 && (
        <nav class="footer-links" aria-label={t('footer_links')}>
          {links.map(l => (
            <a href={l.url} target={embed ? '_blank' : '_self'} rel="noopener noreferrer" key={l.url}>{l.label}</a>
          ))}
        </nav>
      )}
      {powered && <p class="powered">{t('powered_by')}</p>}
    </footer>
  );
}

function liveMessage(s: PayState, lang: string, t: T): string {
  if (s.status === 'paid') return t('paid_announce');
  if (s.status !== 'pending') return terminalTitle(s, t);
  const a = pendingAttempt(s);
  if (a?.details?.address) {
    const p = progressOf(a);
    const m = (n: number) => money(n, a.currency, lang);
    if (p.received > 0 && p.received < p.required) return t('underpaid', { received: m(p.received), required: m(p.required), remaining: m(p.required - p.received) });
    if (p.received > 0) return `${t('detected')} ${t('confirmations', { done: p.confirmed >= p.required ? p.min_confirmations : 0, total: p.min_confirmations })}`;
  }
  const ended = lastEnded(s);
  if (!a && ended) return t(ended.status === 'failed' ? 'attempt_failed' : 'attempt_expired');
  return '';
}

export function App() {
  const [state, setState] = useState<PayState | null>(null);
  const [lang, setLang] = useState<Language>('en');
  const [dict, setDict] = useState<Dict | null>(null);
  const [loadError, setLoadError] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ key: string; raw: string } | null>(null);
  const fallback = useRef<Dict | null>(null);
  const skew = useRef(0);
  const focusKey = useRef<string | null>(null);
  const announced = useRef<Record<string, boolean>>({});
  const lastAttemptId = useRef<number | null>(null);
  const lastSaid = useRef('');
  const refreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const stateRef = useRef<PayState | null>(null);
  const busyRef = useRef(false);
  const [dismissed, setDismissed] = useState<number | null>(null);

  const t: T = useCallback((key, vars) => translate(dict, fallback.current, key, vars), [dict]);

  const adopt = (s: PayState) => {
    if (s.server_time) skew.current = Date.parse(s.server_time) - Date.now();
    stateRef.current = s;
    setState(s);
  };

  const setLanguage = async (code: Language) => {
    const tasks: Promise<Dict>[] = [loadDict(code)];
    if (!fallback.current) tasks.push(loadDict('en'));
    const [d, f] = await Promise.all(tasks);
    if (f) fallback.current = f;
    setDict(d);
    setLang(code);
    applyLanguage(code);
  };

  const announce = (type: string, extra?: { status?: string }) => {
    const key = type + (extra?.status ?? '');
    if (announced.current[key]) return;
    announced.current[key] = true;
    postToParent(stateRef.current, type, extra);
  };

  const refresh = useCallback(() => {
    if (refreshTimer.current) return;
    refreshTimer.current = setTimeout(async () => {
      refreshTimer.current = null;
      try {
        const s = await api<PayState>(`/pay/${id}/state`);
        const prev = stateRef.current;
        const showing = prev ? pendingAttempt(prev) : null;
        const hidden = showing && !(s.attempts ?? []).some(x => x.id === showing.id && x.status === 'pending');
        if (hidden || !busyRef.current) {
          if (prev && prev.status !== s.status && s.status !== 'pending') focusKey.current = 'title';
          setDismissed(null);
          adopt(s);
        } else if (s.server_time) {
          skew.current = Date.parse(s.server_time) - Date.now();
        }
      } catch {}
    }, 150);
  }, []);

  useEffect(() => {
    if (embed) document.body.classList.add('embed');
    let cancelled = false;
    (async () => {
      try {
        const s = await api<PayState>(`/pay/${id}/state`);
        await setLanguage(chooseLanguage(s.branding?.default_lang));
        if (cancelled) return;
        adopt(s);
      } catch (e) {
        const message = e instanceof Error ? e.message : 'failed';
        try {
          await setLanguage(chooseLanguage(null));
        } catch {}
        setLoadError(message);
        postError(null, 'load_failed', message);
        return;
      }
      if (window.EventSource) {
        const es = new EventSource(`/pay/${id}/events`);
        es.addEventListener('change', refresh);
        setInterval(refresh, 20000);
      } else {
        setInterval(refresh, 4000);
      }
      if ('ResizeObserver' in window) {
        new ResizeObserver(() => {
          if (stateRef.current) postToParent(stateRef.current, 'resize', { height: Math.ceil(document.documentElement.getBoundingClientRect().height) });
        }).observe(document.body);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const shown = useMemo(() => {
    if (!state) return null;
    if (dismissed === null) return state;
    return { ...state, attempts: (state.attempts ?? []).filter(x => x.id !== dismissed) };
  }, [state, dismissed]);

  useLayoutEffect(() => {
    if (!shown) return;
    document.title = t('page_title', { name: shown.tenant.name });
    applyBranding(shown);
  });

  useEffect(() => {
    if (!shown) return;
    const key = focusKey.current;
    if (key) {
      focusKey.current = null;
      const target = document.querySelector<HTMLElement>(`#app [data-focus="${key}"]`) ?? (key === 'lang' ? document.getElementById('lang') : null);
      target?.focus({ preventScroll: true });
    }
    const said = liveMessage(shown, lang, t);
    if (said !== lastSaid.current) {
      lastSaid.current = said;
      const live = document.getElementById('live');
      if (live) live.textContent = said;
    }
    if (shown.status !== 'pending') announce(shown.status === 'paid' ? 'paid' : shown.status, { status: shown.status });
    announce('ready');
    postToParent(shown, 'resize', { height: Math.ceil(document.documentElement.getBoundingClientRect().height) });
  });

  useEffect(() => {
    if (!shown || embed || shown.status !== 'paid' || !shown.return_url) return;
    const h = setTimeout(() => goBack(shown), 2500);
    return () => clearTimeout(h);
  }, [shown?.status]);

  const choose = async (m: Method, option?: string, save?: boolean) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError(null);
    try {
      const s = await api<PayState>(`/pay/${id}/attempts`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ provider_id: m.provider_id, option: option ?? null, save_method: save === true }),
      });
      setDismissed(null);
      adopt(s);
      focusKey.current = 'primary';
    } catch (e) {
      const err = e as ApiError;
      setError({ key: 'error_generic', raw: err.status === 409 ? '' : err.message });
      postError(stateRef.current, 'attempt_failed', err.message);
    }
    busyRef.current = false;
    setBusy(false);
  };

  if (loadError && !state) {
    return (
      <div class="card">
        <p class="error" role="alert">{dict ? t('error_load') : loadError}</p>
      </div>
    );
  }
  if (!shown) return <p class="muted">Loading</p>;

  const s = shown;
  const pending = pendingAttempt(s);
  const ended = lastEnded(s);
  if (pending && lastAttemptId.current !== pending.id) {
    focusKey.current = focusKey.current ?? 'primary';
    lastAttemptId.current = pending.id;
  }
  const canSwitch = s.methods.length > 1 || (s.methods.length === 1 && s.methods[0].options.length > 0);
  const logo = isDark(s) && s.branding?.logo_dark_url ? s.branding.logo_dark_url : s.tenant.logo_url;

  return (
    <>
      <div class="card">
        <header class="brand">
          {logo && /^https:\/\//.test(logo) && <img src={logo} alt="" referrerpolicy="no-referrer" />}
          <span class="name">{s.tenant.name}</span>
        </header>
        <div class="amount">{money(s.amount, s.currency, lang)}</div>
        {s.description && <p class="desc">{s.description}</p>}
        <Summary state={s} lang={lang} t={t} />
        {s.status !== 'pending' ? (
          <Terminal state={s} lang={lang} t={t} />
        ) : pending ? (
          <>
            <AttemptView a={pending} state={s} skew={skew.current} lang={lang} t={t} onExpired={refresh} />
            {canSwitch && (
              <p>
                <button
                  class="link"
                  type="button"
                  onClick={() => {
                    focusKey.current = 'methods';
                    setDismissed(pending.id);
                  }}
                >
                  {t('use_other_method')}
                </button>
              </p>
            )}
          </>
        ) : s.methods.length ? (
          <>
            {ended && <p class="notice warn">{t(ended.status === 'failed' ? 'attempt_failed' : 'attempt_expired')}</p>}
            <h2 class="muted choose" tabIndex={-1} data-focus="methods">{ended ? t('new_request') : t('choose_method')}</h2>
            <MethodList methods={s.methods} busy={busy} lang={lang} t={t} onChoose={choose} canSave={s.can_save_card} />
          </>
        ) : (
          <p class="error">{t('no_methods')}</p>
        )}
        {error && <p class="error">{error.raw ? `${t(error.key)} (${error.raw})` : t(error.key)}</p>}
      </div>
      <Footer state={s} t={t} />
      <div class="lang">
        <select
          id="lang"
          aria-label={t('language')}
          value={lang}
          onChange={async e => {
            await setLanguage(e.currentTarget.value as Language);
            focusKey.current = 'lang';
          }}
        >
          {LANGUAGES.map(c => (
            <option value={c} key={c}>{NAMES[c]}</option>
          ))}
        </select>
      </div>
    </>
  );
}
