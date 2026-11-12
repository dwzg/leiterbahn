import math
import random

import pytest

from leiterbahn.kicad import planar


def square(x, y, side, tag=None, hole=None):
    rings = [[(x, y), (x + side, y), (x + side, y + side), (x, y + side)]]
    if hole:
        hx, hy, hs = hole
        rings.append([(hx, hy), (hx + hs, hy), (hx + hs, hy + hs), (hx, hy + hs)])
    return planar.polygon(rings, tag)


def test_segment_distance():
    assert planar.seg_dist((0, 0, 10, 0), (0, 3, 10, 3))[0] == pytest.approx(3)
    d, p, q = planar.seg_dist((0, 0, 10, 0), (5, -1, 5, 1))          # crossing
    assert d == 0 and p == q == (5, 0)
    assert planar.seg_dist((0, 0, 0, 0), (3, 4, 3, 4))[0] == pytest.approx(5)   # two points


def test_gap_between_tracks_subtracts_half_widths():
    a = [planar.capsule(0, 0, 10, 0, 0.5, "A")]
    b = [planar.capsule(0, 3, 10, 3, 0.25, "B")]
    gap, (x, y), ta, tb = planar.closest(a, b)
    assert gap == pytest.approx(2.25)
    assert y == pytest.approx(0.5 + 2.25 / 2) and (ta, tb) == ("A", "B")


def test_gap_between_two_pours_and_where():
    gap, (x, y), ta, tb = planar.closest([square(0, 0, 10, "GND1")], [square(12, 0, 10, "GND2")])
    assert gap == pytest.approx(2.0) and x == pytest.approx(11.0) and (ta, tb) == ("GND1", "GND2")


def test_touching_and_overlapping_copper_is_zero():
    assert planar.closest([square(0, 0, 10)], [square(10, 0, 5)])[0] == 0
    assert planar.closest([planar.capsule(0, 0, 10, 0, 0.2)], [planar.capsule(5, -5, 5, 5, 0.2)])[0] == 0


def test_copper_inside_a_pour_is_zero_but_inside_its_hole_is_not():
    pour = square(0, 0, 10, "pour", hole=(4, 4, 2))
    assert planar.closest([planar.circle(2, 2, 0.3, "via")], [pour])[0] == 0
    gap, _, ta, tb = planar.closest([planar.circle(5, 5, 0.3, "via")], [pour])
    assert gap == pytest.approx(0.7) and (ta, tb) == ("via", "pour")
    assert planar.closest([pour], [planar.circle(2, 2, 0.3, "via")])[2:] == ("pour", "via")


def test_empty_group_gives_none():
    assert planar.closest([], [square(0, 0, 1)]) is None
    with pytest.raises(ValueError):
        planar.polygon([[(0, 0), (1, 1)]])


def test_grid_search_agrees_with_brute_force():
    rnd = random.Random(7)

    def brute(a, b):
        return max(0.0, min(planar.seg_dist(ea, eb)[0] - sa.r - sb.r
                            for sa in a for sb in b for ea in sa.edges for eb in sb.edges))

    def tracks(n, x0, x1):
        return [planar.capsule(rnd.uniform(x0, x1), rnd.uniform(0, 40), rnd.uniform(x0, x1), rnd.uniform(0, 40),
                               rnd.uniform(0.1, 0.5)) for _ in range(n)]
    for trial in range(60):
        a = tracks(rnd.randint(1, 12), 0, 40)
        b = tracks(rnd.randint(1, 12), *((45, 90) if trial % 2 else (30, 70)))
        assert planar.closest(a, b)[0] == pytest.approx(brute(a, b), abs=1e-9)


def test_fine_polygons_far_from_the_gap_do_not_slow_it_down():
    # two pours with 4000 vertices each along the facing edges, 2 mm apart
    def comb(x, sign):
        pts = [(x + sign * (0.2 if i % 2 else 0.0), i * 0.025) for i in range(4000)]
        return planar.polygon([pts + [(x + sign * 40, 100), (x + sign * 40, 0)]])
    assert planar.closest([comb(49, -1)], [comb(51, 1)])[0] == pytest.approx(2.0)


def test_rects_overlap_with_margin():
    assert planar.rects_overlap((0, 0, 2, 2), (1, 1, 3, 3))
    assert not planar.rects_overlap((0, 0, 2, 2), (2.5, 0, 3, 2))
    assert planar.rects_overlap((0, 0, 2, 2), (2.5, 0, 3, 2), margin=0.6)


