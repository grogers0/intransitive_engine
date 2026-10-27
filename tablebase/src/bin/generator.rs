use std::collections::{HashMap, VecDeque};
use std::cmp;
use std::fs;
use std::io::{self, Write};

use rayon::prelude::*;
use tablebase::{PieceCount, WDL, position_index, normalization, tablebase_name};
use base::{Outcome, Player, Position};

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
#[derive(Clone)]
struct MemTb {
    wdl: WdlVec,
    dtz: Vec<u8>,
    dtm: Vec<u16>,
}

impl MemTb {
    fn new(len: usize, enable_dtm: bool) -> MemTb {
        let dtm = if enable_dtm { vec![0; len] } else { Vec::new() };
        MemTb {
            wdl: WdlVec::new(len),
            dtz: vec![0; len],
            dtm,
        }
    }
}

#[derive(Clone)]
struct WdlVec {
    len: usize,
    bytes: Box<[u8]>,
}

impl WdlVec {
    fn new(len: usize) -> WdlVec {
        let bytes = vec![0xff; (len + 3) / 4].into_boxed_slice();
        WdlVec { len, bytes }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn get_option(&self, idx: usize) -> Option<WDL> {
        debug_assert!(idx < self.len);
        let byte_idx = idx >> 2;
        let bit_idx = (idx & 0x03) << 1;
        match (self.bytes[byte_idx] >> bit_idx) & 0x03 {
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

    fn set(&mut self, idx: usize, wdl: WDL) {
        debug_assert!(idx < self.len);
        let byte_idx = idx >> 2;
        let bit_idx = (idx & 0x03) << 1;
        let mut byte = self.bytes[byte_idx];
        byte &= !(0x03 << bit_idx);
        byte |= match wdl { WDL::Loss => 0, WDL::Draw => 1, WDL::Win => 2 } << bit_idx;
        // Shouldn't overwrite unless it's unset (setting to same value is ok)
        debug_assert!(self.bytes[byte_idx] == byte ||
            (self.bytes[byte_idx] >> bit_idx) & 0x03 == 0x03);
        self.bytes[byte_idx] = byte;
    }
}

fn main() {
    let opts = parse_command_line_args();
    init_threads(&opts);

    let mut tablebases = HashMap::new();

    for total_count in 0..=opts.upto_pieces {
        for piece_count in PieceCount::enumerate_useful_piece_counts(total_count) {
            let piece_count = normalization::normalize_count(piece_count);
            if tablebases.contains_key(&piece_count) { continue; }
            let (tb1, tb2) = gen_tablebase(piece_count, &tablebases, opts.enable_dtm);
            tablebases.insert(piece_count, tb1);
            let inverse_piece_count = normalization::normalize_count(piece_count.players_flipped());
            tablebases.insert(inverse_piece_count, tb2);
        }
    }

    check_all_tablebases(&tablebases, opts.enable_dtm);

    output(&tablebases, &opts);

    println!("{} unique indexes", tablebases.iter().map(|(_, tablebase)| tablebase.wdl.len()).sum::<usize>());
}

fn check_all_tablebases(tablebases: &HashMap<PieceCount, MemTb>, enable_dtm: bool) {
    if cfg!(debug_assertions) {
        tablebases.par_iter().for_each(|(&piece_count, tablebase)| {
            assert!(normalization::is_count_normalized(piece_count));
            (0..tablebase.wdl.len()).into_par_iter().for_each(|before_move_index| {
                let mut position = Position::from_board_and_player(
                    position_index::index_to_board(before_move_index as u64, piece_count), Player::Blue);
                assert!(position.get_outcome().is_none());

                let expected_wdl = tablebase.wdl.get(before_move_index);
                let mut best_wdl = WDL::Loss;
                let mut min_dtz = u8::MAX;
                let mut max_dtz = 0;
                let mut min_dtm = u16::MAX;
                let mut max_dtm = 0;
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
                                &position.board, after_move_piece_count) as usize;
                            let wdl = -tablebases[&after_move_piece_count].wdl.get(after_move_index);
                            best_wdl = cmp::max(best_wdl, wdl);

                            if wdl == expected_wdl {
                                let dtz = if m.is_capture {
                                    1
                                } else {
                                    tablebases[&after_move_piece_count].dtz[after_move_index] + 1
                                };
                                min_dtz = cmp::min(min_dtz, dtz);
                                max_dtz = cmp::max(max_dtz, dtz);

                                if enable_dtm {
                                    let dtm = tablebases[&after_move_piece_count]
                                        .dtm[after_move_index] + 1;
                                    min_dtm = cmp::min(min_dtm, dtm);
                                    max_dtm = cmp::max(max_dtm, dtm);
                                }
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
                assert_eq!(expected_dtz, tablebase.dtz[before_move_index], "{}", position.board.to_fen());
                if enable_dtm {
                    let expected_dtm = match expected_wdl {
                        WDL::Win => min_dtm,
                        WDL::Draw => 0,
                        WDL::Loss => max_dtm,
                    };
                    assert_eq!(expected_dtm, tablebase.dtm[before_move_index], "{}", position.board.to_fen());
                }
            });
        });
    }
}

// Checking all legal moves is pretty slow so precompute it once for each position
struct PrecomputedMoves {
    // Position indexes which can reach the corner in one move (or capture for immediate win)
    win_in_one: Vec<usize>,
    captures: Vec<Vec<(PieceCount, usize)>>,
    // NOTE - non-capturing moves. The index is into the opposite tablebase
    moves: Vec<Vec<usize>>,
    // From the given position, which positions (in the opposite tablebase) can reach this position by moving
    backward_moves: Vec<Vec<usize>>,
}
fn precompute_moves_single(piece_count: PieceCount, opposite_piece_count: PieceCount) -> PrecomputedMoves {
    let len = position_index::index_count(piece_count) as usize;
    let mut captures: Vec<Vec<(PieceCount, usize)>> = vec![Vec::new(); len];
    let mut moves: Vec<Vec<usize>> = vec![Vec::new(); len];

    let mut win_in_one = (0..len).into_par_iter()
        .zip(captures.par_iter_mut())
        .zip(moves.par_iter_mut())
        .filter_map(|((before_move_index, captures_inner), moves_inner)| {
        let mut can_win_in_one = false;
        let mut position = Position::from_board_and_player(
            position_index::index_to_board(before_move_index as u64, piece_count), Player::Blue);
        for m in position.legal_moves().collect::<Vec<_>>() {
            let undo_move = position.make_move(m);
            match position.get_outcome() {
                Some(Outcome::Win(Player::Blue)) => can_win_in_one = true,
                Some(Outcome::Win(Player::Red)) => unreachable!(),
                Some(Outcome::Draw) => unreachable!(),
                None => {
                    let pre_normalize_piece_count = if m.is_capture {
                        PieceCount::from_board(&position.board)
                    } else {
                        piece_count
                    };
                    let undo_normalize = normalization::normalize_position(&mut position, pre_normalize_piece_count);
                    let after_move_index = position_index::board_to_index(
                        &position.board, undo_normalize.piece_count) as usize;
                    if m.is_capture {
                        captures_inner.push((undo_normalize.piece_count, after_move_index));
                    } else {
                        debug_assert_eq!(opposite_piece_count, undo_normalize.piece_count);
                        moves_inner.push(after_move_index);
                    }

                    normalization::undo_normalize_position(&mut position, undo_normalize);
                },
            };

            position.undo_move(undo_move);
        }
        if can_win_in_one { Some(before_move_index) } else { None }
    })
    .fold(|| Vec::new(), |mut acc, before_move_index| {
        acc.push(before_move_index);
        acc
    })
    .reduce(|| Vec::new(), |mut a, mut b| {
        if a.len() > b.len() {
            a.extend(b);
            a
        } else {
            b.extend(a);
            b
        }
    });

    // To allow binary search during DTM calculation
    win_in_one.sort_unstable();

    PrecomputedMoves { win_in_one, captures, moves, backward_moves: Vec::new() }
}
fn compute_backward_moves(precomputed_moves: &mut PrecomputedMoves, opposite_precomputed_moves: &PrecomputedMoves) {
    precomputed_moves.backward_moves = vec![Vec::new(); precomputed_moves.moves.len()];
    for before_move_index in 0..opposite_precomputed_moves.moves.len() {
        for &after_move_index in &opposite_precomputed_moves.moves[before_move_index] {
            precomputed_moves.backward_moves[after_move_index].push(before_move_index);
        }
    }

    // TODO - is it necessary? Seems like it may speed up subsequent iterations
    for after_move_index in 0..precomputed_moves.backward_moves.len() {
        precomputed_moves.backward_moves[after_move_index].sort();
    }
}
fn precompute_moves(forward_piece_count: PieceCount, inverse_piece_count: PieceCount) -> [PrecomputedMoves; 2] {
    let mut forward_precomputed_moves = precompute_moves_single(forward_piece_count, inverse_piece_count);
    let mut inverse_precomputed_moves = precompute_moves_single(inverse_piece_count, forward_piece_count);
    compute_backward_moves(&mut forward_precomputed_moves, &inverse_precomputed_moves);
    compute_backward_moves(&mut inverse_precomputed_moves, &forward_precomputed_moves);
    [forward_precomputed_moves, inverse_precomputed_moves]
}


fn compute_wdl(forward_tb: &mut MemTb, inverse_tb: &mut MemTb,
    smaller_tablebases: &HashMap<PieceCount, MemTb>, forward_precomputed_moves: &PrecomputedMoves,
    inverse_precomputed_moves: &PrecomputedMoves) {

    let mut queue: VecDeque<(usize, bool)> = VecDeque::new();

    for is_inverse in [false, true] {
        let (tb, precomputed_moves) = if is_inverse {
            (&mut *inverse_tb, inverse_precomputed_moves)
        } else {
            (&mut *forward_tb, forward_precomputed_moves)
        };

        for &before_move_index in &precomputed_moves.win_in_one {
            tb.wdl.set(before_move_index, WDL::Win);
            for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                queue.push_back((after_backward_move_index, !is_inverse));
            }
        }

        for before_move_index in 0..tb.wdl.len() {
            for &(after_capture_piece_count, after_capture_index) in &precomputed_moves.captures[before_move_index] {
                if smaller_tablebases[&after_capture_piece_count].wdl.get(after_capture_index) == WDL::Loss {
                    tb.wdl.set(before_move_index, WDL::Win);
                    for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                        queue.push_back((after_backward_move_index, !is_inverse));
                    }
                }
            }
        }
    }

    while let Some((before_move_index, is_inverse)) = queue.pop_front() {
        let (tb, precomputed_moves, opposite_tb) = if is_inverse {
            (&mut *inverse_tb, inverse_precomputed_moves, &*forward_tb)
        } else {
            (&mut *forward_tb, forward_precomputed_moves, &*inverse_tb)
        };
        if tb.wdl.get_option(before_move_index).is_some() {
            continue;
        }

        let mut best = Some(WDL::Loss);
        for &after_move_index in &precomputed_moves.moves[before_move_index] {
            let wdl = opposite_tb.wdl.get_option(after_move_index).map(|wdl| -wdl);
            best = match (best, wdl) {
                (_, Some(WDL::Win)) => Some(WDL::Win),
                (Some(WDL::Win), None) => Some(WDL::Win),
                (_, None) => None,
                (None, _) => None,
                (Some(a), Some(b)) => Some(cmp::max(a, b)),
            };
        }
        if let Some(wdl) = best {
            tb.wdl.set(before_move_index, wdl);
            for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                queue.push_back((after_backward_move_index, !is_inverse));
            }
        }
    }

