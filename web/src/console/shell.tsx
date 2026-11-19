import { useEffect, useRef, useState } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import { api, getSession, setSession, useLoad, useSession, errText, type Project } from './api';
import { fmtAmount, statusWord } from './format';
import { href, go, enc } from './nav';
import { PageHead } from './ui';

export const TAB_GROUPS: [string, string, [string, string][]][] = [
  ['overview', 'Home', [['overview', 'Home']]],
  ['payments', 'Payments', [['payments', 'Payments']]],
  ['customers', 'Customers', [['customers', 'Customers']]],
  ['billing', 'Billing', [['subscriptions', 'Subscriptions'], ['plans', 'Plans'], ['billing', 'Platform bill']]],
  ['balance', 'Balance', [['balance', 'Balance'], ['payouts', 'Payouts']]],
  ['developers', 'Developers', [['providers', 'Payment methods'], ['keys', 'API keys'], ['webhooks', 'Webhooks']]],
  ['settings', 'Settings', [['settings', 'Settings'], ['team', 'Team'], ['activity', 'Activity']]],
];

const OPERATOR_TABS: [string, string][] = [
  ['tenants', 'Projects'],
  ['admin', 'Accounts'],
  ['shared', 'Shared providers'],
  ['platform-plans', 'Platform plans'],
  ['treasury', 'Treasury'],
  ['payout-queue', 'Payout queue'],
  ['audit', 'Audit'],
];

const TAB_TITLES: Record<string, string> = {
  overview: 'Home',
  payments: 'Payments',
  customers: 'Customers',
  subscriptions: 'Subscriptions',
  plans: 'Plans',
  billing: 'Platform bill',
  balance: 'Balance',
  payouts: 'Payouts',
  providers: 'Payment methods',
  keys: 'API keys',
  webhooks: 'Webhooks',
  settings: 'Settings',
  team: 'Team',
  activity: 'Activity',
};

const attentionCount: Record<string, number> = {};
const attentionListeners = new Set<() => void>();

export function setAttention(pid: string | number, n: number): void {
  attentionCount[String(pid)] = n;
  attentionListeners.forEach(l => l());
}

function useAttention(pid: string | null): number | undefined {
  const [, tick] = useState(0);
  useEffect(() => {
    const l = () => tick(n => n + 1);
    attentionListeners.add(l);
    if (pid && attentionCount[pid] === undefined) {
      api<{ open: number }>('GET', `/projects/${pid}/attention`).then(r => setAttention(pid, r.open), () => {});
    }
    return () => {
      attentionListeners.delete(l);
    };
  }, [pid]);
  return pid ? attentionCount[pid] : undefined;
}

function groupOf(tab: string) {
  return TAB_GROUPS.find(g => g[2].some(s => s[0] === tab)) ?? null;
}

type MenuItem =
  | { head: string }
  | { sep: true }
  | { href: string; on?: boolean; kids?: ComponentChildren; text?: string }
  | { text: string; onclick: () => void };

function MenuButton(props: { label: ComponentChildren; items: MenuItem[]; cls?: string; ariaLabel?: string }) {
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const btn = useRef<HTMLButtonElement>(null);
  const close = () => setOpen(false);
  useEffect(() => {
    if (!open) return;
    const outside = (e: MouseEvent) => {
      if (!wrap.current?.contains(e.target as Node)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        close();
        btn.current?.focus();
      }
      const links = Array.from(list.current?.querySelectorAll<HTMLElement>('a,button') ?? []);
      const i = links.indexOf(document.activeElement as HTMLElement);
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        (links[i + 1] ?? links[0])?.focus();
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        (links[i - 1] ?? links[links.length - 1])?.focus();
      }
    };
    document.addEventListener('click', outside, true);
    document.addEventListener('keydown', onKey);
    list.current?.querySelector<HTMLElement>('a,button')?.focus();
    return () => {
      document.removeEventListener('click', outside, true);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);
  return (
    <div class="menu-wrap" ref={wrap}>
      <button
        ref={btn}
        class={`bar-btn${props.cls ? ` ${props.cls}` : ''}`}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open ? 'true' : 'false'}
        aria-label={props.ariaLabel}
        onClick={() => setOpen(o => !o)}
      >
        {props.label}
      </button>
      <div class="pop" role="menu" hidden={!open} ref={list}>
        {props.items.map((it, i) => {
          if ('sep' in it) return <div class="pop-sep" role="separator" key={i} />;
          if ('head' in it) return <div class="pop-head" key={i}>{it.head}</div>;
          if ('href' in it) {
            return (
              <a href={it.href} role="menuitem" class={it.on ? 'on' : ''} aria-current={it.on ? 'true' : undefined} onClick={close} key={i}>
                {it.kids ?? it.text}
              </a>
            );
          }
          return (
            <button
              type="button"
              role="menuitem"
              key={i}
              onClick={() => {
                close();
                it.onclick();
              }}
            >
              {it.text}
            </button>
          );
        })}
      </div>
    </div>
  );
}

