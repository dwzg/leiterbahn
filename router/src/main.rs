//! leiterbahn-route problem.json result.json
//!
//! Reads a routing problem (see `problem.rs`), routes it and writes the tracks as grid paths.
use std::{env, fs, process};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: leiterbahn-route problem.json result.json");
        process::exit(2);
    }
    let text = fs::read_to_string(&args[1]).unwrap_or_else(|e| {
        eprintln!("{}: {}", args[1], e);
        process::exit(2);
    });
    let problem: leiterbahn_route::problem::Problem = serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("{}: {}", args[1], e);
        process::exit(2);
    });
    let result = leiterbahn_route::route(&problem, &mut |line| eprintln!("{}", line));
    fs::write(&args[2], serde_json::to_string(&result).unwrap()).unwrap_or_else(|e| {
        eprintln!("{}: {}", args[2], e);
        process::exit(2);
    });
}
