"""Draws app-icon.png for kit: a graphite tile with a silver "k" and the yellow live dot.

Usage: python scripts/make_icon.py <path to Archivo variable TTF> <out.png>
Then: cd app/src-tauri; cargo tauri icon ../../app-icon.png
"""
import sys
from PIL import Image, ImageDraw, ImageFont

S = 1024
font_path, out = sys.argv[1], sys.argv[2]

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
r = int(S * 0.22)
d.rounded_rectangle([0, 0, S - 1, S - 1], radius=r, fill=(20, 21, 23, 255))
# Hairline plate rule, inset like a printed label's border.
inset = int(S * 0.075)
d.rounded_rectangle([inset, inset, S - 1 - inset, S - 1 - inset], radius=int(r * 0.6), outline=(74, 76, 81, 255), width=int(S * 0.012))

font = ImageFont.truetype(font_path, int(S * 0.74))
axes = {a["name"]: a for a in font.get_variation_axes()}
values = []
for a in font.get_variation_axes():
    name = a["name"].lower() if isinstance(a["name"], str) else a["name"].decode().lower()
    values.append(112 if "width" in name else 800)
font.set_variation_by_axes(values)
text = "k"
bbox = d.textbbox((0, 0), text, font=font)
w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]
x = (S - w) / 2 - bbox[0] - S * 0.075
y = (S - h) / 2 - bbox[1]
d.text((x, y), text, font=font, fill=(236, 232, 223, 255))

# The live mark.
c = (int(S * 0.775), int(S * 0.735))
rad = int(S * 0.068)
d.ellipse([c[0] - rad, c[1] - rad, c[0] + rad, c[1] + rad], fill=(232, 185, 49, 255))

img.save(out)
print(out)
