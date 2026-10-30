// SPDX-License-Identifier: MIT
//! Several threads inside one frame (unit threads1; the dependence map is
//! `re/out/lead/blender/threads.md`). Nothing here reproduces a function of the original: the
//! frame loops' lines ([`Sampler::sample_line`], [`Sampler::supersample_line`]) are sampled in
//! bands of lines at once, each band on a private copy of the state the lines read, and the
//! lines then go to the output ([`Sampler::sample_line_out`], [`Sampler::supersample_line_out`])
//! in order, on the frame's own state, as one thread hands them over.
//!
//! What a line's samples read besides the scene, and what this module does about each:
//! - the active slice list and the slices' edges (`start_line`): a band replays the lines above
//!   it with [`Sampler::skip_line`], the same bookkeeping without the samples;
//! - the materials' working blocks (`MatWork`) and 3D texture word, which `shadow_map` and the
//!   vertex colours leave changed behind a sample: compared, bit for bit, between a band's
//!   start and the end of the band above it ([`Carry`]); when they differ the band is sampled
//!   again, on the main thread, from the end state of the band above;
//! - the adaptive sampler's cache row of the line above (`rows[sub]`): a band first samples the
//!   line above it (warm-up) and the row is compared like the working blocks;
//! - not compared, because no sample reads what an earlier one left there: the triangles'
//!   "met" bit (cleared by `s_trace` before it is read), the fault (read only by the line
//!   writer: the first one in line order is handed to the output), the ray counter (the
//!   cells' mailboxes are checked to be 0, which a counter of 1 or more never equals; a thread
//!   that goes on with its scene state into another band, see [`render`], finds there only
//!   numbers of its own earlier rays, all below its counter, which only grows), the
//!   chunk memory's books (allocations are only counted, and dropped with the frame), the
//!   layer rays' fields (each is written before it is read for a primary ray).
//!   `RENDER_BANDS_STRICT=1` compares all of the scene state as well (slow; for checks).
//!
//! Switches (read once): `RENDER_THREADS=N` (1, the default, leaves the frame loops alone);
//! `RENDER_BANDS_STRICT=1`; `RENDER_BANDS_STEAL=0`: no band takes over part of another;
//! `RENDER_BANDS_PROBE=<file>`: the first frame is sampled once in order and alongside on test
//! copies, and the dependence counts are added to the file.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use crate::frame::{Tracer, World};
use crate::mem_chunk::ChunkBuffer;
use crate::render::{LineLoop, SampleEnv, Sampler};
use crate::shade::{shade, MatWork, ShadeState};
use crate::trace::TN_MET;
use crate::types::{RVertex, Ray, Rgbazo, Tn};

