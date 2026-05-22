#[derive(Clone, Debug)]
pub struct Student {
    pub name: String,
    pub number: u16,
    pub targets: Vec<Target>,
    pub forced_targets: Vec<Target>,
    pub close_to: Vec<u16>,
    pub forced_close_to: Vec<u16>,
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
    pub randomness: f32,
}

#[derive(Clone, Debug)]
pub struct SeatingResult {
    pub layout: Vec<Option<usize>>,
    pub cost: f32,
}
