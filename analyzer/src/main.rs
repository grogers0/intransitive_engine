use std::cmp;
use std::collections::{BinaryHeap, HashSet};
use std::time::Duration;

use engine_core::{tt, search, history, Move, Value, Position};

fn main() {
    let opts = search::Opts {
        max_depth: Some(6),
        max_nodes: None,
        search_moves: HashSet::new(),
        soft_deadline: None, // Some(Duration::from_secs(3)),
        hard_deadline: None, // Some(Duration::from_secs(60)),
    };
    let top_n = 40;
    let mut tt = tt::TranspositionTable::new(tt::DEFAULT_TT_SIZE_MB);
    let mut history = history::HistoryTable::new();
    for fen_str in std::env::args().skip(1) {
        let mut position = Position::from_fen(&fen_str);
        tt.next_search();
        let move_values = search::evaluate_moves(&mut position, &opts, &mut tt, &mut history);
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
