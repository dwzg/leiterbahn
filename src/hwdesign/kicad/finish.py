"""Finish what the autorouter left open: a grid search, one connection at a time.

Freerouting leaves single connections open on dense boards and does not say why (two
neighbouring header pins, a test point under a pad). This router is slow and plain, but it
is deterministic and only needs a free path: tracks on a 0.1 mm grid, 45 degree steps, vias
between all routing layers. Where there is no free path it takes one through the autorouter's
tracks, if each of those finds another way; otherwise nothing is changed. KiCad's DRC stays
the judge of the result.
"""
import heapq
import json
import math
import os
import subprocess
import tempfile

import pcbnew

FINE = 0.05            # grid of the second try, in a window around the connection
WINDOW = 6.0           # mm around the two ends that the second try may use
HOLE = 0.25            # copper to a foreign drilled hole


def open_pairs(pcb_path):
    """What KiCad's DRC reports as unconnected: [((uuid, x, y), (uuid, x, y))] in mm."""
    with tempfile.TemporaryDirectory() as tmp:
        out = os.path.join(tmp, "drc.json")
        subprocess.run(["kicad-cli", "pcb", "drc", "--severity-all", "--format", "json", "-o", out, pcb_path],
                       check=True, capture_output=True)
        with open(out) as f:
            data = json.load(f)
    pairs = []
    for u in data.get("unconnected_items", []):
        a, b = u["items"][0], u["items"][1]
        pairs.append(((a["uuid"], a["pos"]["x"], a["pos"]["y"]), (b["uuid"], b["pos"]["x"], b["pos"]["y"])))
    return pairs


def copper_items(pb, layers, fixed=()):
    """Pads, tracks and vias as plain data the grid can stamp:
    {uuid: dict(kind, net, layers, box | seg | at, r, hole, soft, obj)}. soft: an unlocked track
    or via that is not in `fixed` (uuids): the search may remove it when there is no other way."""
    mm = pcbnew.ToMM
    items = {}
    for fp in pb.GetFootprints():
        for pd in fp.Pads():
            on = {l for l in layers if pd.IsOnLayer(l)}
            hole = mm(max(pd.GetDrillSize().x, pd.GetDrillSize().y)) / 2
            if not on and not hole:
                continue
            bb = pd.GetBoundingBox()
            items[pd.m_Uuid.AsString()] = dict(
                kind="pad", net=pd.GetNetname(), layers=on, hole=hole, soft=False, obj=pd,
                at=(mm(pd.GetPosition().x), mm(pd.GetPosition().y)),
                box=(mm(bb.GetLeft()), mm(bb.GetTop()), mm(bb.GetRight()), mm(bb.GetBottom())))
    for t in pb.GetTracks():
        uuid = t.m_Uuid.AsString()
        soft = not t.IsLocked() and uuid not in fixed
        if t.GetClass() == "PCB_VIA":
            items[uuid] = dict(
                kind="via", net=t.GetNetname(), layers=set(layers), hole=mm(t.GetDrill()) / 2, soft=soft, obj=t,
                at=(mm(t.GetPosition().x), mm(t.GetPosition().y)), r=mm(t.GetWidth(pcbnew.F_Cu)) / 2)
        else:
            a, e = t.GetStart(), t.GetEnd()
            items[uuid] = dict(kind="seg", net=t.GetNetname(), layers={t.GetLayer()}, hole=0.0, soft=soft, obj=t,
                               seg=(mm(a.x), mm(a.y), mm(e.x), mm(e.y)), r=mm(t.GetWidth()) / 2)
    return items


def rule_areas(pb, layers):
    """[(box, layers, no_tracks, no_vias)] of the board's rule areas, by bounding box."""
    mm = pcbnew.ToMM
    out = []
    for z in pb.Zones():
        if z.GetIsRuleArea():
            bb = z.GetBoundingBox()
            out.append(((mm(bb.GetLeft()), mm(bb.GetTop()), mm(bb.GetRight()), mm(bb.GetBottom())),
                        {l for l in layers if z.IsOnLayer(l)}, z.GetDoNotAllowTracks(), z.GetDoNotAllowVias()))
    return out


