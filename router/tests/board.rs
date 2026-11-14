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
        weights: Default::default(),
        only: Vec::new(),
        pours: true,
        room: 0.8,
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

/// A pour on a layer with tracks is given to the router with the pieces that hang in it: whether
/// it still joins them is seen there, as the tracks come to lie.
#[test]
fn copper_on_a_pour_of_its_net_hangs_in_it() {
    let board = Board::parse(&board_text()).unwrap();
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &options()).unwrap();
    let names: Vec<&str> = p.nets.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, vec!["+5V", "GND", "SIG"]);
    let gnd = &p.nets[1];
    assert_eq!((gnd.pieces.len(), gnd.pours.len()), (2, 1), "R2's pad and the header's pin, in one pour");
    assert_eq!(
        (gnd.pours[0].layer, gnd.pours[0].pieces.clone(), gnd.pours[0].outline.len()),
        (1, vec![0, 1], 4)
    );
    assert!(p.nets[0].pours.is_empty() && p.nets[2].pours.is_empty());
    let sig = &p.nets[2];
    assert_eq!(sig.pieces.len(), 3, "two pads and a hand-drawn track that touches neither");
    assert_eq!((p.nets[0].width, p.nets[0].narrow, sig.width), (0.5, Some(0.2), 0.2));
    assert_eq!(p.keepouts.len(), 1);
    assert_eq!(p.fixed.len(), 4, "the four lines of the outline");
    // routing one net only: the other one's pads are in the way like any copper
    let all: usize = p.nets[0].pieces.iter().map(|piece| piece.len()).sum();
    let one = problem(
        &board,
        &rules,
        &Options {
            only: vec!["SIG".into()],
            ..options()
        },
    )
    .unwrap();
    assert_eq!(one.nets.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), vec!["SIG"]);
    assert_eq!(one.fixed.len(), 2 + 4 + all);
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
    // read again, only the net with the pour is left to look at, and nothing of it is apart
    let left = problem(&again, &rules, &options()).unwrap();
    assert_eq!(left.nets.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), vec!["GND"]);
    let rest = route(&left, &mut |_| {});
    assert!(rest.nets[0].open.is_empty() && rest.nets[0].paths.is_empty(), "{:?}", rest.nets[0]);
}

/// A pad whose copper lies off its hole, and a text on copper: both are where a track must not
/// go. (Read without them, a transistor's pad was 0.4 mm from where it is and a track ran
/// across it; a text on the front was written over.)
#[test]
fn a_pad_off_its_hole_and_a_text_on_copper_are_in_the_way() {
    let text = board_text().replace(
        "(gr_line (start 0 0)",
        r#"(footprint "Q" (layer "F.Cu") (at 4 10 90) (property "Reference" "Q1" (at 0 0 0))
    (pad "1" thru_hole rect (at 0 0 90) (size 1.1 1.8) (drill 0.75 (offset 0 0.4)) (layers "*.Cu") (net "SIG") (uuid "u")))
  (gr_text "AB" (at 15 9 0) (layer "F.Cu") (uuid "u") (effects (font (size 1 1) (thickness 0.15))))
  (gr_line (start 12 4) (end 16 4) (stroke (width 0.3) (type default)) (layer "F.Cu"))
  (gr_line (start 0 0)"#,
    );
    let board = Board::parse(&text).unwrap();
    let q = board.pads.iter().find(|p| p.reference == "Q1").unwrap();
    // turned by 90 degrees the offset (0, 0.4) points to the right, and the pad is 1.8 wide and 1.1 high
    assert!(close(q.rect, [4.4 - 0.9, 10.0 - 0.55, 4.4 + 0.9, 10.0 + 0.55]), "{:?}", q.rect);
    assert_eq!(q.at, [4.0, 10.0]);
    assert_eq!(board.drawn.len(), 2);
    let words = board.drawn.iter().find(|d| d.rect.is_some()).unwrap().rect.unwrap();
    assert!(words[0] < 14.2 && words[2] > 15.8 && words[1] < 8.6 && words[3] > 9.4, "{:?}", words);
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &options()).unwrap();
    let nobody: Vec<_> = p.fixed.iter().filter(|s| s.layers == vec![0] && s.hole == 0.0).collect();
    assert_eq!(nobody.len(), 2, "the text and the line, on the front, of no net");
}