/// Threads for the sampling of one frame: `RENDER_THREADS` (at least 1; 1 when unset).
pub fn threads() -> usize {
    static T: OnceLock<usize> = OnceLock::new();
    *T.get_or_init(|| std::env::var("RENDER_THREADS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(1usize).max(1))
}

fn strict() -> bool {
    static S: OnceLock<bool> = OnceLock::new();
    *S.get_or_init(|| std::env::var("RENDER_BANDS_STRICT").is_ok_and(|v| v != "0" && !v.is_empty()))
}

fn probe_path() -> Option<&'static str> {
    static P: OnceLock<Option<String>> = OnceLock::new();
    P.get_or_init(|| std::env::var("RENDER_BANDS_PROBE").ok().filter(|v| !v.is_empty())).as_deref()
}

static PROBED: AtomicBool = AtomicBool::new(false);

/// Whether this frame is the one the probe looks at (the first one, when the switch is set).
pub(crate) fn probe_wanted() -> bool {
    probe_path().is_some() && !PROBED.swap(true, Ordering::SeqCst)
}

/// Whether the frame can be cut into bands: no tag image (its pixel bits are set during the
/// sampling), no fault already, every cell's mailbox 0 (see the module text), at least two
/// lines.
pub(crate) fn splittable(s: &Sampler, w: &World) -> bool {
    s.tags_on == 0 && w.st.fault.is_none() && w.tracer.bsp.nodes.iter().all(|n| n.mailbox == 0) && s.region[3] > s.region[2]
}

/// A band's surroundings: its own copy of the scene state and of the tracer.
pub struct BandWorld {
    pub st: ShadeState,
    pub tracer: Tracer,
}

impl SampleEnv for BandWorld {
    fn geometry(&self) -> (&[Tn], &[RVertex]) {
        (&self.st.tns, &self.st.verts)
    }
    fn shade(&mut self, ray: &mut Ray, under: Rgbazo, primary: i16) -> Rgbazo {
        shade(&mut self.st, &mut self.tracer, ray, under, primary)
    }
    fn chunk(&mut self) -> &mut ChunkBuffer {
        &mut self.tracer.chunk
    }
    fn chunk_malloc(&mut self, size: i32) -> u32 {
        self.tracer.chunk.malloc(&mut self.tracer.heap, size)
    }
}

/// A band's tracer: the tree shared with the frame's ([`crate::pool::Pool`]), the "met" marks
/// of the shadow walk in a scratch of its own, as the triangles are shared too
/// ([`crate::trace::MetMarks`]).
fn clone_tracer(t: &Tracer) -> Tracer {
    Tracer { bsp: t.bsp.clone(), mailbox: t.mailbox, chunk: t.chunk.clone(), heap: t.heap.clone(), arrays: t.arrays.clone(), shaft: Default::default(), met: crate::trace::MetMarks::scratch() }
}

fn work_bits(w: &MatWork) -> [u32; 20] {
    let mut b = [0u32; 20];
    let f = w.base_ambient.iter().chain(&w.ambient).chain(&w.diffuse).chain(&w.specular).chain([&w.reflectivity, &w.opacity, &w.refraction, &w.existence]);
    for (o, v) in b.iter_mut().zip(f) {
        *o = v.to_bits();
    }
    b
}

const WORK_FIELDS: [&str; 8] = ["base_ambient", "ambient", "diffuse", "specular", "reflectivity", "opacity", "refraction", "existence"];

fn field_of(slot: usize) -> usize {
    if slot < 16 {
        slot / 4
    } else {
        slot - 12
    }
}

fn rgbazo_bits(c: &Rgbazo) -> [u32; 6] {
    [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits(), c.z.to_bits(), c.o.to_bits()]
}

/// A cache entry as the adaptive sampler reads it: an empty one (red −1) only by its red.
fn seam_bits(c: &Rgbazo) -> [u32; 6] {
    if c.r.to_bits() == (-1.0f32).to_bits() {
        [c.r.to_bits(), 0, 0, 0, 0, 0]
    } else {
        rgbazo_bits(c)
    }
}

/// What a line reads that the lines before it may have left changed (see the module text).
#[derive(Clone, Debug, Default, PartialEq)]
struct Carry {
    work: Vec<([u32; 20], u32)>,
    seam: Vec<[u32; 6]>,
    strict: Option<String>,
}

fn carry(st: &ShadeState, s: &Sampler, lp: &LineLoop, adaptive: bool, strict: bool) -> Carry {
    let work = st.materials.iter().map(|m| (work_bits(&m.work), m.tex3d)).collect();
    let seam = if adaptive { s.ss.rows.get(s.ss.sub as usize).map_or(Vec::new(), |r| r.iter().map(seam_bits).collect()) } else { Vec::new() };
    let strict = strict.then(|| strict_text(st, s, lp));
    Carry { work, seam, strict }
}

/// A hash of a value's `Debug` text, made without keeping the text.
struct HashWriter(std::collections::hash_map::DefaultHasher);

impl std::fmt::Write for HashWriter {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        std::hash::Hasher::write(&mut self.0, s.as_bytes());
        Ok(())
    }
}

fn hash_of(f: impl FnOnce(&mut HashWriter)) -> u64 {
    let mut h = HashWriter(Default::default());
    f(&mut h);
    std::hash::Hasher::finish(&h.0)
}

/// Every part of the scene state but the fault, as hashes of their `Debug` text: the
/// triangles without the "met" bit, the materials without the working block and 3D texture
/// word (which [`Carry`] holds bit for bit).
fn part_hashes(st: &ShadeState) -> Vec<(&'static str, u64)> {
    use std::fmt::Write as _;
    let ShadeState { glb, camera, tns, verts, uvs, materials, maps, models, lights, light_head, pictures, picture_head, tex3ds, noise, default_material, texture_pixels, fault: _ } = st;
    let mut out = vec![
        ("tns", hash_of(|h| {
            for t in tns {
                let mut c = t.clone();
                c.flags &= !TN_MET;
                let _ = write!(h, "{c:?}");
            }
        })),
        ("materials", hash_of(|h| {
            for m in materials {
                let mut c = m.clone();
                c.work = MatWork::default();
                c.tex3d = 0;
                let _ = write!(h, "{c:?}");
            }
        })),
    ];
    macro_rules! part {
        ($($f:ident),*) => {$(
            out.push((stringify!($f), hash_of(|h| { let _ = write!(h, "{:?}", $f); })));
        )*};
    }
    part!(glb, camera, verts, uvs, maps, models, lights, light_head, pictures, picture_head, tex3ds, noise, default_material, texture_pixels);
    out
}

/// `RENDER_BANDS_STRICT`: the scene state (see [`part_hashes`]), the active list and the
/// slices' edges and lists, as hashes.
fn strict_text(st: &ShadeState, s: &Sampler, lp: &LineLoop) -> String {
    use std::fmt::Write as _;
    let slices = hash_of(|h| {
        for l in &s.slices {
            let _ = write!(h, "{:?}", (l.x0.to_bits(), l.dx0.to_bits(), l.x1.to_bits(), l.dx1.to_bits(), l.y_end, l.tn, l.next));
        }
    });
    format!("{:?} {:?} {slices:x}", part_hashes(st), lp.active)
}

