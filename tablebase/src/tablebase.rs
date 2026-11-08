use std::collections::HashMap;
use std::fs;
use std::io;
use std::sync::atomic::{AtomicU8, AtomicU16, Ordering};

use crate::{WDL, PieceCount, normalization, position_index, tablebase_name};
use base::{Value, Position, Player, Piece, MAX_PLY};
use memmap2::{Mmap, MmapMut};
use rayon::prelude::*;

pub trait Tablebase: Send + Sync {
    type Item;
    fn get(&self, index: u64) -> Self::Item;
    fn len(&self) -> u64;
}

pub trait WdlTablebase: Tablebase<Item = WDL> {}
pub trait DtzTablebase: Tablebase<Item = u8> {}
pub trait DtmTablebase: Tablebase<Item = u16> {}
impl<T> WdlTablebase for T where T: Tablebase<Item = WDL> {}
impl<T> DtzTablebase for T where T: Tablebase<Item = u8> {}
impl<T> DtmTablebase for T where T: Tablebase<Item = u16> {}

pub(crate) struct TablebasesForPieceCount<'a> {
    pub(crate) wdl: WdlTablebaseMmap<'a>,
    pub(crate) dtz: DtzTablebaseMmap<'a>,
    pub(crate) dtm: Option<DtmTablebaseMmap<'a>>,
}

pub struct TablebaseMap<'a> {
    max_piece_total: u8,
    pub(crate) by_piece_count: HashMap<PieceCount, TablebasesForPieceCount<'a>>,
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

impl TablebaseMap<'_> {
    pub fn new(dir: &str) -> Self {
        let mut tb = Self { max_piece_total: 0, by_piece_count: HashMap::new() };
        tb.refresh(dir).unwrap();
        tb
    }

    pub fn refresh(&mut self, dir: &str) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() { continue; } // Ignore
            if let Some(filename) = path.file_name().and_then(|os_str| os_str.to_str()) {
                let piece_count = filename_to_piece_count(filename);
                if self.by_piece_count.contains_key(&piece_count) {
                    continue; // Already loaded
                }
                let len = position_index::index_count(piece_count);

                let tb_name = tablebase_name(piece_count);
                let wdl = WdlTablebaseMmap::new(&format!("{}/{}.tbwdl", dir, tb_name), len)?;
                let dtz = DtzTablebaseMmap::new(&format!("{}/{}.tbdtz", dir, tb_name), len)?;
                let dtm = DtmTablebaseMmap::new(&format!("{}/{}.tbdtm", dir, tb_name), len).ok();
                self.by_piece_count.insert(piece_count,
                    TablebasesForPieceCount { wdl, dtz, dtm });
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

    // Raw APIs used by the generator, not intended to be used by search
    pub fn lookup_wdl_raw(&self, index: u64, piece_count: PieceCount) -> Option<WDL> {
        self.by_piece_count.get(&piece_count).map(|tbs_for_piece_count| {
            tbs_for_piece_count.wdl.get(index)
        })
    }
    pub fn lookup_dtz_raw(&self, index: u64, piece_count: PieceCount) -> Option<u8> {
        self.by_piece_count.get(&piece_count).map(|tbs_for_piece_count| {
            tbs_for_piece_count.dtz.get(index)
        })
    }
    pub fn lookup_dtm_raw(&self, index: u64, piece_count: PieceCount) -> Option<u16> {
        self.by_piece_count.get(&piece_count).and_then(|tbs_for_piece_count| {
            tbs_for_piece_count.dtm.as_ref().map(|dtm_tb| dtm_tb.get(index))
        })
    }
}

fn open_mmap_mut<'a, T>(filename: &str, len_bytes: u64) -> io::Result<(MmapMut, &'a [T])> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(filename)?;
    file.set_len(len_bytes)?;
    let mmap = unsafe { MmapMut::map_mut(&file)? };
    let len = mmap.len() / std::mem::size_of::<T>();
    let slice: &[T] = unsafe { std::slice::from_raw_parts(mmap.as_ptr() as *const T, len) };
    Ok((mmap, slice))
}

