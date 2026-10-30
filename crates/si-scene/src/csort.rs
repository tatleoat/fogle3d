// SPDX-License-Identifier: MIT
//! The C library's `qsort` as the original's sorts meet it.
//!
//! The program is linked dynamically against the C library, so this is not read from the
//! binary: it is glibc's `qsort` as its sources of the time have it (`stdlib/msort.c`,
//! versions 2.1 to 2.3). For an array that needs less than 1024 bytes of temporary space,
//! and for a larger one as long as memory can be had, that is a top-down merge sort with
//! the routine below; only when the temporary array cannot be allocated does it fall back
//! to a quicksort.
//!
//! It matters because two of the original's comparison functions are not symmetric (the
//! element comparison answers "after" for one order of two equal names and "equal" for the
//! other): the result then depends on which comparisons the sort makes and in which order.

/// Sorts `v` with `cmp` exactly as glibc's merge sort does: the two halves (the first of
/// `n / 2` entries) are sorted, then merged taking from the first half whenever
/// `cmp(first, second) <= 0`.
pub fn msort<T: Copy>(v: &mut [T], cmp: &mut dyn FnMut(&T, &T) -> i32) {
    let n = v.len();
    if n <= 1 {
        return;
    }
    let n1 = n / 2;
    {
        let (a, b) = v.split_at_mut(n1);
        msort(a, cmp);
        msort(b, cmp);
    }
    let mut tmp: Vec<T> = Vec::with_capacity(n);
    let (mut i, mut j) = (0usize, n1);
    while i < n1 && j < n {
        if cmp(&v[i], &v[j]) <= 0 {
            tmp.push(v[i]);
            i += 1;
        } else {
            tmp.push(v[j]);
            j += 1;
        }
    }
    // what is left of the first half is moved up; what is left of the second half is in place
    tmp.extend_from_slice(&v[i..n1]);
    let k = tmp.len();
    v[..k].copy_from_slice(&tmp);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_and_keeps_equal_entries_in_order() {
        let mut v = [(3, 'a'), (1, 'b'), (3, 'c'), (2, 'd'), (1, 'e')];
        msort(&mut v, &mut |a, b| a.0 - b.0);
        assert_eq!(v, [(1, 'b'), (1, 'e'), (2, 'd'), (3, 'a'), (3, 'c')]);
    }

    #[test]
    fn a_one_sided_comparison_decides_by_the_order_of_the_calls() {
        // "x is after y" is reported only when x is the first argument
        let mut v = ['x', 'y'];
        msort(&mut v, &mut |a, b| (*a == 'x' && *b == 'y') as i32);
        assert_eq!(v, ['y', 'x']);
        let mut v = ['y', 'x'];
        msort(&mut v, &mut |a, b| (*a == 'x' && *b == 'y') as i32);
        assert_eq!(v, ['y', 'x']);
    }
}
