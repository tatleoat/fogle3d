// SPDX-License-Identifier: MIT
//! The built-in functions of the expression language (`exp_fct.c`, finding 0013).
//!
//! Each has the C signature `(unsigned long nargs, SI_AttributeValue *args)` and returns in
//! the x87 register; the evaluator stores the result to a `float`. The functions below
//! return the register's value as the original leaves it: where the last instruction pair
//! is a store to a float and a reload, the value is float-rounded; where it is not, it is
//! the unrounded 80-bit value. The operation order and the rounding points are those of the
//! instruction sequences.
//!
//! Classes, as in finding 0008: **copy** (moves values), **model** (x87 arithmetic,
//! reproduced exactly on [`F80`]), **libm** (goes through the C library's `sin`, `cos`,
//! `tan`, `asin`, `acos`, `log`, `exp`, `powf`, `sinf`: not provable bit for bit).

use crate::scene::{EvalUndefined, Scene};
use crate::token::chapter;
use crate::tree::*;
use crate::value::AttrValue;
use si_x87::{trig, Round, F80};

pub type Builtin = fn(&mut dyn Scene, u32, &[AttrValue]) -> Result<F80, EvalUndefined>;

/// The double 0.0174532925 (0x0864fbe0, 0x0864fbe8, 0x0864fc00): degrees to radians, ten
/// digits, not π/180 to full precision.
pub const DEG2RAD: f64 = 0.0174532925;
/// The double 57.29577951308232 (0x0864fc08, 0x0864fc18, 0x0864fc28): radians to degrees.
pub const RAD2DEG: f64 = 57.29577951308232;
/// The double 6.283185307179586 (0x0864fc48, 0x0864fc58).
pub const TWO_PI: f64 = 6.283185307179586;
/// The float 1.1920929e-7 (0x0864fc30) and the double of the same value (0x0864fc40).
pub const EPSILON: f32 = 1.1920929e-7;

fn x(v: f32) -> F80 {
    F80::from_f32(v)
}

fn arg<'a, 'b>(args: &'b [AttrValue<'a>], i: usize) -> Result<&'b AttrValue<'a>, EvalUndefined> {
    args.get(i).ok_or(EvalUndefined::MissingArgument)
}

/// A C library function of one double. When a recorded call tape is running (the emulation
/// check, finding 0011) the answer is the recorded one, and the call must match the tape;
/// otherwise the host's function.
fn libm(name: &str, v: f64, host: fn(f64) -> f64) -> F80 {
    trig::replay::next(name, &[v.to_bits() as u128]).unwrap_or_else(|| F80::from_f64(host(v)))
}

/// The x87 `fprem` loop on two values that are exact doubles: the remainder of the division
/// cut toward zero, with the sign of the dividend. It is exact, so the double remainder is
/// the register's.
fn fprem(a: f64, b: f64) -> f64 {
    a % b
}

/// Whether a float has no fractional part [soft 0x08248ef4 isInteger]: its `fprem` by 1,
/// stored to a double, equals zero. False for a NaN or an infinity. Class: model.
pub fn is_integer(v: f32) -> bool {
    fprem(v as f64, 1.0) == 0.0
}

/// The reference a function argument names, as the original finds it: the argument's node,
/// its word at +4 taken for a leaf record whatever the node is, and that record's kind
/// compared with `kinds`.
fn reference<'a>(v: &AttrValue<'a>, kinds: &[i32]) -> Result<Option<(&'a NopNode, &'a ArgNode)>, EvalUndefined> {
    let AttrValue::Node(node) = v else { return Err(EvalUndefined::NodeMisread) };
    if node.get_oper() != op::LEAF {
        // +4 is the left operand; its first word is its operator
        return match node.left.as_deref() {
            Some(l) if kinds.contains(&(l.oper as i32)) => Err(EvalUndefined::NodeMisread),
            _ => Ok(None),
        };
    }
    let Some(leaf) = node.leaf.as_deref() else { return Ok(None) };
    if !kinds.contains(&leaf.kind) {
        return Ok(None);
    }
    match &leaf.data {
        NopData::Arg(a) => Ok(Some((leaf, a))),
        _ => Err(EvalUndefined::NullPointer(40)),
    }
}

/// `sin(x)`, x in degrees [soft 0x08249d14 si_exprSin]: r32(sin(r53(x × 0.0174532925))).
/// No argument: 0. Class: libm.
pub fn expr_sin(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    let a = (arg(args, 0)?.in_float() * F80::from_f64(DEG2RAD)).to_f64();
    Ok(x(trig::sin(a).to_f32()))
}

