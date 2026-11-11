"""Generate KiCad schematics from Python.

Each part declares its nets (pin -> net). The drawing is separate:
  - wire(...) / link(...) draw real wires between pins and points,
  - power(...) attaches a power symbol (GND, +3V3, ...) to a pin,
  - tag(...) attaches a net label to a pin,
and finish() gives every declared pin that is still undrawn a short stub with a
net label, and marks unlisted pins no-connect. Junction dots are added where
three or more wire ends meet or a wire end lands on another wire.

The declared nets are the source of truth: compare them with KiCad's own
netlist (kicad-cli sch export netlist) to prove the drawing realizes them.

A design can have several sheets: Schematic is the root page, Schematic.sheet()
adds pages with the same drawing API. References and nets are design-wide; in a
multi-sheet design every net label is written as a global label, so a net name
means the same net on every sheet.

Output is the KiCad 10 file format (version 20260306), the one KiCad 10.0
writes itself. Library symbols are copied from the installed libraries, which
are in that format too.
"""
import copy
import math
import os
import uuid

from .sexp import Sym, dump, find, find1, parse

SYMBOL_DIR = os.environ.get("KICAD_SYMBOL_DIR", "/usr/share/kicad/symbols")
FILE_VERSION = "20260306"         # what KiCad 10.0.8 writes (kicad-cli sch upgrade)
GENERATOR_VERSION = "10.0"
GRID = 1.27
POWER_SYMBOLS = {"GND": "power:GND", "+3V3": "power:+3V3", "+12V": "power:+12V",
                 "+5V": "power:+5V", "VBUS": "power:VBUS", "GNDA": "power:GNDA"}


def _uid():
    return str(uuid.uuid4())


def _n(v):
    s = ("%.4f" % float(v)).rstrip("0").rstrip(".")
    return Sym("0" if s in ("-0", "") else s)


def _snap(v, grid=GRID):
    return round(round(v / grid) * grid, 4)


def _font(size=1.27, hide=False, justify=None, bold=False):
    f = ["font", ["size", _n(size), _n(size)]]
    if bold:
        f.append(["bold", Sym("yes")])
    eff = ["effects", f]
    if justify:
        eff.append(["justify"] + [Sym(j) for j in justify.split()])
    if hide:
        eff.append(["hide", Sym("yes")])
    return eff


def _just(rot, j):
    """KiCad mirrors field justification on parts rotated 180 deg; pre-compensate."""
    if rot % 360 == 180:
        return {"left": "right", "right": "left"}.get(j, j)
    return j


def _rot(vx, vy, deg):
    """Rotate a screen vector (y down) counter-clockwise as seen on screen."""
    a = math.radians(deg)
    c, s = round(math.cos(a), 9), round(math.sin(a), 9)
    return vx * c + vy * s, -vx * s + vy * c


class SymbolLib:
    """Loads library symbols and flattens derived ('extends') symbols. symbol_dir is one
    directory or a list searched in order: project libraries first, KiCad's own (SYMBOL_DIR) last.

    The first directory that has <lib>.kicad_sym provides the whole library.
    """

    def __init__(self, symbol_dir=SYMBOL_DIR):
        self.dirs = [symbol_dir] if isinstance(symbol_dir, (str, os.PathLike)) else list(symbol_dir)
        if not self.dirs:
            raise ValueError("SymbolLib needs at least one directory")
        self.dir = self.dirs[-1]
        self._libs = {}

    def path(self, lib):
        """The file that provides library `lib`: the first match on the search path."""
        for d in self.dirs:
            p = os.path.join(d, lib + ".kicad_sym")
            if os.path.exists(p):
                return p
        raise FileNotFoundError("symbol library %s.kicad_sym not found in %s" % (lib, ", ".join(map(str, self.dirs))))

    def _lib(self, lib):
        if lib not in self._libs:
            with open(self.path(lib)) as f:
                self._libs[lib] = parse(f.read())
        return self._libs[lib]

    def _raw(self, lib, name):
        for s in find(self._lib(lib), "symbol"):
            if s[1] == name:
                return s
        raise KeyError("symbol not found: %s:%s" % (lib, name))

    def flat(self, lib_id):
        lib, name = lib_id.split(":", 1)
        sym = copy.deepcopy(self._raw(lib, name))
        ext = find1(sym, "extends")
        if ext is not None:
            pname = ext[1]
            out = self.flat("%s:%s" % (lib, pname))
            child = {p[1]: p for p in find(sym, "property")}
            for i, c in enumerate(out):
                if isinstance(c, list) and c and c[0] == "property" and c[1] in child:
                    out[i] = child.pop(c[1])
            subs = [i for i, c in enumerate(out) if isinstance(c, list) and c and c[0] == "symbol"]
            at = subs[0] if subs else len(out)
            for p in child.values():
                out.insert(at, p)
                at += 1
            for c in out:
                if isinstance(c, list) and c and c[0] == "symbol":
                    c[1] = name + c[1][len(pname):]
            sym = out
        sym[1] = lib_id
        return sym


