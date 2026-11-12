//! Where the parts want to be, found with forces: a start for the annealing that does not depend
//! on the order in which a constructive placer set the parts down.
//!
//! Each part is a charge as large as its courtyard. Connections pull (the same nets, tethers and
//! anchors the annealing counts, in a smooth form that has a slope), like charges push each other
//! apart. The push is the field of the charge density, found by solving Poisson's equation on a
//! grid of `BIN` mm with cosine sums (no flow through the board's edge), as in ePlace (Lu et al.,
//! 2015). First the pull alone acts and the parts gather; then the push grows by `GROW` per step
//! until no more than `spill` of the courtyard area lies where a bin holds more courtyard than it
//! has room, or until that share stops falling.
//!
//! What a part may not do is put in as charge that does not move: a keep-out, a part fixed by
//! hand, and everything outside the rectangles the part must stay in. Parts with the same
//! rectangles on the same side share one field. That charge is as dense as the parts are on
//! average in the room they have, so that the field is nought when they are spread evenly over
//! that room (denser, and the parts would gather in the middle of their room; none, and they would
//! gather at its walls).
//!
//! The sides stay as they came. The rotation is the best of those allowed for the connections as
//! they lie, asked again every `TURN` steps. Overlaps remain: `legalise()` sets the parts down one
//! by one, the largest first, each at the place where it is allowed and its connections are
//! shortest among those up to `LOOK` mm further away than the nearest such place.
//!
//! On a board of 287 parts (254 that move, 55 to 70 % of the room full, eight groups of
//! rectangles, 26 keep-outs), eight seeds each: the annealing ended at a cost of 2974 mm on
//! average from the placement as it came and at 2705 mm from this start, nets and tethers 15 %
//! shorter, every seed better than the best without. Two things made the difference between
//! worse and better than without: stopping the push early (see `spill` in the problem), and
//! choosing the place by the connections when the overlaps are removed (by distance alone, one
//! part in ten went more than 6 mm from where it wanted to be).

use super::{Pose, Rng, State};
use std::f64::consts::PI;

/// Side of a bin of the density grid, mm (the board is divided into whole bins of about this).
const BIN: f64 = 1.0;
/// Steps with the pull alone, and the most steps with the push.
const WIRES: usize = 30;
const MOST: usize = 600;
/// The push grows by this per step.
const GROW: f64 = 1.02;
/// The rotations are asked again every so many steps.
const TURN: usize = 25;
/// The push ends when the share too full has not got smaller for this many steps.
const WAIT: usize = 60;
/// When overlaps are removed, a part looks this far beyond the nearest place where it is allowed, mm.
const LOOK: f64 = 3.0;

struct Grid {
    nx: usize,
    ny: usize,
    hx: f64,
    hy: f64,
}

impl Grid {
    fn new(size: [f64; 2]) -> Grid {
        let nx = (size[0] / BIN).ceil().max(2.0) as usize;
        let ny = (size[1] / BIN).ceil().max(2.0) as usize;
        Grid {
            nx,
            ny,
            hx: size[0] / nx as f64,
            hy: size[1] / ny as f64,
        }
    }

    /// Every bin the rectangle reaches into, with the area they share.
    fn cover(&self, r: [f64; 4], mut f: impl FnMut(usize, f64)) {
        let (x0, y0) = (r[0].max(0.0), r[1].max(0.0));
        let (x1, y1) = (r[2].min(self.nx as f64 * self.hx), r[3].min(self.ny as f64 * self.hy));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let (i0, j0) = ((x0 / self.hx).floor() as usize, (y0 / self.hy).floor() as usize);
        let (i1, j1) = (((x1 / self.hx).ceil() as usize).min(self.nx), ((y1 / self.hy).ceil() as usize).min(self.ny));
        for j in j0..j1 {
            let oy = y1.min((j + 1) as f64 * self.hy) - y0.max(j as f64 * self.hy);
            for i in i0..i1 {
                let ox = x1.min((i + 1) as f64 * self.hx) - x0.max(i as f64 * self.hx);
                if ox > 0.0 && oy > 0.0 {
                    f(j * self.nx + i, ox * oy);
                }
            }
        }
    }
}