    // Anything that can't be forced into a win/loss is a draw
    for tb in [&mut *forward_tb, &mut *inverse_tb] {
        for index in 0..tb.wdl.len() {
            if tb.wdl.get_option(index).is_none() {
                tb.wdl.set(index, WDL::Draw);
            }
        }
    }
}

fn compute_dtz(forward_tb: &mut MemTb, inverse_tb: &mut MemTb,
    smaller_tablebases: &HashMap<PieceCount, MemTb>, forward_precomputed_moves: &PrecomputedMoves,
    inverse_precomputed_moves: &PrecomputedMoves) {
    let mut queue: VecDeque<(usize, bool)> = VecDeque::new();

    for is_inverse in [false, true] {
        let (tb, precomputed_moves) = if is_inverse {
            (&mut *inverse_tb, inverse_precomputed_moves)
        } else {
            (&mut *forward_tb, forward_precomputed_moves)
        };

        for &before_move_index in &precomputed_moves.win_in_one {
            tb.dtz[before_move_index] = 1;
            for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                queue.push_back((after_backward_move_index, !is_inverse));
            }
        }

        for before_move_index in 0..tb.wdl.len() {
            for &(after_capture_piece_count, after_capture_index) in &precomputed_moves.captures[before_move_index] {
                if smaller_tablebases[&after_capture_piece_count].wdl.get(after_capture_index) == WDL::Loss {
                    tb.dtz[before_move_index] = 1;
                    for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                        queue.push_back((after_backward_move_index, !is_inverse));
                    }
                }
            }
        }
    }

    while let Some((before_move_index, is_inverse)) = queue.pop_front() {
        let (tb, precomputed_moves, opposite_tb) = if is_inverse {
            (&mut *inverse_tb, inverse_precomputed_moves, &*forward_tb)
        } else {
            (&mut *forward_tb, forward_precomputed_moves, &*inverse_tb)
        };

        if tb.wdl.get(before_move_index) == WDL::Draw {
            continue;
        } else if tb.dtz[before_move_index] != 0 {
            continue; // already set
        }
        let is_win = tb.wdl.get(before_move_index) == WDL::Win;

        const MAX_DTZ: u8 = u8::MAX;
        let mut best = if is_win { MAX_DTZ } else { 1 };
        for &after_move_index in &precomputed_moves.moves[before_move_index] {
            if tb.wdl.get(before_move_index) != -opposite_tb.wdl.get(after_move_index) { continue; }

            let after_move_dtz = opposite_tb.dtz[after_move_index];
            if is_win {
                // minimize the dtz, unknown values are treated as high since we are breadth-first
                if after_move_dtz != 0 {
                    best = cmp::min(best, after_move_dtz + 1);
                }
            } else {
                // maximize the dtz, unknown values poison the calculation
                let after_move_dtz_plus_one = if after_move_dtz == 0 { MAX_DTZ } else { after_move_dtz + 1 };
                best = cmp::max(best, after_move_dtz_plus_one);
            }
        }
        if best != MAX_DTZ {
            tb.dtz[before_move_index] = best;
            for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                queue.push_back((after_backward_move_index, !is_inverse));
            }
        }
    }

    // Anything with dtz >= 200 is a draw (TODO need to check if that's >= or >)
    for &dtz in &forward_tb.dtz {
        assert!(dtz < 200);
    }
    for &dtz in &inverse_tb.dtz {
        assert!(dtz < 200);
    }
}

