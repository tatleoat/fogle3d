// SPDX-License-Identifier: MIT
//! Where the renderer's two halves meet (finding 0012, section 16).
//!
//! // adapter: not in place
//!
//! `SI_image`, `SI_initialize` with its dictionary walks, `SI_renExit` and the frame buffer
//! routines are ported in place, on the memory image. Everything below `SI_update` and the
//! two frame loops is ported on this crate's own records ([`crate::update::Prepared`],
//! [`crate::softtriangle::Scene`], [`crate::shade::ShadeState`]). [`Render`] is the
//! [`Outside`] that `SI_image` is run with until those are converted: it answers the
//! callees of this package (`SI_initialize`'s four set-up routines, `SI_update`, the two
//! frame loops, the picture writer, the chunk memory) by building the crate's records
//! from the image and running the ports on them, and hands every other call on.
//!
//! Nothing here reproduces a function of the original. The scene is read from the image:
//! the links between its records where they are followed here, the records' contents by
//! the readers of [`crate::inimage`] (materials, 2D and 3D textures, pictures, lights, a
//! node's mesh, face of splines or patch and what the converters read of the node), the
//! picture list from its head and the dictionary's chapters as the set-up walks them. A
//! node of kind 10 (a NURBS surface) is converted on the image by `si-nurbs`' port of
//! `SI_nurbs2Triangle` ([`nurbs_on_image`], thirteenth pass), and a node of kind 2 whose
//! face is of NURBS curves by `si-model`'s port of `SI_faceConvertToMesh`
//! ([`nurbs_face_on_image`]); `SI_metaSysGetSurface (0x0845b2c4)` is a hook
//! ([`crate::softtriangle::Tessellators`]).
//!
//! After every `SI_update` what the set-up made is stored in the image where the original
//! stores it ([`write_back`]: materials, textures, pictures and their list, 3D textures,
//! the nodes' flag words).
//!
//! What is still not in place (finding 0012, section 18.4 has the exact list): the
//! controls and the camera copy are copied out of the image for `initData` and the three
//! set-up routines after it, whose results (but for the picture's two lines and the four
//! byte lines) stay in [`Prepared`]; `SI_update` and the frame loops run on [`Prepared`],
//! the scene rebuilt from the image at every update; a material's jitter tables, a node's
//! render arrays, a patch's subdivision and what `si_lights` makes of a light are not
//! written back.
//!
//! Since the tenth pass (finding 0012, section 19): with a [`FileHost`] ([`Render::with_host`])
//! the picture writer runs on the image ([`crate::rle_image`]) and a frame ends as a file;
//! `si_fatal`, `SI_txt2dSetWrap` and `longjmp` are answered here by their ports; when
//! `SI_renChunk` is a record of the image the chunk routines `SI_image` calls are
//! [`crate::mem_chunk_image`]'s, `initBuffers`' pieces are taken from the arena in the
//! original's order and the jitter tables of materials and lights are written into it.
//! Without a host the lines are kept only; without an arena the chunk calls are answered
//! as before (a mark counter, blocks of `SI_malloc`). The ray tracer's own chunk books
//! ([`crate::frame::render`]) are still a buffer of the frame's.

use std::collections::HashMap;

use crate::camera::{Camera, Fade};
use crate::frame::{Job, Line, PictureEvent, PictureFiles};
use crate::host::FileHost;
use crate::geometry::RenModel;
use crate::img::{dict, f, g, light, material, model, Outside};
use crate::materials::{DictElem, ModelElem, TextureDict};
use crate::inimage;
use crate::shade::{ShadeModel, ShadeState};
use crate::softtriangle::Scene;
use crate::types::Rgbazo;
use crate::update::{Options, Prepared, Settings};
use si_math::types::Vector;
use si_mem::Mem;

/// The camera record's members set-up reads (`SI_Camera`; the offsets are those of
/// [`Camera`]).
pub fn camera_from_image(m: &Mem, at: u32) -> Camera {
    let v = |o: u32| Vector::new(m.f32(at + o), m.f32(at + o + 4), m.f32(at + o + 8));
    Camera {
        kind: m.i16(at + 0x0a),
        position: v(0x10),
        interest: v(0x1c),
        roll: m.f32(at + 0x30),
        near: m.f32(at + 0x34),
        far: m.f32(at + 0x38),
        fov: m.f32(at + 0x40),
        dof: m.i32(at + 0x6c),
        dof_focal: m.f32(at + 0x70),
        dof_focus: m.f32(at + 0x74),
        dof_near: m.f32(at + 0x7c),
        dof_far: m.f32(at + 0x84),
        dof_beyond: m.f32(at + 0x8c),
        dof_aperture: m.f32(at + 0x94),
        dof_circle: m.f32(at + 0x98),
        window: m.i16(at + 0x108),
        window_v: core::array::from_fn(|i| m.f32(at + 0x10c + 4 * i as u32)),
    }
}

/// The render controls (`SI_ren`) as [`Settings`] and [`Options`]; each member's address
/// is in its own description.
pub fn controls_from_image(m: &Mem) -> (Settings, Options) {
    let s = Settings {
        width: m.i16(g::WIDTH),
        height: m.i16(g::HEIGHT),
        pixel_ratio: m.f32(0x0879_0d34),
        level: m.i16(0x0879_0d38),
        field: m.i16(g::FIELDS),
        field_no: m.i16(g::FIELD_NOW),
        crop: m.i32(g::CROP),
        crop_left: m.f32(0x0879_0e0c),
        crop_right: m.f32(0x0879_0e10),
        crop_top: m.f32(g::CROP_TOP),
        crop_bottom: m.f32(g::CROP_BOTTOM),
        crop_width: m.i16(g::CROP_WIDTH),
        crop_height: m.i16(g::CROP_HEIGHT),
        resume: m.i16(g::RESUMED),
        resume_line: m.i32(g::RESUME_LINE),
    };
    let name = m.u32(g::NAME);
    let o = Options {
        raytrace: m.u16(0x0879_0c7c),
        sequence: m.i16(g::SEQUENCE),
        adaptive: m.i16(g::ADAPTIVE),
        sub: m.i16(0x0879_0d3c),
        contrast: m.f32(0x0879_0d4c),
        blur: m.i16(0x0879_0df0),
        shutter: m.f32(0x0879_0df4),
        blur_step: m.f32(0x0879_0dfc),
        timing: m.i16(g::TIMING),
        tree_stats: m.i16(0x0879_0dc8),
        mem_stats: m.i16(0x0879_0dca),
        first_frame: m.i16(g::FRAME_FIRST),
        frame: m.i16(g::FRAME_NOW),
        textures_2d: m.i16(0x0879_1166),
        name: if name != 0 { m.cstr(name) } else { Vec::new() },
        default_width: m.i16(0x0878_1d9a),
    };
    (s, o)
}

/// [`camera_from_image`] the other way: the members of a camera record set-up reads, stored
/// at `at`.
pub fn camera_to_image(m: &mut Mem, at: u32, c: &Camera) {
    m.set_i16(at + 0x0a, c.kind);
    for (o, v) in [(0x10, c.position), (0x1c, c.interest)] {
        m.set_f32(at + o, v.x);
        m.set_f32(at + o + 4, v.y);
        m.set_f32(at + o + 8, v.z);
    }
    for (o, v) in [(0x30, c.roll), (0x34, c.near), (0x38, c.far), (0x40, c.fov), (0x70, c.dof_focal), (0x74, c.dof_focus), (0x7c, c.dof_near), (0x84, c.dof_far), (0x8c, c.dof_beyond), (0x94, c.dof_aperture), (0x98, c.dof_circle)] {
        m.set_f32(at + o, v);
    }
    m.set_i32(at + 0x6c, c.dof);
    m.set_i16(at + 0x108, c.window);
    for (i, v) in c.window_v.iter().enumerate() {
        m.set_f32(at + 0x10c + 4 * i as u32, *v);
    }
}

/// [`controls_from_image`] the other way, for the members of [`Settings`] and the sampling
/// words of [`Options`] (the output name is not stored).
pub fn controls_to_image(m: &mut Mem, s: &Settings, o: &Options) {
    m.set_i16(g::WIDTH, s.width);
    m.set_i16(g::HEIGHT, s.height);
    m.set_f32(0x0879_0d34, s.pixel_ratio);
    m.set_i16(0x0879_0d38, s.level);
    m.set_i16(g::FIELDS, s.field);
    m.set_i16(g::FIELD_NOW, s.field_no);
    m.set_i32(g::CROP, s.crop);
    m.set_f32(0x0879_0e0c, s.crop_left);
    m.set_f32(0x0879_0e10, s.crop_right);
    m.set_f32(g::CROP_TOP, s.crop_top);
    m.set_f32(g::CROP_BOTTOM, s.crop_bottom);
    m.set_i16(g::CROP_WIDTH, s.crop_width);
    m.set_i16(g::CROP_HEIGHT, s.crop_height);
    m.set_i16(g::RESUMED, s.resume);
    m.set_i32(g::RESUME_LINE, s.resume_line);
    m.set_u16(0x0879_0c7c, o.raytrace);
    m.set_i16(g::SEQUENCE, o.sequence);
    m.set_i16(g::ADAPTIVE, o.adaptive);
    m.set_i16(0x0879_0d3c, o.sub);
    m.set_f32(0x0879_0d4c, o.contrast);
    m.set_i16(g::FRAME_FIRST, o.first_frame);
    m.set_i16(g::FRAME_NOW, o.frame);
    m.set_i16(0x0878_1d9a, o.default_width);
}

/// The switches and colours of `SI_ren` the shading model reads, into the prepared state.
pub fn shading_from_image(m: &Mem, p: &mut Prepared) {
    let f6: [f32; 6] = core::array::from_fn(|i| m.f32(0x0879_0cf8 + 4 * i as u32));
    let s = &mut p.st.glb;
    s.fade = m.i16(0x0879_0ce0);
    s.background = Rgbazo { r: f6[0], g: f6[1], b: f6[2], a: f6[3], z: f6[4], o: f6[5] };
    s.max_depth = m.i16(0x0879_0dc4);
    s.fog = m.i16(0x0879_0e3c);
    s.fog_density = m.f32(0x0879_0e40);
    s.fog_base = m.f32(0x0879_0e44);
    s.fog_thickness = m.f32(0x0879_0e48);
    s.fog_start = m.f32(0x0879_0e4c);
    s.fog_end = m.f32(0x0879_0e50);
    s.fog_color = core::array::from_fn(|i| m.f32(0x0879_0e64 + 4 * i as u32));
    s.textures = m.i16(0x0879_1164);
    s.textures_2d = m.i16(0x0879_1166);
    s.textures_3d = m.i16(0x0879_1168);
    s.reflection = m.i16(0x0879_116a);
    s.refraction = m.i16(0x0879_116c);
    s.shadows = m.i16(0x0879_116e);
    s.no_spec_alpha = m.i16(g::SI_REN_NO_SPEC_ALPHA);
    s.ambience = core::array::from_fn(|i| m.f32(0x0879_0ddc + 4 * i as u32));
    s.w_dec = m.i16(0x0879_0dec);
    s.frame = m.i16(g::FRAME_NOW);
    p.fade = Fade { start: m.f32(0x0879_0ce4), end: m.f32(0x0879_0cec), amount: m.f32(0x0879_0d10) };
    p.bsp_params.min_count = m.i16(0x0879_0dba);
    p.bsp_params.max_level = m.i16(0x0879_0dbc);
    p.g.right_hand = m.i32(0x0867_93ec) != 0;
    p.g.glb_manip = m.i32(0x0878_1560);
}

/// A picture as the frame loop handed it to the picture writer: the name `buildName` made
/// and the lines in the order they were written (top line first; `y` counts from the
/// bottom). With a [`FileHost`] the same lines also go through the picture writer on the
/// image (`si_rleInit`, `SI_rleWrite`, `SI_rleClose`: [`crate::rle_image`]) and end as the
/// file of that name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picture {
    pub name: Vec<u8>,
    pub lines: Vec<Line>,
    pub closed: bool,
    /// Lines handed to the picture writer, kept or not.
    pub written: usize,
}

/// `SI_image`'s surroundings: see the module's description.
pub struct Render<'a> {
    /// Every callee that is not this package's: the dictionary, the instance code, the GL
    /// layer, the C library, the tag and script routines.
    pub rest: &'a mut dyn Outside,
    pub files: &'a mut dyn PictureFiles,
    /// The scene as the ports see it, rebuilt from the image at every `SI_update`.
    pub p: Prepared,
    /// What set-up said and could not do ([`Job::notes`]).
    pub notes: Vec<String>,
    /// The pictures rendered.
    pub pictures: Vec<Picture>,
    /// The machine's files: with one, a picture is written through it.
    pub host: Option<&'a mut dyn FileHost>,
    /// Keep every line of every picture in [`Render::pictures`] (the default). A caller
    /// that renders a long sequence to files clears this: a picture then keeps its name,
    /// its count of lines in [`Picture::written`] and no line.
    pub keep_lines: bool,
    /// Every line as it is handed to the picture writer, with its picture's name: for a
    /// caller that takes the lines as they come instead of having them kept.
    pub on_line: Option<Box<dyn FnMut(&[u8], &Line) + 'a>>,
    in_writer: bool,
    pending_icon: Option<Vec<u8>>,
    jumped: bool,
    background_bytes: [u8; 4],
    rng: crate::jitter::Drand48,
    marks: u32,
    /// What `SI_fsDitherInit` set up for the frame (si-imgfmt's record of the diffusion
    /// rows and the noise; the rows' blocks themselves are taken in the image).
    dither: Option<si_imgfmt::gradation::OutputDither>,
}

