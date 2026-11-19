import { useState } from 'preact/hooks';
import { api, errText, useLoad, type Project } from './api';
import { fmtAmount, fmtDay, fmtWhen, statusWord, when } from './format';
import { enc, go, href } from './nav';
import { ProjectShell } from './shell';
import { Chips, ClickRow, Empty, Kv, Money, Msg, Pill, Section, Skeleton, Table, TextField, SelectField, useNote } from './ui';
import { confirmDialog, toast } from './dialogs';

function CustomersList(props: { pid: string }) {
  const { pid } = props;
  const [q, setQ] = useState('');
  const [applied, setApplied] = useState('');
  const { data, error } = useLoad(
    () => api<{ customers: { customer_ref: string; payments: number; paid: number; last_payment?: string }[] }>('GET', `/projects/${pid}/customers${applied.trim() ? `?q=${enc(applied.trim())}` : ''}`),
    [pid, applied],
  );
  return (
    <>
      <div class="filterbar">
        <input type="search" placeholder="Customer ref starts with" aria-label="Search customers" value={q} onInput={e => setQ(e.currentTarget.value)} onKeyDown={e => e.key === 'Enter' && setApplied(q)} />
        <button onClick={() => setApplied(q)}>Search</button>
      </div>
      <div>
        {error ? (
          <Msg text={error} bad />
        ) : !data ? (
          <Skeleton />
        ) : data.customers.length ? (
          <Table head={['Customer', ['Payments', 'num'], ['Paid', 'num'], 'Last payment']}>
            {data.customers.map(c => (
              <ClickRow key={c.customer_ref} onOpen={() => go(`/p/${pid}/customers/${enc(c.customer_ref)}`)}>
                <td>{c.customer_ref}</td>
                <td class="num">{String(c.payments)}</td>
                <td class="num">{String(c.paid)}</td>
                <td class="muted">{fmtWhen(c.last_payment)}</td>
              </ClickRow>
            ))}
          </Table>
        ) : (
          <Empty title="No customers yet." hint="Pass customer_ref when your app creates a payment and customers appear here." />
        )}
      </div>
    </>
  );
}

export function Customers(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="customers" sub="Everyone who paid you, by the customer_ref your app sends.">
      {() => <CustomersList pid={props.pid} />}
    </ProjectShell>
  );
}

interface CustomerData {
  customer_ref: string;
  email?: string | null;
  paid_totals: Record<string, number>;
  payments: { id: string; amount: number; currency: string; status: string; reference: string; created: string }[];
  saved_methods: { id: number; brand?: string; last4?: string; exp_month?: number; exp_year?: number }[];
  subscriptions: { id: number; plan_name?: string; plan_id: number; amount: number | null; currency: string; status: string; cancel_at_period_end?: boolean; next_charge?: string }[];
}

