use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::cmp::max;

use crate::moves::Move;
use crate::position::Position;
use crate::heuristic;

pub fn best_move(position: &mut Position, max_depth: Option<usize>,
    soft_deadline_dur: Option<Duration>, hard_deadline_dur: Option<Duration>)
    -> Option<Move> {
    let move_scores = evaluate_moves(position, max_depth, soft_deadline_dur, hard_deadline_dur);
    let mut best_move: Option<Move> = None;
    let mut best_score = -i16::MAX;
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
    soft_deadline_dur: Option<Duration>, hard_deadline_dur: Option<Duration>)
    -> HashMap<Move, i16> {
    let start_time = Instant::now();
    if let Some(_) = position.get_outcome() {
        return HashMap::new(); // No moves to try
    }

    let hard_deadline = hard_deadline_dur.map(|dur| start_time + dur);
    let soft_deadline = soft_deadline_dur.map(|dur| start_time + dur).or(hard_deadline);

    let mut move_scores = HashMap::new();
    let mut reached_depth = 0;
    'outer:
    for depth in 1.. {
        let mut depth_move_scores = HashMap::new();

        let hard_deadline = if depth == 1 { None } else { hard_deadline };
        for m in position.legal_moves().collect::<Vec<_>>() {
            let undo = position.make_move(m);
            let child_result = negamax(position, depth - 1, -i16::MAX, i16::MAX, hard_deadline);
            position.undo_move(undo);

            match child_result {
                None => break 'outer,
                Some(v) => depth_move_scores.insert(m, -v),
            };
        }

        move_scores = depth_move_scores;
        reached_depth = depth;

        if max_depth.map(|md| depth >= md).unwrap_or(false) ||
            soft_deadline.map(|deadline| Instant::now() >= deadline).unwrap_or(false) {
                break;
        }
    }

    eprintln!("reached depth {reached_depth}");

    move_scores
}

fn negamax(position: &mut Position, depth: usize, mut alpha: i16, beta: i16,
    deadline: Option<Instant>) -> Option<i16> {
    if depth == 0 || position.get_outcome().is_some() {
        return Some(position.active_player.sign() * heuristic::calculate(position));
    } else if deadline.is_some() && depth >= 3 && Instant::now() > deadline.unwrap() {
        // TODO - min depth heuristic should be tuned to avoid too frequent time comparisons
        return None;
    }

    // TODO - order better moves like captures first
    let moves = position.legal_moves().collect::<Vec<_>>();

    let mut value = -i16::MAX;
    for m in moves {
        let undo = position.make_move(m);
        let child_result = negamax(position, depth - 1, -beta, -alpha, deadline);
        position.undo_move(undo);

        value = max(value, -child_result?);
        alpha = max(alpha, value);
        if alpha >= beta {
            break;
        }
    }
    Some(value)
}
