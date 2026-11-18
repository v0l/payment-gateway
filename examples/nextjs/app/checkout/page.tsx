"use client";

import Script from "next/script";
import { useState } from "react";

const gatewayUrl = process.env.NEXT_PUBLIC_GATEWAY_URL ?? "http://127.0.0.1:8080";

export default function Checkout() {
  const [ready, setReady] = useState(false);
  const [status, setStatus] = useState("");

  async function pay() {
    const res = await fetch("/api/checkout", { method: "POST" });
    const { id } = await res.json();
    (window as any).PaymentGateway.open({
      gateway: gatewayUrl,
      paymentId: id,
      container: "#pay",
      onPaid: () => setStatus("Paid, thank you"),
    });
  }

  return (
    <main>
      <Script src={`${gatewayUrl}/gateway.js`} onReady={() => setReady(true)} />
      <button disabled={!ready} onClick={pay}>
        Pay 19.99 EUR
      </button>
      <p>{status}</p>
      <div id="pay" />
    </main>
  );
}
