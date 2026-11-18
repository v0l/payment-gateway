import test from "node:test";
import assert from "node:assert/strict";
import { Gateway, GatewayError } from "../dist/esm/index.js";

function fakeFetch(responses) {
  const calls = [];
  const fn = async (url, init) => {
    calls.push({ url, init });
    const r = responses.shift();
    if (r instanceof Error) throw r;
    return new Response(r.body === undefined ? "" : JSON.stringify(r.body), { status: r.status, headers: r.headers });
  };
  fn.calls = calls;
  return fn;
}

function client(responses, extra = {}) {
  const f = fakeFetch(responses);
  const sleeps = [];
  const gw = new Gateway({ baseUrl: "http://gw/", apiKey: "k", fetch: f, sleep: async (ms) => sleeps.push(ms), ...extra });
  return { gw, f, sleeps };
}

test("retries 429 honoring retry-after, then succeeds", async () => {
  const { gw, f, sleeps } = client([
    { status: 429, body: { error: "slow" }, headers: { "retry-after": "2" } },
    { status: 200, body: { id: "p" } },
  ]);
  assert.equal((await gw.getPayment("p")).id, "p");
  assert.equal(f.calls.length, 2);
  assert.deepEqual(sleeps, [2000]);
});

test("retries 5xx up to the limit and then throws GatewayError", async () => {
  const { gw, f } = client([1, 2, 3, 4].map(() => ({ status: 503, body: { error: "down" } })), { maxRetries: 2 });
  await assert.rejects(gw.getPayment("p"), (e) => e instanceof GatewayError && e.status === 503);
  assert.equal(f.calls.length, 3);
});

test("create reuses one idempotency key across retries", async () => {
  const { gw, f } = client([
    { status: 500, body: { error: "x" } },
    { status: 201, body: { id: "p" } },
  ]);
  await gw.createPayment({ amount: 1, currency: "EUR" });
  const keys = f.calls.map((c) => c.init.headers["idempotency-key"]);
  assert.ok(keys[0]);
  assert.equal(keys[0], keys[1]);
});

test("does not retry 4xx and exposes the error detail", async () => {
  const { gw, f } = client([
    { status: 400, body: { error: "bad", error_detail: { code: "invalid_request", message: "bad", field: "amount" } } },
  ]);
  await assert.rejects(gw.createPayment({ amount: 0, currency: "EUR" }), (e) => e.code === "invalid_request" && e.field === "amount");
  assert.equal(f.calls.length, 1);
});

test("does not retry a non-idempotent post on 500", async () => {
  const { gw, f } = client([{ status: 500, body: { error: "x" } }]);
  await assert.rejects(gw.cancelPayment("p"), GatewayError);
  assert.equal(f.calls.length, 1);
});

test("pagination iterator follows x-next-cursor", async () => {
  const { gw, f } = client([
    { status: 200, body: [{ id: "a" }], headers: { "x-next-cursor": "c1" } },
    { status: 200, body: [{ id: "b" }] },
  ]);
  const ids = [];
  for await (const p of gw.listPayments({ limit: 1 })) ids.push(p.id);
  assert.deepEqual(ids, ["a", "b"]);
  assert.match(f.calls[1].url, /cursor=c1/);
});
