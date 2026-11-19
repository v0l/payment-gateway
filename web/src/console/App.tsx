import { useEffect } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import { LocationProvider, useLocation } from 'preact-iso';
import { exec } from 'preact-iso/router';
import { api, getSession, setSession, useSession, type Me } from './api';
import { ConfirmEmailPage, InvitePage, LoginPage, ResetConfirmPage, ResetRequestPage } from './auth';
import { go } from './nav';
import { NewProject, OperatorTenants, SupportView, Admin, SharedAdmin, PlatformPlans, TreasuryPage, PayoutQueue } from './operator';
import { Home } from './home';
import { NewPayment, Payments } from './payments';
import { PaymentDetail } from './paydetail';
import { Balance, Payouts } from './balance';
import { CustomerDetail, Customers, Plans, PlatformBill, SubscriptionDetail, Subscriptions } from './billing';
import { ApiKeys, Webhooks } from './developers';
import { ProviderForm, Providers } from './providers';
import { Activity, AuditLog } from './activity';
import { Account } from './account';
import { Settings, Team } from './settings';

function Redirect(props: { to: string }) {
  useEffect(() => {
    go(props.to, true);
  }, [props.to]);
  return null;
}

function Landing() {
  const me = getSession().me!;
  return me.projects.length ? <Redirect to={`/p/${me.projects[0].id}/overview`} /> : <NewProject />;
}

type Params = Record<string, string>;
type Route = [string, (p: Params) => ComponentChildren, boolean?];

function projectPage(p: Params): ComponentChildren {
  const pid = p.pid;
  const sub = p.sub;
  switch (p.tab) {
    case 'overview':
      return <Home pid={pid} />;
    case 'payments':
      if (sub === 'new') return <NewPayment pid={pid} />;
      return sub ? <PaymentDetail pid={pid} id={sub} /> : <Payments pid={pid} />;
    case 'customers':
      return sub ? <CustomerDetail pid={pid} customerRef={sub} /> : <Customers pid={pid} />;
    case 'providers':
      return sub ? <ProviderForm pid={pid} id={sub} /> : <Providers pid={pid} />;
    case 'keys':
      return <ApiKeys pid={pid} />;
    case 'webhooks':
      return <Webhooks pid={pid} />;
    case 'plans':
      return <Plans pid={pid} />;
    case 'subscriptions':
      return sub ? <SubscriptionDetail pid={pid} id={sub} /> : <Subscriptions pid={pid} />;
    case 'billing':
      return <PlatformBill pid={pid} />;
    case 'balance':
      return <Balance pid={pid} />;
    case 'payouts':
      return <Payouts pid={pid} />;
    case 'team':
      return <Team pid={pid} />;
    case 'activity':
      return <Activity pid={pid} />;
    case 'settings':
      return <Settings pid={pid} />;
    default:
      return <Redirect to="/" />;
  }
}

const SIGNED_IN: Route[] = [
  ['/console', () => <Landing />],
  ['/console/new', () => <NewProject />],
  ['/console/account', () => <Account />],
  ['/console/admin', () => <Admin />, true],
  ['/console/shared', () => <SharedAdmin />, true],
  ['/console/tenants', () => <OperatorTenants />, true],
  ['/console/support/:id', p => <SupportView id={p.id} />, true],
  ['/console/payout-queue', () => <PayoutQueue />, true],
  ['/console/platform-plans', () => <PlatformPlans />, true],
  ['/console/treasury', () => <TreasuryPage />, true],
  ['/console/audit', () => <AuditLog />, true],
  ['/console/p/:pid/:tab/:sub?', projectPage],
];

const ANONYMOUS: Route[] = [
  ['/console/invite/:token', p => <InvitePage token={p.token} />],
  ['/console/reset/:token', p => <ResetConfirmPage token={p.token} />],
  ['/console/confirm-email/:token', p => <ConfirmEmailPage token={p.token} />],
  ['/console/reset', () => <ResetRequestPage />],
];

function Routes(props: { routes: Route[]; fallback: () => ComponentChildren; operator: boolean }) {
  const { path } = useLocation();
  for (const [pattern, render, operatorOnly] of props.routes) {
    const m = exec(path, pattern);
    if (m) {
      if (operatorOnly && !props.operator) return <Redirect to="/" />;
      return <>{render(m.params)}</>;
    }
  }
  return <>{props.fallback()}</>;
}

function Root() {
  const s = useSession();
  if (!s.ready) return <p class="muted pad">Loading</p>;
  if (!s.me) return <Routes routes={ANONYMOUS} fallback={() => <LoginPage />} operator={false} />;
  return <Routes routes={SIGNED_IN} fallback={() => <Redirect to="/" />} operator={s.me.is_operator} />;
}

export function App() {
  useEffect(() => {
    api<Me>('GET', '/me').then(setSession, () => setSession(null));
  }, []);
  return (
    <LocationProvider scope="/console">
      <Root />
    </LocationProvider>
  );
}
