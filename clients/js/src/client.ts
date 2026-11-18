export interface Attempt {
  id: number;
  provider_id: number;
  method: string;
  option: string | null;
  status: string;
  amount: number;
  currency: string;
  base_amount: number;
  processing_fee: number;
  expires: string;
  paid_at: string | null;
  details: Record<string, any>;
  [key: string]: unknown;
}

export interface Refund {
  id: number;
  attempt_id: number;
  amount: number;
  currency: string;
  status: string;
  error: string | null;
  created: string;
  payment_id?: string;
}

export interface Payment {
  id: string;
  reference: string;
  amount: number;
  currency: string;
  tax: number;
  status: string;
  created: string;
  expires: string;
  paid_at: string | null;
  description: string | null;
  customer_ref: string | null;
  metadata: any;
  pay_url: string;
  attempts: Attempt[];
  refunds: Refund[];
  methods?: Array<Record<string, any>>;
  [key: string]: unknown;
}

export interface CreatePaymentInput {
  reference?: string;
  amount: number;
  currency: string;
  tax?: number;
  expires_in?: number;
  description?: string;
  customer_ref?: string;
  customer_email?: string;
  metadata?: unknown;
  methods?: string[];
  success_url?: string;
  cancel_url?: string;
}

export interface CreateAttemptInput {
  method: string;
  provider_id?: number;
  option?: string;
  save_method?: boolean;
  saved_method_id?: number;
}

export interface RefundInput {
  amount?: number;
  destination?: string;
  attempt_id?: number;
}

export interface SavedMethod {
  id: number;
  customer_ref: string;
  provider_id: number;
  brand: string | null;
  last4: string | null;
  exp_month: number | null;
  exp_year: number | null;
}

export interface ListPaymentsParams {
  status?: string;
  reference?: string;
  customer_ref?: string;
  created_from?: string | number;
  created_to?: string | number;
  limit?: number;
  cursor?: string;
}

export interface Page<T> {
  items: T[];
  next: string | null;
}

export interface RequestOptions {
  idempotencyKey?: string;
  signal?: AbortSignal;
}

export interface ClientOptions {
  baseUrl: string;
  apiKey: string;
  fetch?: typeof fetch;
  maxRetries?: number;
  retryBaseDelayMs?: number;
  retryMaxDelayMs?: number;
  timeoutMs?: number;
  userAgent?: string;
  sleep?: (ms: number) => Promise<void>;
}

export class GatewayError extends Error {
  readonly status: number;
  readonly code: string;
  readonly field?: string;
  readonly body: any;
  readonly retryAfter?: number;

  constructor(status: number, body: any, retryAfter?: number) {
    const detail = body?.error_detail;
    super(detail?.message ?? body?.error ?? `request failed with status ${status}`);
    this.name = "GatewayError";
    this.status = status;
    this.code = detail?.code ?? body?.code ?? "error";
    this.field = detail?.field;
    this.body = body;
    this.retryAfter = retryAfter;
  }
}

export class NetworkError extends Error {
  constructor(message: string, readonly cause?: unknown) {
    super(message);
    this.name = "NetworkError";
  }
}

export function newIdempotencyKey(): string {
  const c = (globalThis as any).crypto;
  if (c?.randomUUID) return c.randomUUID();
  return Array.from({ length: 32 }, () => Math.floor(Math.random() * 16).toString(16)).join("");
}

const RETRYABLE_STATUS = (status: number) => status === 429 || (status >= 500 && status <= 599);

export class Gateway {
  private readonly baseUrl: string;
  private readonly apiKey: string;
  private readonly fetchImpl: typeof fetch;
  private readonly maxRetries: number;
  private readonly retryBaseDelayMs: number;
  private readonly retryMaxDelayMs: number;
  private readonly timeoutMs: number;
  private readonly userAgent: string;
  private readonly sleep: (ms: number) => Promise<void>;

