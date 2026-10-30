// SPDX-License-Identifier: MIT
//! General m × n matrices of doubles: the original's `matrixGenDbl.c` (finding 0008).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. The arithmetic is on doubles, so no operation here can be written
//! with plain `f64` operators: the x87 unit rounds every result to 64 bits first and to 53
//! bits only when it is stored, and rounding twice is not rounding once. Every step goes
//! through [`F80`] and is rounded with `to_f64()` exactly where the original stores. What
//! decides the bits of the sums is where the running value lives: in memory (loaded, added
//! to and stored again in every iteration: rounded every time) or in a register across the
//! loop (rounded once, or never). Each function says which.
//!
//! Exactness classes as in the other files of the crate: **copy** (only moves values) and
//! **model** (bit-exact given the x87 model of [`crate::x87`]). Nothing here calls the C
//! library's mathematics.
//!
//! # The matrix type
//!
//! `SI_GenMatrixDbl` is 12 bytes: `long rows` at offset 0, `long columns` at offset 4 and at
//! offset 8 a pointer to a table of `rows` pointers, each to a separately allocated row of
//! `columns` doubles. Element (i, j) is `m->row[i][j]`: row-major, through one indirection
//! per row. [`GenMatrixDbl`] keeps the two counts and one `Vec<f64>`, rows one after
//! another.
//!
//! Only the dispose, copy and print routines check a matrix pointer, and no function checks
//! that the dimensions of its operands fit: each takes its loop bounds from one operand
//! (said in its comment) and indexes the others with them. Where an operand is too small
//! the original reads or writes past a heap block; here the element accessors panic.
//! That, and the content of memory nobody has written (see [`general_matrix_allocate_dbl`]),
//! is where this port must differ from the original.

// the index loops are the original's
#![allow(clippy::needless_range_loop)]

use crate::text::printf_f;
use crate::x87::{fmov64 as fmovd, x, F80};

/// The determinant below which [`general_matrix_inverse2_dbl`] (0x08662548) and
/// [`general_matrix_inverse3_dbl`] (0x08662550) refuse: the double 0.0001
/// (0x3f1a36e2eb1c432d), compared with an unrounded 80-bit determinant.
pub const INVERSE_EPS: f64 = 1e-4;
/// The squared length above which [`vtn_inv_dbl`] divides: the double 1e-10 (0x08662558,
/// 0x3ddb7cdfd9d7bdbb).
pub const VTN_INV_EPS: f64 = 1e-10;
/// The factor of the "is this column new" test of
/// [`general_matrix_greville_inverse_dbl`]: the double 0.001 (0x08662560,
/// 0x3f50624dd2f1a9fc).
pub const GREVILLE_EPS: f64 = 0.001;

/// `SI_GenMatrixDbl`: `rows` × `cols` doubles, element (i, j) at `data[i * cols + j]`.
///
/// Do not change `rows` or `cols` of a matrix after it is made: here they fix the layout of
/// `data` (in the original they are only loop bounds, the rows being separate blocks).
#[derive(Clone, Debug, PartialEq)]
pub struct GenMatrixDbl {
    /// Offset 0 of the original structure (`long`).
    pub rows: usize,
    /// Offset 4 (`long`).
    pub cols: usize,
    /// The rows, one after another. Offset 8 of the original is a `double **` to a table of
    /// row pointers.
    pub data: Vec<f64>,
}

impl GenMatrixDbl {
    /// Element (i, j), `m->row[i][j]`. Panics outside the matrix, where the original would
    /// read past a heap block.
    pub fn get(&self, i: usize, j: usize) -> f64 {
        assert!(i < self.rows && j < self.cols, "element ({i}, {j}) of a {} × {} matrix", self.rows, self.cols);
        self.data[i * self.cols + j]
    }

    /// Stores element (i, j). Panics outside the matrix, where the original would write past
    /// a heap block.
    pub fn set(&mut self, i: usize, j: usize, v: f64) {
        assert!(i < self.rows && j < self.cols, "element ({i}, {j}) of a {} × {} matrix", self.rows, self.cols);
        self.data[i * self.cols + j] = v;
    }

    /// Row `i` as a slice.
    pub fn row(&self, i: usize) -> &[f64] {
        &self.data[i * self.cols..(i + 1) * self.cols]
    }
}

// ------------------------------------------------------------------ allocate, dispose, copy

/// A new `rows` × `cols` matrix [soft 0x0851bed0 SI_generalMatrixAllocateDbl], or `None`
/// where the original returns a null pointer.
///
/// The original asks its allocator `SI_malloc` (0x080d0ee8, read for this but not ported
/// here) for the 12-byte structure, then for the table of `4 × rows` bytes, then for each
/// row of `8 × cols` bytes, and returns null as soon as one request fails, without freeing
/// what it already has. `SI_malloc` returns null for a size of 0 and otherwise calls
/// `malloc`. So:
///
/// - `rows` ≤ 0 or `cols` ≤ 0: `None`. (Zero is `SI_malloc`'s own answer. A negative count
///   becomes an unsigned size of 2 GB or more, which `malloc` cannot deliver in a 32-bit
///   process. For counts whose size wraps around 32 bits the original does something else
///   again, and overruns the heap; that is not modelled: `None`.)
/// - a table or row of 2 GB or more, or 4 GB or more altogether: `None`, as a 32-bit process
///   cannot have them; and `None` if this process cannot get the memory either.
///
/// **The elements are not initialised by the original.** `SI_malloc` does not clear memory;
/// it only writes the byte 0x6f into the first byte of each block and 0x6b into the last
/// (the letters "ok"). So after allocation the lowest byte of the first element of every row
/// is 0x6f, the highest byte of the last element of every row is 0x6b and all other bytes
/// are whatever the heap held. Rust must differ here: this function returns those two bytes
/// on a background of zeros, that is element 0 of a row is 5.5e-322, the last element is
/// 2⁶⁸⁹ (or, in a one-column matrix, the single element has both bytes). Code that reads an
/// element it has not written gets these values instead of the original's garbage.
/// Class: copy.
pub fn general_matrix_allocate_dbl(rows: i32, cols: i32) -> Option<GenMatrixDbl> {
    if rows <= 0 || cols <= 0 {
        return None;
    }
    let (r, c) = (rows as u64, cols as u64);
    if 4 * r > i32::MAX as u64 || 8 * c > i32::MAX as u64 || 12 + 4 * r + 8 * r * c > u32::MAX as u64 {
        return None;
    }
    let (rows, cols) = (rows as usize, cols as usize);
    let mut data: Vec<f64> = Vec::new();
    data.try_reserve_exact(rows * cols).ok()?;
    data.resize(rows * cols, 0.0);
    for row in data.chunks_exact_mut(cols) {
        // SI_malloc's marks on the row's block: first byte 0x6f, last byte 0x6b
        row[0] = f64::from_bits(row[0].to_bits() | 0x6f);
        row[cols - 1] = f64::from_bits(row[cols - 1].to_bits() | 0x6b << 56);
    }
    Some(GenMatrixDbl { rows, cols, data })
}

/// Frees a matrix and clears the caller's pointer to it
/// [soft 0x0851bf50 SI_generalMatrixDisposeDbl]. The original takes the address of the
/// pointer: a null pointer is left alone; otherwise every row that is not null is freed, then
/// the table, then the structure, and the pointer is set to null. Here: the option becomes
/// `None`. Class: copy.
pub fn general_matrix_dispose_dbl(m: &mut Option<GenMatrixDbl>) {
    *m = None;
}

/// A newly allocated copy of `a` [soft 0x0851bfd4 SI_generalMatrixCopyDbl]. `None` in gives
/// `None` (the original checks this one pointer); `None` also when the allocation fails,
/// which [`general_matrix_allocate_dbl`] does for a matrix without rows or columns. The
/// elements are moved with `fld`/`fstp`. Class: copy.
pub fn general_matrix_copy_dbl(a: Option<&GenMatrixDbl>) -> Option<GenMatrixDbl> {
    let a = a?;
    let mut out = general_matrix_allocate_dbl(a.rows as i32, a.cols as i32)?;
    for i in 0..a.rows {
        for j in 0..a.cols {
            out.set(i, j, fmovd(a.get(i, j)));
        }
    }
    Some(out)
}

