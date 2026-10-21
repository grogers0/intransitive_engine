use std::{cmp, ops};

use base::{Board, Cell, Player, Piece};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct PieceCount {
    pub blue_rock: u8,
    pub blue_paper: u8,
    pub blue_scissors: u8,
    pub red_rock: u8,
    pub red_paper: u8,
    pub red_scissors: u8,
}

impl PieceCount {
    pub const ZERO: Self = Self {
        blue_rock: 0,
        blue_paper: 0,
        blue_scissors: 0,
        red_rock: 0,
        red_paper: 0,
        red_scissors: 0,
    };

    pub fn single(player: Player, piece: Piece) -> Self {
        let mut pc = Self::ZERO;
        *pc.get_mut(player, piece) += 1;
        pc
    }

    pub fn get(&self, player: Player, piece: Piece) -> u8 {
        match (player, piece) {
            (Player::Blue, Piece::Rock)     => self.blue_rock,
            (Player::Blue, Piece::Paper)    => self.blue_paper,
            (Player::Blue, Piece::Scissors) => self.blue_scissors,
            (Player::Red,  Piece::Rock)     => self.red_rock,
            (Player::Red,  Piece::Paper)    => self.red_paper,
            (Player::Red,  Piece::Scissors) => self.red_scissors,
        }
    }

    pub fn get_mut(&mut self, player: Player, piece: Piece) -> &mut u8 {
        match (player, piece) {
            (Player::Blue, Piece::Rock)     => &mut self.blue_rock,
            (Player::Blue, Piece::Paper)    => &mut self.blue_paper,
            (Player::Blue, Piece::Scissors) => &mut self.blue_scissors,
            (Player::Red,  Piece::Rock)     => &mut self.red_rock,
            (Player::Red,  Piece::Paper)    => &mut self.red_paper,
            (Player::Red,  Piece::Scissors) => &mut self.red_scissors,
        }
    }

    pub fn total(&self) -> u8 {
        self.blue_rock + self.blue_paper + self.blue_scissors +
            self.red_rock + self.red_paper + self.red_scissors
    }

    pub fn total_for_player(&self, player: Player) -> u8 {
        Piece::iter().map(|piece| self.get(player, piece)).sum()
    }

    pub fn from_board(board: &Board) -> Self {
        let mut ret = PieceCount::ZERO;
        for y in 0..9 {
            for x in 0..9 {
                if let Cell::Occupied(player, piece) = board.get(x, y) {
                    *ret.get_mut(player, piece) += 1;
                }
            }
        }
        ret
    }

    pub fn players_flipped(&self) -> Self {
        Self {
            blue_rock: self.red_rock,
            blue_paper: self.red_paper,
            blue_scissors: self.red_scissors,
            red_rock: self.blue_rock,
            red_paper: self.blue_paper,
            red_scissors: self.blue_scissors,
        }
    }

    pub fn enumerate_useful_piece_counts(total: u8) -> Vec<Self> {
        let mut ret = Vec::new();
        for blue_total in 1..total {
            let red_total = total - blue_total;
            for blue_rock in 0..=blue_total {
                for blue_paper in 0..=(blue_total - blue_rock) {
                    let blue_scissors = blue_total - blue_rock - blue_paper;
                    for red_rock in 0..=red_total {
                        for red_paper in 0..=(red_total - red_rock) {
                            let red_scissors = red_total - red_rock - red_paper;
                            ret.push(PieceCount {
                                blue_rock, blue_paper, blue_scissors,
                                red_rock, red_paper, red_scissors,
                            });
                        }
                    }
                }
            }
        }
        ret
    }
}

impl ops::Add for PieceCount {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            blue_rock: self.blue_rock + other.blue_rock,
            blue_paper: self.blue_paper + other.blue_paper,
            blue_scissors: self.blue_scissors + other.blue_scissors,
            red_rock: self.red_rock + other.red_rock,
            red_paper: self.red_paper + other.red_paper,
            red_scissors: self.red_scissors + other.red_scissors,
        }
    }
}

impl ops::Sub for PieceCount {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        debug_assert!(self >= other);
        Self {
            blue_rock: self.blue_rock - other.blue_rock,
            blue_paper: self.blue_paper - other.blue_paper,
            blue_scissors: self.blue_scissors - other.blue_scissors,
            red_rock: self.red_rock - other.red_rock,
            red_paper: self.red_paper - other.red_paper,
            red_scissors: self.red_scissors - other.red_scissors,
        }
    }
}

impl cmp::PartialOrd for PieceCount {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        let mut less_cnt = 0u8;
        let mut greater_cnt = 0u8;
        let mut check = |s: u8, o: u8| {
            if s < o {
                less_cnt += 1;
            } else if s > 0 {
                greater_cnt += 1;
            }
        };
        check(self.blue_rock, other.blue_rock);
        check(self.blue_paper, other.blue_paper);
        check(self.blue_scissors, other.blue_scissors);
        check(self.red_rock, other.red_rock);
        check(self.red_paper, other.red_paper);
        check(self.red_scissors, other.red_scissors);
        if less_cnt > 0 {
           if greater_cnt == 0 {
               Some(cmp::Ordering::Less)
           } else {
               None // Not strictly less
           }
        } else if greater_cnt > 0 {
            Some(cmp::Ordering::Greater)
        } else {
            Some(cmp::Ordering::Equal)
        }
    }
}
