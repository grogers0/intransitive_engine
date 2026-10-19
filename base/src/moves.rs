use std::fmt;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Move {
    pub from_x: u8,
    pub from_y: u8,
    pub to_x: u8,
    pub to_y: u8,
    pub is_capture: bool,
}

fn x_char(x: u8) -> char {
    ('a' as u8 + x) as char
}

fn y_char(y: u8) -> char {
    ('1' as u8 + y) as char
}

fn parse_x(ch: char) -> u8 {
    assert!(ch >= 'a' && ch <= 'i');
    ch as u8 - 'a' as u8
}

fn parse_y(ch: char) -> u8 {
    assert!(ch >= '1' && ch <= '9');
    ch as u8 - '1' as u8
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.is_valid() {
            write!(f, "{}", x_char(self.from_x))?;
            write!(f, "{}", y_char(self.from_y))?;
            write!(f, "{}", if self.is_capture { 'x' } else { '-' })?;
            write!(f, "{}", x_char(self.to_x))?;
            write!(f, "{}", y_char(self.to_y))
        } else {
            write!(f, "invalid")
        }
    }
}

impl Move {
    pub fn invalid() -> Move {
        Move {
            from_x: 9, from_y: 9, to_x: 9, to_y: 9, is_capture: false,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.from_x < 9 && self.from_y < 9 && self.to_x < 9 && self.to_y < 9 &&
            (self.from_x != self.to_x || self.from_y != self.to_y) &&
            (self.to_x + 1 - self.from_x) <= 2 && (self.to_y + 1 - self.from_y) <= 2
    }

    pub fn parse(s: &str) -> Move {
        let mut chars = s.chars().peekable();
        if let 'R' | 'P' | 'S' = chars.peek().unwrap() { chars.next(); }
        let from_x = parse_x(chars.next().unwrap());
        let from_y = parse_y(chars.next().unwrap());
        let is_capture = match chars.next().unwrap() {
            'x' => true,
            '-' => false,
            _ => panic!(),
        };
        if let 'R' | 'P' | 'S' = chars.peek().unwrap() { chars.next(); }
        let to_x = parse_x(chars.next().unwrap());
        let to_y = parse_y(chars.next().unwrap());
        let m = Move { from_x, from_y, to_x, to_y, is_capture };
        debug_assert!(m.is_valid());
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_valid_moves() -> Vec<Move> {
        let mut ret = Vec::new();
        for from_x in 0..9 {
            for from_y in 0..9 {
                let min_x = if from_x > 0 { from_x - 1 } else { 0 };
                let max_x = if from_x < 8 { from_x + 1 } else { 8 };
                let min_y = if from_y > 0 { from_y - 1 } else { 0 };
                let max_y = if from_y < 8 { from_y + 1 } else { 8 };

                for to_x in min_x..=max_x {
                    for to_y in min_y..=max_y {
                        if to_x == from_x && to_y == from_y { continue; }
                        for is_capture in [false, true] {
                            ret.push(Move { from_x, from_y, to_x, to_y, is_capture });
                        }
                    }
                }
            }
        }
        ret
    }

    #[test]
    fn test_all_valid_are_valid() {
        for m in all_valid_moves() {
            assert!(m.is_valid());
        }
    }

    #[test]
    fn test_invalid_is_not_valid() {
        assert!(!Move::invalid().is_valid());
    }
}
