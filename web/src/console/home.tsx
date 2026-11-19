import { useEffect, useMemo, useState } from 'preact/hooks';
import { api, errText, useLoad, type Project } from './api';
import { fmtAmount, fmtDay, fmtWhen, fmtWhenAfter, methodName, sats, statusWord, tzOffset } from './format';
import { href, go } from './nav';
import { setAttention, ProjectShell } from './shell';
import { BigMoney, Chips, ClickRow, Msg, PageHead, Section, Skeleton, copyText } from './ui';
import { confirmDialog, toast } from './dialogs';

const WALLET_SWATCH = ['w1', 'w2', 'w3', 'w4', 'w5'];

interface Series {
  currency: string;
  today: number[];
  last_week: number[];
  today_total: number;
  last_week_same_time: number;
}

function TodayChart(props: { s: Series }) {
  const { s } = props;
  const w = 1000;
  const h = 150;
  const max = Math.max(...s.today, ...s.last_week, 1);
  const pts = (arr: number[]) => arr.map((v, i) => `${(((i + 1) / 24) * w).toFixed(1)},${(h - 4 - (v / max) * (h - 14)).toFixed(1)}`);
  const today = pts(s.today);
  const last = today[today.length - 1].split(',');
  return (
    <figure class="chart">
      <svg
        viewBox={`0 0 ${w} ${h}`}
        preserveAspectRatio="none"
        class="chart-svg"
        role="img"
        aria-label={`Paid so far today ${fmtAmount(s.today_total, s.currency)}, against ${fmtAmount(s.last_week_same_time, s.currency)} by this time last week`}
      >
        <polyline points={`0,${h - 4} ${pts(s.last_week).join(' ')}`} class="chart-week" vector-effect="non-scaling-stroke" />
        <polyline points={`0,${h - 4} ${today.join(' ')}`} class="chart-today" vector-effect="non-scaling-stroke" />
        <line x1={last[0]} x2={last[0]} y1={0} y2={h} class="chart-now" vector-effect="non-scaling-stroke" />
        <circle cx={last[0]} cy={last[1]} r={3.5} class="chart-dot" />
      </svg>
      <div class="legend">
        <span>Today</span>
        <span class="prev">Same day last week</span>
      </div>
      <div class="chart-axis">
        {['00:00', '06:00', '12:00', '18:00', '24:00'].map(t => (
          <span key={t}>{t}</span>
        ))}
      </div>
    </figure>
  );
}

function Delta(props: { now: number; then: number }) {
  const { now, then } = props;
  if (!then) return now ? <span class="up">New</span> : <span />;
  const pct = ((now - then) / then) * 100;
  return <span class={pct >= 0 ? 'up' : 'down'}>{`${pct >= 0 ? '+' : '\u2212'}${Math.abs(pct).toFixed(1)}%`}</span>;
}

interface Funds {
  wallets: { name: string; method: string; currency: string; available: number | null; settling: number; settles: string }[];
  available: number;
  settling: number;
  in_payout: number;
  latest_payout: { amount: number; currency: string; status: string; finished_at?: string; created: string } | null;
}

interface Month {
  start: string;
  figures: {
    currency: string;
    paid: number;
    paid_prev: number;
    payments: number;
    payments_prev: number;
    success_rate: number | null;
    success_rate_prev: number | null;
    refunded: number;
    refunded_prev: number;
  }[];
}

interface HomeData {
  series: Series[];
  funds: Funds;
  month: Month;
  health: { name: string; method: string; note: string; state: string; platform_managed?: boolean }[];
}

function Today(props: { data: HomeData }) {
  const { series } = props.data;
  const [currency, setCurrency] = useState<string | null>(series[0]?.currency ?? null);
  const s = series.find(x => x.currency === currency);
  if (!s) {
    return (
      <section class="sec today-sec">
        <h2>Today</h2>
        <div class="today">
          <span class="muted">Paid so far</span>
          <BigMoney amount={0} currency={currency ?? 'EUR'} />
        </div>
        <p class="muted">No payments today or on the same day last week yet.</p>
      </section>
    );
  }
  return (
    <section class="sec today-sec">
      <h2>Today</h2>
      <div class="today">
        <span class="muted">Paid so far</span>
        <BigMoney amount={s.today_total} currency={s.currency} />
        <span class="muted">{`${fmtAmount(s.last_week_same_time, s.currency)} by this time last week`}</span>
        <Delta now={s.today_total} then={s.last_week_same_time} />
        {series.length > 1 ? <Chips options={series.map(x => [x.currency, x.currency])} value={currency ?? ''} onPick={setCurrency} label="Currency" /> : null}
      </div>
      <TodayChart s={s} />
    </section>
  );
}

