// SPDX-License-Identifier: MIT
//! The knot insertions on the memory image (finding 0025, section 19): a bridge from the image's records to the owned
//! forms of [`crate::insert`] and back, with the heap bookkeeping of the original (the scratch blocks of the span's
//! triangle, the new point and knot arrays, the old ones freed).
//!
//! The arithmetic is the owned form's (compared with the emulated original, finding 0025 section 11); this module only
//! reads the record, calls it and lays the result out the way the original leaves the heap and the record.

use crate::image::{free_clear, free_some, srf_dispose_lines, LEFT, SI_GBL_INDEX};
use crate::insert;
use crate::own::{Curve, Host, Surface};
use crate::{NurbsCrv, NurbsPoint, NurbsSrf, Statics};
use si_hrc::image::nurbs::{knots_allocate, nurbs_points_allocate};
use si_mem::{Mem, Outside};
use si_types::Ptr32;

/// The most records or knots the port reads for one array (a count read from a record is checked against the
/// mapped bytes first).
const MAX: i32 = 1 << 24;

fn read_knots(m: &Mem, at: u32, n: i32) -> Option<Vec<f64>> {
    if at == 0 {
        return None;
    }
    if n <= 0 || n > MAX || !m.is_mapped(at, (n as u32).wrapping_mul(8)) {
        return Some(Vec::new());
    }
    Some((0..n as u32).map(|i| m.f64(at.wrapping_add(8 * i))).collect())
}

fn read_points(m: &Mem, at: u32, n: i32) -> Option<Vec<NurbsPoint>> {
    if at == 0 {
        return None;
    }
    if n <= 0 || n > MAX || !m.is_mapped(at, (n as u32).wrapping_mul(0x24)) {
        return Some(Vec::new());
    }
    Some((0..n as u32).map(|i| m.read::<NurbsPoint>(Ptr32::new(at.wrapping_add(0x24 * i)))).collect())
}

/// A curve of the image as its owned form (no length tables).
pub fn read_curve(m: &Mem, c: u32) -> Curve {
    let rec: NurbsCrv = m.read(Ptr32::new(c));
    let knots = read_knots(m, rec.dir.knots.addr(), rec.dir.nb_knots);
    let points = read_points(m, m.u32(c.wrapping_add(0x2c)), rec.dir.nb_points);
    Curve::from_parts(rec, knots, points)
}

/// A surface of the image as its owned form (no trims, projected curves or display lines: the insertions do not look
/// at them).
pub fn read_surface(m: &Mem, s: u32) -> Surface {
    let rec: NurbsSrf = m.read(Ptr32::new(s));
    let ku = read_knots(m, rec.u.knots.addr(), rec.u.nb_knots);
    let kv = read_knots(m, rec.v.knots.addr(), rec.v.nb_knots);
    let pts = read_points(m, m.u32(s.wrapping_add(0x54)), rec.nb_points);
    Surface::from_parts(rec, ku, kv, pts)
}

/// `SI_gblIndex` as the owned routines see it: a window of the table the image's pointer names, and the entries to
/// write back.
struct Gbl {
    table: Option<Vec<i32>>,
    base: Vec<i32>,
    lines_disposed: bool,
}

impl Host for Gbl {
    fn gbl_index(&mut self) -> Option<&mut Vec<i32>> {
        self.table.as_mut()
    }
    fn srf_dispose_lines(&mut self, _srf: &mut Surface) {
        self.lines_disposed = true;
    }
}

fn gbl_window(m: &Mem, entries: i32) -> Gbl {
    let g = m.u32(SI_GBL_INDEX);
    if g == 0 {
        return Gbl { table: None, base: Vec::new(), lines_disposed: false };
    }
    // A table the original indexes by the new point numbers: the window covers every index the owned form can write.
    let mut n = entries.clamp(0, MAX) as u32;
    while n > 0 && !m.is_mapped(g, n * 4) {
        n -= 1;
    }
    let v: Vec<i32> = (0..n).map(|i| m.i32(g.wrapping_add(4 * i))).collect();
    Gbl { table: Some(v.clone()), base: v, lines_disposed: false }
}

