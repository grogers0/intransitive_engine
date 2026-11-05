use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::cmp;
use std::fs;

use rayon::prelude::*;
use tablebase::{PieceCount, WDL, position_index, normalization, tablebase_name, TablebaseMap, validation, AtomicBitset};
use tablebase::tablebase::{DtmTablebaseMut, WdlTablebaseMut, DtzTablebaseMut};
use base::{AdjacentLocs, Board, Cell, Outcome, Piece, Player, Position};

struct Opts {
    upto_pieces: u8,
    enable_dtm: bool,
    tablebase_dir: String,
    compressed_output: bool,
    num_threads: usize,
}

fn main() {
    let opts = parse_command_line_args();
    init_threads(&opts);

    let mut tablebase_map = TablebaseMap::new(&opts.tablebase_dir);
    for total_count in 0..=opts.upto_pieces {
        let mut already_generated = HashSet::new();
        for piece_count in PieceCount::enumerate_useful_piece_counts(total_count) {
            let piece_count = normalization::normalize_count(piece_count);
            if already_generated.contains(&piece_count) ||
                tablebase_already_generated(piece_count, &tablebase_map, opts.enable_dtm) {
                    continue;
            }
            gen_tablebase(piece_count, &tablebase_map, &opts);
            already_generated.insert(piece_count);
            already_generated.insert(normalization::normalize_count(
                    piece_count.players_flipped()));
        }
        tablebase_map.refresh(&opts.tablebase_dir).unwrap();
    }

    if cfg!(debug_assertions) {
        validation::validate_tablebase(&tablebase_map, !opts.enable_dtm);
    }
}

fn tablebase_already_generated(piece_count: PieceCount, tablebase_map: &TablebaseMap, enable_dtm: bool) -> bool {
    tablebase_map.lookup_wdl_raw(0, piece_count).is_some() &&
        tablebase_map.lookup_dtz_raw(0, piece_count).is_some() &&
        (!enable_dtm || tablebase_map.lookup_dtm_raw(0, piece_count).is_some())
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

fn initialize_roots<'a>(piece_count: PieceCount, wdl_tb: &'a WdlTablebaseMut, queue: &'a AtomicBitset, scope: &rayon::Scope<'a>) {
    scope.spawn(move |_| {
        // TODO - handle moves that win in 1 by blocking all moves in a more efficient way
        (0..wdl_tb.len()).into_par_iter().for_each(|before_move_index| {
            let mut position = Position::from_board_and_player(
                position_index::index_to_board(before_move_index as u64, piece_count), Player::Blue);
            // Handle board where blue can't make any legal moves
            match position.get_outcome() {
                None => (),
                Some(Outcome::Win(Player::Blue)) | Some(Outcome::Draw) => unreachable!(),
                Some(Outcome::Win(Player::Red)) => {
                    wdl_tb.store(before_move_index, WDL::Loss, Ordering::Relaxed);
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
                        queue.insert(position_index::board_to_index(board, piece_count));
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
                        queue.insert(position_index::board_to_index(board, piece_count));
                    });
                    *board.get_mut(captured_x, captured_y) = Cell::Empty;
                }
                *board.get_mut(capturing_x, capturing_y) = Cell::Empty;
            });
        });
    });
}

