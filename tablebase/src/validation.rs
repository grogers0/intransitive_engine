use std::cmp;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

use crate::{WDL, normalization, position_index, PieceCount, TablebaseMap, tablebase_name};
use base::{Outcome, Player, Position};

pub fn validate_tablebase(tablebase_map: &TablebaseMap, disable_dtm: bool) {
    tablebase_map.get_covered_piece_counts().into_par_iter().for_each(|piece_count| {
        assert!(normalization::is_count_normalized(piece_count));
        let num_failures = AtomicUsize::new(0);
        let best_failure: Mutex<Option<(u8, String)>> = Mutex::new(None);
        (0..position_index::index_count(piece_count)).into_par_iter().for_each(|before_move_index| {
            let mut position = Position::from_board_and_player(
                position_index::index_to_board(before_move_index, piece_count), Player::Blue);
            let mut best_wdl = WDL::Loss;
            let mut min_dtz = u8::MAX;
            let mut max_dtz = 0;
            let mut min_dtm = u16::MAX;
            let mut max_dtm = 0;
            let mut dtm_disabled = disable_dtm;
            for m in position.legal_moves().collect::<Vec<_>>() {
                let undo_move = position.make_move(m);
                match position.get_outcome() {
                    Some(Outcome::Win(Player::Blue)) => {
                        best_wdl = WDL::Win;
                        min_dtz = cmp::min(min_dtz, 1);
                        max_dtz = cmp::max(max_dtz, 1);

                        min_dtm = cmp::min(min_dtm, 1);
                        max_dtm = cmp::max(max_dtm, 1);
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
                        let wdl = -tablebase_map.lookup_wdl_raw(
                            after_move_index, after_move_piece_count).unwrap();
                        best_wdl = cmp::max(best_wdl, wdl);

                        let dtz = if m.is_capture {
                            1
                        } else {
                            tablebase_map.lookup_dtz_raw(
                                after_move_index, after_move_piece_count).unwrap() + 1
                        };
                        max_dtz = cmp::max(max_dtz, dtz);
                        if wdl == WDL::Win {
                            min_dtz = cmp::min(min_dtz, dtz);
                        }

                        let dtm_opt = tablebase_map.lookup_dtm_raw(
                                after_move_index, after_move_piece_count).map(|inner| inner + 1);
                        if let Some(dtm) = dtm_opt {
                            max_dtm = cmp::max(max_dtm, dtm);
                            if wdl == WDL::Win {
                                min_dtm = cmp::min(min_dtm, dtm);
                            }
                        } else {
                            dtm_disabled = true;
                        }

                        normalization::undo_normalize_position(&mut position, undo_normalize);
                    }
                }

                position.undo_move(undo_move);
            }

            let computed_wdl = best_wdl;
            let tablebase_wdl = tablebase_map.lookup_wdl_raw(before_move_index, piece_count).unwrap();

            let computed_dtz = match computed_wdl {
                WDL::Win => min_dtz,
                WDL::Draw => 0,
                WDL::Loss => max_dtz,
            };
            let tablebase_dtz = tablebase_map.lookup_dtz_raw(before_move_index, piece_count).unwrap();

            let computed_dtm = match computed_wdl {
                WDL::Win => min_dtm,
                WDL::Draw => 0,
                WDL::Loss => max_dtm,
            };
            let tablebase_dtm = tablebase_map.lookup_dtm_raw(before_move_index, piece_count);

            if computed_wdl != tablebase_wdl || computed_dtz != tablebase_dtz || (!dtm_disabled && Some(computed_dtm) != tablebase_dtm) {
                num_failures.fetch_add(1, Ordering::Relaxed);
                let mut guard = best_failure.lock().unwrap();
                if guard.is_none() || computed_dtz < guard.as_ref().unwrap().0 {
                    use std::fmt::Write;
                    let mut debug_message = String::new();
                    writeln!(debug_message, "{} b:", position.board.to_fen()).unwrap();
                    if computed_wdl != tablebase_wdl {
                        writeln!(debug_message, "    computed_wdl ({computed_wdl:?}) != tablebase_wdl ({tablebase_wdl})").unwrap();
                    }
                    if computed_dtz != tablebase_dtz {
                        writeln!(debug_message, "    computed_dtz ({computed_dtz}) != tablebase_dtz ({tablebase_dtz})").unwrap();
                    }
                    if !dtm_disabled && Some(computed_dtm) != tablebase_dtm {
                        writeln!(debug_message, "    computed_dtm ({computed_dtm}) != tablebase_dtm ({})", tablebase_dtm.unwrap()).unwrap();
                    }

                    writeln!(debug_message, "Here are the legal moves:").unwrap();

                    for m in position.legal_moves().collect::<Vec<_>>() {
                        let undo_move = position.make_move(m);
                        match position.get_outcome() {
                            Some(Outcome::Win(Player::Blue)) => {
                                writeln!(debug_message, "    {m} => {} r | Win, dtz: 1, dtm: 1", position.board.to_fen()).unwrap();
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
                                let wdl = -tablebase_map.lookup_wdl_raw(
                                    after_move_index, after_move_piece_count).unwrap();
                                let dtz = if m.is_capture { 1 }
                                else { tablebase_map.lookup_dtz_raw(
                                    after_move_index, after_move_piece_count).unwrap() + 1 };
                                let dtm = tablebase_map.lookup_dtm_raw(
                                    after_move_index, after_move_piece_count).map(|inner| inner + 1);
                                writeln!(debug_message, "    {m} => {} b | {wdl:?}, dtz: {dtz}, dtm: {dtm:?}", position.board.to_fen()).unwrap();

                                normalization::undo_normalize_position(&mut position, undo_normalize);
                            }
                        }

                        position.undo_move(undo_move);
                    }
                    *guard = Some((computed_dtz, debug_message));
                }
            }
        });
        if let Some((_, debug_message)) = best_failure.into_inner().unwrap() {
            panic!("For {} found {} positions with validation failures, one example is {}",
                tablebase_name(piece_count), num_failures.load(Ordering::Relaxed), debug_message);
        }
    });
}
