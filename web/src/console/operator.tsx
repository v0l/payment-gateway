import { useState } from 'preact/hooks';
import { api, errText, getSession, setSession, useLoad, type Me } from './api';
import { fmtAmount, fmtWhen, methodName, sats } from './format';
import { confirmChoice, confirmDialog, toast } from './dialogs';
import { enc, go, href } from './nav';
import { OperatorPage } from './shell';
import { ClickRow, Empty, Figure, Money, Msg, Pill, SecretBox, Section, Skeleton, Table, TextField, AreaField, Check, SelectField, useNote } from './ui';
import { FeeMoney } from './providers';

export function NewProject() {
  const [name, setName] = useState('');
  const [note, out, outNode] = useNote();
  return (
    <OperatorPage active="new" title="New project" sub="A project has its own API key, payment methods and webhooks.">
      <div class="card">
        <TextField label="Name" placeholder="My shop" value={name} onInput={setName} />
        <div class="row">
          <button
            class="primary"
            onClick={async () => {
              try {
                const r = await api<{ id: number; api_key: string; webhook_secret: string }>('POST', '/projects', { name });
                outNode(
                  <>
                    <SecretBox title="API key" value={r.api_key} />
                    <SecretBox title="Webhook signing secret" value={r.webhook_secret} />
                    <p><a href={href(`/p/${r.id}/overview`)}>Go to project</a></p>
                  </>,
                );
                setSession(await api<Me>('GET', '/me'));
              } catch (e) {
                out(errText(e), true);
              }
            }}
          >
            Create project
          </button>
        </div>
        {note}
      </div>
    </OperatorPage>
  );
}

interface Stats {
  pending: { value: number; currency: string; payments: number }[];
  success_rate: number | null;
  paid: number;
  failed: number;
  failed_webhooks: number;
}

function DashboardFigures(props: { s: Stats }) {
  const { s } = props;
  return (
    <>
      {s.pending.map(p => (
        <Figure key={p.currency} value={fmtAmount(p.value, p.currency)} label={`${p.payments} pending in ${p.currency}`} />
      ))}
      <Figure value={s.success_rate === null ? 'None' : `${Math.round(s.success_rate * 100)}%`} label={`${s.paid} paid, ${s.failed} expired or cancelled`} />
      <Figure value={String(s.failed_webhooks)} label="failed webhook deliveries" tone={s.failed_webhooks ? 'bad' : ''} />
    </>
  );
}

interface Hit {
  tenant_id: number;
  payment_id: string;
  reference: string;
  status: string;
  matched: string;
}

