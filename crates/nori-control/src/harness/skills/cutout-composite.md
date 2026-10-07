---
name: cutout-composite
title: Cut out and composite
when: Remove or replace a background, put a person or product onto another picture, combine photos into one scene.
---
# Cut out and composite

Mask, never erase; match light, colour and scale; ground the subject.

## Steps

1. `doc_overview`: find the subject layer and the background layer (place the missing one with
   `layer_place path=… x y width`, only paths the person gave you). Name them (`layer_update
   name=Subject`).
2. Select the subject:
   - plain background: `select_color x y tolerance=24–48 contiguous=true` on the background,
     then `select_invert`;
   - simple forms: `select_ellipse` / `select_rect`; anything else `select_polygon points=…`
     around the outline (20+ points for curves);
   - `select_modify grow=-1 feather=1.5` to lose the fringe.
3. `layer_addMask layerId=Subject from=selection`, then `select_none`. To refine, select again and
   `layer_mask action=…` or add to the mask with another selection.
4. Arrange: `layer_reorder` (subject above background), `layer_transform scale=…` so sizes are
   believable, `layer_move` so the subject stands on the ground line of the scene.
5. Match: an adjustment clipped to the subject (`layer_addAdjustment` above it, then
   `layer_update clipped=true`): curves or exposure for brightness, hueSaturation or curves per
   channel for colour temperature, so the subject sits in the scene's light.
6. Shadow: `vector_addShape shape=ellipse` under the subject's feet or base, fill `#000000`,
   `layer_update blend=multiply opacity=0.3`, `filter_apply filter=gaussianBlur
   params={"radius": 12}`; reorder it just under the subject.

## Checks

- `harness_look`, then `page_look region=[…]` on the edges at full size: no halo of the old
  background, no hard cut-paper edge, hair and soft edges feathered.
- The subject's light comes from the same side as the scene's; its scale matches nearby objects.
- `doc_overview`: the subject layer has a mask (`mask.enabled`), the original pixels are intact.
