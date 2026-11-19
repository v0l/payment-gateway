import { useEffect, useState } from 'preact/hooks';
import { api, errText, useLoad, type Project } from './api';
import { methodName } from './format';
import { confirmChoice, confirmDialog } from './dialogs';
import { go, href } from './nav';
import { ProjectShell } from './shell';
import { Empty, Msg, Pill, Section, Skeleton, Table, TextField, SelectField, AreaField, useNote } from './ui';

type Field = [string, string, string?];

const PROVIDER_TEMPLATES: Record<string, { label?: string; fields: Field[]; note: string }> = {
  'lightning/lnd': { fields: [['url', 'LND gRPC url (https://host:10009)'], ['cert', 'tls.cert (PEM)', 'area'], ['macaroon', 'Macaroon (hex)', 'area']], note: 'Use a macaroon limited to invoices and offchain payments.' },
  'onchain/lnd': {
    fields: [['url', 'LND gRPC url (https://host:10009)'], ['cert', 'tls.cert (PEM)', 'area'], ['macaroon', 'Macaroon (hex)', 'area'], ['address_type', 'Address type (p2tr, p2wkh, np2wkh)'], ['min_confirmations', 'Confirmations needed (1-100)']],
    note: '',
  },
  'onchain/xpub': {
    label: 'Bitcoin on-chain (your xpub)',
    fields: [
      ['xpub', 'Extended public key (xpub, ypub, zpub) or output descriptor', 'area'],
      ['min_confirmations', 'Confirmations needed (0-100, default 1)'],
      ['esplora_url', 'Esplora url (optional, leave blank for the gateway default)'],
    ],
    note: 'The gateway only sees public keys. Payments go straight to your wallet, and refunds are done by hand from it.',
  },
  'stripe/stripe': { fields: [['secret_key', 'Secret key (sk_live_...)'], ['webhook_secret', 'Webhook signing secret (whsec_...)']], note: 'Add the webhook URL shown after saving to Stripe, for checkout.session events.' },
  'revolut/revolut': { fields: [['token', 'Merchant API key'], ['api_version', 'API version (2024-09-01)']], note: 'The webhook is registered for you.' },
  'paypal/paypal': {
    fields: [['client_id', 'Client ID'], ['client_secret', 'Client secret'], ['mode', 'Mode (sandbox or live)'], ['webhook_id', 'Webhook ID (from the PayPal developer dashboard)']],
    note: 'Subscribe the webhook URL shown after saving to CHECKOUT.ORDER.APPROVED, PAYMENT.CAPTURE.COMPLETED, PAYMENT.CAPTURE.REFUNDED and CUSTOMER.DISPUTE events.',
  },
  'fixedfloat/fixedfloat': { fields: [['api_key', 'API key'], ['api_secret', 'API secret'], ['coins', 'Coins, comma separated (XMR,LTC)']], note: 'Needs an on-chain provider in the same project.' },
};

interface ProviderRow {
  id: number;
  name: string;
  method: string;
  provider_type: string;
  fee_rate: number | null;
  fee_base: number | null;
  fee_currency: string | null;
  enabled: boolean;
  auto_disabled_at?: string | null;
  health?: { successes: number; errors: number; last_error?: string } | null;
  config?: Record<string, unknown>;
  hook_url?: string;
}

function healthText(r: ProviderRow): string {
  const h = r.health;
  if (r.auto_disabled_at) return 'Switched off after repeated errors';
  if (!h) return 'No calls yet';
  return `${h.successes} worked, ${h.errors} failed${h.last_error ? `. Last error: ${h.last_error}` : ''}`;
}

interface SharedRow {
  id: number;
  name: string;
  method: string;
  network: string;
  fee_bps: number;
  fee_base_msat: number;
  enabled: boolean;
}

export function FeeMoney(props: { bps: number; base?: number }) {
  return (
    <span>
      <span class="money">{`${props.bps / 100}%`}</span>
      {props.base ? (
        <>
          {' '}
          <span class="money">
            {`+ ${props.base.toLocaleString()} msats`}
            <small>fee</small>
          </span>
        </>
      ) : null}
    </span>
  );
}