fn compute_dtm(forward_tb: &mut MemTb, inverse_tb: &mut MemTb,
    smaller_tablebases: &HashMap<PieceCount, MemTb>, forward_precomputed_moves: &PrecomputedMoves,
    inverse_precomputed_moves: &PrecomputedMoves) {
    let mut queue = VecDeque::new();

    for is_inverse in [false, true] {
        let (tb, precomputed_moves) = if is_inverse {
            (&mut *inverse_tb, inverse_precomputed_moves)
        } else {
            (&mut *forward_tb, forward_precomputed_moves)
        };

        for before_move_index in 0..tb.wdl.len() {
            for &(after_capture_piece_count, after_capture_index) in &precomputed_moves.captures[before_move_index] {
                if smaller_tablebases[&after_capture_piece_count].wdl.get(after_capture_index) == WDL::Loss {
                    tb.dtm[before_move_index] = smaller_tablebases[&after_capture_piece_count].dtm[after_capture_index] + 1;
                    for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                        queue.push_back((after_backward_move_index, !is_inverse));
                    }
                }
            }
        }

        // NOTE - process wins in 1 ply after captures to overwrite if needed
        for &before_move_index in &precomputed_moves.win_in_one {
            tb.dtm[before_move_index] = 1;
            for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                queue.push_back((after_backward_move_index, !is_inverse));
            }
        }
    }

    while let Some((before_move_index, is_inverse)) = queue.pop_front() {
        let (tb, precomputed_moves, opposite_tb) = if is_inverse {
            (&mut *inverse_tb, inverse_precomputed_moves, &*forward_tb)
        } else {
            (&mut *forward_tb, forward_precomputed_moves, &*inverse_tb)
        };

        if tb.wdl.get(before_move_index) == WDL::Draw {
            continue;
        }
        let is_win = tb.wdl.get(before_move_index) == WDL::Win;

        const MAX_DTM: u16 = u16::MAX;
        let mut best = if is_win {
            if let Ok(_) = precomputed_moves.win_in_one.binary_search(&before_move_index) {
                1
            } else {
                MAX_DTM
            }
        } else {
            0
        };
        for &after_move_index in &precomputed_moves.moves[before_move_index] {
            if tb.wdl.get(before_move_index) != -opposite_tb.wdl.get(after_move_index) { continue; }

            let after_move_dtm = opposite_tb.dtm[after_move_index];
            if is_win {
                // minimize the dtm, unknown values are treated as high since we are breadth-first
                if after_move_dtm != 0 {
                    best = cmp::min(best, after_move_dtm + 1);
                }
            } else {
                // maximize the dtm, unknown values poison the calculation
                let after_move_dtm_plus_one = if after_move_dtm == 0 { MAX_DTM } else { after_move_dtm + 1 };
                best = cmp::max(best, after_move_dtm_plus_one);
            }
        }
        for &(after_capture_piece_count, after_capture_index) in &precomputed_moves.captures[before_move_index] {
            if tb.wdl.get(before_move_index) !=
                -smaller_tablebases[&after_capture_piece_count].wdl.get(after_capture_index) {
                    continue;
            }
            let after_capture_dtm = smaller_tablebases[&after_capture_piece_count].dtm[after_capture_index] + 1;
            if is_win {
                best = cmp::min(best, after_capture_dtm);
            } else {
                best = cmp::max(best, after_capture_dtm);
            }
        }

        // NOTE - best can be improved after set the first time since the wavefront might explore
        // a capture first which leads to a high DTM but then discover a faster path directly to
        // the goal. TODO - we could probably iteratively increase the limit ply to avoid this
        if best != MAX_DTM && best != tb.dtm[before_move_index] {
            tb.dtm[before_move_index] = best;
            for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                queue.push_back((after_backward_move_index, !is_inverse));
            }
        }
    }
}


