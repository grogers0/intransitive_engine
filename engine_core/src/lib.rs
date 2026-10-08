pub mod compact_move;
pub mod compact_position;
pub mod heuristic;
pub mod moves;
pub mod position;
pub mod search;
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

    pub fn sign(&self) -> i16 {
        match self {
            Player::Blue => 1,
            Player::Red => -1,
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

pub use position::Position;
pub use moves::Move;
