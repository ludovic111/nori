---
name: batch-edits
title: Make batch edits
when: The same change to many layers or pages — restyle all headings, recolour a palette, rename layers, resize or align a set, translate text.
---
# Make batch edits

Find everything first, change it in one undoable step, verify all of it.

## Steps

1. Inventory: `doc_overview` (every page's layers as a tree). Build the exact list of layer ids
   the change applies to (by kind, name, style, colour, size) and say how many there are.
2. Prefer the system over one-by-one edits:
   - text that shares a role → `text_defineStyle` once, `text_applyStyle layerIds=[…]`; changing
     the style later re-sets every layer that uses it;
   - repeated elements on every page → a master page (`page_addMaster`, `page_update master=…`).
3. Otherwise one `doc_batch` with one command per layer: `{"commands": [{"command":
   "text.update", "params": {"layerId": "L3", "color": "#1d3557"}}, …], "label": "Recolour
   headings"}` — one undo step, rolled back if any fails. Batches of up to ~50 commands; split
   bigger jobs into named batches.
4. Renames: `layer.update name=…` in a batch, with a clear scheme ("Heading 1", "Photo — Lake").
5. Positions: `layer_align layerIds=[…] to=left relativeTo=margins`, `to=distributeVertical`.

## Checks

- `doc_overview` again: every listed layer changed, nothing else did (compare counts by kind and
  style before and after).
- `harness_look` each touched page; `harness_check` for contrast if colours changed.
- Report the count ("Restyled 14 headings on 8 pages").
