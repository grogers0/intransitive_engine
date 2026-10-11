use std::cmp;
use std::collections::BinaryHeap;
use std::time::Duration;

use engine_core::{tt, search, Move, Value, Position};

fn main() {
    let soft_deadline: Option<Duration> = None; // Some(Duration::from_secs(3));
    let hard_deadline: Option<Duration> = None; // Duration::from_secs(60);
    let max_depth: Option<u8> = Some(6);
    let top_n = 100;
    let mut tt = tt::TranspositionTable::new(tt::DEFAULT_TT_SIZE_MB);
    for fen_str in std::env::args().skip(1) {
        let mut position = Position::from_fen(&fen_str);
        tt.next_search();
        let move_values = search::evaluate_moves(
            &mut position, max_depth, soft_deadline, hard_deadline, &mut tt);
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