/// `cos(x)`, x in degrees [soft 0x08249d50 si_exprCos]. Class: libm.
pub fn expr_cos(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    let a = (arg(args, 0)?.in_float() * F80::from_f64(DEG2RAD)).to_f64();
    Ok(x(trig::cos(a).to_f32()))
}

/// `tan(x)`, x in degrees [soft 0x08249d8c si_exprTan]: 0 for an odd multiple of 90 (the
/// remainder by 90 is zero and the remainder by 180 is not), else
/// r32(tan(r53(x × 0.0174532925))). Class: libm.
pub fn expr_tan(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    let v = arg(args, 0)?.in_float();
    let d = v.to_f64();
    if fprem(d, 90.0) == 0.0 && fprem(d, 180.0) != 0.0 {
        return Ok(F80::ZERO);
    }
    let a = (v * F80::from_f64(DEG2RAD)).to_f64();
    Ok(x(trig::tan(a).to_f32()))
}

/// `atan(x)` in degrees [soft 0x08249e20 si_exprAtan]: r32(fpatan(x, 1) × 57.29577951308232),
/// the product at 64 bits. Class: libm (the unit's `fpatan`).
pub fn expr_atan(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    let r = trig::fpatan(arg(args, 0)?.in_float(), F80::ONE) * F80::from_f64(RAD2DEG);
    Ok(x(r.to_f32()))
}

fn arc(args: &[AttrValue], nargs: u32, f: fn(f64) -> F80) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    let v = arg(args, 0)?.in_float().to_f32();
    // outside −1..1, and for a NaN: 0
    if x(v).lt(F80::from_f64(-1.0)) || x(v).unordered(F80::from_f64(-1.0)) || F80::ONE.lt(x(v)) {
        return Ok(F80::ZERO);
    }
    Ok(x((f(v as f64) * F80::from_f64(RAD2DEG)).to_f32()))
}

/// `asin(x)` in degrees [soft 0x08249e50 si_exprAsin]: the argument rounded to a float; 0
/// outside −1..1; else r32(asin(x) × 57.29577951308232). Class: libm.
pub fn expr_asin(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    arc(args, nargs, trig::asin)
}

/// `acos(x)` in degrees [soft 0x08249eb0 si_exprAcos]. Class: libm.
pub fn expr_acos(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    arc(args, nargs, trig::acos)
}

/// `log(x)`, natural [soft 0x08249f10 si_exprLog]: the argument rounded to a float; 0
/// unless it is above 1.1920929e-7; else r32(log(x)). No error is raised. Class: libm (the
/// host's double logarithm stands in for the C library's).
pub fn expr_log(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    let v = arg(args, 0)?.in_float().to_f32();
    if !x(v).gt(x(EPSILON)) {
        return Ok(F80::ZERO);
    }
    Ok(x(libm("log", v as f64, f64::ln).to_f32()))
}

/// `exp(x)` [soft 0x08249f5c si_exprExp]: r32(exp(r53(x))). Class: libm (host's double
/// exponential).
pub fn expr_exp(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    Ok(x(libm("exp", arg(args, 0)?.in_float().to_f64(), f64::exp).to_f32()))
}

/// `abs(x)` [soft 0x08249f98 si_exprFabs]: r32(|x|). Class: model.
pub fn expr_fabs(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    Ok(x(arg(args, 0)?.in_float().abs().to_f32()))
}

/// `sqrt(x)` [soft 0x08249fc0 si_exprSqrt]: the argument rounded to a float; 0 if it is
/// negative or a NaN; else r32(√x). No error is raised. Class: model.
pub fn expr_sqrt(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    let v = x(arg(args, 0)?.in_float().to_f32());
    if !(F80::ZERO.lt(v) || F80::ZERO.eq(v)) {
        return Ok(F80::ZERO);
    }
    Ok(x(v.sqrt().to_f32()))
}

/// `cond(c, a, b)` [soft 0x0824a008 si_conditionalFct]: a if c, cut to an integer, is
/// exactly 1; else b. Fewer than three arguments: 0. Both branches have been evaluated by
/// the time the function runs. Class: copy.
pub fn conditional(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs <= 2 {
        return Ok(F80::ZERO);
    }
    let which = if arg(args, 0)?.in_int() == 1 { 1 } else { 2 };
    Ok(arg(args, which)?.in_float())
}

