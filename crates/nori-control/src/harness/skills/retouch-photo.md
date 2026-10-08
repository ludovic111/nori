---
name: retouch-photo
title: Retouch a photo
when: A photo needs tone, colour, cleanup or sharpening ("make it pop", "fix the exposure", "warmer", "moody black and white").
---
# Retouch a photo

Global before local, adjustments as layers, sharpen last.

## Steps

1. Open it if it isn't (`doc_open path=…`, only when asked), then `harness_look` to see it and
   `doc_overview` for the photo layer's id. Note what is wrong: too dark or flat, a colour cast,
   crooked, distracting edges.
2. Framing first: `doc_rotate` for quarter turns, `layer_transform rotate=…` for a tilted horizon
   (then crop the corners away), `doc_crop x y width height` to a stronger frame (rule of thirds,
   keep the aspect the person needs).
3. Tone: `layer_addAdjustment kind=levels` with inputBlack/inputWhite at the ends of the
   histogram (a flat photo: black 10–25, white 230–245), or `kind=curves` with a gentle S:
   `{"rgb": [[0,0],[64,54],[192,204],[255,255]]}`. Exposure off by a stop: `kind=exposure
   {"exposure": 0.5}`.
4. Colour: a cast → `kind=curves` on the channel (`{"blue": [[0,0],[128,118],[255,255]]}` warms);
   then `kind=vibrance {"vibrance": 20}`. Black and white: `kind=blackWhite`, then curves for punch.
5. Local: select the area (`select_ellipse`, `select_polygon`, `select_color`), `select_modify
   feather=…` (2–5 % of the area's size), add the adjustment, then `layer_addMask from=selection`
   on it. A vignette: ellipse selection, `select_invert`, feather big, exposure −0.4 masked.
6. Sharpen last, non-destructively: `layer_duplicate` the photo, `filter_apply filter=sharpen
   params={"amount": 0.6, "radius": 1}` on the copy (lower its opacity if it crunches).
7. Export only if asked: `export_file path=… quality=88` (JPEG for photos), never over the original.

## Checks

- `harness_look`: no clipped highlights (big pure-white areas) or crushed shadows, skin natural,
  horizon level. Compare with the original: `layer_update visible=false` on the adjustments,
  look, then show them again.
- `doc_overview`: the original photo layer is untouched; each change is its own named layer.