interface Hit {
  label: string;
  sub: string;
  path: string;
}

function SearchBox(props: { projectId: string | null }) {
  const { projectId } = props;
  const s = useSession();
  const me = s.me!;
  const input = useRef<HTMLInputElement>(null);
  const resultsRef = useRef<HTMLDivElement>(null);
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<Hit[] | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const hide = () => setHits(null);
  const open = (path: string) => {
    hide();
    setQ('');
    input.current?.blur();
    go(path);
  };

  const run = async (text: string) => {
    if (text.length < 2) return hide();
    const jobs: Promise<Hit[]>[] = [];
    if (projectId) {
      jobs.push(
        api<{ payments: { id: string; description?: string; reference: string; amount: number; currency: string; status: string }[] }>(
          'GET',
          `/projects/${projectId}/payments?q=${enc(text)}`,
        ).then(
          r => r.payments.slice(0, 6).map(p => ({ label: p.description || p.reference, sub: `${fmtAmount(p.amount, p.currency)}, ${statusWord(p.status)}, ${p.id.slice(0, 10)}`, path: `/p/${projectId}/payments/${p.id}` })),
          () => [],
        ),
      );
      jobs.push(
        api<{ customers: { customer_ref: string; payments: number }[] }>('GET', `/projects/${projectId}/customers?q=${enc(text)}`).then(
          r => r.customers.slice(0, 4).map(c => ({ label: c.customer_ref, sub: `${c.payments} payments`, path: `/p/${projectId}/customers/${enc(c.customer_ref)}` })),
          () => [],
        ),
      );
    }
    if (me.is_operator && text.length >= 6) {
      jobs.push(
        api<{ hits: { reference: string; tenant_id: number; matched: string; payment_id: string }[] }>('GET', `/operator/search?q=${enc(text)}`).then(
          r => r.hits.map(h => ({ label: h.reference, sub: `Project ${h.tenant_id}, ${h.matched} match`, path: `/p/${h.tenant_id}/payments/${h.payment_id}` })),
          () => [],
        ),
      );
    }
    const lists = await Promise.all(jobs);
    setHits(lists.flat());
  };

  useEffect(() => {
    const onShortcut = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      const typing = t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable);
      if ((e.key === 'k' && (e.ctrlKey || e.metaKey)) || (e.key === '/' && !typing)) {
        if (input.current) {
          e.preventDefault();
          input.current.focus();
          input.current.select();
        }
      }
    };
    document.addEventListener('keydown', onShortcut);
    return () => document.removeEventListener('keydown', onShortcut);
  }, []);

  const options = () => Array.from(resultsRef.current?.querySelectorAll<HTMLElement>('a') ?? []);

  return (
    <div class="search" role="search">
      <input
        ref={input}
        type="search"
        class="search-input"
        placeholder={projectId ? 'Find a payment, customer or reference' : 'Search'}
        aria-label="Search payments"
        autocomplete="off"
        value={q}
        onInput={e => {
          const v = e.currentTarget.value;
          setQ(v);
          if (timer.current) clearTimeout(timer.current);
          timer.current = setTimeout(() => run(v.trim()), 180);
        }}
        onKeyDown={e => {
          const opts = options();
          if (e.key === 'Escape') {
            hide();
            input.current?.blur();
          }
          if (e.key === 'ArrowDown' && opts.length) {
            e.preventDefault();
            opts[0].focus();
          }
          if (e.key === 'Enter' && opts.length) {
            e.preventDefault();
            const first = hits?.[0];
            if (first) open(first.path);
          }
        }}
        onBlur={() => setTimeout(() => {
          if (!resultsRef.current?.contains(document.activeElement)) hide();
        }, 150)}
      />
      <kbd>Ctrl K</kbd>
      <div
        class="search-results"
        role="listbox"
        hidden={hits === null}
        ref={resultsRef}
        onKeyDown={e => {
          const opts = options();
          const i = opts.indexOf(document.activeElement as HTMLElement);
          if (e.key === 'ArrowDown') {
            e.preventDefault();
            (opts[i + 1] ?? opts[0]).focus();
          }
          if (e.key === 'ArrowUp') {
            e.preventDefault();
            if (i <= 0) input.current?.focus();
            else opts[i - 1].focus();
          }
          if (e.key === 'Escape') {
            hide();
            input.current?.focus();
          }
        }}
      >
        {hits === null
          ? null
          : hits.length
            ? hits.map((x, i) => (
                <a
                  href={href(x.path)}
                  role="option"
                  id={`sr-${i}`}
                  key={i}
                  onClick={e => {
                    e.preventDefault();
                    open(x.path);
                  }}
                >
                  <span>{x.label}</span>
                  <small>{x.sub}</small>
                </a>
              ))
            : <p class="muted">{`Nothing matches \u201c${q.trim()}\u201d.`}</p>}
      </div>
    </div>
  );
}

