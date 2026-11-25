import { useEffect, useRef, useState } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import { api, errText, useLoad, type Project } from './api';
import { fmtAmount, fmtFull, methodName, sats, shownAttempt, statusWord, type PayAttempt } from './format';
import { enc, href } from './nav';
import { ProjectShell } from './shell';
import { BigMoney, Kv, Pill, Section, SignedMoney, Table, copyText, useNote } from './ui';
import { confirmDialog, toast } from './dialogs';
import { DeliveryDetail } from './deliveries';

const EVENT_WORDS: Record<string, string> = {
  'payment.paid': 'Paid',
  'payment.late_payment': 'Paid after it closed',
  'payment.expired': 'Expired unpaid',
  'payment.cancelled': 'Cancelled',
  'payment.disputed': 'Dispute opened',
  'payment.dispute_closed': 'Dispute closed',
  'payment.marked_paid': 'Marked paid',
  'payment.note_added': 'Note added',
  'attempt.failed': 'Attempt declined',
  'attempt.received': 'Deposit seen',
  'refund.requested': 'Refund requested',
  'refund.completed': 'Refund completed',
  'refund.failed': 'Refund failed',
};

interface Pay {
  id: string;
  amount: number;
  currency: string;
  status: string;
  reference: string;
  description?: string | null;
  customer_ref?: string | null;
  created: string;
  expires: string;
  paid_at?: string | null;
  pay_url: string;
  receipt_url?: string | null;
  dispute_status?: string | null;
  metadata?: Record<string, unknown> | null;
  attempts: PayAttempt[];
  refunds?: { amount: number; status: string; created: string; manual?: boolean }[];
  line_items?: { name: string; sku?: string; quantity: number; unit_amount: number; discount?: number; tax?: number; total: number }[];
}

interface EventRow {
  created: string;
  type: string;
  detail?: { reason?: string; error?: string; manual?: boolean; dispute?: string };
}

interface Delivery {
  id: number;
  url?: string;
  event_type: string;
  delivered_at?: string | null;
  abandoned_at?: string | null;
  last_attempt_at?: string | null;
  created: string;
  last_status?: number | null;
  last_duration_ms?: number | null;
  attempts: number;
}

interface Detail {
  payment: Pay;
  events?: EventRow[];
  deliveries: Delivery[];
  receipts: { ref: string; amount: number; confirmed: boolean }[];
  customer_email?: string | null;
}

function Timeline(props: { pay: Pay; events: EventRow[]; deliveries: Delivery[] }) {
  const { pay, events, deliveries } = props;
  interface Item { at: string; text: string; sub?: string; tone: string; hint?: boolean; n: number }
  const items: Omit<Item, 'n'>[] = [{ at: pay.created, text: 'Created', sub: pay.reference, tone: 'grey' }];
  pay.attempts.forEach(a => {
    const d = a.details;
    items.push({
      at: (a.paid_at || a.expires)!,
      text: `${methodName(a.method)} attempt ${statusWord(a.status).toLowerCase()}`,
      sub: d ? (d.bolt11 ? `${d.bolt11.slice(0, 32)}\u2026` : d.address || d.checkout_url || '') : undefined,
      tone: a.status === 'paid' ? '' : a.status === 'failed' ? 'bad' : 'grey',
      hint: !a.paid_at,
    });
  });
  events.forEach(e => {
    const d = e.detail ?? {};
    const extra = d.reason || d.error || (d.manual ? 'Marked by hand' : '') || '';
    items.push({
      at: e.created,
      text: EVENT_WORDS[e.type] ?? e.type,
      sub: extra,
      tone: /failed|disput|expired|cancel/.test(e.type) ? (e.type === 'payment.dispute_closed' && d.dispute === 'won' ? '' : 'bad') : '',
    });
  });
  deliveries.forEach(d => {
    items.push({
      at: (d.delivered_at || d.last_attempt_at || d.created)!,
      text: `Webhook ${d.event_type}${d.delivered_at ? ' delivered' : d.abandoned_at ? ' given up' : ' retrying'}`,
      sub: `${d.url ?? ''}${d.last_status ? `, answered ${d.last_status}` : ''}${d.last_duration_ms ? ` in ${d.last_duration_ms} ms` : ''}`,
      tone: d.delivered_at ? '' : 'bad',
    });
  });
  const sorted: Item[] = items.map((it, n) => ({ ...it, n })).sort((a, b) => new Date(b.at).getTime() - new Date(a.at).getTime() || b.n - a.n);
  return (
    <ol class="steps">
      {sorted.map(i => (
        <li class={i.tone} key={i.n}>
          {i.text}
          <small>{`${i.hint ? 'Expires ' : ''}${fmtFull(i.at)}${i.sub ? `, ${i.sub}` : ''}`}</small>
        </li>
      ))}
    </ol>
  );
}

