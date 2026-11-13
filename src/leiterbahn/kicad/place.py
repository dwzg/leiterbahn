"""Placement by connections, on both board sides.

Parts whose position matters (connectors, power parts) are fixed by hand with fix() or
centre(). The rest is placed group by group around an anchor point: first the large parts,
then every remaining part at the free spot where its pads are closest to the pads they
connect to. Coordinates are millimetres from the board's top left corner, seen from the top.

    p = Placer(b, planes=("GND",))                        # after b.outline(...)
    p.fix("J1", 4, 10)                                    # footprint origin, as Board.place()
    p.inside(["U1", "R1"], (0, 0, 30, None))              # these stay left of x = 30
    p.only(["U2"], "F")                                   # too tall for the back
    p.keep(10, 4, 12, 20)                                 # room for a hand route
    p.large(["U1", "U2"], anchor=(20, 12))
    p.small(["R1", "R2", "C1"], anchor=(20, 12))
    p.improve(tmp_dir)                                    # then move, turn and exchange them for shorter connections

It places; it does not judge a layout. Look at the render: a switching loop or a sense
resistor belongs where the circuit needs it, not where the wire length is shortest.
"""
import json
import math
import os
import subprocess

import pcbnew

from . import intent

CELL = 5.0                                # bucket size of the collision index, mm
RAIL = 5                                  # a net with more pads than this is a supply rail


def passive(ref):
    """True for resistors and capacitors (R12, C3): the parts placed last, on either side."""
    return ref[0] in "RC" and ref[1:].isdigit()


def with_lanes(box, pads, lane):
    """The box (x0, y0, x1, y1) of a part, grown so that it reaches `lane` mm beyond the outer
    end of every long pad that points out of the part. pads: the boxes of its SMD pads. A
    part with fewer than five pads, or lane 0, keeps its box."""
    if not lane or len(pads) < 5:
        return tuple(box)
    mx = sum(q[0] + q[2] for q in pads) / 2 / len(pads)
    my = sum(q[1] + q[3] for q in pads) / 2 / len(pads)
    x0, y0, x1, y1 = box
    for q in pads:
        w, h, cx, cy = q[2] - q[0], q[3] - q[1], (q[0] + q[2]) / 2, (q[1] + q[3]) / 2
        if w >= 1.4 * h and abs(cx - mx) > 0.3:             # a pad along x: out to the side it is on
            x0, x1 = (x0, max(x1, q[2] + lane)) if cx > mx else (min(x0, q[0] - lane), x1)
        elif h >= 1.4 * w and abs(cy - my) > 0.3:
            y0, y1 = (y0, max(y1, q[3] + lane)) if cy > my else (min(y0, q[1] - lane), y1)
    return (x0, y0, x1, y1)


def ease(judge, movable, most=3, passes=2, gain=0.2, many=None, say=None):
    """Let the router say which parts are in the way of the tracks, and move them out, one at a time.

    judge(out) is yours: it places the board anew, calls Placer.evict(*t) for each t of `out`
    ([(ref, x0, y0, x1, y1, sides)]) after the placement, prepares the board as for routing and
    returns Board.glance() of it (16 rounds). movable: the parts that may be moved, e.g.
    Placer.loose() of your placement. Each pass takes the `most` movable parts that stand
    beside the most cells in conflict (glance()["in_way"]), tries each alone on top of what was
    kept before, and keeps the one that leaves the fewest cells in conflict (`settled`), if
    those are fewer by the share `gain`; otherwise it stops. many(outs) may judge several at
    once (processes): by default one after the other.

    Returns (out, look, tried): what to evict, the glance with it, and [(ref or None,
    settled)] for all that was looked at. On a board of 287 parts this found the one resistor
    that made three of eight placements hard to route, in one pass of four glances each: 167,
    163 and 142 cells in conflict became 39, 12 and 36.
    """
    many = many or (lambda outs: [judge(out) for out in outs])
    out, look = [], many([[]])[0]
    tried = [(None, look["settled"])]
    for _ in range(passes):
        todo = [t for t in look["in_way"] if t[0] in movable and all(t[0] != o[0] for o in out)][:most]
        if not todo or look["settled"] == 0:
            break
        looks = many([out + [(t[0], *t[2], t[3])] for t in todo])
        tried += [(t[0], got["settled"]) for t, got in zip(todo, looks, strict=True)]
        if say:
            say("in the way: " + ", ".join("%s (%s cells left of %s)" % (t[0], got["settled"], look["settled"])
                                           for t, got in zip(todo, looks, strict=True)))
        best = min(range(len(todo)), key=lambda k: looks[k]["settled"])
        if looks[best]["settled"] > (1.0 - gain) * look["settled"]:
            break
        out, look = out + [(todo[best][0], *todo[best][2], todo[best][3])], looks[best]
    return out, look, tried