/// A text on copper is in the way where it is, not far to either side of it: KiCad sets these
/// seven letters 6.35 mm wide and 1.7 mm high, and a line end written into the text is no letter.
#[test]
fn a_text_on_copper_is_about_as_wide_as_it_is_set() {
    let text = board_text().replace(
        "(gr_line (start 0 0)",
        r#"(gr_text "JTAG_EN\n" (at 10 9 0) (layer "F.Cu") (uuid "u") (effects (font (size 1 1) (thickness 0.15))))
  (gr_line (start 0 0)"#,
    );
    let board = Board::parse(&text).unwrap();
    let words = board.drawn.iter().find(|d| d.rect.is_some()).unwrap().rect.unwrap();
    let (wide, high) = (words[2] - words[0], words[3] - words[1]);
    assert!((6.35..8.0).contains(&wide) && (1.7..2.0).contains(&high), "{} x {}", wide, high);
    assert!(
        (words[0] + words[2] - 20.0).abs() < 1e-9 && (words[1] + words[3] - 18.0).abs() < 1e-9,
        "about its place"
    );
}

/// What the command line does by itself: a via costs as much as 6 mm of track, and pours on
/// layers with tracks are not trusted.
#[test]
fn the_defaults_of_the_board_mode() {
    let d = Options::default();
    assert_eq!((d.step, d.via_cost, d.max_iter, d.pours, d.room), (0.05, 6.0, 200, false, 0.8));
    assert!(d.hole_clear.is_none() && d.weights.is_empty() && d.only.is_empty());
}

/// What the board's constraints ask for at least goes before the net class.
#[test]
fn the_boards_least_widths_go_before_the_net_class() {
    let project = PROJECT.replace(
        r#""min_hole_clearance": 0.25,"#,
        r#""min_hole_clearance": 0.25, "min_track_width": 0.3, "min_clearance": 0.3, "min_via_diameter": 1.5,
           "min_via_annular_width": 0.3, "min_through_hole_diameter": 0.4,"#,
    );
    let board = Board::parse(&board_text()).unwrap();
    let rules = Rules::parse(&project, &board).unwrap();
    assert_eq!((rules.track, rules.clearance, rules.via), (0.3, 0.3, (1.5, 0.4)));
    assert_eq!(rules.width.get("+5V"), Some(&0.5));
    let older = PROJECT.replace(
        r#""netclass_patterns": ["#,
        r#""netclass_assignments": {"SIG": "Power", "GND": ["Mains"]}, "netclass_patterns": ["#,
    );
    let rules = Rules::parse(&older, &board).unwrap();
    assert_eq!((rules.width.get("SIG"), rules.wider.get("GND")), (Some(&0.5), Some(&0.6)));
}

/// A pad on a pour of its net asks tracks for room. The pour joins it if pours are trusted;
/// if not, it is a piece like any other (the pour is on a layer with tracks: they may cut it).
#[test]
fn pads_on_a_pour_ask_for_room_or_are_routed_like_the_rest() {
    let board = Board::parse(&board_text()).unwrap();
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &options()).unwrap();
    assert_eq!(p.rooms.len(), 2, "R2's ground pad and the header's ground pin, on the back");
    assert!(p.rooms.iter().all(|r| r.room == 0.8 && r.layers == vec![1]));
    let all = problem(&board, &rules, &Options { pours: false, ..options() }).unwrap();
    let names: Vec<&str> = all.nets.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, vec!["+5V", "GND", "SIG"]);
    assert_eq!(all.rooms.len(), 2);
}

