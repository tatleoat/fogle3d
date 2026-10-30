// SPDX-License-Identifier: MIT
//! `objg-meshes <count> <seed>`: made-up polygon meshes in the original's memory layout,
//! as text, for the vector generator of `obj_global.c` (`re/tools/si-model/gen_objg.py`).
//! Each mesh is a polygon soup filed by the port's rebuild on the image (checked against the
//! binary in the earlier passes), so its stars and edges are what the program makes; the
//! vertex and polygon flags are then set to made-up values. Lines (hex words):
//!
//! ```text
//! mesh <n>
//! H  w0 .. w20            the 0x54-byte record (pointers and lists zeroed)
//! V  i  w0 .. w4 | star words
//! P  i  w0 .. w5 | corner words
//! E  k  w0 .. w4          edge k, 0 to the count
//! end
//! ```
//!
//! Nothing here reproduces a routine; it is a case builder.

use si_mem::Mem;
use si_model::mesh::Mesh as V;
use si_model::mesh_image::{from_value, rebuild};
use si_model::model_image::NoOutside;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
    fn coord(&mut self) -> f32 {
        (self.below(2001) as f32 - 1000.0) / 250.0
    }
}

fn soup(r: &mut Lcg) -> V {
    let nv = 3 + r.below(8) as usize;
    let pos: Vec<[f32; 3]> = (0..nv).map(|_| [r.coord(), r.coord(), r.coord()]).collect();
    let mut m = V::with_vertices(&pos);
    m.flags = [0x804u16, 0x2, 0x4, 0x8, 0x0, 0x80c][r.below(6) as usize];
    m.cos_angle = [0.5f32, 0.0, 0.9][r.below(3) as usize];
    let style = r.below(3);
    for _ in 0..1 + r.below(8) {
        let n = 3 + r.below(if style == 0 { 3 } else { 5 }) as usize;
        let mut corners: Vec<i32> = Vec::new();
        let mut pool: Vec<i32> = (0..nv as i32).collect();
        for k in 0..n {
            if k > 2 && k + 2 < n && r.below(8) == 0 {
                corners.push(-1);
            }
            if style < 2 && !pool.is_empty() {
                let at = r.below(pool.len() as u32) as usize;
                corners.push(pool.remove(at));
            } else {
                corners.push(r.below(nv as u32) as i32);
            }
        }
        m.push_polygon(&corners);
    }
    m
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let count: u32 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let mut r = Lcg(a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1));
    for n in 0..count {
        let v = soup(&mut r);
        let mut m = Mem::program();
        si_model::model_image::init(&mut m);
        let lists = m.place(0x24, 4);
        m.set_fill_bytes(lists, 0, 0x24);
        m[si_model::model_image::g::SI_GRAF_LISTS] = lists;
        let mesh = from_value(&mut m, &v);
        rebuild::update_nodes(&mut m, &mut NoOutside, mesh, 1);
        // made-up flags: vertices bit 0 (a mark the tools test) and others, polygons selected, hidden
        let (nv, np) = (m.i32(mesh + 0x14), m.i32(mesh + 0x18));
        for i in 0..nv as u32 {
            let at = m.u32(mesh) + 0x14 * i;
            let f = [0u16, 1, 1, 3, 0x8000, 0x21, 0xffff][r.below(7) as usize];
            m.set_u16(at + 0x12, f);
        }
        for i in 0..np as u32 {
            let at = m.u32(mesh + 4) + 0x18 * i;
            let f = [0u16, 0, 1, 1, 1, 0x400, 0x800, 0x801, 0x5, 0xc01][r.below(10) as usize];
            m.set_u16(at + 0x14, f);
        }
        println!("mesh {n}");
        let mut h = Vec::new();
        for k in 0..21u32 {
            let off = 4 * k;
            let w = if matches!(off, 0 | 4 | 8 | 0x24 | 0x28 | 0x2c | 0x30 | 0x34) { 0 } else { m.u32(mesh + off) };
            h.push(format!("{w:08x}"));
        }
        println!("H {}", h.join(" "));
        for i in 0..nv as u32 {
            let at = m.u32(mesh) + 0x14 * i;
            let ws: Vec<String> = [0u32, 4, 8, 0x10].iter().map(|o| format!("{:08x}", if *o == 0x10 { m.u32(at + 0x10) } else { m.u32(at + o) })).collect();
            let star = m.u32(at + 0xc);
            let cnt = m.u16(at + 0x10) as u32;
            let sw: Vec<String> = (0..if star == 0 { 0 } else { 3 * cnt }).map(|k| format!("{:08x}", m.u32(star + 4 * k))).collect();
            println!("V {i} {} | {}", ws.join(" "), sw.join(" "));
        }
        for i in 0..np as u32 {
            let at = m.u32(mesh + 4) + 0x18 * i;
            let ws: Vec<String> = [0u32, 4, 8, 0x10, 0x14].iter().map(|o| format!("{:08x}", m.u32(at + o))).collect();
            let nodes = m.u32(at + 0xc);
            let cnt = m.u16(at + 0x10) as u32;
            let nw: Vec<String> = (0..if nodes == 0 { 0 } else { 9 * cnt }).map(|k| format!("{:08x}", m.u32(nodes + 4 * k))).collect();
            println!("P {i} {} | {}", ws.join(" "), nw.join(" "));
        }
        let ne = m.i32(mesh + 0x1c);
        for k in 0..=ne {
            let e = si_hrc::image::geom::edge_at(&m, m.u32(mesh + 8), k);
            let ws: Vec<String> = (0..5).map(|j| format!("{:08x}", m.u32(e + 4 * j))).collect();
            println!("E {k} {}", ws.join(" "));
        }
        println!("end");
    }
}
