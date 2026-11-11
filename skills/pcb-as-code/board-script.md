# Board script skeleton

A working order of passes, condensed from a 46 × 40 mm two-layer board (buck regulator,
ESP32 module, relay, sense inputs). Replace the parts, keep the order.

```python
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "tools", "hw-design", "src"))
import pcbnew  # noqa: E402
from hwdesign.kicad.board import (Board, acute_angles, drc, foreign_copper,  # noqa: E402
                                  narrow_tracks, path_length, render)

BUILD = os.path.join(HERE, "build")
PCB = os.path.join(HERE, "my-board.kicad_pcb")

b = Board(os.path.join(BUILD, "my-board.net"), track=0.25, clearance=0.2, via=(0.6, 0.3), min_drill=0.2,
          declared=os.path.join(HERE, "intended_netlist.json"), fp_dirs=[os.path.join(HERE, "lib")])
b.netclass("Power", ["+12V", "GND"], track=0.4)
b.netclass("Harness", ["LINE_A", "LINE_B"], track=0.4, clearance=0.5)   # lines that leave the box
b.outline_rounded(100, 100, 46, 40, r=1.5)

# 1. placement: datasheet-critical parts fixed, the rest near what they serve
b.place("U1", 121.0, 111.6, 270)
b.place_near("R6", *b.pad_xy("Q1", "1"))
for f in ("FID1", "FID2", "FID3"):                  # Freerouting ignores pad-local clearance
    p = b.fps[f].GetPosition()
    x, y = pcbnew.ToMM(p.x), pcbnew.ToMM(p.y)
    b.keepout(x - 1.8, y - 1.8, 3.6, 3.6, layers=(pcbnew.F_Cu,), name=f + "-clear")
assert not b.courtyard_overlaps()


# 2. hand routes: the paths the circuit depends on, pad to pad (see SKILL.md on vias)
def hand_route():
    T = b.track
    T("SW", [("U1", 6), ("C3", 2), ("D3", 1), ("L1", 1)], 0.6)   # hot loop, top layer only
    T("FB", [("R9", 2), ("R10", 1), (120.05, 110.8)], 0.25)       # may end inside a pad
    b.via("GND", 115.6, 110.8)                                     # own ground via into the plane


hand_route()
b.chamfer_corners()                                   # after ALL hand routes
b.keepout(116.2, 102.0, 15.4, 20.0, layers=(pcbnew.B_Cu,), name="buck-plane",
          tracks=True, vias=False, fills=False)       # plane stays whole, vias allowed

# 3. autoroute, then clean up in this order
b.autoroute(BUILD, passes=150)
b.fill_zones()                                        # plane pins count as open until the planes are filled
print("open after autorouting:", b.unrouted())
b.remove_redundant()                                  # autorouter duplicates of hand routes
b.straighten()                                        # micro-jogs
routed, failed = b.route_open(BUILD)                  # the connections Freerouting left, one at a time
print("finisher:", routed, "routed;", failed)         # failed: (net, reason, end, end), no free path
print("improved:", b.improve())                       # each run again: shorter, fewer vias, class width
b.widen()                                             # neck-downs back to class width
for layer in (pcbnew.F_Cu, pcbnew.B_Cu):
    b.zone("GND", layer)
b.fill_zones()
b.remove_redundant(nets=["GND"])                      # ground tracks the pours cover
b.stitch("GND", pitch=3.0)
b.prune_dangling()
if b.unrouted():                                      # the clean-up passes can open a joint again
    b.route_open(BUILD)
    b.prune_dangling()
    b.fill_zones()
assert b.unrouted() == 0, "unrouted connections"

b.save(PCB)
b.write_project(os.path.join(HERE, "my-board.kicad_pro"), min_track=0.12)
drc(PCB, os.path.join(BUILD, "drc.rpt"))

# 4. checks DRC cannot do; fail the build on any of them
pb = pcbnew.LoadBoard(PCB)
bad = []
for net, a, c, limit_mm, max_vias in [("SW", "U1.6", "L1.1", 8, 0), ("FB", "U1.3", "R10.1", 3, 0)]:
    r = path_length(pb, b.resolve(net), a, c)
    if r is None or r[0] > limit_mm or r[1] > max_vias:
        bad.append((net, a, c, r))
bad += foreign_copper(pb, (119.5, 113.3, 126.0, 117.8), {b.resolve(n) for n in ("SW", "+12V", "GND")})
bad += [n for n in narrow_tracks(pb, b.classes, b.rules["track"]) if n[-1] == "auto"]
bad += acute_angles(pb)
print("layout problems:", bad or "none")
render(PCB, os.path.join(BUILD, "front.png"), layers="F.Cu,F.SilkS,Edge.Cuts", width=2000)
render(PCB, os.path.join(BUILD, "back.png"), layers="B.Cu,Edge.Cuts", width=2000)
sys.exit(1 if bad else 0)
```

