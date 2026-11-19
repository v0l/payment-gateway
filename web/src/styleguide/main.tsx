import { render } from 'preact';
import type { ComponentChildren } from 'preact';
import '../tokens.css';
import '../shared/shell.css';
import '../console/console.css';
import '../components.css';
import { Pill } from '../console/ui';

const TOKENS = ['bg', 'card', 'code', 'text', 'muted', 'line', 'line-strong', 'accent', 'ok', 'warn', 'bad', 'info'];
const PAYMENT = ['pending', 'paid', 'expired', 'cancelled', 'failed', 'disputed', 'partially_refunded', 'refunded'];
const ATTEMPT = ['pending', 'paid', 'expired', 'cancelled', 'failed'];
const REFUND = ['pending', 'completed', 'refund_failed'];
const WITHDRAWAL = ['requested', 'processing', 'completed', 'failed', 'cancelled'];
const WEBHOOK = ['delivered', 'waiting', 'retrying', 'failed'];
const QR_CELLS = '111111101100010111010001101110101011101110101110101000100101111110111111100000000010101010111011101001001';

function Section(props: { title: string; children: ComponentChildren }) {
  return (
    <section>
      <h3>{props.title}</h3>
      {props.children}
    </section>
  );
}

function Swatches() {
  return (
    <div class="swatches">
      {TOKENS.map(t => (
        <div class="swatch" key={t}>
          <i class={`sw-${t}`} />
          <span>{`--${t}`}</span>
        </div>
      ))}
    </div>
  );
}

function Buttons() {
  return (
    <div class="row">
      <button class="primary">Save changes</button>
      <button>Cancel</button>
      <button class="danger">Delete project</button>
      <button class="primary" disabled>Saving</button>
    </div>
  );
}

function Inputs() {
  return (
    <div>
      <div class="field">
        <label>Project name</label>
        <input placeholder="Coffee shop" />
        <div class="hint">Shown to your customers on the pay page.</div>
      </div>
      <div class="field invalid">
        <label>Webhook URL</label>
        <input value="http://localhost/hook" aria-invalid="true" />
        <div class="hint">Use an https URL that is reachable from the internet.</div>
      </div>
      <div class="field">
        <label>Currency</label>
        <select>
          <option>EUR</option>
          <option>USD</option>
        </select>
      </div>
    </div>
  );
}

