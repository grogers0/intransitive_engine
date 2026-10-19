use std::collections::BTreeSet;

use crate::{combinatorics, PieceCount};

use base::{Cell, Board, Player, Piece, Position};

pub fn board_to_index(board: &Board) -> (u64, PieceCount) {
    let mut filled = BTreeSet::new();
    let mut to_fill = Vec::new();
    let mut piece_indexes = Vec::new();
    let mut index = 0;
    let mut mult = 1;
    let mut piece_count = PieceCount::ZERO;

    for player in [Player::Blue, Player::Red] {
        for piece in [Piece::Rock, Piece::Paper, Piece::Scissors] {
            for y in 0..9 {
                for x in 0..9 {
                    if let Cell::Occupied(player2, piece2) = board.get(x, y) &&
                        player == player2 && piece == piece2
                    {
                        assert!((x, y) != Position::corner_goal(player));

                        let mut i = y * 9 + x;
                        to_fill.push(i);

                        i -= filled.range(0..i).count() as u8;
                        if player == Player::Red && !filled.contains(&0) { i -= 1; }
                        piece_indexes.push(i);
                    }
                }
            }

            let mut n = 80 - filled.len() as u8;
            if player == Player::Red && filled.contains(&0) {
                n += 1;
            }
            let k = piece_indexes.len() as u8;
            let c = combinatorics::choose(n, k);
            let r = combinatorics::rank(&piece_indexes);
            debug_assert!(r < c);
            index += mult * r;
            mult *= c;
            for &i in &to_fill {
                filled.insert(i);
            }
            *piece_count.get_mut(player, piece) = k;
            piece_indexes.clear();
            to_fill.clear();
        }
    }

    (index, piece_count)
}

pub fn index_to_board(mut index: u64, piece_count: PieceCount) -> Board {
    let mut board = Board::new();
    let mut filled = BTreeSet::new();
    let mut to_fill = Vec::new();

    for player in [Player::Blue, Player::Red] {
        for piece in [Piece::Rock, Piece::Paper, Piece::Scissors] {
            let k = piece_count.get(player, piece);
            let mut n = 80 - filled.len() as u8;
            if player == Player::Red && filled.contains(&0) {
                n += 1;
            }
            let c = combinatorics::choose(n, k);
            let r = index % c;
            index /= c;

            let piece_indexes = combinatorics::unrank(r, k);

            for &i in &piece_indexes {
                let mut i = i;
                if player == Player::Red && !filled.contains(&0) { i += 1; }
                for &f in &filled {
                    if f <= i {
                        i += 1;
                    } else {
                        break;
                    }
                }

                to_fill.push(i);
                let x = i % 9;
                let y = i / 9;
                debug_assert_eq!(board.get(x, y), Cell::Empty);
                *board.get_mut(x, y) = Cell::Occupied(player, piece);
            }

            for &i in &to_fill {
                filled.insert(i);
            }
            to_fill.clear();
        }
    }

    board
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalization::normalize;
    use base::zobrist::ZobristAccumulator;
    use rand::{Rng, RngExt};

    #[test]
    fn test_random() {
        for _ in 0..1000 {
            test_random_once();
        }
    }

    fn add_random_piece<R: Rng + ?Sized>(board: &mut Board, player: Player, rng: &mut R) {
        let pieces = [Piece::Rock, Piece::Paper, Piece::Scissors];
        let piece = pieces[rng.random_range(0..pieces.len())];
        loop {
            let x = rng.random_range(0..9);
            let y = rng.random_range(0..9);
            if (x, y) == Position::corner_goal(player) {
                continue;
            } else if let Cell::Occupied(_, _) = board.get(x, y) {
                continue;
            } else {
                *board.get_mut(x, y) = Cell::Occupied(player, piece);
                return;
            }
        }
    }

    fn test_random_once() {
        let mut rng = rand::rng();
        let total_pieces = rng.random_range(2..5);
        let blue_pieces = rng.random_range(1..total_pieces);
        let red_pieces = total_pieces - blue_pieces;
        let mut board = Board::new();
        for _ in 0..blue_pieces {
            add_random_piece(&mut board, Player::Blue, &mut rng);
        }
        for _ in 0..red_pieces {
            add_random_piece(&mut board, Player::Red, &mut rng);
        }

        let (index, piece_count) = board_to_index(&board);
        let board2 = index_to_board(index, piece_count);
        assert_eq!(board, board2);
    }
}
