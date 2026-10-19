
pub fn choose(n: u8, k: u8) -> u64 {
    if k > n { return 0; }
    let n = n as u64;
    let k = k as u64;
    let mut ret = 1;
    // Assumes k is small - this is how we use it
    for i in 0..k {
        ret *= n - i;
        ret /= i + 1;
    }
    ret
}

fn is_strictly_increasing(vs: &[u8]) -> bool {
    let mut i = 0;
    while i + 1 < vs.len() {
        if vs[i] >= vs[i + 1] {
            return false;
        }
        i += 1;
    }
    true
}

pub fn rank(vs: &[u8]) -> u64 {
    debug_assert!(is_strictly_increasing(vs));
    let mut ret = 0;
    for i in 0..vs.len() {
        ret += choose(vs[i], (i + 1) as u8);
    }
    ret
}

pub fn unrank(mut idx: u64, mut k: u8) -> Vec<u8> {
    let mut ret = Vec::with_capacity(k as usize);
    while k > 0 {
        if idx == 0 {
            ret.push(k - 1);
        } else {
            let mut v = 1;
            for n in (k + 1).. {
                let v_next = v * (n as u64) / (n as u64 - k as u64);
                if v_next > idx {
                    ret.push(n - 1);
                    idx -= v;
                    break;
                }
                v = v_next;
            }
        }
        k -= 1;
    }

    ret.reverse();
    ret
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp;

    #[test]
    fn test_choose_exhaustive() {
        const MAX_N: u8 = 20; // larger than this would overflow a 64 bit number
        let factorial = |n: u8| -> u64 {
            assert!(n <= MAX_N);
            let mut ret = 1;
            for i in 0..n {
                ret *= 1 + i as u64;
            }
            ret
        };
        for n in 0..=20 {
            for k in 0..=n {
                // Compare against the textbook definition of the choose function
                let expected = factorial(n) / (factorial(n - k) * factorial(k));
                assert_eq!(choose(n, k), expected);
            }
        }
    }

    #[test]
    fn test_rank_unrank_example() {
        assert_eq!(72, rank(&[0, 1, 3, 6, 8]));
        assert_eq!(unrank(72, 5), vec![0, 1, 3, 6, 8]);
    }

    #[test]
    fn test_rank_unrank_exhaustive() {
        const MAX_N: u8 = 40;
        const MAX_K: u8 = 4;

        for n in 1..=MAX_N {
            for k in 1..cmp::min(n, MAX_K) {
                let max_idx = choose(n, k);
                for i in 0..max_idx {
                    let vs = unrank(i, k);
                    assert_eq!(vs.len(), k as usize);
                    assert!(is_strictly_increasing(&vs));
                    assert!(vs[vs.len() - 1] < n);

                    assert_eq!(i, rank(&vs));
                }
            }
        }
    }
}
