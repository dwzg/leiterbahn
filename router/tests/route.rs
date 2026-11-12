use hwroute::problem::{Problem, Solution};
use serde_json::json;

fn pad(x: f64, y: f64, layer: usize) -> serde_json::Value {
    json!({"box": [x - 0.2, y - 0.2, x + 0.2, y + 0.2], "layers": [layer]})
}

fn solve(p: serde_json::Value) -> Solution {
    let problem: Problem = serde_json::from_value(p).unwrap();
    hwroute::route(&problem, &mut |_| {})
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
