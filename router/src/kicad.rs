//! A KiCad board file (.kicad_pcb, with the rules in the .kicad_pro beside it) as a routing
//! problem, and the router's answer written back into the file. No KiCad is needed for either.
//!
//! What is joined already is found here, not asked of KiCad: copper of one net that overlaps on
//! a layer, and copper that lies on a filled pour of its net.
use crate::problem::{Keepout, Net, Problem, Role, Shape, Solution, Via};
use crate::sexp::{parse, Sexp};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Pad {
    pub reference: String,
    pub number: String,
    pub net: String,
    pub at: [f64; 2],
    /// Bounding box of its copper.
    pub rect: [f64; 4],
    /// For a plain pad that lies at an angle: its copper as a box about its middle, and the
    /// degrees that box is turned by. (As its bounding box, a pad of a part at 45 degrees is
    /// nearly three times as wide as it is, and the pins of such a part shut each other in: a
    /// board with 39 of its 94 parts at 45 degrees had a third of its connections left open.)
    pub turned: Option<([f64; 4], f64)>,
    /// A clearance of its own, given for the pad or for its footprint: a mounting hole that
    /// asks for 1.7 mm round it.
    pub clearance: Option<f64>,
    /// Copper layers it is on, as indices into `Board::copper`.
    pub layers: Vec<usize>,
    /// Radius of the drilled hole, 0 for none.
    pub hole: f64,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub width: f64,
    pub layer: usize,
    pub net: String,
    pub locked: bool,
}

#[derive(Debug, Clone)]
pub struct Hole {
    pub at: [f64; 2],
    pub size: f64,
    pub drill: f64,
    pub net: String,
    pub locked: bool,
}

#[derive(Debug, Clone)]
pub struct Zone {
    pub net: String,
    pub layers: Vec<usize>,
    /// A rule area: (no tracks, no vias).
    pub keepout: Option<(bool, bool)>,
    /// Bounding box of its outline.
    pub bounds: [f64; 4],
    /// The filled copper: (layer, outline of one island).
    pub fills: Vec<(usize, Vec<[f64; 2]>)>,
}

#[derive(Debug, Clone, Default)]
pub struct Board {
    /// Copper layers front to back: (name, routed on). Layers of type `power` are planes.
    pub copper: Vec<(String, bool)>,
    pub pads: Vec<Pad>,
    pub segments: Vec<Segment>,
    pub vias: Vec<Hole>,
    pub zones: Vec<Zone>,
    /// Text and drawings on copper layers: copper of no net, in the way of every track.
    pub drawn: Vec<Drawn>,
    /// The lines of the board's outline and of every cut-out in it (`layer` is not used).
    pub cuts: Vec<Drawn>,
    /// Bounding box of the outline's centre lines.
    pub edge: Option<[f64; 4]>,
}

/// A piece of a drawing or a text on a copper layer: a box, or a line with half its width.
#[derive(Debug, Clone)]
pub struct Drawn {
    pub layer: usize,
    pub rect: Option<[f64; 4]>,
    pub seg: [f64; 4],
    pub r: f64,
}

#[derive(Debug, Clone)]
pub struct Rules {
    pub track: f64,
    pub clearance: f64,
    pub via: (f64, f64),
    /// Track width and clearance of the nets in a net class.
    pub width: HashMap<String, f64>,
    pub wider: HashMap<String, f64>,
    pub hole_clear: f64,
    pub hole_to_hole: f64,
    pub edge_clear: f64,
}

fn turn(p: [f64; 2], degrees: f64) -> [f64; 2] {
    // KiCad's angles: positive is anticlockwise on a sheet whose y axis points down
    let (s, c) = degrees.to_radians().sin_cos();
    [p[0] * c + p[1] * s, -p[0] * s + p[1] * c]
}

fn grow(b: &mut [f64; 4], p: [f64; 2]) {
    *b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
}

const EMPTY: [f64; 4] = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];

fn xy(s: &Sexp) -> [f64; 2] {
    [s.num(1).unwrap_or(0.0), s.num(2).unwrap_or(0.0)]
}

fn points(s: &Sexp) -> Vec<[f64; 2]> {
    s.find("pts").map(|p| p.all("xy").map(xy).collect()).unwrap_or_default()
}

