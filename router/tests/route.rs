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

/// How much of net n's track (mm) is nearer than `apart` mm to the line from (x0, y) to (x1, y).
fn beside_line(s: &Solution, n: usize, y: f64, x0: f64, x1: f64, apart: f64, step: f64) -> f64 {
    let mut sum = 0.0;
    for path in &s.nets[n].paths {
        for w in path.cells.windows(2) {
            for k in 0..20 {
                let t = (k as f64 + 0.5) / 20.0;
                let px = (w[0][1] as f64 + (w[1][1] as f64 - w[0][1] as f64) * t) * step;
                let py = (w[0][2] as f64 + (w[1][2] as f64 - w[0][2] as f64) * t) * step;
                let d = (py - y).hypot((x0 - px).max(px - x1).max(0.0));
                if d < apart {
                    sum += (((w[1][1] as f64 - w[0][1] as f64).powi(2) + (w[1][2] as f64 - w[0][2] as f64).powi(2)).sqrt()) * step / 20.0;
                }
            }
        }
    }
    sum
}

/// A quiet net from left to right, and 0.35 mm beside its straight way a track of a noisy net
/// that stays, 6 mm long. Without the wish to keep apart it runs straight, beside that track all
/// the way; with it, it goes round at the distance asked for, and says how much is left beside.
#[test]
fn a_quiet_net_keeps_away_from_noisy_copper_that_stays() {
    for apart in [0.0, 0.6] {
        let s = solve(json!({
            "box": [0, 0, 10, 6], "step": 0.05, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0}, "apart": apart,
            "fixed": [{"seg": [2.0, 3.35, 8.0, 3.35], "r": 0.075, "layers": [0], "clearance": 0.15, "role": "noisy"}],
            "nets": [{"name": "Q", "width": 0.15, "clearance": 0.15, "role": "quiet", "pieces": [[pad(1.0, 3.0, 0)], [pad(9.0, 3.0, 0)]]}]
        }));
        assert!(s.nets[0].open.is_empty());
        let near = beside_line(&s, 0, 3.35, 2.0, 8.0, 0.55, 0.05);
        if apart == 0.0 {
            assert!(near > 5.5 && s.nets[0].beside == 0.0, "{} mm beside it, said {}", near, s.nets[0].beside);
        } else {
            assert!(near < 0.5 && s.nets[0].beside < 0.5, "{} mm beside it, said {}", near, s.nets[0].beside);
            assert!(s.nets[0].cost < 9.5, "{} mm for a way of 8 mm", s.nets[0].cost);
        }
    }
}

/// A noisy net and a quiet one, both routed, their pads 0.4 mm apart at both ends: side by side
/// all the way unless they are to keep apart. Then one of them bows out, and only what must be
/// near (at the pads) is.
#[test]
fn a_quiet_and_a_noisy_net_that_are_both_routed_part() {
    let pads = |y: f64| json!([[{"box": [0.9, y - 0.1, 1.1, y + 0.1], "layers": [0]}], [{"box": [8.9, y - 0.1, 9.1, y + 0.1], "layers": [0]}]]);
    let mut near = Vec::new();
    for apart in [0.0, 0.6] {
        let s = solve(json!({
            "box": [0, 0, 10, 6], "step": 0.05, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0}, "apart": apart,
            "nets": [{"name": "N", "width": 0.15, "clearance": 0.15, "role": "noisy", "pieces": pads(2.8)},
                     {"name": "Q", "width": 0.15, "clearance": 0.15, "role": "quiet", "pieces": pads(3.2)}]
        }));
        assert!(s.nets[0].open.is_empty() && s.nets[1].open.is_empty());
        near.push(s.nets[0].beside + s.nets[1].beside);
        if apart > 0.0 {
            assert!(s.nets[1].beside < 2.5, "{} mm of the quiet net beside the noisy one", s.nets[1].beside);
        }
    }
    assert_eq!(near[0], 0.0); // nothing is counted where nothing was asked
}

