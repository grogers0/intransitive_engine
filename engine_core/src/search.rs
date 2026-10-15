use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

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
    pub evals: usize,
    pub tt_full_hits: usize,
    pub tt_partial_hits: usize,
    pub max_depth: Option<u8>,
    pub max_nodes: Option<usize>,
    pub soft_deadline: Option<Instant>,
    pub hard_deadline: Option<Instant>,
    pub tt: &'a mut tt::TranspositionTable,
}

pub fn best_move(position: &mut Position, opts: &Opts, tt: &mut tt::TranspositionTable) -> Move {
    if let Some(_) = position.get_outcome() {
        return Move::invalid(); // No moves to try
    }

    let start_time = Instant::now();
    let hard_deadline = opts.hard_deadline.map(|dur| start_time + dur);
    let soft_deadline = opts.soft_deadline.map(|dur| start_time + dur).or(hard_deadline);
    let mut state = State {
        position,
        nodes: 0,
        evals: 0,
        tt_full_hits: 0,
        tt_partial_hits: 0,
        max_depth: opts.max_depth,
        max_nodes: opts.max_nodes,
        soft_deadline,
        hard_deadline,
        tt,
    };

    let mut moves = state.position.legal_moves()
        .filter(|m| opts.search_moves.is_empty() || opts.search_moves.contains(m))
        .collect::<Vec<_>>();
    moves.sort_by_key(|&m| !m.is_capture);
    let mut best_move = moves[0];
    let mut best_value = -Value::infinity();

    let mut reached_depth = 0;
    'outer:
    for depth in 1u8.. {
        let hard_deadline = state.hard_deadline;
        if depth == 1 {
            state.hard_deadline = None;
        }

        // Sort the moves with the previous best move first, then all other captures, then everything else
        moves.sort_by_key(|&m| (best_move != m, !m.is_capture));
        let mut alpha = -Value::infinity();
        let mut depth_best_move = best_move;

        for &m in &moves {
            let undo = state.position.make_move(m);
            let child_result = negamax(&mut state, depth - 1, -Value::infinity(), -alpha);
            state.position.undo_move(undo);

            if !child_result.is_valid() {
                break 'outer;
            } else {
                let v = -child_result.add_ply();
                if v > alpha {
                    depth_best_move = m;
                    alpha = v;
                }
            }
        }
        state.hard_deadline = hard_deadline;

        best_value = alpha;
        best_move = depth_best_move;
        reached_depth = depth;

        if depth >= MAX_PLY ||
            state.max_depth.map(|md| depth >= md).unwrap_or(false) ||
            state.soft_deadline.map(|deadline| Instant::now() >= deadline).unwrap_or(false) ||
            best_value.is_win_or_loss() {
                break;
        }
    }

    eprintln!("best_move: {}, value: {}, reached depth {}, searched {} nodes, made {} evals, full TT hits: {}, partial TT hits: {}, took {:?}",
        best_move, best_value, reached_depth, state.nodes, state.evals, state.tt_full_hits,
        state.tt_partial_hits, start_time.elapsed());

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
        evals: 0,
        tt_full_hits: 0,
        tt_partial_hits: 0,
        max_depth: opts.max_depth,
        max_nodes: opts.max_nodes,
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
            if !opts.search_moves.is_empty() && !opts.search_moves.contains(&m) {
                continue;
            }
            let undo = state.position.make_move(m);
            let child_result = negamax(&mut state, depth - 1, -Value::infinity(), Value::infinity());
            state.position.undo_move(undo);

            if !child_result.is_valid() {
                break 'outer;
            } else {
                depth_move_scores.insert(m, -child_result.add_ply());
            }
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

    eprintln!("reached depth {}, searched {} nodes, made {} evals, full TT hits: {}, partial TT hits: {}, took {:?}",
        reached_depth, state.nodes, state.evals, state.tt_full_hits, state.tt_partial_hits,
        start_time.elapsed());

    move_scores
}

fn negamax(state: &mut State, depth: u8, mut alpha: Value, beta: Value) -> Value {
    if let Some(outcome) = state.position.get_outcome() {
        return heuristic::calculate_outcome(outcome, state.position.active_player);
    } else if depth >= 3 &&
        (state.hard_deadline.map(|dl| Instant::now() > dl).unwrap_or(false) ||
         state.max_nodes.map(|max| state.nodes > max).unwrap_or(false)) {
        return Value::invalid();
    }
    state.nodes += 1;

    let alpha_orig = alpha;
    let mut best_move = Move::invalid();

    if let Some(tt_data) = state.tt.get(state.position.zobrist.hash_value()) {
        if tt_data.depth >= depth {
            if tt_data.bound == tt::Bound::Exact {
                state.tt_full_hits += 1;
                return tt_data.value;
            } else if tt_data.bound == tt::Bound::Lower && tt_data.value >= beta {
                state.tt_full_hits += 1;
                return tt_data.value;
            } else if tt_data.bound == tt::Bound::Upper && tt_data.value <= alpha {
                state.tt_full_hits += 1;
                return tt_data.value;
            }
        }
        state.tt_partial_hits += 1;
        best_move = tt_data.best_move;
    }

    // If depth == 0, only search series of immediate captures (i.e. quiescence search)
    let mut moves = state.position.legal_moves()
        .filter(|m| depth > 0 || m.is_capture)
        .collect::<Vec<_>>();
    // Sort the moves with the TT best move first, then all other captures, then everything else
    moves.sort_by_key(|&m| (best_move != m, !m.is_capture));

    let mut value = -Value::infinity();
    let mut bound_override: Option<tt::Bound> = None;
    if moves.is_empty() {
        state.evals += 1;
        value = heuristic::calculate_diff(state.position);
        bound_override = Some(tt::Bound::Exact);
    } else {
        for m in moves {
            let undo = state.position.make_move(m);
            let child_result = negamax(state, depth.saturating_sub(1), -beta, -alpha);
            state.position.undo_move(undo);

            if !child_result.is_valid() {
                return Value::invalid();
            }
            let v = -child_result.add_ply();
            if v > value {
                best_move = m;
                value = v;
            }
            if value > alpha {
                alpha = value;
            }
            if alpha >= beta {
                break;
            }
        }
    }

    let bound = if let Some(b) = bound_override {
        b
    } else if value <= alpha_orig {
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

    value
}
