use std::{fmt, ops};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct Value(pub i16);

pub const MAX_PLY: u8 = 255;

mod value_consts {
    pub const INVALID: i16 = 32_002;
    pub const INFINITY: i16 = 32_001;
    pub const NEG_INFINITY: i16 = -32_001;
    pub const WIN: i16 = 32_000;
    pub const WIN_IN_MAX_PLY: i16 = WIN - (super::MAX_PLY as i16);
    pub const LOSS: i16 = -WIN;
    pub const LOSS_IN_ONE_PLY: i16 = LOSS + 1;
    pub const LOSS_IN_MAX_PLY: i16 = LOSS + (super::MAX_PLY as i16);
    pub const DRAW: i16 = 0;
}

impl Value {
    pub fn invalid() -> Value { Value(value_consts::INVALID) }
    pub fn infinity() -> Value { Value(value_consts::INFINITY) }
    pub fn draw() -> Value { Value(value_consts::DRAW) }
    pub fn win() -> Value { Value(value_consts::WIN) }
    pub fn win_in(ply: u8) -> Value { Value(value_consts::WIN - (ply as i16)) }
    pub fn loss() -> Value { Value(value_consts::LOSS) }
    pub fn loss_in(ply: u8) -> Value { Value(value_consts::LOSS + (ply as i16)) }

    pub fn is_valid(self) -> bool {
        self.0 != value_consts::INVALID
    }

    pub fn add_ply(self) -> Self {
        if self.is_win() {
            debug_assert!(self.0 != value_consts::WIN_IN_MAX_PLY);
            Self(self.0 - 1)
        } else if self.is_loss() {
            debug_assert!(self.0 != value_consts::LOSS_IN_MAX_PLY);
            Self(self.0 + 1)
        } else {
            self
        }
    }

    pub fn is_win(self) -> bool {
        debug_assert!(self.is_valid());
        self.0 >= value_consts::WIN_IN_MAX_PLY
    }

    pub fn is_loss(self) -> bool {
        debug_assert!(self.is_valid());
        self.0 <= value_consts::LOSS_IN_MAX_PLY
    }

    pub fn is_win_or_loss(self) -> bool {
        self.is_win() || self.is_loss()
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            value_consts::INVALID => write!(f, "invalid"),
            value_consts::INFINITY => write!(f, "infinity"),
            value_consts::NEG_INFINITY => write!(f, "-infinity"),
            value_consts::WIN => write!(f, "win"),
            value_consts::WIN_IN_MAX_PLY..value_consts::WIN  => {
                write!(f, "win in {} ply", value_consts::WIN - self.0)
            },
            value_consts::LOSS => write!(f, "loss"),
            value_consts::LOSS_IN_ONE_PLY..=value_consts::LOSS_IN_MAX_PLY  => {
                write!(f, "loss in {} ply", self.0 - value_consts::LOSS)
            },
            v => write!(f, "{}", v)
        }
    }
}

impl ops::Neg for Value {
    type Output = Self;
    fn neg(self) -> Self {
        debug_assert!(self.is_valid());
        Self(-self.0)
    }
}

impl ops::AddAssign for Value {
    fn add_assign(&mut self, other: Self) {
        debug_assert!(self.is_valid() && other.is_valid());
        self.0 = self.0 + other.0;
        debug_assert!(!self.is_win_or_loss());
    }
}
