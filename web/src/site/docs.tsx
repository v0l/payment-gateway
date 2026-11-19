import { render } from 'preact';
import { useMemo, useState } from 'preact/hooks';
import './reference.css';

interface Param {
  name: string;
  in: string;
  required?: boolean;
}

interface Operation {
  summary: string;
  'x-required-scope'?: string;
  parameters?: Param[];
}

interface Spec {
  info: { title: string; version: string; description: string };
  paths: Record<string, Record<string, Operation>>;
}

interface Row {
  method: string;
  full: string;
  op: Operation;
  text: string;
}

function Params(props: { op: Operation }) {
  const rows = props.op.parameters ?? [];
  if (!rows.length) return null;
  return (
    <table>
      <thead>
        <tr>
          {['Name', 'In', 'Required'].map(h => (
            <th scope="col" key={h}>{h}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map(p => (
          <tr key={p.name + p.in}>
            <td><code>{p.name}</code></td>
            <td>{p.in}</td>
            <td>{p.required ? 'required' : 'optional'}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Reference(props: { spec: Spec }) {
  const { spec } = props;
  const [query, setQuery] = useState('');
  const rows = useMemo<Row[]>(
    () =>
      Object.keys(spec.paths).flatMap(path =>
        Object.keys(spec.paths[path]).map(method => {
          const op = spec.paths[path][method];
          const full = `/v1${path}`;
          return { method, full, op, text: `${method} ${full} ${op.summary || ''}`.toLowerCase() };
        }),
      ),
    [spec],
  );
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = rows.filter(r => terms.every(t => r.text.includes(t)));
  return (
    <>
      <div class="search">
        <label class="sr-only" for="ref-search">Search the API reference</label>
        <input id="ref-search" type="search" placeholder="Search endpoints, for example refund or subscription" autocomplete="off" value={query} onInput={e => setQuery(e.currentTarget.value)} />
        <p class="count" id="ref-count" role="status">{terms.length ? `${shown.length} of ${rows.length} endpoints` : ''}</p>
      </div>
      <article class="doc" id="root">
        <h1>{spec.info.title}</h1>
        <p>{`Version ${spec.info.version}. ${spec.info.description}`}</p>
        <p>Authenticate with an Authorization: Bearer header carrying a project API key. List endpoints accept limit (1 to 100) and cursor, and return the next cursor in the X-Next-Cursor header.</p>
        {rows.map(r => (
          <section class="op" aria-label={`${r.method.toUpperCase()} ${r.full}`} hidden={!shown.includes(r)} key={r.method + r.full}>
            <div class="line">
              <span class={`method ${r.method}`}>{r.method.toUpperCase()}</span>
              <span class="path">{r.full}</span>
            </div>
            <p class="summary">{r.op.summary}</p>
            {r.op['x-required-scope'] ? <p class="scope">{`Scope: ${r.op['x-required-scope']}`}</p> : null}
            <Params op={r.op} />
          </section>
        ))}
        <p><a href="/v1/openapi.json">OpenAPI document (JSON)</a></p>
      </article>
    </>
  );
}

const host = document.getElementById('reference')!;
fetch('/v1/openapi.json')
  .then(r => r.json() as Promise<Spec>)
  .then(spec => {
    host.replaceChildren();
    render(<Reference spec={spec} />, host);
  })
  .catch((e: unknown) => {
    const root = document.getElementById('root');
    if (root) root.textContent = `Could not load the API description: ${e instanceof Error ? e.message : String(e)}`;
  });