fn gbl_flush(m: &mut Mem, h: &Gbl) {
    let g = m.u32(SI_GBL_INDEX);
    if let Some(t) = &h.table {
        for (i, (new, old)) in t.iter().zip(&h.base).enumerate() {
            if new != old {
                m.set_i32(g.wrapping_add(4 * i as u32), *new);
            }
        }
    }
}

fn write_points(m: &mut Mem, at: u32, pts: &[NurbsPoint]) {
    for (i, p) in pts.iter().enumerate() {
        m.write(Ptr32::new(at.wrapping_add(0x24 * i as u32)), *p);
    }
}

/// The three scratch blocks of an insertion: two `SI_calloc(order - 1, 8)` (the knot distances) and
/// `SI_nurbsPointsAllocate(per * order * (order + 1) / 2)` (the triangle or triangles). Their contents are the owned
/// form's business; what the image keeps of them is the heap's bookkeeping (sizes and the order they are freed in).
fn scratch(m: &mut Mem, order: i32, per: i32) -> [u32; 3] {
    let dl = m.si_calloc(order.wrapping_sub(1) as u32, 8);
    let dr = m.si_calloc(order.wrapping_sub(1) as u32, 8);
    let tri = nurbs_points_allocate(m, per.wrapping_mul(order.wrapping_add(1).wrapping_mul(order) >> 1));
    [dl, dr, tri]
}

fn scratch_free(m: &mut Mem, s: [u32; 3]) {
    // the triangle first, then the distances
    free_some(m, s[2]);
    free_some(m, s[0]);
    free_some(m, s[1]);
}

/// [soft 0x084d4130 SI_nurbsCrvKnotInsert], on the image: `(crv, t, r, correct)`. The owned form
/// ([`insert::crv_knot_insert`]) does the work on a copy of the record; the image then gets what the original leaves:
/// for order above 4 the three scratch blocks first (distances, triangle); `SI_nurbsPointsAllocate` of the new points
/// (as many as the highest index written plus one), the old points freed and replaced, the parameterisation word (+0x24)
/// 2, the point count (+4) raised by `r`; `SI_calloc` of the new knots, the old ones freed and replaced, the range and
/// knot count; the scratch freed (triangle, then the two distance arrays). Where `SI_gblIndex` is set the new index of
/// each old point is recorded. Answers 1, or 0 when nothing is inserted. The port answers 0 and leaves the image alone
/// where the original would not end or would read outside its arrays (the owned form refuses).
pub fn crv_knot_insert(m: &mut Mem, c: u32, t: f64, r: i32, correct: i16) -> i32 {
    let mut cu = read_curve(m, c);
    let (order, n) = (cu.rec.dir.order, cu.rec.dir.nb_points);
    let mut host = gbl_window(m, n.wrapping_add(r.max(0)).wrapping_add(8));
    let mut st = Statics { left: m.i32(LEFT) };
    let mut work = None;
    let res = insert::crv_knot_insert_scratch(&mut st, &mut host, &mut cu, t, r, correct, &mut work);
    m.set_i32(LEFT, st.left);
    match res {
        None => 0,
        Some(0) => 0,
        Some(v) => {
            let sc = if order > 4 { scratch(m, order, 1) } else { [0; 3] };
            if let (true, Some((dl, dr, tri, _))) = (order > 4, work.as_ref()) {
                for (i, v) in dl.iter().enumerate() {
                    m.set_f64(sc[0].wrapping_add(8 * i as u32), *v);
                }
                for (i, v) in dr.iter().enumerate() {
                    m.set_f64(sc[1].wrapping_add(8 * i as u32), *v);
                }
                write_points(m, sc[2], tri);
            }
            let new = cu.points.as_deref().unwrap_or(&[]);
            let p = nurbs_points_allocate(m, new.len() as i32);
            write_points(m, p, new);
            gbl_flush(m, &host);
            free_clear(m, c.wrapping_add(0x2c));
            m.set_u32(c.wrapping_add(0x2c), p);
            m.set_i32(c.wrapping_add(0x24), 2);
            m.set_i32(c.wrapping_add(4), cu.rec.dir.nb_points);
            let kn = cu.knots.as_deref().unwrap_or(&[]);
            let k = knots_allocate(m, cu.rec.dir.nb_knots);
            for (i, v) in kn.iter().enumerate() {
                m.set_f64(k.wrapping_add(8 * i as u32), *v);
            }
            free_clear(m, c.wrapping_add(0xc));
            m.set_u32(c.wrapping_add(0xc), k);
            m.set_f64(c.wrapping_add(0x10), cu.rec.dir.range_start.0);
            m.set_f64(c.wrapping_add(0x18), cu.rec.dir.range_end.0);
            m.set_i32(c.wrapping_add(8), cu.rec.dir.nb_knots);
            if order > 4 {
                scratch_free(m, sc);
            }
            v
        }
    }
}

