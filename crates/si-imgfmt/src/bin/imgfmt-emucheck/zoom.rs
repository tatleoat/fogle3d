// SPDX-License-Identifier: MIT
//! Checks for the file-to-file resizers and the filter tables (`si_imgfmt::zoom`).

use crate::util::*;
use si_imgfmt::zoom::{self, Filter, Mitchell, ZoomError};
use si_x87::vectors::{Case, Check, Verdict, Word};
use si_x87::F80;

fn clear(mut b: Vec<u8>) -> Vec<u8> {
    // the picture chunk's spare word is stale stack in the original (the generator zeroes it)
    if b.len() >= 104 {
        b[102] = 0;
        b[103] = 0;
    }
    b
}

fn said(c: &Case, ours: &[String]) -> Verdict {
    Verdict::from(messages(c) == ours, || format!("messages {:?}, port {ours:?}", messages(c)))
}

/// A file-to-file routine: result, the output file, the messages.
fn file_op(c: &Case, result: u32, name: &str, r: Result<Vec<u8>, ZoomError>, open_message: Option<&str>) -> Verdict {
    let theirs = file_out(c, "/t/o.pic").or_else(|| file_out(c, "/t/a_longer_name.pic"));
    let file = |ours: &[u8]| match &theirs {
        Some(t) => same_bytes("file", t, &clear(ours.to_vec())),
        None => Verdict::Different("the original wrote no file".into()),
    };
    match r {
        Ok(bytes) => Verdict::from(result == 1, || "the original failed".into()).and(|| file(&bytes)).and(|| said(c, &[])),
        Err(ZoomError::Open(e)) => {
            let mut m: Vec<String> = Some(e.message(name)).filter(|m| !m.is_empty()).into_iter().collect();
            m.extend(open_message.map(|t| t.replace("%s", "/t/a.pic")));
            Verdict::from(result == 0, || "the original succeeded".into())
                .and(|| Verdict::from(theirs.is_none(), || "the original wrote a file".into()))
                .and(|| said(c, &m))
        }
        Err(ZoomError::EndOfFile(bytes)) => Verdict::from(result == 0, || "the original succeeded".into())
            .and(|| file(&bytes))
            .and(|| said(c, &["Unexpected End Of File in input file!\n".to_string()])),
        Err(ZoomError::Silent(bytes)) => Verdict::from(result == 0, || "the original succeeded".into()).and(|| file(&bytes)).and(|| said(c, &[])),
        Err(ZoomError::Fault(_)) => Verdict::Skip,
    }
}

fn i16s(c: &Case, name: &str) -> Vec<i16> {
    c.input(name).i16s()
}

