//! Grid router with negotiated congestion.
//!
//! All nets are routed, each along its cheapest path, and may at first lie on top of each
//! other. A cell that two nets want gets more expensive with every round (and stays a little
//! more expensive for good), so the net with the better way round takes it. No net is laid
//! "first": the order of the nets does not decide who gets the last free place, as it does
//! when one connection is routed after the other. (McMurchie and Ebeling, PathFinder, 1995.)
//!
//! Tracks run on a square grid in eight directions; a via joins all routing layers. Widths and
//! clearances are kept by counting, for every kind of track (width, clearance), how much copper
//! lies too near each cell for the centre of such a track.
pub mod kicad;
pub mod place;
pub mod problem;
pub mod sexp;

use problem::{Heat, Path, Problem, Role, Routed, Shape, Solution, Stuck};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

mod smooth;
mod untangle;

const NONE: u32 = u32::MAX;
const DIAG: f32 = std::f32::consts::SQRT_2;
const STEPS: [(i32, i32, f32); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, DIAG),
    (1, -1, DIAG),
    (-1, 1, DIAG),
    (-1, -1, DIAG),
];

/// Copper in cell units: a rectangle or a line, grown by `r`.
#[derive(Clone, Debug)]
struct Item {
    rect: bool,
    a: [f64; 4],
    /// A rectangle may be turned about its middle: (cosine, sine) of the angle, anticlockwise on a
    /// sheet whose y axis points down, as KiCad counts. (1, 0): upright.
    turn: (f64, f64),
    r: f64,
    layers: Vec<usize>,
    hole: f64,
    clearance: f64,
}

/// The copper a net has been given by the router.
#[derive(Clone, Default)]
struct Laid {
    nodes: Vec<Vec<u32>>,
    /// The kind of track of each path.
    kinds: Vec<usize>,
    paths: Vec<Path>,
    items: Vec<Item>,
    vias: Vec<u32>,
    open: Vec<usize>,
    /// A search found no way even across other nets: trying again changes nothing.
    stuck: bool,
}

/// Where tracks have changed, and when: the board in tiles of `tile` cells, each with the
/// count of changes at the last one near it.
struct Fresh {
    stamps: Vec<u32>,
    nx: i32,
    ny: i32,
    tile: i32,
    now: u32,
}

impl Fresh {
    /// Has anything changed in the box since `since`?
    fn hit(&self, b: [i32; 4], since: u32) -> bool {
        let (i0, i1) = ((b[0] / self.tile).clamp(0, self.nx - 1), (b[2] / self.tile).clamp(0, self.nx - 1));
        let (j0, j1) = ((b[1] / self.tile).clamp(0, self.ny - 1), (b[3] / self.tile).clamp(0, self.ny - 1));
        (j0..=j1).any(|j| (i0..=i1).any(|i| self.stamps[(j * self.nx + i) as usize] > since))
    }

    /// The cells have changed: the tiles within `reach` cells of them count as changed now.
    fn note(&mut self, cells: impl Iterator<Item = (i32, i32)>, reach: i32) {
        self.now += 1;
        let mut tiles: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
        for (i, j) in cells {
            tiles.insert((i / self.tile, j / self.tile));
        }
        let r = reach / self.tile + 1;
        for (ti, tj) in tiles {
            for j in (tj - r).max(0)..=(tj + r).min(self.ny - 1) {
                for i in (ti - r).max(0)..=(ti + r).min(self.nx - 1) {
                    self.stamps[(j * self.nx + i) as usize] = self.now;
                }
            }
        }
    }
}

struct Grids {
    nx: usize,
    ny: usize,
    nl: usize,
    step: f64,
    /// Kinds of track: (half width, clearance), and of via: clearance. In millimetres.
    kinds: Vec<(f64, f64)>,
    via_kinds: Vec<f64>,
    via_r: f64,
    via_hole: f64,
    hole_clear: f64,
    /// Per kind of track, a cell for every place on every layer; per kind of via, one for every place.
    track: Vec<Vec<Cell>>,
    via: Vec<Vec<Cell>>,
    /// Nets that are to keep apart: how far, in mm (0: none are), and per cell how much copper
    /// of noisy nets (0) and of quiet nets (1) a track's centre there would be too near to.
    apart: f64,
    near: [Vec<Cell>; 3],
    /// The planes of nets that have one: per cell of a layer, whether a via there is in the plane.
    planes: Vec<Vec<bool>>,
}

/// What a track's centre (or a via's) finds at one place: how much copper that stays is too
/// near (`hard`), how much routed copper of other nets (`soft`), and what the place has cost
/// in earlier rounds. Together in 8 bytes: the search reads all three for every step, and as
/// three arrays they were three places in memory.
#[derive(Clone, Copy, Default)]
struct Cell {
    hard: u16,
    soft: u16,
    hist: f32,
}