/// A new `n` × `n` identity matrix [soft 0x0851c044 SI_generalMatrixIdentityDbl]: allocated
/// with [`general_matrix_allocate_dbl`] (`None` if that fails, so for `n` ≤ 0), then every
/// element written, 1.0 on the diagonal and +0.0 elsewhere (integer stores). Class: copy.
pub fn general_matrix_identity_dbl(n: i32) -> Option<GenMatrixDbl> {
    let mut out = general_matrix_allocate_dbl(n, n)?;
    for i in 0..out.rows {
        for j in 0..out.cols {
            out.set(i, j, if i == j { 1.0 } else { 0.0 });
        }
    }
    Some(out)
}

// ------------------------------------------------------------------ element by element

/// `out[i][j] = a[i][j] + b[i][j]` over the rows and columns of `a`
/// [soft 0x0851c0b8 SI_generalMatrixAddDbl]. One addition per element, in the register,
/// stored to double: rounded to 64 bits and then to 53. Class: model.
///
/// The bounds are `a`'s; `b` and `out` are not checked (panic here if smaller). Elements of
/// a larger `out` outside those bounds are untouched. Each element is read before it is
/// written, so in the original the output may be either operand; here pass a clone of it.
/// Nothing in the program calls this.
pub fn general_matrix_add_dbl(a: &GenMatrixDbl, b: &GenMatrixDbl, out: &mut GenMatrixDbl) {
    for i in 0..a.rows {
        for j in 0..a.cols {
            out.set(i, j, (x(a.get(i, j)) + b.get(i, j)).to_f64());
        }
    }
}

/// `out[i][j] = a[i][j] − b[i][j]` over the rows and columns of `a`
/// [soft 0x0851c11c SI_generalMatrixSubtractDbl]. Everything else as for
/// [`general_matrix_add_dbl`]. Class: model. Nothing in the program calls this.
pub fn general_matrix_subtract_dbl(a: &GenMatrixDbl, b: &GenMatrixDbl, out: &mut GenMatrixDbl) {
    for i in 0..a.rows {
        for j in 0..a.cols {
            out.set(i, j, (x(a.get(i, j)) - b.get(i, j)).to_f64());
        }
    }
}

/// Copies the elements of `a` into the existing matrix `out`, over the rows and columns of
/// `a` [soft 0x0851c17c SI_generalMatrixCopyElement]. `out` is not checked (panic here if
/// smaller) and keeps its own dimensions and whatever lies outside `a`'s bounds.
/// Class: copy.
pub fn general_matrix_copy_element(a: &GenMatrixDbl, out: &mut GenMatrixDbl) {
    for i in 0..a.rows {
        for j in 0..a.cols {
            out.set(i, j, fmovd(a.get(i, j)));
        }
    }
}

// ------------------------------------------------------------------ products

/// The loops of [soft 0x0851c1c8 SI_generalMatrixMultiplyDbl], writing into `t`: see
/// [`general_matrix_multiply_dbl`].
fn multiply_loops(a: &GenMatrixDbl, b: &GenMatrixDbl, t: &mut GenMatrixDbl) {
    for i in 0..a.rows {
        for j in 0..b.cols {
            t.set(i, j, 0.0);
            for k in 0..a.cols {
                let v = (x(a.get(i, k)) * b.get(k, j) + t.get(i, j)).to_f64();
                t.set(i, j, v);
            }
        }
    }
}

/// The temporary of the alias case: see [`general_matrix_multiply_dbl_into_a`].
fn multiply_in_temporary(a: &GenMatrixDbl, b: &GenMatrixDbl) -> GenMatrixDbl {
    let mut t = general_matrix_allocate_dbl(a.rows as i32, a.cols as i32)
        .expect("the original dereferences the null pointer of a failed allocation");
    multiply_loops(a, b, &mut t);
    t
}

/// The product `a × b` into `out`, for an `out` that is neither operand
/// [soft 0x0851c1c8 SI_generalMatrixMultiplyDbl], written straight into `out`. `a` and `b`
/// may be the same matrix. Class: model.
///
/// For `i` over the rows of `a` and `j` over the columns of `b`: `out[i][j]` is first set to
/// +0.0 (an integer store) and then, for `k` over the columns of `a`, replaced by
/// `a[i][k] × b[k][j] + out[i][j]`: the product stays in the register (not rounded to
/// double), the running sum is loaded from memory, and the result is stored back, so the sum
/// is rounded to double **in every iteration**.
///
/// Nothing is checked: `b` needs `a.cols` rows and `out` needs `a.rows` × `b.cols` elements
/// (panic here otherwise); a larger `out` keeps its other elements.
///
/// The original compares the pointers: when `out` is `a` or `b` it works in a temporary
/// instead, see [`general_matrix_multiply_dbl_into_a`] and
/// [`general_matrix_multiply_dbl_into_b`].
pub fn general_matrix_multiply_dbl(a: &GenMatrixDbl, b: &GenMatrixDbl, out: &mut GenMatrixDbl) {
    multiply_loops(a, b, out);
}

/// [soft 0x0851c1c8 SI_generalMatrixMultiplyDbl] called with the output being its first
/// operand: `a` becomes `a × b`. For all three arguments being one matrix, pass a clone as
/// `b`. Class: model.
///
/// In this case the original computes the product, with the same loops, in a temporary
/// matrix allocated with the dimensions **of `a`** (rows of `a` × columns of `a`, not
/// columns of `b`), copies that to the output with [`general_matrix_copy_element`] and
/// disposes it. The allocation is not checked.
///
/// The temporary has the right shape only when `b` is square. If `b` has more columns than
/// `a` the original overruns the temporary's rows (panic here). If it has fewer, the
/// columns from `b.cols` on are never computed and the copy carries the temporary's
/// uninitialised content into the output: here that is the content
/// [`general_matrix_allocate_dbl`] gives, not the original's garbage.
pub fn general_matrix_multiply_dbl_into_a(a: &mut GenMatrixDbl, b: &GenMatrixDbl) {
    let t = multiply_in_temporary(a, b);
    general_matrix_copy_element(&t, a);
}

/// [soft 0x0851c1c8 SI_generalMatrixMultiplyDbl] called with the output being its second
/// operand: `b` becomes `a × b`, through the same temporary as in
/// [`general_matrix_multiply_dbl_into_a`], of which `a.rows` × `a.cols` elements are copied
/// back. Class: model.
pub fn general_matrix_multiply_dbl_into_b(a: &GenMatrixDbl, b: &mut GenMatrixDbl) {
    let t = multiply_in_temporary(a, b);
    general_matrix_copy_element(&t, b);
}

/// The transpose of `a` into `out`, for an `out` that is another matrix:
/// `out[j][i] = a[i][j]` over the rows and columns of `a`
/// [soft 0x0851c2cc SI_generalMatrixTransposeDbl]. `out` is not checked: it needs `a.cols`
/// rows of `a.rows` elements (panic here otherwise). Class: copy.
///
/// There is no temporary and no pointer comparison here: see
/// [`general_matrix_transpose_dbl_in_place`] for what the original does to a matrix given as
/// its own output.
pub fn general_matrix_transpose_dbl(a: &GenMatrixDbl, out: &mut GenMatrixDbl) {
    for i in 0..a.rows {
        for j in 0..a.cols {
            out.set(j, i, fmovd(a.get(i, j)));
        }
    }
}

