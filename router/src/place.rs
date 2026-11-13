//! Placement of parts by simulated annealing.
//!
//! Starts from a placement that is allowed (the one a constructive placer made, or with `spread`
//! what forces made of it: see `place/spread.rs`) and changes it
//! move by move: a part is shifted, turned, put on the other side, or changes places with
//! another. A move that breaks a rule is not made. A move that makes the cost smaller is kept;
//! one that makes it larger is kept with a probability that falls as the run goes on, so the
//! placement can leave a poor arrangement early and only settles at the end.
//!
//! The cost, in millimetres:
//!   - per net, its weight times the half perimeter of the box around its pads;
//!   - per tether (a capacitor's pad to the supply pin it serves), weight times distance;
//!   - per part, `pull` times the distance of its middle from its anchor (blocks stay together);
//!   - `back_cost` per part on the back;
//!   - `crowd` times the square of the pads too many, per square of `BIN` mm on each side. Too
//!     many is more than `dense`, by default what one square in ten held at the start: the
//!     placement that came in could be routed, so nothing should get much fuller than its
//!     fuller places. Without this, two of four results on a board of 287 parts left
//!     connections without a way.
//!
//! What it does not see: whether the result can be routed. On a board with 158 nets the half
//! perimeter followed the routed length of a net alone on the board closely (correlation 0.95
//! over 20 placements) and the routed result with all nets only loosely (0.6 to 0.7). So a
//! result of this is a candidate; route it to judge it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod spread;

