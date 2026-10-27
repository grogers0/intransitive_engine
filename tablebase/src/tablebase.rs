use std::collections::HashMap;
use std::fs;
use std::io;

use crate::{WDL, PieceCount, normalization, position_index};
use base::{Value, Position, Player, Piece, MAX_PLY};
use memmap2::Mmap;

pub struct Tablebase {
    max_piece_total: u8,
    wdl: HashMap<PieceCount, Mmap>,
    dtz: HashMap<PieceCount, Mmap>,
    dtm: HashMap<PieceCount, Mmap>,
}

fn filename_to_piece_count(filename: &str) -> PieceCount {
    let mut piece_count = PieceCount::ZERO;
    for ch in filename.chars() {
        match ch {
            'R' => *piece_count.get_mut(Player::Blue, Piece::Rock) += 1,
            'P' => *piece_count.get_mut(Player::Blue, Piece::Paper) += 1,
            'S' => *piece_count.get_mut(Player::Blue, Piece::Scissors) += 1,
            'r' => *piece_count.get_mut(Player::Red, Piece::Rock) += 1,
            'p' => *piece_count.get_mut(Player::Red, Piece::Paper) += 1,
            's' => *piece_count.get_mut(Player::Red, Piece::Scissors) += 1,
            '.' => break,
            _ => panic!("misunderstood filename {}", filename),
        }
    }
    piece_count
}

impl Tablebase {
    pub fn new(dir: &str) -> Self {
        let mut tb = Self {
            max_piece_total: 0,
            wdl: HashMap::new(),
            dtz: HashMap::new(),
            dtm: HashMap::new(),
        };
        tb.refresh(dir).unwrap();
        tb
    }

    pub fn refresh(&mut self, dir: &str) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() { continue; } // Ignore
            if let Some(filename) = path.file_name().and_then(|os_str| os_str.to_str()) {
                let piece_count = filename_to_piece_count(filename);
                let tablebase_map = if filename.ends_with(".tbwdl") {
                    &mut self.wdl
                } else if filename.ends_with(".tbdtz") {
                    &mut self.dtz
                } else if filename.ends_with(".tbdtm") {
                    &mut self.dtm
                } else {
                    continue; // Skip unknown files
                };
                if tablebase_map.contains_key(&piece_count) {
                    continue; // Already loaded
                }
                let file = fs::File::open(&path)?;
                let mmap = unsafe { Mmap::map(&file)? };
                assert_eq!(mmap[mmap.len() - 1], 0); // Only mode 0 supported right now
                tablebase_map.insert(piece_count, mmap);
                self.max_piece_total = std::cmp::max(self.max_piece_total, piece_count.total()); 
            }
        }
        Ok(())
    }

    // Looks up only WDL
    pub fn lookup_fast(&self, position: &mut Position) -> Option<Value> {
        if position.total_pieces > self.max_piece_total { return None; }
        let (index, piece_count) = Self::position_to_index(position);
        match self.lookup_wdl_raw(index, piece_count) {
            None => None,
            Some(WDL::Loss) => Some(Value::loss()),
            Some(WDL::Draw) => Some(Value::draw()),
            Some(WDL::Win) => Some(Value::win()),
        }
    }

    // Looks up both WDL and DTZ (or DTM if available)
    pub fn lookup_slow(&self, position: &mut Position) -> Option<Value> {
        if position.total_pieces > self.max_piece_total { return None; }
        let (index, piece_count) = Self::position_to_index(position);
        let cast_ply = |ply: u16| -> u8 {
            u8::try_from(ply).unwrap_or(u8::MAX)
        };
        let saturate_ply = |ply: u8| -> u8 {
            std::cmp::min(ply, MAX_PLY)
        };
        match self.lookup_wdl_raw(index, piece_count) {
            None => None,
            Some(WDL::Loss) => {
                if let Some(dtm) = self.lookup_dtm_raw(index, piece_count) {
                    Some(Value::loss_in(saturate_ply(cast_ply(dtm))))
                } else if let Some(dtz) = self.lookup_dtz_raw(index, piece_count) {
                    Some(Value::loss_in(saturate_ply(dtz)))
                } else {
                    Some(Value::loss())
                }
            },
            Some(WDL::Draw) => Some(Value::draw()),
            Some(WDL::Win) => {
                if let Some(dtm) = self.lookup_dtm_raw(index, piece_count) {
                    Some(Value::win_in(saturate_ply(cast_ply(dtm))))
                } else if let Some(dtz) = self.lookup_dtz_raw(index, piece_count) {
                    Some(Value::win_in(saturate_ply(dtz)))
                } else {
                    Some(Value::win())
                }
            },
        }
    }

    fn position_to_index(position: &mut Position) -> (u64, PieceCount) {
        let pre_normalize_piece_count = PieceCount::from_board(&position.board);
        let undo_normalize = normalization::normalize_position(
            position, pre_normalize_piece_count);
        let piece_count = undo_normalize.piece_count;
        let index = position_index::board_to_index(&position.board, piece_count);
        normalization::undo_normalize_position(position, undo_normalize);
        (index, piece_count)
    }

    // Raw APIs used by the generator, not indended to be used by search
    pub fn lookup_wdl_raw(&self, index: u64, piece_count: PieceCount) -> Option<WDL> {
        self.wdl.get(&piece_count).map(|wdl_mmap| {
            let byte_idx = (index as usize) >> 2;
            let bit_idx = (index & 0x03) << 1;
            match (wdl_mmap[byte_idx] >> bit_idx) & 0x03 {
                0 => WDL::Loss,
                1 => WDL::Draw,
                2 => WDL::Win,
                3 => panic!("malformed WDL file for piece count: {piece_count:?} contains unset field at index {index}"),
                _ => unreachable!(),
            }
        })
    }
    pub fn lookup_dtz_raw(&self, index: u64, piece_count: PieceCount) -> Option<u8> {
        self.dtz.get(&piece_count).map(|dtz_mmap| {
            dtz_mmap[index as usize]
        })
    }
    pub fn lookup_dtm_raw(&self, index: u64, piece_count: PieceCount) -> Option<u16> {
        self.dtm.get(&piece_count).map(|dtm_mmap| {
            let byte_idx = (index as usize) << 1;
            u16::from_le_bytes(dtm_mmap.as_ref()[byte_idx..(byte_idx + 1)].try_into().unwrap())
        })
    }
    pub fn get_covered_piece_counts(&self) -> Vec<PieceCount> {
        self.wdl.keys().cloned().collect()
    }
}
