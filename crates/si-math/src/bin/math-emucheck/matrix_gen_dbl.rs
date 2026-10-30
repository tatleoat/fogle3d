// SPDX-License-Identifier: MIT
//! Checks for `matrixGenDbl.c` (`si_math::matrix_gen_dbl`). Vectors: `re/tools/si-math/emu/gen_gendbl.py`.
//!
//! A matrix of a case is three fields: `a` (rows, columns, pointer), `arows` (the table of row
//! pointers) and `ad` (the elements, rows one after another).
use crate::util::*;
use si_math::matrix_gen_dbl as g;
use si_math::matrix_gen_dbl::GenMatrixDbl;
use si_x87::vectors::{Case, Check, Verdict, Word};

/// The matrix `name` as the case hands it in.
fn inm(c: &Case, name: &str) -> GenMatrixDbl {
    let (rows, cols) = dims(c, name);
    GenMatrixDbl { rows, cols, data: c.input(&format!("{name}d")).f64s() }
}

fn dims(c: &Case, name: &str) -> (usize, usize) {
    let w = c.input(name).words();
    (w[0].value().unwrap() as usize, w[1].value().unwrap() as usize)
}

/// An output matrix nobody has written yet: the routine's `:o` buffer.
fn primed(c: &Case, name: &str) -> GenMatrixDbl {
    let (rows, cols) = dims(c, name);
    GenMatrixDbl { rows, cols, data: vec![SENTINEL64; rows * cols] }
}

/// The elements of matrix `name` after the call against the port's.
fn data(c: &Case, name: &str, got: &GenMatrixDbl) -> Verdict {
    f64s_exact(c, &format!("{name}d"), &got.data)
}

/// A matrix the routine allocated and returned: null against `None`, else the dimensions
/// and every byte the original wrote (the rest of a fresh block is not defined).
fn returned(c: &Case, got: Option<GenMatrixDbl>) -> Verdict {
    match (c.try_output("ret"), got) {
        (Some(f), None) => Verdict::from(f.is_null() != mutate(), || "the port returns no matrix".into()),
        (Some(_), Some(_)) => Verdict::Different("the original returns null, the port a matrix".into()),
        (None, None) => Verdict::Different("the original returns a matrix, the port none".into()),
        (None, Some(m)) => {
            let d = c.output("ret.dims").u32s();
            let bytes: Vec<u8> = m.data.iter().flat_map(|v| v.to_bits().to_le_bytes()).map(|b| b ^ mutate() as u8).collect();
            let w = c.output("ret.data").written();
            let ok = d == [m.rows as u32, m.cols as u32]
                && w.len() == bytes.len()
                && w.iter().zip(&bytes).all(|(e, b)| e.map_or(true, |e| e as u8 == *b));
            Verdict::from(ok, || format!("port {} x {}: {:x?}", m.rows, m.cols, bytes))
        }
    }
}

fn eax_bool(c: &Case, got: bool) -> Verdict {
    eax(c, got as i32)
}

