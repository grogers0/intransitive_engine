use std::collections::HashSet;
use std::io::{self, Write};
use std::time::Duration;

use engine_core::{Move, Position, Player, search};

fn main() {
    let mut position = Position::initial();
    for line in io::stdin().lines().map(|line| line.unwrap()) {
        match line.as_str() {
            s if s.starts_with("setoption ") => {
                // TODO if we implement options
            },
            s if s.starts_with("position ") => {
                position = Position::parse_rpsi_position(&line);
            },
            s if s.starts_with("legalmoves ") => check_legal_moves(&position, &line),
            s if s.starts_with("go ") => {
                let search_opts = parse_search_opts(position.active_player, &line);
                // TODO - nodes?
                let best_move = search::best_move(&mut position, search_opts.depth,
                    search_opts.soft_deadline, search_opts.hard_deadline).unwrap();
                println!("bestmove {}", best_move);
            },
            "rpsi" => {
                println!("id name groge_bot");
                println!("id author Greg Rogers");
                // TODO - id version <git_hash>?
                // TODO - options? These seem to be the ones from UCI:
                //     Hash: Sets the size of the transposition table in megabytes.
                //     MultiPV: Tells the engine to output multiple principal variations (lines of analysis) at the same time.
                //     Ponder: Enables the engine to think ahead on the opponent's expected time (background calculation).
                println!("protocol 1");
                println!("rules 2");
                println!("mode V6 Intransitive");
                println!("rpsiok");
            },
            "isready" => println!("readyok"),
            "newgame V6" => {
                position = Position::initial();
                // TODO - we would reset the transposition table here when it's implemented
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

struct SearchOpts {
    soft_deadline: Option<Duration>,
    hard_deadline: Option<Duration>,
    depth: Option<usize>,
    nodes: Option<usize>,
    search_moves: HashSet<Move>,
}

fn parse_search_opts(player: Player, line: &str) -> SearchOpts {
    let mut infinite = false;
    let mut red_time: Option<Duration> = None;
    let mut blue_time: Option<Duration> = None;
    let mut red_incr: Option<Duration> = None;
    let mut blue_incr: Option<Duration> = None;
    let mut move_time: Option<Duration> = None;
    let mut search_moves = HashSet::new();
    let mut depth: Option<usize> = None;
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

    let one_sec = Duration::from_millis(1000);
    // Give some time for the network to send to the server
    let pad_deadline = |mut deadline: Duration| -> Duration {
        if deadline >= one_sec {
            deadline -= one_sec;
        } else {
            deadline = Duration::from_millis(0);
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

    SearchOpts { soft_deadline, hard_deadline, depth, nodes, search_moves }
}
