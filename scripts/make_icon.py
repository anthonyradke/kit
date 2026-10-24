"""Draws app-icon.png (1024 px): gradient headphones on a near-black tile.
Run `cargo tauri icon app-icon.png` from app/src-tauri afterwards."""
import sys
from PIL import Image, ImageDraw, ImageChops

S = 4096  # supersampled, downscaled at the end
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle([0, 0, S - 1, S - 1], radius=S * 0.22, fill=(11, 12, 16, 255))

# Gradient (violet -> blue -> cyan, diagonal) used through a mask.
grad = Image.new("RGBA", (S, S))
gp = grad.load()
stops = [(0.0, (139, 92, 246)), (0.55, (109, 139, 255)), (1.0, (56, 189, 248))]
for y in range(0, S, 4):
    for x in range(0, S, 4):
        t = min(1.0, max(0.0, (x * 0.65 + y * 0.35) / S))
        for (a, ca), (b, cb) in zip(stops, stops[1:]):
            if a <= t <= b:
                k = (t - a) / (b - a)
                c = tuple(int(ca[i] + (cb[i] - ca[i]) * k) for i in range(3))
                break
        for dy in range(4):
            for dx in range(4):
                gp[x + dx, y + dy] = c + (255,)

mask = Image.new("L", (S, S), 0)
m = ImageDraw.Draw(mask)
cx, top, r, w = S / 2, S * 0.24, S * 0.30, S * 0.075
# Headband: thick arc.
m.arc([cx - r, top, cx + r, top + 2 * r], start=180, end=360, fill=255, width=int(w))
# Arms down to the cups.
m.rectangle([cx - r, top + r - 2, cx - r + w, S * 0.60], fill=255)
m.rectangle([cx + r - w, top + r - 2, cx + r, S * 0.60], fill=255)
# Ear cups.
cw, ch = S * 0.15, S * 0.26
for sx in (cx - r - cw * 0.25, cx + r - cw * 0.75):
    m.rounded_rectangle([sx, S * 0.50, sx + cw, S * 0.50 + ch], radius=cw * 0.42, fill=255)
# Three level bars between the cups (the "room" in headroom).
bw = S * 0.045
for i, h in enumerate((0.10, 0.17, 0.12)):
    x = cx + (i - 1) * bw * 1.9 - bw / 2
    m.rounded_rectangle([x, S * 0.68 - S * h, x + bw, S * 0.68], radius=bw / 2, fill=255)

img.paste(grad, (0, 0), mask)
img = img.resize((1024, 1024), Image.LANCZOS)
img.save(sys.argv[1] if len(sys.argv) > 1 else "app-icon.png")