/// The cost of each line, top first, for cutting the bands: per sample column 1, plus 16 per
/// slice covering it (from the slices' bookkeeping alone).
fn line_costs(s: &Sampler, lp: &LineLoop, top: i16, n: usize) -> Vec<u64> {
    let mut s = s.clone();
    let mut lp = lp.clone();
    let mut costs = Vec::with_capacity(n);
    for i in 0..n {
        let y = top - i as i16;
        s.skip_line(&mut lp, y);
        let mut pixel: Option<usize> = None;
        let mut c = 0u64;
        let mut xs = s.region[0] as i16;
        while xs as i32 <= s.region[1] {
            s.start_column(&mut pixel, xs);
            let mut at = pixel;
            let mut len = 0u64;
            while let Some(k) = at {
                len += 1;
                at = s.slices[k].link.get();
                if len > 1_000_000 {
                    break;
                }
            }
            c += 1 + 16 * len;
            xs = xs.wrapping_add(1);
        }
        costs.push(c);
    }
    costs
}

/// Cuts `n` lines into `k` bands of about the same cost: the first line of each band and, last,
/// `n`.
fn cut(costs: &[u64], k: usize) -> Vec<usize> {
    let n = costs.len();
    let mut prefix = vec![0u64; n + 1];
    for i in 0..n {
        prefix[i + 1] = prefix[i] + costs[i];
    }
    let mut bounds = vec![0usize];
    for b in 1..k {
        let goal = prefix[n] as u128 * b as u128 / k as u128;
        let i = prefix.partition_point(|&p| (p as u128) < goal);
        let lo = bounds[bounds.len() - 1] + 1;
        let hi = n - (k - b);
        bounds.push(i.clamp(lo, hi));
    }
    bounds.push(n);
    bounds
}

/// A band's own copy of what the lines read.
struct State {
    s: Sampler,
    lp: LineLoop,
    w: BandWorld,
}

/// What a band hands back.
struct BandOut {
    /// The samples of each line (sample columns x min to x max), top line first.
    lines: Vec<Vec<Rgbazo>>,
    /// The first fault each line's shading met.
    faults: Vec<Option<u32>>,
    /// What the first [`TRACKED`] lines left (to find where a band sampled again meets it).
    after: Vec<Carry>,
    start: Carry,
    end: Carry,
    state: Option<State>,
    secs: f64,
}

/// Lines of a band whose end [`Carry`] is kept for the convergence test.
const TRACKED: usize = 16;

/// Samples line `y`: its samples and the first fault its shading met. The fault is cleared
/// first; nothing in the sampling reads it.
fn sample_one(st: &mut State, adaptive: bool, y: i16, cols: (usize, usize)) -> (Vec<Rgbazo>, Option<u32>) {
    st.w.st.fault = None;
    if adaptive {
        st.s.supersample_line(&mut st.w, &mut st.lp, y);
    } else {
        st.s.sample_line(&mut st.w, &mut st.lp, y);
    }
    (st.s.line[cols.0..=cols.1].to_vec(), st.w.st.fault)
}

/// One band while the threads run (speed4). A band's lines are `first..last`; its thread takes
/// them one at a time (`next`), and a thread that has finished its own band may take the second
/// half of what another band has not yet begun (`last` then shrinks and a new band starts at the
/// cut). `start` and `end` are the band's [`Carry`] as soon as known, so that a band's state is
/// kept for the convergence test only when the band below it does not start as it ended.
struct Slot {
    first: usize,
    last: usize,
    next: usize,
    done: bool,
    start: Option<Carry>,
    end: Option<Carry>,
}

/// What the threads share: the bands (in order of creation), the states kept for the
/// convergence test (by band), and counts for the log line.
struct Board {
    slots: Vec<Slot>,
    kept: Vec<Option<State>>,
    steals: usize,
    clones: usize,
}

/// A band takes over the second half of another only when that has at least this many lines
/// not yet begun (twice as many when adaptive: each new band samples one line more first, its
/// warm-up).
const STEAL_MIN: usize = 4;

