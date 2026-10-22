use std::collections::HashMap;
use std::fs;
use std::io;

use tablebase::{PieceCount, WDLZ, position_index, normalization};
use base::{AdjacentLocs, Move, Cell, Outcome, Player, Position};

struct Opts {
    upto_pieces: u8,
    filename: String,
    format: Format,
}

fn tablebase_name(pc: PieceCount) -> String {
    let cap = pc.blue_rock + pc.blue_paper + pc.blue_scissors +
        pc.red_rock + pc.red_paper + pc.red_scissors;
    let mut s = String::with_capacity(cap as usize);
    for _ in 0..pc.blue_rock     { s.push('R'); }
    for _ in 0..pc.blue_paper    { s.push('P'); }
    for _ in 0..pc.blue_scissors { s.push('S'); }
    for _ in 0..pc.red_rock      { s.push('r'); }
    for _ in 0..pc.red_paper     { s.push('p'); }
    for _ in 0..pc.red_scissors  { s.push('s'); }
    s
}

type MemTb = Vec<WDLZ>;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum Format {
    Binary, Text,
}


fn main() {
    let opts = parse_command_line_args();
    let mut tablebases = HashMap::new();

    for total_count in 0..=opts.upto_pieces {
        for piece_count in PieceCount::enumerate_useful_piece_counts(total_count) {
            let piece_count = normalization::normalize_count(piece_count);
            if tablebases.contains_key(&piece_count) { continue; }
            let (tb1, tb2) = gen_tablebase(piece_count, &tablebases);
            tablebases.insert(piece_count, tb1);
            let inverse_piece_count = normalization::normalize_count(piece_count.players_flipped());
            tablebases.insert(inverse_piece_count, tb2);
        }
    }

    check_all_tablebases(&tablebases);

    /*
    for (&piece_count, tablebase) in &tablebases {
        println!("----------\n{}", tablebase_name(piece_count));
        for index in 0..tablebase.len() {
            let board = position_index::index_to_board(index as u64, piece_count);
            if tablebase[index] != WDLZ::invalid() {
                println!("{} {}", board.to_fen(), tablebase[index]);
            }
        }
    }
    */

    println!("{} bytes", tablebases.iter().map(|(_, tablebase)| tablebase.len()).sum::<usize>());
}

fn check_all_tablebases(tablebases: &HashMap<PieceCount, MemTb>) {
    if cfg!(debug_assertions) {
        for (&piece_count, tablebase) in tablebases {
            assert!(normalization::is_count_normalized(piece_count));
            for index in 0..tablebase.len() {
                let stored = tablebase[index];
                assert!(stored.is_valid());
                let board = position_index::index_to_board(index as u64, piece_count);
                let mut position = Position::from_board_and_player(board, Player::Blue);
                assert!(position.get_outcome().is_none());
                let mut best = WDLZ::loss();
                let mut best_move = Move::invalid();
                for m in position.legal_moves().collect::<Vec<_>>() {
                    let undo = position.make_move(m);
                    match position.get_outcome() {
                        Some(Outcome::Win(Player::Blue)) => {
                            best = WDLZ::win_in(1);
                            best_move = m;
                        },
                        Some(Outcome::Win(Player::Red)) => unreachable!(),
                        Some(Outcome::Draw) => unreachable!(),
                        None => {
                            let res = normalization::normalize_position(&mut position);
                            let index2 = position_index::board_to_index(&position.board,res.piece_count) as usize;
                            let v = -tablebases[&res.piece_count][index2].add_ply();
                            if v > best {
                                best = v;
                                best_move = m;
                            }
                            normalization::undo_normalize_position(&mut position, res);
                        }
                    }

                    position.undo_move(undo);
                }
                assert_eq!(best, stored, "board: {}, best_move: {best_move}, best: {best} != stored in tablebase: {stored}", position.board.to_fen());
            }
        }
    }
}