def symbol_pins(flat, unit=1):
    """{number: dict(x, y, angle, type, name)} in library coordinates (y up)."""
    pins = {}
    for sub in find(flat, "symbol"):
        parts = sub[1].rsplit("_", 2)
        u = int(parts[-2]) if len(parts) >= 3 and parts[-2].isdigit() else 0
        if u not in (0, unit):
            continue
        for p in find(sub, "pin"):
            at = find1(p, "at")
            pins[str(find1(p, "number")[1])] = dict(
                x=float(at[1]), y=float(at[2]), angle=int(float(at[3])),
                type=str(p[1]), name=str(find1(p, "name")[1]))
    return pins


class Page:
    """One schematic sheet and its drawing API. Schematic is the root page of a design;
    Schematic.sheet() adds more pages.

    Parts, references, warnings and the power-symbol numbering are design-wide, the drawing
    (wires, labels, texts, layout checks) is per page.
    """

    def __init__(self, design, name, uuid, path, paper):
        self.design = design
        self.name = name         # sheet name; "" for the root
        self.uuid = uuid         # uuid of this sheet's file
        self.path = path         # instance path of the symbols on this page
        self.paper = paper
        self.lib = design.lib
        self.parts = design.parts            # ref -> dict(lib_id, value, footprint, fields, nets, pins): all pages
        self.warnings = design.warnings
        self.lib_symbols = {}
        self.items = []
        self.segments = []       # wire segments ((x1,y1),(x2,y2))
        self.drawn = set()       # (ref, pin) already connected by drawing
        self.syminfo = {}        # ref -> placement data for layout checks
        self.labels = []         # (net, x, y, angle)
        self.texts = []          # (s, x, y, size)
        self.blocks = []         # (name, box): sheet symbols, obstacles for the layout checks
        self.placed = []         # (ref, pin numbers) of the symbols on this page, in order
        self._pins = set()       # (ref, pin) on this page
        self.finished = False

    # ---- low-level items --------------------------------------------------
    def _seg(self, a, b):
        a = (_snap(a[0]), _snap(a[1]))
        b = (_snap(b[0]), _snap(b[1]))
        if a == b:
            return
        if a[0] != b[0] and a[1] != b[1]:
            raise ValueError("wire segments must be horizontal or vertical: %s -> %s" % (a, b))
        self.segments.append((a, b))
        self.items.append(["wire", ["pts", ["xy", _n(a[0]), _n(a[1])], ["xy", _n(b[0]), _n(b[1])]],
                           ["stroke", ["width", _n(0)], ["type", Sym("solid")]], ["uuid", _uid()]])

    def _label(self, net, x, y, angle=0):
        just = {0: "left bottom", 90: "left bottom", 180: "right bottom", 270: "right bottom"}[angle]
        self.labels.append((net, x, y, angle))
        self.items.append(["label", net, ["at", _n(x), _n(y), _n(angle)],
                           _font(justify=just), ["uuid", _uid()]])

    def no_connect(self, x, y):
        self.items.append(["no_connect", ["at", _n(x), _n(y)], ["uuid", _uid()]])

    def text(self, s, x, y, size=1.27, bold=False):
        self.texts.append((s, x, y, size))
        self.items.append(["text", s, ["exclude_from_sim", Sym("no")],
                           ["at", _n(x), _n(y), _n(0)],
                           _font(size, justify="left bottom", bold=bold), ["uuid", _uid()]])

    def frame(self, x, y, w, h, title=None):
        """Dashed rectangle around a functional block, with a bold title."""
        self.items.append(["rectangle", ["start", _n(x), _n(y)], ["end", _n(x + w), _n(y + h)],
                           ["stroke", ["width", _n(0.2)], ["type", Sym("dash")]],
                           ["fill", ["type", Sym("none")]], ["uuid", _uid()]])
        if title:
            self.text(title, x + 1.27, y + 3.3, size=1.8, bold=True)

    # ---- parts ------------------------------------------------------------
    def add(self, ref, lib_id, pos, value=None, footprint=None, nets=None, fields=None,
            rot=0, mirror=False, in_bom=True, on_board=True, dnp=False, unit=1, ref_at=None,
            val_at=None, hide_value=False, show=None):
        """Place a symbol at pos with rotation rot (0/90/180/270, CCW on screen).

        nets maps pin number -> net name (None = no-connect). ref_at/val_at are
        (dx, dy) offsets from pos for the reference and value text; by default the
        library offsets are rotated with the symbol and the text kept horizontal.
        """
        nets = dict(nets or {})
        fields = dict(fields or {})
        units = self.design._units.setdefault(ref, {})
        if unit in units:
            other = units[unit]
            raise ValueError("duplicate reference %s%s" % (ref, "" if other is self else
                             " (already on sheet %s)" % (other.name or "root")))
        flat = self.lib.flat(lib_id)
        self.lib_symbols.setdefault(lib_id, flat)
        lpins = symbol_pins(flat, unit)
        X, Y = _snap(pos[0]), _snap(pos[1])
        unknown = set(nets) - set(lpins)
        if unknown:
            raise ValueError("%s: no such pins %s (has %s)" % (ref, sorted(unknown), sorted(lpins)))

        node = ["symbol", ["lib_id", lib_id], ["at", _n(X), _n(Y), _n(rot)]]
        if mirror:
            node.append(["mirror", Sym("y")])
        node += [["unit", unit], ["body_style", 1],
                ["exclude_from_sim", Sym("no")], ["in_bom", Sym("yes" if in_bom else "no")],
                ["on_board", Sym("yes" if on_board else "no")], ["in_pos_files", Sym("yes")],
                ["dnp", Sym("yes" if dnp else "no")], ["uuid", _uid()]]
        mx = -1 if mirror else 1
        lib_props = {p[1]: p for p in find(flat, "property")}
        vals = {"Reference": ref}
        if value is not None:
            vals["Value"] = value
        if footprint is not None:
            vals["Footprint"] = footprint
        for name, lp in lib_props.items():
            if name.startswith("ki_"):
                continue
            p = copy.deepcopy(lp)
            if name in vals:
                p[2] = vals[name]
            at = find1(p, "at")
            override = {"Reference": ref_at, "Value": val_at}.get(name)
            if override is not None:
                dx, dy = override[0], override[1]
                just = override[2] if len(override) > 2 else "left"
                eff = find1(p, "effects")
                eff[:] = [e for e in eff if not (isinstance(e, list) and e[0] == "justify")]
                if just != "center":
                    eff.append(["justify", Sym(_just(rot, just))])
            else:
                dx, dy = _rot(mx * float(at[1]), -float(at[2]), rot)
            at[1], at[2] = _n(X + dx), _n(Y + dy)
            while len(at) < 4:
                at.append(_n(0))
            at[3] = _n(rot % 180)   # KiCad field angles are relative: this keeps text horizontal
            if name == "Value" and hide_value:
                if find1(p, "hide") is None and find1(find1(p, "effects"), "hide") is None:
                    p.insert(p.index(at) + 1, ["hide", Sym("yes")])
            node.append(p)
        vat = find1(next(p for p in node if isinstance(p, list) and p and p[0] == "property" and p[1] == "Value"), "at")
        vjust = None
        if val_at is not None and len(val_at) > 2 and val_at[2] != "center":
            vjust = val_at[2]
        elif val_at is not None and len(val_at) == 2:
            vjust = "left"
        flags = [["show_name", Sym("no")], ["do_not_autoplace", Sym("no")]]
        for name, val in fields.items():
            if show and name in show:
                node.append(["property", name, val,
                             ["at", vat[1], _n(float(vat[2]) + self.LINE * (1 + list(show).index(name))),
                              _n(rot % 180)], *copy.deepcopy(flags),
                             _font(justify=_just(rot, vjust) if vjust else None)])
            else:
                node.append(["property", name, val, ["at", _n(X), _n(Y), _n(0)], ["hide", Sym("yes")],
                             *copy.deepcopy(flags), _font()])
        for num in sorted(lpins, key=lambda s: (len(s), s)):
            node.append(["pin", num, ["uuid", _uid()]])
        node.append(["instances", ["project", self.design.project,
                                   ["path", self.path, ["reference", ref], ["unit", unit]]]])
        self.items.append(node)
        units[unit] = self
        # a further unit of a part on the same page is listed as U1B, U1C, ...
        key = ref if ref not in self.syminfo else ref + chr(64 + unit)
        self.syminfo[key] = dict(node=node, X=X, Y=Y, rot=rot, mx=mx, flat=flat, unit=unit,
                                 fields={p[1]: p for p in find(node, "property")}, show=list(show or []))

        pins = {}
        for num, p in lpins.items():
            vx, vy = _rot(mx * p["x"], -p["y"], rot)
            a = math.radians(p["angle"])
            dvx, dvy = _rot(mx * math.cos(a), -math.sin(a), rot)   # toward the body
            pins[num] = dict(p, at=(_snap(X + vx), _snap(Y + vy)),
                             out=(-round(dvx), -round(dvy)))
        self.placed.append((ref, list(pins)))
        self._pins.update((ref, num) for num in pins)
        if ref in self.parts:                   # another unit of a part that is already placed
            part = self.parts[ref]
            part["pins"].update(pins)
            part["nets"].update({k: v for k, v in nets.items() if v})
            part["declared"].update(nets)
        else:
            self.parts[ref] = dict(lib_id=lib_id, value=value or str(lib_props["Value"][2]),
                                   footprint=footprint or str(lib_props["Footprint"][2]),
                                   fields=fields, nets={k: v for k, v in nets.items() if v},
                                   declared=nets, pins=pins)
        return pins

    def pin(self, ref, num):
        return self.parts[ref]["pins"][str(num)]["at"]

    def _pt(self, p):
        if isinstance(p, tuple) and len(p) == 2 and isinstance(p[0], str):
            at = self.pin(*p)
            if (p[0], str(p[1])) not in self._pins:
                raise ValueError("%s pin %s is on another sheet: a wire cannot leave its page" % (p[0], p[1]))
            self.drawn.add((p[0], str(p[1])))
            return at
        return (_snap(p[0]), _snap(p[1]))

    # ---- drawing ----------------------------------------------------------
    def wire(self, *points):
        """Polyline through pins (('R1', 2)) and/or points ((x, y)); segments must be orthogonal."""
        pts = [self._pt(p) for p in points]
        for a, b in zip(pts, pts[1:], strict=False):
            self._seg(a, b)
        return pts[-1]

    def link(self, a, b, first="h"):
        """Connect two pins/points with one or two orthogonal segments (h: horizontal first)."""
        pa, pb = self._pt(a), self._pt(b)
        if pa[0] == pb[0] or pa[1] == pb[1]:
            self._seg(pa, pb)
        elif first == "h":
            self._seg(pa, (pb[0], pa[1]))
            self._seg((pb[0], pa[1]), pb)
        else:
            self._seg(pa, (pa[0], pb[1]))
            self._seg((pa[0], pb[1]), pb)

    def power(self, net, at, length=2.54, direction=None):
        """Attach a power symbol for net to a pin or point.

        The symbol goes `length` away from the pin, in the pin's outward direction
        unless direction ('up', 'down', 'left', 'right') is given. GND-type symbols
        hang downward, supply symbols point upward.
        """
        lib_id = self.design.power_symbols[net]
        p = self._pt(at)
        if direction is None and isinstance(at, tuple) and isinstance(at[0], str):
            out = self.parts[at[0]]["pins"][str(at[1])]["out"]
        else:
            out = {"up": (0, -1), "down": (0, 1), "left": (-1, 0), "right": (1, 0), None: (0, 1)}[direction]
        end = (p[0] + out[0] * length, p[1] + out[1] * length)
        self._seg(p, end)
        is_gnd = lib_id.split(":", 1)[1].startswith(("GND", "Earth"))   # drawn hanging down
        rot = 0
        # power symbols are drawn with their pin at the origin pointing toward the body
        self.design._pwr_n += 1
        ref = "#PWR%03d" % self.design._pwr_n
        flat = self.lib.flat(lib_id)
        self.lib_symbols.setdefault(lib_id, flat)
        if is_gnd and out == (0, -1):
            rot = 180
        elif not is_gnd and out == (0, 1):
            rot = 180
        elif out == (1, 0):
            rot = 90 if is_gnd else 270
        elif out == (-1, 0):
            rot = 270 if is_gnd else 90
        self.add(ref, lib_id, end, net, None, {"1": net}, rot=rot, in_bom=False, on_board=False,
                 hide_value=False)
        self.drawn.add((ref, "1"))

    def tag(self, at, net=None, length=2.54, direction=None):
        """Net label on a pin (stub outward) or at a point at the end of a wire."""
        p = self._pt(at)
        if isinstance(at, tuple) and isinstance(at[0], str):
            ref, num = at[0], str(at[1])
            net = net or self.parts[ref]["declared"][num]
            out = self.parts[ref]["pins"][num]["out"]
        else:
            out = {"up": (0, -1), "down": (0, 1), "left": (-1, 0), "right": (1, 0)}[direction or "right"]
        if length:
            end = (p[0] + out[0] * length, p[1] + out[1] * length)
            self._seg(p, end)
        else:
            end = p
        angle = {(-1, 0): 180, (1, 0): 0, (0, -1): 90, (0, 1): 270}[out]
        self._label(net, end[0], end[1], angle)

    def power_flag(self, net, at, length=2.54, direction="up"):
        """PWR_FLAG on a pin or point, on a short wire in `direction` from it.

        On a pin, the pin still gets its label or power symbol from finish(): the flag alone
        would leave it on a net of its own. Put the flag where a real pin is: on a wire that
        only reaches power symbols, KiCad reports the flag's pin as not connected.
        """
        on_pin = isinstance(at, tuple) and isinstance(at[0], str)
        was_drawn = on_pin and (at[0], str(at[1])) in self.drawn
        p = self._pt(at)
        if on_pin and not was_drawn:
            self.drawn.discard((at[0], str(at[1])))
        d = {"up": (0, -1, 0), "down": (0, 1, 180), "left": (-1, 0, 90), "right": (1, 0, 270)}[direction]
        end = (p[0] + d[0] * length, p[1] + d[1] * length)
        self._seg(p, end)
        self.design._flag_n += 1
        self.add("#FLG%02d" % self.design._flag_n, "power:PWR_FLAG", end, None, None, {"1": net},
                 rot=d[2], in_bom=False, on_board=False)
        self.drawn.add(("#FLG%02d" % self.design._flag_n, "1"))

    # ---- finishing --------------------------------------------------------
    def finish(self):
        """Label undrawn pins, no-connect unlisted ones, add junctions (on this page)."""
        for ref, nums in list(self.placed):
            if ref.startswith("#"):
                continue
            part = self.parts[ref]
            unlisted = []
            for num in nums:
                p = part["pins"][num]
                declared = part["declared"]
                if num not in declared or declared[num] is None:
                    if num not in declared and p["type"] != "no_connect":
                        unlisted.append(num)
                    self.no_connect(*p["at"])
                    continue
                if (ref, num) in self.drawn:
                    continue
                net = declared[num]
                if net in self.design.power_symbols:
                    self.power(net, (ref, num))
                else:
                    self.tag((ref, num))
            if unlisted:
                self.warnings.append("%s: unlisted pins marked no-connect: %s" % (
                    ref, ", ".join("%s(%s)" % (n, part["pins"][n]["name"]) for n in sorted(unlisted))))
        self.autoplace_fields()
        self._junctions()
        self.finished = True

    def _junctions(self):
        ends = {}
        for a, b in self.segments:
            for p in (a, b):
                ends[p] = ends.get(p, 0) + 1
        pin_pts = {}
        for ref, nums in self.placed:
            for num in nums:
                at = self.parts[ref]["pins"][num]["at"]
                pin_pts[at] = pin_pts.get(at, 0) + 1
        dots = set()
        for ref, nums in self.placed:
            for num in nums:
                q = self.parts[ref]["pins"][num]["at"]
                for a, b in self.segments:
                    if q in (a, b):
                        continue
                    if (a[0] == b[0] == q[0] and min(a[1], b[1]) < q[1] < max(a[1], b[1])) or \
                       (a[1] == b[1] == q[1] and min(a[0], b[0]) < q[0] < max(a[0], b[0])):
                        self.warnings.append("pin %s.%s lies on a wire %s-%s (connects!)" % (ref, num, a, b))
        for p, n in ends.items():
            if n + pin_pts.get(p, 0) >= 3:
                dots.add(p)
            for a, b in self.segments:
                if p in (a, b):
                    continue
                if a[0] == b[0] == p[0] and min(a[1], b[1]) < p[1] < max(a[1], b[1]):
                    dots.add(p)
                elif a[1] == b[1] == p[1] and min(a[0], b[0]) < p[0] < max(a[0], b[0]):
                    dots.add(p)
        for p in sorted(dots):
            self.items.append(["junction", ["at", _n(p[0]), _n(p[1])], ["diameter", _n(0)],
                               ["color", 0, 0, 0, 0], ["uuid", _uid()]])

    # ---- layout checks ----------------------------------------------------
    CHAR_W = 0.95            # stroke-font advance per character (caps/digits), fraction of the size;
                             # measured: "SMCJ30A" at 1.27 mm is 8.3 mm wide
    LINE = 1.9               # pitch of stacked field lines (1.27 mm text): 1.4 reads as touching
    GAP = 0.4                # clear space the overlap check demands between texts
    BODY_GAP = 0.5           # ... and between a text and another part's body

    def _xf(self, info, x, y):
        vx, vy = _rot(info["mx"] * x, -y, info["rot"])
        return info["X"] + vx, info["Y"] + vy

    def body_box(self, ref):
        """Bounding box of a symbol's graphics and pins, in sheet coordinates."""
        info = self.syminfo[ref]
        xs, ys = [], []
        for sub in find(info["flat"], "symbol"):
            parts = sub[1].rsplit("_", 2)
            u = int(parts[-2]) if len(parts) >= 3 and parts[-2].isdigit() else 0
            if u not in (0, info["unit"]):
                continue
            for g in sub:
                if not isinstance(g, list) or not g:
                    continue
                pts = []
                if g[0] == "rectangle":
                    pts = [find1(g, "start")[1:3], find1(g, "end")[1:3]]
                elif g[0] in ("polyline", "bezier"):
                    pts = [p[1:3] for p in find(find1(g, "pts"), "xy")]
                elif g[0] == "circle":
                    c, r = find1(g, "center"), float(find1(g, "radius")[1])
                    pts = [(float(c[1]) - r, float(c[2]) - r), (float(c[1]) + r, float(c[2]) + r)]
                elif g[0] == "arc":
                    pts = [find1(g, k)[1:3] for k in ("start", "mid", "end")]
                elif g[0] == "pin":
                    at = find1(g, "at")
                    ln = float(find1(g, "length")[1])
                    a = math.radians(float(at[3]))
                    pts = [(float(at[1]), float(at[2])),
                           (float(at[1]) + ln * math.cos(a), float(at[2]) + ln * math.sin(a))]
                for px, py in pts:
                    sx, sy = self._xf(info, float(px), float(py))
                    xs.append(sx)
                    ys.append(sy)
        if not xs:
            return None
        return (min(xs), min(ys), max(xs), max(ys))

    def _field_box(self, prop, rot=0):
        eff = find1(prop, "effects")
        hide = find1(prop, "hide")
        if (eff is None or find1(eff, "hide") is not None or not str(prop[2]) or
                (hide is not None and str(hide[1]) == "yes")):
            return None
        size = float(find1(find1(eff, "font"), "size")[1])
        at = find1(prop, "at")
        x, y = float(at[1]), float(at[2])
        w, h = len(str(prop[2])) * size * self.CHAR_W, size
        j = find1(eff, "justify")
        js = [str(v) for v in j[1:]] if j is not None else []
        if rot % 360 == 180:
            js = [{"left": "right", "right": "left"}.get(v, v) for v in js]
        x0 = x if "left" in js else (x - w if "right" in js else x - w / 2)
        y0 = y - h if "bottom" in js else (y if "top" in js else y - h / 2)
        return (x0, y0, x0 + w, y0 + h)

    def _label_box(self, net, x, y, angle, size=1.27):
        if self.design.sheets:                 # global label: text in a box centred on the wire
            m = 0.375 * size
            w, hh = len(net) * size * self.CHAR_W + 2 * m, size / 2 + m
            return {0: (x, y - hh, x + w, y + hh), 180: (x - w, y - hh, x, y + hh),
                    90: (x - hh, y - w, x + hh, y), 270: (x - hh, y, x + hh, y + w)}[angle]
        w, h = len(net) * size * self.CHAR_W, size
        if angle == 0:
            return (x, y - h, x + w, y)
        if angle == 180:
            return (x - w, y - h, x, y)
        if angle == 90:
            return (x - h, y - w, x, y)
        return (x - h, y, x, y + w)

    @staticmethod
    def _hit(a, b, pad=0.15):
        return a[0] < b[2] + pad and b[0] < a[2] + pad and a[1] < b[3] + pad and b[1] < a[3] + pad

    def _obstacles(self, skip_ref=None):
        obs = []
        for ref in self.syminfo:
            bb = self.body_box(ref)
            if bb and ref != skip_ref:
                obs.append(("body " + ref, bb))
            if ref == skip_ref:
                continue
            for name, prop in self.syminfo[ref]["fields"].items():
                fb = self._field_box(prop, self.syminfo[ref]["rot"])
                if fb:
                    obs.append(("%s.%s" % (ref, name), fb))
        for net, x, y, a in self.labels:
            obs.append(("label " + net, self._label_box(net, x, y, a)))
        for s, x, y, size in self.texts:
            obs.append(("text " + s[:20], (x, y - size, x + len(s) * size * self.CHAR_W, y)))
        for (a, b) in self.segments:
            obs.append(("wire", (min(a[0], b[0]) - 0.05, min(a[1], b[1]) - 0.05,
                                 max(a[0], b[0]) + 0.05, max(a[1], b[1]) + 0.05)))
        obs += self.blocks
        return obs

    def autoplace_fields(self, refs=None, passes=2):
        """Move Reference/Value (+ shown extra fields) of colliding parts to the first clear side."""
        refs = refs or [r for r in self.syminfo if not r.startswith("#")]
        for _ in range(passes):
            for ref in refs:
                info = self.syminfo[ref]
                stack = [info["fields"].get("Reference"), info["fields"].get("Value")] + \
                        [info["fields"][n] for n in info["show"]]
                stack = [f for f in stack if f is not None and self._field_box(f, info["rot"])]
                if len(stack) < 2:
                    continue
                own = self.body_box(ref)
                obs = self._obstacles(skip_ref=ref)

                def clear(stack=stack, info=info, obs=obs, own=own):
                    for f in stack:
                        b = self._field_box(f, info["rot"])
                        if b and (any(self._hit(b, o) for _, o in obs) or (own and self._hit(b, own, 0.05))):
                            return False
                    return True
                if clear():
                    continue
                x0, y0, x1, y1 = own
                cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
                n, dy = len(stack), self.LINE
                top = cy - dy * (n - 1) / 2
                cands = [[(x1 + 0.6, top + i * dy, "left") for i in range(n)],
                         [(x0 - 0.6, top + i * dy, "right") for i in range(n)],
                         [(cx, y0 - 0.8 - dy * (n - 1 - i), "center") for i in range(n)],
                         [(cx, y1 + 0.8 + dy * i, "center") for i in range(n)],
                         [(x1 + 0.6, y0 - 0.2 + i * dy, "left") for i in range(n)],
                         [(x0 - 0.6, y1 + 0.8 + i * dy, "right") for i in range(n)]]
                saved = [(list(find1(f, "at")), list(find1(f, "effects"))) for f in stack]
                for cand in cands:
                    for f, (x, y, j) in zip(stack, cand, strict=True):
                        at = find1(f, "at")
                        at[1], at[2] = _n(x), _n(y)
                        eff = find1(f, "effects")
                        eff[:] = [e for e in eff if not (isinstance(e, list) and e[0] == "justify")]
                        if j != "center":
                            eff.append(["justify", Sym(_just(info["rot"], j))])
                    if clear():
                        break
                else:
                    for f, (at, eff) in zip(stack, saved, strict=True):
                        find1(f, "at")[:] = at
                        find1(f, "effects")[:] = eff

    def overlaps(self):
        """List of human-readable text collisions on this page (fields, labels, notes vs everything)."""
        out = []
        texts = []
        for ref, info in self.syminfo.items():
            for name, prop in info["fields"].items():
                fb = self._field_box(prop, info["rot"])
                if fb:
                    texts.append(("%s.%s" % (ref, name), ref, fb))
        for net, x, y, a in self.labels:
            texts.append(("label " + net, None, self._label_box(net, x, y, a)))
        for s, x, y, size in self.texts:
            texts.append(("text " + s[:20], None, (x, y - size, x + len(s) * size * self.CHAR_W, y)))
        for i, (na, ra, ba) in enumerate(texts):
            for nb, rb, bb in texts[i + 1:]:
                if self._hit(ba, bb, self.GAP) and not (ra and ra == rb and ra.startswith("#")):
                    out.append("%s <-> %s" % (na, nb))
            for ref in self.syminfo:
                if ref == ra:
                    continue
                body = self.body_box(ref)
                if body and self._hit(ba, body, self.BODY_GAP):
                    out.append("%s <-> body %s" % (na, ref))
            for name, box in self.blocks:
                if self._hit(ba, box, self.BODY_GAP):
                    out.append("%s <-> %s" % (na, name))
        return out

    # ---- output -----------------------------------------------------------
    def _global(self, label):
        """A net label as a global label: in a multi-sheet design a net name is design-wide."""
        _, net, at, _eff, uid = label
        just = "left" if int(float(at[3])) in (0, 90) else "right"
        return ["global_label", net, ["shape", Sym("passive")], at, _font(justify=just), uid,
                ["property", "Intersheetrefs", "${INTERSHEET_REFS}", ["at", at[1], at[2], _n(0)],
                 ["hide", Sym("yes")], ["show_name", Sym("no")], ["do_not_autoplace", Sym("no")], _font()]]

    def _write(self, path, title_block, root):
        multi = bool(self.design.sheets)
        doc = ["kicad_sch", ["version", Sym(FILE_VERSION)], ["generator", "eeschema"],
               ["generator_version", GENERATOR_VERSION], ["uuid", self.uuid], ["paper", self.paper], title_block,
               ["lib_symbols"] + list(self.lib_symbols.values())]
        doc += [self._global(i) if multi and i[0] == "label" else i for i in self.items]
        if root:
            doc.append(["sheet_instances", ["path", "/", ["page", "1"]]])
        with open(path, "w") as f:
            f.write(dump(doc) + "\n")