/// `av(a, b, …)` [soft 0x0824a054 si_averageExpression]: the sum, accumulated in the
/// register in argument order, divided by the count; unrounded. Class: model.
pub fn average(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    let mut sum = F80::ZERO;
    if nargs == 0 {
        return Ok(sum);
    }
    for i in 0..nargs as usize {
        sum = arg(args, i)?.in_float() + sum;
    }
    Ok(sum / F80::from_i64(nargs as i64))
}

/// `MIN(a, b)` [soft 0x0824a0ac si_exprMin]: starts from the largest float and takes each
/// argument unless the running value is strictly below it, so a NaN argument is taken and
/// the argument after it replaces it. Class: copy.
pub fn expr_min(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    let mut m = x(f32::MAX);
    for i in 0..nargs as usize {
        let v = arg(args, i)?.in_float();
        if !m.lt(v) {
            m = v;
        }
    }
    Ok(m)
}

/// `MAX(a, b)` [soft 0x0824a108 si_exprMax]: starts from the most negative float and takes
/// each argument unless the running value is strictly above it. Class: copy.
pub fn expr_max(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    let mut m = x(f32::MIN);
    for i in 0..nargs as usize {
        let v = arg(args, i)?.in_float();
        if !m.gt(v) {
            m = v;
        }
    }
    Ok(m)
}

/// `pow(x, y)` [soft 0x0824a160 si_exprPower], x rounded to a float: 0 for a negative x
/// with a y that (as a float) is not whole; 0 for |x| ≤ 1.1920929e-7 with a negative y; 1
/// for y = 0; else `powf(x, r32(y))`. No error is raised. Class: libm (the host's double
/// power, rounded to a float, stands in for the C library's `powf`).
pub fn expr_power(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs <= 1 {
        return Ok(F80::ZERO);
    }
    let base = arg(args, 0)?.in_float().to_f32();
    let y = arg(args, 1)?.in_float();
    // 0 > x: a negative base (a NaN goes on)
    if F80::ZERO.gt(x(base)) && !is_integer(y.to_f32()) {
        return Ok(F80::ZERO);
    }
    let mag = x(base).abs();
    let tol = F80::from_f64(EPSILON as f64);
    if (mag.lt(tol) || mag.eq(tol)) && F80::ZERO.gt(y) {
        return Ok(F80::ZERO);
    }
    if F80::ZERO.eq(y) {
        return Ok(F80::ONE);
    }
    let e = y.to_f32();
    Ok(trig::replay::next("powf", &[base.to_bits() as u128, e.to_bits() as u128])
        .unwrap_or_else(|| x((base as f64).powf(e as f64) as f32)))
}

/// `at_time(t, element.parameter)` [soft 0x0824a214 si_timeOffsetFct]: the value of the
/// parameter's function curve at time t (seconds), float-rounded; 0 if the second argument
/// is not a reference, the element is missing or it has no curve. Class: copy.
pub fn time_offset(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs <= 1 {
        return Ok(F80::ZERO);
    }
    let t = arg(args, 0)?.in_float();
    if arg(args, 1)?.kind() != 10 {
        return Ok(F80::ZERO);
    }
    let Some((leaf, a)) = reference(arg(args, 1)?, &[nop::ARG, nop::CUSTOM_ARG])? else { return Ok(F80::ZERO) };
    let v = s.fcurve_value(a, leaf.kind == nop::CUSTOM_ARG, t.to_f32());
    Ok(x(v.map_or(0.0, |v| v.to_f32())))
}

/// `at_frame(f, element.parameter)` [soft 0x0824a2fc si_frameOffsetFct]: the same at a
/// frame number (the first argument cut to an integer). Class: copy.
pub fn frame_offset(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs <= 1 {
        return Ok(F80::ZERO);
    }
    let frame = arg(args, 0)?.in_int();
    if arg(args, 1)?.kind() != 10 {
        return Ok(F80::ZERO);
    }
    let Some((leaf, a)) = reference(arg(args, 1)?, &[nop::ARG, nop::CUSTOM_ARG])? else { return Ok(F80::ZERO) };
    let v = s.fcurve_frame_value(a, leaf.kind == nop::CUSTOM_ARG, frame);
    Ok(x(v.map_or(0.0, |v| v.to_f32())))
}

