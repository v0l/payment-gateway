import os

from flask import Flask, request

from payment_gateway import WebhookVerificationError, verify_webhook

app = Flask(__name__)
SECRET = os.environ["GATEWAY_WEBHOOK_SECRET"]


@app.post("/webhook")
def webhook():
    try:
        event = verify_webhook(request.get_data(), request.headers, SECRET, 300)
    except WebhookVerificationError:
        return "", 400
    print(event["type"], event["data"]["payment"]["reference"])
    return "", 200


if __name__ == "__main__":
    app.run(port=3000)