function OperatorTenantsBody() {
  const [term, setTerm] = useState('');
  const [hits, setHits] = useState<Hit[] | null>(null);
  const [searchError, setSearchError] = useState('');
  const health = useLoad(() => api<Record<string, number>>('GET', '/operator/health'), []);
  const tenants = useLoad(
    () => api<{ tenants: { id: number; name: string; enabled: boolean; trusted: boolean; payments: number; volume_30d: number; limit_payments_per_day: number | null; last_payment?: string | null }[] }>('GET', '/operator/tenants'),
    [],
  );
  const search = async () => {
    try {
      const r = await api<{ hits: Hit[] }>('GET', `/operator/search?q=${enc(term.trim())}`);
      setHits(r.hits);
      setSearchError('');
    } catch (e) {
      setSearchError(errText(e));
    }
  };
  const h = health.data;
  return (
    <>
      <div class="ledger">
        {health.error ? (
          <Msg text={health.error} bad />
        ) : h ? (
          <>
            <Figure value={String(h.tenants)} label="projects" />
            <Figure value={String(h.tenants_disabled)} label="disabled" />
            <Figure value={String(h.pending_payments)} label="waiting payments" />
            <Figure value={String(h.pending_deliveries)} label="webhooks waiting" />
            <Figure value={String(h.abandoned_deliveries_24h)} label="webhooks given up, 24 h" tone={h.abandoned_deliveries_24h ? 'bad' : ''} />
          </>
        ) : null}
      </div>
      <Section title="Search every project">
        <div class="filterbar">
          <input type="search" placeholder="Payment id, reference, invoice hash or txid" aria-label="Search every project" value={term} onInput={e => setTerm(e.currentTarget.value)} onKeyDown={e => e.key === 'Enter' && search()} />
          <button onClick={search}>Search</button>
        </div>
        <div>
          {searchError ? (
            <Msg text={searchError} bad />
          ) : hits ? (
            hits.length ? (
              <Table head={['Project', 'Payment', 'Reference', 'Status', 'Matched on']}>
                {hits.map(x => (
                  <ClickRow key={x.payment_id} onOpen={() => go(`/p/${x.tenant_id}/payments/${x.payment_id}`)}>
                    <td>{String(x.tenant_id)}</td>
                    <td class="mono">{x.payment_id.slice(0, 12)}</td>
                    <td>{x.reference}</td>
                    <td><Pill status={x.status} /></td>
                    <td class="muted">{x.matched}</td>
                  </ClickRow>
                ))}
              </Table>
            ) : (
              <p class="muted">Nothing matches.</p>
            )
          ) : null}
        </div>
      </Section>
      <Section title="All projects">
        <div>
          {tenants.error ? (
            <Msg text={tenants.error} bad />
          ) : !tenants.data ? (
            <Skeleton />
          ) : (
            <Table head={['Project', 'Status', ['Payments', 'num'], ['Volume, 30 days', 'num'], 'Daily limit', 'Last payment', '']}>
              {tenants.data.tenants.map(t => (
                <tr key={t.id}>
                  <td><a href={href(`/p/${t.id}/overview`)}>{t.name}</a></td>
                  <td><Pill status={t.enabled ? (t.trusted ? 'active' : 'pending') : 'failed'} word={t.enabled ? (t.trusted ? 'Live' : 'Test mode') : 'Disabled'} /></td>
                  <td class="num">{String(t.payments)}</td>
                  <td class="num">{fmtAmount(t.volume_30d, 'EUR')}</td>
                  <td class="muted">{t.limit_payments_per_day === null ? 'Default' : String(t.limit_payments_per_day)}</td>
                  <td class="muted">{t.last_payment ? fmtWhen(t.last_payment) : 'Never'}</td>
                  <td class="num"><a href={href(`/support/${t.id}`)}>Support view</a></td>
                </tr>
              ))}
            </Table>
          )}
        </div>
      </Section>
    </>
  );
}

export function OperatorTenants() {
  return (
    <OperatorPage active="tenants" title="Projects" sub="Every project on the platform.">
      <OperatorTenantsBody />
    </OperatorPage>
  );
}

interface Support {
  project: { name: string };
  members: number;
  stats: Stats;
  recent_payments: { id: string; reference: string; amount: number; currency: string; status: string; created: string }[];
  providers: { name: string; method: string; type: string; enabled: boolean }[];
  webhook_endpoints: { endpoint_id: number | null; recent_total: number; recent_failed: number }[];
}

