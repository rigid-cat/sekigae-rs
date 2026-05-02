use std::cmp::min;
use rand::Rng;
use rand::seq::SliceRandom;
use std::collections::HashMap;
use std::io::{Error, ErrorKind, Result};

use crate::model::{AnnealingConfig, SeatingResult, Student, Target};

#[derive(Clone, Copy, Debug)]
struct Evaluation {
    cost: i64,
    satisfied: usize,
    weighted_bonus: i64,
    preference_bonus: i64,
    preference_penalty: i64,
}

/// empty_seat_indices で指定した席を必ず空席に固定したまま探索する。
pub fn find_best_seating_with_blocked(
    students: &[Student],
    rows: usize,
    cols: usize,
    empty_seat_indices: &[usize],
    config: AnnealingConfig,
) -> Result<SeatingResult> {
    if rows == 0 || cols == 0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "rows and cols must be greater than 0",
        ));
    }
    if config.start_temp <= 0.0 || config.end_temp <= 0.0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "start_temp and end_temp must be greater than 0",
        ));
    }

    let seat_count = rows * cols;
    let mut blocked = vec![false; seat_count];
    for &idx in empty_seat_indices {
        if idx >= seat_count {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "empty seat index out of range",
            ));
        }
        blocked[idx] = true;
    }

    let movable_seats: Vec<usize> = (0..seat_count).filter(|idx| !blocked[*idx]).collect();

    if students.len() > movable_seats.len() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "number of students exceeds available seats",
        ));
    }

    // 初期解: 空席固定を守りつつ、利用可能席だけをシャッフルする。
    let mut rng = rand::rng();
    let mut current_layout = vec![None; seat_count];
    let mut shuffled_seats = movable_seats.clone();
    shuffled_seats.shuffle(&mut rng);

    for (student_idx, &seat_idx) in shuffled_seats.iter().take(students.len()).enumerate() {
        current_layout[seat_idx] = Some(student_idx);
    }

    let mut current_eval = evaluate_layout(&current_layout, students, cols, rows, config.randomness);
    let mut best_layout = current_layout.clone();
    let mut best_eval = current_eval;

    if movable_seats.len() >= 2 {
        let steps = config.iterations.max(1);
        for step in 0..steps {

            let temp = cooling_temperature(step, steps, config.start_temp, config.end_temp);
            let (a, b) = random_swap_indices(movable_seats.len(), &mut rng);
            let seat_a = movable_seats[a];
            let seat_b = movable_seats[b];

            current_layout.swap(seat_a, seat_b);
            let candidate_eval = evaluate_layout(&current_layout, students, cols, rows, config.randomness);
            let delta = candidate_eval.cost - current_eval.cost;

            let accept = if delta <= 0 {
                true
            } else {
                let probability = (-(delta as f64) / temp).exp();
                rng.random::<f64>() < probability
            };

            if accept {
                current_eval = candidate_eval;
                if current_eval.cost < best_eval.cost {
                    best_eval = current_eval;
                    best_layout = current_layout.clone();
                }
            } else {
                current_layout.swap(seat_a, seat_b);
            }
        }
    }

    Ok(SeatingResult {
        layout: best_layout,
        satisfied: best_eval.satisfied,
        weighted_bonus: best_eval.weighted_bonus,
        preference_bonus: best_eval.preference_bonus,
        preference_penalty: best_eval.preference_penalty,
    })
}