interface FeedItem {
  kind: string;
  title: string;
  amount?: number | null;
  currency?: string | null;
  customer?: string;
  reference?: string;
  detail?: string;
  payment_id?: string;
  at: string;
  tone: string;
  method?: string;
}

function FeedRow(props: { pid: string; it: FeedItem }) {
  const { pid, it } = props;
  const amount = it.amount !== null && it.amount !== undefined && it.currency ? `${fmtAmount(it.amount, it.currency)} ` : '';
  let title = ['payment', 'refund', 'payout', 'dispute'].includes(it.kind) ? amount + it.title.toLowerCase() : it.title;
  title = title.charAt(0).toUpperCase() + title.slice(1);
  const what = [it.customer, it.reference, it.detail].filter(Boolean).join(', ');
  const open = () => go(`/p/${pid}/payments/${it.payment_id}`);
  return (
    <li class={it.payment_id ? 'click' : ''} tabIndex={it.payment_id ? 0 : undefined} onClick={it.payment_id ? open : undefined} onKeyDown={it.payment_id ? e => e.key === 'Enter' && open() : undefined}>
      <time datetime={it.at}>{fmtWhen(it.at)}</time>
      <span class={`dot ${it.tone}`} aria-hidden="true" />
      <b>{title}</b>
      <span class="meth">{it.kind === 'webhook' ? 'Webhook' : it.kind === 'payout' ? 'Payout' : methodName(it.method)}</span>
      <span class="what">{what}</span>
    </li>
  );
}

