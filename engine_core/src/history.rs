use crate::{Move, Player};

// Implement history heuristic

pub struct HistoryTable([i16; 2 * 9 * 9 * 3 * 3]);

impl HistoryTable {
    pub fn new() -> Self {
        Self([0; 2 * 9 * 9 * 3 * 3])
    }

    pub fn clear(&mut self) {
        self.0.fill(0);
    }

    // Low (highly negative) values are better for ordering
    pub fn get(&self, player: Player, m: Move) -> i16 {
        self.0[Self::idx(player, m)]
    }

    pub fn give_bonus(&mut self, player: Player, m: Move, depth: u8) {
        let bonus = Self::bonus(depth);
        let idx = Self::idx(player, m);
        self.0[idx] = self.0[idx].saturating_sub(bonus);
    }

    pub fn give_penalty(&mut self, player: Player, m: Move, depth: u8) {
        let penalty = Self::bonus(depth) / 2;
        let penalty = std::cmp::max(1, penalty); // Avoid penalizing by 0
        let idx = Self::idx(player, m);
        self.0[idx] = self.0[idx].saturating_add(penalty);
    }

    fn bonus(depth: u8) -> i16 {
        (depth as i16).saturating_add(depth as i16)
    }

    fn idx(player: Player, m: Move) -> usize {
        debug_assert!(m.is_valid());
        let mut ret = (player as usize) * 9 * 9 * 3 * 3;
        ret += (m.from_y as usize) * 9 * 3 * 3;
        ret += (m.from_x as usize) * 3 * 3;
        ret += ((m.from_y as usize) + 1 - (m.to_y as usize)) * 3;
        ret += (m.from_x as usize) + 1 - (m.to_x as usize);
        ret
    }
}
