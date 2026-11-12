"""Orthogonal wires between the pins of a net, on a 1.27 mm grid.

Pure geometry: no KiCad, no files. `Page.autowire()` fills a `Grid` with what is on the page
and asks it for one path after the other. The rules a path obeys are the ones that decide
what KiCad connects:

- a wire never runs over the end of a foreign pin and never along a foreign wire;
- it may cross a foreign wire at a right angle, but not where that wire ends, bends or
  branches, and it neither ends nor bends on the crossing;
- a pin is reached and left only straight in its own direction, through the cells in front
  of it, which are kept free for its net.
"""
import heapq
import itertools

CELL = 1.27
# costs: prefer short, straight, uncrossed. Off the 2.54 mm lines costs little: a pin between the lines
# is left by a straight wire; at 0.4 a 30 mm run jogged 1.27 mm right at the pin to get onto a line.
STEP, BEND, CROSS, OFF_GRID = 1.0, 3.0, 8.0, 0.1
DIRS = ((1, 0), (-1, 0), (0, 1), (0, -1))


def cell(p):
    return (round(p[0] / CELL), round(p[1] / CELL))


def point(c):
    return (round(c[0] * CELL, 4), round(c[1] * CELL, 4))


def box_cells(box, grow=0.0):
    """Cells whose centre lies in the box (x0, y0, x1, y1) in mm, grown by `grow`."""
    x0, y0, x1, y1 = box
    for i in range(int(-(-(x0 - grow) // CELL)), int((x1 + grow) // CELL) + 1):
        for j in range(int(-(-(y0 - grow) // CELL)), int((y1 + grow) // CELL) + 1):
            yield (i, j)


class Grid:
    def __init__(self):
        self.hard = set()        # bodies, labels, notes: no wire
        self.soft = {}           # cell -> extra cost (reference and value texts)
        self.pins = {}           # pin end -> (net, set of cells it may be reached from)
        self.keep = {}           # cells in front of a pin -> its net
        self.h, self.v = {}, {}  # cell -> net of the horizontal / vertical wire through it
        self.node = {}           # cell -> net of a wire end, bend or branch

    # ---- filling ----------------------------------------------------------
    def add_pin(self, at, out, net, reach=2):
        c = cell(at)
        if c in self.pins and self.pins[c][0] != net:
            net = _Clash()                               # two nets on one spot: nobody routes here
        front = (c[0] + out[0], c[1] + out[1])
        self.pins[c] = (net, self.pins.get(c, (None, set()))[1] | {front})
        self.hard.discard(c)
        for k in range(1, reach + 1):
            self.keep.setdefault((c[0] + k * out[0], c[1] + k * out[1]), net)

    def add_wire(self, net, a, b):
        """A straight wire from cell a to cell b."""
        if a == b:
            return
        along = self.h if a[1] == b[1] else self.v
        dx, dy = _sign(b[0] - a[0]), _sign(b[1] - a[1])
        c = a
        while True:
            along[c] = net
            if c == b:
                break
            c = (c[0] + dx, c[1] + dy)
        self.node[a] = self.node[b] = net

    def add_path(self, net, path):
        """Wires along a path of cells; returns its straight runs as (cell, cell)."""
        runs, start = [], path[0]
        for i in range(1, len(path)):
            if i == len(path) - 1 or _dir(path[i - 1], path[i]) != _dir(path[i], path[i + 1]):
                runs.append((start, path[i]))
                start = path[i]
        for a, b in runs:
            self.add_wire(net, a, b)
        return runs

    # ---- rules ------------------------------------------------------------
    def _foreign(self, table, c, net):
        return c in table and table[c] != net

    def _free(self, c, net, bounds):
        """May a wire of `net` lie on c at all (bends and ends included)?"""
        if not (bounds[0] <= c[0] <= bounds[2] and bounds[1] <= c[1] <= bounds[3]):
            return False
        if c in self.pins:
            return False                                  # pins are entered by _step only
        if c in self.keep:
            return self.keep[c] == net
        return c not in self.hard and not self._foreign(self.node, c, net) \
            and not self._foreign(self.h, c, net) and not self._foreign(self.v, c, net)

    def _crossable(self, c, d, net, bounds):
        """May a wire of `net` pass straight through c in direction d, over a foreign wire?"""
        if not (bounds[0] <= c[0] <= bounds[2] and bounds[1] <= c[1] <= bounds[3]):
            return False
        if c in self.pins or c in self.hard or c in self.node or self.keep.get(c, net) != net:
            return False
        same, other = (self.h, self.v) if d[1] == 0 else (self.v, self.h)
        return c not in same and self._foreign(other, c, net)

    def tappable(self, c, net):
        """May a branch of `net` start or end on cell c of its own wire?"""
        return not self._foreign(self.h, c, net) and not self._foreign(self.v, c, net)

    def _step(self, c, d, net, bounds):
        """Cells passed and the cost of leaving c in direction d, or None. The last cell is where the
        wire may bend or end; cells before it are crossings of foreign wires."""
        if c in self.pins and (c[0] + d[0], c[1] + d[1]) not in self.pins[c][1]:
            return None
        passed, cost = [], 0.0
        n = (c[0] + d[0], c[1] + d[1])
        while self._crossable(n, d, net, bounds):
            passed.append(n)
            cost += STEP + CROSS
            n = (n[0] + d[0], n[1] + d[1])
        if n in self.pins:
            if self.pins[n][0] != net or passed or c not in self.pins[n][1]:
                return None
        elif not self._free(n, net, bounds):
            return None
        off = n[0] % 2 if d[0] == 0 else n[1] % 2          # a run between the 2.54 mm lines
        return passed + [n], cost + STEP + self.soft.get(n, 0.0) + (OFF_GRID if off else 0.0)

    # ---- search -----------------------------------------------------------
    def route(self, net, sources, goal, bounds, toward=()):
        """Cheapest path from any source cell to a state accepted by goal(cell, direction).

        `toward` lists boxes of cells (x0, y0, x1, y1) that contain every goal: the search then
        heads for the nearest one instead of flooding the block.
        """
        def ahead(c):
            if not toward:
                return 0.0
            return min(max(b[0] - c[0], c[0] - b[2], 0) + max(b[1] - c[1], c[1] - b[3], 0) for b in toward)

        tie = itertools.count()
        heap, best, back = [], {}, {}
        for s in sources:
            if s in self.pins or self.tappable(s, net):
                heapq.heappush(heap, (ahead(s), next(tie), 0.0, s, None))
                best[(s, None)] = 0.0
        while heap:
            _, _, cost, c, d = heapq.heappop(heap)
            if best.get((c, d), 1e18) < cost:
                continue
            if d is not None and goal(c, d):
                path, state = [c], (c, d)
                while state in back:
                    state, between = back[state]
                    path += between[::-1] + [state[0]]
                return path[::-1]
            for e in DIRS:
                if d is not None and (e[0] == -d[0] and e[1] == -d[1]):
                    continue
                step = self._step(c, e, net, bounds)
                if step is None:
                    continue
                cells, extra = step
                new = cost + extra + (BEND if d is not None and e != d else 0.0)
                state = (cells[-1], e)
                if new < best.get(state, 1e18):
                    best[state] = new
                    back[state] = ((c, d), cells[:-1])
                    heapq.heappush(heap, (new + ahead(cells[-1]), next(tie), new, cells[-1], e))
        return None


class _Clash:
    """A net that equals nothing, not even itself: marks a spot no route may use."""

    def __eq__(self, other):
        return False

    def __ne__(self, other):
        return True

    __hash__ = object.__hash__


def _sign(v):
    return (v > 0) - (v < 0)


def _dir(a, b):
    return (_sign(b[0] - a[0]), _sign(b[1] - a[1]))
