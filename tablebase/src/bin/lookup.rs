use base::{Board, Position, Player};
use tablebase::Tablebase;

struct Opts {
    dir: String,
    position: Option<Position>,
}

fn main() {
    let opts = parse_command_line_args();
    let tb = Tablebase::new(&opts.dir);

    if let Some(pos) = opts.position {
        lookup_and_print(pos.clone(), &tb);
    } else {
        for line in std::io::stdin().lines() {
            let pos = parse_position(line.unwrap().split_whitespace().map(|s| s.to_string()));
            lookup_and_print(pos, &tb);
        }
    }
}

fn to_player_str(pos: &Position) -> &'static str {
    match pos.active_player {
        Player::Blue => "b",
        Player::Red => "r",
    }
}

fn lookup_and_print(mut pos: Position, tb: &Tablebase) {
    if let Some(v) = tb.lookup_slow(&mut pos) {
        println!("{} {} {}", pos.board.to_fen(), to_player_str(&pos), v);
    } else {
        println!("not found in tablebase");
    }
}

fn parse_position<It: Iterator<Item=String>>(mut tokens: It) -> Position {
    let fen_str = tokens.next().unwrap().to_string();
    let board = Board::from_fen(&fen_str);
    let player = match tokens.next().as_ref().map(|s| s.as_str()) {
        None => Player::Blue,
        Some("b") => Player::Blue,
        Some("r") => Player::Red,
        Some(token) => panic!("unknown player \"{}\"", token),
    };
    assert!(tokens.next().is_none());
    Position::from_board_and_player(board, player)
}

fn parse_command_line_args() -> Opts {
    let mut opts = Opts {
        dir: "data/tablebase".to_string(),
        position: None,
    };

    let mut args = std::env::args();
    args.next(); // Skip binary name
    while let Some(token) = args.next() {
        match token.as_str() {
            "--directory" | "--dir" | "--tablebase_dir" | "--tablebase-dir" => {
                opts.dir = args.next().unwrap().to_string();
            },
            "--" => {
                opts.position = Some(parse_position(args));
                break;
            },
            _ => {
                panic!("unknown command line argument \"{}\"", token);
            },
        };
    }

    opts
}