/// What a drawing or a text on a copper layer covers, as boxes and lines. `origin` and `angle`:
/// where the footprint it belongs to lies (the board's own drawings: the origin, unturned).
/// A text is taken as the box round it, generously: its glyphs are not known here.
fn drawn(item: &Sexp, layer: usize, origin: [f64; 2], angle: f64, out: &mut Vec<Drawn>) {
    let place = |p: [f64; 2]| {
        let t = turn(p, angle);
        [origin[0] + t[0], origin[1] + t[1]]
    };
    let half = item
        .find("stroke")
        .and_then(|s| s.find("width"))
        .or_else(|| item.find("width"))
        .and_then(|w| w.num(1))
        .unwrap_or(0.0)
        / 2.0;
    let filled = item
        .find("fill")
        .map(|f| !matches!(f.text(1), Some("none") | Some("no") | None))
        .unwrap_or(false);
    let line = |a: [f64; 2], b: [f64; 2], out: &mut Vec<Drawn>| {
        out.push(Drawn {
            layer,
            rect: None,
            seg: [a[0], a[1], b[0], b[1]],
            r: half,
        })
    };
    let boxed = |pts: &[[f64; 2]], out: &mut Vec<Drawn>| {
        let mut rect = EMPTY;
        pts.iter().for_each(|&p| grow(&mut rect, p));
        out.push(Drawn {
            layer,
            rect: Some([rect[0] - half, rect[1] - half, rect[2] + half, rect[3] + half]),
            seg: [rect[0], rect[1], rect[2], rect[3]],
            r: 0.0,
        })
    };
    let head = item.head().unwrap_or("");
    match &head[head.find('_').map(|i| i + 1).unwrap_or(0)..] {
        "line" => {
            if let (Some(a), Some(b)) = (item.find("start"), item.find("end")) {
                line(place(xy(a)), place(xy(b)), out);
            }
        }
        "arc" => {
            if let (Some(a), Some(m), Some(b)) = (item.find("start").map(xy), item.find("mid").map(xy), item.find("end").map(xy)) {
                // the circle through the three points, walked in steps of 15 degrees at most (two
                // chords for a quarter circle of 3 mm lie 0.2 mm inside it)
                let d = 2.0 * (a[0] * (m[1] - b[1]) + m[0] * (b[1] - a[1]) + b[0] * (a[1] - m[1]));
                let mut pts = vec![a, m, b];
                if d.abs() > 1e-12 {
                    let sq = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
                    let cx = (sq(a) * (m[1] - b[1]) + sq(m) * (b[1] - a[1]) + sq(b) * (a[1] - m[1])) / d;
                    let cy = (sq(a) * (b[0] - m[0]) + sq(m) * (a[0] - b[0]) + sq(b) * (m[0] - a[0])) / d;
                    let r = ((a[0] - cx).powi(2) + (a[1] - cy).powi(2)).sqrt();
                    let at = |p: [f64; 2]| (p[1] - cy).atan2(p[0] - cx);
                    let tau = std::f64::consts::TAU;
                    let (from, mut via, mut to) = (at(a), (at(m) - at(a)).rem_euclid(tau), (at(b) - at(a)).rem_euclid(tau));
                    if via > to {
                        // the other way round
                        (via, to) = (via - tau, to - tau);
                    }
                    let _ = via;
                    let n = (to.abs() / (tau / 24.0)).ceil().max(1.0) as usize;
                    pts = (0..=n)
                        .map(|k| from + to * k as f64 / n as f64)
                        .map(|t| [cx + r * t.cos(), cy + r * t.sin()])
                        .collect();
                }
                for w in pts.windows(2) {
                    line(place(w[0]), place(w[1]), out);
                }
            }
        }
        "rect" => {
            if let (Some(a), Some(b)) = (item.find("start").map(xy), item.find("end").map(xy)) {
                let corners: Vec<[f64; 2]> = [[a[0], a[1]], [b[0], a[1]], [b[0], b[1]], [a[0], b[1]]].into_iter().map(place).collect();
                if filled {
                    boxed(&corners, out);
                } else {
                    for k in 0..4 {
                        line(corners[k], corners[(k + 1) % 4], out);
                    }
                }
            }
        }
        "poly" => {
            let pts: Vec<[f64; 2]> = points(item).into_iter().map(place).collect();
            if filled {
                boxed(&pts, out);
            } else {
                for k in 0..pts.len() {
                    line(pts[k], pts[(k + 1) % pts.len()], out);
                }
            }
        }
        "circle" => {
            if let (Some(c), Some(e)) = (item.find("center").map(xy), item.find("end").map(xy)) {
                let r = ((e[0] - c[0]).powi(2) + (e[1] - c[1]).powi(2)).sqrt();
                let round: Vec<[f64; 2]> = (0..16)
                    .map(|k| (k as f64 * std::f64::consts::TAU / 16.0).sin_cos())
                    .map(|(sin, cos)| place([c[0] + r * cos, c[1] + r * sin]))
                    .collect();
                if filled {
                    boxed(&round, out);
                } else {
                    for k in 0..16 {
                        line(round[k], round[(k + 1) % 16], out);
                    }
                }
            }
        }
        "text" => {
            // (gr_text "words" ...) or (fp_text kind "words" ...)
            let words = item.text(if head == "gr_text" { 1 } else { 2 }).unwrap_or("");
            let Some(at) = item.find("at") else { return };
            let font = item.find("effects").and_then(|e| e.find("font"));
            let size = font.and_then(|f| f.find("size")).map(xy).unwrap_or([1.0, 1.0]); // height, width
            let thick = font.and_then(|f| f.find("thickness")).and_then(|t| t.num(1)).unwrap_or(0.15);
            let lines: Vec<&str> = words.split("\\n").collect();
            let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64 * size[1] * 1.3 + thick + 0.4;
            let height = lines.len() as f64 * size[0] * 1.6 + thick + 0.4;
            // to one side of its place or about it: either way within its width on both sides
            let aside = item.find("effects").and_then(|e| e.find("justify")).is_some();
            let (w, h) = (if aside { width } else { width / 2.0 }, if aside { height } else { height / 2.0 });
            let own = at.num(3).unwrap_or(0.0);
            let centre = place(xy(at));
            let corners: Vec<[f64; 2]> = [[-w, -h], [w, -h], [w, h], [-w, h]]
                .into_iter()
                .map(|c| turn(c, own))
                .map(|c| [centre[0] + c[0], centre[1] + c[1]])
                .collect();
            let mut rect = EMPTY;
            corners.iter().for_each(|&p| grow(&mut rect, p));
            out.push(Drawn {
                layer,
                rect: Some(rect),
                seg: [rect[0], rect[1], rect[2], rect[3]],
                r: 0.0,
            });
        }
        _ => {}
    }
}

