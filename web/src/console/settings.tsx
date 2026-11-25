import { useEffect, useState } from 'preact/hooks';
import { api, errText, getSession, setSession, useLoad, type Me, type Project } from './api';
import { fmtWhen } from './format';
import { confirmDialog, toast } from './dialogs';
import { go } from './nav';
import { ProjectShell } from './shell';
import { AreaField, Check, Msg, SecretBox, Section, SelectField, Skeleton, Table, TextField, useNote } from './ui';
import type { JSX } from 'preact';

interface Branding {
  default_lang?: string;
  theme: string;
  radius: string;
  font: string;
  accent_dark?: string | null;
  logo_dark_url?: string | null;
  thanks_text?: string | null;
  footer_links?: { label: string; url: string }[];
  show_powered_by?: boolean;
  can_hide_powered_by?: boolean;
}

function BrandingForm(props: { pid: string }) {
  const { pid } = props;
  const { data: b, error } = useLoad(() => api<Branding>('GET', `/projects/${pid}/branding`), [pid]);
  const [f, setF] = useState<Record<string, string>>({});
  const [links, setLinks] = useState('');
  const [powered, setPowered] = useState(true);
  const [note, out] = useNote();
  useEffect(() => {
    if (!b) return;
    setF({
      default_lang: b.default_lang || 'en',
      theme: b.theme,
      radius: b.radius,
      font: b.font,
      accent_dark: b.accent_dark || '',
      logo_dark_url: b.logo_dark_url || '',
      thanks_text: b.thanks_text || '',
    });
    setLinks((b.footer_links ?? []).map(l => `${l.label} | ${l.url}`).join('\n'));
    setPowered(b.show_powered_by !== false);
  }, [b]);
  if (error) return <div class="err" role="alert">{error}</div>;
  if (!b) return null;
  const set = (k: string) => (v: string) => setF({ ...f, [k]: v });
  const opts = (xs: string[]): [string, string][] => xs.map(x => [x, x]);
  return (
    <div class="card form">
      <SelectField label="Default language" value={f.default_lang ?? 'en'} onChange={set('default_lang')} options={opts(['en', 'es', 'fr', 'de', 'pt', 'it', 'ga'])} />
      <SelectField label="Theme" value={f.theme ?? ''} onChange={set('theme')} options={opts(['auto', 'light', 'dark'])} />
      <SelectField label="Corner radius" value={f.radius ?? ''} onChange={set('radius')} options={opts(['none', 'sm', 'md', 'lg'])} />
      <SelectField label="Font" value={f.font ?? ''} onChange={set('font')} options={opts(['system', 'serif', 'mono', 'rounded'])} />
      <TextField label="Accent colour in dark mode" placeholder="#52c3d1" value={f.accent_dark ?? ''} onInput={set('accent_dark')} />
      <TextField label="Logo URL for dark mode" placeholder="https://..." value={f.logo_dark_url ?? ''} onInput={set('logo_dark_url')} />
      <AreaField label="Thank-you text" placeholder="Shown after a successful payment" value={f.thanks_text ?? ''} onInput={set('thanks_text')} />
      <AreaField label="Footer links" placeholder="One per line: Label | https://..." value={links} onInput={setLinks} />
      <div class="field">
        <Check checked={powered} disabled={!b.can_hide_powered_by} onChange={setPowered}>Show Powered by on the pay page</Check>
      </div>
      <div class="row">
        <button
          class="primary"
          onClick={async () => {
            const payload: Record<string, unknown> = {};
            ['default_lang', 'theme', 'radius', 'font'].forEach(k => (payload[k] = f[k]));
            ['accent_dark', 'logo_dark_url', 'thanks_text'].forEach(k => (payload[k] = (f[k] ?? '').trim() || null));
            payload.footer_links = links
              .split('\n')
              .map(l => l.trim())
              .filter(Boolean)
              .map(l => {
                const i = l.indexOf('|');
                return { label: l.slice(0, i).trim(), url: l.slice(i + 1).trim() };
              });
            if (b.can_hide_powered_by) payload.show_powered_by = powered;
            try {
              await api('POST', `/projects/${pid}/branding`, payload);
              out('Saved.');
            } catch (e) {
              out(errText(e), true);
            }
          }}
        >
          Save branding
        </button>
      </div>
      {note}
    </div>
  );
}

interface Limits {
  limits: Record<string, number>;
  defaults: Record<string, number>;
  usage: Record<string, number>;
}

