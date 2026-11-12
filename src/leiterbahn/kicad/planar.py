"""Plane geometry without pcbnew: the smallest gap between two groups of copper shapes, and
the contours and rectangle tests behind board cut-outs.

Everything is plain numbers in one unit (the board code passes mm), so the logic can be
tested on machines without KiCad.
"""
import math


# ---- distances ---------------------------------------------------------------
def _pt_seg(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    l2 = vx * vx + vy * vy
    u = 0.0 if l2 == 0 else max(0.0, min(1.0, ((px - ax) * vx + (py - ay) * vy) / l2))
    qx, qy = ax + u * vx, ay + u * vy
    return math.hypot(px - qx, py - qy), qx, qy


def seg_dist(a, b):
    """Distance between two segments (x1, y1, x2, y2); a segment with equal ends is a point.

    Returns (distance, point on a, point on b); crossing segments give 0 at the crossing.
    """
    ax, ay, bx, by = a
    cx, cy, dx, dy = b
    o1 = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
    o2 = (bx - ax) * (dy - ay) - (by - ay) * (dx - ax)
    o3 = (dx - cx) * (ay - cy) - (dy - cy) * (ax - cx)
    o4 = (dx - cx) * (by - cy) - (dy - cy) * (bx - cx)
    if o1 * o2 < 0 and o3 * o4 < 0:
        t = o3 / (o3 - o4)
        p = (ax + t * (bx - ax), ay + t * (by - ay))
        return 0.0, p, p
    best = None
    for px, py, seg, on_a in ((ax, ay, b, True), (bx, by, b, True), (cx, cy, a, False), (dx, dy, a, False)):
        d, qx, qy = _pt_seg(px, py, *seg)
        if best is None or d < best[0]:
            best = (d, (px, py), (qx, qy)) if on_a else (d, (qx, qy), (px, py))
    return best


class Shape:
    """A piece of copper: edges (x1, y1, x2, y2) grown by radius r, or a filled polygon (r = 0)."""
    __slots__ = ("edges", "r", "rings", "tag", "box")

    def __init__(self, edges, r, rings, tag):
        self.edges, self.r, self.rings, self.tag = edges, r, rings, tag
        xs = [v for e in edges for v in (e[0], e[2])]
        ys = [v for e in edges for v in (e[1], e[3])]
        self.box = (min(xs) - r, min(ys) - r, max(xs) + r, max(ys) + r)

    def contains(self, x, y):
        """Point inside a polygon shape (even-odd over all rings, so holes count as outside)."""
        if not self.rings:
            return any(_pt_seg(x, y, *e)[0] < self.r for e in self.edges)
        inside = False
        for ring in self.rings:
            j = len(ring) - 1
            for i in range(len(ring)):
                xi, yi = ring[i]
                xj, yj = ring[j]
                if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
                    inside = not inside
                j = i
        return inside


def capsule(ax, ay, bx, by, r, tag=None):
    """A track: the segment a-b with round ends, r = half its width."""
    return Shape([(ax, ay, bx, by)], r, None, tag)


def circle(x, y, r, tag=None):
    """A via or round pad."""
    return Shape([(x, y, x, y)], r, None, tag)


def polygon(rings, tag=None):
    """A filled polygon; rings are lists of (x, y), the outline first, then any holes."""
    rings = [list(r) for r in rings if len(r) >= 3]
    if not rings:
        raise ValueError("a polygon needs a ring of at least three points")
    edges = [(r[i - 1][0], r[i - 1][1], r[i][0], r[i][1]) for r in rings for i in range(len(r))]
    return Shape(edges, 0.0, rings, tag)


def _chunks(shapes, c):
    """Edges cut into pieces no longer than 2 c, so each piece touches only a few grid cells.
    Each piece: (x1, y1, x2, y2, r, shape index, xmin, ymin, xmax, ymax)."""
    out = []
    for i, s in enumerate(shapes):
        for ax, ay, bx, by in s.edges:
            n = max(1, int(math.ceil(max(abs(bx - ax), abs(by - ay)) / (2 * c))))
            for k in range(n):
                x1, y1 = ax + (bx - ax) * k / n, ay + (by - ay) * k / n
                x2, y2 = ax + (bx - ax) * (k + 1) / n, ay + (by - ay) * (k + 1) / n
                out.append((x1, y1, x2, y2, s.r, i, min(x1, x2), min(y1, y2), max(x1, x2), max(y1, y2)))
    return out


def _box_dist(ax0, ay0, ax1, ay1, bx0, by0, bx1, by1):
    return math.hypot(max(bx0 - ax1, ax0 - bx1, 0.0), max(by0 - ay1, ay0 - by1, 0.0))


def closest(shapes_a, shapes_b):
    """Smallest gap between any shape of group a and any shape of group b.

    Returns (gap, (x, y), tag_a, tag_b): the gap is 0 where they touch or overlap, (x, y) is
    the middle of the gap. None when a group is empty.

    Brute force over all pairs of edges takes minutes on a board with pours. Here the edges
    of b go into a grid, and each edge of a is compared only with what lies within reach R;
    R doubles until the best gap found is at most R, which proves nothing closer was missed.
    """
    if not shapes_a or not shapes_b:
        return None
    boxes = [s.box for s in shapes_a] + [s.box for s in shapes_b]
    span = max(max(b[2] for b in boxes) - min(b[0] for b in boxes),
               max(b[3] for b in boxes) - min(b[1] for b in boxes), 1e-9)
    bb = (min(s.box[0] for s in shapes_b), min(s.box[1] for s in shapes_b),
          max(s.box[2] for s in shapes_b), max(s.box[3] for s in shapes_b))
    c0 = span / 200.0
    reach, cell, best = c0, None, None
    fl = math.floor
    while True:
        if cell is None or reach / 4 > cell:
            cell = max(c0, reach / 4)
            ea, eb, grid = _chunks(shapes_a, cell), _chunks(shapes_b, cell), {}
            for j, e in enumerate(eb):
                for cx in range(int(fl((e[6] - e[4]) / cell)), int(fl((e[8] + e[4]) / cell)) + 1):
                    for cy in range(int(fl((e[7] - e[4]) / cell)), int(fl((e[9] + e[4]) / cell)) + 1):
                        grid.setdefault((cx, cy), []).append(j)
        for x1, y1, x2, y2, ra, ia, ax0, ay0, ax1, ay1 in ea:
            lim = ra + (reach if best is None else min(reach, max(best[0], 0.0)))
            if _box_dist(ax0, ay0, ax1, ay1, *bb) > lim:
                continue
            seen = set()
            for cx in range(int(fl((ax0 - lim) / cell)), int(fl((ax1 + lim) / cell)) + 1):
                for cy in range(int(fl((ay0 - lim) / cell)), int(fl((ay1 + lim) / cell)) + 1):
                    cand = grid.get((cx, cy))
                    if not cand or _box_dist(ax0, ay0, ax1, ay1, cx * cell, cy * cell,
                                             (cx + 1) * cell, (cy + 1) * cell) > lim:
                        continue
                    for j in cand:
                        if j in seen:
                            continue
                        seen.add(j)
                        e = eb[j]
                        if _box_dist(ax0, ay0, ax1, ay1, e[6], e[7], e[8], e[9]) - e[4] > lim:
                            continue
                        d, p, q = seg_dist((x1, y1, x2, y2), e[:4])
                        gap = d - ra - e[4]
                        if best is None or gap < best[0]:
                            best = (gap, p, q, d, ra, e[4], ia, e[5])
                            lim = ra + min(reach, max(gap, 0.0))
        if (best is not None and best[0] <= reach) or reach > 2 * span:
            break
        reach *= 2
    gap, p, q, d, ra, rb, ia, ib = best
    if gap > 0:
        # no edges within reach of each other, but one shape may lie wholly inside the other
        for outers, inners, swap in ((shapes_b, shapes_a, False), (shapes_a, shapes_b, True)):
            for outer in outers:
                if not outer.rings:
                    continue
                for inner in inners:
                    if (outer.box[0] <= inner.box[0] and outer.box[1] <= inner.box[1]
                            and inner.box[2] <= outer.box[2] and inner.box[3] <= outer.box[3]
                            and outer.contains(inner.edges[0][0], inner.edges[0][1])):
                        ta, tb = (outer.tag, inner.tag) if swap else (inner.tag, outer.tag)
                        return 0.0, (inner.edges[0][0], inner.edges[0][1]), ta, tb
    if gap <= 0 or d == 0:
        mid = ((p[0] + q[0]) / 2, (p[1] + q[1]) / 2)
    else:
        t = (ra + gap / 2) / d                       # from a's centre line to the middle of the gap
        mid = (p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1]))
    return max(0.0, gap), mid, shapes_a[ia].tag, shapes_b[ib].tag


