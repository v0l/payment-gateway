# Express checkout

A button that creates a payment and redirects to the hosted pay page, plus a webhook endpoint that verifies signatures.

```
cd clients/js && npm install && npm run build && cd -
cd examples/express-checkout && npm install
GATEWAY_URL=http://127.0.0.1:8080 GATEWAY_API_KEY=pgw_... GATEWAY_WEBHOOK_SECRET=whsec_... npm start
```

Set the project webhook URL to `http://<host>:3000/webhook`.
