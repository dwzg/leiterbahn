//! leiterbahn-route problem.json result.json
//!     Routes a problem (see `problem.rs`) and writes the tracks as grid paths.
//!
//! leiterbahn-route board in.kicad_pcb out.kicad_pcb [--step 0.05] [--via-cost 6] [--max-iter 200]
//!                        [--hole-clear mm] [--weight NET=4]... [--only NET]... [--problem file.json] [--report file.tsv]
//!                        [--pours trust|ignore] [--room 0.8] [--planes found|typed]
//!     Routes every net of a KiCad board that is in more than one piece and writes the board with
//!     the new tracks. The rules come from the .kicad_pro beside the board. --problem also writes
//!     the problem as it was read from the board, --report a line per net: name, cost of its tracks
//!     in mm, their cost with the net alone on the board, pieces left apart. --via-cost: what a
//!     via costs, as mm of track (more: fewer vias, longer tracks). A pad on a pour of
//!     its net is joined by tracks like any other, unless the pour is on a plane layer; with
//!     --pours trust a pour joins the pads that hang in it for as long as its copper still has
//!     a way from one to the other past the tracks laid, and only a pad that a track has cut
//!     off gets one: less copper where ground is poured. Tracks keep
//!     --room mm from a pad on a pour where they can, for the spokes that hold it. A plane is a
//!     layer of type power: no tracks on it, vias into it (an outer one only if something is
//!     poured on it). --planes found also takes every inner
//!     layer for one that pours fill by more than half. (No share tells a plane from a signal
//!     layer flooded with ground: of five boards, the inner layers their designers left without
//!     tracks were filled by 68 to 73 %, those with up to 539 tracks by 58 to 74 %.)
//!
//! leiterbahn-route place problem.json result.json
//!     Improves a placement (see `place.rs`): parts are shifted, turned and exchanged where that
//!     shortens the connections, within the rules given.
use leiterbahn_route::{kicad, place, problem::Problem, route};
use std::{env, fs, process};

fn fail(message: String) -> ! {
    eprintln!("{}", message);
    process::exit(2);
}

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| fail(format!("{}: {}", path, e)))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() >= 4 && args[1] == "board" {
        let mut options = kicad::Options::default();
        let mut dump = None;
        let mut find_planes = false;
        let mut report = None;
        let mut rest = args[4..].iter();
        while let Some(flag) = rest.next() {
            let value = rest.next().unwrap_or_else(|| fail(format!("{} needs a value", flag)));
            let number = || value.parse::<f64>().unwrap_or_else(|_| fail(format!("{} {}: not a number", flag, value)));
            match flag.as_str() {
                "--step" => options.step = number(),
                "--via-cost" => options.via_cost = number(),
                "--max-iter" => options.max_iter = number() as u32,
                "--hole-clear" => options.hole_clear = Some(number()),
                "--problem" => dump = Some(value.clone()),
                "--report" => report = Some(value.clone()),
                "--only" => options.only.push(value.clone()),
                "--room" => options.room = number(),
                "--planes" => match value.as_str() {
                    "found" => find_planes = true,
                    "typed" => find_planes = false,
                    _ => fail(format!("--planes {}: found or typed", value)),
                },
                "--pours" => match value.as_str() {
                    "trust" => options.pours = true,
                    "ignore" => options.pours = false,
                    _ => fail(format!("--pours {}: trust or ignore", value)),
                },
                "--weight" => match value.rsplit_once('=').and_then(|(net, w)| Some((net, w.parse::<f64>().ok()?))) {
                    Some((net, w)) => drop(options.weights.insert(net.to_string(), w)),
                    None => fail(format!("--weight {}: write NET=number", value)),
                },
                _ => fail(format!("unknown option {}", flag)),
            }
        }
        let text = read(&args[2]);
        let mut board = kicad::Board::parse(&text).unwrap_or_else(|e| fail(format!("{}: {}", args[2], e)));
        for layer in &board.freed {
            eprintln!("{} is of type power, but nothing is poured on it: tracks are laid there", layer);
        }
        if find_planes {
            for layer in board.planes(0.5) {
                eprintln!("{} is taken for a plane: pours fill more than half of it", layer);
            }
        }
        let project = args[2].strip_suffix(".kicad_pcb").unwrap_or(&args[2]).to_string() + ".kicad_pro";
        let rules = kicad::Rules::parse(&read(&project), &board).unwrap_or_else(|e| fail(format!("{}: {}", project, e)));
        let problem = kicad::problem(&board, &rules, &options).unwrap_or_else(|e| fail(format!("{}: {}", args[2], e)));
        if let Some(path) = dump {
            fs::write(&path, serde_json::to_string(&problem).unwrap()).unwrap_or_else(|e| fail(format!("{}: {}", path, e)));
        }
        let pieces: usize = problem.nets.iter().map(|n| n.pieces.len() - 1 + !n.plane.is_empty() as usize).sum();
        eprintln!("{} nets in pieces, {} connections to make", problem.nets.len(), pieces);
        let solution = route(&problem, &mut |line| eprintln!("{}", line));
        let (cost, alone): (f64, f64) = solution.nets.iter().fold((0.0, 0.0), |t, n| (t.0 + n.cost, t.1 + n.alone));
        eprintln!("tracks and vias cost {:.0} mm; each net alone on the board: {:.0} mm", cost, alone);
        if let Some(path) = &report {
            let rows: Vec<String> = solution
                .nets
                .iter()
                .map(|n| format!("{}\t{:.2}\t{:.2}\t{}", n.name, n.cost, n.alone, n.open.len()))
                .collect();
            fs::write(path, rows.join("\n") + "\n").unwrap_or_else(|e| fail(format!("{}: {}", path, e)));
        }
        for net in solution.nets.iter().filter(|n| !n.open.is_empty()) {
            eprintln!("open: {} ({} pieces apart)", net.name, net.open.len());
        }
        let seed = text
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
        let out = kicad::write(&text, &board, &problem, &solution, seed).unwrap_or_else(|e| fail(e));
        fs::write(&args[3], out).unwrap_or_else(|e| fail(format!("{}: {}", args[3], e)));
        process::exit(if solution.nets.iter().all(|n| n.open.is_empty()) { 0 } else { 1 });
    }
    if args.len() == 4 && args[1] == "place" {
        let problem: place::Problem = serde_json::from_str(&read(&args[2])).unwrap_or_else(|e| fail(format!("{}: {}", args[2], e)));
        let result = place::place(&problem, &mut |line| eprintln!("{}", line)).unwrap_or_else(|e| fail(format!("{}: {}", args[2], e)));
        fs::write(&args[3], serde_json::to_string(&result).unwrap()).unwrap_or_else(|e| fail(format!("{}: {}", args[3], e)));
        return;
    }
    if args.len() != 3 {
        fail("usage: leiterbahn-route problem.json result.json\n       leiterbahn-route board in.kicad_pcb out.kicad_pcb [options]\n       leiterbahn-route place problem.json result.json".to_string());
    }
    let problem: Problem = serde_json::from_str(&read(&args[1])).unwrap_or_else(|e| fail(format!("{}: {}", args[1], e)));
    let result = route(&problem, &mut |line| eprintln!("{}", line));
    fs::write(&args[2], serde_json::to_string(&result).unwrap()).unwrap_or_else(|e| fail(format!("{}: {}", args[2], e)));
}
