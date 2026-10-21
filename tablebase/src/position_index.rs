use std::collections::BTreeSet;

use crate::{combinatorics, PieceCount};

use base::{Cell, Board, Player, Piece};

pub fn board_to_index(board: &Board, mut piece_count: PieceCount) -> u64 {
    debug_assert_eq!(piece_count, PieceCount::from_board(board));
    let mut index = 0;

    for piece in Piece::iter() {
        if board.get(0, 0) == Cell::Occupied(Player::Blue, piece) {
            *piece_count.get_mut(Player::Blue, piece) -= 1;
            break;
        }
        index += combos_with_blue_in_corner(piece_count, piece);
    }

    let mut to_fill = Vec::new();
    let mut piece_indexes = Vec::new();
    let mut filled = BTreeSet::new();
    let mut slots = 79;
    let mut mult = 1;

    for player in Player::iter() {
        for piece in Piece::iter() {
            for y in 0..9 {
                let min_x = if y == 0 { 1 } else { 0 };
                for x in min_x..9 {
                    if board.get(x, y) == Cell::Occupied(player, piece) {
                        let mut i = y * 9 + x - 1;
                        to_fill.push(i);
                        i -= filled.range(0..i).count() as u8;
                        piece_indexes.push(i);
                    }
                }
            }

            let n = if player == Player::Red { slots + 1 } else { slots };
            let k = piece_indexes.len() as u8;
            let c = combinatorics::choose(n, k);
            let r = combinatorics::rank(&piece_indexes);
            debug_assert!(r < c);
            index += mult * r;
            mult *= c;
            slots -= k;
            for &i in &to_fill {
                filled.insert(i);
            }
            piece_indexes.clear();
            to_fill.clear();
        }
    }

    index
}

pub fn index_to_board(mut index: u64, mut piece_count: PieceCount) -> Board {
    let mut board = Board::new();

    for piece in Piece::iter() {
        let combos = combos_with_blue_in_corner(piece_count, piece);
        if index < combos {
            *piece_count.get_mut(Player::Blue, piece) -= 1;
            *board.get_mut(0, 0) = Cell::Occupied(Player::Blue, piece);
            break;
        }
        index -= combos;
    }

    let mut slots = 79;
    let mut filled = BTreeSet::new();
    for player in Player::iter() {
        for piece in Piece::iter() {
            let k = piece_count.get(player, piece);
            let n = if player == Player::Red { slots + 1 } else { slots };
            let c = combinatorics::choose(n, k);
            let r = index % c;
            index /= c;
            slots -= k;

            let piece_indexes = combinatorics::unrank(r, k);

            for mut i in piece_indexes.into_iter().rev() {
                i += 1; // Skip bottom left corner
                for &f in &filled {
                    if f <= i {
                        i += 1;
                    } else {
                        break;
                    }
                }

                filled.insert(i);
                let x = i % 9;
                let y = i / 9;
                debug_assert_eq!(board.get(x, y), Cell::Empty);
                *board.get_mut(x, y) = Cell::Occupied(player, piece);
            }
        }
    }

    board
}

fn combos_with_blue_in_corner(mut piece_count: PieceCount, corner_piece: Piece) -> u64 {
    if piece_count.get(Player::Blue, corner_piece) == 0 {
        return 0;
    }
    *piece_count.get_mut(Player::Blue, corner_piece) -= 1;
    let piece_count = piece_count;
    // Remainder of blue pieces are not eligible for either corner
    let mut slots = 79;
    let mut ret = 1;
    for player in Player::iter() {
        for piece in Piece::iter() {
            let this_slots = if player == Player::Red { slots + 1 } else { slots };
            let this_piece_count = piece_count.get(player, piece);
            ret *= combinatorics::choose(this_slots, this_piece_count);
            slots -= this_piece_count;
        }
    }
    ret
}

fn combos_without_blue_in_corner(piece_count: PieceCount) -> u64 {
    // Neither corner is eligible for blue
    let mut slots = 79;
    let mut ret = 1;
    for player in Player::iter() {
        for piece in Piece::iter() {
            let this_slots = if player == Player::Red { slots + 1 } else { slots };
            let this_piece_count = piece_count.get(player, piece);
            ret *= combinatorics::choose(this_slots, this_piece_count);
            slots -= this_piece_count;
        }
    }
    ret
}


pub fn index_count(piece_count: PieceCount) -> u64 {
    Piece::iter().map(|piece| combos_with_blue_in_corner(piece_count, piece)).sum::<u64>() +
        combos_without_blue_in_corner(piece_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, RngExt};
    use base::Position;

    #[test]
    fn test_random() {
        for _ in 0..10_000 {
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
        let total_pieces = rng.random_range(2..6);
        let blue_pieces = rng.random_range(1..total_pieces);
        let red_pieces = total_pieces - blue_pieces;
        let mut board = Board::new();
        for _ in 0..blue_pieces {
            add_random_piece(&mut board, Player::Blue, &mut rng);
        }
        for _ in 0..red_pieces {
            add_random_piece(&mut board, Player::Red, &mut rng);
        }

        let piece_count = PieceCount::from_board(&board);
        let index = board_to_index(&board, piece_count);
        assert!(index < index_count(piece_count), "{index} should be less than {}", index_count(piece_count));
        let board2 = index_to_board(index, piece_count);
        assert_eq!(board, board2);
    }

    #[test]
    fn test_last_index() {
        for total_pieces in 0..9 {
            for pc in PieceCount::enumerate_useful_piece_counts(total_pieces) {
                let mut board = Board::new();
                for player in [Player::Blue, Player::Red] {
                    for piece in [Piece::Rock, Piece::Paper, Piece::Scissors] {
                        for _ in 0..pc.get(player, piece) {
                            let max_x = if player == Player::Blue { 7 } else { 8 };
                            assert!(total_pieces < 9); // Simplifies below logic
                            let y = 8;
                            for x in (0..=max_x).rev() {
                                if let Cell::Empty = board.get(x, y) {
                                    *board.get_mut(x, y) = Cell::Occupied(player, piece);
                                    break;
                                }
                            }
                        }
                    }
                }
                let index = board_to_index(&board, pc);
                assert_eq!(index, index_count(pc) - 1);
                let board2 = index_to_board(index, pc);
                assert_eq!(board, board2);
            }
        }
    }
}
