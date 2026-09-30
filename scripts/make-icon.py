#!/usr/bin/env python3
"""Build a macOS iconset from the app's logo.

The logo (`assets/logo.png`, the orange loop, square and cropped to its art)
is composited in its own colours onto a rounded plate in the app's chrome
colour. The sidebar draws `assets/logo-small.png`, cut from the same art, so
the icon and the sidebar cannot disagree about what the logo is.
"""
import pathlib
import sys

from PIL import Image, ImageDraw

PLATE = (0x11, 0x12, 0x14)
# macOS art occupies roughly this much of its tile; a full-bleed glyph looks
# oversized next to every other icon in the Dock. The loop is round and
# airy, so it takes a little more of the tile than a solid glyph would.
INSET = 0.68
CORNER = 0.225


def tile(logo: Image.Image, px: int) -> Image.Image:
    out = Image.new("RGBA", (px, px), (0, 0, 0, 0))
    mask = Image.new("L", (px, px), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, px - 1, px - 1], radius=int(px * CORNER), fill=255
    )
    out.paste(Image.new("RGBA", (px, px), PLATE + (255,)), (0, 0), mask)

    inner = max(1, int(px * INSET))
    art = logo.resize((inner, inner), Image.LANCZOS)
    off = (px - inner) // 2
    out.alpha_composite(art, (off, off))
    return out


def main() -> None:
    out = pathlib.Path(sys.argv[1])
    logo = Image.open("assets/logo.png").convert("RGBA")
    for size in (16, 32, 64, 128, 256, 512, 1024):
        for scale, suffix in ((1, ""), (2, "@2x")):
            px = size * scale
            if px > 1024:
                continue
            tile(logo, px).save(out / f"icon_{size}x{size}{suffix}.png")
    # The window's runtime icon (acp-app embeds it); eframe shows this in the
    # Dock, not the bundle's icns, so it is written from the same tile.
    tile(logo, 512).save("assets/icon.png")


if __name__ == "__main__":
    main()