const LIMIT_ROWS: [string, string, string | null][] = [
  ['payments_per_day', 'Payments per 24 hours', 'payments_per_day'],
  ['pending_payments', 'Open pending payments', 'pending_payments'],
  ['amount_per_payment', 'Max amount per payment (EUR cents)', null],
  ['volume_per_day', 'Volume per 24 hours (EUR cents)', 'volume_per_day'],
  ['providers', 'Providers', 'providers'],
  ['webhook_endpoints', 'Webhook endpoints', 'webhook_endpoints'],
  ['attempts_per_payment', 'Attempts per payment', null],
];

function LimitsBox(props: { pid: string }) {
  const { pid } = props;
  const me = getSession().me!;
  const { data: v, error } = useLoad(() => api<Limits>('GET', `/projects/${pid}/limits`), [pid]);
  const [vals, setVals] = useState<Record<string, string>>({});
  const [note, out] = useNote();
  useEffect(() => {
    if (v) setVals(Object.fromEntries(LIMIT_ROWS.map(r => [r[0], String(v.limits[r[0]])])));
  }, [v]);
  if (error) return <div>{error}</div>;
  if (!v) return null;
  return (
    <div>
      <Table head={['Limit', ['Used', 'num'], 'Limit', ['Default', 'num']]}>
        {LIMIT_ROWS.map(r => (
          <tr key={r[0]}>
            <td>{r[1]}</td>
            <td class="num">{r[2] ? String(v.usage[r[2]]) : ''}</td>
            <td>
              {me.is_operator ? (
                <input value={vals[r[0]] ?? ''} type="number" min="0" aria-label={r[1]} onInput={e => setVals({ ...vals, [r[0]]: e.currentTarget.value })} />
              ) : (
                <span>{String(v.limits[r[0]])}</span>
              )}
            </td>
            <td class="num muted">{String(v.defaults[r[0]])}</td>
          </tr>
        ))}
      </Table>
      {note}
      {me.is_operator ? (
        <button
          class="primary"
          onClick={async () => {
            const payload: Record<string, number | null> = {};
            Object.keys(vals).forEach(k => {
              const n = parseInt(vals[k], 10);
              payload[k] = n === v.defaults[k] ? null : n;
            });
            try {
              await api('POST', `/admin/projects/${pid}/limits`, payload);
              out('Saved.');
            } catch (e) {
              out(errText(e), true);
            }
          }}
        >
          Save limits
        </button>
      ) : null}
    </div>
  );
}

interface SettingsData {
  legal_name?: string | null;
  legal_address?: string | null;
  legal_vat_id?: string | null;
  legal_email?: string | null;
  notify_webhook_abandoned?: boolean;
  notify_provider_failing?: boolean;
  notify_refund_failed?: boolean;
  allow_manual_paid?: boolean;
  cooling_off_hours: number;
  delete_requested_at?: string | null;
}

