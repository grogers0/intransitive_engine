use std::cmp;

use rayon::prelude::*;

use crate::{WDL, normalization, position_index, PieceCount, Tablebase};
use base::{Outcome, Player, Position};

pub fn validate_tablebase(tablebase: &Tablebase, disable_dtm: bool) {
    tablebase.get_covered_piece_counts().into_par_iter().for_each(|piece_count| {
        assert!(normalization::is_count_normalized(piece_count));
        (0..position_index::index_count(piece_count)).into_par_iter().for_each(|before_move_index| {
            let mut position = Position::from_board_and_player(
                position_index::index_to_board(before_move_index, piece_count), Player::Blue);
            assert!(position.get_outcome().is_none());

            let expected_wdl = tablebase.lookup_wdl_raw(before_move_index, piece_count).unwrap();
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
                        assert_eq!(expected_wdl, WDL::Win);
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
                        let wdl = -tablebase.lookup_wdl_raw(
                            after_move_index, after_move_piece_count).unwrap();
                        best_wdl = cmp::max(best_wdl, wdl);

                        let dtz = if m.is_capture {
                            1
                        } else {
                            tablebase.lookup_dtz_raw(
                                after_move_index, after_move_piece_count).unwrap() + 1
                        };
                        max_dtz = cmp::max(max_dtz, dtz);
                        if wdl == WDL::Win {
                            min_dtz = cmp::min(min_dtz, dtz);
                        }

                        let dtm_opt = tablebase.lookup_dtm_raw(
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

            assert_eq!(best_wdl, expected_wdl, "{}", position.board.to_fen());
            let expected_dtz = match expected_wdl {
                WDL::Win => min_dtz,
                WDL::Draw => 0,
                WDL::Loss => max_dtz,
            };
            let actual_dtz = tablebase.lookup_dtz_raw(before_move_index, piece_count).unwrap();
            assert_eq!(expected_dtz, actual_dtz, "{}", position.board.to_fen());
            if !dtm_disabled {
                let expected_dtm = match expected_wdl {
                    WDL::Win => min_dtm,
                    WDL::Draw => 0,
                    WDL::Loss => max_dtm,
                };
                let actual_dtm = tablebase.lookup_dtm_raw(before_move_index, piece_count).unwrap();
                assert_eq!(expected_dtm, actual_dtm, "{}", position.board.to_fen());
            }
        });
    });
}
