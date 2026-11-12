# PCB review reference

## List the copper at a marked spot

Run with KiCad's Python. `X0, Y0` is the board's top-left edge in mm, `W_MM` its width and
`W_PX` the image width: these four set the pixel scale.

```python
import math
import pcbnew

b = pcbnew.LoadBoard("board.kicad_pcb")
mm = pcbnew.ToMM
X0, Y0, W_MM, W_PX = 100.0, 100.0, 46.0, 1600   # board origin and size, image width
s = W_PX / W_MM
spots = {1: (555, 55), 2: (552, 368)}            # pixel positions of the marks


def dist(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    L = vx * vx + vy * vy or 1e-9
    u = max(0, min(1, ((px - ax) * vx + (py - ay) * vy) / L))
    return math.hypot(px - ax - u * vx, py - ay - u * vy)


for n, (px, py) in spots.items():
    x, y = X0 + px / s, Y0 + py / s
    print(f"mark {n}: ({x:.2f}, {y:.2f})")
    for t in b.GetTracks():
        a, c = t.GetStart(), t.GetEnd()
        d = dist(x, y, mm(a.x), mm(a.y), mm(c.x), mm(c.y))
        if d < 1.0:
            print(f"  {d:.2f} {t.GetNetname()} {t.GetClass()} {b.GetLayerName(t.GetLayer())}"
                  f" ({mm(a.x):.2f},{mm(a.y):.2f})-({mm(c.x):.2f},{mm(c.y):.2f})"
                  f" {'locked' if t.IsLocked() else 'auto'}")
    for fp in b.GetFootprints():
        for p in fp.Pads():
            q = p.GetPosition()
            if math.hypot(mm(q.x) - x, mm(q.y) - y) < 1.6:
                print(f"  pad {fp.GetReference()}.{p.GetNumber()} {p.GetNetname()}")
```

Check the calibration on one mark you can identify by eye first. A scale error of a few
percent puts every mark on the wrong part.

## Tracks-only render

```python
import pcbnew
from leiterbahn.kicad.board import render

b = pcbnew.LoadBoard("board.kicad_pcb")
for z in list(b.Zones()):
    if not z.GetIsRuleArea():
        b.Remove(z)
pcbnew.SaveBoard("/tmp/tracks-only.kicad_pcb", b)
render("/tmp/tracks-only.kicad_pcb", "front-traces.png", layers="F.Cu,F.SilkS,Edge.Cuts", width=1600)
render("/tmp/tracks-only.kicad_pcb", "back-traces.png", layers="B.Cu,B.SilkS,Edge.Cuts", width=1600)
```

The back render is drawn as seen through the board from the front: same orientation as the
front, mirrored text. Say so when you deliver it.

## Causes and fixes

**Hook at a via.** The route went through a point past the via and turned back. Route from
the via straight, or at 45°, into the line it joins.

**90° corner by a pad row.** Leave the pad straight for just over the pad length, then 45°;
`chamfer_corners()` softens what is left. If a neighbouring part sits in the way, move the
part a fraction of a millimetre rather than bending the route around it.

**Neck-down and zig-zag.** A part blocks the straight escape from a pin. Move or rotate the
part. `widen()` repairs the ones where clearance allows; `narrow_tracks` reports the rest.

**Small triangle.** An autorouter duplicate of a hand-routed connection.
`remove_redundant()` removes it; `acute_angles` catches it if it comes back.

**Off-centre pad exit.** Hand-route that link and end the route at a point inside the pad, in
line with where the trace arrives.

**Stub and via next to a through-hole pad.** Usually a side effect of a neighbouring hand
route. Hand-route that net too, or move the hand route that pushed it.

**Long bottom-layer line.** Feed the load from its near side: a via at the nearest source and
a short route. Then add a track keepout (vias allowed) so the autorouter cannot bring it back.

**Crossing nets near their pins.** Swap the pin assignment when both pins can do either job
(two GPIOs with the same features, two connector pins). Update the schematic, firmware pin
map, docs and silkscreen in the same change.

**Island in the pour.** A keepout or a fence of tracks closes off a strip. Move the keepout
(often a fiducial) or open a gap in the fence.