/// Past two walls that stand in its straight way a track has many shapes of one length, and the
/// search leaves it with a bend wherever it stepped aside: 15 corner points here, of which a
/// handful are needed. Straightened it has those, is no longer, and keeps clear as before.
#[test]
fn a_track_is_straightened_without_growing_longer() {
    for (x, y) in [(9.0, 3.4), (9.0, 4.6), (7.0, 5.2)] {
        let mut got = Vec::new();
        for straight in [false, true] {
            let s = solve(json!({
                "box": [0, 0, 10, 6], "step": 0.05, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0}, "straight": straight,
                "fixed": [{"seg": [4.0, 2.0, 4.0, 3.6], "r": 0.075, "layers": [0], "clearance": 0.15},
                          {"seg": [6.0, 3.9, 6.0, 5.6], "r": 0.075, "layers": [0], "clearance": 0.15}],
                "nets": [{"name": "A", "width": 0.15, "clearance": 0.15, "pieces": [[pad(1.0, 3.0, 0)], [pad(x, y, 0)]]}]
            }));
            assert!(s.nets[0].open.is_empty());
            for (wx, y0, y1) in [(4.0, 2.0, 3.6), (6.0, 3.9, 5.6)] {
                for &(_, _, ax, ay, bx, by) in &lines(&s) {
                    for k in 0..=20 {
                        let (px, py) = ((ax + (bx - ax) * k as f64 / 20.0) * 0.05, (ay + (by - ay) * k as f64 / 20.0) * 0.05);
                        let d = (px - wx).hypot((y0 - py).max(py - y1).max(0.0));
                        assert!(d > 0.075 + 0.075 + 0.15 - 0.03, "{} mm from a wall's middle at ({}, {})", d, px, py);
                    }
                }
            }
            got.push((s.nets[0].paths.iter().map(|p| p.cells.len()).sum::<usize>(), s.nets[0].cost));
        }
        assert!(got[0].0 >= 10 && got[1].0 <= 6, "corner points {:?}", got);
        assert!(got[1].1 <= got[0].1 + 1e-6, "cost {:?}", got);
    }
}

/// The net's first pad is shut in by copper that stays; its three other pads are free. They are
/// joined to each other, and one connection is said to be open, not three.
#[test]
fn a_pad_that_is_shut_in_leaves_one_connection_open_not_all() {
    let wall = |a: [f64; 4]| json!({"seg": a, "r": 0.075, "layers": [0], "clearance": 0.15});
    let s = solve(json!({
        "box": [0, 0, 10, 6], "step": 0.05, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "fixed": [wall([1.0, 2.0, 3.0, 2.0]), wall([3.0, 2.0, 3.0, 4.0]), wall([3.0, 4.0, 1.0, 4.0]), wall([1.0, 4.0, 1.0, 2.0])],
        "nets": [{"name": "A", "width": 0.15, "clearance": 0.15,
                  "pieces": [[pad(2.0, 3.0, 0)], [pad(5.0, 1.0, 0)], [pad(8.0, 3.0, 0)], [pad(5.0, 5.0, 0)]]}]
    }));
    assert_eq!(s.nets[0].open.len(), 1, "open: {:?}", s.nets[0].open);
    assert_eq!(s.nets[0].paths.len(), 2);
}

/// Four nets through a gap 1.2 mm wide in a wall, their pads in opposite order on its two sides,
/// and two rounds, which is too few for them to agree. Taking out the net most in the way and
/// laying it in the room left leaves a connection open; one of the 24 orders leaves none.
#[test]
fn nets_that_do_not_agree_are_settled_by_an_order() {
    let mut open = Vec::new();
    for untangle in [false, true] {
        let nets: Vec<_> = (0..4)
            .map(|n| {
                json!({"name": format!("N{}", n), "width": 0.15, "clearance": 0.15,
                       "pieces": [[pad(1.0, 1.0 + n as f64 * 0.8, 0)], [pad(9.0, 1.0 + (3 - n) as f64 * 0.8, 0)]]})
            })
            .collect();
        let s = solve(json!({
            "box": [0, 0, 10, 6], "step": 0.05, "layers": 2, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
            "max_iter": 2, "untangle": untangle, "relax": 0,
            "fixed": [{"seg": [5.0, 0.0, 5.0, 2.0], "r": 0.075, "layers": [0, 1], "clearance": 0.15},
                      {"seg": [5.0, 3.2, 5.0, 6.0], "r": 0.075, "layers": [0, 1], "clearance": 0.15}],
            "nets": nets
        }));
        let tracks = lines(&s);
        for a in &tracks {
            for b in &tracks {
                assert!(a.0 == b.0 || a.1 != b.1 || gap(a, b) * 0.05 >= 0.3 - 1e-6, "two nets too near");
            }
        }
        open.push(s.nets.iter().map(|n| n.open.len()).sum::<usize>());
    }
    assert_eq!(open, [1, 0]);
}