export function SupportView(props: { id: string }) {
  const { data: r, error } = useLoad(() => api<Support>('GET', `/operator/tenants/${props.id}/support`), [props.id]);
  return (
    <OperatorPage
      active="tenants"
      title={r ? r.project.name : 'Support view'}
      sub={
        <>
          <a href={href('/tenants')}>Projects</a>
          {` / project ${props.id}`}
        </>
      }
    >
      <div>
        {error ? (
          <div class="err" role="alert">{error}</div>
        ) : !r ? (
          <Skeleton />
        ) : (
          <>
            <div class="banner info" role="note">
              <div>
                <b>Read-only support view</b>
                {`${r.members} members. Opening this view was recorded in the audit log.`}
              </div>
            </div>
            <div class="ledger"><DashboardFigures s={r.stats} /></div>
            <Section title="Recent payments">
              {r.recent_payments.length ? (
                <Table head={['Payment', 'Reference', ['Amount', 'num'], 'Status', 'Created']}>
                  {r.recent_payments.map(p => (
                    <tr key={p.id}>
                      <td class="mono">{p.id.slice(0, 10)}</td>
                      <td>{p.reference}</td>
                      <td class="num">{fmtAmount(p.amount, p.currency)}</td>
                      <td><Pill status={p.status} /></td>
                      <td class="muted">{fmtWhen(p.created)}</td>
                    </tr>
                  ))}
                </Table>
              ) : (
                <p class="muted">None.</p>
              )}
            </Section>
            <Section title="Payment methods">
              {r.providers.length ? (
                <Table head={['Name', 'Method', 'Type', 'Status']}>
                  {r.providers.map((p, i) => (
                    <tr key={i}>
                      <td>{p.name}</td>
                      <td>{methodName(p.method)}</td>
                      <td class="muted">{p.type}</td>
                      <td><Pill status={p.enabled ? 'active' : 'expired'} word={p.enabled ? 'On' : 'Off'} /></td>
                    </tr>
                  ))}
                </Table>
              ) : (
                <p class="muted">None.</p>
              )}
            </Section>
            <Section title="Webhook endpoints">
              {r.webhook_endpoints.length ? (
                <Table head={['Endpoint', ['Sent, 7 days', 'num'], ['Failed', 'num']]}>
                  {r.webhook_endpoints.map((e, i) => (
                    <tr key={i}>
                      <td>{e.endpoint_id === null ? 'Default' : `Endpoint ${e.endpoint_id}`}</td>
                      <td class="num">{String(e.recent_total)}</td>
                      <td class="num">{String(e.recent_failed)}</td>
                    </tr>
                  ))}
                </Table>
              ) : (
                <p class="muted">None.</p>
              )}
            </Section>
          </>
        )}
      </div>
    </OperatorPage>
  );
}

interface Account {
  id: number;
  email: string;
  is_operator: boolean;
  last_login?: string | null;
  disabled: boolean;
}

interface ProviderHealth {
  tenant_id: number;
  name: string;
  method: string;
  enabled: boolean;
  auto_disabled_at?: string | null;
  errors?: number;
  successes?: number;
  consecutive_failures?: number;
  last_error?: string | null;
}

