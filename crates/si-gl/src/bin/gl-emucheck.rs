// SPDX-License-Identifier: MIT
//! Runs a scenario of IRIS GL calls through the Rust port with the recording back end and
//! prints the transcript that `re/tools/si-gl/emu_gl.py` prints for the original routine run
//! in the emulation harness: per call the outside calls with their arguments, the return
//! value and the outputs, and on a `state` line the layer's whole state.
//! `re/tools/si-gl/compare.py` counts the agreement per entry point.
//!
//!   gl-emucheck scenario.scn > port.txt
//!
//! The scenario format is described at the top of `emu_gl.py`.

use si_gl::rec::{SharedLog, Val};
use si_gl::state::*;
use si_gl::sys::{Exit, Sys};
use si_gl::IrisGl;
use std::fmt::Write as _;
use std::panic::{catch_unwind, AssertUnwindSafe};

#[derive(Clone, Debug)]
#[allow(dead_code)]
enum Tok {
    Int(u32),
    F(u32),
    D(u64),
    Str(Vec<u8>),
    Null,
    Words(Vec<u32>),
    Shorts(Vec<u16>),
    Bytes(Vec<u8>),
    Doubles(Vec<u64>),
    Out(char, usize),
}

fn unescape(text: &str) -> Vec<u8> {
    let b = text.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1] == b'x' {
            out.push(u8::from_str_radix(&text[i + 2..i + 4], 16).unwrap());
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn int(text: &str) -> u64 {
    let (neg, t) = match text.strip_prefix('-') {
        Some(t) => (true, t),
        None => (false, text),
    };
    let v = match t.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).unwrap(),
        None => t.parse::<u64>().unwrap(),
    };
    if neg {
        v.wrapping_neg()
    } else {
        v
    }
}