class Grid:
    """Cells of `step` mm over the board; one byte per cell and layer: 1 = no track centre here,
    2 = only by removing a track that may be removed."""

    def __init__(self, box, step):
        self.x0, self.y0 = box[0], box[1]
        self.step = step
        self.nx = int(round((box[2] - box[0]) / step)) + 1
        self.ny = int(round((box[3] - box[1]) / step)) + 1

    def plane(self):
        return [bytearray(self.nx) for _ in range(self.ny)]

    def cell(self, x, y):
        return int(round((x - self.x0) / self.step)), int(round((y - self.y0) / self.step))

    def xy(self, i, j):
        return self.x0 + i * self.step, self.y0 + j * self.step

    def stamp(self, plane, box, grow, value=b"\x01"):
        """Block every cell whose centre is within `grow` of the rectangle."""
        i0 = max(0, int(math.ceil((box[0] - grow - self.x0) / self.step - 1e-9)))
        i1 = min(self.nx - 1, int(math.floor((box[2] + grow - self.x0) / self.step + 1e-9)))
        j0 = max(0, int(math.ceil((box[1] - grow - self.y0) / self.step - 1e-9)))
        j1 = min(self.ny - 1, int(math.floor((box[3] + grow - self.y0) / self.step + 1e-9)))
        if i1 < i0:
            return
        fill = value * (i1 - i0 + 1)
        for j in range(j0, j1 + 1):
            plane[j][i0:i1 + 1] = fill

    def stamp_item(self, plane, it, grow, value=b"\x01"):
        if it["kind"] == "pad":
            self.stamp(plane, it["box"], grow, value)
        elif it["kind"] == "via":
            x, y = it["at"]
            self.stamp(plane, (x, y, x, y), grow + it["r"], value)
        else:
            x0, y0, x1, y1 = it["seg"]
            n = max(1, int(math.hypot(x1 - x0, y1 - y0) / self.step))
            for k in range(n + 1):
                x, y = x0 + (x1 - x0) * k / n, y0 + (y1 - y0) * k / n
                self.stamp(plane, (x, y, x, y), grow + it["r"], value)


def obstacles(grid, items, areas, edge, net, layers, half, clearance, via_r):
    """(blocked plane per layer, plane where no via may sit) for a track of half width `half` on `net`."""
    blocked = {l: grid.plane() for l in layers}
    no_via = grid.plane()
    keep = clearance + grid.step / 2                       # a track between two cell centres is up to half a
    x0, y0, x1, y1 = edge                                  # cell nearer to what the centres are clear of
    for plane, grow in [(blocked[l], half) for l in layers] + [(no_via, via_r)]:
        inset = 0.3 + grow                                 # copper to the board edge
        for box in ((x0 - 9, y0 - 9, x1 + 9, y0), (x0 - 9, y1, x1 + 9, y1 + 9), (x0 - 9, y0, x0, y1),
                    (x1, y0, x1 + 9, y1)):
            grid.stamp(plane, box, inset)
    for box, on, no_tracks, no_vias in areas:
        for l in on & set(layers):
            if no_tracks:
                grid.stamp(blocked[l], box, half)
        if no_vias and on:
            grid.stamp(no_via, box, via_r)
    for soft, value in ((True, b"\x02"), (False, b"\x01")):         # what cannot be removed is stamped on top
        for it in items.values():
            if (it["net"] == net and net) or it["soft"] != soft:
                continue
            for l in it["layers"] & set(layers):
                grid.stamp_item(blocked[l], it, half + keep, value)
            if it["layers"]:                               # a via lands on every layer: keep off all foreign copper
                grid.stamp_item(no_via, it, via_r + keep, value)
            if it["hole"]:
                x, y = it["at"]
                for l in layers:
                    grid.stamp(blocked[l], (x, y, x, y), it["hole"] + HOLE + half, value)
                grid.stamp(no_via, (x, y, x, y), it["hole"] + HOLE + via_r, value)
    for it in items.values():                              # own SMD pads take no via either (no via in pad),
        if it["net"] != net:                               # and a second hole keeps its distance from the first
            continue
        if it["kind"] == "pad" and not it["hole"]:
            grid.stamp(no_via, it["box"], via_r * 0.5)
        elif it["hole"]:
            x, y = it["at"]
            grid.stamp(no_via, (x, y, x, y), it["hole"] + HOLE + via_r)
    return blocked, no_via