/// The field of a charge density on the grid: -grad of the potential p with -laplace p = density
/// (less its mean) and no field through the edge.
struct Field {
    nx: usize,
    ny: usize,
    cx: Vec<f64>,
    sx: Vec<f64>,
    cy: Vec<f64>,
    sy: Vec<f64>,
    kx: Vec<f64>,
    ky: Vec<f64>,
    a: Vec<f64>,
    t: Vec<f64>,
    ex: Vec<f64>,
    ey: Vec<f64>,
}

impl Field {
    fn new(grid: &Grid) -> Field {
        let table = |n: usize, f: fn(f64) -> f64| -> Vec<f64> { (0..n * n).map(|k| f(PI * (k / n) as f64 * ((k % n) as f64 + 0.5) / n as f64)).collect() };
        let (nx, ny) = (grid.nx, grid.ny);
        Field {
            nx,
            ny,
            cx: table(nx, f64::cos),
            sx: table(nx, f64::sin),
            cy: table(ny, f64::cos),
            sy: table(ny, f64::sin),
            kx: (0..nx).map(|u| PI * u as f64 / (nx as f64 * grid.hx)).collect(),
            ky: (0..ny).map(|v| PI * v as f64 / (ny as f64 * grid.hy)).collect(),
            a: vec![0.0; nx * ny],
            t: vec![0.0; nx * ny],
            ex: vec![0.0; nx * ny],
            ey: vec![0.0; nx * ny],
        }
    }

    /// rho[j * nx + i] -> ex, ey in the same order.
    fn solve(&mut self, rho: &[f64]) {
        let (nx, ny) = (self.nx, self.ny);
        for u in 0..nx {
            let cu = &self.cx[u * nx..(u + 1) * nx];
            for j in 0..ny {
                self.t[u * ny + j] = rho[j * nx..(j + 1) * nx].iter().zip(cu).map(|(r, c)| r * c).sum();
            }
        }
        for u in 0..nx {
            for v in 0..ny {
                let s: f64 = self.t[u * ny..(u + 1) * ny]
                    .iter()
                    .zip(&self.cy[v * ny..(v + 1) * ny])
                    .map(|(t, c)| t * c)
                    .sum();
                let norm = (if u == 0 { 1.0 } else { 2.0 }) / nx as f64 * (if v == 0 { 1.0 } else { 2.0 }) / ny as f64;
                let k2 = self.kx[u] * self.kx[u] + self.ky[v] * self.ky[v];
                self.a[u * ny + v] = if u + v == 0 { 0.0 } else { s * norm / k2 };
            }
        }
        // ex = sum over u, v of a * kx * sin(kx x) * cos(ky y)
        for u in 0..nx {
            for j in 0..ny {
                self.t[u * ny + j] = (0..ny).map(|v| self.a[u * ny + v] * self.cy[v * ny + j]).sum();
            }
        }
        for j in 0..ny {
            for i in 0..nx {
                self.ex[j * nx + i] = (0..nx).map(|u| self.kx[u] * self.sx[u * nx + i] * self.t[u * ny + j]).sum();
            }
        }
        // ey = sum over u, v of a * ky * cos(kx x) * sin(ky y)
        for u in 0..nx {
            for j in 0..ny {
                self.t[u * ny + j] = (0..ny).map(|v| self.a[u * ny + v] * self.ky[v] * self.sy[v * ny + j]).sum();
            }
        }
        for j in 0..ny {
            for i in 0..nx {
                self.ey[j * nx + i] = (0..nx).map(|u| self.cx[u * nx + i] * self.t[u * ny + j]).sum();
            }
        }
    }
}

/// The parts with the same rectangles to stay in, on one side: they share a field.
struct System {
    side: usize,
    /// Per bin: the share of it these parts may be in at all, and the share that is free.
    allowed: Vec<f64>,
    room: Vec<f64>,
    parts: Vec<usize>,
}

struct Work<'a, 'b> {
    state: &'a State<'b>,
    grid: Grid,
    field: Field,
    systems: Vec<System>,
    /// Per part: the middle of its courtyard, and its form.
    centre: Vec<[f64; 2]>,
    form: Vec<usize>,
    /// Courtyard area of the parts that move, per side and bin.
    moving: [Vec<f64>; 2],
    rho: Vec<f64>,
}