function ConfigPanels(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const { data: s, error } = useLoad(() => api<SettingsData>('GET', `/projects/${pid}/settings`), [pid]);
  const [legal, setLegal] = useState({ name: '', address: '', vat_id: '', email: '' });
  const [notify, setNotify] = useState<Record<string, boolean>>({});
  const [manual, setManual] = useState(false);
  const [legalNote, legalOut] = useNote();
  const [notifNote, notifOut] = useNote();
  const [manualNote, manualOut] = useNote();
  useEffect(() => {
    if (!s) return;
    setLegal({ name: s.legal_name || '', address: s.legal_address || '', vat_id: s.legal_vat_id || '', email: s.legal_email || '' });
    setNotify({ webhook_abandoned: !!s.notify_webhook_abandoned, provider_failing: !!s.notify_provider_failing, refund_failed: !!s.notify_refund_failed });
    setManual(!!s.allow_manual_paid);
  }, [s]);
  if (error) return <Msg text={error} bad />;
  if (!s) return <div />;
  const kinds: [string, string][] = [['webhook_abandoned', 'A webhook delivery is abandoned'], ['provider_failing', 'A provider keeps failing'], ['refund_failed', 'A refund fails']];
  return (
    <div>
      <Section title="Legal details for receipts">
        <div class="card form">
          <TextField label="Legal name" value={legal.name} onInput={v => setLegal({ ...legal, name: v })} />
          <AreaField label="Address" rows={3} value={legal.address} onInput={v => setLegal({ ...legal, address: v })} />
          <TextField label="VAT id" value={legal.vat_id} onInput={v => setLegal({ ...legal, vat_id: v })} />
          <TextField label="Contact email" value={legal.email} onInput={v => setLegal({ ...legal, email: v })} />
          <div class="row">
            <button
              class="primary"
              disabled={p.role === 'viewer'}
              onClick={async () => {
                try {
                  await api('POST', `/projects/${pid}/settings/legal`, legal);
                  legalOut('Saved.');
                } catch (e) {
                  legalOut(errText(e), true);
                }
              }}
            >
              Save legal details
            </button>
          </div>
          {legalNote}
        </div>
      </Section>
      <Section title="Notifications">
        <div class="card">
          {kinds.map(k => (
            <div key={k[0]}>
              <Check checked={!!notify[k[0]]} onChange={v => setNotify({ ...notify, [k[0]]: v })}>{k[1]}</Check>
            </div>
          ))}
          <div class="row">
            <button
              disabled={p.role === 'viewer'}
              onClick={async () => {
                try {
                  await api('POST', `/projects/${pid}/settings/notifications`, notify);
                  notifOut('Saved.');
                } catch (e) {
                  notifOut(errText(e), true);
                }
              }}
            >
              Save notifications
            </button>
          </div>
          {notifNote}
        </div>
      </Section>
      <Section title="Manual payment handling">
        <div class="card">
          <Check
            checked={manual}
            disabled={p.role !== 'owner'}
            onChange={async v => {
              setManual(v);
              try {
                await api('POST', `/projects/${pid}/manual-paid`, { enabled: v });
                manualOut('Saved.');
              } catch (e) {
                setManual(!v);
                manualOut(errText(e), true);
              }
            }}
          >
            Let admins mark waiting payments as paid, with a reason. Needed for Accept in the attention queue.
          </Check>
          {manualNote}
        </div>
      </Section>
    </div>
  );
}

function DangerZone(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const { data: s, reload } = useLoad(() => api<SettingsData>('GET', `/projects/${pid}/settings`), [pid]);
  const [token, setToken] = useState('');
  const [note, out, outNode] = useNote();
  if (!s) return <Section title="Disable or delete"><div class="card" /></Section>;
  return (
    <Section title="Disable or delete">
      <div class="card">
        <div class="row">
          <button
            class="danger"
            disabled={p.enabled === false}
            onClick={async () => {
              const r = await confirmDialog({ title: 'Disable project', body: 'API keys stop working and no new payments are accepted. You can enable it again from the operator area or by calling the settings endpoint.', keep: 'Keep enabled', confirm: 'Disable', danger: true });
              if (!r) return;
              try {
                await api('POST', `/projects/${pid}/settings/enabled`, { enabled: false });
                toast('Project disabled.');
                location.assign('/console/');
              } catch (e) {
                out(errText(e), true);
              }
            }}
          >
            {p.enabled === false ? 'Project disabled' : 'Disable project'}
          </button>
        </div>
        <p class="muted">{`Deleting a project needs a token and a ${s.cooling_off_hours} hour cooling-off period. It is blocked while there are pending payments or a balance.`}</p>
        {s.delete_requested_at ? (
          <>
            <p>{`Deletion requested ${fmtWhen(s.delete_requested_at)}. Paste the token to confirm once the cooling-off has passed.`}</p>
            <div class="row">
              <input placeholder="Deletion token" aria-label="Deletion token" autocomplete="off" value={token} onInput={e => setToken(e.currentTarget.value)} />
              <button
                class="danger"
                onClick={async () => {
                  const r = await confirmDialog({ title: 'Delete project', body: 'This disables the project, revokes its keys and removes its members. It cannot be undone.', keep: 'Keep project', confirm: 'Delete forever', danger: true });
                  if (!r) return;
                  try {
                    await api('POST', `/projects/${pid}/delete-confirm`, { token });
                    toast('Project deleted.');
                    setSession(await api<Me>('GET', '/me'));
                    go('/');
                  } catch (e) {
                    out(errText(e), true);
                  }
                }}
              >
                Delete project
              </button>
              <button
                onClick={async () => {
                  await api('DELETE', `/projects/${pid}/delete-request`);
                  reload();
                }}
              >
                Cancel request
              </button>
            </div>
          </>
        ) : (
          <button
            class="danger"
            onClick={async () => {
              try {
                const r = await api<{ token: string }>('POST', `/projects/${pid}/delete-request`, {});
                outNode(<SecretBox title="Deletion token" value={r.token} />);
                reload();
              } catch (e) {
                out(errText(e), true);
              }
            }}
          >
            Request deletion
          </button>
        )}
      </div>
      {note}
    </Section>
  );
}