fn dist2(item: &Item, x: f64, y: f64) -> f64 {
    let a = &item.a;
    if item.rect {
        let (mut x, mut y) = (x, y);
        if item.turn.1 != 0.0 {
            // the point as the turned rectangle sees it
            let (cx, cy, (c, s)) = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0, item.turn);
            let (dx, dy) = (x - cx, y - cy);
            (x, y) = (cx + dx * c - dy * s, cy + dx * s + dy * c);
        }
        let dx = (a[0] - x).max(x - a[2]).max(0.0);
        let dy = (a[1] - y).max(y - a[3]).max(0.0);
        return dx * dx + dy * dy;
    }
    let (vx, vy) = (a[2] - a[0], a[3] - a[1]);
    let len2 = vx * vx + vy * vy;
    let t = if len2 > 0.0 {
        (((x - a[0]) * vx + (y - a[1]) * vy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dy) = (x - a[0] - t * vx, y - a[1] - t * vy);
    dx * dx + dy * dy
}

/// Add `delta` to the hard (or soft) count of every cell of one layer whose centre is within
/// `grow` cells of the item.
#[allow(clippy::too_many_arguments)]
fn raster(grid: &mut [Cell], soft: bool, nx: usize, ny: usize, layer: usize, item: &Item, grow: f64, delta: i32) {
    let a = &item.a;
    let (mut x0, mut x1) = (a[0].min(a[2]) - grow, a[0].max(a[2]) + grow);
    let (mut y0, mut y1) = (a[1].min(a[3]) - grow, a[1].max(a[3]) + grow);
    if item.rect && item.turn.1 != 0.0 {
        let (w, h, (c, s)) = ((a[2] - a[0]) / 2.0, (a[3] - a[1]) / 2.0, item.turn);
        let (cx, cy, ex, ey) = (
            (a[0] + a[2]) / 2.0,
            (a[1] + a[3]) / 2.0,
            (w * c).abs() + (h * s).abs(),
            (w * s).abs() + (h * c).abs(),
        );
        (x0, x1, y0, y1) = (cx - ex - grow, cx + ex + grow, cy - ey - grow, cy + ey + grow);
    }
    if x1 < 0.0 || y1 < 0.0 || x0 > (nx - 1) as f64 || y0 > (ny - 1) as f64 {
        return;
    }
    let i0 = (x0 - 1e-9).ceil().max(0.0) as usize;
    let i1 = ((x1 + 1e-9).floor() as usize).min(nx - 1);
    let j0 = (y0 - 1e-9).ceil().max(0.0) as usize;
    let j1 = ((y1 + 1e-9).floor() as usize).min(ny - 1);
    let limit = grow * grow + 1e-9;
    let (dx, dy) = (a[2] - a[0], a[3] - a[1]);
    for j in j0..=j1 {
        let row = (layer * ny + j) * nx;
        // of a slanted line only the part near this row matters: without this a long diagonal
        // costs its whole bounding box
        let (mut i0, mut i1) = (i0, i1);
        if !item.rect && dy != 0.0 {
            let (t0, t1) = (((j as f64 - grow - a[1]) / dy).clamp(0.0, 1.0), ((j as f64 + grow - a[1]) / dy).clamp(0.0, 1.0));
            let (xa, xb) = (a[0] + t0 * dx, a[0] + t1 * dx);
            i0 = i0.max((xa.min(xb) - grow - 1e-9).ceil().max(0.0) as usize);
            i1 = i1.min(((xa.max(xb) + grow + 1e-9).floor().max(0.0) as usize).min(nx - 1));
        }
        for i in i0..=i1 {
            if dist2(item, i as f64, j as f64) <= limit {
                let cell = &mut grid[row + i];
                let count = if soft { &mut cell.soft } else { &mut cell.hard };
                *count = (*count as i32 + delta).clamp(0, u16::MAX as i32) as u16;
            }
        }
    }
}

impl Grids {
    fn node(&self, l: usize, i: usize, j: usize) -> u32 {
        ((l * self.ny + j) * self.nx + i) as u32
    }

    fn split(&self, node: u32) -> (usize, usize, usize) {
        let n = node as usize;
        (n / (self.nx * self.ny), n % self.nx, (n / self.nx) % self.ny)
    }

    /// Count the item in (or out of) every kind's grids. `fixed`: copper that does not lie on the
    /// grid, which tracks keep a little further from.
    ///
    /// How much further: a track runs from cell centre to cell centre, and between two centres
    /// it can be nearer to a corner beside it than at either: for a distance r to keep and a step
    /// of length L, by root(r squared + L squared / 4) less r. A slanted step is only made where
    /// the two cells beside it are free as well (see `Search::run`), and those four cells
    /// between them leave no corner nearer than r to the step: so only the straight step of one
    /// cell counts, 1 um at r = 0.27 mm on a 0.05 mm grid, and a fiftieth of a cell is added.
    /// (It was half a cell for all, 25 um on that grid, and then that for a slanted step plus a
    /// tenth of a cell, 8 um: at a connector of 0.4 mm pitch with 0.2 mm between pads and 0.2 mm
    /// clearance a pad has 26 um to either side of its middle line for a track of 0.147 mm,
    /// and with 8 um gone a quarter of the pads had no cell there.) Laid copper is crossed at a
    /// price, not closed, so nothing guards a slanted step past it: there the slanted step counts.
    fn stamp(&mut self, item: &Item, soft: bool, fixed: bool, delta: i32) {
        let (nx, ny, nl, step) = (self.nx, self.ny, self.nl, self.step);
        // (Laid copper lies on the grid, but the same holds between two cells of it: a track past a
        // via on a 0.1 mm grid came 2 um nearer than a clearance of 0.254 mm allows.)
        let with = |r: f64| {
            if fixed {
                (r * r + step * step / 4.0).sqrt() + 0.02 * step
            } else {
                (r * r + step * step / 2.0).sqrt()
            }
        };
        let centre = Item {
            rect: false,
            turn: (1.0, 0.0),
            a: [
                (item.a[0] + item.a[2]) / 2.0,
                (item.a[1] + item.a[3]) / 2.0,
                (item.a[0] + item.a[2]) / 2.0,
                (item.a[1] + item.a[3]) / 2.0,
            ],
            r: 0.0,
            layers: vec![],
            hole: 0.0,
            clearance: 0.0,
        };
        for k in 0..self.kinds.len() {
            let (half, clearance) = self.kinds[k];
            let grow = item.r + with(item.clearance.max(clearance) + half) / step;
            let grid = &mut self.track[k];
            for &l in &item.layers {
                raster(grid, soft, nx, ny, l, item, grow, delta);
            }
            if item.hole > 0.0 {
                let grow = item.hole + with(self.hole_clear + half) / step;
                for l in 0..nl {
                    raster(grid, soft, nx, ny, l, &centre, grow, delta);
                }
            }
        }
        for c in 0..self.via_kinds.len() {
            let grid = &mut self.via[c];
            if !item.layers.is_empty() {
                let keep = (item.clearance.max(self.via_kinds[c]) + self.via_r).max(self.hole_clear + self.via_hole / 2.0);
                raster(grid, soft, nx, ny, 0, item, item.r + with(keep) / step, delta);
            }
            if item.hole > 0.0 {
                let grow = item.hole + with(self.hole_clear + self.via_r) / step;
                raster(grid, soft, nx, ny, 0, &centre, grow, delta);
            }
        }
    }
}

impl Grids {
    /// Is there anything tracks keep away from where they can?
    fn shy(&self) -> bool {
        !self.near[0].is_empty()
    }

    /// Count the cells within `room` mm of the item as cells all tracks keep out of where they can.
    fn room(&mut self, item: &Item, room: f64) {
        let thin = self.kinds.iter().map(|k| k.0).fold(f64::MAX, f64::min);
        let grow = item.r + (room + thin) / self.step;
        for which in 0..3 {
            for &l in &item.layers {
                raster(&mut self.near[which], false, self.nx, self.ny, l, item, grow, 1);
            }
        }
    }

    /// Count the item in (or out of) the cells that tracks of the other kind are to keep out of:
    /// `which` is 0 for copper of a noisy net, 1 for copper of a quiet one. (`near[2]` holds what
    /// all nets keep away from, see room(), and so do the other two.)
    fn beside(&mut self, item: &Item, which: usize, delta: i32) {
        if self.apart <= 0.0 || which > 1 || !self.shy() {
            return;
        }
        let thin = self.kinds.iter().map(|k| k.0).fold(f64::MAX, f64::min);
        let grow = item.r + (self.apart + thin) / self.step;
        for &l in &item.layers {
            raster(&mut self.near[which], false, self.nx, self.ny, l, item, grow, delta);
        }
    }
}

/// Is the copper of two nets nearer than they may be? (A little generous: it only chooses which
/// nets to lay again; the grids decide whether the result is clean.)
fn clash(a: &Item, b: &Item, step: f64) -> bool {
    if !a.layers.iter().any(|l| b.layers.contains(l)) {
        return false;
    }
    let ends = |i: &Item| ([i.a[0], i.a[1]], [i.a[2], i.a[3]]);
    let ((p, q), (r, s)) = (ends(a), ends(b));
    let to = |x: [f64; 2], i: &Item| dist2(i, x[0], x[1]).sqrt();
    let side = |u: [f64; 2], v: [f64; 2], w: [f64; 2]| (v[0] - u[0]) * (w[1] - u[1]) - (v[1] - u[1]) * (w[0] - u[0]);
    let cross = (side(p, q, r) > 0.0) != (side(p, q, s) > 0.0) && (side(r, s, p) > 0.0) != (side(r, s, q) > 0.0);
    let apart = if cross { 0.0 } else { to(p, b).min(to(q, b)).min(to(r, a)).min(to(s, a)) };
    apart < a.r + b.r + a.clearance.max(b.clearance) / step + 1.5
}

/// What the search needs of one net.
struct Job {
    kind: usize,
    /// The kind of a narrow track, if the net may have one.
    narrow: Option<usize>,
    via_kind: usize,
    clearance: f64,
    /// Its pieces' copper, and per piece the cells a track may start or end on, with the shape.
    items: Vec<Vec<Item>>,
    cells: Vec<Vec<(u32, usize)>>,
    boxes: Vec<[i32; 4]>,
    /// The copper its tracks are to keep away from: `Grids::near` 0 (it is quiet) or 1 (noisy).
    shy: Option<usize>,
    /// Its plane, if it has one (`Grids::planes`): then its last piece is that plane, with no cells.
    plane: Option<usize>,
}

/// The cells a search has reached and not yet gone on from, cheapest first to a quarter of a
/// cell. A binary heap took 43 % of the router's time on a board with 158 nets: here a cell
/// goes into the bucket of its cost, and the buckets are emptied in turn. The ring of buckets
/// covers costs up to `RING / 4` cells above the cheapest waiting cell; what costs more (a way
/// across other nets, late in the rounds) waits in a heap until the ring gets there.
struct Queue {
    ring: Vec<Vec<(u32, f32)>>,
    far: BinaryHeap<Reverse<(u64, u32, u32)>>,
    /// The bucket being emptied, the highest one used, and how many cells are in the ring.
    at: u64,
    top: u64,
    count: usize,
}

const RING: u64 = 1 << 16;

impl Queue {
    fn new() -> Queue {
        Queue {
            ring: vec![Vec::new(); RING as usize],
            far: BinaryHeap::new(),
            at: 0,
            top: 0,
            count: 0,
        }
    }

    fn clear(&mut self) {
        if self.count > 0 {
            for b in self.at..=self.top.min(self.at + RING - 1) {
                self.ring[(b % RING) as usize].clear();
            }
        }
        self.far.clear();
        (self.at, self.top, self.count) = (0, 0, 0);
    }

    /// Go back to a cheaper bucket, for cells that are about to be put in below the one being
    /// emptied (`Search::wave` going on from more sources). False if the ring cannot hold both
    /// that bucket and its highest one.
    fn rewind(&mut self, to: u64) -> bool {
        if to >= self.at {
            return true;
        }
        if self.count > 0 && self.top >= to + RING {
            return false;
        }
        if self.count == 0 {
            self.top = to;
        }
        self.at = to;
        true
    }

    /// `f`: what the way through this cell will cost at least; `g`: what it cost to get here.
    fn push(&mut self, f: f32, g: f32, node: u32) {
        let b = ((f * 4.0) as u64).max(self.at);
        if b < self.at + RING {
            self.ring[(b % RING) as usize].push((node, g));
            self.top = self.top.max(b);
            self.count += 1;
        } else {
            self.far.push(Reverse((b, g.to_bits(), node)));
        }
    }

    /// What has come within the ring's reach moves from the heap into the ring.
    fn pull(&mut self) {
        while let Some(&Reverse((b, g, node))) = self.far.peek() {
            if b >= self.at + RING {
                break;
            }
            self.far.pop();
            self.ring[(b % RING) as usize].push((node, f32::from_bits(g)));
            self.top = self.top.max(b);
            self.count += 1;
        }
    }

    fn pop(&mut self) -> Option<(u32, f32)> {
        loop {
            if self.count == 0 {
                let &Reverse((b, _, _)) = self.far.peek()?;
                self.at = b;
                self.pull();
            }
            if let Some(entry) = self.ring[(self.at % RING) as usize].pop() {
                self.count -= 1;
                return Some(entry);
            }
            self.at += 1;
            self.pull();
        }
    }
}

#[derive(Clone, Copy)]
struct Mark {
    g: f32,
    prev: u32,
    seen: u32,
}

struct Search {
    /// Per cell: what the way to it costs, the cell before it, and the search that wrote this.
    marks: Vec<Mark>,
    round: u32,
    queue: Queue,
    target: Vec<u32>,
    /// Cells where the net being laid has a via already (== `laying`): a free change of layer.
    own_via: Vec<u32>,
    laying: u32,
    /// Cells the search may use: [i0, j0, i1, j1].
    window: [i32; 4],
    expanded: u64,
    /// The copper the net being laid keeps away from (see `Grids::near`), and what a cell too
    /// near it costs more, in cells.
    shy: Option<usize>,
    shy_cost: f32,
    /// For a search with many goals: per tile of `FAR` cells, how far the nearest goal is at
    /// least, and the search that wrote this.
    far: Vec<f32>,
    far_seen: Vec<u32>,
    /// Counts the searches made and gone on with: what `far_seen` holds.
    far_round: u32,
    /// A way that costs more than this is not looked for.
    bound: f32,
    /// The last search found no way and had looked at every cell it could reach.
    spent: bool,
    /// Per cell: the mark of the last search without a way that reached it (`Router::lay_on`).
    reach: Vec<u32>,
    reached: u32,
    /// The plane of the net being laid, while that is still to be joined (`Grids::planes`): a way
    /// may end in a via anywhere inside it.
    plane: Option<usize>,
}

/// The side of a tile of `Search::far`, in cells.
const FAR: i32 = 8;

fn item_of(shape: &Shape, x0: f64, y0: f64, step: f64, clearance: f64) -> Item {
    let (rect, a) = match (shape.rect, shape.seg) {
        (Some(b), _) => (true, b),
        (None, Some(s)) => (false, s),
        (None, None) => panic!("a shape needs a box or a seg"),
    };
    let (sin, cos) = shape.angle.to_radians().sin_cos();
    Item {
        rect,
        turn: if rect && shape.angle != 0.0 { (cos, sin) } else { (1.0, 0.0) },
        a: [(a[0] - x0) / step, (a[1] - y0) / step, (a[2] - x0) / step, (a[3] - y0) / step],
        r: shape.r / step,
        layers: shape.layers.clone(),
        hole: shape.hole / step,
        clearance,
    }
}

/// Cells from which a track is in contact with the item: the middle half of a pad, the centre of
/// a drilled one, the cells along a track.
fn cells_of(grids: &Grids, item: &Item) -> Vec<u32> {
    let a = &item.a;
    let mut pts: Vec<(i64, i64)> = Vec::new();
    if item.hole > 0.0 {
        pts.push((((a[0] + a[2]) / 2.0).round() as i64, ((a[1] + a[3]) / 2.0).round() as i64));
    } else if item.rect && item.turn.1 != 0.0 {
        // a turned pad: the cells within its middle half, or the one at its middle
        let (cx, cy) = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0);
        let half = Item {
            a: [(a[0] + cx) / 2.0, (a[1] + cy) / 2.0, (a[2] + cx) / 2.0, (a[3] + cy) / 2.0],
            ..item.clone()
        };
        let reach = ((a[2] - a[0]).abs().max((a[3] - a[1]).abs()) / 2.0).ceil() as i64;
        for j in cy.round() as i64 - reach..=cy.round() as i64 + reach {
            for i in cx.round() as i64 - reach..=cx.round() as i64 + reach {
                if dist2(&half, i as f64, j as f64) < 1e-9 {
                    pts.push((i, j));
                }
            }
        }
        if pts.is_empty() {
            pts.push((cx.round() as i64, cy.round() as i64));
        }
    } else if item.rect {
        let (dx, dy) = ((a[2] - a[0]) * 0.25, (a[3] - a[1]) * 0.25);
        let (i0, i1) = ((a[0] + dx).round() as i64, (a[2] - dx).round() as i64);
        let (j0, j1) = ((a[1] + dy).round() as i64, (a[3] - dy).round() as i64);
        for j in j0..=j1.max(j0) {
            for i in i0..=i1.max(i0) {
                pts.push((i, j));
            }
        }
    } else {
        let len = ((a[2] - a[0]).powi(2) + (a[3] - a[1]).powi(2)).sqrt();
        let n = (len.floor() as usize).max(1);
        for k in 0..=n {
            let t = k as f64 / n as f64;
            let p = ((a[0] + (a[2] - a[0]) * t).round() as i64, (a[1] + (a[3] - a[1]) * t).round() as i64);
            if pts.last() != Some(&p) {
                pts.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for &l in &item.layers {
        for &(i, j) in &pts {
            if i >= 0 && j >= 0 && (i as usize) < grids.nx && (j as usize) < grids.ny {
                out.push(grids.node(l, i as usize, j as usize));
            }
        }
    }
    out
}

/// The bends and layer changes of a path.
fn corners(grids: &Grids, nodes: &[u32]) -> Vec<[u32; 3]> {
    let cell = |n: u32| {
        let (l, i, j) = grids.split(n);
        [l as i64, i as i64, j as i64]
    };
    let mut out: Vec<[i64; 3]> = vec![cell(nodes[0])];
    for k in 1..nodes.len().saturating_sub(1) {
        let (p, c, n) = (*out.last().unwrap(), cell(nodes[k]), cell(nodes[k + 1]));
        if p[0] != c[0] || c[0] != n[0] || (c[1] - p[1]) * (n[2] - c[2]) != (c[2] - p[2]) * (n[1] - c[1]) {
            out.push(c);
        }
    }
    if nodes.len() > 1 {
        out.push(cell(nodes[nodes.len() - 1]));
    }
    out.iter().map(|c| [c[0] as u32, c[1] as u32, c[2] as u32]).collect()
}

/// All the cells of a track given by its bends; None if a piece of it is not straight or slanted
/// by 45 degrees, a layer change moves, or a bend is off the board.
fn expand(grids: &Grids, bends: &[[u32; 3]]) -> Option<Vec<u32>> {
    let mut nodes: Vec<u32> = Vec::new();
    for (k, b) in bends.iter().enumerate() {
        if b[0] as usize >= grids.nl || b[1] as usize >= grids.nx || b[2] as usize >= grids.ny {
            return None;
        }
        let here = grids.node(b[0] as usize, b[1] as usize, b[2] as usize);
        if k == 0 {
            nodes.push(here);
            continue;
        }
        let p = bends[k - 1];
        let (dx, dy) = (b[1] as i64 - p[1] as i64, b[2] as i64 - p[2] as i64);
        if p[0] != b[0] {
            if dx != 0 || dy != 0 {
                return None;
            }
            nodes.push(here);
        } else if dx != 0 && dy != 0 && dx.abs() != dy.abs() {
            return None;
        } else {
            for s in 1..=dx.abs().max(dy.abs()) {
                nodes.push(grids.node(
                    b[0] as usize,
                    (p[1] as i64 + s * dx.signum()) as usize,
                    (p[2] as i64 + s * dy.signum()) as usize,
                ));
            }
        }
    }
    (nodes.len() > 1).then_some(nodes)
}

/// A path's copper: a line per straight piece, a via per layer change.
fn copper(grids: &Grids, job: &Job, half: f64, bends: &[[u32; 3]], items: &mut Vec<Item>, vias: &mut Vec<u32>) {
    for w in bends.windows(2) {
        let (p, q) = (w[0], w[1]);
        if p[0] == q[0] {
            items.push(Item {
                rect: false,
                a: [p[1] as f64, p[2] as f64, q[1] as f64, q[2] as f64],
                turn: (1.0, 0.0),
                r: half / grids.step,
                layers: vec![p[0] as usize],
                hole: 0.0,
                clearance: job.clearance,
            });
        } else {
            items.push(Item {
                rect: false,
                a: [p[1] as f64, p[2] as f64, p[1] as f64, p[2] as f64],
                turn: (1.0, 0.0),
                r: grids.via_r / grids.step,
                layers: (0..grids.nl).collect(),
                hole: grids.via_hole / 2.0 / grids.step,
                clearance: job.clearance,
            });
            let flat = (p[2] as usize * grids.nx + p[1] as usize) as u32;
            if !vias.contains(&flat) {
                vias.push(flat);
            }
        }
    }
}

impl Search {
    /// Cheapest path from any of `sources` to a cell marked in `self.target`. `press`: what a
    /// cell another net lies on costs more, per net; None: such a cell is closed. `memory`: how
    /// much of what a cell cost in earlier rounds still counts (0: nothing).
    #[allow(clippy::too_many_arguments)]
    fn run(
        &mut self,
        grids: &Grids,
        kind: usize,
        via_kind: usize,
        sources: &[u32],
        goals: &[[i32; 4]],
        press: Option<f32>,
        memory: f32,
        via_cost: f32,
        limit: u64,
    ) -> Option<Vec<u32>> {
        self.wave(grids, kind, via_kind, sources, None, goals, press, memory, via_cost, limit)
    }

    /// `run`, or with `more` the last search gone on with: the sources from index `more` on are
    /// new, and what that search had reached and still had waiting stays. To be asked for only
    /// if nothing but the goals has changed since (fewer of them) and no other search was made.
    /// A net of 168 pads is joined pad by pad, always from all that is joined to the nearest
    /// pad left: begun anew each time, each of those 167 searches opened 400 000 cells around
    /// everything joined so far, 64 million for the net.
    #[allow(clippy::too_many_arguments)]
    fn wave(
        &mut self,
        grids: &Grids,
        kind: usize,
        via_kind: usize,
        sources: &[u32],
        more: Option<usize>,
        goals: &[[i32; 4]],
        press: Option<f32>,
        memory: f32,
        via_cost: f32,
        limit: u64,
    ) -> Option<Vec<u32>> {
        if more.is_none() {
            self.round += 1;
        }
        // (the estimates are per call: with fewer goals than the search had before, they are larger)
        self.far_round += 1;
        let (round, far_round) = (self.round, self.far_round);
        let window = self.window;
        let (nx, ny, nl) = (grids.nx as i32, grids.ny as i32, grids.nl);
        let (track, via) = (&grids.track[kind], &grids.via[via_kind]);
        let (shy, shy_cost) = (self.shy.map(|a| &grids.near[a]), self.shy_cost);
        // How far the nearest goal is at least. With many goals that is looked up per tile, and
        // worked out when a tile is first touched: one box round all the goals, as it was, is no
        // estimate at all inside that box, and a net of forty pads searched in all directions from
        // all it had laid (300 000 cells opened for a way of 50).
        let many = goals.len() > 12;
        let tiles = nx / FAR + 1;
        let (far, far_seen) = (&mut self.far, &mut self.far_seen);
        let mut h = |i: i32, j: i32| -> f32 {
            let (lo, hi, at) = if many {
                let (ti, tj) = (i / FAR, j / FAR);
                let at = (tj * tiles + ti) as usize;
                if far_seen[at] == far_round {
                    return far[at];
                }
                ([ti * FAR, tj * FAR], [ti * FAR + FAR - 1, tj * FAR + FAR - 1], at)
            } else {
                ([i, j], [i, j], 0)
            };
            let mut best = f32::MAX;
            for b in goals {
                let dx = (b[0] - hi[0]).max(lo[0] - b[2]).max(0) as f32;
                let dy = (b[1] - hi[1]).max(lo[1] - b[3]).max(0) as f32;
                best = best.min(dx.max(dy) + (DIAG - 1.0) * dx.min(dy));
            }
            if many {
                (far[at], far_seen[at]) = (best, far_round);
            }
            best
        };
        let usable = |s: u32| !(track[s as usize].hard > 0 || (press.is_none() && track[s as usize].soft > 0));
        // going on: from the cheapest bucket a new source belongs in, if the ring can go back to it
        let mut from = 0;
        if let Some(at) = more {
            let low = sources[at..]
                .iter()
                .filter(|&&s| usable(s))
                .map(|&s| {
                    let (_l, i, j) = grids.split(s);
                    (h(i as i32, j as i32) * 4.0) as u64
                })
                .min();
            if low.is_none_or(|low| self.queue.rewind(low)) {
                from = at;
            } else {
                self.round += 1;
                return self.wave(grids, kind, via_kind, sources, None, goals, press, memory, via_cost, limit);
            }
        } else {
            self.queue.clear();
        }
        for &s in &sources[from..] {
            if !usable(s) {
                continue;
            }
            self.marks[s as usize] = Mark {
                g: 0.0,
                prev: NONE,
                seen: round,
            };
            let (_l, i, j) = grids.split(s);
            self.queue.push(h(i as i32, j as i32), 0.0, s);
        }
        let mut opened = 0u64;
        self.spent = false;
        let mut dry = true;
        while let Some((node, g)) = self.queue.pop() {
            let n = node as usize;
            if g > self.marks[n].g {
                continue; // a cheaper way to this cell was found after this one was queued
            }
            if self.queue.at as f32 / 4.0 > self.bound {
                dry = false;
                break; // whatever is found from here on costs more than the caller will pay
            }
            if more.is_some() {
                // A cell that waited since before a goal was reached waits by its distance from
                // that goal, perhaps: it goes back by what it is from those left. (Without this
                // going on opened more cells than beginning anew, round every goal just reached.)
                let (_l, i, j) = grids.split(node);
                let f = g + h(i as i32, j as i32);
                if (f * 4.0) as u64 > self.queue.at {
                    self.queue.push(f, g, node);
                    continue;
                }
            }
            // (a way that has just changed layers inside the net's plane has reached the plane)
            let in_plane = self.plane.is_some_and(|m| {
                let prev = self.marks[n].prev;
                let area = (nx * ny) as u32;
                prev != NONE && prev / area != node / area && grids.planes[m][(node % area) as usize]
            });
            if self.target[n] != 0 || in_plane {
                let mut path = vec![node];
                while self.marks[*path.last().unwrap() as usize].prev != NONE {
                    path.push(self.marks[*path.last().unwrap() as usize].prev);
                }
                path.reverse();
                self.expanded += opened;
                return Some(path);
            }
            opened += 1;
            if opened > limit {
                dry = false;
                break;
            }
            let (l, i, j) = grids.split(node);
            let (i, j) = (i as i32, j as i32);
            let layer = l * (nx * ny) as usize;
            for &(di, dj, len) in &STEPS {
                let (a, c) = (i + di, j + dj);
                if a < window[0] || c < window[1] || a > window[2] || c > window[3] {
                    continue;
                }
                let m = layer + (c * nx + a) as usize;
                let cell = track[m];
                if cell.hard > 0 {
                    continue;
                }
                if di != 0 && dj != 0 && (track[layer + (j * nx + a) as usize].hard > 0 || track[layer + (c * nx + i) as usize].hard > 0) {
                    continue;
                }
                let crowd = match (press, cell.soft) {
                    (_, 0) => 1.0,
                    (None, _) => continue,
                    (Some(p), s) => 1.0 + p * s as f32,
                };
                let mut ng = g + len * (1.0 + cell.hist * memory) * crowd;
                if shy.is_some_and(|near| near[m].hard > 0) {
                    ng += len * shy_cost;
                }
                let mark = &mut self.marks[m];
                if mark.seen != round || ng < mark.g {
                    *mark = Mark {
                        g: ng,
                        prev: node,
                        seen: round,
                    };
                    self.queue.push(ng + h(a, c), ng, m as u32);
                }
            }
            let flat = (j * nx + i) as usize;
            let cost = if self.own_via[flat] == self.laying {
                0.0
            } else {
                let cell = via[flat];
                if cell.hard > 0 {
                    continue;
                }
                let crowd = match (press, cell.soft) {
                    (_, 0) => 1.0,
                    (None, _) => continue,
                    (Some(p), s) => 1.0 + p * s as f32,
                };
                via_cost * (1.0 + cell.hist * memory) * crowd
            };
            for other in 0..nl {
                let m = other * (nx * ny) as usize + flat;
                let cell = track[m];
                if other == l || cell.hard > 0 || (press.is_none() && cell.soft > 0) {
                    continue;
                }
                let ng = g + cost + press.unwrap_or(0.0) * cell.soft as f32;
                let mark = &mut self.marks[m];
                if mark.seen != round || ng < mark.g {
                    *mark = Mark {
                        g: ng,
                        prev: node,
                        seen: round,
                    };
                    self.queue.push(ng + h(i, j), ng, m as u32);
                }
            }
        }
        self.expanded += opened;
        self.spent = dry;
        None
    }
}

struct Router<'a> {
    problem: &'a Problem,
    grids: Grids,
    jobs: Vec<Job>,
    laid: Vec<Laid>,
    search: Search,
    via_cost: f32,
    /// All cells, and whether a search that finds no way in its window may use them.
    whole: [i32; 4],
    widen: bool,
    /// The passes that shorten are on: `lay_near` leaves a track alone that is straight already.
    direct: bool,
    /// How much more than its tracks that go the net's new ones may cost, in cells: a search
    /// ends without a way when that is spent. None: whatever it takes.
    budget: Option<f32>,
    /// The rounds are getting nowhere: a way that crosses another net is checked against the
    /// whole board (see `lay_on`).
    far: bool,
    /// They are about to stall: a way at the net's width that crosses another net is checked
    /// against a narrow one.
    pinch: bool,
}

impl Router<'_> {
    /// Take the net's own pads out of the way of its tracks (or put them back), and keep its
    /// vias out of its own pads: no via in a pad, no second hole beside the first.
    fn own(&mut self, n: usize, delta: i32) {
        let (nx, ny, step) = (self.grids.nx, self.grids.ny, self.grids.step);
        let (via_r, hole_clear, via_kind) = (self.grids.via_r, self.grids.hole_clear, self.jobs[n].via_kind);
        for piece in 0..self.jobs[n].items.len() {
            for s in 0..self.jobs[n].items[piece].len() {
                let item = self.jobs[n].items[piece][s].clone();
                self.grids.stamp(&item, false, true, -delta);
                let grid = &mut self.grids.via[via_kind];
                if item.hole > 0.0 {
                    let (x, y) = ((item.a[0] + item.a[2]) / 2.0, (item.a[1] + item.a[3]) / 2.0);
                    let centre = Item {
                        rect: false,
                        a: [x, y, x, y],
                        turn: (1.0, 0.0),
                        r: 0.0,
                        layers: vec![],
                        hole: 0.0,
                        clearance: 0.0,
                    };
                    raster(grid, false, nx, ny, 0, &centre, item.hole + (hole_clear + via_r) / step, delta);
                } else if item.rect {
                    raster(grid, false, nx, ny, 0, &item, via_r * 0.5 / step, delta);
                }
            }
        }
    }

    fn soft(&mut self, n: usize, delta: i32) {
        for k in 0..self.laid[n].items.len() {
            let item = self.laid[n].items[k].clone();
            self.grids.stamp(&item, true, false, delta);
            if let Some(a) = self.jobs[n].shy.filter(|&a| a < 2) {
                self.grids.beside(&item, 1 - a, delta);
            }
        }
    }

    /// Count the net's tracks and vias as copper that stays, or take that back: closed to the
    /// others, not crossed at a price.
    fn hard(&mut self, n: usize, delta: i32) {
        for k in 0..self.laid[n].items.len() {
            let item = self.laid[n].items[k].clone();
            self.grids.stamp(&item, false, false, delta);
        }
    }

    /// The cells (a diagonal step as its length) of the net's tracks that are nearer than they
    /// should be to copper the net is to keep away from.
    fn beside_of(&self, n: usize, laid: &Laid) -> f32 {
        let Some(near) = self.jobs[n].shy.filter(|_| self.grids.shy()).map(|a| &self.grids.near[a]) else {
            return 0.0;
        };
        let plane = (self.grids.nx * self.grids.ny) as u32;
        let mut total = 0.0;
        for path in &laid.nodes {
            for w in path.windows(2) {
                let apart = w[0].abs_diff(w[1]);
                if w[0] / plane == w[1] / plane && near[w[1] as usize].hard > 0 {
                    total += if apart == 1 || apart == self.grids.nx as u32 { 1.0 } else { DIAG };
                }
            }
        }
        total
    }

    /// Cells of the net's tracks and vias that another net's copper is too near to. The net's own
    /// copper must be out of the counts. With `remember`, those cells get dearer for good.
    fn conflicts(&mut self, n: usize, remember: f32) -> u64 {
        let via_kind = self.jobs[n].via_kind;
        let mut count = 0;
        for (path, &kind) in self.laid[n].nodes.iter().zip(&self.laid[n].kinds) {
            for &node in path {
                let cell = &mut self.grids.track[kind][node as usize];
                if cell.soft > 0 {
                    count += 1;
                    cell.hist += remember;
                }
            }
        }
        for &flat in &self.laid[n].vias {
            let cell = &mut self.grids.via[via_kind][flat as usize];
            if cell.soft > 0 {
                count += 1;
                cell.hist += remember;
            }
        }
        count
    }

    /// Where the net's tracks lie too near another's (the net itself taken out of the count
    /// before): how many cells, the box round them in cells, and the layers.
    fn where_stuck(&self, n: usize) -> Option<(u64, [usize; 4], Vec<usize>)> {
        let (mut count, mut b, mut layers) = (0, [usize::MAX, usize::MAX, 0, 0], Vec::new());
        for (path, &kind) in self.laid[n].nodes.iter().zip(&self.laid[n].kinds) {
            for &node in path {
                if self.grids.track[kind][node as usize].soft > 0 {
                    let (l, i, j) = self.grids.split(node);
                    count += 1;
                    b = [b[0].min(i), b[1].min(j), b[2].max(i), b[3].max(j)];
                    if !layers.contains(&l) {
                        layers.push(l);
                    }
                }
            }
        }
        layers.sort_unstable();
        (count > 0).then_some((count, b, layers))
    }

    /// The cells around the net's pads and tracks, `margin` cells wide.
    fn around(&self, n: usize, margin: i32) -> [i32; 4] {
        let mut b = self.jobs[n].boxes.iter().fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |u, b| {
            [u[0].min(b[0]), u[1].min(b[1]), u[2].max(b[2]), u[3].max(b[3])]
        });
        for item in &self.laid[n].items {
            let a = &item.a;
            b = [
                b[0].min(a[0].min(a[2]) as i32),
                b[1].min(a[1].min(a[3]) as i32),
                b[2].max(a[0].max(a[2]) as i32),
                b[3].max(a[1].max(a[3]) as i32),
            ];
        }
        [
            (b[0] - margin).max(0),
            (b[1] - margin).max(0),
            (b[2] + margin).min(self.grids.nx as i32 - 1),
            (b[3] + margin).min(self.grids.ny as i32 - 1),
        ]
    }

    /// Where the net lies differently than before: the cells of the tracks that went or came.
    /// By cells, not by tracks: the same copper may come back as other tracks, cut elsewhere.
    fn changed(&self, old: &Laid, new: &Laid) -> Vec<(i32, i32)> {
        let cells = |laid: &Laid| -> std::collections::HashSet<(u32, usize)> {
            laid.nodes
                .iter()
                .zip(&laid.kinds)
                .flat_map(|(path, &kind)| path.iter().map(move |&c| (c, kind)))
                .collect()
        };
        let (was, is) = (cells(old), cells(new));
        was.symmetric_difference(&is)
            .map(|&(node, _kind)| {
                let (_l, i, j) = self.grids.split(node);
                (i as i32, j as i32)
            })
            .collect()
    }

    /// What the net's tracks cost: their length in cells, a via as `via_cost`, a narrow track by as much more as it is narrower.
    fn cost(&self, n: usize) -> f32 {
        self.cost_of(n, &self.laid[n])
    }

    fn cost_of(&self, n: usize, laid: &Laid) -> f32 {
        let plane = (self.grids.nx * self.grids.ny) as u32;
        let mut total = 0.0;
        for (path, &kind) in laid.nodes.iter().zip(&laid.kinds) {
            // a narrow piece counts for more, by as much as it is narrower (a quarter of the width, four
            // times the resistance): at half as much again for all, 8 mm of a 0.6 mm net stayed at 0.127 mm
            let factor = if kind == self.jobs[n].kind {
                1.0
            } else {
                (self.grids.kinds[self.jobs[n].kind].0 / self.grids.kinds[kind].0).clamp(1.5, 4.0) as f32
            };
            for w in path.windows(2) {
                let apart = w[0].abs_diff(w[1]);
                total += if w[0] / plane != w[1] / plane {
                    self.via_cost
                } else if apart == 1 || apart == self.grids.nx as u32 {
                    factor
                } else {
                    factor * DIAG
                };
            }
        }
        total + self.search.shy_cost * self.beside_of(n, laid)
    }

    /// Lay the net anew: from its first piece, always to the nearest piece not yet joined.
    fn lay(&mut self, n: usize, press: Option<f32>, memory: f32) {
        self.lay_on(n, press, memory, false, None)
    }

    /// Has piece p a way to any other piece of its net (or into its plane), with the net alone on
    /// the board?
    fn reaches(&mut self, n: usize, p: usize) -> bool {
        self.search.shy = None;
        self.own(n, 1);
        self.search.laying += 1;
        let job = &self.jobs[n];
        let sources: Vec<u32> = job.cells[p].iter().map(|c| c.0).collect();
        let (mut marked, mut goals) = (Vec::new(), Vec::new());
        for (q, cells) in job.cells.iter().enumerate().filter(|c| c.0 != p) {
            goals.push(job.boxes[q]);
            for &(node, _s) in cells {
                if self.search.target[node as usize] == 0 {
                    self.search.target[node as usize] = q as u32 + 1;
                    marked.push(node);
                }
            }
        }
        self.search.plane = job.plane;
        self.search.window = self.whole;
        let mut found = false;
        for kind in [Some(job.kind), job.narrow].into_iter().flatten() {
            found = found
                || self
                    .search
                    .run(
                        &self.grids,
                        kind,
                        job.via_kind,
                        &sources,
                        &goals,
                        Some(0.0),
                        0.0,
                        self.via_cost,
                        self.problem.limit,
                    )
                    .is_some();
        }
        self.search.plane = None;
        for node in marked {
            self.search.target[node as usize] = 0;
        }
        self.own(n, -1);
        found
    }

    /// Lay only what must be laid anew: the net's tracks that no other net lies on stay (as far
    /// as they still hang together), and the connections that went with the others are searched
    /// again. A net of forty pads with one branch in another's way was laid again whole, forty
    /// searches for one, and came out different all over each round.
    fn relay(&mut self, n: usize, press: Option<f32>, memory: f32) {
        self.lay_on(n, press, memory, true, None)
    }

    /// Lay again only the net's tracks near what has changed, `fresh`: the others stay as in
    /// `relay`. For the passes that shorten, where a net of forty pads was laid again whole in
    /// each pass because one track somewhere near it had moved.
    fn lay_near(&mut self, n: usize, press: Option<f32>, fresh: &Fresh, since: u32) {
        self.lay_on(n, press, 0.0, true, Some((fresh, since)))
    }

    /// The cells round a track.
    fn bounds(&self, path: &[u32]) -> [i32; 4] {
        let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for &c in path {
            let (_l, i, j) = self.grids.split(c);
            b = [b[0].min(i as i32), b[1].min(j as i32), b[2].max(i as i32), b[3].max(j as i32)];
        }
        b
    }

    /// Is the track no more than a twentieth (and a cell) longer than the shortest there could be
    /// between its two ends, a via counted if they are on different layers?
    fn straight(&self, path: &[u32]) -> bool {
        let plane = (self.grids.nx * self.grids.ny) as u32;
        let mut cost = 0.0f32;
        for w in path.windows(2) {
            cost += if w[0] / plane != w[1] / plane {
                self.via_cost
            } else if w[0].abs_diff(w[1]) == 1 || w[0].abs_diff(w[1]) == self.grids.nx as u32 {
                1.0
            } else {
                DIAG
            };
        }
        let (a, b) = (self.grids.split(path[0]), self.grids.split(*path.last().unwrap()));
        let (dx, dy) = (a.1.abs_diff(b.1) as f32, a.2.abs_diff(b.2) as f32);
        let least = dx.max(dy) + (DIAG - 1.0) * dx.min(dy) + if a.0 != b.0 { self.via_cost } else { 0.0 };
        cost <= least * 1.05 + 1.0
    }

    /// Has anything changed near the net's tracks or pads since `since`?
    fn is_fresh(&self, n: usize, fresh: &Fresh, since: u32) -> bool {
        self.jobs[n].boxes.iter().any(|&b| fresh.hit(b, since)) || self.laid[n].nodes.iter().any(|path| fresh.hit(self.bounds(path), since))
    }

    fn lay_on(&mut self, n: usize, press: Option<f32>, memory: f32, keep: bool, fresh: Option<(&Fresh, u32)>) {
        self.search.shy = self.jobs[n].shy.filter(|_| self.grids.shy());
        self.own(n, 1);
        self.search.laying += 1;
        let before = if keep { std::mem::take(&mut self.laid[n]) } else { Laid::default() };
        let mut laid = Laid::default();
        let mut near: Vec<Item> = Vec::new();
        let apart = (self.problem.via.hole + self.problem.hole_to_hole) / self.grids.step;
        let job = &self.jobs[n];
        let mut origin: HashMap<u32, [usize; 2]> = HashMap::new();
        for (p, cells) in job.cells.iter().enumerate() {
            for &(node, shape) in cells {
                origin.entry(node).or_insert([p, shape]);
            }
        }
        // What stays: the tracks no other net is on, less those that then end on nothing. Pieces
        // and tracks that hang together are a group; `group[p]` is that of piece p, `held[k]` that
        // of the k-th track that stays.
        let pieces = job.cells.len();
        let mut group: Vec<usize> = (0..pieces).collect();
        let mut held: Vec<usize> = Vec::new();
        let mut held_box: Vec<[i32; 4]> = Vec::new();
        if !before.nodes.is_empty() {
            let plane = (self.grids.nx * self.grids.ny) as u32;
            let via_grid = &self.grids.via[job.via_kind];
            let mut stays: Vec<bool> = before
                .nodes
                .iter()
                .zip(&before.kinds)
                .map(|(path, &kind)| {
                    let track = &self.grids.track[kind];
                    let far = fresh.is_none_or(|(fresh, since)| !fresh.hit(self.bounds(path), since));
                    // In the passes that shorten, a track at the net's width that is as good as
                    // straight between its ends, and near nothing it should keep away from, stays
                    // as well: a new search has nothing to win there. (A ground net of 124 pads was near something changed all over the
                    // board in every pass, and its tries took 24 to 36 s each, 56 of the 59 s of a
                    // pass together with one other supply net, mostly to no gain.)
                    let shy = self.search.shy.map(|a| &self.grids.near[a]);
                    let apart = |path: &[u32]| shy.is_none_or(|near| path.iter().all(|&c| near[c as usize].hard == 0));
                    let far = far || (self.direct && fresh.is_some() && kind == job.kind && self.straight(path) && apart(path));
                    far && path.iter().all(|&c| track[c as usize].soft == 0 && track[c as usize].hard == 0)
                        && path
                            .windows(2)
                            .all(|w| w[0] / plane == w[1] / plane || via_grid[(w[0] % plane) as usize].soft == 0)
                })
                .collect();
            let mut on: HashMap<u32, Vec<usize>> = HashMap::new(); // cell -> the tracks that stay on it
            loop {
                on.clear();
                for (k, path) in before.nodes.iter().enumerate() {
                    if stays[k] {
                        for &c in path {
                            on.entry(c).or_default().push(k);
                        }
                    }
                }
                let mut changed = false;
                for (k, path) in before.nodes.iter().enumerate() {
                    let ends = [path[0], *path.last().unwrap()];
                    if stays[k] && ends.iter().any(|c| !origin.contains_key(c) && on[c].iter().all(|&j| j == k)) {
                        stays[k] = false;
                        changed = true;
                    }
                }
                if !changed {
                    break;
                }
            }
            // groups: pieces are 0..pieces, tracks follow
            let mut root: Vec<usize> = (0..pieces + before.nodes.len()).collect();
            fn find(root: &mut [usize], mut a: usize) -> usize {
                while root[a] != a {
                    root[a] = root[root[a]];
                    a = root[a];
                }
                a
            }
            for (k, path) in before.nodes.iter().enumerate() {
                if !stays[k] {
                    continue;
                }
                for c in [path[0], *path.last().unwrap()] {
                    let mut others: Vec<usize> = on[&c].iter().filter(|&&j| j != k).map(|&j| pieces + j).collect();
                    if let Some(at) = origin.get(&c) {
                        others.push(at[0]);
                    }
                    for o in others {
                        let (a, b) = (find(&mut root, pieces + k), find(&mut root, o));
                        root[a.max(b)] = a.min(b);
                    }
                }
            }
            for (p, g) in group.iter_mut().enumerate() {
                *g = find(&mut root, p);
            }
            for (k, path) in before.nodes.iter().enumerate() {
                let g = find(&mut root, pieces + k);
                if !stays[k] || g >= pieces {
                    continue; // (a track that hangs on no piece cannot be, and would be of no use)
                }
                let had = laid.vias.len();
                copper(
                    &self.grids,
                    job,
                    self.grids.kinds[before.kinds[k]].0,
                    &before.paths[k].cells,
                    &mut laid.items,
                    &mut laid.vias,
                );
                for &flat in &laid.vias[had..] {
                    let (x, y) = ((flat as usize % self.grids.nx) as f64, (flat as usize / self.grids.nx) as f64);
                    let hole = Item {
                        rect: false,
                        a: [x, y, x, y],
                        turn: (1.0, 0.0),
                        r: 0.0,
                        layers: vec![],
                        hole: 0.0,
                        clearance: 0.0,
                    };
                    raster(&mut self.grids.via[job.via_kind], false, self.grids.nx, self.grids.ny, 0, &hole, apart, 1);
                    near.push(hole);
                    self.search.own_via[flat as usize] = self.search.laying;
                }
                let mut b = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
                for &c in path {
                    let (_l, i, j) = self.grids.split(c);
                    b = [b[0].min(i as i32), b[1].min(j as i32), b[2].max(i as i32), b[3].max(j as i32)];
                }
                held.push(g);
                held_box.push(b);
                laid.paths.push(before.paths[k].clone());
                laid.kinds.push(before.kinds[k]);
                laid.nodes.push(path.clone());
            }
        }
        // what the new tracks may cost: what those that went did, and the budget
        let mut left = self.budget.map(|b| b + 2.0 + self.cost_of(n, &before) - self.cost_of(n, &laid));
        // (the first `held.len()` tracks of `laid` are those that stayed: the new ones follow)
        let mut joined: Vec<bool> = group.iter().map(|&g| g == group[0]).collect();
        let mut apart_from: Vec<usize> = Vec::new(); // pieces a new group of joined pieces began with
        let cells_of = |g: usize, laid: &Laid| -> Vec<u32> {
            let mut cells: Vec<u32> = Vec::new();
            for (p, own) in job.cells.iter().enumerate() {
                if group[p] == g {
                    cells.extend(own.iter().map(|c| c.0));
                }
            }
            for (path, _) in laid.nodes.iter().zip(&held).filter(|t| *t.1 == g) {
                cells.extend(path.iter().copied());
            }
            cells
        };
        let mut sources: Vec<u32> = cells_of(group[0], &laid);
        // Once a search at the net's width has found no way to anything and had looked at all it
        // could reach, a later one for this net need only start from cells that one did not
        // reach: from the others there is still no way. (`shut`: how many sources there were
        // then, and the window.) A ground net of 124 pads had 21 pins of fine pitch that only a
        // narrow track reaches, and each of them was looked for at full width over the whole
        // board first, 6 million cells, every time the net was laid: 12 s of it.
        let mut shut: Option<(usize, [i32; 4])> = None;
        // The search a turn began with, if the next may go on with it: (its number, how many
        // sources there were, its window). Not after a way with a via: the net's own vias keep
        // others of its vias away, and what the search had reached does not know of the new one.
        let mut going: Option<(u32, usize, [i32; 4])> = None;
        while joined.iter().any(|j| !j) {
            let mut marked = Vec::new();
            let mut goals = Vec::new();
            for (p, cells) in job.cells.iter().enumerate() {
                if !joined[p] {
                    goals.push(job.boxes[p]);
                    for &(node, _s) in cells {
                        if self.search.target[node as usize] == 0 {
                            self.search.target[node as usize] = p as u32 + 1;
                            marked.push(node);
                        }
                    }
                }
            }
            for ((path, &g), &bounds) in laid.nodes.iter().zip(&held).zip(&held_box) {
                // a track that stayed, of a group not yet joined: reaching it is reaching its group
                if !joined[g] {
                    goals.push(bounds);
                    for &node in path {
                        if self.search.target[node as usize] == 0 {
                            self.search.target[node as usize] = g as u32 + 1;
                            marked.push(node);
                        }
                    }
                }
            }
            let mut kind = job.kind;
            self.search.plane = job.plane.filter(|_| !joined[pieces - 1]);
            self.search.bound = left.unwrap_or(f32::INFINITY);
            // (what is left has no cell to be reached at: a piece shut in for good, see `route`)
            let nowhere = marked.is_empty() && self.search.plane.is_none();
            shut = shut.filter(|s| s.1 == self.search.window && left.is_none());
            let fresh_sources: Option<Vec<u32>> = shut.map(|(from, _)| {
                let token = self.search.reached;
                sources[from..].iter().copied().filter(|&c| self.search.reach[c as usize] != token).collect()
            });
            let opened = self.search.expanded;
            let more = going
                .filter(|g| g.0 == self.search.round && g.2 == self.search.window && left.is_none() && fresh_sources.is_none())
                // (only while many pads are left: towards a few, a search begun anew goes straight
                // for the nearest, and going on means sorting again all that was waiting. With
                // more than 12 left, a board of 371 mostly middling nets took 4 % longer over
                // eight seeds; with more than 32, 4 % less, and the boards with a net of a hundred
                // pads and more kept most of what they had won.)
                .filter(|_| goals.len() > 32)
                .map(|g| g.1);
            let mut found = if nowhere || fresh_sources.as_ref().is_some_and(|f| f.is_empty()) {
                None
            } else {
                self.search.wave(
                    &self.grids,
                    kind,
                    job.via_kind,
                    fresh_sources.as_deref().unwrap_or(&sources),
                    more,
                    &goals,
                    press,
                    memory,
                    self.via_cost,
                    self.problem.limit,
                )
            };
            going = (found.is_some() && left.is_none() && fresh_sources.is_none()).then_some((self.search.round, 0, self.search.window));
            if !nowhere && found.is_none() && left.is_none() {
                let searched = fresh_sources.as_ref().is_none_or(|f| !f.is_empty());
                if searched && self.search.spent && (shut.is_some() || self.search.expanded - opened > 100_000) {
                    // what this search reached, within its window, is where no way begins
                    if shut.is_none() {
                        self.search.reached += 1;
                    }
                    let (token, round, w) = (self.search.reached, self.search.round, self.search.window);
                    let (nx, area) = (self.grids.nx, self.grids.nx * self.grids.ny);
                    for l in 0..self.grids.nl {
                        for j in w[1].max(0) as usize..=(w[3] as usize).min(self.grids.ny - 1) {
                            let row = l * area + j * nx;
                            for m in row + w[0].max(0) as usize..=row + (w[2] as usize).min(nx - 1) {
                                if self.search.marks[m].seen == round {
                                    self.search.reach[m] = token;
                                }
                            }
                        }
                    }
                    shut = Some((sources.len(), w));
                } else if searched {
                    shut = None;
                }
            }
            if !nowhere && found.is_none() && self.widen && self.search.window != self.whole {
                // no way near the net: the whole board, for the rest of this net
                self.search.window = self.whole;
                found = self.search.run(
                    &self.grids,
                    kind,
                    job.via_kind,
                    &sources,
                    &goals,
                    press,
                    memory,
                    self.via_cost,
                    self.problem.limit,
                );
            }
            // A way was found near the net, but only across another net, and the rounds are getting
            // nowhere: then one look at the whole board for a way that costs less. (Crossing is
            // always possible near the net, so a search that looks further only when it finds
            // nothing never did: two tracks crossed in open ground for 40 rounds, each with a free
            // way 5 mm outside its bounds.)
            if let (Some(path), true) = (&found, self.far && self.widen && press.is_some() && self.search.window != self.whole) {
                let plane = (self.grids.nx * self.grids.ny) as u32;
                let (track, via) = (&self.grids.track[kind], &self.grids.via[job.via_kind]);
                let crosses = path.iter().any(|&c| track[c as usize].soft > 0)
                    || path.windows(2).any(|w| w[0] / plane != w[1] / plane && via[(w[0] % plane) as usize].soft > 0);
                if crosses {
                    let near = self.search.window;
                    let cost = self.search.marks[*path.last().unwrap() as usize].g;
                    self.search.window = self.whole;
                    self.search.bound = cost - 0.5;
                    let wider = self.search.run(
                        &self.grids,
                        kind,
                        job.via_kind,
                        &sources,
                        &goals,
                        press,
                        memory,
                        self.via_cost,
                        self.problem.limit,
                    );
                    match wider {
                        Some(way) => found = Some(way),
                        None => {
                            // nothing cheaper anywhere: the way near the net it is (searched again: the marks are gone)
                            self.search.window = near;
                            self.search.bound = left.unwrap_or(f32::INFINITY);
                            found = self.search.run(
                                &self.grids,
                                kind,
                                job.via_kind,
                                &sources,
                                &goals,
                                press,
                                memory,
                                self.via_cost,
                                self.problem.limit,
                            );
                        }
                    }
                }
            }
            // The way found is at the net's width and across another net, and the rounds are about to
            // stall: a narrow track may get by where the full width cannot. (Not sooner: tried from
            // the second and from the sixth round without progress, it left twice as much narrow
            // track on a board of 159 nets, 24 placements, as none at all.) (Two supply pins side
            // by side at 0.5 mm pitch, each with a 0.4 mm track to its capacitor below: 0.1 mm
            // between the two tracks where 0.15 mm is asked. A narrow track was only laid where the
            // full width found no way at all, and across the other net there always is one.)
            if let (Some(path), Some(narrow), true) = (&found, job.narrow, self.pinch && press.is_some()) {
                let plane = (self.grids.nx * self.grids.ny) as u32;
                let (track, via) = (&self.grids.track[kind], &self.grids.via[job.via_kind]);
                let crosses = path.iter().any(|&c| track[c as usize].soft > 0)
                    || path.windows(2).any(|w| w[0] / plane != w[1] / plane && via[(w[0] % plane) as usize].soft > 0);
                if crosses {
                    let cost = self.search.marks[*path.last().unwrap() as usize].g;
                    let worth = (self.grids.kinds[job.kind].0 / self.grids.kinds[narrow].0).clamp(1.5, 4.0) as f32;
                    self.search.bound = cost / worth;
                    let thin = self.search.run(
                        &self.grids,
                        narrow,
                        job.via_kind,
                        &sources,
                        &goals,
                        press,
                        memory,
                        self.via_cost,
                        self.problem.limit,
                    );
                    self.search.bound = left.unwrap_or(f32::INFINITY);
                    if let Some(way) = thin {
                        (kind, found) = (narrow, Some(way));
                    }
                }
            }
            if let (None, Some(narrow), false) = (&found, job.narrow, nowhere) {
                kind = narrow;
                found = self.search.run(
                    &self.grids,
                    kind,
                    job.via_kind,
                    &sources,
                    &goals,
                    press,
                    memory,
                    self.via_cost,
                    self.problem.limit,
                );
            }
            self.search.bound = f32::INFINITY;
            if let (Some(left), Some(path)) = (left.as_mut(), &found) {
                *left -= self.search.marks[*path.last().unwrap() as usize].g;
            }
            // (a way that ends on no marked cell has ended in the net's plane, its last piece)
            let mut piece = found.as_ref().map(|path| match self.search.target[*path.last().unwrap() as usize] {
                0 => pieces - 1,
                p => p as usize - 1,
            });
            self.search.plane = None;
            // A narrow track only as long as it must be: it was laid because a pin had no way out at the
            // net's width, and then ran narrow all the way (7 mm of 0.127 mm to a 100 uF capacitor). From
            // the first cell of it where the full width has room to the last such cell, at full width
            // where that has a way that is not too dear; what is left narrow is the piece at the pin.
            let mut parts: Vec<(Vec<u32>, usize)> = Vec::new();
            if let (true, Some(thin)) = (kind != job.kind, &found) {
                let wide = &self.grids.track[job.kind];
                let fits = |c: u32| wide[c as usize].hard == 0 && (press.is_some() || wide[c as usize].soft == 0);
                let first = thin.iter().position(|&c| fits(c));
                let last = thin.iter().rposition(|&c| fits(c));
                if let (Some(i), Some(j)) = (first, last) {
                    if i < j {
                        let end = thin[j];
                        // what the narrow piece in between is worth: a track a quarter as wide has four
                        // times the resistance, so the full width may be up to that much dearer
                        let worth = (self.grids.kinds[job.kind].0 / self.grids.kinds[kind].0).clamp(1.5, 4.0) as f32;
                        let most = worth * (self.search.marks[end as usize].g - self.search.marks[thin[i] as usize].g);
                        let own = self.search.target[end as usize] == 0;
                        if own {
                            self.search.target[end as usize] = u32::MAX;
                        }
                        let (_l, a, c) = self.grids.split(end);
                        let mut to = goals.clone();
                        to.push([a as i32, c as i32, a as i32, c as i32]);
                        let mut starts = sources.clone();
                        starts.push(thin[i]);
                        let way = self.search.run(
                            &self.grids,
                            job.kind,
                            job.via_kind,
                            &starts,
                            &to,
                            press,
                            memory,
                            self.via_cost,
                            self.problem.limit,
                        );
                        if own {
                            self.search.target[end as usize] = 0;
                        }
                        let way = way.filter(|w| self.search.marks[*w.last().unwrap() as usize].g <= most);
                        if let Some(way) = way {
                            let at = self.search.target[*way.last().unwrap() as usize];
                            if at != 0 {
                                piece = Some(at as usize - 1); // the full width found its own way to a piece
                            }
                            if way[0] == thin[i] && i > 0 {
                                parts.push((thin[..=i].to_vec(), kind));
                            }
                            let reaches = *way.last().unwrap() == end && j + 1 < thin.len();
                            parts.push((way, job.kind));
                            if reaches {
                                parts.push((thin[j..].to_vec(), kind));
                            }
                        }
                    }
                }
                // No such way (a second narrow place further on, or the way round is too dear): then
                // the narrow track's own way, at full width wherever that fits for 0.8 mm or more, and
                // narrow only through the places that are too tight. A gate drive ran 8 mm at
                // 0.127 mm for two such places 0.5 mm long.
                if parts.is_empty() {
                    let (nx, plane) = (self.grids.nx as u32, (self.grids.nx * self.grids.ny) as u32);
                    let open = |c: u32| wide[c as usize].hard == 0;
                    let step_fits = |a: u32, b: u32| {
                        let (lo, hi) = (a.min(b), a.max(b));
                        a / plane != b / plane
                            || match hi - lo {
                                d if d == nx + 1 => open(lo + 1) && open(lo + nx),
                                d if d == nx - 1 => open(lo - 1) && open(lo + nx),
                                _ => true,
                            }
                    };
                    let least = (0.8 / self.grids.step) as usize;
                    let mut runs: Vec<(usize, usize)> = Vec::new(); // stretches of the way that fit, first and last cell
                                                                    // (at full width only where no other net is in the way now, whatever crossing costs)
                    let fits = |c: u32| wide[c as usize].hard == 0 && wide[c as usize].soft == 0;
                    let mut a = 0;
                    while a < thin.len() {
                        if !fits(thin[a]) {
                            a += 1;
                            continue;
                        }
                        let mut b = a;
                        while b + 1 < thin.len() && fits(thin[b + 1]) && step_fits(thin[b], thin[b + 1]) {
                            b += 1;
                        }
                        if b - a >= least {
                            runs.push((a, b));
                        }
                        a = b + 1;
                    }
                    let mut at = 0;
                    for &(a, b) in &runs {
                        if a > at {
                            parts.push((thin[at..=a].to_vec(), kind));
                        }
                        parts.push((thin[a..=b].to_vec(), job.kind));
                        at = b;
                    }
                    if !runs.is_empty() && at + 1 < thin.len() {
                        parts.push((thin[at..].to_vec(), kind));
                    }
                }
            }
            for node in marked {
                self.search.target[node as usize] = 0;
            }
            let Some(path) = found else {
                laid.stuck = press.is_some();
                // What is joined so far has no way on. The pieces left may still have ways to each
                // other: go on from the next of them, so that one pad shut in costs one connection
                // and not all of the net's (a net of six pieces came out with five apart).
                let left: Vec<usize> = (0..joined.len()).filter(|&p| !joined[p]).collect();
                apart_from.push(left[0]);
                let next = group[left[0]];
                (0..pieces).filter(|&p| group[p] == next).for_each(|p| joined[p] = true);
                if left.iter().all(|&p| group[p] == next) {
                    break;
                }
                sources = cells_of(next, &laid);
                shut = None;
                continue;
            };
            let piece = piece.unwrap();
            let next = group[piece];
            (0..pieces).filter(|&p| group[p] == next).for_each(|p| joined[p] = true);
            if parts.is_empty() {
                parts.push((path, kind));
            }
            let (had_sources, had_vias) = (sources.len(), laid.vias.len());
            sources.extend(cells_of(next, &laid));
            for (path, kind) in parts {
                sources.extend(path.iter().copied());
                let bends = corners(&self.grids, &path);
                let had = laid.vias.len();
                copper(&self.grids, job, self.grids.kinds[kind].0, &bends, &mut laid.items, &mut laid.vias);
                for &flat in &laid.vias[had..] {
                    // the net's next via: on this one or a hole's distance away
                    let (x, y) = ((flat as usize % self.grids.nx) as f64, (flat as usize / self.grids.nx) as f64);
                    let hole = Item {
                        rect: false,
                        a: [x, y, x, y],
                        turn: (1.0, 0.0),
                        r: 0.0,
                        layers: vec![],
                        hole: 0.0,
                        clearance: 0.0,
                    };
                    raster(&mut self.grids.via[job.via_kind], false, self.grids.nx, self.grids.ny, 0, &hole, apart, 1);
                    near.push(hole);
                    self.search.own_via[flat as usize] = self.search.laying;
                }
                laid.paths.push(Path {
                    cells: bends,
                    width: self.grids.kinds[kind].0 * 2.0,
                    from: origin.get(&path[0]).copied(),
                    to: origin.get(path.last().unwrap()).copied(),
                });
                laid.kinds.push(kind);
                laid.nodes.push(path);
            }
            going = going.filter(|_| laid.vias.len() == had_vias).map(|g| (g.0, had_sources, g.2));
        }
        for hole in &near {
            raster(
                &mut self.grids.via[self.jobs[n].via_kind],
                false,
                self.grids.nx,
                self.grids.ny,
                0,
                hole,
                apart,
                -1,
            );
        }
        laid.open = apart_from;
        self.laid[n] = laid;
        self.own(n, -1);
    }
}

