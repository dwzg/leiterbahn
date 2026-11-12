//! leiterbahn-route problem.json result.json
//!     Routes a problem (see `problem.rs`) and writes the tracks as grid paths.
//!
//! leiterbahn-route board in.kicad_pcb out.kicad_pcb [--step 0.05] [--via-cost 3] [--max-iter 200]
//!                        [--hole-clear mm] [--problem file.json]
//!     Routes every net of a KiCad board that is in more than one piece and writes the board with
//!     the new tracks. The rules come from the .kicad_pro beside the board. --problem also writes
//!     the problem as it was read from the board.
use leiterbahn_route::{kicad, problem::Problem, route};
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
        let mut options = kicad::Options {
            step: 0.05,
            via_cost: 3.0,
            max_iter: 200,
            hole_clear: None,
        };
        let mut dump = None;
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
                _ => fail(format!("unknown option {}", flag)),
            }
        }
        let text = read(&args[2]);
        let board = kicad::Board::parse(&text).unwrap_or_else(|e| fail(format!("{}: {}", args[2], e)));
        let project = args[2].strip_suffix(".kicad_pcb").unwrap_or(&args[2]).to_string() + ".kicad_pro";
        let rules = kicad::Rules::parse(&read(&project), &board).unwrap_or_else(|e| fail(format!("{}: {}", project, e)));
        let problem = kicad::problem(&board, &rules, &options).unwrap_or_else(|e| fail(format!("{}: {}", args[2], e)));
        if let Some(path) = dump {
            fs::write(&path, serde_json::to_string(&problem).unwrap()).unwrap_or_else(|e| fail(format!("{}: {}", path, e)));
        }
        let pieces: usize = problem.nets.iter().map(|n| n.pieces.len() - 1).sum();
        eprintln!("{} nets in pieces, {} connections to make", problem.nets.len(), pieces);
        let solution = route(&problem, &mut |line| eprintln!("{}", line));
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
    if args.len() != 3 {
        fail("usage: leiterbahn-route problem.json result.json\n       leiterbahn-route board in.kicad_pcb out.kicad_pcb [options]".to_string());
    }
    let problem: Problem = serde_json::from_str(&read(&args[1])).unwrap_or_else(|e| fail(format!("{}: {}", args[1], e)));
    let result = route(&problem, &mut |line| eprintln!("{}", line));
    fs::write(&args[2], serde_json::to_string(&result).unwrap()).unwrap_or_else(|e| fail(format!("{}: {}", args[2], e)));
}
