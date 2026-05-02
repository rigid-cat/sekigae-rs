use crate::model::{Student, Target};

/// デモ用の生徒データ。
/// c は列、r は行で 0 始まり。
pub fn sample_students() -> Vec<Student> {
    vec![
        Student::new("A", 1, vec![Target::new(0, 0), Target::new(1, 0)]),
        Student::new("B", 2, vec![Target::new(1, 0), Target::new(2, 0)]),
        Student::new("C", 3, vec![Target::new(2, 0), Target::new(3, 0)]),
        Student::new("D", 4, vec![Target::new(3, 0), Target::new(4, 0)]),
        Student::new("E", 5, vec![Target::new(4, 0), Target::new(3, 0)]),
        Student::new("F", 6, vec![Target::new(0, 1), Target::new(1, 1)]),
        Student::new("G", 7, vec![Target::new(1, 1), Target::new(2, 1)]),
        Student::new("H", 8, vec![Target::new(2, 1), Target::new(3, 1)]),
        Student::new("I", 9, vec![Target::new(3, 1), Target::new(4, 1)]),
        Student::new("J", 10, vec![Target::new(4, 1), Target::new(4, 2)]),
        Student::new("K", 11, vec![Target::new(0, 2), Target::new(1, 2)]),
        Student::new("L", 12, vec![Target::new(1, 2), Target::new(2, 2)]),
    ]
}