thread_local! {
    static PICTURES_UNDER: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
    static KEEP_LINES: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// For a program that renders long sequences through a world it does not build itself:
/// whether every [`Render`] made on this thread keeps the lines of its pictures
/// ([`Render::keep_lines`]; the default is yes). With pictures written to files a sequence
/// of hundreds of frames would otherwise hold every line of every frame. Not a function of
/// the original.
pub fn keep_picture_lines(keep: bool) {
    KEEP_LINES.with(|k| k.set(keep));
}

/// For a program that renders through a world it does not build itself (the render
/// command's, which makes its [`Render`] with [`Render::new`]): while a directory is set on
/// this thread, every [`Render`] made here writes its pictures to the machine's files below
/// it ([`crate::host::DiskFiles::under`]: the path the frame's name gives, taken below the
/// directory). `None` stops it.
pub fn write_pictures_under(root: Option<std::path::PathBuf>) {
    PICTURES_UNDER.with(|p| *p.borrow_mut() = root);
}

impl<'a> Render<'a> {
    pub fn new(rest: &'a mut dyn Outside, files: &'a mut dyn PictureFiles) -> Render<'a> {
        // (the host lives as long as the program: one small record a render)
        let host = PICTURES_UNDER.with(|p| p.borrow().clone()).map(|root| Box::leak(Box::new(crate::host::DiskFiles::under(root))) as &'a mut dyn FileHost);
        Render {
            rest,
            files,
            p: Prepared::default(),
            notes: Vec::new(),
            pictures: Vec::new(),
            host,
            keep_lines: KEEP_LINES.with(|k| k.get()),
            on_line: None,
            in_writer: false,
            pending_icon: None,
            jumped: false,
            background_bytes: [0; 4],
            rng: crate::jitter::Drand48::seeded(0),
            marks: 0,
            dither: None,
        }
    }

    /// The files a picture is written through. The image must hold the binary's data
    /// segment as far as the renderer reads it ([`crate::data::init`]: texts, and globals
    /// such as `SI_rightHand`, without which every winding and normal is turned round);
    /// nothing here writes it.
    pub fn with_host(mut self, host: &'a mut dyn FileHost) -> Render<'a> {
        self.host = Some(host);
        self
    }

    /// A fatal error of a routine that runs on the crate's own records: the original's
    /// `si_fatal(text, more, 2)`, which puts the text in `SI_renMessage`, shows it and
    /// leaves through `longjmp` ([`crate::renutil_image::fatal`]; the text is handed over
    /// whole, with an empty second part).
    fn fatal(&mut self, m: &mut Mem, text: &str) {
        self.notes.push(format!("fatal: {text}"));
        let bytes: Vec<u8> = text.bytes().take(0x3ff).collect();
        let sp = m.sp();
        let at = m.local(bytes.len() as u32 + 1);
        m.put_cstr(at, &bytes);
        if m.u8(crate::data::text::FMT_LINE) == 0 {
            crate::data::place_texts(m);
        }
        crate::renutil_image::fatal(m, self, at, at + bytes.len() as u32, 2);
        m.set_sp(sp);
        self.jumped = true;
    }

    /// `SI_addIconToPic(path)` (0x085c4808, `geticon.c`; ported in `si-picture` on a
    /// file's bytes): the picture is read through the host, given its icon and written
    /// back. A picture that cannot be read or written gets the original's status text, in
    /// the notes.
    fn add_icon(&mut self, path: &[u8]) {
        let Some(host) = self.host.as_mut() else { return };
        let shown = String::from_utf8_lossy(path).into_owned();
        match host.read_file(path) {
            Some(mut data) => match si_picture::add_icon_to_pic(&mut data, &shown) {
                Ok(()) => {
                    if !host.write_file(path, &data) {
                        self.notes.push(format!("Cannot write icon to : {shown}"));
                    }
                }
                Err(text) => self.notes.push(text),
            },
            None => self.notes.push(format!("Cannot read file : {shown}")),
        }
    }

    /// The renderer's arena, when it is a record of the image.
    fn arena(m: &Mem) -> Option<u32> {
        let chunk = m.u32(g::SI_REN_CHUNK);
        (chunk != 0 && m.is_mapped(chunk, 0x40)).then_some(chunk)
    }

    /// `initData`, `si_initViewport`, `initBuffers`, `initFilter` on the controls and the
    /// camera copy in the image; then, in the image, what `SI_image` itself reads of their
    /// results: the picture's first and last line (0x08790788, 0x0879078c) and the four
    /// outgoing byte lines (0x08790b54 …), which here are blocks of `SI_malloc` (the
    /// original takes them from chunk memory).
    fn begin(&mut self, m: &mut Mem) {
        let mut p = Prepared::default();
        (p.settings, p.options) = controls_from_image(m);
        p.camera = camera_from_image(m, g::SI_DATABASE);
        shading_from_image(m, &mut p);
        let mut job = Job::new(&mut *self.files);
        job.begin(&mut p);
        self.notes.append(&mut job.notes);
        m.set_f32(g::PICTURE_Y_MIN, p.view.picture[2]);
        m.set_f32(g::PICTURE_Y_MAX, p.view.picture[3]);
        // initData on the image (0x082765e8, listing): a scene without a light (the chain
        // at 0x087907e4 empty) gets a copy of `SI_defaultLight` (0x08781e00, 0x240 bytes)
        // in chunk memory at the head of the chain, taken before initBuffers' pieces. The
        // crate's `init_data` above does the same on its own records, which `build_scene`
        // replaces with the image's chain at every update: without this the frame had no
        // light at all (finding 0012, section 21). The original writes through the piece
        // without a test; a null piece is left unwritten here.
        if m.u32(g::DB_LIGHTS) == 0 {
            let l = match Self::arena(m) {
                Some(chunk) => crate::mem_chunk_image::chunk_malloc(m, chunk, 0x240),
                None => m.si_malloc(0x240),
            };
            if l != 0 {
                m.copy(l, SI_DEFAULT_LIGHT, 0x240);
            }
            m.set_u32(g::DB_LIGHTS, l);
        }
        match Self::arena(m) {
            // with the arena in the image: every piece `initBuffers` takes from it, in its
            // order and of its sizes (see [`crate::update::BufferSizes`]): the scan-line
            // and the column table (cleared), the line of samples, the four byte lines,
            // the depth line and the four outgoing byte lines. The two tables and the
            // outgoing lines are stored where the original keeps their addresses; the
            // working lines of the frame loops stay in the crate's own records.
            Some(chunk) => {
                use crate::mem_chunk_image::chunk_malloc;
                let (rows, columns, w) = (p.buffers.scan_lines as i32, p.buffers.columns as i32, p.buffers.line as i32);
                let scan = chunk_malloc(m, chunk, rows.wrapping_mul(4));
                let cols = chunk_malloc(m, chunk, columns.wrapping_mul(4));
                if scan != 0 && rows > 0 {
                    m.set_fill_bytes(scan, 0, rows as u32 * 4);
                }
                if cols != 0 && columns > 0 {
                    m.set_fill_bytes(cols, 0, columns as u32 * 4);
                }
                m.set_u32(0x0879_08ec, scan);
                m.set_u32(0x0879_08f0, cols);
                chunk_malloc(m, chunk, columns.wrapping_mul(48));
                for _ in 0..4 {
                    chunk_malloc(m, chunk, w);
                }
                chunk_malloc(m, chunk, w.wrapping_mul(4));
                for line in [g::OUT_RED, g::OUT_GREEN, g::OUT_BLUE, g::OUT_ALPHA] {
                    let piece = chunk_malloc(m, chunk, w);
                    m.set_u32(line, piece);
                }
            }
            // without it: one block of `SI_malloc` for the four outgoing lines, laid out as
            // the arena lays them (padSize(w) apart), with room behind the last for what a
            // crop wider than the lines writes there (`renutil::output`; in the original
            // that lands in the next piece of chunk memory)
            None => {
                let w = p.buffers.line.max(1) as i32;
                let pad = crate::mem_chunk::pad_size(w) as u32;
                let crop = if p.settings.crop != 0 { (p.settings.crop_width as i32).max(0) as u32 } else { 0 };
                let block = m.si_malloc(3 * pad + pad.max(crop));
                for (k, line) in [g::OUT_RED, g::OUT_GREEN, g::OUT_BLUE, g::OUT_ALPHA].into_iter().enumerate() {
                    m.set_u32(line, block + k as u32 * pad);
                }
            }
        }
        self.p = p;
    }

}

/// The records met so far, by their address in the image.
#[derive(Default)]
struct Seen {
    mats: HashMap<u32, usize>,
    maps: HashMap<u32, usize>,
    tex3ds: HashMap<u32, u32>,
    pictures: HashMap<u32, usize>,
}

/// Where the scene's records lie in the image, by the index this crate's records have: what
/// [`write_back`] stores the set-up's results to.
#[derive(Clone, Debug, Default)]
pub struct Placed {
    pub nodes: Vec<u32>,
    pub materials: Vec<u32>,
    pub maps: Vec<u32>,
    pub tex3ds: Vec<u32>,
    /// A picture record `SI_global2dMapInsert` makes during the update has no address
    /// yet: 0.
    pub pictures: Vec<u32>,
    /// The current dictionary (`SI_elemDicts` + 0x70 × the first word of `SI_procData`).
    pub dictionary: u32,
    /// The word that heads the picture list (`SI_grafLists` + 0x1c + 0x24 × the second
    /// word of `SI_procData`), 0 when there are no graf lists.
    pub picture_head: u32,
}

/// `SI_defaultMaterial` (0x0869dde0): the record a root without materials renders with.
pub const SI_DEFAULT_MATERIAL: u32 = 0x0869_dde0;
/// `SI_defaultLight` (0x08781e00, 0x240 bytes, `.bss`): the light of a scene without one.
/// `SI_matterInit (0x08291bec)` makes it at start-up: `si_lightInitialize(light, 1)` (a
/// point light) and its position (+0x34 … +0x3c) (100, 100, 100).
pub const SI_DEFAULT_LIGHT: u32 = 0x0878_1e00;

/// The NURBS surfaces of the scene's nodes, converted on the image where `SI_collect` would
/// convert them (`RENDER_NO_NURBS=1` in the environment leaves them to the hook, for
/// measuring), and the faces of NURBS curves likewise (`RENDER_NO_NURBS_FACES=1` leaves them
/// to the hook). `nodes` are the nodes' addresses, one for each of the scene's nodes.
fn convert_surfaces(m: &mut Mem, nodes: &[u32], scene: &mut Scene, notes: &mut Vec<String>) {
    static NO_NURBS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let no_nurbs = *NO_NURBS.get_or_init(|| std::env::var_os("RENDER_NO_NURBS").is_some_and(|v| v == "1"));
    static NO_NURBS_FACES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let no_nurbs_faces = *NO_NURBS_FACES.get_or_init(|| std::env::var_os("RENDER_NO_NURBS_FACES").is_some_and(|v| v == "1"));
    let mut side = FaceSide::default();
    for (k, src) in scene.nodes.iter_mut().enumerate() {
        let collected = src.algo_type != 13 && src.garbage_matte != 1 && src.visible != 0 && src.box_only == 0;
        let Some(&node) = nodes.get(k) else { continue };
        if !no_nurbs && collected && src.def == crate::softtriangle::NodeDef::Nurbs {
            match nurbs_on_image(m, node) {
                Ok(def) => src.def = def,
                Err(e) => notes.push(format!("adapter: SI_nurbs2Triangle on the node at {node:#x}: {e}")),
            }
        }
        let face = m.u32(node + 0x48);
        let nurbs_face = m.i32(node + 0x30) == 2 && face != 0 && m.i16(face + 0x84) == 0;
        if !no_nurbs_faces && collected && nurbs_face && src.def == crate::softtriangle::NodeDef::Face {
            match nurbs_face_on_image(m, node, &mut side) {
                Ok(def) => src.def = def,
                Err(e) => notes.push(format!("adapter: SI_faceConvertToMesh on the node at {node:#x}: {e}")),
            }
        }
    }
    for (name, n) in &side.zero {
        notes.push(format!("adapter: SI_faceConvertToMesh's call of {name} answered 0, {n} times"));
    }
}

/// The job with the image behind it, for motion blur (`si_blurGeometry`, [`crate::blur`]):
/// everything `SI_update` asks of the job is the job's; what the blur walk asks of the scene
/// at other times is answered on the image.
struct BlurJob<'j, 'a, 'x> {
    job: &'j mut Job<'a>,
    m: &'x mut Mem,
    rest: &'x mut dyn Outside,
    /// The address of each node of the scene, in the scene's order ([`Placed::nodes`]).
    nodes: &'x [u32],
}

impl crate::geometry::GeometryEnv for BlurJob<'_, '_, '_> {
    fn check_abort(&mut self) {
        crate::geometry::GeometryEnv::check_abort(&mut *self.job);
    }
    fn collect(&mut self, st: &mut ShadeState, g: &mut crate::geometry::Geometry, node: usize) {
        crate::geometry::GeometryEnv::collect(&mut *self.job, st, g, node);
    }
}

