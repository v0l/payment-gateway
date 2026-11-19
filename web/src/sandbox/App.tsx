import { useEffect, useRef, useState } from 'preact/hooks';
import type { ComponentChildren } from 'preact';

interface Part<T> {
  ok: boolean;
  error?: string;
  value: T;
}

interface NodeInfo {
  pubkey: string;
  synced: boolean;
  height: number;
  onchain_sats: number;
  unconfirmed_sats: number;
  active_channels: number;
}

interface Channel {
  active: boolean;
  capacity_sats: number;
  local_sats: number;
  remote_sats: number;
}

interface Status {
  ready: boolean;
  chain: Part<{ height: number; miner_sats: number }>;
  gateway: Part<NodeInfo>;
  customer: Part<NodeInfo>;
  channels: Part<Channel[]>;
}

interface Attempt {
  method: string;
  status: string;
  details?: { bolt11?: string; address?: string };
}

interface Payment {
  id: string;
  reference: string;
  amount: number;
  currency: string;
  status: string;
  pay_url: string;
  attempts?: Attempt[];
}

interface Hook {
  received: string;
  event_type: string;
  payment_id: string;
  status: string;
  signature_valid: boolean;
  body: string;
}

const sats = (n: number) => `${Number(n).toLocaleString()} sats`;

class Unauthorized extends Error {}

function useSandboxApi(token: string, signOut: () => void) {
  return async <T,>(path: string, body?: unknown): Promise<T> => {
    const headers: Record<string, string> = { authorization: `Bearer ${token}` };
    const init: RequestInit = { headers };
    if (body !== undefined) {
      init.method = 'POST';
      headers['content-type'] = 'application/json';
      init.body = JSON.stringify(body);
    }
    const r = await fetch(path, init);
    if (r.status === 401) {
      signOut();
      throw new Unauthorized('unauthorized');
    }
    const b = (await r.json().catch(() => ({}))) as { error?: string };
    if (!r.ok) throw new Error(b.error ?? `HTTP ${r.status}`);
    return b as T;
  };
}

function Kv(props: { rows: [string, string | number, boolean?][] }) {
  return (
    <dl class="kv">
      {props.rows.map(r => (
        <>
          <dt>{r[0]}</dt>
          <dd class={r[2] ? 'mono' : ''}>{String(r[1])}</dd>
        </>
      ))}
    </dl>
  );
}

function PartView<T>(props: { part: Part<T> | undefined; render: (v: T) => ComponentChildren }) {
  const p = props.part;
  if (!p) return <div class="muted">Loading</div>;
  if (!p.ok) return <span class="err">{p.error}</span>;
  return <>{props.render(p.value)}</>;
}

function NodeView(props: { n: NodeInfo }) {
  const { n } = props;
  return (
    <Kv
      rows={[
        ['Pubkey', `${n.pubkey.slice(0, 20)}...`, true],
        ['Synced', n.synced ? 'yes' : 'no'],
        ['Height', n.height],
        ['On-chain', sats(n.onchain_sats) + (n.unconfirmed_sats ? ` (+${sats(n.unconfirmed_sats)} pending)` : '')],
        ['Active channels', n.active_channels],
      ]}
    />
  );
}

function ActionButton(props: { run: () => Promise<unknown>; after: () => void; ghost?: boolean; children: ComponentChildren; id?: string }) {
  const [busy, setBusy] = useState(false);
  return (
    <button
      id={props.id}
      class={props.ghost ? 'ghost' : ''}
      disabled={busy}
      onClick={async () => {
        setBusy(true);
        try {
          await props.run();
        } catch (e) {
          alert(e instanceof Error ? e.message : String(e));
        }
        setBusy(false);
        props.after();
      }}
    >
      {props.children}
    </button>
  );
}

function Msg(props: { text: string; bad: boolean }) {
  return <div class={props.bad ? 'err' : 'muted'}>{props.text}</div>;
}

