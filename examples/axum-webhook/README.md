# axum webhook receiver

```
cd examples/axum-webhook
GATEWAY_WEBHOOK_SECRET=whsec_... cargo run
```

Point the project webhook URL at `http://<host>:3000/webhook`. Bad signatures get a 400.