function CustomerBody(props: { pid: string; refText: string; p: Project }) {
  const { pid, refText, p } = props;
  const { data: c, error, reload } = useLoad(() => api<CustomerData>('GET', `/projects/${pid}/customers/${enc(refText)}`), [pid, refText]);
  const [note, out] = useNote();
  if (error) return <Msg text={error} bad />;
  if (!c) return <Skeleton />;
  const totals = Object.keys(c.paid_totals).map(k => fmtAmount(c.paid_totals[k], k)).join(', ');
  return (
    <>
      <Section title="Customer">
        <Kv pairs={[['Customer ref', c.customer_ref], ['Email', c.email], ['Paid in total', totals || 'Nothing yet']]} />
      </Section>
      <Section title="Payments">
        {c.payments.length ? (
          <Table head={[['Amount', 'num'], 'Status', 'Reference', 'Created']}>
            {c.payments.map(x => (
              <ClickRow key={x.id} onOpen={() => go(`/p/${pid}/payments/${x.id}`)}>
                <td class="amt">{fmtAmount(x.amount, x.currency)}</td>
                <td><Pill status={x.status} /></td>
                <td>{x.reference}</td>
                <td class="muted">{fmtWhen(x.created)}</td>
              </ClickRow>
            ))}
          </Table>
        ) : (
          <p class="muted">No payments.</p>
        )}
      </Section>
      <Section title="Saved cards">
        {c.saved_methods.length ? (
          <Table head={['Card', 'Expires', '']}>
            {c.saved_methods.map(m => (
              <tr key={m.id}>
                <td>{`${m.brand || 'Card'} ending ${m.last4 || '????'}`}</td>
                <td class="muted">{m.exp_month ? `${m.exp_month}/${m.exp_year}` : ''}</td>
                <td class="num">
                  <button
                    class="link"
                    type="button"
                    disabled={p.role === 'viewer'}
                    onClick={async () => {
                      const r = await confirmDialog({ title: 'Delete saved card', body: `Delete ${m.brand || 'card'} ending ${m.last4 || '????'}? The customer will have to enter it again.`, keep: 'Keep it', confirm: 'Delete', danger: true });
                      if (!r) return;
                      try {
                        await api('DELETE', `/projects/${pid}/customers/${enc(refText)}/saved-methods/${m.id}`);
                        toast('Saved card deleted.');
                        reload();
                      } catch (e) {
                        out(errText(e), true);
                      }
                    }}
                  >
                    Delete
                  </button>
                </td>
              </tr>
            ))}
          </Table>
        ) : (
          <p class="muted">No saved cards.</p>
        )}
      </Section>
      <Section title="Subscriptions">
        {c.subscriptions.length ? (
          <Table head={['Plan', ['Amount', 'num'], 'Status', 'Next charge']}>
            {c.subscriptions.map(x => (
              <ClickRow key={x.id} onOpen={() => go(`/p/${pid}/subscriptions/${x.id}`)}>
                <td>{x.plan_name || `Plan ${x.plan_id}`}</td>
                <td class="num">{x.amount === null ? '' : fmtAmount(x.amount, x.currency)}</td>
                <td><Pill status={x.status} word={x.cancel_at_period_end ? `${statusWord(x.status)}, ends` : null} /></td>
                <td class="muted">{x.next_charge ? fmtDay(x.next_charge) : ''}</td>
              </ClickRow>
            ))}
          </Table>
        ) : (
          <p class="muted">No subscriptions.</p>
        )}
      </Section>
      {note}
    </>
  );
}

export function CustomerDetail(props: { pid: string; customerRef: string }) {
  return (
    <ProjectShell pid={props.pid} tab="customers" title={props.customerRef}>
      {p => <CustomerBody pid={props.pid} refText={props.customerRef} p={p} />}
    </ProjectShell>
  );
}

interface Plan {
  id: number;
  name: string;
  amount: number;
  currency: string;
  interval: string;
  interval_count: number;
  trial_days: number;
  grace_days: number;
  active: boolean;
}

const intervalText = (p: Plan) => ` every ${p.interval_count > 1 ? `${p.interval_count} ${p.interval}s` : p.interval}`;

