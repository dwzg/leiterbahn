//! The routing problem and its result, as JSON. Lengths are millimetres; `layers` are indices
//! into the routing layers (0 = front), which is all the router knows of the stack.
use serde::{Deserialize, Serialize};

fn d_true() -> bool {
    true
}
fn d_iter() -> u32 {
    200
}
fn d_one() -> f64 {
    1.0
}
fn d_relax() -> u32 {
    12
}
fn d_limit() -> u64 {
    50_000_000
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Problem {
    /// The rectangle to route in: [x0, y0, x1, y1]. Cell (0, 0) is at (x0, y0).
    #[serde(rename = "box")]
    pub area: [f64; 4],
    pub step: f64,
    /// Number of routing layers.
    pub layers: usize,
    pub via: Via,
    /// Copper to a foreign drilled hole.
    #[serde(default)]
    pub hole_clear: f64,
    /// Between the holes of two vias.
    #[serde(default)]
    pub hole_to_hole: f64,
    /// The board's edge as a rectangle, and the copper-free strip along it.
    pub edge: Option<[f64; 4]>,
    #[serde(default)]
    pub edge_clear: f64,
    #[serde(default)]
    pub keepouts: Vec<Keepout>,
    /// Copper that stays: of nets that are not routed here, and of no net.
    #[serde(default)]
    pub fixed: Vec<Shape>,
    pub nets: Vec<Net>,
    #[serde(default = "d_iter")]
    pub max_iter: u32,
    /// Passes over the finished result in which nets are laid again, pushing others aside, where
    /// that makes the weighted sum of lengths smaller.
    #[serde(default = "d_relax")]
    pub relax: u32,
    /// Cells one search may open before it gives up.
    #[serde(default = "d_limit")]
    pub limit: u64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Via {
    pub d: f64,
    pub hole: f64,
    /// What a via costs, in millimetres of track.
    pub cost: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Keepout {
    #[serde(rename = "box")]
    pub area: [f64; 4],
    pub layers: Vec<usize>,
    #[serde(default = "d_true")]
    pub tracks: bool,
    #[serde(default = "d_true")]
    pub vias: bool,
}

/// A piece of copper: a rectangle (`box`, a pad) or a line with round ends (`seg` and `r`;
/// a via is a line of no length).
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Shape {
    #[serde(rename = "box")]
    pub rect: Option<[f64; 4]>,
    pub seg: Option<[f64; 4]>,
    #[serde(default)]
    pub r: f64,
    /// Routing layers it has copper on.
    pub layers: Vec<usize>,
    /// Radius of its drilled hole, 0 for none.
    #[serde(default)]
    pub hole: f64,
    /// The clearance its net asks for (fixed copper only; a routed net's is the net's).
    #[serde(default)]
    pub clearance: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Net {
    pub name: String,
    pub width: f64,
    /// A connection that finds no way at `width`, even across other nets, is laid this wide.
    pub narrow: Option<f64>,
    pub clearance: f64,
    /// How much a short way matters for this net: once all nets are laid, a net is given a
    /// shorter way at the cost of others if the sum of lengths times weights gets smaller.
    #[serde(default = "d_one")]
    pub weight: f64,
    /// What is to be joined: each piece is copper that is one already (a pad, or pads with
    /// the tracks between them).
    pub pieces: Vec<Vec<Shape>>,
}

#[derive(Serialize, Debug, Default)]
pub struct Solution {
    pub nets: Vec<Routed>,
    pub iterations: u32,
    /// Cells shared by two nets after each iteration.
    pub conflicts: Vec<u64>,
    /// Nets that got shorter in each pass over the finished result.
    pub shortened: Vec<u32>,
    pub expanded: u64,
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct Routed {
    pub name: String,
    pub paths: Vec<Path>,
    /// Pieces that were not joined to the first one.
    pub open: Vec<usize>,
    /// What its tracks cost, in millimetres of track (a via counts as its cost, a narrow track
    /// half as much again), and what they would cost with the net alone on the board.
    pub cost: f64,
    pub alone: f64,
}

/// One track: the cells of its bends and layer changes, [layer, i, j]. `from` and `to` name the
/// copper it starts and ends on as [piece, shape]; none: it starts on an earlier path of the net.
#[derive(Serialize, Debug, Default, Clone)]
pub struct Path {
    pub cells: Vec<[u32; 3]>,
    pub width: f64,
    pub from: Option<[usize; 2]>,
    pub to: Option<[usize; 2]>,
}
