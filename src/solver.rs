use sekigae3::{ILSA, Problem, Seat};
use std::collections::HashMap;
use std::io::{Error, ErrorKind, Result};

use crate::model::{AnnealingConfig, SeatingResult, Student};

struct CorrectedDistanceFn;

impl sekigae3::DistanceFn for CorrectedDistanceFn {
    fn distance(&self, a: (i16, i16), b: (i16, i16)) -> u16 {
        (((a.0 - b.0).abs()).pow(2) + ((a.1 - b.1).abs() + 1).pow(2)) as u16
    }
}

fn add_pair_weights(
    pair_weight_sum: &mut HashMap<(usize, usize), f32>,
    number_to_idx: &HashMap<u16, usize>,
    a: usize,
    wanted_ids: &[u16],
    weight: f32,
) {
    for wanted in wanted_ids {
        if let Some(&b) = number_to_idx.get(wanted)
            && a != b
        {
            let key = if a < b { (a, b) } else { (b, a) };
            *pair_weight_sum.entry(key).or_insert(0.0) += weight;
        }
    }
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

    let mut number_to_idx = HashMap::new();
    for (idx, student) in students.iter().enumerate() {
        number_to_idx.insert(student.number, idx);
    }

    let total_solver_students = movable_seats.len();

    let seats = movable_seats
        .iter()
        .map(|seat_idx| Seat {
            x: (*seat_idx % cols) as i16,
            y: (*seat_idx / cols) as i16,
        })
        .collect::<Vec<_>>();

    let seat_id_by_global = movable_seats
        .iter()
        .enumerate()
        .map(|(seat_id, global_idx)| (*global_idx, seat_id as u16))
        .collect::<HashMap<_, _>>();

    let randomness = config.randomness.clamp(0.0, 1.0);
    let soft_scale = 1.0 - randomness;
    const FORCED_WEIGHT: f32 = 1_000_000.0;
    const FORCED_PAIR_WEIGHT: f32 = 100_000_000.0;

    let mut want_seats = vec![Vec::<(u16, f32)>::new(); total_solver_students];
    for (student_idx, student) in students.iter().enumerate() {
        let mut prefs: HashMap<u16, f32> = HashMap::new();

        if soft_scale > 0.0 {
            let target_len = student.targets.len().max(1) as f32;

            for (rank, target) in student.targets.iter().enumerate() {
                let global_idx = target.r.saturating_mul(cols).saturating_add(target.c);
                if global_idx >= seat_count || blocked[global_idx] {
                    continue;
                }

                if let Some(&seat_id) = seat_id_by_global.get(&global_idx) {
                    let weight = ((target_len - rank as f32).max(0.1)) * soft_scale;
                    prefs
                        .entry(seat_id)
                        .and_modify(|existing| *existing = existing.max(weight))
                        .or_insert(weight);
                }
            }
        }

        for target in &student.forced_targets {
            let global_idx = target.r.saturating_mul(cols).saturating_add(target.c);
            if global_idx >= seat_count || blocked[global_idx] {
                continue;
            }

            if let Some(&seat_id) = seat_id_by_global.get(&global_idx) {
                prefs
                    .entry(seat_id)
                    .and_modify(|existing| *existing = existing.max(FORCED_WEIGHT))
                    .or_insert(FORCED_WEIGHT);
            }
        }

        let mut prefs = prefs.into_iter().collect::<Vec<_>>();
        prefs.sort_by_key(|(seat_id, _)| *seat_id);
        want_seats[student_idx] = prefs;
    }

    let mut pair_weight_sum: HashMap<(usize, usize), f32> = HashMap::new();
    for (a, student) in students.iter().enumerate() {
        let avoid_weight = -soft_scale;
        add_pair_weights(
            &mut pair_weight_sum,
            &number_to_idx,
            a,
            &student.close_to,
            soft_scale,
        );
        add_pair_weights(
            &mut pair_weight_sum,
            &number_to_idx,
            a,
            &student.forced_close_to,
            FORCED_PAIR_WEIGHT,
        );
        add_pair_weights(
            &mut pair_weight_sum,
            &number_to_idx,
            a,
            &student.avoid,
            avoid_weight,
        );
        add_pair_weights(
            &mut pair_weight_sum,
            &number_to_idx,
            a,
            &student.forced_avoid,
            -FORCED_PAIR_WEIGHT,
        );
    }

    let mut pair_edges = vec![Vec::<(u16, f32)>::new(); total_solver_students];
    for ((a, b), weight) in pair_weight_sum {
        if weight.abs() > f32::EPSILON {
            pair_edges[a].push((b as u16, weight));
        }
    }

    let problem = Problem::with_distance_fn(seats, want_seats, pair_edges, CorrectedDistanceFn);

    let mut ilsa = ILSA::new(&problem, config.seed);
    let budget = if config.budget == 0 {
        total_solver_students.max(1)
    } else {
        config.budget
    };

    let best = ilsa.solve(budget);

    let by_seat = best.by_seat();
    if by_seat.len() != total_solver_students {
        return Err(Error::other(
            "sekigae3 solver returned invalid assignment length",
        ));
    }

    // 最終レイアウトを構築
    let mut layout = vec![None; seat_count];
    for (local_seat_idx, &student_id) in by_seat.iter().enumerate() {
        if (student_id as usize) < students.len() {
            let global_seat_idx = movable_seats[local_seat_idx];
            layout[global_seat_idx] = Some(student_id as usize);
        }
    }

    Ok(SeatingResult {
        layout,
        cost: best.cost(),
    })
}
