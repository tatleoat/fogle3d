// SPDX-License-Identifier: MIT
//! One frame from a prepared scene, on this crate's own records. Nothing here reproduces a
//! function of the original: it binds ported pieces the way `SI_image`'s frame loop does
//! through its callees (`SI_sample` or `SI_supersample`, with `SI_shade`, the tree walk
//! and the chunk memory behind them, and `SI_Output` in front of the picture writer).
//!
//! It is what stands behind the two frame loops' addresses in [`crate::adapter`], and what
//! `render-check` renders the sample scenes with.

use crate::mem_chunk::{chunk_allocate, ChunkBuffer, CountingHeap};
use crate::render::{Antialias, FrameEnv, SampleEnv};
use crate::renutil::{LineSink, OutputLine, OutputState};
use crate::rnd_tag::TagImage;
use crate::shade::{shade, ShadeEnv, ShadeState};
use crate::trace::{s_trace, search, shadow_map, trace, Bsp, Mailbox};
use crate::types::{Ray, Rgbazo};
use crate::update::Prepared;
use si_math::geometry::{RVertex, Tn};
use si_math::types::Vector;

/// The texture stage alone: what `shadowMap` calls. The tree is not reached from there.
struct TexturesOnly;
impl ShadeEnv for TexturesOnly {
    fn search(&mut self, _: &ShadeState, _: Vector) -> u32 {
        unreachable!("shadowMap does not search the tree")
    }
    fn s_trace(&mut self, _: &mut ShadeState, _: &mut Ray, _: usize, _: f32, _: usize) -> [f32; 4] {
        unreachable!("shadowMap traces no ray")
    }
    fn trace(&mut self, _: &ShadeState, _: &mut Ray, _: usize) -> bool {
        unreachable!("shadowMap traces no ray")
    }
}

/// What `SI_shade` calls outside `shade.c`, all ported: the tree, the ray counter, the
/// chunk memory the secondary rays come from; the texture routines are the trait's defaults.
pub struct Tracer {
    pub bsp: Bsp,
    pub mailbox: Mailbox,
    pub chunk: ChunkBuffer,
    pub heap: CountingHeap,
}

impl ShadeEnv for Tracer {
    fn search(&mut self, _: &ShadeState, p: Vector) -> u32 {
        // (without a cell the original stores what si_search returns: null)
        search(&self.bsp.nodes, self.bsp.root, p).map_or(0, |n| n as u32)
    }
    fn s_trace(&mut self, st: &mut ShadeState, ray: &mut Ray, tn: usize, distance: f32, light: usize) -> [f32; 4] {
        s_trace(&mut self.bsp, &mut self.mailbox, st, ray, tn, distance, light, &mut |st, ray, mat, p| shadow_map(st, &mut TexturesOnly, ray, mat, p))
    }
    fn trace(&mut self, st: &ShadeState, ray: &mut Ray, tn: usize) -> bool {
        trace(&mut self.bsp, &mut self.mailbox, ray, &st.tns, &st.verts, tn)
    }
    fn ray_alloc(&mut self) {
        self.chunk.malloc(&mut self.heap, 0x48);
    }
}

/// One line of the picture as the frame loop hands it to `SI_rleWrite`: the line's number
/// (0 at the bottom) and its red, green, blue and alpha bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub y: i32,
    pub red: Vec<u8>,
    pub green: Vec<u8>,
    pub blue: Vec<u8>,
    pub alpha: Vec<u8>,
}

/// The frame loop's surroundings: the scene, and a sink that keeps the lines.
pub(crate) struct World<'a> {
    pub(crate) st: &'a mut ShadeState,
    pub(crate) tracer: Tracer,
    lines: Vec<Line>,
    status: Vec<String>,
    /// The output dither `SI_fsDitherInit` set up for this frame, with the float at
    /// 0x08790e38 (the noise's amplitude), when the dither is on (0x08790e30).
    dither: Option<(&'a mut si_imgfmt::gradation::OutputDither, f32)>,
    /// The depth file the frame loops opened and wrote (`zFp`, 0x087932c0).
    depth: Option<DepthFile>,
}