function PlansBody(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const { data: rows, error, reload } = useLoad(() => api<Plan[]>('GET', `/projects/${pid}/plans`), [pid]);
  const [name, setName] = useState('');
  const [amount, setAmount] = useState('');
  const [cur, setCur] = useState('EUR');
  const [unit, setUnit] = useState('month');
  const [count, setCount] = useState('1');
  const [trial, setTrial] = useState('0');
  const [grace, setGrace] = useState('3');
  const [note, out] = useNote();
  return (
    <>
      <Section title="Plans">
        <div>
          {error ? (
            <Msg text={error} bad />
          ) : !rows ? (
            <Skeleton />
          ) : rows.length ? (
            <Table head={['Plan', 'Price', ['Trial', 'num'], ['Grace', 'num'], 'Status', '']}>
              {rows.map(r => (
                <tr key={r.id}>
                  <td>
                    <b>{r.name}</b>
                    <span class="muted">{`  #${r.id}`}</span>
                  </td>
                  <td><span><Money amount={r.amount} currency={r.currency} />{intervalText(r)}</span></td>
                  <td class="num">{`${r.trial_days} days`}</td>
                  <td class="num">{`${r.grace_days} days`}</td>
                  <td><Pill status={r.active ? 'active' : 'expired'} word={r.active ? 'Active' : 'Archived'} /></td>
                  <td class="num">
                    {r.active && p.role !== 'viewer' ? (
                      <button
                        class="link"
                        type="button"
                        onClick={async () => {
                          const v = await confirmDialog({ title: `Archive ${r.name}`, body: 'No new subscriptions can use it. Existing subscriptions keep billing.', keep: 'Keep it', confirm: 'Archive plan' });
                          if (!v) return;
                          try {
                            await api('POST', `/projects/${pid}/plans/${r.id}/archive`, {});
                            reload();
                          } catch (e) {
                            out(errText(e), true);
                          }
                        }}
                      >
                        Archive
                      </button>
                    ) : null}
                  </td>
                </tr>
              ))}
            </Table>
          ) : (
            <Empty title="No plans yet." hint="Create a plan below, then subscribe customers to it." />
          )}
        </div>
      </Section>
      <Section title="Create a plan">
        <div class="card form">
          <TextField label="Name" placeholder="Pro" value={name} onInput={setName} />
          <TextField label="Amount in minor units" type="number" placeholder="1500" value={amount} onInput={setAmount} />
          <TextField label="Currency" value={cur} onInput={setCur} />
          <SelectField label="Billed every" value={unit} onChange={setUnit} options={['month', 'year', 'week', 'day'].map(u => [u, u.charAt(0).toUpperCase() + u.slice(1)])} />
          <TextField label="Number of intervals" type="number" value={count} onInput={setCount} />
          <TextField label="Trial days" type="number" value={trial} onInput={setTrial} />
          <TextField label="Grace days for pay-link methods" type="number" value={grace} onInput={setGrace} />
          <div class="row">
            <button
              class="primary"
              disabled={p.role === 'viewer'}
              onClick={async () => {
                try {
                  await api('POST', `/projects/${pid}/plans`, { name, amount: Number(amount), currency: cur, interval: unit, interval_count: Number(count), trial_days: Number(trial), grace_days: Number(grace) });
                  setName('');
                  toast('Plan created.');
                  reload();
                } catch (e) {
                  out(errText(e), true);
                }
              }}
            >
              Create plan
            </button>
          </div>
          {note}
        </div>
      </Section>
    </>
  );
}

export function Plans(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="plans" sub="Prices you bill customers again and again.">
      {p => <PlansBody pid={props.pid} p={p} />}
    </ProjectShell>
  );
}

interface Sub {
  id: number;
  customer_ref: string;
  status: string;
  current_period_end?: string;
  current_period_start?: string;
  next_charge?: string;
  plan_id: number;
  cancel_at_period_end?: boolean;
  credit: number;
  pay_url?: string;
  periods: { index: number; period_start: string; period_end: string; amount: number; currency: string; status: string; failures: number; payment_id?: string }[];
}

