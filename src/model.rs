#[derive(Clone, Debug)]
pub struct Student {
    pub name: String,
    pub number: u16,
    pub targets: Vec<Target>,
    pub close_to: Vec<u16>,
    pub avoid: Vec<u16>,
    pub seat_pref: Option<String>,
}

impl Student {
    pub fn new(
        name: &str,
        number: u16,
        targets: Vec<Target>,
        close_to: Vec<u16>,
        avoid: Vec<u16>,
        seat_pref: Option<String>,
    ) -> Self {
        Self {
            name: name.to_string(),
            number,
            targets,
            close_to,
            avoid,
            seat_pref,
        }
    }

    // targets が空の生徒は「どこでも可」とみなす。
    pub fn is_satisfied_at(&self, seat: Target) -> bool {
        self.targets.is_empty() || self.targets.iter().any(|target| *target == seat)
    }

    // targets の先頭を第1希望として、順位が高いほど大きい値を返す。
    // 完全一致でない場合でも、近い位置に小さいボーナスを与える。
    pub fn preference_bonus_at(&self, seat: Target) -> i64 {
        if self.targets.is_empty() {
            return 1000;
        }

        if let Some(rank) = self.targets.iter().position(|target| *target == seat) {
            (self.targets.len() - rank) as i64
        } else {
            // 近い位置にボーナス
            let mut min_distance = f64::INFINITY;
            for target in &self.targets {
                let dr = seat.r as f64 - target.r as f64;
                let dc = seat.c as f64 - target.c as f64;
                let dist = (dr * dr + dc * dc).sqrt();
                if dist < min_distance {
                    min_distance = dist;
                }
            }
            if min_distance <= 2.0 {
                // 距離1: 0.5, 距離2: 0.25 など
                (self.targets.len() as f64 * 0.5_f64.powf(min_distance) * 1000.0) as i64
            } else {
                0
            }
        }
    }

    pub fn social_bonus_at(&self, seat_idx: usize, layout: &[Option<usize>], students: &[Student], cols: usize, rows: usize) -> i64 {
        let mut bonus = 0i64;
        let adj = adjacent_seats(seat_idx, cols, rows);
        for &adj_idx in &adj {
            if let Some(adj_student_idx) = layout[adj_idx] {
                let adj_student = &students[adj_student_idx];
                if self.close_to.contains(&adj_student.number) {
                    bonus += 10; // 近づきたい人が隣にいる
                }
                if self.avoid.contains(&adj_student.number) {
                    bonus -= 20; // 遠ざけたい人が隣にいる
                }
            }
        }
        bonus
    }

    pub fn seat_pref_bonus_at(&self, seat: Target) -> i64 {
        if let Some(ref pref) = self.seat_pref {
            if pref == "前" && seat.r <= 1 { // 前から2列目まで
                5
            } else {
                0
            }
        } else {
            0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub c: usize,
    pub r: usize,
}

impl Target {
    pub const fn new(c: usize, r: usize) -> Self {
        Self { c, r }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AnnealingConfig {
    pub iterations: usize,
    pub start_temp: f64,
    pub end_temp: f64,
    pub randomness: f64,
}

impl Default for AnnealingConfig {
    fn default() -> Self {
        Self {
            iterations: 100_000,
            start_temp: 8.0,
            end_temp: 0.05,
            randomness: 0.5,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SeatingResult {
    pub layout: Vec<Option<usize>>,
    pub satisfied: usize,
    pub weighted_bonus: i64,
    pub preference_bonus: i64,
    pub preference_penalty: i64,
}

fn adjacent_seats(seat_idx: usize, cols: usize, rows: usize) -> Vec<usize> {
    let r = seat_idx / cols;
    let c = seat_idx % cols;
    let mut adj = Vec::new();
    if r > 0 {
        adj.push(seat_idx - cols);
    }
    if r < rows - 1 {
        adj.push(seat_idx + cols);
    }
    if c > 0 {
        adj.push(seat_idx - 1);
    }
    if c < cols - 1 {
        adj.push(seat_idx + 1);
    }
    adj
}