function WhereMoneyWent(props: { pay: Pay }) {
  const { pay } = props;
  const a = shownAttempt(pay);
  if (pay.status === 'pending' || pay.status === 'expired' || pay.status === 'cancelled') {
    return (
      <Section title="Where the money went" cls="flow">
        <p class="muted">{pay.status === 'pending' ? `Nothing has been paid yet. The customer has until ${fmtFull(pay.expires)}.` : 'This payment was not paid, so no money moved.'}</p>
      </Section>
    );
  }
  let net = pay.amount;
  const refunds = (pay.refunds ?? []).filter(r => r.status !== 'failed');
  refunds.forEach(r => {
    net -= r.amount;
  });
  const where = !a
    ? 'Not paid yet'
    : a.platform_managed
      ? 'Added to your balance with the platform'
      : a.method === 'lightning' || a.method === 'onchain'
        ? `Paid into your own ${methodName(a.method)} wallet`
        : `Held by ${methodName(a.method)} and paid out on its schedule`;
  return (
    <Section title="Where the money went" cls="flow" id="money">
      <div class="table-wrap plain">
        <table>
          <tbody>
            <tr>
              <td>Customer paid</td>
              <td class="num muted">{a && a.currency !== pay.currency ? (a.currency === 'BTC' ? sats(a.amount) : fmtAmount(a.amount, a.currency)) : ''}</td>
              <td class="num">{fmtAmount(pay.amount, pay.currency)}</td>
            </tr>
            {a && a.processing_fee ? (
              <tr>
                <td>{"Processing fee added to the customer's total"}</td>
                <td />
                <td class="num muted">{fmtAmount(a.processing_fee, pay.currency)}</td>
              </tr>
            ) : null}
            {a && a.platform_fee ? (
              <tr>
                <td>Platform fee</td>
                <td class="num muted">{sats(a.platform_fee)}</td>
                <td />
              </tr>
            ) : null}
            {refunds.map((r, i) => (
              <tr key={i}>
                <td>{`Refund, ${r.manual ? 'send by hand' : statusWord(r.status).toLowerCase()}`}</td>
                <td />
                <td class="num"><SignedMoney amount={-r.amount} currency={pay.currency} /></td>
              </tr>
            ))}
            <tr class="total">
              <td>{where}</td>
              <td class="num muted">{a && a.net_credited ? sats(a.net_credited) : ''}</td>
              <td class="num">{fmtAmount(net, pay.currency)}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </Section>
  );
}

interface Preview {
  amount: number;
  currency: string;
  refundable: number;
  full: boolean;
  charge: number;
  charge_currency: string;
  fee_kept?: number;
  remaining_after: number;
  destination_required?: boolean;
  method: string;
  attempt_id: number;
}

