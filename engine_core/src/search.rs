use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use std::cmp::max;

use crate::{Value, Move, MAX_PLY};
use crate::position::Position;
use crate::heuristic;
use crate::tt;

pub struct Opts {
    pub max_depth: Option<u8>,
    pub max_nodes: Option<usize>,
    pub search_moves: HashSet<Move>, // If non-empty only search these moves
    pub soft_deadline: Option<Duration>,
    pub hard_deadline: Option<Duration>,
}

struct State<'a> {
    pub position: &'a mut Position,
    pub nodes: usize,
    pub max_depth: Option<u8>,
    pub max_nodes: Option<usize>,
    pub search_moves: HashSet<Move>, // If non-empty only search these moves
    pub soft_deadline: Option<Instant>,
    pub hard_deadline: Option<Instant>,
    pub tt: &'a mut tt::TranspositionTable,
}

pub fn best_move(position: &mut Position, opts: &Opts, tt: &mut tt::TranspositionTable)
    -> Option<Move> {
    let move_scores = evaluate_moves(position, opts, tt);
    let mut best_move: Option<Move> = None;
    let mut best_score = -Value::infinity();
    for (m, score) in move_scores {
        if best_move.is_none() || score > best_score {
            best_move = Some(m);
            best_score = score;
        }
    }
    if best_move.is_some() {
        eprintln!("best score: {best_score} best move: {}", best_move.unwrap());
    }
    best_move
}

// Returns {move -> score for current player}
pub fn evaluate_moves(position: &mut Position, opts: &Opts, tt: &mut tt::TranspositionTable)
    -> HashMap<Move, Value> {
    if let Some(_) = position.get_outcome() {
        return HashMap::new(); // No moves to try
    }

    let start_time = Instant::now();
    let hard_deadline = opts.hard_deadline.map(|dur| start_time + dur);
    let soft_deadline = opts.soft_deadline.map(|dur| start_time + dur).or(hard_deadline);
    let mut state = State {
        position,
        nodes: 0,
        max_depth: opts.max_depth,
        max_nodes: opts.max_nodes,
        search_moves: opts.search_moves.clone(),
        soft_deadline,
        hard_deadline,
        tt,
    };

    let mut move_scores = HashMap::new();
    let mut reached_depth = 0;
    'outer:
    for depth in 1u8.. {
        let mut depth_move_scores = HashMap::new();

        let hard_deadline = state.hard_deadline;
        if depth == 1 {
            state.hard_deadline = None;
        }

        for m in state.position.legal_moves().collect::<Vec<_>>() {
            if !state.search_moves.is_empty() && !state.search_moves.contains(&m) {
                continue;
            }
            let undo = state.position.make_move(m);
            let child_result = negamax(&mut state, depth - 1, -Value::infinity(), Value::infinity());
            state.position.undo_move(undo);

            match child_result {
                None => break 'outer,
                Some(v) => depth_move_scores.insert(m, -v),
            };
        }
        state.hard_deadline = hard_deadline;

        move_scores = depth_move_scores;
        reached_depth = depth;

        if depth >= MAX_PLY ||
            state.max_depth.map(|md| depth >= md).unwrap_or(false) ||
            state.soft_deadline.map(|deadline| Instant::now() >= deadline).unwrap_or(false) {
                break;
        }
    }

    eprintln!("reached depth {}, searched {} nodes, took {:?}",
        reached_depth, state.nodes, start_time.elapsed());

    move_scores
}

fn negamax(state: &mut State, depth: u8, mut alpha: Value, beta: Value)
    -> Option<Value> {
    if state.position.get_outcome().is_some() {
        return Some(heuristic::calculate(state.position));
    } else if depth >= 3 &&
        (state.hard_deadline.map(|dl| Instant::now() > dl).unwrap_or(false) ||
         state.max_nodes.map(|max| state.nodes > max).unwrap_or(false)) {
        return None;
    }
    state.nodes += 1;

    let alpha_orig = alpha;
    let mut best_move: Option<Move> = None;

    if let Some(tt_data) = state.tt.get(state.position.zobrist.hash_value()) {
        if tt_data.depth >= depth {
            if tt_data.bound == tt::Bound::Exact {
                return Some(tt_data.value);
            } else if tt_data.bound == tt::Bound::Lower && tt_data.value >= beta {
                return Some(tt_data.value);
            } else if tt_data.bound == tt::Bound::Upper && tt_data.value <= alpha {
                return Some(tt_data.value);
            }
        }
        best_move = Some(tt_data.best_move);
    }

    // If depth == 0, only search series of immediate captures (i.e. quiescence search)
    let mut moves = state.position.legal_moves()
        .filter(|m| depth > 0 || m.is_capture)
        .collect::<Vec<_>>();
    if moves.is_empty() {
        return Some(heuristic::calculate(state.position));
    }
    // Sort the moves with the TT best move first, then all other captures, then everything else
    moves.sort_by_key(|&m|
        (best_move.map(|bm| bm != m).unwrap_or(false), !m.is_capture));

    let mut value = -Value::infinity();
    let mut best_move = moves[0];
    for m in moves {
        let undo = state.position.make_move(m);
        let child_result = negamax(state, depth.saturating_sub(1), -beta, -alpha);
        state.position.undo_move(undo);

        let child_value = -child_result?;
        if child_value > value {
            best_move = m;
            value = child_value;
        }
        alpha = max(alpha, value);
        if alpha >= beta {
            break;
        }
    }

    let bound = if value <= alpha_orig {
        tt::Bound::Upper
    } else if value >= beta {
        tt::Bound::Lower
    } else {
        tt::Bound::Exact
    };
    let tt_data = tt::TTData {
        depth,
        bound,
        generation: state.tt.generation(),
        best_move,
        value,
    };
    state.tt.put(state.position.zobrist.hash_value(), &tt_data);

    Some(value)
}
