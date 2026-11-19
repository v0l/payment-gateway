import { Btn, SkipLink } from '../shared/components';
import { LANDING_TABS, SiteFooter, TopBar } from './layout';

const curl = `curl $GW/v1/payments -H "Authorization: Bearer $KEY" \\
  -H "Idempotency-Key: order-123" \\
  -d '{"reference":"order-123","amount":1800,"currency":"EUR"}'`;

export const LANDING_DESCRIPTION =
  'A payment gateway for apps. Ask it to collect money by Lightning, on-chain, card, bank or PayPal and it tells you when it is paid.';

export function Landing() {
  return (
    <>
      <SkipLink target="#content" />
      <TopBar tabs={LANDING_TABS} />
      <main id="content" class="home">
        <section class="hero" aria-labelledby="hero-title">
          <div>
            <h1 id="hero-title">Ask for money. Get told when it lands.</h1>
            <p class="lede">
              Your app creates a payment. The customer pays by Lightning, on-chain Bitcoin or card, and the gateway sends your app one signed webhook when the money has arrived. You can keep the funds in your own wallets or let the gateway hold them for you.
            </p>
            <div class="actions">
              <Btn href="/guide/quickstart-api" primary big>Take a first payment</Btn>
              <Btn href="/console" big>Open the console</Btn>
            </div>
          </div>
          <aside class="record" aria-label="A paid payment as the console shows it">
            <div class="record-head">
              <div class="who">
                <span>Northwind Hosting</span>
                <span class="pill">Paid</span>
              </div>
              <div class="figure">
                €18<small>.00</small>
              </div>
              <p>VPS Dublin-3, 1 month. Paid by Lightning, 20,023 sats.</p>
            </div>
            <ol>
              <li><time>14:02</time><div><b><span class="dot" />Payment created</b><span>Pay link sent to the customer</span></div></li>
              <li><time>14:03</time><div><b><span class="dot" />Lightning invoice shown</b><span>Customer scans the QR code</span></div></li>
              <li><time>14:03</time><div><b><span class="dot ok" />Paid</b><span>Settled once, balance credited</span></div></li>
              <li><time>14:03</time><div><b><span class="dot ok" />Webhook delivered</b><span>payment.paid, signed, answered 200</span></div></li>
            </ol>
          </aside>
        </section>

        <section class="band" aria-labelledby="what-title">
          <header>
            <h2 id="what-title">What your customers can pay with</h2>
            <p>Turn methods on per project. The pay page offers the ones you enabled.</p>
          </header>
          <dl class="rows">
            <div><dt>Lightning</dt><dd><span class="sub">Settles instantly.</span>Your own LND node, or the platform node if you have none.</dd></div>
            <div><dt>On-chain Bitcoin</dt><dd><span class="sub">A fresh address for every attempt.</span>Confirmations are counted for you, and short payments can be topped up on the same address.</dd></div>
            <div><dt>Card through Revolut</dt><dd><span class="sub">Hosted checkout, refunds and disputes.</span>Stripe and PayPal work the same way. Saved cards can be charged for subscriptions.</dd></div>
            <div><dt>Self-custody or held for you</dt><dd><span class="sub">You choose per project.</span>Connect your own providers and funds go straight to you, or use shared platform wallets and withdraw the balance when you want it.</dd></div>
          </dl>
        </section>

        <section class="band" aria-labelledby="how-title">
          <header>
            <h2 id="how-title">How it works</h2>
            <p>One payment object from first request to receipt.</p>
          </header>
          <ol class="rows steps">
            <li>
              <div>
                <h3>Create a payment</h3>
                <p>Send an amount and a reference with an idempotency key, so a retry never charges twice.</p>
                <pre class="code"><code>{curl}</code></pre>
              </div>
            </li>
            <li>
              <div>
                <h3>The customer pays</h3>
                <p>Send them to the hosted pay page, embed it in an iframe, or draw your own screen from the API.</p>
              </div>
            </li>
            <li>
              <div>
                <h3>Your app is told</h3>
                <p>A signed webhook arrives once the payment settles, with three days of retries if your server is down. The receipt link is on the payment.</p>
              </div>
            </li>
          </ol>
        </section>

        <section class="band" aria-labelledby="run-title">
          <header>
            <h2 id="run-title">Use it hosted, or run your own</h2>
            <p>The same code either way.</p>
          </header>
          <div class="split">
            <div>
              <h3>Sign up and build</h3>
              <p>The console has projects, API keys, payment methods, webhooks, customers, subscriptions, balance and payouts. New projects start in test mode with low limits.</p>
              <Btn href="/console">Sign in</Btn>
            </div>
            <div>
              <h3>Deploy it yourself</h3>
              <p>One binary and a MariaDB database. The operator guide covers LND, backups, key rotation, monitoring and runbooks.</p>
              <Btn href="/operator/">Read the operator guide</Btn>
            </div>
          </div>
        </section>

        <section id="pricing" class="band pricing" aria-labelledby="pricing-title">
          <header>
            <h2 id="pricing-title">Pricing</h2>
            <p>Free while you build.</p>
          </header>
          <div class="rows">
            <div>
              <h3>Test mode</h3>
              <p>Every new project starts here at no cost, with low limits, so you can finish an integration before any money moves.</p>
            </div>
            <div>
              <h3>Live</h3>
              <p>Live prices have not been published yet. Plans are a monthly fee plus a fee on volume, and they are set per project. Ask the operator for an invite and the current terms.</p>
            </div>
          </div>
        </section>

        <nav class="next" aria-label="Where to go next">
          <a href="/guide/">Documentation<span>Concepts, quickstarts, webhooks, going live</span></a>
          <a href="/docs">API reference<span>Every /v1 endpoint, with search</span></a>
          <a href="/operator/">Operator guide<span>Deploy and run your own gateway</span></a>
          <a href="/console">Console<span>Projects, keys, payments, balance</span></a>
        </nav>
      </main>
      <SiteFooter />
    </>
  );
}
