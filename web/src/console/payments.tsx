import { useEffect, useRef, useState } from 'preact/hooks';
import { api, downloadBlob, errText, type Project } from './api';
import { fmtAmount, fmtDay, fmtFull, fmtWhen, methodName, shownAttempt, statusWord, type PayAttempt } from './format';
import { enc, go, href } from './nav';
import { ProjectShell } from './shell';
import { BigMoney, ClickRow, Kv, Msg, PageHead, Pill, Table, TextField, SelectField, useNote } from './ui';
import { confirmDialog, toast } from './dialogs';

export interface PaymentRowData {
  id: string;
  amount: number;
  currency: string;
  status: string;
  description?: string | null;
  reference: string;
  customer_ref?: string | null;
  created: string;
  expires?: string;
  paid_at?: string | null;
  dispute_status?: string | null;
  attempts?: PayAttempt[];
  refunds?: { amount: number; status: string; created: string }[];
}

const STATUSES = ['', 'pending', 'paid', 'expired', 'cancelled', 'refunded', 'partially_refunded'];
const FILTER_KEYS = ['q', 'status', 'method', 'customer', 'from', 'to', 'min_amount', 'max_amount'] as const;
type FilterKey = (typeof FILTER_KEYS)[number];
type Filters = Record<FilterKey, string>;
const METHOD_OPTIONS = ['', 'lightning', 'onchain', 'stripe', 'revolut', 'paypal', 'fixedfloat'];
const SETTLE_DAYS: Record<string, number> = { stripe: 7, paypal: 7, revolut: 1 };
const EMPTY: Filters = { q: '', status: '', method: '', customer: '', from: '', to: '', min_amount: '', max_amount: '' };

function filterQuery(values: Partial<Filters>, extra?: string): string {
  const parts: string[] = [];
  FILTER_KEYS.forEach(k => {
    const v = values[k];
    if (v) parts.push(`${k}=${enc(v)}`);
  });
  if (extra) parts.push(extra);
  return parts.length ? `?${parts.join('&')}` : '';
}

export function availableWhen(p: PaymentRowData): string {
  if (p.status === 'pending') return 'When paid';
  if (p.status !== 'paid' && p.status !== 'partially_refunded') return '';
  const a = shownAttempt(p);
  if (!a) return 'Manual';
  if (a.platform_managed) return 'In your balance';
  const days = SETTLE_DAYS[a.method];
  if (!days) return 'In your wallet';
  const at = new Date((a.paid_at || p.paid_at)!);
  at.setDate(at.getDate() + days);
  return at <= new Date() ? `Paid out by ${methodName(a.method)}` : `From ${fmtDay(at.toISOString())}`;
}

function PaymentPanel(props: { pid: string; p: PaymentRowData }) {
  const { pid, p } = props;
  const a = shownAttempt(p);
  return (
    <aside class="panel" aria-label="Selected payment">
      <div class="panel-head">
        <span class="mono">{p.id.slice(0, 16)}</span>
        <a href={href(`/p/${pid}/payments/${p.id}`)}>Open payment</a>
      </div>
      <div>
        <BigMoney amount={p.amount} currency={p.currency} />
        <Pill status={p.dispute_status === 'open' ? 'disputed' : p.status} />
      </div>
      <div class="muted">{p.description || p.reference}</div>
      <ol class="steps">
        {p.paid_at ? (
          <li>
            {`Paid${a ? ` over ${methodName(a.method)}` : ''}`}
            <small>{fmtFull(p.paid_at)}</small>
          </li>
        ) : null}
        {(p.refunds ?? []).map((r, i) => (
          <li class={r.status === 'failed' ? 'bad' : 'warn'} key={i}>
            {`Refund ${fmtAmount(r.amount, p.currency)} ${statusWord(r.status).toLowerCase()}`}
            <small>{fmtFull(r.created)}</small>
          </li>
        ))}
        {p.dispute_status ? <li class="bad">{`Dispute ${p.dispute_status}`}</li> : null}
        {!p.paid_at && p.status === 'pending' ? (
          <li class="warn">
            Waiting for the customer
            <small>{`Expires ${fmtFull(p.expires)}`}</small>
          </li>
        ) : null}
        <li class="grey">
          Created
          <small>{fmtFull(p.created)}</small>
        </li>
      </ol>
      <Kv pairs={[['Customer', p.customer_ref], ['Method', a ? methodName(a.method) : null], ['Reference', p.reference], ['Available', availableWhen(p)]]} />
    </aside>
  );
}

