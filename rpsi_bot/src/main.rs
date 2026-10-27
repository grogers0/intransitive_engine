use std::collections::HashSet;
use std::io::{self, Write};
use std::time::Duration;

use base::{Move, Position, Player, tt, history};
use tablebase::Tablebase;

const HASH_OPTION_PREFIX: &str = "setoption name Hash value ";

fn main() {
    let opts = parse_command_line_args();
    const TT_MAX_SIZE_MB: usize = 128_000; // TODO - set this limit based on system memory?
    let mut position = Position::initial();
    let mut tt = tt::TranspositionTable::new(tt::DEFAULT_TT_SIZE_MB);
    let mut history = history::HistoryTable::new();
    let tablebase = Tablebase::new(&opts.tablebase_dir);
    for line in io::stdin().lines().map(|line| line.unwrap()) {
        match line.as_str() {
            s if s.starts_with("setoption ") => {
                if s.starts_with(HASH_OPTION_PREFIX) {
                    if let Ok(size_mb) = s[HASH_OPTION_PREFIX.len()..].parse::<usize>() {
                        tt.resize(size_mb);
                        eprintln!("resized transposition table to {} MB", size_mb);
                    } else {
                        println!("info string ignored incorrectly formatted option: {}", s);
                        eprintln!("ignored incorrectly formatted option: {}", s);
                    }
                } else {
                    println!("info string ignored unknown option: {}", s);
                    eprintln!("ignored unknown option: {}", s);
                }
            },
            s if s.starts_with("position ") => {
                position = Position::parse_rpsi_position(&line);
            },
            s if s.starts_with("legalmoves ") => check_legal_moves(&position, &line),
            s if s.starts_with("go ") => {
                let search_opts = parse_search_opts(position.active_player, &line);
                tt.next_search();
                let best_move = search::best_move(
                    &mut position, &search_opts, &mut tt, &mut history, &tablebase);
                println!("bestmove {}", best_move);
            },
            "rpsi" => {
                println!("id name groge_bot");
                println!("id author Greg Rogers");
                // TODO - id version <git_hash>?
                // TODO - options? These seem to be the ones from UCI:
                //     MultiPV: Tells the engine to output multiple principal variations (lines of analysis) at the same time.
                //     Ponder: Enables the engine to think ahead on the opponent's expected time (background calculation).

                // Transposition table size in MB
                println!("option name Hash type spin default {} min 1 max {}",
                    tt::DEFAULT_TT_SIZE_MB, TT_MAX_SIZE_MB);
                println!("protocol 1");
                println!("rules 2");
                println!("mode V6 Intransitive");
                println!("rpsiok");
            },
            "isready" => println!("readyok"),
            "newgame V6" => {
                position = Position::initial();
                tt.clear();
                history.clear();
            },
            "stop" => {
                // TODO - interrupt searching and emit the best move
            },
            "quit" => return, // Exit immediately
            _ => {
                println!("info string unknown command: {}", line);
                eprintln!("unknown command: {}", line);
            },
        }
        io::stdout().flush().unwrap();
    }
}

fn check_legal_moves(position: &Position, line: &str) {
    let mut tokens = line.split(" ");
    assert_eq!("legalmoves", tokens.next().unwrap());

    // The presented legal moves don't include capture info so swallow it
    let parsed_moves = tokens.map(|s| Move::parse(s)).collect::<HashSet<_>>();
    let calculated_moves = position.legal_moves().map(|mut m| {m.is_capture = false; m})
        .collect::<HashSet<_>>();

    for m in parsed_moves.difference(&calculated_moves) {
        println!("info string legal move {} missing from calculated moves", m);
        eprintln!("legal move {} missing from calculated moves", m);
    }
    for m in calculated_moves.difference(&parsed_moves) {
        println!("info string calculated move {} as legal but was missing from legal moves", m);
        eprintln!("calculated move {} as legal but was missing from legal moves", m);
    }
}