/// The depth file of a frame, as the frame loops leave it: `fopen(name, "wb+")` at the start
/// (`<output name>.<frame>.Zpic`), one `SI_byteswap_fwrite(depth, 4, width, zFp)` per line
/// (rows of big-endian floats, no header; finding 0014, section on the compositor), `fclose`
/// at the end. The adapter replays it through the machine's files.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DepthFile {
    pub name: Vec<u8>,
    pub bytes: Vec<u8>,
    pub closed: bool,
}

impl SampleEnv for World<'_> {
    fn geometry(&self) -> (&[Tn], &[RVertex]) {
        (&self.st.tns, &self.st.verts)
    }
    fn shade(&mut self, ray: &mut Ray, under: Rgbazo, primary: i16) -> Rgbazo {
        shade(self.st, &mut self.tracer, ray, under, primary)
    }
    fn chunk(&mut self) -> &mut ChunkBuffer {
        &mut self.tracer.chunk
    }
    fn chunk_malloc(&mut self, size: i32) -> u32 {
        self.tracer.chunk.malloc(&mut self.tracer.heap, size)
    }
}

impl LineSink for World<'_> {
    /// `SI_fsDitherScan (0x085cfb60)`: si-imgfmt's port ([`si_imgfmt::gradation::OutputDither::scan`])
    /// on the state `SI_fsDitherInit` left, over the width it was given. A row shorter than
    /// that width (the original would read and write past it) is left as it is.
    fn dither_scan(&mut self, samples: &[Rgbazo], red: &mut [u8], green: &mut [u8], blue: &mut [u8], alpha: &mut [u8], depth: &mut [f32]) {
        let Some((d, amplitude)) = self.dither.as_mut() else { return };
        let input: Vec<si_imgfmt::gradation::Rgbazo> = samples.iter().map(|s| si_imgfmt::gradation::Rgbazo { rgba: [s.r, s.g, s.b, s.a], depth: s.z }).collect();
        let _ = d.scan(&input, [red, green, blue, alpha], depth, *amplitude);
    }
    /// `si_incrustration (0x08261240)` is not ported.
    fn incrustation(&mut self, _: &mut OutputLine, _: i32) {}
    fn rle_write(&mut self, y: i32, width: i32, line: &OutputLine) {
        // a line handed over after the shading met a fault is one the original never wrote:
        // its process ended at the fault ([`ShadeState::fault`])
        if self.st.fault.is_some() {
            return;
        }
        let w = width.max(0) as usize;
        self.lines.push(Line { y, red: line.red[..w].to_vec(), green: line.green[..w].to_vec(), blue: line.blue[..w].to_vec(), alpha: line.alpha[..w].to_vec() });
    }
    /// `SI_byteswap_fwrite(depth, 4, width, zFp)` in `SI_Output` and the frame loops: the
    /// floats go out big-endian (the original swaps the items in place, writes, and swaps
    /// them back). With no file open (`fopen` failed: `zFp` null) the original would write
    /// through a null stream and fault; nothing is kept here.
    fn depth_write(&mut self, row: &[f32]) {
        if let Some(d) = self.depth.as_mut() {
            for v in row {
                d.bytes.extend_from_slice(&v.to_bits().to_be_bytes());
            }
        }
    }
    fn fb_write(&mut self, _: i16) {}
    fn status(&mut self, text: &str) {
        self.status.push(text.to_string());
    }
}

impl FrameEnv for World<'_> {
    fn display_status(&mut self, text: &str) {
        self.status.push(text.to_string());
    }
    fn check_render_abort(&mut self) {}
    fn check_for_exit(&mut self) {}
    fn depth_open(&mut self, name: &[u8]) {
        self.depth = Some(DepthFile { name: name.to_vec(), bytes: Vec::new(), closed: false });
    }
    fn depth_close(&mut self) {
        if let Some(d) = self.depth.as_mut() {
            d.closed = true;
        }
    }
    fn tag_write_scanline(&mut self, _: &TagImage) {}
}

/// Renders the frame `update` prepared: `SI_supersample` when adaptive supersampling is on
/// (0x08790d3a = 1), else `SI_sample`, as `SI_image` chooses. Returns the lines in the
/// order the loop writes them (top line first), background lines included. When the
/// shading meets a fault of the original ([`ShadeState::fault`]) the lines end with the
/// last one written before it.
///
/// Left out, each because its routine is not ported: incrustation, the depth file,
/// tags, the display. The chunk memory is a
/// buffer of its own of 200 blocks of 5 MB, as `SI_matterInit` makes `SI_renChunk`; only
/// its books are kept. Without dither: [`render_dithered`] has it.
pub fn render(p: &mut Prepared, background_bytes: [u8; 4]) -> Vec<Line> {
    render_dithered(p, background_bytes, None)
}