/// Four layers, the inner two planes. GND is poured on In1: the header's drilled ground pin is on
/// it; the resistor's ground pad on the back is not, and gets a net of its own with the plane to
/// end in. Routed, it has a via inside the plane, and read again nothing of GND is apart.
#[test]
fn a_pad_off_its_plane_is_given_a_way_into_it() {
    let text = board_text()
        .replace(
            r#"(0 "F.Cu" signal) (2 "B.Cu" signal)"#,
            r#"(0 "F.Cu" signal) (4 "In1.Cu" power) (6 "In2.Cu" power) (2 "B.Cu" signal)"#,
        )
        .replace(r#"(zone (net "GND") (layer "B.Cu")"#, r#"(zone (net "GND") (layer "In1.Cu")"#)
        .replace(r#"(filled_polygon (layer "B.Cu")"#, r#"(filled_polygon (layer "In1.Cu")"#);
    let board = Board::parse(&text).unwrap();
    assert_eq!(board.copper.iter().map(|c| c.1).collect::<Vec<_>>(), vec![true, false, false, true]);
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &Options { pours: false, ..options() }).unwrap();
    let gnd: Vec<_> = p.nets.iter().filter(|n| n.name == "GND").collect();
    assert_eq!(gnd.len(), 1, "one way to make: the resistor's pad; the header's pin is on the plane");
    assert_eq!((gnd[0].pieces.len(), gnd[0].plane.len(), gnd[0].plane[0].len()), (1, 1, 4));
    assert!(
        p.fixed.iter().any(|s| s.hole > 0.0 && s.rect.is_some_and(|r| (r[0] - 10.67).abs() < 1e-9)),
        "the pin on the plane is in the way"
    );
    let solution = route(&p, &mut |_| {});
    assert!(solution.nets.iter().all(|n| n.open.is_empty()));
    let (_tracks, vias) = copper_of(&p, &solution);
    let into: Vec<_> = vias.iter().filter(|v| v.0 == "GND").collect();
    assert!(
        !into.is_empty() && into.iter().any(|v| v.1[0] >= 9.0 && v.1[0] <= 19.0 && v.1[1] >= 1.0 && v.1[1] <= 11.0),
        "{:?}",
        into
    );
}

/// A pad of a part that lies at 45 degrees is that pad turned, not the box round it.
#[test]
fn a_pad_at_an_angle_keeps_its_shape() {
    let text = board_text()
        .replace("(at 5 6 90)", "(at 5 6 45)")
        .replace("(at -1 0 90)", "(at -1 0 45)")
        .replace("(at 1 0 90)", "(at 1 0 45)");
    let board = Board::parse(&text).unwrap();
    let pad = board.pads.iter().find(|p| p.reference == "R1" && p.number == "1").unwrap();
    let (own, angle) = pad.turned.expect("turned");
    let middle = [5.0 - 0.5f64.sqrt(), 6.0 + 0.5f64.sqrt()];
    assert!(
        close(own, [middle[0] - 0.5, middle[1] - 0.25, middle[0] + 0.5, middle[1] + 0.25]) && angle == 45.0,
        "{:?}",
        own
    );
    assert!(pad.rect[2] - pad.rect[0] > 1.0, "the box round it is wider than the pad is long or broad");
    assert!(board.pads.iter().filter(|p| p.reference != "R1").all(|p| p.turned.is_none()));
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &options()).unwrap();
    let turned: Vec<_> = p.nets.iter().flat_map(|n| n.pieces.iter().flatten()).filter(|s| s.angle != 0.0).collect();
    assert_eq!(turned.len(), 2);
}

/// Two pads of one net side by side on a part at 45 degrees do not touch because the boxes round
/// them do: they are two pieces, with a connection to make.
#[test]
fn turned_pads_side_by_side_are_two_pieces() {
    let pad = |n: &str, y: f64| {
        format!("(pad \"{n}\" smd roundrect (at 0 {y} 45) (size 1 0.3) (layers \"F.Cu\") (roundrect_rratio 0.25) (net \"SIG\") (uuid \"u\"))")
    };
    let part = format!(
        "(footprint \"U\" (layer \"F.Cu\") (at 12 7 45) (property \"Reference\" \"U1\" (at 0 0 0)) {} {})\n  (gr_line (start 0 0)",
        pad("1", -0.3),
        pad("2", 0.3)
    );
    let text = board_text().replacen("(gr_line (start 0 0)", &part, 1);
    let board = Board::parse(&text).unwrap();
    let (a, b) = (
        board.pads.iter().find(|p| p.reference == "U1" && p.number == "1").unwrap(),
        board.pads.iter().find(|p| p.reference == "U1" && p.number == "2").unwrap(),
    );
    // 0.6 mm from middle to middle and 0.3 mm broad: 0.3 mm of room between them, and the boxes overlap
    assert!(a.turned.is_some() && a.rect[0] < b.rect[2] && b.rect[0] < a.rect[2] && a.rect[1] < b.rect[3] && b.rect[1] < a.rect[3]);
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &options()).unwrap();
    let sig = p.nets.iter().find(|n| n.name == "SIG").unwrap();
    let alone = sig.pieces.iter().filter(|piece| piece.len() == 1 && piece[0].angle != 0.0).count();
    assert_eq!(alone, 2, "{} pieces", sig.pieces.len());
}

