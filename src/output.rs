use std::fs::File;
use std::io::{Result, Write};
use std::path::Path;

use crate::model::{Student, Target};

pub fn build_grid(
    layout: &[Option<usize>],
    students: &[Student],
    rows: usize,
    cols: usize,
) -> Vec<Vec<String>> {
    let mut grid = Vec::with_capacity(rows);
    for r in 0..rows {
        let mut row = Vec::with_capacity(cols);
        for c in 0..cols {
            let seat_idx = r * cols + c;
            let cell = match layout[seat_idx] {
                Some(student_idx) => {
                    format!("{}({})", students[student_idx].name, students[student_idx].number)
                }
                None => String::new(),
            };
            row.push(cell);
        }
        grid.push(row);
    }
    grid
}

pub fn print_grid(grid: &[Vec<String>]) {
    println!("--- seating layout ---");
    for row in grid {
        println!("{}", row.join(" | "));
    }
}

pub fn print_student_report(layout: &[Option<usize>], students: &[Student], cols: usize) {
    let mut assigned = vec![None; students.len()];
    for (seat_idx, occupant) in layout.iter().enumerate() {
        if let Some(student_idx) = occupant {
            assigned[*student_idx] = Some(Target::new(seat_idx % cols, seat_idx / cols));
        }
    }

    println!("--- student report ---");
    for (idx, student) in students.iter().enumerate() {
        if let Some(seat) = assigned[idx] {
            let status = if student.is_satisfied_at(seat) {
                "OK"
            } else {
                "NG"
            };
            println!(
                "{}({}): row={}, col={} => {}",
                student.name,
                student.number,
                seat.r + 1,
                seat.c + 1,
                status
            );
        }
    }
}

pub fn write_layout_csv(
    layout: &[Option<usize>],
    students: &[Student],
    rows: usize,
    cols: usize,
    path: &str,
) -> Result<()> {
    let grid = build_grid(layout, students, rows, cols);
    let mut csv: CSV<String> = CSV::new();

    for (r, row) in grid.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            csv.insert(r, c, cell.clone());
        }
    }

    csv.write(path)
}

pub struct CSV<T> {
    data: Vec<Vec<T>>,
}

impl<T> CSV<T>
where
    T: Default + ToString,
{
    pub fn new() -> CSV<T> {
        CSV { data: Vec::new() }
    }

    pub fn insert<I>(&mut self, r: usize, c: usize, data: I)
    where
        I: Into<T>,
    {
        let cols = self
            .data
            .iter()
            .map(|row| row.len())
            .max()
            .unwrap_or(0)
            .max(c + 1);

        // 可変サイズ挿入を許すため、足りない行・列を default 値で補完する。
        if self.data.len() <= r {
            self.data.resize_with(r + 1, || {
                let mut row = Vec::new();
                row.resize_with(cols, T::default);
                row
            });
        }

        for row in &mut self.data {
            if row.len() < cols {
                row.resize_with(cols, T::default);
            }
        }

        self.data[r][c] = data.into();
    }

    pub fn write<P>(&self, path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let mut file = File::create(path)?;

        for row in &self.data {
            for (i, clm) in row.iter().enumerate() {
                if i > 0 {
                    write!(file, ",")?;
                }

                let item = clm.to_string();
                write!(file, "{}", escape_csv_field(&item))?;
            }
            writeln!(file)?;
        }

        Ok(())
    }
}

fn escape_csv_field(s: &str) -> String {
    let needs_quotes = s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r');

    if needs_quotes {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
