use std::{fmt, ops};

// Representation of either a Win, Draw, or Loss, and how many moves until the next capture if
// a win or loss

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct WDLZ(i8);

mod wdlz_consts {
    pub const INVALID: i8 = i8::MIN;
    pub const DRAW: i8 = 0;
    pub const WIN: i8 = i8::MAX;
    pub const WIN_IN_MAX_PLY: i8 = 1;
    pub const LOSS: i8 = -WIN;
    pub const LOSS_IN_MAX_PLY: i8 = -1;
}

impl WDLZ {
    pub fn invalid() -> Self { Self(wdlz_consts::INVALID) }
    pub fn is_valid(&self) -> bool { self.0 != wdlz_consts::INVALID }

    pub fn draw() -> Self { Self(wdlz_consts::DRAW) }

    pub fn win() -> Self { Self(wdlz_consts::WIN) }
    pub fn win_in(ply: u8) -> Self {
        assert!(ply < wdlz_consts::WIN as u8);
        Self(wdlz_consts::WIN - ply as i8)
    }
    pub fn is_win(&self) -> bool {
        debug_assert!(self.is_valid());
        self.0 > 0
    }
    pub fn loss() -> Self { Self(wdlz_consts::LOSS) }
    pub fn loss_in(ply: u8) -> Self {
        assert!(ply < wdlz_consts::WIN as u8);
        Self(wdlz_consts::LOSS + ply as i8)
    }
    pub fn is_loss(&self) -> bool {
        debug_assert!(self.is_valid());
        self.0 < 0
    }

    pub fn add_ply(&self) -> Self {
        debug_assert!(self.is_valid());
        const WIN_IN_ONE_LESS_THEN_MAX_PLY: i8 = wdlz_consts::WIN_IN_MAX_PLY + 1;
        match self.0 {
            wdlz_consts::INVALID => panic!("invalid"),
            wdlz_consts::LOSS..wdlz_consts::LOSS_IN_MAX_PLY => Self(self.0 + 1),
            wdlz_consts::LOSS_IN_MAX_PLY => panic!("too many plies"),
            wdlz_consts::DRAW => Self(self.0),
            wdlz_consts::WIN_IN_MAX_PLY => panic!("too many plies"),
            WIN_IN_ONE_LESS_THEN_MAX_PLY..=wdlz_consts::WIN => Self(self.0 - 1),
        }
    }
}

impl ops::Neg for WDLZ {
    type Output = Self;
    fn neg(self) -> Self {
        debug_assert!(self.is_valid());
        Self(-self.0)
    }
}

impl fmt::Display for WDLZ {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            wdlz_consts::INVALID => write!(f, "!?"),
            wdlz_consts::LOSS..=wdlz_consts::LOSS_IN_MAX_PLY => {
                write!(f, "L{}", self.0 - wdlz_consts::LOSS)
            },
            wdlz_consts::DRAW => write!(f, "D"),
            wdlz_consts::WIN_IN_MAX_PLY..=wdlz_consts::WIN => {
                write!(f, "W{}", wdlz_consts::WIN - self.0)
            },
        }
    }
}
