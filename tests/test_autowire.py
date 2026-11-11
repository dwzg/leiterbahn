"""Wiring inside blocks: the router's rules, and that what it draws connects exactly the declared nets."""
import pytest
from conftest import comb  # noqa: F401  (fixture module)

from hwdesign.kicad import autowire as aw
from hwdesign.kicad.schematic import Schematic, SymbolLib


def design(symdir, **kw):
    return Schematic("demo", title="Demo", lib=SymbolLib(str(symdir)), autowire=True, **kw)


def connected(page):
    """Groups of pins that the drawing connects, by KiCad's rules: wire ends meet wire ends, pins and
    labels; a wire end on another wire is a junction; crossing wires do not touch; equal labels and
    equal power symbols join."""
    parent = {}

    def find(a):
        parent.setdefault(a, a)
        while parent[a] != a:
            parent[a] = parent[parent[a]]
            a = parent[a]
        return a

    def union(a, b):
        parent[find(a)] = find(b)

    pins = {}
    for ref, nums in page.placed:
        for num in nums:
            at = page.parts[ref]["pins"][num]["at"]
            pins[(ref, num)] = at
            union(("pin", ref, num), ("pt", at))
            if ref.startswith("#PWR"):
                union(("pin", ref, num), ("name", page.parts[ref]["declared"][num]))
    for net, x, y, _a in page.labels:
        union(("pt", (x, y)), ("name", net))
    points = {p for seg in page.segments for p in seg} | set(pins.values()) | {(x, y) for _n, x, y, _a in page.labels}
    for a, b in page.segments:
        union(("pt", a), ("pt", b))
        for p in points:
            if (a[0] == b[0] == p[0] and min(a[1], b[1]) < p[1] < max(a[1], b[1])) or \
               (a[1] == b[1] == p[1] and min(a[0], b[0]) < p[0] < max(a[0], b[0])):
                union(("pt", a), ("pt", p))
    groups = {}
    for (ref, num) in pins:
        if not ref.startswith("#") and page.parts[ref]["declared"].get(num) is not None:
            groups.setdefault(find(("pin", ref, num)), set()).add((ref, num))
    return sorted(sorted(g) for g in groups.values())


def declared(page):
    nets = {}
    for ref, nums in page.placed:
        if ref.startswith("#"):
            continue
        for num in nums:
            net = page.parts[ref]["declared"].get(num)
            if net is not None:
                nets.setdefault(net, set()).add((ref, num))
    return sorted(sorted(g) for g in nets.values())


# ---- the router's rules -----------------------------------------------------------
def test_a_wire_crosses_a_foreign_wire_at_a_right_angle_but_not_at_its_end():
    g = aw.Grid()
    g.add_wire("A", (5, 0), (5, 10))                       # vertical wire of net A
    lim = (0, 0, 20, 10)
    path = g.route("B", {(0, 5)}, lambda c, d: c == (10, 5), lim)
    assert (5, 5) in path and len(path) == 11              # straight across
    g2 = aw.Grid()
    g2.add_wire("A", (5, 0), (5, 5))                       # A ends exactly where B wants to pass
    path = g2.route("B", {(0, 5)}, lambda c, d: c == (10, 5), lim)
    assert (5, 5) not in path                              # around the end, not through it


def test_a_wire_does_not_run_along_a_foreign_wire_or_over_a_foreign_pin():
    g = aw.Grid()
    g.add_wire("A", (0, 5), (10, 5))
    g.add_pin(aw.point((5, 3)), (0, -1), "C")              # a pin of net C, pointing up
    path = g.route("B", {(0, 4)}, lambda c, d: c == (10, 4), (0, 0, 20, 10))
    assert all(c[1] != 5 for c in path)                    # never on A's row
    assert (5, 3) not in path and (5, 2) not in path       # nor on C's pin or the cell in front of it


def test_a_pin_is_reached_only_from_the_front():
    g = aw.Grid()
    g.add_pin(aw.point((10, 5)), (-1, 0), "A")             # points left
    path = g.route("A", {(12, 5)}, lambda c, d: c == (10, 5), (0, 0, 20, 10))
    assert path[-2] == (9, 5)                              # came around to the pin's front


# ---- on a page --------------------------------------------------------------------
def test_pins_of_a_net_inside_one_frame_are_wired_without_a_label(symdir):
    sch = design(symdir)
    sch.frame(50, 50, 100, 80, "Block")
    sch.add("R1", "tlib:R2P", (80, 80), "1k", "", {"1": "IN", "2": "MID"})
    sch.add("R2", "tlib:R2P", (110, 100), "1k", "", {"1": "MID", "2": "GND"})
    sch.finish()
    assert [lab[0] for lab in sch.labels] == ["IN"]         # MID stays inside: no label
    assert connected(sch) == declared(sch)
    assert not sch.warnings