impl crate::blur::BlurWorld for BlurJob<'_, '_, '_> {
    fn frame_rate(&mut self) -> f32 {
        self.m.f32(g::SI_FRAME_RATE)
    }
    /// `animate (0x08275bb0)`: `SI_frameCurrent` = the render's frame for
    /// `SI_sceneUpdateItems(time, 1, &SI_database, 1)`, put back; then (the part of
    /// `si_blurGeometry` after its first `animate`) `si_traverseExpandedInstance(root)`. The
    /// nodes are read from the image again: their sources, and the matrices, the winding bit
    /// and the flag word of the models. What stays as `build_scene` made it: the materials,
    /// textures, lights and the camera (the original's renderer reads those records at the
    /// time they are used, so a material or a light animated in the shutter would differ).
    fn animate(&mut self, geo: &mut crate::geometry::Geometry, time: f32, expanded: bool) {
        let m = &mut *self.m;
        let saved = m.i32(g::SI_FRAME_CURRENT);
        m.set_i32(g::SI_FRAME_CURRENT, m.i16(g::FRAME_NOW) as i32);
        self.rest.call(m, f::SI_SCENE_UPDATE_ITEMS, &[time.to_bits(), 1, g::SI_DATABASE, 1]);
        m.set_i32(g::SI_FRAME_CURRENT, saved);
        if expanded && !std::env::var_os("RENDER_NO_EXPANDED_INSTANCES").is_some_and(|v| v == "1") {
            let root = m.u32(g::DB_ROOT);
            self.rest.call(m, f::SI_TRAVERSE_EXPANDED_INSTANCE, &[root]);
        }
        for (k, &a) in self.nodes.iter().enumerate() {
            let maps = self.job.scene.nodes[k].maps;
            let mut src = inimage::node_source_at(m, a);
            src.maps = maps;
            self.job.scene.nodes[k] = src;
            let r = &mut geo.models[k];
            let v = |o: u32| Vector::new(m.f32(a + o), m.f32(a + o + 4), m.f32(a + o + 8));
            r.reversed = m.u16(a + 0x08) & 8 != 0;
            r.gscl = v(0x16c);
            r.cscl = v(0x3a0);
            r.gmat = core::array::from_fn(|i| core::array::from_fn(|j| m.f32(a + 0x1c4 + 16 * i as u32 + 4 * j as u32)));
            r.flags = m.u16(a + model::TXT_FLAGS);
        }
        convert_surfaces(m, self.nodes, &mut self.job.scene, &mut self.job.notes);
    }
    /// `SI_modelMove (0x08229a90)` of the node: the answer of the render command's world.
    fn model_move(&mut self, node: usize) -> u32 {
        self.rest.call(self.m, 0x0822_9a90, &[self.nodes[node]])
    }
    /// `SI_modelConstraintMove (0x08229aec)`.
    fn model_constraint_move(&mut self, node: usize) -> u32 {
        self.rest.call(self.m, 0x0822_9aec, &[self.nodes[node]])
    }
    fn pair_count(&self, node: usize) -> usize {
        self.job.scene.nodes[node].render.tcoords.len()
    }
}

impl crate::update::UpdateEnv for BlurJob<'_, '_, '_> {
    fn status_message(&mut self, text: &str) {
        self.job.status_message(text);
    }
    fn init_fade(&mut self, p: &mut Prepared) {
        self.job.init_fade(p);
    }
    fn init_dof(&mut self, p: &mut Prepared) {
        self.job.init_dof(p);
    }
    fn materials(&mut self, p: &mut Prepared) {
        self.job.materials(p);
    }
    fn maps_list(&mut self, p: &mut Prepared) {
        self.job.maps_list(p);
    }
    fn textures(&mut self, p: &mut Prepared) {
        self.job.textures(p);
    }
    fn lights(&mut self, p: &mut Prepared) {
        self.job.lights(p);
    }
    fn update_txt3d_transf(&mut self, p: &mut Prepared) {
        self.job.update_txt3d_transf(p);
    }
    fn blur_geometry(&mut self, p: &mut Prepared, time: f32) {
        let (root, shutter, threshold) = (p.root, p.shutter_copy, p.blur_threshold);
        crate::blur::blur_geometry(&mut p.st, &mut p.g, self, root, shutter, threshold, time);
    }
}


impl Seen {
    /// One 2D texture (`__SI_LclMap`): its index. Its picture (+0x24, a `glb_map`), when
    /// the record has one already, is read with its pixel block.
    fn map(&mut self, st: &mut ShadeState, m: &Mem, addr: u32) -> (usize, bool) {
        if let Some(i) = self.maps.get(&addr) {
            return (*i, true);
        }
        let mut r = inimage::map_at(m, addr);
        let glb = m.u32(addr + 0x24);
        if glb != 0 {
            let i = *self.pictures.entry(glb).or_insert_with(|| {
                st.pictures.push(inimage::glb_at(m, glb, None));
                st.pictures.len() - 1
            });
            r.glb = Some(i);
        }
        st.maps.push(r);
        self.maps.insert(addr, st.maps.len() - 1);
        (st.maps.len() - 1, false)
    }

    /// A list of 2D textures (linked through +0x16c): its head's index.
    fn map_list(&mut self, st: &mut ShadeState, m: &Mem, mut addr: u32) -> Option<usize> {
        let mut head = None;
        let mut last: Option<usize> = None;
        while addr != 0 {
            let (i, known) = self.map(st, m, addr);
            if let Some(l) = last {
                st.maps[l].next = Some(i);
            }
            head = head.or(Some(i));
            last = Some(i);
            if known {
                break;
            }
            st.maps[i].next = None;
            addr = m.u32(addr + crate::img::lclmap::NEXT);
        }
        head
    }

    /// A 3D texture: its index + 1, 0 for none.
    fn tex3d(&mut self, st: &mut ShadeState, m: &Mem, addr: u32) -> u32 {
        if addr == 0 {
            return 0;
        }
        if let Some(i) = self.tex3ds.get(&addr) {
            return *i;
        }
        st.tex3ds.push(inimage::tex3d_at(m, addr));
        self.tex3ds.insert(addr, st.tex3ds.len() as u32);
        st.tex3ds.len() as u32
    }

    /// A material with its texture list (+0x6e8, +0x6ec), its reflection map (+0x6f4) and
    /// its 3D texture (+0x6fc).
    fn material(&mut self, st: &mut ShadeState, m: &Mem, addr: u32) -> usize {
        if let Some(i) = self.mats.get(&addr) {
            return *i;
        }
        let mut r = inimage::material_at(m, addr);
        r.maps = self.map_list(st, m, m.u32(addr + material::TEXTURES));
        let reflect = m.u32(addr + 0x6f4);
        r.reflect_map = (reflect != 0).then(|| self.map(st, m, reflect).0);
        r.tex3d = self.tex3d(st, m, m.u32(addr + material::TEXTURE3D));
        st.materials.push(r);
        self.mats.insert(addr, st.materials.len() - 1);
        if addr == SI_DEFAULT_MATERIAL {
            st.default_material = Some(st.materials.len() - 1);
        }
        st.materials.len() - 1
    }
}

/// The scene of the image as this crate's records: see [`Render::update`].
pub fn build_scene<'f>(p: &mut Prepared, files: &'f mut dyn PictureFiles, m: &Mem) -> (Job<'f>, Placed) {
    p.st.pictures.clear();
    p.st.picture_head = None;
    p.st.materials.clear();
    p.st.maps.clear();
    p.st.models.clear();
    p.st.lights.clear();
    p.st.tex3ds.clear();
    p.st.default_material = None;
    p.st.tns.clear();
    p.st.verts.clear();
    p.st.uvs.clear();
    p.g.models.clear();
    let mut seen = Seen::default();
    // the picture list, from its head (SI_getMapsList), through +0x24
    let graf = m.u32(g::SI_GRAF_LISTS);
    let picture_head = if graf != 0 { graf.wrapping_add(0x1c).wrapping_add((m.i32(m.u32(g::SI_PROC_DATA) + 4) as u32).wrapping_mul(0x24)) } else { 0 };
    let mut a = if picture_head != 0 { m.u32(picture_head) } else { 0 };
    while a != 0 && !seen.pictures.contains_key(&a) {
        seen.pictures.insert(a, p.st.pictures.len());
        p.st.pictures.push(inimage::glb_at(m, a, None));
        a = m.u32(a + 0x24);
        let n = p.st.pictures.len();
        p.st.pictures[n - 1].next = (a != 0).then_some(n);
    }
    p.st.picture_head = (!p.st.pictures.is_empty()).then_some(0);
    // the counter of texture pixels (SI_stats +0x34)
    p.st.texture_pixels = m.i32(g::SI_STATS + 0x34);
    // the nodes: the root chain first, then whatever a `latdep` or an element names
    let mut index: HashMap<u32, usize> = HashMap::new();
    let mut order: Vec<u32> = Vec::new();
    let mut number = |addr: u32, order: &mut Vec<u32>| -> usize {
        *index.entry(addr).or_insert_with(|| {
            order.push(addr);
            order.len() - 1
        })
    };
    let root = m.u32(g::DB_ROOT);
    p.root = (root != 0).then(|| number(root, &mut order));
    let dictionary = m.u32(g::SI_ELEM_DICTS).wrapping_add((m.i32(m.u32(g::SI_PROC_DATA)) as u32).wrapping_mul(dict::DICT_SIZE));
    let chapter = |no: u32| m.u32(dictionary + dict::DICT_LISTS).wrapping_add(m.u32(no).wrapping_mul(dict::LIST_SIZE));
    let elem_of = |ch: u32, i: u32| m.u32(ch + dict::LIST_ELEMS).wrapping_add(i.wrapping_mul(dict::ELEM_SIZE));
    let models = chapter(g::CID_MODELS);
    let n_models = m.i32(models + dict::LIST_COUNT).max(0) as u32;
    let mut scene = Scene::default();
    let mut own_tex3d = Vec::new();
    let mut done = 0;
    while done < order.len() {
        let a = order[done];
        done += 1;
        let link = |addr: u32, order: &mut Vec<u32>, number: &mut dyn FnMut(u32, &mut Vec<u32>) -> usize| (addr != 0).then(|| number(addr, order));
        let latdep = m.u32(a + 0x44);
        let elem = m.i32(a + model::ELEM_ID);
        let registered = if elem >= 0 && (elem as u32) < n_models { m.u32(elem_of(models, elem as u32) + dict::ELEM_DATA) } else { 0 };
        let n_mat = m.i16(a + model::NB_MATERIALS);
        let array = m.u32(a + model::MATERIALS);
        let materials: Vec<usize> = if array == 0 { Vec::new() } else { (0..n_mat.max(0) as u32).map(|i| seen.material(&mut p.st, m, m.u32(array + 4 * i))).collect() };
        let own_maps = seen.map_list(&mut p.st, m, m.u32(a + model::TEXTURES));
        own_tex3d.push(seen.tex3d(&mut p.st, m, m.u32(a + model::TEXTURE3D)));
        let v = |o: u32| Vector::new(m.f32(a + o), m.f32(a + o + 4), m.f32(a + o + 8));
        let ren = RenModel {
            reversed: m.u16(a + 0x08) & 8 != 0,
            kind: m.i32(a + model::KIND),
            latdep: if latdep != 0 { number(latdep, &mut order) } else { done - 1 },
            gscl: v(0x16c),
            cscl: v(0x3a0),
            gmat: core::array::from_fn(|i| core::array::from_fn(|j| m.f32(a + 0x1c4 + 16 * i as u32 + 4 * j as u32))),
            materials: materials.clone(),
            flags: m.u16(a + model::TXT_FLAGS),
            n_maps: m.i16(a + model::NB_TEXTURES),
            brother: link(m.u32(a + model::BROTHER), &mut order, &mut number),
            son: link(m.u32(a + model::SON), &mut order, &mut number),
            registered: if registered != 0 { number(registered, &mut order) } else { done - 1 },
            ..RenModel::default()
        };
        let mut src = inimage::node_source_at(m, a);
        src.maps = own_maps;
        p.g.models.push(ren);
        scene.nodes.push(src);
    }
    // the MODELS chapter: one record per element, its node's materials and textures
    let mut job_models = Vec::new();
    let mut texture_models = Vec::new();
    let mut node_of = Vec::new();
    for i in 0..n_models {
        let e = elem_of(models, i);
        let node = m.u32(e + dict::ELEM_DATA);
        let k = index.get(&node).copied();
        let unused = m.i16(e + dict::ELEM_UNUSED);
        let sm = match k {
            Some(k) => ShadeModel {
                flags: p.g.models[k].flags,
                maps: scene.nodes[k].maps,
                tex3d: own_tex3d[k],
                materials: p.g.models[k].materials.iter().map(|x| Some(*x)).collect(),
                unused,
            },
            None => ShadeModel { unused: if unused != 0 { unused } else { 1 }, ..ShadeModel::default() },
        };
        p.st.models.push(sm);
        job_models.push(DictElem { data: k.map(|_| i as usize), unused: if k.is_some() { unused } else { 1 }, dirty: 0 });
        texture_models.push(ModelElem { elem: DictElem { data: k.map(|_| i as usize), unused, dirty: 0 }, w24: m.u32(e + 0x24) });
        node_of.push(k);
    }
    let mat_chapter = chapter(g::CID_MATERIALS);
    let mut job_materials = Vec::new();
    for i in 0..m.i32(mat_chapter + dict::LIST_COUNT).max(0) as u32 {
        let e = elem_of(mat_chapter, i);
        let (addr, unused) = (m.u32(e + dict::ELEM_DATA), m.i16(e + dict::ELEM_UNUSED));
        let data = (addr != 0 && unused == 0).then(|| seen.material(&mut p.st, m, addr));
        job_materials.push(DictElem { data, unused, dirty: 0 });
    }
    // A measuring switch, off unless `RENDER_NODE_MATERIALS=1` is in the environment
    // (finding 0012, section 23): the materials of a node that are no element of the
    // MATERIALS chapter are handed to `SI_materials` after the chapter's, as if the loader
    // had made them elements. The original's loader does (`SI_dbCheckConvertModelsMatTxt`,
    // a load step si-scene does not make yet); the renderer prepares the chapter's records
    // only, so without it such a material keeps the values its file gave (no ambient term
    // from the scene's ambience among them). Not the renderer's own doing: it is a stand-in
    // for the loader, for measuring what the load step would change.
    static NODE_MATERIALS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *NODE_MATERIALS.get_or_init(|| std::env::var_os("RENDER_NODE_MATERIALS").is_some_and(|v| v == "1")) {
        let listed: std::collections::HashSet<usize> = job_materials.iter().filter_map(|e| e.data).collect();
        let mut extra: Vec<usize> = p.g.models.iter().flat_map(|r| r.materials.iter().copied()).filter(|i| !listed.contains(i)).collect();
        extra.sort();
        extra.dedup();
        for i in extra {
            job_materials.push(DictElem { data: Some(i), unused: 0, dirty: 0 });
        }
    }
    // SI_defaultMaterial is brought up to date at every update, whoever uses it
    if m.is_mapped(SI_DEFAULT_MATERIAL, 0xdcc) {
        seen.material(&mut p.st, m, SI_DEFAULT_MATERIAL);
    }
    // the lights
    let mut l = m.u32(g::DB_LIGHTS);
    p.st.light_head = (l != 0).then_some(0);
    while l != 0 {
        let mut r = inimage::light_at(m, l, n_models);
        l = m.u32(l + 0x140);
        r.next = (l != 0).then_some(p.st.lights.len() + 1);
        p.st.lights.push(r);
    }
    let _ = light::SELECTIVE;
    // the 2D textures chapter and the PICTURES chapter, as the texture set-up walks them
    let map_chapter = chapter(g::CID_TEXTURES_2D);
    let mut job_maps = Vec::new();
    for i in 0..m.i32(map_chapter + dict::LIST_COUNT).max(0) as u32 {
        let e = elem_of(map_chapter, i);
        let addr = m.u32(e + dict::ELEM_DATA);
        let data = if addr == 0 { None } else { seen.maps.get(&addr).copied().or_else(|| seen.map_list(&mut p.st, m, addr)) };
        job_maps.push(DictElem { data, unused: m.i16(e + dict::ELEM_UNUSED), dirty: m.u8(e + dict::ELEM_WORK) });
    }
    let pic_chapter = chapter(g::CID_PICTURES);
    let mut job_pictures = Vec::new();
    for i in 0..m.i32(pic_chapter + dict::LIST_COUNT).max(0) as u32 {
        let e = elem_of(pic_chapter, i);
        let addr = m.u32(e + dict::ELEM_DATA);
        let unused = m.i16(e + dict::ELEM_UNUSED);
        let data = (addr != 0 && unused == 0).then(|| {
            *seen.pictures.entry(addr).or_insert_with(|| {
                p.st.pictures.push(inimage::glb_at(m, addr, None));
                p.st.pictures.len() - 1
            })
        });
        job_pictures.push(DictElem { data, unused, dirty: m.u8(e + dict::ELEM_WORK) });
    }
    // the 3D textures share one table of random numbers, which SI_updTxt3dTurb makes when
    // the first of them is read from its file: it holds the same numbers in every session
    if !p.st.tex3ds.is_empty() {
        crate::tt_noise::init_noise(&mut p.st.noise);
    }
    let mut placed = Placed { nodes: order.clone(), dictionary, picture_head, ..Placed::default() };
    let by_index = |map: &HashMap<u32, usize>, n: usize| {
        let mut v = vec![0u32; n];
        for (addr, i) in map {
            v[*i] = *addr;
        }
        v
    };
    placed.materials = by_index(&seen.mats, p.st.materials.len());
    placed.maps = by_index(&seen.maps, p.st.maps.len());
    placed.pictures = by_index(&seen.pictures, p.st.pictures.len());
    placed.tex3ds = vec![0; p.st.tex3ds.len()];
    for (addr, i) in &seen.tex3ds {
        placed.tex3ds[*i as usize - 1] = *addr;
    }
    let mut job = Job::new(files);
    job.scene = scene;
    job.materials = job_materials;
    job.models = job_models;
    job.node_of = node_of;
    job.own_tex3d = own_tex3d;
    job.textures = TextureDict { pictures: job_pictures, maps: job_maps, models: texture_models };
    job.chapters_given = true;
    job.preview = crate::materials::Preview { selection_call: m.i16(0x0869_bc8a), optimize: m.i16(0x0879_1640) };
    (job, placed)
}

