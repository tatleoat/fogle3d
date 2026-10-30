// SPDX-License-Identifier: MIT
//! The sorting helpers of `sort.c` (0x0851ca00-0x0851d020) on the memory image: quicksorts of an
//! integer array (with an optional parallel array of pointers), of a float array and of an array of
//! C strings, each able to note the exchanges it makes in a list the caller reads back afterwards
//! (`SI_swapGet`) and frees (`SI_swapDispose`). Finding 0025, section "sort.c".
//!
//! The original's partitions walk their arrays without a bound check (a pivot that is not in
//! the range makes them run on); this port reads and writes the same words of the image, so
//! a case that overruns does so identically, up to the point where the original would fault: a scan
//! stops there (the image has recorded the fault) instead of running on for ever over zeros.

use si_mem::Mem;

si_mem::globals! {
    /// The list cursor of [`swap_get`].
    SWAP_CURR: u32 = 0x086dc2a0, "swapCurr";
    /// The last record of the list of noted exchanges.
    SWAP_EOL: u32 = 0x086dc2a4, "swapEol";
    /// The first record of the list of noted exchanges (12-byte records: first index, second
    /// index, next record).
    SWAP_LIST: u32 = 0x086dc2a8, "swapList";
}

/// The word `fld`/`fstp` of a float leaves: a signalling NaN comes back quiet.
fn quiet(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}

/// [soft 0x0851ca00 swapRecord]: appends the record (`a`, `b`, next 0) to the list of noted
/// exchanges (`SI_malloc(12)`; the first record also becomes the head).
pub fn swap_record(m: &mut Mem, a: i32, b: i32) {
    let r = m.si_malloc(12);
    m.set_i32(r, a);
    m.set_i32(r + 4, b);
    m.set_u32(r + 8, 0);
    if m.read(SWAP_LIST) == 0 {
        m.write(SWAP_EOL, r);
        m.write(SWAP_LIST, r);
    } else {
        let eol = m.read(SWAP_EOL);
        m.set_u32(eol + 8, r);
        m.write(SWAP_EOL, r);
    }
}

/// [soft 0x0851ca58 SI_swapGet]: steps the cursor to the next record (the head when there is
/// no cursor), stores its two indices through `out_a` and `out_b` and answers 1; 0 at the end
/// (the cursor is then null, so the next call starts again at the head).
pub fn swap_get(m: &mut Mem, out_a: u32, out_b: u32) -> bool {
    let cur = m.read(SWAP_CURR);
    let next = if cur != 0 { m.u32(cur + 8) } else { m.read(SWAP_LIST) };
    m.write(SWAP_CURR, next);
    if next == 0 {
        return false;
    }
    let a = m.u32(next);
    m.set_u32(out_a, a);
    let b = m.u32(next + 4);
    m.set_u32(out_b, b);
    true
}

/// [soft 0x0851caa8 SI_swapDispose]: frees every record of the list; cursor, tail and head
/// become null.
pub fn swap_dispose(m: &mut Mem) {
    loop {
        let head = m.read(SWAP_LIST);
        if head == 0 {
            break;
        }
        m.write(SWAP_CURR, head);
        let next = m.u32(head + 8);
        m.write(SWAP_LIST, next);
        m.free(head);
        m.write(SWAP_CURR, 0);
    }
    m.write(SWAP_CURR, 0);
    m.write(SWAP_EOL, 0);
    m.write(SWAP_LIST, 0);
}

/// [soft 0x0851cb0c iswap]: exchanges the integers at `a` and `b`, and the pointers at `pa`
/// and `pb` when both are non-null.
pub fn iswap(m: &mut Mem, a: u32, b: u32, pa: u32, pb: u32) {
    let t = m.u32(a);
    let v = m.u32(b);
    m.set_u32(a, v);
    m.set_u32(b, t);
    if pa != 0 && pb != 0 {
        let t = m.u32(pa);
        let v = m.u32(pb);
        m.set_u32(pa, v);
        m.set_u32(pb, t);
    }
}

