use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, AtomicU16, Ordering};
use std::cmp;
use std::fs;
use std::io::{self, Write};

use rayon::prelude::*;
use tablebase::{PieceCount, WDL, position_index, normalization, tablebase_name, Tablebase, validation, AtomicBitset};
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

    let mut disk_tablebase = Tablebase::new(&opts.output_dir);
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

fn tablebase_already_generated(piece_count: PieceCount, disk_tablebase: &Tablebase, enable_dtm: bool) -> bool {
    disk_tablebase.lookup_wdl_raw(0, piece_count).is_some() &&
        disk_tablebase.lookup_dtz_raw(0, piece_count).is_some() &&
        (!enable_dtm || disk_tablebase.lookup_dtm_raw(0, piece_count).is_some())
}

// Checking all legal moves is pretty slow so precompute it once for each position
struct PrecomputedMoves {
    // Position indexes which can reach the corner in one move (or capture for immediate win)
    win_in_one: Vec<usize>,
    captures: Vec<Vec<(PieceCount, u64)>>,
    // NOTE - non-capturing moves. The index is into the opposite tablebase
    moves: Vec<Vec<usize>>,
    // From the given position, which positions (in the opposite tablebase) can reach this position by moving
    backward_moves: Vec<Vec<usize>>,
}
fn precompute_moves_single(piece_count: PieceCount, opposite_piece_count: PieceCount) -> PrecomputedMoves {
    let len = position_index::index_count(piece_count) as usize;
    let mut captures: Vec<Vec<(PieceCount, u64)>> = vec![Vec::new(); len];
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
                        &position.board, undo_normalize.piece_count);
                    if m.is_capture {
                        captures_inner.push((undo_normalize.piece_count, after_move_index));
                    } else {
                        debug_assert_eq!(opposite_piece_count, undo_normalize.piece_count);
                        moves_inner.push(after_move_index as usize);
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

fn compute_wdl_and_dtz(forward_tb: &mut MemTb, inverse_tb: &mut MemTb, disk_tablebase: &Tablebase,
    forward_precomputed_moves: &PrecomputedMoves, inverse_precomputed_moves: &PrecomputedMoves) {

    let forward_queue = AtomicBitset::new(forward_tb.wdl.len());
    let inverse_queue = AtomicBitset::new(inverse_tb.wdl.len());

    [false, true].into_par_iter().with_max_len(1).for_each(|is_inverse| {
        let (tb, precomputed_moves, opposite_queue) = if is_inverse {
            (&*inverse_tb, &*inverse_precomputed_moves, &forward_queue)
        } else {
            (&*forward_tb, &*forward_precomputed_moves, &inverse_queue)
        };

        rayon::scope(|scope| {
            scope.spawn(|_| {
                precomputed_moves.win_in_one.par_iter().with_min_len(65536).for_each(|&before_move_index| {
                    tb.wdl.set(before_move_index, WDL::Win);
                    tb.dtz[before_move_index].store(1, Ordering::Release);
                    for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                        opposite_queue.insert(after_backward_move_index);
                    }
                });
            });

            (0..tb.wdl.len()).into_par_iter().with_min_len(65536).for_each(|before_move_index| {
                for &(after_capture_piece_count, after_capture_index) in &precomputed_moves.captures[before_move_index] {
                    if disk_tablebase.lookup_wdl_raw(after_capture_index, after_capture_piece_count).unwrap() == WDL::Loss {
                        tb.wdl.set(before_move_index, WDL::Win);
                        tb.dtz[before_move_index].store(1, Ordering::Release);
                        for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                            opposite_queue.insert(after_backward_move_index);
                        }
                    }
                }
            });
        });
    });

    let mut made_progress = true;
    let mut ply_threshold = 1;
    while made_progress {
        ply_threshold += 1;
        made_progress = [false, true].into_par_iter().with_max_len(1).map(|is_inverse| {
            let (tb, precomputed_moves, opposite_tb, queue, opposite_queue) = if is_inverse {
                (&*inverse_tb, &*inverse_precomputed_moves, &*forward_tb, &inverse_queue, &forward_queue)
            } else {
                (&*forward_tb, &*forward_precomputed_moves, &*inverse_tb, &forward_queue, &inverse_queue)
            };
            queue.par_drain().map(|before_move_index| {
                if tb.dtz[before_move_index].load(Ordering::Acquire) != 0 {
                    let wdl = tb.wdl.get_option(before_move_index);
                    debug_assert!(wdl == Some(WDL::Win) || wdl == Some(WDL::Loss));
                    return false; // Already calulated
                }

                let mut best_wdl = Some(WDL::Loss);
                let mut min_dtz = u8::MAX;
                let mut max_dtz = 0;
                for &after_move_index in &precomputed_moves.moves[before_move_index] {
                    let dtz = opposite_tb.dtz[after_move_index].load(Ordering::Acquire) + 1;
                    let wdl = opposite_tb.wdl.get_option(after_move_index).map(|wdl| -wdl);
                    best_wdl = match (best_wdl, wdl) {
                        (_, Some(WDL::Win)) => Some(WDL::Win),
                        (Some(WDL::Win), None) => Some(WDL::Win),
                        (_, None) => None,
                        (None, _) => None,
                        (Some(a), Some(b)) => Some(cmp::max(a, b)),
                    };
                    max_dtz = cmp::max(max_dtz, dtz);
                    if wdl == Some(WDL::Win) {
                        if dtz != 1 {
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
                }
                if let Some(wdl) = best_wdl {
                    let dtz = if wdl == WDL::Win { min_dtz } else { max_dtz };
                    if dtz == ply_threshold {
                        tb.wdl.set(before_move_index, wdl);
                        tb.dtz[before_move_index].store(dtz, Ordering::Release);
                        for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                            opposite_queue.insert(after_backward_move_index);
                        }
                    } else {
                        // This can happen because of concurrent processing racing one section of
                        // the TB ahead. We'll try again next iteration and succeed.
                        assert!(dtz > ply_threshold);
                        queue.insert(before_move_index);
                    }
                    true
                } else {
                    false
                }
            }).reduce(|| false, |a, b| a || b)
        }).reduce(|| false, |a, b| a || b);
    }

    // Anything that can't be forced into a win/loss is a draw
    [&*forward_tb, &*inverse_tb].par_iter().with_max_len(1).for_each(|tb| {
        (0..tb.wdl.len()).into_par_iter().with_min_len(65536).for_each(|index| {
            if tb.wdl.get_option(index).is_none() {
                tb.wdl.set(index, WDL::Draw);
            }
        });
    });

    [&*forward_tb, &*inverse_tb].par_iter().with_max_len(1).for_each(|tb| {
        (0..tb.wdl.len()).into_par_iter().with_min_len(65536).for_each(|index| {
            if tb.dtz[index].load(Ordering::Relaxed) > 200 {
                // This would be downgraded to a draw, but in practice I don't think it can
                // actually happen
                assert!(false, "greater than 200 plies dtz reached");
            }
        });
    });
}