function SubscriptionsBody(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const [current, setCurrent] = useState('');
  const { data: rows, error } = useLoad(() => api<Sub[]>('GET', `/projects/${pid}/subscriptions${current ? `?status=${current}` : ''}`), [pid, current]);
  const { data: plans } = useLoad(() => api<Plan[]>('GET', `/projects/${pid}/plans`).catch(() => [] as Plan[]), [pid]);
  const [customer, setCustomer] = useState('');
  const [planId, setPlanId] = useState('');
  const [saved, setSaved] = useState('');
  const [note, out] = useNote();
  const active = (plans ?? []).filter(r => r.active);
  const chosen = planId || (active[0] ? String(active[0].id) : '');
  return (
    <>
      <Section title="Subscriptions" aside={<Chips options={[['', 'All'], ['active', 'Active'], ['trialing', 'Trial'], ['past_due', 'Past due'], ['paused', 'Paused'], ['cancelled', 'Cancelled']]} value={current} onPick={setCurrent} label="Status" />}>
        <div>
          {error ? (
            <Msg text={error} bad />
          ) : !rows ? (
            <Skeleton />
          ) : rows.length ? (
            <Table head={['Subscription', 'Customer', 'Status', 'Period ends', 'Next charge']}>
              {rows.map(r => (
                <ClickRow key={r.id} onOpen={() => go(`/p/${pid}/subscriptions/${r.id}`)}>
                  <td class="mono">{`#${r.id}`}</td>
                  <td>{r.customer_ref}</td>
                  <td><Pill status={r.status} /></td>
                  <td class="muted">{when(r.current_period_end)}</td>
                  <td class="muted">{when(r.next_charge) || 'None'}</td>
                </ClickRow>
              ))}
            </Table>
          ) : (
            <Empty title={current ? 'None with this status.' : 'No subscriptions yet.'} hint="Subscribe a customer to a plan below, or from your app through the API." />
          )}
        </div>
      </Section>
      <Section title="Subscribe a customer">
        <div class="card form">
          <TextField label="Customer" placeholder="customer_ref" value={customer} onInput={setCustomer} />
          <div class="field">
            <label for="sub-plan">Plan</label>
            <select id="sub-plan" aria-label="Plan" value={chosen} onChange={e => setPlanId(e.currentTarget.value)}>
              {active.map(r => (
                <option value={String(r.id)} key={r.id}>{`${r.name}, ${fmtAmount(r.amount, r.currency)}${intervalText(r)}`}</option>
              ))}
            </select>
          </div>
          <TextField label="Saved card id" type="number" placeholder="Optional" value={saved} onInput={setSaved} />
          <div class="row">
            <button
              class="primary"
              disabled={p.role === 'viewer'}
              onClick={async () => {
                const b: Record<string, unknown> = { customer_ref: customer, plan_id: Number(chosen) };
                if (saved) b.saved_method_id = Number(saved);
                try {
                  const r = await api<{ id: number }>('POST', `/projects/${pid}/subscriptions`, b);
                  go(`/p/${pid}/subscriptions/${r.id}`);
                } catch (e) {
                  out(errText(e), true);
                }
              }}
            >
              Subscribe customer
            </button>
          </div>
          {note}
        </div>
      </Section>
    </>
  );
}

export function Subscriptions(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="subscriptions" sub="Customers billed on a schedule.">
      {p => <SubscriptionsBody pid={props.pid} p={p} />}
    </ProjectShell>
  );
}

