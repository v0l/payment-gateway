import { useEffect, useState } from 'preact/hooks';
import { api, errText, useLoad, type Project } from './api';
import { fmtWhen } from './format';
import { ProjectShell, OperatorPage } from './shell';
import { Empty, Msg, Skeleton, Table } from './ui';

interface AuditRow {
  id: number;
  created: string;
  actor_email?: string | null;
  actor_kind: string;
  actor_id?: string | number | null;
  action: string;
  target_kind?: string | null;
  target_id?: string | null;
  ip?: string | null;
  tenant_id: number | null;
  details?: unknown;
}

function ActivityTable(props: { rows: AuditRow[]; withProject: boolean }) {
  const { rows, withProject } = props;
  const head = ['When', 'Who', 'Action', 'Target', 'IP'];
  if (withProject) head.splice(1, 0, 'Project');
  if (!rows.length) return <Empty title="Nothing recorded yet." hint="Sign-ins, key changes, refunds and settings changes are recorded here." />;
  return (
    <Table head={head}>
      {rows.map(e => {
        const actor = e.actor_email || e.actor_kind + (e.actor_id ? ` ${e.actor_id}` : '');
        const target = e.target_kind ? `${e.target_kind} ${e.target_id || ''}` : '';
        return (
          <tr title={e.details ? JSON.stringify(e.details) : ''} key={e.id}>
            <td class="muted" title={e.created}>{fmtWhen(e.created)}</td>
            {withProject ? <td class="muted">{e.tenant_id === null ? 'Platform' : String(e.tenant_id)}</td> : null}
            <td>{actor}</td>
            <td>{e.action}</td>
            <td class="mono trunc">{target}</td>
            <td class="muted">{e.ip ?? ''}</td>
          </tr>
        );
      })}
    </Table>
  );
}

function ActivityView(props: { path: string; withProject: boolean }) {
  const [action, setAction] = useState('');
  const [actor, setActor] = useState('');
  const [rows, setRows] = useState<AuditRow[] | null>(null);
  const [more, setMore] = useState(false);
  const [error, setError] = useState('');
  const load = async (append: boolean) => {
    const before = append && rows?.length ? rows[rows.length - 1].id : null;
    const q = ['limit=50'];
    if (action.trim()) q.push(`action=${encodeURIComponent(action.trim())}`);
    if (actor.trim()) q.push(`actor=${encodeURIComponent(actor.trim())}`);
    if (before) q.push(`before=${before}`);
    try {
      const page = await api<AuditRow[]>('GET', `${props.path}?${q.join('&')}`);
      setRows(prev => (append && prev ? [...prev, ...page] : page));
      setMore(page.length === 50);
    } catch (e) {
      setError(errText(e));
    }
  };
  useEffect(() => {
    load(false);
  }, [props.path]);
  const enter = (e: KeyboardEvent) => e.key === 'Enter' && load(false);
  return (
    <>
      <div class="filterbar">
        <input type="search" placeholder="Action, for example provider" aria-label="Action" value={action} onInput={e => setAction(e.currentTarget.value)} onKeyDown={enter} />
        <input type="search" placeholder="Who, email or id" aria-label="Who" value={actor} onInput={e => setActor(e.currentTarget.value)} onKeyDown={enter} />
        <button onClick={() => load(false)}>Filter</button>
      </div>
      <div>{error ? <Msg text={error} bad /> : rows ? <ActivityTable rows={rows} withProject={props.withProject} /> : <Skeleton />}</div>
      <div class="more">{more ? <button onClick={() => load(true)}>Older</button> : <span />}</div>
    </>
  );
}

export function Activity(props: { pid: string }) {
  return (
    <ProjectShell pid={props.pid} tab="activity" sub="Who did what in this project. Kept for audit.">
      {(_p: Project) => <ActivityView path={`/projects/${props.pid}/activity`} withProject={false} />}
    </ProjectShell>
  );
}

export function AuditLog() {
  return (
    <OperatorPage active="audit" title="Audit log" sub="Every sensitive action across the platform.">
      <ActivityView path="/admin/audit" withProject />
    </OperatorPage>
  );
}

export { useLoad };