def test_a_net_that_leaves_the_frame_gets_one_label_per_block(symdir):
    sch = design(symdir)
    sch.frame(50, 50, 80, 80, "One")
    sch.frame(150, 50, 80, 80, "Two")
    sch.add("R1", "tlib:R2P", (70, 80), "1k", "", {"1": "A", "2": "X"})
    sch.add("R2", "tlib:R2P", (100, 100), "1k", "", {"1": "X", "2": "GND"})
    sch.add("R3", "tlib:R2P", (190, 90), "1k", "", {"1": "X", "2": "GND"})
    sch.finish()
    xs = [lab for lab in sch.labels if lab[0] == "X"]
    assert len(xs) == 2 and sum(1 for lab in xs if lab[1] < 130) == 1
    assert connected(sch) == declared(sch)
    # wires stay in their frame
    assert all(50 <= p[0] <= 130 or 150 <= p[0] <= 230 for seg in sch.segments for p in seg)


def test_crossing_nets_at_an_ic_stay_separate(symdir):
    sch = design(symdir)
    sch.frame(40, 40, 160, 120, "IC")
    sch.add("U1", "tlib:COMB", (120, 100), "COMB", "",
            {"1": "N1", "2": "N2", "3": "N3", "4": "N4", "5": "N4", "6": "N6", "7": "N7", "8": "N8"})
    for i, net in enumerate(("N3", "N1", "N8", "N2", "N6", "N4")):      # deliberately not in pin order
        sch.add("R%d" % (i + 1), "tlib:R2P", (60 + 8 * i, 70), "1k", "", {"1": "+3V3", "2": net})
    sch.add("R9", "tlib:R2P", (170, 120), "1k", "", {"1": "N7", "2": "GND"})
    sch.finish()
    assert connected(sch) == declared(sch)
    assert [lab[0] for lab in sch.labels] == []             # every net is inside the block
    assert not sch.warnings


def test_without_autowire_nothing_changes(symdir):
    sch = Schematic("demo", title="Demo", lib=SymbolLib(str(symdir)))
    sch.frame(50, 50, 100, 80, "Block")
    sch.add("R1", "tlib:R2P", (80, 80), "1k", "", {"1": "IN", "2": "MID"})
    sch.add("R2", "tlib:R2P", (110, 100), "1k", "", {"1": "MID", "2": "GND"})
    sch.finish()
    assert sorted(lab[0] for lab in sch.labels) == ["IN", "MID", "MID"]


def test_wires_drawn_by_the_script_are_joined_to_the_rest_of_their_net(symdir):
    sch = design(symdir)
    sch.frame(40, 40, 160, 120, "IC")
    sch.add("U1", "tlib:COMB", (120, 100), "COMB", "",
            {"1": "N", "2": "N", "3": "X", "4": None, "5": None, "6": None, "7": None, "8": None})
    sch.add("R1", "tlib:R2P", (60, 70), "1k", "", {"1": "+3V3", "2": "N"})
    sch.add("R2", "tlib:R2P", (80, 70), "1k", "", {"1": "N", "2": "GND"})
    x, y = sch.pin("R2", 1)
    sch.wire(("R2", 1), (x, y - 5.08))                     # a stub drawn by hand, far from pins 1 and 2 of U1
    sch.finish()                                           # ... which finish() joins with a wire of their own
    assert connected(sch) == declared(sch)
    assert not sch.warnings


# ---- reference and value texts next to wired parts ------------------------------------------
@pytest.mark.parametrize("rot,mirror,flips", [
    (0, False, False), (90, False, True), (180, False, True), (270, False, False),
    (0, True, True), (90, True, False), (180, True, False), (270, True, True)])
def test_justification_swap_matches_kicad(rot, mirror, flips):
    # Read off a KiCad 10 plot of a resistor in all eight orientations with a left-justified
    # field: where the text ran to the left of its anchor, KiCad had swapped the justification.
    # The library only knew about 180 deg, so texts moved by autoplace_fields() landed on the
    # body of every resistor lying on its side.
    from hwdesign.kicad.schematic import _flips, _just
    assert _flips(rot, -1 if mirror else 1) is flips
    assert _just(rot, "left", -1 if mirror else 1) == ("right" if flips else "left")