impl Board {
    pub fn parse(text: &str) -> Result<Board, String> {
        let root = parse(text)?;
        if root.head() != Some("kicad_pcb") {
            return Err("not a KiCad board file".to_string());
        }
        let mut board = Board::default();
        for layer in root.find("layers").map(|l| l.items()).unwrap_or(&[]) {
            if let (Some(name), Some(kind)) = (layer.text(1), layer.text(2)) {
                if name.ends_with(".Cu") {
                    board.copper.push((name.to_string(), kind != "power"));
                }
            }
        }
        let index: HashMap<String, usize> = board.copper.iter().enumerate().map(|(n, c)| (c.0.clone(), n)).collect();
        let on = |names: Vec<&str>| -> Vec<usize> {
            let mut out: Vec<usize> = Vec::new();
            for name in names {
                match name {
                    "*.Cu" => out.extend(0..index.len()),
                    "F&B.Cu" => out.extend([index.get("F.Cu"), index.get("B.Cu")].into_iter().flatten().copied()),
                    other => out.extend(index.get(other).copied()),
                }
            }
            out.sort();
            out.dedup();
            out
        };
        let net = |s: &Sexp| s.find("net").and_then(|n| n.text(n.items().len() - 1)).unwrap_or("").to_string();
        let locked = |s: &Sexp| s.find("locked").map(|l| l.text(1) != Some("no")).unwrap_or(false) || s.words().contains(&"locked");
        // A drawing on a solder mask layer bares the copper under it: a track of another net there
        // would lie open beside what the opening is for (KiCad: solder mask bridge). So it is in the
        // way on the outer copper layer of its side, like a drawing on that layer.
        let bared = |layer: &str| -> Option<usize> {
            match layer {
                "F.Mask" => index.get("F.Cu").copied(),
                "B.Mask" => index.get("B.Cu").copied(),
                _ => None,
            }
        };
        let mut outline = EMPTY;
        for item in root.items() {
            match item.head() {
                Some("footprint") => {
                    let at = item.find("at").ok_or("a footprint without a position")?;
                    let (origin, angle) = (xy(at), at.num(3).unwrap_or(0.0));
                    let reference = item
                        .all("property")
                        .find(|p| p.text(1) == Some("Reference"))
                        .and_then(|p| p.text(2))
                        .unwrap_or("")
                        .to_string();
                    for pad in item.all("pad") {
                        let place = pad.find("at").ok_or("a pad without a position")?;
                        let local = turn(xy(place), angle);
                        let centre = [origin[0] + local[0], origin[1] + local[1]];
                        let own = place.num(3).unwrap_or(0.0);
                        let size = pad.find("size").map(xy).unwrap_or([0.0, 0.0]);
                        // The copper may lie off the hole: (drill 0.75 (offset 0 0.4)). Read without
                        // that, a transistor's pads were taken to be 0.4 mm from where they are, and a
                        // track was laid across one.
                        let off = pad.find("drill").and_then(|d| d.find("offset")).map(xy).unwrap_or([0.0, 0.0]);
                        let off = turn(off, own);
                        let middle = [centre[0] + off[0], centre[1] + off[1]];
                        let mut rect = EMPTY;
                        if pad.text(3) == Some("circle") {
                            grow(&mut rect, [middle[0] - size[0] / 2.0, middle[1] - size[1] / 2.0]);
                            grow(&mut rect, [middle[0] + size[0] / 2.0, middle[1] + size[1] / 2.0]);
                        } else {
                            for (sx, sy) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
                                let corner = turn([sx * size[0], sy * size[1]], own);
                                grow(&mut rect, [middle[0] + corner[0], middle[1] + corner[1]]);
                            }
                        }
                        let slant = own.rem_euclid(90.0);
                        let plain = pad.find("primitives").is_none() && pad.text(3) != Some("circle");
                        let turned = (plain && slant > 0.01 && slant < 89.99).then(|| {
                            (
                                [
                                    middle[0] - size[0] / 2.0,
                                    middle[1] - size[1] / 2.0,
                                    middle[0] + size[0] / 2.0,
                                    middle[1] + size[1] / 2.0,
                                ],
                                own,
                            )
                        });
                        for shape in pad.find("primitives").map(|p| p.items()).unwrap_or(&[]) {
                            let mut pts = points(shape);
                            pts.extend(["start", "end", "mid", "center"].iter().filter_map(|k| shape.find(k)).map(xy));
                            let half = shape.find("width").and_then(|w| w.num(1)).unwrap_or(0.0) / 2.0;
                            for p in pts {
                                let p = turn(p, own);
                                grow(&mut rect, [centre[0] + p[0] - half, centre[1] + p[1] - half]);
                                grow(&mut rect, [centre[0] + p[0] + half, centre[1] + p[1] + half]);
                            }
                        }
                        let hole = pad.find("drill").map(|d| {
                            d.items()[1..]
                                .iter()
                                .filter_map(|v| if let Sexp::Atom(a) = v { a.parse::<f64>().ok() } else { None })
                                .fold(0.0, f64::max)
                                / 2.0
                        });
                        let layers = on(pad.find("layers").map(|l| l.words()).unwrap_or_default());
                        if layers.is_empty() && hole.unwrap_or(0.0) == 0.0 {
                            continue;
                        }
                        board.pads.push(Pad {
                            reference: reference.clone(),
                            number: pad.text(1).unwrap_or("").to_string(),
                            net: net(pad),
                            at: centre,
                            rect,
                            turned,
                            clearance: pad
                                .find("clearance")
                                .or_else(|| item.find("clearance"))
                                .and_then(|c| c.num(1))
                                .filter(|c| *c > 0.0),
                            layers,
                            hole: hole.unwrap_or(0.0),
                        });
                    }
                    for part in item.items() {
                        if matches!(part.head(), Some("fp_text" | "fp_line" | "fp_arc" | "fp_rect" | "fp_poly" | "fp_circle")) {
                            let on = part.find("layer").and_then(|l| l.text(1));
                            if let Some(&layer) = on.and_then(|l| index.get(l)) {
                                drawn(part, layer, origin, angle, &mut board.drawn);
                            } else if on == Some("Edge.Cuts") && part.head() != Some("fp_text") {
                                drawn(part, 0, origin, angle, &mut board.cuts);
                            } else if let Some(layer) = on.and_then(bared) {
                                drawn(part, layer, origin, angle, &mut board.drawn);
                            }
                        }
                    }
                }
                Some("gr_text") => {
                    let on = item.find("layer").and_then(|l| l.text(1));
                    if let Some(layer) = on.and_then(|l| index.get(l).copied()).or_else(|| on.and_then(bared)) {
                        drawn(item, layer, [0.0, 0.0], 0.0, &mut board.drawn);
                    }
                }
                Some("segment") => {
                    let layer = item.find("layer").and_then(|l| l.text(1)).and_then(|l| index.get(l)).copied();
                    if let (Some(a), Some(b), Some(layer)) = (item.find("start"), item.find("end"), layer) {
                        board.segments.push(Segment {
                            a: xy(a),
                            b: xy(b),
                            width: item.find("width").and_then(|w| w.num(1)).unwrap_or(0.0),
                            layer,
                            net: net(item),
                            locked: locked(item),
                        });
                    }
                }
                Some("arc") => return Err("the board has curved tracks, which are not read yet".to_string()),
                Some("via") => {
                    let at = item.find("at").ok_or("a via without a position")?;
                    board.vias.push(Hole {
                        at: xy(at),
                        size: item.find("size").and_then(|s| s.num(1)).unwrap_or(0.0),
                        drill: item.find("drill").and_then(|s| s.num(1)).unwrap_or(0.0),
                        net: net(item),
                        locked: locked(item),
                    });
                }
                Some("zone") => {
                    let mut names = item.find("layers").map(|l| l.words()).unwrap_or_default();
                    names.extend(item.find("layer").and_then(|l| l.text(1)));
                    let mut bounds = EMPTY;
                    for polygon in item.all("polygon") {
                        for p in points(polygon) {
                            grow(&mut bounds, p);
                        }
                    }
                    let keepout = item.find("keepout").map(|k| {
                        let no = |what: &str| k.find(what).and_then(|w| w.text(1)) == Some("not_allowed");
                        (no("tracks"), no("vias"))
                    });
                    let fills = item
                        .all("filled_polygon")
                        .filter_map(|f| Some((*index.get(f.find("layer")?.text(1)?)?, points(f))))
                        .collect();
                    board.zones.push(Zone {
                        net: net(item),
                        layers: on(names),
                        keepout,
                        bounds,
                        fills,
                    });
                }
                Some("gr_line") | Some("gr_arc") | Some("gr_rect") | Some("gr_poly") | Some("gr_circle") => {
                    let on = item.find("layer").and_then(|l| l.text(1));
                    if let Some(layer) = on.and_then(|l| index.get(l).copied()).or_else(|| on.and_then(bared)) {
                        drawn(item, layer, [0.0, 0.0], 0.0, &mut board.drawn);
                    }
                    if item.find("layer").and_then(|l| l.text(1)) != Some("Edge.Cuts") {
                        continue;
                    }
                    drawn(item, 0, [0.0, 0.0], 0.0, &mut board.cuts);
                    let mut pts = points(item);
                    pts.extend(["start", "end", "mid"].iter().filter_map(|k| item.find(k)).map(xy));
                    if item.head() == Some("gr_circle") {
                        if let (Some(c), Some(e)) = (item.find("center").map(xy), item.find("end").map(xy)) {
                            let r = ((e[0] - c[0]).powi(2) + (e[1] - c[1]).powi(2)).sqrt();
                            pts = vec![[c[0] - r, c[1] - r], [c[0] + r, c[1] + r]];
                        }
                    }
                    for p in pts {
                        grow(&mut outline, p);
                    }
                }
                _ => {}
            }
        }
        if outline != EMPTY {
            board.edge = Some(outline);
        }
        Ok(board)
    }
}

