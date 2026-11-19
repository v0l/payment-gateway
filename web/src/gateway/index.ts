const MODAL_STYLE = {
  overlay: 'position:fixed;inset:0;z-index:2147483000;background:rgba(0,0,0,.55);display:flex;align-items:center;justify-content:center;padding:16px',
  dialog: 'position:relative;background:#fff;border-radius:12px;max-width:480px;width:100%;max-height:100%;overflow:auto',
  close: 'position:absolute;top:6px;right:6px;z-index:1;min-width:44px;min-height:44px;border:0;border-radius:22px;background:#fff;color:#000;font:24px/1 sans-serif;cursor:pointer',
};

interface GatewayEvent {
  source?: string;
  type?: string;
  payment_id?: string;
  height?: number;
  code?: string;
  message?: string;
}

interface OpenOptions {
  gateway: string;
  paymentId: string;
  mode?: string;
  container?: string | Element;
  title?: string;
  closeLabel?: string;
  onReady?: () => void;
  onPaid?: (m: GatewayEvent) => void;
  onExpired?: (m: GatewayEvent) => void;
  onCancelled?: (m: GatewayEvent) => void;
  onError?: (e: { code: string; message: string }) => void;
  onClosed?: () => void;
}

interface Handle {
  frame: HTMLIFrameElement;
  close(): void;
}

function open(options: OpenOptions): Handle {
  if (!options || !options.gateway || !options.paymentId) {
    throw new Error('gateway and paymentId are required');
  }
  const base = String(options.gateway).replace(/\/+$/, '');
  const gatewayOrigin = new URL(base).origin;
  const modal = options.mode === 'modal';
  let container: Element | null = null;
  if (!modal) {
    container = typeof options.container === 'string' ? document.querySelector(options.container) : (options.container ?? null);
    if (!container) throw new Error('container not found');
  }

  const frame = document.createElement('iframe');
  frame.src = `${base}/embed/${encodeURIComponent(options.paymentId)}`;
  frame.title = options.title || 'Payment';
  frame.setAttribute('allow', 'clipboard-write');
  frame.style.cssText = 'border:0;width:100%;min-height:420px;display:block';

  let overlay: HTMLDivElement | null = null;
  let opener: Element | null = null;
  let closed = false;

  const onKey = (ev: KeyboardEvent) => {
    if (ev.key === 'Escape') handle.close();
  };

  if (modal) {
    opener = document.activeElement;
    overlay = document.createElement('div');
    overlay.style.cssText = MODAL_STYLE.overlay;
    const dialog = document.createElement('div');
    dialog.setAttribute('role', 'dialog');
    dialog.setAttribute('aria-modal', 'true');
    dialog.setAttribute('aria-label', frame.title);
    dialog.style.cssText = MODAL_STYLE.dialog;
    const x = document.createElement('button');
    x.type = 'button';
    x.setAttribute('aria-label', options.closeLabel || 'Close');
    x.textContent = '\u00d7';
    x.style.cssText = MODAL_STYLE.close;
    x.addEventListener('click', () => handle.close());
    const o = overlay;
    o.addEventListener('mousedown', ev => {
      if (ev.target === o) handle.close();
    });
    dialog.appendChild(x);
    dialog.appendChild(frame);
    o.appendChild(dialog);
    document.body.appendChild(o);
    x.focus();
    document.addEventListener('keydown', onKey);
  } else {
    container!.appendChild(frame);
  }

  const fail = (code: string, message: string) => options.onError?.({ code, message });

  frame.addEventListener('error', () => fail('frame_error', 'the payment frame failed to load'));

  const handler = (ev: MessageEvent<GatewayEvent>) => {
    if (ev.origin !== gatewayOrigin || ev.source !== frame.contentWindow) return;
    const m = ev.data;
    if (!m || m.source !== 'payment-gateway' || m.payment_id !== options.paymentId) return;
    switch (m.type) {
      case 'pgw:resize':
        if (typeof m.height === 'number' && m.height > 0) frame.style.height = `${m.height}px`;
        break;
      case 'pgw:ready':
        options.onReady?.();
        break;
      case 'pgw:paid':
        options.onPaid?.(m);
        break;
      case 'pgw:expired':
        options.onExpired?.(m);
        break;
      case 'pgw:cancelled':
        options.onCancelled?.(m);
        break;
      case 'pgw:error':
        fail(String(m.code || 'error'), String(m.message || ''));
        break;
    }
  };
  window.addEventListener('message', handler);

  const handle: Handle = {
    frame,
    close() {
      if (closed) return;
      closed = true;
      window.removeEventListener('message', handler);
      document.removeEventListener('keydown', onKey);
      if (modal && overlay?.parentNode) overlay.parentNode.removeChild(overlay);
      else frame.parentNode?.removeChild(frame);
      if (modal && opener instanceof HTMLElement) opener.focus();
      options.onClosed?.();
    },
  };
  return handle;
}

(window as unknown as { PaymentGateway: { open: typeof open } }).PaymentGateway = { open };
