use crate::PieceCount;

use base::{Player, Piece, Position, Board, Cell};

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
pub enum PieceShift {
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
    if !is_count_normalized(PieceCount::from_board(&position.board)) { return false; }
    true
}

pub fn is_count_normalized(pc: PieceCount) -> bool {
    if pc.blue_rock < pc.blue_paper || pc.blue_rock < pc.blue_scissors { return false; }
    if pc.blue_rock > pc.blue_paper && pc.blue_rock <= pc.blue_scissors { return false; }
    if pc.blue_rock == pc.blue_paper && pc.blue_rock == pc.blue_scissors {
        if pc.red_rock < pc.red_paper || pc.red_rock < pc.red_scissors { return false; }
        if pc.red_rock > pc.red_paper && pc.red_rock <= pc.red_scissors { return false; }
    }
    true
}

// WARNING - does not update zobrist hashes, do not use it in the transposition table until undo!
pub fn normalize_position(position: &mut Position, mut piece_count: PieceCount) -> NormalizeResult {
    debug_assert_eq!(piece_count, PieceCount::from_board(&position.board));
    let mut res = NormalizeResult {
        piece_count: PieceCount::ZERO,
        invert_player: false,
        piece_shift: PieceShift::Noop,
    };

    if position.active_player == Player::Red {
        res.invert_player = true;
        invert_player(position);
        piece_count = piece_count.players_flipped();
    }
    let piece_shift = calc_piece_shift(piece_count);
    res.piece_shift = piece_shift.invert();
    shift_pieces(&mut position.board, piece_shift);
    res.piece_count = shift_piece_count(piece_count, piece_shift);

    debug_assert!(is_normalized(position));
    debug_assert_eq!(res.piece_count, PieceCount::from_board(&position.board));
    res
}

pub fn normalize_board(board: &mut Board, piece_count: PieceCount) -> PieceShift {
    debug_assert_eq!(piece_count, PieceCount::from_board(board));
    let piece_shift = calc_piece_shift(piece_count);
    let ret = piece_shift.invert();
    shift_pieces(board, piece_shift);
    ret
}

pub fn normalize_count(piece_count: PieceCount) -> PieceCount {
    let piece_shift = calc_piece_shift(piece_count);
    shift_piece_count(piece_count, piece_shift)
}

pub fn undo_normalize_position(position: &mut Position, res: NormalizeResult) {
    if res.invert_player {
        invert_player(position);
    }
    shift_pieces(&mut position.board, res.piece_shift);
}

pub fn undo_normalize_board(board: &mut Board, piece_shift: PieceShift) {
    shift_pieces(board, piece_shift);
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

fn invert_player(position: &mut Position) {
    position.active_player = position.active_player.invert();
    position.board.invert_perspective();
}

fn shift_pieces(board: &mut Board, piece_shift: PieceShift) {
    for y in 0..9 {
        for x in 0..9 {
            if let Cell::Occupied(_player, piece) = board.get_mut(x, y) {
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
        let piece_count = PieceCount::from_board(&pos.board);
        let res = normalize_position(&mut pos, piece_count);
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
        undo_normalize_position(&mut pos, res);
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
        let piece_count = PieceCount::from_board(&pos.board);
        let res = normalize_position(&mut pos, piece_count);
        undo_normalize_position(&mut pos, res);
        assert_eq!(pos, pos2);
    }

    #[test]
    fn test_piece_count_normalizing_example() {
        let pc = PieceCount {
            blue_rock: 2,
            blue_paper: 1,
            blue_scissors: 2,
            red_rock: 1,
            red_paper: 2,
            red_scissors: 3,
        };
        assert!(!is_count_normalized(pc));
        let pcn = normalize_count(pc);
        assert!(is_count_normalized(pcn));
        assert_eq!(pcn, PieceCount {
            blue_rock: 2,
            blue_paper: 2,
            blue_scissors: 1,
            red_rock: 3,
            red_paper: 1,
            red_scissors: 2,

        });
    }

    #[test]
    fn test_piece_count_normalizing_exhaustive() {
        for total_pieces in 0..7 {
            for pc in PieceCount::enumerate_useful_piece_counts(total_pieces) {

                let pc_n = normalize_count(pc);
                assert!(is_count_normalized(pc_n));

                let pc_fn = normalize_count(pc.players_flipped());
                assert!(is_count_normalized(pc_fn));

                assert_eq!(pc_n, normalize_count(pc_fn.players_flipped()));
            }
        }
    }
}