fn gen_tablebase(base_piece_count: PieceCount, tablebases: &HashMap<PieceCount, MemTb>) -> (MemTb, MemTb) {
    debug_assert!(normalization::is_count_normalized(base_piece_count));
    let mut base_tablebase = vec![WDLZ::invalid(); position_index::index_count(base_piece_count) as usize];
    let inverse_piece_count = normalization::normalize_count(base_piece_count.players_flipped());
    let mut inverse_tablebase = vec![WDLZ::invalid(); position_index::index_count(inverse_piece_count) as usize];
    debug_assert_eq!(base_tablebase.len(), inverse_tablebase.len());

    let mut prev_queue = Vec::new();
    let mut next_queue = Vec::new();

    for is_inverse in [false, true] {
        let (tablebase, piece_count, opposite_tablebase) = if is_inverse {
            (&mut inverse_tablebase, inverse_piece_count, &mut base_tablebase)
        } else {
            (&mut base_tablebase, base_piece_count, &mut inverse_tablebase)
        };
        // TODO - rather than enumerating all positions, we can probably smartly enumerate only
        // those with wins in 1 ply
        for before_move_index in 0..tablebase.len() {
            let mut position = Position::from_board_and_player(
                position_index::index_to_board(before_move_index as u64, piece_count), Player::Blue);
            let mut best = WDLZ::loss();
            for m in position.legal_moves().collect::<Vec<_>>() {
                let undo_move = position.make_move(m);
                let score = match position.get_outcome() {
                    Some(Outcome::Win(Player::Blue)) => WDLZ::win_in(1),
                    Some(Outcome::Win(Player::Red)) => unreachable!(),
                    Some(Outcome::Draw) => unreachable!(),
                    None => {
                        let undo_normalize = normalization::normalize_position(&mut position);
                        let after_move_index = position_index::board_to_index(
                            &position.board, undo_normalize.piece_count) as usize;
                        let mut score = if m.is_capture {
                            tablebases[&undo_normalize.piece_count][after_move_index]
                        } else {
                            opposite_tablebase[after_move_index]
                        };
                        if score.is_valid() {
                            score = -score.add_ply();
                        }
                        normalization::undo_normalize_position(&mut position, undo_normalize);
                        score
                    },
                };
                if score.is_valid() {
                    if score.is_win() || (best.is_valid() && score > best) {
                        best = score;
                    }
                } else if best.is_valid() && !best.is_win() {
                    best = WDLZ::invalid();
                }

                position.undo_move(undo_move);
            }
            if best.is_valid() {
                tablebase[before_move_index] = best;
                next_queue.push((before_move_index, is_inverse));
            }
        }
    }

    while !next_queue.is_empty() {
        prev_queue.clear();
        std::mem::swap(&mut next_queue, &mut prev_queue);
        for &(target_after_move_index, is_inverse) in &prev_queue {
            let (tablebase, piece_count, opposite_tablebase, opposite_piece_count) = if !is_inverse {
                (&mut base_tablebase, base_piece_count, &mut inverse_tablebase, inverse_piece_count)
            } else {
                (&mut inverse_tablebase, inverse_piece_count, &mut base_tablebase, base_piece_count)
            };
            let mut board = position_index::index_to_board(target_after_move_index as u64, piece_count);
            for from_y in 0..9 {
                for from_x in 0..9 {
                    if let Cell::Occupied(Player::Red, from_piece) = board.get(from_x, from_y) {
                        for (to_x, to_y) in AdjacentLocs::new(from_x, from_y) {
                            if board.get(to_x, to_y) != Cell::Empty { continue; }
                            if (to_x, to_y) == Position::corner_goal(Player::Red) { continue; }
                            *board.get_mut(from_x, from_y) = Cell::Empty;
                            *board.get_mut(to_x, to_y) = Cell::Occupied(Player::Red, from_piece);
                            board.invert_perspective();
                            let undo_normalize = normalization::normalize_board(
                                &mut board, piece_count.players_flipped());

                            let before_move_index =
                                position_index::board_to_index(&board, opposite_piece_count) as usize;
                            if !opposite_tablebase[before_move_index].is_valid() {
                                let mut position = Position::from_board_and_player(board.clone(), Player::Blue);
                                let mut best = WDLZ::loss();
                                for m in position.legal_moves().collect::<Vec<_>>() {
                                    let undo_move = position.make_move(m);
                                    let score = match position.get_outcome() {
                                        Some(Outcome::Win(Player::Blue)) => WDLZ::win_in(1),
                                        Some(Outcome::Win(Player::Red)) => unreachable!(),
                                        Some(Outcome::Draw) => unreachable!(),
                                        None => {
                                            let undo_normalize = normalization::normalize_position(&mut position);
                                            let after_move_index = position_index::board_to_index(
                                                &position.board, undo_normalize.piece_count) as usize;
                                            let mut score = if m.is_capture {
                                                tablebases[&undo_normalize.piece_count][after_move_index]
                                            } else {
                                                tablebase[after_move_index]
                                            };
                                            if score.is_valid() {
                                                score = -score.add_ply();
                                            }
                                            normalization::undo_normalize_position(&mut position, undo_normalize);
                                            score
                                        },
                                    };
                                    if score.is_valid() {
                                        if score.is_win() || (best.is_valid() && score > best) {
                                            best = score;
                                        }
                                    } else if best.is_valid() && !best.is_win() {
                                        best = WDLZ::invalid();
                                    }

                                    position.undo_move(undo_move);
                                }
                                if best.is_valid() {
                                    opposite_tablebase[before_move_index] = best;
                                    next_queue.push((before_move_index, !is_inverse));
                                }
                            }

                            normalization::undo_normalize_board(&mut board, undo_normalize);
                            board.invert_perspective();
                            *board.get_mut(to_x, to_y) = Cell::Empty;
                            *board.get_mut(from_x, from_y) = Cell::Occupied(Player::Red, from_piece);
                        }
                    }
                }
            }
        }
    }

    // If we can't force a win or loss in any remaining position, it's a draw
    for tablebase in [&mut base_tablebase, &mut inverse_tablebase] {
        for index in 0..tablebase.len() {
            if !tablebase[index].is_valid() {
                tablebase[index] = WDLZ::draw();
            }
        }
    }

    (base_tablebase, inverse_tablebase)
}