function ProjectForm(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const [f, setF] = useState<Record<string, string>>({
    webhook_url: p.webhook_url || '',
    display_name: p.display_name || '',
    logo_url: p.logo_url || '',
    accent_color: p.accent_color || '',
    allowed_origins: (p.allowed_origins ?? []).join('\n'),
    default_success_url: p.default_success_url || '',
    default_cancel_url: p.default_cancel_url || '',
    legal_name: p.legal_name || '',
    address: p.address || '',
    tax_id: p.tax_id || '',
    support_email: p.support_email || '',
  });
  const [receipts, setReceipts] = useState(p.customer_receipts !== false);
  const [note, out] = useNote();
  const set = (k: string) => (v: string) => setF({ ...f, [k]: v });
  return (
    <div class="card form">
      <TextField label="Webhook URL (https)" placeholder="https://example.com/hooks/payments" value={f.webhook_url} onInput={set('webhook_url')} />
      <TextField label="Display name on pay page" value={f.display_name} onInput={set('display_name')} />
      <TextField label="Logo URL" placeholder="https://..." value={f.logo_url} onInput={set('logo_url')} />
      <TextField label="Accent colour" placeholder="#5b3df5" value={f.accent_color} onInput={set('accent_color')} />
      <TextField label="Default success URL" value={f.default_success_url} onInput={set('default_success_url')} />
      <TextField label="Default cancel URL" value={f.default_cancel_url} onInput={set('default_cancel_url')} />
      <AreaField label="Allowed embed origins" placeholder="One per line. Sites allowed to embed the pay page." value={f.allowed_origins} onInput={set('allowed_origins')} />
      <TextField label="Legal name (receipts)" placeholder="Seller name on receipts" value={f.legal_name} onInput={set('legal_name')} />
      <AreaField label="Address (receipts)" placeholder="Seller address on receipts" value={f.address} onInput={set('address')} />
      <TextField label="Tax ID (receipts)" placeholder="VAT or tax ID" value={f.tax_id} onInput={set('tax_id')} />
      <TextField label="Support email (receipts)" placeholder="support@example.com" value={f.support_email} onInput={set('support_email')} />
      <Check checked={receipts} onChange={setReceipts} disabled={p.role === 'viewer'}>
        <span>Email receipts to customers. Turn off if your app sends its own. Owner notifications are not affected.</span>
      </Check>
      <div class="row">
        <button
          class="primary"
          disabled={p.role === 'viewer'}
          onClick={async () => {
            const payload: Record<string, unknown> = {};
            Object.keys(f).forEach(k => (payload[k] = f[k].trim() || null));
            payload.allowed_origins = f.allowed_origins.split(/\s+/).filter(Boolean);
            payload.customer_receipts = receipts;
            try {
              await api('POST', `/projects/${pid}/settings`, payload);
              out('Saved.');
            } catch (e) {
              out(errText(e), true);
            }
          }}
        >
          Save settings
        </button>
      </div>
      {note}
    </div>
  );
}

