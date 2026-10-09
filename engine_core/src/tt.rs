use std::mem;

use crate::moves::Move;
use crate::compact_move::CompactMove;
use crate::Value;

// This largely follows the design of the stockfish TT. It's a 3-way associative hash table

pub const DEFAULT_TT_SIZE_MB: usize = 100;

const CLUSTER_SIZE: usize = 3;
const GENERATION_BITS: usize = 5;
const GENERATION_MASK: u8 = (1 << GENERATION_BITS) - 1;

#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Bound {
    Undefined, Lower, Upper, Exact
}

impl From<u8> for Bound {
    fn from(v: u8) -> Bound {
        match v {
            0 => Bound::Undefined,
            1 => Bound::Lower,
            2 => Bound::Upper,
            3 => Bound::Exact,
            _ => panic!(),
        }
    }
}

// Shift then mask
const BOUND_SHIFT: usize = GENERATION_BITS;
const BOUND_MASK: u8 = 0x03;
// TODO - PV flag in the top bit

pub struct TTData {
    pub depth: u8,
    pub bound: Bound,
    pub generation: u8,
    pub best_move: Move,
    pub value: Value,
}

#[derive(Clone)]
struct TTEntry {
    key: u16,
    depth: u8,
    misc: u8, // Principal Variation (1 bit), Bound Type (2 bits), Generation (5 bits)
    best_move: u16,
    value: i16,
}

impl TTEntry {
    fn zero() -> TTEntry {
        TTEntry { key: 0, depth: 0, misc: 0, best_move: 0, value: 0 }
    }

    fn to_data(&self) -> TTData {
        TTData {
            depth: self.depth,
            bound: Bound::from((self.misc >> BOUND_SHIFT) & BOUND_MASK),
            generation: (self.misc & GENERATION_MASK),
            best_move: CompactMove(self.best_move).into(),
            value: Value(self.value),
        }
    }

    fn from_data(key: u16, tt_data: &TTData) -> TTEntry {
        let mut misc = tt_data.generation;
        misc |= (tt_data.bound as u8) << BOUND_SHIFT;
        TTEntry {
            key,
            depth: tt_data.depth,
            misc,
            best_move: CompactMove::from(tt_data.best_move).0,
            value: tt_data.value.0,
        }
    }

    fn generations_ago(&self, curr_generation: u8) -> u8 {
        (curr_generation - self.misc) & GENERATION_MASK
    }
}

#[derive(Clone)]
#[repr(align(32))]
struct Cluster {
    entries: [TTEntry; CLUSTER_SIZE]
}

impl Cluster {
    fn zero() -> Cluster {
        Cluster { entries: [TTEntry::zero(), TTEntry::zero(), TTEntry::zero()] }
    }
}

pub struct TranspositionTable {
    pub generation: u8,
    num_clusters: usize,
    clusters: Box<[Cluster]>,
}

impl TranspositionTable {
    fn calc_num_clusters(size_mb: usize) -> usize {
        assert!(mem::size_of::<Cluster>() == 32);
        size_mb * 1024 * 1024 / mem::size_of::<Cluster>()
    }

    pub fn new(size_mb: usize) -> TranspositionTable {
        let generation = 0;
        let num_clusters = Self::calc_num_clusters(size_mb);
        let clusters = vec![Cluster::zero(); num_clusters].into_boxed_slice();
        TranspositionTable { generation, num_clusters, clusters }
    }

    pub fn resize(&mut self, size_mb: usize) {
        let num_clusters = Self::calc_num_clusters(size_mb);
        if self.num_clusters == num_clusters {
            return;
        }
        self.generation = 0;
        self.num_clusters = num_clusters;
        self.clusters = vec![Cluster::zero(); num_clusters].into_boxed_slice();
    }

    pub fn clear(&mut self) {
        self.generation = 0;
        self.clusters.fill(Cluster::zero());
    }

    pub fn next_search(&mut self) {
        self.generation = (self.generation + 1) & GENERATION_MASK;
    }

    pub fn get(&self, key: u64) -> Option<TTData> {
        let cluster_idx = self.cluster_idx(key);
        let key = key as u16;

        for entry_idx in 0..CLUSTER_SIZE {
            let entry = &self.clusters[cluster_idx].entries[entry_idx];
            if entry.key == key && (entry.misc & (BOUND_MASK << BOUND_SHIFT)) != 0 {
                return Some(entry.to_data());
            }
        }
        return None;
    }

    pub fn put(&mut self, key: u64, data: &TTData) {
        let cluster_idx = self.cluster_idx(key);
        let key = key as u16;

        let mut entry_idx = CLUSTER_SIZE;
        for i in 0..CLUSTER_SIZE {
            if self.clusters[cluster_idx].entries[i].key == key {
                entry_idx = i;
                break;
            }
        }
        if entry_idx == CLUSTER_SIZE {
            entry_idx = 0;
            let entries = &self.clusters[cluster_idx].entries;
            // older entries are less useful, this metric is copied from stockfish
            let mut best_goodness = entries[0].depth.saturating_sub(
                8 * entries[0].generations_ago(self.generation));
            for i in 1..CLUSTER_SIZE {
                let i_goodness = entries[i].depth.saturating_sub(
                    8 * entries[i].generations_ago(self.generation));
                if i_goodness > best_goodness {
                    entry_idx = i;
                    best_goodness = i_goodness;
                }
            }
        }

        // TODO - avoid overwriting in some situations?

        self.clusters[cluster_idx].entries[entry_idx] = TTEntry::from_data(key, data);
    }

    fn cluster_idx(&self, key: u64) -> usize {
        ((key >> 16) % self.num_clusters as u64) as usize
    }
}
