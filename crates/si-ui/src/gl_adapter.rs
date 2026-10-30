// SPDX-License-Identifier: MIT
//! The IRIS GL calls of `event.c` and `window.c` on the IRIS GL layer of `crates/si-gl`.
//!
//! [`crate::sys::Gl`] lists the IRIS GL entry points the two files call, with the original's
//! arguments. Here each one is handed to the method of the same name of
//! [`si_gl::IrisGl`], which is the port of the original's own layer (finding 0016): a call
//! of `qread` in this crate is a call of `qread` there, exactly as in the original. Nothing
//! is added or left out; the only work is where IRIS GL passes arrays through pointers
//! (`getdev`, `qcontrol`, `v2i`, strings), which are read from and written to the memory
//! image.

use crate::mem::{Addr, Mem};
use crate::sys::Gl;
use si_gl::IrisGl;

/// The layer as the toolkit's IRIS GL.
pub struct SiGl(pub IrisGl);

impl Gl for SiGl {
    fn qtest(&mut self, _: &mut Mem) -> i32 {
        self.0.qtest() as i32
    }
    fn qread(&mut self, _: &mut Mem, value: &mut i16) -> i32 {
        let (device, v) = self.0.qread();
        *value = v;
        device as i32
    }
    fn qenter(&mut self, _: &mut Mem, device: u16, value: i16) {
        self.0.qenter(device, value)
    }
    fn qdevice(&mut self, _: &mut Mem, device: u16) {
        self.0.qdevice(device)
    }
    fn unqdevice(&mut self, _: &mut Mem, device: u16) {
        self.0.unqdevice(device)
    }
    fn tie(&mut self, _: &mut Mem, button: u16, valuator1: u16, valuator2: u16) {
        self.0.tie(button, valuator1, valuator2)
    }
    fn getbutton(&mut self, _: &mut Mem, device: u16) -> i32 {
        self.0.getbutton(device)
    }
    fn getvaluator(&mut self, _: &mut Mem, device: u16) -> i32 {
        self.0.getvaluator(device)
    }
    fn winset(&mut self, _: &mut Mem, id: i32) {
        self.0.winset(id)
    }
    fn winget(&mut self, _: &mut Mem) -> i32 {
        self.0.winget()
    }
    fn drawmode(&mut self, _: &mut Mem, mode: i32) {
        self.0.drawmode(mode)
    }
    fn getdrawmode(&mut self, _: &mut Mem) -> i32 {
        self.0.getdrawmode()
    }
    fn frontbuffer(&mut self, _: &mut Mem, enable: i32) {
        self.0.frontbuffer(enable)
    }
    fn backbuffer(&mut self, _: &mut Mem, enable: i32) {
        self.0.backbuffer(enable)
    }
    fn getbuffer(&mut self, _: &mut Mem) -> i32 {
        self.0.getbuffer()
    }
    fn swapbuffers(&mut self, _: &mut Mem) {
        self.0.swapbuffers()
    }
    fn gflush(&mut self, _: &mut Mem) {
        self.0.gflush()
    }
    fn gsync(&mut self, _: &mut Mem) {
        self.0.gsync()
    }
    fn color(&mut self, _: &mut Mem, index: u16) {
        self.0.color(index)
    }
    fn clear(&mut self, _: &mut Mem) {
        self.0.clear()
    }
    fn writemask(&mut self, _: &mut Mem, mask: u16) {
        self.0.writemask(mask)
    }
    fn getwritemask(&mut self, _: &mut Mem) -> i32 {
        self.0.getwritemask() as i32
    }
    fn font(&mut self, _: &mut Mem, index: i16) {
        self.0.font(index)
    }
    fn getdescender(&mut self, _: &mut Mem) -> i32 {
        self.0.getdescender()
    }
    fn strwidth(&mut self, _: &mut Mem, text: &[u8]) -> i32 {
        self.0.strwidth(text)
    }
    fn charstr(&mut self, _: &mut Mem, text: &[u8]) {
        self.0.charstr(Some(text))
    }
    fn cmov2i(&mut self, _: &mut Mem, x: i32, y: i32) {
        self.0.cmov2i(x, y)
    }
    fn recti(&mut self, _: &mut Mem, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.0.recti(x1, y1, x2, y2)
    }
    fn sboxi(&mut self, _: &mut Mem, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.0.sboxi(x1, y1, x2, y2)
    }
    fn sboxfi(&mut self, _: &mut Mem, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.0.sboxfi(x1, y1, x2, y2)
    }
    fn sboxs(&mut self, _: &mut Mem, x1: i16, y1: i16, x2: i16, y2: i16) {
        self.0.sboxs(x1, y1, x2, y2)
    }
    fn setpattern(&mut self, _: &mut Mem, index: i16) {
        self.0.setpattern(index)
    }
    fn pushviewport(&mut self, _: &mut Mem) {
        self.0.pushviewport()
    }
    fn popviewport(&mut self, _: &mut Mem) {
        self.0.popviewport()
    }
    fn pushmatrix(&mut self, _: &mut Mem) {
        self.0.pushmatrix()
    }
    fn popmatrix(&mut self, _: &mut Mem) {
        self.0.popmatrix()
    }
    fn ortho2(&mut self, _: &mut Mem, left: f32, right: f32, bottom: f32, top: f32) {
        self.0.ortho2(left, right, bottom, top)
    }
    fn viewport(&mut self, _: &mut Mem, left: i16, right: i16, bottom: i16, top: i16) {
        self.0.viewport(left, right, bottom, top)
    }
    fn bgnline(&mut self, _: &mut Mem) {
        self.0.bgnline()
    }
    fn endline(&mut self, _: &mut Mem) {
        self.0.endline()
    }
    fn move2s(&mut self, _: &mut Mem, x: i16, y: i16) {
        self.0.move2s(x, y)
    }
    fn draw2s(&mut self, _: &mut Mem, x: i16, y: i16) {
        self.0.draw2s(x, y)
    }
    fn setvaluator(&mut self, _: &mut Mem, device: u16, init: i16, min: i16, max: i16) {
        self.0.setvaluator(device, init, min, max)
    }
    fn getdev(&mut self, m: &mut Mem, n: i32, devices: Addr, values: &mut [i16]) {
        let devs: Vec<u16> = (0..n.max(0) as u32).map(|i| m.u16(devices + 2 * i)).collect();
        self.0.getdev(&devs, values)
    }
    fn qcontrol(&mut self, m: &mut Mem, cmd: i32, idata: Option<&[i16]>, ocount: i32, odata: Addr) -> i32 {
        let mut out = vec![0i16; ocount.max(0) as usize];
        let r = self.0.qcontrol(cmd, idata.unwrap_or(&[]), &mut out);
        for (i, v) in out.iter().enumerate() {
            m.set_i16(odata + 2 * i as u32, *v);
        }
        r
    }
    fn v2i(&mut self, _: &mut Mem, v: [i32; 2]) {
        self.0.v2i(&v)
    }
    fn v2s(&mut self, _: &mut Mem, v: [i16; 2]) {
        self.0.v2s(&v)
    }
    fn bgnclosedline(&mut self, _: &mut Mem) {
        self.0.bgnclosedline()
    }
    fn endclosedline(&mut self, _: &mut Mem) {
        self.0.endclosedline()
    }
    fn bgnpolygon(&mut self, _: &mut Mem) {
        self.0.bgnpolygon()
    }
    fn endpolygon(&mut self, _: &mut Mem) {
        self.0.endpolygon()
    }
    fn RGBcolor(&mut self, _: &mut Mem, red: i16, green: i16, blue: i16) {
        self.0.rgbcolor(red, green, blue)
    }
    fn deflinestyle(&mut self, _: &mut Mem, index: i16, pattern: u16) {
        self.0.deflinestyle(index, pattern)
    }
    fn defpattern(&mut self, m: &mut Mem, index: i16, size: i16, mask: Addr) {
        // a pattern of `size` rows of `size` bits: size × size / 16 words
        let n = (size.max(0) as u32 * size.max(0) as u32) / 16;
        let rows: Vec<u16> = (0..n).map(|i| m.u16(mask + 2 * i)).collect();
        self.0.defpattern(index, size, &rows)
    }
    fn defcursor(&mut self, m: &mut Mem, index: i16, glyph: Addr) {
        // 16 rows of 16 bits, or 32 rows of 32 for the large kind; the layer takes what
        // its current cursor type needs from the 64 words handed over
        let rows: Vec<u16> = (0..64).map(|i| m.try_bytes(glyph + 2 * i, 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))).collect();
        self.0.defcursor(index, &rows)
    }
    fn curorigin(&mut self, _: &mut Mem, index: i16, x: i16, y: i16) {
        self.0.curorigin(index, x, y)
    }
    fn curstype(&mut self, _: &mut Mem, kind: i32) {
        self.0.curstype(kind as u32)
    }
    fn setcursor(&mut self, _: &mut Mem, index: i16, _colour: i32, _mask: i32) {
        self.0.setcursor(index)
    }
    fn getgdesc(&mut self, _: &mut Mem, inquiry: i32) -> i32 {
        self.0.getgdesc(inquiry)
    }
    fn mapcolor(&mut self, _: &mut Mem, index: u16, red: i16, green: i16, blue: i16) {
        self.0.mapcolor(index, red, green, blue)
    }
    fn noport(&mut self, _: &mut Mem) {
        self.0.noport()
    }
    fn foreground(&mut self, _: &mut Mem) {
        self.0.foreground()
    }
    fn winopen(&mut self, _: &mut Mem, name: &[u8]) -> i32 {
        self.0.winopen(name)
    }
    fn winclose(&mut self, _: &mut Mem, id: i32) {
        self.0.winclose(id)
    }
    fn getmcolor(&mut self, m: &mut Mem, index: u16, red: Addr, green: Addr, blue: Addr) {
        // the layer answers nothing for an index outside the map; the words then stay
        if let Some((r, g, b)) = self.0.getmcolor(index) {
            m.set_u16(red, r);
            m.set_u16(green, g);
            m.set_u16(blue, b);
        }
    }
    fn prefposition(&mut self, _: &mut Mem, x1: i32, x2: i32, y1: i32, y2: i32) {
        self.0.prefposition(x1, x2, y1, y2)
    }
    fn RGBmode(&mut self, _: &mut Mem) {
        self.0.rgbmode()
    }
    fn gconfig(&mut self, _: &mut Mem) {
        self.0.gconfig()
    }
    fn winpop(&mut self, _: &mut Mem) {
        self.0.winpop()
    }
    fn getsize(&mut self, m: &mut Mem, width: Addr, height: Addr) {
        // without a current window the layer stores nothing
        if let Some((w, h)) = self.0.getsize() {
            m.set_i32(width, w);
            m.set_i32(height, h);
        }
    }
    fn qcontrol_out(&mut self, _: &mut Mem, cmd: i32, out: &mut [i16]) -> i32 {
        self.0.qcontrol(cmd, &[], out)
    }
}