fn gen_tablebase(forward_piece_count: PieceCount, smaller_tablebases: &HashMap<PieceCount, MemTb>,
    enable_dtm: bool) -> (MemTb, MemTb) {
    debug_assert!(normalization::is_count_normalized(forward_piece_count));
    let inverse_piece_count = normalization::normalize_count(forward_piece_count.players_flipped());

    let forward_len = position_index::index_count(forward_piece_count) as usize;
    let inverse_len = position_index::index_count(inverse_piece_count) as usize;
    debug_assert_eq!(forward_len, inverse_len);

    let [forward_precomputed_moves, inverse_precomputed_moves] =
        precompute_moves(forward_piece_count, inverse_piece_count);

    let mut forward_tb = MemTb::new(forward_len, enable_dtm);
    let mut inverse_tb = MemTb::new(inverse_len, enable_dtm);

    compute_wdl(&mut forward_tb, &mut inverse_tb, smaller_tablebases,
        &forward_precomputed_moves, &inverse_precomputed_moves);
    compute_dtz(&mut forward_tb, &mut inverse_tb, smaller_tablebases,
        &forward_precomputed_moves, &inverse_precomputed_moves);
    if enable_dtm {
        compute_dtm(&mut forward_tb, &mut inverse_tb, smaller_tablebases,
            &forward_precomputed_moves, &inverse_precomputed_moves);
    }

    (forward_tb, inverse_tb)
}