function SharedMethods(props: { pid: string }) {
  const { pid } = props;
  const { data: rows, error, reload } = useLoad(() => api<SharedRow[]>('GET', `/projects/${pid}/shared`), [pid]);
  const [note, out] = useNote();
  const toggle = async (r: SharedRow) => {
    try {
      await api('POST', `/projects/${pid}/shared/${r.id}/enabled`, { enabled: !r.enabled });
      reload();
    } catch (e) {
      out(errText(e), true);
    }
  };
  return (
    <Section title="Platform payment methods">
      <p class="muted">{"Use the platform's own Lightning and on-chain wallets. Payments are held as a balance for you, minus the fee shown."}</p>
      <div>
        {error ? (
          <Msg text={error} bad />
        ) : !rows ? (
          <Skeleton />
        ) : !rows.length ? (
          <Empty title="No platform payment methods offered yet." hint="An operator can make Lightning and on-chain wallets available to this project." />
        ) : (
          <Table head={['Name', 'Method', 'Network', 'Platform fee', 'Status', '']}>
            {rows.map(r => (
              <tr key={r.id}>
                <td>{r.name}</td>
                <td class="muted">{methodName(r.method)}</td>
                <td class="muted">{r.network}</td>
                <td><FeeMoney bps={r.fee_bps} base={r.fee_base_msat} /></td>
                <td><Pill status={r.enabled ? 'active' : 'expired'} word={r.enabled ? 'On' : 'Off'} /></td>
                <td class="num">
                  <button
                    class={r.enabled ? 'small' : 'small primary'}
                    onClick={() =>
                      r.enabled
                        ? confirmChoice(`Disable ${r.name}?`, 'New payments can no longer use this method. Payments already started and your balance stay as they are.', 'Keep method', 'Disable method', () => toggle(r))
                        : toggle(r)
                    }
                  >
                    {r.enabled ? 'Turn off' : 'Turn on'}
                  </button>
                </td>
              </tr>
            ))}
          </Table>
        )}
      </div>
      {note}
    </Section>
  );
}

function ProvidersBody(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const canWrite = p.role !== 'viewer';
  const { data: rows, error, reload } = useLoad(() => api<ProviderRow[]>('GET', `/projects/${pid}/providers`), [pid]);
  const [note, out] = useNote();
  return (
    <>
      <Section title="Your payment methods" aside={<button class="primary" disabled={!canWrite} onClick={() => go(`/p/${pid}/providers/new`)}>Add payment method</button>}>
        <div>
          {note}
          {error ? (
            <Msg text={error} bad />
          ) : !rows ? (
            <Skeleton />
          ) : rows.length ? (
            <Table head={['Name', 'Method', 'Fee to customer', 'Status', 'Last 24 hours', '']}>
              {rows.map(r => (
                <tr key={r.id}>
                  <td>{r.name}</td>
                  <td class="muted">{`${methodName(r.method)}, ${r.provider_type}`}</td>
                  <td class="muted">{`${(r.fee_rate ? `${r.fee_rate}%` : '') + (r.fee_base ? ` + ${r.fee_base} ${r.fee_currency || ''}` : '')}` || 'None'}</td>
                  <td><Pill status={r.auto_disabled_at ? 'failed' : r.enabled ? 'active' : 'expired'} word={r.auto_disabled_at ? 'Switched off' : r.enabled ? 'On' : 'Off'} /></td>
                  <td class="muted wrap">{healthText(r)}</td>
                  <td class="num">
                    <button
                      class="link"
                      type="button"
                      disabled={!canWrite}
                      onClick={async () => {
                        try {
                          const t = await api<{ message: string; ok: boolean }>('POST', `/projects/${pid}/providers/${r.id}/test`);
                          out(t.message, !t.ok);
                        } catch (e) {
                          out(errText(e), true);
                        }
                      }}
                    >
                      Test
                    </button>{' '}
                    <button
                      class="link"
                      type="button"
                      disabled={!canWrite}
                      onClick={async () => {
                        await api('POST', `/projects/${pid}/providers/${r.id}/enabled`, { enabled: !r.enabled });
                        reload();
                      }}
                    >
                      {r.enabled ? 'Turn off' : 'Turn on'}
                    </button>{' '}
                    <a href={href(`/p/${pid}/providers/${r.id}`)}>Edit</a>
                  </td>
                </tr>
              ))}
            </Table>
          ) : (
            <Empty title="No payment methods of your own yet." hint="Connect your Lightning node, an on-chain wallet or a card provider to take payments directly." />
          )}
        </div>
      </Section>
      <div><SharedMethods pid={pid} /></div>
    </>
  );
}

export function Providers(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="providers" sub="How customers can pay you. Money from your own methods goes straight to you.">
      {p => <ProvidersBody pid={props.pid} p={p} />}
    </ProjectShell>
  );
}

interface XpubStatus {
  script: string;
  network: string;
  next_index: number;
  next_address: string;
  source: string;
  chain: { ok: boolean; message: string };
  preview: string[];
  issued: { index: number; address: string }[];
}