function AdminBody() {
  const me = getSession().me!;
  const accounts = useLoad(() => api<Account[]>('GET', '/admin/accounts'), []);
  const providers = useLoad(() => api<ProviderHealth[]>('GET', '/admin/providers'), []);
  const [email, setEmail] = useState('');
  const [op, setOp] = useState(false);
  const [note, out, outNode] = useNote();
  return (
    <>
      <Section title="Invite someone">
        <div class="row">
          <input type="email" placeholder="customer@example.com" aria-label="Email" value={email} onInput={e => setEmail(e.currentTarget.value)} />
          <Check checked={op} onChange={setOp}>Operator</Check>
          <button
            class="primary"
            onClick={async () => {
              try {
                const r = await api<{ email: string; invite_url: string }>('POST', '/admin/invites', { email, operator: op });
                outNode(<SecretBox title={`Invite link for ${r.email}`} value={r.invite_url} />);
                accounts.reload();
              } catch (e) {
                out(errText(e), true);
              }
            }}
          >
            Create invite
          </button>
        </div>
        {note}
      </Section>
      <Section title="All accounts">
        <div>
          {accounts.error ? (
            <Msg text={accounts.error} bad />
          ) : !accounts.data ? (
            <Skeleton />
          ) : (
            <Table head={['Email', 'Role', 'Last sign-in', 'Status', '']}>
              {accounts.data.map(a => (
                <tr key={a.id}>
                  <td>{a.email}</td>
                  <td class="muted">{a.is_operator ? 'Operator' : 'Customer'}</td>
                  <td class="muted">{a.last_login ? fmtWhen(a.last_login) : 'Never'}</td>
                  <td><Pill status={a.disabled ? 'failed' : 'active'} word={a.disabled ? 'Disabled' : 'Active'} /></td>
                  <td class="num">
                    {a.id === me.id ? null : (
                      <button
                        class="link"
                        type="button"
                        onClick={async () => {
                          try {
                            await api('POST', `/admin/accounts/${a.id}/disabled`, { enabled: a.disabled });
                            accounts.reload();
                          } catch (e) {
                            out(errText(e), true);
                          }
                        }}
                      >
                        {a.disabled ? 'Enable' : 'Disable'}
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </Table>
          )}
        </div>
      </Section>
      <Section title="Payment method health">
        <div>
          {providers.error ? (
            <Msg text={providers.error} bad />
          ) : !providers.data ? (
            <Skeleton />
          ) : providers.data.length ? (
            <Table head={['Project', 'Payment method', 'Status', ['Errors, 24 h', 'num'], ['In a row', 'num'], 'Last error']}>
              {providers.data.map((r, i) => (
                <tr key={i}>
                  <td class="muted">{String(r.tenant_id)}</td>
                  <td>{`${r.name}, ${methodName(r.method)}`}</td>
                  <td><Pill status={r.auto_disabled_at ? 'failed' : r.enabled ? 'active' : 'expired'} word={r.auto_disabled_at ? 'Switched off' : r.enabled ? 'On' : 'Off'} /></td>
                  <td class="num">{`${r.errors || 0} of ${(r.errors || 0) + (r.successes || 0)}`}</td>
                  <td class="num">{String(r.consecutive_failures || 0)}</td>
                  <td class="muted trunc">{r.last_error ?? ''}</td>
                </tr>
              ))}
            </Table>
          ) : (
            <p class="muted">No payment methods configured yet.</p>
          )}
        </div>
      </Section>
    </>
  );
}

export function Admin() {
  return (
    <OperatorPage active="admin" title="Accounts" sub="Invite people and disable accounts. Lift test limits from a project's settings.">
      <AdminBody />
    </OperatorPage>
  );
}

interface SharedProvider {
  id: number;
  name: string;
  method: string;
  provider_type: string;
  network: string;
  fee_bps: number;
  fee_base_msat: number;
  allow_untrusted: boolean;
  allow_trusted: boolean;
  enabled: boolean;
}

function FeeOverride(props: { r: SharedProvider; close: () => void; out: (t: string, bad?: boolean) => void }) {
  const { r } = props;
  const [project, setProject] = useState('');
  const [bps, setBps] = useState('');
  const [base, setBase] = useState('');
  return (
    <div class="card">
      <h3>{`Fee override for ${r.name}`}</h3>
      <p class="muted">Leave a fee blank to use the provider default. Leave both blank to remove the override.</p>
      <TextField label="Project id" type="number" min="1" value={project} onInput={setProject} />
      <TextField label="Fee (basis points)" type="number" min="0" value={bps} onInput={setBps} />
      <TextField label="Base fee (msats)" type="number" min="0" value={base} onInput={setBase} />
      <div class="row">
        <button
          class="primary"
          onClick={async () => {
            if (!project) {
              props.out('Enter the project id the override is for.', true);
              return;
            }
            try {
              await api('POST', `/admin/shared/${r.id}/projects/${enc(project)}/fee`, { fee_bps: bps === '' ? null : Number(bps), fee_base_msat: base === '' ? null : Number(base) });
              props.close();
              props.out(`Override saved for project ${project}.`);
            } catch (e) {
              props.out(errText(e), true);
            }
          }}
        >
          Save override
        </button>
        <button onClick={props.close}>Cancel</button>
      </div>
    </div>
  );
}

function SharedAdminBody() {
  const list = useLoad(() => api<SharedProvider[]>('GET', '/admin/shared'), []);
  const [note, out] = useNote();
  const [override, setOverride] = useState<SharedProvider | null>(null);
  const [name, setName] = useState('');
  const [kind, setKind] = useState('lightning');
  const [network, setNetwork] = useState('mainnet');
  const [url, setUrl] = useState('');
  const [cert, setCert] = useState('');
  const [mac, setMac] = useState('');
  const [bps, setBps] = useState('');
  const [base, setBase] = useState('');
  const [untrusted, setUntrusted] = useState(false);
  const rows = list.data;
  const flag = (on: boolean) => <Pill status={on ? 'active' : 'expired'} word={on ? 'Allowed' : 'Blocked'} />;
  return (
    <>
      <Section title="Providers">
        <div>
          {list.error ? (
            <Msg text={list.error} bad />
          ) : !rows ? (
            <Skeleton />
          ) : !rows.length ? (
            <Empty title="No shared providers yet." hint="Add a Lightning or on-chain LND wallet below." />
          ) : (
            <Table head={['Name', 'Method', 'Network', 'Fee', 'Test mode', 'Live', 'Status', '']}>
              {rows.map(r => {
                const toggle = async () => {
                  try {
                    await api('POST', `/admin/shared/${r.id}`, { enabled: !r.enabled });
                    list.reload();
                  } catch (e) {
                    out(errText(e), true);
                  }
                };
                return (
                  <tr key={r.id}>
                    <td>{r.name}</td>
                    <td class="muted">{`${methodName(r.method)}, ${r.provider_type}`}</td>
                    <td class="muted">{r.network}</td>
                    <td><FeeMoney bps={r.fee_bps} base={r.fee_base_msat} /></td>
                    <td>{flag(r.allow_untrusted)}</td>
                    <td>{flag(r.allow_trusted)}</td>
                    <td><Pill status={r.enabled ? 'active' : 'expired'} word={r.enabled ? 'On' : 'Off'} /></td>
                    <td class="num">
                      <button
                        onClick={() =>
                          r.enabled
                            ? confirmChoice(`Disable ${r.name}?`, 'Projects can no longer start payments on it. Existing balances and refunds are not affected.', 'Keep provider', 'Disable provider', toggle)
                            : toggle()
                        }
                      >
                        {r.enabled ? 'Disable provider' : 'Enable provider'}
                      </button>{' '}
                      <button onClick={() => setOverride(r)}>Override fee</button>
                    </td>
                  </tr>
                );
              })}
            </Table>
          )}
        </div>
        <div>{override ? <FeeOverride r={override} close={() => setOverride(null)} out={out} /> : null}</div>
      </Section>
      <Section title="Add a shared provider">
        <div class="card">
          <div class="banner warn" role="note">
            <div>
              <b>Test mode projects stay off mainnet</b>
              Only providers on testnet, signet or regtest can be allowed for test mode projects.
            </div>
          </div>
          <div class="form">
            <TextField label="Name" placeholder="Platform Lightning" value={name} onInput={setName} />
            <SelectField label="Method" value={kind} onChange={setKind} options={[['lightning', 'Lightning'], ['onchain', 'On-chain']]} />
            <SelectField label="Network" value={network} onChange={setNetwork} options={['mainnet', 'testnet', 'signet', 'regtest'].map(n => [n, n])} />
            <TextField label="LND url" placeholder="https://host:10009" value={url} onInput={setUrl} />
            <AreaField label="tls.cert" placeholder="tls.cert (PEM)" value={cert} onInput={setCert} />
            <AreaField label="Macaroon" placeholder="Macaroon (hex)" value={mac} onInput={setMac} />
            <TextField label="Platform fee (basis points)" type="number" min="0" placeholder="100" value={bps} onInput={setBps} />
            <TextField label="Base fee (msats)" type="number" min="0" placeholder="0" value={base} onInput={setBase} />
            <Check checked={untrusted} onChange={setUntrusted}>Allow test mode projects</Check>
            <div class="row">
              <button
                class="primary"
                onClick={async () => {
                  try {
                    await api('POST', '/admin/shared', { name, method: kind, provider_type: 'lnd', network, config: { url: url.trim(), cert: cert.trim(), macaroon: mac.trim() }, fee_bps: Number(bps || 0), fee_base_msat: Number(base || 0), allow_untrusted: untrusted });
                    setCert('');
                    setMac('');
                    out('Shared provider added.');
                    list.reload();
                  } catch (e) {
                    out(errText(e), true);
                  }
                }}
              >
                Add shared provider
              </button>
            </div>
          </div>
        </div>
      </Section>
      {note}
    </>
  );
}

export function SharedAdmin() {
  return (
    <OperatorPage active="shared" title="Shared providers" sub="Platform wallets that projects can turn on. The platform holds the funds and keeps the fee.">
      <SharedAdminBody />
    </OperatorPage>
  );
}

interface PlatformPlan {
  id: number;
  name: string;
  live: boolean;
  currency: string;
  monthly_price: number;
  included_volume: number;
  fee_bps: number;
  fee_base: number;
  limits: unknown;
  active: boolean;
}

function PlanRow(props: { r: PlatformPlan; out: (t: string, bad?: boolean) => void; reload: () => void }) {
  const { r } = props;
  const [name, setName] = useState(r.name);
  const [price, setPrice] = useState(String(r.monthly_price));
  const [included, setIncluded] = useState(String(r.included_volume));
  const [bps, setBps] = useState(String(r.fee_bps));
  const [base, setBase] = useState(String(r.fee_base));
  const [active, setActive] = useState(r.active);
  return (
    <tr>
      <td class="mono">{String(r.id)}</td>
      <td><input value={name} aria-label="Name" onInput={e => setName(e.currentTarget.value)} /></td>
      <td class="muted">{r.live ? 'Live' : 'Test'}</td>
      <td><input type="number" value={price} aria-label="Monthly price" onInput={e => setPrice(e.currentTarget.value)} /></td>
      <td><input type="number" value={included} aria-label="Included volume" onInput={e => setIncluded(e.currentTarget.value)} /></td>
      <td><input type="number" value={bps} aria-label="Fee in basis points" onInput={e => setBps(e.currentTarget.value)} /></td>
      <td><input type="number" value={base} aria-label="Base fee" onInput={e => setBase(e.currentTarget.value)} /></td>
      <td><input type="checkbox" aria-label="Active" checked={active} onChange={e => setActive(e.currentTarget.checked)} /></td>
      <td>
        <button
          class="primary"
          onClick={async () => {
            try {
              await api('POST', `/admin/platform/plans/${r.id}`, { name, live: r.live, currency: r.currency, monthly_price: Number(price), included_volume: Number(included), fee_bps: Number(bps), fee_base: Number(base), limits: r.limits, active });
              props.out('Plan saved');
              props.reload();
            } catch (e) {
              props.out(errText(e), true);
            }
          }}
        >
          Save
        </button>
      </td>
    </tr>
  );
}

export function PlatformPlans() {
  const { data: rows, error, reload } = useLoad(() => api<PlatformPlan[]>('GET', '/admin/platform/plans'), []);
  const [note, out] = useNote();
  return (
    <OperatorPage active="platform-plans" title="Platform plans" sub="Prices are in minor units of the plan currency. Zero means free.">
      <div>
        {note}
        {error ? (
          <Msg text={error} bad />
        ) : rows ? (
          <div class="plan-editor">
            <Table head={['Id', 'Name', 'Mode', 'Monthly price', 'Included volume', 'Fee, basis points', 'Base fee', 'Active', '']}>
              {rows.map(r => (
                <PlanRow key={r.id} r={r} out={out} reload={reload} />
              ))}
            </Table>
          </div>
        ) : null}
      </div>
    </OperatorPage>
  );
}

interface Treasury {
  latest?: { alerts?: string[] } | null;
  assets: number | null;
  liabilities: number;
  difference: number | null;
  tolerance_msat: number;
  fees_earned: number;
  pending_payouts: number;
  wallets?: { identity: string; kind: string; msat: number; error?: string | null }[];
  stuck: { kind: string; tenant_id: number; reference: string; since: string; link: string }[];
  tenants: { tenant_id: number; name: string; balance: number; pending_payout: number }[];
  runs: { created: string; trigger_kind: string; assets: number; liabilities: number; difference: number; drift_accounts: number; unbalanced_transactions: number; stuck_count: number; status: string }[];
}

const STUCK_LABELS: Record<string, string> = {
  settled_without_credit: 'Settled, no ledger credit',
  withdrawal_sending: 'Withdrawal stuck sending',
  unconfirmed_receipt: 'On-chain receipt unconfirmed',
  paid_without_webhook: 'Paid, webhook not delivered',
};

function stuckHref(link: string): string {
  return link.startsWith('#/') ? href(link.slice(1)) : link;
}

export function TreasuryPage() {
  const { data: t, error, reload } = useLoad(() => api<Treasury>('GET', '/admin/treasury'), []);
  const [note, out] = useNote();
  const alerts = t?.latest?.alerts ?? [];
  return (
    <OperatorPage active="treasury" title="Treasury" sub="What the platform holds against what it owes. Runs hourly. Nothing here moves money or fixes balances.">
      <div>
        <div class="row">
          <button
            class="primary"
            onClick={async () => {
              try {
                await api('POST', '/admin/treasury/run', {});
                reload();
              } catch (e) {
                out(errText(e), true);
              }
            }}
          >
            Run reconciliation now
          </button>
        </div>
        {note}
        <div>
          {error ? (
            <Msg text={error} bad />
          ) : t ? (
            <>
              {alerts.length ? (
                <div class="banner bad" role="alert">
                  <div>{alerts.map((a, i) => <div key={i}>{a}</div>)}</div>
                </div>
              ) : null}
              <div class="ledger">
                <Figure value={t.assets === null ? 'No run yet' : sats(t.assets)} label="assets at the last run" />
                <Figure value={sats(t.liabilities)} label="owed to projects and payouts" />
                <Figure value={t.difference === null ? 'None' : sats(t.difference)} label="difference" tone={t.difference !== null && t.difference + t.tolerance_msat < 0 ? 'bad' : ''} />
                <Figure value={sats(t.fees_earned)} label="fees earned" />
                <Figure value={sats(t.pending_payouts)} label="payouts on the way" />
              </div>
              <h3>Wallets</h3>
              <Table head={['Wallet', 'Kind', 'Balance', 'Error']}>
                {(t.wallets ?? []).map((w, i) => (
                  <tr key={i}>
                    <td class="mono">{w.identity}</td>
                    <td>{w.kind}</td>
                    <td>{sats(w.msat)}</td>
                    <td class="muted">{w.error ?? ''}</td>
                  </tr>
                ))}
              </Table>
              <h3>Stuck money</h3>
              {t.stuck.length ? (
                <Table head={['Problem', 'Project', 'Reference', 'Since', '']}>
                  {t.stuck.map((i, n) => (
                    <tr key={n}>
                      <td>{STUCK_LABELS[i.kind] ?? i.kind}</td>
                      <td>{String(i.tenant_id)}</td>
                      <td class="mono">{i.reference}</td>
                      <td class="muted">{fmtWhen(i.since)}</td>
                      <td><a href={stuckHref(i.link)}>Open</a></td>
                    </tr>
                  ))}
                </Table>
              ) : (
                <p class="muted">Nothing stuck.</p>
              )}
              <h3>Balances per project</h3>
              <Table head={['Project', 'Balance', 'Pending payout']}>
                {t.tenants.map(r => (
                  <tr key={r.tenant_id}>
                    <td><a href={href(`/p/${r.tenant_id}/balance`)}>{r.name}</a></td>
                    <td>{sats(r.balance)}</td>
                    <td>{sats(r.pending_payout)}</td>
                  </tr>
                ))}
              </Table>
              <h3>Reconciliation history</h3>
              <Table head={['When', 'Trigger', 'Assets', 'Liabilities', 'Difference', 'Drift', 'Unbalanced', 'Stuck', 'Status']}>
                {t.runs.map((r, i) => (
                  <tr key={i}>
                    <td class="muted">{fmtWhen(r.created)}</td>
                    <td>{r.trigger_kind}</td>
                    <td>{sats(r.assets)}</td>
                    <td>{sats(r.liabilities)}</td>
                    <td>{sats(r.difference)}</td>
                    <td>{String(r.drift_accounts)}</td>
                    <td>{String(r.unbalanced_transactions)}</td>
                    <td>{String(r.stuck_count)}</td>
                    <td><Pill status={r.status === 'ok' ? 'completed' : 'failed'} word={r.status === 'ok' ? 'OK' : 'Alert'} /></td>
                  </tr>
                ))}
              </Table>
            </>
          ) : null}
        </div>
      </div>
    </OperatorPage>
  );
}

interface QueueItem {
  withdrawal: { id: number; status: string; purpose: string; amount: number; currency: string; destination_label: string; destination_kind: string; created: string };
  project: string;
  trusted: boolean;
  balances: { kind: string; balance: number; currency: string }[];
  completed_count: number;
  failed_count: number;
  recent: { id: number; status: string; amount: number; currency: string }[];
}

function QueueCard(props: { q: QueueItem; reload: () => void; fail: (t: string) => void }) {
  const { q } = props;
  const w = q.withdrawal;
  const decide = async (path: string, opts: { title: string; body: string; keep: string; confirm: string; danger?: boolean; input?: string; done: string }) => {
    const v = await confirmDialog(opts);
    if (!v) return;
    try {
      await api('POST', path, opts.input ? { reason: v.text } : {});
      toast(opts.done);
      props.reload();
    } catch (e) {
      props.fail(errText(e));
    }
  };
  const amount = fmtAmount(w.amount, w.currency);
  return (
    <div class="card">
      <div class="row">
        <b>{`#${w.id} ${q.project}`}</b>
        <Pill status={q.trusted ? 'completed' : 'pending'} word={q.trusted ? 'Live' : 'Test mode'} />
        <Pill status={w.status} word={`${w.status.charAt(0).toUpperCase()}${w.status.slice(1)}${w.purpose === 'verify' ? ', test' : ''}`} />
        <Money amount={w.amount} currency={w.currency} />
        <span class="mono">{`${w.destination_kind} ${w.destination_label}`}</span>
      </div>
      <div class="muted">
        <span>Balances: </span>
        {q.balances.length ? q.balances.map((b, i) => <span key={i}>{`${b.kind} `}<Money amount={b.balance} currency={b.currency} />{' '}</span>) : 'none'}
      </div>
      <div class="muted">{`Completed ${q.completed_count}, failed or rejected ${q.failed_count}. Requested ${fmtWhen(w.created)}.`}</div>
      <div class="muted">{`Recent: ${q.recent.map(r => `#${r.id} ${r.status} ${fmtAmount(r.amount, r.currency)}`).join('; ') || 'none'}`}</div>
      <div class="row">
        {w.status === 'requested' ? (
          <>
            <button
              class="primary"
              onClick={() =>
                decide(`/admin/withdrawals/${w.id}/approve`, {
                  title: 'Approve this withdrawal?',
                  body: `The worker sends ${amount} from ${q.project} to ${w.destination_label} as soon as it runs. This cannot be undone.`,
                  keep: 'Keep in queue',
                  confirm: 'Approve withdrawal',
                  done: 'Withdrawal approved',
                })
              }
            >
              Approve withdrawal
            </button>
            <button
              class="danger"
              onClick={() =>
                decide(`/admin/withdrawals/${w.id}/reject`, {
                  title: 'Reject this withdrawal?',
                  body: `The reserved ${amount} returns to ${q.project} and the project is told it failed.`,
                  keep: 'Keep in queue',
                  confirm: 'Reject withdrawal',
                  danger: true,
                  input: 'Reason shown to the project',
                  done: 'Withdrawal rejected',
                })
              }
            >
              Reject withdrawal
            </button>
          </>
        ) : null}
      </div>
    </div>
  );
}

export function PayoutQueue() {
  const [status, setStatus] = useState('requested');
  const { data: rows, error, reload } = useLoad(() => api<QueueItem[]>('GET', `/admin/withdrawals?status=${status}`), [status]);
  const [alertMsg, setAlert] = useState('');
  return (
    <OperatorPage active="payout-queue" title="Payout queue" sub="Approve or reject withdrawals that need review.">
      <div>
        <div class="filterbar">
          <select aria-label="Status" value={status} onChange={e => setStatus(e.currentTarget.value)}>
            {['requested', 'approved', 'sending', 'completed', 'failed', 'rejected', 'cancelled'].map(s => (
              <option value={s} key={s}>{s.charAt(0).toUpperCase() + s.slice(1)}</option>
            ))}
          </select>
        </div>
        {alertMsg || error ? <div class="err" role="alert">{alertMsg || error}</div> : null}
        <div>
          {!rows ? (
            <div><div class="skeleton" /><div class="skeleton" /></div>
          ) : rows.length ? (
            rows.map(q => <QueueCard key={q.withdrawal.id} q={q} reload={() => { setAlert(''); reload(); }} fail={setAlert} />)
          ) : (
            <div class="empty">
              <b>{`Nothing ${status}.`}</b>
              New withdrawals that need review appear here.
            </div>
          )}
        </div>
      </div>
    </OperatorPage>
  );
}
