#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
out=tests/fixtures
did=0

if [ -n "${STRIPE_TEST_KEY:-}" ]; then
  case "$STRIPE_TEST_KEY" in sk_test_*) ;; *) echo "STRIPE_TEST_KEY must be an sk_test_ key" >&2; exit 1 ;; esac
  stripe() { curl -sS -u "$STRIPE_TEST_KEY:" "$@"; }
  api=https://api.stripe.com/v1
  mkdir -p "$out/stripe"

  session=$(stripe "$api/checkout/sessions" \
    -d mode=payment -d "client_reference_id=fixture" \
    -d "success_url=https://example.com/ok" -d "cancel_url=https://example.com/cancel" \
    -d "line_items[0][quantity]=1" -d "line_items[0][price_data][currency]=eur" \
    -d "line_items[0][price_data][unit_amount]=1000" \
    -d "line_items[0][price_data][product_data][name]=fixture")
  echo "$session" | jq . > "$out/stripe/checkout_session.json"
  session_id=$(echo "$session" | jq -r .id)
  stripe "$api/checkout/sessions/$session_id" | jq . > "$out/stripe/session_retrieve.json"

  customer=$(stripe "$api/customers" -d email=fixture@example.com | jq -r .id)
  pm=$(stripe "$api/payment_methods/pm_card_visa/attach" -d "customer=$customer" | jq -r .id)
  intent=$(stripe "$api/payment_intents" -d amount=1000 -d currency=eur -d "customer=$customer" \
    -d "payment_method=$pm" -d confirm=true -d off_session=true)
  echo "$intent" | jq . > "$out/stripe/off_session_intent.json"
  stripe "$api/refunds" -d "payment_intent=$(echo "$intent" | jq -r .id)" | jq . > "$out/stripe/refund.json"

  declined_pm=$(stripe "$api/payment_methods/pm_card_chargeCustomerFail/attach" -d "customer=$customer" | jq -r .id)
  stripe "$api/payment_intents" -d amount=1000 -d currency=eur -d "customer=$customer" \
    -d "payment_method=$declined_pm" -d confirm=true -d off_session=true | jq . > "$out/stripe/card_declined.json"
  did=1
fi

if [ -n "${REVOLUT_SANDBOX_TOKEN:-}" ]; then
  revolut() {
    curl -sS -H "Authorization: Bearer $REVOLUT_SANDBOX_TOKEN" -H "Revolut-Api-Version: 2024-09-01" \
      -H "Content-Type: application/json" "$@"
  }
  api=https://sandbox-merchant.revolut.com/api
  mkdir -p "$out/revolut"
  order=$(revolut "$api/orders" -d '{"amount":1000,"currency":"EUR","merchant_order_data":{"reference":"fixture"}}')
  echo "$order" | jq . > "$out/revolut/order_created.json"
  revolut "$api/orders/$(echo "$order" | jq -r .id)" | jq . > "$out/revolut/order_pending.json"
  did=1
fi

if [ "$did" = 0 ]; then
  echo "set STRIPE_TEST_KEY (sk_test_...) and/or REVOLUT_SANDBOX_TOKEN" >&2
  exit 1
fi

echo "recorded. review the diff, then run: cargo test --test providers_e2e"
echo "completed orders, refunds and webhook bodies need a paid sandbox order and are edited by hand"
