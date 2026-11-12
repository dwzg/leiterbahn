use leiterbahn_route::problem::{Problem, Solution};
use serde_json::json;

fn pad(x: f64, y: f64, layer: usize) -> serde_json::Value {
    json!({"box": [x - 0.2, y - 0.2, x + 0.2, y + 0.2], "layers": [layer]})
}

fn solve(p: serde_json::Value) -> Solution {
    let problem: Problem = serde_json::from_value(p).unwrap();
    leiterbahn_route::route(&problem, &mut |_| {})
}

/// Track centre lines as (net, layer, x0, y0, x1, y1) in cells.
fn lines(s: &Solution) -> Vec<(usize, u32, f64, f64, f64, f64)> {
    let mut out = Vec::new();
    for (n, net) in s.nets.iter().enumerate() {
        for path in &net.paths {
            for w in path.cells.windows(2) {
                if w[0][0] == w[1][0] {
                    out.push((n, w[0][0], w[0][1] as f64, w[0][2] as f64, w[1][1] as f64, w[1][2] as f64));
                }
            }
        }
    }
    out
}

fn gap(a: &(usize, u32, f64, f64, f64, f64), b: &(usize, u32, f64, f64, f64, f64)) -> f64 {
    let mut best = f64::MAX;
    for k in 0..=40 {
        let t = k as f64 / 40.0;
        let (x, y) = (a.2 + (a.4 - a.2) * t, a.3 + (a.5 - a.3) * t);
        for m in 0..=40 {
            let u = m as f64 / 40.0;
            let (p, q) = (b.2 + (b.4 - b.2) * u, b.3 + (b.5 - b.3) * u);
            best = best.min(((x - p).powi(2) + (y - q).powi(2)).sqrt());
        }
    }
    best
}

