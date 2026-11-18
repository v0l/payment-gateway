import { NextResponse } from "next/server";
import { Gateway } from "@payment-gateway/client";

const gw = new Gateway({
  baseUrl: process.env.GATEWAY_URL ?? "http://127.0.0.1:8080",
  apiKey: process.env.GATEWAY_API_KEY ?? "",
});

export async function POST() {
  const reference = `order-${Date.now()}`;
  const payment = await gw.createPayment(
    { reference, amount: 1999, currency: "EUR" },
    { idempotencyKey: reference },
  );
  return NextResponse.json({ id: payment.id });
}