# ---- rectangles and board contours ---------------------------------------------
def rects_overlap(a, b, margin=0.0):
    """True if rectangles (x0, y0, x1, y1) come closer than margin (0: they overlap)."""
    return a[0] < b[2] + margin and b[0] < a[2] + margin and a[1] < b[3] + margin and b[1] < a[3] + margin


def rounded_rect(x, y, w, h, r=0.0):
    """Closed contour of a rectangle with corner radius r, as a list of
    ("seg", start, end) and ("arc", start, mid, end); zero-length sides are left out."""
    if r < 0 or r > min(w, h) / 2 + 1e-9:
        raise ValueError("corner radius %g does not fit a %g x %g rectangle" % (r, w, h))
    out = [("seg", a, b) for a, b in [((x + r, y), (x + w - r, y)), ((x + w, y + r), (x + w, y + h - r)),
                                       ((x + w - r, y + h), (x + r, y + h)), ((x, y + h - r), (x, y + r))]
           if math.hypot(a[0] - b[0], a[1] - b[1]) > 1e-9]
    if r > 0:
        k = r * (1 - math.sqrt(0.5))
        out += [("arc", (x, y + r), (x + k, y + k), (x + r, y)),
                ("arc", (x + w - r, y), (x + w - k, y + k), (x + w, y + r)),
                ("arc", (x + w, y + h - r), (x + w - k, y + h - k), (x + w - r, y + h)),
                ("arc", (x + r, y + h), (x + k, y + h - k), (x, y + h - r))]
    return out


