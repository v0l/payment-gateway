import { render } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';

export interface ConfirmOptions {
  title: string;
  body: string;
  keep: string;
  confirm: string;
  danger?: boolean;
  input?: string | null;
  inputType?: 'text' | 'area';
  muted?: boolean;
}

interface Pending extends ConfirmOptions {
  resolve: (v: { text: string } | null) => void;
}

let setPending: (p: Pending | null) => void = () => {};
let pushToast: (text: string) => void = () => {};

export function confirmDialog(o: ConfirmOptions): Promise<{ text: string } | null> {
  return new Promise(resolve => setPending({ ...o, resolve }));
}

export function confirmChoice(title: string, body: string, safeLabel: string, dangerLabel: string, onConfirm: () => void): void {
  confirmDialog({ title, body, keep: safeLabel, confirm: dangerLabel, danger: true, muted: true }).then(r => {
    if (r) onConfirm();
  });
}

export function toast(text: string): void {
  pushToast(text);
}

function Dialog(props: { p: Pending; close: (v: { text: string } | null) => void }) {
  const { p } = props;
  const [text, setText] = useState('');
  const keep = useRef<HTMLButtonElement>(null);
  const prev = useRef(document.activeElement as HTMLElement | null);
  useEffect(() => {
    keep.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') props.close(null);
    };
    document.addEventListener('keydown', onKey);
    const before = prev.current;
    return () => {
      document.removeEventListener('keydown', onKey);
      before?.focus?.();
    };
  }, []);
  return (
    <div class="dialog-scrim modal">
      <div class="dialog" role="alertdialog" aria-modal="true" aria-label={p.title}>
        <h4>{p.title}</h4>
        <p class={p.muted ? 'muted' : undefined}>{p.body}</p>
        {p.input ? (
          p.inputType === 'text' ? (
            <input type="text" aria-label={p.input} placeholder={p.input} value={text} onInput={e => setText(e.currentTarget.value)} />
          ) : (
            <textarea rows={2} aria-label={p.input} placeholder={p.input} value={text} onInput={e => setText(e.currentTarget.value)} />
          )
        ) : null}
        <div class="row">
          <button ref={keep} onClick={() => props.close(null)}>{p.keep}</button>
          <button class={p.danger ? 'danger' : 'primary'} onClick={() => props.close({ text: p.input ? text : '' })}>{p.confirm}</button>
        </div>
      </div>
    </div>
  );
}

function Overlays() {
  const [pending, setP] = useState<Pending | null>(null);
  const [toasts, setToasts] = useState<{ id: number; text: string }[]>([]);
  useEffect(() => {
    setPending = setP;
    let n = 0;
    pushToast = text => {
      const id = ++n;
      setToasts(t => [...t, { id, text }]);
      setTimeout(() => setToasts(t => t.filter(x => x.id !== id)), 5000);
    };
    return () => {
      setPending = () => {};
      pushToast = () => {};
    };
  }, []);
  return (
    <>
      {pending ? (
        <Dialog
          p={pending}
          close={v => {
            setP(null);
            pending.resolve(v);
          }}
        />
      ) : null}
      {toasts.map(t => (
        <div class="toast" role="status" key={t.id}>{t.text}</div>
      ))}
    </>
  );
}

export function mountOverlays(): void {
  const host = document.createElement('div');
  host.className = 'overlay-root';
  document.body.appendChild(host);
  render(<Overlays />, host);
}