interface SavedView {
  id: number;
  name: string;
  filter: Partial<Filters>;
}

function bulkCancel(props: { pid: string; onOut: (t: string, bad?: boolean) => void; done: () => void }) {
  return async () => {
    const r = await confirmDialog({
      title: 'Cancel stale waiting payments',
      body: 'Cancels every waiting payment older than 24 hours (up to 200 at a time). Customers can no longer pay them. This cannot be undone.',
      keep: 'Keep them',
      confirm: 'Cancel payments',
      danger: true,
    });
    if (!r) return;
    try {
      const res = await api<{ cancelled: number; skipped: number; more: boolean }>('POST', `/projects/${props.pid}/payments-bulk-cancel`, { older_than_hours: 24 });
      props.onOut(`${res.cancelled} cancelled, ${res.skipped} skipped${res.more ? '. Run again for more.' : '.'}`);
      props.done();
    } catch (e) {
      props.onOut(errText(e), true);
    }
  };
}

function PaymentsList(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const [filters, setFilters] = useState<Filters>(EMPTY);
  const [current, setCurrent] = useState('all');
  const [rows, setRows] = useState<PaymentRowData[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [saved, setSaved] = useState<SavedView[]>([]);
  const [counts, setCounts] = useState<Record<string, number> | null>(null);
  const [error, setError] = useState('');
  const [noteNode, setNote] = useNote();
  const latest = useRef(filters);
  latest.current = filters;

  const load = async (reset: boolean, f: Filters, before: string | null) => {
    try {
      const r = await api<{ payments: PaymentRowData[]; next: string | null }>('GET', `/projects/${pid}/payments${filterQuery(f, before && !reset ? `before=${enc(before)}` : undefined)}`);
      setRows(prev => (reset ? r.payments : [...prev, ...r.payments]));
      setNext(r.next);
      setLoaded(true);
      setError('');
    } catch (e) {
      setError(errText(e));
    }
  };
  const loadViews = async () => {
    const [c, v] = await Promise.all([
      api<Record<string, number>>('GET', `/projects/${pid}/payment-view-counts`).catch(() => null),
      api<{ views: SavedView[] }>('GET', `/projects/${pid}/saved-views`).catch(() => ({ views: [] as SavedView[] })),
    ]);
    setCounts(c);
    setSaved(v.views);
  };
  useEffect(() => {
    load(true, EMPTY, null);
    loadViews();
  }, [pid]);

  const apply = (f: Filters, view: string) => {
    setFilters(f);
    setCurrent(view);
    load(true, f, null);
  };
  const setField = (k: FilterKey, v: string, run: boolean) => {
    const f = { ...filters, [k]: v };
    setFilters(f);
    if (run) {
      setCurrent('custom');
      load(true, f, null);
    }
  };

  const builtin: [string, string, Partial<Filters>][] = [
    ['all', 'All', {}],
    ['pending', 'Waiting', { status: 'pending' }],
    ['paid', 'Paid', { status: 'paid' }],
    ['refunded', 'Refunded', { status: 'refunded' }],
    ['expired', 'Expired', { status: 'expired' }],
    ['last_7_days', 'Last 7 days', { from: new Date(Date.now() - 6 * 864e5).toISOString().slice(0, 10) }],
  ];
  const tabs: [string, string, number | undefined, Partial<Filters>][] = [
    ...builtin.map<[string, string, number | undefined, Partial<Filters>]>(b => [b[0], b[1], counts ? counts[b[0]] : undefined, b[2]]),
    ...saved.map<[string, string, number | undefined, Partial<Filters>]>(v => [`saved:${v.id}`, v.name, undefined, v.filter]),
  ];

  const saveView = async () => {
    const r = await confirmDialog({ title: 'Save this view', body: 'Saves the current filters as a tab on this page, for you.', input: 'Name', inputType: 'text', keep: 'Cancel', confirm: 'Save view' });
    if (!r || !r.text.trim()) return;
    const f: Partial<Filters> = {};
    FILTER_KEYS.forEach(k => {
      if (filters[k].trim()) f[k] = filters[k].trim();
    });
    try {
      await api('POST', `/projects/${pid}/saved-views`, { name: r.text.trim(), filter: f });
      toast('View saved.');
      loadViews();
    } catch (e) {
      setNote(errText(e), true);
    }
  };

  const exportCsv = async () => {
    try {
      const blob = await downloadBlob(`/projects/${pid}/payments-export${filterQuery(filters)}`);
      const a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = `payments-${pid}.csv`;
      document.body.appendChild(a);
      a.click();
      a.remove();
    } catch (e) {
      setNote(errText(e), true);
    }
  };

  const paid: Record<string, number> = {};
  rows.forEach(x => {
    if (x.status === 'paid') paid[x.currency] = (paid[x.currency] ?? 0) + x.amount;
  });
  const total = `${rows.length}${next ? '+' : ''} shown${Object.keys(paid).map(c => `, ${fmtAmount(paid[c], c)} paid`).join('')}`;
  const sel = rows.find(r => r.id === selected) ?? rows[0];
  const viewer = p.role === 'viewer';
  const onKey = (k: FilterKey) => (e: KeyboardEvent) => {
    if (e.key === 'Enter') setField(k, (e.currentTarget as HTMLInputElement).value, true);
  };
  const cancelStale = bulkCancel({ pid, onOut: (t, bad) => setNote(t, bad), done: () => { load(true, filters, null); loadViews(); } });
  const mobGo = (id: string) => go(`/p/${pid}/payments/${id}`);

  return (
    <>
      <PageHead
        title="Payments"
        sub="Every payment your app created, newest first. Select a row to see it here."
        actions={
          <>
            <button onClick={exportCsv}>Export CSV</button>
            <button disabled={viewer} title="Cancel waiting payments older than 24 hours" onClick={cancelStale}>Cancel stale</button>
            <button class="primary" disabled={viewer} onClick={() => go(`/p/${pid}/payments/new`)}>New payment</button>
          </>
        }
      />
      <div class="views" role="group" aria-label="Views">
        {tabs.map(t => (
          <button
            type="button"
            key={t[0]}
            class={t[0] === current ? 'on' : ''}
            aria-pressed={t[0] === current ? 'true' : 'false'}
            onClick={() => apply({ ...EMPTY, ...t[3] }, t[0])}
          >
            {t[1]}
            {t[2] !== undefined ? <span class="n">{Number(t[2]).toLocaleString()}</span> : null}
          </button>
        ))}
        <button type="button" class="" onClick={saveView}>Save this view</button>
      </div>
      <div class="filterbar">
        <input type="search" placeholder="Search by reference or payment id" aria-label="Reference or id" value={filters.q} onInput={e => setField('q', e.currentTarget.value, false)} onKeyDown={onKey('q')} />
        <select aria-label="Status" value={filters.status} onChange={e => setField('status', e.currentTarget.value, true)}>
          {STATUSES.map(s => (
            <option value={s} key={s}>{s ? statusWord(s) : 'Any status'}</option>
          ))}
        </select>
        <select aria-label="Method" value={filters.method} onChange={e => setField('method', e.currentTarget.value, true)}>
          {METHOD_OPTIONS.map(s => (
            <option value={s} key={s}>{s ? methodName(s) : 'Any method'}</option>
          ))}
        </select>
        <input type="date" aria-label="From date" value={filters.from} onChange={e => setField('from', e.currentTarget.value, true)} />
        <input type="date" aria-label="To date" value={filters.to} onChange={e => setField('to', e.currentTarget.value, true)} />
        <button class="link" type="button" onClick={() => apply(EMPTY, 'all')}>Clear</button>
        <span class="total">{total}</span>
      </div>
      <details class="hide-s">
        <summary>More filters</summary>
        <div class="filterbar">
          <input placeholder="Customer ref or email" aria-label="Customer" value={filters.customer} onInput={e => setField('customer', e.currentTarget.value, false)} onKeyDown={onKey('customer')} />
          <input type="number" min="0" placeholder="Min, minor units" aria-label="Minimum amount in minor units" value={filters.min_amount} onInput={e => setField('min_amount', e.currentTarget.value, false)} onKeyDown={onKey('min_amount')} />
          <input type="number" min="0" placeholder="Max, minor units" aria-label="Maximum amount in minor units" value={filters.max_amount} onInput={e => setField('max_amount', e.currentTarget.value, false)} onKeyDown={onKey('max_amount')} />
          <button class="small" onClick={() => load(true, filters, null)}>Apply</button>
        </div>
      </details>
      {noteNode}
      <div class="split">
        {error ? (
          <Msg text={error} bad />
        ) : loaded && !rows.length ? (
          <div class="empty">
            <b>No payments match.</b>
            <p class="muted">Clear a filter or widen the dates. New payments appear here as soon as your app creates them.</p>
          </div>
        ) : rows.length ? (
          <>
            <Table head={[['Amount', 'num'], 'Status', 'Description', 'Customer', 'Method', 'Available', 'Created']} cls="plain">
              {rows.map(r => {
                const a = shownAttempt(r);
                return (
                  <ClickRow key={r.id} id={r.id} cls={sel && r.id === sel.id ? 'sel' : undefined} onOpen={() => setSelected(r.id)}>
                    <td class="amt">
                      {fmtAmount(r.amount, r.currency)}
                      <small>{r.currency}</small>
                    </td>
                    <td><Pill status={r.dispute_status === 'open' ? 'disputed' : r.status} /></td>
                    <td class="trunc">{r.description || r.reference}</td>
                    <td class="muted trunc">{r.customer_ref ?? ''}</td>
                    <td class="muted">{a ? methodName(a.method) : ''}</td>
                    <td class="muted">{availableWhen(r)}</td>
                    <td class="muted">{fmtWhen(r.created)}</td>
                  </ClickRow>
                );
              })}
            </Table>
            {sel ? <PaymentPanel pid={pid} p={sel} /> : null}
          </>
        ) : null}
      </div>
      <ul class="mob-list">
        {rows.map(r => (
          <li key={r.id} tabIndex={0} onClick={() => mobGo(r.id)} onKeyDown={e => e.key === 'Enter' && mobGo(r.id)}>
            <b>{r.description || r.reference}</b>
            <span class="a">{fmtAmount(r.amount, r.currency)}</span>
            <span class="d">{[r.customer_ref, fmtWhen(r.created)].filter(Boolean).join(', ')}</span>
            <span><Pill status={r.dispute_status === 'open' ? 'disputed' : r.status} /></span>
          </li>
        ))}
      </ul>
      <div class="more">
        {rows.length ? (next ? <button onClick={() => load(false, filters, next)}>Load more</button> : <span class="muted">No more payments</span>) : null}
      </div>
    </>
  );
}

export function Payments(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="payments" bare>
      {p => <PaymentsList pid={props.pid} p={p} />}
    </ProjectShell>
  );
}

