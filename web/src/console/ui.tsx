import { useEffect, useRef, useState } from 'preact/hooks';
import type { ComponentChildren, JSX } from 'preact';
import { STATUS_TONE, fmtAmount, moneyParts, statusWord } from './format';
import { toast } from './dialogs';

export function Pill(props: { status: string; word?: string | null }) {
  const s = props.status;
  return <span class={`pill ${STATUS_TONE[s] ?? s}`}>{props.word ?? statusWord(s)}</span>;
}

export function Money(props: { amount: number; currency: string; fee?: number | null }) {
  const { amount, currency, fee } = props;
  return (
    <span class="money" title={currency === 'BTC' ? `${amount.toLocaleString()} msats` : undefined}>
      {fmtAmount(amount, currency)}
      {fee ? <small>{`+${fmtAmount(fee, currency)} fee`}</small> : null}
    </span>
  );
}

export function BigMoney(props: { amount: number; currency: string; cls?: string }) {
  const [a, b] = moneyParts(props.amount, props.currency);
  return (
    <span class={`big-money ${props.cls ?? ''}`}>
      {a}
      {b ? <small>{b}</small> : null}
    </span>
  );
}

export function SignedMoney(props: { amount: number; currency: string }) {
  const s = fmtAmount(Math.abs(props.amount), props.currency);
  return <span class={`money ${props.amount < 0 ? 'out' : 'in'}`}>{`${props.amount < 0 ? '\u2212' : '+'}${s}`}</span>;
}

export function Skeleton() {
  return (
    <div>
      <div class="skeleton" />
      <div class="skeleton" />
      <div class="skeleton" />
    </div>
  );
}

export function Empty(props: { title: string; hint?: string; children?: ComponentChildren }) {
  return (
    <div class="empty">
      <b>{props.title}</b>
      {props.hint !== undefined ? <p class="muted">{props.hint}</p> : null}
      {props.children}
    </div>
  );
}

export function Msg(props: { text: string; bad?: boolean }) {
  return <div class={props.bad ? 'err' : 'ok-msg'}>{props.text}</div>;
}

export interface Note {
  text: string;
  bad: boolean;
}

export function useNote(): [JSX.Element, (text: string, bad?: boolean) => void, (n: ComponentChildren) => void] {
  const [note, setNote] = useState<Note | null>(null);
  const [extra, setExtra] = useState<ComponentChildren>(null);
  const node = (
    <div>
      {note ? <Msg text={note.text} bad={note.bad} /> : null}
      {extra}
    </div>
  );
  return [
    node,
    (text, bad = false) => {
      setExtra(null);
      setNote({ text, bad });
    },
    n => {
      setNote(null);
      setExtra(n);
    },
  ];
}

export function SecretBox(props: { title: string; value: string }) {
  return (
    <div class="secret">
      <b>{`${props.title} (shown once, copy it now)`}</b>
      <span class="mono">{props.value}</span>
    </div>
  );
}

export function copyText(text: string, label?: string): void {
  navigator.clipboard?.writeText(text).then(() => toast(`${label ?? 'Copied'}.`));
}

let fieldSeq = 0;

export function Field(props: { label: string; children: (id: string) => JSX.Element }) {
  const id = useRef(`f${++fieldSeq}`).current;
  return (
    <div class="field">
      <label for={id}>{props.label}</label>
      {props.children(id)}
    </div>
  );
}

export function TextField(props: {
  label: string;
  value: string;
  onInput: (v: string) => void;
  type?: string;
  placeholder?: string;
  autocomplete?: string;
  required?: boolean;
  minLength?: number;
  maxLength?: number;
  inputMode?: JSX.HTMLAttributes['inputMode'];
  disabled?: boolean;
  min?: string;
  step?: string;
}) {
  return (
    <Field label={props.label}>
      {id => (
        <input
          id={id}
          type={props.type ?? 'text'}
          value={props.value}
          placeholder={props.placeholder}
          autocomplete={props.autocomplete}
          required={props.required}
          minLength={props.minLength}
          maxLength={props.maxLength}
          inputMode={props.inputMode}
          disabled={props.disabled}
          min={props.min}
          step={props.step}
          onInput={e => props.onInput(e.currentTarget.value)}
        />
      )}
    </Field>
  );
}

export function AreaField(props: { label: string; value: string; onInput: (v: string) => void; placeholder?: string; rows?: number }) {
  return (
    <Field label={props.label}>
      {id => <textarea id={id} rows={props.rows} value={props.value} placeholder={props.placeholder} onInput={e => props.onInput(e.currentTarget.value)} />}
    </Field>
  );
}

