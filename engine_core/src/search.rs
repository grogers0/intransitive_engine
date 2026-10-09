use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::cmp::max;

use crate::{Value, Move, MAX_PLY};
use crate::position::Position;
use crate::heuristic;
use crate::tt;

pub fn best_move(position: &mut Position, max_depth: Option<usize>,
    soft_deadline_dur: Option<Duration>, hard_deadline_dur: Option<Duration>,
    tt: &mut tt::TranspositionTable) -> Option<Move> {
    let move_scores = evaluate_moves(
        position, max_depth, soft_deadline_dur, hard_deadline_dur, tt);
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
pub fn evaluate_moves(position: &mut Position, max_depth: Option<usize>,
    soft_deadline_dur: Option<Duration>, hard_deadline_dur: Option<Duration>,
    tt: &mut tt::TranspositionTable) -> HashMap<Move, Value> {
    let start_time = Instant::now();
    if let Some(_) = position.get_outcome() {
        return HashMap::new(); // No moves to try
    }

    let hard_deadline = hard_deadline_dur.map(|dur| start_time + dur);
    let soft_deadline = soft_deadline_dur.map(|dur| start_time + dur).or(hard_deadline);

    let mut move_scores = HashMap::new();
    let mut reached_depth = 0;
    'outer:
    for depth in 1u8.. {
        let mut depth_move_scores = HashMap::new();

        let hard_deadline = if depth == 1 { None } else { hard_deadline };
        for m in position.legal_moves().collect::<Vec<_>>() {
            let undo = position.make_move(m);
            let child_result = negamax(position, depth - 1, -Value::infinity(),
            Value::infinity(), hard_deadline, tt);
            position.undo_move(undo);

            match child_result {
                None => break 'outer,
                Some(v) => depth_move_scores.insert(m, -v),
            };
        }

        move_scores = depth_move_scores;
        reached_depth = depth;

        if depth >= MAX_PLY ||
            max_depth.map(|md| depth as usize >= md).unwrap_or(false) ||
            soft_deadline.map(|deadline| Instant::now() >= deadline).unwrap_or(false) {
                break;
        }
    }

    eprintln!("reached depth {reached_depth}");

    move_scores
}

fn negamax(position: &mut Position, depth: u8, mut alpha: Value, beta: Value,
    deadline: Option<Instant>, tt: &mut tt::TranspositionTable) -> Option<Value> {
    if depth == 0 || position.get_outcome().is_some() {
        return Some(heuristic::calculate(position));
    } else if deadline.is_some() && depth >= 3 && Instant::now() > deadline.unwrap() {
        // TODO - min depth heuristic should be tuned to avoid too frequent time comparisons
        return None;
    }

    let alpha_orig = alpha;
    let mut best_move: Option<Move> = None;

    if let Some(tt_data) = tt.get(position.zobrist.hash_value()) {
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

    let mut moves = position.legal_moves().collect::<Vec<_>>();
    // Sort the moves with the TT best move first, then all other captures, then everything else
    moves.sort_by_key(|&m|
        (best_move.map(|bm| bm != m).unwrap_or(false), !m.is_capture));

    let mut value = -Value::infinity();
    let mut best_move = moves[0];
    for m in moves {
        let undo = position.make_move(m);
        let child_result = negamax(position, depth - 1, -beta, -alpha, deadline, tt);
        position.undo_move(undo);

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
        generation: tt.generation,
        best_move,
        value,
    };
    tt.put(position.zobrist.hash_value(), &tt_data);

    Some(value)
}