function XpubPanel(props: { pid: string; row: ProviderRow; canWrite: boolean }) {
  const { pid, row, canWrite } = props;
  const { data: x, error } = useLoad(() => api<XpubStatus>('GET', `/projects/${pid}/providers/${row.id}/xpub`), [pid, row.id]);
  const [from, setFrom] = useState('0');
  const [count, setCount] = useState('20');
  const [busy, setBusy] = useState(false);
  const [note, out] = useNote();
  if (error) return <div class="card"><Msg text={error} bad /></div>;
  if (!x) {
    return (
      <div class="card">
        <h3>Derivation and chain source</h3>
        <Skeleton />
      </div>
    );
  }
  return (
    <div class="card">
      <h3>Derivation and chain source</h3>
      <p class="muted">{`${x.script} on ${x.network}, receive chain /0/*. Next index ${x.next_index}, next address ${x.next_address}.`}</p>
      <p class="muted">{`Chain source: ${x.source}. ${x.chain.message}${x.chain.ok ? '' : ' (not reachable)'}`}</p>
      <h3>First addresses, check them against your wallet</h3>
      <ul class="mono">
        {x.preview.map((a, i) => (
          <li key={i}>{`${i}  ${a}`}</li>
        ))}
      </ul>
      {x.issued.length ? <h3>Latest addresses handed to customers</h3> : null}
      {x.issued.length ? (
        <ul class="mono">
          {x.issued.map(a => (
            <li key={a.index}>{`${a.index}  ${a.address}`}</li>
          ))}
        </ul>
      ) : null}
      <h3>Rescan a range</h3>
      <div class="row">
        <TextField label="First index" type="number" min="0" value={from} onInput={setFrom} />
        <TextField label="Addresses" type="number" min="1" value={count} onInput={setCount} />
        <button
          disabled={!canWrite || busy}
          onClick={async () => {
            setBusy(true);
            try {
              const r = await api<{ message: string }>('POST', `/projects/${pid}/providers/${row.id}/rescan`, { from: Number(from), count: Number(count) });
              out(r.message);
            } catch (e) {
              out(errText(e), true);
            }
            setBusy(false);
          }}
        >
          Rescan
        </button>
      </div>
      {note}
    </div>
  );
}