fn compute_wdl_and_dtz(forward_piece_count: PieceCount, inverse_piece_count: PieceCount,
    forward_wdl_tb: &mut WdlTablebaseMut, forward_dtz_tb: &mut DtzTablebaseMut,
    inverse_wdl_tb: &mut WdlTablebaseMut, inverse_dtz_tb: &mut DtzTablebaseMut,
    tablebase_map: &TablebaseMap) {

    let forward_queue = AtomicBitset::new(forward_wdl_tb.len());
    let inverse_queue = AtomicBitset::new(inverse_wdl_tb.len());

    rayon::scope(|scope| {
        scope.spawn(|scope|
            initialize_roots(forward_piece_count, forward_wdl_tb, &forward_queue, scope));
        initialize_roots(inverse_piece_count, inverse_wdl_tb, &inverse_queue, scope);
    });

    let mut made_progress = true;
    let mut ply_threshold = 0;
    while made_progress {
        ply_threshold += 1;
        made_progress = [false, true].into_par_iter().with_max_len(1).map(|is_inverse| {
            let (wdl_tb, dtz_tb, opposite_wdl_tb, opposite_dtz_tb, piece_count, opposite_piece_count, queue, opposite_queue) = if is_inverse {
                (&*inverse_wdl_tb, &*inverse_dtz_tb, &*forward_wdl_tb, &*forward_dtz_tb, inverse_piece_count, forward_piece_count, &inverse_queue, &forward_queue)
            } else {
                (&*forward_wdl_tb, &*forward_dtz_tb, &*inverse_wdl_tb, &*inverse_dtz_tb, forward_piece_count, inverse_piece_count, &forward_queue, &inverse_queue)
            };
            queue.par_drain().map(|before_move_index| {
                if let Some(_) = wdl_tb.load(before_move_index, Ordering::Relaxed) {
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
                                &position.board, after_move_piece_count);

                            let dtz;
                            let wdl;
                            if m.is_capture {
                                wdl = tablebase_map.lookup_wdl_raw(after_move_index, after_move_piece_count).map(|wdl| -wdl);
                                dtz = 1;
                            } else {
                                dtz = opposite_dtz_tb.load(after_move_index, Ordering::Acquire) + 1;
                                wdl = opposite_wdl_tb.load(after_move_index, Ordering::Relaxed).map(|wdl| -wdl);
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
                                        dtz = opposite_dtz_tb.load(after_move_index, Ordering::Relaxed) + 1;
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
                            wdl_tb.store(before_move_index, wdl, Ordering::Relaxed);
                            dtz_tb.store(before_move_index, dtz, Ordering::Release);
                            iter_backward_move_indexes(piece_count, opposite_piece_count, &mut position.board,
                                |after_backward_move_index| {
                                    opposite_queue.insert(after_backward_move_index);
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
                        wdl_tb.store(before_move_index, WDL::Draw, Ordering::Relaxed);
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

    [(&*forward_wdl_tb, &*forward_dtz_tb), (&*inverse_wdl_tb, &*inverse_dtz_tb)].par_iter().with_max_len(1).for_each(|(wdl_tb, dtz_tb)| {
        (0..wdl_tb.len()).into_par_iter().for_each(|index| {
            if wdl_tb.load(index, Ordering::Relaxed).is_none() {
                // Anything that can't be forced into a win/loss is a draw
                wdl_tb.store(index, WDL::Draw, Ordering::Relaxed);
            } else if dtz_tb.load(index, Ordering::Relaxed) > 200 {
                // This would be modified to a draw, but in practice I don't think it can actually
                // happen so let's just assert and revisit that later if neeeded
                panic!("greater than 200 plies dtz reached");
            }
        });
    });
}

// Computing DTM has to re-visit nodes and is slower
fn compute_dtm(forward_piece_count: PieceCount, inverse_piece_count: PieceCount,
    forward_wdl_tb: &WdlTablebaseMut, forward_dtz_tb: &DtzTablebaseMut, forward_dtm_tb: &mut DtmTablebaseMut,
    inverse_wdl_tb: &WdlTablebaseMut, inverse_dtz_tb: &DtzTablebaseMut, inverse_dtm_tb: &mut DtmTablebaseMut,
    tablebase_map: &TablebaseMap) {

    let forward_queue = AtomicBitset::new(forward_wdl_tb.len());
    let inverse_queue = AtomicBitset::new(inverse_wdl_tb.len());

    [false, true].into_par_iter().with_max_len(1).for_each(|is_inverse| {
        let (wdl_tb, dtz_tb, dtm_tb, piece_count, opposite_piece_count, opposite_queue, queue) = if is_inverse {
            (&*inverse_wdl_tb, &*inverse_dtz_tb, &*inverse_dtm_tb, inverse_piece_count, forward_piece_count, &forward_queue, &inverse_queue)
        } else {
            (&*forward_wdl_tb, &*forward_dtz_tb, &*forward_dtm_tb, forward_piece_count, inverse_piece_count, &inverse_queue, &forward_queue)
        };

        // All wins in 1 dtz are roots for the dtm search. This is slower than re-enumerating the
        // roots but saves code complexity
        (0..wdl_tb.len()).into_par_iter().for_each(|before_move_index| {
            if dtz_tb.load(before_move_index, Ordering::Relaxed) == 1 && wdl_tb.load(before_move_index, Ordering::Relaxed).unwrap() == WDL::Win {
                let mut position = Position::from_board_and_player(
                    position_index::index_to_board(before_move_index as u64, piece_count), Player::Blue);
                for m in position.legal_moves().collect::<Vec<_>>() {
                    let undo_move = position.make_move(m);
                    let outcome = position.get_outcome();
                    position.undo_move(undo_move);

                    match outcome {
                        Some(Outcome::Win(Player::Blue)) => {
                            dtm_tb.store(before_move_index, 1, Ordering::Relaxed);
                            iter_backward_move_indexes(piece_count, opposite_piece_count, &mut position.board,
                                |after_backward_move_index| {
                                    opposite_queue.insert(after_backward_move_index);
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
            let (wdl_tb, dtm_tb, opposite_wdl_tb, opposite_dtm_tb, piece_count, opposite_piece_count, queue, opposite_queue) = if is_inverse {
                (&*inverse_wdl_tb, &*inverse_dtm_tb, &*forward_wdl_tb, &*forward_dtm_tb, inverse_piece_count, forward_piece_count, &inverse_queue, &forward_queue)
            } else {
                (&*forward_wdl_tb, &*forward_dtm_tb, &*inverse_wdl_tb, &*inverse_dtm_tb, forward_piece_count, inverse_piece_count, &forward_queue, &inverse_queue)
            };
            queue.par_drain().map(|before_move_index| {
                if dtm_tb.load(before_move_index, Ordering::Relaxed) != 0 ||
                    wdl_tb.load(before_move_index, Ordering::Relaxed).unwrap() == WDL::Draw {
                    return false;
                }

                let board = position_index::index_to_board(before_move_index, piece_count);
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
                                let wdl = -tablebase_map.lookup_wdl_raw(
                                    after_move_index, after_move_piece_count).unwrap();
                                let dtm = tablebase_map.lookup_dtm_raw(
                                    after_move_index, after_move_piece_count).unwrap() + 1;
                                debug_assert_ne!(dtm, 1); // Because it would be a win_in_one
                                max_dtm = cmp::max(max_dtm, dtm);
                                if wdl == WDL::Win {
                                    min_dtm = cmp::min(min_dtm, dtm);
                                }
                            } else {
                                let wdl = -opposite_wdl_tb.load(after_move_index, Ordering::Relaxed).unwrap();
                                let dtm = opposite_dtm_tb.load(after_move_index, Ordering::Relaxed) + 1;
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

                let dtm = if wdl_tb.load(before_move_index, Ordering::Relaxed).unwrap() == WDL::Win { min_dtm } else { max_dtm };
                if dtm == ply_threshold {
                    dtm_tb.store(before_move_index, dtm, Ordering::Relaxed);
                    iter_backward_move_indexes(piece_count, opposite_piece_count, &mut position.board,
                        |after_backward_move_index| {
                            opposite_queue.insert(after_backward_move_index);
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
        [(&*forward_wdl_tb, &*forward_dtm_tb), (&*inverse_wdl_tb, &*inverse_dtm_tb)].into_par_iter().with_max_len(1).for_each(|(wdl_tb, dtm_tb)| {
            (0..wdl_tb.len()).into_par_iter().for_each(|index| {
                if wdl_tb.load(index, Ordering::Relaxed).unwrap() == WDL::Draw {
                    assert_eq!(dtm_tb.load(index, Ordering::Relaxed), 0);
                } else {
                    assert_ne!(dtm_tb.load(index, Ordering::Relaxed), 0);
                }
            });
        });
    }
}

fn gen_tablebase(forward_piece_count: PieceCount, tablebase_map: &TablebaseMap, opts: &Opts) {
    debug_assert!(normalization::is_count_normalized(forward_piece_count));
    let inverse_piece_count = normalization::normalize_count(forward_piece_count.players_flipped());

    let forward_len = position_index::index_count(forward_piece_count);
    let inverse_len = position_index::index_count(inverse_piece_count);
    debug_assert_eq!(forward_len, inverse_len);

    fs::create_dir_all(format!("{}/wip", opts.tablebase_dir)).unwrap();
    let wip_filename = |piece_count: PieceCount, extension: &str| -> String {
        format!("{}/wip/{}.tb{}", opts.tablebase_dir, tablebase_name(piece_count), extension)
    };
    let mut forward_wdl_tb = WdlTablebaseMut::new(
        &wip_filename(forward_piece_count, "wdl"), forward_len).unwrap();
    let mut forward_dtz_tb = DtzTablebaseMut::new(
        &wip_filename(forward_piece_count, "dtz"), forward_len).unwrap();
    let mut inverse_wdl_tb = WdlTablebaseMut::new(
        &wip_filename(inverse_piece_count, "wdl"), inverse_len).unwrap();
    let mut inverse_dtz_tb = DtzTablebaseMut::new(
        &wip_filename(inverse_piece_count, "dtz"), inverse_len).unwrap();

    forward_wdl_tb.init();
    forward_dtz_tb.init();
    inverse_wdl_tb.init();
    inverse_dtz_tb.init();

    compute_wdl_and_dtz(forward_piece_count, inverse_piece_count, &mut forward_wdl_tb,
        &mut forward_dtz_tb, &mut inverse_wdl_tb, &mut inverse_dtz_tb, tablebase_map);
    if opts.enable_dtm {
        let mut forward_dtm_tb = DtmTablebaseMut::new(
            &wip_filename(forward_piece_count, "dtm"), forward_len).unwrap();
        let mut inverse_dtm_tb = DtmTablebaseMut::new(
            &wip_filename(inverse_piece_count, "dtm"), inverse_len).unwrap();

        forward_dtm_tb.init();
        inverse_dtm_tb.init();

        compute_dtm(forward_piece_count, inverse_piece_count,
            &forward_wdl_tb, &forward_dtz_tb, &mut forward_dtm_tb,
            &inverse_wdl_tb, &inverse_dtz_tb, &mut inverse_dtm_tb, tablebase_map);

        rayon::scope(|scope| {
            scope.spawn(move |_| { forward_dtm_tb.flush().unwrap() });
            scope.spawn(move |_| { inverse_dtm_tb.flush().unwrap() });
        });
    }

    rayon::scope(|scope| {
        scope.spawn(move |_| { forward_wdl_tb.flush().unwrap() });
        scope.spawn(move |_| { forward_dtz_tb.flush().unwrap() });
        scope.spawn(move |_| { inverse_wdl_tb.flush().unwrap() });
        scope.spawn(move |_| { inverse_dtz_tb.flush().unwrap() });
    });

    let dest_filename = |piece_count: PieceCount, extension: &str| -> String {
        format!("{}/{}.tb{}", opts.tablebase_dir, tablebase_name(piece_count), extension)
    };
    let mut piece_counts = vec![forward_piece_count];
    if inverse_piece_count != forward_piece_count {
        // Symmetric piece counts can be generated as if they are two separate files but we can
        // only rename the file once
        piece_counts.push(inverse_piece_count);
    }
    let mut extensions = vec!["wdl", "dtz"];
    if opts.enable_dtm {
        extensions.push("dtm");
    }
    for piece_count in piece_counts {
        for extension in &extensions {
            fs::rename(wip_filename(piece_count, extension), dest_filename(piece_count, extension)).unwrap();
        }
    }
}


fn parse_command_line_args() -> Opts {
    let mut opts = Opts {
        upto_pieces: 0,
        enable_dtm: false,
        tablebase_dir: "data/tablebase".to_string(),
        compressed_output: false,
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
            "--directory" | "--dir" | "--tablebase_dir" | "--tablebase-dir" => {
                opts.tablebase_dir = args.next().unwrap().to_string();
            },
            "--compress" | "--compressed" => {
                opts.compressed_output = true;
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

