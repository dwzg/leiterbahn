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
    let (w, h) = if r.3.is_multiple_of(180) { (1.0, 0.5) } else { (0.5, 1.0) };
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
    let half = |r: &(String, f64, f64, u32, bool)| if r.3.is_multiple_of(180) { 1.0 } else { 0.5 };
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

/// Twelve parts in a chain from J1 to J2, set down in a row in the wrong order.
fn chain(more: Value) -> Value {
    let mut parts = vec![part("J1", 3.0, 10.0, json!({"fixed": true})), part("J2", 37.0, 10.0, json!({"fixed": true}))];
    let mut nets = vec![
        json!({"name": "first", "pins": [[0, "2"], [2, "1"]]}),
        json!({"name": "last", "pins": [[13, "2"], [1, "1"]]}),
    ];
    for i in 0..12 {
        parts.push(part(&format!("R{}", i), 6.0 + 2.5 * ((i * 5) % 12) as f64, 3.0, json!({})));
        if i > 0 {
            nets.push(json!({"name": format!("N{}", i), "pins": [[i + 1, "2"], [i + 2, "1"]]}));
        }
    }
    let mut v = json!({"size": [40, 20], "edge": 0.5, "gap": 0.2, "back_cost": 2.5, "moves": 0, "spread": true, "parts": parts, "nets": nets});
    for (k, val) in more.as_object().unwrap() {
        v[k] = val.clone();
    }
    v
}

/// The courtyards of the parts in the answer (all 2 x 1 mm), with the side they are on.
fn yards(placed: &Placed) -> Vec<([f64; 4], bool)> {
    let one = |p: &(String, f64, f64, u32, bool)| {
        let (w, h) = if p.3.is_multiple_of(180) { (1.0, 0.5) } else { (0.5, 1.0) };
        ([p.1 - w, p.2 - h, p.1 + w, p.2 + h], p.4)
    };
    placed.parts.iter().map(one).collect()
}

#[test]
fn forces_bring_a_chain_into_order_and_what_they_leave_is_allowed() {
    // no annealing at all: what comes back is the work of the forces and of setting the parts down
    let (_, placed) = run(chain(json!({"keeps": [{"box": [18.0, 6.0, 22.0, 14.0], "sides": "FB"}]})));
    assert!(
        placed.before.nets > 100.0 && placed.started.nets < 30.0,
        "{:?} -> {:?}",
        placed.before,
        placed.started
    );
    assert_eq!(placed.started, placed.after);
    let yards = yards(&placed);
    let fixed = [[2.0, 9.5, 4.0, 10.5], [36.0, 9.5, 38.0, 10.5]];
    for (i, (a, back)) in yards.iter().enumerate() {
        assert!(
            a[0] >= 0.5 - 1e-6 && a[1] >= 0.5 - 1e-6 && a[2] <= 39.5 + 1e-6 && a[3] <= 19.5 + 1e-6,
            "on the board: {:?}",
            a
        );
        assert!(
            a[2] <= 18.0 + 1e-6 || a[0] >= 22.0 - 1e-6 || a[3] <= 6.0 + 1e-6 || a[1] >= 14.0 - 1e-6,
            "out of the keep-out: {:?}",
            a
        );
        let apart = |b: &[f64; 4]| (a[0] - b[2]).max(b[0] - a[2]).max(a[1] - b[3]).max(b[1] - a[3]);
        for (b, other) in &yards[i + 1..] {
            assert!(back != other || apart(b) >= 0.2 - 1e-6, "{:?} and {:?}", a, b);
        }
        assert!(*back || fixed.iter().all(|b| apart(b) >= 0.2 - 1e-6), "{:?}", a);
    }
    // in order from left to right, as the chain runs
    let x = |name: &str| at(&placed, name).1;
    assert!(x("R0") < x("R4") && x("R4") < x("R8") && x("R8") < x("R11"), "{:?}", placed.parts);
}

#[test]
fn forces_keep_a_part_in_its_rectangle_and_give_the_same_for_the_same_seed() {
    let problem = chain(json!({"moves": 500, "seed": 5}));
    let mut held = problem.clone();
    held["parts"][7]["inside"] = json!([[[28.0, 12.0, 39.5, 19.5]]]);
    held["parts"][7]["at"] = json!([33.0, 16.0, 0, false]);
    let (_, a) = run(held.clone());
    let (_, b) = run(held);
    assert_eq!(a.parts, b.parts);
    let r = at(&a, "R5");
    assert!(r.1 >= 28.5 - 1e-6 && r.2 >= 12.5 - 1e-6, "{:?}", r);
    let (_, free) = run(problem);
    assert!(at(&free, "R5").1 < 28.0, "without the rule it lies in the chain: {:?}", at(&free, "R5"));
}

#[test]
fn a_placement_the_forces_cannot_set_down_stays_as_it_came() {
    // R0 must lie where a keep-out is: there is no place for it, and no part is moved
    let mut problem = chain(json!({"keeps": [{"box": [27.0, 11.0, 39.5, 19.5], "sides": "FB"}]}));
    problem["parts"][2]["inside"] = json!([[[28.0, 12.0, 39.0, 19.0]]]);
    let lines = std::cell::RefCell::new(Vec::new());
    let parsed: Problem = serde_json::from_value(problem.clone()).unwrap();
    let placed = place(&parsed, &mut |line| lines.borrow_mut().push(line)).unwrap();
    assert!(lines.borrow().iter().any(|l| l.contains("stays as it came")), "{:?}", lines);
    assert_eq!(placed.moved, 0);
    assert_eq!(placed.before, placed.after);
}

#[test]
fn a_pad_stays_over_the_area_given_for_it() {
    // R1's pad 2 must lie right of x = 20, its pad 1 left of it: the part straddles the line, however its
    // connection to J1 on the left pulls
    let over = json!({"over": [["2", [[20.0, 0.0, 40.0, 20.0]]], ["1", [[0.0, 0.0, 20.0, 20.0]]]]});
    for spread in [false, true] {
        let (_, placed) = run(json!({
            "size": [40, 20], "edge": 0.5, "gap": 0.2, "moves": 4000, "spread": spread,
            "parts": [part("J1", 5.0, 10.0, json!({"fixed": true})), part("R1", 20.0, 4.0, over.clone())],
            "nets": [{"name": "A", "pins": [[0, "2"], [1, "1"]]}]
        }));
        let r = at(&placed, "R1");
        assert!((r.1 - 20.0).abs() <= 0.7 + 1e-6 && r.3.is_multiple_of(180), "{:?}", r);
        assert!((r.2 - 10.0).abs() < 1.0, "level with J1's pad: {:?}", r);
    }
}
