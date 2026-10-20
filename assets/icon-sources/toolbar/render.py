#!/usr/bin/env python3
"""Renders the toolbar icons (NN_name.svg) into assets/toolbar/toolbar-{16,32}.png:
one icon per column in file-name order, row 0 normal, row 1 greyed out (disabled).
Needs rsvg-convert and Pillow. Run from the repository root after changing an icon."""
import glob, io, os, subprocess
from PIL import Image, ImageEnhance

here = os.path.dirname(os.path.abspath(__file__))
svgs = sorted(glob.glob(os.path.join(here, "[0-9][0-9]_*.svg")))
for size in (16, 32):
    strip = Image.new("RGBA", (size * len(svgs), size * 2), (0, 0, 0, 0))
    for i, svg in enumerate(svgs):
        png = subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), svg],
                             check=True, capture_output=True).stdout
        icon = Image.open(io.BytesIO(png)).convert("RGBA")
        strip.paste(icon, (i * size, 0))
        # Disabled: grey, lighter, half transparent (like Windows toolbars).
        grey = ImageEnhance.Color(icon).enhance(0.0)
        grey = ImageEnhance.Brightness(grey).enhance(1.2)
        r, g, b, a = grey.split()
        a = a.point(lambda v: v * 65 // 100)
        strip.paste(Image.merge("RGBA", (r, g, b, a)), (i * size, size))
    strip.save(os.path.join(here, "..", "..", "toolbar", f"toolbar-{size}.png"), optimize=True)
print("\n".join(os.path.basename(s) for s in svgs))
