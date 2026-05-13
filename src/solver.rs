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

    // 確定隣希望を持つメンバーを特定
    let mut number_to_idx = HashMap::new();
    for (idx, student) in students.iter().enumerate() {
        number_to_idx.insert(student.number, idx);
    }

    let mut forced_close_group: HashSet<usize> = HashSet::new();
    for (a, student) in students.iter().enumerate() {
        if !student.forced_close_to.is_empty() {
            forced_close_group.insert(a);
            for wanted in &student.forced_close_to {
                if let Some(&b) = number_to_idx.get(wanted) {
                    forced_close_group.insert(b);
                }
            }
        }
    }

    // 確定隣希望グループがある場合、先にそのグループで配置
    let mut fixed_layout = vec![None; seat_count];
    if !forced_close_group.is_empty() {
        let forced_group_students: Vec<&Student> = forced_close_group
            .iter()
            .map(|&idx| &students[idx])
            .collect();

        // 確定隣希望グループのみで solver を実行
        let group_movable_seats: Vec<usize> = movable_seats.iter().copied().collect();
        let fixed_result = solve_group(
            &forced_group_students,
            rows,
            cols,
            &group_movable_seats,
            &blocked,
            config,
        )?;

        // 結果を固定レイアウトに記録
        for (seat_idx, student_idx) in fixed_result.layout.iter().enumerate() {
            fixed_layout[seat_idx] = student_idx.map(|idx| {
                forced_close_group
                    .iter()
                    .nth(idx)
                    .copied()
                    .unwrap_or(usize::MAX)
            });
        }

        // 使用された座席をブロック
        for seat_idx in 0..seat_count {
            if fixed_layout[seat_idx].is_some() {
                blocked[seat_idx] = true;
            }
        }
    }

    // 残りの生徒で通常の solver を実行
    let remaining_movable_seats: Vec<usize> =
        (0..seat_count).filter(|idx| !blocked[*idx]).collect();
    let real_student_count = students.len();
    let total_solver_students = remaining_movable_seats.len();

    let seats = remaining_movable_seats
        .iter()
        .map(|seat_idx| Seat {
            x: (*seat_idx % cols) as i16,
            y: (*seat_idx / cols) as i16,
        })
        .collect::<Vec<_>>();

    let seat_id_by_global = remaining_movable_seats
        .iter()
        .enumerate()
        .map(|(seat_id, global_idx)| (*global_idx, seat_id as u16))
        .collect::<HashMap<_, _>>();

    let randomness = config.randomness.clamp(0.0, 1.0);
    let soft_scale = 1.0 - randomness;
    const FORCED_WEIGHT: f32 = 1_000_000.0;

    let mut want_seats = vec![Vec::<(u16, f32)>::new(); total_solver_students];
    let mut student_count = 0;
    for (student_idx, student) in students.iter().enumerate() {
        if forced_close_group.contains(&student_idx) {
            continue;  // 既に配置済み
        }

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
        want_seats[student_count] = prefs;
        student_count += 1;
    }

    let mut pair_weight_sum: HashMap<(usize, usize), f32> = HashMap::new();
    let mut remaining_idx_map = HashMap::new();
    let mut remaining_count = 0;
    for (idx, student) in students.iter().enumerate() {
        if !forced_close_group.contains(&idx) {
            remaining_idx_map.insert(idx, remaining_count);
            remaining_count += 1;
        }
    }

    for (a, student) in students.iter().enumerate() {
        if forced_close_group.contains(&a) {
            continue;
        }
        if let Some(&a_mapped) = remaining_idx_map.get(&a) {
            for wanted in &student.close_to {
                if let Some(&b) = number_to_idx.get(wanted) {
                    if forced_close_group.contains(&b) {
                        continue;
                    }
                    if let Some(&b_mapped) = remaining_idx_map.get(&b) {
                        if a_mapped == b_mapped {
                            continue;
                        }
                        let key = if a_mapped < b_mapped {
                            (a_mapped, b_mapped)
                        } else {
                            (b_mapped, a_mapped)
                        };
                        *pair_weight_sum.entry(key).or_insert(0.0) += soft_scale;
                    }
                }
            }
        }
    }

    let mut pair_edges = vec![Vec::<(u16, f32)>::new(); total_solver_students];
    for ((a, b), weight) in pair_weight_sum {
        if weight.abs() > f32::EPSILON {
            pair_edges[a].push((b as u16, weight));
        }
    }

    let problem = Problem::new(seats, want_seats, pair_edges);

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
    let mut layout = fixed_layout;
    for (local_seat_idx, &student_id) in by_seat.iter().enumerate() {
        let global_seat_idx = remaining_movable_seats[local_seat_idx];
        let mut remaining_idx = 0;
        for (idx, student) in students.iter().enumerate() {
            if !forced_close_group.contains(&idx) {
                if remaining_idx == student_id as usize {
                    layout[global_seat_idx] = Some(idx);
                    break;
                }
                remaining_idx += 1;
            }
        }
    }

    Ok(SeatingResult {
        layout,
        cost: best.cost(),
    })
}

/// 確定隣希望グループのみで solver を実行
fn solve_group(
    group_students: &[&Student],
    rows: usize,
    cols: usize,
    movable_seats: &[usize],
    blocked: &[bool],
    config: AnnealingConfig,
) -> Result<SeatingResult> {
    let seat_count = rows * cols;

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

    let mut want_seats = vec![Vec::<(u16, f32)>::new(); group_students.len()];
    for (student_idx, student) in group_students.iter().enumerate() {
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

    let mut number_to_idx = HashMap::new();
    for (idx, student) in group_students.iter().enumerate() {
        number_to_idx.insert(student.number, idx);
    }

    let mut pair_weight_sum: HashMap<(usize, usize), f32> = HashMap::new();
    for (a, student) in group_students.iter().enumerate() {
        for wanted in &student.forced_close_to {
            if let Some(&b) = number_to_idx.get(wanted) {
                if a == b {
                    continue;
                }
                let key = if a < b { (a, b) } else { (b, a) };
                *pair_weight_sum.entry(key).or_insert(0.0) += FORCED_PAIR_WEIGHT;
            }
        }
    }

    let mut pair_edges = vec![Vec::<(u16, f32)>::new(); group_students.len()];
    for ((a, b), weight) in pair_weight_sum {
        if weight.abs() > f32::EPSILON {
            pair_edges[a].push((b as u16, weight));
        }
    }

    let problem = Problem::new(seats, want_seats, pair_edges);

    let mut ilsa = ILSA::new(&problem, config.seed);
    let budget = if config.budget == 0 {
        group_students.len().max(1)
    } else {
        config.budget
    };

    let best = ilsa.solve(budget);

    let by_seat = best.by_seat();
    let mut layout = vec![None; seat_count];
    for (local_seat_idx, &student_id) in by_seat.iter().enumerate() {
        if (student_id as usize) < group_students.len() {
            layout[movable_seats[local_seat_idx]] = Some(student_id as usize);
        }
    }

    Ok(SeatingResult {
        layout,
        cost: best.cost(),
    })
}