export function NewPayment(props: { pid: string }) {
  const { pid } = props;
  return (
    <ProjectShell pid={pid} tab="payments" sub="Creates a pay link you can send to a customer. Your app usually does this through the API.">
      {() => <NewPaymentForm pid={pid} />}
    </ProjectShell>
  );
}

function NewPaymentForm(props: { pid: string }) {
  const { pid } = props;
  const [ref, setRef] = useState(`order-${Math.floor(Date.now() / 1000)}`);
  const [amount, setAmount] = useState('1500');
  const [cur, setCur] = useState('EUR');
  const [desc, setDesc] = useState('');
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useNote();
  return (
    <div class="card form">
      <TextField label="Reference" value={ref} onInput={setRef} />
      <SelectField label="Currency" value={cur} onChange={setCur} options={['EUR', 'USD', 'GBP', 'BTC'].map(c => [c, c])} />
      <TextField label="Amount in minor units (cents, or msats for BTC)" type="number" min="1" value={amount} onInput={setAmount} />
      <TextField label="Description" placeholder="Shown to the customer on the pay page" value={desc} onInput={setDesc} />
      <div class="row">
        <button
          class="primary"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              const r = await api<{ id: string }>('POST', `/projects/${pid}/payments`, { reference: ref, amount: Number(amount), currency: cur, description: desc || null });
              go(`/p/${pid}/payments/${r.id}`);
            } catch (e) {
              setNote(errText(e), true);
            }
            setBusy(false);
          }}
        >
          Create payment
        </button>
        <a href={href(`/p/${pid}/payments`)}>Cancel</a>
      </div>
      {note}
    </div>
  );
}
