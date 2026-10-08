use crate::{Outcome, Player};
use crate::position::{Cell, Position};

// As centi-pieces where positive values mean blue is ahead
pub fn calculate(position: &Position) -> i16 {
    // FIXME - make this NNUE or something good. For now it just uses the raw piece count
    match position.get_outcome() {
        Some(Outcome::Draw) => 0,
        Some(Outcome::Win(Player::Blue)) => i16::MAX,
        Some(Outcome::Win(Player::Red)) => -i16::MAX,
        None => {
            let mut ret = 0;
            for y in 0..9 {
                for x in 0..9 {
                    match position.board.get(x, y) {
                        Cell::Empty => (),
                        Cell::Occupied(Player::Blue, _) => ret += 100,
                        Cell::Occupied(Player::Red, _) => ret -= 100,
                    }
                }
            }
            ret
        },
    }
}
