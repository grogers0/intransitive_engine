use std::cmp;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

use crate::{WDL, normalization, position_index, PieceCount, tablebase_name};
use crate::tablebase::{WdlTablebase, DtzTablebase, DtmTablebase, TablebaseMap};
use base::{Outcome, Player, Position};

pub fn validate_tablebase(piece_count: PieceCount, opposite_piece_count: PieceCount,
    wdl_tb: &impl WdlTablebase, dtz_tb: &impl DtzTablebase, dtm_tb: Option<&impl DtmTablebase>,
    opposite_wdl_tb: &impl WdlTablebase, opposite_dtz_tb: &impl DtzTablebase, opposite_dtm_tb: Option<&impl DtmTablebase>,
    tablebase_map: &TablebaseMap) {
    debug_assert!(normalization::is_count_normalized(piece_count));
    debug_assert_eq!(opposite_piece_count, normalization::normalize_count(piece_count.players_flipped()));

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
        let disable_dtm = dtm_tb.is_none();

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
                    let wdl = if m.is_capture {
                        -tablebase_map.lookup_wdl_raw(
                            after_move_index, after_move_piece_count).unwrap()
                    } else {
                        debug_assert_eq!(opposite_piece_count, after_move_piece_count);
                        -opposite_wdl_tb.get(after_move_index)
                    };
                    best_wdl = cmp::max(best_wdl, wdl);

                    let dtz = if m.is_capture {
                        1
                    } else {
                        opposite_dtz_tb.get(after_move_index) + 1
                    };
                    max_dtz = cmp::max(max_dtz, dtz);
                    if wdl == WDL::Win {
                        min_dtz = cmp::min(min_dtz, dtz);
                    }

                    if !disable_dtm {
                        let dtm = if m.is_capture {
                            tablebase_map.lookup_dtm_raw(
                                after_move_index, after_move_piece_count).unwrap() + 1
                        } else {
                            opposite_dtm_tb.unwrap().get(after_move_index) + 1
                        };
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

        let computed_wdl = best_wdl;
        let tablebase_wdl = wdl_tb.get(before_move_index);

        let computed_dtz = match computed_wdl {
            WDL::Win => min_dtz,
            WDL::Draw => 0,
            WDL::Loss => max_dtz,
        };
        let tablebase_dtz = dtz_tb.get(before_move_index);

        let computed_dtm = match computed_wdl {
            WDL::Win => min_dtm,
            WDL::Draw => 0,
            WDL::Loss => max_dtm,
        };
        let tablebase_dtm = if disable_dtm { 0 } else { dtm_tb.unwrap().get(before_move_index) };

        if computed_wdl != tablebase_wdl || computed_dtz != tablebase_dtz || (!disable_dtm && computed_dtm != tablebase_dtm) {
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
                if !disable_dtm && computed_dtm != tablebase_dtm {
                    writeln!(debug_message, "    computed_dtm ({computed_dtm}) != tablebase_dtm ({})", tablebase_dtm).unwrap();
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
                            let wdl = if m.is_capture {
                                -tablebase_map.lookup_wdl_raw(
                                    after_move_index, after_move_piece_count).unwrap()
                            } else {
                                -opposite_wdl_tb.get(after_move_index)
                            };
                            let dtz = if m.is_capture {
                                1
                            } else {
                                opposite_dtz_tb.get(after_move_index) + 1
                            };
                            let dtm = if disable_dtm {
                                None
                            } else if m.is_capture {
                                Some(tablebase_map.lookup_dtm_raw(
                                        after_move_index, after_move_piece_count).unwrap() + 1)
                            } else {
                                Some(opposite_dtm_tb.unwrap().get(after_move_index) + 1)
                            };

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
}


pub fn validate_tablebases(tablebase_map: &TablebaseMap, disable_dtm: bool) {
    tablebase_map.by_piece_count.par_iter().for_each(|(&piece_count, tbs_for_piece_count)| {
        let wdl_tb = &tbs_for_piece_count.wdl;
        let dtz_tb = &tbs_for_piece_count.dtz;
        let dtm_tb = if disable_dtm { None } else { tbs_for_piece_count.dtm.as_ref() };
        let opposite_piece_count = normalization::normalize_count(piece_count.players_flipped());
        let opposite_tbs_for_piece_count = tablebase_map.by_piece_count.get(&opposite_piece_count);
        let opposite_wdl_tb = &opposite_tbs_for_piece_count.unwrap().wdl;
        let opposite_dtz_tb = &opposite_tbs_for_piece_count.unwrap().dtz;
        let opposite_dtm_tb = if disable_dtm { None } else { opposite_tbs_for_piece_count.unwrap().dtm.as_ref() };

        validate_tablebase(piece_count, opposite_piece_count, wdl_tb, dtz_tb, dtm_tb,
            opposite_wdl_tb, opposite_dtz_tb, opposite_dtm_tb, tablebase_map);
    });
}
