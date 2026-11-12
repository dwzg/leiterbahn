"""Route many nets at once with the `leiterbahn-route` program (router/, Rust): negotiated congestion.

route_open() lays one connection after the other, and the first one takes the place the second
one needed: on a dense board the result depends on the order. leiterbahn-route lets all nets lie where
they want, on top of each other, and makes every contested cell dearer round by round until each
is used by one net. This module turns a board into its problem file and its answer into tracks.
"""
import json
import os
import subprocess
import time

HERE = os.path.dirname(os.path.abspath(__file__))
CRATE = os.path.normpath(os.path.join(HERE, "..", "..", "..", "router"))


def binary():
    """Path of the leiterbahn-route program; built with cargo when it is missing or older than its source."""
    exe = os.environ.get("LEITERBAHN_ROUTE") or os.path.join(CRATE, "target", "release", "leiterbahn-route")
    if os.environ.get("LEITERBAHN_ROUTE"):
        return exe
    code = os.path.join(CRATE, "src")
    src = [os.path.join(CRATE, "Cargo.toml")] + [os.path.join(code, f) for f in os.listdir(code)]
    if not os.path.exists(exe) or os.path.getmtime(exe) < max(os.path.getmtime(f) for f in src):
        done = subprocess.run(["cargo", "build", "--release"], cwd=CRATE, capture_output=True, text=True)
        if done.returncode:
            raise RuntimeError("leiterbahn-route does not build (is cargo installed?):\n" + done.stderr[-2000:])
    return exe


def shape_of(it, index):
    """An item of finish.copper_items() as leiterbahn-route's shape. index: {layer id: routing layer number}."""
    if it["kind"] == "via" or it["hole"]:
        layers = sorted(index.values())
    else:
        layers = sorted(index[l] for l in it["layers"] if l in index)
    if it["kind"] == "pad":
        return dict(box=list(it["box"]), layers=layers, hole=it["hole"])
    seg = it["seg"] if it["kind"] == "seg" else it["at"] * 2
    return dict(seg=list(seg), r=it["r"], layers=layers, hole=it["hole"])


def touches(it, box):
    if it["kind"] == "pad":
        x0, y0, x1, y1 = it["box"]
    else:
        s = it["seg"] if it["kind"] == "seg" else it["at"] * 2
        x0, y0, x1, y1 = min(s[0], s[2]), min(s[1], s[3]), max(s[0], s[2]), max(s[1], s[3])
    return x0 <= box[2] and box[0] <= x1 and y0 <= box[3] and box[1] <= y1


def problem(items, piece, rules, only=None, skip=(), box=None, step=0.1, via_cost=3.0, max_iter=200, weights=None):
    """(problem for leiterbahn-route, {net: [[item key]]} of the nets in it).

    items: finish.copper_items(); piece: {item key: number}, the same number for copper that is
    joined already. rules: dict(routing [layer ids], edge, edge_clear, clearance, via (d, hole),
    hole_clear, width {net: mm}, track, wider {net: clearance}, areas [(box, layers, no_tracks,
    no_vias)]). A net is routed if it has more than one piece (inside `box`, if given), is not in
    `skip` and is in `only` (if given); all other copper is in the way. weights: {net: number},
    see negotiate().
    """
    weights = weights or {}
    area = list(box or rules["edge"])
    index = {l: n for n, l in enumerate(rules["routing"])}
    by_net = {}
    for key, it in items.items():
        if it["net"] and key in piece and (box is None or touches(it, area)):
            by_net.setdefault(it["net"], {}).setdefault(piece[key], []).append(key)
    routed = {net: list(groups.values()) for net, groups in by_net.items()
              if len(groups) > 1 and net not in skip and (only is None or net in only)}
    keep = lambda net: max(rules["clearance"], rules["wider"].get(net, 0.0))       # noqa: E731
    nets = [dict(name=net, width=rules["width"].get(net, rules["track"]), narrow=rules["track"], clearance=keep(net),
                 weight=weights.get(net, 1.0),
                 pieces=[[shape_of(items[k], index) for k in keys] for keys in groups])
            for net, groups in routed.items()]
    reach = max(rules["wider"].values(), default=rules["clearance"]) + 1.0
    near = [area[0] - reach, area[1] - reach, area[2] + reach, area[3] + reach]
    fixed = [dict(shape_of(it, index), clearance=keep(it["net"])) for it in items.values()
             if it["net"] not in routed and touches(it, near)]
    keepouts = [dict(box=list(b), layers=sorted(index[l] for l in on if l in index), tracks=bool(no_tracks),
                     vias=bool(no_vias)) for b, on, no_tracks, no_vias in rules["areas"]]
    via = dict(d=rules["via"][0], hole=rules["via"][1], cost=via_cost)
    return dict(box=area, step=step, layers=len(index), via=via,
                hole_clear=rules["hole_clear"], hole_to_hole=rules["hole_clear"],
                edge=list(rules["edge"]), edge_clear=rules["edge_clear"],
                keepouts=keepouts, fixed=fixed, nets=nets, max_iter=max_iter), routed


def solve(prob, tmp_dir, say=None):
    """Run leiterbahn-route on a problem; returns its answer (see router/src/problem.rs)."""
    os.makedirs(tmp_dir, exist_ok=True)
    src, out = os.path.join(tmp_dir, "route-problem.json"), os.path.join(tmp_dir, "route-result.json")
    with open(src, "w") as f:
        json.dump(prob, f)
    done = subprocess.run([binary(), src, out], capture_output=True, text=True)
    if done.returncode:
        raise RuntimeError("leiterbahn-route failed: " + done.stderr[-2000:])
    if say:
        for line in done.stderr.splitlines():
            say(line)
    with open(out) as f:
        return json.load(f)


