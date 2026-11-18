import express from "express";
import { Gateway, GatewayError, verifyWebhook, WebhookVerificationError } from "@payment-gateway/client";

const gatewayUrl = process.env.GATEWAY_URL ?? "http://127.0.0.1:8080";
const gw = new Gateway({ baseUrl: gatewayUrl, apiKey: process.env.GATEWAY_API_KEY ?? "" });
const webhookSecret = process.env.GATEWAY_WEBHOOK_SECRET ?? "";
const port = Number(process.env.PORT ?? 3000);

const app = express();

app.post("/webhook", express.raw({ type: "*/*" }), (req, res) => {
  try {
    const event = verifyWebhook(req.body, req.headers, webhookSecret, 300);
    console.log("event", event.type, event.data.payment.reference);
    res.sendStatus(200);
  } catch (e) {
    if (e instanceof WebhookVerificationError) return res.sendStatus(400);
    throw e;
  }
});

app.get("/", (_req, res) => {
  res.type("html").send(`<form method="post" action="/checkout"><button>Pay 19.99 EUR</button></form>`);
});

app.post("/checkout", async (req, res) => {
  const reference = `order-${Date.now()}`;
  try {
    const payment = await gw.createPayment(
      { reference, amount: 1999, currency: "EUR", description: "Example order" },
      { idempotencyKey: reference },
    );
    res.redirect(303, payment.pay_url);
  } catch (e) {
    if (e instanceof GatewayError) return res.status(502).send(`${e.code}: ${e.message}`);
    throw e;
  }
});

app.listen(port, () => console.log(`listening on ${port}`));