def cells_of(grid, it, layers):
    """Grid cells from which a track is in contact with this item: [(layer, i, j)]."""
    on = sorted(it["layers"] & set(layers)) if it["kind"] not in ("via",) and not it["hole"] else list(layers)
    out = []
    if it["kind"] == "point":
        on = sorted(it["layers"] & set(layers))
        pts = [grid.cell(*it["at"])]
    elif it["kind"] == "pad" and not it["hole"]:
        x0, y0, x1, y1 = it["box"]
        dx, dy = (x1 - x0) * 0.25, (y1 - y0) * 0.25        # the middle half: inside the copper of any pad shape
        i0, j0 = grid.cell(x0 + dx, y0 + dy)
        i1, j1 = grid.cell(x1 - dx, y1 - dy)
        pts = [(i, j) for i in range(i0, i1 + 1) for j in range(j0, j1 + 1)]
    elif it["kind"] == "seg":
        x0, y0, x1, y1 = it["seg"]
        n = max(1, int(math.hypot(x1 - x0, y1 - y0) / grid.step))
        pts = sorted({grid.cell(x0 + (x1 - x0) * k / n, y0 + (y1 - y0) * k / n) for k in range(n + 1)})
    else:
        pts = [grid.cell(*it["at"])]
    for l in on:
        out += [(l, i, j) for i, j in pts if 0 <= i < grid.nx and 0 <= j < grid.ny]
    return out


def search(grid, blocked, no_via, starts, goals, layers, via_cost, limit, rip=0.0):
    """Cheapest path from any start cell to any goal cell: [(layer, i, j)], or a string saying why not.
    rip: cost of a cell that is only free once a removable track is gone (0: such cells are closed)."""
    wall = (1,) if rip else (1, 2)
    goal = {(l, i, j) for l, i, j in goals if blocked[l][j][i] not in wall}
    if not goal:
        return "no room for a track at the second end"
    if all(blocked[l][j][i] in wall for l, i, j in starts):
        return "no room for a track at the first end"
    gi = sum(i for _l, i, _j in goal) / len(goal)
    gj = sum(j for _l, _i, j in goal) / len(goal)
    spread = max(max(abs(i - gi), abs(j - gj)) for _l, i, j in goal)

    def h(i, j):
        """Distance to the goals' middle, less their spread: never more than the way to the nearest."""
        dx, dy = abs(i - gi), abs(j - gj)
        return max(0.0, max(dx, dy) + 0.414 * min(dx, dy) - 1.5 * spread)

    best, prev, heap = {}, {}, []
    for node in starts:
        l, i, j = node
        if blocked[l][j][i] not in wall:
            best[node] = 0.0
            heapq.heappush(heap, (h(i, j), 0.0, node))
    steps = ((1, 0, 1.0), (-1, 0, 1.0), (0, 1, 1.0), (0, -1, 1.0),
             (1, 1, 1.414), (1, -1, 1.414), (-1, 1, 1.414), (-1, -1, 1.414))
    nx, ny = grid.nx, grid.ny
    done = 0
    while heap:
        _f, g, node = heapq.heappop(heap)
        if g > best.get(node, 1e18):
            continue
        if node in goal:
            path = [node]
            while path[-1] in prev:
                path.append(prev[path[-1]])
            return path[::-1]
        done += 1
        if done > limit:
            return "search limit"
        l, i, j = node
        plane = blocked[l]
        for di, dj, cost in steps:
            a, c = i + di, j + dj
            if not (0 <= a < nx and 0 <= c < ny):
                continue
            v = plane[c][a]
            if v in wall:
                continue
            if di and dj and (plane[j][a] in wall or plane[c][i] in wall):    # no cutting of corners
                continue
            nxt = (l, a, c)
            ng = g + cost + (rip if v else 0.0)
            if ng < best.get(nxt, 1e18):
                best[nxt] = ng
                prev[nxt] = node
                heapq.heappush(heap, (ng + h(a, c), ng, nxt))
        nv = no_via[j][i]
        if nv not in wall:
            for other in layers:
                v = blocked[other][j][i]
                if other != l and v not in wall:
                    nxt = (other, i, j)
                    ng = g + via_cost + (4 * rip if nv else 0.0) + (rip if v else 0.0)
                    if ng < best.get(nxt, 1e18):
                        best[nxt] = ng
                        prev[nxt] = node
                        heapq.heappush(heap, (ng + h(i, j), ng, nxt))
    return "no path"