/// `*` and `?` as in KiCad's net class patterns.
fn matches(pattern: &[u8], name: &[u8]) -> bool {
    match (pattern.first(), name.first()) {
        (None, None) => true,
        (Some(b'*'), _) => matches(&pattern[1..], name) || (!name.is_empty() && matches(pattern, &name[1..])),
        (Some(b'?'), Some(_)) => matches(&pattern[1..], &name[1..]),
        (Some(p), Some(n)) if p == n => matches(&pattern[1..], &name[1..]),
        _ => false,
    }
}

impl Rules {
    /// The rules of a project file, for the nets of `board`.
    pub fn parse(project: &str, board: &Board) -> Result<Rules, String> {
        let root: serde_json::Value = serde_json::from_str(project).map_err(|e| e.to_string())?;
        let classes = root["net_settings"]["classes"].as_array().ok_or("the project file has no net classes")?;
        let class = |name: &str| classes.iter().find(|c| c["name"] == name);
        let default = class("Default").ok_or("the project file has no Default net class")?;
        let get = |c: &serde_json::Value, key: &str| c[key].as_f64().unwrap_or(0.0);
        let limits = &root["board"]["design_settings"]["rules"];
        // What the board's constraints ask for at least goes before what a net class says: a
        // project with a Default class of 0.2 mm tracks and 0.6 mm vias and constraints of 0.3 mm
        // and 1.5 mm was routed with the class's, 199 tracks and 32 vias against its own rules.
        let least = |key: &str| limits[key].as_f64().unwrap_or(0.0);
        let thinnest = least("min_track_width").max(least("min_connection"));
        let drill = get(default, "via_drill").max(least("min_through_hole_diameter"));
        let mut rules = Rules {
            track: get(default, "track_width").max(thinnest),
            clearance: get(default, "clearance").max(least("min_clearance")),
            via: (
                get(default, "via_diameter")
                    .max(least("min_via_diameter"))
                    .max(drill + 2.0 * least("min_via_annular_width")),
                drill,
            ),
            width: HashMap::new(),
            wider: HashMap::new(),
            hole_clear: limits["min_hole_clearance"].as_f64().unwrap_or(0.25),
            hole_to_hole: limits["min_hole_to_hole"].as_f64().unwrap_or(0.25),
            edge_clear: limits["min_copper_edge_clearance"].as_f64().unwrap_or(0.3),
        };
        let mut nets: Vec<&str> = board.pads.iter().map(|p| p.net.as_str()).collect();
        nets.sort();
        nets.dedup();
        for entry in root["net_settings"]["netclass_patterns"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            let (Some(pattern), Some(found)) = (entry["pattern"].as_str(), entry["netclass"].as_str().and_then(class)) else {
                continue;
            };
            for net in nets.iter().filter(|n| matches(pattern.as_bytes(), n.as_bytes())) {
                if !rules.width.contains_key(*net) {
                    rules.width.insert(net.to_string(), get(found, "track_width").max(thinnest));
                    if get(found, "clearance") > rules.clearance {
                        rules.wider.insert(net.to_string(), get(found, "clearance"));
                    }
                }
            }
        }
        // (projects of KiCad 6 and 7 name the class per net: {"net": "class"} or {"net": ["class"]})
        if let Some(by_net) = root["net_settings"]["netclass_assignments"].as_object() {
            for (net, named) in by_net {
                let name = named.as_str().or_else(|| named.as_array().and_then(|a| a.first()).and_then(|n| n.as_str()));
                if let (Some(found), false) = (name.and_then(class), rules.width.contains_key(net)) {
                    rules.width.insert(net.to_string(), get(found, "track_width").max(thinnest));
                    if get(found, "clearance") > rules.clearance {
                        rules.wider.insert(net.to_string(), get(found, "clearance"));
                    }
                }
            }
        }
        Ok(rules)
    }