/// Samples the frame's lines in bands on `n` threads and writes them in order: the body of
/// [`Sampler::sample_frame`] or [`Sampler::supersample_frame`] with the lines' samples taken
/// first. The picture, the depth file and the fault come out as those of the loop on one
/// thread (checked by `re/tools/threads1/tcompare.sh`).
///
/// The bands start as `n` cuts of about the same estimated cost; a thread that finishes early
/// takes the second half of the not yet begun lines of the band with the most estimated cost
/// left (speed4: the estimate is poor, and one band could take three times the mean). A new band
/// is made as before: a copy of the frame's sampler positioned by [`Sampler::skip_line`] and a
/// warm-up line, compared with the band above by its [`Carry`]. The thread goes on with the
/// scene state and tracer of its finished band when that state is not kept for the convergence
/// test (what differs there from a copy of the frame's is what the module text lists as not
/// compared, or the [`Carry`]); otherwise it starts from a copy of the frame's. (Going on with the
/// sampler as well would leave the adaptive sampler's cache rows of the band's own lines for the
/// warm-up line to read: the seam then differs and the band is sampled again.)
/// `RENDER_BANDS_STEAL=0` keeps the first cut.
pub(crate) fn render(s: &mut Sampler, world: &mut World, adaptive: bool, n: usize) {
    let t0 = Instant::now();
    let copies0 = crate::pool::copies();
    let strict = strict();
    let lp = if adaptive { s.supersample_begin(world) } else { s.sample_begin(world) };
    let top = s.region[3] as i16;
    let count = (s.region[3] - s.region[2] + 1).max(0) as usize;
    let cols = (s.region[0].max(0) as usize, s.region[1].max(0) as usize);
    let costs = line_costs(s, &lp, top, count);
    let mut prefix = vec![0u64; count + 1];
    for i in 0..count {
        prefix[i + 1] = prefix[i] + costs[i];
    }
    let k = n.min(count).max(1);
    let bounds = cut(&costs, k);
    let steal = steal_on();
    let steal_min = if adaptive { 2 * STEAL_MIN } else { STEAL_MIN };
    let t1 = Instant::now();
    let board = std::sync::Mutex::new(Board {
        slots: (0..k).map(|b| Slot { first: bounds[b], last: bounds[b + 1], next: bounds[b], done: false, start: None, end: None }).collect(),
        kept: (0..k).map(|_| None).collect(),
        steals: 0,
        clones: 0,
    });
    // the band (index) whose last line is just above `first`, if it is done
    let above = |g: &Board, first: usize| g.slots.iter().position(|x| x.done && x.last == first && x.first < first);
    let below = |g: &Board, last: usize| g.slots.iter().position(|x| x.first == last && x.last > last);
    let mut outs: Vec<(usize, BandOut)> = {
        let (s0, lp0, st0, tr0) = (&*s, &lp, &*world.st, &world.tracer);
        let (board, prefix) = (&board, &prefix);
        std::thread::scope(|sc| {
            let handles: Vec<_> = (0..k)
                .map(|b0| {
                    std::thread::Builder::new()
                        .name(format!("band{b0}"))
                        .stack_size(16 << 20)
                        .spawn_scoped(sc, move || {
                            let lock = || board.lock().expect("the bands' board");
                            let mut done: Vec<(usize, BandOut)> = Vec::new();
                            // the scene state and tracer of this thread's last band, when not kept
                            let mut mine: Option<BandWorld> = None;
                            let mut job = Some(b0);
                            while let Some(b) = job {
                                let t = Instant::now();
                                let first = lock().slots[b].first;
                                let w = mine.take().unwrap_or_else(|| {
                                    lock().clones += 1;
                                    BandWorld { st: st0.clone(), tracer: clone_tracer(tr0) }
                                });
                                let mut st = State { s: s0.clone(), lp: lp0.clone(), w };
                                for i in 0..first {
                                    let y = top - i as i16;
                                    if adaptive && i + 1 == first {
                                        // the warm-up: the line above, for the cache row it leaves
                                        st.s.supersample_line(&mut st.w, &mut st.lp, y);
                                        st.w.st.fault = None;
                                    } else {
                                        st.s.skip_line(&mut st.lp, y);
                                    }
                                }
                                let start = carry(&st.w.st, &st.s, &st.lp, adaptive, strict && first > 0);
                                {
                                    let mut g = lock();
                                    // the band above, done and ending as this one starts: its state is not needed
                                    let free = above(&g, first).filter(|&p| g.slots[p].end.as_ref() == Some(&start)).and_then(|p| g.kept[p].take());
                                    g.slots[b].start = Some(start.clone());
                                    drop(g);
                                    drop(free);
                                }
                                let (mut lines, mut faults, mut after) = (Vec::new(), Vec::new(), Vec::new());
                                loop {
                                    let (i, last) = {
                                        let mut g = lock();
                                        let sl = &mut g.slots[b];
                                        if sl.next >= sl.last {
                                            break;
                                        }
                                        sl.next += 1;
                                        (sl.next - 1, sl.last)
                                    };
                                    let (l, f) = sample_one(&mut st, adaptive, top - i as i16, cols);
                                    lines.push(l);
                                    faults.push(f);
                                    if first > 0 && !strict && i - first < TRACKED && i + 1 < last {
                                        after.push(carry(&st.w.st, &st.s, &st.lp, adaptive, false));
                                    }
                                }
                                let mut g = lock();
                                let last = g.slots[b].last;
                                let end = carry(&st.w.st, &st.s, &st.lp, adaptive, strict && last < count);
                                g.slots[b].done = true;
                                g.slots[b].end = Some(end.clone());
                                // the state is kept when the band below may need it (it starts otherwise, or is
                                // not known yet) and for the last band (the frame's state after the loop)
                                let needed = last == count || below(&g, last).is_none_or(|q| g.slots[q].start.as_ref() != Some(&end));
                                let secs = t.elapsed().as_secs_f64();
                                done.push((b, BandOut { lines, faults, after, start, end, state: None, secs }));
                                if needed {
                                    g.kept[b] = Some(st);
                                } else {
                                    mine = Some(st.w);
                                }
                                // take over half of the band with the most estimated cost not yet begun
                                job = None;
                                if steal {
                                    let left = |x: &Slot| prefix[x.last] - prefix[x.next];
                                    let v = (0..g.slots.len()).filter(|&v| !g.slots[v].done && g.slots[v].last - g.slots[v].next >= steal_min).max_by_key(|&v| (left(&g.slots[v]), std::cmp::Reverse(v)));
                                    if let Some(v) = v {
                                        let (nx, ls) = (g.slots[v].next, g.slots[v].last);
                                        let goal = (prefix[nx] + prefix[ls]) / 2;
                                        let mid = prefix.partition_point(|&p| p < goal).clamp(nx + steal_min / 2, ls - steal_min / 2);
                                        g.slots[v].last = mid;
                                        g.slots.push(Slot { first: mid, last: ls, next: mid, done: false, start: None, end: None });
                                        g.kept.push(None);
                                        g.steals += 1;
                                        job = Some(g.slots.len() - 1);
                                    }
                                }
                            }
                            done
                        })
                        .expect("a band thread")
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().expect("a band thread's result")).collect()
        })
    };
    let t2 = Instant::now();
    let Board { slots, mut kept, steals, clones } = board.into_inner().expect("the bands' board");
    for (b, out) in outs.iter_mut() {
        out.state = kept[*b].take();
    }
    outs.sort_by_key(|(b, _)| slots[*b].first);
    let bounds: Vec<usize> = outs.iter().map(|(b, _)| slots[*b].first).chain([count]).collect();
    debug_assert!(outs.iter().zip(bounds.iter().skip(1)).all(|((b, _), &l)| slots[*b].last == l));
    let mut outs: Vec<BandOut> = outs.into_iter().map(|(_, o)| o).collect();
    // in order: a band whose start differs from the end of the band above is sampled again
    // from that end, line by line, until what a line leaves is what the band's own line left
    // (from there on the band's own lines and end hold), or to the band's end
    let (mut reruns, mut rerun_lines) = (0, 0);
    let mut differ = Vec::new();
    for b in 1..outs.len() {
        if outs[b].start != outs[b - 1].end {
            differ.push(diff_text(&outs[b].start, &outs[b - 1].end));
            let mut st = outs[b - 1].state.take().expect("the end state of the band above");
            let (first, last) = (bounds[b], bounds[b + 1]);
            let mut met = false;
            for i in 0..last - first {
                let (l, f) = sample_one(&mut st, adaptive, top - (first + i) as i16, cols);
                outs[b].lines[i] = l;
                outs[b].faults[i] = f;
                rerun_lines += 1;
                if outs[b].after.get(i).is_some_and(|a| *a == carry(&st.w.st, &st.s, &st.lp, adaptive, false)) {
                    met = true;
                    break;
                }
            }
            if !met {
                outs[b].end = carry(&st.w.st, &st.s, &st.lp, adaptive, strict && last < count);
                outs[b].state = Some(st);
            }
            reruns += 1;
        } else {
            outs[b - 1].state = None;
        }
    }
    let t3 = Instant::now();
    for (b, out) in outs.iter().enumerate() {
        for (i, samples) in out.lines.iter().enumerate() {
            let y = top - (bounds[b] + i) as i16;
            s.line_checks(world);
            s.line[cols.0..=cols.1].copy_from_slice(samples);
            if let Some(v) = out.faults[i] {
                world.st.fault.get_or_insert(v);
            }
            if adaptive {
                s.supersample_line_out(world);
            } else {
                s.sample_line_out(world, y);
            }
        }
    }
    // the scene state as the last band left it (the working blocks as on one thread)
    if let Some(last) = outs.last_mut().and_then(|o| o.state.take()) {
        let fault = world.st.fault;
        *world.st = last.w.st;
        world.st.fault = fault;
        world.tracer.bsp = last.w.tracer.bsp;
    }
    if adaptive {
        s.supersample_end(world, &lp);
    } else {
        s.sample_end(world);
    }
    let t4 = Instant::now();
    let copies1 = crate::pool::copies();
    let secs: Vec<String> = outs.iter().map(|o| format!("{:.3}", o.secs)).collect();
    let sizes: Vec<String> = (0..outs.len()).map(|b| (bounds[b + 1] - bounds[b]).to_string()).collect();
    eprintln!(
        "bands\tthreads={n}\tadaptive={}\tlines={count}\tbands={}\treruns={reruns}\trerun_lines={rerun_lines}\tbegin_s={:.3}\tsample_s={:.3}\torder_s={:.3}\tout_s={:.3}\tsteals={steals}\tcopies={clones}\tshared_mb={:.1}\tper_copy_mb={:.1}\tpool_copies={}\tband_s={}\tband_lines={}{}",
        adaptive as i32,
        outs.len(),
        (t1 - t0).as_secs_f64(),
        (t2 - t1).as_secs_f64(),
        (t3 - t2).as_secs_f64(),
        (t4 - t3).as_secs_f64(),
        shared_bytes(&world.st, &world.tracer) as f64 / 1e6,
        (own_bytes(&world.st, &world.tracer) + sampler_bytes(s)) as f64 / 1e6,
        copies1.0 - copies0.0,
        secs.join(","),
        sizes.join(","),
        if differ.is_empty() { String::new() } else { format!("\tdiffer={}", differ.join(";")) }
    );
}

/// The bytes of the arrays the band threads share with the frame's state ([`crate::pool::Pool`]):
/// triangles, vertices, texture coordinates, the tree's nodes and links, the pictures' pixels.
fn shared_bytes(st: &ShadeState, t: &Tracer) -> usize {
    let pixels: usize = st.pictures.iter().map(|p| p.pixels.as_ref().map_or(0, |x| x.block.len())).sum();
    st.tns.bytes() + st.verts.bytes() + st.uvs.bytes() + t.bsp.nodes.bytes() + t.bsp.links.bytes() + pixels
}

/// About the bytes a band's copy of the scene state and tracer holds of its own (the arrays'
/// items only).
fn own_bytes(st: &ShadeState, t: &Tracer) -> usize {
    use std::mem::size_of_val;
    let lights: usize = st.lights.iter().map(|l| size_of_val(l) + l.zmap.iter().map(|r| r.len() * 4).sum::<usize>() + l.select_table.len() * 4).sum();
    st.materials.iter().map(|m| size_of_val(m) + 2 * 201 * 8).sum::<usize>()
        + st.maps.len() * std::mem::size_of::<crate::shade::ShadeMap>()
        + st.models.iter().map(|m| size_of_val(m) + m.materials.len() * 16).sum::<usize>()
        + lights
        + st.tex3ds.len() * std::mem::size_of::<crate::t3d_ren::ShadeTex3d>()
        + t.heap.live.len() * 8
        + t.heap.poked.len() * 8
}

/// About the bytes of a copy of the sampler (the arrays' items only).
fn sampler_bytes(s: &Sampler) -> usize {
    use std::mem::size_of;
    s.slices.len() * size_of::<crate::render::Slice>()
        + s.scan_lines.len() * size_of::<Option<usize>>()
        + s.columns.len() * size_of::<Option<usize>>()
        + s.line.len() * size_of::<Rgbazo>()
        + s.ss.rows.iter().map(|r| r.len() * size_of::<Rgbazo>()).sum::<usize>()
        + s.ss.winner.len() * size_of::<Ray>()
}

/// `RENDER_BANDS_STEAL=0`: the bands stay as first cut (no band takes over part of another).
fn steal_on() -> bool {
    static S: OnceLock<bool> = OnceLock::new();
    *S.get_or_init(|| !std::env::var("RENDER_BANDS_STEAL").is_ok_and(|v| v.trim() == "0"))
}

/// Which parts of two [`Carry`] differ, for the log line.
fn diff_text(a: &Carry, b: &Carry) -> String {
    let mut parts = Vec::new();
    let mut fields = [0usize; 9];
    for (x, y) in a.work.iter().zip(&b.work) {
        for slot in 0..20 {
            if x.0[slot] != y.0[slot] {
                fields[field_of(slot)] += 1;
            }
        }
        if x.1 != y.1 {
            fields[8] += 1;
        }
    }
    for (f, c) in fields.iter().enumerate() {
        if *c > 0 {
            parts.push(format!("{}:{c}", WORK_FIELDS.get(f).copied().unwrap_or("tex3d")));
        }
    }
    let seam = a.seam.iter().zip(&b.seam).filter(|(x, y)| x != y).count();
    if seam > 0 {
        parts.push(format!("seam:{seam}"));
    }
    if a.strict != b.strict {
        parts.push("strict".into());
    }
    parts.join(",")
}

// ------------------------------------------------------------------------------ the probe

/// How a test copy of the probe changes the state before each sample's shading.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Nothing changed: a copy run on its own from the frame's start.
    Plain,
    /// The working blocks set back to the frame start's before each shading call.
    ResetWork,
    /// The working blocks set to a NaN before each shading call.
    PoisonWork,
}