fn output(tablebases: &HashMap<PieceCount, MemTb>, opts: &Opts) {
    match opts.output_format {
        OutputFormat::Blackhole => (), // Output nothing
        OutputFormat::Binary => {
            for (&piece_count, tablebase) in tablebases {
                output_binary(piece_count, tablebase, opts).unwrap();
            }
        },
        OutputFormat::Text => {
            for (&piece_count, tablebase) in tablebases {
                output_text(piece_count, tablebase, opts).unwrap();
            }
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
            let dtz = tablebase.dtz[index];
            if opts.enable_dtm {
                let dtm = tablebase.dtm[index];
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
    let len = (tablebase.wdl.len() as u64).to_le_bytes();
    {
        let mut file = fs::File::create(
            format!("{}/{}.tbw", opts.output_dir, tablebase_name(piece_count)))?;
        file.write_all(&tablebase.wdl.bytes)?;
        file.write_all(&[0u8])?;
    }
    {
        let mut file = fs::File::create(
            format!("{}/{}.tbz", opts.output_dir, tablebase_name(piece_count)))?;
        file.write_all(tablebase.dtz.as_slice())?;
        file.write_all(&[0u8])?;
    }
    if opts.enable_dtm {
        let mut file = fs::File::create(
            format!("{}/{}.tbm", opts.output_dir, tablebase_name(piece_count)))?;
        // TODO - probably faster way to do this
        for &v in &tablebase.dtm {
            file.write_all(&v.to_le_bytes())?;
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