    fn keep(&self, net: &str) -> f64 {
        self.wider.get(net).copied().unwrap_or(0.0).max(self.clearance)
    }
}

fn point_seg(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (vx, vy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = vx * vx + vy * vy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * vx + (p[1] - a[1]) * vy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((p[0] - a[0] - t * vx).powi(2) + (p[1] - a[1] - t * vy).powi(2)).sqrt()
}

fn crossing(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let side = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let (s1, s2, s3, s4) = (side(a, b, c), side(a, b, d), side(c, d, a), side(c, d, b));
    (s1 > 0.0) != (s2 > 0.0) && (s3 > 0.0) != (s4 > 0.0) && s1 != 0.0 && s2 != 0.0 && s3 != 0.0 && s4 != 0.0
}

/// Smallest distance between two lines of finite length.
fn seg_seg(a: [f64; 4], b: [f64; 4]) -> f64 {
    let (p, q, r, s) = ([a[0], a[1]], [a[2], a[3]], [b[0], b[1]], [b[2], b[3]]);
    if crossing(p, q, r, s) {
        return 0.0;
    }
    point_seg(p, r, s).min(point_seg(q, r, s)).min(point_seg(r, p, q)).min(point_seg(s, p, q))
}

fn inside(p: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut odd = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// One piece of copper with what is needed to tell whether it touches another.
struct Copper {
    net: String,
    /// Copper layers (all of the board's, for a via or a drilled pad).
    layers: Vec<usize>,
    rect: Option<[f64; 4]>,
    /// A pad at an angle (`Pad::turned`): `rect` is then the box round it.
    turned: Option<([f64; 4], f64)>,
    /// What tracks keep from it, if not their clearance: the board's edge.
    keep: Option<f64>,
    seg: [f64; 4],
    r: f64,
    hole: f64,
}

impl Copper {
    fn shape(&self, routing: &[Option<usize>]) -> Shape {
        let layers = self.layers.iter().filter_map(|&l| routing[l]).collect();
        match self.rect {
            Some(rect) => Shape {
                rect: Some(self.turned.map_or(rect, |t| t.0)),
                angle: self.turned.map_or(0.0, |t| t.1),
                seg: None,
                r: 0.0,
                layers,
                hole: self.hole,
                clearance: 0.0,
                role: Role::None,
                room: 0.0,
            },
            None => Shape {
                rect: None,
                angle: 0.0,
                seg: Some(self.seg),
                r: self.r,
                layers,
                hole: self.hole,
                clearance: 0.0,
                role: Role::None,
                room: 0.0,
            },
        }
    }

    fn touches(&self, other: &Copper) -> bool {
        if !self.layers.iter().any(|l| other.layers.contains(l)) {
            return false;
        }
        let to_rect = |rect: [f64; 4], it: &Copper| -> bool {
            let s = it.seg;
            if [[s[0], s[1]], [s[2], s[3]]]
                .iter()
                .any(|p| rect[0] <= p[0] && p[0] <= rect[2] && rect[1] <= p[1] && p[1] <= rect[3])
            {
                return true;
            }
            let sides = [
                [rect[0], rect[1], rect[2], rect[1]],
                [rect[2], rect[1], rect[2], rect[3]],
                [rect[2], rect[3], rect[0], rect[3]],
                [rect[0], rect[3], rect[0], rect[1]],
            ];
            sides.iter().any(|side| seg_seg(s, *side) <= it.r)
        };
        match (self.rect, other.rect) {
            (Some(a), Some(b)) => a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3],
            (Some(a), None) => to_rect(a, other),
            (None, Some(b)) => to_rect(b, self),
            (None, None) => seg_seg(self.seg, other.seg) <= self.r + other.r,
        }
    }

    /// Does it lie on this island of a pour?
    fn on(&self, island: &[[f64; 2]], bounds: [f64; 4]) -> bool {
        let reach = self.r;
        let own = match self.rect {
            Some(r) => r,
            None => [
                self.seg[0].min(self.seg[2]) - reach,
                self.seg[1].min(self.seg[3]) - reach,
                self.seg[0].max(self.seg[2]) + reach,
                self.seg[1].max(self.seg[3]) + reach,
            ],
        };
        if own[0] > bounds[2] || bounds[0] > own[2] || own[1] > bounds[3] || bounds[1] > own[3] {
            return false;
        }
        let marks: Vec<[f64; 2]> = match self.rect {
            Some(r) => vec![
                [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0],
                [r[0], r[1]],
                [r[2], r[1]],
                [r[2], r[3]],
                [r[0], r[3]],
            ],
            None => vec![[self.seg[0], self.seg[1]], [self.seg[2], self.seg[3]]],
        };
        if marks.iter().any(|&m| inside(m, island)) {
            return true;
        }
        island.iter().any(|&v| match self.rect {
            Some(r) => r[0] <= v[0] && v[0] <= r[2] && r[1] <= v[1] && v[1] <= r[3],
            None => point_seg(v, [self.seg[0], self.seg[1]], [self.seg[2], self.seg[3]]) <= self.r,
        })
    }
}

/// One island of a pour: net, layer, outline, bounding box.
type Island<'a> = (&'a str, usize, &'a Vec<[f64; 2]>, [f64; 4]);

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

pub struct Options {
    pub step: f64,
    pub via_cost: f64,
    pub max_iter: u32,
    /// Copper to a foreign hole; None: the project's limit.
    pub hole_clear: Option<f64>,
    /// Nets for which a short way matters more (see `problem::Net::weight`).
    pub weights: HashMap<String, f64>,
    /// Route these nets only (empty: all); the others' copper is in the way as it is.
    pub only: Vec<String>,
    /// A pad on a filled pour of its net counts as joined to all else on that pour, also on a
    /// layer that tracks are laid on. Without it (the default from the command line) only the
    /// pours on plane layers join: on a layer with tracks the new tracks cut a pour apart or
    /// shut a pad off from it, and what the pour had joined is then open (six boards routed
    /// both ways: 54 connections left open with the pours trusted, 16 without).
    pub pours: bool,
    /// Tracks keep this far from a pad that hangs in a pour, where a way exists that is not much
    /// longer (mm; 0: no such wish): the pour reaches the pad by spokes, and a track past it at
    /// clearance cuts them.
    pub room: f64,
}

/// The board as a routing problem: every net that is in more than one piece is to be joined,
/// all other copper is in the way. Nets and pieces are in an order that depends on their names
/// and places only, not on the order in the file.
pub fn problem(board: &Board, rules: &Rules, options: &Options) -> Result<Problem, String> {
    let edge = board.edge.ok_or("the board has no outline")?;
    let mut number = 0;
    let routing: Vec<Option<usize>> = board
        .copper
        .iter()
        .map(|c| {
            c.1.then(|| {
                number += 1;
                number - 1
            })
        })
        .collect();
    let all: Vec<usize> = (0..board.copper.len()).collect();
    let mut copper: Vec<Copper> = Vec::new();
    for pad in &board.pads {
        let layers = if pad.hole > 0.0 { all.clone() } else { pad.layers.clone() };
        copper.push(Copper {
            net: pad.net.clone(),
            layers,
            rect: Some(pad.rect),
            turned: pad.turned,
            keep: pad.clearance.map(|c| c.max(rules.keep(&pad.net))),
            seg: [pad.at[0], pad.at[1], pad.at[0], pad.at[1]],
            r: 0.0,
            hole: pad.hole,
        });
    }
    for s in &board.segments {
        copper.push(Copper {
            net: s.net.clone(),
            layers: vec![s.layer],
            rect: None,
            turned: None,
            keep: None,
            seg: [s.a[0], s.a[1], s.b[0], s.b[1]],
            r: s.width / 2.0,
            hole: 0.0,
        });
    }
    for v in &board.vias {
        copper.push(Copper {
            net: v.net.clone(),
            layers: all.clone(),
            rect: None,
            turned: None,
            keep: None,
            seg: [v.at[0], v.at[1], v.at[0], v.at[1]],
            r: v.size / 2.0,
            hole: v.drill / 2.0,
        });
    }
    // The outline as it is drawn, and every cut-out: a line that no copper comes nearer to than the
    // edge clearance, on all layers. (Only the box round the outline was known: copper lay across
    // rounded corners, notches and slots, 46 times on four boards.)
    for d in &board.cuts {
        copper.push(Copper {
            net: String::new(),
            layers: all.clone(),
            rect: d.rect,
            turned: None,
            keep: Some(rules.edge_clear),
            seg: d.seg,
            r: 0.0,
            hole: 0.0,
        });
    }
    for d in &board.drawn {
        copper.push(Copper {
            net: String::new(),
            layers: vec![d.layer],
            rect: d.rect,
            turned: None,
            keep: None,
            seg: d.seg,
            r: d.r,
            hole: 0.0,
        });
    }
    // what is one piece already: copper of a net that touches, and copper on the same island of a pour
    let mut by_net: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (n, c) in copper.iter().enumerate() {
        if !c.net.is_empty() {
            by_net.entry(c.net.as_str()).or_default().push(n);
        }
    }
    let islands: Vec<Island> = board
        .zones
        .iter()
        .filter(|z| z.keepout.is_none() && !z.net.is_empty())
        .flat_map(|z| z.fills.iter().map(move |(layer, pts)| (z.net.as_str(), *layer, pts)))
        .filter(|(_, _, pts)| pts.len() > 2)
        .map(|(net, layer, pts)| {
            let mut bounds = EMPTY;
            pts.iter().for_each(|&p| grow(&mut bounds, p));
            (net, layer, pts, bounds)
        })
        .collect();
    let mut parent: Vec<usize> = (0..copper.len() + islands.len()).collect();
    for members in by_net.values() {
        for (k, &a) in members.iter().enumerate() {
            for &b in &members[k + 1..] {
                if copper[a].touches(&copper[b]) {
                    let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                    parent[ra.max(rb)] = ra.min(rb);
                }
            }
        }
    }
    let mut rooms: Vec<Shape> = Vec::new();
    for (i, (net, layer, pts, bounds)) in islands.iter().enumerate() {
        for &a in by_net.get(net).map(|m| m.as_slice()).unwrap_or(&[]) {
            if copper[a].layers.contains(layer) && copper[a].on(pts, *bounds) {
                if options.pours || routing[*layer].is_none() {
                    let (ra, rb) = (root(&mut parent, a), root(&mut parent, copper.len() + i));
                    parent[ra.max(rb)] = ra.min(rb);
                }
                // joined by tracks or not, the pour will hold the pad by its spokes
                if let (Some(rect), Some(on), true) = (copper[a].rect, routing[*layer], options.room > 0.0) {
                    rooms.push(Shape {
                        rect: Some(copper[a].turned.map_or(rect, |t| t.0)),
                        angle: copper[a].turned.map_or(0.0, |t| t.1),
                        seg: None,
                        r: 0.0,
                        layers: vec![on],
                        hole: 0.0,
                        clearance: 0.0,
                        role: Role::None,
                        room: options.room,
                    });
                }
            }
        }
    }
    let place = |c: &Copper| -> (i64, i64, usize) {
        let (x, y) = c
            .rect
            .map(|r| ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0))
            .unwrap_or((c.seg[0].min(c.seg[2]), c.seg[1].min(c.seg[3])));
        ((x * 1e4).round() as i64, (y * 1e4).round() as i64, c.layers[0])
    };
    let mut nets = Vec::new();
    let mut routed: HashSet<&str> = HashSet::new();
    let mut held: Vec<usize> = Vec::new(); // copper of routed nets that is on their plane: joined, and in the way
    for (name, members) in &by_net {
        let mut pieces: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &m in members {
            pieces.entry(root(&mut parent, m)).or_default().push(m);
        }
        // The net's pours on plane layers. What lies on one is joined; every other piece is joined
        // by a way of its own that ends in a via into the plane, wherever that suits it.
        let planes: Vec<usize> = (0..islands.len())
            .filter(|&i| islands[i].0 == *name && routing[islands[i].1].is_none())
            .collect();
        let on_plane: HashSet<usize> = planes.iter().map(|&i| root(&mut parent, copper.len() + i)).collect();
        let apart = pieces.keys().filter(|k| !on_plane.contains(k)).count();
        let wanted = options.only.is_empty() || options.only.iter().any(|o| o == name);
        if !wanted || (planes.is_empty() && pieces.len() < 2) || (!planes.is_empty() && apart == 0) {
            continue;
        }
        let mut pieces: Vec<(usize, Vec<usize>)> = pieces.into_iter().collect();
        for piece in pieces.iter_mut() {
            piece.1.sort_by_key(|&m| (place(&copper[m]), m));
        }
        pieces.sort_by_key(|piece| place(&copper[piece.1[0]]));
        routed.insert(name);
        let net = |pieces: Vec<Vec<Shape>>, plane: Vec<Vec<[f64; 2]>>| Net {
            name: name.to_string(),
            width: rules.width.get(*name).copied().unwrap_or(rules.track),
            narrow: Some(rules.track),
            clearance: rules.keep(name),
            weight: options.weights.get(*name).copied().unwrap_or(1.0),
            role: Role::None,
            pieces,
            plane,
        };
        let shapes = |piece: &Vec<usize>| -> Vec<Shape> { piece.iter().map(|&m| copper[m].shape(&routing)).collect() };
        if planes.is_empty() {
            nets.push(net(pieces.iter().map(|piece| shapes(&piece.1)).collect(), Vec::new()));
        } else {
            let polygons: Vec<Vec<[f64; 2]>> = planes.iter().map(|&i| islands[i].2.clone()).collect();
            for (key, piece) in &pieces {
                if on_plane.contains(key) {
                    held.extend(piece.iter().copied());
                } else {
                    nets.push(net(vec![shapes(piece)], polygons.clone()));
                }
            }
        }
    }
    let held: HashSet<usize> = held.into_iter().collect();
    let fixed = copper
        .iter()
        .enumerate()
        .filter(|(n, c)| !routed.contains(c.net.as_str()) || held.contains(n))
        .map(|(_, c)| Shape {
            clearance: c.keep.unwrap_or_else(|| rules.keep(&c.net)),
            ..c.shape(&routing)
        })
        .collect();
    let keepouts = board
        .zones
        .iter()
        .filter_map(|z| {
            z.keepout.map(|k| Keepout {
                area: z.bounds,
                layers: z.layers.iter().filter_map(|&l| routing[l]).collect(),
                tracks: k.0,
                vias: k.1,
            })
        })
        .collect();
    Ok(Problem {
        area: edge,
        step: options.step,
        layers: number,
        via: Via {
            d: rules.via.0,
            hole: rules.via.1,
            cost: options.via_cost,
        },
        hole_clear: options.hole_clear.unwrap_or(rules.hole_clear),
        hole_to_hole: rules.hole_to_hole,
        edge: Some(edge),
        edge_clear: rules.edge_clear,
        keepouts,
        fixed,
        rooms,
        nets,
        max_iter: options.max_iter,
        relax: 12,
        alone: false,
        apart: 0.0,
        shy: 3.0,
        straight: true,
        glance: false,
        seed: 0,
        relay: true,
        untangle: true,
        stall: 20,
        limit: 50_000_000,
    })
}