/// [soft 0x0851cb40 ifindpivot]: the first index after `lo` (up to `hi`) whose integer is
/// above `arr[lo]`; `lo` when a smaller one comes first; -1 when all are equal.
pub fn ifindpivot(m: &Mem, arr: u32, lo: i32, hi: i32) -> i32 {
    let at = |i: i32| arr.wrapping_add((i as u32).wrapping_mul(4));
    let mut i = lo;
    loop {
        i = i.wrapping_add(1);
        if hi < i {
            return -1;
        }
        if m.i32(at(lo)) < m.i32(at(i)) {
            return i;
        }
        if m.i32(at(i)) < m.i32(at(lo)) {
            return lo;
        }
    }
}

/// [soft 0x0851cb84 ipartition]: partitions `arr[lo..=hi]` around `pivot` (values below it
/// first), exchanging the parallel pointer array `parr` (when non-null) with it, and notes each
/// exchange with [`swap_record`] when `note` is non-zero. Returns the first index of the upper part.
pub fn ipartition(m: &mut Mem, arr: u32, parr: u32, lo: i32, hi: i32, pivot: i32, note: i16) -> i32 {
    let (mut lo, mut hi) = (lo, hi);
    let at = |i: i32| arr.wrapping_add((i as u32).wrapping_mul(4));
    let pat = |i: i32| if parr == 0 { 0 } else { parr.wrapping_add((i as u32).wrapping_mul(4)) };
    loop {
        if note != 0 {
            swap_record(m, lo, hi);
        }
        iswap(m, at(lo), at(hi), pat(lo), pat(hi));
        while m.i32(at(lo)) < pivot && m.fault().is_none() {
            lo = lo.wrapping_add(1);
        }
        while pivot <= m.i32(at(hi)) && m.fault().is_none() {
            hi = hi.wrapping_sub(1);
        }
        if lo > hi {
            return lo;
        }
    }
}

/// [soft 0x0851cc7c SI_quicksort]: sorts the integers `arr[lo..=hi]` ascending (and `parr`
/// with them when non-null), noting the exchanges when `note` is non-zero.
pub fn quicksort(m: &mut Mem, arr: u32, parr: u32, lo: i32, hi: i32, note: i16) {
    let p = ifindpivot(m, arr, lo, hi);
    if p >= 0 && m.fault().is_none() {
        let pv = m.i32(arr.wrapping_add((p as u32).wrapping_mul(4)));
        let k = ipartition(m, arr, parr, lo, hi, pv, note);
        quicksort(m, arr, parr, lo, k.wrapping_sub(1), note);
        quicksort(m, arr, parr, k, hi, note);
    }
}

/// [soft 0x0851ccfc fswap]: exchanges the floats at `a` and `b` (through the x87 register: a
/// signalling NaN is stored quiet).
pub fn fswap(m: &mut Mem, a: u32, b: u32) {
    let t = quiet(m.u32(a));
    let v = quiet(m.u32(b));
    m.set_u32(a, v);
    m.set_u32(b, t);
}

/// [soft 0x0851cd14 ffindpivot]: as [`ifindpivot`] for floats (ordered compares: a NaN is
/// neither above nor below).
pub fn ffindpivot(m: &Mem, arr: u32, lo: i32, hi: i32) -> i32 {
    let at = |i: i32| arr.wrapping_add((i as u32).wrapping_mul(4));
    let pivot = m.f32(at(lo));
    let mut i = lo;
    loop {
        i = i.wrapping_add(1);
        if hi < i {
            return -1;
        }
        let v = m.f32(at(i));
        if v > pivot {
            return i;
        }
        if v < pivot {
            return lo;
        }
    }
}

