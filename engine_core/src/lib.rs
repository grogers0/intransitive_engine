pub mod compact_move;
pub mod compact_position;
pub mod heuristic;
pub mod moves;
pub mod position;
pub mod search;
pub mod tt;
pub mod value;
pub mod zobrist;

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum Player {
    Blue, Red,
}

impl Player {
    pub fn invert(&self) -> Player {
        match self {
            Player::Blue => Player::Red,
            Player::Red => Player::Blue,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum Piece {
    Rock, Paper, Scissors,
}

impl Piece {
    pub fn can_capture(self, other: Piece) -> bool {
        match self {
            Piece::Rock => other == Piece::Scissors,
            Piece::Paper => other == Piece::Rock,
            Piece::Scissors => other == Piece::Paper,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum Outcome {
    Draw, Win(Player),
}

pub use value::{Value, MAX_PLY};
pub use position::Position;
pub use moves::Move;
pub use tt::TranspositionTable;