/// `SI_makeVertices`' call of `SI_nurbs2Triangle (0x084e16f8)` for a node of kind 10
/// (`si_nurbs::convert::nurbs2triangle`, the port on the image): the node's definition that
/// lends positions (the first of the array at +0x38 when +0x34 is 1, else the second), its
/// current surface (+0x48), its material array (+0x20c), its element number (+0x354) and
/// `flag` −1, as `SI_collect` calls it. The original writes the four results into the
/// node (+0x228 … +0x234); the node's render arrays are the adapter's records
/// ([`crate::softvertex::NodeRender`]), so they go to locals of the image instead, the
/// vertex and triangle blocks are read into a [`crate::softtriangle::NodeDef::NurbsOf`]
/// and given back to the heap. A corner that is not a vertex of the block is refused.
pub fn nurbs_on_image(m: &mut Mem, node: u32) -> Result<crate::softtriangle::NodeDef, String> {
    use crate::types::{RVertex, Vector};
    let nb = m.i16(node + 0x34);
    let defs = m.u32(node + 0x38);
    let shape = m.u32(defs.wrapping_add(if nb == 1 { 0 } else { 4 }));
    let faulted = m.fault().is_some();
    let out = m.local(16);
    for k in 0..4 {
        m.set_u32(out + 4 * k, 0);
    }
    // unit route3: the full form, whose trimmed branch calls out for the surface evaluators; those are answered by
    // si-nurbs' routed ports, and a call no port answers refuses the node (it stays with the hook, as the earlier
    // form's `Halt::Unported` left it)
    let mut unrouted = TrimSide::default();
    let r = si_nurbs::convert2::nurbs2triangle_full(
        m, &mut si_nurbs::routed::Routed::new(&mut unrouted), shape, m.u32(node + 0x48), m.u32(node + 0x20c), m.i32(node + 0x354), out, out + 4, out + 8, out + 12, -1,
    );
    let r = match (r, unrouted.names.first()) {
        (Ok(()), Some(n)) => Err(si_nurbs::image::Halt::Refused(*n)),
        (r, _) => r,
    };
    let (nv, verts, nt, tris) = (m.i32(out), m.u32(out + 4), m.i32(out + 8), m.u32(out + 12));
    m.release(16);
    r.map_err(|h| format!("{h:?}"))?;
    let f3 = |m: &Mem, a: u32| Vector::new(m.f32(a), m.f32(a + 4), m.f32(a + 8));
    let ptr = |p: u32| if p == 0 { crate::softvertex::NO_UV } else { p };
    let mut out_verts = Vec::with_capacity(nv.max(0) as usize);
    for k in 0..nv.max(0) as u32 {
        let a = verts.wrapping_add(0x4c * k);
        out_verts.push(RVertex {
            w00: [m.u32(a), m.u32(a + 4)],
            raw: f3(m, a + 0x08),
            raw_normal: f3(m, a + 0x14),
            world: f3(m, a + 0x20),
            uv: ptr(m.u32(a + 0x40)),
            w44: m.u32(a + 0x44),
            p48: ptr(m.u32(a + 0x48)),
            ..RVertex::default()
        });
    }
    let mut corners = Vec::with_capacity(nt.max(0) as usize);
    for t in 0..nt.max(0) as u32 {
        let a = tris.wrapping_add(0x60 * t);
        let mut c = [None; 3];
        for (i, slot) in c.iter_mut().enumerate() {
            let p = m.u32(a + 4 * i as u32);
            if p != 0 {
                let off = p.wrapping_sub(verts);
                if off % 0x4c != 0 || off / 0x4c >= nv.max(0) as u32 {
                    return Err(format!("a corner at {p:#x} outside the vertex block"));
                }
                *slot = Some((off / 0x4c) as usize);
            }
        }
        corners.push(c);
    }
    if verts != 0 {
        m.free(verts);
    }
    if tris != 0 {
        m.free(tris);
    }
    if !faulted && m.fault().is_some() {
        return Err(format!("a fault of the image: {:?}", m.fault()));
    }
    Ok(crate::softtriangle::NodeDef::NurbsOf { verts: out_verts, corners })
}

/// The calls of a trimmed surface's triangulation that no `si-nurbs` port answers: remembered by name.
#[derive(Default)]
struct TrimSide {
    names: Vec<&'static str>,
}
impl si_mem::Outside for TrimSide {
    fn call(&mut self, _: &mut Mem, r: si_mem::Routine, _: &[u32]) -> si_mem::Ret {
        self.names.push(r.name);
        si_mem::Ret::NONE
    }
}

/// What `SI_faceConvertToMesh` (`si_model::mesh_image::face`) calls outside `face.c`,
/// answered on the image by the ports: `SI_nurbsCrvConvertToLinear`, `SI_nurbsCrvInverse`
/// (`si-nurbs`' image forms; the reversal's two calls into the animation package answer 0,
/// counted), `SI_nurbsCrvInit`, `SI_nurbsCrvDispose` (`si-hrc`'s), `SI_splineConvertToLinear`,
/// `SI_splineCleanup`, `SI_splineInverse` (`si-spline`'s, their calls out answered 0,
/// counted). Every other routine (`SI_meshUpdate`'s `SI_showActivity` and status message)
/// answers 0 and is counted by name in `zero`; a port that refuses is written in `refused`.
#[derive(Default)]
pub struct FaceSide {
    pub zero: std::collections::BTreeMap<&'static str, usize>,
    pub refused: Vec<String>,
}

struct FaceSideQuiet<'a>(&'a mut std::collections::BTreeMap<&'static str, usize>);
impl si_mem::Outside for FaceSideQuiet<'_> {
    fn call(&mut self, _: &mut Mem, r: si_mem::Routine, _: &[u32]) -> si_mem::Ret {
        *self.0.entry(r.name).or_default() += 1;
        si_mem::Ret::NONE
    }
}
impl si_spline::spline_image::Outside for FaceSideQuiet<'_> {
    fn call(&mut self, _: &mut Mem, c: &si_spline::spline_image::Callee, _: &[u32]) -> u32 {
        *self.0.entry(c.name).or_default() += 1;
        0
    }
}

impl si_model::model_image::Outside for FaceSide {
    fn call(&mut self, m: &mut Mem, callee: &si_model::model_image::Callee, a: &[u32]) -> u32 {
        use si_model::mesh_image::face as fc;
        use si_types::Ptr32;
        let w = |k: usize| a.get(k).copied().unwrap_or(0);
        match callee.addr {
            x if x == fc::SI_NURBS_CRV_CONVERT_TO_LINEAR.addr => match si_nurbs::image::crv_convert_to_linear(m, w(0)) {
                Ok(c) => c,
                Err(h) => {
                    self.refused.push(format!("SI_nurbsCrvConvertToLinear: {h:?}"));
                    0
                }
            },
            x if x == fc::SI_NURBS_CRV_INVERSE.addr => si_nurbs::image::crv_inverse(m, &mut FaceSideQuiet(&mut self.zero), w(0)) as u32,
            x if x == fc::SI_NURBS_CRV_INIT.addr => {
                si_hrc::image::nurbs::nurbs_crv_init(m, w(0), w(1));
                0
            }
            x if x == fc::SI_NURBS_CRV_DISPOSE.addr => {
                si_hrc::image::nurbs::nurbs_crv_dispose(m, w(0));
                0
            }
            x if x == fc::SI_SPLINE_CONVERT_TO_LINEAR.addr => si_spline::convert_image::spline_convert_to_linear(m, &mut FaceSideQuiet(&mut self.zero), Ptr32::new(w(0)), w(1) as i16, w(2) as i32, f32::from_bits(w(3))).addr(),
            x if x == fc::SI_SPLINE_CLEANUP.addr => {
                si_spline::convert_image::spline_cleanup(m, &mut FaceSideQuiet(&mut self.zero), Ptr32::new(w(0)));
                0
            }
            x if x == fc::SI_SPLINE_INVERSE.addr => {
                si_spline::convert_image::spline_inverse(m, &mut FaceSideQuiet(&mut self.zero), Ptr32::new(w(0)));
                0
            }
            _ => {
                *self.zero.entry(callee.name).or_default() += 1;
                0
            }
        }
    }
}

/// `SI_face2triangle`'s calls of `SI_faceConvertToMesh(face, 0)` (0x08435960) for a node of
/// kind 2 whose face is of NURBS curves (+0x84 is 0), on the image with `si-model`'s port
/// (`si_model::mesh_image::face::face_convert_to_mesh`) and [`FaceSide`]: the node's
/// current face (+0x48), then, when it gave a mesh and the face that lends positions (the
/// first of the array at +0x38 when +0x34 is 1, else the second) is another record, that
/// one. The meshes are read ([`inimage::mesh_at`]) into a
/// [`crate::softtriangle::NodeDef::FaceMeshOf`] and disposed of as the original disposes
/// of them after `SI_meshTriangles` (`SI_meshDispose`, the shape's first; `si-model`'s port).
pub fn nurbs_face_on_image(m: &mut Mem, node: u32, side: &mut FaceSide) -> Result<crate::softtriangle::NodeDef, String> {
    use si_model::mesh_image::{face::face_convert_to_mesh, obj::mesh_dispose};
    let nb = m.i16(node + 0x34);
    let defs = m.u32(node + 0x38);
    let shape = if defs != 0 { m.u32(defs.wrapping_add(if nb == 1 { 0 } else { 4 })) } else { 0 };
    let current = m.u32(node + 0x48);
    let faulted = m.fault().is_some();
    let refused = side.refused.len();
    let held = m.local(8);
    let made = face_convert_to_mesh(m, side, current, 0);
    m.set_u32(held, made);
    m.set_u32(held + 4, 0);
    let mesh = m.u32(held);
    let mut def = crate::softtriangle::NodeDef::FaceMeshOf { mesh: None, shape: None };
    if mesh != 0 {
        let other = (shape != 0 && shape != current).then(|| face_convert_to_mesh(m, side, shape, 0));
        m.set_u32(held + 4, other.unwrap_or(0));
        let read = |m: &Mem, a: u32| (a != 0).then(|| Box::new(inimage::mesh_at(m, a, 0)));
        def = crate::softtriangle::NodeDef::FaceMeshOf { mesh: read(m, mesh), shape: other.map(|o| read(m, o)) };
        if other.is_some() {
            mesh_dispose(m, held + 4);
        }
        mesh_dispose(m, held);
    }
    m.release(8);
    if side.refused.len() > refused {
        return Err(side.refused[refused..].join("; "));
    }
    if !faulted && m.fault().is_some() {
        return Err(format!("a fault of the image: {:?}", m.fault()));
    }
    Ok(def)
}

