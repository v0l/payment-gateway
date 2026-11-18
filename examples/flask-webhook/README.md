# Flask webhook receiver

```
cd examples/flask-webhook
pip install -r requirements.txt
GATEWAY_WEBHOOK_SECRET=whsec_... python app.py
```

Point the project webhook URL at `http://<host>:3000/webhook` (set `PORT` to change the port). Bad signatures get a 400.