/// A part's courtyard and pads at one rotation on one side, relative to its origin.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Shape {
    #[serde(rename = "box")]
    pub rect: [f64; 4],
    pub pads: HashMap<String, [f64; 2]>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Part {
    pub name: String,
    /// Origin, rotation in degrees, on the back.
    pub at: (f64, f64, u32, bool),
    /// Stays where it is.
    #[serde(default)]
    pub fixed: bool,
    /// Has pads through the board: in the way on both sides.
    #[serde(default)]
    pub through: bool,
    /// Its shape per rotation and side, keyed "90/F" or "0/B". It may only be turned or put on
    /// a side for which there is a shape.
    pub shapes: HashMap<String, Shape>,
    /// Its courtyard must lie within one rectangle of each group.
    #[serde(default)]
    pub inside: Vec<Vec<[f64; 4]>>,
    /// Rectangles its courtyard must not reach into on the sides named: a `Keep` for this part alone.
    #[serde(default)]
    pub outside: Vec<Keep>,
    /// Pads that must lie within one of the rectangles given with them: (pad, rectangles). For a
    /// pad on a plane that covers only part of the board.
    #[serde(default)]
    pub over: Vec<(String, Vec<[f64; 4]>)>,
    #[serde(default)]
    pub anchor: Option<[f64; 2]>,
    #[serde(default)]
    pub pull: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Net {
    pub name: String,
    /// (part index, pad name)
    pub pins: Vec<(usize, String)>,
    #[serde(default = "one")]
    pub weight: f64,
}

/// A pad that should be near another: (part, pad) to (part, pad).
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Tether {
    pub from: (usize, String),
    pub to: (usize, String),
    #[serde(default = "one")]
    pub weight: f64,
}

/// A rectangle no courtyard may reach into, on the sides named ("F", "B" or "FB").
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Keep {
    #[serde(rename = "box")]
    pub rect: [f64; 4],
    pub sides: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Problem {
    /// Width and height of the board; parts keep `edge` from its sides and `gap` from each other.
    pub size: [f64; 2],
    #[serde(default)]
    pub edge: f64,
    #[serde(default)]
    pub gap: f64,
    #[serde(default)]
    pub back_cost: f64,
    #[serde(default)]
    pub keeps: Vec<Keep>,
    pub parts: Vec<Part>,
    pub nets: Vec<Net>,
    #[serde(default)]
    pub tethers: Vec<Tether>,
    /// Moves tried per part that may move.
    #[serde(default = "d_moves")]
    pub moves: u64,
    #[serde(default = "d_seed")]
    pub seed: u64,
    /// Origins lie on multiples of this.
    #[serde(default = "d_grid")]
    pub grid: f64,
    /// How readily a worse placement is kept, in mm of cost, at the start and at the end.
    #[serde(default = "d_warm")]
    pub warm: f64,
    #[serde(default = "d_cold")]
    pub cold: f64,
    /// Pads a square of `BIN` mm may hold on one side before it costs; None: see above.
    #[serde(default)]
    pub dense: Option<f64>,
    /// What a pad too many costs, in mm (times the number too many).
    #[serde(default = "one")]
    pub crowd: f64,
    /// Begin from where forces put the parts (see `place/spread.rs`), not from where they are.
    #[serde(default)]
    pub spread: bool,
    /// For that: the share of the courtyard area that may still lie on other courtyards when the
    /// forces stop. The rest is the work of the steps after: at 0.05 the forces had driven the
    /// parts so far apart that the result was worse than without them, at 0.2 to 0.3 best.
    #[serde(default = "d_spill")]
    pub spill: f64,
    /// How far a part is shifted at most at the start of the annealing, mm; None: a quarter of the board.
    #[serde(default)]
    pub reach: Option<f64>,
    /// Placements annealed side by side, each on a processor of its own, the first as warm as the
    /// schedule says and the others warmer by even steps up to `hottest` times. Every so often two
    /// neighbours change places, readily if the warmer one has the cheaper placement: so what a warm
    /// one finds by leaping about is settled by a cool one, and nothing is lost when a cool one is
    /// stuck. 1: one placement, as it always was.
    #[serde(default = "d_chains")]
    pub chains: usize,
    #[serde(default = "d_hottest")]
    pub hottest: f64,
}

fn d_chains() -> usize {
    1
}
fn d_hottest() -> f64 {
    8.0
}

fn d_spill() -> f64 {
    0.2
}
fn one() -> f64 {
    1.0
}
fn d_moves() -> u64 {
    20_000
}
fn d_seed() -> u64 {
    1
}
fn d_grid() -> f64 {
    0.05
}
fn d_warm() -> f64 {
    1.0
}
fn d_cold() -> f64 {
    0.01
}

/// What the cost is made of, in mm.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Cost {
    pub nets: f64,
    pub tethers: f64,
    pub pull: f64,
    pub back: f64,
    pub crowd: f64,
}

impl Cost {
    pub fn total(&self) -> f64 {
        self.nets + self.tethers + self.pull + self.back + self.crowd
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Placed {
    /// Every part that may move: name, origin, rotation, on the back.
    pub parts: Vec<(String, f64, f64, u32, bool)>,
    pub before: Cost,
    /// What the annealing began from: as before, or what the forces made of it.
    pub started: Cost,
    pub after: Cost,
    pub tried: u64,
    pub kept: u64,
    /// Parts that are somewhere else than before.
    pub moved: usize,
}

const ROTS: [u32; 4] = [0, 90, 180, 270];
const CELL: f64 = 2.5;
/// Side of the squares in which pads are counted, mm.
pub const BIN: f64 = 2.0;

/// One shape of a part with its pads by number.
#[derive(Clone)]
struct Form {
    rect: [f64; 4],
    pads: Vec<[f64; 2]>,
}

/// A pad: (part, pad number).
type Pin = (usize, usize);

#[derive(Clone, Copy, PartialEq)]
struct Pose {
    x: f64,
    y: f64,
    /// Index into the part's forms: rotation * 2 + back.
    form: usize,
}

#[derive(Clone)]
struct State<'a> {
    problem: &'a Problem,
    forms: Vec<Vec<Option<Form>>>,
    pose: Vec<Pose>,
    /// Per net: (part, pad number). Per part: its nets and tethers.
    pins: Vec<Vec<(usize, usize)>>,
    of_part: Vec<Vec<usize>>,
    tethers: Vec<(Pin, Pin, f64)>,
    tied: Vec<Vec<usize>>,
    /// Per part: (pad number, the rectangles of which one must hold it).
    over: Vec<Vec<(usize, Vec<[f64; 4]>)>>,
    /// Which parts' courtyards reach into a cell, per side.
    cells: Vec<Vec<u32>>,
    nx: usize,
    ny: usize,
    /// Pads per square and side, how many a square may hold, and the squares of the excess summed.
    pads: Vec<i32>,
    bx: usize,
    by: usize,
    dense: f64,
    excess: f64,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// In [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.unit() * n as f64) as usize
    }
}

impl State<'_> {
    fn rect(&self, p: usize) -> [f64; 4] {
        let pose = &self.pose[p];
        let r = &self.forms[p][pose.form].as_ref().unwrap().rect;
        [pose.x + r[0], pose.y + r[1], pose.x + r[2], pose.y + r[3]]
    }

    fn pad(&self, p: usize, pad: usize) -> [f64; 2] {
        let pose = &self.pose[p];
        let d = &self.forms[p][pose.form].as_ref().unwrap().pads[pad];
        [pose.x + d[0], pose.y + d[1]]
    }

    fn sides(&self, p: usize) -> [bool; 2] {
        let back = self.pose[p].form % 2 == 1;
        if self.problem.parts[p].through {
            [true, true]
        } else {
            [!back, back]
        }
    }

    fn span(&self, r: &[f64; 4]) -> (usize, usize, usize, usize) {
        let g = self.problem.gap;
        let cell = |v: f64, n: usize| ((v / CELL).floor().max(0.0) as usize).min(n - 1);
        (
            cell(r[0] - g, self.nx),
            cell(r[1] - g, self.ny),
            cell(r[2] + g, self.nx),
            cell(r[3] + g, self.ny),
        )
    }

    fn index(&mut self, p: usize, add: bool) {
        let (r, sides) = (self.rect(p), self.sides(p));
        let (i0, j0, i1, j1) = self.span(&r);
        for (side, _) in sides.iter().enumerate().filter(|s| *s.1) {
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let cell = &mut self.cells[(side * self.ny + j) * self.nx + i];
                    if add {
                        cell.push(p as u32);
                    } else if let Some(k) = cell.iter().position(|&q| q == p as u32) {
                        cell.swap_remove(k);
                    }
                }
            }
        }
    }

    /// Count the part's pads into the squares they lie in (or out of them).
    fn count(&mut self, p: usize, delta: i32) {
        let (pose, sides) = (self.pose[p], self.sides(p));
        let form = self.forms[p][pose.form].as_ref().unwrap();
        for d in &form.pads {
            let i = (((pose.x + d[0]) / BIN).floor().max(0.0) as usize).min(self.bx - 1);
            let j = (((pose.y + d[1]) / BIN).floor().max(0.0) as usize).min(self.by - 1);
            for (side, _) in sides.iter().enumerate().filter(|s| *s.1) {
                let k = (side * self.by + j) * self.bx + i;
                let over = |c: i32, dense: f64| (c as f64 - dense).max(0.0).powi(2);
                self.excess -= over(self.pads[k], self.dense);
                self.pads[k] += delta;
                self.excess += over(self.pads[k], self.dense);
            }
        }
    }

    /// May part p be where it is? It must be out of the index while this is asked.
    fn allowed(&self, p: usize) -> bool {
        let (problem, r, sides) = (self.problem, self.rect(p), self.sides(p));
        let e = problem.edge - 1e-9;
        if r[0] < e || r[1] < e || r[2] > problem.size[0] - e || r[3] > problem.size[1] - e {
            return false;
        }
        let within = |b: &[f64; 4]| r[0] >= b[0] - 1e-9 && r[1] >= b[1] - 1e-9 && r[2] <= b[2] + 1e-9 && r[3] <= b[3] + 1e-9;
        if !problem.parts[p].inside.iter().all(|group| group.iter().any(within)) {
            return false;
        }
        for (pad, rects) in &self.over[p] {
            let q = self.pad(p, *pad);
            if !rects.iter().any(|b| q[0] >= b[0] && q[1] >= b[1] && q[0] <= b[2] && q[1] <= b[3]) {
                return false;
            }
        }
        for keep in problem.keeps.iter().chain(&problem.parts[p].outside) {
            let b = &keep.rect;
            let on = (sides[0] && keep.sides.contains('F')) || (sides[1] && keep.sides.contains('B'));
            if on && r[0] < b[2] && b[0] < r[2] && r[1] < b[3] && b[1] < r[3] {
                return false;
            }
        }
        let g = problem.gap - 1e-9;
        let (i0, j0, i1, j1) = self.span(&r);
        for (side, _) in sides.iter().enumerate().filter(|s| *s.1) {
            for j in j0..=j1 {
                for i in i0..=i1 {
                    for &q in &self.cells[(side * self.ny + j) * self.nx + i] {
                        let o = self.rect(q as usize);
                        if q as usize != p && r[0] < o[2] + g && o[0] < r[2] + g && r[1] < o[3] + g && o[1] < r[3] + g {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    fn net_cost(&self, n: usize) -> f64 {
        let pins = &self.pins[n];
        if pins.len() < 2 {
            return 0.0;
        }
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for &(p, pad) in pins {
            let q = self.pad(p, pad);
            b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])];
        }
        self.problem.nets[n].weight * (b[2] - b[0] + b[3] - b[1])
    }

    fn tether_cost(&self, t: usize) -> f64 {
        let (a, b, w) = self.tethers[t];
        let (p, q) = (self.pad(a.0, a.1), self.pad(b.0, b.1));
        w * ((p[0] - q[0]).abs() + (p[1] - q[1]).abs())
    }

    /// What the part costs by itself: (pull to its anchor, being on the back).
    fn own_cost(&self, p: usize) -> (f64, f64) {
        let part = &self.problem.parts[p];
        let r = self.rect(p);
        let pull = part
            .anchor
            .map_or(0.0, |a| part.pull * (((r[0] + r[2]) / 2.0 - a[0]).abs() + ((r[1] + r[3]) / 2.0 - a[1]).abs()));
        (pull, if self.pose[p].form % 2 == 1 { self.problem.back_cost } else { 0.0 })
    }

    fn cost(&self) -> Cost {
        let mut c = Cost {
            nets: (0..self.pins.len()).map(|n| self.net_cost(n)).sum(),
            ..Cost::default()
        };
        c.tethers = (0..self.tethers.len()).map(|t| self.tether_cost(t)).sum();
        for p in 0..self.pose.len() {
            let (pull, back) = self.own_cost(p);
            c.pull += pull;
            c.back += back;
        }
        c.crowd = self.problem.crowd * self.excess;
        c
    }

    /// The cost of everything the parts in `who` have a share in.
    fn share(&self, who: &[usize], nets: &mut Vec<usize>, ties: &mut Vec<usize>) -> f64 {
        nets.clear();
        ties.clear();
        let mut total = 0.0;
        for &p in who {
            nets.extend(&self.of_part[p]);
            ties.extend(&self.tied[p]);
            let (pull, back) = self.own_cost(p);
            total += pull + back;
        }
        nets.sort_unstable();
        nets.dedup();
        ties.sort_unstable();
        ties.dedup();
        total + nets.iter().map(|&n| self.net_cost(n)).sum::<f64>() + ties.iter().map(|&t| self.tether_cost(t)).sum::<f64>()
    }

    /// Put the part's courtyard middle at (cx, cy) in this form, its origin on the grid.
    fn pose_at(&self, p: usize, form: usize, cx: f64, cy: f64) -> Pose {
        let r = &self.forms[p][form].as_ref().unwrap().rect;
        let g = self.problem.grid;
        let snap = |v: f64| (v / g).round() * g;
        Pose {
            x: snap(cx - (r[0] + r[2]) / 2.0),
            y: snap(cy - (r[1] + r[3]) / 2.0),
            form,
        }
    }
}

/// Moves `from` to `to` of the `total` of an annealing run (`span`), `hot` times as warm as the
/// schedule says. `tally`: moves tried and kept.
fn sweep(state: &mut State, rng: &mut Rng, free: &[usize], span: [u64; 3], hot: f64, tally: &mut (u64, u64)) {
    let [from, to, total] = span;
    let problem = state.problem;
    let (mut nets, mut ties) = (Vec::new(), Vec::new());
    let reach = problem.reach.unwrap_or(problem.size[0].max(problem.size[1]) / 4.0);
    for k in from..to {
        let progress = k as f64 / total as f64;
        let warmth = hot * problem.warm * (problem.cold / problem.warm).powf(progress);
        let radius = (reach * (1.0 - progress).powi(2)).max(4.0 * problem.grid);
        let a = free[rng.below(free.len())];
        let kind = rng.unit();
        let mut who = vec![a];
        let ra = state.rect(a);
        let (cx, cy) = ((ra[0] + ra[2]) / 2.0, (ra[1] + ra[3]) / 2.0);
        let old_form = state.pose[a].form;
        let mut next = Vec::new();
        if kind < 0.6 {
            // a shift
            let (dx, dy) = ((rng.unit() * 2.0 - 1.0) * radius, (rng.unit() * 2.0 - 1.0) * radius);
            next.push(state.pose_at(a, old_form, cx + dx, cy + dy));
        } else if kind < 0.8 {
            // another rotation or side, about its middle
            let form = rng.below(8);
            if form == old_form || state.forms[a][form].is_none() {
                continue;
            }
            next.push(state.pose_at(a, form, cx, cy));
        } else {
            // change places with another part, each keeping its rotation and taking the other's side
            let b = free[rng.below(free.len())];
            let rb = state.rect(b);
            let (bx, by) = ((rb[0] + rb[2]) / 2.0, (rb[1] + rb[3]) / 2.0);
            let (fa, fb) = (old_form / 2 * 2 + state.pose[b].form % 2, state.pose[b].form / 2 * 2 + old_form % 2);
            if b == a || state.forms[a][fa].is_none() || state.forms[b][fb].is_none() || (cx - bx).abs().max((cy - by).abs()) > 2.0 * radius + 3.0 {
                continue;
            }
            next.push(state.pose_at(a, fa, bx, by));
            next.push(state.pose_at(b, fb, cx, cy));
            who.push(b);
        }
        tally.0 += 1;
        let was = state.share(&who, &mut nets, &mut ties) + problem.crowd * state.excess;
        let old: Vec<Pose> = who.iter().map(|&p| state.pose[p]).collect();
        for &p in &who {
            state.index(p, false);
            state.count(p, -1);
        }
        for (&p, &pose) in who.iter().zip(&next) {
            state.pose[p] = pose;
        }
        // each against what lies there, and against each other: the first goes in before the second is asked
        let mut fine = true;
        let mut put = 0;
        for &p in &who {
            fine = state.allowed(p);
            if !fine {
                break;
            }
            state.index(p, true);
            put += 1;
        }
        for &p in &who {
            state.count(p, 1);
        }
        let delta = if fine {
            state.share(&who, &mut nets, &mut ties) + problem.crowd * state.excess - was
        } else {
            0.0
        };
        if fine && (delta <= 0.0 || rng.unit() < (-delta / warmth).exp()) {
            tally.1 += 1;
        } else {
            for &p in &who[..put] {
                state.index(p, false);
            }
            for &p in &who {
                state.count(p, -1);
            }
            for (&p, &pose) in who.iter().zip(&old) {
                state.pose[p] = pose;
            }
            for &p in &who {
                state.index(p, true);
                state.count(p, 1);
            }
        }
    }
}

/// Improve the placement. `say` gets a line now and then.
pub fn place(problem: &Problem, say: &mut dyn FnMut(String)) -> Result<Placed, String> {
    let n = problem.parts.len();
    let mut pad_ids: Vec<HashMap<&str, usize>> = vec![HashMap::new(); n];
    for (p, part) in problem.parts.iter().enumerate() {
        let mut names: Vec<&str> = part.shapes.values().flat_map(|s| s.pads.keys().map(|k| k.as_str())).collect();
        names.sort_unstable();
        names.dedup();
        pad_ids[p] = names.into_iter().enumerate().map(|(i, name)| (name, i)).collect();
    }
    let mut forms: Vec<Vec<Option<Form>>> = Vec::new();
    let mut pose = Vec::new();
    for (p, part) in problem.parts.iter().enumerate() {
        let mut mine: Vec<Option<Form>> = vec![None; 8];
        for (key, shape) in &part.shapes {
            let (rot, side) = key.split_once('/').ok_or(format!("{}: shape {}: write 90/F", part.name, key))?;
            let r = ROTS
                .iter()
                .position(|v| v.to_string() == rot)
                .ok_or(format!("{}: rotation {}", part.name, rot))?;
            let mut pads = vec![[f64::NAN; 2]; pad_ids[p].len()];
            for (name, at) in &shape.pads {
                pads[pad_ids[p][name.as_str()]] = *at;
            }
            if pads.iter().any(|q| q[0].is_nan()) {
                return Err(format!("{}: shape {} lacks a pad that another shape has", part.name, key));
            }
            mine[r * 2 + (side == "B") as usize] = Some(Form { rect: shape.rect, pads });
        }
        let r = ROTS
            .iter()
            .position(|&v| v == part.at.2 % 360)
            .ok_or(format!("{}: rotation {}", part.name, part.at.2))?;
        let form = r * 2 + part.at.3 as usize;
        if mine[form].is_none() {
            return Err(format!(
                "{}: no shape for where it is ({}/{})",
                part.name,
                part.at.2,
                if part.at.3 { "B" } else { "F" }
            ));
        }
        forms.push(mine);
        pose.push(Pose {
            x: part.at.0,
            y: part.at.1,
            form,
        });
    }
    let pin = |(p, pad): &(usize, String)| -> Result<(usize, usize), String> {
        let part = problem.parts.get(*p).ok_or(format!("no part {}", p))?;
        Ok((*p, *pad_ids[*p].get(pad.as_str()).ok_or(format!("{} has no pad {}", part.name, pad))?))
    };
    let mut pins = Vec::new();
    let mut of_part = vec![Vec::new(); n];
    for (k, net) in problem.nets.iter().enumerate() {
        let mine: Vec<(usize, usize)> = net.pins.iter().map(pin).collect::<Result<_, _>>()?;
        for &(p, _) in &mine {
            of_part[p].push(k);
        }
        pins.push(mine);
    }
    let mut over = vec![Vec::new(); n];
    for (p, part) in problem.parts.iter().enumerate() {
        for (pad, rects) in &part.over {
            let id = *pad_ids[p].get(pad.as_str()).ok_or(format!("{} has no pad {}", part.name, pad))?;
            over[p].push((id, rects.clone()));
        }
    }
    let mut tethers = Vec::new();
    let mut tied = vec![Vec::new(); n];
    for (k, t) in problem.tethers.iter().enumerate() {
        let (a, b) = (pin(&t.from)?, pin(&t.to)?);
        tied[a.0].push(k);
        tied[b.0].push(k);
        tethers.push((a, b, t.weight));
    }
    let nx = (problem.size[0] / CELL).ceil().max(1.0) as usize;
    let ny = (problem.size[1] / CELL).ceil().max(1.0) as usize;
    let bx = (problem.size[0] / BIN).ceil().max(1.0) as usize;
    let by = (problem.size[1] / BIN).ceil().max(1.0) as usize;
    let cells = vec![Vec::new(); 2 * nx * ny];
    let pads = vec![0; 2 * bx * by];
    let mut state = State {
        problem,
        forms,
        pose,
        pins,
        of_part,
        tethers,
        tied,
        over,
        cells,
        nx,
        ny,
        pads,
        bx,
        by,
        dense: f64::MAX,
        excess: 0.0,
    };
    for p in 0..n {
        state.index(p, true);
        state.count(p, 1);
    }
    // what one square in ten holds now (of those that hold anything) is what a square may hold
    let mut held: Vec<i32> = state.pads.iter().copied().filter(|&c| c > 0).collect();
    held.sort_unstable();
    state.dense = problem.dense.unwrap_or(held.get(held.len() * 9 / 10).copied().unwrap_or(0) as f64);
    state.excess = state.pads.iter().map(|&c| (c as f64 - state.dense).max(0.0).powi(2)).sum();
    say(format!(
        "a square of {} mm may hold {:.0} pads on a side; the fullest holds {}",
        BIN,
        state.dense,
        held.last().copied().unwrap_or(0)
    ));
    let start = state.pose.clone();
    let before = state.cost();
    let free: Vec<usize> = (0..n).filter(|&p| !problem.parts[p].fixed).collect();
    let mut result = Placed {
        before,
        started: before,
        after: before,
        ..Placed::default()
    };
    let mut rng = Rng(problem.seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    if problem.spread && !free.is_empty() {
        for &p in &free {
            state.index(p, false);
            state.count(p, -1);
        }
        let want = spread::spread(&mut state, &free, &mut rng, say);
        match spread::legalise(&mut state, &free, &want) {
            Some(moved) => say(format!(
                "every part set down where it is allowed, {:.2} mm from where it wanted to be on average",
                moved
            )),
            None => {
                say("a part found no place where it is allowed: the placement stays as it came".to_string());
                for &p in &free {
                    state.pose[p] = start[p];
                    state.index(p, true);
                }
            }
        }
        for &p in &free {
            state.count(p, 1);
        }
        result.started = state.cost();
        say(format!("cost {:.0} mm as it came, {:.0} mm now", before.total(), result.started.total()));
    }
    if !free.is_empty() {
        let total = problem.moves * free.len() as u64;
        let reach = problem.reach.unwrap_or(problem.size[0].max(problem.size[1]) / 4.0);
        let tenth = (total / 10).max(1);
        let mut tally = (0u64, 0u64);
        let report = |say: &mut dyn FnMut(String), done: u64, cost: f64| {
            let radius = (reach * (1.0 - (done - 1) as f64 / total as f64).powi(2)).max(4.0 * problem.grid);
            say(format!(
                "{:3.0} %: cost {:.0} mm, parts move within {:.1} mm",
                done as f64 * 100.0 / total as f64,
                cost,
                radius
            ));
        };
        if problem.chains <= 1 {
            let mut from = 0;
            while from < total {
                let to = ((from / tenth + 1) * tenth).min(total);
                sweep(&mut state, &mut rng, &free, [from, to, total], 1.0, &mut tally);
                if to.is_multiple_of(tenth) {
                    report(say, to, state.cost().total());
                }
                from = to;
            }
        } else {
            // slot 0 is the coolest; a placement moves from slot to slot
            let chains = problem.chains;
            let hot: Vec<f64> = (0..chains).map(|c| problem.hottest.powf(c as f64 / (chains - 1) as f64)).collect();
            let mut slots: Vec<(State, Rng, (u64, u64))> = (0..chains)
                .map(|c| {
                    (
                        state.clone(),
                        Rng((problem.seed.wrapping_add(c as u64 * 7919)).wrapping_mul(0x9E3779B97F4A7C15) | 1),
                        (0, 0),
                    )
                })
                .collect();
            let turn = (total / 200).max(1); // (50 to 5000 turns made no difference to the result)
            let (mut from, mut round, mut swaps) = (0, 0u64, 0u64);
            while from < total {
                let to = (from + turn).min(total);
                let free = &free;
                std::thread::scope(|scope| {
                    for (slot, &heat) in slots.iter_mut().zip(&hot) {
                        scope.spawn(move || sweep(&mut slot.0, &mut slot.1, free, [from, to, total], heat, &mut slot.2));
                    }
                });
                let warmth = problem.warm * (problem.cold / problem.warm).powf(to as f64 / total as f64);
                let costs: Vec<f64> = slots.iter().map(|slot| slot.0.cost().total()).collect();
                let mut costs = costs;
                for c in ((round % 2) as usize..chains - 1).step_by(2) {
                    // the cooler one takes the warmer one's placement: always if that is the cheaper
                    let gain = (costs[c] - costs[c + 1]) * (1.0 / (warmth * hot[c]) - 1.0 / (warmth * hot[c + 1]));
                    if gain >= 0.0 || rng.unit() < gain.exp() {
                        let (a, b) = slots.split_at_mut(c + 1);
                        std::mem::swap(&mut a[c].0, &mut b[0].0);
                        costs.swap(c, c + 1);
                        swaps += 1;
                    }
                }
                if to / tenth > from / tenth {
                    report(say, to, costs[0]);
                }
                (from, round) = (to, round + 1);
            }
            let best = (0..chains)
                .min_by(|&a, &b| slots[a].0.cost().total().total_cmp(&slots[b].0.cost().total()))
                .unwrap();
            say(format!(
                "{} placements side by side changed places {} times; the best was in slot {}",
                chains, swaps, best
            ));
            tally = slots.iter().fold((0, 0), |t, slot| (t.0 + slot.2 .0, t.1 + slot.2 .1));
            state = slots.swap_remove(best).0;
        }
        (result.tried, result.kept) = tally;
    }
    result.after = state.cost();
    for &p in &free {
        let pose = state.pose[p];
        result.moved += (pose != start[p]) as usize;
        result
            .parts
            .push((problem.parts[p].name.clone(), pose.x, pose.y, ROTS[pose.form / 2], pose.form % 2 == 1));
    }
    Ok(result)
}