/// [soft 0x0851c2cc SI_generalMatrixTransposeDbl] called with the output being the input:
/// the elements below the diagonal are overwritten with their mirror images before they are
/// read, so a square matrix becomes the symmetric matrix of its upper triangle, not its
/// transpose. (A matrix that is not square is overrun by the original; panic here.)
/// Class: copy.
pub fn general_matrix_transpose_dbl_in_place(m: &mut GenMatrixDbl) {
    for i in 0..m.rows {
        for j in 0..m.cols {
            let v = fmovd(m.get(i, j));
            m.set(j, i, v);
        }
    }
}

// ------------------------------------------------------------------ printing

/// The text [soft 0x0851c318 SI_generalMatrixPrintDbl] prints on the standard output: the
/// line `"--- %s %4d %4d\n"` (0x086dc140) with the name, the rows and the columns, then for
/// each row its elements as `"%10.4f"` (0x086dc150) one after another without separator and
/// a line feed (0x086dc157). Nothing for a null matrix (`None`), the one pointer the
/// original checks. Nothing in the program calls this. Class: copy.
pub fn general_matrix_print_dbl(name: &str, m: Option<&GenMatrixDbl>) -> String {
    let Some(m) = m else {
        return String::new();
    };
    let mut s = format!("--- {} {:4} {:4}\n", name, m.rows, m.cols);
    for i in 0..m.rows {
        for j in 0..m.cols {
            s.push_str(&printf_f(m.get(i, j), 10, 4));
        }
        s.push('\n');
    }
    s
}

// ------------------------------------------------------------------ 2 × 2 and 3 × 3 inverses

/// The four values [soft 0x0851c398 SI_generalMatrixInverse2Dbl] stores, or `None` where it
/// returns 0: see [`general_matrix_inverse2_dbl`].
fn inverse2(a: &GenMatrixDbl) -> Option<[[f64; 2]; 2]> {
    let (a00, a01, a10, a11) = (a.get(0, 0), a.get(0, 1), a.get(1, 0), a.get(1, 1));
    let det = x(a00) * a11 - x(a01) * a10;
    if det.abs().lt(x(INVERSE_EPS)) {
        return None;
    }
    let r = F80::ONE / det;
    Some([[(x(a11) * r).to_f64(), (-x(a01) * r).to_f64()], [(-x(a10) * r).to_f64(), (x(a00) * r).to_f64()]])
}

/// The inverse of the upper left 2 × 2 corner of `a`, by the closed formula, into the upper
/// left corner of `out` [soft 0x0851c398 SI_generalMatrixInverse2Dbl]. Returns `true` for
/// the original's 1; `false` for its 0, when the magnitude of the determinant is below
/// [`INVERSE_EPS`] = 0.0001 (an absolute limit, whatever the scale of the matrix), and then
/// `out` is untouched. Neither matrix is checked: both need two rows and two columns (panic
/// here otherwise). Class: model.
///
/// `det = a00·a11 − a01·a10`, both products and the difference unrounded. If `|det|` is
/// below the limit (strictly; a NaN determinant is not) the original returns 0 and stores
/// nothing. Otherwise `r = 1 / det` (unrounded) and the results are `a11·r`, `(−a01)·r`,
/// `(−a10)·r`, `a00·r`, each rounded to double once.
///
/// The original has loaded all four elements before its first store, so its output may be
/// its input: [`general_matrix_inverse2_dbl_in_place`].
pub fn general_matrix_inverse2_dbl(a: &GenMatrixDbl, out: &mut GenMatrixDbl) -> bool {
    match inverse2(a) {
        Some(v) => {
            store_corner(out, &v);
            true
        }
        None => false,
    }
}

/// [soft 0x0851c398 SI_generalMatrixInverse2Dbl] with the output being the input, as
/// [`general_matrix_pseudo_inverse_dbl`] calls it. Class: model.
pub fn general_matrix_inverse2_dbl_in_place(m: &mut GenMatrixDbl) -> bool {
    match inverse2(m) {
        Some(v) => {
            store_corner(m, &v);
            true
        }
        None => false,
    }
}

/// The nine values [soft 0x0851c428 SI_generalMatrixInverse3Dbl] stores, or `None` where it
/// returns 0: see [`general_matrix_inverse3_dbl`].
fn inverse3(a: &GenMatrixDbl) -> Option<[[f64; 3]; 3]> {
    let (a00, a01, a02) = (a.get(0, 0), a.get(0, 1), a.get(0, 2));
    let (a10, a11, a12) = (a.get(1, 0), a.get(1, 1), a.get(1, 2));
    let (a20, a21, a22) = (a.get(2, 0), a.get(2, 1), a.get(2, 2));
    let ca = x(a22) * a11 - x(a12) * a21;
    let cb = -(x(a10) * a22 - x(a12) * a20);
    let cc = x(a10) * a21 - x(a11) * a20;
    let det = (x(a00) * ca + x(a01) * cb) + x(a02) * cc;
    if det.abs().lt(x(INVERSE_EPS)) {
        return None;
    }
    let r = F80::ONE / det;
    Some([
        [
            (ca * r).to_f64(),
            (-(x(a01) * a22 - x(a02) * a21) * r).to_f64(),
            ((x(a01) * a12 - x(a02) * a11) * r).to_f64(),
        ],
        [
            (cb * r).to_f64(),
            ((x(a00) * a22 - x(a02) * a20) * r).to_f64(),
            (-(x(a00) * a12 - x(a02) * a10) * r).to_f64(),
        ],
        [
            (cc * r).to_f64(),
            (-(x(a00) * a21 - x(a01) * a20) * r).to_f64(),
            ((x(a00) * a11 - x(a01) * a10) * r).to_f64(),
        ],
    ])
}

/// The inverse of the upper left 3 × 3 corner of `a`, as adjugate over determinant, into the
/// upper left corner of `out` [soft 0x0851c428 SI_generalMatrixInverse3Dbl]. Returns `true`
/// for the original's 1; `false` for its 0, when the magnitude of the determinant is below
/// [`INVERSE_EPS`] = 0.0001, and then `out` is untouched. The operations are those of
/// `SI_matrix3FastInverse` in doubles, with another limit and without its message and copy.
/// Neither matrix is checked: both need three rows and columns (panic here otherwise).
/// Class: model.
///
/// With `A = a22·a11 − a12·a21`, `B = −(a10·a22 − a12·a20)`, `C = a10·a21 − a11·a20`
/// (unrounded), `det = (a00·A + a01·B) + a02·C`, unrounded. If `|det|` is below the limit
/// (strictly; a NaN is not): 0, nothing stored. Otherwise `r = 1 / det` and each result is
/// a cofactor times `r`, rounded to double once; the cofactors with a minus sign are
/// negated before the multiplication.
///
/// The original has loaded all nine elements before its first store, so its output may be
/// its input: [`general_matrix_inverse3_dbl_in_place`].
pub fn general_matrix_inverse3_dbl(a: &GenMatrixDbl, out: &mut GenMatrixDbl) -> bool {
    match inverse3(a) {
        Some(v) => {
            store_corner(out, &v);
            true
        }
        None => false,
    }
}

/// [soft 0x0851c428 SI_generalMatrixInverse3Dbl] with the output being the input, as
/// [`general_matrix_pseudo_inverse_dbl`] calls it. Class: model.
pub fn general_matrix_inverse3_dbl_in_place(m: &mut GenMatrixDbl) -> bool {
    match inverse3(m) {
        Some(v) => {
            store_corner(m, &v);
            true
        }
        None => false,
    }
}

/// Stores an N × N block of results into the upper left corner of `out`.
fn store_corner<const N: usize>(out: &mut GenMatrixDbl, v: &[[f64; N]; N]) {
    for (i, row) in v.iter().enumerate() {
        for (j, e) in row.iter().enumerate() {
            out.set(i, j, *e);
        }
    }
}