function SettingsBody(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const me = getSession().me!;
  const [req, setReq] = useState(!!p.require_2fa);
  const [reqNote, reqOut] = useNote();
  const [keyNote, keyOut, keyNode] = useNote();
  const [trustNote, trustOut] = useNote();
  const rotate = async (title: string, body: string, keep: string, confirm: string, path: string, label: string, field: string) => {
    const r = await confirmDialog({ title, body, keep, confirm, danger: true });
    if (!r) return;
    try {
      const x = await api<Record<string, string>>('POST', `/projects/${pid}${path}`, {});
      keyNode(<SecretBox title={label} value={x[field]} />);
    } catch (e) {
      keyOut(errText(e), true);
    }
  };
  const operator: JSX.Element | null = me.is_operator ? (
    <Section title="Operator">
      <div class="card">
        <p class="muted">{p.trusted ? 'This project is live.' : 'This project is in test mode with lower limits.'}</p>
        <div class="row">
          <button
            onClick={async () => {
              try {
                await api('POST', `/admin/projects/${pid}/trusted`, { trusted: !p.trusted });
                trustOut('Updated. Reload to see it.');
              } catch (e) {
                trustOut(errText(e), true);
              }
            }}
          >
            {p.trusted ? 'Move back to test limits' : 'Lift test limits and go live'}
          </button>
        </div>
        {trustNote}
      </div>
    </Section>
  ) : null;
  return (
    <>
      <Section title="Project"><ProjectForm pid={pid} p={p} /></Section>
      <Section title="Pay page branding"><BrandingForm pid={pid} /></Section>
      <Section title="Limits and usage"><LimitsBox pid={pid} /></Section>
      {p.role === 'owner' ? (
        <Section title="Security">
          <div class="card">
            <Check
              checked={req}
              onChange={async v => {
                setReq(v);
                try {
                  await api('POST', `/projects/${pid}/require-2fa`, { required: v });
                  reqOut('Saved.');
                } catch (e) {
                  setReq(!v);
                  reqOut(errText(e), true);
                }
              }}
            >
              Require two factor for members of this project
            </Check>
            {reqNote}
          </div>
        </Section>
      ) : null}
      <ConfigPanels pid={pid} p={p} />
      {operator}
      <Section title="Keys and secrets">
        <div class="row">
          <button disabled={p.role === 'viewer'} onClick={() => rotate('Rotate the project API key', 'The old key stops working immediately.', 'Keep the old key', 'Rotate key', '/rotate-key', 'New API key', 'api_key')}>Rotate API key</button>
          {p.role === 'owner' ? (
            <button onClick={() => rotate('Rotate the webhook signing secret', 'Update your receiver with the new secret straight away, or it will reject events.', 'Keep the old secret', 'Rotate secret', '/rotate-webhook-secret', 'New webhook secret', 'webhook_secret')}>
              Rotate webhook signing secret
            </button>
          ) : null}
        </div>
        {keyNote}
      </Section>
      {p.role === 'owner' ? <DangerZone pid={pid} p={p} /> : null}
    </>
  );
}

export function Settings(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="settings" sub="How the pay page looks, where webhooks go and who can do what.">
      {p => <SettingsBody pid={props.pid} p={p} />}
    </ProjectShell>
  );
}

interface Member {
  account_id: number;
  email: string;
  role: string;
}

function TeamBody(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const me = getSession().me!;
  const { data: rows, error, reload } = useLoad(() => api<Member[]>('GET', `/projects/${pid}/members`), [pid]);
  const [email, setEmail] = useState('');
  const [role, setRole] = useState('viewer');
  const [note, out, outNode] = useNote();
  return (
    <>
      <Section title="Members">
        <div>
          {error ? (
            <Msg text={error} bad />
          ) : !rows ? (
            <Skeleton />
          ) : (
            <Table head={['Member', 'Role', '']}>
              {rows.map(m => (
                <tr key={m.account_id}>
                  <td>{m.email}</td>
                  <td><span class="pill ok">{m.role.charAt(0).toUpperCase() + m.role.slice(1)}</span></td>
                  <td class="num">
                    {m.account_id === me.id || p.role !== 'owner' ? null : (
                      <button
                        class="link"
                        type="button"
                        onClick={async () => {
                          const r = await confirmDialog({ title: `Remove ${m.email}`, body: 'They lose access to this project straight away.', keep: 'Keep member', confirm: 'Remove member', danger: true });
                          if (!r) return;
                          try {
                            await api('DELETE', `/projects/${pid}/members/${m.account_id}`);
                            toast('Member removed.');
                            reload();
                          } catch (e) {
                            out(errText(e), true);
                          }
                        }}
                      >
                        Remove
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </Table>
          )}
        </div>
      </Section>
      {p.role !== 'viewer' ? (
        <Section title="Invite a teammate">
          <div class="row">
            <input type="email" placeholder="teammate@example.com" aria-label="Email" value={email} onInput={e => setEmail(e.currentTarget.value)} />
            <select aria-label="Role" value={role} onChange={e => setRole(e.currentTarget.value)}>
              {[['viewer', 'Viewer, can read'], ['admin', 'Admin, can take actions'], ['owner', 'Owner, can change everything']].map(r => (
                <option value={r[0]} key={r[0]}>{r[1]}</option>
              ))}
            </select>
            <button
              class="primary"
              onClick={async () => {
                try {
                  const r = await api<{ email: string; expires_in_days: number; invite_url: string }>('POST', `/projects/${pid}/invites`, { email, role });
                  outNode(<SecretBox title={`Invite link for ${r.email}, valid ${r.expires_in_days} days`} value={r.invite_url} />);
                } catch (e) {
                  out(errText(e), true);
                }
              }}
            >
              Create invite link
            </button>
          </div>
          {note}
        </Section>
      ) : null}
    </>
  );
}

export function Team(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="team" sub="People who can see or work on this project.">
      {p => <TeamBody pid={props.pid} p={p} />}
    </ProjectShell>
  );
}