/// Six nets of four pads each, their pads interleaved on a small board with two layers: nets
/// must give way to each other for several rounds. Laid again only where another net lies on
/// them, or whole: both ways every net is joined and no two are nearer than they may be.
#[test]
fn laying_again_only_what_is_in_the_way_gives_a_clean_board_too() {
    for relay in [true, false] {
        let nets: Vec<_> = (0..6)
            .map(|n| {
                let f = n as f64;
                json!({"name": format!("N{}", n), "width": 0.15, "clearance": 0.15,
                       "pieces": [[pad(1.0 + f * 0.9, 1.0, 0)], [pad(6.0 - f * 0.9, 5.0, 0)], [pad(1.0, 1.9 + f * 0.6, 0)], [pad(7.0, 4.6 - f * 0.6, 0)]]})
            })
            .collect();
        let s = solve(json!({
            "box": [0, 0, 8, 6], "step": 0.05, "layers": 2, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0}, "relay": relay, "nets": nets
        }));
        assert!(
            s.nets.iter().all(|n| n.open.is_empty()),
            "relay {}: open {:?}",
            relay,
            s.nets.iter().map(|n| n.open.len()).collect::<Vec<_>>()
        );
        assert!(s.iterations > 2, "the nets were not in each other's way: {} rounds", s.iterations);
        let tracks = lines(&s);
        for a in &tracks {
            for b in &tracks {
                assert!(
                    a.0 == b.0 || a.1 != b.1 || gap(a, b) * 0.05 >= 0.3 - 1e-6,
                    "relay {}: two nets {} mm apart",
                    relay,
                    gap(a, b) * 0.05
                );
            }
        }
    }
}

/// A pad that is to have room round it lies 0.5 mm beside a net's straight way. Without the wish
/// the track passes it at clearance; with it, it goes round at the distance asked, a little longer.
#[test]
fn a_track_gives_room_to_a_pad_that_asks_for_it() {
    let mut nearest = Vec::new();
    for room in [0.0, 0.8] {
        let rooms = if room > 0.0 {
            json!([{"box": [4.8, 3.3, 5.2, 3.7], "layers": [0], "room": room}])
        } else {
            json!([])
        };
        let s = solve(json!({
            "box": [0, 0, 10, 6], "step": 0.05, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
            "fixed": [{"box": [4.8, 3.3, 5.2, 3.7], "layers": [0], "clearance": 0.15}], "rooms": rooms,
            "nets": [{"name": "A", "width": 0.15, "clearance": 0.15, "pieces": [[pad(1.0, 3.0, 0)], [pad(9.0, 3.0, 0)]]}]
        }));
        assert!(s.nets[0].open.is_empty());
        let mut d = f64::MAX;
        for &(_, _, ax, ay, bx, by) in &lines(&s) {
            for k in 0..=40 {
                let (px, py) = ((ax + (bx - ax) * k as f64 / 40.0) * 0.05, (ay + (by - ay) * k as f64 / 40.0) * 0.05);
                d = d.min((4.8 - px).max(px - 5.2).max(0.0).hypot((3.3 - py).max(py - 3.7).max(0.0)));
            }
        }
        nearest.push((d, s.nets[0].cost));
    }
    assert!(nearest[0].0 < 0.35 && nearest[1].0 >= 0.8 + 0.075 - 0.06, "nearest to the pad: {:?}", nearest);
    assert!(nearest[1].1 < nearest[0].1 + 1.5, "cost {:?}", nearest);
}

/// Sixteen pads in a square of four by four, 6 mm apart. Each search is for the nearest pad not
/// yet joined: with more than twelve of them the estimate of the distance was one box round
/// them all, nothing inside it, and every search spread in all directions from all the net
/// had laid (here eight times the cells).
#[test]
fn a_net_of_many_pads_is_searched_towards_the_nearest_pad() {
    let pieces: Vec<_> = (0..16)
        .map(|k| json!([pad(6.0 + 6.0 * (k % 4) as f64, 6.0 + 6.0 * (k / 4) as f64, 0)]))
        .collect();
    let s = solve(json!({
        "box": [0, 0, 30, 30], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0}, "relax": 0,
        "nets": [{"name": "A", "width": 0.2, "clearance": 0.2, "pieces": pieces}]
    }));
    assert!(s.nets[0].open.is_empty() && s.nets[0].paths.len() == 15);
    assert!(s.nets[0].cost < 90.5, "{}", s.nets[0].cost);
    assert!(s.expanded < 20_000, "{} cells opened", s.expanded); // 8580; with the box 66930
}

