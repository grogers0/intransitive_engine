use crate::{Outcome, Value};
use crate::position::{Cell, Position};

// As centi-pieces where positive values mean the active player is ahead
pub fn calculate(position: &Position) -> Value {
    // FIXME - make this NNUE or something good. For now it just uses the raw piece count
    match position.get_outcome() {
        Some(Outcome::Draw) => Value::draw(),
        Some(Outcome::Win(player)) => {
            if player == position.active_player { Value::win() } else { Value::loss() }
        },
        None => {
            let mut piece_diff = 0;
            for y in 0..9 {
                for x in 0..9 {
                    match position.board.get(x, y) {
                        Cell::Empty => (),
                        Cell::Occupied(player, _) => {
                            if player == position.active_player {
                                piece_diff += 1;
                            } else {
                                piece_diff -= 1;
                            }
                        },
                    }
                }
            }
            Value(piece_diff * 100)
        },
    }
}