function SubscriptionBody(props: { pid: string; id: string; p: Project }) {
  const { pid, id, p } = props;
  const { data, error, reload } = useLoad(async () => {
    const [s, plans] = await Promise.all([api<Sub>('GET', `/projects/${pid}/subscriptions/${id}`), api<Plan[]>('GET', `/projects/${pid}/plans`)]);
    return { s, plans };
  }, [pid, id]);
  const [note, out] = useNote();
  const [newPlan, setNewPlan] = useState('');
  const [proration, setProration] = useState('none');
  if (error) return <Msg text={error} bad />;
  if (!data) return <Skeleton />;
  const { s, plans } = data;
  const cur = plans.find(x => x.id === s.plan_id);
  const others = plans.filter(x => x.active && x.id !== s.plan_id);
  const canWrite = p.role !== 'viewer';
  const run = async (action: string, payload?: unknown) => {
    try {
      await api('POST', `/projects/${pid}/subscriptions/${id}/${action}`, payload ?? {});
      reload();
    } catch (e) {
      out(errText(e), true);
    }
  };
  const live = s.status === 'active' || s.status === 'trialing';
  return (
    <>
      <Section title="Details">
        <Kv
          pairs={[
            ['Customer', <a href={href(`/p/${pid}/customers/${enc(s.customer_ref)}`)}>{s.customer_ref}</a>],
            ['Status', <Pill status={s.status} />],
            [
              'Plan',
              cur ? (
                <span>
                  <a href={href(`/p/${pid}/plans`)}>{cur.name}</a>
                  {', '}
                  <Money amount={cur.amount} currency={cur.currency} />
                  {intervalText(cur)}
                </span>
              ) : (
                <a href={href(`/p/${pid}/plans`)}>{`Plan ${s.plan_id}`}</a>
              ),
            ],
            ['Current period', `${when(s.current_period_start)} to ${when(s.current_period_end)}`],
            ['Next charge', when(s.next_charge) || 'None'],
            ['Cancels at period end', s.cancel_at_period_end ? 'Yes' : 'No'],
            ['Credit', <Money amount={s.credit} currency={cur ? cur.currency : 'EUR'} />],
            s.pay_url ? ['Pay link', <a href={s.pay_url}>Open pay link</a>] : null,
          ]}
        />
        <div class="row">
          {canWrite && live ? <button onClick={() => run('pause')}>Pause</button> : null}
          {canWrite && s.status === 'paused' ? <button onClick={() => run('resume')}>Resume</button> : null}
          {canWrite && s.status !== 'cancelled' ? (
            <>
              {!s.cancel_at_period_end ? (
                <button
                  onClick={async () => {
                    const v = await confirmDialog({ title: 'Cancel at the end of this period?', body: `The customer keeps access until ${when(s.current_period_end)} and is not charged again.`, keep: 'Keep subscription', confirm: 'Cancel at period end', danger: true });
                    if (v) run('cancel', { at_period_end: true });
                  }}
                >
                  Cancel at period end
                </button>
              ) : null}
              <button
                class="danger"
                onClick={async () => {
                  const v = await confirmDialog({ title: 'Cancel this subscription now?', body: 'Billing stops at once and any unpaid invoice is voided.', keep: 'Keep subscription', confirm: 'Cancel subscription', danger: true });
                  if (v) run('cancel', {});
                }}
              >
                Cancel now
              </button>
            </>
          ) : null}
        </div>
        {note}
      </Section>
      {canWrite && live ? (
        <Section title="Change plan">
          <div class="row">
            <select aria-label="New plan" value={newPlan} onChange={e => setNewPlan(e.currentTarget.value)}>
              <option value="">{others.length ? 'Choose a plan' : 'No other active plans'}</option>
              {others.map(x => (
                <option value={String(x.id)} key={x.id}>{`${x.name}, ${fmtAmount(x.amount, x.currency)}${intervalText(x)}`}</option>
              ))}
            </select>
            <select aria-label="Proration" value={proration} onChange={e => setProration(e.currentTarget.value)}>
              <option value="none">No proration</option>
              <option value="prorate">Prorate remaining days</option>
            </select>
            <button
              onClick={() => {
                if (!newPlan) {
                  out('Choose a plan first.', true);
                  return;
                }
                run('change-plan', { plan_id: Number(newPlan), proration });
              }}
            >
              Change plan
            </button>
          </div>
        </Section>
      ) : null}
      <Section title="Invoices">
        {s.periods.length ? (
          <Table head={['Period', 'From', 'To', ['Amount', 'num'], 'Status', ['Failures', 'num'], 'Payment']}>
            {s.periods.map(r => (
              <tr key={r.index}>
                <td>{String(r.index)}</td>
                <td class="muted">{when(r.period_start)}</td>
                <td class="muted">{when(r.period_end)}</td>
                <td class="num"><Money amount={r.amount} currency={r.currency} /></td>
                <td><Pill status={r.status === 'open' ? 'pending' : r.status === 'void' ? 'cancelled' : r.status} word={r.status === 'void' ? 'Void' : r.status === 'open' ? 'Open' : null} /></td>
                <td class="num">{String(r.failures)}</td>
                <td>{r.payment_id ? <a href={href(`/p/${pid}/payments/${r.payment_id}`)} class="mono">{r.payment_id.slice(0, 10)}</a> : null}</td>
              </tr>
            ))}
          </Table>
        ) : (
          <p class="muted">No invoices yet.</p>
        )}
      </Section>
    </>
  );
}