class Sheet(Page):
    """A page below the root, made by Schematic.sheet(). Same drawing API, its own file."""

    def __init__(self, design, name, file, symbol_uuid, paper, title):
        Page.__init__(self, design, name, _uid(), "/%s/%s" % (design.root, symbol_uuid), paper)
        self.file = file
        self.symbol_uuid = symbol_uuid
        self.title = title


class Schematic(Page):
    """A design and its root page. sym_dirs lists project symbol library directories, searched
    before KiCad's own (like Board(fp_dirs=...)).

    Pass lib=SymbolLib([...]) instead for full control of the search path.

    power_symbols adds supply nets to the built-in POWER_SYMBOLS: {net: lib_id of the power
    symbol to draw}. The symbol only gives the shape; KiCad names the net after the symbol's
    value, which is set to the net. So {"GNDF": "power:GNDA", "VIO": "power:+3V3"} gives a
    second ground and another rail their own nets.
    """

    def __init__(self, project, title="", rev="", date="", company="", comments=(), paper="A3", lib=None,
                 sym_dirs=(), power_symbols=None):
        self.project = project
        self.root = _uid()
        self.title, self.rev, self.date, self.company = title, rev, date, company
        self.comments = list(comments)
        if lib is not None and sym_dirs:
            raise ValueError("pass lib or sym_dirs, not both")
        self.lib = lib or SymbolLib(list(sym_dirs) + [SYMBOL_DIR])
        self.power_symbols = dict(POWER_SYMBOLS, **(power_symbols or {}))
        self.parts = {}          # ref -> dict(lib_id, value, footprint, fields, nets, pins)
        self.warnings = []
        self.sheets = []         # pages below the root, in page order
        self._units = {}         # ref -> {unit: page}: one reference namespace for the design
        self._pwr_n = 0
        self._flag_n = 0
        Page.__init__(self, self, "", self.root, "/" + self.root, paper)

    def sheet(self, name, file, at=None, size=(40.64, 15.24), paper=None, title=None):
        """Add a sheet and return its Page (same drawing API); it is saved as `file` next to the
        root, which gets a sheet symbol at `at` (default: a row along the top of the page).

        The hierarchy is flat and has no sheet pins: with sheets, every net label in the design
        is written as a global label, so a net name is one net on all pages. Call this before
        drawing, so the layout checks measure labels in their global shape.
        """
        if not file.endswith(".kicad_sch") or os.path.isabs(file):
            raise ValueError("sheet file must be a relative .kicad_sch path: %s" % file)
        if any(s.name == name or s.file == file for s in self.sheets):
            raise ValueError("duplicate sheet name or file: %s, %s" % (name, file))
        w, h = size
        if at is None:
            at = (25.4 + len(self.sheets) * (w + 10.16), 25.4)
        x, y = _snap(at[0]), _snap(at[1])
        page = Sheet(self, name, file, _uid(), paper or self.paper,
                     title or (self.title + " - " + name if self.title else name))
        self.sheets.append(page)
        self.items.append(
            ["sheet", ["at", _n(x), _n(y)], ["size", _n(w), _n(h)],
             ["exclude_from_sim", Sym("no")], ["in_bom", Sym("yes")], ["on_board", Sym("yes")], ["dnp", Sym("no")],
             ["stroke", ["width", _n(0.1524)], ["type", Sym("solid")]], ["fill", ["color", 0, 0, 0, _n(0)]],
             ["uuid", page.symbol_uuid],
             ["property", "Sheetname", name, ["at", _n(x), _n(y - 0.7), _n(0)],
              ["show_name", Sym("no")], ["do_not_autoplace", Sym("no")], _font(justify="left bottom")],
             ["property", "Sheetfile", file, ["at", _n(x), _n(y + h + 0.6), _n(0)],
              ["show_name", Sym("no")], ["do_not_autoplace", Sym("no")], _font(justify="left top")],
             ["instances", ["project", self.project,
                            ["path", "/" + self.root, ["page", str(len(self.sheets) + 1)]]]]])
        self.blocks.append(("sheet " + name, (x, y, x + w, y + h)))
        self.texts.append((name, x, y - 0.7, 1.27))
        self.texts.append((file, x, y + h + 0.6 + 1.27, 1.27))
        return page

    def finish(self):
        """Label undrawn pins, no-connect unlisted ones, add junctions: on the root page and
        on every sheet that has not been finished yet."""
        Page.finish(self)
        for s in self.sheets:
            if not s.finished:
                s.finish()

    def overlaps(self, design=True):
        """List of human-readable text collisions (fields, labels, notes vs everything) in the
        whole design; collisions on a sheet are prefixed with its name.

        With design=False only the root page is checked; a sheet's own overlaps() checks that sheet.
        """
        out = Page.overlaps(self)
        if design:
            for s in self.sheets:
                out += ["%s: %s" % (s.name, o) for o in s.overlaps()]
        return out

    def netlist(self):
        """The declared nets of the whole design, all sheets: {net: [(ref, pin), ...]}."""
        out = {}
        for ref, p in self.parts.items():
            if ref.startswith("#"):
                continue
            for pin, net in p["nets"].items():
                out.setdefault(net, []).append((ref, pin))
        return {k: sorted(v) for k, v in out.items()}

    def _sym_uri(self, lib, directory, version_var):
        path = os.path.abspath(self.lib.path(lib))
        root = os.path.abspath(directory)
        try:
            inside = os.path.commonpath([path, root]) == root
        except ValueError:                                  # different drives
            inside = False
        if inside:                                          # project library
            return "${KIPRJMOD}/" + os.path.relpath(path, root).replace(os.sep, "/")
        if os.path.dirname(path) == os.path.abspath(self.lib.dirs[-1]):   # KiCad's own, last on the path
            return "${%s_SYMBOL_DIR}/%s.kicad_sym" % (version_var, lib)
        return path.replace(os.sep, "/")

    def write_lib_tables(self, directory, version_var="KICAD10"):
        """Write sym-lib-table and fp-lib-table for the libraries the design uses.

        Libraries inside `directory` (the project) are addressed through ${KIPRJMOD}, KiCad's
        own through ${<version_var>_SYMBOL_DIR} / _FOOTPRINT_DIR.
        """
        sym_libs = sorted({lid.split(":")[0] for page in [self] + self.sheets for lid in page.lib_symbols})
        fp_libs = sorted({p["footprint"].split(":")[0] for r, p in self.parts.items()
                          if ":" in p["footprint"] and not r.startswith("#")})
        with open(os.path.join(directory, "sym-lib-table"), "w") as f:
            f.write("(sym_lib_table\n\t(version 7)\n")
            for lib in sym_libs:
                f.write('\t(lib (name "%s") (type "KiCad") (uri "%s") (options "") (descr ""))\n'
                        % (lib, self._sym_uri(lib, directory, version_var)))
            f.write(")\n")
        with open(os.path.join(directory, "fp-lib-table"), "w") as f:
            f.write("(fp_lib_table\n\t(version 7)\n")
            for lib in fp_libs:
                if os.path.isdir(os.path.join(directory, "lib", lib + ".pretty")):   # project library
                    uri = "${KIPRJMOD}/lib/%s.pretty" % lib
                else:
                    uri = "${%s_FOOTPRINT_DIR}/%s.pretty" % (version_var, lib)
                f.write('\t(lib (name "%s") (type "KiCad") (uri "%s") (options "") (descr ""))\n' % (lib, uri))
            f.write(")\n")

    def _title_block(self, title):
        tb = ["title_block"]
        if title:
            tb.append(["title", title])
        if self.date:
            tb.append(["date", self.date])
        if self.rev:
            tb.append(["rev", self.rev])
        if self.company:
            tb.append(["company", self.company])
        for i, c in enumerate(self.comments[:9], 1):
            tb.append(["comment", i, c])
        return tb

    def save(self, path):
        """Write the root sheet to `path` and every sheet's file next to it."""
        self._write(path, self._title_block(self.title), root=True)
        for s in self.sheets:
            p = os.path.join(os.path.dirname(path), s.file)
            os.makedirs(os.path.dirname(p) or ".", exist_ok=True)
            s._write(p, self._title_block(s.title), root=False)


def compare_netlist(declared, kicad_net_path):
    """Compare declared nets with KiCad's netlist by pin grouping (names only where labeled).

    Returns a list of human-readable differences (empty = identical connectivity).
    """
    from .netlist import read_netlist
    _, nets = read_netlist(kicad_net_path)
    got = []
    for name, nodes in nets.items():
        pins = frozenset((r, p) for r, p in nodes if not r.startswith("#"))
        if len(pins) > 1 or (pins and not name.startswith("unconnected-")):
            got.append((name.lstrip("/"), pins))
    want = {frozenset(tuple(x) for x in v): k for k, v in declared.items()}
    have = {pins: name for name, pins in got if pins}
    diffs = []
    for pins, name in want.items():
        if len(pins) < 2:
            continue
        if pins not in have:
            diffs.append("declared net %s %s not found as one KiCad net" % (name, sorted(pins)))
        elif not have[pins].startswith("Net-(") and have[pins] != name:
            diffs.append("net %s is named %s in KiCad" % (name, have[pins]))
    for pins, name in have.items():
        if len(pins) >= 2 and pins not in want:
            diffs.append("KiCad net %s %s was not declared" % (name, sorted(pins)))
    return diffs
