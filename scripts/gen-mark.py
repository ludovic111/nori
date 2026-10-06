#!/usr/bin/env python3
"""Writes nori's mark and app icon: three square sheets laid in layers, offset up and to the right,
each cut at its top-left corner like a Chakra Petch letter. The top sheet is solid, the one under it
shows as a solid L, and the back one dissolves into ordered dither (the grain of the interface).
One colour.

    brand/mark.svg                              ink on paper
    brand/icon.svg                              the app icon (then run scripts/make-icons.sh)
    crates/nori-desktop/assets/icons/mark.svg   the window's copy, in currentColor
"""
import os

# The sheets, in a 64 × 64 box: side, step between sheets, gap that keeps them apart in one ink,
# and the cut at each sheet's top-left corner.
SIDE, STEP, GAP, CUT = 27.0, 10.0, 3.0, 4.0
SPAN = SIDE + 2 * STEP
X0 = 32 - SPAN / 2           # the top (front) sheet's left edge
Y0 = 32 + SPAN / 2 - SIDE    # and its top edge


def sheet(x, y):
    """A whole sheet, cut at its top-left corner."""
    return [(x + CUT, y), (x + SIDE, y), (x + SIDE, y + SIDE), (x, y + SIDE), (x, y + CUT)]


def peek(x, y):
    """What shows of the sheet at (x, y) behind the one at (x - STEP, y + STEP): an L along its top
    and right edges, a gap away from the sheet in front."""
    fx, fy = x - STEP, y + STEP
    return [(x + CUT, y), (x + SIDE, y), (x + SIDE, y + SIDE), (fx + SIDE + GAP, y + SIDE),
            (fx + SIDE + GAP, fy - GAP), (x, fy - GAP), (x, y + CUT)]


FRONT = sheet(X0, Y0)
MIDDLE = peek(X0 + STEP, Y0 - STEP)
BACK = peek(X0 + 2 * STEP, Y0 - 2 * STEP)


def inside(p, poly):
    x, y = p
    c = False
    n = len(poly)
    for i in range(n):
        x1, y1 = poly[i]
        x2, y2 = poly[(i + 1) % n]
        if (y1 > y) != (y2 > y) and x < (x2 - x1) * (y - y1) / (y2 - y1) + x1:
            c = not c
    return c


B = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]


def dots(cell=2.2, size=1.75):
    """The back sheet in ordered dither: solid at its corner, thinning out towards both ends of the L."""
    out = []
    bx, by = X0 + 2 * STEP, Y0 - 2 * STEP
    left, top = bx, by
    j = 0
    y = top
    while y < by + SIDE:
        i = 0
        x = left
        while x < bx + SIDE:
            c = (x + cell / 2, y + cell / 2)
            if inside(c, BACK) and x + size <= bx + SIDE + 0.01 and y >= by - 0.01:
                # 0 at the far corner, 1 at either end of the L.
                t = 1 - ((c[0] - bx) + (by + SIDE - c[1])) / (2 * SIDE)
                t /= 1 - (SIDE - (STEP - GAP) / 2) / (2 * SIDE)
                level = 1 - 0.8 * max(0, min(1, t)) ** 1.3
                if level > (B[j % 4][i % 4] + 0.5) / 16:
                    out.append(f'<rect x="{x:.2f}" y="{y:.2f}" width="{size}" height="{size}"/>')
            x += cell
            i += 1
        y += cell
        j += 1
    return out


def pts(p):
    return " ".join(f"{x:g},{y:g}" for x, y in p)


def mark(fill="currentColor"):
    return (f'<g fill="{fill}"><polygon points="{pts(FRONT)}"/><polygon points="{pts(MIDDLE)}"/>'
            + "".join(dots()) + "</g>")


def icon():
    tile = ("M383.41 100 L640.59 100 C722.19 100 763 100 799.79 112.14 L806.92 113.89 C854.88 131.34 892.66 169.12 910.11 217.08 C924 261 924 301.81 924 383.41 L924 640.59 C924 722.19 924 763 911.86 799.79 L910.11 806.92 C892.66 854.88 854.88 892.66 806.92 910.11 C763 924 722.19 924 640.59 924 L383.41 924 C301.81 924 261 924 224.21 911.86 L217.08 910.11 C169.12 892.66 131.34 854.88 113.89 806.92 C100 763 100 722.19 100 640.59 L100 383.41 C100 301.81 100 261 112.14 224.21 L113.89 217.08 C131.34 169.12 169.12 131.34 217.08 113.89 C261 100 301.81 100 383.41 100 Z")
    # Dithered light in the top-left corner of the tile, as on the app's page.
    corner = []
    cell = 16
    for j in range(0, 26):
        for i in range(0, 26):
            x = 100 + i * cell
            y = 100 + j * cell
            d = ((i / 26) ** 2 + (j / 26) ** 2) ** 0.5 / 1.2
            level = max(0, 1 - d * 1.5) ** 1.4
            if level > (B[j % 4][i % 4] + 0.5) / 16:
                corner.append(f'<rect x="{x}" y="{y}" width="{cell - 5}" height="{cell - 5}"/>')
    return (f'''<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <!-- nori app icon: the macOS icon grid (824 px continuous-corner tile on 1024) in near black,
       a corner of dithered light like the app's page, and the mark (brand/mark.svg) in white at
       about 58 % of the tile. scripts/gen-mark.py writes this file; scripts/make-icons.sh renders
       every size from it. -->
  <defs>
    <path id="tile" d="{tile}"/>
    <clipPath id="tile-clip"><use href="#tile"/></clipPath>
  </defs>
  <use href="#tile" fill="#0b0b0b"/>
  <g clip-path="url(#tile-clip)" fill="#fff" fill-opacity="0.16">{"".join(corner)}</g>
  <use href="#tile" fill="none" stroke="#fff" stroke-opacity="0.16" stroke-width="3"/>
  <g transform="translate(512 512) scale(10.2) translate(-32 -32)">{mark("#fff")}</g>
</svg>''')


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text + "\n")


if __name__ == "__main__":
    os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    write("crates/nori-desktop/assets/icons/mark.svg", '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">' + mark() + "</svg>")
    write("brand/mark.svg", '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">\n  <!-- nori\'s mark: three sheets laid in layers, cut square; the back one dissolves into dither\n       (the grain of the interface). One colour: ink on paper or paper on ink.\n       scripts/gen-mark.py writes it. -->\n  ' + mark("#0a0a0a") + "\n</svg>")
    write("brand/icon.svg", icon())
