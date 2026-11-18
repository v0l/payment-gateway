import http.server
import json
import os
import pathlib
import sys
import threading
import time
import unittest
import urllib.request

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from payment_gateway import Gateway, GatewayError, WebhookVerificationError, verify_webhook  # noqa: E402


class Receiver:
    def __init__(self):
        self.received = []
        outer = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                body = self.rfile.read(int(self.headers.get("content-length", 0)))
                outer.received.append((body, dict(self.headers)))
                self.send_response(200)
                self.send_header("content-length", "2")
                self.end_headers()
                self.wfile.write(b"ok")

            def log_message(self, format, *args):
                pass

        self.server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server.server_address[1]}/hook"

    def wait_for(self, payment_id, timeout=20):
        deadline = time.time() + timeout
        while time.time() < deadline:
            for body, headers in list(self.received):
                try:
                    if json.loads(body)["data"]["payment"]["id"] == payment_id:
                        return body, headers
                except (ValueError, KeyError):
                    pass
            time.sleep(0.2)
        raise AssertionError("timed out waiting for webhook")

    def close(self):
        self.server.shutdown()


def post(url, headers, payload):
    request = urllib.request.Request(url, data=json.dumps(payload).encode(), headers=headers, method="POST")
    with urllib.request.urlopen(request) as r:
        return r.status


@unittest.skipUnless(os.environ.get("GATEWAY_URL"), "GATEWAY_URL not set")
class Conformance(unittest.TestCase):
    def test_scenarios(self):
        env = os.environ
        url = env["GATEWAY_URL"]
        rx = Receiver()
        self.addCleanup(rx.close)
        status = post(
            f"{url}/admin/tenants/{env['GATEWAY_TENANT_ID']}/settings",
            {"authorization": f"Bearer {env['GATEWAY_ADMIN_TOKEN']}", "content-type": "application/json"},
            {"webhook_url": rx.url},
        )
        self.assertEqual(status, 200)

        gw = Gateway(url, env["GATEWAY_API_KEY"])
        reference = f"py-{int(time.time() * 1000)}"
        key = f"key-{reference}"

        created = gw.create_payment(idempotency_key=key, reference=reference, amount=1000, currency="EUR")
        self.assertEqual(created["status"], "pending")
        replay = gw.create_payment(idempotency_key=key, reference=reference, amount=1000, currency="EUR")
        self.assertEqual(replay["id"], created["id"])
        with self.assertRaises(GatewayError) as ctx:
            gw.create_payment(idempotency_key=key, reference=reference, amount=2000, currency="EUR")
        self.assertEqual(ctx.exception.status, 409)

        with_attempt = gw.create_attempt(created["id"], "lightning")
        lookup = with_attempt["attempts"][0]["details"]["payment_hash"]
        post(
            f"{url}/hook/{env['GATEWAY_TENANT_ID']}/{env['GATEWAY_PROVIDER_ID']}",
            {"x-mock-secret": env["GATEWAY_MOCK_SECRET"]},
            {"lookup": lookup},
        )

        body, headers = rx.wait_for(created["id"])
        secret = env["GATEWAY_WEBHOOK_SECRET"]
        event = verify_webhook(body, headers, secret, 300)
        self.assertEqual(event["type"], "payment.paid")
        with self.assertRaises(WebhookVerificationError):
            verify_webhook(body + b" ", headers, secret, 300)
        with self.assertRaises(WebhookVerificationError):
            verify_webhook(body, headers, "whsec_wrong", 300)

        self.assertEqual(gw.get_payment(created["id"])["status"], "paid")
        refund = gw.refund_payment(created["id"], amount=500)
        self.assertEqual(refund["status"], "completed")
        self.assertEqual([p["id"] for p in gw.list_payments(reference=reference, limit=1)], [created["id"]])


if __name__ == "__main__":
    unittest.main()
