use std::collections::HashMap;
use std::fs;
use std::io;

use tablebase::{PieceCount, WDLZ, position_index, normalization};
use base::{AdjacentLocs, Move, Outcome, Player, Piece, Position};
use base::position::{Board, Cell};

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
            let (tb1, tb2) = gen_ply1_tablebase(piece_count, &tablebases);
            // TODO
            tablebases.insert(piece_count, tb1);
            let inverse_piece_count = normalization::normalize_count(piece_count.players_flipped());
            tablebases.insert(inverse_piece_count, tb2);
        }
    }

    println!("{} bytes", tablebases.iter().map(|(pc, tb)| tb.len()).sum::<usize>());

    check_all_tablebases(&tablebases);

    for (piece_count, tablebase) in tablebases {
        println!("----------\n{}", tablebase_name(piece_count));
        for index in 0..tablebase.len() {
            let board = position_index::index_to_board(index as u64, piece_count);
            if tablebase[index] != WDLZ::invalid() {
                println!("{} {}", board.to_fen(), tablebase[index]);
            }
        }
    }

}

fn check_all_tablebases(tablebases: &HashMap<PieceCount, MemTb>) {
    return; // FIXME
    if cfg!(debug_assertions) {
        for (&piece_count, tablebase) in tablebases {
            if piece_count.total() == 1 { continue; }
            assert!(normalization::is_count_normalized(piece_count));
            for index in 0..tablebase.len() {
                let stored = tablebase[index];
                assert!(stored.is_valid());
                let board = position_index::index_to_board(index as u64, piece_count);
                let mut position = Position::from_board_and_player(board, Player::Blue);
                assert!(position.get_outcome().is_none());
                let mut best = WDLZ::loss();
                for m in position.legal_moves().collect::<Vec<_>>() {
                    let undo = position.make_move(m);
                    match position.get_outcome() {
                        Some(Outcome::Win(Player::Blue)) => best = WDLZ::win_in(1),
                        Some(_) => unreachable!(),
                        None => {
                            let res = normalization::normalize(&mut position);
                            let index2 = position_index::board_to_index(&position.board,res.piece_count) as usize;
                            let v = -tablebases[&res.piece_count][index2].add_ply();
                            if v > best { best = v; }
                            normalization::undo_normalize(&mut position, res);
                        }
                    }

                    position.undo_move(undo);
                }
                assert_eq!(best, stored);
            }
        }
    }
}

fn iter_all_boards<F: FnMut(&Board) -> ()>(mut piece_count: PieceCount, board: &mut Board, mut f: F) {
    fn recurse<F: FnMut(&Board) -> ()>(piece_count: &mut PieceCount, prev_xy: Option<(u8, u8)>,
        board: &mut Board, f: &mut F) {
        if *piece_count == PieceCount::ZERO {
            f(board);
            return;
        }
        let min_y = prev_xy.map(|(_, y)| y).unwrap_or(0);
        let min_x = |y: u8| -> u8 {
            if let Some((px, py)) = prev_xy && py == y {
                px + 1
            } else {
                0
            }
        };
        for y in min_y..9 {
            for x in min_x(y)..9 {
                if let Cell::Occupied(_, _) = board.get(x, y) { continue; }
                for player in Player::iter() {
                    if (x, y) == Position::corner_goal(player) { continue; }
                    for piece in Piece::iter() {
                        if piece_count.get(player, piece) == 0 { continue; }
                        *board.get_mut(x, y) = Cell::Occupied(player, piece);
                        *piece_count.get_mut(player, piece) -= 1;
                        let next_xy = if *piece_count.get_mut(player, piece) > 0 { Some((x, y)) } else { None };
                        let mut board2 = board.clone();
                        recurse(piece_count, prev_xy, &mut board2, f);
                        *piece_count.get_mut(player, piece) += 1;
                        *board.get_mut(x, y) = Cell::Empty;
                    }
                }
            }
        }
    }
    recurse(&mut piece_count, None, board, &mut f);
}