fn open_mmap<'a, T>(filename: &str) -> io::Result<(Mmap, &'a [T])> {
    let file = fs::File::open(filename)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let len = mmap.len() / std::mem::size_of::<T>();
    let slice: &[T] = unsafe { std::slice::from_raw_parts(mmap.as_ptr() as *const T, len) };
    Ok((mmap, slice))
}


fn par_memset(slice: &mut [u8], val: u8) {
    slice.par_chunks_mut(8 * 1024 * 1024).for_each(|chunk| chunk.fill(val));
}

pub struct WdlTablebaseMut<'a> {
    mmap: MmapMut,
    slice: &'a [AtomicU8],
    len: u64,
}

impl <'a> WdlTablebaseMut<'a> {
    pub fn new(filename: &str, len: u64) -> io::Result<Self> {
        let (mmap, slice) = open_mmap_mut(filename, (len + 3) / 4)?;
        Ok(Self { mmap, slice, len })
    }

    pub fn init(&mut self) {
        par_memset(&mut self.mmap, 0xff);
    }

    pub fn load(&self, index: u64, ordering: Ordering) -> Option<WDL> {
        debug_assert!(index < self.len);
        let byte_offset = index >> 2;
        let bit_offset = (index & 0x03) << 1;
        let atom_ref: &AtomicU8 = unsafe { self.slice.get_unchecked(byte_offset as usize) };
        let byte_val = atom_ref.load(ordering);
        match (byte_val >> bit_offset) & 0x03 {
            0 => Some(WDL::Loss),
            1 => Some(WDL::Draw),
            2 => Some(WDL::Win),
            3 => None,
            _ => unreachable!(),
        }
    }

    pub fn store(&self, index: u64, wdl: WDL, ordering: Ordering) {
        debug_assert!(index < self.len);
        let byte_offset = index >> 2;
        let bit_offset = (index & 0x03) << 1;
        let atom_ref: &AtomicU8 = unsafe { self.slice.get_unchecked(byte_offset as usize) };
        let mut prev_byte_val = atom_ref.load(ordering);
        loop {
            let mut byte_val = prev_byte_val;
            byte_val &= !(0x03 << bit_offset);
            byte_val |= match wdl { WDL::Loss => 0, WDL::Draw => 1, WDL::Win => 2 } << bit_offset;
            debug_assert!(byte_val == prev_byte_val ||
                ((prev_byte_val >> bit_offset) & 0x03) == 0x03);
            let cas_result = atom_ref.compare_exchange_weak(prev_byte_val, byte_val, ordering, ordering);
            match cas_result {
                Ok(_) => break,
                Err(updated_byte_val) => prev_byte_val = updated_byte_val,
            }
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.mmap.flush()
    }
}

impl Tablebase for WdlTablebaseMut<'_> {
    type Item = WDL;

    fn get(&self, index: u64) -> WDL {
        self.load(index, Ordering::Relaxed).unwrap()
    }

    fn len(&self) -> u64 {
        self.len
    }
}

pub struct WdlTablebaseMmap<'a> {
    _mmap: Mmap,
    slice: &'a [u8],
    len: u64,
}

impl WdlTablebaseMmap<'_> {
    fn new(filename: &str, len: u64) -> io::Result<Self> {
        let (mmap, slice) = open_mmap(filename)?;
        assert_eq!(slice.len() as u64, (len + 3) / 4);
        Ok(Self { _mmap: mmap, slice, len })
    }
}

impl Tablebase for WdlTablebaseMmap<'_> {
    type Item = WDL;

    fn get(&self, index: u64) -> WDL {
        debug_assert!(index < self.len);
        let byte_offset = index >> 2;
        let bit_offset = (index & 0x03) << 1;
        let byte_val = *unsafe { self.slice.get_unchecked(byte_offset as usize) };
        match (byte_val >> bit_offset) & 0x03 {
            0 => WDL::Loss,
            1 => WDL::Draw,
            2 => WDL::Win,
            3 => panic!(),
            _ => unreachable!(),
        }

    }

    fn len(&self) -> u64 {
        self.len
    }
}


pub struct DtzTablebaseMut<'a> {
    mmap: MmapMut,
    slice: &'a [AtomicU8],
}

impl <'a> DtzTablebaseMut<'a> {
    pub fn new(filename: &str, len: u64) -> io::Result<Self> {
        let (mmap, slice) = open_mmap_mut(filename, len)?;
        Ok(Self { mmap, slice })
    }

