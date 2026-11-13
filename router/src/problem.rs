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
fn d_stall() -> u32 {
    20
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
    /// Places tracks keep `Shape::room` away from on these layers where they find a way that is
    /// not more than `shy` times longer: a through-hole pad that hangs in a pour by its spokes
    /// (a track led round it at clearance left it one spoke, and DRC calls that starved).
    #[serde(default)]
    pub rooms: Vec<Shape>,
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
    /// The rounds end when the conflicts have not got fewer for this many of them.
    #[serde(default = "d_stall")]
    pub stall: u32,
    /// Lay no tracks: only say, per net, which pieces have no way to the first with the net alone on
    /// the board (`open`), and what its tracks would cost (`alone`). Seconds for a board.
    #[serde(default)]
    pub alone: bool,
    /// Tracks of quiet nets keep this far from the copper of noisy nets, and the other way round,
    /// where they find a way that is not more than `shy` times longer for it. 0: no such wish.
    #[serde(default)]
    pub apart: f64,
    /// A net that is in another's way is laid again only where it is, see `Router::relay`.
    #[serde(default = "d_true")]
    pub relay: bool,
    /// Only look: end after `max_iter` rounds and say what is still in conflict and where
    /// (`Solution::conflicts`, `Solution::stuck`), without a result that could be drawn. What is
    /// left after 8 rounds tells how the board will end: of ten placements of one board, those
    /// that came clean had 88 to 132 cells in conflict then, those that kept connections open
    /// 186 to 323.
    #[serde(default)]
    pub glance: bool,
    /// Another run of the same problem: with a seed other than 0 the nets are laid in a slightly
    /// other order (a net's reach counts as 0.75 to 1.25 times what it is). Where the rounds
    /// are a close thing, any change of order ends them elsewhere: eight placements of a board
    /// took 11 to 172 rounds, and with one tie broken otherwise 11 to 200, the same placement
    /// 164 or 77. Several seeds side by side, and the first to come clean wins (negotiate(race=)).
    #[serde(default)]
    pub seed: u64,
    /// Settle nets that go on changing places by trying their orders: see untangle.rs.
    #[serde(default = "d_true")]
    pub untangle: bool,
    /// Straighten the finished tracks: see smooth.rs.
    #[serde(default = "d_true")]
    pub straight: bool,
    #[serde(default = "d_shy")]
    pub shy: f64,
}

fn d_shy() -> f64 {
    3.0
}

/// What a net is to others: `quiet` nets (a feedback node, a sense line) and `noisy` ones (a
/// switch node, a gate, a clock) are kept apart, see `Problem::apart`.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[default]
    None,
    Quiet,
    Noisy,
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
    /// What its net is to others (fixed copper only; a routed net's is the net's).
    #[serde(default)]
    pub role: Role,
    /// In `Problem::rooms`: the distance tracks keep from it, mm.
    #[serde(default)]
    pub room: f64,
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
    #[serde(default)]
    pub role: Role,
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
    /// With `Problem::glance`: the nets that lie in another's way when the rounds end.
    pub stuck: Vec<Stuck>,
    /// With `Problem::glance`: where the nets were in each other's way in any of the rounds.
    pub heat: Vec<Heat>,
}

/// A square of the board, 1 mm wide, on one layer, and how often cells of it were used by two
/// nets at once: summed over the cells and the rounds.
#[derive(Serialize, Debug, Default, Clone)]
pub struct Heat {
    pub x: f64,
    pub y: f64,
    pub layer: usize,
    pub heat: f64,
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct Stuck {
    pub name: String,
    /// Cells of its tracks that are too near another net's.
    pub cells: u64,
    /// The box round those cells, in mm.
    pub area: [f64; 4],
    pub layers: Vec<usize>,
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
    /// Millimetres of its tracks nearer than `Problem::apart` to copper it is to keep away from.
    pub beside: f64,
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