fn refs(c: &Case, name: &str) -> Vec<String> {
    c.try_output(name)
        .map(|f| {
            f.words()
                .iter()
                .map(|w| match w {
                    Word::Ref { name, offset: 0 } => name.clone(),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "SI_generalMatrixAllocateDbl" => |c: &Case| returned(c, g::general_matrix_allocate_dbl(c.input("rows").i32(), c.input("cols").i32())),
        "SI_generalMatrixIdentityDbl" => |c: &Case| returned(c, g::general_matrix_identity_dbl(c.input("n").i32())),
        "SI_generalMatrixCopyDbl" => |c: &Case| {
            if c.input("a").is_null() {
                returned(c, g::general_matrix_copy_dbl(None))
            } else {
                returned(c, g::general_matrix_copy_dbl(Some(&inm(c, "a"))))
            }
        },
        // the port drops the matrix; what can be compared is that the caller's pointer is
        // cleared and that the original frees every row it has, then the table, then the
        // structure (the vector lists the freed blocks in order)
        "SI_generalMatrixDisposeDbl" => |c: &Case| {
            let mut m = if c.try_input("m").is_some() { Some(GenMatrixDbl { rows: 0, cols: 0, data: Vec::new() }) } else { None };
            let had = m.is_some();
            g::general_matrix_dispose_dbl(&mut m);
            let mut want: Vec<String> = Vec::new();
            if had {
                want = c.input("t").words().iter().filter_map(|w| if let Word::Ref { name, .. } = w { Some(name.clone()) } else { None }).collect();
                want.push("t".into());
                want.push("m".into());
            }
            let cleared = c.output("pp").words() == [Word::Value { bytes: 4, value: 0 }];
            Verdict::from(m.is_none() && cleared && refs(c, "freed") == want, || format!("freed {:?}, expected {want:?}", refs(c, "freed")))
        },
        "SI_generalMatrixAddDbl" => |c: &Case| {
            let mut out = primed(c, "a");
            g::general_matrix_add_dbl(&inm(c, "a"), &inm(c, "b"), &mut out);
            data(c, if c.try_input("o").is_some() { "o" } else { "a" }, &out)
        },
        "SI_generalMatrixSubtractDbl" => |c: &Case| {
            let mut out = primed(c, "a");
            g::general_matrix_subtract_dbl(&inm(c, "a"), &inm(c, "b"), &mut out);
            data(c, if c.try_input("o").is_some() { "o" } else { "a" }, &out)
        },
        "SI_generalMatrixCopyElement" => |c: &Case| {
            let mut out = primed(c, "o");
            g::general_matrix_copy_element(&inm(c, "a"), &mut out);
            data(c, "o", &out)
        },
        "SI_generalMatrixMultiplyDbl" => |c: &Case| {
            let mut out = primed(c, "o");
            g::general_matrix_multiply_dbl(&inm(c, "a"), &inm(c, "b"), &mut out);
            data(c, "o", &out)
        },
        "SI_generalMatrixMultiplyDbl#square" => |c: &Case| {
            let mut out = primed(c, "o");
            g::general_matrix_multiply_dbl(&inm(c, "a"), &inm(c, "a"), &mut out);
            data(c, "o", &out)
        },
        "SI_generalMatrixMultiplyDbl#into-a" => |c: &Case| {
            let mut a = inm(c, "a");
            g::general_matrix_multiply_dbl_into_a(&mut a, &inm(c, "b"));
            data(c, "a", &a)
        },
        "SI_generalMatrixMultiplyDbl#into-b" => |c: &Case| {
            let mut b = inm(c, "b");
            g::general_matrix_multiply_dbl_into_b(&inm(c, "a"), &mut b);
            data(c, "b", &b)
        },
        "SI_generalMatrixTransposeDbl" => |c: &Case| {
            let mut out = primed(c, "o");
            g::general_matrix_transpose_dbl(&inm(c, "a"), &mut out);
            data(c, "o", &out)
        },
        "SI_generalMatrixTransposeDbl#inplace" => |c: &Case| {
            let mut a = inm(c, "a");
            g::general_matrix_transpose_dbl_in_place(&mut a);
            data(c, "a", &a)
        },
        "SI_generalMatrixPrintDbl" => |c: &Case| {
            let name = String::from_utf8(c.input("name").u8s()).unwrap();
            let m = if c.input("m").is_null() { None } else { Some(inm(c, "m")) };
            text(c, "stdout", &g::general_matrix_print_dbl(&name, m.as_ref()))
        },
        "SI_generalMatrixInverse2Dbl" => |c: &Case| {
            let mut out = primed(c, "o");
            let ok = g::general_matrix_inverse2_dbl(&inm(c, "a"), &mut out);
            eax_bool(c, ok).and(|| data(c, "o", &out))
        },
        "SI_generalMatrixInverse2Dbl#inplace" => |c: &Case| {
            let mut a = inm(c, "a");
            let ok = g::general_matrix_inverse2_dbl_in_place(&mut a);
            eax_bool(c, ok).and(|| data(c, "a", &a))
        },
        "SI_generalMatrixInverse3Dbl" => |c: &Case| {
            let mut out = primed(c, "o");
            let ok = g::general_matrix_inverse3_dbl(&inm(c, "a"), &mut out);
            eax_bool(c, ok).and(|| data(c, "o", &out))
        },
        "SI_generalMatrixInverse3Dbl#inplace" => |c: &Case| {
            let mut a = inm(c, "a");
            let ok = g::general_matrix_inverse3_dbl_in_place(&mut a);
            eax_bool(c, ok).and(|| data(c, "a", &a))
        },
        "SI_generalMatrixPseudoInverseDbl" => |c: &Case| {
            let mut out = primed(c, "o");
            let ok = g::general_matrix_pseudo_inverse_dbl(&inm(c, "a"), &mut out);
            eax_bool(c, ok).and(|| data(c, "o", &out))
        },
        "vtnAnormDbl" => |c: &Case| {
            let n = c.input("n").i32().max(0) as usize;
            st0(c, g::vtn_anorm_dbl(&c.input("v").f64s()[..n]))
        },
        "vtnInvDbl" => |c: &Case| {
            let v = c.input("v").f64s();
            let mut out = vec![SENTINEL64; v.len()];
            g::vtn_inv_dbl(&v, &mut out);
            f64s_exact(c, if c.try_output("out").is_some() { "out" } else { "v" }, &out)
        },
        "SI_generalMatrixGrevilleInverseDbl" => |c: &Case| {
            let mut out = primed(c, "o");
            let ok = g::general_matrix_greville_inverse_dbl(&inm(c, "a"), &mut out);
            eax_bool(c, ok).and(|| data(c, "o", &out))
        },
        _ => return None,
    })
}