/// The block a picture record's pixels lie in: its start, from the pointer at +0x1c (which
/// points width + 4 pixels of five bytes into it).
fn pixel_block(m: &Mem, record: u32) -> u32 {
    let p = m.u32(record + 0x1c);
    if p == 0 {
        0
    } else {
        p.wrapping_sub(((m.i16(record + 0x16) as i32 + 4).max(0) as u32).wrapping_mul(5))
    }
}

/// A text as a block of `malloc`, as `strdup` leaves it.
fn dup_text(m: &mut Mem, text: &[u8]) -> u32 {
    let a = m.malloc(text.len() as u32 + 1);
    m.set_bytes(a, text);
    m.set_u8(a + text.len() as u32, 0);
    a
}

/// What `SI_materials`, `SI_getMapsList`, `SI_textures`, `SI_collect`'s projection of the
/// textures and `updateTxt3DTransf` left in this crate's records, stored in the image where
/// the original stores it:
///
/// - each material: the working block, the two decay tables, the flag word, the count of 2D
///   textures, the reflection map (+0x6f4);
/// - each 2D texture: the working factors, the region and its flags, the projection's
///   centre, matrix and cached box, the frame's file name (+0x20, a block of `malloc`; the
///   name before it is freed when it is one) and the picture record (+0x24);
/// - each picture record: a new one is a block of `SI_calloc(1, 0x28)` with its name
///   (`strdup`), entered in the dictionary through `SI_elemdictAddPicture (0x08102ab0)` and
///   put at the head of the picture list; a record that lost its last user leaves the
///   dictionary through `SI_elemdictPictureElemRemove (0x08102c34)` and is freed with its
///   name and pixels; the pixels of a picture that was read are a block of `SI_malloc`
///   (+0x1c points width + 4 pixels into it), the size, the users and the link;
/// - each 3D texture: the working factors, and the made texture's matrix;
/// - each node of the MODELS chapter: the flag word (+0x214);
/// - the ray tracing word (0x08790c7c), the picture list's head as `SI_update` keeps it
///   (0x087907e0), the counter of texture pixels (`SI_stats` +0x34), and the "needed" byte
///   of the elements of the PICTURES and 2D textures chapters.
///
/// A material's two jitter tables (+0x6b0, +0x6c0) and a light's (+0x134) live in chunk
/// memory: they are stored when `SI_renChunk` is a record of the image ([`jitter_to_arena`]),
/// the default material's first and then the MATERIALS chapter's in its order, as
/// `SI_materials` makes them; else not at all (nothing that is ported reads them).
pub fn write_back(m: &mut Mem, rest: &mut dyn Outside, p: &Prepared, job: &Job, placed: &mut Placed) {
    let st = &p.st;
    placed.pictures.resize(st.pictures.len(), 0);
    // what the picture list asked of the dictionary, in the order it asked
    for event in &job.picture_events {
        match event {
            PictureEvent::Added(i, text) => {
                let a = m.si_calloc(1, 0x28);
                let name = dup_text(m, text);
                m.set_u32(a, name);
                placed.pictures[*i] = a;
                rest.call(m, f::SI_ELEMDICT_ADD_PICTURE, &[placed.dictionary, name, a, 0]);
            }
            PictureEvent::Removed(i) => {
                let a = placed.pictures[*i];
                if a != 0 {
                    let name = m.u32(a);
                    rest.call(m, f::SI_ELEMDICT_PICTURE_ELEM_REMOVE, &[placed.dictionary, a, name]);
                    let block = pixel_block(m, a);
                    for b in [block, name, a] {
                        if b != 0 {
                            m.free(b);
                        }
                    }
                    placed.pictures[*i] = 0;
                }
            }
        }
    }
    for (i, gl) in st.pictures.iter().enumerate() {
        let a = placed.pictures[i];
        if gl.freed || a == 0 {
            continue;
        }
        let old = pixel_block(m, a);
        inimage::glb_to_image(m, a, gl);
        match &gl.pixels {
            None => {
                if old != 0 {
                    m.free(old);
                    m.set_u32(a + 0x1c, 0);
                }
            }
            Some(px) => {
                let n = px.block.len() as u32;
                let same = old != 0 && m.block_size(old) == Some(n) && m.bytes(old, n) == &px.block[..];
                if !same {
                    if old != 0 {
                        m.free(old);
                    }
                    let block = m.si_malloc(n);
                    m.set_bytes(block, &px.block);
                    m.set_u32(a + 0x1c, block + 5 * px.origin as u32);
                }
            }
        }
    }
    for (i, gl) in st.pictures.iter().enumerate() {
        if !gl.freed && placed.pictures[i] != 0 {
            m.set_u32(placed.pictures[i] + 0x24, gl.next.map_or(0, |n| placed.pictures[n]));
        }
    }
    let head = st.picture_head.map_or(0, |i| placed.pictures[i]);
    if placed.picture_head != 0 {
        m.set_u32(placed.picture_head, head);
    }
    m.set_u32(g::SI_DATABASE + 0x1a0, head);
    for (i, &a) in placed.maps.iter().enumerate() {
        if a == 0 {
            continue;
        }
        let r = &st.maps[i];
        inimage::map_setup_to_image(m, a, r);
        let cur = m.u32(a + 0x20);
        let cur_text = (cur != 0).then(|| m.cstr(cur));
        if r.file != cur_text {
            if cur != 0 && m.block_size(cur).is_some() {
                m.free(cur);
            }
            let name = match &r.file {
                Some(text) => dup_text(m, text),
                None => 0,
            };
            m.set_u32(a + 0x20, name);
        }
        m.set_u32(a + 0x24, r.glb.map_or(0, |k| placed.pictures[k]));
    }
    for (i, &a) in placed.materials.iter().enumerate() {
        if a == 0 {
            continue;
        }
        inimage::material_setup_to_image(m, a, &st.materials[i]);
        m.set_u32(a + 0x6f4, st.materials[i].reflect_map.map_or(0, |k| placed.maps.get(k).copied().unwrap_or(0)));
    }
    for (i, &a) in placed.tex3ds.iter().enumerate() {
        if a != 0 {
            inimage::tex3d_setup_to_image(m, a, &st.tex3ds[i]);
        }
    }
    let chunk = m.u32(g::SI_REN_CHUNK);
    if chunk != 0 && m.is_mapped(chunk, 0x40) {
        let mut order: Vec<usize> = st.default_material.into_iter().collect();
        order.extend(job.materials.iter().filter(|e| e.unused == 0).filter_map(|e| e.data));
        order.extend(0..placed.materials.len());
        let mut done = vec![false; placed.materials.len()];
        for i in order {
            let a = placed.materials.get(i).copied().unwrap_or(0);
            if a == 0 || std::mem::replace(&mut done[i], true) {
                continue;
            }
            jitter_to_arena(m, chunk, a + 0x6ac, &st.materials[i].jitter_6ac);
            jitter_to_arena(m, chunk, a + 0x6bc, &st.materials[i].jitter_6bc);
        }
        let mut l = m.u32(g::DB_LIGHTS);
        for light in &st.lights {
            if l == 0 {
                break;
            }
            jitter_to_arena(m, chunk, l + 0x130, &light.jitter);
            l = m.u32(l + 0x140);
        }
    }
    for (k, &a) in placed.nodes.iter().enumerate() {
        m.set_u16(a + model::TXT_FLAGS, p.g.models[k].flags);
    }
    m.set_u16(0x0879_0c7c, p.options.raytrace);
    m.set_i32(g::SI_STATS + 0x34, st.texture_pixels);
    // the preview's marking of what is needed
    let chapter = |m: &Mem, no: u32| m.u32(placed.dictionary + dict::DICT_LISTS).wrapping_add(m.u32(no).wrapping_mul(dict::LIST_SIZE));
    for (no, list) in [(g::CID_PICTURES, &job.textures.pictures), (g::CID_TEXTURES_2D, &job.textures.maps)] {
        let ch = chapter(m, no);
        let count = m.i32(ch + dict::LIST_COUNT).max(0) as usize;
        for (i, e) in list.iter().enumerate().take(count) {
            let at = m.u32(ch + dict::LIST_ELEMS) + i as u32 * dict::ELEM_SIZE + dict::ELEM_WORK;
            if m.u8(at) != e.dirty {
                m.set_u8(at, e.dirty);
            }
        }
    }
}

/// A jitter table the set-up made, written where `si_initJitter` writes it
/// ([`crate::jitter_image::init_jitter`]): the table of row pointers and each row as a
/// piece of the arena, the record's pointer (+4) and its cleared word (+8). Nothing for a
/// record whose table was not made in this update.
fn jitter_to_arena(m: &mut Mem, chunk: u32, record: u32, j: &crate::jitter::Jitter) {
    use crate::mem_chunk_image::chunk_malloc;
    if j.table.is_empty() {
        return;
    }
    let table = chunk_malloc(m, chunk, (j.table.len() as i32).wrapping_mul(4));
    if table == 0 {
        return;
    }
    for (i, row) in j.table.iter().enumerate() {
        let piece = chunk_malloc(m, chunk, (row.len() as i32).wrapping_mul(8));
        m.set_u32(table + 4 * i as u32, piece);
        if piece == 0 {
            continue;
        }
        for (k, pair) in row.iter().enumerate() {
            m.set_f32(piece + 8 * k as u32, pair[0]);
            m.set_f32(piece + 8 * k as u32 + 4, pair[1]);
        }
    }
    m.set_u32(record + 4, table);
    m.set_u16(record + 8, j.next as u16);
}

/// `si_cleanUpTexture()` on the image's records (the port on this crate's records is
/// [`crate::materials::clean_up_texture`]): every picture element in use whose record has
/// pixels gets them freed and the pointer cleared.
fn clean_up_pictures(m: &mut Mem) {
    let dictionary = m.u32(g::SI_ELEM_DICTS).wrapping_add((m.i32(m.u32(g::SI_PROC_DATA)) as u32).wrapping_mul(dict::DICT_SIZE));
    let ch = m.u32(dictionary + dict::DICT_LISTS).wrapping_add(m.u32(g::CID_PICTURES).wrapping_mul(dict::LIST_SIZE));
    for i in 0..m.i32(ch + dict::LIST_COUNT).max(0) as u32 {
        let e = m.u32(ch + dict::LIST_ELEMS).wrapping_add(i * dict::ELEM_SIZE);
        let record = m.u32(e + dict::ELEM_DATA);
        if record == 0 || m.i16(e + dict::ELEM_UNUSED) != 0 {
            continue;
        }
        let block = pixel_block(m, record);
        if block != 0 {
            m.free(block);
            m.set_u32(record + 0x1c, 0);
        }
    }
}


