#!/usr/bin/env python3
"""Convert public/assets/icons/app.ico to a square PNG source for `tauri icon`."""
from pathlib import Path
from PIL import Image

root = Path(__file__).resolve().parent.parent
ico = root / "public/assets/icons/app.ico"
out = root / "src-tauri/icons/app-icon.png"
out.parent.mkdir(parents=True, exist_ok=True)
im = Image.open(ico)
sizes = sorted(im.info.get("sizes", [im.size]), key=lambda s: s[0] * s[1])
try:
    im.size = sizes[-1]
except Exception:
    pass
im = im.convert("RGBA")
side = max(im.size)
canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
canvas.paste(im, ((side - im.width) // 2, (side - im.height) // 2))
canvas = canvas.resize((1024, 1024), Image.LANCZOS)
canvas.save(out)
print("wrote", out)
