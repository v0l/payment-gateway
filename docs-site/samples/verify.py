import hashlib
import hmac


def verify(secret, header, body, now_seconds, tolerance_seconds=300):
    parts = dict(p.split("=", 1) for p in header.split(",") if "=" in p)
    try:
        timestamp = int(parts["t"])
        given = bytes.fromhex(parts["v1"])
    except (KeyError, ValueError):
        return False
    if abs(now_seconds - timestamp) > tolerance_seconds:
        return False
    signed = f"{timestamp}.{body}".encode()
    expected = hmac.new(secret.encode(), signed, hashlib.sha256).digest()
    return hmac.compare_digest(given, expected)