/// A look says where the nets were in each other's way: two nets on one layer that must cross.
#[test]
fn a_glance_says_where_the_nets_fought() {
    let s = solve(json!({
        "box": [0, 0, 10, 6], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "glance": true, "max_iter": 4,
        "nets": [{"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(0.4, 3.0, 0)], [pad(9.6, 3.0, 0)]]},
                 {"name": "B", "width": 0.2, "clearance": 0.2, "pieces": [[pad(5.0, 0.4, 0)], [pad(5.0, 5.6, 0)]]}]
    }));
    assert!(s.nets.is_empty() && !s.stuck.is_empty());
    let hottest = s.heat.iter().max_by(|a, b| a.heat.total_cmp(&b.heat)).expect("no heat");
    assert!(
        (hottest.x - 4.5).abs() <= 1.0 && (hottest.y - 2.5).abs() <= 1.0 && hottest.layer == 0,
        "{:?}",
        hottest
    );
    assert!(s.heat.iter().all(|h| (h.x - 4.5).abs() < 3.0), "{:?}", s.heat);
}

/// A wall across the board with one opening, B straight through its middle (and on to a pad
/// far away), A's pads to the left and right of B in the opening: A goes round B's pad.
/// Shortening tries A straight across B; B then has no way at all, and used to search all of
/// its area to find that out. It may only get as much longer as A got shorter: the search
/// ends there.
#[test]
fn a_pushed_net_does_not_search_further_than_the_pusher_has_won() {
    let wall = |x0: f64, x1: f64| json!({"box": [x0, 29.9, x1, 30.1], "layers": [0]});
    let s = solve(json!({
        "box": [0, 0, 60, 60], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "fixed": [wall(0.0, 25.5), wall(34.5, 60.0)],
        "nets": [{"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(26.0, 30.0, 0)], [pad(34.0, 30.0, 0)]]},
                 {"name": "B", "width": 0.2, "clearance": 0.2,
                  "pieces": [[pad(30.0, 26.0, 0)], [pad(30.0, 34.0, 0)], [pad(3.0, 3.0, 0)]]}]
    }));
    assert!(s.nets.iter().all(|n| n.open.is_empty()));
    assert!(
        (s.nets[1].cost - 44.0).abs() < 1.0 && s.nets[0].cost > 12.0,
        "{} {}",
        s.nets[0].cost,
        s.nets[1].cost
    );
    assert!(s.expanded < 150_000, "{} cells opened", s.expanded); // 92515; without the limit 282664
}

/// A seed lays the nets in another order: the result is still a clean board, and seed 0 is the order as it was.
#[test]
fn another_seed_routes_the_same_board_clean() {
    let problem = |seed: u64| {
        let nets: Vec<_> = (0..6)
            .map(|k| {
                let y = 2.0 + k as f64;
                json!({"name": format!("N{}", k), "width": 0.2, "clearance": 0.2,
                       "pieces": [[pad(1.0, y, 0)], [pad(19.0, 9.0 - y, 0)], [pad(5.0 + 2.0 * k as f64, if k % 2 == 0 { 1.0 } else { 8.0 }, 0)]]})
            })
            .collect();
        json!({"box": [0, 0, 20, 9], "step": 0.1, "layers": 2, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0}, "seed": seed, "nets": nets})
    };
    let plain = {
        let mut p = problem(0);
        p.as_object_mut().unwrap().remove("seed");
        solve(p)
    };
    let zero = solve(problem(0));
    assert_eq!(serde_json::to_string(&zero.nets).unwrap(), serde_json::to_string(&plain.nets).unwrap());
    assert!(zero.nets.iter().all(|n| n.open.is_empty()));
    for seed in 1..4 {
        let s = solve(problem(seed));
        assert!(s.nets.iter().all(|n| n.open.is_empty()), "seed {}", seed);
    }
}

/// Two tight places on the way: the pin's way out between its neighbours, and a slit in a wall
/// further on, both too narrow for the net's width. No way at full width joins the two ends of
/// the narrow track, and it stayed narrow all along (8 mm). Narrow only through the two places.
#[test]
fn a_track_is_narrow_only_through_each_tight_place() {
    let pin = json!({"box": [0.6, 2.875, 1.4, 3.125], "layers": [0]});
    let s = solve(json!({
        "box": [0, 0, 10, 6], "step": 0.05, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "fixed": [{"box": [0.6, 2.375, 1.4, 2.625], "layers": [0]}, {"box": [0.6, 3.375, 1.4, 3.625], "layers": [0]},
                  {"box": [0.2, 2.375, 0.4, 3.625], "layers": [0]},
                  {"box": [4.9, 0.0, 5.1, 2.6], "layers": [0]}, {"box": [4.9, 3.4, 5.1, 6.0], "layers": [0]}],
        "nets": [{"name": "A", "width": 0.6, "narrow": 0.15, "clearance": 0.2, "pieces": [[pin], [pad(9.0, 3.0, 0)]]}]
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
    assert!(length(true) > 0.5 && length(true) < 3.0, "narrow for {} mm", length(true));
    assert!(length(false) > 4.5, "wide for {} mm", length(false));
    let narrow: Vec<_> = s.nets[0].paths.iter().filter(|p| p.width < 0.3).collect();
    assert_eq!(narrow.len(), 2, "{:?}", s.nets[0].paths);
    // and what is narrow counts by as much as it is narrower: a quarter of the width, four times its length
    let cost = length(false) + 4.0 * length(true);
    assert!(
        (s.nets[0].cost - cost).abs() < 0.2,
        "{} for {} wide and {} narrow",
        s.nets[0].cost,
        length(false),
        length(true)
    );
}

/// A net with a plane: its pad is joined by a way that ends in a via inside the plane's outline,
/// as near as that is, and nothing else needs to be reached.
#[test]
fn a_pad_is_joined_to_its_plane_by_a_via_inside_it() {
    let s = solve(json!({
        "box": [0, 0, 20, 10], "step": 0.1, "layers": 2, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        "fixed": [{"box": [4.0, 0.0, 4.2, 10.0], "layers": [0]}],
        "nets": [{"name": "GND", "width": 0.2, "clearance": 0.2, "pieces": [[pad(2.0, 5.0, 0)]],
                  "plane": [[[8.0, 1.0], [19.0, 1.0], [19.0, 9.0], [8.0, 9.0]]]},
                 {"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(10.0, 2.0, 0)], [pad(10.0, 8.0, 0)]]}]
    }));
    assert!(
        s.nets.iter().all(|n| n.open.is_empty()),
        "{:?}",
        s.nets.iter().map(|n| n.open.clone()).collect::<Vec<_>>()
    );
    let gnd = &s.nets[0];
    // every way of the net: a layer change is a via; the one that joins the plane lies inside it
    let vias: Vec<[u32; 3]> = gnd
        .paths
        .iter()
        .flat_map(|p| p.cells.windows(2).filter(|w| w[0][0] != w[1][0]).map(|w| w[0]))
        .collect();
    assert!(!vias.is_empty());
    let last = gnd.paths.last().unwrap().cells.last().unwrap();
    assert!(last[1] >= 80 && last[1] <= 190 && last[2] >= 10 && last[2] <= 90, "the way ends at {:?}", last);
    assert!(last[1] <= 90, "at the plane's near edge, not deep inside: {:?}", last);
    // the wall on the front between pad and plane: over it on the back, so the way has two vias or starts with one
    assert!(gnd.cost < 6.0 + 2.0 * 3.0 + 1.5, "{}", gnd.cost);
}

/// A box turned by 45 degrees is in the way where it lies, not all over the box round it: a
/// pad inside that outer box is reached, and the track keeps its distance from the turned box.
#[test]
fn a_turned_box_is_in_the_way_only_where_it_lies() {
    let s = solve(json!({
        "box": [0, 0, 10, 10], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0},
        // 6 mm long and 0.4 mm wide about (5, 5), turned (anticlockwise, y down): from (2.9, 7.1) to (7.1, 2.9)
        "fixed": [{"box": [2.0, 4.8, 8.0, 5.2], "angle": 45.0, "layers": [0]}],
        // the first pad is inside the box round the turned one (2.76 to 7.24 both ways), the second beyond it
        "nets": [{"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(3.2, 3.0, 0)], [pad(8.0, 6.0, 0)]]}]
    }));
    assert!(s.nets[0].open.is_empty());
    assert!(s.nets[0].cost > 6.5, "round an end of it, not across: {}", s.nets[0].cost);
    for w in lines(&s).iter() {
        for t in 0..=20 {
            let (x, y) = ((w.2 + (w.4 - w.2) * t as f64 / 20.0) * 0.1, (w.3 + (w.5 - w.3) * t as f64 / 20.0) * 0.1);
            // the point as the turned box sees it: along it and across it from its middle
            let (along, across) = (((x - 5.0) - (y - 5.0)) / 2f64.sqrt(), ((x - 5.0) + (y - 5.0)) / 2f64.sqrt());
            let apart = ((along.abs() - 3.0).max(0.0).powi(2) + (across.abs() - 0.2).max(0.0).powi(2)).sqrt();
            // the clearance and half the track
            assert!(apart > 0.2 + 0.1 - 1e-6, "the track is {} mm from the turned box at ({}, {})", apart, x, y);
        }
    }
}

/// A row of pads 0.2 mm wide at 0.4 mm pitch, a clearance of 0.2 mm, a track of 0.13 mm, a grid
/// of 0.05 mm that the row does not lie on: the cell nearest the middle pad's middle line is
/// 0.02 mm off it, and a track from there passes the neighbours at 0.215 mm. That is allowed,
/// and with half a cell kept on top of the clearance it was not: the pad had no way out.
#[test]
fn a_pad_of_fine_pitch_off_the_grid_has_a_way_out() {
    let pin = |x: f64| json!({"box": [x - 0.1, 0.7, x + 0.1, 1.3], "layers": [0]});
    let s = solve(json!({
        "box": [0, 0, 3, 5], "step": 0.05, "layers": 1, "via": {"d": 0.45, "hole": 0.3, "cost": 3.0},
        "fixed": [pin(1.02), pin(1.82)],
        "nets": [{"name": "A", "width": 0.13, "clearance": 0.2, "pieces": [[pin(1.42)], [pad(1.4, 4.0, 0)]]}]
    }));
    assert!(s.nets[0].open.is_empty(), "the middle pad is shut in");
    for w in lines(&s).iter().filter(|w| w.3.min(w.5) < 30.0) {
        // beside the neighbours (y up to 1.3 mm and the clearance): no nearer to either than clearance and half the track
        for x in [w.2 * 0.05, w.4 * 0.05] {
            assert!(
                (x - 1.12).min(1.72 - x) > 0.2 + 0.065 - 1e-9,
                "a track at x = {} between pads that end at 1.12 and 1.72",
                x
            );
        }
    }
}

/// Two nets that cross on one layer, each with a free way round the other's end, 6 mm from its
/// own pads. A search looks 4 mm about the net's pads and tracks; the track that gives way
/// moves out round by round until it is past the other's end. (What is done for nets that stay
/// stuck, the wider look and the tracks given up about a knot, must not get in the way of this.)
#[test]
fn nets_that_cross_part_within_a_few_rounds() {
    let s = solve(json!({
        "box": [0, 0, 20, 20], "step": 0.1, "layers": 1, "via": {"d": 0.6, "hole": 0.3, "cost": 3.0}, "max_iter": 60,
        "nets": [{"name": "A", "width": 0.2, "clearance": 0.2, "pieces": [[pad(4.0, 10.0, 0)], [pad(16.0, 10.0, 0)]]},
                 {"name": "B", "width": 0.2, "clearance": 0.2, "pieces": [[pad(10.0, 4.0, 0)], [pad(10.0, 16.0, 0)]]}]
    }));
    assert!(s.nets.iter().all(|n| n.open.is_empty()));
    assert!(s.iterations < 12, "{} rounds: {:?}", s.iterations, s.conflicts);
    // one of them goes round an end of the other, the other lies straight
    let (longer, shorter) = (s.nets[0].cost.max(s.nets[1].cost), s.nets[0].cost.min(s.nets[1].cost));
    assert!(longer > 17.0 && longer < 20.0 && shorter < 12.5, "{} and {}", longer, shorter);
}