function ProviderFormBody(props: { pid: string; row: ProviderRow | null }) {
  const { pid, row } = props;
  const keys = Object.keys(PROVIDER_TEMPLATES);
  const [sel, setSel] = useState(row ? `${row.method}/${row.provider_type}` : keys[0]);
  const [name, setName] = useState(row ? row.name : '');
  const [rate, setRate] = useState(row && row.fee_rate !== null ? String(row.fee_rate) : '');
  const [base, setBase] = useState(row && row.fee_base !== null ? String(row.fee_base) : '');
  const [fcur, setFcur] = useState(row && row.fee_currency ? row.fee_currency : '');
  const [values, setValues] = useState<Record<string, string>>(() => {
    const init: Record<string, string> = {};
    Object.keys(row?.config ?? {}).forEach(k => {
      const v = row!.config![k];
      if (v && v !== 'set') init[k] = String(v);
    });
    return init;
  });
  const [busy, setBusy] = useState(false);
  const [noteNode, out] = useNote();
  const [hook, setHook] = useState('');
  const [preview, setPreview] = useState<string[] | null>(null);
  const t = PROVIDER_TEMPLATES[sel];
  const save = async () => {
    const parts = sel.split('/');
    const cfg: Record<string, unknown> = {};
    t.fields.forEach(f => {
      const v = (values[f[0]] ?? '').trim();
      if (v === '') {
        if (row?.config && row.config[f[0]] === 'set') cfg[f[0]] = 'set';
        return;
      }
      if (f[0] === 'coins') cfg[f[0]] = v.split(',').map(c => c.trim().toUpperCase()).filter(Boolean);
      else if (f[0] === 'min_confirmations') cfg[f[0]] = Number(v);
      else cfg[f[0]] = v;
    });
    const payload = { method: parts[0], provider_type: parts[1], name, config: cfg, fee_rate: rate === '' ? null : Number(rate), fee_base: base === '' ? null : Number(base), fee_currency: fcur || null };
    setBusy(true);
    try {
      const r = row
        ? await api<{ hook_url?: string; preview_addresses?: string[] }>('POST', `/projects/${pid}/providers/${row.id}`, payload)
        : await api<{ hook_url?: string; preview_addresses?: string[] }>('POST', `/projects/${pid}/providers`, payload);
      out('Saved.');
      setHook((r && r.hook_url) || (row && row.hook_url) || '');
      setPreview(r?.preview_addresses ?? null);
      if (!row && !r?.preview_addresses) setTimeout(() => go(`/p/${pid}/providers`), 1200);
    } catch (e) {
      out(errText(e), true);
    }
    setBusy(false);
  };
  return (
    <div class="card">
      <div class="form">
        <SelectField
          label="Type"
          value={sel}
          disabled={!!row}
          onChange={setSel}
          options={keys.map(k => {
            const x = k.split('/');
            return [k, PROVIDER_TEMPLATES[k].label ?? `${methodName(x[0])} through ${x[1]}`];
          })}
        />
        <TextField label="Name" value={name} onInput={setName} />
      </div>
      <div>
        {t.fields.map(f => {
          const cur = row && row.config ? row.config[f[0]] : null;
          const placeholder = cur === 'set' ? (f[2] === 'area' ? 'Saved. Leave as is to keep it.' : 'Saved. Leave blank to keep it.') : '';
          return f[2] === 'area' ? (
            <AreaField key={f[0]} label={f[1]} placeholder={placeholder} value={values[f[0]] ?? ''} onInput={v => setValues({ ...values, [f[0]]: v })} />
          ) : (
            <TextField key={f[0]} label={f[1]} placeholder={placeholder} value={values[f[0]] ?? ''} onInput={v => setValues({ ...values, [f[0]]: v })} />
          );
        })}
        {t.note ? <p class="muted">{t.note}</p> : null}
      </div>
      <h3>{"Processing fee added to the customer's total"}</h3>
      <div class="row">
        <input type="number" step="0.01" placeholder="e.g. 1.5" aria-label="Fee percent" value={rate} onInput={e => setRate(e.currentTarget.value)} />
        <input type="number" placeholder="minor units" aria-label="Fixed fee" value={base} onInput={e => setBase(e.currentTarget.value)} />
        <input placeholder="EUR" aria-label="Fee currency" value={fcur} onInput={e => setFcur(e.currentTarget.value)} />
      </div>
      <div class="row">
        <button class="primary" disabled={busy} onClick={save}>{row ? 'Save changes' : 'Add payment method'}</button>
        {row ? (
          <button
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              try {
                const r = await api<{ message: string; ok: boolean }>('POST', `/projects/${pid}/providers/${row.id}/test`);
                out(r.message, !r.ok);
              } catch (e) {
                out(errText(e), true);
              }
              setBusy(false);
            }}
          >
            Test connection
          </button>
        ) : null}
        {row ? (
          <button
            class="danger"
            onClick={async () => {
              const r = await confirmDialog({ title: `Delete ${row.name}`, body: 'Customers can no longer pay with it. Payments already made are not affected.', keep: 'Keep it', confirm: 'Delete', danger: true });
              if (!r) return;
              try {
                await api('DELETE', `/projects/${pid}/providers/${row.id}`);
                go(`/p/${pid}/providers`);
              } catch (e) {
                out(errText(e), true);
              }
            }}
          >
            Delete
          </button>
        ) : null}
        <a href={href(`/p/${pid}/providers`)}>Back</a>
      </div>
      <div>
        {noteNode}
        {hook ? (
          <p class="muted">
            {'Webhook URL for this provider: '}
            <span class="mono">{hook}</span>
          </p>
        ) : null}
        {preview ? (
          <div>
            <p>Check these first addresses against your wallet before you take payments:</p>
            <ul class="mono">
              {preview.map((a, i) => (
                <li key={i}>{`${i}  ${a}`}</li>
              ))}
            </ul>
            <a href={href(`/p/${pid}/providers`)}>Done</a>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function ProviderFormLoader(props: { pid: string; id: string; canWrite: boolean }) {
  const { pid, id } = props;
  const { data, error } = useLoad(async () => {
    if (id === 'new') return { row: null as ProviderRow | null };
    const rows = await api<ProviderRow[]>('GET', `/projects/${pid}/providers`);
    return { row: rows.find(r => String(r.id) === String(id)) ?? null };
  }, [pid, id]);
  if (error) return <Msg text={error} bad />;
  if (!data) return null;
  return (
    <>
      <ProviderFormBody pid={pid} row={data.row} />
      {data.row && data.row.provider_type === 'xpub' ? <XpubPanel pid={pid} row={data.row} canWrite={props.canWrite} /> : null}
    </>
  );
}

export function ProviderForm(props: { pid: string; id: string }) {
  const [title, setTitle] = useState('Payment methods');
  useEffect(() => {
    if (props.id === 'new') {
      setTitle('Add a payment method');
      return;
    }
    api<ProviderRow[]>('GET', `/projects/${props.pid}/providers`).then(rows => {
      const r = rows.find(x => String(x.id) === String(props.id));
      setTitle(r ? r.name : 'Add a payment method');
    }, () => {});
  }, [props.pid, props.id]);
  return (
    <ProjectShell pid={props.pid} tab="providers" title={title}>
      {p => <ProviderFormLoader pid={props.pid} id={props.id} canWrite={p.role !== 'viewer'} />}
    </ProjectShell>
  );
}
