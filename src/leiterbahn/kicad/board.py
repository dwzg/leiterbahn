"""Build a KiCad PCB from a schematic netlist with the pcbnew Python API.

Flow: read the KiCad netlist (kicad-cli sch export netlist --format kicadsexpr),
load and place footprints, assign nets, draw the outline, autoroute with
Freerouting (Specctra DSN/SES), pour zones, save, then DRC with kicad-cli.

Boards have 2, 4 or 6 copper layers (Board(layers=...)). Inner layers listed in
`planes` get KiCad's layer type "power": the Specctra export then writes
`(layer In1.Cu (type power))` instead of `(type signal)`, the DSN's mark for a
plane layer that is not for routing. Pours on any layer are exported as
`(plane <net> (polygon <layer> ...))`. Both were read in DSN files exported by
KiCad 10.0.8; how Freerouting routes such a board has not been run yet.
"""
import hashlib
import heapq
import json
import math
import os
import re
import subprocess
import sys
import zlib

import pcbnew

from . import planar
from .netlist import read_netlist  # noqa: F401  (re-exported)

FP_DIR = os.environ.get("KICAD_FOOTPRINT_DIR", "/usr/share/kicad/footprints")
MM = pcbnew.FromMM


def repeatable():
    """Call first in a board script: the same script then gives the same board every time.

    Python orders its sets by a hash that changes with every start; the finishing passes
    walk such sets, and two runs of one script left different connections open (and rework
    drawn for the one result met the other). This starts the script again with
    PYTHONHASHSEED=0 unless it is set. KiCad's item ids, random as well, are seeded by Board().
    """
    if os.environ.get("PYTHONHASHSEED") is None:
        os.environ["PYTHONHASHSEED"] = "0"
        os.execv(sys.executable, [sys.executable] + sys.argv)


def copper_layers(board):
    """Enabled copper layers of a pcbnew BOARD, front to back: [F_Cu, In1_Cu, ..., B_Cu]."""
    return [int(l) for l in board.GetEnabledLayers().CuStack()]


