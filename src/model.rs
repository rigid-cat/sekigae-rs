#[derive(Clone, Debug)]
pub struct Student {
    pub name: String,
    pub number: u16,
    pub targets: Vec<Target>,
    pub forced_targets: Vec<Target>,
    pub tags: Vec<String>,
    pub close_to: Vec<u16>,
    #[allow(dead_code)]
    pub forced_close_to: Vec<u16>,
    #[allow(dead_code)]
    pub avoid: Vec<u16>,
    #[allow(dead_code)]
    pub forced_avoid: Vec<u16>,
}

impl Student {
    pub fn new(
        name: &str,
        number: u16,
        targets: Vec<Target>,
        forced_targets: Vec<Target>,
        tags: Vec<String>,
        close_to: Vec<u16>,
        forced_close_to: Vec<u16>,
        avoid: Vec<u16>,
        forced_avoid: Vec<u16>,
    ) -> Self {
        Self {
            name: name.to_string(),
            number,
            targets,
            forced_targets,
            tags,
            close_to,
            forced_close_to,
            avoid,
            forced_avoid,
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
    pub randomness: f32,
}

#[derive(Clone, Debug)]
pub struct SeatingResult {
    pub layout: Vec<Option<usize>>,
    pub cost: f32,
}
