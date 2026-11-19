export const project = { name: "Northwind Hosting", mode: "Live", role: "Owner" };

export const satsPerEur = 1112;

export const methods: Record<string, string> = {
  lightning: "Lightning",
  onchain: "On-chain",
  card: "Card",
  paypal: "PayPal",
  bank: "Bank transfer",
};

export const payments = [
  { id: "pay_7Q4M2X", time: "21:46", day: "Today", desc: "VPS Dublin-3, 1 month", cents: 1800, method: "lightning", status: "paid", customer: "cus_amelia.k" },
  { id: "pay_3N8T5B", time: "21:31", day: "Today", desc: "VPS Frankfurt-1, 12 months", cents: 19200, method: "onchain", status: "paid", customer: "cus_tomasz.w" },
  { id: "pay_9C1D7R", time: "21:12", day: "Today", desc: "Credit top-up", cents: 5000, method: "card", status: "paid", customer: "cus_priya.n" },
  { id: "pay_2K6W4Z", time: "20:58", day: "Today", desc: "VPS Dublin-3, 1 month", cents: 1800, method: "lightning", status: "pending", customer: "cus_liam.o" },
  { id: "pay_5H3V8L", time: "20:40", day: "Today", desc: "IPv4 /29 block", cents: 4500, method: "paypal", status: "paid", customer: "cus_noor.s" },
  { id: "pay_1B9X6E", time: "20:05", day: "Today", desc: "Credit top-up", cents: 10000, method: "bank", status: "paid", customer: "cus_jonas.b" },
  { id: "pay_6T2J0P", time: "19:52", day: "Today", desc: "VPS Dublin-3, 1 month", cents: 1800, method: "lightning", status: "expired", customer: "cus_hana.m" },
  { id: "pay_4R7F3A", time: "19:20", day: "Today", desc: "Dedicated host, 1 month", cents: 14900, method: "card", status: "refunded", customer: "cus_ravi.d" },
  { id: "pay_0Y5G9S", time: "18:44", day: "Today", desc: "VPS Frankfurt-2, 3 months", cents: 5400, method: "onchain", status: "pending", customer: "cus_elsa.v" },
  { id: "pay_8D3M1C", time: "17:30", day: "Today", desc: "Credit top-up", cents: 2500, method: "card", status: "failed", customer: "cus_omar.f" },
  { id: "pay_2W8Q7N", time: "16:15", day: "Today", desc: "VPS Dublin-3, 1 month", cents: 1800, method: "lightning", status: "paid", customer: "cus_sofia.r" },
  { id: "pay_7L4K9H", time: "15:02", day: "Today", desc: "Reverse DNS add-on", cents: 300, method: "paypal", status: "paid", customer: "cus_amelia.k" },
  { id: "pay_3E6U2T", time: "11:48", day: "Today", desc: "Dedicated host, 1 month", cents: 14900, method: "card", status: "disputed", customer: "cus_yuki.t" },
  { id: "pay_9P1Z5V", time: "09:10", day: "Today", desc: "VPS Frankfurt-1, 12 months", cents: 19200, method: "bank", status: "paid", customer: "cus_marta.l" },
  { id: "pay_4A8S2D", time: "23:41", day: "Yesterday", desc: "VPS Dublin-3, 1 month", cents: 1800, method: "lightning", status: "paid", customer: "cus_dara.q" },
  { id: "pay_6V1N7K", time: "22:09", day: "Yesterday", desc: "Credit top-up", cents: 7500, method: "lightning", status: "paid", customer: "cus_felix.g" },
  { id: "pay_1G5C3Y", time: "18:55", day: "Yesterday", desc: "VPS Frankfurt-1, 12 months", cents: 19200, method: "onchain", status: "paid", customer: "cus_ines.p" },
  { id: "pay_8X2R6M", time: "14:20", day: "Yesterday", desc: "IPv4 /29 block", cents: 4500, method: "card", status: "paid", customer: "cus_kai.h" },
];

