import { useEffect, useRef, useState } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import { api, errText, setSession, type Me } from './api';
import { go, href } from './nav';
import { TextField } from './ui';

function AuthScreen(props: { children: ComponentChildren }) {
  useEffect(() => {
    document.title = 'Console';
  }, []);
  return (
    <main class="auth" id="main">
      <div class="brand">
        <span class="mark" aria-hidden="true" />
        Payment gateway
      </div>
      {props.children}
    </main>
  );
}

function AuthForm(props: { title: string; sub: string; label: string; onSubmit: () => Promise<void>; children: ComponentChildren; footer?: ComponentChildren }) {
  const [err, setErr] = useState('');
  const [busy, setBusy] = useState(false);
  const form = useRef<HTMLFormElement>(null);
  useEffect(() => {
    form.current?.querySelector('input')?.focus();
  }, []);
  return (
    <AuthScreen>
      <h1>{props.title}</h1>
      <p class="muted">{props.sub}</p>
      <form
        ref={form}
        class="card"
        onSubmit={async e => {
          e.preventDefault();
          setErr('');
          setBusy(true);
          try {
            await props.onSubmit();
          } catch (x) {
            setErr(errText(x));
          }
          setBusy(false);
        }}
      >
        {props.children}
        <div class="row">
          <button class="primary" type="submit" disabled={busy}>{props.label}</button>
        </div>
        <div class="err" role="alert">{err}</div>
      </form>
      {props.footer}
    </AuthScreen>
  );
}

export function LoginPage() {
  const [email, setEmail] = useState('');
  const [pw, setPw] = useState('');
  const [challenge, setChallenge] = useState<string | null>(null);
  const [recovery, setRecovery] = useState(false);
  if (challenge) return <SecondStep challenge={challenge} recovery={recovery} setRecovery={setRecovery} />;
  return (
    <AuthForm
      title="Payment gateway"
      sub="Sign in to your console. Accounts are by invite."
      label="Sign in"
      footer={<p class="muted"><a href={href('/reset')}>Forgot your password?</a></p>}
      onSubmit={async () => {
        const m = await api<Me>('POST', '/login', { email, password: pw });
        if (m.two_factor) {
          setChallenge(m.challenge ?? '');
          return;
        }
        setSession(m);
        go('/', true);
      }}
    >
      <TextField label="Email" type="email" autocomplete="username" required value={email} onInput={setEmail} />
      <TextField label="Password" type="password" autocomplete="current-password" required value={pw} onInput={setPw} />
    </AuthForm>
  );
}

function SecondStep(props: { challenge: string; recovery: boolean; setRecovery: (v: boolean) => void }) {
  const [code, setCode] = useState('');
  const { recovery } = props;
  return (
    <AuthForm
      title={recovery ? 'Recovery code' : 'Two factor code'}
      sub={recovery ? 'Each recovery code works once.' : 'Enter the 6 digit code from your authenticator app.'}
      label="Verify"
      footer={
        recovery ? null : (
          <p class="muted">
            <a
              href="#"
              onClick={e => {
                e.preventDefault();
                props.setRecovery(true);
              }}
            >
              Use a recovery code instead
            </a>
          </p>
        )
      }
      onSubmit={async () => {
        const m = await api<Me>('POST', '/login/2fa', recovery ? { challenge: props.challenge, recovery_code: code } : { challenge: props.challenge, code });
        setSession(m);
        go('/', true);
      }}
    >
      {recovery ? (
        <TextField label="Recovery code" autocomplete="off" required placeholder="xxxx-xxxx-xxxx-xxxx" value={code} onInput={setCode} />
      ) : (
        <TextField label="Code" inputMode="numeric" autocomplete="one-time-code" maxLength={8} required value={code} onInput={setCode} />
      )}
    </AuthForm>
  );
}

export function ResetRequestPage() {
  const [email, setEmail] = useState('');
  const [sent, setSent] = useState(false);
  if (sent) {
    return (
      <AuthScreen>
        <h1>Check your email</h1>
        <p class="muted">If an account exists for that address, a reset link is on its way. It works once and expires in 1 hour.</p>
        <p><a href={href('/')}>Back to sign in</a></p>
      </AuthScreen>
    );
  }
  return (
    <AuthForm
      title="Reset password"
      sub="Enter your email and we will send a reset link if an account exists."
      label="Send reset link"
      onSubmit={async () => {
        await api('POST', '/reset', { email });
        setSent(true);
      }}
    >
      <TextField label="Email" type="email" autocomplete="username" required value={email} onInput={setEmail} />
    </AuthForm>
  );
}

export function ResetConfirmPage(props: { token: string }) {
  const [pw, setPw] = useState('');
  return (
    <AuthForm
      title="Choose a new password"
      sub="You will be signed out everywhere and can sign in with the new password."
      label="Set password"
      onSubmit={async () => {
        await api('POST', '/reset/confirm', { token: props.token, password: pw });
        location.replace('/console/');
      }}
    >
      <TextField label="New password (10+ characters)" type="password" autocomplete="new-password" required minLength={10} value={pw} onInput={setPw} />
    </AuthForm>
  );
}

export function ConfirmEmailPage(props: { token: string }) {
  const [state, setState] = useState<{ ok: boolean; email?: string } | null>(null);
  useEffect(() => {
    api<{ email: string }>('POST', '/email/confirm', { token: props.token }).then(
      r => setState({ ok: true, email: r.email }),
      () => setState({ ok: false }),
    );
  }, []);
  if (!state) {
    return (
      <AuthScreen>
        <h1>Confirming email</h1>
      </AuthScreen>
    );
  }
  if (!state.ok) {
    return (
      <AuthScreen>
        <h1>Link not valid</h1>
        <p class="muted">It may have expired or already been used.</p>
      </AuthScreen>
    );
  }
  return (
    <AuthScreen>
      <h1>Email confirmed</h1>
      <p class="muted">{`Your sign in email is now ${state.email}.`}</p>
      <p><a href="/console/">Continue</a></p>
    </AuthScreen>
  );
}

export function InvitePage(props: { token: string }) {
  const [info, setInfo] = useState<{ email: string; project?: string } | null | false>(null);
  const [pw, setPw] = useState('');
  useEffect(() => {
    api<{ email: string; project?: string }>('GET', `/invite/${encodeURIComponent(props.token)}`).then(setInfo, () => setInfo(false));
  }, []);
  if (info === null) return <AuthScreen>{null}</AuthScreen>;
  if (info === false) {
    return (
      <AuthScreen>
        <h1>Invite not valid</h1>
        <p class="muted">It may have expired or already been used. Ask for a new one.</p>
      </AuthScreen>
    );
  }
  return (
    <AuthForm
      title="Accept invite"
      sub={`Create a password for ${info.email}${info.project ? ` to join ${info.project}` : ''}.`}
      label="Create account"
      onSubmit={async () => {
        const m = await api<Me>('POST', '/signup', { token: props.token, password: pw });
        setSession(m);
        location.replace('/console/');
      }}
    >
      <TextField label="Password (10+ characters)" type="password" autocomplete="new-password" required minLength={10} value={pw} onInput={setPw} />
    </AuthForm>
  );
}
