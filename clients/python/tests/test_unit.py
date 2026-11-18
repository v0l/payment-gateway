import json
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from payment_gateway import (  # noqa: E402
    Gateway,
    GatewayError,
    NetworkError,
    WebhookVerificationError,
    sign_webhook,
    verify_webhook,
)

VECTORS = json.loads((pathlib.Path(__file__).resolve().parents[2] / "test-vectors" / "webhook.json").read_text())
HEADER = VECTORS["header"]


class WebhookTests(unittest.TestCase):
    def test_signatures_match_server_vectors(self):
        for v in VECTORS["vectors"]:
            self.assertEqual(sign_webhook(v["body"], v["secret"], v["timestamp"]), v["signature"])

    def test_verifies_server_vectors(self):
        for v in VECTORS["vectors"]:
            if v["body"]:
                verify_webhook(v["body"].encode(), {"X-Gateway-Signature": v["header"]}, v["secret"], 300, v["timestamp"] + 5)

    def test_rejects_tampering(self):
        v = VECTORS["vectors"][0]
        headers = {HEADER: v["header"]}
        now = v["timestamp"]
        with self.assertRaises(WebhookVerificationError):
            verify_webhook(v["body"] + " ", headers, v["secret"], 300, now)
        with self.assertRaises(WebhookVerificationError):
            verify_webhook(v["body"], headers, "wrong", 300, now)
        replay = v["header"].replace(f"t={v['timestamp']}", f"t={v['timestamp'] + 1}")
        with self.assertRaises(WebhookVerificationError):
            verify_webhook(v["body"], {HEADER: replay}, v["secret"], 300, now)

    def test_tolerance(self):
        v = VECTORS["vectors"][0]
        headers = {HEADER: v["header"]}
        t = v["timestamp"]
        with self.assertRaises(WebhookVerificationError) as ctx:
            verify_webhook(v["body"], headers, v["secret"], 300, t + 301)
        self.assertEqual(ctx.exception.reason, "timestamp_outside_tolerance")
        with self.assertRaises(WebhookVerificationError):
            verify_webhook(v["body"], headers, v["secret"], 300, t - 301)
        verify_webhook(v["body"], headers, v["secret"], 300, t + 300)
        verify_webhook(v["body"], headers, v["secret"], 0, t + 10**6)

    def test_missing_and_malformed(self):
        v = VECTORS["vectors"][0]
        with self.assertRaises(WebhookVerificationError) as ctx:
            verify_webhook(v["body"], {}, v["secret"])
        self.assertEqual(ctx.exception.reason, "missing_header")
        with self.assertRaises(WebhookVerificationError) as ctx:
            verify_webhook(v["body"], {HEADER: "t=abc,v1=00"}, v["secret"])
        self.assertEqual(ctx.exception.reason, "malformed_header")


def scripted(responses):
    calls = []

    def transport(method, url, headers, body, timeout):
        calls.append((method, url, headers, body))
        r = responses.pop(0)
        if isinstance(r, Exception):
            raise r
        status, payload, extra = r
        return status, extra, json.dumps(payload).encode()

    return transport, calls


class ClientTests(unittest.TestCase):
    def make(self, responses, **kw):
        transport, calls = scripted(responses)
        sleeps = []
        gw = Gateway("http://gw/", "k", transport=transport, sleep=sleeps.append, **kw)
        return gw, calls, sleeps

    def test_retry_after_on_429(self):
        gw, calls, sleeps = self.make([(429, {"error": "slow"}, {"retry-after": "2"}), (200, {"id": "p"}, {})])
        self.assertEqual(gw.get_payment("p")["id"], "p")
        self.assertEqual(sleeps, [2.0])
        self.assertEqual(len(calls), 2)

    def test_gives_up_on_5xx(self):
        gw, calls, _ = self.make([(503, {"error": "x"}, {})] * 3, max_retries=2)
        with self.assertRaises(GatewayError) as ctx:
            gw.get_payment("p")
        self.assertEqual(ctx.exception.status, 503)
        self.assertEqual(len(calls), 3)

    def test_idempotency_key_stable_across_retries(self):
        gw, calls, _ = self.make([(500, {"error": "x"}, {}), (201, {"id": "p"}, {})])
        gw.create_payment(amount=1, currency="EUR")
        keys = [c[2]["idempotency-key"] for c in calls]
        self.assertTrue(keys[0])
        self.assertEqual(keys[0], keys[1])

    def test_error_detail_and_no_retry_on_4xx(self):
        body = {"error": "bad", "error_detail": {"code": "invalid_request", "message": "bad", "field": "amount"}}
        gw, calls, _ = self.make([(400, body, {})])
        with self.assertRaises(GatewayError) as ctx:
            gw.create_payment(amount=0, currency="EUR")
        self.assertEqual((ctx.exception.code, ctx.exception.field), ("invalid_request", "amount"))
        self.assertEqual(len(calls), 1)

    def test_no_retry_for_unkeyed_post(self):
        gw, calls, _ = self.make([(500, {"error": "x"}, {})])
        with self.assertRaises(GatewayError):
            gw.cancel_payment("p")
        self.assertEqual(len(calls), 1)

    def test_network_error_retried_for_get(self):
        gw, calls, _ = self.make([NetworkError("down"), (200, {"id": "p"}, {})])
        self.assertEqual(gw.get_payment("p")["id"], "p")

    def test_pagination(self):
        gw, calls, _ = self.make([(200, [{"id": "a"}], {"x-next-cursor": "c1"}), (200, [{"id": "b"}], {})])
        self.assertEqual([p["id"] for p in gw.list_payments(limit=1)], ["a", "b"])
        self.assertIn("cursor=c1", calls[1][1])


if __name__ == "__main__":
    unittest.main()