pub fn lookup(name: &str) -> Option<Check> {
    Some(match name {
        "mitchellInit" => |c: &Case| {
            let m = Mitchell::new(c.input("b").f32(), c.input("c").f32());
            Verdict::from(c.output("coef").same_f32(&[m.p0, m.p2, m.p3, m.q0, m.q1, m.q2, m.q3]), || format!("port {m:?}"))
        },
        "filterInteg" => |c: &Case| {
            let v = zoom::filter_integ(c.input("x").f32(), c.input("on").i32() != 0, c.input("blur").f32(), &Mitchell::standard());
            Verdict::from(c.output("st0").same_f80(F80::from_f32(v)), || format!("port {v:?}"))
        },
        "makeFilt" => |c: &Case| {
            let Ok((table, most)) = zoom::make_filt(c.input("anx").i32(), c.input("bnx").i32(), c.input("blur").f32()) else { return Verdict::Skip };
            let Some(n) = c.try_output("fn") else { return Verdict::Different("the original's table was not read".into()) };
            let flat: Vec<i16> = table.iter().flat_map(|f: &Filter| f.weights.iter().copied()).collect();
            Verdict::from(n.i32s() == table.iter().map(|f| f.n).collect::<Vec<_>>(), || "counts".into())
                .and(|| Verdict::from(c.output("ftot").i32s() == table.iter().map(|f| f.total).collect::<Vec<_>>(), || "sums".into()))
                .and(|| Verdict::from(c.output("ffirst").i32s() == table.iter().map(|f| f.first).collect::<Vec<_>>(), || "first indices".into()))
                .and(|| Verdict::from(c.try_output("fw").map(|f| f.i16s()).unwrap_or_default() == flat, || "weights".into()))
                .and(|| Verdict::from(c.output("most").i32() == most, || "largest count".into()))
        },
        "clampRow" => |c: &Case| {
            let n = c.input("n").i32().max(0) as usize;
            let mut out = vec![0u8; n];
            zoom::clamp_row(&i16s(c, "row")[..n], &mut out);
            let w = c.output("out").written();
            Verdict::from((0..24).all(|i| if i < n { w[i] == Some(out[i] as u128) } else { w[i].is_none() }), || format!("port {out:?}"))
        },
        "addRow" => |c: &Case| {
            let n = c.input("n").i32().max(0) as usize;
            let mut acc = c.input("acc").i32s();
            zoom::add_row(&mut acc[..n], &i16s(c, "row")[..n], c.input("w").i32());
            Verdict::from(c.after("acc").i32s() == acc, || format!("port {acc:?}"))
        },
        "divRow" => |c: &Case| {
            let n = c.input("n").i32().max(0) as usize;
            let mut out = vec![0i16; n];
            if zoom::div_row(&c.input("acc").i32s()[..n], &mut out, c.input("total").i32()).is_err() {
                return Verdict::Skip;
            }
            let w = c.output("out").written();
            Verdict::from((0..24).all(|i| if i < n { w[i] == Some(out[i] as u16 as u128) } else { w[i].is_none() }), || format!("port {out:?}"))
        },
        "applyXFilt" => |c: &Case| {
            let n = c.input("n").i32().max(0) as usize;
            let rec = c.input("filt").words();
            let weights = i16s(c, "w");
            let table: Vec<Filter> = (0..6)
                .map(|i| {
                    let count = rec[4 * i].value().unwrap() as u32 as i32;
                    let first = match &rec[4 * i + 2] {
                        Word::Ref { offset, .. } => *offset as i32 / 2,
                        _ => 0,
                    };
                    Filter { n: count, total: rec[4 * i + 1].value().unwrap() as u32 as i32, first, weights: weights[8 * i..8 * i + count as usize].to_vec() }
                })
                .collect();
            let mut out = vec![0i16; n];
            if zoom::apply_x_filt(&mut out, &table[..n], &i16s(c, "src")).is_err() {
                return Verdict::Skip;
            }
            let w = c.output("out").written();
            Verdict::from((0..6).all(|i| if i < n { w[i] == Some(out[i] as u16 as u128) } else { w[i].is_none() }), || format!("port {out:?}"))
        },
        "SI_filterZoom" => |c: &Case| {
            let out_name = if c.try_input("out").map(text).unwrap_or_default().contains("longer") { "/t/a_longer_name.pic" } else { "/t/o.pic" };
            let r = zoom::filter_zoom(&file_in(c, "/t/a.pic"), out_name, c.input("zx").f32(), c.input("zy").f32(), c.input("blur").f32());
            file_op(c, c.output("ax").u32() & 0xffff, "/t/a.pic", r, Some("cannot open picture file %s."))
        },
        "SI_pictureEnlargeDisk" => |c: &Case| {
            let comment = c.input("comment").to_bytes();
            let r = zoom::enlarge_disk(&file_in(c, "/t/a.pic"), false, c.input("fx").i32(), c.input("fy").i32(), Some(cstr(&comment)));
            file_op(c, c.output("eax").u32(), "/t/a.pic", r, None)
        },
        "SI_pictureReduce" => |c: &Case| {
            let comment = c.input("comment").to_bytes();
            let r = zoom::reduce(&file_in(c, "/t/a.pic"), false, c.input("fx").i32(), c.input("fy").i32(), Some(cstr(&comment)));
            file_op(c, c.output("eax").u32(), "/t/a.pic", r, None)
        },
        "0x085db2ec" => |c: &Case| {
            let ours = zoom::debug(&c.input("f").to_bytes());
            let theirs = c.try_output("stderr").map(text).unwrap_or_default();
            Verdict::from(theirs == ours, || format!("stderr {theirs:?}, port {ours:?}"))
        },
        _ => return None,
    })
}
