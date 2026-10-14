use std::collections::HashMap;

use crate::{Outcome, Value, Player, Piece};
use crate::position::{Cell, Position};

pub fn calculate_outcome(outcome: Outcome, active_player: Player) -> Value {
    match outcome {
        Outcome::Draw => Value::draw(),
        Outcome::Win(winner) => if winner == active_player { Value::win() } else { Value::loss() },
    }
}

// As milli-pieces where positive values mean the active player is ahead
pub fn calculate_diff(position: &Position) -> Value {
    // FIXME - make this NNUE or something good. For now it just uses the raw piece count
    let mut value = total_piece_diff(position);
    value += self_guarding_diff(position);
    value += aggression_diff(position);
    value += influence_diff(position);
    value
}

// Simple number of pieces of each side
fn total_piece_diff(position: &Position) -> Value {
    let mut diff = 0;
    for y in 0..9 {
        for x in 0..9 {
            diff += match position.board.get(x, y) {
                Cell::Empty => 0,
                Cell::Occupied(p, _) => if p == position.active_player { 1 } else { -1 },
            }
        }
    }
    Value(diff * 1000)
}

// Directly guarding your other pieces prevents attacks
fn self_guarding_diff(position: &Position) -> Value {
    let mut diff = 0;
    for y1 in 1..9 {
        for x1 in 0..9 {
            let (player1, piece1) =
                if let Cell::Occupied(player, piece) = position.board.get(x1, y1) {
                    (player, piece)
                } else {
                    continue;
                };
            for i in 0..4 {
                // Since guarding is symmetrical, we only check 4 of the 8 adjacent cells, i.e.
                // (xa, ya, xb, yb) is never checked in the opposite direction (xb, yb, xa, ya)
                if let Some((x2, y2)) = match i {
                    0 if x1 > 0 => Some((x1 - 1, y1 - 1)),
                    1           => Some((x1    , y1 - 1)),
                    2 if x1 < 8 => Some((x1 + 1, y1 - 1)),
                    3 if x1 > 0 => Some((x1 - 1, y1    )),
                    _ => None
                } {
                    if let Cell::Occupied(player2, piece2) = position.board.get(x2, y2) {
                        if player1 == player2 && piece1 != piece2 {
                            diff += if player1 == position.active_player { 1 } else { -1 };
                        }
                    }
                }
            }
        }
    }
    Value(diff) // TODO - what weight to give this?
}

// Try to advance toward the goal
fn aggression_diff(position: &Position) -> Value {
    let dist_to_goal = |x: u8, y: u8, player: Player| -> i16 {
        let (goal_x, goal_y) = match player {
            Player::Blue => (8, 8),
            Player::Red => (0, 0),
        };
        dist(x, y, goal_x, goal_y) as i16
    };
    let mut total_dist_active = 0;
    let mut pieces_active = 0;
    let mut total_dist_other = 0;
    let mut pieces_other = 0;

    for y in 0..9 {
        for x in 0..9 {
            if let Cell::Occupied(player, _) = position.board.get(x, y) {
                if player == position.active_player {
                    total_dist_active += dist_to_goal(x, y, player);
                    pieces_active += 1;
                } else {
                    total_dist_other += dist_to_goal(x, y, player);
                    pieces_other += 1;
                }
            }
        }
    }

    if pieces_active == 0 { pieces_active = 1; }
    if pieces_other == 0 { pieces_other = 1; }

    let avg_dist_active = total_dist_active / pieces_active;
    let avg_dist_other = total_dist_other / pieces_other;

    let diff = avg_dist_other - avg_dist_active;
    Value(rounding_div(diff, 3)) // TODO - what weight to give this?
}

// each piece type should try to influence more of the board so it doesn't just run to a corner and
// also doesn't stay too close to other pieces of its type
fn influence_diff(position: &Position) -> Value {
    let mut influence = HashMap::new();
    for is_active in [false, true] {
        influence.insert(is_active, HashMap::new());
        for piece in [Piece::Rock, Piece::Paper, Piece::Scissors] {
            influence.get_mut(&is_active).unwrap().insert(piece, vec![vec![9; 9]; 9]);
        }
    }

    for y in 0..9 {
        for x in 0..9 {
            if let Cell::Occupied(player, piece) = position.board.get(x, y) {
                let is_active = player == position.active_player;
                for y2 in 0..9 {
                    for x2 in 0..9 {
                        let d = dist(x, y, x2, y2) as i16;
                        if d < influence[&is_active][&piece][y2 as usize][x2 as usize] {
                            influence.get_mut(&is_active).unwrap().get_mut(&piece).unwrap()
                                [y2 as usize][x2 as usize] = d;
                        }
                    }
                }
            }
        }
    }

    let mut diff = 0;
    for is_active in [false, true] {
        let sign = if is_active { -1 } else { 1 };
        for piece in [Piece::Rock, Piece::Paper, Piece::Scissors] {
            for y in 0..9 {
                for x in 0..9 {
                    diff += sign * influence[&is_active][&piece][y][x];
                }
            }
        }
    }
    Value(rounding_div(diff, 40)) // TODO - what weight?


}

fn dist(x1: u8, y1: u8, x2: u8, y2: u8) -> u8 {
    let dx = if x1 > x2 { x1 - x2 } else { x2 - x1 };
    let dy = if y1 > y2 { y1 - y2 } else { y2 - y1 };
    if dx > dy { dx } else { dy }
}

fn rounding_div(q: i16, d: i16) -> i16 {
    (q + (q.signum() * d / 2)) / d
}