Notes:
- `track()` points are `(x, y)` in mm or `(ref, pad)`. A point inside a pad connects; it does
  not have to be the pad centre. That lets a route enter a pad straight instead of jogging to
  its centre.
- `via_near(net, ref, pad)` after autorouting adds a ground via where a small top-layer island
  would otherwise hang on one via.
- `place_near` searches outwards for the first free spot; check the result in the render.
  Fixed placement with a comment saying why is easier to review.

## Four layers, two ground domains, cut-outs

What changes against the skeleton. This ran on a 40 × 30 mm test board with hand routes
only: DRC clean, four copper Gerbers, `min_gap` 2.0 mm on `In1.Cu`. Freerouting was not part
of that run.

```python
from hwdesign.kicad.board import Board, drc, min_gap

b = Board(NET, layers=4, planes=[pcbnew.In1_Cu], declared=DECLARED)   # In1.Cu: pours only
b.outline_rounded(100, 100, 40, 30, r=1.5)
b.cutout(118.5, 106, 1.0, 12, r=0.5)      # slot inside the board; r = half the width: round ends
b.cutout(108, 99, 6, 3, r=0.5)            # reaches past the top edge: a notch, open to that edge
# placement as usual: fits() and place_near() keep courtyards out of both cut-outs

SIDE_A = ["GND", "SIG", "MID"]            # every net of each side, not only the grounds
SIDE_B = ["GNDA", "ISO"]
b.zone("GND", pcbnew.In1_Cu, rect=(100.5, 100.5, 17.5, 29))                       # (x, y, w, h)
b.zone("GNDA", pcbnew.In1_Cu, polygon=[(120, 100.5), (139.5, 100.5), (139.5, 129.5), (120, 129.5)])
b.fill_zones()
b.via_near("GND", "R2", 2)                # an SMD ground pad down to its plane
b.prune_dangling()

b.save(PCB)
drc(PCB, os.path.join(BUILD, "drc.rpt"))
pb = pcbnew.LoadBoard(PCB)                # the saved board has its pours filled
gap = min_gap(pb, [b.resolve(n) for n in SIDE_A], [b.resolve(n) for n in SIDE_B])
assert gap and gap[0] >= 2.0, "isolation: %s" % (gap,)     # (mm, layer, x, y, net_a, net_b)
```

Notes:
- `b.copper_layers` lists the enabled copper layers, front to back. `keepout()` and
  `foreign_copper()` cover all of them unless `layers=` names some; on a two-layer board
  that is front and back, as before.
- `zone()` regions are taken as given; the pour still stays clear of the board edge and of
  cut-outs. The 2.0 mm above is the distance between the two regions (118.0 to 120.0).
- `min_gap` compares copper on the same layer: pads, tracks, vias and filled pours, inner
  layers included. Its point is the middle of the narrowest place; look there in a render.
- `stitch(net)` needs the net's pour on every layer it is poured on, as before. On the other
  copper layers the via now has to clear foreign pads and tracks.
- `path_length` follows vias onto inner layers; a via counts once however many layers it
  spans.
