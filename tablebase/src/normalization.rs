use crate::PieceCount;

use base::{Player, Piece, Position, Cell};

// To avoid repetetive positions taking extra space in the tablebase, we normalize according to the
// following rules:
//
// 1) blue is always the player to move
// 2) the types of pieces are shifted so that these relations on the number of pieces are true:
//     a) blue_rock >= blue_paper && blue_rock >= blue_scissors
//     b) if blue_rock > blue_paper then blue_rock > blue_scissors
//     c) if blue_rock == blue_paper && blue_rock == blue_scissors then a & b hold for red as well


// NOTE - forward means rock -> paper, backward means paper -> rock
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum PieceShift {
    Forward, Backward, Noop,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct NormalizeResult {
    pub piece_count: PieceCount,
    invert_player: bool,
    piece_shift: PieceShift,
}

pub fn is_normalized(position: &Position) -> bool {
    if position.active_player == Player::Red { return false; }
    let pc = calc_piece_counts(position);
    if pc.blue_rock < pc.blue_paper || pc.blue_rock < pc.blue_scissors { return false; }
    if pc.blue_rock > pc.blue_paper && pc.blue_rock <= pc.blue_scissors { return false; }
    if pc.blue_rock == pc.blue_paper && pc.blue_rock == pc.blue_scissors {
        if pc.red_rock < pc.red_paper || pc.red_rock < pc.red_scissors { return false; }
        if pc.red_rock > pc.red_paper && pc.red_rock <= pc.red_scissors { return false; }
    }
    true
}

// WARNING - does not update zobrist hashes, do not use it in the transposition table until undo!
pub fn normalize(position: &mut Position) -> NormalizeResult {
    let mut res = NormalizeResult {
        piece_count: PieceCount::ZERO,
        invert_player: false,
        piece_shift: PieceShift::Noop,
    };

    if position.active_player == Player::Red {
        res.invert_player = true;
        invert_player(position);
    }
    let mut piece_count = calc_piece_counts(position);
    let piece_shift = calc_piece_shift(piece_count);
    res.piece_shift = piece_shift.invert();
    shift_pieces(position, piece_shift);
    piece_count = shift_piece_count(piece_count, piece_shift);

    res.piece_count = piece_count;

    debug_assert!(is_normalized(position));
    res
}

pub fn undo_normalize(position: &mut Position, res: NormalizeResult) {
    if res.invert_player {
        invert_player(position);
    }
    shift_pieces(position, res.piece_shift);
}

impl PieceShift {
    fn invert(self) -> Self {
        match self {
            PieceShift::Forward  => PieceShift::Backward,
            PieceShift::Noop     => PieceShift::Noop,
            PieceShift::Backward => PieceShift::Forward,
        }
    }

    fn shift(&self, piece: Piece) -> Piece {
        match (self, piece) {
            (PieceShift::Forward,  Piece::Rock)     => Piece::Paper,
            (PieceShift::Forward,  Piece::Paper)    => Piece::Scissors,
            (PieceShift::Forward,  Piece::Scissors) => Piece::Rock,
            (PieceShift::Noop,     Piece::Rock)     => Piece::Rock,
            (PieceShift::Noop,     Piece::Paper)    => Piece::Paper,
            (PieceShift::Noop,     Piece::Scissors) => Piece::Scissors,
            (PieceShift::Backward, Piece::Rock)     => Piece::Scissors,
            (PieceShift::Backward, Piece::Paper)    => Piece::Rock,
            (PieceShift::Backward, Piece::Scissors) => Piece::Paper,
        }
    }
}

// Returns [total, upper_left, lower_right]
fn calc_piece_counts(position: &Position) -> PieceCount {
    let mut ret = PieceCount::ZERO;
    for y in 0..9 {
        for x in 0..9 {
            if let Cell::Occupied(player, piece) = position.board.get(x, y) {
                *ret.get_mut(player, piece) += 1;
            }
        }
    }
    ret
}

fn calc_piece_shift(pc: PieceCount) -> PieceShift {
    if pc.blue_rock == pc.blue_paper && pc.blue_rock == pc.blue_scissors {
        if pc.red_rock >= pc.red_paper && pc.red_rock >= pc.red_scissors &&
            (pc.red_rock == pc.red_paper || pc.red_rock > pc.red_scissors)
        {
            PieceShift::Noop
        } else if pc.red_paper >= pc.red_scissors && pc.red_paper >= pc.red_rock &&
            (pc.red_paper == pc.red_scissors || pc.red_paper > pc.red_rock)
        {
            PieceShift::Backward
        } else {
            debug_assert!(pc.red_scissors >= pc.red_rock && pc.red_scissors >= pc.red_paper &&
                (pc.red_scissors == pc.red_rock || pc.red_scissors > pc.red_paper));
            PieceShift::Forward
        }
    } else {
        if pc.blue_rock >= pc.blue_paper && pc.blue_rock >= pc.blue_scissors &&
            (pc.blue_rock == pc.blue_paper || pc.blue_rock > pc.blue_scissors)
        {
            PieceShift::Noop
        } else if pc.blue_paper >= pc.blue_scissors && pc.blue_paper >= pc.blue_rock &&
            (pc.blue_paper == pc.blue_scissors || pc.blue_paper > pc.blue_rock)
        {
            PieceShift::Backward
        } else {
            debug_assert!(pc.blue_scissors >= pc.blue_rock && pc.blue_scissors >= pc.blue_paper &&
                (pc.blue_scissors == pc.blue_rock || pc.blue_scissors > pc.blue_paper));
            PieceShift::Forward
        }
    }
}

fn swap_cells(position: &mut Position, x1: u8, y1: u8, x2: u8, y2: u8) {
    let tmp = position.board.get(x1, y1);
    *position.board.get_mut(x1, y1) = position.board.get(x2, y2);
    *position.board.get_mut(x2, y2) = tmp;
}

fn swap_player(position: &mut Position, x: u8, y: u8) {
    if let Cell::Occupied(player, _piece) = position.board.get_mut(x, y) {
        *player = player.invert();
    }
}

fn invert_player(position: &mut Position) {
    position.active_player = position.active_player.invert();
    for y in 0..9 {
        for x in 0..(8 - y) {
            let x2 = 8 - y;
            let y2 = 8 - x;
            swap_player(position, x, y);
            swap_player(position, x2, y2);
            swap_cells(position, x, y, x2, y2);
        }
    }
    for i in 0..9 {
        swap_player(position, i, 8 - i);
    }
}

fn shift_pieces(position: &mut Position, piece_shift: PieceShift) {
    for y in 0..9 {
        for x in 0..9 {
            if let Cell::Occupied(_player, piece) = position.board.get_mut(x, y) {
                *piece = piece_shift.shift(*piece);
            }
        }
    }
}

fn shift_piece_count(piece_count: PieceCount, piece_shift: PieceShift) -> PieceCount {
    let mut ret = PieceCount::ZERO;
    for player in [Player::Blue, Player::Red] {
        for piece in [Piece::Rock, Piece::Paper, Piece::Scissors] {
            *ret.get_mut(player, piece_shift.shift(piece)) = piece_count.get(player, piece);
        }
    }
    ret
}

#[cfg(test)]
mod tests {
    use super::*;
    use base::Outcome;
    use rand::RngExt;

    #[test]
    fn test_initial() {
        let mut pos = Position::initial();
        assert!(!is_normalized(&pos)); // More paper than rock
        let res = normalize(&mut pos);
        assert_eq!(res, NormalizeResult {
            piece_count: PieceCount {
                blue_rock: 4, blue_paper: 3, blue_scissors: 3,
                red_rock: 4, red_paper: 3, red_scissors: 3,
            },
            invert_player: false,
            piece_shift: PieceShift::Forward,
            // TODO - use reflection symmetry?
            // reflect: false,
        });
        assert_ne!(pos, Position::initial());
        undo_normalize(&mut pos, res);
        assert_eq!(pos, Position::initial());
    }

    #[test]
    fn test_random() {
        for _ in 0..1000 {
            test_random_once();
        }
    }

    fn test_random_once() {
        let mut rng = rand::rng();
        let mut pos = Position::randomized_initial(&mut rng);
        let num_moves = rng.random_range(0..50);
        for _ in 0..num_moves {
            if let Some(Outcome::Win(_)) = pos.get_outcome() {
                break;
            }
            let moves = pos.legal_moves().collect::<Vec<_>>();
            pos.make_move(moves[rng.random_range(0..moves.len())]);
        }

        let pos2 = pos.clone();
        let res = normalize(&mut pos);
        undo_normalize(&mut pos, res);
        assert_eq!(pos, pos2);
    }
}