class Board:
    """A board built from a netlist. layers is the copper layer count (2, 4 or 6); planes lists
    inner layers (pcbnew.In1_Cu, ...) reserved for pours; copper_layers holds the enabled
    copper layers, front to back.

    A plane layer gets KiCad's layer type "power". The Specctra export passes that on as
    `(layer In1.Cu (type power))`, the DSN's mark for a layer that is not for routing (the
    type "mixed" is exported as `signal`, like any routing layer).
    """

    def __init__(self, netlist_path, track=0.3, clearance=0.2, via=(0.6, 0.3), min_drill=0.3,
                 declared=None, route_margin=0.02, fp_dirs=(), layers=2, planes=(), seed=1):
        if layers not in (2, 4, 6):
            raise ValueError("layers must be 2, 4 or 6, not %r" % (layers,))
        pcbnew.KIID.SeedGenerator(seed)                     # the same ids for the same script: see repeatable()
        self.fp_dirs = list(fp_dirs)
        self.comps, self.nets = read_netlist(netlist_path)
        self.declared = {}
        if declared:
            with open(declared) as f:
                self.declared = json.load(f)
        self.board = pcbnew.BOARD()
        ds = self.board.GetDesignSettings()
        ds.SetCopperLayerCount(layers)
        self.copper_layers = copper_layers(self.board)
        self.planes = list(planes)
        self.on_plane = set()                               # (ref, pad) tied to a plane: see fan_to_planes()
        for l in self.planes:
            if l not in self.copper_layers[1:-1]:
                raise ValueError("planes must be inner copper layers of a %d-layer board: %r" % (layers, l))
            self.board.SetLayerType(l, pcbnew.LT_POWER)
        ds.m_MinThroughDrill = MM(min_drill)
        self.rules = dict(track=track, clearance=clearance, via=via, min_drill=min_drill)
        self.classes = {}
        self.ns = ds.m_NetSettings
        d = self.ns.GetDefaultNetclass()
        d.SetTrackWidth(MM(track))
        d.SetClearance(MM(clearance + route_margin))   # route a hair wider than the DRC rule
        d.SetViaDiameter(MM(via[0]))
        d.SetViaDrill(MM(via[1]))
        self.fps = {}
        self.netinfo = {}
        self._stitch_vias = []
        self.placed = set()
        self.cutouts = []            # (x, y, w, h) of slots and notches cut out of the board
        self._contour, self._slots, self._edge_shapes, self._corner = [], [], [], 0.0
        self._load()

    @classmethod
    def reopen(cls, pcb_path, track=0.3, clearance=0.2, via=(0.6, 0.3), min_drill=0.3, declared=None, planes=()):
        """A saved board, for rework after routing: rip_up(), track(), via(), route_open(),
        prune_dangling(), fill_zones(), unrouted(), save().

        Give the rules the board was built with, and call netclass() again for its net
        classes: neither is read back from the file. What belongs to building a board
        (outline, placement, autoroute) is not for a reopened one.
        Rework on the saved file starts from the same board every time, and takes minutes
        where the run that built the board takes its autorouter's time again.
        """
        self = object.__new__(cls)
        self.fp_dirs, self.comps = [], {}
        with open(pcb_path, "rb") as f:                    # new ids repeat from run to run, and are not those
            pcbnew.KIID.SeedGenerator(zlib.crc32(f.read()) or 1)       # the board was built with
        self.board = pcbnew.LoadBoard(pcb_path)
        self.declared = {}
        if declared:
            with open(declared) as f:
                self.declared = json.load(f)
        self.copper_layers = copper_layers(self.board)
        self.planes = list(planes)
        self.on_plane = set()
        self.rules = dict(track=track, clearance=clearance, via=via, min_drill=min_drill)
        self.classes = {}
        self.ns = self.board.GetDesignSettings().m_NetSettings
        self.fps = {fp.GetReference(): fp for fp in self.board.GetFootprints()}
        self.nets, self.netinfo = {}, {}
        for name, info in self.board.GetNetsByName().items():
            if str(name):
                self.nets[str(name)] = []
                self.netinfo[str(name)] = info
        for ref, fp in self.fps.items():
            for pad in fp.Pads():
                if pad.GetNetname():
                    self.nets[pad.GetNetname()].append((ref, pad.GetNumber()))
        self._stitch_vias = []
        self.placed = set(self.fps)
        self.cutouts = []
        self._contour, self._slots, self._edge_shapes, self._corner = [], [], [], 0.0
        bb = self.board.GetBoardEdgesBoundingBox()
        self.box = tuple(pcbnew.ToMM(v) for v in (bb.GetLeft(), bb.GetTop(), bb.GetWidth(), bb.GetHeight()))
        return self

    # ---- setup ------------------------------------------------------------
    def resolve(self, name):
        """Map a plain net name to the board's net name (KiCad prefixes local nets with the sheet path)."""
        if name in self.nets:
            return name
        for cand in ("/" + name,):
            if cand in self.nets:
                return cand
        if name in self.declared:                   # unlabeled net: find it by its pins
            pins = {tuple(p) for p in self.declared[name]}
            for net, nodes in self.nets.items():
                if pins == {n for n in nodes if not n[0].startswith("#")}:
                    return net
        raise KeyError("no such net: " + name)

    def _net(self, name):
        if name not in self.netinfo:
            ni = pcbnew.NETINFO_ITEM(self.board, name)
            self.board.Add(ni)
            self.netinfo[name] = ni
        return self.netinfo[name]

    def _load(self):
        pin_net = {}
        for net, nodes in self.nets.items():
            for ref, pin in nodes:
                pin_net[(ref, pin)] = net
        for ref, c in self.comps.items():
            if ":" not in c["footprint"]:
                continue
            lib, name = c["footprint"].split(":", 1)
            fp = None
            for d in self.fp_dirs + [FP_DIR]:          # project libraries first
                if os.path.isdir(os.path.join(d, lib + ".pretty")):
                    fp = pcbnew.FootprintLoad(os.path.join(d, lib + ".pretty"), name)
                    break
            if fp is None:
                raise RuntimeError("footprint not found: " + c["footprint"])
            fp.SetReference(ref)
            fp.SetValue(c["value"])
            fp.SetFPIDAsString(c["footprint"])
            if c["uuid"]:                              # on a sub-sheet: /<sheet uuid>/<symbol uuid>
                fp.SetPath(pcbnew.KIID_PATH(c.get("path") or "/" + c["uuid"]))
            self.board.Add(fp)
            for pad in fp.Pads():
                net = pin_net.get((ref, pad.GetNumber()))
                if net:
                    pad.SetNet(self._net(net))
            fp.SetExcludedFromBOM(c["exclude_from_bom"])
            if hasattr(fp, "SetDNP"):
                fp.SetDNP(c["dnp"])
            for name, val in c["fields"].items():
                if name in ("Footprint",):
                    continue
                fp.SetField(name, val)
                f = fp.GetField(name)
                if f is not None:
                    f.SetVisible(False)
            self.fps[ref] = fp

    def netclass(self, name, nets, track, clearance=None):
        nc = pcbnew.NETCLASS(name)
        d = self.ns.GetDefaultNetclass()
        nc.SetTrackWidth(MM(track))
        nc.SetClearance(MM(clearance + 0.02) if clearance else d.GetClearance())
        nc.SetViaDiameter(d.GetViaDiameter())
        nc.SetViaDrill(d.GetViaDrill())
        nets = [self.resolve(n) for n in nets]
        self.classes[name] = dict(nets=list(nets), track=track,
                                  clearance=clearance or self.rules["clearance"])
        self.ns.SetNetclass(name, nc)
        for n in nets:
            self.ns.SetNetclassPatternAssignment(n, name)
        self.ns.RecomputeEffectiveNetclasses()

    # ---- geometry ---------------------------------------------------------
    def place(self, ref, x, y, rot=0, back=False):
        """Fixed placement (marks the part as placed for later place_near calls)."""
        self.placed.add(ref)
        fp = self.fps[ref]
        if back != fp.IsFlipped():
            fp.Flip(fp.GetPosition(), pcbnew.FLIP_DIRECTION_LEFT_RIGHT)
        fp.SetPosition(pcbnew.VECTOR2I_MM(x, y))
        fp.SetOrientationDegrees(rot)

    def _crt(self, fp):
        cy = fp.GetCourtyard(pcbnew.F_CrtYd if not fp.IsFlipped() else pcbnew.B_CrtYd)
        return cy if cy.OutlineCount() else None

    @staticmethod
    def _box_mm(bb):
        return (pcbnew.ToMM(bb.GetLeft()), pcbnew.ToMM(bb.GetTop()),
                pcbnew.ToMM(bb.GetRight()), pcbnew.ToMM(bb.GetBottom()))

    def _hits_cutout(self, box, margin=0.0):
        """True if box (x0, y0, x1, y1) comes closer than margin to a cut-out."""
        return any(planar.rects_overlap(box, (x, y, x + w, y + h), margin) for x, y, w, h in self.cutouts)

    def fits(self, ref, edge=0.3):
        """True if ref's courtyard is inside the board (with edge margin), clear of cut-outs
        and clear of all other courtyards."""
        me = self._crt(self.fps[ref])
        if me is None:
            return True
        bb = me.BBox()
        x, y, w, h = self.box
        if (pcbnew.ToMM(bb.GetLeft()) < x + edge or pcbnew.ToMM(bb.GetTop()) < y + edge or
                pcbnew.ToMM(bb.GetRight()) > x + w - edge or pcbnew.ToMM(bb.GetBottom()) > y + h - edge):
            return False
        if self._hits_cutout(self._box_mm(bb), edge):
            return False
        mine = self.fps[ref]
        mb = self._box_mm(bb)
        for other, fp in self.fps.items():
            if other == ref or other not in self.placed or not self._share_space(mine, fp):
                continue
            o = self._crt(fp)
            if o is None:
                continue
            ob = self._box_mm(o.BBox())                  # cheap test first: Collide is the slow part
            if mb[0] > ob[2] or ob[0] > mb[2] or mb[1] > ob[3] or ob[1] > mb[3]:
                continue
            if me.Collide(o):
                return False
        return True

    @staticmethod
    def _through(fp):
        return any(p.GetAttribute() in (pcbnew.PAD_ATTRIB_PTH, pcbnew.PAD_ATTRIB_NPTH) for p in fp.Pads())

    def _share_space(self, a, b):
        """Can the courtyards of footprints a and b collide? On one side always; on opposite
        sides only if one of them goes through the board (its pins and solder joints are on
        the other side too)."""
        return a.IsFlipped() == b.IsFlipped() or self._through(a) or self._through(b)

    def place_near(self, ref, x, y, rots=(0, 90, 180, 270), step=0.25, max_r=20.0, edge=0.3, back=False):
        """Put ref at the free spot closest to (x, y), trying the given rotations at each radius;
        back=True puts it on the back of the board."""
        r = 0.0
        while r <= max_r:
            n = max(1, int(2 * math.pi * r / step)) if r else 1
            for i in range(n):
                a = 2 * math.pi * i / n
                px, py = x + r * math.cos(a), y + r * math.sin(a)
                for rot in rots:
                    self.place(ref, px, py, rot, back)
                    if self.fits(ref, edge):
                        self.placed.add(ref)
                        return px, py, rot
            r += step
        raise RuntimeError("no free spot for %s near (%.1f, %.1f)" % (ref, x, y))

    def pad_xy(self, ref, num):
        for p in self.fps[ref].Pads():
            if p.GetNumber() == str(num):
                v = p.GetPosition()
                return pcbnew.ToMM(v.x), pcbnew.ToMM(v.y)
        raise KeyError((ref, num))

    def _set_edge(self):
        """(Re)draw the board contour and the slots on Edge.Cuts."""
        for sh in self._edge_shapes:
            self.board.Remove(sh)
        self._edge_shapes = []
        V = pcbnew.VECTOR2I_MM
        for prim in self._contour + [p for slot in self._slots for p in slot]:
            sh = pcbnew.PCB_SHAPE(self.board)
            if prim[0] == "seg":
                sh.SetShape(pcbnew.SHAPE_T_SEGMENT)
                sh.SetStart(V(*prim[1]))
                sh.SetEnd(V(*prim[2]))
            else:
                sh.SetShape(pcbnew.SHAPE_T_ARC)
                sh.SetArcGeometry(V(*prim[1]), V(*prim[2]), V(*prim[3]))
            sh.SetLayer(pcbnew.Edge_Cuts)
            sh.SetWidth(MM(0.1))
            self.board.Add(sh)
            self._edge_shapes.append(sh)

    def outline(self, x, y, w, h):
        """Rectangular board outline. Replaces an earlier outline and its cut-outs."""
        self.outline_rounded(x, y, w, h, r=0.0)

    def outline_rounded(self, x, y, w, h, r=1.5):
        """Rectangular board outline with rounded corners. Replaces an earlier outline and its cut-outs."""
        self.box = (x, y, w, h)
        self._corner = r
        self._contour, self._slots, self.cutouts = planar.rounded_rect(x, y, w, h, r), [], []
        self._set_edge()

    def cutout(self, x, y, w, h, r=0.0):
        """Cut a rectangle out of the board (Edge.Cuts), e.g. for a flex cable: a slot when it
        lies wholly inside the outline, a notch when it reaches one board edge; r rounds the corners.

        A slot gets radius r on all four corners (r = half the short side gives round ends), a
        notch on its two inner corners; a notch may be given reaching past the edge. Call
        after outline().
        fits(), place_near(), stitch() and via_near() keep parts and vias out of cut-outs;
        pours stay clear through KiCad's board-edge clearance.
        Returns the cut-out as (x, y, w, h), clipped to the board.
        """
        bx, by, bw, bh = self.box
        side = planar.cutout_side(self.box, (x, y, w, h), self._corner)
        x0, y0, x1, y1 = max(x, bx), max(y, by), min(x + w, bx + bw), min(y + h, by + bh)
        if self._hits_cutout((x0, y0, x1, y1)):
            raise ValueError("cut-out %s overlaps another cut-out" % ((x, y, w, h),))
        if side == "slot":
            self._slots.append(planar.rounded_rect(x, y, w, h, r))
        else:
            self._contour, _ = planar.notch(self._contour, self.box, (x, y, w, h), r)
        self.cutouts.append((x0, y0, x1 - x0, y1 - y0))
        self._set_edge()
        return self.cutouts[-1]

    def text(self, s, x, y, size=1.0, thickness=0.15, layer=pcbnew.F_SilkS, angle=0, justify="center"):
        t = pcbnew.PCB_TEXT(self.board)
        t.SetText(s)
        t.SetLayer(layer)
        t.SetTextSize(pcbnew.VECTOR2I(MM(size), MM(size)))
        t.SetTextThickness(MM(thickness))
        t.SetTextAngleDegrees(angle)
        t.SetHorizJustify({"left": pcbnew.GR_TEXT_H_ALIGN_LEFT, "right": pcbnew.GR_TEXT_H_ALIGN_RIGHT,
                           "center": pcbnew.GR_TEXT_H_ALIGN_CENTER}[justify])
        t.SetPosition(pcbnew.VECTOR2I_MM(x, y))
        self.board.Add(t)
        return t

    def _silk_obstacles(self, gap):
        def inflate(bb, d):
            bb = pcbnew.BOX2I(bb.GetOrigin(), bb.GetSize())
            bb.Inflate(MM(d))
            return bb
        obs = []
        for fp in self.board.GetFootprints():
            for pad in fp.Pads():
                obs.append(inflate(pad.GetBoundingBox(), gap))
            for g in fp.GraphicalItems():
                if g.GetLayer() == pcbnew.F_SilkS and g.GetClass() != "PCB_FIELD":
                    obs.append(inflate(g.GetBoundingBox(), gap / 2))
            r = fp.Reference()
            if r.IsVisible() and r.GetLayer() == pcbnew.F_SilkS:
                obs.append(inflate(r.GetBoundingBox(), gap / 2))
        for item in self.board.GetDrawings():
            if item.GetLayer() == pcbnew.F_SilkS:
                obs.append(inflate(item.GetBoundingBox(), gap))
        return obs

    def label_near(self, ref, pad, text, size=1.0, thickness=0.15, gap=0.2, edge=0.5, dist=(1.4, 2.2, 3.0)):
        """Silkscreen text next to a pad, at the first clear spot around it. Returns the text or None."""
        x, y = self.pad_xy(ref, pad)
        obs = self._silk_obstacles(gap)
        bx, by, bw, bh = self.box
        board = pcbnew.BOX2I(pcbnew.VECTOR2I_MM(bx + edge, by + edge), pcbnew.VECTOR2I_MM(bw - 2 * edge, bh - 2 * edge))
        t = self.text(text, x, y, size=size, thickness=thickness)
        for d in dist:
            for dx, dy in ((0, -d), (0, d), (d + 0.3 * len(text), 0), (-d - 0.3 * len(text), 0),
                           (d, -d), (-d, -d), (d, d), (-d, d)):
                t.SetPosition(pcbnew.VECTOR2I_MM(x + dx, y + dy))
                tb = t.GetBoundingBox()
                if (board.Contains(tb) and not any(tb.Intersects(o) for o in obs)
                        and not self._hits_cutout(self._box_mm(tb), edge)):
                    return t
        self.board.Remove(t)
        return None

    def clip_silk(self, ref):
        """Drop a footprint's silkscreen graphics that leave the board outline (e.g. an overhanging antenna)."""
        x, y, w, h = self.box
        board = pcbnew.BOX2I(pcbnew.VECTOR2I_MM(x + 0.3, y + 0.3), pcbnew.VECTOR2I_MM(w - 0.6, h - 0.6))
        fp = self.fps[ref]
        for g in list(fp.GraphicalItems()):
            if (g.GetLayer() == pcbnew.F_SilkS and g.GetClass() != "PCB_FIELD"
                    and not board.Contains(g.GetBoundingBox())):
                fp.Remove(g)

    def separate_silk(self, clearance=0.16):
        """Drop silkscreen graphics of one footprint that come within `clearance` of another's.

        Courtyards 0.2 mm apart do not keep silkscreen apart: a pin-1 mark outside the courtyard
        of one part met the outline of the next at 0.09 mm (the rules ask for 0.15). Of two
        such graphics the smaller goes (the mark, not the outline). Boxes are compared, so a
        slanted line counts for more room than it takes. Returns how many were dropped.
        """
        dropped = 0
        for layer in (pcbnew.F_SilkS, pcbnew.B_SilkS):
            items = []
            for fp in self.board.GetFootprints():
                for g in fp.GraphicalItems():
                    if g.GetLayer() == layer and g.GetClass() != "PCB_FIELD":
                        bb = g.GetBoundingBox()
                        box = (bb.GetLeft(), bb.GetTop(), bb.GetRight(), bb.GetBottom())
                        items.append([box, (box[2] - box[0]) + (box[3] - box[1]), fp, g, True])
            items.sort(key=lambda t: t[0][0])
            d = MM(clearance)
            for i, a in enumerate(items):
                for b in items[i + 1:]:
                    if b[0][0] >= a[0][2] + d:
                        break
                    if (a[4] and b[4] and a[2] is not b[2]
                            and a[0][1] < b[0][3] + d and b[0][1] < a[0][3] + d and a[0][0] < b[0][2] + d):
                        loser = a if a[1] < b[1] else b
                        loser[4] = False
                        loser[2].Remove(loser[3])
                        dropped += 1
        return dropped

    def place_refs(self, size=0.8, thickness=0.12, gap=0.2, edge=0.5, keep=(), silk=0.16):
        """Put every reference label just outside its courtyard, clear of pads, silk and other labels.

        Each board side by itself: a label on the back is kept clear of the pads, silk and
        labels of the back (silk: the distance between two silkscreen items the board's rules
        ask for, 0.15 mm in KiCad's defaults). Tries above, below, right, left; falls back to
        the courtyard centre on the fab layer. Refs in `keep` are left where they are. Returns
        refs that found no clear spot.
        """
        def inflate(bb, d):
            bb = pcbnew.BOX2I(bb.GetOrigin(), bb.GetSize())
            bb.Inflate(MM(d))
            return bb

        sides = {False: (pcbnew.F_Cu, pcbnew.F_SilkS, pcbnew.F_CrtYd, pcbnew.F_Fab),
                 True: (pcbnew.B_Cu, pcbnew.B_SilkS, pcbnew.B_CrtYd, pcbnew.B_Fab)}
        obstacles = {False: [], True: []}
        for back, (cu, silk_layer, _, _) in sides.items():
            for fp in self.board.GetFootprints():
                for pad in fp.Pads():
                    if pad.IsOnLayer(cu) or pad.HasHole():
                        obstacles[back].append(inflate(pad.GetBoundingBox(), gap))
                for g in fp.GraphicalItems():
                    if g.GetLayer() == silk_layer and g.GetClass() != "PCB_FIELD":
                        obstacles[back].append(inflate(g.GetBoundingBox(), max(gap / 2, silk)))
            for item in self.board.GetDrawings():
                if item.GetLayer() == silk_layer:
                    obstacles[back].append(inflate(item.GetBoundingBox(), gap))
        x, y, w, h = self.box
        board = pcbnew.BOX2I(pcbnew.VECTOR2I_MM(x + edge, y + edge), pcbnew.VECTOR2I_MM(w - 2 * edge, h - 2 * edge))
        placed, failed = {False: [], True: []}, []
        fps = sorted(self.board.GetFootprints(), key=lambda f: -f.GetBoundingBox(False).GetArea())
        for fp in fps:
            ref = fp.GetReference()
            r = fp.Reference()
            if not r.IsVisible() or ref in keep:
                continue
            back = fp.IsFlipped()
            r.SetTextSize(pcbnew.VECTOR2I(MM(size), MM(size)))
            r.SetTextThickness(MM(thickness))
            r.SetTextAngleDegrees(0)
            r.SetKeepUpright(True)
            cy = fp.GetCourtyard(sides[back][2])
            bb = cy.BBox() if cy.OutlineCount() else fp.GetBoundingBox(False)
            bb = pcbnew.BOX2I(bb.GetOrigin(), bb.GetSize())
            for g in fp.GraphicalItems():                  # silk that reaches beyond the courtyard
                if g.GetLayer() == sides[back][1] and g.GetClass() != "PCB_FIELD":
                    bb.Merge(g.GetBoundingBox())
            cx, cyy = bb.Centre().x, bb.Centre().y
            r.SetPosition(pcbnew.VECTOR2I(int(cx), int(cyy)))
            tb = r.GetBoundingBox()                        # the text as drawn: taller than `size`, and not centred
            ox, oy = tb.Centre().x - cx, tb.Centre().y - cyy
            tw, th = tb.GetWidth(), tb.GetHeight()
            d = MM(max(gap, silk + 0.02))
            cands = [(cx - ox, bb.GetTop() - d - th // 2 - oy), (cx - ox, bb.GetBottom() + d + th // 2 - oy),
                     (bb.GetRight() + d + tw // 2 - ox, cyy - oy), (bb.GetLeft() - d - tw // 2 - ox, cyy - oy)]
            ok_pos = None
            for px, py in cands:
                r.SetPosition(pcbnew.VECTOR2I(int(px), int(py)))
                tb = r.GetBoundingBox()
                if not board.Contains(tb) or self._hits_cutout(self._box_mm(tb), edge):
                    continue
                if any(tb.Intersects(o) for o in obstacles[back]):
                    continue
                if any(tb.Intersects(p) for p in placed[back]):
                    continue
                ok_pos = (px, py)
                break
            if ok_pos is None:
                r.SetPosition(pcbnew.VECTOR2I(int(cx), int(cyy)))
                r.SetLayer(sides[back][3])     # no clear silk spot: keep it on the fab layer only
                failed.append(ref)
            else:
                placed[back].append(inflate(r.GetBoundingBox(), max(gap / 2, silk)))
        return failed

    def zone(self, net, layer, inset=0.3, clearance=0.3, min_width=0.2, thermal_gap=0.3, spoke=0.4,
             rect=None, polygon=None):
        """Copper pour of `net` on `layer`: the board box `inset` mm inside the outline, or only
        the region rect=(x, y, w, h) or polygon=[(x, y), ...].

        A region is taken as given (no inset), so two nets can each have their own area on one
        layer: two ground domains on one plane. The gap between two such pours is the gap
        between their regions; prove it with min_gap().
        """
        if rect is not None and polygon is not None:
            raise ValueError("zone() takes rect or polygon, not both")
        if polygon is not None:
            pts = [(float(px), float(py)) for px, py in polygon]
            if len(pts) < 3:
                raise ValueError("a zone polygon needs at least three points")
        elif rect is not None:
            x, y, w, h = rect
            pts = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
        else:
            x, y, w, h = self.box
            pts = [(x + inset, y + inset), (x + w - inset, y + inset),
                   (x + w - inset, y + h - inset), (x + inset, y + h - inset)]
        z = pcbnew.ZONE(self.board)
        z.SetLayer(layer)
        z.SetNet(self._net(self.resolve(net)))
        o = z.Outline()
        o.NewOutline()
        for px, py in pts:
            o.Append(MM(px), MM(py))
        z.SetLocalClearance(MM(clearance))
        z.SetMinThickness(MM(min_width))
        z.SetPadConnection(pcbnew.ZONE_CONNECTION_THT_THERMAL)   # SMD solid (reflow), THT thermal
        z.SetThermalReliefGap(MM(thermal_gap))
        z.SetThermalReliefSpokeWidth(MM(spoke))
        z.SetIslandRemovalMode(pcbnew.ISLAND_REMOVAL_MODE_ALWAYS)
        self.board.Add(z)
        return z

    def keepout(self, x, y, w, h, layers=None, name="keepout",
                tracks=True, vias=True, fills=True):
        """Rule area; by default no copper pour, tracks or vias (footprint pads still allowed)
        on every copper layer of the board.

        layers=None means all copper layers: front and back on a two-layer board, the inner
        layers too on four or six.
        tracks=True, vias=False, fills=False keeps a plane solid: no routing, pour and vias allowed.
        """
        z = pcbnew.ZONE(self.board)
        z.SetIsRuleArea(True)
        z.SetZoneName(name)
        ls = pcbnew.LSET()
        for l in (self.copper_layers if layers is None else layers):
            ls.AddLayer(l)
        z.SetLayerSet(ls)
        z.SetDoNotAllowZoneFills(fills)
        z.SetDoNotAllowTracks(tracks)
        z.SetDoNotAllowVias(vias)
        z.SetDoNotAllowPads(False)
        z.SetDoNotAllowFootprints(False)
        o = z.Outline()
        o.NewOutline()
        for px, py in [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]:
            o.Append(MM(px), MM(py))
        self.board.Add(z)
        return z

    def courtyard_overlaps(self):
        boxes = {}
        for ref, fp in self.fps.items():
            cy = fp.GetCourtyard(pcbnew.F_CrtYd if not fp.IsFlipped() else pcbnew.B_CrtYd)
            if cy.OutlineCount():
                boxes[ref] = cy
        out = []
        refs = sorted(boxes)
        for i, a in enumerate(refs):
            for b in refs[i + 1:]:
                if self._share_space(self.fps[a], self.fps[b]) and boxes[a].Collide(boxes[b]):
                    out.append((a, b))
        x, y, w, h = self.box
        edge = []
        for ref, cy in boxes.items():
            bb = cy.BBox()
            if (pcbnew.ToMM(bb.GetLeft()) < x - 0.01 or pcbnew.ToMM(bb.GetTop()) < y - 0.01 or
                    pcbnew.ToMM(bb.GetRight()) > x + w + 0.01 or pcbnew.ToMM(bb.GetBottom()) > y + h + 0.01
                    or self._hits_cutout(self._box_mm(bb), -0.01)):
                edge.append(ref)
        return out, edge

    # ---- hand routing (locked, so the autorouter keeps it) --------------------
    def _xy(self, p):
        return self.pad_xy(*p) if isinstance(p[0], str) else p

    def _width(self, net):
        for c in self.classes.values():
            if net in c["nets"]:
                return c["track"]
        return self.rules["track"]

    def track(self, net, pts, width=None, layer=pcbnew.F_Cu, locked=True):
        """Polyline of locked segments; points are (x, y) or (ref, pad)."""
        net = self.resolve(net)
        w = width or self._width(net)
        pts = [self._xy(p) for p in pts]
        out = []
        for (x1, y1), (x2, y2) in zip(pts, pts[1:], strict=False):
            s = pcbnew.PCB_TRACK(self.board)
            s.SetStart(pcbnew.VECTOR2I_MM(x1, y1))
            s.SetEnd(pcbnew.VECTOR2I_MM(x2, y2))
            s.SetWidth(MM(w))
            s.SetLayer(layer)
            s.SetNet(self._net(net))
            s.SetLocked(locked)
            self.board.Add(s)
            out.append(s)
        return out

    def via(self, net, x, y, size=None, locked=True):
        size = size or self.rules["via"]
        v = pcbnew.PCB_VIA(self.board)
        v.SetPosition(pcbnew.VECTOR2I_MM(x, y))
        v.SetWidth(MM(size[0]))
        v.SetDrill(MM(size[1]))
        v.SetNet(self._net(self.resolve(net)))
        v.SetLocked(locked)
        self.board.Add(v)
        return v

    def rip_up(self, x0, y0, x1, y1, nets=None, keep=(), layers=None, vias=True):
        """Remove the unlocked tracks and vias that have an end in a rectangle; returns how many.

        For a spot where a connection stays open because the tracks around it were laid
        first: take them out, route the hard connection with route_open(only=[...]), then
        the rest with route_open(). nets: only these; keep: not these; layers: tracks on
        these layers only; vias=False leaves the vias. What stays outside the rectangle
        keeps its copper, and route_open() joins it again.
        """
        mm = pcbnew.ToMM
        nets = None if nets is None else {self.resolve(n) for n in nets}
        keep = {self.resolve(n) for n in keep}

        def inside(p):
            return x0 <= mm(p.x) <= x1 and y0 <= mm(p.y) <= y1
        n = 0
        for t in list(self.board.GetTracks()):
            net = t.GetNetname()
            if t.IsLocked() or net in keep or (nets is not None and net not in nets):
                continue
            if t.GetClass() == "PCB_VIA":
                hit = vias and inside(t.GetPosition())
            else:
                hit = (layers is None or t.GetLayer() in layers) and (inside(t.GetStart()) or inside(t.GetEnd()))
            if hit:
                # Delete, not Remove: with Remove, pcbnew crashed on the next count of open
                # connections once Python had let go of the removed tracks (seen on a reopened
                # six-layer board with 90 tracks removed; a small board does not show it).
                self.board.Delete(t)
                n += 1
        self.board.BuildConnectivity()
        return n

    def via_near(self, net, ref, pad, rmin=0.9, rmax=3.0, clearance=None, size=None):
        """Drop a via on `net` at the closest legal spot around a pad (after routing).

        Legal: clear of every other net's pads, tracks and vias by the clearance on
        every copper layer, and of the board edge and cut-outs. Returns (x, y) or None.
        """
        size = size or self.rules["via"]
        clr = clearance if clearance is not None else self.rules["clearance"] + 0.05
        net = self.resolve(net)
        px, py = self.pad_xy(ref, pad)
        rv = size[0] / 2
        x0, y0, w, h = self.box
        items = []                                   # (kind, geometry, net)
        for fp in self.board.GetFootprints():
            for p in fp.Pads():
                if (p.GetNetname() == net and not p.GetDrillSize().x) \
                        or not any(p.IsOnLayer(l) for l in self.copper_layers):
                    continue
                bb = p.GetBoundingBox()
                items.append(("box", (pcbnew.ToMM(bb.GetLeft()), pcbnew.ToMM(bb.GetTop()),
                                      pcbnew.ToMM(bb.GetRight()), pcbnew.ToMM(bb.GetBottom())), p.GetNetname()))
        for tr in self.board.GetTracks():
            if tr.GetNetname() == net and tr.GetClass() != "PCB_VIA":
                continue
            if tr.GetClass() == "PCB_VIA":
                c = tr.GetPosition()
                vw = max(tr.GetWidth(l) for l in self.copper_layers if tr.IsOnLayer(l))
                items.append(("circ", (pcbnew.ToMM(c.x), pcbnew.ToMM(c.y), pcbnew.ToMM(vw) / 2), tr.GetNetname()))
            else:
                s, e = tr.GetStart(), tr.GetEnd()
                items.append(("seg", (pcbnew.ToMM(s.x), pcbnew.ToMM(s.y), pcbnew.ToMM(e.x), pcbnew.ToMM(e.y),
                                      pcbnew.ToMM(tr.GetWidth()) / 2), tr.GetNetname()))

        def free(x, y):
            if not (x0 + rv + 0.4 <= x <= x0 + w - rv - 0.4 and y0 + rv + 0.4 <= y <= y0 + h - rv - 0.4):
                return False
            if self._hits_cutout((x, y, x, y), rv + 0.4):
                return False
            for kind, g, n in items:
                if kind == "box":
                    dx = max(g[0] - x, 0, x - g[2])
                    dy = max(g[1] - y, 0, y - g[3])
                    d = math.hypot(dx, dy)
                    gap = 0.5 if n == net else clr        # same-net pads: keep the via off the pad
                elif kind == "circ":
                    d = math.hypot(x - g[0], y - g[1]) - g[2]
                    gap = 0.3 if n == net else clr        # drill-to-drill spacing for same-net vias
                else:
                    ax, ay, bx, by, hw = g
                    vx, vy = bx - ax, by - ay
                    L2 = vx * vx + vy * vy or 1e-12
                    u = max(0.0, min(1.0, ((x - ax) * vx + (y - ay) * vy) / L2))
                    d = math.hypot(x - ax - u * vx, y - ay - u * vy) - hw
                    gap = clr
                if d < rv + gap:
                    return False
            return True
        r = rmin
        while r <= rmax:
            n = max(8, int(2 * math.pi * r / 0.2))
            for i in range(n):
                a = 2 * math.pi * i / n
                x, y = px + r * math.cos(a), py + r * math.sin(a)
                if free(x, y):
                    self.via(net, x, y, size)
                    side = pcbnew.B_Cu if self.fps[ref].IsFlipped() else pcbnew.F_Cu
                    self.track(net, [(ref, pad), (x, y)], 0.3, layer=side, locked=False)
                    return round(x, 2), round(y, 2)
            r += 0.1
        return None

    def fan_to_planes(self, reach=2.5, share=1.2, track=0.3, clearance=None, step=0.2, lane=1.0):
        """Before autorouting: give every SMD pad of a plane's net a via into its plane.

        A plane is a pour on a layer named in Board(planes=...). Each pad of the pour's net
        that lies inside the pour's outline gets the nearest legal via within `reach` mm and
        a straight track to it on the pad's side; a via of the same net closer than `share`
        mm is used again. A pad at least 0.5 mm larger than the via (an exposed pad) takes
        the via inside, in the middle if nothing on the other side is in the way.
        Legal: the via and the track clear all other copper, the via is inside the pour, off
        its own net's SMD pads and outside rule areas that forbid vias. Pads that already
        have a track of their net (hand routes) are left alone.

        The other pins of an IC keep their way out: `lane` mm straight out of every long pad
        of a part with five or more pads counts as a track of that pad's net (lane=0: not).
        Without it a ground pin's via sat 0.25 mm in front of the next pin of a 0.5 mm pitch
        QFN, and that pin could not be routed on an otherwise empty board.

        The pads tied to their plane, through-hole pads included, are remembered in
        `on_plane`, and autoroute() takes them off the router's list: Freerouting does not
        connect pins to a plane. Returns [(ref, pad)] of pads that found no spot; those
        stay with the router.
        """
        mm = pcbnew.ToMM
        clr = clearance if clearance is not None else self.rules["clearance"] + 0.05
        size = self.rules["via"]
        rv = size[0] / 2
        zones = [(z.GetNetname(), z.Outline()) for z in self.board.Zones()
                 if not z.GetIsRuleArea() and any(z.IsOnLayer(l) for l in self.planes)]
        no_vias = [z.Outline() for z in self.board.Zones() if z.GetIsRuleArea() and z.GetDoNotAllowVias()]
        cell = 2.0
        index = {}                                          # (cx, cy) -> [(layer or None: all, kind, geometry, net)]

        def add(layers, kind, g, net):
            if kind == "box":
                x0, y0, x1, y1 = g
            elif kind == "circ":
                x0, y0, x1, y1 = g[0] - g[2], g[1] - g[2], g[0] + g[2], g[1] + g[2]
            else:
                x0, y0 = min(g[0], g[2]) - g[4], min(g[1], g[3]) - g[4]
                x1, y1 = max(g[0], g[2]) + g[4], max(g[1], g[3]) + g[4]
            for cx in range(int((x0 - 1) // cell), int((x1 + 1) // cell) + 1):
                for cy in range(int((y0 - 1) // cell), int((y1 + 1) // cell) + 1):
                    index.setdefault((cx, cy), []).append((layers, kind, g, net))

        def pad_box(pd):
            bb = pd.GetBoundingBox()
            return (mm(bb.GetLeft()), mm(bb.GetTop()), mm(bb.GetRight()), mm(bb.GetBottom()))

        def side_of(pd):
            return None if pd.GetDrillSize().x else (pcbnew.B_Cu if pd.IsOnLayer(pcbnew.B_Cu) else pcbnew.F_Cu)

        boxes = {}                                          # pad -> its box, the same object as in the index
        for fp in self.board.GetFootprints():
            for pd in fp.Pads():
                if not any(pd.IsOnLayer(l) for l in self.copper_layers):
                    continue                                # paste-only pads of an exposed pad are no copper
                boxes[pd.m_Uuid.AsString()] = pad_box(pd)
                add(side_of(pd), "box", boxes[pd.m_Uuid.AsString()], pd.GetNetname())
            pads = [boxes[pd.m_Uuid.AsString()] for pd in fp.Pads() if pd.m_Uuid.AsString() in boxes]
            if lane and len(pads) >= 5:
                mx = sum(b[0] + b[2] for b in pads) / 2 / len(pads)
                my = sum(b[1] + b[3] for b in pads) / 2 / len(pads)
                for pd in fp.Pads():
                    b = boxes.get(pd.m_Uuid.AsString())
                    if b is None or side_of(pd) is None:
                        continue
                    w, h = b[2] - b[0], b[3] - b[1]
                    cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
                    hw = self.rules["track"] / 2
                    if w >= 1.4 * h and abs(cx - mx) > 0.3:    # a pad along x: out to the side it is on
                        x = b[2] if cx > mx else b[0]
                        add(side_of(pd), "seg", (x, cy, x + (lane if cx > mx else -lane), cy, hw), pd.GetNetname())
                    elif h >= 1.4 * w and abs(cy - my) > 0.3:
                        y = b[3] if cy > my else b[1]
                        add(side_of(pd), "seg", (cx, y, cx, y + (lane if cy > my else -lane), hw), pd.GetNetname())
        ends = {}                                           # (layer, net) -> track ends
        for tr in self.board.GetTracks():
            if tr.GetClass() == "PCB_VIA":
                c = tr.GetPosition()
                add(None, "circ", (mm(c.x), mm(c.y), mm(tr.GetWidth(pcbnew.F_Cu)) / 2), tr.GetNetname())
            else:
                a, e = tr.GetStart(), tr.GetEnd()
                add(tr.GetLayer(), "seg", (mm(a.x), mm(a.y), mm(e.x), mm(e.y), mm(tr.GetWidth()) / 2), tr.GetNetname())
                ends.setdefault((tr.GetLayer(), tr.GetNetname()), []).extend([(mm(a.x), mm(a.y)), (mm(e.x), mm(e.y))])

        def dist(kind, g, x, y):
            if kind == "box":
                return math.hypot(max(g[0] - x, 0, x - g[2]), max(g[1] - y, 0, y - g[3]))
            if kind == "circ":
                return math.hypot(x - g[0], y - g[1]) - g[2]
            ax, ay, bx, by, hw = g
            vx, vy = bx - ax, by - ay
            u = max(0.0, min(1.0, ((x - ax) * vx + (y - ay) * vy) / (vx * vx + vy * vy or 1e-12)))
            return math.hypot(x - ax - u * vx, y - ay - u * vy) - hw

        def clear(x, y, r, net, layer=None, via=False, own=None):
            """Nothing foreign closer than r + clearance to (x, y) on `layer` (None: on any layer)."""
            for layers, kind, g, n in index.get((int(x // cell), int(y // cell)), ()):
                if layer is not None and layers is not None and layers != layer:
                    continue
                if n != net:
                    gap = clr
                elif g is own:
                    continue                                # a via inside its own exposed pad
                elif via and kind == "box" and layers is not None:
                    gap = 0.15                              # off its own net's SMD pads: solder runs into a via
                elif via and kind == "circ":
                    gap = 0.3                               # drill to drill
                else:
                    continue
                if dist(kind, g, x, y) < r + gap:
                    return False
            return True

        def inside(outline, x, y, r):
            return all(outline.Contains(pcbnew.VECTOR2I_MM(x + dx, y + dy))
                       for dx, dy in ((0, 0), (r, 0), (-r, 0), (0, r), (0, -r)))

        def via_ok(x, y, net, outline, own=None):
            bx, by, bw, bh = self.box
            if not (bx + rv + 0.4 <= x <= bx + bw - rv - 0.4 and by + rv + 0.4 <= y <= by + bh - rv - 0.4):
                return False
            if self._hits_cutout((x, y, x, y), rv + 0.4) or not clear(x, y, rv, net, via=True, own=own):
                return False
            if not inside(outline, x, y, rv + 0.3):
                return False
            return not any(o.Contains(pcbnew.VECTOR2I_MM(x + dx, y + dy)) for o in no_vias
                           for dx, dy in ((0, 0), (rv, 0), (-rv, 0), (0, rv), (0, -rv)))

        def track_ok(px, py, x, y, net, layer):
            n = max(1, int(math.hypot(x - px, y - py) / 0.15))
            return all(clear(px + (x - px) * i / n, py + (y - py) * i / n, track / 2, net, layer) for i in range(n + 1))

        k = int(reach / step)
        spots = sorted(((i * step, j * step) for i in range(-k, k + 1) for j in range(-k, k + 1)
                        if math.hypot(i, j) * step <= reach), key=lambda d: d[0] * d[0] + d[1] * d[1])
        made = {}                                           # net -> vias placed here
        missing = []
        for ref in sorted(self.fps):
            for pd in self.fps[ref].Pads():
                net = pd.GetNetname()
                c = pd.GetPosition()
                outline = next((o for n, o in zones if n == net and o.Contains(c)), None)
                if outline is None or not pd.GetNumber() or pd.m_Uuid.AsString() not in boxes:
                    continue
                layer = side_of(pd)
                if layer is None:                           # a through-hole pad is in the plane already
                    self.on_plane.add((ref, pd.GetNumber()))
                    continue
                box = boxes[pd.m_Uuid.AsString()]
                if any(box[0] <= x <= box[2] and box[1] <= y <= box[3] for x, y in ends.get((layer, net), ())):
                    continue                                # routed by hand: not ours to decide
                px, py = mm(c.x), mm(c.y)
                found = None
                if min(box[2] - box[0], box[3] - box[1]) >= size[0] + 0.5:
                    m = rv + 0.25                           # an exposed pad takes its via inside, the middle first
                    for dx, dy in spots:
                        x, y = px + dx, py + dy
                        if box[0] + m <= x <= box[2] - m and box[1] + m <= y <= box[3] - m \
                                and via_ok(x, y, net, outline, own=box):
                            found = (x, y, True)
                            break
                for vx, vy in sorted(made.get(net, ()), key=lambda v: math.hypot(v[0] - px, v[1] - py)):
                    if math.hypot(vx - px, vy - py) > share:
                        break
                    if track_ok(px, py, vx, vy, net, layer):
                        found = (vx, vy, False)
                        break
                if found is None:
                    for dx, dy in spots:
                        x, y = px + dx, py + dy
                        if via_ok(x, y, net, outline) and track_ok(px, py, x, y, net, layer):
                            found = (x, y, True)
                            break
                if found is None:
                    missing.append((ref, pd.GetNumber()))
                    continue
                x, y, new = found
                if new:
                    v = pcbnew.PCB_VIA(self.board)
                    v.SetPosition(pcbnew.VECTOR2I_MM(x, y))
                    v.SetWidth(MM(size[0]))
                    v.SetDrill(MM(size[1]))
                    v.SetNet(pd.GetNet())
                    v.SetLocked(True)
                    self.board.Add(v)
                    add(None, "circ", (x, y, rv), net)
                    made.setdefault(net, []).append((x, y))
                if (x, y) != (px, py):                      # a via in the middle of its pad needs no track: one
                    t = pcbnew.PCB_TRACK(self.board)        # of no length made DRC report "tracks crossing"
                    t.SetStart(c)
                    t.SetEnd(pcbnew.VECTOR2I_MM(x, y))
                    t.SetWidth(MM(track))
                    t.SetLayer(layer)
                    t.SetNet(pd.GetNet())
                    t.SetLocked(True)
                    self.board.Add(t)
                    add(layer, "seg", (px, py, x, y, track / 2), net)
                self.on_plane.add((ref, pd.GetNumber()))
        return missing

    @staticmethod
    def _strip_pins(dsn_path, pins):
        """Take (ref, pad) pins off the nets of a Specctra DSN file. Returns how many were removed."""
        with open(dsn_path) as f:
            text = f.read()
        start = text.index("(network")
        count = [0]

        def pins_clause(m):
            keep = []
            for tok in m.group(1).split():
                ref, _, pad = tok.strip('"').partition("-")
                if (ref, pad) in pins:
                    count[0] += 1
                else:
                    keep.append(tok)
            return "(pins %s)" % " ".join(keep) if keep else ""
        body = re.sub(r"\(pins([^()]*)\)", pins_clause, text[start:])
        with open(dsn_path, "w") as f:
            f.write(text[:start] + body)
        return count[0]

    # ---- routing ----------------------------------------------------------
    def autoroute(self, workdir, passes=30, timeout=900, strategies=None, parallel=True, cache=True,
                  fanout=False, optimize=True):
        """Route with Freerouting, trying several strategies; import the best session.

        fanout: let Freerouting (2.5 and later) first give every SMD pin a via. Off by
        default: on a board with parts on both sides those vias block the other side.
        optimize=False skips its optimisation stage, for a quick look at routability.

        All strategies run at the same time, each with its share of the CPU threads, so
        the wall time is one run, not one per strategy. Results are cached by a hash of
        the DSN and the options: an unchanged placement re-imports its session instantly.
        Each result is scored on a fresh copy of the unrouted board (loaded from disk), so
        the live board is only touched once, by the final import.
        """
        strategies = strategies or [[], ["-us", "global"], ["-is", "random"],
                                    ["-us", "hybrid", "-hr", "1:1"], ["-is", "sequential"]]
        dsn = os.path.join(workdir, "route.dsn")
        pre = os.path.join(workdir, "preroute.kicad_pcb")
        pcbnew.SaveBoard(pre, self.board)
        if not pcbnew.ExportSpecctraDSN(self.board, dsn):
            raise RuntimeError("DSN export failed")
        if self.on_plane:
            self._strip_pins(dsn, self.on_plane)            # tied to their plane by fan_to_planes()
        with open(dsn, "rb") as f:
            dsn_hash = hashlib.sha256(f.read()).hexdigest()[:16]
        cdir = os.path.join(workdir, "route-cache")
        os.makedirs(cdir, exist_ok=True)
        threads = max(1, (os.cpu_count() or 2) // (len(strategies) if parallel else 1))
        base = ["--router.fanout.enabled=" + ("true" if fanout else "false")]
        if not optimize:
            base.append("--router.optimizer.enabled=false")
        jobs = []
        for i, extra in enumerate(strategies):
            key = hashlib.sha256(("%s|%d|%s" % (dsn_hash, passes, " ".join(base + extra))).encode()).hexdigest()[:16]
            ses = os.path.join(cdir, key + ".ses")
            log = ses[:-4] + ".log"
            if cache and os.path.exists(ses):
                jobs.append((i, extra, ses, log, None))
                continue
            cmd = ["freerouting", "-de", dsn, "-do", ses[:-4] + ".part.ses", "-mp", str(passes), "-mt", str(threads),
                   "--gui.enabled=false"] + base + extra
            lf = open(log, "w")
            proc = subprocess.Popen(cmd, stdout=lf, stderr=subprocess.STDOUT, text=True)
            jobs.append((i, extra, ses, log, (proc, lf)))
            if not parallel:
                self._finish(proc, lf, ses, timeout)
                jobs[-1] = (i, extra, ses, log, None)
        for _i, _extra, ses, _log, run in jobs:
            if run:
                self._finish(run[0], run[1], ses, timeout)
        best = None
        for i, extra, ses, log, run in jobs:
            if not os.path.exists(ses):
                continue
            trial = pcbnew.LoadBoard(pre)
            if not pcbnew.ImportSpecctraSES(trial, ses):
                continue
            trial.BuildConnectivity()
            left = trial.GetConnectivity().GetUnconnectedCount(False)
            print("  route attempt %d (%s)%s: %d unrouted" % (i + 1, " ".join(extra) or "default",
                                                           " [cached]" if run is None and cache else "", left))
            if best is None or left < best[0]:
                best = (left, ses, log)
        if best is None:
            raise RuntimeError("Freerouting produced no usable session")
        if not pcbnew.ImportSpecctraSES(self.board, best[1]):
            raise RuntimeError("SES import failed")
        with open(best[2]) as f:
            return f.read()

    @staticmethod
    def _finish(proc, logfile, ses, timeout):
        try:
            proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            proc.kill()
        logfile.close()
        part = ses[:-4] + ".part.ses"
        if os.path.exists(part):
            os.replace(part, ses)

    def ref_text(self, ref, visible=True, dx=None, dy=None):
        """Hide a reference label or move it relative to the footprint origin."""
        fp = self.fps[ref]
        r = fp.Reference()
        r.SetVisible(visible)
        if dx is not None or dy is not None:
            pos = fp.GetPosition()
            r.SetPosition(pcbnew.VECTOR2I(pos.x + MM(dx or 0), pos.y + MM(dy or 0)))

    def stitch(self, net="GND", pitch=4.0, via=(0.6, 0.3), margin=0.45, edge=1.5, hole_gap=0.5):
        """Drop vias on a grid wherever the net's pour covers the via on every layer it is poured on.

        On copper layers where the net has no pour, the via must clear the pads and tracks of
        other nets instead (a through via crosses those layers too). Vias stay out of cut-outs.
        """
        net = self.resolve(net)
        self.fill_zones()
        polys = []
        for z in self.board.Zones():
            if z.GetNetname() != net or z.GetIsRuleArea():
                continue
            for layer in z.GetLayerSet().Seq():
                polys.append((layer, z.GetFilledPolysList(layer)))
        layers = {l for l, _ in polys}
        # layers the vias pass through without a pour of this net: foreign copper is in the way there
        foreign = []
        clr = MM(self.rules["clearance"] + 0.05)
        for l in self.copper_layers:
            if l in layers:
                continue
            for fp in self.board.GetFootprints():
                foreign += [(l, p.GetEffectiveShape(l)) for p in fp.Pads()
                            if p.IsOnLayer(l) and p.GetNetname() != net]
            foreign += [(l, t.GetEffectiveShape(l)) for t in self.board.GetTracks()
                        if t.GetClass() != "PCB_VIA" and t.GetLayer() == l and t.GetNetname() != net]
        probe = pcbnew.PCB_VIA(self.board)
        probe.SetWidth(MM(via[0]))
        probe.SetDrill(MM(via[1]))
        holes = []   # (x, y, radius) in mm
        for fp in self.board.GetFootprints():
            for pad in fp.Pads():
                d = pad.GetDrillSize()
                if d.x > 0:
                    p = pad.GetPosition()
                    holes.append((pcbnew.ToMM(p.x), pcbnew.ToMM(p.y), pcbnew.ToMM(max(d.x, d.y)) / 2))
        for tr in self.board.GetTracks():
            if tr.GetClass() == "PCB_VIA":
                p = tr.GetPosition()
                holes.append((pcbnew.ToMM(p.x), pcbnew.ToMM(p.y), pcbnew.ToMM(tr.GetDrillValue()) / 2))
        banned = [z.Outline() for z in self.board.Zones() if z.GetIsRuleArea() and z.GetDoNotAllowVias()]
        x, y, w, h = self.box
        r = via[0] / 2 + margin
        ring = [(0, 0)] + [(r * math.cos(a * math.pi / 4), r * math.sin(a * math.pi / 4)) for a in range(8)]
        n = 0
        gy = y + edge
        while gy <= y + h - edge:
            gx = x + edge
            while gx <= x + w - edge:
                ok = all(any(p.Contains(pcbnew.VECTOR2I_MM(gx + dx, gy + dy)) for l, p in polys if l == layer)
                         for layer in layers for dx, dy in ring)
                if ok and any(o.Collide(pcbnew.VECTOR2I_MM(gx, gy), MM(via[0] / 2)) for o in banned):
                    ok = False
                if ok and any(math.hypot(gx - hx, gy - hy) < hr + via[1] / 2 + hole_gap
                              for hx, hy, hr in holes):
                    ok = False
                if ok and self._hits_cutout((gx, gy, gx, gy), via[0] / 2 + margin):
                    ok = False
                if ok and foreign:
                    probe.SetPosition(pcbnew.VECTOR2I_MM(gx, gy))
                    if any(shape.Collide(probe.GetEffectiveShape(l), clr) for l, shape in foreign):
                        ok = False
                if ok:
                    holes.append((gx, gy, via[1] / 2))
                    v = pcbnew.PCB_VIA(self.board)
                    v.SetPosition(pcbnew.VECTOR2I_MM(gx, gy))
                    v.SetWidth(MM(via[0]))
                    v.SetDrill(MM(via[1]))
                    v.SetNet(self._net(net))
                    self.board.Add(v)
                    self._stitch_vias.append(v)
                    n += 1
                gx += pitch
            gy += pitch
        self.fill_zones()
        n -= self._prune_floating_vias(net)
        return n

    def _reaches_pad(self, item, limit=400):
        """Breadth-first search over connected items; True once any pad is reached."""
        conn = self.board.GetConnectivity()
        seen, todo = {item.m_Uuid.AsString()}, [item]
        while todo and len(seen) < limit:
            cur = todo.pop()
            for nxt in conn.GetConnectedItems(cur):
                if nxt.GetClass() == "PAD":
                    return True
                key = nxt.m_Uuid.AsString()
                if key not in seen:
                    seen.add(key)
                    todo.append(nxt)
        return False

    def _prune_floating_vias(self, net):
        """Remove stitching vias whose copper cluster never reaches a pad of the net."""
        removed_total = 0
        for _ in range(10):
            removed = 0
            for v in list(self._stitch_vias):
                if not self._reaches_pad(v):
                    self.board.Remove(v)
                    self._stitch_vias.remove(v)
                    removed += 1
            removed_total += removed
            self.fill_zones()
            if removed == 0:
                break
        return removed_total

    def write_project(self, pro_path, min_track=0.15, min_via=0.5, min_annular=0.1, silk_clearance=0.15):
        """Store design rules and net classes in the .kicad_pro (KiCad keeps them there)."""
        data = {}
        if os.path.exists(pro_path):
            with open(pro_path) as f:
                data = json.load(f)
        data.setdefault("meta", {"filename": os.path.basename(pro_path), "version": 3})
        rules = data.setdefault("board", {}).setdefault("design_settings", {}).setdefault("rules", {})
        rules.update({"min_track_width": min_track, "min_clearance": self.rules["clearance"],
                      "min_through_hole_diameter": self.rules["min_drill"], "min_via_diameter": min_via,
                      "min_via_annular_width": min_annular, "min_copper_edge_clearance": 0.3,
                      "min_silk_clearance": silk_clearance})
        v_d, v_h = self.rules["via"]

        def cls(name, track, clearance, prio):
            return {"name": name, "track_width": track, "clearance": clearance, "via_diameter": v_d,
                    "via_drill": v_h, "microvia_diameter": 0.3, "microvia_drill": 0.1,
                    "diff_pair_width": 0.2, "diff_pair_gap": 0.25, "diff_pair_via_gap": 0.25,
                    "wire_width": 6, "bus_width": 12, "line_style": 0, "pcb_color": "rgba(0, 0, 0, 0.000)",
                    "schematic_color": "rgba(0, 0, 0, 0.000)", "priority": prio}
        classes = [cls("Default", self.rules["track"], self.rules["clearance"], 2147483647)]
        patterns = []
        for i, (name, c) in enumerate(self.classes.items()):
            classes.append(cls(name, c["track"], c["clearance"], i))
            patterns += [{"netclass": name, "pattern": n} for n in c["nets"]]
        data["net_settings"] = {"classes": classes, "meta": {"version": 4},
                                "netclass_patterns": patterns}
        with open(pro_path, "w") as f:
            json.dump(data, f, indent=2)

    def straighten(self, tol=0.12, rounds=20):
        """Merge autorouter micro-jogs: two unlocked segments meeting at a plain bend
        (no pad, via or T-junction there) become one when the bend deviates less than
        `tol` mm from the straight line, and the straight segment keeps its clearance
        to every other net. Returns the number of merges. Run before the pours.
        """
        mm = pcbnew.ToMM
        cls_clear = {}
        for c in self.classes.values():
            for n in c["nets"]:
                cls_clear[n] = c["clearance"]
        base = self.rules["clearance"]

        def clear_of(net):
            return cls_clear.get(net, base)

        merged = 0
        for _ in range(rounds):
            tracks = [t for t in self.board.GetTracks()]
            segs = [t for t in tracks if t.GetClass() != "PCB_VIA"]
            vias = [t for t in tracks if t.GetClass() == "PCB_VIA"]
            pads = [p for fp in self.board.GetFootprints() for p in fp.Pads()]
            ends = {}
            for s in segs:
                for pt in (s.GetStart(), s.GetEnd()):
                    k = (s.GetNetCode(), s.GetLayer(), round(mm(pt.x), 3), round(mm(pt.y), 3))
                    ends.setdefault(k, []).append(s)
            done = set()
            changed = False
            for k, group in ends.items():
                if len(group) != 2:
                    continue
                s1, s2 = group
                if s1.IsLocked() or s2.IsLocked() or s1.GetWidth() != s2.GetWidth():
                    continue
                if id(s1) in done or id(s2) in done:
                    continue
                pt = pcbnew.VECTOR2I_MM(k[2], k[3])
                net = s1.GetNetname()
                # Joint on a pad, via or T-junction: only merge when the straight segment still
                # covers the joint (deviation under half the width), so nothing gets disconnected.
                anchored = (any(v.GetNetCode() == k[0] and v.HitTest(pt) for v in vias)
                            or any(p.GetNetCode() == k[0] and p.HitTest(pt) for p in pads)
                            or any(o is not s1 and o is not s2 and o.GetNetCode() == k[0] and o.GetLayer() == k[1]
                                   and o.HitTest(pt, 0) for o in segs))
                def far(s, k=k):
                    st = s.GetStart()
                    return s.GetEnd() if (round(mm(st.x), 3), round(mm(st.y), 3)) == k[2:] else st
                a, b = far(s1), far(s2)
                ax, ay, bx, by = mm(a.x), mm(a.y), mm(b.x), mm(b.y)
                vx, vy = bx - ax, by - ay
                L = math.hypot(vx, vy)
                if L < 1e-6:
                    continue
                dev = abs((k[2] - ax) * vy - (k[3] - ay) * vx) / L
                if dev > tol or (anchored and dev > 0.4 * mm(s1.GetWidth())):
                    continue
                # clearance of the straight replacement against other nets' copper
                new = pcbnew.PCB_TRACK(self.board)
                new.SetStart(a)
                new.SetEnd(b)
                new.SetWidth(s1.GetWidth())
                new.SetLayer(s1.GetLayer())
                new.SetNet(s1.GetNet())
                shape = new.GetEffectiveShape(s1.GetLayer())
                ok = True
                for o in segs + vias:
                    if o.GetNetCode() == k[0] or (o.GetClass() != "PCB_VIA" and o.GetLayer() != k[1]):
                        continue
                    need = MM(max(clear_of(net), clear_of(o.GetNetname())))
                    if o.GetEffectiveShape(k[1]).Collide(shape, need):
                        ok = False
                        break
                if ok:
                    for p in pads:
                        if p.GetNetCode() == k[0] or not p.IsOnLayer(k[1]):
                            continue
                        need = MM(max(clear_of(net), clear_of(p.GetNetname())))
                        if p.GetEffectiveShape(k[1]).Collide(shape, need):
                            ok = False
                            break
                if not ok:
                    continue
                self.board.Remove(s1)
                self.board.Remove(s2)
                self.board.Add(new)
                done.update((id(s1), id(s2)))
                merged += 1
                changed = True
            if not changed:
                break
        return merged

    def chamfer_corners(self, c=0.4, min_turn=60.0):
        """Cut sharp corners of the locked hand routes into two 45-degree bends.

        Only plain corners: exactly two segments of one net and layer meet, no via, no pad and
        no third track there. Each leg is cut back by `c` mm (at most 40 % of the leg).
        Returns the number of corners cut.
        """
        segs = [t for t in self.board.GetTracks() if t.GetClass() != "PCB_VIA" and t.IsLocked()]
        vias = [t for t in self.board.GetTracks() if t.GetClass() == "PCB_VIA"]
        pads = [p for fp in self.board.GetFootprints() for p in fp.Pads()]
        ends = {}
        for s in segs:
            for k in ("S", "E"):
                pt = s.GetStart() if k == "S" else s.GetEnd()
                ends.setdefault((s.GetNetCode(), s.GetLayer(), pt.x, pt.y), []).append((s, k))
        cut = 0
        for (net, layer, x, y), lst in ends.items():
            if len(lst) != 2:
                continue
            pt = pcbnew.VECTOR2I(x, y)
            if any(v.GetNetCode() == net and v.HitTest(pt) for v in vias):
                continue
            if any(p.GetNetCode() == net and p.IsOnLayer(layer) and p.HitTest(pt) for p in pads):
                continue
            if any(o.GetNetCode() == net and o.GetLayer() == layer and o not in (lst[0][0], lst[1][0])
                   and o.HitTest(pt, 0) for o in segs):
                continue
            legs = []
            for s, k in lst:
                far = s.GetEnd() if k == "S" else s.GetStart()
                L = math.hypot(far.x - x, far.y - y)
                legs.append((s, k, (far.x - x) / L if L else 0, (far.y - y) / L if L else 0, L))
            (s1, k1, ux, uy, l1), (s2, k2, vx, vy, l2) = legs
            if l1 == 0 or l2 == 0:
                continue
            turn = 180.0 - math.degrees(math.acos(max(-1.0, min(1.0, ux * vx + uy * vy))))
            d = min(MM(c), 0.4 * l1, 0.4 * l2)
            if turn < min_turn or d < MM(0.05):
                continue
            a = pcbnew.VECTOR2I(int(x + ux * d), int(y + uy * d))
            b_ = pcbnew.VECTOR2I(int(x + vx * d), int(y + vy * d))
            (s1.SetStart if k1 == "S" else s1.SetEnd)(a)
            (s2.SetStart if k2 == "S" else s2.SetEnd)(b_)
            n = pcbnew.PCB_TRACK(self.board)
            n.SetStart(a)
            n.SetEnd(b_)
            n.SetWidth(min(s1.GetWidth(), s2.GetWidth()))
            n.SetLayer(layer)
            n.SetNetCode(net)
            n.SetLocked(True)
            self.board.Add(n)
            cut += 1
        return cut

    def remove_redundant(self, nets=None):
        """Remove unlocked track segments whose removal leaves every connection intact
        (autorouter duplicates that form loops with hand routes). Returns the number removed.

        With `nets` (e.g. ["GND"]) and filled pours, it removes tracks of those nets that the
        pour already makes redundant. The fill is not redone per removal: a removed track only
        frees room, so whatever stays connected against the old fill stays connected.
        """
        want = {self.resolve(n) for n in nets} if nets else None
        self.board.BuildConnectivity()
        base = self.board.GetConnectivity().GetUnconnectedCount(False)
        removed = 0
        # Short segments are joints between two tracks whose end caps already overlap; KiCad
        # calls them redundant, but removing them leaves a kink of overlapping copper.
        segs = [t for t in self.board.GetTracks() if t.GetClass() != "PCB_VIA" and not t.IsLocked()
                and t.GetLength() > 2 * t.GetWidth() and (want is None or t.GetNetname() in want)]
        # longest first: of two parallel paths, keep the shorter one
        segs.sort(key=lambda s: -s.GetLength())
        for s in segs:
            self.board.Remove(s)
            self.board.BuildConnectivity()
            if self.board.GetConnectivity().GetUnconnectedCount(False) > base:
                self.board.Add(s)
            else:
                removed += 1
        self.board.BuildConnectivity()
        return removed

    def _class_width(self, net):
        for c in self.classes.values():
            if net in c["nets"]:
                return c["track"]
        return self.rules["track"]

    def widen(self):
        """Autorouter neck-downs: set unlocked tracks narrower than their net class back to
        the class width where clearance allows. Returns (widened, still_narrow list)."""
        mm = pcbnew.ToMM
        cls_clear = {n: c["clearance"] for c in self.classes.values() for n in c["nets"]}
        base = self.rules["clearance"]
        tracks = list(self.board.GetTracks())
        pads = [p for fp in self.board.GetFootprints() for p in fp.Pads()]
        widened, narrow = 0, []
        for s in tracks:
            if s.GetClass() == "PCB_VIA" or s.IsLocked():
                continue
            net = s.GetNetname()
            want = MM(self._class_width(net))
            if s.GetWidth() >= want - MM(0.005):
                continue
            old = s.GetWidth()
            s.SetWidth(want)
            shape = s.GetEffectiveShape(s.GetLayer())
            ok = True
            for o in tracks:
                if o is s or o.GetNetCode() == s.GetNetCode():
                    continue
                if o.GetClass() != "PCB_VIA" and o.GetLayer() != s.GetLayer():
                    continue
                need = MM(max(cls_clear.get(net, base), cls_clear.get(o.GetNetname(), base)))
                if o.GetEffectiveShape(s.GetLayer()).Collide(shape, need):
                    ok = False
                    break
            if ok:
                for p in pads:
                    if p.GetNetCode() == s.GetNetCode() or not p.IsOnLayer(s.GetLayer()):
                        continue
                    need = MM(max(cls_clear.get(net, base), cls_clear.get(p.GetNetname(), base)))
                    if p.GetEffectiveShape(s.GetLayer()).Collide(shape, need):
                        ok = False
                        break
            if ok:
                widened += 1
            else:
                s.SetWidth(old)
                a = s.GetStart()
                narrow.append((net, round(mm(old), 2), round(mm(a.x), 2), round(mm(a.y), 2)))
        return widened, narrow

    def prune_dangling(self, rounds=10):
        """Remove unlocked vias and track stubs that lead nowhere (autorouter leftovers).

        What leads nowhere is KiCad's own decision (the test behind DRC's "track has
        unconnected end" and "via is connected on only one layer"), with the zones filled:
        a via that only feeds an inner plane stays. An own geometric test stood here before;
        it took a track end 0.36 mm from the centre of a 0.6 mm via for loose where KiCad
        saw the copper overlap, and removed three whole connections of a routed board.
        Returns the number of items removed.
        """
        removed = 0
        for _ in range(rounds):
            self.fill_zones()
            conn = self.board.GetConnectivity()
            gone = [tr for tr in self.board.GetTracks()
                    if not tr.IsLocked() and tr not in self._stitch_vias and conn.TestTrackEndpointDangling(tr, True)]
            if not gone:
                break
            if os.environ.get("PRUNE_DEBUG"):
                for tr in gone:
                    s, e = tr.GetStart(), tr.GetEnd()
                    print("   prune", tr.GetNetname(), tr.GetClass()[4:], self.board.GetLayerName(tr.GetLayer()),
                          (round(pcbnew.ToMM(s.x), 2), round(pcbnew.ToMM(s.y), 2)),
                          (round(pcbnew.ToMM(e.x), 2), round(pcbnew.ToMM(e.y), 2)))
            removed += len(gone)
            for tr in gone:                                # Delete, not Remove: see rip_up()
                self.board.Delete(tr)
            del gone
        self.fill_zones()
        return removed

    def route_open(self, tmp_dir, **kw):
        """Connect what the autorouter left open, one connection at a time, with a grid search:
        (routed, failed). Run it after autoroute() and before the outer pours; see finish.route_open."""
        from .finish import route_open
        return route_open(self, tmp_dir, **kw)

    def negotiate(self, tmp_dir, **kw):
        """Route all nets that are in pieces at once, by negotiated congestion: see negotiate.negotiate()."""
        from .negotiate import negotiate
        return negotiate(self, tmp_dir, **kw)

    def improve(self, **kw):
        """Route again what is routed, run by run, and keep what is shorter, has fewer vias or
        reaches its net class's width: dict(runs, better, mm, vias, widened). After route_open();
        see finish.improve."""
        from .finish import improve
        return improve(self, **kw)

    def fill_zones(self):
        self.board.BuildConnectivity()
        pcbnew.ZONE_FILLER(self.board).Fill(self.board.Zones())
        self.board.BuildConnectivity()

    def unrouted(self):
        self.board.BuildConnectivity()
        return self.board.GetConnectivity().GetUnconnectedCount(False)

    def save(self, path, mask_min_web=0.1):
        self.board.GetDesignSettings().m_SolderMaskMinWidth = MM(mask_min_web)   # JLC: 0.1 mm web
        self.board.SetFileName(path)
        pcbnew.SaveBoard(path, self.board)


def path_length(board, net, a, b):
    """Shortest copper path between two pads along tracks and vias: (mm, vias) or None.

    Pours are ignored on purpose: this measures what was routed. DRC passes a buck whose
    switch node wanders 30 mm around the board; this check does not.
    A via joins every copper layer it spans, inner layers included, and counts once.
    a, b: "REF.PAD" strings.
    """
    mm = pcbnew.ToMM
    cu = copper_layers(board)
    adj, pads, segs = {}, {}, []

    def key(x, y, l):
        return (round(x, 3), round(y, 3), l)

    def link(u, v, w, via=0):
        adj.setdefault(u, []).append((v, w, via))
        adj.setdefault(v, []).append((u, w, via))
    for t in board.GetTracks():
        if t.GetNetname() != net:
            continue
        if t.GetClass() == "PCB_VIA":
            p = t.GetPosition()
            ks = [key(mm(p.x), mm(p.y), l) for l in cu if t.IsOnLayer(l)]
            for i, u in enumerate(ks):
                for v in ks[i + 1:]:
                    link(u, v, 0, 1)
        else:
            s, e = t.GetStart(), t.GetEnd()
            u, v = key(mm(s.x), mm(s.y), t.GetLayer()), key(mm(e.x), mm(e.y), t.GetLayer())
            link(u, v, mm(t.GetLength()))
            segs.append((t, u, v))
    nodes = [k for k in adj]
    for t, u, v in segs:                       # endpoints that land mid-segment
        for k in nodes:
            if k[2] == u[2] and k not in (u, v) and t.HitTest(pcbnew.VECTOR2I_MM(k[0], k[1]), 0):
                link(k, u, math.hypot(k[0] - u[0], k[1] - u[1]))
                link(k, v, math.hypot(k[0] - v[0], k[1] - v[1]))
    for fp in board.GetFootprints():
        for p in fp.Pads():
            if p.GetNetname() != net:
                continue
            name = "%s.%s" % (fp.GetReference(), p.GetNumber())
            q = p.GetPosition()
            for layer in cu:
                if not p.IsOnLayer(layer):
                    continue
                pk = ("PAD", name, layer)
                pads.setdefault(name, []).append(pk)
                for k in nodes:
                    if k[2] == layer and p.HitTest(pcbnew.VECTOR2I_MM(k[0], k[1])):
                        link(pk, k, math.hypot(k[0] - mm(q.x), k[1] - mm(q.y)))
                for tr, u, v in segs:                  # a track that crosses the pad without ending on it
                    if u[2] == layer and tr.HitTest(q, 0):
                        link(pk, u, math.hypot(u[0] - mm(q.x), u[1] - mm(q.y)))
                        link(pk, v, math.hypot(v[0] - mm(q.x), v[1] - mm(q.y)))
            for i, u in enumerate(pads.get(name, [])):       # a through-hole pad joins its layers like a via
                for v in pads[name][i + 1:]:
                    link(u, v, 0, 1)
    if a not in pads or b not in pads:
        return None
    dst = set(pads[b])
    dist = {s: (0.0, 0) for s in pads[a]}
    pq = [(0.0, 0, i, s) for i, s in enumerate(pads[a])]
    tick = len(pq)
    while pq:
        d, nv, _, u = heapq.heappop(pq)
        if u in dst:
            return round(d, 2), nv
        if (d, nv) > dist.get(u, (1e9, 0)):
            continue
        for v, w, via in adj.get(u, []):
            nd = (d + w, nv + via)
            if nd < dist.get(v, (1e9, 0)):
                dist[v] = nd
                tick += 1
                heapq.heappush(pq, (nd[0], nd[1], tick, v))
    return None


def foreign_copper(board, rect, allowed, layers=None):
    """Tracks and vias inside rect (x0, y0, x1, y1) on nets not in `allowed`, on every copper
    layer unless `layers` names some.

    Guards an area such as a buck's hot loop against the autorouter threading an
    unrelated net through it. Returns [(net, layer name, x, y)].
    """
    if layers is None:
        layers = copper_layers(board)
    x0, y0, x1, y1 = rect
    box = pcbnew.BOX2I(pcbnew.VECTOR2I_MM(x0, y0), pcbnew.VECTOR2I_MM(x1 - x0, y1 - y0))
    out = []
    for t in board.GetTracks():
        if t.GetNetname() in allowed:
            continue
        if t.GetClass() == "PCB_VIA":
            hit = box.Contains(t.GetPosition())
            lname = "via"
        else:
            if t.GetLayer() not in layers:
                continue
            hit = box.Intersects(t.GetBoundingBox())
            lname = board.GetLayerName(t.GetLayer())
        if hit:
            c = t.GetPosition() if t.GetClass() == "PCB_VIA" else t.GetStart()
            out.append((t.GetNetname(), lname, round(pcbnew.ToMM(c.x), 2), round(pcbnew.ToMM(c.y), 2)))
    return out


def _poly_shapes(polys, tag):
    """A pcbnew SHAPE_POLY_SET as planar polygons (mm), one per outline with its holes."""
    mm = pcbnew.ToMM

    def ring(chain):
        return [(mm(chain.CPoint(i).x), mm(chain.CPoint(i).y)) for i in range(chain.PointCount())]
    return [planar.polygon([ring(polys.Outline(i))] + [ring(polys.Hole(i, j)) for j in range(polys.HoleCount(i))],
                           tag) for i in range(polys.OutlineCount()) if polys.Outline(i).PointCount() >= 3]


def min_gap(board, nets_a, nets_b):
    """Smallest copper-to-copper distance between two groups of nets on one copper layer:
    (mm, layer name, x, y, net_a, net_b), with (x, y) in the middle of the gap. None when the
    groups share no copper layer.

    Proves an isolation gap, e.g. 2 mm between two ground domains: DRC only knows the
    clearance rule, and a pour or a track of one side reaching into the other's area passes
    it. Counts pads, tracks, vias and filled zones on every copper layer (inner layers
    too); the zones must be filled, as in a saved board. Copper on different layers is not
    compared: that distance is the stackup's. 0 means the groups touch.
    """
    mm = pcbnew.ToMM
    groups = [{nets_a} if isinstance(nets_a, str) else set(nets_a),
              {nets_b} if isinstance(nets_b, str) else set(nets_b)]
    if groups[0] & groups[1]:
        raise ValueError("nets in both groups: %s" % sorted(groups[0] & groups[1]))
    cu = copper_layers(board)
    shapes = {l: ([], []) for l in cu}

    def side(item):
        net = item.GetNetname()
        return net, (0 if net in groups[0] else 1 if net in groups[1] else None)

    def outline(item, layer):
        poly = pcbnew.SHAPE_POLY_SET()
        item.TransformShapeToPolygon(poly, layer, 0, MM(0.002), pcbnew.ERROR_OUTSIDE)
        return poly
    for fp in board.GetFootprints():
        for p in fp.Pads():
            net, g = side(p)
            if g is not None:
                for l in cu:
                    if p.IsOnLayer(l):
                        shapes[l][g].extend(_poly_shapes(outline(p, l), net))
    for t in board.GetTracks():
        net, g = side(t)
        if g is None:
            continue
        if t.GetClass() == "PCB_VIA":
            c = t.GetPosition()
            for l in cu:
                if t.IsOnLayer(l):
                    shapes[l][g].append(planar.circle(mm(c.x), mm(c.y), mm(t.GetWidth(l)) / 2, net))
        elif t.GetClass() == "PCB_ARC":
            shapes[t.GetLayer()][g].extend(_poly_shapes(outline(t, t.GetLayer()), net))
        else:
            a, b = t.GetStart(), t.GetEnd()
            shapes[t.GetLayer()][g].append(
                planar.capsule(mm(a.x), mm(a.y), mm(b.x), mm(b.y), mm(t.GetWidth()) / 2, net))
    for z in board.Zones():
        net, g = side(z)
        if g is None or z.GetIsRuleArea():
            continue
        for l in cu:
            if z.IsOnLayer(l):
                shapes[l][g].extend(_poly_shapes(z.GetFilledPolysList(l), net))
    best = None
    for l in cu:
        r = planar.closest(*shapes[l])
        if r is not None and (best is None or r[0] < best[0]):
            best = (round(r[0], 3), board.GetLayerName(l), round(r[1][0], 3), round(r[1][1], 3), r[2], r[3])
    return best


def acute_angles(board, limit=89.0):
    """Joints where two tracks of a net meet at less than `limit` degrees (etch traps):
    shared endpoints and T-junctions. Returns [(net, layer, x, y, angle)].

    A piece shorter than the track is wide is not judged: it lies inside the round ends of its
    neighbours, and its direction says nothing about the copper's shape. The tracks on both
    sides of it are still compared with each other, their ends being close enough.
    """
    mm = pcbnew.ToMM
    segs = [t for t in board.GetTracks() if t.GetClass() != "PCB_VIA"]
    vias = [t for t in board.GetTracks() if t.GetClass() == "PCB_VIA"]
    pads = [p for fp in board.GetFootprints() for p in fp.Pads()]
    out = []

    def covered(net, layer, pt):
        return (any(v.GetNetCode() == net and v.HitTest(pt) for v in vias)
                or any(p.GetNetCode() == net and p.IsOnLayer(layer) and p.HitTest(pt) for p in pads))

    def vec(s, at):
        a, b = s.GetStart(), s.GetEnd()
        if (a.x - at.x) ** 2 + (a.y - at.y) ** 2 > (b.x - at.x) ** 2 + (b.y - at.y) ** 2:
            a, b = b, a
        return (b.x - a.x, b.y - a.y)          # pointing away from the joint

    def angle(u, v):
        lu, lv = math.hypot(*u), math.hypot(*v)
        if lu == 0 or lv == 0:
            return 180.0
        c = (u[0] * v[0] + u[1] * v[1]) / (lu * lv)
        return math.degrees(math.acos(max(-1.0, min(1.0, c))))
    def stub(t):
        return math.hypot(t.GetEnd().x - t.GetStart().x, t.GetEnd().y - t.GetStart().y) < t.GetWidth()
    segs = [t for t in segs if not stub(t)]
    for i, s in enumerate(segs):
        for pt in (s.GetStart(), s.GetEnd()):
            for o in segs[i + 1:]:
                if o.GetNetCode() != s.GetNetCode() or o.GetLayer() != s.GetLayer():
                    continue
                e = min((o.GetStart(), o.GetEnd()), key=lambda q: (q.x - pt.x) ** 2 + (q.y - pt.y) ** 2)
                if math.hypot(e.x - pt.x, e.y - pt.y) <= max(s.GetWidth(), o.GetWidth()) / 2:
                    ang = angle(vec(s, pt), vec(o, e))        # shared (or overlapping) end
                elif o.HitTest(pt, 0):                         # T-junction onto o's middle
                    d = (o.GetEnd().x - o.GetStart().x, o.GetEnd().y - o.GetStart().y)
                    u = vec(s, pt)
                    ang = min(angle(u, d), angle(u, (-d[0], -d[1])))
                else:
                    continue
                if ang < limit and not covered(s.GetNetCode(), s.GetLayer(), pt):
                    out.append((s.GetNetname(), board.GetLayerName(s.GetLayer()),
                                round(mm(pt.x), 2), round(mm(pt.y), 2), round(ang)))
    return out


def narrow_tracks(board, classes, default):
    """Tracks narrower than their net class (neck-downs). classes: Board.classes."""
    mm = pcbnew.ToMM
    want = {n: c["track"] for c in classes.values() for n in c["nets"]}
    out = []
    for t in board.GetTracks():
        if t.GetClass() == "PCB_VIA":
            continue
        w = want.get(t.GetNetname(), default)
        if mm(t.GetWidth()) < w - 0.005:
            a = t.GetStart()
            out.append((t.GetNetname(), round(mm(t.GetWidth()), 2), w, round(mm(a.x), 2), round(mm(a.y), 2),
                        "hand" if t.IsLocked() else "auto"))
    return out


def drc(pcb_path, report_path, schematic_parity=True):
    cmd = ["kicad-cli", "pcb", "drc", "--severity-all", "-o", report_path]
    if schematic_parity:
        cmd.append("--schematic-parity")
    cmd.append(pcb_path)
    r = subprocess.run(cmd, capture_output=True, text=True)
    return r.stdout + r.stderr


def render(pcb_path, png_path, layers="F.Cu,B.Cu,F.SilkS,F.Fab,Edge.Cuts,F.CrtYd", width=2400):
    svg = png_path[:-4] + ".svg"
    subprocess.run(["kicad-cli", "pcb", "export", "svg", "--mode-single", "--fit-page-to-board",
                    "--exclude-drawing-sheet", "-l", layers, "-o", svg, pcb_path],
                   capture_output=True, text=True, check=True)
    subprocess.run(["rsvg-convert", "-w", str(width), "-b", "white", svg, "-o", png_path], check=True)
    return png_path
