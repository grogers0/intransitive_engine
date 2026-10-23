pub mod combinatorics;
pub mod normalization;
pub mod piece_count;
pub mod position_index;

pub use piece_count::PieceCount;
pub use normalization::{normalize_position, normalize_board, undo_normalize_position, undo_normalize_board, is_normalized};

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