def closed(contour):
    """Every end point is shared by exactly two primitives."""
    ends = {}
    for prim in contour:
        for p in (prim[1], prim[-1]):
            k = (round(p[0], 6), round(p[1], 6))
            ends[k] = ends.get(k, 0) + 1
    return all(n == 2 for n in ends.values())


def test_rounded_rect_is_a_closed_contour():
    plain = planar.rounded_rect(0, 0, 10, 5)
    assert [p[0] for p in plain] == ["seg"] * 4 and closed(plain)
    r = planar.rounded_rect(0, 0, 10, 5, 1.5)
    assert sum(p[0] == "arc" for p in r) == 4 and closed(r)
    ends = planar.rounded_rect(0, 0, 10, 2, 1.0)             # round ends: the short sides vanish
    assert sum(p[0] == "seg" for p in ends) == 2 and closed(ends)
    with pytest.raises(ValueError):
        planar.rounded_rect(0, 0, 10, 2, 1.5)


def test_cutout_side():
    box = (100, 100, 40, 30)
    assert planar.cutout_side(box, (110, 110, 5, 2)) == "slot"
    assert planar.cutout_side(box, (100, 110, 3, 5)) == "left"
    assert planar.cutout_side(box, (138, 110, 5, 5)) == "right"           # may reach past the edge
    assert planar.cutout_side(box, (110, 99, 5, 3)) == "top"
    assert planar.cutout_side(box, (110, 128, 5, 2)) == "bottom"
    for bad in [(90, 90, 5, 5), (100, 100, 5, 5), (100, 110, 40, 2), (110, 110, 0, 2)]:
        with pytest.raises(ValueError):
            planar.cutout_side(box, bad)
    with pytest.raises(ValueError):                                       # inside the rounded corner
        planar.cutout_side(box, (101, 99, 5, 3), corner=2.0)


@pytest.mark.parametrize("rect, clipped, edge", [
    ((100, 110, 3, 5), (100, 110, 3, 5), ("x", 100)), ((137, 110, 9, 5), (137, 110, 3, 5), ("x", 140)),
    ((110, 95, 6, 8), (110, 100, 6, 3), ("y", 100)), ((110, 127, 6, 3), (110, 127, 6, 3), ("y", 130))])
@pytest.mark.parametrize("r", [0.0, 1.0])
def test_notch_keeps_the_contour_closed(rect, clipped, edge, r):
    box = (100, 100, 40, 30)
    contour, got = planar.notch(planar.rounded_rect(*box, 2.0), box, rect, r)
    assert got == pytest.approx(clipped) and closed(contour)
    assert sum(p[0] == "arc" for p in contour) == 4 + (2 if r else 0)
    # the board edge is open across the notch: no segment on that edge spans the notch's middle
    axis, line = edge
    mid = clipped[1] + clipped[3] / 2 if axis == "x" else clipped[0] + clipped[2] / 2
    for prim in contour:
        if prim[0] == "seg":
            (ax, ay), (bx, by) = prim[1], prim[2]
            if axis == "x" and ax == bx == line:
                assert not min(ay, by) < mid < max(ay, by)
            if axis == "y" and ay == by == line:
                assert not min(ax, bx) < mid < max(ax, bx)


def test_two_notches_on_one_edge_and_a_clash():
    box = (0, 0, 40, 30)
    contour, _ = planar.notch(planar.rounded_rect(*box), box, (5, 0, 4, 2))
    contour, _ = planar.notch(contour, box, (20, 0, 4, 2))
    assert closed(contour)
    with pytest.raises(ValueError):
        planar.notch(contour, box, (7, 0, 4, 2))                          # overlaps the first notch
    with pytest.raises(ValueError):
        planar.notch(contour, box, (10, 10, 4, 2))                        # a slot, not a notch


def test_notch_arcs_lie_on_the_corner_circles():
    box = (0, 0, 40, 30)
    contour, _ = planar.notch(planar.rounded_rect(*box), box, (10, 0, 6, 4), 1.0)
    arcs = [p for p in contour if p[0] == "arc"]
    assert len(arcs) == 2
    for _, start, mid, end in arcs:
        centre = (11.0, 3.0) if min(start[0], end[0]) < 13 else (15.0, 3.0)
        for p in (start, mid, end):
            assert math.hypot(p[0] - centre[0], p[1] - centre[1]) == pytest.approx(1.0)