/// [`render`] with the output dither on (0x08790e30): `SI_Output` hands each line to
/// `SI_fsDitherScan` on the state `SI_fsDitherInit` made for this frame, with the noise's
/// amplitude (the float at 0x08790e38).
pub fn render_dithered(p: &mut Prepared, background_bytes: [u8; 4], dither: Option<(&mut si_imgfmt::gradation::OutputDither, f32)>) -> Vec<Line> {
    render_with_depth(p, background_bytes, dither, None).0
}

/// [`render_dithered`] with the depth file (switch 0x08790e2c, `Z_CHANNEL YES`) when `depth`
/// holds the output name (the string at 0x08790d5c, as written: not prefixed with the
/// picture path): the frame loops open `<name>.<frame>.Zpic`, write a line of floats after
/// each picture line (the lines above and below the region included) and close it; the file
/// they leave is returned. A frame whose shading met a fault leaves it unclosed.
pub fn render_with_depth(p: &mut Prepared, background_bytes: [u8; 4], dither: Option<(&mut si_imgfmt::gradation::OutputDither, f32)>, depth: Option<Vec<u8>>) -> (Vec<Line>, Option<DepthFile>) {
    let mut s = p.sampler();
    let width = p.settings.width;
    let bytes = || vec![0u8; (width.max(0) as usize) + 10];
    // the byte lines (0x08790b40 …) as this frame's `si_initFade` left them: their first
    // `width` bytes the background's; `SI_Output` writes only the columns x0..=x1 over them
    let filled = |b: u8| {
        let mut line = bytes();
        line[..width.max(0) as usize].fill(b);
        line
    };
    s.background_bytes = background_bytes;
    s.out = OutputState {
        x0: p.view.region_px[0],
        x1: p.view.region_px[1],
        red: filled(background_bytes[0]),
        green: filled(background_bytes[1]),
        blue: filled(background_bytes[2]),
        alpha: filled(background_bytes[3]),
        depth: vec![0.0; (width.max(0) as usize) + 10],
        line: OutputLine { red: bytes(), green: bytes(), blue: bytes(), alpha: bytes() },
        width,
        frame: p.options.frame,
        to_file: true,
        dither: dither.is_some(),
        depth_file: depth.is_some(),
        ..OutputState::default()
    };
    if let Some(name) = depth {
        s.name = name;
    }
    s.aa = Antialias { size: p.view.filter_size as i32, filter: p.view.filter, blank: p.blank, ..Antialias::default() };
    // what si_initDof left (crate::bound::init_dof): the limit, the line counts, the tables
    s.ss.dof_limit = p.dof.limit;
    s.dof = crate::render::DofBlur { lines: p.view.dof_lines, half: p.view.dof_half, radius: p.dof.radius, z2: p.st.glb.dof_z2, filters: p.dof.filters.clone(), ..crate::render::DofBlur::default() };
    let mut heap = CountingHeap::new(0x1000_0000, 0xf000_0000);
    let chunk = chunk_allocate(&mut heap, 200, 5_000_000).expect("a chunk buffer");
    let bsp = std::mem::take(&mut p.bsp);
    let adaptive = p.options.adaptive == 1;
    let mut world = World { st: &mut p.st, tracer: Tracer { bsp, mailbox: Mailbox { current: 0 }, chunk, heap }, lines: Vec::new(), status: Vec::new(), dither, depth: None };
    world.tracer.chunk.initialize();
    // several threads inside the frame (`RENDER_THREADS`, crate::bands); one by default
    if crate::bands::probe_wanted() {
        crate::bands::probe(&mut s, &mut world, adaptive);
    } else if crate::bands::threads() > 1 && crate::bands::splittable(&s, &world) {
        crate::bands::render(&mut s, &mut world, adaptive, crate::bands::threads());
    } else if adaptive {
        s.supersample_frame(&mut world);
    } else {
        s.sample_frame(&mut world);
    }
    let World { tracer, lines, depth, .. } = world;
    p.bsp = tracer.bsp;
    (lines, depth)
}