#[test]
fn a_free_connection_runs_straight() {
    let s = solve(json!({
        "box": [0, 0, 10, 6], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "nets": [{"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(1.0, 3.0, 0)], [pad(9.0, 3.0, 0)]]}]
    }));
    assert!(s.nets[0].open.is_empty());
    let path = &s.nets[0].paths[0];
    assert_eq!(path.cells.len(), 2, "{:?}", path.cells);
    assert_eq!((path.from, path.to), (Some([0, 0]), Some([1, 0])));
}

/// One layer, a wall with one gap a single track wide. A's shortest way is through the gap, but
/// it can also go round the wall's end; B's pads are such that only the gap joins them. Routed
/// one after the other, shortest first, A takes the gap and B stays open.
#[test]
fn the_net_with_another_way_leaves_the_gap_to_the_one_without() {
    let wall = |x0: f64, x1: f64| json!({"box": [x0, 4.9, x1, 5.1], "layers": [0]});
    let s = solve(json!({
        "box": [0, 0, 12, 10], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "edge": [0, 0, 12, 10], "edge_clear": 0.2,
        "fixed": [wall(3.0, 5.6), wall(6.4, 12.0), {"box": [5.0, 0.0, 5.2, 3.2], "layers": [0]},
                  {"box": [5.0, 6.8, 5.2, 10.0], "layers": [0]}, {"box": [7.0, 0.0, 7.2, 3.2], "layers": [0]},
                  {"box": [7.0, 6.8, 7.2, 10.0], "layers": [0]}, {"box": [0.0, 3.6, 5.0, 3.8], "layers": [0]},
                  {"box": [0.0, 6.2, 5.0, 6.4], "layers": [0]}],
        "nets": [
            {"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(6.0, 4.2, 0)], [pad(6.0, 5.8, 0)]]},
            {"name": "B", "width": 0.2, "clearance": 0.2, "pieces": [[pad(6.0, 1.0, 0)], [pad(6.0, 9.0, 0)]]}
        ]
    }));
    for net in &s.nets {
        assert!(
            net.open.is_empty(),
            "{} open after {} rounds, conflicts {:?}",
            net.name,
            s.iterations,
            s.conflicts
        );
    }
    let all = lines(&s);
    for a in &all {
        for b in &all {
            if a.0 != b.0 && a.1 == b.1 {
                assert!(gap(a, b) >= 4.0 - 0.3, "tracks of two nets {} cells apart", gap(a, b));
            }
        }
    }
}

#[test]
fn two_nets_that_cross_use_the_second_layer() {
    let s = solve(json!({
        "box": [0, 0, 10, 10], "step": 0.1, "layers": 2, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "edge": [0, 0, 10, 10], "edge_clear": 0.3,
        "fixed": [{"box": [0.0, 0.0, 4.0, 0.6], "layers": [0, 1]}, {"box": [6.0, 0.0, 10.0, 0.6], "layers": [0, 1]}],
        "nets": [
            {"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(0.6, 5.0, 0)], [pad(9.4, 5.0, 0)]]},
            {"name": "B", "width": 0.2, "clearance": 0.2, "pieces": [[pad(5.0, 0.6, 0)], [pad(5.0, 9.4, 0)]]}
        ]
    }));
    assert!(s.nets.iter().all(|n| n.open.is_empty()), "{:?}", s.conflicts);
    let vias: usize = s
        .nets
        .iter()
        .flat_map(|n| &n.paths)
        .map(|p| p.cells.windows(2).filter(|w| w[0][0] != w[1][0]).count())
        .sum();
    assert_eq!(vias, 2);
    let all = lines(&s);
    for a in &all {
        for b in &all {
            if a.0 != b.0 && a.1 == b.1 {
                assert!(gap(a, b) >= 4.0 - 0.3);
            }
        }
    }
}

#[test]
fn a_net_of_three_pads_becomes_a_tree() {
    let s = solve(json!({
        "box": [0, 0, 10, 10], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "nets": [{"name": "A", "width": 0.2, "clearance": 0.2,
                  "pieces": [[pad(1.0, 5.0, 0)], [pad(9.0, 5.0, 0)], [pad(8.0, 9.0, 0)]]}]
    }));
    assert!(s.nets[0].open.is_empty());
    assert_eq!(s.nets[0].paths.len(), 2);
    assert!(
        s.nets[0].paths[1].from.is_none(),
        "the third pad joins the track, not a pad: {:?}",
        s.nets[0].paths[1]
    );
}

#[test]
fn a_shut_in_pad_is_reported_open() {
    let s = solve(json!({
        "box": [0, 0, 10, 10], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "fixed": [{"box": [4.9, 0.0, 5.1, 10.0], "layers": [0]}],
        "nets": [{"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(1.0, 5.0, 0)], [pad(9.0, 5.0, 0)]]}]
    }));
    assert_eq!(s.nets[0].open, vec![1]);
}

/// A wall with one gap, a single track wide; both nets are shortest through the gap and either can
/// go round the wall's end instead (2.4 mm to the side: a way round is looked for within 3 mm of
/// a net). The one whose weight is larger gets the gap.
#[test]
fn the_heavier_net_gets_the_short_way() {
    let wall = |x0: f64, x1: f64| json!({"box": [x0, 4.9, x1, 5.1], "layers": [0]});
    for heavy in ["A", "B"] {
        let net = |name: &str, x: f64| {
            json!({"name": name, "width": 0.2, "clearance": 0.2, "weight": if name == heavy { 5.0 } else { 1.0 },
                   "pieces": [[pad(x, 4.0, 0)], [pad(x, 6.0, 0)]]})
        };
        let s = solve(json!({
            "box": [0, 0, 12, 10], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
            "edge": [0, 0, 12, 10], "edge_clear": 0.2,
            "fixed": [wall(3.0, 5.6), wall(6.4, 9.0)],
            "nets": [net("A", 5.0), net("B", 7.0)]
        }));
        assert!(s.nets.iter().all(|n| n.open.is_empty()));
        let (short, long) = if heavy == "A" { (&s.nets[0], &s.nets[1]) } else { (&s.nets[1], &s.nets[0]) };
        assert!(short.cost <= short.alone + 0.3, "{}: {} mm, alone {} mm", short.name, short.cost, short.alone);
        assert!(long.cost > long.alone + 2.0, "{}: {} mm, alone {} mm", long.name, long.cost, long.alone);
        assert!(s.nets.iter().all(|n| n.cost + 1e-6 >= n.alone));
    }
}

/// A net that the rounds pushed onto a long way keeps it only as long as the short one is taken.
#[test]
fn no_net_costs_less_than_alone_on_the_board() {
    let s = solve(json!({
        "box": [0, 0, 10, 10], "step": 0.1, "layers": 2, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "nets": [
            {"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(1.0, 5.0, 0)], [pad(9.0, 5.0, 0)]]},
            {"name": "B", "width": 0.2, "clearance": 0.2, "pieces": [[pad(5.0, 1.0, 0)], [pad(5.0, 9.0, 0)]]},
            {"name": "C", "width": 0.2, "clearance": 0.2, "pieces": [[pad(1.0, 1.0, 0)], [pad(2.0, 1.0, 0)]]}
        ]
    }));
    assert!(s.nets.iter().all(|n| n.open.is_empty() && n.cost + 1e-6 >= n.alone && n.alone > 0.0));
    assert!(
        (s.nets[2].cost - s.nets[2].alone).abs() < 1e-6,
        "a net with nothing in its way is as short as alone"
    );
    assert!(!s.shortened.is_empty());
}

/// Asked only whether each net has a way at all: two nets that would cross on one layer both
/// have one (each is alone when asked), the one behind a wall has none; no tracks come back.
#[test]
fn alone_says_which_nets_have_no_way_at_all() {
    let s = solve(json!({
        "box": [0, 0, 10, 10], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0}, "alone": true,
        "fixed": [{"box": [0.0, 7.4, 10.0, 7.6], "layers": [0]}],
        "nets": [
            {"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(1.0, 4.0, 0)], [pad(9.0, 4.0, 0)]]},
            {"name": "B", "width": 0.2, "clearance": 0.2, "pieces": [[pad(5.0, 1.0, 0)], [pad(5.0, 6.5, 0)]]},
            {"name": "C", "width": 0.2, "clearance": 0.2, "pieces": [[pad(5.0, 2.0, 0)], [pad(5.0, 9.0, 0)]]}
        ]
    }));
    let open: Vec<usize> = s.nets.iter().map(|n| n.open.len()).collect();
    assert_eq!(open, vec![0, 0, 1]);
    assert!(s.nets.iter().all(|n| n.paths.is_empty()) && s.nets[0].alone > 7.0 && s.iterations == 0);
}

/// With no patience at all for rounds that bring no fewer conflicts, the rounds start over from a
/// low price up to three times and then end: what can be routed still is.
#[test]
fn rounds_that_stall_start_over_and_still_end() {
    let s = solve(json!({
        "box": [0, 0, 10, 10], "step": 0.1, "layers": 2, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0}, "stall": 0, "max_iter": 50,
        "nets": [
            {"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(1.0, 5.0, 0)], [pad(9.0, 5.0, 0)]]},
            {"name": "B", "width": 0.2, "clearance": 0.2, "pieces": [[pad(5.0, 1.0, 0)], [pad(5.0, 9.0, 0)]]}
        ]
    }));
    assert!(s.nets.iter().all(|n| n.open.is_empty()) && s.iterations <= 6, "{} rounds", s.iterations);
}

/// A pin between two others, 0.5 mm apart: a 0.6 mm track has no way out of it, a 0.15 mm one
/// has. The net's other pad is 8 mm away with nothing in between. The track is narrow at the pin
/// and wide from where there is room, whichever of the two pads the net starts from.
#[test]
fn a_narrow_track_is_only_as_long_as_it_must_be() {
    let pin = json!({"box": [0.6, 2.875, 1.4, 3.125], "layers": [0]});
    for swapped in [false, true] {
        let pieces = if swapped {
            json!([[pad(9.0, 3.0, 0)], [pin]])
        } else {
            json!([[pin], [pad(9.0, 3.0, 0)]])
        };
        let s = solve(json!({
            "box": [0, 0, 10, 6], "step": 0.05, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
            "fixed": [{"box": [0.6, 2.375, 1.4, 2.625], "layers": [0]}, {"box": [0.6, 3.375, 1.4, 3.625], "layers": [0]},
                      {"box": [0.2, 2.375, 0.4, 3.625], "layers": [0]}],
            "nets": [{"name": "A", "width": 0.6, "narrow": 0.15, "clearance": 0.2, "pieces": pieces}]
        }));
        assert!(s.nets[0].open.is_empty());
        let length = |narrow: bool| -> f64 {
            let mut sum = 0.0;
            for path in s.nets[0].paths.iter().filter(|p| (p.width < 0.3) == narrow) {
                for w in path.cells.windows(2) {
                    sum += (((w[1][1] - w[0][1]).pow(2) + (w[1][2] - w[0][2]).pow(2)) as f64).sqrt() * 0.05;
                }
            }
            sum
        };
        assert!(
            length(true) > 0.2 && length(true) < 1.5,
            "narrow for {} mm (started at the pin: {})",
            length(true),
            !swapped
        );
        assert!(length(false) > 6.0, "wide for {} mm", length(false));
    }
}
