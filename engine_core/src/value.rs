use std::{fmt, ops};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct Value(pub i16);

pub const MAX_PLY: u8 = 255;

mod ValueConsts {
    pub const UNDEFINED: i16 = 32_002;
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
    pub fn undef() -> Value { Value(ValueConsts::UNDEFINED) }
    pub fn infinity() -> Value { Value(ValueConsts::INFINITY) }
    pub fn draw() -> Value { Value(ValueConsts::DRAW) }
    pub fn win() -> Value { Value(ValueConsts::WIN) }
    pub fn win_in(ply: u8) -> Value { Value(ValueConsts::WIN - (ply as i16)) }
    pub fn loss() -> Value { Value(ValueConsts::LOSS) }
    pub fn loss_in(ply: u8) -> Value { Value(ValueConsts::LOSS + (ply as i16)) }

    pub fn is_valid(self) -> bool {
        self.0 != ValueConsts::UNDEFINED
    }

    pub fn add_ply(self) -> Self {
        if self.is_win() {
            debug_assert!(self.0 != ValueConsts::WIN_IN_MAX_PLY);
            Self(self.0 - 1)
        } else if self.is_loss() {
            debug_assert!(self.0 != ValueConsts::LOSS_IN_MAX_PLY);
            Self(self.0 + 1)
        } else {
            self
        }
    }

    pub fn is_win(self) -> bool {
        debug_assert!(self.is_valid());
        self.0 >= ValueConsts::WIN_IN_MAX_PLY
    }

    pub fn is_loss(self) -> bool {
        debug_assert!(self.is_valid());
        self.0 <= ValueConsts::LOSS_IN_MAX_PLY
    }

    pub fn is_win_or_loss(self) -> bool {
        self.is_win() || self.is_loss()
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            ValueConsts::UNDEFINED => write!(f, "undefined"),
            ValueConsts::INFINITY => write!(f, "infinity"),
            ValueConsts::NEG_INFINITY => write!(f, "-infinity"),
            ValueConsts::WIN => write!(f, "win"),
            ValueConsts::WIN_IN_MAX_PLY..ValueConsts::WIN  => {
                write!(f, "win in {} ply", ValueConsts::WIN - self.0)
            },
            ValueConsts::LOSS => write!(f, "loss"),
            ValueConsts::LOSS_IN_ONE_PLY..=ValueConsts::LOSS_IN_MAX_PLY  => {
                write!(f, "loss in {} ply", self.0 - ValueConsts::LOSS)
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