struct ProbeWorld<'a> {
    w: &'a mut BandWorld,
    mode: Mode,
    start_work: &'a [MatWork],
}

const POISON: u32 = 0x7fc0_dead;

impl SampleEnv for ProbeWorld<'_> {
    fn geometry(&self) -> (&[Tn], &[RVertex]) {
        self.w.geometry()
    }
    fn shade(&mut self, ray: &mut Ray, under: Rgbazo, primary: i16) -> Rgbazo {
        match self.mode {
            Mode::Plain => {}
            Mode::ResetWork => {
                for (m, f) in self.w.st.materials.iter_mut().zip(self.start_work) {
                    m.work = *f;
                }
            }
            Mode::PoisonWork => {
                let p = f32::from_bits(POISON);
                for m in &mut self.w.st.materials {
                    m.work = MatWork { base_ambient: [p; 4], ambient: [p; 4], diffuse: [p; 4], specular: [p; 4], reflectivity: p, opacity: p, refraction: p, existence: p };
                }
            }
        }
        self.w.shade(ray, under, primary)
    }
    fn chunk(&mut self) -> &mut ChunkBuffer {
        self.w.chunk()
    }
    fn chunk_malloc(&mut self, size: i32) -> u32 {
        self.w.chunk_malloc(size)
    }
}

