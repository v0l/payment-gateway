import { useState } from 'preact/hooks';
import { api, errText, useLoad, type Project } from './api';
import { fmtDay, fmtWhen } from './format';
import { confirmDialog, toast } from './dialogs';
import { ProjectShell } from './shell';
import { Chips, Empty, Msg, Pill, SecretBox, Section, Skeleton, Table, TextField, AreaField, useNote } from './ui';
import { DeliveryDetail } from './deliveries';

interface KeyRow {
  id: number;
  name: string;
  prefix: string;
  scopes: string[];
  last_used?: string | null;
  expires?: string | null;
  revoked_at?: string | null;
}

const SCOPES = ['payments:read', 'payments:write', 'refunds:write', 'saved:write', 'admin'];

function KeysBody(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const { data: rows, error, reload } = useLoad(() => api<KeyRow[]>('GET', `/projects/${pid}/keys`), [pid]);
  const [name, setName] = useState('');
  const [expires, setExpires] = useState('');
  const [chosen, setChosen] = useState<string[]>([]);
  const [noteNode, out, outNode] = useNote();
  return (
    <>
      <Section title="Keys">
        <div>
          {error ? (
            <Msg text={error} bad />
          ) : !rows ? (
            <Skeleton />
          ) : rows.length ? (
            <Table head={['Name', 'Key', 'Scopes', 'Last used', 'Expires', '']}>
              {rows.map(k => {
                const state = k.revoked_at ? 'revoked' : k.expires && new Date(k.expires) < new Date() ? 'expired' : '';
                return (
                  <tr key={k.id}>
                    <td>{k.name}</td>
                    <td class="mono">{`${k.prefix}\u2026`}</td>
                    <td class="muted">{k.scopes.join(', ')}</td>
                    <td class="muted">{k.last_used ? fmtWhen(k.last_used) : 'Never'}</td>
                    <td class="muted">{k.expires ? fmtDay(k.expires) : 'Never'}</td>
                    <td class="num">
                      {state ? (
                        <Pill status={state === 'revoked' ? 'failed' : 'expired'} word={state === 'revoked' ? 'Revoked' : 'Expired'} />
                      ) : (
                        <button
                          class="link"
                          type="button"
                          disabled={p.role === 'viewer'}
                          onClick={async () => {
                            const r = await confirmDialog({ title: `Revoke ${k.name}`, body: 'Requests that use this key stop working immediately.', keep: 'Keep key', confirm: 'Revoke key', danger: true });
                            if (!r) return;
                            try {
                              await api('POST', `/projects/${pid}/keys/${k.id}/revoke`, {});
                              toast('Key revoked.');
                              reload();
                            } catch (e) {
                              out(errText(e), true);
                            }
                          }}
                        >
                          Revoke
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </Table>
          ) : (
            <Empty title="No extra keys yet." hint="Create a key per server so you can revoke one without touching the others." />
          )}
        </div>
      </Section>
      <Section title="Create a key">
        <p class="muted">{p.trusted ? 'Keys created here are live keys (pgw_live_).' : 'Keys created here are test keys (pgw_test_) until the project goes live.'}</p>
        <div class="card form">
          <TextField label="Name" placeholder="Key name, for example Billing server" value={name} onInput={setName} />
          <TextField label="Expires (optional)" type="date" value={expires} onInput={setExpires} />
          <div class="field">
            <label>Scopes</label>
            <div class="row">
              {SCOPES.map(s => (
                <label class="check" key={s}>
                  <input type="checkbox" value={s} checked={chosen.includes(s)} onChange={e => setChosen(e.currentTarget.checked ? [...chosen, s] : chosen.filter(x => x !== s))} />
                  {s}
                </label>
              ))}
            </div>
          </div>
          <div class="row">
            <button
              class="primary"
              disabled={p.role === 'viewer'}
              onClick={async () => {
                const req: Record<string, unknown> = { name, scopes: chosen };
                if (expires) req.expires = new Date(`${expires}T23:59:59Z`).toISOString();
                try {
                  const r = await api<{ name: string; api_key: string }>('POST', `/projects/${pid}/keys`, req);
                  outNode(<SecretBox title={`New API key (${r.name})`} value={r.api_key} />);
                  setName('');
                  reload();
                } catch (e) {
                  out(errText(e), true);
                }
              }}
            >
              Create key
            </button>
          </div>
          {noteNode}
        </div>
      </Section>
    </>
  );
}

export function ApiKeys(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="keys" sub="Keys your servers use to call the API. Each key shows once, when you create it.">
      {p => <KeysBody pid={props.pid} p={p} />}
    </ProjectShell>
  );
}

interface Delivery {
  id: number;
  created: string;
  event_type: string;
  delivered_at?: string | null;
  abandoned_at?: string | null;
  attempts: number;
  last_error?: string | null;
}

interface Endpoint {
  id: number;
  url: string;
  events: string[];
  enabled: boolean;
  delivered: number;
  failed: number;
  pending: number;
}

interface Health {
  endpoints: { url: string; state: string; deliveries_7d: number; failed_7d: number; last_success?: string | null; last_failure?: string | null }[];
}

function SignatureHelper(props: { pid: string }) {
  const [body, setBody] = useState('');
  const [secret, setSecret] = useState('');
  const [sig, setSig] = useState('');
  const [note, out] = useNote();
  return (
    <Section title="Check a signature">
      <div class="card form">
        <AreaField label="Body" rows={4} placeholder="Raw request body exactly as received" value={body} onInput={setBody} />
        <TextField label="Secret" placeholder="Signing secret (whsec_...)" autocomplete="off" value={secret} onInput={setSecret} />
        <TextField label="Signature header" placeholder="x-gateway-signature header value" value={sig} onInput={setSig} />
        <div class="row">
          <button
            onClick={async () => {
              try {
                const r = await api<{ valid: boolean; age_seconds: number }>('POST', `/projects/${props.pid}/webhook-verify`, { body, secret, signature: sig.trim() });
                out(r.valid ? `The signature is valid. Age ${r.age_seconds} s.` : 'The signature does not match.', !r.valid);
              } catch (e) {
                out(errText(e), true);
              }
            }}
          >
            Check signature
          </button>
        </div>
        {note}
      </div>
    </Section>
  );
}

function WebhooksBody(props: { pid: string; p: Project }) {
  const { pid, p } = props;
  const canWrite = p.role !== 'viewer';
  const [undeliveredOnly, setUndelivered] = useState(false);
  const deliveries = useLoad(() => api<Delivery[]>('GET', `/projects/${pid}/deliveries${undeliveredOnly ? '?undelivered=1' : ''}`), [pid, undeliveredOnly]);
  const health = useLoad(() => api<Health>('GET', `/projects/${pid}/webhook-health`), [pid]);
  const endpoints = useLoad(() => api<Endpoint[]>('GET', `/projects/${pid}/webhook-endpoints`), [pid]);
  const [detail, setDetail] = useState<number | null>(null);
  const [toolNote, toolOut] = useNote();
  const [epNote, epOut, epNode] = useNote();
  const [epUrl, setEpUrl] = useState('');
  const [epEvents, setEpEvents] = useState('');
  const rows = deliveries.data;
  return (
    <>
      <Section
        title="Deliveries"
        aside={
          <div class="actions">
            <Chips options={[['all', 'All'], ['undelivered', 'Not delivered']]} value="all" onPick={v => setUndelivered(v === 'undelivered')} label="Show" />
            <button
              disabled={!canWrite}
              onClick={async () => {
                try {
                  const r = await api<{ queued: number }>('POST', `/projects/${pid}/deliveries-resend-failed`, {});
                  toolOut(`${r.queued} queued for delivery.`);
                  deliveries.reload();
                } catch (e) {
                  toolOut(errText(e), true);
                }
              }}
            >
              Send all failed again
            </button>
          </div>
        }
      >
        {toolNote}
        <div>
          {deliveries.error ? (
            <Msg text={deliveries.error} bad />
          ) : !rows ? (
            <Skeleton />
          ) : rows.length ? (
            <Table head={['Sent', 'Event', 'State', ['Tries', 'num'], 'Error', '']}>
              {rows.map(d => {
                const state = d.delivered_at ? 'delivered' : d.abandoned_at ? 'failed' : 'waiting';
                return (
                  <tr key={d.id}>
                    <td class="muted">{fmtWhen(d.created)}</td>
                    <td>{d.event_type}</td>
                    <td><Pill status={state} word={d.delivered_at ? 'Delivered' : d.abandoned_at ? 'Gave up' : 'Retrying'} /></td>
                    <td class="num">{String(d.attempts)}</td>
                    <td class="muted trunc">{d.last_error ?? ''}</td>
                    <td class="num">
                      <button class="link" type="button" onClick={() => setDetail(d.id)}>Details</button>{' '}
                      <button
                        class="link"
                        type="button"
                        disabled={!canWrite}
                        onClick={async () => {
                          await api('POST', `/projects/${pid}/deliveries/${d.id}/replay`, {});
                          toast('Webhook queued.');
                          deliveries.reload();
                        }}
                      >
                        Send again
                      </button>
                    </td>
                  </tr>
                );
              })}
            </Table>
          ) : (
            <Empty title={undeliveredOnly ? 'Everything was delivered.' : 'No webhooks sent yet.'} hint="Events are sent here when payments change." />
          )}
        </div>
        <div>{detail !== null ? <DeliveryDetail pid={pid} did={detail} /> : null}</div>
      </Section>
      <Section title="Endpoints">
        {p.webhook_url ? (
          <p class="muted">
            {'Default endpoint from Settings: '}
            <span class="mono">{p.webhook_url}</span>
          </p>
        ) : (
          <p class="muted">No default endpoint is set in Settings.</p>
        )}
        <div>
          {endpoints.error ? (
            <Msg text={endpoints.error} bad />
          ) : endpoints.data && endpoints.data.length ? (
            <Table head={['Endpoint', 'Events', 'State', ['Delivered', 'num'], ['Failed', 'num'], ['Waiting', 'num'], '']}>
              {endpoints.data.map(e => {
                const base = `/projects/${pid}/webhook-endpoints/${e.id}`;
                return (
                  <tr key={e.id}>
                    <td class="trunc">{e.url}</td>
                    <td class="muted trunc">{e.events.length ? e.events.join(', ') : 'All'}</td>
                    <td><Pill status={e.enabled ? 'active' : 'expired'} word={e.enabled ? 'On' : 'Off'} /></td>
                    <td class="num">{String(e.delivered)}</td>
                    <td class="num">{String(e.failed)}</td>
                    <td class="num">{String(e.pending)}</td>
                    <td class="num">
                      <button
                        class="link"
                        type="button"
                        disabled={!canWrite}
                        onClick={async () => {
                          const r = await api<{ ok: boolean; error?: string }>('POST', `${base}/test`, {});
                          epOut(r.ok ? 'Test event delivered.' : `Test failed: ${r.error}`, !r.ok);
                        }}
                      >
                        Send test
                      </button>{' '}
                      <button
                        class="link"
                        type="button"
                        disabled={!canWrite}
                        onClick={async () => {
                          await api('POST', base, { enabled: !e.enabled });
                          endpoints.reload();
                        }}
                      >
                        {e.enabled ? 'Turn off' : 'Turn on'}
                      </button>{' '}
                      <button
                        class="link"
                        type="button"
                        disabled={!canWrite}
                        onClick={async () => {
                          try {
                            const r = await api<{ secret: string }>('POST', `${base}/rotate-secret`, {});
                            epNode(<SecretBox title="New signing secret" value={r.secret} />);
                          } catch (x) {
                            epOut(errText(x), true);
                          }
                        }}
                      >
                        New secret
                      </button>{' '}
                      <button
                        class="link"
                        type="button"
                        disabled={!canWrite}
                        onClick={async () => {
                          const v = await confirmDialog({ title: 'Delete this endpoint', body: `Events stop going to ${e.url}.`, keep: 'Keep it', confirm: 'Delete endpoint', danger: true });
                          if (v) {
                            await api('DELETE', base);
                            endpoints.reload();
                          }
                        }}
                      >
                        Delete
                      </button>
                    </td>
                  </tr>
                );
              })}
            </Table>
          ) : endpoints.data ? (
            <p class="muted">No endpoints yet. Add one below.</p>
          ) : null}
        </div>
        <div class="row">
          <input placeholder="https://example.com/hook" aria-label="Endpoint URL" value={epUrl} onInput={e => setEpUrl(e.currentTarget.value)} />
          <input placeholder="Events, comma separated. Empty for all" aria-label="Events" value={epEvents} onInput={e => setEpEvents(e.currentTarget.value)} />
          <button
            disabled={!canWrite}
            onClick={async () => {
              const events = epEvents.split(',').map(x => x.trim()).filter(Boolean);
              try {
                const r = await api<{ secret: string }>('POST', `/projects/${pid}/webhook-endpoints`, { url: epUrl, events });
                epNode(<SecretBox title="Signing secret" value={r.secret} />);
                setEpUrl('');
                endpoints.reload();
              } catch (e) {
                epOut(errText(e), true);
              }
            }}
          >
            Add endpoint
          </button>
        </div>
        {epNote}
      </Section>
      <Section title="Endpoint health">
        <div>
          {health.error ? (
            <Msg text={health.error} bad />
          ) : !health.data ? (
            <Skeleton />
          ) : health.data.endpoints.length ? (
            <Table head={['Endpoint', 'State', ['Sent, 7 days', 'num'], ['Failed', 'num'], 'Last success', 'Last failure']}>
              {health.data.endpoints.map((e, i) => (
                <tr key={i}>
                  <td class="trunc">{e.url}</td>
                  <td><Pill status={e.state === 'healthy' ? 'completed' : e.state === 'failing' ? 'failed' : 'pending'} word={e.state.charAt(0).toUpperCase() + e.state.slice(1)} /></td>
                  <td class="num">{String(e.deliveries_7d)}</td>
                  <td class="num">{String(e.failed_7d)}</td>
                  <td class="muted">{e.last_success ? fmtWhen(e.last_success) : ''}</td>
                  <td class="muted">{e.last_failure ? fmtWhen(e.last_failure) : ''}</td>
                </tr>
              ))}
            </Table>
          ) : (
            <p class="muted">No endpoints yet.</p>
          )}
        </div>
      </Section>
      <SignatureHelper pid={pid} />
    </>
  );
}

export function Webhooks(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="webhooks" sub="Events sent to your servers when payments change.">
      {p => <WebhooksBody pid={props.pid} p={p} />}
    </ProjectShell>
  );
}