fn parse_search_opts(player: Player, line: &str) -> search::Opts {
    let mut infinite = false;
    let mut red_time: Option<Duration> = None;
    let mut blue_time: Option<Duration> = None;
    let mut red_incr: Option<Duration> = None;
    let mut blue_incr: Option<Duration> = None;
    let mut move_time: Option<Duration> = None;
    let mut search_moves = HashSet::new();
    let mut depth: Option<u8> = None;
    let mut nodes: Option<usize> = None;

    fn parse_as_millis(token: &str) -> Duration {
        return Duration::from_millis(token.parse().unwrap());
    }
    let mut tokens = line.split(" ");
    assert_eq!("go", tokens.next().unwrap());
    enum State {
        Init, BlueTime, BlueIncr, RedTime, RedIncr, MoveTime, Depth, Nodes, SearchMoves,
    }
    let mut state = State::Init;
    for token in tokens {
        match state {
            State::Init => match token {
                "btime" => state = State::BlueTime,
                "rtime" => state = State::RedTime,
                "binc" => state = State::BlueIncr,
                "rinc" => state = State::RedIncr,
                "movetime" => state = State::MoveTime,
                "depth" => state = State::Depth,
                "nodes" => state = State::Nodes,
                "infinite" => infinite = true,
                "searchmoves" => state = State::SearchMoves,
                _ => {
                    println!("info string misunderstood go option {}", token);
                    eprintln!("misunderstood go option {}", token);
                },
            },
            State::BlueTime => { blue_time = Some(parse_as_millis(token)); state = State::Init },
            State::RedTime => { red_time = Some(parse_as_millis(token)); state = State::Init },
            State::BlueIncr => { blue_incr = Some(parse_as_millis(token)); state = State::Init },
            State::RedIncr => { red_incr = Some(parse_as_millis(token)); state = State::Init },
            State::MoveTime => { move_time = Some(parse_as_millis(token)); state = State::Init },
            State::Depth => { depth = Some(token.parse().unwrap()); state = State::Init },
            State::Nodes => { nodes = Some(token.parse().unwrap()); state = State::Init },
            State::SearchMoves => {
                // Remainder of tokens are the move(s) to search
                search_moves.insert(Move::parse(token));
            },
        }
    }

    let buffer_time = Duration::from_secs(2);
    let one_sec = Duration::from_secs(1);
    // Give some time for the network to send to the server
    let pad_deadline = |mut deadline: Duration| -> Duration {
        if deadline >= buffer_time {
            deadline -= buffer_time;
        } else {
            deadline = Duration::from_secs(0);
        }
        deadline
    };

    let (soft_deadline, mut hard_deadline) = if infinite {
        (None, None)
    } else if let Some(_) = move_time {
        (move_time, move_time)
    } else if player == Player::Blue {
        if blue_incr.is_some() && blue_time.is_some() {
            (blue_incr, Some(pad_deadline(blue_time.unwrap())))
        } else {
            println!("info string missing blue time limits, defaulting to 1s");
            eprintln!("missing blue time limits, defaulting to 1s");
            (Some(one_sec), Some(one_sec))
        }
    } else {
        if red_incr.is_some() && red_time.is_some() {
            (red_incr, Some(pad_deadline(red_time.unwrap())))
        } else {
            println!("info string missing red time limits, defaulting to 1s");
            eprintln!("missing red time limits, defaulting to 1s");
            (Some(one_sec), Some(one_sec))
        }
    };

    let abandonment_time = pad_deadline(Duration::from_secs(30));
    if hard_deadline.is_none() || hard_deadline.unwrap() > abandonment_time {
        hard_deadline = Some(abandonment_time);
    }

    search::Opts {
        max_depth: depth,
        max_nodes: nodes,
        search_moves,
        soft_deadline,
        hard_deadline,
    }
}

struct Opts {
    tablebase_dir: String,
    tt_size_mb: usize,
}

fn parse_command_line_args() -> Opts {
    let mut opts = Opts {
        tablebase_dir: "data/tablebase".to_string(),
        tt_size_mb: tt::DEFAULT_TT_SIZE_MB,
    };

    let mut args = std::env::args();
    args.next(); // Skip binary name
    while let Some(token) = args.next() {
        match token.as_str() {
            "--hash_mb" | "--hash-mb" | "--tt_size_mb" | "--tt-size-mb" => {
                opts.tt_size_mb = args.next().unwrap().parse().unwrap();
            },
            "--tablebase" | "--tablebase_dir" | "--tablebase-dir" => {
                opts.tablebase_dir = args.next().unwrap().to_string();
            },
            _ => {
                panic!("unknown command line argument \"{}\"", token);
            },
        };
    }

    opts
}