function ActivityFeed(props: { pid: string }) {
  const { pid } = props;
  const [kind, setKind] = useState('all');
  const [items, setItems] = useState<FeedItem[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [error, setError] = useState('');
  const load = async (append: boolean, before: string | null) => {
    try {
      const r = await api<{ items: FeedItem[]; next: string | null }>('GET', `/projects/${pid}/feed?kind=${kind}${append && before ? `&before=${encodeURIComponent(before)}` : ''}`);
      setItems(prev => (append ? [...prev, ...r.items] : r.items));
      setNext(r.next);
    } catch (e) {
      setError(errText(e));
    }
  };
  useEffect(() => {
    load(false, null);
  }, [kind]);
  return (
    <Section
      title="Activity"
      aside={
        <Chips
          options={[['all', 'All'], ['payments', 'Payments'], ['refunds', 'Refunds and disputes'], ['payouts', 'Payouts'], ['webhooks', 'Webhooks']]}
          value={kind}
          onPick={setKind}
          label="Show"
        />
      }
    >
      <ul class="feed">
        {error ? (
          <Msg text={error} bad />
        ) : items.length ? (
          items.map((it, i) => <FeedRow pid={pid} it={it} key={i} />)
        ) : (
          <li class="muted">
            <span>{kind === 'all' ? 'Nothing has happened yet. New payments, refunds and webhook problems show up here.' : 'Nothing of this kind yet.'}</span>
          </li>
        )}
      </ul>
      <div class="more">
        {next ? <button class="small" onClick={() => load(true, next)}>Show older</button> : <span />}
      </div>
    </Section>
  );
}

interface Row {
  key: string;
  title: string;
  who?: string;
  method?: string;
  why: string;
  due?: string;
  due_soon?: boolean;
  at?: string;
  amount: number | null;
  currency?: string | null;
  action: string;
  action_label: string;
  payment_id?: string;
  bulk?: string | null;
}

interface Group {
  kind: string;
  title: string;
  tone: string;
  rows: Row[];
}

interface Attention {
  open: number;
  waiting: { payments: number; volume: number; currency: string }[];
  groups: Group[];
  manual_paid_allowed: boolean;
}

function RowAction(props: { pid: string; p: Project; row: Row; onOut: (t: string) => void; reload: () => void }) {
  const { pid, p, row } = props;
  const label = row.action_label;
  const payHref = href(`/p/${pid}/payments/${row.payment_id}`);
  if (row.action === 'providers') return <a href={href(`/p/${pid}/providers`)}>{label}</a>;
  if (row.action === 'payouts') return <a href={href(`/p/${pid}/payouts`)}>{label}</a>;
  if (row.action === 'subscription') return <a href={href(`/p/${pid}/subscriptions/${row.key.split(':')[1]}`)}>{label}</a>;
  if (row.action === 'copy_link') {
    return <button class="link" type="button" onClick={() => copyText(`${location.origin}/pay/${row.payment_id}`, 'Pay link copied')}>{label}</button>;
  }
  if (row.action === 'resend_failed') {
    return (
      <button
        class="link"
        type="button"
        disabled={p.role === 'viewer'}
        onClick={async () => {
          try {
            const r = await api<{ queued: number }>('POST', `/projects/${pid}/deliveries-resend-failed`, {});
            toast(`${r.queued} webhooks queued again.`);
            props.reload();
          } catch (x) {
            props.onOut(errText(x));
          }
        }}
      >
        {label}
      </button>
    );
  }
  return <a href={payHref}>{label}</a>;
}

function AttentionQueue(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const { data: r, error, reload } = useLoad(() => api<Attention>('GET', `/projects/${pid}/attention`), [pid]);
  const [selected, setSelected] = useState<Record<string, string>>({});
  const [out, setOut] = useState('');
  useEffect(() => {
    if (r) {
      setAttention(pid, r.open);
      setSelected({});
    }
  }, [r]);
  if (error) return <div class="queue"><Msg text={error} bad /></div>;
  if (!r) return <div class="queue"><Skeleton /></div>;
  const waiting = r.waiting.map(w => `${w.payments} for ${fmtAmount(w.volume, w.currency)}`).join(' and ');
  if (!r.groups.length) {
    return (
      <div>
        <div class="queue">
          <div class="quiet" role="status">
            <span class="dot" aria-hidden="true" />
            <span>{`Nothing needs you right now.${waiting ? ` ${waiting} waiting for customers to pay.` : ''}`}</span>
          </div>
        </div>
      </div>
    );
  }
  const canWrite = p.role !== 'viewer';
  const ids = Object.keys(selected);
  const kinds: Record<string, number> = {};
  ids.forEach(k => {
    kinds[selected[k]] = (kinds[selected[k]] ?? 0) + 1;
  });
  const bulk = async (action: string) => {
    const chosen = ids.filter(k => selected[k] === action);
    const v = await confirmDialog({
      title: action === 'cancel' ? `Cancel ${chosen.length} payments` : `Accept ${chosen.length} payments as paid`,
      body: action === 'cancel' ? 'The customers can no longer pay them. This cannot be undone.' : 'They are marked paid without a provider confirmation. Your webhook gets a manual flag and the reason.',
      keep: 'Go back',
      confirm: action === 'cancel' ? 'Cancel payments' : 'Mark paid',
      danger: true,
      input: action === 'mark_paid' ? 'Reason, for example seen in the bank statement' : null,
    });
    if (!v) return;
    try {
      const res = await api<{ done: string[]; failed: string[] }>('POST', `/projects/${pid}/attention/bulk`, { action, payments: chosen, reason: v.text });
      toast(`${res.done.length} done${res.failed.length ? `, ${res.failed.length} could not be changed` : ''}.`);
      reload();
    } catch (e) {
      setOut(errText(e));
    }
  };
  const toggle = (row: Row, on: boolean) => {
    const next = { ...selected };
    if (on && row.bulk) next[row.payment_id!] = row.bulk;
    else delete next[row.payment_id!];
    setSelected(next);
  };
  const onOut = (t: string) => setOut(t);
  return (
    <div>
      <div class="queue">
        <div class="table-wrap" tabIndex={0} role="region" aria-label="Needs attention">
          <div class="bulkbar" hidden={!ids.length}>
            <span>{`${ids.length} selected`}</span>
            <span class="sp" />
            {kinds.mark_paid ? (
              <button
                class="small"
                type="button"
                disabled={!canWrite || !r.manual_paid_allowed}
                title={r.manual_paid_allowed ? undefined : 'Turn on manual payment handling in Settings first'}
                onClick={() => bulk('mark_paid')}
              >
                {`Accept ${kinds.mark_paid} as paid`}
              </button>
            ) : null}
            {kinds.cancel ? <button class="small" type="button" disabled={!canWrite} onClick={() => bulk('cancel')}>{`Cancel ${kinds.cancel}`}</button> : null}
          </div>
          <table>
            <thead>
              <tr>
                <th class="cb" />
                <th>Payment</th>
                <th>Why it is here</th>
                <th>When</th>
                <th class="num">Amount</th>
                <th>Next step</th>
              </tr>
            </thead>
            <tbody>
              {r.groups.flatMap(g => {
                const bulkRows = g.rows.filter(x => x.bulk);
                const allOn = bulkRows.length > 0 && bulkRows.every(x => selected[x.payment_id!]);
                const head = (
                  <tr class="group" key={`g-${g.kind}`}>
                    <td class="cb">
                      {bulkRows.length && canWrite ? (
                        <input
                          type="checkbox"
                          aria-label={`Select all in ${g.title}`}
                          checked={allOn}
                          onChange={e => {
                            const next = { ...selected };
                            bulkRows.forEach(x => {
                              if (e.currentTarget.checked) next[x.payment_id!] = x.bulk!;
                              else delete next[x.payment_id!];
                            });
                            setSelected(next);
                          }}
                        />
                      ) : null}
                    </td>
                    <td colSpan={6}>
                      <span class={`tone ${g.tone}`} aria-hidden="true" />
                      {g.title}
                      <span class="muted">{String(g.rows.length)}</span>
                    </td>
                  </tr>
                );
                const rows = g.rows.map((x, i) => {
                  const cells = (
                    <>
                      <td class="cb">
                        {x.bulk && canWrite ? (
                          <input type="checkbox" data-group={g.kind} aria-label={`Select ${x.title}`} checked={!!selected[x.payment_id!]} onChange={e => toggle(x, e.currentTarget.checked)} />
                        ) : null}
                      </td>
                      <td class="two">
                        <b>{x.title}</b>
                        <span class="sub2">{[x.who, methodName(x.method)].filter(Boolean).join(', ')}</span>
                      </td>
                      <td class="why">{x.why}</td>
                      <td class={`due${x.due_soon ? ' soon' : ''}`}>{fmtWhenAfter(x.due, x.at)}</td>
                      <td class="amt">{x.amount !== null && x.currency ? fmtAmount(x.amount, x.currency) : ''}</td>
                      <td><RowAction pid={pid} p={p} row={x} onOut={onOut} reload={reload} /></td>
                    </>
                  );
                  return x.payment_id ? (
                    <ClickRow key={`${g.kind}-${i}`} onOpen={() => go(`/p/${pid}/payments/${x.payment_id}`)}>{cells}</ClickRow>
                  ) : (
                    <tr key={`${g.kind}-${i}`}>{cells}</tr>
                  );
                });
                return [head, ...rows];
              })}
              {waiting ? (
                <tr class="fold">
                  <td class="cb" />
                  <td colSpan={5}>
                    {`${waiting} waiting for customers to pay. Nothing to do yet. `}
                    <a href={href(`/p/${pid}/payments`)}>Show them</a>
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>
        <ul class="queue-mob">
          {r.groups.flatMap(g => [
            <li class="g" key={`m-${g.kind}`}>
              <span class={`tone ${g.tone}`} aria-hidden="true" />
              {`${g.title} `}
              <span class="muted">{String(g.rows.length)}</span>
            </li>,
            ...g.rows.map((x, i) => (
              <li key={`m-${g.kind}-${i}`}>
                <b>{x.title}</b>
                <span class="a">{x.amount !== null && x.currency ? fmtAmount(x.amount, x.currency) : ''}</span>
                <span class="d">{x.why}</span>
                <span class="d">{[x.who, fmtWhenAfter(x.due, x.at)].filter(Boolean).join(', ')}</span>
                <span class="n"><RowAction pid={pid} p={p} row={x} onOut={onOut} reload={reload} /></span>
              </li>
            )),
          ])}
          {waiting ? (
            <li class="d">
              {`${waiting} waiting for customers to pay. `}
              <a href={href(`/p/${pid}/payments`)}>Show them</a>
            </li>
          ) : null}
        </ul>
      </div>
      {out ? <Msg text={out} bad /> : null}
    </div>
  );
}

function FundsBlock(props: { pid: string; f: Funds }) {
  const { pid, f } = props;
  if (!f.wallets.length) {
    return (
      <section class="funds">
        <h2>Funds</h2>
        <p class="hint">Add a payment method to start taking money.</p>
        <a href={href(`/p/${pid}/providers`)}>Add a payment method</a>
      </section>
    );
  }
  const lp = f.latest_payout;
  return (
    <section class="funds">
      <h2>Funds</h2>
      <p class="hint">Held for you by the platform, ready to withdraw</p>
      <BigMoney amount={f.available} currency="BTC" />
      <div class="line">
        <span>Settling</span>
        <span>{f.settling ? sats(f.settling) : 'Nothing'}</span>
      </div>
      {f.in_payout ? (
        <div class="line">
          <span>In payouts on the way</span>
          <span>{sats(f.in_payout)}</span>
        </div>
      ) : null}
      <div class="line">
        <span>Latest payout</span>
        <span>{lp ? `${fmtAmount(lp.amount, lp.currency)}, ${statusWord(lp.status).toLowerCase()} ${fmtDay(lp.finished_at || lp.created)}` : 'None yet'}</span>
      </div>
      <div class="wallets">
        {f.wallets.map((w, i) => {
          const amount = w.available === null ? (w.settling ? `${sats(w.settling)} settling` : w.currency === 'BTC' ? 'In your wallet' : 'Paid out by provider') : sats(w.available);
          return (
            <div class="w" key={i}>
              <i class={WALLET_SWATCH[i % 5]} aria-hidden="true" />
              <b>{w.name}</b>
              <span class="money">{amount}</span>
              <small>{`${methodName(w.method)}, settles ${w.settles}${w.available !== null && w.settling ? `, ${sats(w.settling)} settling` : ''}`}</small>
            </div>
          );
        })}
      </div>
      <a href={href(`/p/${pid}/balance`)}>Open balance</a>
    </section>
  );
}

function MonthBlock(props: { m: Month }) {
  const { m } = props;
  const figs: [string, string, string, preact.ComponentChildren][] = [];
  m.figures.forEach(f => {
    const c = f.currency;
    figs.push([`Paid, ${c}`, fmtAmount(f.paid, c), fmtAmount(f.paid_prev, c), <Delta now={f.paid} then={f.paid_prev} />]);
    figs.push([`Paid payments, ${c}`, String(f.payments), String(f.payments_prev), <Delta now={f.payments} then={f.payments_prev} />]);
    if (f.success_rate !== null) {
      figs.push([`Success rate, ${c}`, `${Math.round(f.success_rate * 1000) / 10}%`, f.success_rate_prev === null ? 'none' : `${Math.round(f.success_rate_prev * 1000) / 10}%`, <span />]);
    }
    if (f.refunded || f.refunded_prev) figs.push([`Refunded, ${c}`, fmtAmount(f.refunded, c), fmtAmount(f.refunded_prev, c), <span />]);
  });
  const start = new Date(m.start);
  return (
    <section>
      <h2>This month</h2>
      <p class="hint">{`1 to ${new Date().getDate()} ${start.toLocaleDateString([], { month: 'long' })}, against the same days last month`}</p>
      {figs.length ? (
        <dl class="figs">
          {figs.map((x, i) => (
            <div key={i}>
              <dt>{x[0]}</dt>
              <dd>
                {x[1]}
                <span class="prev">
                  {x[2]}
                  {x[3]}
                </span>
              </dd>
            </div>
          ))}
        </dl>
      ) : (
        <p class="muted">No payments this month or last month yet.</p>
      )}
    </section>
  );
}

function HealthBlock(props: { pid: string; list: HomeData['health'] }) {
  return (
    <section>
      <h2>Payment methods</h2>
      {props.list.length ? (
        <ul class="health">
          {props.list.map((h, i) => (
            <li key={i}>
              <i class={h.state} aria-hidden="true" />
              <span>{h.name + (h.platform_managed ? ' (platform)' : '')}</span>
              <small>{`${methodName(h.method)}, ${h.note}`}</small>
            </li>
          ))}
        </ul>
      ) : (
        <p class="muted">
          None set up. <a href={href(`/p/${props.pid}/providers`)}>Add one</a>
        </p>
      )}
    </section>
  );
}

function HomeBody(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const tz = useMemo(() => tzOffset(), []);
  const { data, error } = useLoad(() => api<HomeData>('GET', `/projects/${pid}/home?tz=${tz}`), [pid]);
  return (
    <>
      {!p.trusted ? (
        <p class="banner info" role="note">
          <span>This project is in test mode. Ask an operator to lift the limits to take live payments.</span>
        </p>
      ) : null}
      <PageHead title="Needs attention" sub="Payments and problems that need someone, grouped by what to do." />
      <div class="home">
        <div>
          <AttentionQueue pid={pid} p={p} />
          {data ? <Today data={data} /> : <div />}
          <ActivityFeed pid={pid} />
        </div>
        <aside class="rail" aria-label="Funds and figures">
          {error ? <Msg text={error} bad /> : data ? (
            <>
              <FundsBlock pid={pid} f={data.funds} />
              <MonthBlock m={data.month} />
              <HealthBlock pid={pid} list={data.health} />
            </>
          ) : <Skeleton />}
        </aside>
      </div>
    </>
  );
}

export function Home(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="overview" bare>
      {p => <HomeBody pid={props.pid} p={p} />}
    </ProjectShell>
  );
}
