use sekigae3::{ILSA, Problem, Seat};
use std::collections::{HashMap, HashSet};
use std::io::{Error, ErrorKind, Result};

use crate::model::{AnnealingConfig, SeatingResult, Student};

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

    // sekigae3 は「座席数 = 探索対象人数」を前提とするため、
    // 利用可能席が余る場合はダミー生徒を追加して整合させる。
    let real_student_count = students.len();
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

    let mut want_seats = vec![Vec::<(u16, f32)>::new(); total_solver_students];
    for (student_idx, student) in students.iter().enumerate() {
        let mut prefs = Vec::new();
        let mut seen = HashSet::new();
        let target_len = student.targets.len().max(1) as f32;

        for (rank, target) in student.targets.iter().enumerate() {
            let global_idx = target.r.saturating_mul(cols).saturating_add(target.c);
            if global_idx >= seat_count || blocked[global_idx] {
                continue;
            }

            if let Some(&seat_id) = seat_id_by_global.get(&global_idx)
                && seen.insert(seat_id)
            {
                // 順位が高い希望ほど重みを強くする。
                let weight = (target_len - rank as f32).max(0.1);
                prefs.push((seat_id, weight));
            }
        }

        want_seats[student_idx] = prefs;
    }

    let mut number_to_idx = HashMap::new();
    for (idx, student) in students.iter().enumerate() {
        number_to_idx.insert(student.number, idx);
    }

    let mut pair_weight_sum: HashMap<(usize, usize), f32> = HashMap::new();
    for (a, student) in students.iter().enumerate() {
        for wanted in &student.close_to {
            if let Some(&b) = number_to_idx.get(wanted) {
                if a == b {
                    continue;
                }
                let key = if a < b { (a, b) } else { (b, a) };
                *pair_weight_sum.entry(key).or_insert(0.0) += 1.0;
            }
        }
    }

    let mut pair_edges = vec![Vec::<(u16, f32)>::new(); total_solver_students];
    for ((a, b), weight) in pair_weight_sum {
        if weight.abs() > f32::EPSILON {
            pair_edges[a].push((b as u16, weight));
        }
    }

    // 問題作成
    let problem = Problem::new(seats, want_seats, pair_edges);

    let mut ilsa = ILSA::new(&problem, config.seed);
    let budget = if config.budget == 0 {
        total_solver_students.max(1)
    } else {
        config.budget
    };

    // 最適化
    let best = ilsa.solve(budget);

    let by_seat = best.by_seat();
    if by_seat.len() != total_solver_students {
        return Err(Error::other(
            "sekigae3 solver returned invalid assignment length",
        ));
    }

    let mut layout = vec![None; seat_count];
    for (local_seat_idx, &student_id) in by_seat.iter().enumerate() {
        let global_seat_idx = movable_seats[local_seat_idx];
        let assigned_idx = student_id as usize;
        if assigned_idx < real_student_count {
            layout[global_seat_idx] = Some(assigned_idx);
        }
    }

    Ok(SeatingResult {
        layout,
        cost: best.cost(),
    })
}