/// The slope of a smooth largest-less-smallest of the values (the weighted average of Hsu et al.).
fn slope(xs: &[f64], gamma: f64, out: &mut [f64]) {
    let (lo, hi) = xs.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
    let (mut sp, mut xp, mut sn, mut xn) = (0.0, 0.0, 0.0, 0.0);
    for &x in xs {
        let (ep, en) = (((x - hi) / gamma).exp(), ((lo - x) / gamma).exp());
        sp += ep;
        xp += x * ep;
        sn += en;
        xn += x * en;
    }
    let (wp, wn) = (xp / sp, xn / sn);
    for (k, &x) in xs.iter().enumerate() {
        let (ep, en) = (((x - hi) / gamma).exp(), ((lo - x) / gamma).exp());
        out[k] = ep * (1.0 + (x - wp) / gamma) / sp - en * (1.0 - (x - wn) / gamma) / sn;
    }
}

impl Work<'_, '_> {
    fn rect(&self, p: usize) -> &[f64; 4] {
        &self.state.forms[p][self.form[p]].as_ref().unwrap().rect
    }

    fn half(&self, p: usize) -> [f64; 2] {
        let r = self.rect(p);
        [(r[2] - r[0]) / 2.0, (r[3] - r[1]) / 2.0]
    }

    fn pin(&self, p: usize, pad: usize) -> [f64; 2] {
        let form = self.state.forms[p][self.form[p]].as_ref().unwrap();
        let (r, d, c) = (&form.rect, &form.pads[pad], &self.centre[p]);
        [c[0] + d[0] - (r[0] + r[2]) / 2.0, c[1] + d[1] - (r[1] + r[3]) / 2.0]
    }

    fn sides(&self, p: usize) -> [bool; 2] {
        let back = self.form[p] % 2 == 1;
        if self.state.problem.parts[p].through {
            [true, true]
        } else {
            [!back, back]
        }
    }

    /// The part as a charge: its courtyard with half the gap round it, no smaller than a bin and a
    /// half each way (a smaller one would fall between the bins), and how dense it then is.
    fn charge(&self, p: usize) -> ([f64; 4], f64) {
        let (h, c, g) = (self.half(p), self.centre[p], self.state.problem.gap / 2.0);
        let (w, v) = (h[0] + g, h[1] + g);
        let (ww, vv) = (w.max(0.75 * self.grid.hx), v.max(0.75 * self.grid.hy));
        ([c[0] - ww, c[1] - vv, c[0] + ww, c[1] + vv], w * v / (ww * vv))
    }

    /// The pull on every part: the slope of what its nets, tethers and anchor cost.
    fn pull(&self, gamma: f64, grad: &mut [[f64; 2]]) {
        let state = self.state;
        let (mut xs, mut ys, mut dx, mut dy) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let two = |a: (usize, usize), b: (usize, usize)| [a, b];
        let nets = state.pins.iter().enumerate().map(|(n, pins)| (state.problem.nets[n].weight, pins.clone()));
        let ties = state.tethers.iter().map(|&(a, b, w)| (w, two(a, b).to_vec()));
        for (weight, pins) in nets.chain(ties) {
            if pins.len() < 2 {
                continue;
            }
            xs.clear();
            ys.clear();
            for &(p, pad) in &pins {
                let q = self.pin(p, pad);
                xs.push(q[0]);
                ys.push(q[1]);
            }
            dx.resize(xs.len(), 0.0);
            dy.resize(xs.len(), 0.0);
            slope(&xs, gamma, &mut dx);
            slope(&ys, gamma, &mut dy);
            for (k, &(p, _)) in pins.iter().enumerate() {
                grad[p][0] += weight * dx[k];
                grad[p][1] += weight * dy[k];
            }
        }
        for (p, part) in state.problem.parts.iter().enumerate() {
            if let Some(a) = part.anchor {
                let d = [self.centre[p][0] - a[0], self.centre[p][1] - a[1]];
                grad[p][0] += part.pull * d[0] / (d[0] * d[0] + 0.25).sqrt();
                grad[p][1] += part.pull * d[1] / (d[1] * d[1] + 0.25).sqrt();
            }
        }
    }

