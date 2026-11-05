use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, AtomicU16, Ordering};
use std::cmp;
use std::fs;
use std::io::{self, Write};

use rayon::prelude::*;
use tablebase::{PieceCount, WDL, position_index, normalization, tablebase_name, TablebaseMap, validation, AtomicBitset};
use base::{AdjacentLocs, Board, Cell, Outcome, Piece, Player, Position};

struct Opts {
    upto_pieces: u8,
    enable_dtm: bool,
    output_dir: String,
    output_format: OutputFormat,
    num_threads: usize,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum OutputFormat {
    Blackhole, Binary, Text, ZlibCompressed,
}


// NOTE - all vectors will have the same length
struct MemTb {
    wdl: WdlVec,
    dtz: Box<[AtomicU8]>,
    dtm: Box<[AtomicU16]>,
}

impl MemTb {
    fn new(len: usize, enable_dtm: bool) -> MemTb {
        let wdl = WdlVec::new(len);
        let dtz = (0..len).map(|_| AtomicU8::new(0)).collect::<Vec<_>>().into_boxed_slice();
        let dtm = if enable_dtm {
            (0..len).map(|_| AtomicU16::new(0)).collect::<Vec<_>>().into_boxed_slice()
        } else {
            Box::new([])
        };
        MemTb { wdl, dtz, dtm }
    }
}

impl Clone for MemTb {
    fn clone(&self) -> Self {
        let wdl = self.wdl.clone();
        let dtz = self.dtz.iter()
            .map(|atomic_ref| AtomicU8::new(atomic_ref.load(Ordering::Relaxed)))
            .collect::<Vec<_>>().into_boxed_slice();
        let dtm = self.dtm.iter()
            .map(|atomic_ref| AtomicU16::new(atomic_ref.load(Ordering::Relaxed)))
            .collect::<Vec<_>>().into_boxed_slice();
        Self { wdl, dtz, dtm }
    }
}

struct WdlVec {
    len: usize,
    bytes: Box<[AtomicU8]>,
}

impl WdlVec {
    fn new(len: usize) -> WdlVec {
        let bytes = (0..((len + 3) / 4)).map(|_| AtomicU8::new(0xff))
            .collect::<Vec<_>>().into_boxed_slice();
        WdlVec { len, bytes }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn get_option(&self, idx: usize) -> Option<WDL> {
        debug_assert!(idx < self.len);
        let byte_idx = idx >> 2;
        let bit_idx = (idx & 0x03) << 1;
        let byte_val = self.bytes[byte_idx].load(Ordering::Relaxed);
        match (byte_val >> bit_idx) & 0x03 {
            0 => Some(WDL::Loss),
            1 => Some(WDL::Draw),
            2 => Some(WDL::Win),
            3 => None,
            _ => unreachable!(),
        }
    }

    fn get(&self, idx: usize) -> WDL {
        self.get_option(idx).unwrap()
    }

    fn set(&self, idx: usize, wdl: WDL) {
        debug_assert!(idx < self.len);
        let byte_idx = idx >> 2;
        let bit_idx = (idx & 0x03) << 1;
        let mut prev_byte_val = self.bytes[byte_idx].load(Ordering::Relaxed);
        loop {
            let mut byte_val = prev_byte_val;
            byte_val &= !(0x03 << bit_idx);
            byte_val |= match wdl { WDL::Loss => 0, WDL::Draw => 1, WDL::Win => 2 } << bit_idx;
            debug_assert!(byte_val == prev_byte_val ||
                ((prev_byte_val >> bit_idx) & 0x03) == 0x03);
            // Shouldn't overwrite unless it's unset (setting to same value is ok)
            let cas_result = self.bytes[byte_idx].compare_exchange_weak(prev_byte_val, byte_val,
                Ordering::Relaxed, Ordering::Relaxed);
            match cas_result {
                Ok(_) => break,
                Err(updated_byte_val) => prev_byte_val = updated_byte_val,
            }
        }
    }
}

impl Clone for WdlVec {
    fn clone(&self) -> Self {
        let bytes = self.bytes.iter()
            .map(|byte_ref| AtomicU8::new(byte_ref.load(Ordering::Relaxed)))
            .collect::<Vec<_>>().into_boxed_slice();
        Self { len: self.len, bytes }
    }
}

fn main() {
    let opts = parse_command_line_args();
    init_threads(&opts);

    let mut disk_tablebase = TablebaseMap::new(&opts.output_dir);
    for total_count in 0..=opts.upto_pieces {
        rayon::scope(|scope| {
            let mut already_generated = HashSet::new();
            for piece_count in PieceCount::enumerate_useful_piece_counts(total_count) {
                let piece_count = normalization::normalize_count(piece_count);
                if already_generated.contains(&piece_count) ||
                    tablebase_already_generated(piece_count, &disk_tablebase, opts.enable_dtm) {
                        continue;
                }
                let (tb1, tb2) = gen_tablebase(piece_count, &disk_tablebase, opts.enable_dtm);
                let inverse_piece_count =
                    normalization::normalize_count(piece_count.players_flipped());

                already_generated.insert(piece_count);
                already_generated.insert(inverse_piece_count);

                let opts = &opts;
                scope.spawn(move |_| {
                    output(piece_count, &tb1, opts);
                });
                scope.spawn(move |_| {
                    output(inverse_piece_count, &tb2, opts);
                });
            }
        });
        disk_tablebase.refresh(&opts.output_dir).unwrap();
    }

    if cfg!(debug_assertions) {
        validation::validate_tablebase(&disk_tablebase, !opts.enable_dtm);
    }
}

fn tablebase_already_generated(piece_count: PieceCount, disk_tablebase: &TablebaseMap, enable_dtm: bool) -> bool {
    disk_tablebase.lookup_wdl_raw(0, piece_count).is_some() &&
        disk_tablebase.lookup_dtz_raw(0, piece_count).is_some() &&
        (!enable_dtm || disk_tablebase.lookup_dtm_raw(0, piece_count).is_some())
}

// TODO - should this be parallelized? Would need to copy the board state
fn iter_all_boards<F: FnMut(&mut Board) -> ()>(mut piece_count: PieceCount, board: &mut Board, mut f: F) {
    fn recurse<F: FnMut(&mut Board) -> ()>(piece_count: &mut PieceCount, board: &mut Board, f: &mut F,
        cont_player_id: usize, cont_piece_id: usize, cont_x: u8, cont_y: u8) {
        let mut check = |piece_count: &mut PieceCount, player_id: usize, piece_id: usize, x: u8, y: u8| {
            let player = Player::ALL[player_id];
            let piece = Piece::ALL[piece_id];
            if let Cell::Occupied(_, _) = board.get(x, y) { return; }
            if (x, y) == Position::corner_goal(player) { return; }

            *board.get_mut(x, y) = Cell::Occupied(player, piece);
            *piece_count.get_mut(player, piece) -= 1;

            recurse(piece_count, board, f, player_id, piece_id, x + 1, y);

            *piece_count.get_mut(player, piece) += 1;
            *board.get_mut(x, y) = Cell::Empty;
        };

        if *piece_count == PieceCount::ZERO {
            f(board);
        } else {
            for player_id in cont_player_id..Player::ALL.len() {
                let player = Player::ALL[player_id];
                let is_continuing = player_id == cont_player_id;
                let min_piece_id = if is_continuing { cont_piece_id } else { 0 };
                for piece_id in min_piece_id..Piece::ALL.len() {
                    let piece = Piece::ALL[piece_id];
                    if piece_count.get(player, piece) != 0 {
                        let is_continuing = is_continuing && piece_id == cont_piece_id;
                        let min_y = if is_continuing { cont_y } else { 0 };
                        for y in min_y..9 {
                            let is_continuing = is_continuing && y == cont_y;
                            let min_x = if is_continuing { cont_x } else { 0 };
                            for x in min_x..9 {
                                check(piece_count, player_id, piece_id, x, y);
                            }
                        }
                        return;
                    }
                }
            }
        }
    }
    recurse(&mut piece_count, board, &mut f, 0, 0, 0, 0);
}

fn iter_backward_move_indexes<F: Fn(u64) -> ()>(piece_count: PieceCount, inverse_piece_count: PieceCount,
    board: &mut Board, f: F) {
    let players_flipped_piece_count = piece_count.players_flipped();
    for from_y in 0..9 {
        for from_x in 0..9 {
            if let Cell::Occupied(Player::Red, piece) = board.get(from_x, from_y) {
                for (to_x, to_y) in AdjacentLocs::new(from_x, from_y) {
                    if let Cell::Occupied(_, _) = board.get(to_x, to_y) {
                        continue;
                    } else if (to_x, to_y) == Position::corner_goal(Player::Red) {
                        continue;
                    }
                    *board.get_mut(from_x, from_y) = Cell::Empty;
                    *board.get_mut(to_x, to_y) = Cell::Occupied(Player::Red, piece);
                    board.invert_perspective();
                    let undo_normalize = normalization::normalize_board(board, players_flipped_piece_count);
                    let after_move_index = position_index::board_to_index(board, inverse_piece_count);
                    f(after_move_index);
                    normalization::undo_normalize_board(board, undo_normalize);
                    board.invert_perspective();
                    *board.get_mut(to_x, to_y) = Cell::Empty;
                    *board.get_mut(from_x, from_y) = Cell::Occupied(Player::Red, piece);
                }
            }
        }
    }
}

fn initialize_roots<'a>(piece_count: PieceCount, tb: &'a MemTb, queue: &'a AtomicBitset, scope: &rayon::Scope<'a>) {
    scope.spawn(move |_| {
        // TODO - handle moves that win in 1 by blocking all moves in a more efficient way
        (0..tb.wdl.len()).into_par_iter().with_min_len(65536).for_each(|before_move_index| {
            let mut position = Position::from_board_and_player(
                position_index::index_to_board(before_move_index as u64, piece_count), Player::Blue);
            // Handle board where blue can't make any legal moves
            match position.get_outcome() {
                None => (),
                Some(Outcome::Win(Player::Blue)) | Some(Outcome::Draw) => unreachable!(),
                Some(Outcome::Win(Player::Red)) => {
                    tb.wdl.set(before_move_index, WDL::Loss);
                    // dtz/dtm is already 0 so no need to update it
                    return;
                }
            }
            // Handle wins in 1 move that aren't explicitly covered below
            let legal_moves = position.legal_moves()
                .filter(|m| !m.is_capture)
                .filter(|m| (m.to_x, m.to_y) != Position::corner_goal(Player::Blue))
                .collect::<Vec<_>>();
            for m in legal_moves {
                let undo_move = position.make_move(m);
                let outcome = position.get_outcome();
                position.undo_move(undo_move);

                match outcome {
                    Some(Outcome::Win(Player::Blue)) => {
                        queue.insert(before_move_index);
                        break;
                    },
                    Some(Outcome::Win(Player::Red)) => unreachable!(),
                    Some(Outcome::Draw) => unreachable!(),
                    None => (),
                }
            }
        });
    });

    scope.spawn(move |_| {
        // Moving to the goal is a win in one
        let available_blue_pieces = Piece::iter()
            .filter(|&piece| piece_count.get(Player::Blue, piece) != 0)
            .collect::<Vec<_>>();
        available_blue_pieces.into_par_iter().with_max_len(1).for_each(|piece| {
            [(8, 7), (7, 7), (7, 8)].into_par_iter().with_max_len(1).for_each(|(x, y)| {
                let mut board = Board::new();
                *board.get_mut(x, y) = Cell::Occupied(Player::Blue, piece);
                let remaining_piece_count = piece_count - PieceCount::single(Player::Blue, piece);
                iter_all_boards(remaining_piece_count, &mut board, |board| {
                    let is_win = match board.get(8, 8) {
                        Cell::Empty => true,
                        Cell::Occupied(Player::Red, piece2) => piece.can_capture(piece2),
                        Cell::Occupied(Player::Blue, _) => unreachable!(),
                    };
                    if is_win {
                        queue.insert(position_index::board_to_index(board, piece_count) as usize);
                    }
                });
            });
        });
    });

    // Positions with captures are roots.  NOTE - there are positions with forced losses with a
    // capture, e.g.  Rs7/ps7/9/9/9/9/9/9/9
    let available_captured_pieces = Piece::iter()
        .filter(|&captured_piece|
            piece_count.get(Player::Red, captured_piece) != 0 &&
            piece_count.get(Player::Blue, captured_piece.captured_by()) != 0)
        .collect::<Vec<_>>();
    available_captured_pieces.into_par_iter().with_max_len(1).for_each(|captured_piece| {
        let capturing_piece = captured_piece.captured_by();
        let remaining_piece_count = piece_count - PieceCount::single(Player::Blue, capturing_piece) -
            PieceCount::single(Player::Red, captured_piece);

        (0..9).into_par_iter().with_max_len(1).for_each(|capturing_y| {
            (0..9).into_par_iter().with_max_len(1).for_each(|capturing_x| {
                if (capturing_x, capturing_y) == Position::corner_goal(Player::Blue) { return; }
                let mut board = Board::new();
                *board.get_mut(capturing_x, capturing_y) = Cell::Occupied(Player::Blue, capturing_piece);
                for (captured_x, captured_y) in AdjacentLocs::new(capturing_x, capturing_y) {
                    // Already checked captures into goal above
                    if (captured_x, captured_y) == Position::corner_goal(Player::Red) { continue; }
                    *board.get_mut(captured_x, captured_y) = Cell::Occupied(Player::Red, captured_piece);
                    iter_all_boards(remaining_piece_count, &mut board, |board| {
                        queue.insert(position_index::board_to_index(board, piece_count) as usize);
                    });
                    *board.get_mut(captured_x, captured_y) = Cell::Empty;
                }
                *board.get_mut(capturing_x, capturing_y) = Cell::Empty;
            });
        });
    });
}

fn compute_wdl_and_dtz(forward_piece_count: PieceCount, inverse_piece_count: PieceCount,
    forward_tb: &mut MemTb, inverse_tb: &mut MemTb, disk_tablebase: &TablebaseMap) {

    let forward_queue = AtomicBitset::new(forward_tb.wdl.len());
    let inverse_queue = AtomicBitset::new(inverse_tb.wdl.len());

    rayon::scope(|scope| {
        scope.spawn(|scope|
            initialize_roots(forward_piece_count, forward_tb, &forward_queue, scope));
        initialize_roots(inverse_piece_count, inverse_tb, &inverse_queue, scope);
    });

    let mut made_progress = true;
    let mut ply_threshold = 0;
    while made_progress {
        ply_threshold += 1;
        made_progress = [false, true].into_par_iter().with_max_len(1).map(|is_inverse| {
            let (tb, opposite_tb, piece_count, opposite_piece_count, queue, opposite_queue) = if is_inverse {
                (&*inverse_tb, &*forward_tb, inverse_piece_count, forward_piece_count, &inverse_queue, &forward_queue)
            } else {
                (&*forward_tb, &*inverse_tb, forward_piece_count, inverse_piece_count, &forward_queue, &inverse_queue)
            };
            queue.par_drain().map(|before_move_index| {
                if let Some(_) = tb.wdl.get_option(before_move_index) {
                    return false;
                }

                let board = position_index::index_to_board(before_move_index as u64, piece_count);
                let mut position = Position::from_board_and_player(board, Player::Blue);
                let mut best_wdl = Some(WDL::Loss);
                let mut min_dtz = u8::MAX;
                let mut max_dtz = 0;
                for m in position.legal_moves().collect::<Vec<_>>() {
                    let undo_move = position.make_move(m);
                    let outcome = position.get_outcome();

                    match outcome {
                        Some(Outcome::Win(Player::Blue)) => {
                            best_wdl = Some(WDL::Win);
                            min_dtz = 1;
                            position.undo_move(undo_move);
                            break;
                        },
                        Some(Outcome::Win(Player::Red)) => unreachable!(),
                        Some(Outcome::Draw) => unreachable!(),
                        None => {
                            let pre_normalize_piece_count = if m.is_capture {
                                PieceCount::from_board(&position.board)
                            } else {
                                piece_count
                            };
                            let undo_normalize = normalization::normalize_position(&mut position, pre_normalize_piece_count);
                            let after_move_piece_count = undo_normalize.piece_count;
                            let after_move_index = position_index::board_to_index(
                                &position.board, after_move_piece_count) as usize;

                            let dtz;
                            let wdl;
                            if m.is_capture {
                                wdl = disk_tablebase.lookup_wdl_raw(after_move_index as u64, after_move_piece_count).map(|wdl| -wdl);
                                dtz = 1;
                            } else {
                                dtz = opposite_tb.dtz[after_move_index].load(Ordering::Acquire) + 1;
                                wdl = opposite_tb.wdl.get_option(after_move_index).map(|wdl| -wdl);
                            }
                            best_wdl = match (best_wdl, wdl) {
                                (_, Some(WDL::Win)) => Some(WDL::Win),
                                (Some(WDL::Win), None) => Some(WDL::Win),
                                (_, None) => None,
                                (None, _) => None,
                                (Some(a), Some(b)) => Some(cmp::max(a, b)),
                            };
                            max_dtz = cmp::max(max_dtz, dtz);
                            if wdl == Some(WDL::Win) {
                                if dtz != 1 || ply_threshold == 1 {
                                    min_dtz = cmp::min(min_dtz, dtz);
                                } else {
                                    // NOTE - if dtz == 1 there was a concurrent modification. A racing
                                    // thread stored wdl as a loss (a win for us) after we read the unset
                                    // dtz. Since we iterate ply-by-ply this is irrelevant to the result
                                    // for this node since the dtz being stored would be above the ply
                                    // threshold.
                                    let mut dtz = 1;
                                    while dtz == 1 {
                                        dtz = opposite_tb.dtz[after_move_index].load(Ordering::Relaxed) + 1;
                                    }
                                    assert!(dtz > ply_threshold);
                                }
                            }

                            normalization::undo_normalize_position(&mut position, undo_normalize);
                        }
                    }
                    position.undo_move(undo_move);
                }

                match best_wdl {
                    Some(WDL::Win) | Some(WDL::Loss) => {
                        let wdl = best_wdl.unwrap();
                        let dtz = if wdl == WDL::Win { min_dtz } else { max_dtz };
                        if dtz == ply_threshold {
                            tb.wdl.set(before_move_index, wdl);
                            tb.dtz[before_move_index].store(dtz, Ordering::Release);
                            iter_backward_move_indexes(piece_count, opposite_piece_count, &mut position.board,
                                |after_backward_move_index| {
                                    opposite_queue.insert(after_backward_move_index as usize);
                                });
                        } else {
                            // This can happen because of concurrent processing racing one section of
                            // the TB ahead. We'll try again next iteration and succeed.
                            debug_assert!(dtz > ply_threshold);
                            queue.insert(before_move_index);
                        }
                        true
                    },
                    Some(WDL::Draw) => {
                        // Most drawn positions can't be forced by backwards propagation, so it
                        // isn't useful to try any further backwards even if it's known this
                        // position is a draw
                        tb.wdl.set(before_move_index, WDL::Draw);
                        false
                    },
                    None => {
                        // This node will be re-enqueued when child nodes change to possibly allow the
                        // WDL to be determined
                        false
                    },
                }
            }).reduce(|| false, |a, b| a || b)
        }).reduce(|| false, |a, b| a || b);
    }

    [&*forward_tb, &*inverse_tb].par_iter().with_max_len(1).for_each(|tb| {
        (0..tb.wdl.len()).into_par_iter().with_min_len(65536).for_each(|index| {
            if tb.wdl.get_option(index).is_none() {
                // Anything that can't be forced into a win/loss is a draw
                tb.wdl.set(index, WDL::Draw);
            } else if tb.dtz[index].load(Ordering::Relaxed) > 200 {
                // This would be modified to a draw, but in practice I don't think it can actually
                // happen so let's just assert and revisit that later if neeeded
                panic!("greater than 200 plies dtz reached");
            }
        });
    });
}

// Computing DTM has to re-visit nodes and is slower
fn compute_dtm(forward_piece_count: PieceCount, inverse_piece_count: PieceCount,
    forward_tb: &mut MemTb, inverse_tb: &mut MemTb, disk_tablebase: &TablebaseMap) {

    let forward_queue = AtomicBitset::new(forward_tb.wdl.len());
    let inverse_queue = AtomicBitset::new(inverse_tb.wdl.len());

    [false, true].into_par_iter().with_max_len(1).for_each(|is_inverse| {
        let (tb, piece_count, opposite_piece_count, opposite_queue, queue) = if is_inverse {
            (&*inverse_tb, inverse_piece_count, forward_piece_count, &forward_queue, &inverse_queue)
        } else {
            (&*forward_tb, forward_piece_count, inverse_piece_count, &inverse_queue, &forward_queue)
        };

        // All wins in 1 dtz are roots for the dtm search. This is slower than re-enumerating the
        // roots but saves code complexity
        (0..tb.wdl.len()).into_par_iter().with_min_len(65536).for_each(|before_move_index| {
            if tb.dtz[before_move_index].load(Ordering::Relaxed) == 1 && tb.wdl.get(before_move_index) == WDL::Win {
                let mut position = Position::from_board_and_player(
                    position_index::index_to_board(before_move_index as u64, piece_count), Player::Blue);
                for m in position.legal_moves().collect::<Vec<_>>() {
                    let undo_move = position.make_move(m);
                    let outcome = position.get_outcome();
                    position.undo_move(undo_move);

                    match outcome {
                        Some(Outcome::Win(Player::Blue)) => {
                            tb.dtm[before_move_index].store(1, Ordering::Relaxed);
                            iter_backward_move_indexes(piece_count, opposite_piece_count, &mut position.board,
                                |after_backward_move_index| {
                                    opposite_queue.insert(after_backward_move_index as usize);
                                });
                            return;
                        },
                        Some(Outcome::Win(Player::Red)) => unreachable!(),
                        Some(Outcome::Draw) => unreachable!(),
                        None => (),
                    }
                }
                // Must sit in the queue until dtm == ply_threshold or a
                // faster path is found
                queue.insert(before_move_index);
            }
        });
    });

    let mut made_progress = true;
    let mut ply_threshold = 1;
    while made_progress {
        ply_threshold += 1;
        made_progress = [false, true].into_par_iter().with_max_len(1).map(|is_inverse| {
            let (tb, opposite_tb, piece_count, opposite_piece_count, queue, opposite_queue) = if is_inverse {
                (&*inverse_tb, &*forward_tb, inverse_piece_count, forward_piece_count, &inverse_queue, &forward_queue)
            } else {
                (&*forward_tb, &*inverse_tb, forward_piece_count, inverse_piece_count, &forward_queue, &inverse_queue)
            };
            queue.par_drain().map(|before_move_index| {
                if tb.dtm[before_move_index].load(Ordering::Relaxed) != 0 ||
                    tb.wdl.get(before_move_index) == WDL::Draw {
                    return false;
                }

                let board = position_index::index_to_board(before_move_index as u64, piece_count);
                let mut position = Position::from_board_and_player(board, Player::Blue);
                let mut min_dtm = u16::MAX;
                let mut max_dtm = 0;
                for m in position.legal_moves().collect::<Vec<_>>() {
                    let undo_move = position.make_move(m);
                    match position.get_outcome() {
                        Some(Outcome::Win(Player::Blue)) => unreachable!(), // calculated already
                        Some(Outcome::Win(Player::Red)) => unreachable!(),
                        Some(Outcome::Draw) => unreachable!(),
                        None => {
                            let pre_normalize_piece_count = if m.is_capture {
                                PieceCount::from_board(&position.board)
                            } else {
                                piece_count
                            };
                            let undo_normalize = normalization::normalize_position(&mut position, pre_normalize_piece_count);
                            let after_move_piece_count = undo_normalize.piece_count;
                            let after_move_index = position_index::board_to_index(
                                &position.board, after_move_piece_count);
                            if m.is_capture {
                                let wdl = -disk_tablebase.lookup_wdl_raw(
                                    after_move_index, after_move_piece_count).unwrap();
                                let dtm = disk_tablebase.lookup_dtm_raw(
                                    after_move_index, after_move_piece_count).unwrap() + 1;
                                debug_assert_ne!(dtm, 1); // Because it would be a win_in_one
                                max_dtm = cmp::max(max_dtm, dtm);
                                if wdl == WDL::Win {
                                    min_dtm = cmp::min(min_dtm, dtm);
                                }
                            } else {
                                let wdl = -opposite_tb.wdl.get(after_move_index as usize);
                                let dtm = opposite_tb.dtm[after_move_index as usize].load(Ordering::Relaxed) + 1;
                                // If the dtm of the child node is unset, treat it as high
                                let dtm = if dtm == 1 { u16::MAX } else { dtm };
                                max_dtm = cmp::max(max_dtm, dtm);
                                if wdl == WDL::Win {
                                    min_dtm = cmp::min(min_dtm, dtm);
                                }
                            }

                            normalization::undo_normalize_position(&mut position, undo_normalize);
                        }
                    }

                    position.undo_move(undo_move);
                }

                let dtm = if tb.wdl.get(before_move_index) == WDL::Win { min_dtm } else { max_dtm };
                if dtm == ply_threshold {
                    tb.dtm[before_move_index].store(dtm, Ordering::Relaxed);
                    iter_backward_move_indexes(piece_count, opposite_piece_count, &mut position.board,
                        |after_backward_move_index| {
                            opposite_queue.insert(after_backward_move_index as usize);
                        });
                } else {
                    // This can happen because of concurrent processing racing one section of
                    // the TB ahead, or because we keep requeuing winning captures until the
                    // threshold is reached. We'll try again next iteration.
                    debug_assert!(dtm > ply_threshold);
                    queue.insert(before_move_index);
                }
                true
            }).reduce(|| false, |a, b| a || b)
        }).reduce(|| false, |a, b| a || b);
    }

    if cfg!(debug_assertions) {
        [(&*forward_tb, forward_piece_count), (&*inverse_tb, inverse_piece_count)].into_par_iter().with_max_len(1).for_each(|(tb, pc)| {
            (0..tb.wdl.len()).into_par_iter().with_min_len(65535).for_each(|index| {
                if tb.wdl.get(index) == WDL::Draw {
                    assert_eq!(tb.dtm[index].load(Ordering::Relaxed), 0);
                } else {
                    assert_ne!(tb.dtm[index].load(Ordering::Relaxed), 0, "{}", position_index::index_to_board(index as u64, pc).to_fen());
                }
            });
        });
    }
}


fn gen_tablebase(forward_piece_count: PieceCount, disk_tablebase: &TablebaseMap, enable_dtm: bool) -> (MemTb, MemTb) {
    debug_assert!(normalization::is_count_normalized(forward_piece_count));
    let inverse_piece_count = normalization::normalize_count(forward_piece_count.players_flipped());

    let forward_len = position_index::index_count(forward_piece_count) as usize;
    let inverse_len = position_index::index_count(inverse_piece_count) as usize;
    debug_assert_eq!(forward_len, inverse_len);

    let mut forward_tb = MemTb::new(forward_len, enable_dtm);
    let mut inverse_tb = MemTb::new(inverse_len, enable_dtm);

    compute_wdl_and_dtz(forward_piece_count, inverse_piece_count,
        &mut forward_tb, &mut inverse_tb, disk_tablebase);
    if enable_dtm {
        compute_dtm(forward_piece_count, inverse_piece_count,
            &mut forward_tb, &mut inverse_tb, disk_tablebase);
    }

    (forward_tb, inverse_tb)
}


fn output(piece_count: PieceCount, tablebase: &MemTb, opts: &Opts) {
    match opts.output_format {
        OutputFormat::Blackhole => (), // Output nothing
        OutputFormat::Binary => {
            output_binary(piece_count, tablebase, opts).unwrap();
        },
        OutputFormat::Text => {
            output_text(piece_count, tablebase, opts).unwrap();
        },
        OutputFormat::ZlibCompressed => unimplemented!(),
    }
}

fn output_text(piece_count: PieceCount, tablebase: &MemTb, opts: &Opts) -> io::Result<()> {
    fs::create_dir_all(&format!("{}", opts.output_dir))?;
    let mut file = fs::File::create(
        &format!("{}/{}.txt", opts.output_dir, tablebase_name(piece_count)))?;

    let len = tablebase.wdl.len();
    const CHUNK_SIZE: usize = 1 << 16;
    for chunk_index in 0..((len + CHUNK_SIZE - 1) / CHUNK_SIZE) {
        let min_index = chunk_index * CHUNK_SIZE;
        let max_index = cmp::min(min_index + CHUNK_SIZE, len);
        let s = (min_index..max_index).into_par_iter().map(|index| {
            let board = position_index::index_to_board(index as u64, piece_count);
            let wdl = tablebase.wdl.get(index);
            let dtz = tablebase.dtz[index].load(Ordering::Relaxed);
            if opts.enable_dtm {
                let dtm = tablebase.dtm[index].load(Ordering::Relaxed);
                format!("{} {wdl} {dtz} {dtm}\n", board.to_fen())
            } else {
                format!("{} {wdl} {dtz}\n", board.to_fen())
            }
        }).reduce(|| String::new(), |a, b| { a + &b });
        file.write_all(s.as_bytes())?;
    }
    Ok(())
}

fn output_binary(piece_count: PieceCount, tablebase: &MemTb, opts: &Opts) -> io::Result<()> {
    fs::create_dir_all(&format!("{}", opts.output_dir))?;
    {
        let mut file = fs::File::create(
            format!("{}/{}.tbwdl", opts.output_dir, tablebase_name(piece_count)))?;
        file.write_all(unsafe {
            std::slice::from_raw_parts(
                tablebase.wdl.bytes.as_ptr() as *const u8,
                tablebase.wdl.bytes.len())
        })?;
        file.write_all(&[0u8])?;
    }
    {
        let mut file = fs::File::create(
            format!("{}/{}.tbdtz", opts.output_dir, tablebase_name(piece_count)))?;
        file.write_all(unsafe {
            std::slice::from_raw_parts(
                tablebase.dtz.as_ptr() as *const u8,
                tablebase.dtz.len())
        })?;
        file.write_all(&[0u8])?;
    }
    if opts.enable_dtm {
        let mut file = fs::File::create(
            format!("{}/{}.tbdtm", opts.output_dir, tablebase_name(piece_count)))?;
        // TODO - probably faster way to do this
        for atomic_ref in &tablebase.dtm {
            file.write_all(&atomic_ref.load(Ordering::Relaxed).to_le_bytes())?;
        }
        file.write_all(&[0u8])?;
    }
    Ok(())
}

fn parse_command_line_args() -> Opts {
    let mut opts = Opts {
        upto_pieces: 0,
        enable_dtm: false,
        output_dir: "data/tablebase".to_string(),
        output_format: OutputFormat::Binary, // TODO - default to ZlibCompressed when implemented
        num_threads: 0, // If 0, num_cpus is used
    };

    let mut args = std::env::args();
    args.next(); // Skip binary name
    while let Some(token) = args.next() {
        match token.as_str() {
            "--upto_pieces" | "--upto-pieces" | "--upto" => {
                opts.upto_pieces = args.next().unwrap().parse().unwrap();
            },
            "--enable_dtm" | "--enable-dtm" | "--dtm" => opts.enable_dtm = true,
            "--directory" | "--dir" | "--output_dir" | "--output-dir" => {
                opts.output_dir = args.next().unwrap().to_string();
            },
            "--format" | "--output_format" | "--output-format" => {
                opts.output_format = match args.next().unwrap().as_str() {
                    "none" | "blackhole" => OutputFormat::Blackhole,
                    "binary" | "bin" => OutputFormat::Binary,
                    "compressed" | "zlib-compressed" => OutputFormat::ZlibCompressed,
                    "text" | "fen" | "txt" => OutputFormat::Text,
                    f => panic!("unknown output format: {}", f),
                };
            },
            "--threads" | "--num_threads" | "--num-threads" => {
                opts.num_threads = args.next().unwrap().parse().unwrap();
            },
            _ => {
                panic!("unknown command line argument \"{}\"", token);
            },
        };
    }

    opts
}

fn init_threads(opts: &Opts) {
    let actual_threads = if opts.num_threads != 0 {
        opts.num_threads
    } else {
        std::thread::available_parallelism().unwrap().get()
    };
    rayon::ThreadPoolBuilder::new()
        .num_threads(actual_threads)
        .build_global()
        .unwrap();
}

