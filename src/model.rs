#[derive(Clone, Debug)]
pub struct Student {
    pub name: String,
    pub number: u16,
    pub targets: Vec<Target>,
    pub close_to: Vec<u16>,
}

impl Student {
    pub fn new(name: &str, number: u16, targets: Vec<Target>, close_to: Vec<u16>) -> Self {
        Self {
            name: name.to_string(),
            number,
            targets,
            close_to,
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

#[derive(Clone, Copy, Debug, Default)]
pub struct AnnealingConfig {
    pub seed: u64,
    pub budget: usize,
}

#[derive(Clone, Debug)]
pub struct SeatingResult {
    pub layout: Vec<Option<usize>>,
    pub cost: f32,
}
