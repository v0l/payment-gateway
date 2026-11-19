import { useEffect, useState } from 'preact/hooks';
import { api, errText, useLoad, type Project } from './api';
import { fmtAmount, fmtDay, fmtWhen, methodName, sats, statusWord, tzOffset } from './format';
import { go, href } from './nav';
import { ProjectShell } from './shell';
import { BigMoney, ClickRow, Empty, Figure, Money, Msg, Pill, Section, SignedMoney, Skeleton, Table } from './ui';
import { confirmDialog, toast } from './dialogs';

const WALLET_CLASSES = ['w1', 'w2', 'w3', 'w4', 'w5'];

interface StatementRow {
  id: number;
  created: string;
  transaction_kind: string;
  payment_description?: string | null;
  reference_kind?: string;
  customer_ref?: string | null;
  method?: string;
  amount: number;
  currency: string;
  balance_after: number;
  payment_id?: string | null;
}

function statementLabel(e: StatementRow): string {
  switch (e.transaction_kind) {
    case 'payment_received':
      return e.payment_description ? `Payment, ${e.payment_description}` : 'Payment received';
    case 'refund':
      return e.payment_description ? `Refund of ${e.payment_description}` : 'Refund';
    case 'payout_requested':
      return 'Payout requested';
    case 'payout_cancelled':
      return 'Payout returned';
    case 'payout_completed':
      return 'Payout sent';
    case 'fee':
      return 'Platform fee';
    case 'adjustment':
      return e.reference_kind === 'refund' ? 'Refund reversed' : 'Adjustment';
    default:
      return statusWord(e.transaction_kind);
  }
}

function Statement(props: { pid: string }) {
  const { pid } = props;
  const [rows, setRows] = useState<StatementRow[] | null>(null);
  const [more, setMore] = useState(false);
  const [error, setError] = useState('');
  const load = async (append: boolean) => {
    const before = append && rows?.length ? rows[rows.length - 1].id : null;
    try {
      const page = await api<StatementRow[]>('GET', `/projects/${pid}/statement?limit=50${before ? `&before=${before}` : ''}`);
      setRows(prev => (append && prev ? [...prev, ...page] : page));
      setMore(page.length === 50);
    } catch (e) {
      setError(errText(e));
    }
  };
  useEffect(() => {
    load(false);
  }, [pid]);
  if (error) return <Msg text={error} bad />;
  if (!rows) return null;
  return (
    <>
      {rows.length ? (
        <Table head={['When', 'Entry', ['Amount', 'num'], ['Balance', 'num']]}>
          {rows.map(e => {
            const cells = (
              <>
                <td class="muted">{fmtWhen(e.created)}</td>
                <td class="two">
                  {statementLabel(e)}
                  <span class="sub2">{[e.customer_ref, methodName(e.method)].filter(Boolean).join(', ')}</span>
                </td>
                <td class="num"><SignedMoney amount={e.amount} currency={e.currency} /></td>
                <td class="num">{fmtAmount(e.balance_after, e.currency)}</td>
              </>
            );
            return e.payment_id ? <ClickRow key={e.id} onOpen={() => go(`/p/${pid}/payments/${e.payment_id}`)}>{cells}</ClickRow> : <tr key={e.id}>{cells}</tr>;
          })}
        </Table>
      ) : (
        <Empty title="No balance changes yet." hint="Payments through a platform payment method add to this balance. Your own wallets and card providers pay you directly." />
      )}
      <div class="more">{more ? <button onClick={() => load(true)}>Older</button> : <span />}</div>
    </>
  );
}

interface Funds {
  wallets: { name: string; method: string; available: number | null; settling: number; settles: string }[];
  available: number;
  settling: number;
  in_payout: number;
}

interface PayoutsData {
  limits: { daily_max: number; used_24h: number; min_amount: number; auto_threshold: number; destination_delay_hours: number };
  withdrawals: { id: number; amount: number; currency: string; fee?: number; status: string; purpose: string; destination_label: string; created: string; finished_at?: string }[];
  destinations: Dest[];
  node_ready: boolean;
}

interface Dest {
  id: number;
  label: string;
  masked: string;
  kind: string;
  usable: boolean;
  verified: boolean;
  usable_from: string;
  challenge?: string;
}

