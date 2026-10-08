use crate::{Player, Piece};
use crate::position::{Cell, Board, Position};
use crate::zobrist::ZobristAccumulator;

/* 

The compact position is encoded in 256 bits:

[board_state=243 bits][active_player=1 bit][unused=4 bits][plies_since_capture=8 bits]

where board_state is 81x3 bits representing each cell going from top to bottom left to right. E.g. the indices look like this:

9 | 72 73 74 75 76 77 78 79 80
8 | 63 64 65 66 67 68 69 70 71
7 | 54 55 56 57 58 59 60 61 62
6 | 45 46 47 48 49 50 51 52 53
5 | 36 37 38 39 40 41 42 43 44
4 | 27 28 29 30 31 32 33 34 35
3 | 18 19 20 21 22 23 24 25 26
2 |  9 10 11 12 13 14 15 16 17
1 |  0  1  2  3  4  5  6  7  8
--+---------------------------
  |  a  b  c  d  e  f  g  h  i

The 3 bits representing each cell corresponds to:

0: empty
1: blue rock
2: blue paper
3: blue scissors
4: red rock
5: red paper
6: red scissors
7: unused

if active_player == 0, then blue is to move, otherwise if 1 then red is to move

*/

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompactPosition([u8; 32]);

impl From<&Position> for CompactPosition {
    fn from(position: &Position) -> CompactPosition {
        let mut ret = CompactPosition([0u8; 32]);


        let mut bit_idx = 0;
        let mut byte_idx = 0;
        for y in 0..9 {
            for x in 0..9 {
                let mut cell: u8 = match position.board.get(x, y) {
                    Cell::Empty => 0,
                    Cell::Occupied(Player::Blue, Piece::Rock) => 1,
                    Cell::Occupied(Player::Blue, Piece::Paper) => 2,
                    Cell::Occupied(Player::Blue, Piece::Scissors) => 3,
                    Cell::Occupied(Player::Red, Piece::Rock) => 4,
                    Cell::Occupied(Player::Red, Piece::Paper) => 5,
                    Cell::Occupied(Player::Red, Piece::Scissors) => 6,
                };
                let mut consumed = 0;
                if bit_idx > 5 {
                    if bit_idx != 8 {
                        ret.0[byte_idx] |= cell << bit_idx;
                    }
                    consumed = 8 - bit_idx;
                    cell >>= consumed;
                    byte_idx += 1;
                    bit_idx = 0;
                }
                ret.0[byte_idx] |= cell << bit_idx;
                bit_idx += 3 - consumed;
            }
        }
        assert!(byte_idx == 30 && bit_idx == 3);
        if position.active_player == Player::Red {
            ret.0[byte_idx] |= 1 << bit_idx;
        }
        byte_idx += 1;
        ret.0[byte_idx] = position.plies_since_capture;
        ret
    }
}

impl From<&CompactPosition> for Position {
    fn from(cp: &CompactPosition) -> Position {
        let mut board = Board::new();
        let mut zobrist = ZobristAccumulator::new();
        let mut set_cell = |x: u8, y: u8, player: Player, piece: Piece| {
            *board.get_mut(x, y) = Cell::Occupied(player, piece);
            zobrist.xor_piece(x, y, player, piece);
        };

        let mut bit_idx = 0;
        let mut byte_idx = 0;
        for y in 0..9 {
            for x in 0..9 {
                let mut cell = 0;
                let mut consumed = 0;
                if bit_idx > 5 {
                    if bit_idx != 8 {
                        cell = cp.0[byte_idx] >> bit_idx;
                    }
                    consumed = 8 - bit_idx;
                    byte_idx += 1;
                    bit_idx = 0;
                }
                let mask = (1 << (3 - consumed)) - 1;
                cell |= ((cp.0[byte_idx] >> bit_idx) & mask) << consumed;
                bit_idx += 3 - consumed;

                match cell {
                    0 => (),
                    1 => set_cell(x, y, Player::Blue, Piece::Rock),
                    2 => set_cell(x, y, Player::Blue, Piece::Paper),
                    3 => set_cell(x, y, Player::Blue, Piece::Scissors),
                    4 => set_cell(x, y, Player::Red, Piece::Rock),
                    5 => set_cell(x, y, Player::Red, Piece::Paper),
                    6 => set_cell(x, y, Player::Red, Piece::Scissors),
                    7 => panic!(),
                    _ => unreachable!(),
                };
            }
        }
        assert!(byte_idx == 30 && bit_idx == 3);
        let mut active_player = Player::Blue;
        if (cp.0[byte_idx] & (1 << bit_idx)) != 0 {
            active_player = Player::Red;
            zobrist.xor_player();
        }
        byte_idx += 1;
        let plies_since_capture = cp.0[byte_idx];
        Position { board, active_player, plies_since_capture, zobrist }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::Outcome;

    #[test]
    fn test_initial() {
        let pos = Position::initial();
        let cp = CompactPosition::from(&pos);
        let pos2 = Position::from(&cp);
        assert_eq!(pos, pos2);
    }

    #[test]
    fn test_random() {
        for _ in 0..1000 {
            test_random_once();
        }
    }

    fn test_random_once() {
        let mut pos = Position::initial();
        let num_moves = rand::random_range(1..50);
        for _ in 0..num_moves {
            let moves = pos.legal_moves().collect::<Vec<_>>();
            if moves.is_empty() {
                break;
            } else if let Some(Outcome::Win(_)) = pos.get_outcome() {
                break;
            }
            let idx = rand::random_range(0..moves.len());
            pos.make_move(moves[idx]);
        }
        let cp = CompactPosition::from(&pos);
        let pos2 = Position::from(&cp);
        assert_eq!(pos, pos2);

    }
}
