// SPDX-License-Identifier: MIT
//! Text formatting shared by the original's dump and print functions.

/// C's `%W.Pf` for a `double` as the GNU C library prints it: correctly rounded decimal
/// digits (ties to even, which only exact binary ties can be), `-0.000000` for a negative
/// zero, `inf`, `-inf`, `nan` and `-nan`, right-aligned in `width` columns.
pub(crate) fn printf_f(v: f64, width: usize, precision: usize) -> String {
    let body = if v.is_nan() {
        if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() }
    } else if v.is_infinite() {
        if v < 0.0 { "-inf".to_string() } else { "inf".to_string() }
    } else {
        format!("{v:.precision$}")
    };
    format!("{body:>width$}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_the_c_library() {
        // a float argument is widened to a double first: 0.1f32 is 0.100000001490116…
        assert_eq!(printf_f(0.1f32 as f64, 0, 9), "0.100000001");
        assert_eq!(printf_f(-1.5, 10, 4), "   -1.5000");
        assert_eq!(printf_f(f64::NAN, 6, 2), "   nan");
        assert_eq!(printf_f(-0.0, 0, 6), "-0.000000");
        assert_eq!(printf_f(f64::NEG_INFINITY, 0, 6), "-inf");
        // exact ties go to the even digit, as in the C library
        assert_eq!(printf_f(0.03125, 0, 4), "0.0312");
        assert_eq!(printf_f(0.09375, 0, 4), "0.0938");
    }
}
