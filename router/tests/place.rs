use leiterbahn_route::place::{place, Placed, Problem};
use serde_json::{json, Value};

/// A two-pad part, 2 x 1 mm, pads at its ends; turned by 90 degrees it is 1 x 2 mm.
fn part(name: &str, x: f64, y: f64, more: Value) -> Value {
    let wide = |a: &str, b: &str| json!({"box": [-1.0, -0.5, 1.0, 0.5], "pads": {a: [-0.7, 0.0], b: [0.7, 0.0]}});
    let tall = |a: &str, b: &str| json!({"box": [-0.5, -1.0, 0.5, 1.0], "pads": {a: [0.0, -0.7], b: [0.0, 0.7]}});
    let mut v = json!({"name": name, "at": [x, y, 0, false],
        "shapes": {"0/F": wide("1", "2"), "180/F": wide("2", "1"), "90/F": tall("2", "1"), "270/F": tall("1", "2"),
                   "0/B": wide("2", "1"), "180/B": wide("1", "2"), "90/B": tall("2", "1"), "270/B": tall("1", "2")}});
    for (k, val) in more.as_object().unwrap() {
        v[k] = val.clone();
    }
    v
}

fn run(problem: Value) -> (Problem, Placed) {
    let problem: Problem = serde_json::from_value(problem).unwrap();
    let placed = place(&problem, &mut |_| {}).unwrap();
    (problem, placed)
}

fn at<'a>(placed: &'a Placed, name: &str) -> &'a (String, f64, f64, u32, bool) {
    placed.parts.iter().find(|p| p.0 == name).unwrap()
}

#[test]
fn a_part_moves_to_what_it_is_connected_to_and_keeps_its_distance() {
    let (_, placed) = run(json!({
        "size": [40, 20], "edge": 0.5, "gap": 0.2, "back_cost": 2.5, "moves": 4000,
        "parts": [part("J1", 5.0, 10.0, json!({"fixed": true})), part("R1", 34.0, 4.0, json!({}))],
        "nets": [{"name": "A", "pins": [[0, "2"], [1, "1"]]}]
    }));
    assert_eq!(placed.parts.len(), 1, "the fixed part is not in the answer");
    let r = at(&placed, "R1");
    // its pad 1 beside J1's pad 2 at (5.7, 10): as near as two courtyards 0.2 mm apart allow
    assert!(placed.after.nets < 1.2, "{:?}", placed.after);
    assert!(placed.before.nets > 25.0 && placed.moved == 1 && !r.4, "on the front: the back costs");
    let (w, h) = if r.3 % 180 == 0 { (1.0, 0.5) } else { (0.5, 1.0) };
    let apart = ((r.1 - 5.0).abs() - w - 1.0).max((r.2 - 10.0).abs() - h - 0.5);
    assert!(apart >= 0.2 - 1e-6, "{:?}", r);
}

#[test]
fn the_rules_hold() {
    let (_, placed) = run(json!({
        "size": [40, 20], "edge": 0.5, "gap": 0.2, "moves": 4000,
        "keeps": [{"box": [0.0, 0.0, 12.0, 20.0], "sides": "F"}],
        "parts": [part("J1", 5.0, 10.0, json!({"fixed": true})),
                  part("R1", 34.0, 4.0, json!({"inside": [[[20.0, 0.0, 40.0, 20.0], [0.0, 0.0, 3.0, 3.0]]]})),
                  part("R2", 34.0, 16.0, json!({}))],
        "nets": [{"name": "A", "pins": [[0, "2"], [1, "1"]]}, {"name": "B", "pins": [[0, "1"], [2, "1"]]}]
    }));
    let (r1, r2) = (at(&placed, "R1"), at(&placed, "R2"));
    let half = |r: &(String, f64, f64, u32, bool)| if r.3 % 180 == 0 { 1.0 } else { 0.5 };
    assert!(r1.1 - half(r1) >= 20.0 - 1e-6 && r1.1 < 22.0, "as far left as its area lets it: {:?}", r1);
    // R2 may not be on the front where the keep-out is: on the back under J1, or at the keep-out's edge
    assert!(r2.4 || r2.1 - half(r2) >= 12.0 - 1e-6, "{:?}", r2);
    assert!(placed.after.total() < placed.before.total());
}

#[test]
fn the_same_seed_gives_the_same_placement_and_tethers_pull() {
    let problem = json!({
        "size": [40, 20], "edge": 0.5, "gap": 0.2, "moves": 3000, "seed": 7,
        "parts": [part("U1", 30.0, 10.0, json!({"fixed": true})), part("C1", 5.0, 5.0, json!({})), part("C2", 5.0, 15.0, json!({}))],
        "nets": [],
        "tethers": [{"from": [1, "1"], "to": [0, "1"]}, {"from": [2, "1"], "to": [0, "2"], "weight": 2.0}]
    });
    let (_, a) = run(problem.clone());
    let (_, b) = run(problem);
    assert_eq!(a.parts, b.parts);
    assert!(a.after.tethers < 3.0 && a.before.tethers > 60.0, "{:?} -> {:?}", a.before, a.after);
}

/// Eight parts all tied to one pin: without a limit they crowd round it, with one they spread.
#[test]
fn pads_do_not_crowd_beyond_what_a_square_may_hold() {
    let fullest = |dense: Option<f64>| {
        let mut parts = vec![part("U1", 20.0, 10.0, json!({"fixed": true}))];
        let mut tethers = Vec::new();
        for k in 0..8 {
            parts.push(part(&format!("C{}", k), 3.0 + 4.0 * k as f64, 2.0, json!({})));
            tethers.push(json!({"from": [k + 1, "1"], "to": [0, "1"]}));
        }
        let (problem, placed) = run(json!({"size": [40, 20], "edge": 0.5, "gap": 0.2, "moves": 4000, "back_cost": 0.5,
                                           "dense": dense, "crowd": 5.0, "parts": parts, "nets": [], "tethers": tethers}));
        let mut count = std::collections::HashMap::new();
        for (name, x, y, rot, back) in &placed.parts {
            let shape = &problem.parts.iter().find(|p| &p.name == name).unwrap().shapes[&format!("{}/{}", rot, if *back { "B" } else { "F" })];
            for d in shape.pads.values() {
                *count
                    .entry((*back, ((x + d[0]) / 2.0).floor() as i64, ((y + d[1]) / 2.0).floor() as i64))
                    .or_insert(0) += 1;
            }
        }
        (count.values().copied().max().unwrap(), placed.after)
    };
    let (free, _) = fullest(Some(100.0));
    let (held, cost) = fullest(Some(2.0));
    assert!(free >= 3 && held <= 2, "pads in the fullest square: {} without a limit, {} with", free, held);
    assert_eq!(cost.crowd, 0.0);
}