// ------------------------------------------------------------------ the scene of a frame

use crate::geometry::{Geometry, GeometryEnv};
use crate::jitter::Drand48;
use crate::materials::{self, DictElem, MaterialsEnv, Preview, SetupError, TextureDict};
use crate::softtriangle::Scene;
use crate::update::{init_buffers, init_filter, init_viewport, update, UpdateEnv};

/// Where texture pictures come from: the bytes of `<name>.pic`, or of the default picture
/// (`<SI_resdir>/noIcon.pic`) when `default` is set.
pub trait PictureFiles {
    fn picture_file(&mut self, name: &[u8], default: bool) -> Option<Vec<u8>>;
}

/// No pictures at all.
pub struct NoPictures;
impl PictureFiles for NoPictures {
    fn picture_file(&mut self, _: &[u8], _: bool) -> Option<Vec<u8>> {
        None
    }
}

/// The converters that are ported (meshes; faces of splines and patches whose records the
/// scene carries; NURBS surfaces the adapter converted on the image, `NodeDef::NurbsOf`),
/// and nothing for the others (metaball systems and faces of NURBS curves give no
/// triangles: their tessellators are not ported).
use crate::bound::Ported;

struct Files<'a> {
    files: &'a mut dyn PictureFiles,
    rng: &'a mut Drand48,
    level: i16,
    status: &'a mut Vec<String>,
    /// The PICTURES chapter as the job keeps it.
    pictures: &'a mut Vec<DictElem>,
    events: &'a mut Vec<PictureEvent>,
    errno: bool,
}

/// What the picture list asked of the dictionary during an update, in order: a record
/// entered (`SI_elemdictAddPicture`: the picture and its name) or taken out
/// (`SI_elemdictPictureElemRemove`). [`crate::adapter::write_back`] makes the same calls
/// on the image's dictionary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PictureEvent {
    Added(usize, Vec<u8>),
    Removed(usize),
}

impl MaterialsEnv for Files<'_> {
    fn drand48(&mut self) -> f64 {
        self.rng.next()
    }
    fn picture_file(&mut self, name: &[u8], default: bool) -> Option<Vec<u8>> {
        self.files.picture_file(name, default)
    }
    fn status(&mut self, text: &str) {
        self.status.push(text.to_string());
    }
    fn sampling_level(&self) -> i16 {
        self.level
    }
    fn pow(&self, base: f64, exponent: f64) -> (si_x87::F80, bool) {
        let v = si_x87::trig::pow(base, exponent);
        let d = v.to_f64();
        (v, self.errno && (!d.is_finite() || (d == 0.0 && base != 0.0)))
    }
    /// `SI_elemdictAddPicture (0x08102ab0)` is the dictionary's and not ported: here the
    /// record becomes the chapter's next element.
    fn dict_add_picture(&mut self, st: &ShadeState, picture: usize) {
        self.pictures.push(DictElem { data: Some(picture), unused: 0, dirty: 0 });
        self.events.push(PictureEvent::Added(picture, st.pictures[picture].name.clone()));
    }
    /// `SI_elemdictPictureElemRemove (0x08102c34)` is the dictionary's and not ported: here
    /// the record's element is taken out of use.
    fn dict_remove_picture(&mut self, _: &ShadeState, picture: usize) {
        if let Some(e) = self.pictures.iter_mut().find(|e| e.data == Some(picture) && e.unused == 0) {
            e.unused = 1;
        }
        self.events.push(PictureEvent::Removed(picture));
    }
}

