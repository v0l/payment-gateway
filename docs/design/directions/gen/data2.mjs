import { payments, detail, month, eur, sats } from "./data.mjs";

export const today = payments.filter(p => p.day === "Today");
export const paidToday = today.filter(p => p.status === "paid").reduce((a, p) => a + p.cents, 0);

export const hourlyToday = [0, 0, 0, 0, 0, 0, 0, 0, 0, 19200, 19200, 19200, 19200, 19200, 19200, 19500, 21300, 21300, 21300, 21300, 30800, 61800];
export const hourlyLastWeek = [0, 0, 0, 1800, 1800, 1800, 1800, 1800, 6800, 6800, 11300, 13100, 24000, 24000, 30500, 30500, 35000, 36800, 41300, 46300, 50800, 54000, 57600, 59000];

export const monthFigures = [
  { label: "Paid", value: eur(month.paidCents), prev: eur(month.prevPaidCents), delta: "+20.0%" },
  { label: "Successful payments", value: "291", prev: "248", delta: "+17.3%" },
  { label: "Success rate", value: "93.1%", prev: "91.4%", delta: "+1.7 pts" },
  { label: "Average payment", value: eur(4892), prev: eur(4784), delta: "+2.3%" },
  { label: "Refunded", value: eur(month.refundedCents), prev: eur(51000), delta: "-25.1%" },
];

export const activity = [
  { t: "21:46", kind: "paid", title: `${eur(1800)} paid`, who: "cus_amelia.k", what: "VPS Dublin-3, 1 month", method: "Lightning" },
  { t: "21:31", kind: "paid", title: `${eur(19200)} paid`, who: "cus_tomasz.w", what: "VPS Frankfurt-1, 12 months, 3 confirmations", method: "On-chain" },
  { t: "21:12", kind: "paid", title: `${eur(5000)} paid`, who: "cus_priya.n", what: "Credit top-up", method: "Card" },
  { t: "20:58", kind: "pending", title: `${eur(1800)} payment created`, who: "cus_liam.o", what: "Waiting for the customer to pay the invoice", method: "Lightning" },
  { t: "20:41", kind: "failed", title: "Webhook failed", who: "ops.northwind.example", what: "payment.paid answered 503, retried at 20:51 and delivered", method: "Webhook" },
  { t: "20:40", kind: "paid", title: `${eur(4500)} paid`, who: "cus_noor.s", what: "IPv4 /29 block", method: "PayPal" },
  { t: "20:05", kind: "paid", title: `${eur(10000)} paid`, who: "cus_jonas.b", what: "Credit top-up", method: "Bank transfer" },
  { t: "19:52", kind: "muted", title: `${eur(1800)} expired`, who: "cus_hana.m", what: "Invoice expired unpaid after 60 minutes", method: "Lightning" },
  { t: "19:20", kind: "failed", title: `${eur(14900)} refunded`, who: "cus_ravi.d", what: "Dedicated host, 1 month. Refunded by sam@northwind.example", method: "Card" },
  { t: "18:44", kind: "pending", title: `${eur(5400)} seen on-chain`, who: "cus_elsa.v", what: "1 of 3 confirmations", method: "On-chain" },
  { t: "17:30", kind: "failed", title: `${eur(2500)} declined`, who: "cus_omar.f", what: "Issuer returned insufficient funds", method: "Card" },
  { t: "11:48", kind: "failed", title: "Dispute opened", who: "cus_yuki.t", what: `${eur(14900)}, product not received. Respond by 24 November`, method: "Card" },
];

export const wallets = [
  { name: "Lightning", where: "lnd-dublin-1, 12 channels", avail: 984210, settling: 0, note: "Instant" },
  { name: "On-chain", where: "bc1q...k8vz", avail: 731060, settling: 5400, note: "After 3 confirmations" },
  { name: "Card", where: "Stripe", avail: 490630, settling: 231600, note: "7 days" },
  { name: "PayPal", where: "sam@northwind.example", avail: 164020, settling: 45000, note: "7 days" },
  { name: "Bank transfer", where: "Revolut Business", avail: 187020, settling: 30000, note: "1 day" },
];
export const available = wallets.reduce((a, w) => a + w.avail, 0);
export const settling = wallets.reduce((a, w) => a + w.settling, 0);

export const payouts = {
  next: { date: "Friday 20 November", cents: 2000000, to: "AIB ending 4417", schedule: "Weekly on Fridays, keeping \u20ac5,000.00 in balance" },
  past: [
    { date: "13 Nov", cents: 1840000, state: "Arrived" },
    { date: "6 Nov", cents: 1695000, state: "Arrived" },
    { date: "30 Oct", cents: 1520000, state: "Arrived" },
  ],
};