    /// The push on every part that moves; returns the share of their area that lies where a bin
    /// is fuller than it may be (over the systems, each counted by the area of its own parts).
    fn push(&mut self, free: &[usize], force: &mut [[f64; 2]]) -> f64 {
        let area = self.grid.hx * self.grid.hy;
        for map in self.moving.iter_mut() {
            map.iter_mut().for_each(|c| *c = 0.0);
        }
        for &p in free {
            let ((rect, scale), sides) = (self.charge(p), self.sides(p));
            for (side, _) in sides.iter().enumerate().filter(|s| *s.1) {
                let map = &mut self.moving[side];
                self.grid.cover(rect, |b, a| map[b] += a * scale);
            }
        }
        let (mut spilt, mut all) = (0.0, 0.0);
        for s in 0..self.systems.len() {
            let system = &self.systems[s];
            let moving = &self.moving[system.side];
            let inside = |b: usize| system.allowed[b] > 0.0;
            let charge: f64 = (0..moving.len()).filter(|&b| inside(b)).map(|b| moving[b]).sum();
            let room: f64 = system.room.iter().sum::<f64>() * area;
            if charge <= 0.0 || room <= 0.0 {
                continue;
            }
            let mean = charge / room;
            let mut over = 0.0;
            for (b, (rho, &here)) in self.rho.iter_mut().zip(moving).enumerate() {
                *rho = if inside(b) { here / area + (1.0 - system.room[b]) * mean } else { mean };
                if inside(b) {
                    over += (here - system.room[b] * area).max(0.0);
                }
            }
            let own: f64 = system
                .parts
                .iter()
                .map(|&p| self.charge(p))
                .map(|(r, scale)| (r[2] - r[0]) * (r[3] - r[1]) * scale)
                .sum();
            spilt += over / charge * own;
            all += own;
            self.field.solve(&self.rho);
            for &p in &self.systems[s].parts {
                let (rect, scale) = self.charge(p);
                let (ex, ey) = (&self.field.ex, &self.field.ey);
                let f = &mut force[p];
                self.grid.cover(rect, |b, a| {
                    f[0] += a * scale * ex[b];
                    f[1] += a * scale * ey[b];
                });
            }
        }
        spilt / all.max(1e-12)
    }

    /// Pull and push on every part with the parts that move at `v`, and the share too full.
    fn forces(&mut self, free: &[usize], v: &[[f64; 2]], gamma: f64, pushing: bool) -> (Vec<[f64; 2]>, Vec<[f64; 2]>, f64) {
        for (k, &p) in free.iter().enumerate() {
            self.centre[p] = v[k];
        }
        let n = self.centre.len();
        let (mut pull, mut push) = (vec![[0.0; 2]; n], vec![[0.0; 2]; n]);
        self.pull(gamma, &mut pull);
        let over = if pushing { self.push(free, &mut push) } else { 1.0 };
        (pull, push, over)
    }

    /// The nearest place to c where the part is on the board and within its rectangles.
    fn within(&self, p: usize, c: [f64; 2]) -> [f64; 2] {
        let (problem, h) = (self.state.problem, self.half(p));
        let fit = |c: [f64; 2], b: &[f64; 4]| {
            [
                c[0].clamp(b[0] + h[0], (b[2] - h[0]).max(b[0] + h[0])),
                c[1].clamp(b[1] + h[1], (b[3] - h[1]).max(b[1] + h[1])),
            ]
        };
        let mut c = fit(c, &[problem.edge, problem.edge, problem.size[0] - problem.edge, problem.size[1] - problem.edge]);
        for _ in 0..2 {
            for group in &problem.parts[p].inside {
                let near = group
                    .iter()
                    .filter(|b| b[2] - b[0] >= 2.0 * h[0] && b[3] - b[1] >= 2.0 * h[1])
                    .map(|b| fit(c, b))
                    .min_by(|a, b| ((a[0] - c[0]).abs() + (a[1] - c[1]).abs()).total_cmp(&((b[0] - c[0]).abs() + (b[1] - c[1]).abs())));
                c = near.unwrap_or(c);
            }
        }
        c
    }