impl Render<'_> {
    /// `SI_update(time)`: the scene from the image, the ported `SI_update` on it, and in
    /// the image what `SI_image` reads afterwards: the count of filed triangles (`SI_stats`
    /// +0x00), the region's first and last pixel column (0x087907a0, 0x087907a4) and the
    /// background's four bytes (0x08790d14).
    ///
    /// Read from the image for the scene: the root chain (0x087907e8) through +0x2c0 and
    /// +0x2c4; each node, its mesh and its texture coordinate sets
    /// ([`inimage::node_source_at`]) and its +0x08 bit 3, +0x30, +0x44, +0x16c, +0x1c4,
    /// +0x206, +0x20c, +0x214, +0x216, +0x21c, +0x224, +0x2c0, +0x2c4, +0x354, +0x3a0; each
    /// material, 2D texture, picture, 3D texture and light whole ([`inimage`]), the lists
    /// through +0x6ec, +0x6f4, +0x6fc, +0x16c, +0x24 and the light chain (0x087907e4)
    /// through +0x140; the MODELS and MATERIALS chapters of the current dictionary (+0x04
    /// and +0x48 of each element).
    fn update(&mut self, m: &mut Mem, time: f32) {
        let (s, mut o) = controls_from_image(m);
        // (the sampling level and the ray tracing word are initData's and the set-up's by now)
        o.raytrace = self.p.options.raytrace;
        self.p.settings = Settings { level: self.p.settings.level, ..s };
        self.p.options = o;
        self.p.st.glb.frame = m.i16(g::FRAME_NOW);
        self.p.st.fault = None;
        // SI_update's two calls out of the renderer, before it reads anything of the scene
        // (0x0826ee30 … 0x0826ee94, listing): `SI_frameCurrent` (0x08782040) = the frame (0x08790dda)
        // and, in sequence mode, `SI_sceneUpdateItems (0x082221a0)(time, 1, &SI_database,
        // 1)`, which brings the scene to the frame's time and copies the current camera
        // over the renderer's (`SI_database`); `SI_frameCurrent` put back; then
        // `SI_edGetElementsGrafNodeInfo (0x08104acc)(&SI_elemDicts[SI_procData[0]])`. Both
        // are the embedding program's to answer (the render command's world does not yet:
        // finding 0012, section 21). The scene, and the camera that `SI_camera` reads, are
        // read from the image afterwards.
        let saved = m.i32(g::SI_FRAME_CURRENT);
        m.set_i32(g::SI_FRAME_CURRENT, m.i16(g::FRAME_NOW) as i32);
        if m.i16(g::SEQUENCE) == 2 {
            self.rest.call(m, f::SI_SCENE_UPDATE_ITEMS, &[time.to_bits(), 1, g::SI_DATABASE, 1]);
        }
        m.set_i32(g::SI_FRAME_CURRENT, saved);
        let dictionary = m.u32(g::SI_ELEM_DICTS).wrapping_add((m.i32(m.u32(g::SI_PROC_DATA)) as u32).wrapping_mul(dict::DICT_SIZE));
        self.rest.call(m, f::SI_ED_GET_ELEMENTS_GRAF_NODE_INFO, &[dictionary]);
        // `si_traverseExpandedInstance (0x082739e0)(root chain)`: the original makes it in
        // SI_update after `si_lights`, before `si_traverse`; it brings the copies of the
        // instances' masters (made once by the render command) to the frame's state. The
        // scene is read from the image just below, so the call goes here. The embedding
        // program answers it (`RENDER_NO_EXPANDED_INSTANCES=1` in the environment leaves it
        // out, for measuring).
        static NO_EXPANDED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*NO_EXPANDED.get_or_init(|| std::env::var_os("RENDER_NO_EXPANDED_INSTANCES").is_some_and(|v| v == "1")) {
            let root = m.u32(g::DB_ROOT);
            self.rest.call(m, f::SI_TRAVERSE_EXPANDED_INSTANCE, &[root]);
        }
        self.p.camera = camera_from_image(m, g::SI_DATABASE);
        let (mut job, mut placed) = build_scene(&mut self.p, &mut *self.files, m);
        // A 3D texture whose turbulence object (+0x8c) is null was made by a loader that
        // answered the readers' call of `SI_updTxt3dTurb (0x082b8d20)` with nothing (the
        // original's readers always make it: `texture3DTransfer`). The call is made here,
        // with the port on the image, as the reader would have made it, and the scene read
        // again. Not the renderer's own doing: it goes once the loader makes the call.
        let missing: Vec<u32> = placed.tex3ds.iter().zip(&self.p.st.tex3ds).filter(|(a, t)| **a != 0 && t.txt.is_none() && m.u32(**a + 0x8c) == 0).map(|(a, _)| *a).collect();
        if !missing.is_empty() {
            drop(job);
            for &a in &missing {
                crate::t3d_fct_image::upd_txt3d_turb(m, a, &mut crate::tt::TrigLib);
            }
            self.notes.push(format!("adapter: SI_updTxt3dTurb made {} 3D textures ready that the loader had left without", missing.len()));
            (job, placed) = build_scene(&mut self.p, &mut *self.files, m);
        }
        // the NURBS surfaces and the faces of NURBS curves, converted on the image where
        // `SI_collect` would convert them (see [`convert_surfaces`])
        convert_surfaces(m, &placed.nodes, &mut job.scene, &mut self.notes);
        job.rng = self.rng.clone();
        // motion blur (sequence mode with the switch on): `si_blurGeometry` animates the scene
        // to other times, which only the image can do; the world around the job does that
        let r = {
            let mut world = BlurJob { job: &mut job, m: &mut *m, rest: &mut *self.rest, nodes: &placed.nodes };
            crate::update::update(&mut self.p, &mut world, time)
        };
        self.rng = job.rng.clone();
        self.background_bytes = job.background_bytes;
        let mut notes = std::mem::take(&mut job.notes);
        write_back(m, &mut *self.rest, &self.p, &job, &mut placed);
        drop(job);
        let fatal = r.err().map(str::to_string).or_else(|| notes.iter().find_map(|n| n.strip_prefix("fatal: ").map(str::to_string)));
        self.notes.append(&mut notes);
        m.set_i32(g::SI_STATS, self.p.g.bins.accepted);
        m.set_i32(g::REGION_PX_X_MIN, self.p.view.region_px[0]);
        m.set_i32(g::REGION_PX_X_MAX, self.p.view.region_px[1]);
        m.set_bytes(g::BACKGROUND_BYTES, &self.background_bytes);
        if let Some(text) = fatal {
            self.fatal(m, &text);
        }
    }

    /// One of the two frame loops: the frame from the prepared scene, each line to the
    /// picture being written (when the picture file is on, 0x08790d5a) and to the display
    /// (when that is on, 0x08790d58: the line is put in the outgoing byte lines and
    /// `SI_fbWrite` called). The licence call both loops start with is made by the caller,
    /// the dispatch of `SI_sample` and `SI_supersample` (see `call`).
    fn frame(&mut self, m: &mut Mem) {
        // the output dither (0x08790e30, as `SI_Output` tests it) on the state
        // `SI_fsDitherInit` made after this frame's update; the noise's amplitude is the float
        // at 0x08790e38
        let amplitude = m.f32(0x0879_0e38);
        let dither = if m.i32(g::DITHER) != 0 { self.dither.as_mut().map(|d| (d, amplitude)) } else { None };
        // the depth file (switch 0x08790e2c, `Z_CHANNEL YES`): the loops open `<name>.<frame>.Zpic`
        // with the output name as the string at 0x08790d5c holds it
        let depth = (m.i16(0x0879_0e2c) != 0).then(|| m.cstr(m.u32(g::NAME)));
        let (lines, depth) = crate::frame::render_with_depth(&mut self.p, self.background_bytes, dither, depth);
        if let Some(file) = depth {
            self.write_depth_file(m, file);
        }
        // with noise the dither drew from the process's generator, which the next update's
        // jitter tables go on from
        if let Some(d) = self.dither.as_ref().filter(|d| d.noise) {
            self.rng = crate::jitter::Drand48(d.random.0);
        }
        let fault = self.p.st.fault;
        for line in lines {
            let (display, file) = (m.i16(g::DISPLAY) != 0, m.i16(g::PICTURE_FILE) != 0);
            if display || (file && self.host.is_some()) {
                m.set_i16(g::OUT_LINE, line.y as i16);
                for (at, bytes) in [(g::OUT_RED, &line.red), (g::OUT_GREEN, &line.green), (g::OUT_BLUE, &line.blue), (g::OUT_ALPHA, &line.alpha)] {
                    let p = m.u32(at);
                    m.set_bytes(p, bytes);
                }
            }
            // (SI_Output's order: the picture file, then the display)
            if file {
                let width = line.red.len() as u32;
                let a = [line.y as u32, width, m.u32(g::OUT_RED), m.u32(g::OUT_GREEN), m.u32(g::OUT_BLUE), m.u32(g::OUT_ALPHA)];
                self.write_line(m, &a, Some(line.clone()));
                if self.jumped {
                    return;
                }
            }
            if display {
                crate::fbuffer::fb_write(m, self, line.y as i16 as i32 as u32);
            }
        }
        // the original's process ends in a segmentation fault during the frame: the lines
        // before it are written (stdio flushes them on the way out), the fault is recorded
        // in the image for whoever embeds the renderer, and the render is left as after a
        // jump (the original does not come back at all)
        if let Some(addr) = fault {
            self.notes.push(format!("fault: SI_shade reads address {addr:#x} (a vertex's texture pairs through a null pointer): the original ends in a segmentation fault"));
            m.record_fault(addr, 4, si_mem::Access::Read);
            self.jumped = true;
        }
    }
}

impl Render<'_> {
    /// The depth file the frame loops left ([`crate::frame::DepthFile`]) through the machine's
    /// files: `fopen(name, "wb+")` (the name is the program's, `sprintf("%s.%d.Zpic", name,
    /// frame)`: relative to the working directory when the output name is), the rows, and
    /// `fclose`. Without a host nothing is written. A file the loops left open (the frame ended
    /// in a fault) gets the whole 4096-byte blocks stdio would have flushed before the process
    /// ended: that block size is an assumption, not read from the program.
    ///
    /// Host convention (not the program's): the host here has one root for everything the
    /// frame writes, and no working directory of its own, so a relative name is taken below
    /// the picture path (`SI_picturePath`, the scene's directory), which puts the depth file
    /// beside the picture instead of in the root shared by all scenes.
    fn write_depth_file(&mut self, m: &mut Mem, mut file: crate::frame::DepthFile) {
        if self.host.is_none() {
            return;
        }
        if file.name.first() != Some(&b'/') {
            let mut path = m.cstr(m.u32(g::SI_PICTURE_PATH));
            if path.last() != Some(&b'/') {
                path.push(b'/');
            }
            path.extend_from_slice(&file.name);
            file.name = path;
        }
        let Some(host) = self.host.as_mut() else { return };
        let stream = host.fopen(&file.name, b"wb+");
        if stream == 0 {
            return;
        }
        let n = if file.closed { file.bytes.len() } else { file.bytes.len() / 4096 * 4096 };
        host.fwrite(stream, &file.bytes[..n]);
        host.fclose(stream);
    }

    /// `SI_fsDitherInit(noise, width)` (0x085cfac4, listing; ported on values in si-imgfmt,
    /// `OutputDither::new`): the width and the noise switch (sign-extended) in its two
    /// globals (0x086fade8, 0x086fadec); with noise on `srand48(0xd431)`, which reseeds the
    /// process's generator the jitter shares; then two rows of three floats a pixel
    /// (12 × width bytes each, the product in 32 bits) from the renderer's chunk memory
    /// (`SI_chunkMalloc(SI_renChunk, …)`), stored at 0x086fade4 and 0x086fade0, the second
    /// cleared (three words a pixel, for a width above 0). The diffusion itself runs on
    /// si-imgfmt's record of the two rows, not on these blocks. The original writes through
    /// the second block without a test; a null block is left unwritten here.
    /// `SI_fsDitherInit (0x085cfac4)`'s stores into the image: unchecked (read from the
    /// listing; the values are si-imgfmt's checked port; measured only on `IK-PageTurn`'s
    /// three frames, finding 0012, section 23.4).
    fn dither_init(&mut self, m: &mut Mem, noise: u32, width: u32) {
        let (noise, width) = (noise as i16, width as i32);
        m.set_i32(0x086f_ade8, width);
        m.set_i32(0x086f_adec, noise as i32);
        if noise != 0 {
            self.rng = crate::jitter::Drand48::seeded(0xd431);
        }
        let size = width.wrapping_mul(12) as u32;
        let chunk = m.u32(g::SI_REN_CHUNK);
        let first = self.call(m, f::SI_CHUNK_MALLOC, &[chunk, size]);
        m.set_u32(0x086f_ade4, first);
        let second = self.call(m, f::SI_CHUNK_MALLOC, &[chunk, size]);
        m.set_u32(0x086f_ade0, second);
        if second != 0 && width > 0 {
            m.set_fill_bytes(second, 0, size);
        }
        self.dither = Some(si_imgfmt::gradation::OutputDither::new(noise, width, si_imgfmt::gradation::Drand48(self.rng.0)));
    }

    /// `SI_rleWrite(line, width, red, green, blue, alpha)`: the line is kept (unless the
    /// caller asked otherwise) and, with a host, written.
    fn write_line(&mut self, m: &mut Mem, args: &[u32], line: Option<Line>) {
        let keep = self.keep_lines;
        if let Some(pic) = self.pictures.last_mut() {
            pic.written += 1;
            if keep || self.on_line.is_some() {
                let n = args[1] as i32;
                let bytes = |m: &Mem, p: u32| if n > 0 { m.bytes(p, n as u32).to_vec() } else { Vec::new() };
                let line = line.unwrap_or_else(|| Line { y: args[0] as i32, red: bytes(m, args[2]), green: bytes(m, args[3]), blue: bytes(m, args[4]), alpha: bytes(m, args[5]) });
                if let Some(take) = self.on_line.as_mut() {
                    take(&pic.name, &line);
                }
                if keep {
                    pic.lines.push(line);
                }
            }
        }
        if let Some(host) = self.host.take() {
            crate::rle_image::rle_write(m, self, &mut *host, args[0] as i32, args[1] as i32, args[2], args[3], args[4], args[5]);
            self.host = Some(host);
        }
    }
}