const ledgerRows = [
  { t: "21:46", what: "Payment pay_7Q4M2X", who: "cus_amelia.k", wallet: "Lightning", cents: 1800 },
  { t: "21:31", what: "Payment pay_3N8T5B", who: "cus_tomasz.w", wallet: "On-chain", cents: 19200 },
  { t: "20:05", what: "Payment pay_1B9X6E", who: "cus_jonas.b", wallet: "Bank transfer", cents: 10000 },
  { t: "19:20", what: "Refund of pay_4R7F3A", who: "cus_ravi.d", wallet: "Card", cents: -14900 },
  { t: "16:15", what: "Payment pay_2W8Q7N", who: "cus_sofia.r", wallet: "Lightning", cents: 1800 },
  { t: "11:48", what: "Held for dispute on pay_3E6U2T", who: "cus_yuki.t", wallet: "Card", cents: -14900 },
  { t: "09:10", what: "Payment pay_9P1Z5V", who: "cus_marta.l", wallet: "Bank transfer", cents: 19200 },
  { t: "06:00", what: "Card and PayPal payments from 11 Nov released", who: "31 payments", wallet: "Card, PayPal", cents: 128640 },
];
export const ledger = (() => {
  let bal = available;
  return ledgerRows.map(r => {
    const row = { ...r, balance: bal };
    bal -= r.cents;
    return row;
  });
})();
export const openingToday = available - ledgerRows.reduce((a, r) => a + r.cents, 0);

export const availableWhen = {
  paid: { lightning: "Now", onchain: "Now", card: "25 Nov", paypal: "25 Nov", bank: "19 Nov" },
};

export const queue = [
  { group: "Disputes", tone: "bad", rows: [
    { id: "pay_3E6U2T", desc: "Dedicated host, 1 month", who: "cus_yuki.t", method: "Card", cents: 14900, reason: "Customer says the service was not received", due: "Respond by 24 Nov", dueSoon: true, act: "Respond" },
  ] },
  { group: "Paid the wrong amount or late", tone: "warn", rows: [
    { id: "pay_7J3H1N", desc: "VPS Frankfurt-2, 3 months", who: "cus_bram.d", method: "On-chain", cents: 5400, reason: "Paid partial, \u20ac15.80 short", due: "Expired 16:02", act: "Request the rest" },
    { id: "pay_2P9Q4R", desc: "VPS Dublin-3, 1 month", who: "cus_lena.k", method: "On-chain", cents: 1800, reason: "Paid late, 14 minutes after expiry", due: "Seen 13:20", act: "Accept" },
  ] },
  { group: "Waiting for confirmations", tone: "warn", rows: [
    { id: "pay_0Y5G9S", desc: "VPS Frankfurt-2, 3 months", who: "cus_elsa.v", method: "On-chain", cents: 5400, reason: "1 of 3 confirmations", due: "About 20 min", act: "Watch" },
  ] },
  { group: "Declined", tone: "bad", rows: [
    { id: "pay_8D3M1C", desc: "Credit top-up", who: "cus_omar.f", method: "Card", cents: 2500, reason: "Insufficient funds, not retried", due: "Since 17:30", act: "Send new link" },
  ] },
  { group: "Webhook endpoint failing", tone: "bad", rows: [
    { id: "whe_audit", desc: "ops.northwind.example/hooks/audit", who: "4 events", method: "Webhook", cents: null, reason: "Answers 503 since 20:41, retrying with backoff", due: "Next try 22:10", act: "Retry now" },
  ] },
];

export const fyi = [
  { t: "13 Nov", text: `Payout of ${eur(1840000)} arrived at AIB ending 4417` },
  { t: "Today 06:00", text: `${eur(128640)} from card and PayPal payments on 11 Nov became available` },
  { t: "Yesterday", text: "Lightning channel to ACINQ opened, 5,000,000 sats inbound" },
];

export const health = [
  { name: "Lightning node", state: "ok", note: "lnd-dublin-1 online, 12 channels" },
  { name: "Bitcoin node", state: "ok", note: "Synced at block 871,204" },
  { name: "Stripe", state: "ok", note: "Connected" },
  { name: "PayPal", state: "ok", note: "Connected" },
  { name: "Revolut", state: "warn", note: "Access token expires in 5 days" },
];

export { payments, detail, month, eur, sats };

export function money(cents) {
  const s = eur(cents);
  const i = s.lastIndexOf(".");
  return `${s.slice(0, i)}<small>${s.slice(i)}</small>`;
}

export function signed(cents) {
  return cents < 0 ? `\u2212${eur(-cents)}` : `+${eur(cents)}`;
}