function Dashboard(props: { token: string; signOut: () => void }) {
  const api = useSandboxApi(props.token, props.signOut);
  const [status, setStatus] = useState<Status | null>(null);
  const [payments, setPayments] = useState<Payment[] | null>(null);
  const [hooks, setHooks] = useState<Hook[] | null>(null);
  const [mineN, setMineN] = useState('1');
  const [fundGw, setFundGw] = useState('1000000');
  const [fundCu, setFundCu] = useState('1000000');
  const [faucetAddr, setFaucetAddr] = useState('');
  const [faucetSats, setFaucetSats] = useState('100000');
  const [payBolt, setPayBolt] = useState('');
  const [toolMsg, setToolMsg] = useState<{ text: string; bad: boolean }>({ text: '', bad: false });
  const [ref, setRef] = useState('sandbox-order');
  const [amount, setAmount] = useState('1500');
  const [cur, setCur] = useState('EUR');
  const [pMsg, setPMsg] = useState<{ text: string; bad: boolean }>({ text: '', bad: false });
  const [log, setLog] = useState<string | null>(null);
  const live = useRef(true);

  const refresh = async () => {
    try {
      const [s, p, h] = await Promise.all([api<Status>('/sandbox/api/status'), api<Payment[]>('/sandbox/api/payments'), api<Hook[]>('/sandbox/api/hooks')]);
      if (live.current) {
        setStatus(s);
        setPayments(p);
        setHooks(h);
      }
    } catch {}
  };
  useEffect(() => {
    live.current = true;
    refresh();
    const t = setInterval(refresh, 3000);
    return () => {
      live.current = false;
      clearInterval(t);
    };
  }, [props.token]);

  const fail = (set: (m: { text: string; bad: boolean }) => void) => (e: unknown) => set({ text: e instanceof Error ? e.message : String(e), bad: true });

  return (
    <div id="console">
      <header>
        <h1>Gateway sandbox</h1>
        <span class="badge">regtest, no real money</span>
        <span id="ready" class={`badge ${status ? (status.ready ? 'ok' : 'bad') : ''}`}>{status ? (status.ready ? 'ready' : 'not ready') : 'checking'}</span>
        <span class="grow" />
        <button class="ghost" id="signout" onClick={props.signOut}>Sign out</button>
      </header>
      <main>
        <section id="chain">
          <h2>Chain</h2>
          <div id="chain-body" class="muted">
            <PartView part={status?.chain} render={c => <Kv rows={[['Height', c.height], ['Miner wallet', sats(c.miner_sats)]]} />} />
          </div>
          <div class="row">
            <input id="mine-n" class="narrow" type="number" min="1" max="200" value={mineN} onInput={e => setMineN(e.currentTarget.value)} />
            <ActionButton id="mine" after={refresh} run={() => api('/sandbox/api/mine', { blocks: Number(mineN) })}>Mine blocks</ActionButton>
          </div>
        </section>
        <section>
          <h2>Gateway node (receives payments)</h2>
          <div id="gw-body" class="muted">
            <PartView part={status?.gateway} render={n => <NodeView n={n} />} />
          </div>
          <div class="row">
            <input id="fund-gw" type="number" value={fundGw} onInput={e => setFundGw(e.currentTarget.value)} />
            <ActionButton after={refresh} run={() => api('/sandbox/api/fund', { target: 'gateway', sats: Number(fundGw) })}>Fund on-chain</ActionButton>
          </div>
        </section>
        <section>
          <h2>Customer node (pays)</h2>
          <div id="cu-body" class="muted">
            <PartView part={status?.customer} render={n => <NodeView n={n} />} />
          </div>
          <div class="row">
            <input id="fund-cu" type="number" value={fundCu} onInput={e => setFundCu(e.currentTarget.value)} />
            <ActionButton after={refresh} run={() => api('/sandbox/api/fund', { target: 'customer', sats: Number(fundCu) })}>Fund on-chain</ActionButton>
          </div>
          <div id="channels" class="muted">
            {status?.channels.ok ? (
              status.channels.value.length ? (
                <table>
                  <tr>
                    <th>Channel</th>
                    <th>Capacity</th>
                    <th>Customer</th>
                    <th>Gateway</th>
                  </tr>
                  {status.channels.value.map((c, i) => (
                    <tr key={i}>
                      <td>{c.active ? 'active' : 'pending'}</td>
                      <td>{sats(c.capacity_sats)}</td>
                      <td>{sats(c.local_sats)}</td>
                      <td>{sats(c.remote_sats)}</td>
                    </tr>
                  ))}
                </table>
              ) : (
                <span class="muted">No channels yet</span>
              )
            ) : null}
          </div>
        </section>
        <section>
          <h2>Setup</h2>
          <p class="muted lead">
            Mines blocks, funds both nodes, opens a channel, creates the <span class="mono">sandbox</span> tenant and its LND providers. Safe to run again.
          </p>
          <div class="row">
            <ActionButton
              id="bootstrap"
              after={refresh}
              run={async () => {
                setLog('Working. This can take a minute.\n');
                try {
                  const r = await api<{ log?: string[]; ok: boolean; error?: string }>('/sandbox/api/bootstrap', {});
                  setLog(`${(r.log ?? []).join('\n')}${r.ok ? '\nDone.' : `\nFAILED: ${r.error}`}`);
                } catch (e) {
                  setLog(prev => `${prev ?? ''}\nFAILED: ${e instanceof Error ? e.message : String(e)}`);
                }
              }}
            >
              Bootstrap regtest
            </ActionButton>
            <ActionButton
              id="rotate"
              ghost
              after={refresh}
              run={async () => {
                const r = await api<{ api_key: string }>('/sandbox/api/rotate-key', {});
                setLog(`New sandbox API key (shown once):\n${r.api_key}`);
              }}
            >
              New API key
            </ActionButton>
          </div>
          <div id="setup-log" class="log" hidden={log === null}>{log}</div>
        </section>
        <section>
          <h2>Faucet</h2>
          <p class="muted lead">Send regtest coins to any address, for example one from a payment page.</p>
          <div class="row">
            <input id="faucet-addr" placeholder="bcrt1..." value={faucetAddr} onInput={e => setFaucetAddr(e.currentTarget.value)} />
            <input id="faucet-sats" class="wide-num" type="number" value={faucetSats} onInput={e => setFaucetSats(e.currentTarget.value)} />
            <ActionButton
              after={refresh}
              run={() =>
                api<{ txid: string }>('/sandbox/api/faucet', { address: faucetAddr, sats: Number(faucetSats) }).then(r => setToolMsg({ text: `sent ${r.txid}`, bad: false })).catch(fail(setToolMsg))
              }
            >
              Send
            </ActionButton>
          </div>
          <p class="muted tight">Pay a lightning invoice from the customer node</p>
          <div class="row">
            <input id="pay-bolt" placeholder="lnbcrt..." value={payBolt} onInput={e => setPayBolt(e.currentTarget.value)} />
            <ActionButton after={refresh} run={() => api('/sandbox/api/pay-invoice', { bolt11: payBolt }).then(() => setToolMsg({ text: 'invoice paid', bad: false })).catch(fail(setToolMsg))}>Pay</ActionButton>
          </div>
          <Msg text={toolMsg.text} bad={toolMsg.bad} />
        </section>
        <section>
          <h2>New payment</h2>
          <div class="row">
            <input id="p-ref" placeholder="reference" value={ref} onInput={e => setRef(e.currentTarget.value)} />
            <input id="p-amount" class="mid" type="number" value={amount} onInput={e => setAmount(e.currentTarget.value)} />
            <select id="p-cur" value={cur} onChange={e => setCur(e.currentTarget.value)}>
              <option>EUR</option>
              <option>USD</option>
              <option>BTC</option>
            </select>
          </div>
          <div class="row">
            <ActionButton after={refresh} run={() => api('/sandbox/api/payments', { reference: ref, amount: Number(amount), currency: cur }).then(() => setPMsg({ text: 'created', bad: false })).catch(fail(setPMsg))}>Create payment</ActionButton>
            <span class="muted">amount in minor units (cents, msats)</span>
          </div>
          <Msg text={pMsg.text} bad={pMsg.bad} />
        </section>
        <section class="wide">
          <h2>Payments</h2>
          <div id="payments" class="muted">
            {payments && payments.length ? (
              <table>
                <tr>
                  {['Id', 'Reference', 'Amount', 'Status', 'Attempts and actions'].map(h => (
                    <th key={h}>{h}</th>
                  ))}
                </tr>
                {payments.map(p => (
                  <PaymentRow key={p.id} p={p} api={api} refresh={refresh} setMsg={setPMsg} />
                ))}
              </table>
            ) : (
              <span class="muted">None yet</span>
            )}
          </div>
        </section>
        <section class="wide">
          <h2>Webhooks received</h2>
          <div id="hooks" class="muted">
            {hooks && hooks.length ? (
              <table>
                <tr>
                  {['Time', 'Event', 'Payment', 'Status', 'Signature', 'Body'].map(h => (
                    <th key={h}>{h}</th>
                  ))}
                </tr>
                {hooks.map((h, i) => (
                  <tr key={i}>
                    <td>{new Date(h.received).toLocaleTimeString()}</td>
                    <td>{h.event_type}</td>
                    <td class="mono">{h.payment_id.slice(0, 8)}</td>
                    <td>{h.status}</td>
                    <td><span class={`pill ${h.signature_valid ? 'paid' : 'failed'}`}>{h.signature_valid ? 'valid' : 'invalid'}</span></td>
                    <td>
                      <details>
                        <summary>json</summary>
                        <pre class="mono">{JSON.stringify(JSON.parse(h.body || '{}'), null, 2)}</pre>
                      </details>
                    </td>
                  </tr>
                ))}
              </table>
            ) : (
              <span class="muted">None yet. Pay a payment and the signed event shows up here.</span>
            )}
          </div>
        </section>
      </main>
    </div>
  );
}