/// [soft 0x0851cd68 fpartition]: as [`ipartition`] for floats; `pivot` is the float the caller
/// pushed.
pub fn fpartition(m: &mut Mem, arr: u32, lo: i32, hi: i32, pivot: f32, note: i16) -> i32 {
    let (mut lo, mut hi) = (lo, hi);
    let at = |i: i32| arr.wrapping_add((i as u32).wrapping_mul(4));
    loop {
        if note != 0 {
            swap_record(m, lo, hi);
        }
        fswap(m, at(lo), at(hi));
        while m.f32(at(lo)) < pivot && m.fault().is_none() {
            lo = lo.wrapping_add(1);
        }
        while pivot <= m.f32(at(hi)) && m.fault().is_none() {
            hi = hi.wrapping_sub(1);
        }
        if lo > hi {
            return lo;
        }
    }
}

/// [soft 0x0851ce1c SI_fquicksort]: sorts the floats `arr[lo..=hi]` ascending, noting the
/// exchanges when `note` is non-zero.
pub fn fquicksort(m: &mut Mem, arr: u32, lo: i32, hi: i32, note: i16) {
    let p = ffindpivot(m, arr, lo, hi);
    if p >= 0 && m.fault().is_none() {
        let pv = m.f32(arr.wrapping_add((p as u32).wrapping_mul(4)));
        let k = fpartition(m, arr, lo, hi, pv, note);
        fquicksort(m, arr, lo, k.wrapping_sub(1), note);
        fquicksort(m, arr, k, hi, note);
    }
}

/// [soft 0x0851ce8c sswap]: exchanges the string pointers at `a` and `b`.
pub fn sswap(m: &mut Mem, a: u32, b: u32) {
    let t = m.u32(a);
    let v = m.u32(b);
    m.set_u32(a, v);
    m.set_u32(b, t);
}

/// [soft 0x0851cea4 sfindpivot]: as [`ifindpivot`] for an array of C strings compared with
/// `strcmp`.
pub fn sfindpivot(m: &Mem, arr: u32, lo: i32, hi: i32) -> i32 {
    let at = |i: i32| arr.wrapping_add((i as u32).wrapping_mul(4));
    let pivot = m.u32(at(lo));
    let mut i = lo;
    loop {
        i = i.wrapping_add(1);
        if hi < i {
            return -1;
        }
        if si_mem::clib::strcmp(m, m.u32(at(i)), pivot) > 0 {
            return i;
        }
        if si_mem::clib::strcmp(m, m.u32(at(i)), pivot) < 0 {
            return lo;
        }
    }
}

/// [soft 0x0851cf18 spartition]: as [`ipartition`] for strings (no parallel array); `pivot` is
/// the pivot string's address.
pub fn spartition(m: &mut Mem, arr: u32, lo: i32, hi: i32, pivot: u32, note: i16) -> i32 {
    let (mut lo, mut hi) = (lo, hi);
    let at = |i: i32| arr.wrapping_add((i as u32).wrapping_mul(4));
    loop {
        if note != 0 {
            swap_record(m, lo, hi);
        }
        sswap(m, at(lo), at(hi));
        while si_mem::clib::strcmp(m, m.u32(at(lo)), pivot) < 0 && m.fault().is_none() {
            lo = lo.wrapping_add(1);
        }
        while si_mem::clib::strcmp(m, m.u32(at(hi)), pivot) >= 0 && m.fault().is_none() {
            hi = hi.wrapping_sub(1);
        }
        if lo > hi {
            return lo;
        }
    }
}

/// [soft 0x0851cfb8 SI_squicksort]: sorts the strings `arr[lo..=hi]` by `strcmp`, noting the
/// exchanges when `note` is non-zero.
pub fn squicksort(m: &mut Mem, arr: u32, lo: i32, hi: i32, note: i16) {
    let p = sfindpivot(m, arr, lo, hi);
    if p >= 0 && m.fault().is_none() {
        let pv = m.u32(arr.wrapping_add((p as u32).wrapping_mul(4)));
        let k = spartition(m, arr, lo, hi, pv, note);
        squicksort(m, arr, lo, k.wrapping_sub(1), note);
        squicksort(m, arr, k, hi, note);
    }
}
