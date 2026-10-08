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
        write!(f, "{}", x_char(self.from_x))?;
        write!(f, "{}", y_char(self.from_y))?;
        write!(f, "{}", if self.is_capture { 'x' } else { '-' })?;
        write!(f, "{}", x_char(self.to_x))?;
        write!(f, "{}", y_char(self.to_y))
    }
}

impl Move {
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
        Move { from_x, from_y, to_x, to_y, is_capture }
    }
}