/// √((dx² + dy²) + dz²) of two points, differences and products in the register, rounded
/// to a float at the end.
fn distance(p: [f32; 3], q: [f32; 3]) -> F80 {
    let dx = x(p[0]) - x(q[0]);
    let dy = x(p[1]) - x(q[1]);
    let sum = dx * dx + dy * dy;
    let dz = x(p[2]) - x(q[2]);
    x((sum + dz * dz).sqrt().to_f32())
}

/// `ctr_dist(a., b.)` [soft 0x0824a3e0 si_ctrDistFct]: the distance between the centres of
/// two elements; 0 if either argument is not a plain reference or an element is missing.
/// Class: model.
pub fn ctr_dist(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs < 2 {
        return Ok(F80::ZERO);
    }
    let (Some((_, a)), Some((_, b))) = (reference(arg(args, 0)?, &[nop::ARG])?, reference(arg(args, 1)?, &[nop::ARG])?) else {
        return Ok(F80::ZERO);
    };
    match (s.location(a), s.location(b)) {
        (Some(p), Some(q)) => Ok(distance(p, q)),
        _ => Ok(F80::ZERO),
    }
}

/// `ctr_dist_cam(a.)` [soft 0x0824a5fc si_ctrDistCamFct]: the distance from an element's
/// centre to the current camera. Class: model.
pub fn ctr_dist_cam(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    let Some((_, a)) = reference(arg(args, 0)?, &[nop::ARG])? else { return Ok(F80::ZERO) };
    match s.location(a) {
        Some(p) => Ok(distance(p, s.camera_position())),
        None => Ok(F80::ZERO),
    }
}

/// `ctr_dist_cam_int(a.)` [soft 0x0824a720 si_ctrDistCamIntFct]: the distance to the
/// current camera's interest. Class: model.
pub fn ctr_dist_cam_int(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs == 0 {
        return Ok(F80::ZERO);
    }
    let Some((_, a)) = reference(arg(args, 0)?, &[nop::ARG])? else { return Ok(F80::ZERO) };
    match s.location(a) {
        Some(p) => Ok(distance(p, s.camera_interest())),
        None => Ok(F80::ZERO),
    }
}

/// sinf(r32(fprem(frame, period) × (2π / period))): the phase of a wave of the given period
/// (in frames) at a frame. The quotient and the product are at 64 bits.
fn wave(frame: i32, period: f32) -> F80 {
    let r = fprem(frame as f64, period as f64);
    let k = F80::from_f64(TWO_PI) / x(period);
    trig::sinf((F80::from_f64(r) * k).to_f32())
}

/// `sinus(period, amplitude, [offset])` [soft 0x0824a844 si_sinusFct]: a sine of the given
/// period in frames, at the current frame plus the offset (cut to an integer), times the
/// amplitude; the product is returned unrounded. Fewer than two arguments: 0. Class: libm.
pub fn sinus(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs <= 1 {
        return Ok(F80::ZERO);
    }
    let period = arg(args, 0)?.in_float().to_f32();
    let amplitude = arg(args, 1)?.in_float().to_f32();
    let offset = if nargs > 2 { arg(args, 2)?.in_int() } else { 0 };
    let frame = offset.wrapping_add(s.frame_current());
    Ok(wave(frame, period) * x(amplitude))
}