async function refundFlow(pid: string, id: string, amountText: string, destination: string, out: (t: string, bad?: boolean) => void, reload: () => void) {
  const amount = amountText ? Number(amountText) : null;
  const qs: string[] = [];
  if (amount) qs.push(`amount=${amount}`);
  if (destination) qs.push(`destination=${enc(destination)}`);
  try {
    const pv = await api<Preview>('GET', `/projects/${pid}/payments/${id}/refund-preview${qs.length ? `?${qs.join('&')}` : ''}`);
    if (pv.destination_required && !destination) throw new Error(`A destination is needed for ${pv.method} refunds.`);
    const lines =
      `Refund ${fmtAmount(pv.amount, pv.currency)} of ${fmtAmount(pv.refundable, pv.currency)} refundable (${pv.full ? 'full' : 'partial'}). ` +
      `Sent to the customer: ${fmtAmount(pv.charge, pv.charge_currency)}. ` +
      (pv.fee_kept ? `Processing fee kept: ${fmtAmount(pv.fee_kept, pv.charge_currency)}. ` : '') +
      `Left refundable afterwards: ${fmtAmount(pv.remaining_after, pv.currency)}.`;
    const r = await confirmDialog({ title: 'Confirm refund', body: lines, keep: 'Do not refund', confirm: 'Refund', danger: true });
    if (!r) return;
    const res = await api<{ status: string; error?: string; manual?: boolean; destination?: string }>('POST', `/projects/${pid}/payments/${id}/refund`, { amount, destination: destination || null, attempt_id: pv.attempt_id });
    out(res.manual ? `Refund recorded. Send it by hand from your wallet${res.destination ? ` to ${res.destination}` : ''}.` : `Refund ${res.status}${res.error ? `: ${res.error}` : ''}`, res.status === 'failed');
    reload();
  } catch (e) {
    out(errText(e), true);
  }
}

function PaymentTools(props: { pid: string; id: string; pay: Pay; role: string; out: (t: string, bad?: boolean) => void; reload: () => void }) {
  const { pid, id, pay, role, out, reload } = props;
  const canWrite = role !== 'viewer';
  const base = `/projects/${pid}/payments/${id}`;
  const [notes, setNotes] = useState<{ created: string; body: string }[] | null>(null);
  const [note, setNote] = useState('');
  const loadNotes = () => api<{ notes: { created: string; body: string }[] }>('GET', `${base}/notes`).then(r => setNotes(r.notes));
  useEffect(() => {
    loadNotes();
  }, [id]);
  return (
    <div>
      <div class="notes">
        {notes === null ? null : notes.length ? (
          notes.map((n, i) => (
            <p key={i}>
              <span class="muted">{`${fmtFull(n.created)}  `}</span>
              {n.body}
            </p>
          ))
        ) : (
          <p class="muted">No notes yet.</p>
        )}
      </div>
      {canWrite ? (
        <div class="row">
          <textarea rows={2} placeholder="Internal note for your team, not shown to the customer" aria-label="Internal note" value={note} onInput={e => setNote(e.currentTarget.value)} />
          <button
            onClick={async () => {
              try {
                await api('POST', `${base}/notes`, { body: note });
                setNote('');
                loadNotes();
              } catch (e) {
                out(errText(e), true);
              }
            }}
          >
            Add note
          </button>
        </div>
      ) : null}
      <div class="row">
        <button
          disabled={!canWrite}
          onClick={async () => {
            try {
              await api('POST', `${base}/resend-webhook`, {});
              out('Webhook queued.');
              reload();
            } catch (e) {
              out(errText(e), true);
            }
          }}
        >
          Resend webhook
        </button>
        {(pay.status === 'pending' || pay.status === 'expired') && canWrite ? (
          <button
            onClick={async () => {
              const r = await confirmDialog({
                title: 'Mark as paid',
                body: 'Settles this payment without a provider confirmation. The paid webhook carries a manual flag and the reason below.',
                input: 'Reason',
                keep: 'Cancel',
                confirm: 'Mark paid',
                danger: true,
              });
              if (!r) return;
              try {
                await api('POST', `${base}/mark-paid`, { reason: r.text });
                reload();
              } catch (e) {
                out(errText(e), true);
              }
            }}
          >
            Mark as paid
          </button>
        ) : null}
      </div>
    </div>
  );
}