def _board_problem(board, only=None, skip=(), box=None, step=0.1, via_cost=3.0, max_iter=200, locked=False,
                   weights=None, edge_clear=0.3):
    """(the finisher's view of the board, its copper items, the problem, {net: [[item key]]})."""
    from . import finish
    r = finish._Router(board, None, step, via_cost, 0, False, 0.0, locked)
    board.fill_zones()
    items = finish.copper_items(board.board, board.copper_layers)
    conn, piece = board.board.GetConnectivity(), {}
    for n, (key, it) in enumerate(items.items()):
        if key not in piece:
            piece[key] = n
            for other in conn.GetConnectedItems(it["obj"], 0):
                piece.setdefault(other.m_Uuid.AsString(), n)
    rules = dict(routing=r.routing, edge=r.edge, edge_clear=edge_clear, clearance=r.clearance,
                 via=(r.via_d, r.via_hole), hole_clear=finish.HOLE, width=r.class_width, track=r.thin, wider=r.wider,
                 areas=r.areas)
    prob, routed = problem(items, piece, rules, None if only is None else {board.resolve(n) for n in only},
                           {board.resolve(n) for n in skip}, box, step, via_cost, max_iter,
                           {board.resolve(n): w for n, w in (weights or {}).items()})
    return r, items, prob, routed


def shut_in(board, tmp_dir, step=0.1, skip=(), say=None):
    """The nets that have no way even alone on the board: [(net, [(x, y)])].

    Each net that is in pieces is routed with no other net there. One that still cannot be
    joined has a pad shut in by other parts' pads, fixed copper, keep-out areas or the lack
    of a place for a via; no routing will join it, and the parts have to make room. The
    points are the middles of the pads of the pieces that stayed apart and of the piece they
    should have reached. skip: nets not to ask about (ground nets before their pads have
    their vias into the plane). Takes seconds: ask before routing a placement. (What shut a
    pin in on one board was a ground via of a part on the other side, set 0.2 mm in front
    of the pin by fan_to_planes(lane=0); before the vias were set, the pin had its way.)
    """
    _r, items, prob, routed = _board_problem(board, skip=skip, step=step)
    prob["alone"] = True
    answer = solve(prob, tmp_dir, say)
    found = []
    for net in answer["nets"]:
        if net["open"]:
            keys = routed[net["name"]]
            spots = []
            for piece in [0] + net["open"]:
                for box in (items[key]["box"] for key in keys[piece] if items[key]["kind"] == "pad"):
                    spots.append(((box[0] + box[2]) / 2, (box[1] + box[3]) / 2))
            found.append((net["name"], spots))
    return found


def negotiate(board, tmp_dir, only=None, skip=(), box=None, step=0.1, via_cost=3.0, max_iter=200, locked=False,
              say=None, weights=None, edge_clear=0.3):
    """Route every net of the board that is in more than one piece, all at once.

    box: (x0, y0, x1, y1) on the sheet: route only inside it (copper outside stays; a net is
    joined where its pieces reach into the box). weights: {net: number} for nets whose length
    matters more than that of others (default 1): once all nets are laid, each is given a
    shorter way where it finds one, also by pushing others aside, as long as the sum of
    lengths times weights gets smaller. edge_clear: what tracks and vias keep from the board's
    edge, mm (0.6 mm tracks were laid 0.3 mm from it). Returns dict(nets, routed, open [(net, pieces left
    apart)], rounds, seconds, cost, alone, by_net): cost is all tracks in mm with a via as
    `via_cost` and narrow track half as much again; alone is the same with each net alone on
    the board (no result is below that); by_net is {net: (cost, alone)}. Tracks are laid at
    their net class's width; a connection that has no way at that width, even with all other
    nets out of the way, is laid at the board's track width (narrow_tracks() reports it,
    widen() restores what fits).
    """
    from . import finish
    r, items, prob, routed = _board_problem(board, only, skip, box, step, via_cost, max_iter, locked, weights,
                                            edge_clear)
    started = time.time()
    answer = solve(prob, tmp_dir, say)
    grid = finish.Grid(prob["box"], step)
    left, count = [], 0
    for net in answer["nets"]:
        keys = routed[net["name"]]
        for path in net["paths"]:
            width = path["width"]
            cells = [(r.routing[l], i, j) for l, i, j in path["cells"]]
            ends = []
            for cell, end in ((cells[0], path["from"]), (cells[-1], path["to"])):
                ends.append(items[keys[end[0]][end[1]]] if end else dict(kind="point", at=grid.xy(cell[1], cell[2])))
            tracks, vias, _placed = finish.shapes(grid, cells, width, r.via_d, board.copper_layers, ends[0], ends[1])
            r.draw(net["name"], tracks, vias, width)
            count += 1
        if net["open"]:
            left.append((net["name"], len(net["open"])))
    by_net = {net["name"]: (round(net["cost"], 2), round(net["alone"], 2)) for net in answer["nets"]}
    return dict(nets=len(answer["nets"]), routed=count, open=left, rounds=answer["iterations"],
                conflicts=answer["conflicts"], seconds=round(time.time() - started, 1),
                cost=round(sum(c for c, _ in by_net.values()), 1), alone=round(sum(a for _, a in by_net.values()), 1),
                by_net=by_net)
