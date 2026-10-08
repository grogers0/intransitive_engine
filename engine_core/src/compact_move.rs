use crate::moves::Move;

/*

A compact move is encoded in 16 bits:

[from_x=4 bits][from_y=4 bits][delta_x=2 bits][delta_y=2 bits][is_capture=1 bit][unused=3 bits]

Where delta_x (and delta_y) are the offset between from_x and to_x, stored as:

0: to_x = from_x - 1
1: to_x = from_x
2: to_x = from_x + 1
3: unused

i.e. delta_x = to_x + 1 - from_x
*/

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CompactMove([u8; 2]);

impl From<Move> for CompactMove {
    fn from(m: Move) -> CompactMove {
        let mut ret = CompactMove([0u8; 2]);
        ret.0[0] |= m.from_x;
        ret.0[0] |= m.from_y << 4;
        debug_assert!(m.to_x + 1 - m.from_x <= 2);
        debug_assert!(m.to_y + 1 - m.from_y <= 2);
        ret.0[1] |= m.to_x + 1 - m.from_x;
        ret.0[1] |= (m.to_y + 1 - m.from_y) << 2;
        if m.is_capture {
            ret.0[1] |= 1 << 4;
        }
        ret
    }
}

impl From<CompactMove> for Move {
    fn from(cm: CompactMove) -> Move {
        let from_x = cm.0[0] & 0x0f;
        let from_y = cm.0[0] >> 4;
        let to_x = from_x + (cm.0[1] & 0x03) - 1;
        let to_y = from_y + ((cm.0[1] >> 2) & 0x03) - 1;
        let is_capture = (cm.0[1] & 0x10) != 0;
        Move { from_x, from_y, to_x, to_y, is_capture }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_valid_round_trip() {
        for from_x in 0..9 {
            for from_y in 0..9 {
                let min_x = if from_x > 0 { from_x - 1 } else { 0 };
                let max_x = if from_x < 8 { from_x + 1 } else { 8 };
                let min_y = if from_y > 0 { from_y - 1 } else { 0 };
                let max_y = if from_y < 8 { from_y + 1 } else { 8 };

                for to_x in min_x..=max_x {
                    for to_y in min_y..=max_y {
                        for is_capture in [false, true] {
                            check_round_trip(Move { from_x, from_y, to_x, to_y, is_capture });
                        }
                    }
                }
            }
        }
    }

    fn check_round_trip(m: Move) {
        let cm = CompactMove::from(m);
        let m2 = Move::from(cm);
        assert_eq!(m, m2);
    }
}
