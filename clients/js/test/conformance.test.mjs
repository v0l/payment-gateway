import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { Gateway, GatewayError, verifyWebhook, WebhookVerificationError } from "../dist/esm/index.js";

const env = (name) => {
  const v = process.env[name];
  if (!v) throw new Error(`${name} is required`);
  return v;
};

const skip = process.env.GATEWAY_URL ? false : "GATEWAY_URL not set";

function receiver() {
  const received = [];
  const waiters = [];
  const server = http.createServer((req, res) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const entry = { body: Buffer.concat(chunks), headers: req.headers };
      received.push(entry);
      waiters.splice(0).forEach((w) => w());
      res.end("ok");
    });
  });
  return {
    received,
    server,
    async waitFor(predicate, ms = 20000) {
      const deadline = Date.now() + ms;
      for (;;) {
        const hit = received.find(predicate);
        if (hit) return hit;
        if (Date.now() > deadline) throw new Error("timed out waiting for webhook");
        await new Promise((r) => {
          waiters.push(r);
          setTimeout(r, 200);
        });
      }
    },
  };
}

test("conformance", { skip }, async (t) => {
  const url = env("GATEWAY_URL");
  const secret = env("GATEWAY_WEBHOOK_SECRET");
  const rx = receiver();
  await new Promise((r) => rx.server.listen(0, "127.0.0.1", r));
  t.after(() => rx.server.close());
  const hookUrl = `http://127.0.0.1:${rx.server.address().port}/hook`;
  const admin = await fetch(`${url}/admin/tenants/${env("GATEWAY_TENANT_ID")}/settings`, {
    method: "POST",
    headers: { authorization: `Bearer ${env("GATEWAY_ADMIN_TOKEN")}`, "content-type": "application/json" },
    body: JSON.stringify({ webhook_url: hookUrl }),
  });
  assert.equal(admin.status, 200);

  const gw = new Gateway({ baseUrl: url, apiKey: env("GATEWAY_API_KEY") });
  const reference = `js-${Date.now()}`;
  const key = `key-${reference}`;

  const created = await gw.createPayment({ reference, amount: 1000, currency: "EUR" }, { idempotencyKey: key });
  assert.equal(created.status, "pending");

  const replay = await gw.createPayment({ reference, amount: 1000, currency: "EUR" }, { idempotencyKey: key });
  assert.equal(replay.id, created.id);

  await assert.rejects(
    gw.createPayment({ reference, amount: 2000, currency: "EUR" }, { idempotencyKey: key }),
    (e) => e instanceof GatewayError && e.status === 409,
  );

  const withAttempt = await gw.createAttempt(created.id, { method: "lightning" });
  const lookup = withAttempt.attempts[0].details.payment_hash;
  assert.ok(lookup);

  const hook = await fetch(`${url}/hook/${env("GATEWAY_TENANT_ID")}/${env("GATEWAY_PROVIDER_ID")}`, {
    method: "POST",
    headers: { "x-mock-secret": env("GATEWAY_MOCK_SECRET") },
    body: JSON.stringify({ lookup }),
  });
  assert.equal(hook.status, 200);

  const paidDelivery = await rx.waitFor((d) => {
    try {
      return JSON.parse(d.body.toString()).data.payment.id === created.id && d.headers["x-gateway-event-id"];
    } catch {
      return false;
    }
  });
  const event = verifyWebhook(paidDelivery.body, paidDelivery.headers, secret, 300);
  assert.equal(event.type, "payment.paid");
  assert.equal(event.data.payment.id, created.id);

  const tampered = Buffer.concat([paidDelivery.body, Buffer.from(" ")]);
  assert.throws(() => verifyWebhook(tampered, paidDelivery.headers, secret, 300), WebhookVerificationError);
  assert.throws(() => verifyWebhook(paidDelivery.body, paidDelivery.headers, "whsec_wrong", 300), WebhookVerificationError);

  assert.equal((await gw.getPayment(created.id)).status, "paid");

  const refund = await gw.refundPayment(created.id, { amount: 500 });
  assert.equal(refund.status, "completed");

  const listed = [];
  for await (const p of gw.listPayments({ reference, limit: 1 })) listed.push(p.id);
  assert.deepEqual(listed, [created.id]);

  await assert.rejects(
    gw.createWithdrawal({ amount: 5000000, destination: "not-an-invoice" }),
    (e) => e instanceof GatewayError && e.status === 400,
  );
  const withdrawals = [];
  for await (const w of gw.listWithdrawals()) withdrawals.push(w.id);
  assert.deepEqual(withdrawals, []);
});
