import { createHmac, timingSafeEqual } from "node:crypto";

export const SIGNATURE_HEADER = "x-gateway-signature";
export const DEFAULT_TOLERANCE_SECONDS = 300;

export class WebhookVerificationError extends Error {
  readonly reason: "missing_header" | "malformed_header" | "timestamp_outside_tolerance" | "signature_mismatch";

  constructor(reason: WebhookVerificationError["reason"], message: string) {
    super(message);
    this.name = "WebhookVerificationError";
    this.reason = reason;
  }
}

export type HeaderBag = Headers | Record<string, string | string[] | undefined>;

export interface WebhookEvent {
  id: string;
  type: string;
  created: number;
  data: { payment: Record<string, any>; detail: Record<string, any> };
  [key: string]: unknown;
}

function readHeader(headers: HeaderBag, name: string): string | undefined {
  if (typeof (headers as Headers).get === "function") {
    return (headers as Headers).get(name) ?? undefined;
  }
  const bag = headers as Record<string, string | string[] | undefined>;
  for (const key of Object.keys(bag)) {
    if (key.toLowerCase() === name) {
      const value = bag[key];
      return Array.isArray(value) ? value[0] : value;
    }
  }
  return undefined;
}

export function signWebhook(rawBody: string | Uint8Array, secret: string, timestamp: number): string {
  return createHmac("sha256", secret).update(`${timestamp}.`).update(rawBody).digest("hex");
}

export function verifyWebhook(
  rawBody: string | Uint8Array,
  headers: HeaderBag,
  secret: string,
  toleranceSeconds: number = DEFAULT_TOLERANCE_SECONDS,
  nowSeconds: number = Math.floor(Date.now() / 1000),
): WebhookEvent {
  const header = readHeader(headers, SIGNATURE_HEADER);
  if (!header) {
    throw new WebhookVerificationError("missing_header", `missing ${SIGNATURE_HEADER} header`);
  }
  let timestamp: number | undefined;
  const signatures: string[] = [];
  for (const part of header.split(",")) {
    const [key, value] = part.trim().split("=", 2);
    if (key === "t" && value !== undefined && /^\d+$/.test(value)) timestamp = Number(value);
    if (key === "v1" && value !== undefined) signatures.push(value);
  }
  if (timestamp === undefined || signatures.length === 0) {
    throw new WebhookVerificationError("malformed_header", "malformed signature header");
  }
  const expected = Buffer.from(signWebhook(rawBody, secret, timestamp), "hex");
  let matched = false;
  for (const candidate of signatures) {
    const given = Buffer.from(candidate, "hex");
    if (given.length === expected.length && timingSafeEqual(given, expected)) matched = true;
  }
  if (!matched) {
    throw new WebhookVerificationError("signature_mismatch", "signature does not match");
  }
  if (toleranceSeconds > 0 && Math.abs(nowSeconds - timestamp) > toleranceSeconds) {
    throw new WebhookVerificationError("timestamp_outside_tolerance", "timestamp outside tolerance");
  }
  const text = typeof rawBody === "string" ? rawBody : Buffer.from(rawBody).toString("utf8");
  return JSON.parse(text) as WebhookEvent;
}
