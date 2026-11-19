import { api, useLoad } from './api';

interface DeliveryInfo {
  request: { method: string; url: string; body: string };
  response?: { status?: number | null; duration_ms?: number | null; body?: string | null };
  time_to_deliver_ms?: number | null;
  last_error?: string | null;
}

export function DeliveryDetail(props: { pid: string; did: number }) {
  const { data: d, error } = useLoad(() => api<DeliveryInfo>('GET', `/projects/${props.pid}/deliveries/${props.did}`), [props.pid, props.did]);
  if (error) return <div class="err">{error}</div>;
  if (!d) return null;
  const resp = d.response ?? {};
  return (
    <div class="card delivery-open">
      <dl class="kv">
        <dt>Request</dt>
        <dd class="mono">{`${d.request.method} ${d.request.url}`}</dd>
        <dt>Response status</dt>
        <dd>{resp.status === null || resp.status === undefined ? 'none' : String(resp.status)}</dd>
        <dt>Duration</dt>
        <dd>{resp.duration_ms === null || resp.duration_ms === undefined ? '' : `${resp.duration_ms} ms`}</dd>
        <dt>Time to deliver</dt>
        <dd>{d.time_to_deliver_ms === null || d.time_to_deliver_ms === undefined ? 'not delivered' : `${d.time_to_deliver_ms} ms`}</dd>
        <dt>Error</dt>
        <dd>{d.last_error ?? ''}</dd>
      </dl>
      <h3>Request body</h3>
      <pre>{d.request.body}</pre>
      <h3>Response snippet</h3>
      <pre>{resp.body ?? ''}</pre>
    </div>
  );
}
