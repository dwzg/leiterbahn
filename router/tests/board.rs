use leiterbahn_route::kicad::{copper_of, problem, write, Board, Options, Rules};
use leiterbahn_route::route;

const PROJECT: &str = r#"{"board": {"design_settings": {"rules": {"min_hole_clearance": 0.25, "min_hole_to_hole": 0.25,
  "min_copper_edge_clearance": 0.3}}},
  "net_settings": {"classes": [
    {"name": "Default", "track_width": 0.2, "clearance": 0.2, "via_diameter": 0.6, "via_drill": 0.3},
    {"name": "Power", "track_width": 0.5, "clearance": 0.2, "via_diameter": 0.6, "via_drill": 0.3},
    {"name": "Mains", "track_width": 0.2, "clearance": 0.6, "via_diameter": 0.6, "via_drill": 0.3}],
   "netclass_patterns": [{"netclass": "Power", "pattern": "+5V"}, {"netclass": "Mains", "pattern": "L*"}]}}"#;

/// Two resistors and a header on a 20 x 12 mm board of two layers. R1 lies on its side, R2 on the
/// back. GND is poured on the back; R2's ground pad lies on the pour, the header's drilled ground
/// pin too.
fn board_text() -> String {
    let pad = |n: &str, x: f64, layer: &str, net: &str, angle: f64| {
        format!("(pad \"{n}\" smd roundrect (at {x} 0 {angle}) (size 1 0.5) (layers \"{layer}\") (roundrect_rratio 0.25) (net \"{net}\") (uuid \"u\"))")
    };
    format!(
        r#"(kicad_pcb (version 20260206) (generator "test")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (footprint "R" (layer "F.Cu") (at 5 6 90) (property "Reference" "R1" (at 0 0 0)) {} {})
  (footprint "R" (layer "B.Cu") (at 15 6) (property "Reference" "R2" (at 0 0 0)) {} {})
  (footprint "J" (layer "F.Cu") (at 10 2) (property "Reference" "J1" (at 0 0 0))
    (pad "1" thru_hole circle (at -1.27 0) (size 1.2 1.2) (drill 0.8) (layers "*.Cu" "*.Mask") (net "+5V") (uuid "u"))
    (pad "2" thru_hole circle (at 1.27 0) (size 1.2 1.2) (drill 0.8) (layers "*.Cu" "*.Mask") (net "GND") (uuid "u")))
  (gr_line (start 0 0) (end 20 0) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
  (gr_line (start 20 0) (end 20 12) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
  (gr_line (start 20 12) (end 0 12) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
  (gr_line (start 0 12) (end 0 0) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
  (segment (start 7 9) (end 9 9) (width 0.2) (locked yes) (layer "F.Cu") (net "SIG") (uuid "u"))
  (zone (net "GND") (layer "B.Cu") (uuid "u") (polygon (pts (xy 9 1) (xy 19 1) (xy 19 11) (xy 9 11)))
    (filled_polygon (layer "B.Cu") (pts (xy 9 1) (xy 19 1) (xy 19 11) (xy 9 11))))
  (zone (layers "F.Cu" "B.Cu") (uuid "u") (name "no") (keepout (tracks not_allowed) (vias allowed) (pads allowed))
    (polygon (pts (xy 1 1) (xy 3 1) (xy 3 3) (xy 1 3))))
)
"#,
        pad("1", -1.0, "F.Cu", "+5V", 90.0),
        pad("2", 1.0, "F.Cu", "SIG", 90.0),
        pad("1", -1.0, "B.Cu", "SIG", 0.0),
        pad("2", 1.0, "B.Cu", "GND", 0.0)
    )
}

fn options() -> Options {
    Options {
        step: 0.1,
        via_cost: 3.0,
        max_iter: 50,
        hole_clear: None,
    }
}

fn close(a: [f64; 4], b: [f64; 4]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9)
}

#[test]
fn pads_are_where_kicad_puts_them() {
    let board = Board::parse(&board_text()).unwrap();
    assert_eq!(board.copper, vec![("F.Cu".to_string(), true), ("B.Cu".to_string(), true)]);
    let pad = |reference: &str, number: &str| board.pads.iter().find(|p| p.reference == reference && p.number == number).unwrap();
    // a part turned by 90 degrees (anticlockwise on the sheet, y down): its pad 1 is below its centre
    assert!(close(pad("R1", "1").rect, [4.75, 6.5, 5.25, 7.5]), "{:?}", pad("R1", "1").rect);
    assert!(close(pad("R1", "2").rect, [4.75, 4.5, 5.25, 5.5]));
    assert!(close(pad("R2", "1").rect, [13.5, 5.75, 14.5, 6.25]));
    assert_eq!(
        (pad("R2", "1").layers.clone(), pad("J1", "1").layers.clone(), pad("J1", "1").hole),
        (vec![1], vec![0, 1], 0.4)
    );
    assert_eq!(board.edge, Some([0.0, 0.0, 20.0, 12.0]));
    assert!(board.segments[0].locked && board.zones[1].keepout == Some((true, false)));
}

#[test]
fn rules_come_from_the_project_file() {
    let mut board = Board::parse(&board_text()).unwrap();
    board.pads[0].net = "LINE".to_string();
    let rules = Rules::parse(PROJECT, &board).unwrap();
    assert_eq!((rules.track, rules.clearance, rules.via, rules.hole_clear), (0.2, 0.2, (0.6, 0.3), 0.25));
    assert_eq!(rules.width.get("+5V"), Some(&0.5));
    assert_eq!(rules.wider.get("LINE"), Some(&0.6));
    assert!(!rules.wider.contains_key("+5V") && !rules.width.contains_key("SIG"));
}

#[test]
fn copper_on_a_pour_of_its_net_is_joined_already() {
    let board = Board::parse(&board_text()).unwrap();
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &options()).unwrap();
    let names: Vec<&str> = p.nets.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, vec!["+5V", "SIG"], "GND hangs together by its pour");
    let sig = &p.nets[1];
    assert_eq!(sig.pieces.len(), 3, "two pads and a hand-drawn track that touches neither");
    assert_eq!((p.nets[0].width, p.nets[0].narrow, sig.width), (0.5, Some(0.2), 0.2));
    assert_eq!(p.keepouts.len(), 1);
    assert_eq!(p.fixed.len(), 2, "the two ground pads");
}