function DetailBody(props: { pid: string; id: string; p: Project }) {
  const { pid, id, p } = props;
  const { data: d, error, reload } = useLoad(() => api<Detail>('GET', `/projects/${pid}/payments/${id}`), [pid, id]);
  const [noteNode, out] = useNote();
  const [refundOpen, setRefundOpen] = useState(false);
  const [refundAmount, setRefundAmount] = useState('');
  const [refundDest, setRefundDest] = useState('');
  const [deliveryId, setDeliveryId] = useState<number | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const dataRef = useRef(d);
  dataRef.current = d;
  useEffect(() => {
    const h = setInterval(() => {
      const a = document.activeElement as HTMLElement | null;
      const box = root.current;
      if (!box) return;
      if (a && box.contains(a) && /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName)) return;
      if (document.querySelector('.delivery-open') || box.querySelector('details[open]') || box.querySelector('.card')) return;
      reload();
    }, 5000);
    return () => clearInterval(h);
  }, [pid, id]);
  if (error) return <div class="err" role="alert">{error}</div>;
  if (!d) return <div ref={root} />;
  const pay = d.payment;
  const a = shownAttempt(pay);
  const canWrite = p.role !== 'viewer';
  const dispute = pay.dispute_status;

  const toc: [string, string][] = [['money', 'Where the money went'], ['timeline', 'Timeline'], ['attempts', 'Attempts'], ['webhooks', 'Webhooks'], ['cost', 'Cost'], ['customer', 'Customer'], ['metadata', 'Metadata'], ['notes', 'Notes'], ['api', 'API object']];
  const metaKeys = pay.metadata ? Object.keys(pay.metadata) : [];

  const sections: ComponentChildren[] = [
    <WhereMoneyWent pay={pay} key="money" />,
    <Section title="Timeline" id="timeline" key="timeline"><Timeline pay={pay} events={d.events ?? []} deliveries={d.deliveries} /></Section>,
    pay.line_items && pay.line_items.length ? (
      <Section title="Line items" id="lines" key="lines">
        <Table head={['Item', ['Qty', 'num'], ['Unit', 'num'], ['Discount', 'num'], ['Tax', 'num'], ['Total', 'num']]}>
          {pay.line_items.map((l, i) => (
            <tr key={i}>
              <td>{l.name + (l.sku ? ` (${l.sku})` : '')}</td>
              <td class="num">{String(l.quantity)}</td>
              <td class="num">{fmtAmount(l.unit_amount, pay.currency)}</td>
              <td class="num">{l.discount ? fmtAmount(l.discount, pay.currency) : ''}</td>
              <td class="num">{l.tax ? fmtAmount(l.tax, pay.currency) : ''}</td>
              <td class="num">{fmtAmount(l.total, pay.currency)}</td>
            </tr>
          ))}
        </Table>
      </Section>
    ) : null,
    <Section title="Attempts" id="attempts" key="attempts">
      {pay.attempts.length ? (
        <Table head={['Method', 'Status', ['Charged', 'num'], ['Fee', 'num'], 'Reference']}>
          {pay.attempts.map(x => {
            const det = x.details ?? {};
            return (
              <tr key={x.id}>
                <td>{methodName(x.method) + (x.option ? ` ${x.option}` : '') + (x.platform_managed ? ' (platform)' : '')}</td>
                <td><Pill status={x.status} /></td>
                <td class="num">{x.currency === 'BTC' ? sats(x.amount) : fmtAmount(x.amount, x.currency)}</td>
                <td class="num">{x.processing_fee ? fmtAmount(x.processing_fee, pay.currency) : ''}</td>
                <td class="mono trunc">{det.bolt11 ? `${det.bolt11.slice(0, 28)}\u2026` : det.address || det.checkout_url || det.deposit_address || ''}</td>
              </tr>
            );
          })}
        </Table>
      ) : (
        <p class="muted">The customer has not chosen a method yet.</p>
      )}
    </Section>,
    d.receipts.length ? (
      <Section title="Received on the network" id="receipts" key="receipts">
        <Table head={['Transaction', ['Amount', 'num'], 'State']}>
          {d.receipts.map((r, i) => (
            <tr key={i}>
              <td class="mono trunc">{r.ref}</td>
              <td class="num">{sats(r.amount)}</td>
              <td><Pill status={r.confirmed ? 'completed' : 'pending'} word={r.confirmed ? 'Confirmed' : 'Unconfirmed'} /></td>
            </tr>
          ))}
        </Table>
      </Section>
    ) : null,
    <Section title="Webhooks" id="webhooks" key="webhooks">
      {d.deliveries.length ? (
        <Table head={['Endpoint', 'Event', 'Response', ['Tries', 'num'], '']}>
          {d.deliveries.map(x => (
            <tr key={x.id}>
              <td class="trunc">{x.url || 'Default endpoint'}</td>
              <td>{x.event_type}</td>
              <td>
                <Pill
                  status={x.delivered_at ? 'delivered' : x.abandoned_at ? 'failed' : 'waiting'}
                  word={x.last_status ? String(x.last_status) + (x.last_duration_ms ? `, ${x.last_duration_ms} ms` : '') : x.delivered_at ? 'Delivered' : x.abandoned_at ? 'Gave up' : 'Waiting'}
                />
              </td>
              <td class="num">{String(x.attempts)}</td>
              <td>
                <button class="link" type="button" onClick={() => setDeliveryId(x.id)}>Details</button>{' '}
                <button
                  class="link"
                  type="button"
                  disabled={!canWrite}
                  onClick={async () => {
                    await api('POST', `/projects/${pid}/deliveries/${x.id}/replay`, {});
                    toast('Webhook queued.');
                    reload();
                  }}
                >
                  Send again
                </button>
              </td>
            </tr>
          ))}
        </Table>
      ) : (
        <p class="muted">No webhook endpoint received an event for this payment.</p>
      )}
      {deliveryId !== null ? <DeliveryDetail pid={pid} did={deliveryId} /> : null}
    </Section>,
    <Section title="Cost" id="cost" key="cost">
      <Kv
        pairs={[
          ['Charged to the customer', a ? (a.currency === 'BTC' ? `${sats(a.amount)}, ${fmtAmount(a.base_amount ?? 0, pay.currency)}` : fmtAmount(a.amount, a.currency)) : null],
          ['Processing fee', a && a.processing_fee ? fmtAmount(a.processing_fee, pay.currency) : 'None'],
          ['Platform fee', a && a.platform_fee ? sats(a.platform_fee) : 'None'],
          ['Credited to you', a && a.net_credited ? sats(a.net_credited) : a && a.status === 'paid' ? fmtAmount(pay.amount, pay.currency) : null],
        ]}
      />
    </Section>,
    <Section title="Customer" id="customer" key="customer">
      <Kv
        pairs={[
          ['Customer ref', pay.customer_ref ? <a href={href(`/p/${pid}/customers/${enc(pay.customer_ref)}`)}>{pay.customer_ref}</a> : null],
          ['Email', d.customer_email],
          ['Receipt', pay.receipt_url ? <a href={pay.receipt_url} target="_blank" rel="noopener">Open receipt</a> : null],
        ]}
      />
    </Section>,
    <Section title="Metadata" id="metadata" key="metadata">
      {metaKeys.length ? (
        <Kv pairs={metaKeys.map(k => [k, typeof pay.metadata![k] === 'string' ? (pay.metadata![k] as string) : JSON.stringify(pay.metadata![k]), 'mono'])} />
      ) : (
        <p class="muted">No metadata. Pass metadata when you create the payment to see it here.</p>
      )}
    </Section>,
    <Section title="Notes" id="notes" key="notes"><PaymentTools pid={pid} id={id} pay={pay} role={p.role} out={out} reload={reload} /></Section>,
    <Section title="API object" id="api" key="api">
      <details>
        <summary>Show the payment as the API returns it</summary>
        <pre>{JSON.stringify(pay, null, 2)}</pre>
      </details>
    </Section>,
  ].filter(Boolean);

  return (
    <div ref={root}>
      <div class="detail">
        <nav class="toc" aria-label="On this page">
          {toc.map(x => (
            <a
              href={href(`/p/${pid}/payments/${id}`)}
              key={x[0]}
              onClick={e => {
                e.preventDefault();
                document.getElementById(x[0])?.scrollIntoView({ block: 'start' });
              }}
            >
              {x[1]}
            </a>
          ))}
        </nav>
        <div>
          <div>
            <a class="crumb" href={href(`/p/${pid}/payments`)}>Payments</a>
            <div class="dhead">
              <BigMoney amount={pay.amount} currency={pay.currency} />
              <span class="muted">{pay.currency}</span>
              <Pill status={pay.status} />
              {dispute ? <Pill status={dispute === 'open' ? 'disputed' : 'completed'} word={dispute === 'open' ? 'Disputed' : `Dispute ${dispute}`} /> : null}
              <span class="sp" />
              <div class="actions">
                <button type="button" onClick={() => copyText(pay.pay_url, 'Pay link copied')}>Copy pay link</button>
                {pay.status === 'pending' && canWrite ? (
                  <button
                    class="danger"
                    onClick={async () => {
                      const r = await confirmDialog({ title: 'Cancel this payment', body: 'The customer can no longer pay it.', keep: 'Keep it', confirm: 'Cancel payment', danger: true });
                      if (!r) return;
                      try {
                        await api('POST', `/projects/${pid}/payments/${id}/cancel`, {});
                        reload();
                      } catch (e) {
                        out(errText(e), true);
                      }
                    }}
                  >
                    Cancel payment
                  </button>
                ) : null}
                {(pay.status === 'paid' || pay.status === 'partially_refunded') && canWrite ? <button onClick={() => setRefundOpen(true)}>Refund</button> : null}
              </div>
            </div>
            <p class="dsub">
              {[pay.description || pay.reference, pay.customer_ref, a && pay.paid_at ? `paid over ${methodName(a.method)} ${fmtFull(pay.paid_at)}` : `created ${fmtFull(pay.created)}`].filter(Boolean).join(', ')}
            </p>
          </div>
          {noteNode}
          {refundOpen ? (
            <div>
              <div class="card">
                <h3>Refund</h3>
                <div class="row">
                  <input type="number" placeholder="Amount in minor units, blank for all" aria-label="Refund amount" value={refundAmount} onInput={e => setRefundAmount(e.currentTarget.value)} autofocus />
                  <input placeholder="Lightning invoice or address, for bitcoin payments" aria-label="Refund destination" value={refundDest} onInput={e => setRefundDest(e.currentTarget.value)} />
                  <button class="danger" onClick={() => refundFlow(pid, id, refundAmount, refundDest.trim(), out, reload)}>Preview refund</button>
                </div>
              </div>
            </div>
          ) : null}
          {sections}
        </div>
        <aside class="side-card" aria-label="Properties">
          <h2>Properties</h2>
          <Kv
            pairs={[
              ['Payment id', <span class="mono" title={pay.id}>{`${pay.id.slice(0, 20)}\u2026`}</span>],
              ['Reference', pay.reference],
              ['Created', fmtFull(pay.created)],
              ['Expires', pay.status === 'pending' ? fmtFull(pay.expires) : 'Closed'],
              ['Paid', pay.paid_at ? fmtFull(pay.paid_at) : null],
              ['Method', a ? methodName(a.method) : null],
              ['Pay page', <a href={pay.pay_url} target="_blank" rel="noopener">Open</a>],
            ]}
          />
        </aside>
      </div>
    </div>
  );
}

export function PaymentDetail(props: { pid: string; id: string }) {
  return (
    <ProjectShell pid={props.pid} tab="payments" bare>
      {p => <DetailBody pid={props.pid} id={props.id} p={p} />}
    </ProjectShell>
  );
}