    pub fn init(&mut self) {
        par_memset(&mut self.mmap, 0);
    }

    pub fn load(&self, index: u64, ordering: Ordering) -> u8 {
        debug_assert!(index < self.len());
        let atom_ref: &AtomicU8 = unsafe { self.slice.get_unchecked(index as usize) };
        atom_ref.load(ordering)
    }

    pub fn store(&self, index: u64, dtz: u8, ordering: Ordering) {
        debug_assert!(index < self.len());
        let atom_ref: &AtomicU8 = unsafe { self.slice.get_unchecked(index as usize) };
        if cfg!(debug_assertions) {
            let prev_dtz = atom_ref.swap(dtz, ordering);
            debug_assert!(prev_dtz == 0 || prev_dtz == dtz);
        } else {
            atom_ref.store(dtz, ordering);
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.mmap.flush()
    }
}

impl Tablebase for DtzTablebaseMut<'_> {
    type Item = u8;

    fn get(&self, index: u64) -> u8 {
        self.load(index, Ordering::Relaxed)
    }

    fn len(&self) -> u64 {
        self.slice.len() as u64
    }
}

pub struct DtzTablebaseMmap<'a> {
    _mmap: Mmap,
    slice: &'a [u8],
}

impl DtzTablebaseMmap<'_> {
    fn new(filename: &str, len: u64) -> io::Result<Self> {
        let (mmap, slice) = open_mmap(filename)?;
        assert_eq!(slice.len() as u64, len);
        Ok(Self { _mmap: mmap, slice })
    }
}

impl Tablebase for DtzTablebaseMmap<'_> {
    type Item = u8;

    fn get(&self, index: u64) -> u8 {
        debug_assert!(index < self.len());
        *unsafe { self.slice.get_unchecked(index as usize) }
    }

    fn len(&self) -> u64 {
        self.slice.len() as u64
    }
}


pub struct DtmTablebaseMut<'a> {
    mmap: MmapMut,
    slice: &'a [AtomicU16],
}

impl <'a> DtmTablebaseMut<'a> {
    pub fn new(filename: &str, len: u64) -> io::Result<Self> {
        let (mmap, slice) = open_mmap_mut(filename, len * 2)?;
        Ok(Self { mmap, slice })
    }

    pub fn init(&mut self) {
        par_memset(&mut self.mmap, 0);
    }

    pub fn load(&self, index: u64, ordering: Ordering) -> u16 {
        debug_assert!(index < self.len());
        let atom_ref: &AtomicU16 = unsafe { self.slice.get_unchecked(index as usize) };
        u16::from_le(atom_ref.load(ordering))
    }

    pub fn store(&self, index: u64, dtm: u16, ordering: Ordering) {
        debug_assert!(index < self.len());
        let atom_ref: &AtomicU16 = unsafe { self.slice.get_unchecked(index as usize) };
        if cfg!(debug_assertions) {
            let prev_dtm = u16::from_le(atom_ref.swap(dtm.to_le(), ordering));
            debug_assert!(prev_dtm == 0 || prev_dtm == dtm);
        } else {
            atom_ref.store(dtm.to_le(), ordering);
        }
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.mmap.flush()
    }
}

impl Tablebase for DtmTablebaseMut<'_> {
    type Item = u16;

    fn get(&self, index: u64) -> u16 {
        self.load(index, Ordering::Relaxed)
    }

    fn len(&self) -> u64 {
        self.slice.len() as u64
    }
}

pub struct DtmTablebaseMmap<'a> {
    _mmap: Mmap,
    slice: &'a [u16],
}

impl DtmTablebaseMmap<'_> {
    fn new(filename: &str, len: u64) -> io::Result<Self> {
        let (mmap, slice) = open_mmap(filename)?;
        assert_eq!(slice.len() as u64, len);
        Ok(Self { _mmap: mmap, slice })
    }
}

impl Tablebase for DtmTablebaseMmap<'_> {
    type Item = u16;

    fn get(&self, index: u64) -> u16 {
        debug_assert!(index < self.len());
        *unsafe { self.slice.get_unchecked(index as usize) }
    }

    fn len(&self) -> u64 {
        self.slice.len() as u64
    }
}