    /// What the part's nets and tethers cost as the parts lie, as the annealing counts it.
    fn local(&self, p: usize) -> f64 {
        let state = self.state;
        let mut total = 0.0;
        for &n in &state.of_part[p] {
            let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for &(q, pad) in &state.pins[n] {
                let at = self.pin(q, pad);
                b = [b[0].min(at[0]), b[1].min(at[1]), b[2].max(at[0]), b[3].max(at[1])];
            }
            if state.pins[n].len() > 1 {
                total += state.problem.nets[n].weight * (b[2] - b[0] + b[3] - b[1]);
            }
        }
        for &t in &state.tied[p] {
            let (a, b, w) = state.tethers[t];
            let (a, b) = (self.pin(a.0, a.1), self.pin(b.0, b.1));
            total += w * ((a[0] - b[0]).abs() + (a[1] - b[1]).abs());
        }
        total
    }

    /// Turn every part that moves the way its connections are shortest, on the side it is on.
    fn turn(&mut self, free: &[usize]) -> usize {
        let mut turned = 0;
        for &p in free {
            let (was, mut best) = (self.form[p], (self.local(p) - 1e-6, self.form[p]));
            for form in (0..8).filter(|f| f % 2 == was % 2 && *f != was && self.state.forms[p][*f].is_some()) {
                self.form[p] = form;
                let cost = self.local(p);
                if cost < best.0 {
                    best = (cost, form);
                }
            }
            self.form[p] = best.1;
            turned += (best.1 != was) as usize;
        }
        turned
    }

    /// Net length as the annealing counts it, mm.
    fn length(&self) -> f64 {
        let state = self.state;
        let mut total = 0.0;
        for (n, pins) in state.pins.iter().enumerate().filter(|(_, pins)| pins.len() > 1) {
            let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
            for &(q, pad) in pins {
                let at = self.pin(q, pad);
                b = [b[0].min(at[0]), b[1].min(at[1]), b[2].max(at[0]), b[3].max(at[1])];
            }
            total += state.problem.nets[n].weight * (b[2] - b[0] + b[3] - b[1]);
        }
        total
    }
}