  constructor(options: ClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, "");
    this.apiKey = options.apiKey;
    this.fetchImpl = options.fetch ?? ((...args) => globalThis.fetch(...args));
    this.maxRetries = options.maxRetries ?? 3;
    this.retryBaseDelayMs = options.retryBaseDelayMs ?? 250;
    this.retryMaxDelayMs = options.retryMaxDelayMs ?? 8000;
    this.timeoutMs = options.timeoutMs ?? 30000;
    this.userAgent = options.userAgent ?? "payment-gateway-js/0.1.0";
    this.sleep = options.sleep ?? ((ms) => new Promise((r) => setTimeout(r, ms)));
  }

  private delayFor(attempt: number, retryAfter?: number): number {
    if (retryAfter !== undefined) return Math.min(retryAfter * 1000, this.retryMaxDelayMs);
    const exp = Math.min(this.retryBaseDelayMs * 2 ** attempt, this.retryMaxDelayMs);
    return Math.floor(exp / 2 + Math.random() * (exp / 2));
  }

  private async send(
    method: string,
    path: string,
    body?: unknown,
    options: RequestOptions = {},
  ): Promise<{ json: any; headers: Headers }> {
    const headers: Record<string, string> = {
      authorization: `Bearer ${this.apiKey}`,
      accept: "application/json",
    };
    if (typeof window === "undefined") headers["user-agent"] = this.userAgent;
    if (body !== undefined) headers["content-type"] = "application/json";
    if (options.idempotencyKey) headers["idempotency-key"] = options.idempotencyKey;
    const payload = body === undefined ? undefined : JSON.stringify(body);

    for (let attempt = 0; ; attempt++) {
      let response: Response;
      try {
        const timeout = AbortSignal.timeout(this.timeoutMs);
        const signal = options.signal ? AbortSignal.any([options.signal, timeout]) : timeout;
        response = await this.fetchImpl(this.baseUrl + path, { method, headers, body: payload, signal });
      } catch (e) {
        if (options.signal?.aborted) throw e;
        if (attempt < this.maxRetries && (method === "GET" || options.idempotencyKey)) {
          await this.sleep(this.delayFor(attempt));
          continue;
        }
        throw new NetworkError(`request to ${path} failed: ${(e as Error).message}`, e);
      }
      const text = await response.text();
      let json: any = null;
      if (text) {
        try {
          json = JSON.parse(text);
        } catch {
          json = { error: text };
        }
      }
      if (response.ok) return { json, headers: response.headers };
      const retryHeader = response.headers.get("retry-after");
      const retryAfter = retryHeader && /^\d+$/.test(retryHeader) ? Number(retryHeader) : undefined;
      const safe = method === "GET" || method === "DELETE" || options.idempotencyKey !== undefined || response.status === 429;
      if (RETRYABLE_STATUS(response.status) && safe && attempt < this.maxRetries) {
        await this.sleep(this.delayFor(attempt, retryAfter));
        continue;
      }
      throw new GatewayError(response.status, json, retryAfter);
    }
  }

  private query(params: Record<string, string | number | undefined>): string {
    const q = new URLSearchParams();
    for (const [k, v] of Object.entries(params)) if (v !== undefined) q.set(k, String(v));
    const s = q.toString();
    return s ? `?${s}` : "";
  }

  private async page<T>(path: string, params: Record<string, string | number | undefined>): Promise<Page<T>> {
    const { json, headers } = await this.send("GET", path + this.query(params));
    return { items: json as T[], next: headers.get("x-next-cursor") };
  }

  private async *iterate<T>(
    path: string,
    params: Record<string, string | number | undefined>,
  ): AsyncGenerator<T, void, undefined> {
    let cursor = params.cursor as string | undefined;
    for (;;) {
      const page = await this.page<T>(path, { ...params, cursor });
      for (const item of page.items) yield item;
      if (!page.next) return;
      cursor = page.next;
    }
  }

  async createPayment(input: CreatePaymentInput, options: RequestOptions = {}): Promise<Payment> {
    const key = options.idempotencyKey ?? newIdempotencyKey();
    return (await this.send("POST", "/v1/payments", input, { ...options, idempotencyKey: key })).json;
  }

  async getPayment(id: string): Promise<Payment> {
    return (await this.send("GET", `/v1/payments/${encodeURIComponent(id)}`, undefined)).json;
  }

  async createAttempt(id: string, input: CreateAttemptInput): Promise<Payment> {
    return (await this.send("POST", `/v1/payments/${encodeURIComponent(id)}/attempts`, input)).json;
  }

  async cancelPayment(id: string): Promise<Payment> {
    return (await this.send("POST", `/v1/payments/${encodeURIComponent(id)}/cancel`, {})).json;
  }

  async refundPayment(id: string, input: RefundInput = {}): Promise<Refund> {
    return (await this.send("POST", `/v1/payments/${encodeURIComponent(id)}/refund`, input)).json;
  }

  listPaymentsPage(params: ListPaymentsParams = {}): Promise<Page<Payment>> {
    return this.page("/v1/payments", { ...params });
  }

  listPayments(params: ListPaymentsParams = {}): AsyncGenerator<Payment, void, undefined> {
    return this.iterate("/v1/payments", { ...params });
  }

  listRefunds(params: { payment_id?: string; status?: string; limit?: number } = {}): AsyncGenerator<Refund, void, undefined> {
    return this.iterate("/v1/refunds", { ...params });
  }

  listEvents(params: { type?: string; limit?: number; cursor?: string } = {}): AsyncGenerator<Record<string, any>, void, undefined> {
    return this.iterate("/v1/events", { ...params });
  }

  listMethods(params: { limit?: number } = {}): AsyncGenerator<Record<string, any>, void, undefined> {
    return this.iterate("/v1/methods", { ...params });
  }

  listSavedMethods(params: { customer_ref?: string; limit?: number } = {}): AsyncGenerator<SavedMethod, void, undefined> {
    return this.iterate("/v1/saved-methods", { ...params });
  }

  async deleteSavedMethod(id: number): Promise<void> {
    await this.send("DELETE", `/v1/saved-methods/${id}`, undefined);
  }

  async chargeSavedMethod(id: number, input: CreatePaymentInput, options: RequestOptions = {}): Promise<Payment> {
    const key = options.idempotencyKey ?? newIdempotencyKey();
    return (await this.send("POST", `/v1/saved-methods/${id}/charge`, input, { ...options, idempotencyKey: key })).json;
  }
}