export const detail = {
  ...payments[0],
  invoiceRef: "INV-2291",
  fee: 0,
  node: "lnd-dublin-1",
  events: [
    { t: "21:44:02.118", ms: 0, name: "payment.created", note: "Created by API key pk_live_4f9a", state: "open" },
    { t: "21:44:02.131", ms: 13, name: "attempt.created", note: "Lightning invoice lnbc200160n1p3x... issued, expires 22:44", state: "open" },
    { t: "21:44:40.502", ms: 38384, name: "pay_page.viewed", note: "Pay page opened from Dublin, IE", state: "open" },
    { t: "21:46:11.904", ms: 129786, name: "attempt.paid", note: "HTLC settled via lnd-dublin-1, preimage matched", state: "paid" },
    { t: "21:46:11.931", ms: 129813, name: "payment.paid", note: "Payment marked paid, receipt rn_6671 issued", state: "paid" },
    { t: "21:46:12.470", ms: 130352, name: "webhook.delivered", note: "POST https://billing.northwind.example/hooks/gateway answered 200 in 214 ms", state: "paid" },
  ],
  webhooks: [
    { url: "billing.northwind.example/hooks/gateway", event: "payment.paid", code: 200, ms: 214, at: "21:46:12" },
    { url: "ops.northwind.example/hooks/audit", event: "payment.paid", code: 200, ms: 96, at: "21:46:12" },
  ],
  metadata: { order: "VPS-88213", plan: "dublin-3-small", customer_email: "amelia.k@northwind.example" },
};

export const month = {
  label: "1 to 18 November",
  paidCents: 1423640,
  paidCount: 291,
  waitingCents: 142000,
  waitingCount: 9,
  disputedCents: 14900,
  disputedCount: 1,
  refundedCents: 38200,
  refundedCount: 6,
  openingCents: 1186400,
  prevPaidCents: 1186400,
  daily: [310, 420, 380, 510, 640, 590, 480, 520, 710, 690, 640, 580, 760, 820, 790, 700, 880, 940],
  share: [
    { method: "lightning", pct: 46, cents: 654880, count: 168 },
    { method: "onchain", pct: 21, cents: 298960, count: 22 },
    { method: "card", pct: 19, cents: 270490, count: 61 },
    { method: "paypal", pct: 8, cents: 113890, count: 27 },
    { method: "bank", pct: 6, cents: 85420, count: 13 },
  ],
};

export const attention = [
  { kind: "dispute", title: "Card dispute on pay_3E6U2T", body: "Yuki T. says the dedicated host was not delivered. Evidence due in 6 days.", cta: "Respond", cents: 14900 },
  { kind: "confirm", title: "On-chain payment needs 2 more confirmations", body: "pay_0Y5G9S, 0.00600 BTC to bc1q...k8vz, seen 18:44, 1 of 3 confirmations.", cta: "Watch", cents: 5400 },
  { kind: "failed", title: "Card declined for pay_8D3M1C", body: "Issuer returned insufficient funds. The customer has not retried.", cta: "Send new link", cents: 2500 },
];

export function eur(cents: number, { sign = false }: { sign?: boolean } = {}): string {
  const abs = Math.abs(cents);
  const whole = Math.floor(abs / 100).toLocaleString("en-IE");
  const frac = String(abs % 100).padStart(2, "0");
  const s = cents < 0 ? "-" : sign ? "+" : "";
  return `${s}\u20ac${whole}.${frac}`;
}

export function sats(cents: number): string {
  return Math.round((cents * satsPerEur) / 100).toLocaleString("en-IE");
}

export function esc(s: unknown): string {
  return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export const statusWord: Record<string, string> = {
  paid: "Paid",
  pending: "Waiting",
  expired: "Expired",
  failed: "Failed",
  refunded: "Refunded",
  disputed: "Disputed",
};