export function SubscriptionDetail(props: { pid: string; id: string }) {
  return (
    <ProjectShell pid={props.pid} tab="subscriptions" title={`Subscription #${props.id}`}>
      {p => <SubscriptionBody pid={props.pid} id={props.id} p={p} />}
    </ProjectShell>
  );
}

interface Invoice {
  id: number;
  period_start: number;
  period_end: number;
  lines: { name: string; quantity: number }[];
  total: number;
  currency: string;
  status: string;
  past_due?: boolean;
  payment_method?: string;
  pay_url?: string;
  receipt_url?: string;
}

interface PlatformBill {
  account: {
    plan: { name: string; monthly_price: number; currency: string };
    standing: string;
    usage_count: number;
    usage_volume: number;
    estimated_usage_fee: number;
    credit: number;
  };
  invoices: Invoice[];
}

const INV_PILL: Record<string, string> = { open: 'pending', paid: 'ok', waived: 'cancelled' };
const STANDING_TEXT: Record<string, string> = {
  current: 'Up to date',
  warned: 'Past due: pay soon to avoid restrictions',
  restricted: 'Restricted: only refunds and withdrawals work',
  disabled: 'New payments disabled. Refunds and withdrawals still work',
};
const methodLabel = (m?: string) => (m === 'lightning' ? 'Lightning' : m === 'stripe' || m === 'revolut' ? 'Card' : (m ?? ''));

function PlatformBillBody(props: { pid: string }) {
  const { data: b, error } = useLoad(() => api<PlatformBill>('GET', `/projects/${props.pid}/platform-billing`), [props.pid]);
  if (error) return <div class="err" role="alert">{error}</div>;
  if (!b) return null;
  const a = b.account;
  const bad = a.standing !== 'current';
  return (
    <>
      <div class="card">
        <h3>{`${a.plan.name} plan`}</h3>
        <p class="standing">
          <Pill status={bad ? 'failed' : 'completed'} word={bad ? 'Action needed' : 'Up to date'} />
          {STANDING_TEXT[a.standing]}
        </p>
        <p class="muted">
          {`Monthly price ${fmtAmount(a.plan.monthly_price, a.plan.currency)}. This period: ${a.usage_count} settled payments, ${fmtAmount(a.usage_volume, a.plan.currency)} volume, estimated usage fee ${fmtAmount(a.estimated_usage_fee, a.plan.currency)}. Credit: ${fmtAmount(a.credit, a.plan.currency)}.`}
        </p>
      </div>
      {!b.invoices.length ? (
        <Section title="Invoices"><p class="muted">No invoices yet. The first one is made at the end of this billing period.</p></Section>
      ) : (
        <Section title="Invoices">
          <Table head={['Invoice', 'Period', 'Lines', ['Total', 'num'], 'Status', 'Paid with', '']}>
            {b.invoices.map(i => (
              <tr key={i.id}>
                <td class="mono">{String(i.id)}</td>
                <td class="muted">{`${fmtDay(new Date(i.period_start * 1000).toISOString())} to ${fmtDay(new Date(i.period_end * 1000).toISOString())}`}</td>
                <td class="trunc">{i.lines.map(l => `${l.name} x${l.quantity}`).join('; ')}</td>
                <td class="num"><Money amount={i.total} currency={i.currency} /></td>
                <td>
                  {i.past_due ? <Pill status="failed" word="Past due" /> : <Pill status={INV_PILL[i.status] ?? i.status} word={i.status === 'open' ? 'Open' : i.status === 'paid' ? 'Paid' : 'Waived'} />}
                </td>
                <td class="muted">{methodLabel(i.payment_method)}</td>
                <td>
                  {i.pay_url ? <a href={i.pay_url} target="_blank" rel="noopener">Pay</a> : null}
                  {i.receipt_url ? <a href={i.receipt_url} target="_blank" rel="noopener">Receipt</a> : null}
                </td>
              </tr>
            ))}
          </Table>
        </Section>
      )}
    </>
  );
}

export function PlatformBill(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="billing" sub="What this project pays the platform.">
      {() => <PlatformBillBody pid={props.pid} />}
    </ProjectShell>
  );
}
