#!/usr/bin/env python3
"""Build a macOS iconset from the app's mark.

The mark is stored as an alpha mask (`assets/mark.png`) so the sidebar can
tint it; here it is composited in the app's own ink onto a rounded plate in
the app's own chrome colour. One source, so the icon and the sidebar cannot
disagree about what the logo is.
"""
import pathlib
import sys

from PIL import Image, ImageDraw

PLATE = (0x11, 0x12, 0x14)
INK = (0xF2, 0xF3, 0xF5)
# macOS art occupies roughly this much of its tile; a full-bleed glyph looks
# oversized next to every other icon in the Dock.
INSET = 0.58
CORNER = 0.225


def tile(alpha: Image.Image, px: int) -> Image.Image:
    out = Image.new("RGBA", (px, px), (0, 0, 0, 0))
    mask = Image.new("L", (px, px), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, px - 1, px - 1], radius=int(px * CORNER), fill=255
    )
    out.paste(Image.new("RGBA", (px, px), PLATE + (255,)), (0, 0), mask)

    inner = max(1, int(px * INSET))
    a = alpha.resize((inner, inner), Image.LANCZOS)
    glyph = Image.merge(
        "RGBA", tuple(Image.new("L", (inner, inner), c) for c in INK) + (a,)
    )
    off = (px - inner) // 2
    out.paste(glyph, (off, off), glyph)
    return out


def main() -> None:
    out = pathlib.Path(sys.argv[1])
    alpha = Image.open("assets/mark.png").split()[-1]
    for size in (16, 32, 64, 128, 256, 512, 1024):
        for scale, suffix in ((1, ""), (2, "@2x")):
            px = size * scale
            if px > 1024:
                continue
            tile(alpha, px).save(out / f"icon_{size}x{size}{suffix}.png")


if __name__ == "__main__":
    main()
