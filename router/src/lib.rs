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
pub mod problem;
pub mod sexp;

use problem::{Path, Problem, Routed, Shape, Solution};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

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
    hard: Vec<Vec<u16>>,
    soft: Vec<Vec<u16>>,
    hist: Vec<Vec<f32>>,
    hard_via: Vec<Vec<u16>>,
    soft_via: Vec<Vec<u16>>,
    hist_via: Vec<Vec<f32>>,
}

fn dist2(item: &Item, x: f64, y: f64) -> f64 {
    let a = &item.a;
    if item.rect {
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

/// Add `delta` to every cell of one layer whose centre is within `grow` cells of the item.
fn raster(grid: &mut [u16], nx: usize, ny: usize, layer: usize, item: &Item, grow: f64, delta: i32) {
    let a = &item.a;
    let (x0, x1) = (a[0].min(a[2]) - grow, a[0].max(a[2]) + grow);
    let (y0, y1) = (a[1].min(a[3]) - grow, a[1].max(a[3]) + grow);
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
                *cell = (*cell as i32 + delta).clamp(0, u16::MAX as i32) as u16;
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

    /// Count the item in (or out of) every kind's grids. `margin`: extra distance in mm, for
    /// copper that does not lie on the grid.
    fn stamp(&mut self, item: &Item, soft: bool, margin: f64, delta: i32) {
        let (nx, ny, nl, step) = (self.nx, self.ny, self.nl, self.step);
        let centre = Item {
            rect: false,
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
            let grow = item.r + (item.clearance.max(clearance) + half + margin) / step;
            let grid = if soft { &mut self.soft[k] } else { &mut self.hard[k] };
            for &l in &item.layers {
                raster(grid, nx, ny, l, item, grow, delta);
            }
            if item.hole > 0.0 {
                let grow = item.hole + (self.hole_clear + half + margin) / step;
                for l in 0..nl {
                    raster(grid, nx, ny, l, &centre, grow, delta);
                }
            }
        }
        for c in 0..self.via_kinds.len() {
            let grid = if soft { &mut self.soft_via[c] } else { &mut self.hard_via[c] };
            if !item.layers.is_empty() {
                let keep = (item.clearance.max(self.via_kinds[c]) + self.via_r).max(self.hole_clear + self.via_hole / 2.0);
                raster(grid, nx, ny, 0, item, item.r + (keep + margin) / step, delta);
            }
            if item.hole > 0.0 {
                let grow = item.hole + (self.hole_clear + self.via_r + margin) / step;
                raster(grid, nx, ny, 0, &centre, grow, delta);
            }
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
}

struct Search {
    g: Vec<f32>,
    prev: Vec<u32>,
    seen: Vec<u32>,
    round: u32,
    target: Vec<u32>,
    /// Cells where the net being laid has a via already (== `laying`): a free change of layer.
    own_via: Vec<u32>,
    laying: u32,
    /// Cells the search may use: [i0, j0, i1, j1].
    window: [i32; 4],
    expanded: u64,
}

fn item_of(shape: &Shape, x0: f64, y0: f64, step: f64, clearance: f64) -> Item {
    let (rect, a) = match (shape.rect, shape.seg) {
        (Some(b), _) => (true, b),
        (None, Some(s)) => (false, s),
        (None, None) => panic!("a shape needs a box or a seg"),
    };
    Item {
        rect,
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

/// A path's copper: a line per straight piece, a via per layer change.
fn copper(grids: &Grids, job: &Job, half: f64, bends: &[[u32; 3]], items: &mut Vec<Item>, vias: &mut Vec<u32>) {
    for w in bends.windows(2) {
        let (p, q) = (w[0], w[1]);
        if p[0] == q[0] {
            items.push(Item {
                rect: false,
                a: [p[1] as f64, p[2] as f64, q[1] as f64, q[2] as f64],
                r: half / grids.step,
                layers: vec![p[0] as usize],
                hole: 0.0,
                clearance: job.clearance,
            });
        } else {
            items.push(Item {
                rect: false,
                a: [p[1] as f64, p[2] as f64, p[1] as f64, p[2] as f64],
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
        self.round += 1;
        let round = self.round;
        let window = self.window;
        let (nx, ny, nl) = (grids.nx as i32, grids.ny as i32, grids.nl);
        let (hard, soft, hist) = (&grids.hard[kind], &grids.soft[kind], &grids.hist[kind]);
        let (hard_via, soft_via, hist_via) = (&grids.hard_via[via_kind], &grids.soft_via[via_kind], &grids.hist_via[via_kind]);
        let union = goals.iter().fold([i32::MAX, i32::MAX, i32::MIN, i32::MIN], |u, b| {
            [u[0].min(b[0]), u[1].min(b[1]), u[2].max(b[2]), u[3].max(b[3])]
        });
        let one = [union];
        let goals: &[[i32; 4]] = if goals.len() > 12 { &one } else { goals };
        let h = |i: i32, j: i32| -> f32 {
            let mut best = f32::MAX;
            for b in goals {
                let dx = (b[0] - i).max(i - b[2]).max(0) as f32;
                let dy = (b[1] - j).max(j - b[3]).max(0) as f32;
                best = best.min(dx.max(dy) + (DIAG - 1.0) * dx.min(dy));
            }
            best
        };
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();
        for &s in sources {
            if hard[s as usize] > 0 || (press.is_none() && soft[s as usize] > 0) {
                continue;
            }
            self.g[s as usize] = 0.0;
            self.prev[s as usize] = NONE;
            self.seen[s as usize] = round;
            let (_l, i, j) = grids.split(s);
            heap.push(Reverse((h(i as i32, j as i32).to_bits(), s)));
        }
        let mut opened = 0u64;
        while let Some(Reverse((_f, node))) = heap.pop() {
            let n = node as usize;
            let g = self.g[n];
            if self.target[n] != 0 {
                let mut path = vec![node];
                while self.prev[*path.last().unwrap() as usize] != NONE {
                    path.push(self.prev[*path.last().unwrap() as usize]);
                }
                path.reverse();
                self.expanded += opened;
                return Some(path);
            }
            opened += 1;
            if opened > limit {
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
                if hard[m] > 0 {
                    continue;
                }
                if di != 0 && dj != 0 && (hard[layer + (j * nx + a) as usize] > 0 || hard[layer + (c * nx + i) as usize] > 0) {
                    continue;
                }
                let crowd = match (press, soft[m]) {
                    (_, 0) => 1.0,
                    (None, _) => continue,
                    (Some(p), s) => 1.0 + p * s as f32,
                };
                let ng = g + len * (1.0 + hist[m] * memory) * crowd;
                if self.seen[m] != round || ng < self.g[m] {
                    self.seen[m] = round;
                    self.g[m] = ng;
                    self.prev[m] = node;
                    heap.push(Reverse(((ng + h(a, c)).to_bits(), m as u32)));
                }
            }
            let flat = (j * nx + i) as usize;
            let cost = if self.own_via[flat] == self.laying {
                0.0
            } else {
                if hard_via[flat] > 0 {
                    continue;
                }
                let crowd = match (press, soft_via[flat]) {
                    (_, 0) => 1.0,
                    (None, _) => continue,
                    (Some(p), s) => 1.0 + p * s as f32,
                };
                via_cost * (1.0 + hist_via[flat] * memory) * crowd
            };
            for other in 0..nl {
                let m = other * (nx * ny) as usize + flat;
                if other == l || hard[m] > 0 || (press.is_none() && soft[m] > 0) {
                    continue;
                }
                let ng = g + cost + press.unwrap_or(0.0) * soft[m] as f32;
                if self.seen[m] != round || ng < self.g[m] {
                    self.seen[m] = round;
                    self.g[m] = ng;
                    self.prev[m] = node;
                    heap.push(Reverse(((ng + h(i, j)).to_bits(), m as u32)));
                }
            }
        }
        self.expanded += opened;
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
}

impl Router<'_> {
    /// Take the net's own pads out of the way of its tracks (or put them back), and keep its
    /// vias out of its own pads: no via in a pad, no second hole beside the first.
    fn own(&mut self, n: usize, delta: i32) {
        let margin = self.grids.step / 2.0;
        let (nx, ny, step) = (self.grids.nx, self.grids.ny, self.grids.step);
        let (via_r, hole_clear, via_kind) = (self.grids.via_r, self.grids.hole_clear, self.jobs[n].via_kind);
        for piece in 0..self.jobs[n].items.len() {
            for s in 0..self.jobs[n].items[piece].len() {
                let item = self.jobs[n].items[piece][s].clone();
                self.grids.stamp(&item, false, margin, -delta);
                let grid = &mut self.grids.hard_via[via_kind];
                if item.hole > 0.0 {
                    let (x, y) = ((item.a[0] + item.a[2]) / 2.0, (item.a[1] + item.a[3]) / 2.0);
                    let centre = Item {
                        rect: false,
                        a: [x, y, x, y],
                        r: 0.0,
                        layers: vec![],
                        hole: 0.0,
                        clearance: 0.0,
                    };
                    raster(grid, nx, ny, 0, &centre, item.hole + (hole_clear + via_r) / step, delta);
                } else if item.rect {
                    raster(grid, nx, ny, 0, &item, via_r * 0.5 / step, delta);
                }
            }
        }
    }

    fn soft(&mut self, n: usize, delta: i32) {
        for k in 0..self.laid[n].items.len() {
            let item = self.laid[n].items[k].clone();
            self.grids.stamp(&item, true, 0.0, delta);
        }
    }

    /// Cells of the net's tracks and vias that another net's copper is too near to. The net's own
    /// copper must be out of the counts. With `remember`, those cells get dearer for good.
    fn conflicts(&mut self, n: usize, remember: f32) -> u64 {
        let via_kind = self.jobs[n].via_kind;
        let mut count = 0;
        for (path, &kind) in self.laid[n].nodes.iter().zip(&self.laid[n].kinds) {
            for &node in path {
                if self.grids.soft[kind][node as usize] > 0 {
                    count += 1;
                    self.grids.hist[kind][node as usize] += remember;
                }
            }
        }
        for &flat in &self.laid[n].vias {
            if self.grids.soft_via[via_kind][flat as usize] > 0 {
                count += 1;
                self.grids.hist_via[via_kind][flat as usize] += remember;
            }
        }
        count
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

    /// What the net's tracks cost: their length in cells, a via as `via_cost`, a narrow track half as much again.
    fn cost(&self, n: usize) -> f32 {
        self.cost_of(n, &self.laid[n])
    }

    fn cost_of(&self, n: usize, laid: &Laid) -> f32 {
        let plane = (self.grids.nx * self.grids.ny) as u32;
        let mut total = 0.0;
        for (path, &kind) in laid.nodes.iter().zip(&laid.kinds) {
            let factor = if kind == self.jobs[n].kind { 1.0 } else { 1.5 };
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
        total
    }

    /// Lay the net anew: from its first piece, always to the nearest piece not yet joined.
    fn lay(&mut self, n: usize, press: Option<f32>, memory: f32) {
        self.own(n, 1);
        self.search.laying += 1;
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
        let mut joined = vec![false; job.cells.len()];
        joined[0] = true;
        let mut sources: Vec<u32> = job.cells[0].iter().map(|c| c.0).collect();
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
            let mut kind = job.kind;
            let mut found = self.search.run(
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
            if found.is_none() && self.widen && self.search.window != self.whole {
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
            if let (None, Some(narrow)) = (&found, job.narrow) {
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
            let piece = found.as_ref().map(|path| self.search.target[*path.last().unwrap() as usize] as usize - 1);
            for node in marked {
                self.search.target[node as usize] = 0;
            }
            let Some(path) = found else {
                laid.stuck = press.is_some();
                break;
            };
            let piece = piece.unwrap();
            joined[piece] = true;
            sources.extend(path.iter().copied());
            sources.extend(job.cells[piece].iter().map(|c| c.0));
            let bends = corners(&self.grids, &path);
            let had = laid.vias.len();
            copper(&self.grids, job, self.grids.kinds[kind].0, &bends, &mut laid.items, &mut laid.vias);
            for &flat in &laid.vias[had..] {
                // the net's next via: on this one or a hole's distance away
                let (x, y) = ((flat as usize % self.grids.nx) as f64, (flat as usize / self.grids.nx) as f64);
                let hole = Item {
                    rect: false,
                    a: [x, y, x, y],
                    r: 0.0,
                    layers: vec![],
                    hole: 0.0,
                    clearance: 0.0,
                };
                raster(&mut self.grids.hard_via[job.via_kind], self.grids.nx, self.grids.ny, 0, &hole, apart, 1);
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
        for hole in &near {
            raster(
                &mut self.grids.hard_via[self.jobs[n].via_kind],
                self.grids.nx,
                self.grids.ny,
                0,
                hole,
                apart,
                -1,
            );
        }
        laid.open = (0..joined.len()).filter(|&p| !joined[p]).collect();
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
        hard: vec![vec![0; nx * ny * nl]; kinds.len()],
        soft: vec![vec![0; nx * ny * nl]; kinds.len()],
        hist: vec![vec![0.0; nx * ny * nl]; kinds.len()],
        hard_via: vec![vec![0; nx * ny]; via_kinds.len()],
        soft_via: vec![vec![0; nx * ny]; via_kinds.len()],
        hist_via: vec![vec![0.0; nx * ny]; via_kinds.len()],
        kinds,
        via_kinds,
    };
    let margin = step / 2.0;
    // the board's edge and the keep-out areas: no track centre within half a track of them
    for k in 0..grids.kinds.len() + grids.via_kinds.len() {
        let via = k >= grids.kinds.len();
        let half = if via { grids.via_r } else { grids.kinds[k].0 };
        let layers = if via { 1 } else { nl };
        let grid = if via {
            &mut grids.hard_via[k - grids.kinds.len()]
        } else {
            &mut grids.hard[k]
        };
        if let Some(e) = problem.edge {
            let inset = problem.edge_clear + half;
            for l in 0..layers {
                for j in 0..ny {
                    for i in 0..nx {
                        let (x, y) = (x0 + i as f64 * step, y0 + j as f64 * step);
                        if x < e[0] + inset - 1e-9 || x > e[2] - inset + 1e-9 || y < e[1] + inset - 1e-9 || y > e[3] - inset + 1e-9 {
                            grid[(l * ny + j) * nx + i] += 1;
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
                r: 0.0,
                layers: vec![],
                hole: 0.0,
                clearance: 0.0,
            };
            if via {
                if !keepout.layers.is_empty() {
                    raster(grid, nx, ny, 0, &item, half / step, 1);
                }
            } else {
                for &l in &keepout.layers {
                    raster(grid, nx, ny, l, &item, half / step, 1);
                }
            }
        }
    }
    for shape in &problem.fixed {
        grids.stamp(&item_of(shape, x0, y0, step, shape.clearance), false, margin, 1);
    }
    let mut jobs = Vec::new();
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
        };
        for piece in &net.pieces {
            let items: Vec<Item> = piece.iter().map(|s| item_of(s, x0, y0, step, net.clearance)).collect();
            let mut cells = Vec::new();
            let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
            for (s, item) in items.iter().enumerate() {
                grids.stamp(item, false, margin, 1);
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
    let nodes = nx * ny * nl;
    let mut router = Router {
        problem,
        laid: vec![Laid::default(); jobs.len()],
        jobs,
        search: Search {
            g: vec![0.0; nodes],
            prev: vec![NONE; nodes],
            seen: vec![0; nodes],
            round: 0,
            target: vec![0; nodes],
            own_via: vec![0; nx * ny],
            laying: 0,
            window: [0, 0, nx as i32 - 1, ny as i32 - 1],
            expanded: 0,
        },
        via_cost: (problem.via.cost / step) as f32,
        whole: [0, 0, nx as i32 - 1, ny as i32 - 1],
        widen: true,
        grids,
    };
    let mut solution = Solution::default();
    let near = (4.0 / step) as i32;
    // Weights play no part in these rounds. Three ways to let them were measured on a board with
    // 158 nets, 13 of them weighted (260 mm with only those on the board, 315 mm without weights):
    // heavy nets feel congestion less (one came out six times longer, rounds doubled), heavy nets
    // laid first and slow to give way (308 mm), heavy nets laid first and held (320 mm, rounds
    // three times as long). None is worth its code; weights act in the shortening below.
    let mut press = 0.5f32;
    let mut clean = false;
    for round in 0..problem.max_iter {
        let (mut found, mut again) = (0u64, 0usize);
        for &n in &order {
            if round > 0 {
                router.soft(n, -1);
                let c = router.conflicts(n, 0.4);
                let retry = !router.laid[n].open.is_empty() && !router.laid[n].stuck;
                if c == 0 && !retry {
                    router.soft(n, 1);
                    continue;
                }
                found += c;
            }
            // a search stays within 4 mm of the net's pads and tracks, unless there is no way there:
            // without this bound the supply board's 158 nets took 146 s, nearly all of it searching
            router.search.window = router.around(n, near);
            router.lay(n, Some(press), 1.0);
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
        } else {
            say(format!("round 0: {} nets laid", again));
        }
        press = (press * 1.6).min(1e5);
    }
    if !clean {
        // out of rounds: take out the net with the most conflicts until none is left, then give
        // each of those the room that remains
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
            problem.max_iter,
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
    let whole = router.whole;
    router.widen = false;
    let mut stale = vec![true; router.jobs.len()]; // a net is tried again only if something near it has changed
    say(format!("all nets laid after {:.0} s", started.elapsed().as_secs_f64()));
    let mut ideal = vec![0.0f32; router.jobs.len()];
    for &n in &order {
        // what the net would cost alone on the board: no result is better than that
        let old = router.laid[n].clone();
        router.soft(n, -1);
        router.search.window = router.around(n, margin);
        router.lay(n, Some(0.0), 0.0);
        ideal[n] = if router.laid[n].open.is_empty() { router.cost(n) } else { 0.0 };
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
            .filter(|t| t.0 >= 4.0 && stale[t.1])
            .collect();
        todo.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let (mut better, mut pushed, mut saved) = (0u32, 0u32, 0.0f32);
        for &(_, n) in &todo {
            // n takes its way across others, and those it crosses take what is then free. (Letting
            // them push in turn, two levels deep, took four times as long on a board of 158 nets and
            // saved no more.)
            let mut moved: Vec<(usize, Laid)> = Vec::new();
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
                    router.lay(m, Some(1.5 / weight(m).max(0.1)), 0.0); // a heavier net crosses others more readily
                    let fresh = &router.laid[m];
                    crossed = order
                        .iter()
                        .copied()
                        .filter(|&o| o != m && fresh.items.iter().any(|a| router.laid[o].items.iter().any(|b| clash(a, b, step))))
                        .collect();
                }
                if depth >= 1 || moved.len() + queue.len() + crossed.len() > 6 {
                    router.lay(m, None, 0.0);
                    crossed.clear();
                }
                router.soft(m, 1);
                moved.push((m, old));
                queue.extend(crossed.into_iter().map(|c| (c, depth + 1)));
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
                for (m, _) in &moved {
                    let b = router.around(*m, margin);
                    for &o in &order {
                        let c = router.around(o, 0);
                        stale[o] = stale[o] || (b[0] <= c[2] && c[0] <= b[2] && b[1] <= c[3] && c[1] <= b[3]);
                    }
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
            router.lay(n, None, 0.0);
            let alone = router.cost(n);
            if router.laid[n].open.len() <= old.open.len() && alone < router.cost_of(n, &old) - 0.5 {
                better += 1;
                saved += weight(n) * (router.cost_of(n, &old) - alone);
                let b = router.around(n, margin);
                for &o in &order {
                    let c = router.around(o, 0);
                    stale[o] = stale[o] || (b[0] <= c[2] && c[0] <= b[2] && b[1] <= c[3] && c[1] <= b[3]);
                }
            } else {
                router.laid[n] = old;
                stale[n] = false;
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
            cost: router.cost(n) as f64 * step,
            alone: ideal[n] as f64 * step,
        });
    }
    solution
}
