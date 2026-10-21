pub mod compact_move;
pub mod compact_position;
pub mod heuristic;
pub mod history;
pub mod killers;
pub mod moves;
pub mod position;
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

    const ALL: [Player; 2] = [Player::Blue, Player::Red];
    pub fn iter() -> impl Iterator<Item = Player> {
        Self::ALL.iter().cloned()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum Piece {
    Rock, Paper, Scissors,
}

impl Piece {
    pub fn can_capture(self, other: Piece) -> bool {
        match self {
            Piece::Rock     => other == Piece::Scissors,
            Piece::Paper    => other == Piece::Rock,
            Piece::Scissors => other == Piece::Paper,
        }
    }

    pub fn captured_by(self) -> Piece {
        match self {
            Piece::Rock     => Piece::Paper,
            Piece::Paper    => Piece::Scissors,
            Piece::Scissors => Piece::Rock,
        }
    }

    const ALL: [Piece; 3] = [Piece::Rock, Piece::Paper, Piece::Scissors];
    pub fn iter() -> impl Iterator<Item = Piece> {
        Self::ALL.iter().cloned()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum Outcome {
    Draw, Win(Player),
}

pub use value::{Value, MAX_PLY};
pub use position::{Position, Cell, Board, AdjacentLocs};
pub use moves::Move;
pub use tt::TranspositionTable;