def corners(path):
    """Drop the cells in the middle of straight runs: [(layer, i, j)] with only bends and layer changes."""
    out = [path[0]]
    for k in range(1, len(path) - 1):
        (l0, i0, j0), (l1, i1, j1), (l2, i2, j2) = out[-1], path[k], path[k + 1]
        if l0 != l1 or l1 != l2 or (i1 - i0) * (j2 - j1) != (j1 - j0) * (i2 - i1):
            out.append(path[k])
    out.append(path[-1])
    return out


def seg_dist(p, q):
    """Smallest distance between two segments (x0, y0, x1, y1); a point is a segment of no length."""
    def pt(px, py, x0, y0, x1, y1):
        dx, dy = x1 - x0, y1 - y0
        d2 = dx * dx + dy * dy
        t = 0.0 if not d2 else max(0.0, min(1.0, ((px - x0) * dx + (py - y0) * dy) / d2))
        return math.hypot(px - x0 - t * dx, py - y0 - t * dy)

    def side(ax, ay, bx, by, cx, cy):
        return (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)

    a0, a1, b0, b1 = p[:2], p[2:], q[:2], q[2:]
    if (side(*a0, *a1, *b0) * side(*a0, *a1, *b1) < 0 and side(*b0, *b1, *a0) * side(*b0, *b1, *a1) < 0):
        return 0.0
    return min(pt(*a0, *q), pt(*a1, *q), pt(*b0, *p), pt(*b1, *p))


def touching(p, q):
    """Do two items of one net share copper on a layer?"""
    if not (p["layers"] & q["layers"]) and not (p["hole"] or q["hole"]):
        return False

    def shape(it):
        if it["kind"] == "pad":
            return "box", it["box"], 0.0
        if it["kind"] == "seg":
            return "seg", it["seg"], it["r"]
        return "seg", it["at"] * 2, it.get("r", 0.0)

    (ka, ga, ra), (kb, gb, rb) = shape(p), shape(q)
    if ka == "box" and kb == "box":
        return ga[0] <= gb[2] and gb[0] <= ga[2] and ga[1] <= gb[3] and gb[1] <= ga[3]
    if ka == "box":
        (ka, ga, ra), (kb, gb, rb) = (kb, gb, rb), (ka, ga, ra)
    if kb == "box":                                        # a track or via against a pad: its end is in the pad
        return any(gb[0] <= x <= gb[2] and gb[1] <= y <= gb[3] for x, y in (ga[:2], ga[2:]))
    return seg_dist(ga, gb) <= max(ra, rb)                 # an end on the other's copper, as KiCad wants it


def cluster_of(items, start):
    """The keys of all items of start's net that are joined to it by tracks, vias and pads."""
    net = items[start]["net"]
    rest = {k for k, it in items.items() if it["net"] == net and k != start}
    found, todo = {start}, [start]
    while todo:
        cur = items[todo.pop()]
        near = [k for k in rest if touching(cur, items[k])]
        rest.difference_update(near)
        found.update(near)
        todo += near
    return found


def nearest_on(it, x, y):
    """Where a track that ends at (x, y) has to end instead to be connected to this track, via or
    point (KiCad wants the end on the other's copper; a grid point can be just beside it). None: a pad."""
    if it["kind"] in ("via", "point"):
        return it["at"]
    if it["kind"] == "seg":
        x0, y0, x1, y1 = it["seg"]
        dx, dy = x1 - x0, y1 - y0
        d2 = dx * dx + dy * dy
        t = 0.0 if not d2 else max(0.0, min(1.0, ((x - x0) * dx + (y - y0) * dy) / d2))
        return (x0 + t * dx, y0 + t * dy)
    return None