@pytest.mark.parametrize("rot,mirror", [(90, False), (270, False), (90, True), (0, True), (180, False)])
def test_moved_texts_are_modelled_where_kicad_draws_them(rot, mirror):
    sch = Schematic("t")
    sch.add("R1", "Device:R", (100, 100), "10k", "", {"1": "A", "2": "B"}, rot=rot, mirror=mirror)
    info = sch.syminfo["R1"]
    x0, y0, x1, y1 = sch.body_box("R1")
    f = info["fields"]["Reference"]
    from hwdesign.kicad.schematic import _just, _n
    from hwdesign.kicad.sexp import Sym, find1
    find1(f, "at")[1:3] = [_n(x1 + 0.6), _n(y0 - 2)]
    eff = find1(f, "effects")
    eff[:] = [e for e in eff if not (isinstance(e, list) and e[0] == "justify")]
    eff.append(["justify", Sym(_just(rot, "left", info["mx"]))])
    box = sch._field_box(f, rot, info["mx"])
    assert box[0] == pytest.approx(x1 + 0.6)          # the text starts at the anchor and runs to the right


def test_texts_of_a_part_on_its_side_go_above_it_not_onto_its_wires():
    sch = Schematic("t", autowire=True)
    sch.add("R1", "Device:R", (100, 100), "10k", "", {"1": "A", "2": "B"}, rot=90)
    sch.add("R2", "Device:R", (120, 100), "10k", "", {"1": "B", "2": "C"}, rot=90)
    sch.frame(80, 85, 60, 30, "block")
    sch.finish()
    sch.autoplace_fields()
    assert sch.overlaps() == []
    for ref in ("R1", "R2"):
        info = sch.syminfo[ref]
        body = sch.body_box(ref)
        for name in ("Reference", "Value"):
            box = sch._field_box(info["fields"][name], info["rot"], info["mx"])
            assert box[3] <= body[1] or box[1] >= body[3]        # above or below, not at an end


def test_pins_of_a_mirrored_part_on_its_side_are_where_kicad_puts_them():
    # A FET placed with rot=270 and mirror: KiCad's ERC reported its gate above the body, drain
    # to the left and source to the right, and all three pins as not connected, because the
    # library had mirrored before rotating and wired the opposite points. Rotation by 0 or 180
    # hides the difference, so only parts lying on their side were affected.
    sch = Schematic("t")
    sch.add("Q1", "Transistor_FET:Q_NMOS_GDS", (101.6, 101.6), "x", "", {"1": "G", "2": "D", "3": "S"},
            rot=270, mirror=True)
    at = {n: (round(sch.pin("Q1", n)[0] - 101.6, 2), round(sch.pin("Q1", n)[1] - 101.6, 2)) for n in "123"}
    assert at == {"1": (0.0, -5.08), "2": (-5.08, 2.54), "3": (5.08, 2.54)}
    assert sch.parts["Q1"]["pins"]["1"]["out"] == (0, -1)


@pytest.mark.parametrize("direction,side", [("right", 1), ("left", -1)])
def test_name_of_a_power_symbol_on_its_side_is_beyond_its_tip(direction, side):
    # Seen on a controller with supply pins on its left and right: the rotated symbol's name lay
    # along the stub, struck through by the wire and the arrow.
    sch = Schematic("t")
    sch.power("+5V", (100, 100), length=5.08, direction=direction)
    info = next(v for k, v in sch.syminfo.items() if k.startswith("#PWR"))
    box = sch._field_box(info["fields"]["Value"], info["rot"], info["mx"])
    tip = 100 + side * 5.08
    assert (box[0] - tip) * side > 2.5 and (box[2] - tip) * side > 2.5      # whole name beyond the symbol
    assert box[1] < 100 < box[3]                                            # on the stub's line


def test_tied_inputs_are_joined_in_front_of_the_pins_without_a_loop(symdir):
    # A NAND gate used as an inverter, both inputs on one net: finish() drew a wire across the
    # two pin ends, the router then reached one of those pins again with a 1.27 mm hook next to
    # it, and the wire from the driving output jogged at its pin to get onto a 2.54 mm line.
    sch = design(symdir)
    sch.add("U1", "tlib:COMB", (101.6, 101.6), "x", "", {"1": "A", "2": "A", "7": "Y"})
    sch.add("U2", "tlib:COMB", (60.96, 96.52), "x", "", {"7": "A", "1": "B"})
    sch.frame(40, 70, 90, 60, "block")
    sch.finish()
    assert connected(sch) == declared(sch)
    p1, p2 = sch.pin("U1", 1), sch.pin("U1", 2)
    assert [seg for seg in sch.segments if {seg[0], seg[1]} == {p1, p2}] == []     # nothing along the pin ends
    lines = [line for net, line in sch._wire_nets() if net == "A"]
    cells = set().union(*lines)
    assert sum(len(line) - 1 for line in lines) == len(cells) - 1      # a tree: no loop, no wire drawn twice
    out = sch.pin("U2", 7)
    first = next(seg for seg in sch.segments if out in seg)
    assert abs(first[0][0] - first[1][0]) > 10                         # leaves the output straight, no jog at the pin
