from __future__ import annotations

import hashlib
import hmac
import json
import time
from typing import Any, Mapping, Optional, Union

SIGNATURE_HEADER = "x-gateway-signature"
DEFAULT_TOLERANCE_SECONDS = 300


class WebhookVerificationError(Exception):
    def __init__(self, reason: str, message: str):
        super().__init__(message)
        self.reason = reason


def sign_webhook(raw_body: Union[str, bytes], secret: str, timestamp: int) -> str:
    body = raw_body.encode("utf-8") if isinstance(raw_body, str) else raw_body
    mac = hmac.new(secret.encode("utf-8"), f"{timestamp}.".encode("ascii") + body, hashlib.sha256)
    return mac.hexdigest()


def _header(headers: Any, name: str) -> Optional[str]:
    items = headers.items() if hasattr(headers, "items") else headers
    for key, value in items:
        if key.lower() == name:
            if isinstance(value, (list, tuple)):
                return value[0] if value else None
            return value
    return None


def verify_webhook(
    raw_body: Union[str, bytes],
    headers: Any,
    secret: str,
    tolerance_seconds: int = DEFAULT_TOLERANCE_SECONDS,
    now: Optional[float] = None,
) -> dict:
    header = _header(headers, SIGNATURE_HEADER)
    if not header:
        raise WebhookVerificationError("missing_header", f"missing {SIGNATURE_HEADER} header")
    timestamp: Optional[int] = None
    signatures = []
    for part in header.split(","):
        key, _, value = part.strip().partition("=")
        if key == "t" and value.isdigit():
            timestamp = int(value)
        elif key == "v1" and value:
            signatures.append(value)
    if timestamp is None or not signatures:
        raise WebhookVerificationError("malformed_header", "malformed signature header")
    expected = sign_webhook(raw_body, secret, timestamp)
    matched = False
    for candidate in signatures:
        if hmac.compare_digest(candidate.encode("utf-8"), expected.encode("utf-8")):
            matched = True
    if not matched:
        raise WebhookVerificationError("signature_mismatch", "signature does not match")
    current = time.time() if now is None else now
    if tolerance_seconds > 0 and abs(current - timestamp) > tolerance_seconds:
        raise WebhookVerificationError("timestamp_outside_tolerance", "timestamp outside tolerance")
    text = raw_body.decode("utf-8") if isinstance(raw_body, bytes) else raw_body
    return json.loads(text)