/// The right pseudo-inverse `Aᵀ × (A × Aᵀ)⁻¹` of a matrix with two or three rows, into `out`
/// (which needs `a.cols` rows and `a.rows` columns)
/// [soft 0x0851c5b8 SI_generalMatrixPseudoInverseDbl]. Returns `true` for the original's 1
/// and `false` for its 0, when `A × Aᵀ` is refused by the small inverse (determinant below
/// 0.0001 in magnitude); `out` is then untouched.
///
/// Steps: allocate `t` (`a.cols` × `a.rows`) and `s` (`a.rows` × `a.rows`), neither checked;
/// [`general_matrix_transpose_dbl`]`(a, t)`; [`general_matrix_multiply_dbl`]`(a, t, s)`;
/// invert `s` in place with [`general_matrix_inverse2_dbl`] **if `a.rows` is 2 and with
/// [`general_matrix_inverse3_dbl`] for every other row count**; on success
/// [`general_matrix_multiply_dbl`]`(t, s, out)`; dispose both. Class: model.
///
/// The row count is not checked. With four or more rows the original inverts only the upper
/// left 3 × 3 corner of `A × Aᵀ` and multiplies with the rest as it stands: a wrong result,
/// reproduced here. With one row it reads past the one-row matrix (panic here), and with no
/// rows or columns it dereferences the null pointer of a failed allocation (panic here).
/// The input is used up before the output is written, so in the original a square `a` may
/// be its own output; here pass a clone. Nothing in the program calls this.
pub fn general_matrix_pseudo_inverse_dbl(a: &GenMatrixDbl, out: &mut GenMatrixDbl) -> bool {
    const NULL: &str = "the original dereferences the null pointer of a failed allocation";
    let mut t = general_matrix_allocate_dbl(a.cols as i32, a.rows as i32).expect(NULL);
    let mut s = general_matrix_allocate_dbl(a.rows as i32, a.rows as i32).expect(NULL);
    general_matrix_transpose_dbl(a, &mut t);
    general_matrix_multiply_dbl(a, &t, &mut s);
    let ok = if a.rows == 2 { general_matrix_inverse2_dbl_in_place(&mut s) } else { general_matrix_inverse3_dbl_in_place(&mut s) };
    if ok {
        general_matrix_multiply_dbl(&t, &s, out);
    }
    ok
}

// ------------------------------------------------------------------ Greville's method

/// The sum of the magnitudes of the elements of `v` (its 1-norm, not its length),
/// **unrounded** [soft 0x0851c670 vtnAnormDbl]. The original takes a pointer and a count;
/// here the count is the slice's length. The sum starts at +0.0 and stays in the register
/// through the loop: `sum = |v[i]| + sum`, each addition rounded to 64 bits, never to
/// double. A file-local helper of the original. Class: model.
pub fn vtn_anorm_dbl(v: &[f64]) -> F80 {
    let mut sum = F80::ZERO;
    for &e in v {
        sum = x(e).abs() + sum;
    }
    sum
}

/// The pseudo-inverse of a vector: `out = v / (v · v)`, or all zeros when `v · v` is not
/// above [`VTN_INV_EPS`] = 1e-10 [soft 0x0851c694 vtnInvDbl]. The original takes two
/// pointers and a count; here the count is `v`'s length and `out` needs at least as many
/// elements.
///
/// `s = v[i]·v[i] + s` starts at +0.0 and stays in the register through the loop (rounded
/// to 64 bits each time, never to double). If `1e-10 < s` (false for a NaN): `r = 1 / s`,
/// unrounded, and `out[i] = v[i] × r`, each rounded to double once. Otherwise every
/// `out[i]` is set to +0.0. A file-local helper of the original; the original would give
/// the same result with `out` being `v`. Class: model.
pub fn vtn_inv_dbl(v: &[f64], out: &mut [f64]) {
    let mut s = F80::ZERO;
    for &e in v {
        s = x(e) * x(e) + s;
    }
    if x(VTN_INV_EPS).lt(s) {
        let r = F80::ONE / s;
        for (o, &e) in out[..v.len()].iter_mut().zip(v) {
            *o = (x(e) * r).to_f64();
        }
    } else {
        for o in out[..v.len()].iter_mut() {
            *o = 0.0;
        }
    }
}