/// A scene in this crate's own records with what the element dictionary says about it:
/// everything `SI_update` asks for through its hooks, answered with the ported routines in
/// the order finding 0012, section 12 gives. Not a record of the original.
pub struct Job<'a> {
    /// The source geometry, one entry per node of [`Geometry::models`].
    pub scene: Scene,
    /// The MATERIALS chapter: each element's record is an index into
    /// [`ShadeState::materials`].
    pub materials: Vec<DictElem>,
    /// The MODELS chapter: each element's record is its own number, an index into
    /// [`ShadeState::models`].
    pub models: Vec<DictElem>,
    /// The node of [`Geometry::models`] the dictionary holds for an element number.
    pub node_of: Vec<Option<usize>>,
    /// Per node, its own 3D texture (index + 1 into [`ShadeState::tex3ds`]; 0: none).
    pub own_tex3d: Vec<u32>,
    /// The PICTURES, 2D textures and MODELS chapters as the texture set-up walks them;
    /// brought up to the lists' lengths before each use.
    pub textures: TextureDict,
    /// The PICTURES and 2D textures chapters were read from a dictionary: they are walked
    /// as they are. Otherwise each is brought up to one element per record before use.
    pub chapters_given: bool,
    /// What the picture list asked of the dictionary, in order.
    pub picture_events: Vec<PictureEvent>,
    /// Whether the C library's `pow` reports a domain or range error through `errno`, as
    /// a real one does (`decayTable` then takes the value as 0). The emulation harness's
    /// stand-in for `pow` does not: a check against it clears this.
    pub pow_sets_errno: bool,
    pub files: &'a mut dyn PictureFiles,
    /// The C library's `drand48` state (the jitter tables).
    pub rng: Drand48,
    /// The preview's two switches (0x0869bc8a, 0x08791640); outside a preview: no
    /// selection call, optimisation on.
    pub preview: Preview,
    /// The background's four bytes (0x08790d14), as `si_initFade` leaves them.
    pub background_bytes: [u8; 4],
    /// What the set-up said and what it could not do: status lines, fatal texts of
    /// routines that would leave the render, hooks that are not ported.
    pub notes: Vec<String>,
}

impl<'a> Job<'a> {
    pub fn new(files: &'a mut dyn PictureFiles) -> Job<'a> {
        Job {
            scene: Scene::default(),
            materials: Vec::new(),
            models: Vec::new(),
            node_of: Vec::new(),
            own_tex3d: Vec::new(),
            textures: TextureDict::default(),
            chapters_given: false,
            picture_events: Vec::new(),
            pow_sets_errno: true,
            files,
            rng: Drand48::seeded(0),
            preview: Preview { selection_call: 0, optimize: 1 },
            background_bytes: [0; 4],
            notes: Vec::new(),
        }
    }

    /// The part of `SI_initialize` that is ported on this crate's records: `initData`,
    /// `si_initViewport`, `initBuffers`, `initFilter`, in that order. (`SI_chunkInitialize`
    /// before them belongs to the chunk buffer [`render`] makes; the dictionary walks
    /// after them are ported in place, [`crate::initialize::propagate`] and its
    /// companions.)
    pub fn begin(&mut self, p: &mut Prepared) {
        crate::initialize::init_data(p, self);
        init_viewport(&p.camera, &p.settings, &mut p.view);
        let mut counters = [p.stats[crate::update::stat::BINS], p.stats[crate::update::stat::SCANLINE], p.stats[crate::update::stat::OUTPUT]];
        p.buffers = init_buffers(&p.view, p.settings.width, &mut counters);
        p.stats[crate::update::stat::BINS] = counters[0];
        p.stats[crate::update::stat::SCANLINE] = counters[1];
        p.stats[crate::update::stat::OUTPUT] = counters[2];
        p.g.bins.lines = vec![Vec::new(); p.buffers.scan_lines.max(0) as usize];
        init_filter(p.settings.level, &mut p.view);
    }

    /// `SI_update(time)` and then the frame: the lines as the frame loop writes them, top
    /// line first; a frame without a filed triangle is all background, as `SI_image`
    /// writes it. `Err` is the text of a fatal error.
    pub fn frame(&mut self, p: &mut Prepared, time: f32) -> Result<Vec<Line>, String> {
        update(p, self, time).map_err(|e| e.to_string())?;
        if let Some(fatal) = self.notes.iter().find(|n| n.starts_with("fatal: ")) {
            return Err(fatal.clone());
        }
        if p.g.bins.accepted == 0 {
            let width = p.settings.width.max(0) as usize;
            let b = self.background_bytes;
            let top = si_x87::x(p.view.picture[3]).to_i32(si_x87::Round::Zero);
            return Ok((0..=top).rev().map(|y| Line { y, red: vec![b[0]; width], green: vec![b[1]; width], blue: vec![b[2]; width], alpha: vec![b[3]; width] }).collect());
        }
        Ok(render(p, self.background_bytes))
    }

    fn fail(&mut self, what: &str, e: SetupError) {
        self.notes.push(match e {
            SetupError::Fatal(text) => format!("fatal: {text}"),
            SetupError::Null(at) => format!("fatal: {what}: the original reads through a null pointer ({at})"),
        });
    }
}

impl GeometryEnv for Job<'_> {
    fn collect(&mut self, st: &mut ShadeState, g: &mut Geometry, node: usize) {
        self.scene.collect(st, g, node, &mut Ported);
    }
}