def cutout_side(box, rect, corner=0.0, tol=1e-6):
    """Where a rectangular cut-out sits on a board: "slot" (wholly inside) or the side it
    opens onto ("left", "right", "top", "bottom": a notch).

    box and rect are (x, y, w, h); corner is the outline's corner radius. A notch may reach
    past the edge it opens onto. Raises ValueError for a cut-out outside the board, across a
    corner, or across two edges: those need a different outline, not a cut-out.
    """
    bx0, by0, bx1, by1 = box[0], box[1], box[0] + box[2], box[1] + box[3]
    x0, y0, x1, y1 = rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]
    if rect[2] <= 0 or rect[3] <= 0:
        raise ValueError("cut-out needs a positive width and height")
    if x1 <= bx0 + tol or x0 >= bx1 - tol or y1 <= by0 + tol or y0 >= by1 - tol:
        raise ValueError("cut-out %s lies outside the board %s" % (rect, box))
    open_sides = [s for s, hit in (("left", x0 <= bx0 + tol), ("right", x1 >= bx1 - tol),
                                   ("top", y0 <= by0 + tol), ("bottom", y1 >= by1 - tol)) if hit]
    if not open_sides:
        return "slot"
    if len(open_sides) > 1:
        raise ValueError("cut-out %s crosses more than one board edge" % (rect,))
    side = open_sides[0]
    lo, hi, e0, e1 = (y0, y1, by0, by1) if side in ("left", "right") else (x0, x1, bx0, bx1)
    if lo < e0 + corner - tol or hi > e1 - corner + tol:
        raise ValueError("notch %s reaches into a board corner" % (rect,))
    return side


def notch(contour, box, rect, r=0.0):
    """Cut a notch into a board contour (as made by rounded_rect).

    rect (x, y, w, h) must open onto exactly one edge of box; its two inner corners get radius
    r. Returns (new contour, the notch clipped to the board as (x, y, w, h)).
    """
    side = cutout_side(box, rect)
    if side == "slot":
        raise ValueError("cut-out %s does not reach a board edge" % (rect,))
    bx0, by0, bx1, by1 = box[0], box[1], box[0] + box[2], box[1] + box[3]
    x0, y0 = max(rect[0], bx0), max(rect[1], by0)
    x1, y1 = min(rect[0] + rect[2], bx1), min(rect[1] + rect[3], by1)
    vertical = side in ("left", "right")
    s0, s1 = (y0, y1) if vertical else (x0, x1)             # span along the edge
    depth = (x1 - x0) if vertical else (y1 - y0)
    if r < 0 or r > min(depth, (s1 - s0) / 2) + 1e-9:
        raise ValueError("corner radius %g does not fit a notch %g wide and %g deep" % (r, s1 - s0, depth))
    line = {"left": bx0, "right": bx1, "top": by0, "bottom": by1}[side]
    inward = 1 if side in ("left", "top") else -1

    def pt(s, d):                                         # s along the edge, d into the board
        return (line + inward * d, s) if vertical else (s, line + inward * d)

    def along(p):
        return p[1] if vertical else p[0]

    def on_line(p):
        return abs((p[0] if vertical else p[1]) - line) < 1e-6

    out, cut = [], False
    for prim in contour:
        if (not cut and prim[0] == "seg" and on_line(prim[1]) and on_line(prim[2])
                and min(along(prim[1]), along(prim[2])) <= s0 + 1e-6
                and max(along(prim[1]), along(prim[2])) >= s1 - 1e-6):
            a, b = sorted((prim[1], prim[2]), key=along)
            k = r * (1 - math.sqrt(0.5))
            new = [("seg", a, pt(s0, 0)), ("seg", pt(s0, 0), pt(s0, depth - r)),
                   ("seg", pt(s0 + r, depth), pt(s1 - r, depth)),
                   ("seg", pt(s1, depth - r), pt(s1, 0)), ("seg", pt(s1, 0), b)]
            out += [p for p in new if math.hypot(p[1][0] - p[2][0], p[1][1] - p[2][1]) > 1e-9]
            if r > 0:
                out += [("arc", pt(s0, depth - r), pt(s0 + k, depth - k), pt(s0 + r, depth)),
                        ("arc", pt(s1 - r, depth), pt(s1 - k, depth - k), pt(s1, depth - r))]
            cut = True
        else:
            out.append(prim)
    if not cut:
        raise ValueError("no straight board edge spans the notch %s (a corner or another notch is in the way)"
                         % (rect,))
    return out, (x0, y0, x1 - x0, y1 - y0)