fn output(tablebase: &HashMap<Position, WDLZ>, opts: &Opts) {
    assert!(opts.format == Format::Text); // Not implemented
    let mut writer: Box<dyn io::Write> = match opts.filename.as_str() {
        "-" => Box::new(io::stdout()),
        name => Box::new(fs::File::create(name).unwrap()),
    };
    output_text(tablebase, &mut *writer);
}

fn output_text(tablebase: &HashMap<Position, WDLZ>, writer: &mut dyn io::Write) {
    for (pos, wldz) in tablebase {
        let player_str = match pos.active_player {
            Player::Blue => "b",
            Player::Red => "r",
        };
        write!(*writer, "{} {player_str} {wldz}\n", pos.board.to_fen()).unwrap();
    }
}

fn output_binary(tablebase: &HashMap<Position, WDLZ>, writer: &mut dyn io::Write) {
    panic!();
}

fn parse_command_line_args() -> Opts {
    let mut opts = Opts {
        upto_pieces: 0,
        filename: "-".to_string(),
        format: Format::Text,
    };

    let mut args = std::env::args();
    args.next(); // Skip binary name
    while let Some(token) = args.next() {
        match token.as_str() {
            "--upto_pieces" | "--upto" => opts.upto_pieces = args.next().unwrap().parse().unwrap(),
            "--filename" => opts.filename = args.next().unwrap().to_string(),
            "--format" => opts.format = match args.next().unwrap().as_str() {
                "binary" | "bin" => Format::Binary,
                "text" | "fen" | "txt" => Format::Text,
                f => panic!("unknown output format: {}", f),
            },
            _ => {
                panic!("unknown command line argument \"{}\"", token);
            },
        };
    }

    if opts.upto_pieces <= 1 {
        panic!("must specify number of pieces greater than 1");
    }
    opts
}

