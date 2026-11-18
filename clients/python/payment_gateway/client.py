from __future__ import annotations

import json
import random
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from typing import Any, Callable, Dict, Iterator, Optional, Tuple

VERSION = "0.1.0"


class GatewayError(Exception):
    def __init__(self, status: int, body: Any, retry_after: Optional[int] = None):
        body = body if isinstance(body, dict) else {"error": str(body)}
        detail = body.get("error_detail") or {}
        super().__init__(detail.get("message") or body.get("error") or f"request failed with status {status}")
        self.status = status
        self.code = detail.get("code") or body.get("code") or "error"
        self.field = detail.get("field")
        self.body = body
        self.retry_after = retry_after


class NetworkError(Exception):
    pass


Transport = Callable[[str, str, Dict[str, str], Optional[bytes], float], Tuple[int, Dict[str, str], bytes]]


def urllib_transport(method, url, headers, body, timeout):
    request = urllib.request.Request(url, data=body, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.status, {k.lower(): v for k, v in response.headers.items()}, response.read()
    except urllib.error.HTTPError as e:
        return e.code, {k.lower(): v for k, v in e.headers.items()}, e.read()
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        raise NetworkError(str(e)) from e


def httpx_transport(method, url, headers, body, timeout):
    import httpx

    try:
        r = httpx.request(method, url, headers=headers, content=body, timeout=timeout)
    except httpx.HTTPError as e:
        raise NetworkError(str(e)) from e
    return r.status_code, {k.lower(): v for k, v in r.headers.items()}, r.content


def new_idempotency_key() -> str:
    return str(uuid.uuid4())


class Gateway:
    def __init__(
        self,
        base_url: str,
        api_key: str,
        *,
        max_retries: int = 3,
        retry_base_delay: float = 0.25,
        retry_max_delay: float = 8.0,
        timeout: float = 30.0,
        transport: Optional[Transport] = None,
        sleep: Callable[[float], None] = time.sleep,
    ):
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.max_retries = max_retries
        self.retry_base_delay = retry_base_delay
        self.retry_max_delay = retry_max_delay
        self.timeout = timeout
        self.transport = transport or urllib_transport
        self.sleep = sleep

    def _delay(self, attempt: int, retry_after: Optional[int]) -> float:
        if retry_after is not None:
            return min(float(retry_after), self.retry_max_delay)
        exp = min(self.retry_base_delay * 2**attempt, self.retry_max_delay)
        return exp / 2 + random.random() * exp / 2

    def _send(self, method: str, path: str, body: Any = None, idempotency_key: Optional[str] = None) -> Tuple[Any, Dict[str, str]]:
        headers = {
            "authorization": f"Bearer {self.api_key}",
            "accept": "application/json",
            "user-agent": f"payment-gateway-python/{VERSION}",
        }
        payload = None
        if body is not None:
            headers["content-type"] = "application/json"
            payload = json.dumps(body).encode("utf-8")
        if idempotency_key:
            headers["idempotency-key"] = idempotency_key
        attempt = 0
        while True:
            try:
                status, rsp_headers, raw = self.transport(method, self.base_url + path, headers, payload, self.timeout)
            except NetworkError:
                if attempt < self.max_retries and (method == "GET" or idempotency_key):
                    self.sleep(self._delay(attempt, None))
                    attempt += 1
                    continue
                raise
            if 200 <= status < 300:
                return (json.loads(raw) if raw else None), rsp_headers
            try:
                parsed = json.loads(raw) if raw else {}
            except ValueError:
                parsed = {"error": raw.decode("utf-8", "replace")}
            retry_after = rsp_headers.get("retry-after")
            retry_after = int(retry_after) if retry_after and retry_after.isdigit() else None
            safe = method in ("GET", "DELETE") or idempotency_key is not None or status == 429
            if (status == 429 or 500 <= status <= 599) and safe and attempt < self.max_retries:
                self.sleep(self._delay(attempt, retry_after))
                attempt += 1
                continue
            raise GatewayError(status, parsed, retry_after)

    def _page(self, path: str, params: Dict[str, Any]) -> Tuple[Any, Optional[str]]:
        query = urllib.parse.urlencode({k: v for k, v in params.items() if v is not None})
        items, headers = self._send("GET", path + (f"?{query}" if query else ""))
        return items, headers.get("x-next-cursor")

    def _iterate(self, path: str, params: Dict[str, Any]) -> Iterator[Any]:
        params = dict(params)
        while True:
            items, cursor = self._page(path, params)
            yield from items
            if not cursor:
                return
            params["cursor"] = cursor

    def create_payment(self, idempotency_key: Optional[str] = None, **fields) -> dict:
        key = idempotency_key or new_idempotency_key()
        return self._send("POST", "/v1/payments", fields, key)[0]

    def get_payment(self, payment_id: str) -> dict:
        return self._send("GET", f"/v1/payments/{urllib.parse.quote(payment_id)}")[0]

    def create_attempt(self, payment_id: str, method: str, **fields) -> dict:
        return self._send("POST", f"/v1/payments/{urllib.parse.quote(payment_id)}/attempts", {"method": method, **fields})[0]

    def cancel_payment(self, payment_id: str) -> dict:
        return self._send("POST", f"/v1/payments/{urllib.parse.quote(payment_id)}/cancel", {})[0]

    def refund_payment(self, payment_id: str, **fields) -> dict:
        return self._send("POST", f"/v1/payments/{urllib.parse.quote(payment_id)}/refund", fields)[0]

    def list_payments_page(self, **params) -> Tuple[list, Optional[str]]:
        return self._page("/v1/payments", params)

    def list_payments(self, **params) -> Iterator[dict]:
        return self._iterate("/v1/payments", params)

    def list_refunds(self, **params) -> Iterator[dict]:
        return self._iterate("/v1/refunds", params)

    def list_events(self, **params) -> Iterator[dict]:
        return self._iterate("/v1/events", params)

    def list_methods(self, **params) -> Iterator[dict]:
        return self._iterate("/v1/methods", params)

    def list_saved_methods(self, **params) -> Iterator[dict]:
        return self._iterate("/v1/saved-methods", params)

    def delete_saved_method(self, saved_method_id: int) -> None:
        self._send("DELETE", f"/v1/saved-methods/{saved_method_id}")

    def charge_saved_method(self, saved_method_id: int, idempotency_key: Optional[str] = None, **fields) -> dict:
        key = idempotency_key or new_idempotency_key()
        return self._send("POST", f"/v1/saved-methods/{saved_method_id}/charge", fields, key)[0]