/// The straight line through (x0, y0) and (x1, y1) at x [soft 0x0824a8e0 linear_interp]:
/// t = (x − x0)/(x1 − x0); r32(t·y1 + (1 − t)·y0); 0 if x0 = x1. Class: model.
pub fn linear_interp(xv: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> F80 {
    if x(x1).eq(x(x0)) {
        return F80::ZERO;
    }
    let t = (x(xv) - x(x0)) / (x(x1) - x(x0));
    let u = F80::ONE - t;
    x((t * x(y1) + u * x(y0)).to_f32())
}

/// The cubic with zero slope at both ends through (x0, y0) and (x1, y1) at x
/// [soft 0x0824a928 hermit_interp]. With d = x − x0 and h = 1/(x1 − x0):
/// B = 3d²h² − 2d³h³, A = (2d³h³ − 3d²h²) + 1, r32(B·y1 + A·y0); 0 if x0 = x1. The powers
/// are formed in the order of the instruction sequence. Class: model.
pub fn hermit_interp(xv: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> F80 {
    if x(x1).eq(x(x0)) {
        return F80::ZERO;
    }
    let d = x(xv) - x(x0);
    let h = F80::ONE / (x(x1) - x(x0));
    let d2 = d * d;
    let h2 = h * h;
    let d3 = d * d2;
    let t = d3 * h2;
    let c = h * (t + t);
    let p = h2 * (d2 * F80::from_f64(3.0));
    let b = p + -c;
    let a = (c - p) + F80::ONE;
    x((b * x(y1) + a * x(y0)).to_f32())
}

fn interp(kind: i32, frame: i32, sf: i32, ef: i32, y0: f32, y1: f32) -> F80 {
    // the three frame numbers go through floats
    let (f, s, e) = (frame as f32, sf as f32, ef as f32);
    if kind == 1 {
        linear_interp(f, s, e, y0, y1)
    } else {
        hermit_interp(f, s, e, y0, y1)
    }
}

/// `oscil(sf, ef, sp, ep, interp, sa, ea, interp)` [soft 0x0824a9a0 si_oscillationFct]: a
/// sine whose period goes from sp to ep and whose amplitude goes from sa to ea between
/// frames sf and ef, each along a line (interpolation 1) or the cubic (any other value).
/// 0 outside the frames, for ef ≤ sf, and with fewer than eight arguments. The sine is
/// rounded to a float before it is multiplied; the product is rounded to a float.
/// Class: libm.
pub fn oscillation(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs <= 7 {
        return Ok(F80::ZERO);
    }
    let sf = arg(args, 0)?.in_int();
    let ef = arg(args, 1)?.in_int();
    let frame = s.frame_current();
    if frame < sf || frame > ef || ef < sf {
        return Ok(F80::ZERO);
    }
    let sp = arg(args, 2)?.in_float().to_f32();
    let ep = arg(args, 3)?.in_float().to_f32();
    let ip = arg(args, 4)?.in_int();
    let sa = arg(args, 5)?.in_float().to_f32();
    let ea = arg(args, 6)?.in_float().to_f32();
    let ia = arg(args, 7)?.in_int();
    if sf == ef {
        return Ok(F80::ZERO);
    }
    let period = interp(ip, frame, sf, ef, sp, ep).to_f32();
    let sine = wave(frame, period).to_f32();
    let amplitude = interp(ia, frame, sf, ef, sa, ea);
    Ok(x((amplitude * x(sine)).to_f32()))
}

/// `ease_in(sf, ef, interp)` [soft 0x0824ab90 si_easeInFct]: 0 before frame sf, 1 after
/// frame ef, in between along the line or the cubic from 0 to 1; 1 when ef ≤ sf (tested
/// first). Fewer than three arguments: 0. Class: model.
pub fn ease_in(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs <= 2 {
        return Ok(F80::ZERO);
    }
    let sf = arg(args, 0)?.in_int();
    let ef = arg(args, 1)?.in_int();
    if ef < sf {
        return Ok(F80::ONE);
    }
    let frame = s.frame_current();
    if frame < sf {
        return Ok(F80::ZERO);
    }
    if frame > ef || sf == ef {
        return Ok(F80::ONE);
    }
    Ok(interp(arg(args, 2)?.in_int(), frame, sf, ef, 0.0, 1.0))
}

/// `ease_out(sf, ef, interp)` [soft 0x0824ac74 si_easeOutFct]: 1 before frame sf, 0 after
/// frame ef, in between r32(1 − the ease-in value); 1 when ef ≤ sf. Class: model.
pub fn ease_out(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs <= 2 {
        return Ok(F80::ZERO);
    }
    let sf = arg(args, 0)?.in_int();
    let ef = arg(args, 1)?.in_int();
    let frame = s.frame_current();
    if ef < sf || frame < sf || sf == ef {
        return Ok(F80::ONE);
    }
    if frame > ef {
        return Ok(F80::ZERO);
    }
    let v = interp(arg(args, 2)?.in_int(), frame, sf, ef, 0.0, 1.0);
    Ok(x((F80::ONE - v).to_f32()))
}

/// `noise(x, y, z)` [soft 0x0824ad6c si_noiseFct]: the texture code's solid noise at a
/// point, the three coordinates rounded to floats. The noise itself is in another source
/// file and comes through [`Scene::noise`]. Class: copy.
pub fn noise(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs <= 2 {
        return Ok(F80::ZERO);
    }
    let px = arg(args, 0)?.in_float().to_f32();
    let py = arg(args, 1)?.in_float().to_f32();
    let pz = arg(args, 2)?.in_float().to_f32();
    Ok(s.noise(px, py, pz))
}

/// `fmod(a, b)` [soft 0x0824ade0 si_fmodFct]: 0 for b = 0; else the remainder of a by b
/// (cut toward zero, sign of a), through a double and a float. Class: model.
pub fn fmod(_s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    if nargs <= 1 {
        return Ok(F80::ZERO);
    }
    let a = arg(args, 0)?.in_float().to_f64();
    let b = arg(args, 1)?.in_float();
    if F80::ZERO.eq(b) {
        return Ok(F80::ZERO);
    }
    Ok(x(fprem(a, b.to_f64()) as f32))
}

/// `has_relation(element., relation, direction)` [soft 0x0824ae44 si_hasRelFct]: 1 if the
/// element has the relation, else 0. Four ids are answered without the relation
/// dictionary: 20011 is true of every model; 20012 asks for any relation with an id from 0
/// to 14999 in one of the six groups (toward the animation chapter when the direction is
/// not 0); 20013 and 20030 are true of an animation element that is a root and has data
/// (20030: with a count above zero). Class: copy.
pub fn has_rel(s: &mut dyn Scene, nargs: u32, args: &[AttrValue]) -> Result<F80, EvalUndefined> {
    const ANIMATION: i32 = 1;
    if nargs <= 2 {
        return Ok(F80::ZERO);
    }
    let Some((_, a)) = reference(arg(args, 0)?, &[nop::ARG, nop::CUSTOM_ARG])? else { return Ok(F80::ZERO) };
    let relid = arg(args, 1)?.in_int();
    let direction = arg(args, 2)?.in_int() as i16;
    let Some(info) = s.elem_info(a) else { return Ok(F80::ZERO) };
    let yes = match relid {
        20012 => {
            let chap = if direction != 0 { ANIMATION } else { -1 };
            (0..6).any(|group| s.related_in_range(a, direction, group, 0, 14999, chap))
        }
        20011 => a.chapter == chapter::MODELS,
        20013 => a.chapter == ANIMATION && info.not_root == 0 && info.has_data,
        // (an element without data is read through a null pointer here in the original)
        20030 => a.chapter == ANIMATION && info.not_root == 0 && info.has_data && info.data_count > 0,
        _ => s.relation_check(a, direction, relid),
    };
    Ok(if yes { F80::ONE } else { F80::ZERO })
}

/// One random number [soft 0x082490f0 si_exprRandomEval]:
/// rand() × width × 2⁻³¹ + low, in the register in that order; for an integer-valued call
/// the result goes through a double, `rint` (to nearest, ties to even) and a float.
/// Class: model, given the C library's `rand`.
pub fn random_eval(s: &mut dyn Scene, r: &RandNode) -> F80 {
    let v = F80::from_i32(s.rand()) * x(r.width) * x(f32::from_bits(0x3000_0000)) + x(r.low);
    if r.real == 0 {
        let rounded = F80::from_f64(v.to_f64()).round_int(Round::Nearest);
        return x(rounded.to_f32());
    }
    v
}

/// The Rust function for an entry of the function table, by the original's address.
pub fn builtin_by_address(address: u32) -> Option<Builtin> {
    Some(match address {
        0x08249d14 => expr_sin,
        0x08249d50 => expr_cos,
        0x08249d8c => expr_tan,
        0x08249e50 => expr_asin,
        0x08249eb0 => expr_acos,
        0x08249e20 => expr_atan,
        0x08249f10 => expr_log,
        0x08249f5c => expr_exp,
        0x08249f98 => expr_fabs,
        0x08249fc0 => expr_sqrt,
        0x0824a160 => expr_power,
        0x0824a0ac => expr_min,
        0x0824a108 => expr_max,
        0x0824a054 => average,
        0x0824a008 => conditional,
        0x0824a214 => time_offset,
        0x0824a2fc => frame_offset,
        0x0824a3e0 => ctr_dist,
        0x0824a5fc => ctr_dist_cam,
        0x0824a720 => ctr_dist_cam_int,
        0x0824a844 => sinus,
        0x0824a9a0 => oscillation,
        0x0824ab90 => ease_in,
        0x0824ac74 => ease_out,
        0x0824ad6c => noise,
        0x0824ade0 => fmod,
        0x0824ae44 => has_rel,
        _ => return None,
    })
}