impl UpdateEnv for Job<'_> {
    fn status_message(&mut self, text: &str) {
        self.notes.push(text.to_string());
    }
    fn init_fade(&mut self, p: &mut Prepared) {
        let n = (p.settings.width.max(0) as usize) + 10;
        let (mut a, mut b, mut c, mut d) = (vec![0u8; n], vec![0u8; n], vec![0u8; n], vec![0u8; n]);
        self.background_bytes = p.init_fade([&mut a, &mut b, &mut c, &mut d]);
    }
    fn init_dof(&mut self, p: &mut Prepared) {
        crate::bound::init_dof(p, &mut self.notes);
    }
    fn materials(&mut self, p: &mut Prepared) {
        let node_of = self.node_of.clone();
        let mut env = Files { files: &mut *self.files, rng: &mut self.rng, level: p.settings.level, status: &mut self.notes, pictures: &mut self.textures.pictures, events: &mut self.picture_events, errno: self.pow_sets_errno };
        let r = p.materials(&self.materials, &self.models, &|e| node_of.get(e).copied().flatten(), &mut env);
        if let Err(e) = r {
            self.fail("SI_materials", e);
        }
    }
    fn maps_list(&mut self, p: &mut Prepared) {
        let _ = materials::get_maps_list(&p.st);
    }
    fn textures(&mut self, p: &mut Prepared) {
        let elem = |i: usize| DictElem { data: Some(i), unused: 0, dirty: 0 };
        // (a picture record SI_global2dMapInsert made has its element already)
        while !self.chapters_given && self.textures.pictures.len() < p.st.pictures.len() {
            self.textures.pictures.push(elem(self.textures.pictures.len()));
        }
        while !self.chapters_given && self.textures.maps.len() < p.st.maps.len() {
            self.textures.maps.push(elem(self.textures.maps.len()));
        }
        let mut dict = std::mem::take(&mut self.textures);
        let mut env = Files { files: &mut *self.files, rng: &mut self.rng, level: p.settings.level, status: &mut self.notes, pictures: &mut self.textures.pictures, events: &mut self.picture_events, errno: self.pow_sets_errno };
        let r = materials::textures(&mut p.st, &mut dict, 1, self.preview, &mut env);
        self.textures = dict;
        if let Err(e) = r {
            self.fail("SI_textures", e);
        }
    }
    fn lights(&mut self, p: &mut Prepared) {
        // si_lights with shadowBuffer behind it (crate::bound); the ray tracing word is
        // mirrored as Prepared::lights mirrors it
        let mut notes = Vec::new();
        let mut pass = crate::bound::ShadowPass {
            scene: &self.scene,
            g: &p.g,
            settings: p.settings,
            options: p.options.clone(),
            camera: p.camera,
            root: p.root,
            buffers: p.buffers,
            empty_bounds: p.empty_bounds,
            depth_cells: &mut p.stats[crate::update::stat::SHADOW_DEPTH],
            notes: &mut notes,
            rng: &mut self.rng,
            tess: &mut Ported,
        };
        let r = crate::lights::lights(&mut p.st, &mut p.options.raytrace, p.settings.level, &mut pass);
        p.g.raytrace = p.options.raytrace & 1 != 0;
        p.st.glb.raytrace = p.g.raytrace;
        self.notes.append(&mut notes);
        match r {
            Ok(messages) => self.notes.extend(messages.into_iter().map(str::to_string)),
            Err(e) => self.notes.push(format!("fatal: {e}")),
        }
    }
    fn update_txt3d_transf(&mut self, p: &mut Prepared) {
        let own = self.own_tex3d.clone();
        p.update_txt3d_transf(&|node| own.get(node).copied().unwrap_or(0));
    }
}
