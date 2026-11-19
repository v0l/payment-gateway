from PIL import Image, ImageDraw, ImageFont
import sys, os

root = sys.argv[1]
out = sys.argv[2]
v3 = os.path.join(root, "docs/design/screenshots")
r2 = os.path.join(root, "docs/design/directions/round2")
v4 = os.path.join(root, "docs/design/screenshots/console-v4")
pages = [
    ("home", "console-v3-after-dashboard-1280.png", ["f-overview-1440.png", "d-overview-1440.png"], "home-1440.png"),
    ("payments", "console-v3-after-payments-1280.png", ["f-payments-1440.png", "d-payments-1440.png"], "payments-1440.png"),
    ("payment", "console-v3-after-paymentdetail-1280.png", ["f-payment-1440.png", "e-payment-1440.png"], "payment-1440.png"),
    ("balance", "console-v3-after-balance-1280.png", ["e-overview-1440.png"], "balance-1440.png"),
]
try:
    font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 28)
except OSError:
    font = ImageFont.load_default()
W = 960
for name, before, mocks, after in pages:
    cols = [("v3 before", os.path.join(v3, before))] + [("round 2 " + m[0].upper(), os.path.join(r2, m)) for m in mocks] + [("v4 built", os.path.join(v4, after))]
    imgs = []
    for label, path in cols:
        im = Image.open(path).convert("RGB")
        im = im.resize((W, int(im.height * W / im.width)))
        im = im.crop((0, 0, W, min(im.height, 600)))
        imgs.append((label, im))
    h = max(i.height for _, i in imgs) + 48
    sheet = Image.new("RGB", (W * len(imgs) + 16 * (len(imgs) - 1), h), "white")
    d = ImageDraw.Draw(sheet)
    for n, (label, im) in enumerate(imgs):
        x = n * (W + 16)
        d.text((x + 8, 8), label, fill="black", font=font)
        sheet.paste(im, (x, 48))
    sheet.save(os.path.join(out, f"compare-{name}.png"))
print("ok")
