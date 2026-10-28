use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;
use rayon::iter::ParallelIterator;

const USIZE_BITS: usize = usize::BITS as usize;
const USIZE_SHIFT: usize = USIZE_BITS.trailing_zeros() as usize;
const USIZE_MASK: usize = USIZE_BITS - 1;

// Essentially a BTreeSet<usize> for items under a specific ID, with much more compact storage
pub struct AtomicBitset {
    chunks: Box<[AtomicUsize]>,
}

impl AtomicBitset {
    pub fn new(len: usize) -> Self {
        let num_chunks = (len + USIZE_MASK) >> USIZE_SHIFT;
        let chunks = (0..num_chunks).map(|_| AtomicUsize::new(0))
            .collect::<Vec<_>>().into_boxed_slice();
        Self { chunks }
    }

    pub fn allocated_bytes(&self) -> usize {
        self.chunks.len() * std::mem::size_of::<usize>()
    }

    pub fn contains(&self, idx: usize) -> bool {
        let chunk_idx = idx >> USIZE_SHIFT;
        let mask = 1 << (idx & USIZE_MASK);
        let chunk_val = self.chunks[chunk_idx].load(Ordering::Relaxed);
        (chunk_val & mask) != 0
    }

    // Returns true if the item was actually inserted
    pub fn insert(&self, idx: usize) -> bool {
        let chunk_idx = idx >> USIZE_SHIFT;
        let mask = 1 << (idx & USIZE_MASK);
        let chunk_val = self.chunks[chunk_idx].fetch_or(mask, Ordering::Relaxed);
        (chunk_val & mask) == 0
    }

    // Returns true if the item was previously present
    pub fn remove(&self, idx: usize) -> bool {
        let chunk_idx = idx >> USIZE_SHIFT;
        let mask = 1 << (idx & USIZE_MASK);
        let chunk_val = self.chunks[chunk_idx].fetch_and(!mask, Ordering::Relaxed);
        (chunk_val & mask) != 0
    }

    fn iter_chunk(chunk_idx: usize, mut chunk_val: usize) -> impl Iterator<Item=usize> {
        let offset = chunk_idx * USIZE_BITS;
        std::iter::from_fn(move || {
            if chunk_val != 0 {
                let trailing_zeros = chunk_val.trailing_zeros() as usize;
                chunk_val &= chunk_val - 1; // Clear lowest bit
                Some(offset + trailing_zeros)
            } else {
                None
            }
        })
    }

    pub fn par_drain(&self) -> impl ParallelIterator<Item=usize> {
        self.chunks.par_iter().enumerate()
            .flat_map_iter(|(chunk_idx, chunk_ref)| {
                Self::iter_chunk(chunk_idx, chunk_ref.swap(0, Ordering::Relaxed))
            })
    }

    pub fn par_iter(&self) -> impl ParallelIterator<Item=usize> {
        self.chunks.par_iter().enumerate()
            .flat_map_iter(|(chunk_idx, chunk_ref)| {
                Self::iter_chunk(chunk_idx, chunk_ref.load(Ordering::Relaxed))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use rand::prelude::*;

    #[test]
    fn test_random() {
        for _ in 0..100 {
            test_random_once();
        }
    }

    // Compare behavior vs an equivalent BTreeSet
    fn test_random_once() {
        let mut rng = rand::rng();
        let max = rng.random_range(1..1000);
        let actual = AtomicBitset::new(max);
        let mut expected = BTreeSet::new();
        for _ in 0..100 {
            let idx = rng.random_range(0..max);
            assert_eq!(expected.contains(&idx), actual.contains(idx));
            assert_eq!(expected.insert(idx), actual.insert(idx));
            assert_eq!(expected.contains(&idx), actual.contains(idx));
            if rng.random_bool(0.3) {
                assert_eq!(expected.remove(&idx), actual.remove(idx));
                assert_eq!(expected.contains(&idx), actual.contains(idx));
            }
        }
        for (expected_idx, actual_idx) in expected.iter().cloned().zip(actual.par_iter().collect::<Vec<_>>()) {
            assert_eq!(expected_idx, actual_idx);
        }
        for (expected_idx, actual_idx) in expected.iter().cloned().zip(actual.par_drain().collect::<Vec<_>>()) {
            assert_eq!(expected_idx, actual_idx);
        }
        assert_eq!(actual.par_iter().collect::<Vec<_>>(), vec![]);
    }
}
