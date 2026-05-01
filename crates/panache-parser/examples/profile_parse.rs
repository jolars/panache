use panache_parser::parse;
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("usage: profile_parse <doc> [iters]");
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(200);
    let input = fs::read_to_string(path).expect("read input");

    let start = std::time::Instant::now();
    let mut total_nodes: usize = 0;
    for _ in 0..iters {
        let tree = parse(&input, None);
        total_nodes = total_nodes.wrapping_add(std::hint::black_box(tree).descendants().count());
    }
    let elapsed = start.elapsed();
    eprintln!(
        "{} iters of {} bytes: {:?} total ({:?}/iter), nodes-mod {}",
        iters,
        input.len(),
        elapsed,
        elapsed / iters as u32,
        total_nodes,
    );
}
