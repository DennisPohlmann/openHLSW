#!/usr/bin/env python3
"""Draws the pictures of the MSI setup wizard in the style of the banner of "About openHLSW"
(dark red to orange, soft light curves, the icon, "openHLSW / Gameserver Tool"):

  dialog.bmp  493 x 312  Welcome and Finished pages: the banner as a strip on the left,
                         white on the right where the wizard writes its text.
  banner.bmp  493 x 58   Top of the other pages: white on the left for the page title,
                         the banner fading in on the right.

Needs Pillow and rsvg-convert (for assets/openhlsw.svg). Run from the repository root
after changing the look:  python3 packaging/windows/render-installer-images.py
"""
import io
import math
import os
import subprocess

from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT = os.path.dirname(os.path.abspath(__file__))
FONT = os.path.join(ROOT, "assets", "fonts", "LiberationSans-Regular.ttf")

# Same colors as the About banner (src/app.rs, about_view).
TOP_LEFT, TOP_RIGHT = (140, 24, 14), (232, 96, 40)
BOTTOM_RIGHT, BOTTOM_LEFT = (245, 150, 70), (178, 40, 22)


def gradient(w, h):
    """Bilinear blend of the four corner colors."""
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        v = y / max(h - 1, 1)
        for x in range(w):
            u = x / max(w - 1, 1)
            px[x, y] = tuple(
                round(
                    (1 - u) * (1 - v) * TOP_LEFT[i]
                    + u * (1 - v) * TOP_RIGHT[i]
                    + u * v * BOTTOM_RIGHT[i]
                    + (1 - u) * v * BOTTOM_LEFT[i]
                )
                for i in range(3)
            )
    return img


def curves(img, scale=1.0):
    """The soft white curves of the About banner, drawn smooth at 4x and scaled down."""
    w, h = img.size
    big = Image.new("RGBA", (w * 4, h * 4), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)
    for i in range(9):
        t = i / 8
        pts = []
        for k in range(49):
            x = w * k / 48
            phase = k / 48 * math.pi
            y = h - 10 * scale - t * 70 * scale - math.sin(phase) * (18 + t * 22) * scale
            pts.append((x * 4, y * 4))
        d.line(pts, fill=(255, 255, 255, 28 + int(t * 40)), width=4)
    small = big.resize((w, h), Image.LANCZOS)
    img.paste(small, (0, 0), small)


def icon(size):
    png = subprocess.run(
        ["rsvg-convert", "-w", str(size), "-h", str(size), os.path.join(ROOT, "assets", "openhlsw.svg")],
        check=True,
        capture_output=True,
    ).stdout
    return Image.open(io.BytesIO(png)).convert("RGBA")


def text(img, xy, s, size, anchor):
    """White text with the 1 px shadow of the About banner."""
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    font = ImageFont.truetype(FONT, size)
    d.text((xy[0] + 1, xy[1] + 1), s, font=font, fill=(0, 0, 0, 90), anchor=anchor)
    d.text(xy, s, font=font, fill=(255, 255, 255, 255), anchor=anchor)
    img.paste(layer, (0, 0), layer)


def dialog():
    img = Image.new("RGB", (493, 312), (255, 255, 255))
    strip = gradient(164, 312)
    curves(strip, scale=1.6)
    ic = icon(80)
    strip.paste(ic, ((164 - 80) // 2, 70), ic)
    text(strip, (82, 168), "openHLSW", 24, "mt")
    text(strip, (82, 202), "Gameserver Tool", 14, "mt")
    img.paste(strip, (0, 0))
    img.save(os.path.join(OUT, "dialog.bmp"))


def banner():
    img = Image.new("RGB", (493, 58), (255, 255, 255))
    band = gradient(493, 58)
    curves(band, scale=0.45)
    # Fades in from white over 70 px, from x = 395 on (title and description stay on white).
    mask = Image.new("L", (493, 58), 0)
    mp = mask.load()
    for x in range(493):
        a = 0 if x < 395 else min(255, round((x - 395) / 70 * 255))
        for y in range(58):
            mp[x, y] = a
    img.paste(band, (0, 0), mask)
    ic = icon(44)
    img.paste(ic, (493 - 44 - 10, 7), ic)
    img.save(os.path.join(OUT, "banner.bmp"))


dialog()
banner()
print("dialog.bmp, banner.bmp written to", OUT)