function TopBar(props: { active: string; projectId: string | null; project: Project | null }) {
  const { active, projectId, project } = props;
  const me = getSession().me!;
  const current = me.projects.find(p => String(p.id) === String(projectId));
  const projectItems: MenuItem[] = [
    { head: 'Projects' },
    ...me.projects.map<MenuItem>(p => ({
      href: href(`/p/${p.id}/overview`),
      on: !!current && p.id === current.id,
      kids: (
        <>
          <span>{p.name}</span>
          <small>{p.role}</small>
        </>
      ),
    })),
    { sep: true },
    { href: href('/new'), text: 'New project' },
  ];
  const operatorOn = OPERATOR_TABS.some(t => t[0] === active);
  return (
    <header class="top">
      <div class="top-in">
        <MenuButton
          cls="switch"
          ariaLabel={`Switch project, current ${current ? current.name : 'none'}`}
          label={
            <>
              <span class="mark" aria-hidden="true" />
              <b>{current ? current.name : 'Payment gateway'}</b>
              <span class="chev" aria-hidden="true" />
            </>
          }
          items={projectItems}
        />
        {projectId ? (project && !project.trusted ? <span class="mode test">Test mode</span> : <span class="mode">Live</span>) : null}
        <SearchBox projectId={projectId} />
        {me.is_operator ? (
          <MenuButton
            cls={operatorOn ? 'on' : ''}
            ariaLabel="Operator menu"
            label={
              <>
                <span class="hide-s">Operator</span>
                <span class="chev" aria-hidden="true" />
              </>
            }
            items={[{ head: 'Platform' }, ...OPERATOR_TABS.map<MenuItem>(t => ({ href: href(`/${t[0]}`), text: t[1], on: t[0] === active }))]}
          />
        ) : null}
        <MenuButton
          cls="acct"
          ariaLabel={`Account menu for ${me.email}`}
          label={<span class="avatar">{me.email.slice(0, 2).toUpperCase()}</span>}
          items={[
            { head: me.email },
            { href: href('/account'), text: 'Account and security' },
            {
              text: 'Sign out',
              onclick: () => {
                api('POST', '/logout', {}).then(() => setSession(null));
              },
            },
          ]}
        />
      </div>
    </header>
  );
}