/// The frame itself, sampled in order, with a count of the shading calls after which a
/// working block differs from the frame start's.
struct Counting<'a, 'b> {
    w: &'a mut World<'b>,
    start: &'a [([u32; 20], u32)],
    calls: u64,
    carried: u64,
}

impl SampleEnv for Counting<'_, '_> {
    fn geometry(&self) -> (&[Tn], &[RVertex]) {
        self.w.geometry()
    }
    fn shade(&mut self, ray: &mut Ray, under: Rgbazo, primary: i16) -> Rgbazo {
        let c = self.w.shade(ray, under, primary);
        self.calls += 1;
        if self.w.st.materials.iter().zip(self.start).any(|(m, f)| work_bits(&m.work) != f.0 || m.tex3d != f.1) {
            self.carried += 1;
        }
        c
    }
    fn chunk(&mut self) -> &mut ChunkBuffer {
        self.w.chunk()
    }
    fn chunk_malloc(&mut self, size: i32) -> u32 {
        self.w.chunk_malloc(size)
    }
}

struct TestCopy {
    name: &'static str,
    st: State,
    pw_mode: Mode,
    /// Poison the layer rays' fields at each line start (with the working blocks left).
    poison_rays: bool,
    /// Empty the cache row of the line above at each line start (adaptive only).
    empty_seam: bool,
    lines: u64,
    samples: u64,
    first_line: Option<i16>,
}

