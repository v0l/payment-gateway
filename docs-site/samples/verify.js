const crypto = require("node:crypto");

function verify(secret, header, body, nowSeconds, toleranceSeconds = 300) {
  const parts = Object.fromEntries(header.split(",").map((p) => p.split("=")));
  const timestamp = Number(parts.t);
  if (!Number.isFinite(timestamp) || !parts.v1) return false;
  if (Math.abs(nowSeconds - timestamp) > toleranceSeconds) return false;
  const expected = crypto.createHmac("sha256", secret).update(`${timestamp}.${body}`).digest();
  const given = Buffer.from(parts.v1, "hex");
  return given.length === expected.length && crypto.timingSafeEqual(given, expected);
}

module.exports = { verify };
