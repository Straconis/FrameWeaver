"""Draws FrameWeaver's logo and README banner as SVG files.

The logo is a film strip with a centred FW monogram over a soft spider web (the "weave"), on the
same blue tile as SnapLapse. Small sizes drop the letters, which would blur, and use a play button
instead, the same way SnapLapse has simpler small icons. Run render.sh afterwards to make the
PNG, ICO and banner images.
"""

import math
import os

HERE = os.path.dirname(os.path.abspath(__file__))
C = 256
CYAN = "#67E8F9"
DEFS = (
    '<linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="512" y2="512">'
    '<stop offset="0" stop-color="#3B82F6"/><stop offset="1" stop-color="#4338CA"/></linearGradient>'
    '<clipPath id="tile"><rect width="512" height="512" rx="112"/></clipPath>'
)


def ring(n, r, sag, rot):
    """One ring of the web: straight between spokes, sagging slightly toward the centre."""
    pts = [(C + r * math.cos(math.radians(rot + k * 360 / n)), C + r * math.sin(math.radians(rot + k * 360 / n))) for k in range(n)]
    d = f"M{pts[0][0]:.1f} {pts[0][1]:.1f}"
    for k in range(n):
        (x1, y1), (x2, y2) = pts[k], pts[(k + 1) % n]
        mx, my = (x1 + x2) / 2, (y1 + y2) / 2
        d += f" Q{mx + (C - mx) * sag:.1f} {my + (C - my) * sag:.1f} {x2:.1f} {y2:.1f}"
    return d + " Z"


def web(opacity, width, rings=(70, 128, 190, 255, 325), n=8, rot=22.5, sag=0.10):
    parts = [f'<g clip-path="url(#tile)" fill="none" stroke="#fff" stroke-opacity="{opacity}" '
             f'stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round">']
    for k in range(n):
        a = math.radians(rot + k * 360 / n)
        parts.append(f'<line x1="{C}" y1="{C}" x2="{C + 380 * math.cos(a):.1f}" y2="{C + 380 * math.sin(a):.1f}"/>')
    parts += [f'<path d="{ring(n, r, sag, rot)}"/>' for r in rings]
    return "".join(parts) + "</g>"


def film_strip(x, y, w, h, holes, stroke, hole=(15, 13), inset=17):
    """A film frame with sprocket holes along the top and bottom. Its inside is filled with the
    tile's own gradient, so the web behind it disappears seamlessly."""
    out = [f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="18" fill="url(#g)"/>',
           f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="18" fill="none" stroke="#fff" stroke-width="{stroke}"/>']
    hw, hh = hole
    gap = (w - 44) / holes
    for i in range(holes):
        hx = x + 22 + gap * i + (gap - hw) / 2
        out.append(f'<rect x="{hx:.1f}" y="{y + inset}" width="{hw}" height="{hh}" rx="3" fill="#fff"/>')
        out.append(f'<rect x="{hx:.1f}" y="{y + h - inset - hh}" width="{hw}" height="{hh}" rx="3" fill="#fff"/>')
    return "".join(out)


def play(cx, cy, size):
    w = size * 0.9
    return (f'<path d="M{cx - w / 2:.1f} {cy - size / 2:.1f} V{cy + size / 2:.1f} L{cx + w / 2:.1f} {cy:.1f} Z" '
            f'fill="{CYAN}" stroke="{CYAN}" stroke-width="10" stroke-linejoin="round"/>')


# The letters are placed by measurement: Inter's FW at this size sits 23.5 px high and 1.5 px right
# of the point given, so these numbers put it at the exact centre of the frame (256, 256), with
# equal space above and below between the sprocket holes.
FW = (f'<text x="254.5" y="281.5" font-family="Inter, Segoe UI, Arial, sans-serif" font-weight="800" '
      f'font-size="70" letter-spacing="-4" text-anchor="middle" fill="{CYAN}">FW</text>')

TILE = '<rect width="512" height="512" rx="112" fill="url(#g)"/>'
ICONS = {
    # 48 px and up: the full logo.
    "frameweaver-icon.svg": TILE + web(0.22, 6) + film_strip(132, 170, 248, 172, 6, 16) + FW,
    # 24-32 px: no letters (they'd blur); bolder strip and a play button, a fainter web.
    "frameweaver-icon-small.svg": TILE + web(0.18, 10, rings=(110, 200, 300)) + film_strip(112, 160, 288, 192, 5, 26, hole=(22, 20), inset=22) + play(262, 256, 80),
    # 16-20 px: just a bold film frame and play button.
    "frameweaver-icon-tiny.svg": TILE + film_strip(96, 144, 320, 224, 4, 34, hole=(30, 26), inset=26) + play(264, 256, 104),
}


def svg(body, w=512, h=512):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}"><defs>{DEFS}</defs>{body}</svg>'


def banner():
    """The README banner: logo, name, tagline, platforms, and a strip of separate frames that
    turns into one continuous video bar, which is what FrameWeaver does."""
    icon = (f'<g transform="translate(96 64) scale(.4297)">{ICONS["frameweaver-icon.svg"]}</g>')
    frames = "".join(
        f'<rect x="{96 + i * 52}" y="322" width="40" height="28" rx="6" fill="#fff" fill-opacity="{0.16 + i * 0.07:.2f}"/>'
        for i in range(10)
    )
    bar = ('<path d="M622 322 L644 336 L622 350 Z" fill="#67E8F9"/>'
           '<rect x="664" y="330" width="520" height="12" rx="6" fill="#fff" fill-opacity=".15"/>'
           '<rect x="664" y="330" width="364" height="12" rx="6" fill="#67E8F9"/>'
           '<circle cx="1028" cy="336" r="14" fill="#67E8F9"/>')
    body = (
        '<defs><linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#0B1220"/>'
        '<stop offset="1" stop-color="#1E1B4B"/></linearGradient>'
        '<radialGradient id="glow" cx=".18" cy=".42" r=".55"><stop offset="0" stop-color="#3B82F6" stop-opacity=".35"/>'
        '<stop offset="1" stop-color="#3B82F6" stop-opacity="0"/></radialGradient>'
        '<pattern id="grid" width="32" height="32" patternUnits="userSpaceOnUse"><path d="M32 0H0V32" fill="none" stroke="#fff" stroke-opacity=".04"/></pattern></defs>'
        '<rect width="1280" height="400" rx="24" fill="url(#bg)"/><rect width="1280" height="400" rx="24" fill="url(#grid)"/>'
        '<rect width="1280" height="400" rx="24" fill="url(#glow)"/>'
        + icon +
        '<g font-family="Inter, Segoe UI, Helvetica, Arial, sans-serif" fill="#fff">'
        '<text x="340" y="160" font-size="96" letter-spacing="-2"><tspan font-weight="700">Frame</tspan><tspan font-weight="400">Weaver</tspan></text>'
        '<text x="344" y="214" font-size="28" fill="#CBD5E1">Turn pictures into a time-lapse video of any length.</text>'
        '<text x="344" y="262" font-size="18" font-weight="600" letter-spacing="3" fill="#67E8F9">WINDOWS 10  ·  WINDOWS 11  ·  LINUX</text></g>'
        + frames + bar
    )
    return svg(body, 1280, 400)


if __name__ == "__main__":
    for name, body in ICONS.items():
        open(os.path.join(HERE, name), "w").write(svg(body))
    open(os.path.join(HERE, "frameweaver-banner.svg"), "w").write(banner())
    print("wrote", ", ".join(list(ICONS) + ["frameweaver-banner.svg"]))
