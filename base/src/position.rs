use rand::{Rng, RngExt};
use rand::seq::SliceRandom;

use crate::moves::Move;
use crate::zobrist::ZobristAccumulator;
use crate::{Player, Piece, Outcome};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Cell {
    Empty, Occupied(Player, Piece),
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Board([Cell; 81]);

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Position {
    pub board: Board,
    pub active_player: Player,
    pub plies_since_capture: u8,
    pub total_pieces: u8,
    pub zobrist: ZobristAccumulator,
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct UndoCell {
    x: u8,
    y: u8,
    cell: Cell,
}

pub struct Undo {
    cells: [UndoCell; 2],
    plies_since_capture: u8,
}

impl Board {
    pub fn new() -> Board {
        Board([Cell::Empty; 81])
    }

    pub fn get(&self, x: u8, y: u8) -> Cell {
        debug_assert!(x < 9 && y < 9);
        self.0[x as usize + 9 * y as usize]
    }

    pub fn get_mut(&mut self, x: u8, y: u8) -> &mut Cell {
        debug_assert!(x < 9 && y < 9);
        &mut self.0[x as usize + 9 * y as usize]
    }

    pub fn from_fen(s: &str) -> Board {
        let mut board = Board::new();
        let mut y = 0;
        let mut x = 0;
        for ch in s.chars() {
            assert!(y < 9);
            match ch {
                'R' => {
                    assert!(x < 9);
                    *board.get_mut(x, y) = Cell::Occupied(Player::Blue, Piece::Rock);
                    x += 1;
                },
                'P' => {
                    assert!(x < 9);
                    *board.get_mut(x, y) = Cell::Occupied(Player::Blue, Piece::Paper);
                    x += 1;
                },
                'S' => {
                    assert!(x < 9);
                    *board.get_mut(x, y) = Cell::Occupied(Player::Blue, Piece::Scissors);
                    x += 1;
                },
                'r' => {
                    assert!(x < 9);
                    *board.get_mut(x, y) = Cell::Occupied(Player::Red, Piece::Rock);
                    x += 1;
                },
                'p' => {
                    assert!(x < 9);
                    *board.get_mut(x, y) = Cell::Occupied(Player::Red, Piece::Paper);
                    x += 1;
                },
                's' => {
                    assert!(x < 9);
                    *board.get_mut(x, y) = Cell::Occupied(Player::Red, Piece::Scissors);
                    x += 1;
                },
                '1' ..= '9' => {
                    x += ch as u8 - '0' as u8;
                },
                '/' => {
                    assert!(x == 9);
                    y += 1;
                    x = 0;
                },
                _ => panic!("Invalid FEN board character '{}'", ch),
            };
        }
        assert!(y == 8 && x == 9); // No trailing slash /
        board
    }

    pub fn to_fen(&self) -> String {
        let mut s = String::with_capacity(37);
        let mut empties = 0;
        for y in 0..9 {
            if y > 0 {
                s.push('/');
            }
            for x in 0..9 {
                match self.get(x, y) {
                    Cell::Empty => empties += 1,
                    Cell::Occupied(player, piece) => {
                        if empties > 0 {
                            s.push(('0' as u8 + empties) as char);
                            empties = 0;
                        }
                        let ch = match (player, piece) {
                            (Player::Blue, Piece::Rock) => 'R',
                            (Player::Blue, Piece::Paper) => 'P',
                            (Player::Blue, Piece::Scissors) => 'S',
                            (Player::Red, Piece::Rock) => 'r',
                            (Player::Red, Piece::Paper) => 'p',
                            (Player::Red, Piece::Scissors) => 's',
                        };
                        s.push(ch);
                    },
                }
            }
            if empties > 0 {
                s.push(('0' as u8 + empties) as char);
                empties = 0;
            }
        }
        s
    }
}

pub struct LegalMoves<'a> {
    position: &'a Position,
    x: u8,
    y: u8,
    i: u8,
}

impl<'a> Iterator for LegalMoves<'a> {
    type Item = Move;
    fn next(&mut self) -> Option<Move> {
        fn bump_x(x: &mut u8, y: &mut u8) {
            *x += 1;
            if *x >= 9 { *x = 0; *y += 1; }
        }

        while self.y < 9 {
            match self.position.board.get(self.x, self.y) {
                Cell::Empty => bump_x(&mut self.x, &mut self.y),
                Cell::Occupied(player, piece) => {
                    if player != self.position.active_player {
                        bump_x(&mut self.x, &mut self.y);
                    } else {
                        let to_xy = match self.i {
                            0 if self.x > 0 && self.y > 0 => Some((self.x - 1, self.y - 1)),
                            1 if               self.y > 0 => Some((self.x    , self.y - 1)),
                            2 if self.x < 8 && self.y > 0 => Some((self.x + 1, self.y - 1)),
                            3 if self.x > 0               => Some((self.x - 1, self.y    )),
                            4 if self.x < 8               => Some((self.x + 1, self.y    )),
                            5 if self.x > 0 && self.y < 8 => Some((self.x - 1, self.y + 1)),
                            6 if               self.y < 8 => Some((self.x    , self.y + 1)),
                            7 if self.x < 8 && self.y < 8 => Some((self.x + 1, self.y + 1)),
                            _ => None
                        };

                        let from_x = self.x;
                        let from_y = self.y;
                        self.i += 1;
                        if self.i >= 8 { self.i = 0; bump_x(&mut self.x, &mut self.y); }

                        if let Some((to_x, to_y)) = to_xy {
                            match self.position.board.get(to_x, to_y) {
                                Cell::Empty => return Some(Move { from_x, from_y, to_x, to_y, is_capture: false }),
                                Cell::Occupied(player2, piece2) => {
                                    if player2 != self.position.active_player && piece.can_capture(piece2) {
                                        return Some(Move { from_x, from_y, to_x, to_y, is_capture: true });
                                    }
                                }
                            };
                        }
                    }
                }
            }
        }
        None
    }
}

impl Position {
    pub fn initial() -> Position {
        let mut board = Board::new();
        for i in 0..3 {
            *board.get_mut(1 + i, 3 - i) = Cell::Occupied(Player::Blue, Piece::Rock);
            *board.get_mut(1 + i, 4 - i) = Cell::Occupied(Player::Blue, Piece::Paper);
            *board.get_mut(2 + i, 4 - i) = Cell::Occupied(Player::Blue, Piece::Scissors);
            *board.get_mut(5 + i, 7 - i) = Cell::Occupied(Player::Red, Piece::Rock);
            *board.get_mut(4 + i, 7 - i) = Cell::Occupied(Player::Red, Piece::Paper);
            *board.get_mut(4 + i, 6 - i) = Cell::Occupied(Player::Red, Piece::Scissors);
        }
        *board.get_mut(4, 1) = Cell::Occupied(Player::Blue, Piece::Paper);
        *board.get_mut(7, 4) = Cell::Occupied(Player::Red, Piece::Paper);

        Self::from_board_and_player(board, Player::Blue)
    }

    pub fn randomized_initial<R: Rng + ?Sized>(rng: &mut R) -> Position {
        let mut board = Board::new();
        // Same initial pieces in the same initial spots, but which piece is in which slot is
        // different
        let mut pieces = [Piece::Rock, Piece::Rock, Piece::Rock, Piece::Paper, Piece::Paper,
        Piece::Paper, Piece::Paper, Piece::Scissors, Piece::Scissors, Piece::Scissors];
        pieces.shuffle(rng);
        // If true then mirrored along the diagonal, otherwise rotated 180 degrees. Either of these
        // keeps the game perfectly symmetrical (should be fair)
        let diag_mirror = rng.random_bool(0.5);
        let xys = [
            (3, 1), (2, 2), (1, 3), (4, 1), (3, 2), (2, 3), (1, 4), (4, 2), (3, 3), (2, 4)];

        for (i, (x, y)) in xys.iter().cloned().enumerate() {
            let piece = pieces[i];
            *board.get_mut(x, y) = Cell::Occupied(Player::Blue, piece);

            let (red_x, red_y) = if diag_mirror {
                (8 - y, 8 - x)
            } else {
                (8 - x, 8 - y)
            };
            *board.get_mut(red_x, red_y) = Cell::Occupied(Player::Red, piece);
        }

        Self::from_board_and_player(board, Player::Blue)
    }

    pub fn parse_rpsi_position(s: &str) -> Position {
        let mut tokens = s.split(' ');
        assert!(tokens.next().unwrap() == "position");
        assert!(tokens.next().unwrap() == "fen");
        let board = Board::from_fen(tokens.next().unwrap());
        let active_player = match tokens.next().unwrap() {
            "b" => Player::Blue,
            "r" => Player::Red,
            ch => panic!("Invalid active player '{}'", ch),
        };
        tokens.next().unwrap(); // Skip territory for intransitive

        let mut position = Self::from_board_and_player(board, active_player);

        match tokens.next() {
            Some("moves") => {
                for token in tokens {
                    position.make_move(Move::parse(token));
                }
            },
            None => (),
            _ => panic!("Expected move list or nothing"),
        }

        position
    }

    pub fn from_fen(s: &str) -> Position {
        let mut tokens = s.split(' ');
        let board = Board::from_fen(tokens.next().unwrap());
        let active_player = match tokens.next().unwrap() {
            "b" => Player::Blue,
            "r" => Player::Red,
            ch => panic!("Invalid active player '{}'", ch),
        };
        assert!(tokens.next().is_none());
        Self::from_board_and_player(board, active_player)
    }

    pub fn from_board_and_player(board: Board, active_player: Player) -> Position {
        let plies_since_capture = 0;
        let zobrist = Self::calc_zobrist(&board, active_player);
        let total_pieces = Self::calc_total_pieces(&board);
        Position { board, active_player, plies_since_capture, total_pieces, zobrist }
    }


    pub fn legal_moves(&self) -> LegalMoves<'_> {
        LegalMoves { position: &self, x: 0, y: 0, i: 0 }
    }

    pub fn is_legal_move(&self, m: Move) -> bool {
        if let Cell::Occupied(player, piece) = self.board.get(m.from_x, m.from_y) {
            if player != self.active_player { return false; }
            if let Cell::Occupied(player2, piece2) = self.board.get(m.to_x, m.to_y) {
                if player2 == player {
                    return false;
                } else if !piece.can_capture(piece2) {
                    return false;
                }
            }
        } else {
            return false;
        }
        let dx = m.from_x as i8 - m.to_x as i8;
        let dy = m.from_y as i8 - m.to_y as i8;
        dx >= -1 && dx <= 1 && dy >= -1 && dy <= 1
    }

    // returns (x, y)
    pub fn corner_goal(player: Player) -> (u8, u8) {
        match player {
            Player::Blue => (8, 8),
            Player::Red => (0, 0),
        }
    }

    pub fn get_outcome(&self) -> Option<Outcome> {
        if let Cell::Occupied(Player::Red, _) = self.board.get(0, 0) {
            Some(Outcome::Win(Player::Red))
        } else if let Cell::Occupied(Player::Blue, _) = self.board.get(8, 8) {
            Some(Outcome::Win(Player::Blue))
        } else if self.legal_moves().next().is_none() {
            Some(Outcome::Win(self.active_player.invert()))
        } else if self.plies_since_capture >= 200 {
            Some(Outcome::Draw)
        } else {
            None
        }
    }

    pub fn make_move(&mut self, m: Move) -> Undo {
        debug_assert!(self.is_legal_move(m));
        let plies_since_capture = self.plies_since_capture;
        let cell1 = UndoCell {
            x: m.from_x,
            y: m.from_y,
            cell: self.board.get(m.from_x, m.from_y),
        };
        let cell2 = UndoCell {
            x: m.to_x,
            y: m.to_y,
            cell: self.board.get(m.to_x, m.to_y),
        };
        if m.is_capture {
            self.plies_since_capture = 0;
        } else {
            self.plies_since_capture += 1;
        }
        self.active_player = self.active_player.invert();
        self.zobrist.xor_player();

        if let Cell::Occupied(player, piece) = self.board.get(m.to_x, m.to_y) {
            self.total_pieces -= 1;
            self.zobrist.xor_piece(m.to_x, m.to_y, player, piece);
        }
        if let Cell::Occupied(player, piece) = self.board.get(m.from_x, m.from_y) {
            self.zobrist.xor_piece(m.from_x, m.from_y, player, piece);
            self.zobrist.xor_piece(m.to_x, m.to_y, player, piece);
        } else {
            panic!("illegal move");
        }

        *self.board.get_mut(m.to_x, m.to_y) = self.board.get(m.from_x, m.from_y);
        *self.board.get_mut(m.from_x, m.from_y) = Cell::Empty;

        Undo {
            cells: [cell1, cell2],
            plies_since_capture,
        }
    }

    pub fn undo_move(&mut self, undo: Undo) {
        self.plies_since_capture = undo.plies_since_capture;
        self.active_player = self.active_player.invert();
        self.zobrist.xor_player();

        for &UndoCell { x, y, cell } in &undo.cells {
            if let Cell::Occupied(player, piece) = self.board.get(x, y) {
                self.total_pieces -= 1;
                self.zobrist.xor_piece(x, y, player, piece);
            }
            if let Cell::Occupied(player, piece) = cell {
                self.total_pieces += 1;
                self.zobrist.xor_piece(x, y, player, piece);
            }

            *self.board.get_mut(x, y) = cell;
        }
    }

    fn calc_zobrist(board: &Board, active_player: Player) -> ZobristAccumulator {
        let mut zobrist = ZobristAccumulator::new();
        for y in 0..9 {
            for x in 0..9 {
                if let Cell::Occupied(player, piece) = board.get(x, y) {
                    zobrist.xor_piece(x, y, player, piece);
                }
            }
        }
        if active_player == Player::Red {
            zobrist.xor_player();
        }
        zobrist
    }

    fn calc_total_pieces(board: &Board) -> u8 {
        let mut cnt = 0;
        for y in 0..9 {
            for x in 0..9 {
                if let Cell::Occupied(_, _) = board.get(x, y) {
                    cnt += 1;
                }
            }
        }
        cnt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial() {
        let true_fen = "9/3RP4/2RPS4/1RPS5/1PS3sp1/5spr1/4spr2/4pr3/9";
        let calc_fen = Position::initial().board.to_fen();
        assert_eq!(true_fen, calc_fen);
    }

    #[test]
    fn test_board_fen_round_trip() {
        let fen1 = "9/3RP4/2RPS4/1RPS5/1PS3sp1/5spr1/4spr2/4pr3/9";
        let board = Board::from_fen(fen1);
        let fen2 = board.to_fen();
        assert_eq!(fen1, fen2);
    }
}