fn poison_ray(r: &mut Ray) {
    let p = f32::from_bits(POISON);
    r.dir = crate::types::Vector::new(p, p, p);
    r.v18 = crate::types::Vector::new(p, p, p);
    r.hit = crate::types::Vector::new(p, p, p);
    r.t = p;
    r.w38 = POISON;
    r.w40 = POISON;
    r.w44 = POISON;
}

/// The probe (`RENDER_BANDS_PROBE`): the frame sampled and written in order (its output as
/// [`Sampler::sample_frame`]'s), and alongside it test copies from the frame start, each line
/// compared bit for bit with the frame's: `reset` (working blocks set back before each shading
/// call), `poison` (working blocks a NaN before each call), `rays` (the layer rays' fields a
/// NaN at each line start), `seam` (adaptive: the cache row of the line above emptied at each
/// line start). At the end the scene state is compared with the frame start's, part by part.
pub(crate) fn probe(s: &mut Sampler, world: &mut World, adaptive: bool) {
    let mut lp = if adaptive { s.supersample_begin(world) } else { s.sample_begin(world) };
    let top = s.region[3] as i16;
    let count = (s.region[3] - s.region[2] + 1).max(0) as usize;
    let cols = (s.region[0].max(0) as usize, s.region[1].max(0) as usize);
    let st0 = world.st.clone();
    let hashes0 = part_hashes(&st0);
    let bsp0 = hash_of(|h| {
        use std::fmt::Write as _;
        let _ = write!(h, "{:?}", world.tracer.bsp);
    });
    let start = carry(&st0, s, &lp, false, false).work;
    let start_work: Vec<MatWork> = st0.materials.iter().map(|m| m.work).collect();
    let new_state = |s: &Sampler, lp: &LineLoop, w: &World| State { s: s.clone(), lp: lp.clone(), w: BandWorld { st: w.st.clone(), tracer: clone_tracer(&w.tracer) } };
    let mut copies = vec![
        TestCopy { name: "reset", st: new_state(s, &lp, world), pw_mode: Mode::ResetWork, poison_rays: false, empty_seam: false, lines: 0, samples: 0, first_line: None },
        TestCopy { name: "poison", st: new_state(s, &lp, world), pw_mode: Mode::PoisonWork, poison_rays: false, empty_seam: false, lines: 0, samples: 0, first_line: None },
        TestCopy { name: "rays", st: new_state(s, &lp, world), pw_mode: Mode::Plain, poison_rays: true, empty_seam: false, lines: 0, samples: 0, first_line: None },
    ];
    if adaptive {
        copies.push(TestCopy { name: "seam", st: new_state(s, &lp, world), pw_mode: Mode::Plain, poison_rays: false, empty_seam: true, lines: 0, samples: 0, first_line: None });
    }
    let (mut calls, mut carried) = (0u64, 0u64);
    let mut lines_carrying = 0u64;
    let mut fields = [0u64; 9];
    let mut materials_changed = vec![false; st0.materials.len()];
    let t0 = Instant::now();
    for i in 0..count {
        let y = top - i as i16;
        s.line_checks(world);
        {
            let mut c = Counting { w: world, start: &start, calls: 0, carried: 0 };
            if adaptive {
                s.supersample_line(&mut c, &mut lp, y);
            } else {
                s.sample_line(&mut c, &mut lp, y);
            }
            calls += c.calls;
            carried += c.carried;
        }
        let mut any = false;
        for (k, (m, f)) in world.st.materials.iter().zip(&start).enumerate() {
            let b = work_bits(&m.work);
            for slot in 0..20 {
                if b[slot] != f.0[slot] {
                    fields[field_of(slot)] += 1;
                    any = true;
                    materials_changed[k] = true;
                }
            }
            if m.tex3d != f.1 {
                fields[8] += 1;
                any = true;
                materials_changed[k] = true;
            }
        }
        lines_carrying += any as u64;
        let line = &s.line[cols.0..=cols.1];
        for c in &mut copies {
            if c.poison_rays {
                c.st.lp.rays.iter_mut().for_each(poison_ray);
                c.st.s.ss.winner.iter_mut().for_each(poison_ray);
            }
            if c.empty_seam {
                let sub = c.st.s.ss.sub as usize;
                let first = c.st.lp.first.max(0) as usize;
                if let Some(row) = c.st.s.ss.rows.get_mut(sub) {
                    for e in row.iter_mut().skip(first) {
                        e.r = -1.0;
                    }
                }
            }
            let State { s: cs, lp: clp, w } = &mut c.st;
            let mut pw = ProbeWorld { w, mode: c.pw_mode, start_work: &start_work };
            if adaptive {
                cs.supersample_line(&mut pw, clp, y);
            } else {
                cs.sample_line(&mut pw, clp, y);
            }
            let d = cs.line[cols.0..=cols.1].iter().zip(line).filter(|(a, b)| rgbazo_bits(a) != rgbazo_bits(b)).count() as u64;
            if d > 0 {
                c.lines += 1;
                c.samples += d;
                c.first_line.get_or_insert(y);
            }
        }
        if adaptive {
            s.supersample_line_out(world);
        } else {
            s.sample_line_out(world, y);
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    if adaptive {
        s.supersample_end(world, &lp);
    } else {
        s.sample_end(world);
    }
    // the parts of the scene state the frame left changed (the "met" bits and the fault aside)
    let parts: Vec<&str> = part_hashes(&world.st).into_iter().zip(hashes0).filter(|(a, b)| a != b).map(|(a, _)| a.0).collect();
    let met = world.st.tns.iter().filter(|t| t.flags & TN_MET != 0).count();
    let bsp_same = hash_of(|h| {
        use std::fmt::Write as _;
        let _ = write!(h, "{:?}", world.tracer.bsp);
    }) == bsp0;
    let mut text = String::new();
    use std::fmt::Write as _;
    let _ = writeln!(text, "adaptive={}", adaptive as i32);
    let _ = writeln!(text, "dof={}\tlevel={}\tsub={}\tlines={count}\tcolumns={}\tsample_s={secs:.3}", s.dof_on, s.level, s.ss.sub, cols.1 + 1 - cols.0);
    let _ = writeln!(text, "materials={}\tmaterials_changed={}\tshade_calls={calls}\tshade_calls_leaving_work_changed={carried}\tlines_ending_with_work_changed={lines_carrying}", st0.materials.len(), materials_changed.iter().filter(|c| **c).count());
    let changed: Vec<String> = fields.iter().enumerate().filter(|(_, c)| **c > 0).map(|(f, c)| format!("{}:{c}", WORK_FIELDS.get(f).copied().unwrap_or("tex3d"))).collect();
    let _ = writeln!(text, "work_fields_changed={}", if changed.is_empty() { "none".to_string() } else { changed.join(",") });
    let _ = writeln!(text, "other_state_changed={}\tmet_bits_left={met}\tbsp_unchanged={}\tfault={:?}\tmailbox={}", if parts.is_empty() { "none".to_string() } else { parts.join(",") }, bsp_same as i32, world.st.fault, world.tracer.mailbox.current);
    for c in &copies {
        let _ = writeln!(text, "copy={}\tlines_different={}\tsamples_different={}\tfirst_line_different={:?}", c.name, c.lines, c.samples, c.first_line);
    }
    if let Some(path) = probe_path() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(text.as_bytes());
        }
    }
}
