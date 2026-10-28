pub mod combinatorics;
pub mod normalization;
pub mod piece_count;
pub mod position_index;
pub mod tablebase;
pub mod validation;
pub mod atomic_bitset;

pub use piece_count::PieceCount;
pub use normalization::{normalize_position, normalize_board, undo_normalize_position, undo_normalize_board, is_normalized};
pub use tablebase::Tablebase;
pub use atomic_bitset::AtomicBitset;

use std::{ops, fmt};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub enum WDL {
    Loss, Draw, Win
}

impl ops::Neg for WDL {
    type Output = Self;
    fn neg(self) -> Self {
        match self {
            WDL::Win  => WDL::Loss,
            WDL::Draw => WDL::Draw,
            WDL::Loss => WDL::Win,
        }
    }
}

impl fmt::Display for WDL {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            WDL::Win  => write!(f, "W"),
            WDL::Draw => write!(f, "D"),
            WDL::Loss => write!(f, "L"),
        }
    }
}

pub fn tablebase_name(pc: PieceCount) -> String {
    let mut s = String::with_capacity(pc.total() as usize);
    for _ in 0..pc.blue_rock     { s.push('R'); }
    for _ in 0..pc.blue_paper    { s.push('P'); }
    for _ in 0..pc.blue_scissors { s.push('S'); }
    for _ in 0..pc.red_rock      { s.push('r'); }
    for _ in 0..pc.red_paper     { s.push('p'); }
    for _ in 0..pc.red_scissors  { s.push('s'); }
    s
}
