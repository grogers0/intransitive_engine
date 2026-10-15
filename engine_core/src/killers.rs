use crate::{MAX_PLY, Move};

// The killer heuristic

// Per depth
const NUM_KILLERS: usize = 3;

pub struct Killers([Move; NUM_KILLERS * MAX_PLY as usize]);

impl Killers {
    pub fn new() -> Killers {
        Killers([Move::invalid(); NUM_KILLERS * MAX_PLY as usize])
    }

    // Returns 0 for the most recently added killer, 1 for the next, up to NUM_KILLERS if not found
    pub fn get(&self, depth: u8, m: Move) -> usize {
        let base = depth as usize * NUM_KILLERS;
        for i in 0..NUM_KILLERS {
            if m == self.0[base + i] {
                return i;
            }
        }
        NUM_KILLERS
    }

    pub fn add(&mut self, depth: u8, m: Move) {
        debug_assert!(depth < MAX_PLY);
        let existing_id = self.get(depth, m);
        let base = depth as usize * NUM_KILLERS;

        self.0.copy_within(base..(base + existing_id), base + 1);
        self.0[base] = m;
    }

    pub fn shift_for_depth_increase(&mut self, new_depth: u8) {
        self.0.copy_within(0..((new_depth as usize - 1) * NUM_KILLERS), NUM_KILLERS);
    }
}