fn evaluate_layout(layout: &[Option<usize>], students: &[Student], cols: usize, _rows: usize, randomness: f64) -> Evaluation {
    let mut satisfied = 0usize;
    let mut unsatisfied = 0usize;
    let mut weighted_bonus = 0i64;
    let mut preference_bonus = 0i64;
    let mut preference_penalty = 0i64;

    let mut positions = HashMap::new();
    for (seat_idx, &occupant) in layout.iter().enumerate() {
        if let Some(student_idx) = occupant {
            let student = &students[student_idx];
            let pos = Target::new(seat_idx % cols, seat_idx / cols);
            positions.insert(student.number, pos);
        }
    }

    for (seat_idx, occupant) in layout.iter().enumerate() {
        let Some(student_idx) = occupant else {
            continue;
        };

        let seat = Target::new(seat_idx % cols, seat_idx / cols);
        let student = &students[*student_idx];

        if student.is_satisfied_at(seat) {
            satisfied += 1;
            weighted_bonus += student.preference_bonus_at(seat);
        } else {
            unsatisfied += 1;
        }

        // close_to and avoid
        let mut satisfied = false;
        for &num in &student.close_to {
            if let Some(&other_pos) = positions.get(&num) {
                let size = min(3, student.close_to.len());
                let dr = seat.r as f64 - other_pos.r as f64;
                let dc = seat.c as f64 - other_pos.c as f64;
                let dist = (dr * dr + dc * dc).sqrt();
                let m = if size == 1 { 100 } else { 1 };
                preference_bonus += if dc == 1.0 && dr == 0.0 {
                    satisfied = true;
                    1000 * m / size as i64
                } else if dist <= 1.5 {
                    500 * m / size as i64
                } else if dist <= 2.5 {
                    100 * m / size as i64
                } else if dist <= 3.5 {
                    10 * m / size as i64
                } else {
                    0 // 4マス以上離れたら、くっつきたい欲求を評価しない
                };
                preference_penalty += if dist > 1.0 && size == 1 {
                    (dist * 10000.0) as i64 // 離れすぎているとペナルティ
                } else {
                    0
                };
                preference_penalty += if dist > 3.0 {
                    (dist * 50.0) as i64 // 離れすぎているとペナルティ
                } else {
                    0
                };
            }
        }
        if !satisfied { preference_penalty += 100 }

        for &num in &student.avoid {
            if let Some(&other_pos) = positions.get(&num) {
                let size = min(3, student.avoid.len());
                let dr = seat.r as f64 - other_pos.r as f64;
                let dc = seat.c as f64 - other_pos.c as f64;
                let dist = (dr * dr + dc * dc).sqrt();
                preference_penalty += if dist <= 1.0 {
                    750 / size as i64
                } else if dist <= 2.0 {
                    100 / size as i64
                } else if dist <= 3.0 {
                    10 / size as i64
                } else {
                    0
                };
                preference_penalty += if dist < 2.0 {
                    (dist * 70.0 * if size == 1 { 100.0 } else { 1.0 } ) as i64 // 近づきすぎているとペナルティ
                } else {
                    0
                }
            }
        }
    }

    let mut rng = rand::rng();
    let f: f64 = rng.random();
    let ignore_preferences = f < 1.0 - (1.0 - randomness).powf(10.0);
    let mut preference_cost = preference_penalty - preference_bonus;
    if ignore_preferences {
        preference_cost = 0;
    }

    // 未満足 1 人分の重みを大きくして、満足人数を最優先する。
    Evaluation {
        cost:
        (unsatisfied as i64) * 5000
            + preference_cost
            - weighted_bonus,
        satisfied,
        weighted_bonus,
        preference_bonus,
        preference_penalty,
    }
}

fn cooling_temperature(step: usize, steps: usize, start_temp: f64, end_temp: f64) -> f64 {
    if steps <= 1 {
        return end_temp.max(1e-9);
    }
    let progress = step as f64 / (steps - 1) as f64;
    let ratio = (end_temp / start_temp).max(1e-12);
    (start_temp * ratio.powf(progress)).max(1e-9)
}

fn random_swap_indices(len: usize, rng: &mut impl Rng) -> (usize, usize) {
    let a = rng.random_range(0..len);
    let mut b = rng.random_range(0..(len - 1));
    if b >= a {
        b += 1;
    }
    (a, b)
}