function PaymentRow(props: { p: Payment; api: ReturnType<typeof useSandboxApi>; refresh: () => void; setMsg: (m: { text: string; bad: boolean }) => void }) {
  const { p, api } = props;
  return (
    <tr>
      <td class="mono">{p.id.slice(0, 8)}</td>
      <td>{p.reference}</td>
      <td>{`${p.amount} ${p.currency}`}</td>
      <td><span class={`pill ${p.status}`}>{p.status}</span></td>
      <td>
        {(p.attempts ?? []).map((a, i) => {
          const d = a.details ?? {};
          return (
            <div key={i}>
              <span class={`pill ${a.status}`}>{`${a.method} ${a.status}`}</span>{' '}
              {d.bolt11 ? <code class="mono" title={d.bolt11}>{`${d.bolt11.slice(0, 40)}...`}</code> : null}
              {d.address ? <code class="mono">{d.address}</code> : null}
            </div>
          );
        })}
        <div class="row">
          {p.status === 'pending' ? (
            <>
              {['lightning', 'onchain'].map(m => (
                <button
                  class="ghost"
                  key={m}
                  onClick={() => api(`/sandbox/api/payments/${p.id}/attempt`, { method: m }).then(props.refresh).catch(e => alert(e instanceof Error ? e.message : String(e)))}
                >
                  {`Use ${m}`}
                </button>
              ))}
              <button
                onClick={() =>
                  api<{ message: string }>(`/sandbox/api/payments/${p.id}/pay`, {})
                    .then(r => {
                      props.setMsg({ text: r.message, bad: false });
                      props.refresh();
                    })
                    .catch(e => props.setMsg({ text: e instanceof Error ? e.message : String(e), bad: true }))
                }
              >
                Pay from customer
              </button>
            </>
          ) : null}
          <a href={p.pay_url} target="_blank" rel="noopener">Open pay page</a>
        </div>
      </td>
    </tr>
  );
}

function Login(props: { onToken: (t: string) => void }) {
  const [t, setT] = useState('');
  const [err, setErr] = useState('');
  return (
    <div id="login">
      <section>
        <h2>Admin token</h2>
        <div class="row">
          <input id="token" type="password" placeholder="GATEWAY_ADMIN_TOKEN" autocomplete="off" value={t} onInput={e => setT(e.currentTarget.value)} />
          <button
            id="signin"
            onClick={async () => {
              const r = await fetch('/sandbox/api/status', { headers: { authorization: `Bearer ${t.trim()}` } });
              if (r.status === 401) {
                setErr('Wrong token');
                return;
              }
              sessionStorage.setItem('pgw-admin', t.trim());
              props.onToken(t.trim());
            }}
          >
            Sign in
          </button>
        </div>
        <p id="login-error" class="err">{err}</p>
      </section>
    </div>
  );
}

export function SandboxApp() {
  const [token, setToken] = useState(() => sessionStorage.getItem('pgw-admin') ?? '');
  const signOut = () => {
    sessionStorage.removeItem('pgw-admin');
    setToken('');
  };
  return token ? <Dashboard token={token} signOut={signOut} /> : <Login onToken={setToken} />;
}