fn hexbytes(t: &str) -> Vec<u8> {
    (0..t.len() / 2).map(|i| u8::from_str_radix(&t[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn list(t: &str) -> Vec<u64> {
    t.split(',').filter(|x| !x.is_empty()).map(int).collect()
}

fn tok(t: &str) -> Tok {
    if t == "null" {
        return Tok::Null;
    }
    let b = t.as_bytes();
    if b.len() > 3 && b[0] == b'o' && b[2] == b':' {
        return Tok::Out(b[1] as char, t[3..].parse().unwrap());
    }
    match t.split_once(':') {
        Some(("f", v)) => Tok::F(u32::from_str_radix(v.trim_start_matches("0x"), 16).unwrap()),
        Some(("d", v)) => Tok::D(u64::from_str_radix(v.trim_start_matches("0x"), 16).unwrap()),
        Some(("s", v)) => Tok::Str(unescape(v)),
        Some(("w", v)) | Some(("F", v)) => Tok::Words(list(v).into_iter().map(|x| x as u32).collect()),
        Some(("h", v)) => Tok::Shorts(list(v).into_iter().map(|x| x as u16).collect()),
        Some(("b", v)) => Tok::Bytes(hexbytes(v)),
        Some(("D", v)) => Tok::Doubles(list(v)),
        _ => Tok::Int(int(t) as u32),
    }
}

/// A scenario value for `answer`/`always` (the words of a `w:` list are hexadecimal).
fn val(t: &str) -> Val {
    match t.split_once(':') {
        Some(("f", v)) => Val::F(f32::from_bits(u32::from_str_radix(v, 16).unwrap())),
        Some(("d", v)) => Val::D(f64::from_bits(u64::from_str_radix(v, 16).unwrap())),
        Some(("s", v)) => Val::S(unescape(v)),
        Some(("w", v)) => Val::W(v.split(',').filter(|x| !x.is_empty()).map(|x| u32::from_str_radix(x, 16).unwrap()).collect()),
        Some(("b", v)) => Val::B(hexbytes(v)),
        _ => Val::U(int(t) as u32),
    }
}

struct A<'a>(&'a [Tok]);

impl A<'_> {
    fn u(&self, n: usize) -> u32 {
        match &self.0[n] {
            Tok::Int(v) | Tok::F(v) => *v,
            t => panic!("argument {n}: a word expected, got {t:?}"),
        }
    }
    fn i(&self, n: usize) -> i32 {
        self.u(n) as i32
    }
    fn s(&self, n: usize) -> i16 {
        self.u(n) as i16
    }
    fn h(&self, n: usize) -> u16 {
        self.u(n) as u16
    }
    fn f(&self, n: usize) -> f32 {
        f32::from_bits(self.u(n))
    }
    fn str(&self, n: usize) -> Option<&[u8]> {
        match &self.0[n] {
            Tok::Str(s) => Some(s),
            Tok::Null => None,
            t => panic!("argument {n}: a string expected, got {t:?}"),
        }
    }
    fn words(&self, n: usize) -> &[u32] {
        match &self.0[n] {
            Tok::Words(w) => w,
            t => panic!("argument {n}: words expected, got {t:?}"),
        }
    }
    fn floats<const N: usize>(&self, n: usize) -> [f32; N] {
        let w = self.words(n);
        std::array::from_fn(|i| f32::from_bits(w[i]))
    }
    fn ints<const N: usize>(&self, n: usize) -> [i32; N] {
        let w = self.words(n);
        std::array::from_fn(|i| w[i] as i32)
    }
    fn shorts(&self, n: usize) -> Vec<u16> {
        match &self.0[n] {
            Tok::Shorts(w) => w.clone(),
            t => panic!("argument {n}: shorts expected, got {t:?}"),
        }
    }
    fn sshorts<const N: usize>(&self, n: usize) -> [i16; N] {
        let w = self.shorts(n);
        std::array::from_fn(|i| w[i] as i16)
    }
    fn doubles<const N: usize>(&self, n: usize) -> [f64; N] {
        match &self.0[n] {
            Tok::Doubles(w) => std::array::from_fn(|i| f64::from_bits(w[i])),
            t => panic!("argument {n}: doubles expected, got {t:?}"),
        }
    }
    fn bytes(&self, n: usize) -> &[u8] {
        match &self.0[n] {
            Tok::Bytes(b) => b,
            t => panic!("argument {n}: bytes expected, got {t:?}"),
        }
    }
    fn out(&self, n: usize) -> usize {
        match &self.0[n] {
            Tok::Out(_, k) => *k,
            Tok::Null => 0,
            t => panic!("argument {n}: an output expected, got {t:?}"),
        }
    }
    fn matrix(&self, n: usize) -> [[f32; 4]; 4] {
        let f: [f32; 16] = self.floats(n);
        std::array::from_fn(|r| std::array::from_fn(|c| f[r * 4 + c]))
    }
}

fn ow(words: &[u32]) -> String {
    format!("= out w:[{}]", words.iter().map(|w| format!("{w:x}")).collect::<Vec<_>>().join(","))
}
fn oh(shorts: &[i16]) -> String {
    format!("= out h:[{}]", shorts.iter().map(|w| format!("{:x}", *w as u16)).collect::<Vec<_>>().join(","))
}
fn ob(bytes: &[u8]) -> String {
    format!("= out b:{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// Calls one entry point. Returns what `eax` holds at the original's return (for the calls
/// that return something) and appends the output lines in argument order.
fn dispatch(g: &mut IrisGl, name: &str, a: &A, o: &mut Vec<String>) -> Option<u32> {
    let none = |_: ()| -> Option<u32> { None };
    let pair = |o: &mut Vec<String>, r: Option<(i32, i32)>| {
        let (x, y) = r.unwrap_or((0, 0));
        o.push(ow(&[x as u32]));
        o.push(ow(&[y as u32]));
    };
    let quad = |o: &mut Vec<String>, r: (i16, i16, i16, i16)| {
        for v in [r.0, r.1, r.2, r.3] {
            o.push(oh(&[v]));
        }
    };
    match name {
        // context.c
        "CTX_getContext" => none(g.ctx_get()),
        "CTX_getContextNG" => none(g.ctx_get_ng()),
        // window.c
        "winopen" => Some(g.winopen(a.str(0).unwrap()) as u32),
        "swinopen" => Some(g.swinopen(a.i(0)) as u32),
        "winclose" => none(g.winclose(a.i(0))),
        "foreground" => none(g.foreground()),
        "minsize" => none(g.minsize(a.i(0), a.i(1))),
        "maxsize" => none(g.maxsize(a.i(0), a.i(1))),
        "prefsize" => none(g.prefsize(a.i(0), a.i(1))),
        "prefposition" => none(g.prefposition(a.i(0), a.i(1), a.i(2), a.i(3))),
        "keepaspect" => none(g.keepaspect(a.i(0), a.i(1))),
        "stepunit" => none(g.stepunit(a.i(0), a.i(1))),
        "fudge" => none(g.fudge(a.i(0), a.i(1))),
        "noborder" => none(g.noborder()),
        "noport" => none(g.noport()),
        "winconstraints" => none(g.winconstraints()),
        "windepth" => Some(g.windepth() as u32),
        "winpop" => none(g.winpop()),
        "winpush" => none(g.winpush()),
        "winget" => Some(g.winget() as u32),
        "winset" => none(g.winset(a.i(0))),
        "wintitle" => none(g.wintitle(a.str(0).unwrap())),
        "winmove" => none(g.winmove(a.i(0), a.i(1))),
        "winposition" => none(g.winposition(a.i(0), a.i(1), a.i(2), a.i(3))),
        "getorigin" => {
            let r = g.getorigin();
            pair(o, r);
            None
        }
        "getsize" => {
            let r = g.getsize();
            pair(o, r);
            None
        }
        // event.c
        "qenter" => none(g.qenter(a.h(0), a.s(1))),
        "qreset" => none(g.qreset()),
        "qtest" => Some(g.qtest() as u32),
        "qread" => {
            let (dev, v) = g.qread();
            o.push(oh(&[v]));
            Some(dev as u32)
        }
        "qdevice" => none(g.qdevice(a.h(0))),
        "unqdevice" => none(g.unqdevice(a.h(0))),
        "tie" => none(g.tie(a.h(0), a.h(1), a.h(2))),
        "getbutton" => Some(g.getbutton(a.h(0)) as u32),
        "getvaluator" => Some(g.getvaluator(a.h(0)) as u32),
        "getdev" => {
            let n = a.i(0).max(0) as usize;
            let devs = a.shorts(1);
            let mut vals = vec![0i16; a.out(2)];
            g.getdev(&devs[..n], &mut vals);
            o.push(oh(&vals));
            None
        }
        "qcontrol" => {
            let idata: Vec<i16> = a.shorts(2).iter().map(|v| *v as i16).collect();
            let mut odata = vec![0i16; a.out(4)];
            let icnt = (a.i(1).max(0) as usize).min(idata.len());
            let ocnt = (a.i(3).max(0) as usize).min(odata.len());
            let r = g.qcontrol(a.i(0), &idata[..icnt], &mut odata[..ocnt]);
            o.push(oh(&odata));
            Some(r as u32)
        }
        "setvaluator" => none(g.setvaluator(a.h(0), a.s(1), a.s(2), a.s(3))),
        // graphics.c
        "cmode" => none(g.cmode()),
        "RGBmode" => none(g.rgbmode()),
        "acsize" => none(g.acsize()),
        "stensize" => none(g.stensize(a.i(0))),
        "mssize" => none(g.mssize()),
        "zbsize" => none(g.zbsize(a.i(0))),
        "RGBsize" => none(g.rgbsize(a.i(0))),
        "doublebuffer" => none(g.doublebuffer()),
        "singlebuffer" => none(g.singlebuffer()),
        "monobuffer" => none(g.monobuffer()),
        "stereobuffer" => none(g.stereobuffer()),
        "multimap" => none(g.multimap()),
        "onemap" => none(g.onemap()),
        "overlay" => none(g.overlay()),
        "underlay" => none(g.underlay()),
        "gconfig" => none(g.gconfig()),
        "pushviewport" => none(g.pushviewport()),
        "popviewport" => none(g.popviewport()),
        "getviewport" => {
            let r = g.getviewport();
            quad(o, r);
            None
        }
        "reshapeviewport" => none(g.reshapeviewport()),
        "viewport" => none(g.viewport(a.s(0), a.s(1), a.s(2), a.s(3))),
        "getscrmask" => {
            let r = g.getscrmask();
            quad(o, r);
            None
        }
        "scrmask" => none(g.scrmask(a.s(0), a.s(1), a.s(2), a.s(3))),
        "window" => none(g.window(a.f(0), a.f(1), a.f(2), a.f(3), a.f(4), a.f(5))),
        "ortho" => none(g.ortho(a.f(0), a.f(1), a.f(2), a.f(3), a.f(4), a.f(5))),
        "ortho2" => none(g.ortho2(a.f(0), a.f(1), a.f(2), a.f(3))),
        "perspective" => none(g.perspective(a.s(0), a.f(1), a.f(2), a.f(3))),
        "mmode" => none(g.mmode(a.h(0))),
        "getmmode" => Some(g.getmmode() as u32),
        "loadmatrix" => none(g.loadmatrix(&a.matrix(0))),
        "getmatrix" => {
            let mut m = [[0.0f32; 4]; 4];
            g.getmatrix(&mut m);
            o.push(ow(&m.iter().flatten().map(|f| f.to_bits()).collect::<Vec<_>>()));
            None
        }
        "screenspace" => none(g.screenspace()),
        "multmatrix" => none(g.multmatrix(&a.matrix(0))),
        "pushmatrix" => none(g.pushmatrix()),
        "popmatrix" => none(g.popmatrix()),
        "lookat" => none(g.lookat(a.f(0), a.f(1), a.f(2), a.f(3), a.f(4), a.f(5), a.s(6))),
        "rot" => none(g.rot(a.f(0), a.u(1) as u8)),
        "rotate" => none(g.rotate(a.s(0), a.u(1) as u8)),
        "translate" => none(g.translate(a.f(0), a.f(1), a.f(2))),
        "scale" => none(g.scale(a.f(0), a.f(1), a.f(2))),
        "getgdesc" => Some(g.getgdesc(a.i(0)) as u32),
        "getgconfig" => Some(g.getgconfig(a.i(0)) as u32),
        "defpattern" => none(g.defpattern(a.s(0), a.s(1), &a.shorts(2))),
        "setpattern" => none(g.setpattern(a.s(0))),
        "getpattern" => Some(g.getpattern() as u32),
        "deflinestyle" => none(g.deflinestyle(a.s(0), a.h(1))),
        "setlinestyle" => none(g.setlinestyle(a.s(0))),
        "getlstyle" => Some(g.getlstyle() as u32),
        "lsrepeat" => none(g.lsrepeat(a.i(0))),
        "getlsrepeat" => Some(g.getlsrepeat() as u32),
        "color" => none(g.color(a.h(0))),
        "colorf" => none(g.colorf(a.f(0))),
        "getcolor" => Some(g.getcolor() as u32),
        "writemask" => none(g.writemask(a.h(0))),
        "getwritemask" => Some(g.getwritemask() as u32),
        "mapcolor" => none(g.mapcolor(a.h(0), a.s(1), a.s(2), a.s(3))),
        "getmcolor" => {
            let (r, gr, b) = g.getmcolor(a.h(0)).unwrap_or((0, 0, 0));
            for v in [r, gr, b] {
                o.push(oh(&[v as i16]));
            }
            None
        }
        "cpack" => none(g.cpack(a.u(0))),
        "RGBcolor" => none(g.rgbcolor(a.s(0), a.s(1), a.s(2))),
        "gRGBcolor" => {
            let (r, gr, b) = g.grgbcolor().unwrap_or((0, 0, 0));
            for v in [r, gr, b] {
                o.push(oh(&[v]));
            }
            None
        }
        "lRGBrange" => none(g.lrgbrange(a.s(0), a.s(1), a.s(2), a.s(3), a.s(4), a.s(5), a.i(6), a.i(7))),
        "c3f" => none(g.c3f(&a.floats(0))),
        "c3i" => none(g.c3i(&a.ints(0))),
        "c3s" => none(g.c3s(&a.sshorts(0))),
        "c4f" => none(g.c4f(&a.floats(0))),
        "c4i" => none(g.c4i(&a.ints(0))),
        "c4s" => none(g.c4s(&a.sshorts(0))),
        "drawmode" => none(g.drawmode(a.i(0))),
        "getdrawmode" => Some(g.getdrawmode() as u32),
        "backbuffer" => none(g.backbuffer(a.i(0))),
        "frontbuffer" => none(g.frontbuffer(a.i(0))),
        "swapbuffers" => none(g.swapbuffers()),
        "mswapbuffers" => none(g.mswapbuffers(a.i(0))),
        "getbuffer" => Some(g.getbuffer() as u32),
        "gsync" => none(g.gsync()),
        "clear" => none(g.clear()),
        "rectzoom" => none(g.rectzoom(a.f(0), a.f(1))),
        "lrectwrite" => none(g.lrectwrite(a.s(0), a.s(1), a.s(2), a.s(3), a.bytes(4))),
        "lrectread" => {
            let mut buf = vec![0u8; a.out(4)];
            let r = g.lrectread(a.s(0), a.s(1), a.s(2), a.s(3), &mut buf);
            o.push(ob(&buf));
            Some(r as u32)
        }
        "readdisplay" => {
            let mut buf = vec![0u8; a.out(4)];
            let r = g.readdisplay(a.s(0), a.s(1), a.s(2), a.s(3), &mut buf, a.i(5));
            o.push(ob(&buf));
            Some(r as u32)
        }
        "sbox" => none(g.sbox(a.f(0), a.f(1), a.f(2), a.f(3))),
        "sboxi" => none(g.sboxi(a.i(0), a.i(1), a.i(2), a.i(3))),
        "sboxs" => none(g.sboxs(a.s(0), a.s(1), a.s(2), a.s(3))),
        "rect" => none(g.rect(a.f(0), a.f(1), a.f(2), a.f(3))),
        "recti" => none(g.recti(a.i(0), a.i(1), a.i(2), a.i(3))),
        "rects" => none(g.rects(a.s(0), a.s(1), a.s(2), a.s(3))),
        "sboxf" => none(g.sboxf(a.f(0), a.f(1), a.f(2), a.f(3))),
        "sboxfi" => none(g.sboxfi(a.i(0), a.i(1), a.i(2), a.i(3))),
        "sboxfs" => none(g.sboxfs(a.s(0), a.s(1), a.s(2), a.s(3))),
        "rectf" => none(g.rectf(a.f(0), a.f(1), a.f(2), a.f(3))),
        "rectfi" => none(g.rectfi(a.i(0), a.i(1), a.i(2), a.i(3))),
        "rectfs" => none(g.rectfs(a.s(0), a.s(1), a.s(2), a.s(3))),
        "arc" => none(g.arc(a.f(0), a.f(1), a.f(2), a.s(3), a.s(4))),
        "arci" => none(g.arci(a.i(0), a.i(1), a.i(2), a.s(3), a.s(4))),
        "arcs" => none(g.arcs(a.s(0), a.s(1), a.s(2), a.s(3), a.s(4))),
        "arcf" => none(g.arcf(a.f(0), a.f(1), a.f(2), a.s(3), a.s(4))),
        "arcfi" => none(g.arcfi(a.i(0), a.i(1), a.i(2), a.s(3), a.s(4))),
        "arcfs" => none(g.arcfs(a.s(0), a.s(1), a.s(2), a.s(3), a.s(4))),
        "circ" => none(g.circ(a.f(0), a.f(1), a.f(2))),
        "circi" => none(g.circi(a.i(0), a.i(1), a.i(2))),
        "circs" => none(g.circs(a.s(0), a.s(1), a.s(2))),
        "circf" => none(g.circf(a.f(0), a.f(1), a.f(2))),
        "circfi" => none(g.circfi(a.i(0), a.i(1), a.i(2))),
        "circfs" => none(g.circfs(a.s(0), a.s(1), a.s(2))),
        "bgnpoint" => none(g.bgnpoint()),
        "endpoint" => none(g.endpoint()),
        "bgnline" => none(g.bgnline()),
        "endline" => none(g.endline()),
        "bgnclosedline" => none(g.bgnclosedline()),
        "endclosedline" => none(g.endclosedline()),
        "bgnpolygon" => none(g.bgnpolygon()),
        "endpolygon" => none(g.endpolygon()),
        "bgntmesh" => none(g.bgntmesh()),
        "endtmesh" => none(g.endtmesh()),
        "v2d" => none(g.v2d(&a.doubles(0))),
        "v2f" => none(g.v2f(&a.floats(0))),
        "v2i" => none(g.v2i(&a.ints(0))),
        "v2s" => none(g.v2s(&a.sshorts(0))),
        "v3d" => none(g.v3d(&a.doubles(0))),
        "v3f" => none(g.v3f(&a.floats(0))),
        "v3i" => none(g.v3i(&a.ints(0))),
        "v3s" => none(g.v3s(&a.sshorts(0))),
        "v4d" => none(g.v4d(&a.doubles(0))),
        "v4f" => none(g.v4f(&a.floats(0))),
        "v4i" => none(g.v4i(&a.ints(0))),
        "v4s" => none(g.v4s(&a.sshorts(0))),
        "n3f" => none(g.n3f(&a.floats(0))),
        "pntsmooth" => none(g.pntsmooth(a.u(0))),
        "linesmooth" => none(g.linesmooth(a.u(0))),
        "polysmooth" => none(g.polysmooth(a.u(0))),
        "blendfunction" => none(g.blendfunction(a.i(0), a.i(1))),
        "logicop" => none(g.logicop(a.i(0))),
        "gflush" => none(g.gflush()),
        "finish" => none(g.finish()),
        "move" => none(g.move_(a.f(0), a.f(1), a.f(2))),
        "movei" => none(g.movei(a.i(0), a.i(1), a.i(2))),
        "moves" => none(g.moves(a.s(0), a.s(1), a.s(2))),
        "move2" => none(g.move2(a.f(0), a.f(1))),
        "move2i" => none(g.move2i(a.i(0), a.i(1))),
        "move2s" => none(g.move2s(a.s(0), a.s(1))),
        "draw" => none(g.draw(a.f(0), a.f(1), a.f(2))),
        "drawi" => none(g.drawi(a.i(0), a.i(1), a.i(2))),
        "draws" => none(g.draws(a.s(0), a.s(1), a.s(2))),
        "draw2" => none(g.draw2(a.f(0), a.f(1))),
        "draw2i" => none(g.draw2i(a.i(0), a.i(1))),
        "draw2s" => none(g.draw2s(a.s(0), a.s(1))),
        "getzbuffer" => Some(g.getzbuffer() as u32),
        "zbuffer" => none(g.zbuffer(a.i(0))),
        "depthcue" => none(g.depthcue(a.i(0))),
        "lsetdepth" => none(g.lsetdepth(a.i(0), a.i(1))),
        "czclear" => none(g.czclear(a.u(0), a.i(1))),
        "zclear" => none(g.zclear()),
        "stencil" => none(g.stencil(a.i(0), a.i(1), a.i(2), a.u(3), a.i(4), a.i(5), a.i(6))),
        "swritemask" => none(g.swritemask(a.u(0))),
        "sclear" => none(g.sclear(a.i(0))),
        "mapw" => {
            let (p, q) = g.mapw(a.u(0), a.s(1), a.s(2));
            for v in p.iter().chain(q.iter()) {
                o.push(ow(&[v.to_bits()]));
            }
            None
        }
        "mapw2" => {
            let (px, py) = g.mapw2(a.u(0), a.s(1), a.s(2));
            o.push(ow(&[px.to_bits()]));
            o.push(ow(&[py.to_bits()]));
            None
        }
        "genobj" => Some(g.genobj()),
        "closeobj" => none(g.closeobj()),
        "makeobj" => none(g.makeobj(a.u(0))),
        "callobj" => none(g.callobj(a.u(0))),
        "delobj" => none(g.delobj(a.u(0))),
        "isobj" => Some(g.isobj(a.u(0)) as u32),
        "GFX_fastBlkRead" => {
            let mut buf = vec![0u8; a.out(4)];
            g.gfx_fast_blk_read(a.i(0), a.i(1), a.i(2), a.i(3), &mut buf);
            o.push(ob(&buf));
            None
        }
        "GFX_fastBlkWrite" => none(g.gfx_fast_blk_write(a.i(0), a.i(1), a.i(2), a.i(3), a.bytes(4))),
        // font.c
        "font" => none(g.font(a.s(0))),
        "getfont" => Some(g.getfont() as u32),
        "getdescender" => Some(g.getdescender() as u32),
        "getheight" => Some(g.getheight() as u32),
        "charstr" => none(g.charstr(a.str(0))),
        "lcharstr" => none(g.lcharstr(a.i(0), a.str(1))),
        "strwidth" => Some(g.strwidth(a.str(0).unwrap()) as u32),
        "lstrwidth" => Some(g.lstrwidth(a.i(0), a.str(1).unwrap()) as u32),
        "defrasterfont" => none(g.defrasterfont(a.s(0), a.s(1), a.s(2), a.bytes(3), a.s(4), &a.shorts(5))),
        "deflfont" => none(g.deflfont()),
        "cmov" => none(g.cmov(a.f(0), a.f(1), a.f(2))),
        "cmov2" => none(g.cmov2(a.f(0), a.f(1))),
        "cmov2i" => none(g.cmov2i(a.i(0), a.i(1))),
        "cmov2s" => none(g.cmov2s(a.s(0), a.s(1))),
        "cmovi" => none(g.cmovi(a.i(0), a.i(1), a.i(2))),
        "cmovs" => none(g.cmovs(a.s(0), a.s(1), a.s(2))),
        "getcpos" => {
            let (px, py) = g.getcpos().unwrap_or((0, 0));
            o.push(oh(&[px]));
            o.push(oh(&[py]));
            None
        }
        // cursor.c
        "cursoff" => none(g.cursoff()),
        "curson" => none(g.curson()),
        "curstype" => none(g.curstype(a.u(0))),
        "defcursor" => none(g.defcursor(a.s(0), &a.shorts(1))),
        "curorigin" => none(g.curorigin(a.s(0), a.s(1), a.s(2))),
        "setcursor" => none(g.setcursor(a.s(0))),
        "getcursor" => {
            let (index, shown) = g.getcursor().unwrap_or((0, 0));
            o.push(oh(&[index]));
            o.push(ow(&[shown as u32]));
            None
        }
        // pick.c
        "pick" => none(g.pick(a.i(1))),
        "endpick" => {
            let mut buf = vec![0i16; a.out(0)];
            let r = g.endpick(if matches!(a.0[0], Tok::Null) { None } else { Some(&mut buf) });
            if !matches!(a.0[0], Tok::Null) {
                o.push(oh(&buf));
            }
            Some(r as u32)
        }
        "picksize" => none(g.picksize(a.s(0), a.s(1))),
        "loadname" => none(g.loadname(a.s(0))),
        "popname" => none(g.popname()),
        "pushname" => none(g.pushname(a.s(0))),
        "initnames" => none(g.initnames()),
        "gselect" => none(g.gselect(a.i(1))),
        "endselect" => {
            let mut buf = vec![0i16; a.out(0)];
            let r = g.endselect(if matches!(a.0[0], Tok::Null) { None } else { Some(&mut buf) });
            if !matches!(a.0[0], Tok::Null) {
                o.push(oh(&buf));
            }
            Some(r as u32)
        }
        // misc.c
        "GLOB_initRubberBand" => none(g.glob_init_rubber_band(a.i(0), a.i(1), a.i(2), a.i(3), a.i(4), a.i(5))),
        "GLOB_exitRubberBand" => none(g.glob_exit_rubber_band()),
        "GLOB_drawRubberBand" => none(g.glob_draw_rubber_band(a.i(0), a.i(1), a.i(2), a.i(3))),
        "GLOB_clearRubberBand" => none(g.glob_clear_rubber_band(a.i(0), a.i(1), a.i(2), a.i(3))),
        "GLOB_initSubregion" => none(g.glob_init_subregion(a.i(0), a.i(1), a.i(2), a.i(3))),
        "GLOB_exitSubregion" => none(g.glob_exit_subregion()),
        "GLOB_restoreSubregion" => Some(g.glob_restore_subregion(&a.sshorts(0)) as u32),
        "GLOB_moveDialog" => {
            let (r, dx, dy) = g.glob_move_dialog(a.i(0), a.i(1));
            o.push(ow(&[dx as u32]));
            o.push(ow(&[dy as u32]));
            Some(r as u32)
        }
        "GLOB_hideDialog" => Some(g.glob_hide_dialog(a.i(0)) as u32),
        "GLOB_unhideDialog" => Some(g.glob_unhide_dialog(a.i(0)) as u32),
        _ => {
            let stub = si_gl::unimplemented::STUBS.iter().find(|s| s.0 == name).unwrap_or_else(|| panic!("no dispatch for {name}"));
            (stub.1)(g).map(|v| v as u32)
        }
    }
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5000) {
        for &d in chunk {
            a += d as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

fn hx(words: &[u32]) -> String {
    words.iter().map(|w| format!("{w:x}")).collect::<Vec<_>>().join(",")
}

fn config(c: &GfxConfig) -> String {
    format!("{},{:x}", hx(&[c.rgb as u32, c.red as u32, c.green as u32, c.blue as u32, c.alpha as u32, c.depth as u32, c.stencil as u32]), c.double)
}

fn f64w(v: &[f64]) -> Vec<u32> {
    v.iter().flat_map(|d| [d.to_bits() as u32, (d.to_bits() >> 32) as u32]).collect()
}

/// The state as `emu_gl.py` prints it from the original's memory.
fn state(g: &IrisGl) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!("statics grab={:x},{:x} r={:x},{:x} has_sync={:x} dbg={:x}", g.grab_x, g.grab_y, g.grab_rx, g.grab_ry, g.has_sync as u32, g.dbg_level));
    let Some(c) = g.context() else {
        out.push("ctx none".into());
        return out;
    };
    let init = match &c.init_window {
        Some(i) => hx(&[i.window, i.visual_info, i.colormap, i.glx_context]),
        None => "none".into(),
    };
    out.push(format!(
        "ctx display={:x} root={:x} screen={:x} fixes={:x} size={:x},{:x} atoms={} init={}",
        c.display,
        c.root,
        c.screen,
        c.gl_fixes,
        c.screen_width,
        c.screen_height,
        hx(&[c.atoms.motif_wm_hints, c.atoms.wm_delete_window, c.atoms.wm_state]),
        init
    ));
    let w = &c.wnd;
    let cur = w.current.map_or(0xffff_ffff, |i| w.windows[i].id as u32);
    let v = [w.min_width, w.min_height, w.max_width, w.max_height, w.pos_x, w.pos_y, w.pref_width, w.pref_height, w.aspect_x, w.aspect_y, w.step_x, w.step_y, w.fudge_x, w.fudge_y];
    out.push(format!("wnd n={:x} current={:x} mask={:x} constraints={:x} v={}", w.windows.len(), cur, w.event_mask, w.constraints, hx(&v.map(|x| x as u32))));
    for win in &w.windows {
        out.push(format!(
            "win id={:x} x={:x} vi={:x} glx={:x} cmap={:x} parent={:x} geom={}",
            win.id,
            win.xwindow,
            win.visual_info,
            win.glx_context,
            win.colormap,
            win.parent,
            hx(&[win.x as u32, win.y as u32, win.width as u32, win.height as u32])
        ));
        out.push(match &win.backing {
            Some(b) => format!(" backing {}", hx(&[b.x as u32, b.y as u32, b.width as u32, b.height as u32, b.bytes_per_pixel as u32, b.bytes_per_line as u32, b.image])),
            None => " backing none".into(),
        });
        out.push(format!(" crs {}", hx(&[win.crs.cursor as u32, win.crs.visible as u32])));
        out.push(format!(" fnt {}", hx(&f64w(&win.fnt.pos))));
        let gf = &win.gfx;
        out.push(format!(
            " gfx config={} wanted={} buffer={:x} color={:x} mask={:x} rgba={} ls={} pattern={:x} dm={} mmode={:x} pos={} overlay={:x} zoom={}",
            config(&gf.config),
            config(&gf.wanted),
            gf.buffer,
            gf.color,
            gf.writemask,
            hx(&gf.rgba.map(|x| x as u32)),
            hx(&[gf.line_style as u32, gf.ls_repeat as u32]),
            gf.pattern,
            hx(&[gf.drawmode_effective as u32, gf.drawmode as u32]),
            gf.mmode,
            hx(&f64w(&gf.pos)),
            gf.overlay,
            hx(&gf.zoom.map(|f| f.to_bits()))
        ));
        let m = &win.msc;
        let mw = [
            m.hidden_image,
            m.hidden_width as u32,
            m.hidden_height as u32,
            m.band_x as u32,
            m.band_y as u32,
            m.band_width as u32,
            m.band_height as u32,
            m.band_row_start,
            m.band_row_end,
            m.band_col_start,
            m.band_col_end,
            m.band_gc,
            m.sub_x as u32,
            m.sub_y as u32,
            m.sub_width as u32,
            m.sub_height as u32,
            m.sub_image,
        ];
        out.push(format!(" msc {} last={}", hx(&mw), m.sub_last.iter().map(|v| format!("{:x}", *v as u16)).collect::<Vec<_>>().join(",")));
        let p = &win.pck;
        let pw = [p.buffer_size, p.size_x, p.size_y, p.picking, p.selecting, p.x, p.y, p.w, p.h, p.viewport[0], p.viewport[1], p.viewport[2], p.viewport[3]];
        out.push(format!(" pck buffer={:x} {}", p.buffer as u32, hx(&pw.map(|x| x as u32))));
    }
    let e = &c.evt;
    let queue: Vec<String> = e.queue.iter().map(|q| format!("{:x}:{:x}:{:x}", q.device, q.value as u16, q.time)).collect();
    out.push(format!("evt queue=[{}]", queue.join(",")));
    let mut table = Vec::with_capacity(DEVICE_COUNT * 12);
    let mut queued = Vec::new();
    for (i, d) in e.devices.iter().enumerate() {
        table.extend_from_slice(&d.queued.to_le_bytes());
        table.extend_from_slice(&d.tie[0].to_le_bytes());
        table.extend_from_slice(&d.tie[1].to_le_bytes());
        table.extend_from_slice(&d.word8.to_le_bytes());
        table.extend_from_slice(&[0, 0]);
        if d.queued != 0 {
            queued.push(format!("{i:x}"));
        }
    }
    let keymap: Vec<u8> = e.keymap.iter().flat_map(|k| k.to_le_bytes()).collect();
    out.push(format!(
        "evt devices={:08x} queued=[{}] grabbed={:x} last={:x},{:x} keymap={:08x} keycode_of={}",
        adler32(&table),
        queued.join(","),
        e.grabbed,
        e.last_x,
        e.last_y,
        adler32(&keymap),
        hx(&e.keycode_of.iter().map(|k| *k as u32).collect::<Vec<_>>())
    ));
    let f = &c.fnt;
    out.push(format!("fnt n={:x} current={:x}", f.fonts.len(), f.fonts[f.current].id));
    for font in &f.fonts {
        let mut bytes = Vec::new();
        for ch in &font.chars {
            for v in [ch.width, ch.height, ch.xorig, ch.yorig, ch.advance] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            match &ch.bitmap {
                Some(b) => {
                    bytes.push(1);
                    bytes.extend_from_slice(b);
                }
                None => bytes.push(0),
            }
        }
        out.push(format!(" font id={:x} height={:x} descender={:x} n={:x} chars={:08x}", font.id, font.height, font.descender, font.chars.len(), adler32(&bytes)));
    }
    let gx = &c.gfx;
    out.push(format!("gfx config={} cmap={:08x}", config(&gx.config), adler32(&gx.cmap)));
    out.push(format!(" linestyles=[{}]", gx.line_styles.iter().map(|s| format!("{:x}:{:x}", s.id, s.pattern)).collect::<Vec<_>>().join(",")));
    out.push(format!(" patterns=[{}]", gx.patterns.iter().map(|p| format!("{:x}:{:08x}", p.id, adler32(&p.stipple))).collect::<Vec<_>>().join(",")));
    let cr = &c.crs;
    out.push(format!("crs type={:x} n={:x} reverse={:08x}", cr.size_type, cr.cursors.len(), adler32(&cr.bit_reverse)));
    for cu in &cr.cursors {
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        out.push(format!(
            " cursor id={:x} src={:x} mask={:x} x={:x} fg={} bg={} size={} hot={}",
            cu.id,
            cu.source,
            cu.mask,
            cu.xcursor,
            hex(&cu.fg),
            hex(&cu.bg),
            hx(&[cu.width as u32, cu.height as u32]),
            hx(&[cu.hot_x as u32, cu.hot_y as u32])
        ));
    }
    out
}

fn run(path: &str, g: &mut IrisGl, log: &SharedLog, text: &mut String) {
    let scenario = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    for raw in scenario.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        match toks[0] {
            "answer" | "always" => {
                let name = toks[1];
                let vals: Vec<Val> = toks[2..].iter().map(|t| val(t)).collect();
                if toks[0] == "answer" {
                    log.borrow_mut().answer(name, vals);
                } else {
                    log.borrow_mut().answer_always(name, vals);
                }
            }
            "call" => {
                writeln!(text, "> {line}").unwrap();
                let mut rest = &toks[2..];
                let mut ret = None;
                if let Some(last) = rest.last() {
                    if let Some(kind) = last.strip_prefix("ret:") {
                        ret = Some(kind);
                        rest = &rest[..rest.len() - 1];
                    }
                }
                let args: Vec<Tok> = rest.iter().map(|t| tok(t)).collect();
                log.borrow_mut().clear();
                let mut outs = Vec::new();
                let result = catch_unwind(AssertUnwindSafe(|| dispatch(g, toks[1], &A(&args), &mut outs)));
                for l in log.borrow().lines() {
                    writeln!(text, "  {l}").unwrap();
                }
                match result {
                    Ok(eax) => {
                        match (ret, eax) {
                            (Some("eax"), Some(v)) => writeln!(text, "= ret {v:#x}").unwrap(),
                            (Some("ax"), Some(v)) => writeln!(text, "= ret {:#x}", v & 0xffff).unwrap(),
                            (Some("al"), Some(v)) => writeln!(text, "= ret {:#x}", v & 0xff).unwrap(),
                            (Some(k), None) => panic!("{}: asked for {k} of a call that returns nothing", toks[1]),
                            _ => {}
                        }
                        for l in outs {
                            writeln!(text, "{l}").unwrap();
                        }
                    }
                    Err(payload) => {
                        let why = if payload.downcast_ref::<Exit>().is_some() {
                            "exit".to_string()
                        } else if let Some(s) = payload.downcast_ref::<String>() {
                            s.clone()
                        } else if let Some(s) = payload.downcast_ref::<&str>() {
                            s.to_string()
                        } else {
                            "panic".to_string()
                        };
                        writeln!(text, "= stop {why}").unwrap();
                    }
                }
            }
            "state" => {
                writeln!(text, "> state").unwrap();
                for l in state(g) {
                    writeln!(text, "{l}").unwrap();
                }
            }
            other => panic!("bad scenario line: {other}"),
        }
    }
}

fn main() {
    // the layer's tables come from the user's install (unit rta)
    si_rt::check_or_exit("gl-emucheck");
    std::panic::set_hook(Box::new(|_| {}));
    let (sys, log) = Sys::recording();
    let mut g = IrisGl::new(sys);
    let mut text = String::new();
    for path in std::env::args().skip(1) {
        run(&path, &mut g, &log, &mut text);
    }
    print!("{text}");
}