def runs_in_the_way(items, placed, net, keep):
    """Removable tracks and vias that the new copper `placed` [(layers, seg, r)] comes too close to,
    each as its whole run up to the next pad, junction or fixed track:
    [(net, {uuid}, [end])], an end being the item to reconnect to (a pad, or a point on some layers)."""
    hit = []
    for uuid, it in items.items():
        if not it["soft"] or it["net"] == net:
            continue
        shape = it["seg"] if it["kind"] == "seg" else it["at"] * 2
        for layers, seg, r in placed:
            if it["layers"] & layers and seg_dist(shape, seg) < r + it["r"] + keep:
                hit.append(uuid)
                break

    def key(x, y):
        return (round(x, 3), round(y, 3))

    def points(it):
        if it["kind"] == "seg":
            return [it["seg"][:2], it["seg"][2:]]
        return [it["at"]] if it["kind"] == "via" else []

    nets = {items[u]["net"] for u in hit}
    pads = {n: [it for it in items.values() if it["kind"] == "pad" and it["net"] == n] for n in nets}
    at = {n: {} for n in nets}                             # net -> point -> items that end there
    for uuid, it in items.items():
        if it["net"] in nets:
            for x, y in points(it):
                at[it["net"]].setdefault(key(x, y), []).append(uuid)
    runs, seen = [], set()
    for first in hit:
        if first in seen:
            continue
        n = items[first]["net"]
        chain, ends, todo = {first}, [], [first]
        while todo:
            cur = items[todo.pop()]
            for x, y in points(cur):
                pad = [p for p in pads[n] if p["layers"] & cur["layers"]
                       and p["box"][0] <= x <= p["box"][2] and p["box"][1] <= y <= p["box"][3]]
                others = [u for u in at[n][key(x, y)] if u not in chain]
                if pad:
                    ends.append(pad[0])
                elif len(others) == 1 and items[others[0]]["soft"]:
                    chain.add(others[0])
                    todo.append(others[0])
                elif others:                               # a junction, or copper that stays
                    ends.append(dict(kind="point", net=n, at=(x, y), layers=set(cur["layers"]), hole=0.0))
        seen |= chain
        runs.append((n, chain, ends))
    return runs


def shapes(grid, path, w, via_d, all_layers, a, b):
    """A found path as copper: (tracks [(layer, [(x, y)])], vias [(x, y)], placed [(layers, seg, r)])."""
    pts = corners(path)
    tracks, vias, placed = [], [], []
    run = [pts[0]]
    for node in pts[1:] + [None]:
        if node is not None and node[0] == run[-1][0]:
            run.append(node)
            continue
        xy = [grid.xy(i, j) for _l, i, j in run]
        if run[0] is pts[0]:                               # the first and the last run end on the copper they
            snap = nearest_on(a, *xy[0])                   # join: the end point moves there (an extra piece
            if snap and snap != xy[0]:                     # could fold back on the run), unless a via sits on it
                xy[0:len(xy) > 1] = [snap]
        if node is None:
            snap = nearest_on(b, *xy[-1])
            if snap and snap != xy[-1]:
                if len(xy) > 1 and not (run[0] is pts[0] and len(xy) == 2):
                    xy[-1] = snap
                else:
                    xy.append(snap)
        if len(xy) > 1:
            tracks.append((run[0][0], xy))
            placed += [({run[0][0]}, p + q, w / 2) for p, q in zip(xy, xy[1:], strict=False)]
        if node is not None:                               # a layer change: the via sits on the shared cell
            x, y = grid.xy(node[1], node[2])
            vias.append((x, y))
            placed.append((set(all_layers), (x, y, x, y), via_d / 2))
            run = [node]
    return tracks, vias, placed