function Tables() {
  const rows = [['pay_8f3k2', 'paid', '12.50', 'EUR'], ['pay_1d9ac', 'pending', '4.00', 'EUR'], ['pay_77hq0', 'expired', '99.00', 'EUR']];
  return (
    <div>
      <table>
        <thead>
          <tr>
            {['Payment', 'Status', 'Amount'].map(h => (
              <th key={h}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map(r => (
            <tr class="click" key={r[0]}>
              <td class="mono">{r[0]}</td>
              <td><Pill status={r[1]} word={r[1]} /></td>
              <td class="money">{`${r[2]} ${r[3]}`}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div class="pager">
        <span class="muted">1 to 3 of 3</span>
        <button disabled>Previous</button>
        <button disabled>Next</button>
      </div>
    </div>
  );
}

function Badges() {
  const group = (name: string, list: string[]) => (
    <div class="row">
      <span class="muted">{name}</span>
      {list.map(s => (
        <span class={`pill ${s}`} key={s}>{s}</span>
      ))}
    </div>
  );
  return (
    <div>
      {group('Payment', PAYMENT)}
      {group('Attempt', ATTEMPT)}
      {group('Refund', REFUND)}
      {group('Withdrawal', WITHDRAWAL)}
      {group('Webhook', WEBHOOK)}
    </div>
  );
}

function Money() {
  return (
    <dl class="kv">
      <dt>Fiat</dt>
      <dd class="money">{'\u20ac1,234.50'}</dd>
      <dt>Sats</dt>
      <dd class="money">250,000 sats</dd>
      <dt>Msats</dt>
      <dd class="money">250,000,000 msats</dd>
      <dt>Fee</dt>
      <dd class="money" data-minor="25">{'\u20ac0.25'}</dd>
    </dl>
  );
}

function Copyable() {
  return (
    <div>
      <div class="copyable">
        <code>pk_live_4f9c0a77d2e1</code>
        <button>Copy</button>
      </div>
      <div class="secret">
        <b>Webhook secret (shown once, copy it now)</b>
        <span class="mono">whsec_9b1d52c0e7a4f3318</span>
      </div>
    </div>
  );
}

function Qr() {
  const cells: number[] = [];
  for (let i = 0; i < 81; i++) if (QR_CELLS[i % QR_CELLS.length] === '1') cells.push(i);
  return (
    <div class="qr-block">
      <svg viewBox="0 0 9 9" role="img" aria-label="Example QR code">
        {cells.map(i => (
          <rect x={i % 9} y={Math.floor(i / 9)} width={1} height={1} fill="#000000" key={i} />
        ))}
      </svg>
    </div>
  );
}

function States() {
  return (
    <div>
      <div class="empty">
        <b>No payments yet</b>
        <p class="muted">Create a payment from your app or use the console to send a test payment.</p>
        <button class="primary">Create test payment</button>
      </div>
      <div>
        <div class="skeleton" />
        <div class="skeleton" />
      </div>
      <p class="err" role="alert">Could not load payments. Check your connection and try again.</p>
    </div>
  );
}

function Banners() {
  return (
    <div>
      <div class="banner info"><div><b>Test limits apply</b>Payments above 100 EUR need operator approval.</div></div>
      <div class="banner ok"><div><b>Webhook secret rotated</b>The old secret stops working in 24 hours.</div></div>
      <div class="banner warn"><div><b>Two factor is off</b>Turn it on to protect withdrawals.</div></div>
      <div class="banner bad"><div><b>Provider rejected the key</b>Check the API key for Stripe and save again.</div></div>
      <p>
        <span class="toast" role="status">
          Copied
          <button>Undo</button>
        </span>
      </p>
    </div>
  );
}

function Dialog() {
  return (
    <div class="dialog-scrim">
      <div class="dialog" role="alertdialog" aria-modal="true">
        <h4>Delete the Stripe provider?</h4>
        <p class="muted">Payments in progress keep their current attempt. New payments cannot use Stripe.</p>
        <div class="row">
          <button>Keep provider</button>
          <button class="danger">Delete provider</button>
        </div>
      </div>
    </div>
  );
}

function Recovery() {
  return (
    <div>
      <div class="banner warn"><div><b>Save these recovery codes</b>Each code works once. They are not shown again.</div></div>
      <ul class="codes">
        {['4f7k-9q2m', 'x8d3-a1vz', 'm2pw-6t0c', 'h9ye-r5nb', 'z3ju-8k4s', 'c7fq-1w6d'].map(c => (
          <li key={c}>{c}</li>
        ))}
      </ul>
      <div class="row">
        <button>Copy codes</button>
        <button>Download</button>
        <button class="primary">I saved them</button>
      </div>
    </div>
  );
}

function Panel(props: { theme: 'light' | 'dark' }) {
  return (
    <div class="theme-panel" data-theme={props.theme}>
      <h2>{props.theme === 'light' ? 'Light' : 'Dark'}</h2>
      <p class="sub">Every component the console and pay page use.</p>
      <Section title="Colour"><Swatches /></Section>
      <Section title="Type">
        <div>
          <div class="stat">
            <div class="n">Balance 1,234.50</div>
            <div class="l">Small label</div>
          </div>
          <p>Body text at 14px. Plain words, short sentences.</p>
          <p class="muted">Muted text for hints and metadata.</p>
          <p class="mono">pay_8f3k2 monospace for ids</p>
        </div>
      </Section>
      <Section title="Buttons"><Buttons /></Section>
      <Section title="Inputs and validation"><Inputs /></Section>
      <Section title="Tables and paging"><div class="card"><Tables /></div></Section>
      <Section title="Status pills"><Badges /></Section>
      <Section title="Money"><Money /></Section>
      <Section title="Copyable ids and secrets"><Copyable /></Section>
      <Section title="QR block"><Qr /></Section>
      <Section title="Empty, loading and error"><States /></Section>
      <Section title="Banners and toasts"><Banners /></Section>
      <Section title="Confirm dialog"><Dialog /></Section>
      <Section title="2FA and recovery codes"><Recovery /></Section>
    </div>
  );
}

function Styleguide() {
  const force = new URLSearchParams(location.search).get('theme');
  const themes: ('light' | 'dark')[] = force === 'light' || force === 'dark' ? [force] : ['light', 'dark'];
  return (
    <>
      {themes.map(t => (
        <Panel theme={t} key={t} />
      ))}
    </>
  );
}

render(<Styleguide />, document.getElementById('root')!);