class Placer:
    """Places the footprints of a Board that has its outline.

    nets: {net: [(ref, pad)]}, default the board's declared netlist. planes: nets on a
    plane, whose pads may be anywhere. gap: extra distance between courtyards. edge: distance
    kept from the board outline. Where a part may go is said with inside(), only() and keep().
    rules: functions (ref, box, back, through) that return True where a part must not go, for
    what those three cannot say; box is the courtyard (x0, y0, x1, y1). improve() cannot see
    into a function and refuses to run with one. block: {ref: group}; a
    capacitor prefers supply pins of its own group. back_cost: what a place on the back costs,
    in mm of wire (a via, and a second assembly side): at 1.0 a resistor went under the pad
    it connects to although the front next to it was free. lane: room kept free in front of
    the pins of a part with five or more pads, in mm straight out of every long pad (as
    Board.fan_to_planes() keeps it free of vias): the part's box reaches that far. Without
    it a capacitor was put 0.2 mm in front of four pins of a 0.5 mm pitch QFN, the track to
    it ran along the row, and three of those pins never got out.
    """

    def __init__(self, board, nets=None, planes=("GND",), gap=0.2, edge=0.5, back_cost=2.5, lane=0.0):
        self.b, self.gap, self.edge, self.planes, self.back_cost = board, gap, edge, tuple(planes), back_cost
        self.lane = lane
        self.origin, self.size = (board.box[0], board.box[1]), (board.box[2], board.box[3])
        nets = board.declared if nets is None else nets
        self.nets = {n: [(r, str(p)) for r, p in pads] for n, pads in nets.items()}
        self.pins = {}                    # ref -> {pad: net}
        for net, pads in self.nets.items():
            for ref, pad in pads:
                self.pins.setdefault(ref, {})[pad] = net
        self.at = {}                      # ref -> (x, y, rot, back)
        self.boxes = {}                   # ref -> (x0, y0, x1, y1)
        self.cells = {}                   # (back, cx, cy) -> [ref]
        self.pads = {}                    # net -> [(x, y, ref, pad)] of placed parts
        self.served = {}                  # (ref, pad) -> number of capacitors placed for it
        self.rules = []
        self.keeps = []                   # (x0, y0, x1, y1, sides)
        self.areas = {}                   # ref -> [[(x0, y0, x1, y1)]]: inside one rectangle of each list
        self.bans = {}                    # ref -> [(x0, y0, x1, y1, sides)]: not in any of these
        self.planes_at = {}               # net -> [(x0, y0, x1, y1)]: where a pad on that net may lie
        self.hosts_from = 3               # a part with this many pads can be what a resistor or capacitor belongs to
        self.tied_to = {}                 # (ref, pad) -> (to, to_pad, weight), said with tie()
        self.kept = set()                 # placed with first()
        self.sides = {}                   # ref -> "F" or "B"
        self.auto = {}                    # ref -> (anchor, pull, sides, rots) of the parts near() placed
        self.block = {}
        self.pin_info = getattr(board, "pin_info", {})     # (ref, pad) -> (name, kind): what the symbols say
        self._shape = {}

    # ---- geometry of one part ------------------------------------------------
    def shape(self, ref, rot, back):
        """(courtyard (x0, y0, x1, y1), {pad: [(dx, dy)]}, through) relative to the part's origin."""
        key = (ref, rot, back)
        if key not in self._shape:
            fp = self.b.fps[ref]
            self.b.place(ref, 0, 0, rot, back)
            cy = self.b._crt(fp)
            bb = cy.BBox() if cy is not None else fp.GetBoundingBox(False)
            mm = pcbnew.ToMM
            box = (mm(bb.GetLeft()), mm(bb.GetTop()), mm(bb.GetRight()), mm(bb.GetBottom()))
            pads = {}
            for p in fp.Pads():
                pads.setdefault(p.GetNumber(), []).append((mm(p.GetPosition().x), mm(p.GetPosition().y)))
            smd = [p.GetBoundingBox() for p in fp.Pads() if not p.GetDrillSize().x and p.IsOnCopperLayer()]
            box = with_lanes(box, [(mm(q.GetLeft()), mm(q.GetTop()), mm(q.GetRight()), mm(q.GetBottom())) for q in smd],
                             self.lane)
            self._shape[key] = (box, pads, self.b._through(fp))
            if ref in self.at:                              # asked about a placed part: put it back
                x, y, r, bk = self.at[ref]
                self.b.place(ref, self.origin[0] + x, self.origin[1] + y, r, bk)
        return self._shape[key]

    def area(self, ref):
        x0, y0, x1, y1 = self.shape(ref, 0, False)[0]
        return (x1 - x0) * (y1 - y0)

    # ---- collision index -----------------------------------------------------
    def _cells(self, box, back):
        for cx in range(int(box[0] // CELL), int(box[2] // CELL) + 1):
            for cy in range(int(box[1] // CELL), int(box[3] // CELL) + 1):
                yield (back, cx, cy)

    def free(self, ref, x, y, rot=0, back=False):
        """True if ref fits at (x, y): on the board, allowed by the rules, clear of placed parts.

        Parts on opposite sides only collide if one of them has through-hole pads.
        """
        box, _pads, through = self.shape(ref, rot, back)
        box = (x + box[0], y + box[1], x + box[2], y + box[3])
        e = self.edge
        if box[0] < e or box[1] < e or box[2] > self.size[0] - e or box[3] > self.size[1] - e:
            return False
        if self.sides.get(ref, "FB").find("B" if back else "F") < 0:
            return False
        for group in self.areas.get(ref, ()):
            if not any(r[0] <= box[0] and r[1] <= box[1] and box[2] <= r[2] and box[3] <= r[3] for r in group):
                return False
        for pad, net in self.pins.get(ref, {}).items() if self.planes_at else ():
            rects = self.planes_at.get(net)
            if rects and not all(any(r[0] <= x + dx <= r[2] and r[1] <= y + dy <= r[3] for r in rects)
                                 for dx, dy in _pads.get(pad, ())):
                return False
        mine = "FB" if through else "B" if back else "F"
        for x0, y0, x1, y1, sides in self.keeps + self.bans.get(ref, []):
            if any(c in sides for c in mine) and box[0] < x1 and x0 < box[2] and box[1] < y1 and y0 < box[3]:
                return False
        if any(rule(ref, box, back, through) for rule in self.rules):
            return False
        g = self.gap
        for side in ((False, True) if through else (back,)):
            for cell in self._cells(box, side):
                for other in self.cells.get(cell, ()):
                    o = self.boxes[other]
                    if box[0] < o[2] + g and o[0] < box[2] + g and box[1] < o[3] + g and o[1] < box[3] + g:
                        return False
        return True

    def keep(self, x0, y0, x1, y1, sides="F"):
        """Reserve a rectangle on the front ("F"), the back ("B") or both ("FB"), e.g. for a hand route."""
        self.keeps.append((x0, y0, x1, y1, sides))

    def inside(self, refs, *rects):
        """Each of refs must lie with its whole courtyard inside one of the rectangles
        (x0, y0, x1, y1); None for a side that is open. Called again for a part, both hold."""
        far = 1e9
        group = [tuple(v if v is not None else (-far if k < 2 else far) for k, v in enumerate(r)) for r in rects]
        for ref in refs:
            self.areas.setdefault(ref, []).append(group)

    def outside(self, refs, x0, y0, x1, y1, sides="FB"):
        """None of refs may reach into the rectangle on the front ("F"), the back ("B") or
        both: keep() for these parts alone. For a part the router found in the way of the
        tracks at a place (see evict())."""
        for ref in refs:
            self.bans.setdefault(ref, []).append((x0, y0, x1, y1, sides))

    def over(self, net, *rects):
        """Every pad on `net` lies within one of the rectangles (x0, y0, x1, y1; None: open).

        For a plane that covers only part of the board: a pad over another net's plane
        gets no via into its own, and the router joins it with a thin track (a 100 uF
        capacitor hung on 10 mm of 0.127 mm track that way). A part with pads on two such
        nets then sits across the border between them. Holds for near() and improve();
        parts set with fix() are yours.
        """
        big = 1e9
        self.planes_at[net] = [tuple(v if v is not None else s * big for v, s in zip(r, (-1, -1, 1, 1), strict=True))
                               for r in rects]

    def only(self, refs, side):
        """refs go on the front ("F") or on the back ("B") only."""
        for ref in refs:
            self.sides[ref] = side

    # ---- placing by hand -------------------------------------------------------
    def fix(self, ref, x, y, rot=0, back=False):
        """Put ref's footprint origin at (x, y). Rules, gap and edge do not apply: you decided."""
        if ref in self.at:
            raise ValueError(ref + " is placed already")
        box, pads, through = self.shape(ref, rot, back)
        self.b.place(ref, self.origin[0] + x, self.origin[1] + y, rot, back)
        self.at[ref] = (x, y, rot, back)
        self.boxes[ref] = (x + box[0], y + box[1], x + box[2], y + box[3])
        for side in ((False, True) if through else (back,)):
            for cell in self._cells(self.boxes[ref], side):
                self.cells.setdefault(cell, []).append(ref)
        for pad, net in self.pins.get(ref, {}).items():
            for dx, dy in pads.get(pad, ()):
                self.pads.setdefault(net, []).append((x + dx, y + dy, ref, pad))

    def centre(self, ref, cx, cy, rot=0, back=False):
        """Put the middle of ref's courtyard at (cx, cy): handy for connectors, whose origin is pin 1."""
        (x0, y0, x1, y1), _p, _t = self.shape(ref, rot, back)
        self.fix(ref, cx - (x0 + x1) / 2, cy - (y0 + y1) / 2, rot, back)

    def pad(self, ref, pad):
        """Position of a placed part's pad."""
        x, y, rot, back = self.at[ref]
        dx, dy = self.shape(ref, rot, back)[1][str(pad)][0]
        return (x + dx, y + dy)

    def find(self, net, other=None, block=None):
        """The one two-pad part between `net` and `other`, or the one on `net` whose group name
        (a string, or a tuple ending in one) starts with `block`."""
        found = []
        for ref, _pad in self.nets[net]:
            mine = self.pins[ref]
            if len(mine) != 2 or (other is not None and other not in mine.values()):
                continue
            group = self.block.get(ref, "")
            if block is not None and not (group if isinstance(group, str) else group[-1]).startswith(block):
                continue
            found.append(ref)
        if len(found) != 1:
            raise LookupError("expected one part on %s (%s, %s), found %s" % (net, other, block, found))
        return found[0]

    def face(self, ref, net, side, back=False):
        """The rotation that puts ref's pad on `net` towards "left", "right", "up" or "down"."""
        pad = [k for k, n in self.pins[ref].items() if n == net][0]
        for rot in (0, 90, 180, 270):
            dx, dy = self.shape(ref, rot, back)[1][pad][0]
            if {"left": dx < -0.1, "right": dx > 0.1, "up": dy < -0.1, "down": dy > 0.1}[side]:
                return rot
        raise ValueError("%s: pad on %s is in the middle" % (ref, net))

    # ---- placing by connections ------------------------------------------------
    def _targets(self, ref, net):
        """Placed pads of `net` this part wants to be close to."""
        found = self.pads.get(net, ())
        if len(self.nets[net]) > RAIL:                    # go to a pin that uses the rail, not to another capacitor
            found = [t for t in found if not passive(t[2])]
            same = [t for t in found if self.block.get(t[2]) == self.block.get(ref)]
            found = same or found
        return found

    def cost(self, ref, x, y, rot, back, anchor, pull):
        """(cost in mm of wire, supply pins this spot would decouple). Lower is better."""
        _box, pads, _t = self.shape(ref, rot, back)
        total, hits = 0.0, []
        mine = self.pins.get(ref, {})
        signal = [n for n in mine.values() if n not in self.planes and len(self.nets[n]) <= RAIL]
        for pad, net in mine.items():
            if net in self.planes or pad not in pads:
                continue
            targets = self._targets(ref, net)
            if not targets:
                continue
            rail = len(self.nets[net]) > RAIL
            px, py = x + pads[pad][0][0], y + pads[pad][0][1]
            best = None
            for tx, ty, tref, tpad in targets:
                d = abs(px - tx) + abs(py - ty)
                if rail:
                    d += 4.0 * self.served.get((tref, tpad), 0)     # that pin has its capacitor: try the next
                if best is None or d < best[0]:
                    best = (d, tref, tpad)
            total += best[0] * (0.25 if rail and signal else 1.0)
            if rail and not signal:
                hits.append((best[1], best[2]))
        x0, y0, x1, y1 = self.shape(ref, rot, back)[0]
        total += pull * (abs(x + (x0 + x1) / 2 - anchor[0]) + abs(y + (y0 + y1) / 2 - anchor[1]))
        return total + (self.back_cost if back else 0.0), hits

    def _start(self, ref, anchor):
        """Where the search for ref begins: between the pads its signals go to; for a part on
        supply nets only (a decoupling capacitor) at the supply pin nearest the anchor that
        has no capacitor yet. Between two supply pins is the wrong place for it."""
        mine = [n for n in self.pins.get(ref, {}).values() if n not in self.planes]
        signal = [n for n in mine if len(self.nets[n]) <= RAIL]
        pts = [t for net in (signal or mine) for t in self._targets(ref, net)]
        if not pts:
            return anchor

        def far(t):
            return abs(t[0] - anchor[0]) + abs(t[1] - anchor[1])
        if not signal:
            t = min(pts, key=lambda t: 4.0 * self.served.get((t[2], t[3]), 0) + far(t))
            return (t[0], t[1])
        pts = sorted(pts, key=far)[:6]                      # many candidates: those nearest the anchor
        return (sum(t[0] for t in pts) / len(pts), sum(t[1] for t in pts) / len(pts))

    def near(self, ref, anchor, start=None, sides=(False, True), rots=(0, 90, 180, 270), pull=0.15,
             step=0.5, reach=3.0, max_r=45.0):
        """Place ref at the cheapest free spot around `start` (default: where its nets pull it).

        Searches outwards in rings of `step` mm and stops `reach` mm beyond the first free
        spot. sides: False front, True back. Returns (cost, x, y, rot, back).
        """
        if start is None:
            start = self._start(ref, anchor)
        sx, sy = round(start[0] / step) * step, round(start[1] / step) * step
        best, first = None, None
        for ring in range(int(max_r / step) + 1):
            if first is not None and ring * step > first + reach:
                break
            spots = [(0, 0)] if ring == 0 else \
                [(i, -ring) for i in range(-ring, ring)] + [(ring, i) for i in range(-ring, ring)] + \
                [(i, ring) for i in range(ring, -ring, -1)] + [(-ring, i) for i in range(ring, -ring, -1)]
            for i, j in spots:
                x, y = sx + i * step, sy + j * step
                for back in sides:
                    for rot in rots:
                        if not self.free(ref, x, y, rot, back):
                            continue
                        if first is None:
                            first = ring * step
                        c, hits = self.cost(ref, x, y, rot, back, anchor, pull)
                        if best is None or c < best[0]:
                            best = (c, x, y, rot, back, hits)
        if best is None:
            raise RuntimeError("no room for %s within %.0f mm of (%.1f, %.1f)" % (ref, max_r, sx, sy))
        c, x, y, rot, back, hits = best
        self.fix(ref, x, y, rot, back)
        self.auto[ref] = (tuple(anchor), pull, tuple(sides), tuple(rots))
        for h in hits:
            self.served[h] = self.served.get(h, 0) + 1
        return c, x, y, rot, back

    def big(self, ref):
        """Placed in the first pass: everything but small resistors, capacitors and test points."""
        return not ref.startswith("TP") and (not passive(ref) or self.area(ref) > 12.0)

    def bulk(self, ref):
        """A capacitor too large to be a part's own (see ties())."""
        return ref[0] == "C" and self.big(ref)

    def decouples(self, ref):
        """A capacitor that can be a supply pin's own."""
        return ref[0] == "C" and not self.bulk(ref)

    def linked(self, ref):
        """How many of ref's signal pads have a placed counterpart."""
        return sum(1 for net in self.pins.get(ref, {}).values()
                   if net not in self.planes and len(self.nets[net]) <= RAIL and self.pads.get(net))

    def large(self, refs, anchor, sides=(False,), pull=0.6):
        """First pass: the unplaced large parts of refs around anchor, largest first.

        Run it for every group before small(): a transformer finds no room once the
        resistors of other groups have taken the front.
        """
        for ref in sorted((r for r in refs if r not in self.at and self.big(r)), key=lambda r: (-self.area(r), r)):
            self.near(ref, anchor, start=anchor, sides=sides if not passive(ref) else (False, True), pull=pull)

    def small(self, refs, anchor, sides=(False, True), pull=0.15):
        """Second pass: the rest of refs, always the part with the most connections to placed parts."""
        rest = [r for r in refs if r not in self.at]
        while rest:
            ref = max(rest, key=lambda r: (self.linked(r), self.area(r), r))
            self.near(ref, anchor, sides=sides, pull=pull)
            rest.remove(ref)

    def length(self):
        """Half-perimeter wire length of all nets off the planes, mm: a figure to compare placements by."""
        total = 0.0
        for net, pads in self.pads.items():
            if net in self.planes or len(pads) < 2:
                continue
            xs, ys = [q[0] for q in pads], [q[1] for q in pads]
            total += max(xs) - min(xs) + max(ys) - min(ys)
        return total

    # ---- improving what is placed ------------------------------------------------
    def _lift(self, ref):
        x, y, rot, back = self.at.pop(ref)
        box = self.boxes.pop(ref)
        for side in (False, True):
            for cell in self._cells(box, side):
                if ref in self.cells.get(cell, ()):
                    self.cells[cell].remove(ref)
        for net in set(self.pins.get(ref, {}).values()):
            self.pads[net] = [t for t in self.pads.get(net, ()) if t[2] != ref]

    def problem(self, moves=20000, seed=1, weights=None, spread=False, **more):
        """The placement as a problem for `leiterbahn-route place` (router/src/place.rs).

        The parts near() placed may move, to the sides and rotations it was allowed; what was
        fixed by hand stays. A net with up to 5 pads counts with the half perimeter of the box
        around its pads, times its weight. Resistors and capacitors are tied to the pins they
        belong at (see ties()), and pads on a net given to over() stay within its rectangles.
        """
        weights = weights or {}
        refs = sorted(self.at)
        index = {ref: i for i, ref in enumerate(refs)}
        parts = []
        for ref in refs:
            x, y, rot, back = self.at[ref]
            auto = self.auto.get(ref)
            combos = {(rot % 360, back)}
            if auto:
                may = self.sides.get(ref, "FB")
                combos |= {(r % 360, bk) for bk in auto[2] for r in auto[3] if ("B" if bk else "F") in may}
            shapes, through = {}, False
            for r, bk in sorted(combos):
                box, pads, through = self.shape(ref, r, bk)
                key = "%d/%s" % (r if auto else 0, "B" if bk else "F")
                mine = self.pins.get(ref, {})
                shapes[key] = dict(box=list(box), pads={k: list(v[0]) for k, v in pads.items() if k in mine})
            part = dict(name=ref, at=[x, y, rot % 360 if auto else 0, back], fixed=not auto, through=through,
                        shapes=shapes, inside=[[list(r) for r in group] for group in self.areas.get(ref, ())],
                        outside=[dict(box=list(k[:4]), sides=k[4]) for k in self.bans.get(ref, ())])
            if auto:
                part.update(anchor=list(auto[0]), pull=auto[1])
            parts.append(part)

        def pin(ref, pad):
            return ref in index and pad in next(iter(parts[index[ref]]["shapes"].values()))["pads"]
        nets = []
        for net in sorted(self.nets):
            pins = [(ref, pad) for ref, pad in self.nets[net] if pin(ref, pad)]
            if net in self.planes or len(pins) < 2:
                continue
            if len(self.nets[net]) <= RAIL or not any(not passive(r) for r, _ in pins):
                nets.append(dict(name=net, pins=[[index[r], p] for r, p in pins],
                                 weight=weights.get(net, 1.0) * (1.0 if len(self.nets[net]) <= RAIL else 0.25)))
        tethers = [{"from": [index[ref], pad], "to": [index[to], to_pad], "weight": w}
                   for ref, pad, to, to_pad, w in self.ties() if ref in self.auto and pin(ref, pad) and pin(to, to_pad)]
        for part in parts:
            mine = self.pins.get(part["name"], {})
            part["over"] = [[pad, [list(r) for r in self.planes_at[mine[pad]]]] for pad in sorted(mine)
                            if mine[pad] in self.planes_at and pad in next(iter(part["shapes"].values()))["pads"]]
        return dict(size=list(self.size), edge=self.edge, gap=self.gap, back_cost=self.back_cost, moves=moves,
                    seed=seed, keeps=[dict(box=list(k[:4]), sides=k[4]) for k in self.keeps], parts=parts, nets=nets,
                    tethers=tethers, spread=bool(spread), **more)

    def ties(self):
        """Which pad of a resistor or capacitor belongs at which pin: [(ref, pad, to, to_pad, weight)].

        A part whose nets off the planes all end at one part with three or more pads belongs
        to that part (a boot capacitor between two pins of a controller, a timing resistor
        from a pin to ground, a supply capacitor): each such pad is tied to that part's
        nearest pin on its net, a capacitor at 3, a resistor at 1.5. On a supply rail the
        part it belongs to is the nearest that uses the rail, of its own block if there is
        one; of the capacitors between a rail and a plane each supply pin gets one before any
        gets a second. A part with no such host is tied by its rail pad only, at a quarter. Test
        points and other parts with fewer than three pads are no hosts: capacitors had
        gathered at the test point of their rail, 20 mm from the regulator.

        A large capacitor (see bulk()) is another matter: it belongs
        where the current of its net flows, and that is the largest pad on the net (a
        transistor's drain, an inductor, a diode), not the nearest pin of an integrated
        circuit. Tied to the nearest, a 100 uF capacitor of a converter's output went to the
        controller's sense pin, 32 mm from the transistor. What tie() says replaces all this
        for its pad.
        """
        def far(a, b):
            (ax, ay), (bx, by) = self.pad(*a), self.pad(*b)
            return abs(ax - bx) + abs(ay - by)
        hosts = {}                                         # net -> [(ref, pad)] of parts that can be a host
        for net, pads in self.nets.items():
            if net not in self.planes:
                hosts[net] = [(r, p) for r, p in pads
                              if r in self.at and not passive(r) and len(self.pins.get(r, ())) >= self.hosts_from]
        found = [(ref, pad, to, to_pad, w) for (ref, pad), (to, to_pad, w) in sorted(self.tied_to.items())
                 if ref in self.at and to in self.at]
        # Capacitors between one rail and a plane: every supply pin gets one before any gets a second, the
        # nearest pairs first. (Each to its nearest pin, two shared a pin and the next pin had none within 10 mm.)
        own, waiting = {}, {}
        for ref in sorted(r for r in self.at if passive(r)):
            mine = [(pad, net) for pad, net in sorted(self.pins.get(ref, {}).items()) if net not in self.planes]
            if (len(mine) == 1 and self.decouples(ref) and len(self.nets[mine[0][1]]) > RAIL
                    and (ref, mine[0][0]) not in self.tied_to):
                waiting.setdefault(mine[0][1], []).append((ref, mine[0][0]))
        for net, caps in sorted(waiting.items()):
            pairs = []
            for cap in caps:
                users = hosts.get(net, ())
                # the pins the symbols call supply inputs, if they say: an enable input tied to the rail
                # had taken the capacitor of a converter's supply pin, which then had none within 5 mm
                users = [t for t in users if self.pin_info.get(t, ("", ""))[1].startswith("power_in")] or users
                near = [t for t in users if self.block.get(t[0]) == self.block.get(cap[0])] or users
                pairs += [(far(cap, t), cap, t) for t in near]
            taken = set()
            for _d, cap, t in sorted(pairs):
                if cap not in own and t not in taken:
                    own[cap] = t
                    taken.add(t)
            for _d, cap, t in sorted(pairs):               # more capacitors than pins: the rest to their nearest
                own.setdefault(cap, t)
        for ref in sorted(r for r in self.at if passive(r)):
            mine = [(pad, net) for pad, net in sorted(self.pins.get(ref, {}).items())
                    if net not in self.planes and (ref, pad) not in self.tied_to]
            if not mine:
                continue
            if len(mine) == 1 and (ref, mine[0][0]) in own:
                to = own[(ref, mine[0][0])]
                found.append((ref, mine[0][0], to[0], to[1], 3.0))
                continue
            if self.bulk(ref):
                for pad, net in mine:                      # to the largest pad on its net
                    power = [(self._pad_area(r, q), r, q) for r, q in self.nets[net]
                             if r in self.at and not passive(r) and len(self.pins.get(r, ())) >= 2]
                    if power:
                        most = max(a for a, _, _ in power)
                        to = min((t for t in power if t[0] >= 0.8 * most),
                                 key=lambda t: (far((ref, pad), t[1:]), t[1:]))
                        found.append((ref, pad, to[1], to[2], 3.0))
                continue
            # A part on the net of a pin that takes up what is near it (FB, COMP, an amplifier's input: see
            # intent.QUIET) belongs at that pin, whatever its other end is on: such a node was 30 mm long
            # with its resistor at the far end's part.
            quiet = [(pad, t) for pad, net in mine if len(self.nets[net]) <= 8
                     for t in hosts.get(net, ()) if intent.QUIET.match(self.pin_info.get(t, ("", ""))[0])]
            if quiet and len({pad for pad, _t in quiet}) == 1:
                pad, to = min(quiet, key=lambda q: (far((ref, q[0]), q[1]), q[1]))
                found.append((ref, pad, to[0], to[1], 3.0))
                continue
            common = None
            for _pad, net in mine:
                here = {r for r, _ in hosts.get(net, ())}
                common = here if common is None else common & here
            same = {r for r in common if self.block.get(r) == self.block.get(ref)} or common
            if same:
                host = min(same, key=lambda r: (min(far((ref, pad), t) for pad, net in mine
                                                    for t in hosts[net] if t[0] == r), r))
                for pad, net in mine:
                    to = min((t for t in hosts[net] if t[0] == host), key=lambda t: (far((ref, pad), t), t))
                    found.append((ref, pad, to[0], to[1], 3.0 if ref[0] == "C" else 1.5))
                continue
            for pad, net in mine:                          # no host: the rail it is on holds it a little
                users = hosts.get(net, ())
                if len(self.nets[net]) > RAIL and users:
                    near = [t for t in users if self.block.get(t[0]) == self.block.get(ref)] or users
                    to = min(near, key=lambda t: (far((ref, pad), t), t))
                    found.append((ref, pad, to[0], to[1], 0.25))
        return found

    def _pad_area(self, ref, pad):
        return sum(pcbnew.ToMM(p.GetSize().x) * pcbnew.ToMM(p.GetSize().y)
                   for p in self.b.fps[ref].Pads() if p.GetNumber() == str(pad))

    def tie(self, ref, pad, to, to_pad, weight=3.0):
        """Say where a pad belongs: pad `pad` of ref at pad `to_pad` of `to`. improve() pulls it
        there with `weight` mm of cost per mm of distance, strays() lists it when it is far."""
        self.tied_to[(ref, str(pad))] = (to, str(to_pad), weight)

    def first(self, refs, sides=(False, True), reach=1.5):
        """Place these now, in this order, each as near as it gets to the pins it belongs at
        (see ties(); a part with none goes where its nets pull it), and keep them there:
        improve() does not move them. For the few parts that must win the room next to a
        fine-pitch part: with everything tied to its pins at the same weight, a controller's
        supply capacitor ended 5 mm out behind a divider that could have been anywhere.
        """
        for ref in refs:
            to = [self.pad(t[2], t[3]) for t in self.ties_of(ref)]
            start = (sum(x for x, _ in to) / len(to), sum(y for _, y in to) / len(to)) if to else None
            self.near(ref, start or (self.size[0] / 2, self.size[1] / 2), start=start, sides=sides, pull=0.0,
                      step=0.25, reach=reach)
            del self.auto[ref]
            self.kept.add(ref)

    def ties_of(self, ref):
        """ties() of one part that is not placed yet or placed: where its pads belong."""
        placed = ref in self.at
        if not placed:
            self.at[ref] = (0.0, 0.0, 0, False)            # ties() asks for distances to choose the nearer pin
        try:
            return [t for t in self.ties() if t[0] == ref]
        finally:
            if not placed:
                del self.at[ref]

    def unmet(self, **limits):
        """What the placement does not give the circuit, from what the symbols say of the pins: a supply
        pin without a capacitor near, a feedback or input node that reaches far, a boot capacitor far
        from its pin. [(kind, ref, pad, net, mm, part, text)], worst first: see intent.findings()."""
        return intent.findings(self.pin_info, self.nets, lambda r, q: self.pad(r, q) if r in self.at else None,
                               **limits)

    def mend(self, sides=(False, True), **limits):
        """Move what unmet() lists to the pin it belongs at, as far as there is room nearer than where
        it lies: after improve(), which weighs all wishes against each other and leaves a few unmet
        that a free spot beside the pin would settle. Only parts that near() or improve() placed are
        moved, and a move is kept only if all that is unmet, summed in mm, got less by it: a
        resistor between the inputs of two parts 28 mm apart was moved to the one and the other
        then reached 29 mm. Returns [(part, ref, pad, mm before, mm after)]. limits as
        intent.findings()."""
        def lack():
            return sum(min(f[4], 50.0) for f in self.unmet(**limits))
        moved, tried = [], set()
        while True:
            todo = [f for f in self.unmet(**limits) if f[5] in self.auto and (f[1], f[2], f[5]) not in tried]
            if not todo:
                return moved
            _kind, ref, pad, net, was, part, _text = todo[0]
            tried.add((ref, pad, part))
            mine = [q for q, n in self.pins.get(part, {}).items() if n == net]
            pin, before = self.pad(ref, pad), lack()
            old, auto, served = self.at[part], self.auto[part], dict(self.served)
            self._lift(part)
            try:
                self.near(part, pin, start=pin, sides=sides, pull=0.0, step=0.25, reach=1.0, max_r=was)
                x, y = self.pad(part, mine[0])
                now = math.hypot(x - pin[0], y - pin[1])
                better = lack() < before - 0.5
            except RuntimeError:
                better = False
            if not better:                                 # no better place: back where it was
                if part in self.at:
                    self._lift(part)
                self.fix(part, *old)
                self.served = served
            else:
                moved.append((part, ref, pad, round(was, 1), round(now, 1)))
            self.auto[part] = auto

    def evict(self, ref, x0, y0, x1, y1, sides="FB", reach=1.0, max_r=10.0):
        """Move a part that near() or improve() placed out of the rectangle on those sides, to
        the cheapest free place near where it lay (the other side, where it may go there), and
        keep it out (outside()).

        For a part that stands in the way of the tracks: Board.glance() names those. On a board
        of 287 parts, three of eight placements still had 142 to 167 cells in conflict after 16
        rounds and five had 0 to 57. The three had one resistor on the back below a row of the
        controller's pins, between the capacitors that belong there; with it moved to the
        front they had 12 to 39. Wire length, ties and the density of pads were the same in
        all eight: only the router saw it. Returns (x, y, rot, back), or None if there is no
        room within max_r mm: then the part stays and may stay.
        """
        old, auto, served = self.at[ref], self.auto[ref], dict(self.served)
        self._lift(ref)
        self.bans.setdefault(ref, []).append((x0, y0, x1, y1, sides))
        try:
            self.near(ref, auto[0], start=old[:2], sides=auto[2], rots=auto[3], pull=auto[1], step=0.25,
                      reach=reach, max_r=max_r)
        except RuntimeError:
            self.bans[ref].pop()
            self.fix(ref, *old)
            self.served = served
        self.auto[ref] = auto
        return None if self.at[ref] == old else self.at[ref]

    def loose(self, limit=3.0):
        """The parts near() or improve() placed that no tie as strong as `limit` holds at a pin:
        resistors and the like, not the capacitors at supply pins. What ease() may move."""
        held = {t[0] for t in self.ties() if t[4] >= limit}
        return sorted(ref for ref in self.auto if ref not in held)

    def strays(self, limit=3.0):
        """The resistors and capacitors further than `limit` mm from the pin they belong at
        (see ties()), of those near() or first() placed: [(mm, ref, pad, to, to_pad)], the furthest
        first. Print it after placing:
        a board routes just as well with a boot capacitor 8 mm from its pins."""
        out = []
        for ref, pad, to, to_pad, weight in self.ties():
            (ax, ay), (bx, by) = self.pad(ref, pad), self.pad(to, to_pad)
            d = math.hypot(ax - bx, ay - by)
            placed = ref in self.auto or ref in self.kept          # where a part set with fix() lies was decided
            if weight >= 1.0 and d > limit and placed:
                out.append((round(d, 2), ref, pad, to, to_pad))
        return sorted(out, reverse=True)

    def improve(self, tmp_dir, moves=20000, seed=1, weights=None, say=None, spread=False, **more):
        """Move, turn and exchange the parts near() placed where that shortens the connections.

        Simulated annealing in `leiterbahn-route place`: `moves` tries per part, the same
        result for the same `seed`. weights: {net: number} for nets that matter more. The
        rules of inside(), only() and keep() hold, as do gap and edge; parts fixed by hand
        stay. Returns dict(before, started, after: the cost in mm and what it is made of,
        moved, tried, kept). The cost does not know whether the board can be routed: route
        the result to judge it.

        spread: do not begin from where near() put the parts but from where forces put them:
        connections pull, courtyards push each other apart like charges, then every part is
        set down at an allowed place close by (router/src/place/spread.rs). The sides stay as
        near() chose them. On a board of 287 parts the cost ended 9 % lower than without and
        the nets 15 % shorter, with every one of eight seeds.

        more: other fields of the problem (router/src/place.rs), as warm=0.3, reach=3.0 for a
        run that starts cool and moves parts by 3 mm at most.
        """
        from .negotiate import binary
        if self.rules:
            raise ValueError("improve() cannot see into rules given as functions: use inside(), only(), keep()")
        os.makedirs(tmp_dir, exist_ok=True)
        src, out = os.path.join(tmp_dir, "place-problem.json"), os.path.join(tmp_dir, "place-result.json")
        with open(src, "w") as f:
            json.dump(self.problem(moves, seed, weights, spread, **more), f)
        done = subprocess.run([binary(), "place", src, out], capture_output=True, text=True)
        if done.returncode:
            raise RuntimeError("leiterbahn-route place failed: " + done.stderr[-2000:])
        if say:
            for line in done.stderr.splitlines():
                say(line)
        with open(out) as f:
            answer = json.load(f)
        keep = {ref: self.auto[ref] for ref, *_ in answer["parts"]}
        for ref, *_ in answer["parts"]:
            self._lift(ref)
        for ref, x, y, rot, back in answer["parts"]:
            self.fix(ref, x, y, rot, back)
        self.auto.update(keep)
        return {k: answer[k] for k in ("before", "started", "after", "moved", "tried", "kept")}