function BalanceBody(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const tz = tzOffset();
  const { data, error } = useLoad(
    async () => {
      const [home, po] = await Promise.all([api<{ funds: Funds }>('GET', `/projects/${pid}/home?tz=${tz}`), api<PayoutsData>('GET', `/projects/${pid}/payouts`).catch(() => null)]);
      return { f: home.funds, po };
    },
    [pid],
  );
  const hero = (() => {
    if (error) return <Msg text={error} bad />;
    if (!data) return <Skeleton />;
    const { f } = data;
    const total = f.available + f.settling + f.in_payout;
    const withAmount = f.wallets.filter(w => w.available !== null || w.settling);
    return (
      <>
        <div>
          <h1>Available to withdraw</h1>
          <BigMoney amount={f.available} currency="BTC" />
          <div class="balmeta">
            <span><b>{sats(f.settling)}</b> settling</span>
            <span><b>{sats(f.in_payout)}</b> in payouts on the way</span>
            <span><b>{sats(total)}</b> total</span>
          </div>
          <div class="comp" role="img" aria-label="Balance by wallet">
            {withAmount.map((w, i) => (
              <i class={WALLET_CLASSES[i % 5]} title={w.name} style={{ flexGrow: String(Math.max(1, (w.available ?? 0) + w.settling)) }} key={i} />
            ))}
          </div>
          <div class="actions">
            <a class="btn primary" href={href(`/p/${pid}/payouts`)}>Withdraw</a>
            <a class="btn" href={href(`/p/${pid}/payments`)}>See payments</a>
          </div>
        </div>
        {f.wallets.length ? (
          <Table head={['Wallet', ['Available', 'num']]} cls="plain">
            {f.wallets.map((w, i) => (
              <tr key={i}>
                <td class="two">
                  <span class={`sw ${WALLET_CLASSES[i % 5]}`} aria-hidden="true" />
                  {w.name}
                  <span class="sub2">{`${methodName(w.method)}, settles ${w.settles}`}</span>
                </td>
                <td class="num two nowrap">
                  <b>{w.available === null ? (w.settling ? sats(w.settling) : 'None held') : sats(w.available)}</b>
                  <span class="sub2">{w.available === null ? (w.settling ? 'settling, then in your own wallet' : 'Paid to your own wallet') : w.settling ? `${sats(w.settling)} settling` : 'Nothing settling'}</span>
                </td>
              </tr>
            ))}
          </Table>
        ) : (
          <div class="empty">
            <b>No payment methods yet.</b>
            <a href={href(`/p/${pid}/providers`)}>Add a payment method</a>
          </div>
        )}
      </>
    );
  })();
  const aside = (() => {
    if (!data) return <Skeleton />;
    const { f, po } = data;
    const lim = po?.limits;
    const left = lim ? Math.max(lim.daily_max - lim.used_24h, 0) : 0;
    const next = po?.withdrawals.find(w => ['requested', 'approved', 'sending'].includes(w.status));
    const recent = po ? po.withdrawals.filter(w => w.purpose !== 'verify').slice(0, 4) : [];
    return (
      <>
        <h2>Payouts</h2>
        {po && lim ? (
          <>
            <div class="payout-next">
              {next ? (
                <>
                  <BigMoney amount={next.amount} currency={next.currency} />
                  <p>{`${statusWord(next.status)} to ${next.destination_label}, requested ${fmtWhen(next.created)}`}</p>
                </>
              ) : (
                <p>{`No payout on the way. You can withdraw up to ${sats(Math.min(left, Math.max(f.available, 0)))} now.`}</p>
              )}
            </div>
            <h2>Recent payouts</h2>
            {recent.length ? (
              <ul class="rules">
                {recent.map(w => (
                  <li key={w.id}>
                    <b>{fmtDay(w.finished_at || w.created)}</b>
                    <span>{`${sats(w.amount)}, ${statusWord(w.status).toLowerCase()}`}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p class="muted">None yet.</p>
            )}
            <h2>Limits</h2>
            <ul class="rules">
              <li><b>Left in the next 24 hours</b><span>{sats(left)}</span></li>
              <li><b>Smallest payout</b><span>{sats(lim.min_amount)}</span></li>
              <li><b>Sent without review up to</b><span>{p.trusted ? sats(lim.auto_threshold) : 'Every payout is reviewed'}</span></li>
            </ul>
          </>
        ) : null}
        <h2>When money becomes available</h2>
        <ul class="rules">
          {[['Lightning', 'Instantly'], ['On-chain', 'After confirmations'], ['Card and PayPal', "On the provider's payout schedule"], ['Platform methods', 'In this balance straight away']].map(x => (
            <li key={x[0]}>
              <b>{x[0]}</b>
              <span>{x[1]}</span>
            </li>
          ))}
        </ul>
      </>
    );
  })();
  return (
    <>
      <div class="balance-hero">{hero}</div>
      <div class="balance-grid">
        <Section title="Balance changes">
          <p class="muted">Every payment, refund and payout that moved your platform balance, with the balance after it.</p>
          <Statement pid={pid} />
        </Section>
        <aside>{aside}</aside>
      </div>
    </>
  );
}

export function Balance(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="balance" bare>
      {p => <BalanceBody pid={props.pid} p={p} />}
    </ProjectShell>
  );
}

function WithdrawalPill(props: { w: { status: string; purpose: string } }) {
  return <Pill status={props.w.status} word={statusWord(props.w.status) + (props.w.purpose === 'verify' ? ', test' : '')} />;
}

function DestinationRow(props: { d: Dest; pid: string; act: (m: string, path: string, payload: unknown, done?: string) => Promise<unknown> }) {
  const { d, pid, act } = props;
  const [sig, setSig] = useState('');
  return (
    <>
      <tr>
        <td>{d.label}</td>
        <td class="mono">{d.masked}</td>
        <td class="muted">{d.kind.replace(/_/g, ' ')}</td>
        <td>
          {d.usable ? <Pill status="completed" word="Ready" /> : d.verified ? <Pill status="pending" word="Waiting" /> : <Pill status="pending" word="Not verified" />}
          {d.verified && !d.usable ? <div class="muted">{`Usable from ${new Date(d.usable_from).toLocaleString()}`}</div> : null}
        </td>
        <td>
          <div class="row">
            {!d.verified ? <button onClick={() => act('POST', `/projects/${pid}/payout-destinations/${d.id}/verify`, {}, 'Test payout queued')}>Send test payout</button> : null}
            <button
              onClick={async () => {
                const v = await confirmDialog({ title: 'Remove this destination?', body: `You can no longer withdraw to ${d.label}. Withdrawals already sent are not affected.`, keep: 'Keep destination', confirm: 'Remove destination', danger: true });
                if (v) act('DELETE', `/projects/${pid}/payout-destinations/${d.id}`, undefined, 'Destination removed');
              }}
            >
              Remove
            </button>
          </div>
        </td>
      </tr>
      {d.challenge ? (
        <tr>
          <td colSpan={5}>
            <div class="muted">Or prove you own a legacy address by signing this message:</div>
            <div class="mono-wrap">{d.challenge}</div>
            <div class="row">
              <input placeholder="Signed message (base64)" aria-label="Signed message" value={sig} onInput={e => setSig(e.currentTarget.value)} />
              <button onClick={() => act('POST', `/projects/${pid}/payout-destinations/${d.id}/verify`, { signature: sig }, 'Destination verified')}>Verify signature</button>
            </div>
          </td>
        </tr>
      ) : null}
    </>
  );
}

function PayoutsBody(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const { data: r, error, reload } = useLoad(() => api<PayoutsData>('GET', `/projects/${pid}/payouts`), [pid]);
  const [alertMsg, setAlert] = useState('');
  const [kind, setKind] = useState('onchain');
  const [label, setLabel] = useState('');
  const [value, setValue] = useState('');
  const [destId, setDestId] = useState('');
  const [amountSats, setAmountSats] = useState('');
  const [inv, setInv] = useState('');
  const act = async (method: string, path: string, payload: unknown, done?: string) => {
    setAlert('');
    try {
      const res = await api(method, path, payload);
      if (done) toast(done);
      reload();
      return res;
    } catch (e) {
      setAlert(errText(e));
      return undefined;
    }
  };
  if (error) return <div class="err" role="alert">{error}</div>;
  if (!r) return <Skeleton />;
  const lim = r.limits;
  const left = Math.max(lim.daily_max - lim.used_24h, 0);
  const usable = r.destinations.filter(d => d.usable);
  return (
    <>
      {alertMsg ? <div class="err" role="alert">{alertMsg}</div> : null}
      <div>
        <div class="ledger">
          <Figure value={sats(left)} label="left to withdraw in the next 24 hours" />
          <Figure value={sats(lim.min_amount)} label="smallest withdrawal" />
          <Figure value={p.trusted ? sats(lim.auto_threshold) : 'Every one'} label={p.trusted ? 'sent without review up to this amount' : 'reviewed by an operator first'} />
        </div>
        {!r.node_ready ? (
          <div class="banner warn" role="status">
            <div>
              <b>Payouts are not switched on for this gateway.</b>
              Requests wait in the queue until the operator connects a payout node.
            </div>
          </div>
        ) : null}
        <Section title="Request a withdrawal">
          <div class="row">
            <select aria-label="Pay to" value={destId} onChange={e => setDestId(e.currentTarget.value)}>
              <option value="">A bolt11 invoice</option>
              {usable.map(d => (
                <option value={String(d.id)} key={d.id}>{`${d.label} (${d.masked})`}</option>
              ))}
            </select>
            <input type="number" min="1" placeholder="Amount in sats" aria-label="Amount in sats" value={amountSats} onInput={e => setAmountSats(e.currentTarget.value)} />
            <input placeholder="bolt11 invoice for exactly this amount" aria-label="bolt11 invoice" value={inv} onInput={e => setInv(e.currentTarget.value)} />
            <button
              class="primary"
              onClick={() => {
                const payload: Record<string, unknown> = { amount: Math.round(Number(amountSats) * 1000) };
                if (destId) payload.destination_id = Number(destId);
                else payload.invoice = inv;
                act('POST', `/projects/${pid}/withdrawals`, payload, 'Withdrawal requested');
              }}
            >
              Request withdrawal
            </button>
          </div>
          <p class="muted">A network fee reserve is held with the amount and the unused part is returned when the payment completes.</p>
        </Section>
        <Section title="Destinations">
          <p class="muted">{`Verify a new destination with a small test payout, then wait ${lim.destination_delay_hours} hours before you can withdraw to it.`}</p>
          {r.destinations.length ? (
            <Table head={['Label', 'Destination', 'Type', 'Status', '']}>
              {r.destinations.map(d => (
                <DestinationRow key={d.id} d={d} pid={pid} act={act} />
              ))}
            </Table>
          ) : (
            <Empty title="No destinations yet." hint="Add an on-chain address, Lightning address or LNURL-pay below, or pay a bolt11 invoice per withdrawal." />
          )}
          <div class="row">
            <select aria-label="Type" value={kind} onChange={e => setKind(e.currentTarget.value)}>
              {[['onchain', 'On-chain address'], ['lightning_address', 'Lightning address'], ['lnurl', 'LNURL-pay']].map(k => (
                <option value={k[0]} key={k[0]}>{k[1]}</option>
              ))}
            </select>
            <input placeholder="Label, for example Cold wallet" aria-label="Label" value={label} onInput={e => setLabel(e.currentTarget.value)} />
            <input placeholder="Address" aria-label="Address" value={value} onInput={e => setValue(e.currentTarget.value)} />
            <button onClick={() => act('POST', `/projects/${pid}/payout-destinations`, { kind, label, value }, 'Destination added')}>Add destination</button>
          </div>
        </Section>
        <Section title="History">
          {r.withdrawals.length ? (
            <Table head={['When', ['Amount', 'num'], 'To', 'Status', '']}>
              {r.withdrawals.map(w => (
                <tr key={w.id}>
                  <td class="muted" title={w.created}>{fmtWhen(w.created)}</td>
                  <td class="num"><Money amount={w.amount} currency={w.currency} fee={w.fee} /></td>
                  <td class="mono">{w.destination_label}</td>
                  <td><WithdrawalPill w={w} /></td>
                  <td>
                    {w.status === 'requested' ? (
                      <button
                        onClick={async () => {
                          const v = await confirmDialog({ title: 'Cancel this withdrawal?', body: 'The amount returns to your balance. Nothing has been sent.', keep: 'Keep withdrawal', confirm: 'Cancel withdrawal', danger: true });
                          if (v) act('POST', `/projects/${pid}/withdrawals/${w.id}/cancel`, {}, 'Withdrawal cancelled');
                        }}
                      >
                        Cancel withdrawal
                      </button>
                    ) : null}
                  </td>
                </tr>
              ))}
            </Table>
          ) : (
            <Empty title="No withdrawals yet." hint="Request one above once you have a balance." />
          )}
        </Section>
      </div>
    </>
  );
}

export function Payouts(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="payouts" sub="Withdraw your platform balance to a wallet you control.">
      {p => <PayoutsBody pid={props.pid} p={p} />}
    </ProjectShell>
  );
}

export { WithdrawalPill };