/// The Moore–Penrose pseudo-inverse of the m × n matrix `a` by Greville's column recursion,
/// into `out`, which needs n rows of m elements
/// [soft 0x0851c710 SI_generalMatrixGrevilleInverseDbl]. Always returns `true` (the
/// original's 1): there is no failure path, a rank-deficient matrix simply takes the other
/// branch below.
///
/// With m = `a.rows`, n = `a.cols`, and four work vectors `col`, `kv`, `c` (m doubles) and
/// `d` (n doubles), allocated with `SI_malloc` and not checked:
///
/// 1. `col` = column 0 of `a`; `kv` = [`vtn_inv_dbl`]`(col)`; row 0 of `out` = `kv`.
/// 2. For `k` = 1 … n − 1, with rows 0 … k − 1 of `out` holding the pseudo-inverse of the
///    first `k` columns:
///    - `col` = column `k` of `a`.
///    - `d[j]` for `j` < `k`: set to +0.0, then `d[j] = out[j][i] × col[i] + d[j]` for
///      `i` < m, the sum **stored to double in every iteration** (`d` = `A_k⁺ × col`).
///    - `c[i]` for `i` < m: `col[i]`, then `c[i] = c[i] − a[i][j] × d[j]` for `j` < `k`,
///      again **stored in every iteration** (`c` = the part of the column outside the span
///      of the earlier ones).
///    - `limit` = [`vtn_anorm_dbl`]`(col)` × [`GREVILLE_EPS`] (0.001), unrounded (the
///      original parks it in an 80-bit temporary), compared with [`vtn_anorm_dbl`]`(c)`,
///      also unrounded.
///    - If `limit > norm(c)` (the column is taken as dependent; false when either is a
///      NaN): `s = d[j]·d[j] + s` starting at 1.0, **in the register**; `r = 1 / s`; and for
///      each `i` < m, `t = out[j][i] × d[j] + t` over `j` < `k` starting at +0.0, **in the
///      register**, then `kv[i] = t × r` rounded to double.
///    - Otherwise `kv` = [`vtn_inv_dbl`]`(c)`.
///    - For each `i` < m: `out[j][i] = out[j][i] − d[j] × kv[i]` for `j` < `k` (one product
///      and one subtraction, rounded to double), then `out[k][i] = kv[i]`.
///
/// Class: model. `a` is read all the way through while `out` is written, so the original
/// called with one matrix for both gives neither's pseudo-inverse; this function takes two.
/// Nothing is checked: `a` needs at least one column when it has rows (panic here; the
/// original reads past an empty row).
pub fn general_matrix_greville_inverse_dbl(a: &GenMatrixDbl, out: &mut GenMatrixDbl) -> bool {
    let (m, n) = (a.rows, a.cols);
    let mut col = vec![0.0f64; m];
    let mut kv = vec![0.0f64; m];
    let mut c = vec![0.0f64; m];
    let mut d = vec![0.0f64; n];

    for i in 0..m {
        col[i] = fmovd(a.get(i, 0));
    }
    vtn_inv_dbl(&col, &mut kv);
    for i in 0..m {
        out.set(0, i, fmovd(kv[i]));
    }

    for k in 1..n {
        for i in 0..m {
            col[i] = fmovd(a.get(i, k));
        }
        for j in 0..k {
            d[j] = 0.0;
            for i in 0..m {
                d[j] = (x(out.get(j, i)) * col[i] + d[j]).to_f64();
            }
        }
        for i in 0..m {
            c[i] = fmovd(col[i]);
            for j in 0..k {
                c[i] = (c[i] - x(a.get(i, j)) * d[j]).to_f64();
            }
        }
        let limit = vtn_anorm_dbl(&col) * GREVILLE_EPS;
        let rest = vtn_anorm_dbl(&c);
        if limit.gt(rest) {
            let mut s = F80::ONE;
            for &e in &d[..k] {
                s = x(e) * x(e) + s;
            }
            let r = F80::ONE / s;
            for i in 0..m {
                let mut t = F80::ZERO;
                for j in 0..k {
                    t = x(out.get(j, i)) * d[j] + t;
                }
                kv[i] = (t * r).to_f64();
            }
        } else {
            vtn_inv_dbl(&c, &mut kv);
        }
        for i in 0..m {
            for j in 0..k {
                let v = (out.get(j, i) - x(d[j]) * kv[i]).to_f64();
                out.set(j, i, v);
            }
            out.set(k, i, fmovd(kv[i]));
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A matrix from rows of numbers (test data only).
    fn mat(rows: &[&[f64]]) -> GenMatrixDbl {
        let mut m = general_matrix_allocate_dbl(rows.len() as i32, rows[0].len() as i32).unwrap();
        for (i, r) in rows.iter().enumerate() {
            for (j, v) in r.iter().enumerate() {
                m.set(i, j, *v);
            }
        }
        m
    }

    fn zeros(rows: i32, cols: i32) -> GenMatrixDbl {
        let mut m = general_matrix_allocate_dbl(rows, cols).unwrap();
        m.data.fill(0.0);
        m
    }

    fn close(a: &GenMatrixDbl, b: &GenMatrixDbl, tol: f64) -> bool {
        a.rows == b.rows && a.cols == b.cols && a.data.iter().zip(&b.data).all(|(p, q)| (p - q).abs() <= tol)
    }

    fn p2(e: i32) -> f64 {
        2.0f64.powi(e)
    }

    #[test]
    fn constants_have_the_bits_of_the_binary() {
        assert_eq!(INVERSE_EPS.to_bits(), 0x3f1a_36e2_eb1c_432d);
        assert_eq!(VTN_INV_EPS.to_bits(), 0x3ddb_7cdf_d9d7_bdbb);
        assert_eq!(GREVILLE_EPS.to_bits(), 0x3f50_624d_d2f1_a9fc);
    }

    #[test]
    fn allocate_dispose_copy() {
        assert!(general_matrix_allocate_dbl(0, 3).is_none());
        assert!(general_matrix_allocate_dbl(3, 0).is_none());
        assert!(general_matrix_allocate_dbl(-1, 3).is_none());
        assert!(general_matrix_allocate_dbl(3, -2).is_none());
        assert!(general_matrix_allocate_dbl(i32::MAX, i32::MAX).is_none());
        assert!(general_matrix_allocate_dbl(1, 1 << 28).is_none()); // one row of 2 GB
        let m = general_matrix_allocate_dbl(2, 3).unwrap();
        assert_eq!((m.rows, m.cols, m.data.len()), (2, 3, 6));
        // SI_malloc's two marker bytes per row, on zeros
        let bits: Vec<u64> = m.data.iter().map(|v| v.to_bits()).collect();
        assert_eq!(bits, [0x6f, 0, 0x6b << 56, 0x6f, 0, 0x6b << 56]);
        let one = general_matrix_allocate_dbl(2, 1).unwrap();
        assert_eq!(one.data[1].to_bits(), 0x6b00_0000_0000_006f);

        let mut held = Some(m);
        general_matrix_dispose_dbl(&mut held);
        assert!(held.is_none());
        general_matrix_dispose_dbl(&mut held); // a null pointer is left alone
        assert!(held.is_none());

        assert!(general_matrix_copy_dbl(None).is_none());
        let mut a = mat(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        a.set(1, 1, f64::from_bits(0x7ff0_0000_0000_0001)); // a signalling NaN
        let c = general_matrix_copy_dbl(Some(&a)).unwrap();
        assert_eq!((c.rows, c.cols), (2, 3));
        assert_eq!(c.row(0), [1.0, 2.0, 3.0]);
        assert_eq!(c.get(1, 1).to_bits(), 0x7ff8_0000_0000_0001); // quiet after fld/fstp
        assert_eq!(c.get(1, 2), 6.0);
        // a matrix without rows cannot be allocated, so it cannot be copied
        let empty = GenMatrixDbl { rows: 0, cols: 3, data: Vec::new() };
        assert!(general_matrix_copy_dbl(Some(&empty)).is_none());
    }

    #[test]
    fn identity() {
        let m = general_matrix_identity_dbl(3).unwrap();
        assert_eq!(m.data, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        assert!(m.data.iter().all(|v| v.to_bits() == 0 || *v == 1.0)); // no marker bytes left
        assert_eq!(general_matrix_identity_dbl(1).unwrap().data, [1.0]);
        assert!(general_matrix_identity_dbl(0).is_none());
        assert!(general_matrix_identity_dbl(-4).is_none());
    }

    #[test]
    fn add_subtract_copy_element() {
        let a = mat(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let b = mat(&[&[0.5, -2.0], &[10.0, 0.25]]);
        let mut out = zeros(3, 3);
        out.data.fill(9.0);
        general_matrix_add_dbl(&a, &b, &mut out);
        // the bounds are a's: the rest of a larger output is untouched
        assert_eq!(out.data, [1.5, 0.0, 9.0, 13.0, 4.25, 9.0, 9.0, 9.0, 9.0]);
        general_matrix_subtract_dbl(&a, &b, &mut out);
        assert_eq!(out.data, [0.5, 4.0, 9.0, -7.0, 3.75, 9.0, 9.0, 9.0, 9.0]);
        general_matrix_copy_element(&b, &mut out);
        assert_eq!(out.data, [0.5, -2.0, 9.0, 10.0, 0.25, 9.0, 9.0, 9.0, 9.0]);
        assert_eq!((out.rows, out.cols), (3, 3));
    }

    #[test]
    fn add_and_subtract_round_twice() {
        // 1 + (2^-53 + 2^-64) lies above the middle between 1 and 1 + 2^-52, so double
        // arithmetic gives 1 + 2^-52. The register rounds it to 1 + 2^-53 first (a tie, to
        // even), and the store rounds that tie to 1.
        let tiny = p2(-53) + p2(-64);
        assert_eq!(1.0 + tiny, 1.0 + p2(-52));
        let a = mat(&[&[1.0, 1.0]]);
        let b = mat(&[&[tiny, -tiny]]);
        let mut out = zeros(1, 2);
        general_matrix_add_dbl(&a, &b, &mut out);
        assert_eq!(out.data, [1.0, 1.0 - p2(-53)]); // the second, 1 − tiny, is no tie: the same either way
        assert_eq!(1.0 - tiny, 1.0 - p2(-53));
        let b = mat(&[&[-tiny, 0.0]]);
        general_matrix_subtract_dbl(&a, &b, &mut out);
        assert_eq!(out.data, [1.0, 1.0]);
        assert_eq!(1.0 - (-tiny), 1.0 + p2(-52));
    }

    #[test]
    fn multiply_values() {
        let a = mat(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        let b = mat(&[&[1.0, 0.0], &[0.0, 1.0], &[2.0, -1.0]]);
        let mut out = zeros(2, 2);
        general_matrix_multiply_dbl(&a, &b, &mut out);
        assert_eq!(out.data, [7.0, -1.0, 16.0, -1.0]);
        // into a larger output: only a.rows × b.cols elements are written
        let mut big = zeros(3, 3);
        big.data.fill(9.0);
        general_matrix_multiply_dbl(&a, &b, &mut big);
        assert_eq!(big.data, [7.0, -1.0, 9.0, 16.0, -1.0, 9.0, 9.0, 9.0, 9.0]);
        // the other way round: 3 × 3
        let mut out = zeros(3, 3);
        general_matrix_multiply_dbl(&b, &a, &mut out);
        assert_eq!(out.data, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, -2.0, -1.0, 0.0]);
        // a matrix times itself, both operands being one object
        let s = mat(&[&[1.0, 1.0], &[0.0, 1.0]]);
        let mut out = zeros(2, 2);
        general_matrix_multiply_dbl(&s, &s, &mut out);
        assert_eq!(out.data, [1.0, 2.0, 0.0, 1.0]);
    }

    #[test]
    fn multiply_rounds_the_sum_each_step_but_not_the_products() {
        let e = 1.0 + p2(-30); // e² = 1 + 2^-29 + 2^-60: 61 bits, fits the register, not a double
        // sum = e·e, stored: 1 + 2^-29 (the 2^-60 is lost); then + (−1)·1: exactly 2^-29.
        // A sum kept in the register over the loop would give 2^-29 + 2^-60.
        let a = mat(&[&[e, -1.0]]);
        let b = mat(&[&[e], &[1.0]]);
        let mut out = zeros(1, 1);
        general_matrix_multiply_dbl(&a, &b, &mut out);
        assert_eq!(out.data, [p2(-29)]);
        // the other order: sum = −1, then e·e − 1 with the product still unrounded:
        // 2^-29 + 2^-60. Rounding the product to double first would give 2^-29.
        let a = mat(&[&[-1.0, e]]);
        let b = mat(&[&[1.0], &[e]]);
        general_matrix_multiply_dbl(&a, &b, &mut out);
        assert_eq!(out.data, [p2(-29) + p2(-60)]);
        assert_eq!(e * e - 1.0, p2(-29));
    }

    #[test]
    fn multiply_into_an_operand() {
        let mut a = mat(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let b = mat(&[&[0.0, 1.0], &[1.0, 0.0]]);
        general_matrix_multiply_dbl_into_a(&mut a, &b);
        assert_eq!(a.data, [2.0, 1.0, 4.0, 3.0]);
        let a = mat(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let mut b2 = b.clone();
        general_matrix_multiply_dbl_into_b(&a, &mut b2);
        assert_eq!(b2.data, [2.0, 1.0, 4.0, 3.0]);
        // squaring in place: all three arguments one matrix
        let mut s = mat(&[&[1.0, 1.0], &[0.0, 1.0]]);
        let copy = s.clone();
        general_matrix_multiply_dbl_into_a(&mut s, &copy);
        assert_eq!(s.data, [1.0, 2.0, 0.0, 1.0]);
        // the temporary has a's shape: with a 2 × 2 a and a 2 × 1 b, column 1 of the
        // temporary is never computed and is copied back all the same. The original copies
        // heap garbage; here it is the allocator's marker byte on zeros.
        let mut a = mat(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let b = mat(&[&[1.0], &[1.0]]);
        general_matrix_multiply_dbl_into_a(&mut a, &b);
        assert_eq!(a.get(0, 0), 3.0);
        assert_eq!(a.get(1, 0), 7.0);
        assert_eq!(a.get(0, 1).to_bits(), 0x6b << 56);
        assert_eq!(a.get(1, 1).to_bits(), 0x6b << 56);
    }

    #[test]
    #[should_panic]
    fn multiply_into_an_operand_overruns_when_b_is_wider() {
        // a is 1 × 1, b is 1 × 2: the original writes two elements into a one-element row
        let mut a = mat(&[&[2.0]]);
        let b = mat(&[&[1.0, 1.0]]);
        general_matrix_multiply_dbl_into_a(&mut a, &b);
    }

    #[test]
    fn transpose() {
        let a = mat(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        let mut t = zeros(3, 2);
        general_matrix_transpose_dbl(&a, &mut t);
        assert_eq!(t.data, [1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
        let mut back = zeros(2, 3);
        general_matrix_transpose_dbl(&t, &mut back);
        assert_eq!(back, a);
        // in place the upper triangle is mirrored down instead
        let mut m = mat(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0], &[7.0, 8.0, 9.0]]);
        general_matrix_transpose_dbl_in_place(&mut m);
        assert_eq!(m.data, [1.0, 2.0, 3.0, 2.0, 5.0, 6.0, 3.0, 6.0, 9.0]);
    }

    #[test]
    fn print() {
        assert_eq!(general_matrix_print_dbl("x", None), "");
        let m = mat(&[&[1.0, -2.5, 1234.56789], &[0.00005, f64::INFINITY, -f64::NAN]]);
        let want = "--- jac    2    3\n    1.0000   -2.5000 1234.5679\n    0.0001       inf      -nan\n";
        assert_eq!(general_matrix_print_dbl("jac", Some(&m)), want);
        // a field wider than 10 pushes the following ones along, as in C
        let wide = mat(&[&[123456789.0, 1.0]]);
        assert_eq!(general_matrix_print_dbl("", Some(&wide)), "---     1    2\n123456789.0000    1.0000\n");
        // exact ties go to the even digit: 0.03125 → 0.0312, 0.09375 → 0.0938
        let ties = mat(&[&[0.03125, 0.09375]]);
        assert_eq!(general_matrix_print_dbl("t", Some(&ties)), "--- t    1    2\n    0.0312    0.0938\n");
    }

    #[test]
    fn inverse2_values_and_limit() {
        let a = mat(&[&[4.0, 7.0], &[2.0, 6.0]]);
        let mut out = zeros(2, 2);
        assert!(general_matrix_inverse2_dbl(&a, &mut out));
        assert_eq!(out.data, [0.6, -0.7, -0.2, 0.4]);
        let a = mat(&[&[2.0, 0.0], &[0.0, 4.0]]);
        assert!(general_matrix_inverse2_dbl(&a, &mut out));
        assert_eq!(out.data, [0.5, 0.0, 0.0, 0.25]);
        // refused below 0.0001, strictly, whatever the scale; the output is untouched
        out.data.fill(7.0);
        let small = mat(&[&[0.009, 0.0], &[0.0, 0.011]]); // det 0.000099
        assert!(!general_matrix_inverse2_dbl(&small, &mut out));
        assert_eq!(out.data, [7.0; 4]);
        let at = mat(&[&[1e-4, 0.0], &[0.0, 1.0]]); // det = the limit itself
        assert!(general_matrix_inverse2_dbl(&at, &mut out));
        assert_eq!(out.data, [10000.0, 0.0, 0.0, 1.0]);
        let singular = mat(&[&[1.0, 2.0], &[2.0, 4.0]]);
        assert!(!general_matrix_inverse2_dbl(&singular, &mut out));
        // a NaN determinant is not below the limit
        let nan = mat(&[&[f64::NAN, 0.0], &[0.0, 1.0]]);
        assert!(general_matrix_inverse2_dbl(&nan, &mut out));
        assert!(out.get(0, 0).is_nan());
        // only the corner of a larger matrix is read and written, also in place
        let mut big = mat(&[&[4.0, 7.0, 5.0], &[2.0, 6.0, 5.0], &[5.0, 5.0, 5.0]]);
        assert!(general_matrix_inverse2_dbl_in_place(&mut big));
        assert_eq!(big.data, [0.6, -0.7, 5.0, -0.2, 0.4, 5.0, 5.0, 5.0, 5.0]);
    }

    #[test]
    fn small_inverses_keep_the_determinant_unrounded() {
        // a00 = a11 = 2^20 + 2^-10, a01 = a10 = 2^20: a00·a11 = 2^40 + 2^11 + 2^-20 needs 61
        // bits. In the register the determinant is 2^11 + 2^-20; with the product rounded
        // to double it would be 2^11.
        // r = 2^-11 (1 − 2^-31 + 2^-62) after rounding to 64 bits;
        // a11·r = 2^9 (1 + 2^-31 − 2^-62 + …) → the double 512 + 2^-22 (not 512 + 2^-21);
        // −a01·r = −2^9 (1 − 2^-31 + 2^-62) → the double −(512 − 2^-22) (not −512).
        let big = p2(20);
        let d = big + p2(-10);
        assert_eq!(d * d - big * big, 2048.0);
        let a = mat(&[&[d, big], &[big, d]]);
        let mut out = zeros(2, 2);
        assert!(general_matrix_inverse2_dbl(&a, &mut out));
        let (p, q) = (512.0 + p2(-22), -(512.0 - p2(-22)));
        assert_eq!(out.data, [p, q, q, p]);
        assert_eq!(d / 2048.0, 512.0 + p2(-21));
        // the same block in a 3 × 3 matrix with a22 = 1: det = (a00·A + a01·B) + a02·C with
        // A = d, B = −2^20, C = 0, again 2^11 + 2^-20; element [2][2] is det·r = 1 + 2^-93,
        // which is 1
        let a = mat(&[&[d, big, 0.0], &[big, d, 0.0], &[0.0, 0.0, 1.0]]);
        let mut out = zeros(3, 3);
        assert!(general_matrix_inverse3_dbl(&a, &mut out));
        assert_eq!(out.data, [p, q, 0.0, q, p, 0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn inverse3_values_and_limit() {
        let a = mat(&[&[2.0, 0.0, 0.0], &[0.0, 4.0, 0.0], &[0.0, 0.0, 8.0]]);
        let mut out = zeros(3, 3);
        assert!(general_matrix_inverse3_dbl(&a, &mut out));
        assert_eq!(out.data, [0.5, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.125]);
        let shear = mat(&[&[1.0, 2.0, 3.0], &[0.0, 1.0, 4.0], &[0.0, 0.0, 1.0]]);
        assert!(general_matrix_inverse3_dbl(&shear, &mut out));
        assert_eq!(out.data, [1.0, -2.0, 5.0, 0.0, 1.0, -4.0, 0.0, 0.0, 1.0]);
        let a = mat(&[&[2.0, -1.0, 0.5], &[0.25, 3.0, -2.0], &[1.5, 0.75, 4.0]]);
        assert!(general_matrix_inverse3_dbl(&a, &mut out));
        let mut prod = zeros(3, 3);
        general_matrix_multiply_dbl(&a, &out, &mut prod);
        assert!(close(&prod, &general_matrix_identity_dbl(3).unwrap(), 1e-15));
        let mut same = a.clone();
        assert!(general_matrix_inverse3_dbl_in_place(&mut same));
        assert_eq!(same, out);
        // refused below 0.0001: a perfectly regular but small matrix is "singular"
        out.data.fill(7.0);
        let small = mat(&[&[0.04, 0.0, 0.0], &[0.0, 0.04, 0.0], &[0.0, 0.0, 0.04]]); // det 0.000064
        assert!(!general_matrix_inverse3_dbl(&small, &mut out));
        assert_eq!(out.data, [7.0; 9]);
        let singular = mat(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0], &[7.0, 8.0, 9.0]]);
        assert!(!general_matrix_inverse3_dbl(&singular, &mut out));
        let nan = mat(&[&[f64::NAN, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        assert!(general_matrix_inverse3_dbl(&nan, &mut out));
        assert!(out.get(0, 0).is_nan());
    }

    #[test]
    fn inverse3_determinant_order() {
        // det = (a00·A + a01·B) + a02·C with A = 2^64, B = 1, C = −2^32 and the first row
        // (1, 1, 2^32): 2^64 + 1 rounds back to 2^64 and the sum is 0, although the true
        // determinant is 1: refused
        let a = mat(&[&[1.0, 1.0, p2(32)], &[0.0, p2(32), 1.0], &[1.0, 0.0, p2(32)]]);
        let mut out = zeros(3, 3);
        assert!(!general_matrix_inverse3_dbl(&a, &mut out));
        assert_eq!(out.data, [0.0; 9]);
    }

    #[test]
    fn pseudo_inverse() {
        // two rows: A × A⁺ = I
        let a = mat(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        let mut out = zeros(3, 2);
        assert!(general_matrix_pseudo_inverse_dbl(&a, &mut out));
        let mut prod = zeros(2, 2);
        general_matrix_multiply_dbl(&a, &out, &mut prod);
        assert!(close(&prod, &general_matrix_identity_dbl(2).unwrap(), 1e-13));
        // A × Aᵀ = [[14, 32], [32, 77]], determinant 54: A⁺ = Aᵀ × [[77, −32], [−32, 14]] / 54
        let want = mat(&[&[-17.0 / 18.0, 4.0 / 9.0], &[-1.0 / 9.0, 1.0 / 9.0], &[13.0 / 18.0, -2.0 / 9.0]]);
        assert!(close(&out, &want, 1e-14));
        // three rows, four columns
        let a = mat(&[&[1.0, 0.0, 2.0, 0.0], &[0.0, 1.0, 0.0, 3.0], &[1.0, 1.0, 0.0, 0.0]]);
        let mut out = zeros(4, 3);
        assert!(general_matrix_pseudo_inverse_dbl(&a, &mut out));
        let mut prod = zeros(3, 3);
        general_matrix_multiply_dbl(&a, &out, &mut prod);
        assert!(close(&prod, &general_matrix_identity_dbl(3).unwrap(), 1e-14));
        // dependent rows: refused, output untouched
        let a = mat(&[&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]]);
        let mut out = zeros(3, 2);
        out.data.fill(7.0);
        assert!(!general_matrix_pseudo_inverse_dbl(&a, &mut out));
        assert_eq!(out.data, [7.0; 6]);
        // small rows are refused as well: the limit on det(A × Aᵀ) is absolute
        let a = mat(&[&[0.05, 0.0, 0.0], &[0.0, 0.05, 0.0]]); // det = 0.0025² = 6.25e-6
        assert!(!general_matrix_pseudo_inverse_dbl(&a, &mut out));
    }

    #[test]
    fn pseudo_inverse_of_four_rows_inverts_only_a_corner() {
        // A = diag(1, 1, 1, 2): A × Aᵀ = diag(1, 1, 1, 4). The 3 × 3 routine inverts the
        // upper left corner and leaves the 4; the result is diag(1, 1, 1, 8), not
        // diag(1, 1, 1, 0.5).
        let mut a = general_matrix_identity_dbl(4).unwrap();
        a.set(3, 3, 2.0);
        let mut out = zeros(4, 4);
        assert!(general_matrix_pseudo_inverse_dbl(&a, &mut out));
        let mut want = general_matrix_identity_dbl(4).unwrap();
        want.set(3, 3, 8.0);
        assert_eq!(out, want);
    }

    #[test]
    #[should_panic]
    fn pseudo_inverse_of_one_row_reads_past_the_matrix() {
        let a = mat(&[&[1.0, 2.0, 3.0]]);
        let mut out = zeros(3, 1);
        general_matrix_pseudo_inverse_dbl(&a, &mut out);
    }

    #[test]
    fn anorm_is_a_sum_of_magnitudes_kept_in_the_register() {
        assert_eq!(vtn_anorm_dbl(&[]).to_bits(), F80::ZERO.to_bits());
        assert_eq!(vtn_anorm_dbl(&[3.0, -4.0, 0.5]).to_f64(), 7.5);
        // 2^60 + 1 + 2^60 = 2^61 + 1: 62 bits, held by the register; a double sum loses the 1
        let s = vtn_anorm_dbl(&[p2(60), 1.0, -p2(60)]);
        assert_eq!((s - x(p2(61))).to_f64(), 1.0);
        assert_eq!(p2(60) + 1.0 + p2(60), p2(61));
        assert!(vtn_anorm_dbl(&[1.0, f64::NAN]).is_nan());
    }

    #[test]
    fn vector_inverse() {
        let mut out = [9.0; 3];
        vtn_inv_dbl(&[3.0, 4.0], &mut out);
        assert_eq!(out, [0.12, 0.16, 9.0]); // v / 25; the third element is not the function's
        // v · v must be above 1e-10, else zeros
        vtn_inv_dbl(&[1e-6, 1e-6], &mut out);
        assert_eq!(out, [0.0, 0.0, 9.0]);
        vtn_inv_dbl(&[1e-4], &mut out);
        assert!((out[0] - 1e4).abs() < 1e-8);
        vtn_inv_dbl(&[0.0, 0.0, 0.0], &mut out);
        assert_eq!(out.map(f64::to_bits), [0; 3]);
        // a NaN sum is not above the limit: zeros
        let mut out = [9.0; 2];
        vtn_inv_dbl(&[f64::NAN, 1.0], &mut out);
        assert_eq!(out, [0.0, 0.0]);
    }

    #[test]
    fn vector_inverse_sums_in_the_register() {
        // 2^30 followed by 256 ones: the squares sum to 2^60 + 256 in the register. A double
        // accumulator would stay at 2^60 (each 1 is far below half a unit in the last place,
        // which is 128), although 2^60 + 256 itself is a double.
        // r = 1 / (2^60 (1 + 2^-52)) = 2^-60 (1 − 2^-52) after rounding to 64 bits, so
        // out[0] = 2^-30 (1 − 2^-52) and the others are 2^-60 (1 − 2^-52), both exact.
        let mut v = vec![1.0; 257];
        v[0] = p2(30);
        let mut out = vec![0.0; 257];
        vtn_inv_dbl(&v, &mut out);
        assert_eq!(out[0], p2(-30) - p2(-82));
        assert_eq!(out[1], p2(-60) - p2(-112));
        assert_eq!(out[256], out[1]);
        let mut s = 0.0f64;
        for e in &v {
            s += e * e;
        }
        assert_eq!(s, p2(60));
    }

    #[test]
    fn greville_values() {
        // one column: the vector pseudo-inverse as a row
        let a = mat(&[&[3.0], &[4.0]]);
        let mut out = zeros(1, 2);
        assert!(general_matrix_greville_inverse_dbl(&a, &mut out));
        assert_eq!(out.data, [0.12, 0.16]);
        // a regular square matrix: the inverse
        let a = mat(&[&[4.0, 7.0], &[2.0, 6.0]]);
        let mut out = zeros(2, 2);
        assert!(general_matrix_greville_inverse_dbl(&a, &mut out));
        assert!(close(&out, &mat(&[&[0.6, -0.7], &[-0.2, 0.4]]), 1e-15));
        // wide and tall matrices of full rank: the four Moore–Penrose conditions
        for a in [mat(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]), mat(&[&[1.0, 4.0], &[2.0, 5.0], &[3.0, 7.0]])] {
            let (m, n) = (a.rows as i32, a.cols as i32);
            let mut p = zeros(n, m);
            assert!(general_matrix_greville_inverse_dbl(&a, &mut p));
            let mut ap = zeros(m, m);
            general_matrix_multiply_dbl(&a, &p, &mut ap);
            let mut pa = zeros(n, n);
            general_matrix_multiply_dbl(&p, &a, &mut pa);
            let mut apa = zeros(m, n);
            general_matrix_multiply_dbl(&ap, &a, &mut apa);
            let mut pap = zeros(n, m);
            general_matrix_multiply_dbl(&pa, &p, &mut pap);
            assert!(close(&apa, &a, 1e-12));
            assert!(close(&pap, &p, 1e-12));
            let mut apt = zeros(m, m);
            general_matrix_transpose_dbl(&ap, &mut apt);
            assert!(close(&apt, &ap, 1e-12));
            let mut pat = zeros(n, n);
            general_matrix_transpose_dbl(&pa, &mut pat);
            assert!(close(&pat, &pa, 1e-12));
        }
        // against the other routine, where that one applies
        let a = mat(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        let mut g = zeros(3, 2);
        general_matrix_greville_inverse_dbl(&a, &mut g);
        let mut p = zeros(3, 2);
        general_matrix_pseudo_inverse_dbl(&a, &mut p);
        assert!(close(&g, &p, 1e-13));
    }

    #[test]
    fn greville_on_many_small_matrices() {
        // small integer matrices of every shape up to 4 × 3, rank-deficient ones included:
        // the result satisfies A × P × A = A and P × A × P = P, and (A × P) is symmetric
        let mut rng = crate::x87::testing::Rng(0x5eed_0004);
        for round in 0..600 {
            let (m, n) = (1 + round % 4, 1 + (round / 4) % 3);
            let mut a = zeros(m, n);
            for v in a.data.iter_mut() {
                *v = (rng.next() % 7) as f64 - 3.0;
            }
            let mut p = zeros(n, m);
            assert!(general_matrix_greville_inverse_dbl(&a, &mut p));
            let mut ap = zeros(m, m);
            general_matrix_multiply_dbl(&a, &p, &mut ap);
            let mut apa = zeros(m, n);
            general_matrix_multiply_dbl(&ap, &a, &mut apa);
            let mut pa = zeros(n, n);
            general_matrix_multiply_dbl(&p, &a, &mut pa);
            let mut pap = zeros(n, m);
            general_matrix_multiply_dbl(&pa, &p, &mut pap);
            let mut apt = zeros(m, m);
            general_matrix_transpose_dbl(&ap, &mut apt);
            assert!(close(&apa, &a, 1e-10), "{a:?}");
            assert!(close(&pap, &p, 1e-10), "{a:?}");
            assert!(close(&apt, &ap, 1e-10), "{a:?}");
        }
    }

    #[test]
    fn greville_dependent_columns() {
        // the second column is twice the first: the "dependent" branch. A⁺ = Aᵀ / 25
        let a = mat(&[&[1.0, 2.0], &[2.0, 4.0]]);
        let mut out = zeros(2, 2);
        assert!(general_matrix_greville_inverse_dbl(&a, &mut out));
        assert!(close(&out, &mat(&[&[0.04, 0.08], &[0.08, 0.16]]), 1e-16));
        // a zero matrix: everything zero, no failure
        let a = zeros(2, 3);
        let mut out = zeros(3, 2);
        out.data.fill(7.0);
        assert!(general_matrix_greville_inverse_dbl(&a, &mut out));
        assert_eq!(out.data, [0.0; 6]);
        // a column that is almost dependent (the rest below 0.001 of its 1-norm) is treated
        // as dependent: the tiny component is dropped, not inverted into something huge
        let a = mat(&[&[1.0, 1.0], &[0.0, 1e-4]]);
        let mut out = zeros(2, 2);
        general_matrix_greville_inverse_dbl(&a, &mut out);
        assert!(out.data.iter().all(|v| v.abs() < 1.0));
        // just above the limit it is inverted: the true inverse [[1, −500], [0, 500]]
        let a = mat(&[&[1.0, 1.0], &[0.0, 2e-3]]);
        general_matrix_greville_inverse_dbl(&a, &mut out);
        assert!(close(&out, &mat(&[&[1.0, -500.0], &[0.0, 500.0]]), 1e-9));
    }

    #[test]
    fn greville_rounding_by_hand() {
        // A = [[1, 2], [1, e]] with e = 2^-52 + 2^-63.
        // Column 0 = (1, 1): s = 2, row 0 of the result = (0.5, 0.5).
        // Column 1 = (2, e): d = 0.5·2 + 0 = 1, stored; then 0.5·e + 1 = 1 + 2^-53 + 2^-64,
        //   which the register rounds to 1 + 2^-53 (a tie, to even) and the store to 1 (a
        //   tie again). Double arithmetic would give 1 + 2^-52.
        // c = (2 − 1·1, e − 1·1) = (1, −(1 − 2^-52)) after the store.
        // Not dependent, so kv = c / (c · c): s = 1 + (1 − 2^-51) = 2 − 2^-51,
        //   r = 0.5 (1 + 2^-52), kv = (0.5 + 2^-53, −0.5).
        // Row 0 becomes (0.5 − 1·(0.5 + 2^-53), 0.5 − 1·(−0.5)) = (−2^-53, 1), row 1 = kv.
        let e = p2(-52) + p2(-63);
        let a = mat(&[&[1.0, 2.0], &[1.0, e]]);
        let mut out = zeros(2, 2);
        assert!(general_matrix_greville_inverse_dbl(&a, &mut out));
        assert_eq!(out.data, [-p2(-53), 1.0, 0.5 + p2(-53), -0.5]);
        // what double arithmetic gives for d, and so for the first row
        assert_eq!(0.5 * 2.0 + 0.5 * e, 1.0 + p2(-52));
    }

    #[test]
    #[should_panic]
    fn greville_needs_room_in_the_output() {
        let a = mat(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        let mut out = zeros(2, 2); // needs 3 × 2
        general_matrix_greville_inverse_dbl(&a, &mut out);
    }
}