export function SelectField(props: { label: string; value: string; onChange: (v: string) => void; options: [string, string][]; disabled?: boolean }) {
  return (
    <Field label={props.label}>
      {id => (
        <select id={id} value={props.value} disabled={props.disabled} onChange={e => props.onChange(e.currentTarget.value)}>
          {props.options.map(o => (
            <option value={o[0]} key={o[0]}>{o[1]}</option>
          ))}
        </select>
      )}
    </Field>
  );
}

export function Check(props: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; children: ComponentChildren }) {
  return (
    <label class="check">
      <input type="checkbox" checked={props.checked} disabled={props.disabled} onChange={e => props.onChange(e.currentTarget.checked)} />
      {props.children}
    </label>
  );
}

export type Head = string | [string, string];

export function Table(props: { head: Head[] | null; cls?: string; label?: string; children: ComponentChildren }) {
  const head = props.head;
  const label =
    props.label ??
    (head
      ? `${head
          .map(h => (typeof h === 'string' ? h : h[0]))
          .filter(Boolean)
          .slice(0, 3)
          .join(', ')} table`
      : 'Table');
  return (
    <div class={`table-wrap${props.cls ? ` ${props.cls}` : ''}`} tabIndex={0} role="region" aria-label={label}>
      <table>
        {head ? (
          <thead>
            <tr>
              {head.map((h, i) => (typeof h === 'string' ? <th key={i}>{h}</th> : <th key={i} class={h[1]}>{h[0]}</th>))}
            </tr>
          </thead>
        ) : null}
        <tbody>{props.children}</tbody>
      </table>
    </div>
  );
}

export function ClickRow(props: { onOpen: () => void; cls?: string; id?: string; children: ComponentChildren }) {
  return (
    <tr
      class={`${props.cls ? `${props.cls} ` : ''}click`}
      data-id={props.id}
      tabIndex={0}
      onClick={e => {
        if ((e.target as HTMLElement).closest('a,button,input,label')) return;
        props.onOpen();
      }}
      onKeyDown={e => {
        if (e.key === 'Enter' && e.target === e.currentTarget) props.onOpen();
      }}
    >
      {props.children}
    </tr>
  );
}

export function Section(props: { title: string; cls?: string; id?: string; aside?: ComponentChildren; children: ComponentChildren }) {
  return (
    <section class={`sec${props.cls ? ` ${props.cls}` : ''}`} id={props.id}>
      <div class="sec-head">
        <h2>{props.title}</h2>
        {props.aside}
      </div>
      {props.children}
    </section>
  );
}

export function Kv(props: { pairs: ([string, ComponentChildren, string?] | null)[] }) {
  return (
    <dl class="kv">
      {props.pairs.filter((p): p is [string, ComponentChildren, string?] => !!p).map((p, i) => (
        <div key={i}>
          <dt>{p[0]}</dt>
          <dd class={p[2] || undefined}>{p[1] === null || p[1] === undefined || p[1] === '' ? <span class="muted">None</span> : p[1]}</dd>
        </div>
      ))}
    </dl>
  );
}

export function Chips(props: { options: [string, string, (string | number)?][]; value: string; onPick: (v: string) => void; label: string }) {
  const [current, setCurrent] = useState(props.value);
  useEffect(() => setCurrent(props.value), [props.value]);
  return (
    <div class="chips" role="group" aria-label={props.label}>
      {props.options.map(o => (
        <button
          type="button"
          key={o[0]}
          class={o[0] === current ? 'on' : ''}
          aria-pressed={o[0] === current ? 'true' : 'false'}
          onClick={() => {
            setCurrent(o[0]);
            props.onPick(o[0]);
          }}
        >
          {o[1]}
          {o[2] !== undefined ? <span class="n">{String(o[2])}</span> : null}
        </button>
      ))}
    </div>
  );
}

export function PageHead(props: { title: string; sub?: ComponentChildren; actions?: ComponentChildren }) {
  return (
    <div class="page-head">
      <div>
        <h1>{props.title}</h1>
        {props.sub ? <p class="sub">{props.sub}</p> : null}
      </div>
      {props.actions ? <div class="actions">{props.actions}</div> : null}
    </div>
  );
}

export function Figure(props: { value: string; label: string; tone?: string }) {
  return (
    <div class={`fig${props.tone ? ` ${props.tone}` : ''}`}>
      <div class="fig-n money">{props.value}</div>
      <div class="fig-l">{props.label}</div>
    </div>
  );
}

export function useBusy(): [boolean, <T>(fn: () => Promise<T>) => Promise<T>] {
  const [busy, setBusy] = useState(false);
  const run = async <T,>(fn: () => Promise<T>): Promise<T> => {
    setBusy(true);
    try {
      return await fn();
    } finally {
      setBusy(false);
    }
  };
  return [busy, run];
}

export function Loading(props: { error: string; ready: boolean; children: ComponentChildren }) {
  if (props.error) return <div class="err" role="alert">{props.error}</div>;
  if (!props.ready) return <Skeleton />;
  return <>{props.children}</>;
}