#[test]
fn the_routed_board_read_again_has_nothing_in_pieces() {
    let text = board_text();
    let board = Board::parse(&text).unwrap();
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &options()).unwrap();
    let solution = route(&p, &mut |_| {});
    assert!(solution.nets.iter().all(|n| n.open.is_empty()));
    let (tracks, vias) = copper_of(&p, &solution);
    assert!(!tracks.is_empty() && !vias.is_empty(), "SIG has a pad on the back");
    // a track that joins the hand-drawn one ends on its centre line
    let on_line = |q: [f64; 2]| (q[1] - 9.0).abs() < 1e-9 && (7.0..=9.0).contains(&q[0]);
    assert!(tracks.iter().any(|t| t.net == "SIG" && (on_line(t.a) || on_line(t.b))));
    let out = write(&text, &board, &p, &solution, 1).unwrap();
    assert_eq!(out, write(&text, &board, &p, &solution, 1).unwrap(), "the same answer gives the same file");
    let again = Board::parse(&out).unwrap();
    assert_eq!(again.segments.len(), 1 + tracks.len());
    assert_eq!(again.vias.len(), vias.len());
    assert!(again.segments[1..].iter().all(|s| !s.locked));
    let left = problem(&again, &rules, &options()).unwrap();
    assert!(left.nets.is_empty(), "{:?}", left.nets.iter().map(|n| &n.name).collect::<Vec<_>>());
}