def route_open(board, tmp_dir, width=None, step=0.1, via_cost=3.0, limit=400000, skip=(), only=None, locked=False,
               rip=2.0, rounds=4, fine=True, depth=2):
    """Connect what is unconnected on `board` (a hwdesign Board). Returns (routed, failed):
    failed lists (net, reason, (x, y), (x, y)) for the connections that found no path.

    Each connection is tried at its net class's track width, then at `width` (default: the
    board's track width); narrow_tracks() reports the second kind and widen() restores what
    fits. Ground nets on plane layers are routed like any other net: give their pads vias
    with fan_to_planes() first. Nets in `skip` are left alone; with `only`, all others are.
    Wide nets find room more easily before the autorouter has run: route them first with
    only=[...], locked=True, so the autorouter keeps them.

    Where no free path exists, the search may cross unlocked tracks at the price of `rip` mm
    of detour per 0.1 mm crossed (rip=0: never). Each crossed track is taken out up to the
    next pad or junction and has to find another way, which may in turn cross tracks, `depth`
    levels deep; the last level needs free paths. If one track finds no way, nothing is
    changed and the connection is reported. So a call never leaves more open than it found.
    Locked copper is never touched. `limit` bounds one search (grid cells visited).

    A connection that finds no way on the grid of `step` mm is tried again on a 0.05 mm grid
    in a window around its ends (fine=False: not): between two vias 1 mm apart a 0.1 mm grid
    may have no line that keeps the clearance to both, though a track fits.
    """
    pb = board.board
    routing = [l for l in board.copper_layers if l not in board.planes]
    thin = width or board.rules["track"]
    via_d, via_hole = board.rules["via"]
    clearance = board.rules["clearance"]
    bb = pb.GetBoardEdgesBoundingBox()
    mm = pcbnew.ToMM
    edge = (mm(bb.GetLeft()), mm(bb.GetTop()), mm(bb.GetRight()), mm(bb.GetBottom()))
    grid = Grid(edge, step)
    class_width = {n: c["track"] for c in board.classes.values() for n in c["nets"]}
    skip = {board.resolve(n) for n in skip}
    only = None if only is None else {board.resolve(n) for n in only}
    areas = rule_areas(pb, board.copper_layers)
    poured = {z.GetNetname() for z in pb.Zones() if not z.GetIsRuleArea()}     # joined by copper this module
    pcb = os.path.join(tmp_dir, "route_open.kicad_pcb")                        # does not see: pad to pad only
    fresh = [0]

    def box_of(it):
        if it["kind"] == "seg":
            x0, y0, x1, y1 = it["seg"]
            return (min(x0, x1), min(y0, y1), max(x0, x1), max(y0, y1))
        return it["box"] if it["kind"] == "pad" else it["at"] * 2

    def find(items, net, a, b, rip_mm, ka=None):
        """(tracks, vias, placed, width, margin) or the reason why not; rip_mm as in route_open.
        ka: a's key in items. Then the track may start anywhere on the copper joined to a and end
        on any other copper of the net: the ratsnest names one pad, the nearest track will do."""
        reason = "no path"
        mine, others = [a], [b]
        if ka is not None and net not in poured:
            group = cluster_of(items, ka)
            mine = [items[k] for k in group]
            others = [it for k, it in items.items() if it["net"] == net and k not in group]
        grids = [grid]
        if fine and step > FINE:
            (ax0, ay0, ax1, ay1), (bx0, by0, bx1, by1) = box_of(a), box_of(b)
            x0, y0 = min(ax0, bx0) - WINDOW, min(ay0, by0) - WINDOW
            grids.append(Grid((max(edge[0], math.floor(x0 / step) * step), max(edge[1], math.floor(y0 / step) * step),
                               min(edge[2], max(ax1, bx1) + WINDOW), min(edge[3], max(ay1, by1) + WINDOW)), FINE))
        for g in grids:
            for w in dict.fromkeys((class_width.get(net, thin), thin)):
                blocked, no_via = obstacles(g, items, areas, edge, net, routing, w / 2, clearance, via_d / 2)
                first = {c: it for it in mine for c in cells_of(g, it, routing)}
                last = {c: it for it in others for c in cells_of(g, it, routing)}
                path = search(g, blocked, no_via, list(first), list(last), routing,
                              via_cost / g.step, limit, rip_mm / 0.1)
                if not isinstance(path, str):
                    return shapes(g, path, w, via_d, board.copper_layers, first[path[0]], last[path[-1]]) \
                        + (w, g.step / 2)
                reason = reason if g is not grid else path
        return reason

    def note(items, net, placed):
        """The new copper as obstacles for what follows; returns their keys."""
        keys = []
        for layers, seg, r in placed:
            fresh[0] += 1
            kind = "seg" if len(layers) == 1 else "via"
            keys.append("new%d" % fresh[0])
            items[keys[-1]] = dict(kind=kind, net=net, layers=layers, soft=False, r=r, seg=seg, at=seg[:2],
                                   hole=via_hole / 2 if kind == "via" else 0.0)
        return keys

    drawn = set()

    def draw(net, tracks, vias, w):
        for layer, xy in tracks:
            for p, q in zip(xy, xy[1:], strict=False):     # two connections of a net may share their first steps
                key = (net, layer) + tuple(sorted(((round(p[0], 3), round(p[1], 3)), (round(q[0], 3), round(q[1], 3)))))
                if key not in drawn:
                    drawn.add(key)
                    board.track(net, [p, q], w, layer, locked=locked)
        for x, y in vias:
            if (net, round(x, 3), round(y, 3)) not in drawn:
                drawn.add((net, round(x, 3), round(y, 3)))
                board.via(net, x, y, locked=locked)

    def attempt(items, net, a, b, ka, levels, taken, added, plans):
        """Lay the connection in `items` only: True, or the reason why not. What it took out is in
        `taken`, what it put in under the keys `added`, what to draw in `plans`."""
        got = find(items, net, a, b, 0.0, ka)
        if not isinstance(got, str):
            added += note(items, net, got[2])
            plans.append((net,) + got)
            return True
        if not rip or levels < 1:
            return got
        through = find(items, net, a, b, rip, ka)
        if isinstance(through, str):
            return through
        runs = runs_in_the_way(items, through[2], net, clearance + through[4])
        taken.update({u: items.pop(u) for _n, chain, _e in runs for u in chain if u in items})
        added += note(items, net, through[2])
        plans.append((net,) + through)
        for n, _chain, ends in runs:
            for end in ends[1:]:
                again = attempt(items, n, ends[0], end, None, levels - 1, taken, added, plans)
                if again is not True:
                    return got + ", and the tracks in the way have no other"
        return True

    mine, routed, failed = set(), 0, []
    for _round in range(rounds):
        board.fill_zones()
        pcbnew.SaveBoard(pcb, pb)
        pairs = open_pairs(pcb)
        items = copper_items(pb, board.copper_layers, fixed=mine)
        pairs.sort(key=lambda p: math.hypot(p[0][1] - p[1][1], p[0][2] - p[1][2]))
        failed, progress = [], 0
        before = {t.m_Uuid.AsString() for t in pb.GetTracks()}
        for (ua, ax, ay), (ub, bx, by) in pairs:
            if ua not in items or ub not in items:         # moved in this round: the next one sees what is open
                failed.append((items[ub if ua not in items else ua]["net"] if ua in items or ub in items else "?",
                               "one end was moved; not tried again", (ax, ay), (bx, by)))
                continue
            a, b = items[ua], items[ub]
            net = a["net"]
            if net in skip or (only is not None and net not in only):
                continue
            if net not in poured and ub in cluster_of(items, ua):
                continue                                   # joined by a track laid earlier in this round
            taken, added, plans = {}, [], []
            done = attempt(items, net, a, b, ua, depth, taken, added, plans)
            if done is not True:                           # leave all as it was
                for k in added:
                    items.pop(k, None)
                items.update(taken)
                failed.append((net, done, (ax, ay), (bx, by)))
                continue
            for it in taken.values():
                if "obj" in it:
                    pb.Remove(it["obj"])
            for n, tracks, vias, _placed, w, _m in plans:
                draw(n, tracks, vias, w)
            routed, progress = routed + 1, progress + 1
        mine |= {t.m_Uuid.AsString() for t in pb.GetTracks()} - before
        if not progress or not failed:
            break
    return routed, failed
