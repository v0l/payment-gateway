# Next.js checkout

`/checkout` creates a payment on the server through the JS client and opens the embedded pay page with `gateway.js`.

```
cd clients/js && npm install && npm run build && cd -
cd examples/nextjs && npm install
GATEWAY_URL=http://127.0.0.1:8080 NEXT_PUBLIC_GATEWAY_URL=http://127.0.0.1:8080 GATEWAY_API_KEY=pgw_... npm run dev
```

Add `http://localhost:3000` to the project's `allowed_origins` so the embed can be framed.