impl Outside for Render<'_> {
    fn call(&mut self, m: &mut Mem, addr: u32, args: &[u32]) -> u32 {
        match addr {
            // ported in place
            f::SI_INITIALIZE => crate::initialize::initialize(m, self, args[0]),
            f::BUILD_NAME => {
                crate::rnd_main::build_name(m, args[0], args[1]);
                0
            }
            f::SI_REN_EXIT => {
                crate::rnd_main::ren_exit(m, self, args[0], args[1] as i16, args[2]);
                0
            }
            f::SI_FB_INIT => {
                crate::fbuffer::fb_init(m, self);
                0
            }
            f::SI_FB_WRITE => {
                crate::fbuffer::fb_write(m, self, args[0]);
                0
            }
            f::SI_SHOWFRAME => {
                crate::rnd_main::show_frame(m, self, args[0] as i16, args[1]);
                0
            }
            f::SI_SHOWFIELD => {
                crate::rnd_main::show_field(m, self, args[0] as i16, args[1] as i16, args[2]);
                0
            }
            // ported on the crate's own records
            f::INIT_DATA => {
                self.begin(m);
                0
            }
            // (done with initData above, in the original's order)
            f::SI_INIT_VIEWPORT | f::INIT_BUFFERS | f::INIT_FILTER => 0,
            f::SI_UPDATE => {
                self.update(m, f32::from_bits(args[0]));
                0
            }
            // both loops first ask `getRenderLicense` (`crate::licence`; the client's answer a
            // granted checkout) and return at once when it answers 0
            f::SI_SAMPLE | f::SI_SUPERSAMPLE => {
                if crate::licence::get_render_license(m, self) != 0 {
                    self.frame(m);
                }
                0
            }
            f::SI_CLEAN_UP_TEXTURE => {
                clean_up_pictures(m);
                0
            }
            f::SI_FS_DITHER_INIT => {
                self.dither_init(m, args[0], args[1]);
                0
            }
            // the picture writer: the lines are kept (see `Picture`), and with a host the
            // routines of `rle.c` run on the image
            f::SI_RLE_INIT => {
                let sp = m.sp();
                let name = m.local(1024);
                crate::rnd_main::build_name(m, name, crate::rnd_main::text::SUFFIX_PIC);
                self.pictures.push(Picture { name: m.cstr(name), lines: Vec::new(), closed: false, written: 0 });
                m.set_sp(sp);
                match self.host.take() {
                    Some(host) => {
                        if m.u8(crate::data::text::FMT_PIC) == 0 {
                            crate::data::place_texts(m);
                        }
                        let r = crate::rle_image::rle_init(m, self, &mut *host, args[0] as i16);
                        self.host = Some(host);
                        r
                    }
                    None => 1,
                }
            }
            f::SI_RLE_WRITE => {
                self.write_line(m, args, None);
                0
            }
            f::SI_RLE_CLOSE => {
                if let Some(pic) = self.pictures.last_mut() {
                    pic.closed = args[0] as i16 != 0;
                }
                if let Some(host) = self.host.take() {
                    self.in_writer = true;
                    crate::rle_image::rle_close(m, self, &mut *host, args[0] as i16, args[1] as i16);
                    self.in_writer = false;
                    self.host = Some(host);
                    if let Some(path) = self.pending_icon.take() {
                        self.add_icon(&path);
                    }
                }
                0
            }
            // SI_addIconToPic (0x085c4808), called by SI_rleClose while the host is lent to
            // it: noted, and done when the writer has returned ([`Render::add_icon`])
            f::SI_ADD_ICON_TO_PIC if self.in_writer => {
                self.pending_icon = Some(m.cstr(args[0]));
                0
            }
            // ported in place: the error exit and the wrap flags of a texture
            f::SI_FATAL => {
                crate::renutil_image::fatal(m, self, args[0], args[1], args[2]);
                0
            }
            f::LONGJMP => {
                self.jumped = true;
                0
            }
            f::SI_TXT2D_SET_WRAP => {
                crate::t2d_dial::set_wrap(m, self, args[0], args[1]);
                0
            }
            // ported in place (eleventh pass): the warning on the screen or standard output,
            // and the timing lines of the statistics file
            f::SI_WARNING => {
                crate::renutil_image::warning(m, self, args[0], args[1], args[2]);
                0
            }
            f::SI_DTIME => {
                crate::renutil_image::dtime(m, self, args[0]);
                0
            }
            // the chunk memory: `mem_chunk.c` on the image when the buffer handed over is a
            // record of it (`SI_renChunk` after `SI_chunkAllocate`)
            f::SI_CHUNK_INITIALIZE | f::SI_CHUNK_RECLAIM | f::SI_CHUNK_FREE_ALL | f::SI_CHUNK_SAVE_CURRENT | f::SI_CHUNK_MALLOC
                if args.first().is_some_and(|b| *b != 0 && m.is_mapped(*b, 0x40)) =>
            {
                crate::mem_chunk_image::call(m, addr, args).unwrap_or(0)
            }
            // without one (an image nothing ran `SI_matterInit`'s allocation on): only the
            // mark `SI_image` saves and gives back passes through here, and a piece is a
            // block of `SI_malloc`
            f::SI_CHUNK_INITIALIZE | f::SI_CHUNK_RECLAIM | f::SI_CHUNK_FREE_ALL => 0,
            f::SI_CHUNK_SAVE_CURRENT => {
                self.marks += 1;
                m.set_u32(args[1], self.marks);
                self.marks
            }
            f::SI_CHUNK_MALLOC => m.si_malloc(args[1]),
            f::SETJMP => 0,
            _ => self.rest.call(m, addr, args),
        }
    }

    fn jumped(&mut self) -> bool {
        self.jumped || self.rest.jumped()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::NoPictures;

    /// A node of kind 2 whose face is one closed NURBS curve of order 2 through the corners
    /// of a square (the last of weight 2, its coordinates doubled).
    fn nurbs_square_node(m: &mut Mem) -> u32 {
        let pts = m.calloc(4, 0x24);
        for (k, (x, y, w)) in [(0.0, 0.0, 1.0), (2.0, 0.0, 1.0), (2.0, 2.0, 1.0), (0.0, 4.0, 2.0)].iter().enumerate() {
            let at = pts + 0x24 * k as u32;
            m.set_f64(at, *x);
            m.set_f64(at + 8, *y);
            m.set_f64(at + 0x18, *w);
        }
        let knots = m.calloc(5, 8);
        for k in 0..5 {
            m.set_f64(knots + 8 * k, k as f64);
        }
        let curves = m.calloc(1, 100);
        m.set_i32(curves, 2);
        m.set_i32(curves + 4, 4);
        m.set_i32(curves + 8, 5);
        m.set_u32(curves + 0xc, knots);
        m.set_f64(curves + 0x18, 4.0);
        m.set_i16(curves + 0x20, 1);
        m.set_u32(curves + 0x2c, pts);
        m.set_i32(curves + 0x34, 1);
        let face = si_hrc::image::spline::face_allocate(m).addr();
        m.set_i16(face + 0x78, 1);
        m.set_u32(face + 0x80, curves);
        m.set_i16(face + 0x84, 0);
        let node = m.calloc(1, 0x430);
        m.set_i32(node + 0x30, 2);
        m.set_u32(node + 0x48, face);
        node
    }

    #[test]
    fn a_face_of_nurbs_curves_is_converted_on_the_image_and_made_triangles() {
        let mut m = Mem::new();
        let node = nurbs_square_node(&mut m);
        let mut side = FaceSide::default();
        let def = nurbs_face_on_image(&mut m, node, &mut side).unwrap();
        let crate::softtriangle::NodeDef::FaceMeshOf { mesh: Some(mesh), shape: None } = &def else { panic!("no mesh: {def:?}") };
        assert_eq!((mesh.vertices.len(), mesh.polygons.len()), (4, 1));
        assert!(side.refused.is_empty());
        // only the busy cursor is left unanswered
        assert!(side.zero.keys().all(|k| k.ends_with("Activity")), "{:?}", side.zero);
        let mesh = (**mesh).clone();
        let r = crate::softvertex::face2triangle(true, &mut |_| Some(mesh.clone()), None, 3).unwrap();
        assert_eq!((r.verts.len(), r.tn_count), (4, 2));
        // the corner of weight 2 divided by it
        assert!(r.verts.iter().any(|v| (v.raw.x, v.raw.y) == (0.0, 2.0)));
    }

    /// The dictionary of an empty scene and the few calls of other packages a render of it
    /// makes; every call is noted.
    struct Rest(Vec<u32>);
    impl Outside for Rest {
        fn call(&mut self, _: &mut Mem, f: u32, _: &[u32]) -> u32 {
            self.0.push(f);
            0
        }
    }

    /// An image with the controls of a 6 × 4 picture, one frame, a camera at (0, 0, 10)
    /// looking at the origin, an empty dictionary and no root.
    fn empty_scene() -> (Mem, u32) {
        let mut m = Mem::new();
        crate::rnd_main::place_texts(&mut m);
        let dicts = m.calloc(1, dict::DICT_SIZE);
        let lists = m.calloc(8, dict::LIST_SIZE);
        m.set_u32(dicts + dict::DICT_LISTS, lists);
        let pd = m.calloc(3, 4);
        m.set_u32(g::SI_PROC_DATA, pd);
        m.set_u32(g::SI_ELEM_DICTS, dicts);
        for (k, cid) in [g::CID_LIGHTS, g::CID_MATERIALS, g::CID_MODELS, g::CID_TEXTURES_2D, g::CID_TEXTURES_3D].into_iter().enumerate() {
            m.set_u32(cid, k as u32);
        }
        m.set_i16(g::WIDTH, 6);
        m.set_i16(g::HEIGHT, 4);
        m.set_f32(0x0879_0d34, 1.0);
        m.set_i16(0x0879_0d38, 1);
        for a in [g::FRAME_FIRST, g::FRAME_LAST, g::FRAME_STEP] {
            m.set_i16(a, 1);
        }
        m.set_f32(g::SI_FRAME_RATE, 30.0);
        m.set_i16(g::PICTURE_FILE, 1);
        m.set_i16(g::SI_STANDALONE_RENDERER, 1);
        // the background: a colour whose bytes are 51, 102, 153. It only shows with the
        // fade on (without it si_initFade replaces the background by the blank sample);
        // the alpha is then 1 - the float at 0x08790d10
        for (i, v) in [0.2f32, 0.4, 0.6, 0.9].into_iter().enumerate() {
            m.set_f32(0x0879_0cf8 + 4 * i as u32, v);
        }
        m.set_i16(0x0879_0ce0, 1);
        m.set_f32(0x0879_0ce4, 5.0);
        m.set_f32(0x0879_0cec, 50.0);
        m.set_f32(0x0879_0d10, 1.0);
        let name = m.place_cstr(b"/tmp/shot");
        m.set_u32(g::NAME, name);
        let empty = m.place_cstr(b"");
        m.set_u32(g::PRE_SCRIPT, empty);
        m.set_u32(g::POST_SCRIPT, empty);
        let camera = m.calloc(1, 0x13c);
        m.set_f32(camera + 0x18, 10.0);
        m.set_f32(camera + 0x34, 0.1);
        m.set_f32(camera + 0x38, 100.0);
        m.set_f32(camera + 0x40, 0.8);
        (m, camera)
    }

    #[test]
    fn an_empty_scene_is_all_background_and_answers_0x7e() {
        si_rt::skip_without_install!();
        let (mut m, camera) = empty_scene();
        let (mut rest, mut files) = (Rest(Vec::new()), NoPictures);
        let mut r = Render::new(&mut rest, &mut files);
        let status = crate::rnd_main::image(&mut m, &mut r, camera, 0, 0, 0, 0);
        assert_eq!(status, crate::rnd_main::STATUS_NOTHING, "{:?}", r.notes);
        assert_eq!(r.pictures.len(), 1);
        let pic = &r.pictures[0];
        assert_eq!(pic.name, b"/tmp/shot.1.pic");
        assert!(pic.closed);
        // four lines, top first, each six pixels of the background's bytes
        assert_eq!(pic.lines.iter().map(|l| l.y).collect::<Vec<_>>(), [3, 2, 1, 0]);
        assert!(pic.lines.iter().all(|l| l.red == [51; 6] && l.green == [102; 6] && l.blue == [153; 6] && l.alpha == [0; 6]));
        drop(r);
        // the warning (SI_warning's port: in the stand-alone renderer a line on standard
        // output), and nothing drawn: the display is off
        assert!(rest.0.contains(&f::PRINTF));
        assert!(!rest.0.contains(&f::SI_WARNING));
        assert!(!rest.0.contains(&f::LRECTWRITE));
        assert!(m.fault().is_none());
        // the frame the program was at is put back
        assert_eq!(m.i32(g::SI_FRAME_CURRENT), 0);
    }

    /// One node in the image: a mesh of one square at the origin with a material of
    /// constant shading, both in the dictionary; the camera of the empty scene. Answers the
    /// image, the camera, the node and the material.
    fn square_scene() -> (Mem, u32, u32, u32) {
        use si_model::mesh::Mesh;
        use si_model::mesh_b::{update_nodes, MeshEnv};
        let (mut m, camera) = empty_scene();
        let mut mesh = Mesh::with_vertices(&[[-1.5, -1.5, 0.0], [1.5, -1.5, 0.0], [1.5, 1.5, 0.0], [-1.5, 1.5, 0.0]]);
        mesh.push_polygon(&[0, 1, 2, 3]);
        update_nodes(&mut mesh, true, &mut MeshEnv::default());
        let mesh_rec = inimage::mesh_to_image(&mut m, &mesh, 0);
        // the material: constant (model 1), its diffuse colour is the surface's
        let mat = m.calloc(1, 0xdcc);
        m.set_i16(mat + 0x1e, 1);
        for (i, v) in [1.0f32, 0.5, 0.25, 0.0].into_iter().enumerate() {
            m.set_f32(mat + 0x44 + 4 * i as u32, v);
        }
        m.set_i16(mat + 0xdc8, 1);
        m.set_i16(mat + 0xdca, 1);
        // the node: a mesh (kind 4), its own shape, one definition, one material, shown
        let node = m.calloc(1, 0x430);
        let (defs, mats) = (m.calloc(1, 4), m.calloc(1, 4));
        m.set_u32(defs, mesh_rec);
        m.set_u32(mats, mat);
        m.set_i32(node + model::KIND, 4);
        m.set_i16(node + 0x34, 1);
        m.set_u32(node + 0x38, defs);
        m.set_u32(node + 0x48, mesh_rec);
        m.set_i16(node + model::NB_MATERIALS, 1);
        m.set_u32(node + model::MATERIALS, mats);
        m.set_i16(node + 0x2b8, 1);
        m.set_i32(node + 0x424, 1);
        for o in [0x16c, 0x3a0] {
            for k in 0..3 {
                m.set_f32(node + o + 4 * k, 1.0);
            }
        }
        for k in 0..4 {
            m.set_f32(node + 0x1c4 + 20 * k, 1.0);
        }
        // the two chapters
        let lists = m.u32(m.u32(g::SI_ELEM_DICTS) + dict::DICT_LISTS);
        for (cid, record) in [(g::CID_MODELS, node), (g::CID_MATERIALS, mat)] {
            let chapter = lists + m.u32(cid) * dict::LIST_SIZE;
            let elems = m.calloc(1, dict::ELEM_SIZE);
            m.set_u32(elems + dict::ELEM_DATA, record);
            // every element has a relation record (here: no relations) and a name record
            let (relations, name) = (m.calloc(1, 0x28), m.calloc(1, 0x20));
            m.set_u32(elems + dict::ELEM_RELATIONS, relations);
            m.set_u32(elems + dict::ELEM_NAME_REC, name);
            m.set_i32(chapter + dict::LIST_COUNT, 1);
            m.set_u32(chapter + dict::LIST_ELEMS, elems);
        }
        // no fade: the surface's colour comes out as it is
        m.set_i16(0x0879_0ce0, 0);
        (m, camera, node, mat)
    }

    #[test]
    fn a_square_in_the_image_is_read_and_rendered() {
        si_rt::skip_without_install!();
        let (mut m, camera, node, mat) = square_scene();
        let (mut rest, mut files) = (Rest(Vec::new()), NoPictures);
        let mut r = Render::new(&mut rest, &mut files);
        let status = crate::rnd_main::image(&mut m, &mut r, camera, 0, 0, 0, node);
        assert_eq!(status, 0, "{:?}", r.notes);
        assert!(!r.notes.iter().any(|n| n.starts_with("fatal")), "{:?}", r.notes);
        assert_eq!(r.p.g.bins.accepted, 2, "the square's two triangles are filed");
        let pic = &r.pictures[0];
        assert_eq!(pic.lines.iter().map(|l| l.y).collect::<Vec<_>>(), [3, 2, 1, 0]);
        // the middle of the picture is the material's colour (1, 0.5, 0.25 as bytes, cut
        // off), its corners are not covered
        let line = &pic.lines[1];
        assert_eq!((line.red[2], line.green[2], line.blue[2]), (255, 127, 63));
        assert_eq!((line.red[3], line.green[3], line.blue[3]), (255, 127, 63));
        let top = &pic.lines[0];
        assert_ne!((top.red[0], top.green[0], top.blue[0]), (255, 127, 63));
        drop(r);
        assert!(m.fault().is_none(), "{:?}", m.fault());
        // what SI_materials made of the material is in its record: the working diffuse
        // colour, the flag word (2: normals interpolated), no 2D textures; and the node's
        // flag word has the bit every model gets
        assert_eq!([m.f32(mat + 0x720), m.f32(mat + 0x724), m.f32(mat + 0x728)], [1.0, 0.5, 0.25]);
        assert_eq!(m.u32(mat + 0x6e0), 2);
        assert_eq!(m.i16(mat + 0x6e8), 0);
        assert_eq!(m.u16(node + model::TXT_FLAGS), 4);
    }

    /// lic2: the licence word of `SI_image`. With the granted stand-in the frame is rendered and
    /// the word is 0 afterwards. With the client refusing every checkout (a frame loop's
    /// licence question answered by `get_render_license_with`), the loop renders nothing,
    /// `SI_image` leaves through `longjmp` after the frame, and on the jump path the
    /// interactive program shows the dialogue once and clears the word (answer 0x7f); the
    /// stand-alone renderer shows no dialogue and leaves the word set.
    #[test]
    fn a_refused_licence_stops_the_frame_and_the_interactive_program_says_so() {
        si_rt::skip_without_install!();
        struct Refused<'a, 'b>(&'a mut Render<'b>);
        impl Outside for Refused<'_, '_> {
            fn call(&mut self, m: &mut Mem, f: u32, args: &[u32]) -> u32 {
                if f == f::SI_SAMPLE || f == f::SI_SUPERSAMPLE {
                    if crate::licence::get_render_license_with(m, self, &mut || 1) != 0 {
                        unreachable!();
                    }
                    return 0;
                }
                self.0.call(m, f, args)
            }
            fn jumped(&mut self) -> bool {
                self.0.jumped()
            }
        }
        use crate::licence::SI_RENDERER_LICENSE_FAILED as WORD;
        let (mut m, camera, node, _) = square_scene();
        m.set_u16(WORD, 1);
        let (mut rest, mut files) = (Rest(Vec::new()), NoPictures);
        let mut r = Render::new(&mut rest, &mut files);
        assert_eq!(crate::rnd_main::image(&mut m, &mut r, camera, 0, 0, 0, node), 0, "{:?}", r.notes);
        assert_eq!(r.pictures[0].lines.len(), 4);
        drop(r);
        assert_eq!(m.u16(WORD), 0);
        assert!(!rest.0.contains(&f::SI_DIALOG_REQUEST));
        for standalone in [0i16, 1] {
            let (mut m, camera, node, _) = square_scene();
            crate::data::place_texts(&mut m);
            m.set_i16(g::SI_STANDALONE_RENDERER, standalone);
            let (mut rest, mut files) = (Rest(Vec::new()), NoPictures);
            let mut r = Render::new(&mut rest, &mut files);
            let status = crate::rnd_main::image(&mut m, &mut Refused(&mut r), camera, 0, 0, 0, node);
            assert_eq!(status, crate::rnd_main::STATUS_ABORTED);
            assert!(r.pictures.iter().all(|p| p.lines.is_empty()), "nothing is sampled");
            let notes = r.notes.clone();
            drop(r);
            let dialogs = rest.0.iter().filter(|c| **c == f::SI_DIALOG_REQUEST).count();
            assert_eq!(dialogs, (standalone == 0) as usize, "{notes:?}");
            assert_eq!(m.u16(WORD), standalone as u16);
            // the frame loop's polls (interactive) or its minutes of waiting (stand-alone)
            let sleeps = rest.0.iter().filter(|c| **c == f::SLEEP).count();
            assert_eq!(sleeps, if standalone == 0 { 60 * 12 } else { 60 });
            assert!(m.fault().is_none(), "{:?}", m.fault());
        }
    }

    /// With the dither on (0x08790e30, no noise) `SI_image` calls `SI_fsDitherInit` after the
    /// update: its globals hold the rendered region's width and the switch, its two rows are taken
    /// (the second cleared), and the frame's lines go through the error diffusion: the
    /// square's green of 0.5 × 255 = 127.5 comes out as 127 and 128, alpha as without.
    #[test]
    fn the_dither_diffuses_the_error_of_each_line() {
        si_rt::skip_without_install!();
        let render = |dither: bool| {
            let (mut m, camera, node, _) = square_scene();
            m.set_i32(g::DITHER, dither as i32);
            m.set_i16(g::DITHER_ARG, 0);
            let (mut rest, mut files) = (Rest(Vec::new()), NoPictures);
            let mut r = Render::new(&mut rest, &mut files);
            assert_eq!(crate::rnd_main::image(&mut m, &mut r, camera, 0, 0, 0, node), 0, "{:?}", r.notes);
            let lines = r.pictures[0].lines.clone();
            drop(r);
            (m, lines)
        };
        let (_, plain) = render(false);
        let (m, dithered) = render(true);
        // the width is the rendered region's, first to last pixel column
        let width = m.i32(g::REGION_PX_X_MAX) - m.i32(g::REGION_PX_X_MIN) + 1;
        assert!(width > 0);
        assert_eq!((m.i32(0x086f_ade8), m.i32(0x086f_adec)), (width, 0));
        let second = m.u32(0x086f_ade0);
        assert!(second != 0 && m.u32(0x086f_ade4) != 0 && m.bytes(second, 12 * width as u32).iter().all(|&b| b == 0));
        let greens: Vec<u8> = dithered[1..3].iter().flat_map(|l| l.green[2..4].to_vec()).collect();
        assert!(greens.contains(&127) && greens.contains(&128), "{greens:?}");
        assert_eq!(plain[1].green[2], 127);
        for (a, b) in plain.iter().zip(&dithered) {
            assert_eq!((a.y, &a.alpha), (b.y, &b.alpha));
        }
    }

    /// A scene without a light renders with a copy of `SI_defaultLight` (initData), and
    /// `SI_update` asks the program for the scene at the frame's time and the elements' graph
    /// information before it reads the scene (finding 0012, section 21).
    #[test]
    fn a_scene_without_a_light_gets_the_default_light() {
        si_rt::skip_without_install!();
        let (mut m, camera, node, _) = square_scene();
        for (o, v) in [(0x24, 0.5f32), (0x28, 0.25), (0x2c, 0.125), (0x34, 100.0), (0x38, 100.0), (0x3c, 100.0)] {
            m.set_f32(SI_DEFAULT_LIGHT + o, v);
        }
        m.set_i16(SI_DEFAULT_LIGHT + 0x1e, 1);
        m.set_i16(SI_DEFAULT_LIGHT + 0x176, 1);
        m.set_i16(g::SEQUENCE, 2);
        let (mut rest, mut files) = (Rest(Vec::new()), NoPictures);
        let mut r = Render::new(&mut rest, &mut files);
        let status = crate::rnd_main::image(&mut m, &mut r, camera, 0, 0, 0, node);
        assert_eq!(status, 0, "{:?}", r.notes);
        assert_eq!(r.p.st.lights.len(), 1);
        let l = &r.p.st.lights[0];
        assert_eq!((l.kind, l.color[0], l.color[1], l.color[2], l.on), (1, 0.5, 0.25, 0.125, 1));
        assert_eq!((l.pos_given.x, l.pos_given.y, l.pos_given.z), (100.0, 100.0, 100.0));
        drop(r);
        // the chain's head is the copy, in the arena or a block of the image
        let head = m.u32(g::DB_LIGHTS);
        assert!(head != 0 && head != SI_DEFAULT_LIGHT);
        assert_eq!(m.f32(head + 0x28), 0.25);
        // the two calls, in the original's order
        let at = |f: u32| rest.0.iter().position(|&c| c == f);
        let (a, b) = (at(f::SI_SCENE_UPDATE_ITEMS).expect("SI_sceneUpdateItems"), at(f::SI_ED_GET_ELEMENTS_GRAF_NODE_INFO).expect("SI_edGetElementsGrafNodeInfo"));
        assert!(a < b);
    }

    /// The square again, with the machine's files and the arena in the image: the frame
    /// ends as a picture file on disk (under `re/out/si-render/p10/disk`, which is not
    /// tracked), the project's reader gives back the lines that were rendered, and the
    /// chunk memory the frame used was the image's.
    #[test]
    fn a_frame_ends_as_a_file_on_disk() {
        si_rt::skip_without_install!();
        use crate::host::DiskFiles;
        let (mut m, camera, node, mat) = square_scene();
        crate::data::init(&mut m);
        let arena = crate::mem_chunk_image::ren_chunk(&mut m);
        // a jitter amplitude on the material: its table goes to the arena
        m.set_f32(mat + 0x6ac, 0.5);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../re/out/si-render/p10/disk");
        std::fs::create_dir_all(root.join("tmp")).expect("the scratch directory can be made");
        let _ = std::fs::remove_file(root.join("tmp/shot.1.pic"));
        let mut disk = DiskFiles::under(&root);
        let (mut rest, mut files) = (Rest(Vec::new()), NoPictures);
        let mut r = Render::new(&mut rest, &mut files).with_host(&mut disk);
        let status = crate::rnd_main::image(&mut m, &mut r, camera, 0, 0, 0, node);
        assert_eq!(status, 0, "{:?}", r.notes);
        let pic = r.pictures[0].clone();
        assert_eq!((pic.name.as_slice(), pic.written, pic.closed), (&b"/tmp/shot.1.pic"[..], 4, true));
        let notes = r.notes.clone();
        drop(r);
        assert!(m.fault().is_none(), "{:?}", m.fault());
        assert_eq!(m.u32(g::P_FILE), 0);
        // the file: complete (the mark `C`), with the icon of a sequence's first frame
        let data = std::fs::read(root.join("tmp/shot.1.pic")).expect("the picture is on disk");
        assert_eq!((&data[..4], data[103]), (&[0x53, 0x80, 0xf6, 0x34][..], 0x43));
        assert!(si_picture::icon_offset(&data).is_some(), "{notes:?}");
        // read back by the project's reader: the size, and every line as it was rendered
        let read = si_picture::Picture::read(&data).expect("the reader takes the picture");
        assert_eq!((read.picture.width, read.picture.height), (6, 4));
        for (k, line) in pic.lines.iter().enumerate() {
            assert_eq!(line.y, 3 - k as i32);
            for (plane, want) in [&line.red, &line.green, &line.blue, &line.alpha].into_iter().enumerate() {
                assert_eq!(&read.picture.planes[plane].as_ref().unwrap()[6 * k..6 * k + 6], &want[..], "line {k}, plane {plane}");
            }
        }
        // the arena: `SI_image` freed its block at the end; the outgoing lines and the
        // material's jitter table lay in it
        assert_eq!(m.u32(arena + crate::mem_chunk_image::off::SLOTS + 8), 0);
        let block_of = |m: &Mem, p: u32| m.heap_block_of(p).map(|b| m.heap_blocks()[b.0]);
        let block = block_of(&m, m.u32(g::OUT_RED)).expect("the red line lies in a heap block");
        assert!(m.is_freed(block.0) && block.1 == 5_000_000);
        let table = m.u32(mat + 0x6b0);
        assert_eq!(block_of(&m, table), Some(block));
        assert_eq!(block_of(&m, m.u32(table)), Some(block));
        assert_eq!(m.u16(mat + 0x6b4), 0);
    }

    /// The same frame into files in memory: the picture's bytes are the lines through the
    /// picture writer on values, and a caller that does not want the lines kept gets none.
    #[test]
    fn the_picture_file_is_the_lines_run_coded() {
        si_rt::skip_without_install!();
        use crate::host::MemFiles;
        let (mut m, camera, node, _) = square_scene();
        crate::data::init(&mut m);
        let mut mem = MemFiles::new();
        mem.files.insert(b"/tmp/".to_vec(), Vec::new());
        let (mut rest, mut files) = (Rest(Vec::new()), NoPictures);
        let mut r = Render::new(&mut rest, &mut files).with_host(&mut mem);
        assert_eq!(crate::rnd_main::image(&mut m, &mut r, camera, 0, 0, 0, node), 0);
        let lines = r.pictures[0].lines.clone();
        drop(r);
        let file = mem.files[&b"/tmp/shot.1.pic".to_vec()].clone();
        let mut want = Vec::new();
        for l in &lines {
            crate::rle::write_chanal_mixed(&mut want, 6, 3, [Some(&l.red), Some(&l.green), Some(&l.blue), None]);
            crate::rle::write_chanal_mixed(&mut want, 6, 1, [Some(&l.alpha), None, None, None]);
        }
        assert_eq!(&file[112..112 + want.len()], &want[..]);
        // the first frame's icon follows the lines
        assert!(file.len() > 112 + want.len());
        // without the lines kept
        let (mut m, camera, node, _) = square_scene();
        crate::data::init(&mut m);
        let mut mem2 = MemFiles::new();
        mem2.files.insert(b"/tmp/".to_vec(), Vec::new());
        let (mut rest, mut files) = (Rest(Vec::new()), NoPictures);
        let mut r = Render::new(&mut rest, &mut files).with_host(&mut mem2);
        r.keep_lines = false;
        let taken = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = taken.clone();
        r.on_line = Some(Box::new(move |name, line| sink.borrow_mut().push((name.to_vec(), line.clone()))));
        assert_eq!(crate::rnd_main::image(&mut m, &mut r, camera, 0, 0, 0, node), 0);
        assert_eq!((r.pictures[0].lines.len(), r.pictures[0].written), (0, 4));
        drop(r);
        // the lines came as they were written, with the picture's name
        assert_eq!(taken.borrow().iter().map(|t| t.1.clone()).collect::<Vec<_>>(), lines);
        assert!(taken.borrow().iter().all(|t| t.0 == b"/tmp/shot.1.pic"));
        assert_eq!(mem2.files[&b"/tmp/shot.1.pic".to_vec()], file);
    }
}