/// Find where the parts in `free` want to be. Sets their rotation in `state.pose` and returns the
/// middle of each one's courtyard, in the order of `free`. The places overlap: see `legalise()`.
pub(super) fn spread(state: &mut State, free: &[usize], rng: &mut Rng, say: &mut dyn FnMut(String)) -> Vec<[f64; 2]> {
    let problem = state.problem;
    let n = problem.parts.len();
    let grid = Grid::new(problem.size);
    let (bins, area, g) = (grid.nx * grid.ny, grid.hx * grid.hy, problem.gap / 2.0);
    // what does not move, per side
    let mut blocked = [vec![0.0; bins], vec![0.0; bins]];
    for keep in &problem.keeps {
        for (side, name) in ['F', 'B'].into_iter().enumerate() {
            if keep.sides.contains(name) {
                grid.cover(keep.rect, |b, a| blocked[side][b] += a / area);
            }
        }
    }
    for p in (0..n).filter(|&p| problem.parts[p].fixed) {
        let (r, sides) = (state.rect(p), state.sides(p));
        for (side, _) in sides.iter().enumerate().filter(|s| *s.1) {
            grid.cover([r[0] - g, r[1] - g, r[2] + g, r[3] + g], |b, a| blocked[side][b] += a / area);
        }
    }
    let mut board = vec![0.0; bins];
    grid.cover(
        [problem.edge, problem.edge, problem.size[0] - problem.edge, problem.size[1] - problem.edge],
        |b, a| board[b] = a / area,
    );
    let mut systems: Vec<System> = Vec::new();
    let mut keys: Vec<(String, usize)> = Vec::new();
    let mut work = Work {
        state,
        field: Field::new(&grid),
        grid,
        systems: Vec::new(),
        centre: (0..n).map(|p| state.rect(p)).map(|r| [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0]).collect(),
        form: state.pose.iter().map(|pose| pose.form).collect(),
        moving: [vec![0.0; bins], vec![0.0; bins]],
        rho: vec![0.0; bins],
    };
    for &p in free {
        let sides = work.sides(p);
        for (side, _) in sides.iter().enumerate().filter(|s| *s.1) {
            let key = (format!("{:?}", problem.parts[p].inside), side);
            let s = keys.iter().position(|k| *k == key).unwrap_or_else(|| {
                let mut allowed = board.clone();
                for group in &problem.parts[p].inside {
                    let mut share = vec![0.0; bins];
                    for rect in group {
                        work.grid.cover(*rect, |b, a| share[b] += a / area);
                    }
                    allowed.iter_mut().zip(&share).for_each(|(a, s)| *a = a.min(*s));
                }
                let room = allowed.iter().zip(&blocked[side]).map(|(a, b)| (a - b.min(1.0)).max(0.0)).collect();
                systems.push(System {
                    side,
                    allowed,
                    room,
                    parts: Vec::new(),
                });
                keys.push(key);
                systems.len() - 1
            });
            systems[s].parts.push(p);
        }
    }
    for system in &systems {
        let charge: f64 = system.parts.iter().map(|&p| work.half(p)).map(|h| 4.0 * (h[0] + g) * (h[1] + g)).sum();
        let room = system.room.iter().sum::<f64>() * area;
        say(format!(
            "{} parts on the {} with {:.0} mm2 of courtyard in {:.0} mm2 of room: {:.0} % full",
            system.parts.len(),
            if system.side == 0 { "front" } else { "back" },
            charge,
            room,
            100.0 * charge / room.max(1e-9)
        ));
    }
    work.systems = systems;

    let weight: Vec<f64> = (0..n)
        .map(|p| {
            state.of_part[p].iter().map(|&k| problem.nets[k].weight).sum::<f64>()
                + state.tied[p].iter().map(|&t| state.tethers[t].2).sum::<f64>()
                + problem.parts[p].pull
        })
        .collect();
    let size: Vec<f64> = (0..n).map(|p| work.half(p)).map(|h| 4.0 * (h[0] + g) * (h[1] + g)).collect();
    let mut u: Vec<[f64; 2]> = free.iter().map(|&p| work.centre[p]).collect();
    let mut v = u.clone();
    let far = |a: &[[f64; 2]], b: &[[f64; 2]]| a.iter().zip(b).map(|(a, b)| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sum::<f64>().sqrt();
    let sum = |f: &[[f64; 2]]| free.iter().map(|&p| f[p][0].abs() + f[p][1].abs()).sum::<f64>();
    let most = |g: &[[f64; 2]]| g.iter().map(|g| g[0].abs().max(g[1].abs())).fold(0.0, f64::max).max(1e-12);
    // pull and push as one slope, each part's divided by what holds it (its connections, its size)
    let slope = |pull: &[[f64; 2]], push: &[[f64; 2]], lambda: f64| -> Vec<[f64; 2]> {
        free.iter()
            .map(|&p| {
                let scale = 1.0 / (weight[p] + lambda * size[p]).max(1.0);
                [(pull[p][0] - lambda * push[p][0]) * scale, (pull[p][1] - lambda * push[p][1]) * scale]
            })
            .collect()
    };
    let cap = BIN;
    let (mut a, mut lambda, mut over, mut steps) = (1.0f64, 0.0f64, 1.0f64, 0);
    let mut best: (f64, usize, Vec<[f64; 2]>, Vec<usize>) = (f64::MAX, WIRES, Vec::new(), Vec::new());
    let (pull, push, _) = work.forces(free, &v, 3.0, false);
    let mut grad = slope(&pull, &push, 0.0);
    let mut alpha = 0.1 / most(&grad);
    for step in 0..WIRES + MOST {
        let pushing = step >= WIRES;
        if step == WIRES {
            // parts with the same connections lie on one spot, where nothing would push them apart
            for c in v.iter_mut() {
                *c = [c[0] + (rng.unit() - 0.5) * BIN, c[1] + (rng.unit() - 0.5) * BIN];
            }
            u = v.clone();
            a = 1.0;
            let (pull, push, now) = work.forces(free, &v, 5.5, true);
            over = now;
            lambda = if sum(&push) > 0.0 { sum(&pull) / sum(&push) } else { 1.0 };
            grad = slope(&pull, &push, lambda);
            alpha = 0.1 / most(&grad);
        }
        // Enough when little is too full. In a room that is nearly full that may never come, while
        // the growing push goes on lengthening the connections: so also when it has not got
        // better for `WAIT` steps, and then what counts is how the parts lay when it last did.
        if pushing && over < best.0 - 0.005 {
            best = (over, step, u.clone(), work.form.clone());
        }
        if step > WIRES + 20 && (over < problem.spill || step - best.1 >= WAIT) {
            break;
        }
        steps = step + 1;
        let gamma = if pushing { 0.5 + 5.0 * over.min(1.0) } else { 3.0 };
        let lambda_next = if pushing { lambda * GROW } else { 0.0 };
        let next = (1.0 + (4.0 * a * a + 1.0).sqrt()) / 2.0;
        let m = (a - 1.0) / next;
        // The step: as long as the slope turned out to change less over it than over the last. If it
        // changed more, the step was too long for the slope to be trusted that far: again, shorter.
        let mut tries = 0;
        let (u_next, v_next, grad_next, over_next, trust) = loop {
            if !alpha.is_finite() || alpha * most(&grad) > cap {
                alpha = cap / most(&grad);
            }
            let (mut un, mut vn) = (Vec::with_capacity(u.len()), Vec::with_capacity(u.len()));
            for (k, &p) in free.iter().enumerate() {
                work.centre[p] = v[k]; // the part's size is asked for where it is
                let c = work.within(p, [v[k][0] - alpha * grad[k][0], v[k][1] - alpha * grad[k][1]]);
                vn.push(work.within(p, [c[0] + m * (c[0] - u[k][0]), c[1] + m * (c[1] - u[k][1])]));
                un.push(c);
            }
            let (pull, push, now) = work.forces(free, &vn, gamma, pushing);
            let gn = slope(&pull, &push, lambda_next);
            let trust = far(&vn, &v) / far(&gn, &grad).max(1e-12);
            tries += 1;
            if trust >= 0.95 * alpha || tries >= 4 {
                break (un, vn, gn, now, trust);
            }
            alpha = trust;
        };
        (u, v, grad, alpha, a, lambda) = (u_next, v_next, grad_next, trust, next, lambda_next);
        if pushing {
            over = over_next;
        }
        let mut turned = 0;
        if step + 1 == WIRES || (step > WIRES && (step - WIRES).is_multiple_of(TURN)) {
            for (k, &p) in free.iter().enumerate() {
                work.centre[p] = u[k];
            }
            turned = work.turn(free);
        }
        if step + 1 == WIRES || (step >= WIRES && (step - WIRES).is_multiple_of(50)) {
            for (k, &p) in free.iter().enumerate() {
                work.centre[p] = u[k];
            }
            say(format!(
                "step {}: nets {:.0} mm, {:.0} % of the courtyards where it is too full{}",
                step + 1,
                work.length(),
                if step < WIRES { 100.0 } else { 100.0 * over },
                if turned > 0 { format!(", {} parts turned", turned) } else { String::new() }
            ));
        }
    }
    if over >= problem.spill && !best.2.is_empty() {
        (over, u, work.form) = (best.0, best.2, best.3);
    }
    for (k, &p) in free.iter().enumerate() {
        work.centre[p] = u[k];
    }
    work.turn(free);
    say(format!(
        "spread in {} steps: nets {:.0} mm, {:.0} % of the courtyards where it is too full{}",
        steps,
        work.length(),
        100.0 * over,
        if over >= problem.spill { " (it did not get better than that)" } else { "" }
    ));
    let form = work.form;
    for &p in free {
        state.pose[p].form = form[p];
    }
    u
}

/// Set each part in `free` down near where it wants to be, the largest first: of the places where
/// it is allowed, in any rotation on its side, up to `LOOK` mm further away than the nearest, the
/// one at which its connections cost least. The parts must be out of the index; they are in it
/// afterwards. Returns how far they went on average, mm; None (and none of them in the index) if
/// one finds no place at all.
pub(super) fn legalise(state: &mut State, free: &[usize], want: &[[f64; 2]]) -> Option<f64> {
    let mut order: Vec<usize> = (0..free.len()).collect();
    let size = |state: &State, p: usize| {
        let r = state.rect(p);
        (r[2] - r[0]) * (r[3] - r[1])
    };
    order.sort_by(|&a, &b| size(state, free[b]).total_cmp(&size(state, free[a])).then(a.cmp(&b)));
    for (k, &p) in free.iter().enumerate() {
        state.pose[p] = state.pose_at(p, state.pose[p].form, want[k][0], want[k][1]);
    }
    let step = state.problem.grid.max(0.25);
    let rings = (state.problem.size[0].max(state.problem.size[1]) / step).ceil() as i64;
    let look = (LOOK / step).ceil() as i64;
    let (mut nets, mut ties) = (Vec::new(), Vec::new());
    let mut moved = 0.0;
    for (done, &k) in order.iter().enumerate() {
        let (p, c) = (free[k], want[k]);
        let first = state.pose[p].form;
        let mut forms = vec![first];
        forms.extend((0..8).filter(|f| f % 2 == first % 2 && *f != first && state.forms[p][*f].is_some()));
        // (cost, distance, pose) of the best place, and the ring at which the first was found
        let mut found: Option<(f64, f64, Pose)> = None;
        let mut last = rings;
        let mut r = 0;
        while r <= last {
            for (dx, dy) in ring(r) {
                for &form in &forms {
                    let pose = state.pose_at(p, form, c[0] + dx as f64 * step, c[1] + dy as f64 * step);
                    state.pose[p] = pose;
                    if !state.allowed(p) {
                        continue;
                    }
                    let d = ((dx * dx + dy * dy) as f64).sqrt() * step;
                    let cost = state.share(&[p], &mut nets, &mut ties) + 0.01 * d;
                    if found.as_ref().is_none_or(|f| cost < f.0) {
                        found = Some((cost, d, pose));
                    }
                    last = last.min(r + look);
                }
            }
            r += 1;
        }
        match found {
            Some((_, d, pose)) => {
                state.pose[p] = pose;
                state.index(p, true);
                moved += d;
            }
            None => {
                for &j in &order[..done] {
                    state.index(free[j], false);
                }
                return None;
            }
        }
    }
    Some(moved / free.len().max(1) as f64)
}

/// The places at distance r (the larger of |dx| and |dy|) round the middle.
fn ring(r: i64) -> Vec<(i64, i64)> {
    if r == 0 {
        return vec![(0, 0)];
    }
    let mut out = Vec::with_capacity(8 * r as usize);
    for d in -r..=r {
        out.push((d, -r));
        out.push((d, r));
    }
    for d in -r + 1..r {
        out.push((-r, d));
        out.push((r, d));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Charge in the left half of the board: the field points to the right everywhere between,
    /// away from the charge, and not across the board's edge.
    #[test]
    fn the_field_points_away_from_the_charge() {
        let grid = Grid::new([20.0, 10.0]);
        let mut field = Field::new(&grid);
        let mut rho = vec![0.0; grid.nx * grid.ny];
        grid.cover([2.0, 2.0, 6.0, 8.0], |b, a| rho[b] += a);
        field.solve(&rho);
        let at = |x: usize, y: usize| (field.ex[y * grid.nx + x], field.ey[y * grid.nx + x]);
        assert!(
            at(8, 5).0 > 0.0 && at(15, 5).0 > 0.0 && at(0, 5).0 < 0.0,
            "{:?} {:?} {:?}",
            at(8, 5),
            at(15, 5),
            at(0, 5)
        );
        assert!(at(4, 0).1 < 0.0 && at(4, 9).1 > 0.0);
        assert!(at(0, 5).0.abs() < at(1, 5).0.abs() && at(19, 5).0.abs() < at(8, 5).0.abs());
    }

    #[test]
    fn the_slope_pulls_the_outermost_pads_together() {
        let mut out = [0.0; 3];
        slope(&[0.0, 4.0, 10.0], 0.5, &mut out);
        assert!(out[0] < -0.9 && out[2] > 0.9 && out[1].abs() < 0.01, "{:?}", out);
    }
}
