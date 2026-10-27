use std::cmp;
use std::collections::{BinaryHeap, HashSet};
use std::time::Duration;

use base::{tt, history, Move, Value, Position};
use tablebase::Tablebase;

fn main() {
    let opts = parse_command_line_args();
    let search_opts = search::Opts {
        max_depth: Some(6),
        max_nodes: None,
        search_moves: HashSet::new(),
        soft_deadline: Some(Duration::from_secs(3)),
        hard_deadline: Some(Duration::from_secs(60)),
    };
    let top_n = 40;
    let mut tt = tt::TranspositionTable::new(opts.tt_size_mb);
    let mut history = history::HistoryTable::new();
    let tablebase = Tablebase::new(&opts.tablebase_dir);
    for fen_str in std::env::args().skip(1) {
        let mut position = Position::from_fen(&fen_str);
        tt.next_search();
        let move_values = search::evaluate_moves(
            &mut position, &search_opts, &mut tt, &mut history, &tablebase);
        let mut heap = BinaryHeap::new();
        #[derive(Eq, PartialEq)]
        struct HeapElem(Move, Value);
        impl PartialOrd for HeapElem {
            fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
                Some(self.1.cmp(&other.1).then(format!("{}", other.0).cmp(&format!("{}", self.0))))
            }
        }
        impl Ord for HeapElem {
            fn cmp(&self, other: &Self) -> cmp::Ordering {
                self.1.cmp(&other.1).then(format!("{}", other.0).cmp(&format!("{}", self.0)))
            }
        }
        for (m, value) in move_values {
            heap.push(HeapElem(m, value));
        }
        for _ in 0..top_n {
            if let Some(HeapElem(m, value)) = heap.pop() {
                println!("{} = {}", m, value);
            }
        }
        println!();
    }
}

struct Opts {
    tablebase_dir: String,
    tt_size_mb: usize,
}

fn parse_command_line_args() -> Opts {
    let mut opts = Opts {
        tablebase_dir: "data/tablebase".to_string(),
        tt_size_mb: tt::DEFAULT_TT_SIZE_MB,
    };

    let mut args = std::env::args();
    args.next(); // Skip binary name
    while let Some(token) = args.next() {
        match token.as_str() {
            "--hash_mb" | "--hash-mb" | "--tt_size_mb" | "--tt-size-mb" => {
                opts.tt_size_mb = args.next().unwrap().parse().unwrap();
            },
            "--tablebase" | "--tablebase_dir" | "--tablebase-dir" => {
                opts.tablebase_dir = args.next().unwrap().to_string();
            },
            _ => {
                panic!("unknown command line argument \"{}\"", token);
            },
        };
    }

    opts
}
