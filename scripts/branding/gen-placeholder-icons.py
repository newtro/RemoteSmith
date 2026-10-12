#!/usr/bin/env python3
"""Generate RemoteSmith's placeholder icons into vendor/rustdesk (patch 0105).

A deliberately simple mark: two overlapping screens on a dark rounded square, for "two-way
remote desktop". It is a placeholder until a real logo exists. The geometry is defined once and
rendered both as SVG (Flutter's in-app icon, Linux scalable icon) and as PNG/ICO/ICNS through
Pillow, so every output matches.

Usage: python3 scripts/branding/gen-placeholder-icons.py   (needs Pillow and macOS iconutil)
Then commit the changed files inside vendor/rustdesk as the remotesmith(0105) patch.
"""
import os
import shutil
import subprocess
import sys
import tempfile

from PIL import Image, ImageDraw

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "vendor", "rustdesk")

BG = "#0f3d5e"
FG = "#ffffff"
ACCENT = "#2ec4b6"
CANVAS = 1024
BG_RADIUS = 228
# (x0, y0, x1, y1, corner radius) on the 1024 canvas
BACK = (168, 262, 616, 566, 44)
FRONT = (408, 458, 856, 762, 44)
STROKE = 40
STAND = (512, 762, 752, 800)  # base under the front screen: x0, y0, x1, y1


def svg(template=False):
    if template:
        return (
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {CANVAS} {CANVAS}">'
            f'<rect x="{BACK[0]}" y="{BACK[1]}" width="{BACK[2]-BACK[0]}" height="{BACK[3]-BACK[1]}" '
            f'rx="{BACK[4]}" fill="none" stroke="#000" stroke-width="{STROKE*2}"/>'
            f'<rect x="{FRONT[0]}" y="{FRONT[1]}" width="{FRONT[2]-FRONT[0]}" height="{FRONT[3]-FRONT[1]}" '
            f'rx="{FRONT[4]}" fill="#000"/></svg>\n'
        )
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {CANVAS} {CANVAS}">'
        f'<rect width="{CANVAS}" height="{CANVAS}" rx="{BG_RADIUS}" fill="{BG}"/>'
        f'<rect x="{BACK[0]}" y="{BACK[1]}" width="{BACK[2]-BACK[0]}" height="{BACK[3]-BACK[1]}" '
        f'rx="{BACK[4]}" fill="none" stroke="{FG}" stroke-width="{STROKE}"/>'
        f'<rect x="{FRONT[0]}" y="{FRONT[1]}" width="{FRONT[2]-FRONT[0]}" height="{FRONT[3]-FRONT[1]}" '
        f'rx="{FRONT[4]}" fill="{ACCENT}" stroke="{FG}" stroke-width="{STROKE}"/>'
        f'<rect x="{STAND[0]}" y="{STAND[1]}" width="{STAND[2]-STAND[0]}" height="{STAND[3]-STAND[1]}" '
        f'rx="12" fill="{FG}"/></svg>\n'
    )


def render(size, template=False, padding=0.0):
    """Render at 4x and downsample for anti-aliasing. padding: fraction of the canvas to inset."""
    big = size * 4
    s = big / CANVAS
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def box(b):
        inset = padding * CANVAS
        k = (CANVAS - 2 * inset) / CANVAS
        return [((v * k) + inset) * s for v in b[:4]]

    def r(v):
        return v * s * (1 - 2 * padding)

    if template:
        d.rounded_rectangle(box(BACK), radius=r(BACK[4]), outline="black", width=int(r(STROKE * 2)))
        d.rounded_rectangle(box(FRONT), radius=r(FRONT[4]), fill="black")
    else:
        d.rounded_rectangle(box((0, 0, CANVAS, CANVAS)), radius=r(BG_RADIUS), fill=BG)
        d.rounded_rectangle(box(BACK), radius=r(BACK[4]), outline=FG, width=int(r(STROKE)))
        d.rounded_rectangle(box(FRONT), radius=r(FRONT[4]), fill=ACCENT, outline=FG, width=int(r(STROKE)))
        d.rounded_rectangle(box(STAND), radius=r(12), fill=FG)
    return img.resize((size, size), Image.LANCZOS)


def out(rel):
    p = os.path.join(ROOT, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    return p


def main():
    if not os.path.isfile(os.path.join(ROOT, "Cargo.toml")):
        sys.exit("vendor/rustdesk is not checked out")
    for rel in ("flutter/assets/icon.svg", "res/logo.svg", "res/scalable.svg"):
        open(out(rel), "w").write(svg())
    for size, rel in ((32, "res/32x32.png"), (64, "res/64x64.png"), (128, "res/128x128.png"),
                      (256, "res/128x128@2x.png"), (1024, "res/icon.png")):
        render(size).save(out(rel))
    # macOS app icon artwork sits inside a ~10% margin, as Apple's grid expects.
    render(1024, padding=0.1).save(out("res/mac-icon.png"))
    # Tray: macOS uses a template image (alpha only); Windows a colour .ico.
    render(60, template=True).save(out("res/mac-tray-dark-x2.png"))
    render(48, template=True).save(out("res/mac-tray-light-x2.png"))
    ico_sizes = [(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    render(256).save(out("res/icon.ico"), sizes=ico_sizes)
    render(256).save(out("flutter/windows/runner/resources/app_icon.ico"), sizes=ico_sizes)
    render(32).save(out("res/tray-icon.ico"), sizes=[(32, 32)])
    with tempfile.TemporaryDirectory() as tmp:
        iconset = os.path.join(tmp, "AppIcon.iconset")
        os.makedirs(iconset)
        for base in (16, 32, 128, 256, 512):
            render(base, padding=0.1).save(os.path.join(iconset, f"icon_{base}x{base}.png"))
            render(base * 2, padding=0.1).save(os.path.join(iconset, f"icon_{base}x{base}@2x.png"))
        icns = os.path.join(tmp, "AppIcon.icns")
        subprocess.run(["iconutil", "-c", "icns", "-o", icns, iconset], check=True)
        shutil.copyfile(icns, out("flutter/macos/Runner/AppIcon.icns"))
    print("icons written under", os.path.normpath(ROOT))


if __name__ == "__main__":
    main()