// Computing DTM has to re-visit nodes and is slower
fn compute_dtm(forward_tb: &MemTb, inverse_tb: &MemTb, disk_tablebase: &Tablebase,
    forward_precomputed_moves: &PrecomputedMoves, inverse_precomputed_moves: &PrecomputedMoves) {

    let forward_queue = AtomicBitset::new(forward_tb.wdl.len());
    let inverse_queue = AtomicBitset::new(inverse_tb.wdl.len());

    [false, true].into_par_iter().with_max_len(1).for_each(|is_inverse| {
        let (tb, precomputed_moves, opposite_queue, queue) = if is_inverse {
            (&*inverse_tb, &*inverse_precomputed_moves, &forward_queue, &inverse_queue)
        } else {
            (&*forward_tb, &*forward_precomputed_moves, &inverse_queue, &forward_queue)
        };

        rayon::scope(|scope| {
            scope.spawn(|_| {
                precomputed_moves.win_in_one.par_iter().with_min_len(65536).for_each(|&before_move_index| {
                    tb.dtm[before_move_index].store(1, Ordering::Relaxed);
                    for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                        opposite_queue.insert(after_backward_move_index);
                    }
                });
            });

            (0..tb.wdl.len()).into_par_iter().with_min_len(65536).for_each(|before_move_index| {
                for &(after_capture_piece_count, after_capture_index) in &precomputed_moves.captures[before_move_index] {
                    if disk_tablebase.lookup_wdl_raw(after_capture_index, after_capture_piece_count).unwrap() == WDL::Loss {
                        let dtm = disk_tablebase.lookup_dtm_raw(after_capture_index, after_capture_piece_count).unwrap() + 1;
                        debug_assert_ne!(dtm, 1);
                        // Must sit in the queue until dtm == ply_threshold
                        queue.insert(before_move_index);
                    }
                }
            });
        });
    });

    let mut made_progress = true;
    let mut ply_threshold = 1;
    while made_progress {
        ply_threshold += 1;
        made_progress = [false, true].into_par_iter().with_max_len(1).map(|is_inverse| {
            let (tb, precomputed_moves, opposite_tb, queue, opposite_queue) = if is_inverse {
                (&*inverse_tb, &*inverse_precomputed_moves, &*forward_tb, &inverse_queue, &forward_queue)
            } else {
                (&*forward_tb, &*forward_precomputed_moves, &*inverse_tb, &forward_queue, &inverse_queue)
            };
            queue.par_drain().map(|before_move_index| {
                if tb.dtm[before_move_index].load(Ordering::Relaxed) != 0 ||
                    tb.wdl.get(before_move_index) == WDL::Draw {
                    return false;
                }

                let mut min_dtm = u16::MAX;
                let mut max_dtm = 0;
                for &after_move_index in &precomputed_moves.moves[before_move_index] {
                    let wdl = -opposite_tb.wdl.get(after_move_index);
                    let dtm = opposite_tb.dtm[after_move_index].load(Ordering::Relaxed) + 1;
                    max_dtm = cmp::max(max_dtm, if dtm == 1 { u16::MAX } else { dtm });
                    if wdl == WDL::Win {
                        // If the dtm of the child node is unset, treat it as high
                        if dtm != 1 {
                            min_dtm = cmp::min(min_dtm, dtm);
                        }
                    }
                }
                for &(after_capture_piece_count, after_capture_index) in &precomputed_moves.captures[before_move_index] {
                    let wdl = -disk_tablebase.lookup_wdl_raw(after_capture_index, after_capture_piece_count).unwrap();
                    let dtm = disk_tablebase.lookup_dtm_raw(after_capture_index, after_capture_piece_count).unwrap() + 1;
                    debug_assert_ne!(dtm, 1); // Because it would be a win_in_one
                    max_dtm = cmp::max(max_dtm, dtm);
                    if wdl == WDL::Win {
                        min_dtm = cmp::min(min_dtm, dtm);
                    }
                }

                let dtm = if tb.wdl.get(before_move_index) == WDL::Win { min_dtm } else { max_dtm };
                if dtm == ply_threshold {
                    tb.dtm[before_move_index].store(dtm, Ordering::Relaxed);
                    for &after_backward_move_index in &precomputed_moves.backward_moves[before_move_index] {
                        opposite_queue.insert(after_backward_move_index);
                    }
                } else {
                    // This can happen because of concurrent processing racing one section of
                    // the TB ahead, or because we keep requeuing winning captures until the
                    // threshold is reached. We'll try again next iteration.
                    assert!(dtm > ply_threshold);
                    queue.insert(before_move_index);
                }
                true
            }).reduce(|| false, |a, b| a || b)
        }).reduce(|| false, |a, b| a || b);
    }
}


fn gen_tablebase(forward_piece_count: PieceCount, disk_tablebase: &Tablebase, enable_dtm: bool) -> (MemTb, MemTb) {
    debug_assert!(normalization::is_count_normalized(forward_piece_count));
    let inverse_piece_count = normalization::normalize_count(forward_piece_count.players_flipped());

    let forward_len = position_index::index_count(forward_piece_count) as usize;
    let inverse_len = position_index::index_count(inverse_piece_count) as usize;
    debug_assert_eq!(forward_len, inverse_len);

    let [forward_precomputed_moves, inverse_precomputed_moves] =
        precompute_moves(forward_piece_count, inverse_piece_count);

    let mut forward_tb = MemTb::new(forward_len, enable_dtm);
    let mut inverse_tb = MemTb::new(inverse_len, enable_dtm);

    compute_wdl_and_dtz(&mut forward_tb, &mut inverse_tb, disk_tablebase,
        &forward_precomputed_moves, &inverse_precomputed_moves);
    if enable_dtm {
        compute_dtm(&mut forward_tb, &mut inverse_tb, disk_tablebase,
            &forward_precomputed_moves, &inverse_precomputed_moves);
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