/// [soft 0x084d96f4 SI_nurbsSrfKnotInsertU] and [soft 0x084da6fc SI_nurbsSrfKnotInsertV] on the image: the owned forms
/// ([`insert::srf_knot_insert_u`], `_v`) on a copy, then what the original leaves: the three scratch blocks (the
/// triangles of the cross direction's rows or columns in one block), `SI_nurbsPointsAllocate` of the new grid, the old
/// points freed and replaced, the total (+0x48) and parameterisation word (+0x4c, 2) set, the direction's point count
/// raised by `r`; `SI_calloc` of the new knots, the old freed and replaced; range and knot count; the scratch freed
/// (triangles first); `SI_nurbsSrfDisposeLines`. The 16-bit answer is 1, or 0 when nothing is inserted.
pub fn srf_knot_insert_dir(m: &mut Mem, o: &mut dyn Outside, s: u32, t: f64, r: i32, across_v: bool) -> i16 {
    let mut su = read_surface(m, s);
    let (dir_off, cross_off) = if across_v { (0x24u32, 4u32) } else { (0u32, 0x28u32) };
    let order = m.i32(s.wrapping_add(dir_off));
    let cross = m.i32(s.wrapping_add(cross_off));
    let total = su.rec.nb_points;
    let mut host = gbl_window(m, total.wrapping_add(r.max(0).wrapping_mul(cross.max(0))).wrapping_add(8));
    let mut st = Statics { left: m.i32(LEFT) };
    let mut work = None;
    let res = insert::srf_knot_insert_scratch(&mut st, &mut host, &mut su, t, r, across_v, &mut work);
    m.set_i32(LEFT, st.left);
    match res {
        None | Some(0) => 0,
        Some(v) => {
            if across_v {
                // the V routine disposes of the display lines before it takes its work arrays, the U routine at the end
                srf_dispose_lines(m, o, s);
            }
            let sc = scratch(m, order, cross);
            if let Some((dl, dr, tri, _)) = work.as_ref() {
                for (i, v) in dl.iter().enumerate() {
                    m.set_f64(sc[0].wrapping_add(8 * i as u32), *v);
                }
                for (i, v) in dr.iter().enumerate() {
                    m.set_f64(sc[1].wrapping_add(8 * i as u32), *v);
                }
                let own = (cross.max(0) as usize) * ((order.wrapping_add(1).wrapping_mul(order) >> 1).max(0) as usize);
                write_points(m, sc[2], &tri[..own.min(tri.len())]);
            }
            let new = su.points.as_deref().unwrap_or(&[]);
            let p = nurbs_points_allocate(m, su.rec.nb_points);
            // Copies from beyond the block of triangles (a negative count) take the heap's own words there, so the
            // plan is replayed step by step against the image.
            match work.as_ref().map(|w| w.3.as_slice()).filter(|e| !e.is_empty()) {
                None => write_points(m, p, new),
                Some(events) => {
                    for (at, ev) in events {
                        let to = p.wrapping_add(0x24 * *at as u32);
                        match ev {
                            Ok(pt) => m.write(Ptr32::new(to), *pt),
                            Err(idx) => {
                                for w in 0..9u32 {
                                    let v = m.u32(sc[2].wrapping_add(0x24u32.wrapping_mul(*idx as u32)).wrapping_add(4 * w));
                                    m.set_u32(to.wrapping_add(4 * w), v);
                                }
                            }
                        }
                    }
                }
            }
            gbl_flush(m, &host);
            free_clear(m, s.wrapping_add(0x54));
            m.set_u32(s.wrapping_add(0x54), p);
            m.set_i32(s.wrapping_add(0x48), su.rec.nb_points);
            m.set_i32(s.wrapping_add(0x4c), 2);
            let d = if across_v { &su.rec.v } else { &su.rec.u };
            m.set_i32(s.wrapping_add(dir_off).wrapping_add(4), d.nb_points);
            let kn = (if across_v { &su.knots_v } else { &su.knots_u }).as_deref().unwrap_or(&[]);
            let k = knots_allocate(m, d.nb_knots);
            for (i, x) in kn.iter().enumerate() {
                m.set_f64(k.wrapping_add(8 * i as u32), *x);
            }
            free_clear(m, s.wrapping_add(dir_off).wrapping_add(0xc));
            m.set_u32(s.wrapping_add(dir_off).wrapping_add(0xc), k);
            m.set_i32(s.wrapping_add(dir_off).wrapping_add(8), d.nb_knots);
            m.set_f64(s.wrapping_add(dir_off).wrapping_add(0x10), d.range_start.0);
            m.set_f64(s.wrapping_add(dir_off).wrapping_add(0x18), d.range_end.0);
            scratch_free(m, sc);
            if !across_v {
                srf_dispose_lines(m, o, s);
            }
            v
        }
    }
}