/// Route the problem. `say` gets a line per round.
pub fn route(problem: &Problem, say: &mut dyn FnMut(String)) -> Solution {
    let started = std::time::Instant::now();
    let step = problem.step;
    let [x0, y0, x1, y1] = problem.area;
    let nx = ((x1 - x0) / step).round() as usize + 1;
    let ny = ((y1 - y0) / step).round() as usize + 1;
    let nl = problem.layers;
    let mut kinds: Vec<(f64, f64)> = Vec::new();
    let mut via_kinds: Vec<f64> = Vec::new();
    for net in &problem.nets {
        for width in [Some(net.width), net.narrow].into_iter().flatten() {
            let kind = (width / 2.0, net.clearance);
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        if !via_kinds.contains(&net.clearance) {
            via_kinds.push(net.clearance);
        }
    }
    let mut grids = Grids {
        nx,
        ny,
        nl,
        step,
        via_r: problem.via.d / 2.0,
        via_hole: problem.via.hole,
        hole_clear: problem.hole_clear,
        track: vec![vec![Cell::default(); nx * ny * nl]; kinds.len()],
        via: vec![vec![Cell::default(); nx * ny]; via_kinds.len()],
        kinds,
        via_kinds,
        apart: problem.apart,
        near: if problem.apart > 0.0 || !problem.rooms.is_empty() {
            [
                vec![Cell::default(); nx * ny * nl],
                vec![Cell::default(); nx * ny * nl],
                vec![Cell::default(); nx * ny * nl],
            ]
        } else {
            [Vec::new(), Vec::new(), Vec::new()]
        },
        planes: Vec::new(),
    };
    // the board's edge and the keep-out areas: no track centre within half a track of them
    for k in 0..grids.kinds.len() + grids.via_kinds.len() {
        let via = k >= grids.kinds.len();
        let half = if via { grids.via_r } else { grids.kinds[k].0 };
        let layers = if via { 1 } else { nl };
        let grid = if via { &mut grids.via[k - grids.kinds.len()] } else { &mut grids.track[k] };
        if let Some(e) = problem.edge {
            let inset = problem.edge_clear + half;
            for l in 0..layers {
                for j in 0..ny {
                    for i in 0..nx {
                        let (x, y) = (x0 + i as f64 * step, y0 + j as f64 * step);
                        if x < e[0] + inset - 1e-9 || x > e[2] - inset + 1e-9 || y < e[1] + inset - 1e-9 || y > e[3] - inset + 1e-9 {
                            grid[(l * ny + j) * nx + i].hard += 1;
                        }
                    }
                }
            }
        }
        for keepout in &problem.keepouts {
            if (via && !keepout.vias) || (!via && !keepout.tracks) {
                continue;
            }
            let b = keepout.area;
            let item = Item {
                rect: true,
                a: [(b[0] - x0) / step, (b[1] - y0) / step, (b[2] - x0) / step, (b[3] - y0) / step],
                turn: (1.0, 0.0),
                r: 0.0,
                layers: vec![],
                hole: 0.0,
                clearance: 0.0,
            };
            if via {
                if !keepout.layers.is_empty() {
                    raster(grid, false, nx, ny, 0, &item, half / step, 1);
                }
            } else {
                for &l in &keepout.layers {
                    raster(grid, false, nx, ny, l, &item, half / step, 1);
                }
            }
        }
    }
    let which = |role: Role| match role {
        Role::Noisy => Some(0),
        Role::Quiet => Some(1),
        Role::None => None,
    };
    for shape in &problem.fixed {
        let item = item_of(shape, x0, y0, step, shape.clearance);
        grids.stamp(&item, false, true, 1);
        if let Some(w) = which(shape.role) {
            grids.beside(&item, w, 1);
        }
    }
    for shape in &problem.rooms {
        let item = item_of(shape, x0, y0, step, 0.0);
        grids.room(&item, shape.room);
    }
    let mut jobs = Vec::new();
    let mut plane_of: Vec<(&Vec<Vec<[f64; 2]>>, usize)> = Vec::new(); // the planes made so far: nets may share one
    for net in &problem.nets {
        let kind = grids.kinds.iter().position(|k| *k == (net.width / 2.0, net.clearance)).unwrap();
        let via_kind = grids.via_kinds.iter().position(|c| *c == net.clearance).unwrap();
        let narrow = net
            .narrow
            .filter(|w| *w < net.width)
            .map(|w| grids.kinds.iter().position(|k| *k == (w / 2.0, net.clearance)).unwrap());
        let mut job = Job {
            kind,
            narrow,
            via_kind,
            clearance: net.clearance,
            items: Vec::new(),
            cells: Vec::new(),
            boxes: Vec::new(),
            shy: Some(which(net.role).map_or(2, |w| 1 - w)),
            plane: None,
        };
        for piece in &net.pieces {
            let items: Vec<Item> = piece.iter().map(|s| item_of(s, x0, y0, step, net.clearance)).collect();
            let mut cells = Vec::new();
            let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
            for (s, item) in items.iter().enumerate() {
                grids.stamp(item, false, true, 1);
                if let Some(w) = which(net.role) {
                    grids.beside(item, w, 1);
                }
                for node in cells_of(&grids, item) {
                    let (_l, i, j) = grids.split(node);
                    bounds = [
                        bounds[0].min(i as i32),
                        bounds[1].min(j as i32),
                        bounds[2].max(i as i32),
                        bounds[3].max(j as i32),
                    ];
                    cells.push((node, s));
                }
            }
            job.items.push(items);
            job.cells.push(cells);
            job.boxes.push(bounds);
        }
        if !net.plane.is_empty() {
            // the plane as cells, row by row: between the crossings of a row with the polygons' sides
            let same = plane_of.iter().position(|(polygons, _)| *polygons == &net.plane);
            let at = same.map(|k| plane_of[k].1).unwrap_or_else(|| {
                let mut mask = vec![false; nx * ny];
                for polygon in &net.plane {
                    for j in 0..ny {
                        let y = y0 + j as f64 * step;
                        let mut cuts: Vec<f64> = Vec::new();
                        for (k, a) in polygon.iter().enumerate() {
                            let b = polygon[(k + 1) % polygon.len()];
                            if (a[1] > y) != (b[1] > y) {
                                cuts.push(a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                            }
                        }
                        cuts.sort_by(|a, b| a.total_cmp(b));
                        for pair in cuts.windows(2).step_by(2) {
                            let from = (((pair[0] - x0) / step).ceil().max(0.0)) as usize;
                            let to = (((pair[1] - x0) / step).floor()).min(nx as f64 - 1.0);
                            if to >= 0.0 {
                                for cell in mask[j * nx..j * nx + nx].iter_mut().take(to as usize + 1).skip(from) {
                                    *cell = true;
                                }
                            }
                        }
                    }
                }
                grids.planes.push(mask);
                plane_of.push((&net.plane, grids.planes.len() - 1));
                grids.planes.len() - 1
            });
            let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
            for (flat, _) in grids.planes[at].iter().enumerate().filter(|c| *c.1) {
                let (i, j) = ((flat % nx) as i32, (flat / nx) as i32);
                bounds = [bounds[0].min(i), bounds[1].min(j), bounds[2].max(i), bounds[3].max(j)];
            }
            if bounds[0] <= bounds[2] {
                job.plane = Some(at);
                job.items.push(Vec::new());
                job.cells.push(Vec::new());
                job.boxes.push(bounds);
            }
        }
        jobs.push(job);
    }
    // small nets first: the same order in every round, which is all that matters
    let mut order: Vec<usize> = (0..jobs.len()).filter(|&n| jobs[n].cells.len() > 1).collect();
    let reach = |n: usize| -> i32 {
        let u = jobs[n].boxes.iter().fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |u, b| {
            [u[0].min(b[0]), u[1].min(b[1]), u[2].max(b[2]), u[3].max(b[3])]
        });
        (u[2] - u[0]).max(0) + (u[3] - u[1]).max(0)
    };
    order.sort_by_key(|&n| (reach(n), n));
    if problem.seed > 0 {
        let mix = |n: usize| -> u64 {
            let mut x = (n as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ problem.seed.wrapping_mul(0xD1B5_4A32_D192_ED03);
            x = (x ^ (x >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x ^ (x >> 29)
        };
        order.sort_by_key(|&n| (reach(n) as u64 * (768 + mix(n) % 513) / 1024, n));
    }
    let nodes = nx * ny * nl;
    let mut router = Router {
        problem,
        laid: vec![Laid::default(); jobs.len()],
        jobs,
        search: Search {
            marks: vec![Mark { g: 0.0, prev: NONE, seen: 0 }; nodes],
            queue: Queue::new(),
            round: 0,
            target: vec![0; nodes],
            own_via: vec![0; nx * ny],
            laying: 0,
            window: [0, 0, nx as i32 - 1, ny as i32 - 1],
            expanded: 0,
            shy: None,
            shy_cost: problem.shy as f32,
            far: vec![0.0; (nx / FAR as usize + 1) * (ny / FAR as usize + 1)],
            far_seen: vec![0; (nx / FAR as usize + 1) * (ny / FAR as usize + 1)],
            far_round: 0,
            bound: f32::INFINITY,
            spent: false,
            reach: vec![0; nodes],
            reached: 0,
            plane: None,
        },
        via_cost: (problem.via.cost / step) as f32,
        whole: [0, 0, nx as i32 - 1, ny as i32 - 1],
        widen: true,
        direct: false,
        budget: None,
        far: false,
        pinch: false,
        grids,
    };
    let mut solution = Solution::default();
    let near = (4.0 / step) as i32;
    if problem.alone {
        for &n in &order {
            router.search.window = router.around(n, near);
            router.lay(n, Some(0.0), 0.0); // nothing is counted into the grids: the next net does not see it
        }
        for (n, net) in problem.nets.iter().enumerate() {
            let cost = router.cost(n) as f64 * step;
            solution.nets.push(Routed {
                name: net.name.clone(),
                paths: Vec::new(),
                open: router.laid[n].open.clone(),
                cost,
                alone: cost,
                beside: 0.0,
            });
        }
        solution.expanded = router.search.expanded;
        return solution;
    }
    // A piece that has no way to the rest of its net even with the net alone on the board has
    // none with the others on it. Left in, it is laid across another net round after round,
    // since that is a way, and the rounds never come clean: a board of 479 connections spent
    // 25 minutes on one ground pad and four cells. So it is found before the rounds and left
    // out of them: its cells are no longer anything to reach, and it is reported open.
    for &n in &order {
        router.search.window = router.around(n, near);
        router.lay(n, Some(0.0), 0.0);
        let apart = !router.laid[n].open.is_empty();
        router.laid[n] = Laid::default();
        for p in (0..router.jobs[n].cells.len()).filter(|_| apart) {
            if !router.jobs[n].cells[p].is_empty() && !router.reaches(n, p) {
                router.jobs[n].cells[p].clear();
                say(format!(
                    "{}: a piece of it has no way to the rest with the net alone on the board, and is left open",
                    problem.nets[n].name
                ));
            }
        }
    }
    // Weights play no part in these rounds. Three ways to let them were measured on a board with
    // 158 nets, 13 of them weighted (260 mm with only those on the board, 315 mm without weights):
    // heavy nets feel congestion less (one came out six times longer, rounds doubled), heavy nets
    // laid first and slow to give way (308 mm), heavy nets laid first and held (320 mm, rounds
    // three times as long). None is worth its code; weights act in the shortening below.
    // Tracks given to start from (`Net::laid`): they lie as they were given, and the first round
    // lays each such net as every later round does, keeping what is free and joining the rest.
    let mut warm = vec![false; router.jobs.len()];
    for (n, net) in problem.nets.iter().enumerate() {
        let job = &router.jobs[n];
        let mut laid = Laid::default();
        for path in &net.laid {
            let kind = [Some(job.kind), job.narrow]
                .into_iter()
                .flatten()
                .find(|&k| (router.grids.kinds[k].0 * 2.0 - path.width).abs() < 1e-6);
            let (Some(kind), Some(nodes)) = (kind, expand(&router.grids, &path.cells)) else {
                continue;
            };
            let bends = corners(&router.grids, &nodes);
            copper(&router.grids, job, router.grids.kinds[kind].0, &bends, &mut laid.items, &mut laid.vias);
            laid.paths.push(Path {
                cells: bends,
                width: path.width,
                from: None,
                to: None,
            });
            laid.kinds.push(kind);
            laid.nodes.push(nodes);
        }
        if !laid.nodes.is_empty() {
            router.laid[n] = laid;
            router.soft(n, 1);
            warm[n] = true;
        }
    }
    let mut press = 0.5f32;
    let mut clean = false;
    let (mut fewest, mut since, mut restarts) = (u64::MAX, 0u32, 0u32);
    // Where nets have gone on lying on each other: see below.
    let knot_tile = 16;
    let new_knot = |grids: &Grids| Fresh {
        stamps: vec![0; (grids.nx / knot_tile as usize + 1) * (grids.ny / knot_tile as usize + 1)],
        nx: grids.nx as i32 / knot_tile + 1,
        ny: grids.ny as i32 / knot_tile + 1,
        tile: knot_tile,
        now: 0,
    };
    let mut knot = new_knot(&router.grids);
    for round in 0..problem.max_iter {
        let (mut found, mut again) = (0u64, 0usize);
        // Two nets that lie on each other and are each laid again only where the other is on them
        // can go on so for ever: the way out for the one is through a track of the other that
        // nobody lies on, and that stays. (A board of 1458 connections: two crossings of five
        // cells from round 20 on, each round a search of the whole board for a way that is not
        // there, 17 s a round.) So when the conflicts have not got fewer for four rounds, the nets
        // in conflict give up all their tracks within 2 mm of where they are stuck, then within
        // 4 mm, and so on; after twelve rounds every net with a track there does.
        if since == 0 && knot.now > 0 {
            knot = new_knot(&router.grids);
        }
        router.far = since >= 2;
        router.pinch = since + 4 >= problem.stall;
        let wide = (problem.relay && round > 0 && since >= 4).then(|| ((2.0 * 2f64.powi(((since - 4) / 4) as i32)).min(32.0) / step) as i32);
        for &n in &order {
            if round > 0 {
                router.soft(n, -1);
                let c = router.conflicts(n, 0.4);
                let retry = !router.laid[n].open.is_empty() && !router.laid[n].stuck;
                let beside = c == 0 && since >= 12 && wide.is_some() && router.laid[n].nodes.iter().any(|path| knot.hit(router.bounds(path), 0));
                if c == 0 && !retry && !beside {
                    router.soft(n, 1);
                    continue;
                }
                found += c;
                if let (Some(reach), Some((_cells, b, _layers))) = (wide, router.where_stuck(n)) {
                    let b = b.map(|v| v as i32);
                    knot.note([(b[0], b[1]), (b[2], b[1]), (b[0], b[3]), (b[2], b[3])].into_iter(), reach);
                }
            } else if warm[n] {
                router.soft(n, -1);
            }
            // a search stays within 4 mm of the net's pads and tracks, unless there is no way there:
            // without this bound the supply board's 158 nets took 146 s, nearly all of it searching
            router.search.window = router.around(n, near);
            // Only what is in another's way, until the rounds stall for the first time; from then on
            // whole nets. The first is two to three times as fast on boards that come clean; a board
            // that stalls is helped by nets that take wholly new ways (one of ten boards came clean
            // only so, another kept one connection open instead of two).
            // (Not a net of many pads: laid again whole in every round, a ground net of some hundred
            // pads took a board from 14 minutes to more than 25 over two crossings of five cells.)
            // A net with a weight pays that much less for a cell another lies on: it keeps to its
            // short way longer, and the others, at the full price, are the ones to give way. (As
            // routers for programmable logic weigh a net's delay against congestion by how
            // critical it is. The weight counted only in the passes that shorten, after the rounds
            // had settled who goes round whom.) What a cell that was fought over costs stays the
            // same for all: with that divided as well, one of 24 placements of a board kept a
            // connection open; the rounds end because that price rises for everyone.
            let heavy = problem.nets[n].weight.max(0.1) as f32;
            let (press, memory) = (press / heavy, 1.0);
            if wide.is_some() && knot.now > 0 {
                router.lay_on(n, Some(press), memory, true, Some((&knot, 0)));
            } else if problem.relay && (restarts == 0 || router.jobs[n].cells.len() > 24) {
                router.relay(n, Some(press), memory);
            } else {
                router.lay(n, Some(press), memory);
            }
            router.soft(n, 1);
            again += 1;
        }
        solution.iterations = round + 1;
        if round > 0 {
            solution.conflicts.push(found);
            say(format!("round {}: {} nets laid again, {} cells in conflict", round, again, found));
            if again == 0 {
                clean = true;
                break;
            }
            // A few nets that need the same cells go on changing places for as long as they are
            // asked to (seen on a board of 158 nets: 2 nets, 18 cells, from round 30 to round 200).
            // When the conflicts have not got fewer for `stall` rounds, the price for crossing
            // starts low again: the stuck nets may then lie across others, those are laid again in
            // turn, and what the cells have cost so far is remembered. Of three such boards one
            // came clean in 61 rounds instead of 187, one in 155 instead of 200, one not at all.
            // After three such starts the rounds end.
            if found < fewest {
                (fewest, since) = (found, 0);
            } else {
                since += 1;
            }
            if since >= problem.stall {
                say(format!(
                    "round {}: the conflicts have not got fewer than {} cells for {} rounds",
                    round, fewest, since
                ));
                // which nets, and where: this is where the board is too full
                for &n in &order {
                    router.soft(n, -1);
                    if let Some((cells, b, layers)) = router.where_stuck(n) {
                        say(format!(
                            "   {} is in another's way on {} cells, x {:.2} to {:.2}, y {:.2} to {:.2}, layer {}",
                            problem.nets[n].name,
                            cells,
                            x0 + b[0] as f64 * step,
                            x0 + b[2] as f64 * step,
                            y0 + b[1] as f64 * step,
                            y0 + b[3] as f64 * step,
                            layers.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(" and ")
                        ));
                    }
                    router.soft(n, 1);
                }
                if problem.untangle {
                    let (knots, settled) = router.untangle(&order, near, restarts == 0, false);
                    say(format!("   {} of {} knots settled by an order of their nets", settled, knots));
                    if settled == knots {
                        (fewest, since) = (u64::MAX, 0);
                        continue; // the next round finds nothing in conflict, or goes on from here
                    }
                }
                // (Ending the rounds here when only a dozen cells are left, to settle them by leaving a
                // connection open, was tried: 3 of 24 placements of a board then kept a connection open
                // that starting over had always closed.)
                if restarts >= 3 {
                    break;
                }
                restarts += 1;
                press = 0.5 / 1.6;
                (fewest, since) = (u64::MAX, 0);
            }
        } else {
            say(format!("round 0: {} nets laid", again));
        }
        press = (press * 1.6).min(1e5);
    }
    if problem.glance {
        for &n in &order {
            router.soft(n, -1);
            if let Some((cells, b, layers)) = router.where_stuck(n) {
                solution.stuck.push(Stuck {
                    name: problem.nets[n].name.clone(),
                    cells,
                    area: [
                        x0 + b[0] as f64 * step,
                        y0 + b[1] as f64 * step,
                        x0 + b[2] as f64 * step,
                        y0 + b[3] as f64 * step,
                    ],
                    layers,
                });
            }
            router.soft(n, 1);
        }
        // and where they were in each other's way in any round: what each cell has been remembered for
        let tile = (1.0 / step).round().max(1.0) as usize;
        let (nx, ny, nl) = (router.grids.nx, router.grids.ny, router.grids.nl);
        let (tx, ty) = (nx / tile + 1, ny / tile + 1);
        let mut heat = vec![0.0f64; nl * tx * ty];
        for grid in &router.grids.track {
            for (node, cell) in grid.iter().enumerate().filter(|c| c.1.hist > 0.0) {
                let (l, i, j) = router.grids.split(node as u32);
                heat[(l * ty + j / tile) * tx + i / tile] += cell.hist as f64;
            }
        }
        for grid in &router.grids.via {
            for (flat, cell) in grid.iter().enumerate().filter(|c| c.1.hist > 0.0) {
                for l in 0..nl {
                    heat[(l * ty + flat / nx / tile) * tx + flat % nx / tile] += cell.hist as f64;
                }
            }
        }
        for (k, &h) in heat.iter().enumerate().filter(|h| *h.1 > 0.0) {
            solution.heat.push(Heat {
                x: x0 + (k % tx * tile) as f64 * step,
                y: y0 + (k / tx % ty * tile) as f64 * step,
                layer: k / tx / ty,
                heat: h,
            });
        }
        solution.expanded = router.search.expanded;
        return solution;
    }
    if !clean {
        // out of rounds: take out the net with the most conflicts until none is left, then give
        // each of those the room that remains
        if problem.untangle {
            let (knots, settled) = router.untangle(&order, near, true, true);
            say(format!("out of rounds: {} of {} knots settled by an order of their nets", settled, knots));
        }
        let mut out: Vec<usize> = Vec::new();
        loop {
            let mut worst = (0u64, 0usize);
            for &n in &order {
                router.soft(n, -1);
                let c = router.conflicts(n, 0.0);
                router.soft(n, 1);
                if c > worst.0 {
                    worst = (c, n);
                }
            }
            if worst.0 == 0 {
                break;
            }
            router.soft(worst.1, -1);
            router.laid[worst.1] = Laid::default();
            out.push(worst.1);
        }
        out.sort_by_key(|&n| order.iter().position(|&o| o == n));
        for &n in &out {
            router.search.window = router.whole;
            router.lay(n, None, 0.0);
            router.soft(n, 1);
        }
        say(format!(
            "no clean result in {} rounds: {} nets laid again in the room left",
            solution.iterations,
            out.len()
        ));
    }
    // Shorten what the rounds left long. A net is laid again, across other nets where that is
    // shorter; those it crosses are laid again around it. The exchange is kept if the weighted
    // sum of their costs got smaller and none of them lost a connection. So every step makes the
    // result better, and a net with a larger weight wins a short way from those with a smaller.
    let weight = |n: usize| problem.nets[n].weight as f32;
    let clock = std::time::Instant::now();
    let margin = (3.0 / step) as i32; // a way round may leave the net's own area by 3 mm
    let reach = (1.5 / step) as i32; // how far a track is in the way of another: widths, clearances, the distance kept from noisy copper
    let whole = router.whole;
    router.widen = false;
    router.direct = true;
    // A net is tried again only if something near it has changed, and then only its tracks near that.
    let tile = 32;
    let (tnx, tny) = (router.grids.nx as i32 / tile + 1, router.grids.ny as i32 / tile + 1);
    let mut fresh = Fresh {
        stamps: vec![1; (tnx * tny) as usize],
        nx: tnx,
        ny: tny,
        tile,
        now: 1,
    };
    let mut tried = vec![0u32; router.jobs.len()]; // the count of changes when the net was last tried
    say(format!("all nets laid after {:.0} s", started.elapsed().as_secs_f64()));
    let mut ideal = vec![0.0f32; router.jobs.len()];
    let mut alone = vec![0.0f32; router.jobs.len()]; // the same without what nearness costs: lengths, to say
    for &n in &order {
        // what the net would cost alone on the board: no result is better than that
        let old = router.laid[n].clone();
        router.soft(n, -1);
        router.search.window = router.around(n, margin);
        router.lay(n, Some(0.0), 0.0);
        ideal[n] = if router.laid[n].open.is_empty() { router.cost(n) } else { 0.0 };
        alone[n] = (ideal[n] - router.search.shy_cost * router.beside_of(n, &router.laid[n])).max(0.0);
        router.laid[n] = old;
        router.soft(n, 1);
    }
    say(format!("each net alone: {:.0} s", clock.elapsed().as_secs_f64()));
    for pass in 0..problem.relax {
        let mut todo: Vec<(f32, usize)> = order
            .iter()
            .map(|&n| {
                (
                    if router.laid[n].open.is_empty() {
                        weight(n) * (router.cost(n) - ideal[n])
                    } else {
                        f32::MAX
                    },
                    n,
                )
            })
            .filter(|t| t.0 >= 4.0 && router.is_fresh(t.1, &fresh, tried[t.1]))
            .collect();
        todo.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let (mut better, mut pushed, mut saved) = (0u32, 0u32, 0.0f32);
        for &(_, n) in &todo {
            // n takes its way across others, and those it crosses take what is then free. (Letting
            // them push in turn, two levels deep, took four times as long on a board of 158 nets and
            // saved no more.)
            let mut moved: Vec<(usize, Laid)> = Vec::new();
            let mut futile = false;
            let mut gain = 0.0f32; // what n has won, weighted, less what those it pushed have lost
            let mut queue: std::collections::VecDeque<(usize, u32)> = [(n, 0)].into();
            while let Some((m, depth)) = queue.pop_front() {
                if moved.iter().any(|t| t.0 == m) {
                    continue;
                }
                let old = router.laid[m].clone();
                router.soft(m, -1);
                router.search.window = router.around(m, margin);
                let mut crossed: Vec<usize> = Vec::new();
                if depth < 1 {
                    let press = Some(1.5 / weight(m).max(0.1)); // a heavier net crosses others more readily
                                                                // (A net of many pads also the first time: all of it counts as changed then, and
                                                                // what is straight stays.)
                    if problem.relay && (tried[m] > 0 || router.jobs[m].cells.len() > 24) {
                        router.lay_near(m, press, &fresh, tried[m]);
                    } else {
                        router.lay(m, press, 0.0);
                    }
                    // Even across others it is no shorter than it lies and joins no more: then there is
                    // nothing to be had, by pushing or without. (Most tries end here: 63 of 67 in the
                    // third pass on a board of 158 nets, each of which went on to two more searches.)
                    if router.laid[m].open.len() >= old.open.len() && router.cost(m) >= router.cost_of(m, &old) - 0.5 {
                        router.laid[m] = old;
                        router.soft(m, 1);
                        futile = true;
                        break;
                    }
                    gain = if router.laid[m].open.len() < old.open.len() {
                        f32::INFINITY // a connection more is worth any length
                    } else {
                        weight(m) * (router.cost_of(m, &old) - router.cost(m))
                    };
                    let fresh = &router.laid[m];
                    crossed = order
                        .iter()
                        .copied()
                        .filter(|&o| o != m && fresh.items.iter().any(|a| router.laid[o].items.iter().any(|b| clash(a, b, step))))
                        .collect();
                }
                let mut lost = false;
                if depth >= 1 && problem.relay {
                    // Only what the net that pushes lies on, and no dearer than the pusher has won:
                    // a pushed net with no other way searched its whole area to find that out (a
                    // supply net of forty pads: 4.5 million cells, in every pass).
                    let won = gain;
                    router.budget = Some(won.max(0.0) / weight(m).max(1e-3));
                    router.relay(m, None, 0.0);
                    // It has no way in what is free, and the pusher has won much: then it may cross
                    // others in turn, and those take what is free, or cross in turn, up to a dozen
                    // nets. The nets moved so far are closed to it, or it would go back across the
                    // pusher. (A current sense net of 7 mm alone lay 67 mm long round half the
                    // board: its short way shut two nets in, one of which had a way across seven
                    // others, one of them shut in then. Fifteen nets moved, and their tracks were
                    // 46 mm shorter together. Two levels for every push took four times as long and
                    // saved no more; this is only where a net is left with no way.)
                    if moved.len() < 12 && won * step as f32 >= 2.0 && router.laid[m].open.len() > old.open.len() {
                        router.laid[m] = old.clone();
                        for (o, _) in &moved {
                            router.soft(*o, -1);
                            router.hard(*o, 1);
                        }
                        router.relay(m, Some(1.5 / weight(m).max(0.1)), 0.0);
                        for (o, _) in &moved {
                            router.hard(*o, -1);
                            router.soft(*o, 1);
                        }
                        let fresh = &router.laid[m];
                        crossed = order
                            .iter()
                            .copied()
                            .filter(|&o| o != m && !moved.iter().any(|t| t.0 == o))
                            .filter(|&o| fresh.items.iter().any(|a| router.laid[o].items.iter().any(|b| clash(a, b, step))))
                            .collect();
                    }
                    router.budget = None;
                    gain -= weight(m) * (router.cost(m) - router.cost_of(m, &old));
                    lost = router.laid[m].open.len() > old.open.len();
                } else if depth >= 1 || moved.len() + queue.len() + crossed.len() > 6 {
                    router.lay(m, None, 0.0);
                    crossed.clear();
                }
                router.soft(m, 1);
                moved.push((m, old));
                if lost {
                    break; // the exchange is off: no need to push the others
                }
                queue.extend(crossed.into_iter().map(|c| (c, depth + 1)));
            }
            if futile {
                tried[n] = fresh.now;
                continue;
            }
            let was: f32 = moved.iter().map(|(m, old)| weight(*m) * router.cost_of(*m, old)).sum();
            let now: f32 = moved.iter().map(|(m, _)| weight(*m) * router.cost(*m)).sum();
            let mut fine = true;
            for (m, old) in &moved {
                router.soft(*m, -1);
                fine = fine && router.laid[*m].open.len() <= old.open.len() && router.conflicts(*m, 0.0) == 0;
                router.soft(*m, 1);
            }
            let joined = router.laid[n].open.len() < moved[0].1.open.len();
            if fine && (joined || now < was - 0.5) {
                better += 1;
                pushed += (moved.len() > 1) as u32;
                saved += was - now;
                tried[n] = fresh.now;
                for (m, old) in &moved {
                    fresh.note(router.changed(old, &router.laid[*m]).into_iter(), reach + margin);
                }
                continue;
            }
            for (m, _) in &moved {
                router.soft(*m, -1);
            }
            for (m, old) in moved {
                router.laid[m] = old;
                router.soft(m, 1);
            }
            // not by pushing, then: the shortest way that is free as things lie
            let old = router.laid[n].clone();
            router.soft(n, -1);
            router.search.window = router.around(n, margin);
            if problem.relay && (tried[n] > 0 || router.jobs[n].cells.len() > 24) {
                router.lay_near(n, None, &fresh, tried[n]);
            } else {
                router.lay(n, None, 0.0);
            }
            let alone = router.cost(n);
            if router.laid[n].open.len() <= old.open.len() && alone < router.cost_of(n, &old) - 0.5 {
                better += 1;
                saved += weight(n) * (router.cost_of(n, &old) - alone);
                tried[n] = fresh.now;
                fresh.note(router.changed(&old, &router.laid[n]).into_iter(), reach + margin);
            } else {
                router.laid[n] = old;
                tried[n] = fresh.now;
            }
            router.soft(n, 1);
        }
        solution.shortened.push(better);
        say(format!(
            "pass {} ({:.0} s): {} of {} nets shorter, {} by pushing others, {:.1} mm (weighted) saved",
            pass + 1,
            clock.elapsed().as_secs_f64(),
            better,
            todo.len(),
            pushed,
            saved as f64 * step
        ));
        if better == 0 {
            break;
        }
    }
    router.search.window = whole;
    if problem.straight {
        let (mut before, mut after, mut nets) = (0, 0, 0);
        for round in 0..3 {
            // again, for what one net gives up another may take
            let mut now = 0;
            for &n in &order {
                let (b, a) = router.straighten(n);
                before += if round == 0 { b } else { 0 };
                nets += (round == 0 && a < b) as usize;
                now += a;
            }
            let done = round > 0 && now == after;
            after = now;
            if done {
                break;
            }
        }
        say(format!("straightened: {} bends became {}, in {} nets", before, after, nets));
    }
    solution.expanded = router.search.expanded;
    for (n, net) in problem.nets.iter().enumerate() {
        let laid = &router.laid[n];
        let open = if router.jobs[n].cells.len() > 1 && laid.paths.is_empty() && laid.open.is_empty() {
            (1..router.jobs[n].cells.len()).collect()
        } else {
            laid.open.clone()
        };
        solution.nets.push(Routed {
            name: net.name.clone(),
            paths: laid.paths.clone(),
            open,
            cost: (router.cost(n) - router.search.shy_cost * router.beside_of(n, laid)) as f64 * step,
            alone: alone[n] as f64 * step,
            beside: if router.jobs[n].shy.is_some_and(|a| a < 2) {
                router.beside_of(n, laid) as f64 * step
            } else {
                0.0
            },
        });
    }
    solution
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cells come out cheapest first to a quarter of a cell, also those that waited beyond the ring.
    #[test]
    fn the_queue_gives_the_cheapest_cell_first() {
        let mut q = Queue::new();
        let far = RING as f32; // four times the ring's reach
        for (node, f) in [(1u32, 7.0f32), (2, 3.1), (3, far), (4, 3.9), (5, far / 2.0), (6, 0.2)] {
            q.push(f, f, node);
        }
        let mut out = Vec::new();
        while let Some((node, g)) = q.pop() {
            out.push(node);
            if node == 2 {
                q.push(g + 1.0, g + 1.0, 7); // what a cell leads to costs more than the cell
            }
        }
        assert_eq!(out, vec![6, 2, 4, 7, 1, 5, 3]);
        q.push(1.0, 1.0, 8);
        q.push(far * 3.0, 0.0, 9);
        q.clear();
        assert!(q.pop().is_none());
    }
}