/// A pad drawn as an L is its two bars, not the box round them: the corner between them is free.
#[test]
fn a_pad_drawn_as_a_polygon_is_the_boxes_it_is_made_of() {
    let part = "(footprint \"U\" (layer \"F.Cu\") (at 12 7 90) (property \"Reference\" \"U1\" (at 0 0 0))
    (pad \"1\" smd custom (at 0 0 90) (size 0.01 0.01) (layers \"F.Cu\") (net \"SIG\") (options (clearance outline) (anchor circle))
      (primitives (gr_poly (pts (xy 0.6 0.5) (xy -0.1 0.5) (xy -0.1 -0.5) (xy 0.1 -0.5) (xy 0.1 0.3) (xy 0.6 0.3)) (width 0.1) (fill yes))) (uuid \"u\")))
  (gr_line (start 0 0)";
    let text = board_text().replacen("(gr_line (start 0 0)", part, 1);
    let board = Board::parse(&text).unwrap();
    let pads: Vec<_> = board.pads.iter().filter(|p| p.reference == "U1").collect();
    let covers = |x: f64, y: f64| pads.iter().any(|p| p.rect[0] <= x && x <= p.rect[2] && p.rect[1] <= y && y <= p.rect[3]);
    // turned by 90 degrees (anticlockwise, y down), a point (x, y) of the pad lies at (12 + y, 7 - x)
    assert_eq!(pads.len(), 3, "the anchor and two bars");
    assert!(covers(12.0 - 0.4, 7.0) && covers(12.0 + 0.4, 7.0 - 0.5), "the long bar and the foot");
    assert!(!covers(12.0 - 0.2, 7.0 - 0.45), "the corner between them");
}

/// An outer layer of type power is a plane only if something is poured on it; a teardrop is not.
#[test]
fn an_outer_power_layer_with_no_pour_is_routed_on() {
    let typed = board_text().replace(r#"(2 "B.Cu" signal)"#, r#"(2 "B.Cu" power)"#);
    let board = Board::parse(&typed).unwrap();
    assert_eq!((board.copper[1].1, board.freed.len()), (false, 0), "GND is poured on the back");
    let teardrop = typed.replace(
        r#"(zone (net "GND") (layer "B.Cu")"#,
        r#"(zone (net "GND") (layer "B.Cu") (attr (teardrop (type padvia)))"#,
    );
    let board = Board::parse(&teardrop).unwrap();
    assert_eq!((board.copper[1].1, board.freed.clone()), (true, vec!["B.Cu".to_string()]));
    let front = board_text().replace(r#"(0 "F.Cu" signal)"#, r#"(0 "F.Cu" power)"#);
    let board = Board::parse(&front).unwrap();
    assert_eq!(board.copper.iter().map(|c| c.1).collect::<Vec<_>>(), vec![true, true]);
}

/// A net poured as two islands on a plane layer that nothing joins: the larger one is its plane,
/// and what hangs on the smaller one gets a way into the larger like any other piece.
#[test]
fn of_two_islands_of_a_plane_the_larger_is_the_plane() {
    let text = board_text()
        .replace(
            r#"(0 "F.Cu" signal) (2 "B.Cu" signal)"#,
            r#"(0 "F.Cu" signal) (4 "In1.Cu" power) (6 "In2.Cu" power) (2 "B.Cu" signal)"#,
        )
        .replace(r#"(zone (net "GND") (layer "B.Cu")"#, r#"(zone (net "GND") (layer "In1.Cu")"#)
        .replace(
            r#"(filled_polygon (layer "B.Cu") (pts (xy 9 1) (xy 19 1) (xy 19 11) (xy 9 11)))"#,
            r#"(filled_polygon (layer "In1.Cu") (pts (xy 13 1) (xy 19 1) (xy 19 11) (xy 13 11)))
    (filled_polygon (layer "In1.Cu") (pts (xy 9 1) (xy 12 1) (xy 12 4) (xy 9 4)))"#,
        );
    let board = Board::parse(&text).unwrap();
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &Options { pours: false, ..options() }).unwrap();
    let gnd: Vec<_> = p.nets.iter().filter(|n| n.name == "GND").collect();
    // the header's pin (x 11.27) hangs on the small island, the resistor's pad is on no island
    assert_eq!(gnd.len(), 2, "the pin on the small island and the pad: each gets its way into the large one");
    assert!(
        gnd.iter().all(|n| n.plane.len() == 1 && n.plane[0].iter().all(|q| q[0] >= 13.0)),
        "{:?}",
        gnd[0].plane
    );
}

/// The outline as it is drawn: a corner cut off by an arc, and a slot. Copper keeps the edge
/// clearance from those lines, not only from the box round the board.
#[test]
fn the_outline_as_drawn_and_its_cut_outs_are_kept_clear() {
    let text = board_text().replace(
        "(gr_line (start 0 0) (end 20 0)",
        r#"(gr_arc (start 17 12) (mid 19.12 11.12) (end 20 9) (stroke (width 0.1) (type default)) (layer "Edge.Cuts"))
  (gr_rect (start 6 10) (end 8 10.6) (stroke (width 0.1) (type default)) (fill none) (layer "Edge.Cuts"))
  (gr_line (start 0 0) (end 20 0)"#,
    );
    let board = Board::parse(&text).unwrap();
    // four lines of the outline, four of the slot, and the quarter circle about (17, 9) in six steps
    assert_eq!(board.cuts.len(), 4 + 4 + 6);
    let on_arc: Vec<_> = board
        .cuts
        .iter()
        .filter(|c| c.seg[0] > 16.9 && c.seg[1] > 8.9 && c.seg[2] > 16.9 && c.seg[3] > 8.9 && c.seg[1] != c.seg[3] && c.seg[0] != c.seg[2])
        .collect();
    assert_eq!(on_arc.len(), 6);
    for c in &on_arc {
        for p in [[c.seg[0], c.seg[1]], [c.seg[2], c.seg[3]]] {
            let r = ((p[0] - 17.0f64).powi(2) + (p[1] - 9.0f64).powi(2)).sqrt();
            assert!((r - 3.0).abs() < 0.01, "a point of the arc at {:?}, {} from its middle", p, r);
        }
    }
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &options()).unwrap();
    let edges: Vec<_> = p.fixed.iter().filter(|s| s.layers == vec![0, 1] && s.hole == 0.0 && s.seg.is_some()).collect();
    assert_eq!(edges.len(), 14);
    assert!(edges.iter().all(|s| s.clearance == 0.3), "the project's edge clearance");
}

/// A pad with a clearance of its own keeps tracks that far off, and a drawing on a solder mask
/// layer is in the way on the copper under it.
#[test]
fn a_pads_own_clearance_and_a_drawn_mask_opening_count() {
    let text = board_text().replace(
        "(gr_line (start 0 0) (end 20 0)",
        r#"(footprint "H" (layer "F.Cu") (at 4 3) (property "Reference" "H1" (at 0 0 0))
    (pad "" np_thru_hole circle (at 0 0) (size 2 2) (drill 2) (layers "*.Cu" "*.Mask") (clearance 1.7) (uuid "u")))
  (gr_rect (start 12 8) (end 14 9) (stroke (width 0.1) (type default)) (fill yes) (layer "B.Mask"))
  (gr_line (start 0 0) (end 20 0)"#,
    );
    let board = Board::parse(&text).unwrap();
    assert_eq!(board.pads.iter().find(|p| p.reference == "H1").unwrap().clearance, Some(1.7));
    assert!(
        board.drawn.iter().any(|d| d.layer == 1 && d.rect.is_some()),
        "the opening in the back's mask, on the back"
    );
    // an opening over a pad is that pad's: R2's pads on the back lie at (14, 6) and (16, 6)
    let over = text.replace("(gr_rect (start 12 8) (end 14 9)", "(gr_rect (start 13 5) (end 17 7)");
    assert!(Board::parse(&over).unwrap().drawn.is_empty());
    let rules = Rules::parse(PROJECT, &board).unwrap();
    let p = problem(&board, &rules, &options()).unwrap();
    let hole = p.fixed.iter().find(|s| s.hole == 1.0).unwrap();
    assert_eq!(hole.clearance, 1.7);
}

/// An inner layer of type signal that a pour fills is a plane all the same.
#[test]
fn an_inner_layer_filled_by_a_pour_is_taken_for_a_plane() {
    let text = board_text()
        .replace(
            r#"(0 "F.Cu" signal) (2 "B.Cu" signal)"#,
            r#"(0 "F.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (2 "B.Cu" signal)"#,
        )
        .replace(r#"(zone (net "GND") (layer "B.Cu")"#, r#"(zone (net "GND") (layer "In1.Cu")"#)
        .replace(
            r#"(filled_polygon (layer "B.Cu") (pts (xy 9 1) (xy 19 1) (xy 19 11) (xy 9 11))"#,
            r#"(filled_polygon (layer "In1.Cu") (pts (xy 1 1) (xy 19 1) (xy 19 11) (xy 1 11))"#,
        );
    let mut board = Board::parse(&text).unwrap();
    assert_eq!(board.copper.iter().map(|c| c.1).collect::<Vec<_>>(), vec![true, true, true, true]);
    assert_eq!(
        board.planes(0.5),
        vec!["In1.Cu".to_string()],
        "180 of 240 square mm are filled; In2 has no pour"
    );
    assert_eq!(board.copper.iter().map(|c| c.1).collect::<Vec<_>>(), vec![true, false, true, true]);
    assert!(board.planes(0.5).is_empty());
}
