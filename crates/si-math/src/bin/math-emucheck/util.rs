// SPDX-License-Identifier: MIT
//! Conversions between vector fields and the crate's types, and the comparisons the checks share.
use si_math::types::{Matrix, Matrix3, MatrixD, Vector};
use si_x87::vectors::{Case, Field, Verdict};
use si_x87::F80;

pub fn vec3(f: &Field) -> Vector {
    let v = f.f32s();
    Vector::new(v[0], v[1], v[2])
}

pub fn mat(f: &Field) -> Matrix {
    let v = f.f32s();
    core::array::from_fn(|i| core::array::from_fn(|j| v[4 * i + j]))
}

pub fn matd(f: &Field) -> MatrixD {
    let v = f.f64s();
    core::array::from_fn(|i| core::array::from_fn(|j| v[4 * i + j]))
}

pub fn mat3(f: &Field) -> Matrix3 {
    let v = f.f32s();
    core::array::from_fn(|i| core::array::from_fn(|j| v[3 * i + j]))
}

pub fn flat(m: &Matrix) -> Vec<f32> {
    m.iter().flatten().copied().collect()
}

pub fn flatd(m: &MatrixD) -> Vec<f64> {
    m.iter().flatten().copied().collect()
}

pub fn flat3(m: &Matrix3) -> Vec<f32> {
    m.iter().flatten().copied().collect()
}

/// Self-test of the checks: with `EMUCHECK_MUTATE=1` in the environment every value the port
/// produces has its lowest bit flipped before it is compared, so every compared case must
/// come out different. A file that still reports "identical" then is not being compared.
pub fn mutate() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("EMUCHECK_MUTATE").is_some())
}

fn m32(v: &[f32]) -> Vec<f32> {
    let mut v = v.to_vec();
    if mutate() {
        for e in v.iter_mut() {
            *e = f32::from_bits(e.to_bits() ^ 1);
        }
    }
    v
}

fn m64(v: &[f64]) -> Vec<f64> {
    let mut v = v.to_vec();
    if mutate() {
        for e in v.iter_mut() {
            *e = f64::from_bits(e.to_bits() ^ 1);
        }
    }
    v
}

fn hex32(v: &[f32]) -> String {
    v.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(",")
}

fn hex64(v: &[f64]) -> String {
    v.iter().map(|x| format!("{:016x}", x.to_bits())).collect::<Vec<_>>().join(",")
}

/// The floats the routine left in the buffer `name` against the port's.
pub fn f32s(case: &Case, name: &str, got: &[f32]) -> Verdict {
    let got = &m32(got);
    Verdict::from(case.after(name).same_f32(got), || format!("{name}: port {}", hex32(got)))
}

pub fn f64s(case: &Case, name: &str, got: &[f64]) -> Verdict {
    let got = &m64(got);
    Verdict::from(case.after(name).same_f64(got), || format!("{name}: port {}", hex64(got)))
}

pub fn v3(case: &Case, name: &str, got: &Vector) -> Verdict {
    f32s(case, name, &got.to_array())
}

pub fn m4(case: &Case, name: &str, got: &Matrix) -> Verdict {
    f32s(case, name, &flat(got))
}

/// The routine's integer result against the port's.
pub fn eax(case: &Case, got: i32) -> Verdict {
    let got = got ^ mutate() as i32;
    let e = case.output("eax").i32();
    Verdict::from(e == got, || format!("eax: port {got:#x}, original {e:#x}"))
}

/// A 16-bit result: only the low half of eax is defined.
pub fn ax(case: &Case, got: i16) -> Verdict {
    let got = got ^ mutate() as i16;
    let e = case.output("eax").u32() as u16 as i16;
    Verdict::from(e == got, || format!("ax: port {got:#x}, original {e:#x}"))
}

/// The routine's register result, all 80 bits.
pub fn st0(case: &Case, got: F80) -> Verdict {
    let (m, se) = got.to_bits();
    let got = F80::from_bits(m ^ mutate() as u64, se);
    Verdict::from(case.output("st0").same_f80(got), || format!("st0: port {got:?}"))
}

/// A float result: the original leaves it in st0 after rounding it to float.
pub fn st0_f32(case: &Case, got: f32) -> Verdict {
    st0(case, F80::from_f32(got))
}

/// A buffer the routine must have left as it was (every word unwritten, or unchanged).
pub fn untouched(case: &Case, name: &str) -> Verdict {
    let ok = match case.try_output(name) {
        None => true,
        Some(f) => f.written().iter().all(Option::is_none),
    };
    Verdict::from(ok, || format!("{name}: the original wrote it, the port reports nothing written"))
}

/// What a check puts into an output the port may leave alone, to see whether it did.
pub const SENTINEL: f32 = f32::from_bits(0x5ec7_15e1);

/// As [`f32s`], and where the routine wrote nothing the port must have written nothing
/// either: its buffer was primed with [`SENTINEL`].
pub fn f32s_exact(case: &Case, name: &str, got: &[f32]) -> Verdict {
    let got = &m32(got);
    let w = case.after(name).written();
    let ok = w.len() == got.len()
        && w.iter().zip(got).all(|(e, g)| match e {
            Some(e) => *e as u32 == g.to_bits(),
            None => g.to_bits() == SENTINEL.to_bits(),
        });
    Verdict::from(ok, || format!("{name}: port {} (5ec715e1 = left alone)", hex32(got)))
}

/// Text the routine wrote to a stream against the port's.
pub fn text(case: &Case, name: &str, got: &str) -> Verdict {
    let got = &if mutate() { format!("{got}?") } else { got.to_string() };
    let want = case.try_output(name).map(|f| f.u8s()).unwrap_or_default();
    Verdict::from(want == got.as_bytes(), || format!("{name}: port {got:?}"))
}

/// The double counterpart of [`SENTINEL`].
pub const SENTINEL64: f64 = f64::from_bits(0x5ec7_15e1_5ec7_15e1);

/// As [`f32s_exact`], for doubles primed with [`SENTINEL64`].
pub fn f64s_exact(case: &Case, name: &str, got: &[f64]) -> Verdict {
    let got = &m64(got);
    let w = case.after(name).written();
    let ok = w.len() == got.len()
        && w.iter().zip(got).all(|(e, g)| match e {
            Some(e) => *e as u64 == g.to_bits(),
            None => g.to_bits() == SENTINEL64.to_bits(),
        });
    Verdict::from(ok, || format!("{name}: port {} (5ec715e15ec715e1 = left alone)", hex64(got)))
}
