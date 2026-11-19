import { useState } from 'preact/hooks';
import { api, errText, getSession, setSession, useLoad, type Me } from './api';
import { confirmDialog } from './dialogs';
import { OperatorPage } from './shell';
import { Check, Msg, Section, TextField, useNote } from './ui';

function RecoveryList(props: { codes: string[] }) {
  return (
    <div class="secret">
      <b>Recovery codes (shown once, store them somewhere safe)</b>
      {props.codes.map(c => (
        <span class="mono" key={c}>{c}</span>
      ))}
    </div>
  );
}

function PasswordCard() {
  const [cur, setCur] = useState('');
  const [nw, setNw] = useState('');
  const [note, out] = useNote();
  return (
    <div class="card form">
      <TextField label="Current password" type="password" autocomplete="current-password" value={cur} onInput={setCur} />
      <TextField label="New password, 10 or more characters" type="password" autocomplete="new-password" value={nw} onInput={setNw} />
      <div class="row">
        <button
          class="primary"
          onClick={async () => {
            try {
              await api('POST', '/password', { current: cur, new: nw });
              setCur('');
              setNw('');
              out('Password changed. Other sessions were signed out.');
            } catch (e) {
              out(errText(e), true);
            }
          }}
        >
          Change password
        </button>
      </div>
      {note}
    </div>
  );
}

function EmailCard() {
  const [pw, setPw] = useState('');
  const [email, setEmail] = useState('');
  const [note, out] = useNote();
  return (
    <div class="card form">
      <TextField label="New email" type="email" autocomplete="off" value={email} onInput={setEmail} />
      <TextField label="Current password" type="password" autocomplete="current-password" value={pw} onInput={setPw} />
      <div class="row">
        <button
          class="primary"
          onClick={async () => {
            try {
              const r = await api<{ email: string }>('POST', '/email', { password: pw, new_email: email });
              setPw('');
              out(`Confirmation sent to ${r.email}. The change applies once you open the link.`);
            } catch (e) {
              out(errText(e), true);
            }
          }}
        >
          Send confirmation
        </button>
      </div>
      {note}
    </div>
  );
}

function NotifyCard() {
  const me = getSession().me!;
  const [on, setOn] = useState(!!me.notify);
  const [note, out] = useNote();
  return (
    <div class="card">
      <Check
        checked={on}
        onChange={async v => {
          setOn(v);
          try {
            const r = await api<{ notify: boolean }>('POST', '/notifications', { enabled: v });
            me.notify = r.notify;
            out(r.notify ? 'Notifications on.' : 'Notifications off.');
          } catch (e) {
            setOn(!v);
            out(errText(e), true);
          }
        }}
      >
        <span>Email me about failed webhooks, failing payment methods and failed refunds on projects I own</span>
      </Check>
      {note}
    </div>
  );
}

interface TwoFactorState {
  enabled: boolean;
  recovery_codes_remaining: number;
  operator_grace?: boolean;
}

function TwoFactorOn(props: { st: TwoFactorState; refresh: () => void }) {
  const me = getSession().me!;
  const [pw, setPw] = useState('');
  const [code, setCode] = useState('');
  const [note, out, outNode] = useNote();
  const body = () => ({ password: pw, code });
  return (
    <>
      <p class="ok-msg">{`On. ${props.st.recovery_codes_remaining} recovery codes left.`}</p>
      <TextField label="Password" type="password" autocomplete="current-password" value={pw} onInput={setPw} />
      <TextField label="Current code" inputMode="numeric" autocomplete="one-time-code" maxLength={8} value={code} onInput={setCode} />
      <div class="row">
        <button
          onClick={async () => {
            try {
              const r = await api<{ recovery_codes: string[] }>('POST', '/2fa/recovery-codes', body());
              outNode(<RecoveryList codes={r.recovery_codes} />);
            } catch (e) {
              out(errText(e), true);
            }
          }}
        >
          New recovery codes
        </button>
        <button
          onClick={async () => {
            const r = await confirmDialog({ title: 'Turn off two factor', body: 'Signing in will only need your password.', keep: 'Keep it on', confirm: 'Turn off', danger: true });
            if (!r) return;
            try {
              await api('POST', '/2fa/disable', body());
              me.totp_enabled = false;
              props.refresh();
            } catch (e) {
              out(errText(e), true);
            }
          }}
        >
          Turn off two factor
        </button>
      </div>
      {note}
    </>
  );
}

function TwoFactorOff(props: { st: TwoFactorState; refresh: () => void }) {
  const me = getSession().me!;
  const [setup, setSetup] = useState<{ qr_svg: string; secret: string } | null>(null);
  const [code, setCode] = useState('');
  const [done, setDone] = useState<string[] | null>(null);
  const [note, out] = useNote();
  if (done) {
    return (
      <>
        <p class="ok-msg">Two factor is on.</p>
        <RecoveryList codes={done} />
      </>
    );
  }
  if (setup) {
    return (
      <>
        <p>Scan this QR code with your authenticator app, or enter the key by hand.</p>
        <img src={`data:image/svg+xml;utf8,${encodeURIComponent(setup.qr_svg)}`} alt="Authenticator QR code" width={220} height={220} />
        <div class="secret">
          <b>Key</b>
          <span class="mono">{setup.secret}</span>
        </div>
        <TextField label="Code from the app" inputMode="numeric" autocomplete="one-time-code" maxLength={8} value={code} onInput={setCode} />
        <button
          class="primary"
          onClick={async () => {
            try {
              const a = await api<{ recovery_codes: string[] }>('POST', '/2fa/activate', { code });
              me.totp_enabled = true;
              setDone(a.recovery_codes);
            } catch (e) {
              out(errText(e), true);
            }
          }}
        >
          Activate
        </button>
        {note}
      </>
    );
  }
  return (
    <>
      <p class="muted">{`Off. Accounts that refund payments or rotate keys should turn this on.${props.st.operator_grace ? ' Operators will be required to enable it.' : ''}`}</p>
      <button
        class="primary"
        onClick={async () => {
          try {
            setSetup(await api<{ qr_svg: string; secret: string }>('POST', '/2fa/setup', {}));
          } catch (e) {
            out(errText(e), true);
          }
        }}
      >
        Set up authenticator app
      </button>
      {note}
    </>
  );
}

function TwoFactorCard() {
  const { data: st, error, reload } = useLoad(() => api<TwoFactorState>('GET', '/2fa'), []);
  return (
    <div class="card">
      {error ? <Msg text={error} bad /> : st ? st.enabled ? <TwoFactorOn st={st} refresh={reload} /> : <TwoFactorOff st={st} refresh={reload} /> : null}
    </div>
  );
}

export function Account() {
  const me: Me = getSession().me!;
  void setSession;
  return (
    <OperatorPage active="account" title="Account" sub={me.email}>
      <Section title="Password"><PasswordCard /></Section>
      {me.mail_enabled ? (
        <>
          <Section title="Email"><EmailCard /></Section>
          <Section title="Notifications"><NotifyCard /></Section>
        </>
      ) : null}
      <Section title="Two factor authentication"><TwoFactorCard /></Section>
    </OperatorPage>
  );
}