/// A track of the answer in millimetres.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub net: String,
    /// Routing layer.
    pub layer: usize,
    pub width: f64,
    pub a: [f64; 2],
    pub b: [f64; 2],
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 5e-4
}

/// The answer as copper: tracks whose ends lie on what they join, and vias. A track that joins
/// a track or a via ends on its centre line; one that joins a pad ends on the grid inside it.
pub fn copper_of(problem: &Problem, solution: &Solution) -> (Vec<Track>, Vec<(String, [f64; 2])>) {
    let (x0, y0, step) = (problem.area[0], problem.area[1], problem.step);
    let at = |cell: &[u32; 3]| [x0 + cell[1] as f64 * step, y0 + cell[2] as f64 * step];
    let (mut tracks, mut vias): (Vec<Track>, Vec<(String, [f64; 2])>) = (Vec::new(), Vec::new());
    let mut seen: HashSet<(usize, usize, [i64; 4])> = HashSet::new();
    let mut seen_via: HashSet<(usize, [i64; 2])> = HashSet::new();
    let key = |v: f64| (v * 1000.0).round() as i64;
    for (n, (net, laid)) in problem.nets.iter().zip(&solution.nets).enumerate() {
        for path in &laid.paths {
            // where a track has to end to be joined: on a track's line or a via's centre; anywhere in a pad
            let snap = |end: Option<[usize; 2]>, p: [f64; 2]| -> Option<[f64; 2]> {
                let s = net.pieces[end?[0]][end?[1]].seg?;
                let (vx, vy) = (s[2] - s[0], s[3] - s[1]);
                let len2 = vx * vx + vy * vy;
                let t = if len2 > 0.0 {
                    (((p[0] - s[0]) * vx + (p[1] - s[1]) * vy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                Some([s[0] + t * vx, s[1] + t * vy])
            };
            let cells = &path.cells;
            let mut start = 0;
            for k in 1..=cells.len() {
                if k < cells.len() && cells[k][0] == cells[start][0] {
                    continue;
                }
                let mut line: Vec<[f64; 2]> = cells[start..k].iter().map(at).collect();
                let first = start == 0;
                if first {
                    if let Some(s) = snap(path.from, line[0]).filter(|s| *s != line[0]) {
                        if line.len() > 1 {
                            line[0] = s;
                        } else {
                            line.insert(0, s);
                        }
                    }
                }
                if k == cells.len() {
                    let last = line[line.len() - 1];
                    if let Some(s) = snap(path.to, last).filter(|s| *s != last) {
                        if line.len() > 1 && !(first && line.len() == 2) {
                            let end = line.len() - 1;
                            line[end] = s;
                        } else {
                            line.push(s);
                        }
                    }
                }
                for w in line.windows(2) {
                    let mut ends = [[key(w[0][0]), key(w[0][1])], [key(w[1][0]), key(w[1][1])]];
                    ends.sort();
                    if ends[0] != ends[1] && seen.insert((n, cells[start][0] as usize, [ends[0][0], ends[0][1], ends[1][0], ends[1][1]])) {
                        tracks.push(Track {
                            net: net.name.clone(),
                            layer: cells[start][0] as usize,
                            width: path.width,
                            a: w[0],
                            b: w[1],
                        });
                    }
                }
                if k < cells.len() {
                    let p = at(&cells[k]);
                    if seen_via.insert((n, [key(p[0]), key(p[1])])) {
                        vias.push((net.name.clone(), p));
                    }
                    start = k;
                }
            }
        }
    }
    debug_assert!(tracks.iter().all(|t| !(near(t.a[0], t.b[0]) && near(t.a[1], t.b[1]))));
    (tracks, vias)
}

fn number(v: f64) -> String {
    let text = format!("{:.6}", v);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

/// The board file with the answer's tracks and vias added. Their ids follow from `seed` and
/// their order, so the same answer gives the same file.
pub fn write(text: &str, board: &Board, problem: &Problem, solution: &Solution, seed: u64) -> Result<String, String> {
    let names: Vec<&str> = board.copper.iter().filter(|c| c.1).map(|c| c.0.as_str()).collect();
    let (first, last) = (
        board.copper.first().ok_or("no copper layers")?.0.as_str(),
        board.copper.last().unwrap().0.as_str(),
    );
    let (tracks, vias) = copper_of(problem, solution);
    let mut state = seed;
    let mut uuid = || {
        let mut word = || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        let (a, b) = (word(), word());
        format!(
            "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
            a >> 32,
            (a >> 16) & 0xffff,
            a & 0xfff,
            0x8000 | (b >> 48) & 0x3fff,
            b & 0xffff_ffff_ffff
        )
    };
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut add = String::new();
    for t in &tracks {
        add += &format!(
            "\t(segment\n\t\t(start {} {})\n\t\t(end {} {})\n\t\t(width {})\n\t\t(layer \"{}\")\n\t\t(net \"{}\")\n\t\t(uuid \"{}\")\n\t)\n",
            number(t.a[0]),
            number(t.a[1]),
            number(t.b[0]),
            number(t.b[1]),
            number(t.width),
            names[t.layer],
            quote(&t.net),
            uuid()
        );
    }
    for (net, p) in &vias {
        add += &format!(
            "\t(via\n\t\t(at {} {})\n\t\t(size {})\n\t\t(drill {})\n\t\t(layers \"{}\" \"{}\")\n\t\t(net \"{}\")\n\t\t(uuid \"{}\")\n\t)\n",
            number(p[0]),
            number(p[1]),
            number(problem.via.d),
            number(problem.via.hole),
            first,
            last,
            quote(net),
            uuid()
        );
    }
    let end = text.rfind(')').ok_or("the board file has no end")?;
    let head = text[..end].trim_end_matches([' ', '\t']);
    Ok(format!("{}{}{}{}", head, if head.ends_with('\n') { "" } else { "\n" }, add, &text[end..]))
}