fn gen_ply1_tablebase(base_piece_count: PieceCount, tablebases: &HashMap<PieceCount, MemTb>) -> (MemTb, MemTb) {
    debug_assert!(normalization::is_count_normalized(base_piece_count));
    let mut base_tablebase = vec![WDLZ::invalid(); position_index::index_count(base_piece_count) as usize];
    let inverse_piece_count = normalization::normalize_count(base_piece_count.players_flipped());
    let mut inverse_tablebase = vec![WDLZ::invalid(); position_index::index_count(inverse_piece_count) as usize];
    debug_assert_eq!(base_tablebase.len(), inverse_tablebase.len());

    let mut board = Board::new();

    for (tablebase, piece_count, is_inverse) in [(&mut base_tablebase, base_piece_count, false), (&mut inverse_tablebase, inverse_piece_count, true)] {
        // Any position we can move (with or without capture) to the goal in 1 move is a win in 1 ply
        for piece in Piece::iter() {
            if piece_count.get(Player::Blue, piece) == 0 { continue; }
            for (x, y) in [(8, 7), (7, 8)] {
                *board.get_mut(x, y) = Cell::Occupied(Player::Blue, piece);
                let remaining_piece_count = piece_count - PieceCount::single(Player::Blue, piece);
                iter_all_boards(remaining_piece_count, &mut board, |b| {
                    let is_win = match b.get(8, 8) {
                        Cell::Empty => true,
                        Cell::Occupied(Player::Red, piece2) => piece.can_capture(piece2),
                        Cell::Occupied(Player::Blue, _) => unreachable!(),
                    };
                    if is_win {
                        let index = position_index::board_to_index(b, piece_count);
                        tablebase[index as usize] = WDLZ::win_in(1);
                    }
                });
                *board.get_mut(x, y) = Cell::Empty;
            }
        }

        // Any position that we can capture a piece and end up with red in a losing position (when
        // normalized) means we are in a winning position
        for captured_piece in Piece::iter() {
            let capturing_piece = captured_piece.captured_by();
            if piece_count.get(Player::Red, captured_piece) == 0 { continue; }
            if piece_count.get(Player::Blue, capturing_piece) == 0 { continue; }

            let remaining_piece_count = piece_count - PieceCount::single(Player::Red, captured_piece)
                - PieceCount::single(Player::Blue, capturing_piece);

            for capturing_y in 0..9 {
                for capturing_x in 0..9 {
                    if (capturing_x, capturing_y) == Position::corner_goal(Player::Blue) { continue; }
                    *board.get_mut(capturing_x, capturing_y) = Cell::Occupied(Player::Blue, capturing_piece);
                    for (captured_x, captured_y) in AdjacentLocs::new(capturing_x, capturing_y) {
                        // Already checked captures into goal above
                        if (captured_x, captured_y) == Position::corner_goal(Player::Blue) { continue; }
                        *board.get_mut(captured_x, captured_y) = Cell::Occupied(Player::Red, captured_piece);
                        iter_all_boards(remaining_piece_count, &mut board, |b| {
                            let before_capture_index = position_index::board_to_index(b, piece_count) as usize;
                            if remaining_piece_count.total_for_player(Player::Red) == 0 {
                                println!("trivial win: {}", b.to_fen());
                                // Trivial win by capturing all their pieces
                                tablebase[before_capture_index] = WDLZ::win_in(1);
                            } else {
                                let mut position_after_capture =
                                    Position::from_board_and_player(b.clone(), Player::Blue);
                                position_after_capture.make_move(Move {
                                    from_x: capturing_x, from_y: capturing_y,
                                    to_x: captured_x, to_y: captured_y, is_capture: true,
                                });
                                debug_assert!(position_after_capture.get_outcome().is_none());
                                let res = normalization::normalize(&mut position_after_capture);
                                let after_capture_piece_count = res.piece_count;
                                let after_capture_index = position_index::board_to_index(
                                    &position_after_capture.board, after_capture_piece_count) as usize;

                                if tablebases[&after_capture_piece_count][after_capture_index].is_loss() {
                                    // NOTE - not a win in this many moves, only DTZ of 1
                                    tablebase[before_capture_index] = WDLZ::win_in(1);
                                }
                            }
                        });
                        *board.get_mut(captured_x, captured_y) = Cell::Empty;
                    }
                    *board.get_mut(capturing_x, capturing_y) = Cell::Empty;
                }
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