/// [soft 0x084d96bc SI_nurbsSrfKnotInsert] on the image: `(srf, t, r, dir)`; `dir` 0 inserts into U, anything else into
/// V; the 16-bit answer widened with its sign.
pub fn srf_knot_insert(m: &mut Mem, o: &mut dyn Outside, s: u32, t: f64, r: i32, dir: i32) -> i32 {
    srf_knot_insert_dir(m, o, s, t, r, dir != 0) as i32
}

/// [soft 0x084dc5f4 SI_nurbsSrfSwapUV], on the image (the first form is [`crate::own::srf_swap_uv`]): exchanges the
/// two directions of a surface. Answers 0 and changes nothing when the point total (+0x48) is 0 or the new array cannot
/// be had. Otherwise: `SI_nurbsPointsAllocate(total)` and the points transposed into it (old (i, j) to index
/// `i * nv + j`; where `SI_gblIndex` is set, entry `k` in the old order gets the new index); the two direction blocks
/// (0x24 bytes each), the steps, curve counts, the capping words (+0x88 with +0x8c, +0x8a with +0x8e) and the two words
/// at +0x90 exchanged; without trims and with display lines, a new line array (`SI_calloc(n, 0x14)`) with the lines
/// that ran along V first (counts of the exchanged record), the old array freed; for every trim the x and y of every
/// point of its curves exchanged and, with a trim line, its doubles at +0x10 with +0x28 and +0x20 with +0x18, the two
/// doubles of each node, and `SI_trmCvrRve` on the line; the old points freed and the new ones installed. Answers 1.
pub fn srf_swap_uv(m: &mut Mem, s: u32) -> i32 {
    use crate::trimutils::si_trm_cvr_rve;
    if m.i32(s.wrapping_add(0x48)) == 0 {
        return 0;
    }
    let newp = nurbs_points_allocate(m, m.i32(s.wrapping_add(0x48)));
    if newp == 0 {
        return 0;
    }
    let (old, g) = (m.u32(s.wrapping_add(0x54)), m.u32(SI_GBL_INDEX));
    let mut next: u32 = 0;
    let mut j: i32 = 0;
    while j < m.i32(s.wrapping_add(0x28)) {
        let mut i: i32 = 0;
        while i < m.i32(s.wrapping_add(4)) {
            let to = i.wrapping_mul(m.i32(s.wrapping_add(0x28))).wrapping_add(j);
            for w in 0..9u32 {
                let v = m.u32(old.wrapping_add(0x24 * next).wrapping_add(4 * w));
                m.set_u32(newp.wrapping_add(0x24u32.wrapping_mul(to as u32)).wrapping_add(4 * w), v);
            }
            if g != 0 {
                m.set_i32(g.wrapping_add(4 * next), to);
            }
            next += 1;
            i += 1;
        }
        j += 1;
    }
    let swap32 = |m: &mut Mem, a: u32, b: u32| {
        let (x, y) = (m.u32(a), m.u32(b));
        m.set_u32(a, y);
        m.set_u32(b, x);
    };
    for w in 0..9u32 {
        swap32(m, s.wrapping_add(4 * w), s.wrapping_add(0x24 + 4 * w));
    }
    swap32(m, s.wrapping_add(0x70), s.wrapping_add(0x74));
    swap32(m, s.wrapping_add(0x78), s.wrapping_add(0x7c));
    let (a, b, c, d) = (m.u16(s.wrapping_add(0x88)), m.u16(s.wrapping_add(0x8c)), m.u16(s.wrapping_add(0x8a)), m.u16(s.wrapping_add(0x8e)));
    m.set_u16(s.wrapping_add(0x88), b);
    m.set_u16(s.wrapping_add(0x8c), a);
    m.set_u16(s.wrapping_add(0x8a), d);
    m.set_u16(s.wrapping_add(0x8e), c);
    swap32(m, s.wrapping_add(0x90), s.wrapping_add(0x94));
    if m.i32(s.wrapping_add(0x60)) == 0 && m.i32(s.wrapping_add(0x80)) > 0 {
        let n = m.i32(s.wrapping_add(0x80));
        let lines = m.si_calloc(n as u32, 0x14);
        let rec: NurbsSrf = m.read(Ptr32::new(s));
        let mut k: i32 = 0;
        while k < m.i32(s.wrapping_add(0x80)) {
            let nvl = crate::own::srf_no_vlines(&rec);
            let from = if k < nvl { crate::own::srf_no_ulines(&rec).wrapping_add(k) } else { k.wrapping_sub(nvl) };
            for w in 0..5u32 {
                let v = m.u32(m.u32(s.wrapping_add(0x84)).wrapping_add(0x14u32.wrapping_mul(from as u32)).wrapping_add(4 * w));
                m.set_u32(lines.wrapping_add(0x14u32.wrapping_mul(k as u32)).wrapping_add(4 * w), v);
            }
            k += 1;
        }
        free_clear(m, s.wrapping_add(0x84));
        m.set_u32(s.wrapping_add(0x84), lines);
    }
    let mut t: i32 = 0;
    while t < m.i32(s.wrapping_add(0x60)) {
        let comp = m.u32(m.u32(s.wrapping_add(0x64)).wrapping_add(4 * t as u32));
        let mut ci: i32 = 0;
        while ci < m.i32(comp.wrapping_add(4)) {
            let cv = m.u32(m.u32(comp.wrapping_add(8)).wrapping_add(4 * ci as u32));
            let n = m.i32(cv.wrapping_add(4));
            let mut pi: i32 = 0;
            while pi < n {
                let p = m.u32(cv.wrapping_add(0x2c)).wrapping_add(0x24u32.wrapping_mul(pi as u32));
                let (x, y) = (m.u64(p), m.u64(p.wrapping_add(8)));
                m.set_u64(p, y);
                m.set_u64(p.wrapping_add(8), x);
                pi += 1;
            }
            ci += 1;
        }
        let line = m.u32(comp.wrapping_add(0xc));
        if line != 0 {
            for (a, b) in [(0x10u32, 0x28u32), (0x20, 0x18)] {
                let (x, y) = (m.u64(line.wrapping_add(a)), m.u64(line.wrapping_add(b)));
                m.set_u64(line.wrapping_add(a), y);
                m.set_u64(line.wrapping_add(b), x);
            }
            let mut node = m.u32(line.wrapping_add(8));
            while node != 0 {
                let (x, y) = (m.u64(node), m.u64(node.wrapping_add(8)));
                m.set_u64(node, y);
                m.set_u64(node.wrapping_add(8), x);
                node = m.u32(node.wrapping_add(0x14));
            }
            si_trm_cvr_rve(m, line);
        }
        t += 1;
    }
    free_clear(m, s.wrapping_add(0x54));
    m.set_u32(s.wrapping_add(0x54), newp);
    1
}

/// [soft 0x084e6960 si_nurbsClampFactor], on the image: `(f, dir)` reads the double at `f` and the parameter range of the
/// direction record (`+0x10` start, `+0x18` end). Above the end it is replaced by the end; BELOW THE START IT IS ALSO
/// REPLACED BY THE END (the lower branch stores the same register as the upper one; kept as read, [read] from the stack
/// order). Inside the range, and for an unordered compare (NaN), nothing is written.
pub fn si_nurbs_clamp_factor(m: &mut Mem, f: u32, dir: u32) {
    let (v, lo, hi) = (m.f64(f), m.f64(dir.wrapping_add(0x10)), m.f64(dir.wrapping_add(0x18)));
    if hi < v || v < lo {
        m.set_f64(f, hi);
    }
}
