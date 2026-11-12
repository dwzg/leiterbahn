---
name: pcb-review
description: Work through a person's review of a generated PCB. Find their marked-up render, map each mark to board coordinates and the copper there, explain the cause before changing anything, fix it at the source, and deliver new front and back renders. Use when someone says traces look weird or messy, sends annotated screenshots of a board, circles spots on a render, or asks for pictures of the routed layers.
---

# PCB review loop

People review a board by looking at it and drawing circles. Turn every circle into a cause
and a fix, and show the result the same way they looked at it.

## 1. Find the marks without losing them

- The marked file is usually the newest image in their review folder: `find <dir> -newer
  <last render you delivered>`. They often draw on the very render you sent.
- **Never overwrite a marked-up file.** Before delivering new renders under the same name,
  move theirs aside (`round2-front-marked.png`). Their marks are the record of what they asked
  for.

## 2. Map each mark to the board

- `render()` draws the board edge to edge: `px = (x_mm - x0) * W_px / W_mm`, the same for y.
  Calibrate the offset on a known point (a fiducial, a corner) and check one other point.
- For each mark, list everything within about 1 mm: tracks (net, layer, width, ends, locked
  or autorouted), vias and pads. The snippet is in [reference.md](reference.md).
- Read the geometry, not the picture. "A hook" became "the track leaves the via 0.45 mm
  towards the edge, then turns 90° back" only once the coordinates were listed.

## 3. Name the cause, then report before fixing

Common causes, all seen on real boards ([reference.md](reference.md) has the fix for each):

| Looks like | Usually is |
|---|---|
| hook or overshoot at a via | hand route through a point past the via, then back |
| 90° corner hugging a pad row | hand route without a chamfer, turned too close to the pads |
| thin squeezed trace, zig-zag | autorouter neck-down: a part blocks the straight escape |
| small triangle | autorouter duplicate of a hand route (a loop with an acute joint) |
| trace leaving a pad corner | autorouted link to a pad, entering off-centre |
| stub and via next to a through-hole pad | autorouter went to the other layer for no reason (the pad is already on both) |
| long bottom-layer line | plane slit; usually a feed taken from the far side of the board |
| two nets crossing near their pins | pin assignment, not routing |

Report a table: spot, what is there, cause, proposed fix. Group spots by cause. When the
person asked to see the analysis first, or when a fix changes the design (a pin swap, a moved
part, a firmware constant), wait for their go-ahead.

## 4. Fix at the source

Prefer, in this order:
1. Placement or pin assignment: the cause, not the symptom.
2. A hand route, pad to pad.
3. A cleanup pass or check in the library that catches the whole class of defect next time:
   `widen`, `straighten`, `remove_redundant`, `narrow_tracks`, `acute_angles`.

Rerun the whole chain (schematic, check, board, fab). A pin swap means updating the
schematic, firmware pin map, docs and silkscreen legend together.

## 5. Look before you show

- Render the front and back, both with the pour and with tracks only. A pour hides how
  traces run; tracks only hides islands and slits. The tracks-only snippet is in
  [reference.md](reference.md).
- Check every fixed spot and its neighbours. A fix moves the autorouter's other choices: one
  straight line here pushed a harness line to the other layer next door.
- The `layout-reviewer` agent gives an independent pass.

## 6. Deliver

- Same file names as last time, in the same folder.
- Results first, one line per mark: what it was, what changed.
- Also name what is still not ideal and why: a forced detour, a trade-off.
- **If parts moved, the enclosure may have changed too** (clamping fingers avoid parts and
  fiducials). Check before they print (skill `printed-enclosure` in the plugin `gehaeuse`). Regenerate the
  fab files and say that you did.
