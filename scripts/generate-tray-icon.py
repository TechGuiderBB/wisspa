#!/usr/bin/env python3
"""
Generate the Wisspa menu-bar tray glyph.

Specs (from Brooke 2026-05-17):
  - Rounded square with corner radius ~23% of side length
  - W stroke ~13% of side, mitred caps, optical adjustments so inner vertices
    don't crowd
  - Outline-style: square border + W stroke, no fill

Renders at 4x oversampled then downsamples for clean edges at the target
size (44px = macOS menu-bar @2x). Output: src-tauri/icons/tray.png

Run from repo root:
    /tmp/wisspa-icon-venv/bin/python scripts/generate-tray-icon.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

OUTPUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons" / "tray.png"
TARGET_SIZE = 44  # px, macOS menu-bar @2x
SCALE = 8         # oversample factor
SIDE = TARGET_SIZE * SCALE

CORNER_RATIO = 0.23
STROKE_RATIO = 0.13
COLOR = (255, 255, 255, 255)  # idle glyph is white; recording variant tints at runtime

# Inset the whole drawing so the strokes don't clip the canvas edge.
PADDING_RATIO = 0.04
PAD = int(SIDE * PADDING_RATIO)
INNER_SIDE = SIDE - PAD * 2

CORNER = int(INNER_SIDE * CORNER_RATIO)
STROKE = int(INNER_SIDE * STROKE_RATIO)

img = Image.new("RGBA", (SIDE, SIDE), (0, 0, 0, 0))
draw = ImageDraw.Draw(img)

# Rounded square border.
draw.rounded_rectangle(
    [(PAD, PAD), (PAD + INNER_SIDE - 1, PAD + INNER_SIDE - 1)],
    radius=CORNER,
    outline=COLOR,
    width=STROKE,
)

# Compute the W. Layout inside the square's inner padded area.
W_PAD = STROKE + int(INNER_SIDE * 0.12)
left   = PAD + W_PAD
right  = PAD + INNER_SIDE - W_PAD
top    = PAD + W_PAD
bottom = PAD + INNER_SIDE - W_PAD

# Optical adjustments: bring the inner vertices in slightly so they don't
# crowd the outer downstrokes, and raise them a hair above the baseline so
# the W's two valleys read at the correct visual depth.
inner_inset_x = int((right - left) * 0.05)
inner_valley_lift = int((bottom - top) * 0.08)

mid_x = (left + right) // 2
left_valley_x  = left + (mid_x - left) * 1 // 2 + inner_inset_x // 2
right_valley_x = right - (right - mid_x) * 1 // 2 - inner_inset_x // 2
valley_y = bottom - inner_valley_lift

# Five-point polyline forming a W: top-left → first valley → top-middle
# (raised peak) → second valley → top-right.
# Cap style mitred via joint="curve" off, default is miter.
points = [
    (left,           top),
    (left_valley_x,  valley_y),
    (mid_x,          top + int((bottom - top) * 0.15)),  # central peak slightly below top edge
    (right_valley_x, valley_y),
    (right,          top),
]
draw.line(points, fill=COLOR, width=STROKE, joint=None)

# Downsample with high-quality filter for crisp edges.
final = img.resize((TARGET_SIZE, TARGET_SIZE), Image.LANCZOS)
OUTPUT.parent.mkdir(parents=True, exist_ok=True)
final.save(OUTPUT, "PNG")
print(f"wrote {OUTPUT} ({TARGET_SIZE}x{TARGET_SIZE}, stroke={STROKE//SCALE}px, corner={CORNER//SCALE}px)")