function TabBar(props: { active: string; projectId: string }) {
  const { active, projectId } = props;
  const group = groupOf(active);
  const count = useAttention(projectId);
  return (
    <div class="tabrow">
      <nav class="tabs" aria-label="Project">
        {TAB_GROUPS.map(g => {
          const on = !!group && g[0] === group[0];
          return (
            <a href={href(`/p/${projectId}/${g[2][0][0]}`)} class={on ? 'on' : ''} aria-current={on ? 'page' : undefined} key={g[0]}>
              {g[1]}
              {g[0] === 'overview' && count ? <span class="count" aria-label={`${count} open items`}>{String(count)}</span> : null}
            </a>
          );
        })}
      </nav>
      {group && group[2].length > 1 ? (
        <nav class="subtabs" aria-label={group[1]}>
          {group[2].map(s => (
            <a href={href(`/p/${projectId}/${s[0]}`)} class={s[0] === active ? 'on' : ''} aria-current={s[0] === active ? 'page' : undefined} key={s[0]}>{s[1]}</a>
          ))}
        </nav>
      ) : null}
    </div>
  );
}

function TreasuryBanner() {
  const { data } = useLoad(() => api<{ alerts?: string[] }>('GET', '/admin/treasury/banner'), []);
  if (!data?.alerts?.length) return null;
  return (
    <div class="banner bad" role="alert">
      <div>
        <b>Treasury alert</b>
        {`${data.alerts.join('. ')}. `}
        <a href={href('/treasury')}>Open treasury</a>
      </div>
    </div>
  );
}

export function Layout(props: { active: string; projectId: string | null; project?: Project | null; wide?: boolean; children: ComponentChildren }) {
  const me = getSession().me!;
  const main = useRef<HTMLElement>(null);
  useEffect(() => {
    window.scrollTo(0, 0);
  }, []);
  return (
    <>
      <a
        class="skip"
        href="#main"
        onClick={e => {
          e.preventDefault();
          main.current?.focus();
        }}
      >
        Skip to content
      </a>
      <TopBar active={props.active} projectId={props.projectId} project={props.project ?? null} />
      {props.projectId ? <TabBar active={props.active} projectId={props.projectId} /> : null}
      <main id="main" class={props.wide ? 'wide' : ''} tabIndex={-1} ref={main}>
        <div class="banners">{me.is_operator ? <TreasuryBanner /> : null}</div>
        {props.children}
      </main>
    </>
  );
}

export function ProjectShell(props: {
  pid: string;
  tab: string;
  title?: string;
  sub?: ComponentChildren;
  actions?: (p: Project) => ComponentChildren;
  bare?: boolean;
  wide?: boolean;
  children: (p: Project) => ComponentChildren;
}) {
  const { pid, tab } = props;
  const { data: p, error } = useLoad(() => api<Project>('GET', `/projects/${pid}`), [pid]);
  useEffect(() => {
    if (p) document.title = `${TAB_TITLES[tab] ?? 'Console'}, ${p.display_name || p.name}`;
  }, [p, tab]);
  if (error) {
    return (
      <Layout active="" projectId={pid}>
        <div class="page">
          <div class="err" role="alert">{error}</div>
        </div>
      </Layout>
    );
  }
  return (
    <Layout active={tab} projectId={pid} project={p} wide={props.wide}>
      <div class="page">
        {p ? (
          <>
            {!props.bare ? <PageHead title={props.title ?? TAB_TITLES[tab] ?? tab} sub={props.sub} actions={props.actions ? props.actions(p) : undefined} /> : null}
            {props.children(p)}
          </>
        ) : null}
      </div>
    </Layout>
  );
}

export function OperatorPage(props: { active: string; title: string; sub?: ComponentChildren; children: ComponentChildren }) {
  useEffect(() => {
    document.title = `${props.title}, operator`;
  }, [props.title]);
  return (
    <Layout active={props.active} projectId={null}>
      <div class="page">
        <PageHead title={props.title} sub={props.sub} />
        {props.children}
      </div>
    </Layout>
  );
}

export function errorBox(e: unknown) {
  return <div class="err" role="alert">{errText(e)}</div>;
}
