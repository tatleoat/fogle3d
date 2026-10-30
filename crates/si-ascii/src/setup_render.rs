// SPDX-License-Identifier: MIT
//! The render and preview sections of a setup file: `RENDERING_TYPE` … `DITHERING`,
//! `GLOBAL_SWITCHES`, `PREVIEW` (reader in `asceneio.c`, writer in `setup.c`; see the parent
//! module), and the value readers they use, which also accept a function curve in place of a
//! number.

use super::layout::{shade_setup_read, shade_setup_write};
use super::{shade_setup_glb_read, shade_setup_glb_write, Out, Reader, Setup, SetupError};

/// The picture formats, by the number `PREFERENCE_PICT_FMT` gives and `FORMAT_TYPE` names.
pub const FORMAT_NAMES: [&str; 15] = [
    "35MM_137", "35MM_166", "35MM_185", "70MM_PANAVISION", "70MM_IMAX", "70MM_OMNIMAX", "NTSC", "PAL", "HDTV",
    "SLIDE_35MM", "6CM_X_6CM", "6CM_X_9CM", "4IN_X_5IN", "CUSTOM_ASPECT", "CUSTOM_SIZE",
];
const FORMAT_CUSTOM_ASPECT: usize = 13;
const FORMAT_CUSTOM_SIZE: usize = 14;

/// The rendering types, by number. (`SUBREGION` is read too: as `RENDERING_LINE` with the
/// subregion switched on.)
pub const RENDER_TYPE_NAMES: [&str; 11] = [
    "WIRE_FRAME", "HIDDEN_LINE", "HIDDEN_LINE_FCT", "DEPTHCUE", "RENDERING_LINE", "HARDWARE_LINE", "GHOST",
    "ROTO_WIRE", "ROTO_SHADE", "MENTAL_RAY", "MENTAL_RAY_2X",
];

/// A mental ray shader named in the set-up (`SI_udf…`, 0x3c bytes; the fields the file sets).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Udf {
    /// +0x00 `UDF_NAME`
    pub name: Option<Vec<u8>>,
    /// +0x0c `UDF_ACTIVE`
    pub active: i16,
    /// +0x10 `UDF_INTERNAL`
    pub internal: i16,
    /// +0x14 `UDF_ORDER`: place in the list of output or lens shaders, counted from 1.
    pub order: i32,
    /// +0x18: the kind of shader. Nothing in the setup reader sets it (a new record has 0),
    /// and the writer prints `UDF_ORDER` only for kinds 6 and 10: the order of a shader read
    /// from a setup file is not written back.
    pub kind: i32,
}

/// The mental ray part of the render control (`SI_MentalRay`, at +0x2a0 of the control; the
/// fields a setup file carries, offsets within the part).
#[derive(Clone, Debug, PartialEq)]
pub struct MentalRay {
    /// +0x0c `MR_SHUTTER`
    pub shutter: f32,
    /// +0x18 `MR_MAX_SAMPLE`, +0x1c `MR_MIN_SAMPLE`
    pub max_sample: i32,
    pub min_sample: i32,
    /// +0x20 `MR_THREADS`
    pub threads: i32,
    /// +0x24 `MR_NICE`
    pub nice: i32,
    /// +0x28 `MR_TRACER`, +0x2a `MR_SHADOW`
    pub tracer: i16,
    pub shadow: i16,
    /// +0x2c `MR_FACE`: 1 front, 2 back, 3 both.
    pub face: i32,
    /// +0x138 `MR_FILE_OUTPUT`, +0x13a `MR_FILE_OUTPUT_PER_FRAME`
    pub file_output: i16,
    pub file_output_per_frame: i16,
    /// +0x13c `MR_CONTOUR`, +0x13e `MR_ZPIC`, +0x140 `MR_NORMAL`, +0x142 `MR_VERBOSE`
    pub contour: i16,
    pub zpic: i16,
    pub normal: i16,
    pub verbose: i16,
    /// +0x144 `MR_OUTPUT_NUM` and +0x14c, the output shaders (`MR_RENDER_SHADER`), each at
    /// the place its `UDF_ORDER` says.
    pub output_num: i32,
    pub output_shaders: Vec<Option<Udf>>,
    /// +0x148 `MR_LENS_NUM` and +0x150, the lens shaders (`MR_LENS_SHADER`).
    pub lens_num: i32,
    pub lens_shaders: Vec<Option<Udf>>,
    /// +0x154 `MR_RAY_REFLECTED`, +0x158 `MR_RAY_REFRACTED`, +0x15c `MR_RAY_SHADOW`
    pub ray_reflected: i32,
    pub ray_refracted: i32,
    pub ray_shadow: i32,
    /// +0x160 `MR_ACCEL_METHOD`, +0x164 `MR_ACCEL_METHOD_2X`
    pub accel_method: i32,
    pub accel_method_2x: i32,
    /// +0x168 `MR_PRIMARY_RAY`, +0x16c `MR_SECONDARY_RAY`
    pub primary_ray: i32,
    pub secondary_ray: i32,
    /// +0x170 `MR_MEMORY_LIMIT`
    pub memory_limit: i32,
    /// +0x174 `MR_BSPDEPTH`, +0x178 `MR_BSPSIZE`, +0x17c `MR_BSPDEPTH_2X`, +0x180 `MR_BSPSIZE_2X`
    pub bsp_depth: i32,
    pub bsp_size: i32,
    pub bsp_depth_2x: i32,
    pub bsp_size_2x: i32,
    /// +0x184 `MR_GAMMA`
    pub gamma: f32,
    /// +0x188 `MR_DITHERING`
    pub dithering: i32,
    /// +0x18c `MR_DESATURATE`, +0x18e `MR_ADAPTIVE`
    pub desaturate: i16,
    pub adaptive: i16,
    /// +0x190 `MR_THRESHOLD`
    pub threshold: [f32; 3],
    /// +0x1a0 `MR_FILTER_WIDTH`, +0x1a4 `MR_FILTER_HEIGHT`, +0x1a8 `MR_FILTER_TYPE`
    pub filter_width: f32,
    pub filter_height: f32,
    pub filter_type: i32,
    /// +0x1ac `MR_ATMOS_SHADER`
    pub atmos_shader: Option<Udf>,
    /// +0x1b0 `MR_REFRACTION_REAL`, +0x1b4 `MR_REFRACTION_IMAGINARY`
    pub refraction_real: f32,
    pub refraction_imaginary: f32,
    /// +0x1b8 `MR_TRANSMIT`
    pub transmit: [f32; 3],
    /// +0x1ec `MR_FILE_OUTPUT_TYPE`
    pub file_output_type: i32,
    /// +0x1f0: the `MR_LINK_STRING` lines, joined with newlines. Reading a render section
    /// starts by emptying it.
    pub link_string: Option<Vec<u8>>,
    /// +0x1f4 `MR_FILENAME`
    pub filename: Option<Vec<u8>>,
    /// +0x1f8 `MR_ANTIALIAS`, +0x1fa `MR_MOTION_BLUR`
    pub antialias: i16,
    pub motion_blur: i16,
    /// +0x1fc `MR_BLUR_TYPE`: 0 3D, 1 2D, 2 motion vectors only.
    pub blur_type: i16,
    /// +0x1fe `MR_BLUR2D_ALGO`: 0 uniform, 1 linear, 2 gaussian.
    pub blur2d_algo: i16,
    /// +0x200 `MR_BLUR_SAMPLES`
    pub blur_samples: i32,
    /// +0x204 `MR_CONTLINEW`, +0x208 `MR_CONTDEPTH`
    pub contour_line_width: f32,
    pub contour_depth: f32,
    /// +0x20c `MR_TIME_CONTRAST`
    pub time_contrast: [f32; 4],
    /// +0x21c `MR_GRIDSIZE`
    pub grid_size: f32,
    /// +0x220 … +0x232: `MR_CAUSTIC_ENABLE`, `MR_GLOBILLUM_ENABLE`, `MR_PRW_CAUSTIC_ENABLE`,
    /// `MR_PRW_GLOBILLUM_ENABLE`, `MR_FINAL_GATHER_ENABLE`, `MR_PH_MAP_RBLD_ENABLE`,
    /// `MR_USE_CS_ACC_RAD`, `MR_USE_GI_ACC_RAD`, `MR_USE_FG_RAD_MIN`, `MR_USE_FG_RAD_MAX`.
    pub photon_flags: [i16; 10],
    /// +0x234 `MR_PH_MAP_FILE`
    pub ph_map_file: Option<Vec<u8>>,
    /// +0x238 `MR_CS_FILTER_TYPE`
    pub cs_filter_type: i16,
    /// +0x23c … +0x250: `MR_CS_ACC`, `MR_GI_ACC`, `MR_FG_ACC`, `MR_PH_TRD_REFLECT`,
    /// `MR_PH_TRD_REFRACT`, `MR_PH_TRD_MAX_SUM`. Ints, but written with `%.7g`.
    pub photon_ints: [i32; 6],
    /// +0x254 … +0x264: `MR_CS_FILTER`, `MR_CS_ACC_RAD`, `MR_GI_ACC_RAD`, `MR_FG_RAD_MIN`,
    /// `MR_FG_RAD_MAX`.
    pub photon_reals: [f32; 5],
}

const PHOTON_FLAG_NAMES: [&str; 10] = [
    "MR_CAUSTIC_ENABLE", "MR_GLOBILLUM_ENABLE", "MR_PRW_CAUSTIC_ENABLE", "MR_PRW_GLOBILLUM_ENABLE",
    "MR_FINAL_GATHER_ENABLE", "MR_PH_MAP_RBLD_ENABLE", "MR_USE_CS_ACC_RAD", "MR_USE_GI_ACC_RAD", "MR_USE_FG_RAD_MIN",
    "MR_USE_FG_RAD_MAX",
];
const PHOTON_INT_NAMES: [&str; 6] =
    ["MR_CS_ACC", "MR_GI_ACC", "MR_FG_ACC", "MR_PH_TRD_REFLECT", "MR_PH_TRD_REFRACT", "MR_PH_TRD_MAX_SUM"];
const PHOTON_REAL_NAMES: [&str; 5] = ["MR_CS_FILTER", "MR_CS_ACC_RAD", "MR_GI_ACC_RAD", "MR_FG_RAD_MIN", "MR_FG_RAD_MAX"];

impl Default for MentalRay {
    /// `SI_mentalRayInit` (0x08291808, in part), the fields above.
    fn default() -> Self {
        MentalRay {
            shutter: 0.7,
            max_sample: 1,
            min_sample: -1,
            threads: 1,
            nice: 10,
            tracer: 1,
            shadow: 1,
            face: 3,
            file_output: 0,
            file_output_per_frame: 0,
            contour: 0,
            zpic: 0,
            normal: 0,
            verbose: 0,
            output_num: 0,
            output_shaders: Vec::new(),
            lens_num: 0,
            lens_shaders: Vec::new(),
            ray_reflected: 1,
            ray_refracted: 2,
            ray_shadow: 2,
            accel_method: 1,
            accel_method_2x: 1,
            primary_ray: 0,
            secondary_ray: 0,
            memory_limit: 10,
            bsp_depth: 40,
            bsp_size: 6,
            bsp_depth_2x: 40,
            bsp_size_2x: 6,
            gamma: 1.0,
            dithering: 0,
            desaturate: 0,
            adaptive: 1,
            threshold: [0.1, 0.1, 0.1],
            filter_width: 2.0,
            filter_height: 2.0,
            filter_type: 1,
            atmos_shader: None,
            refraction_real: 1.0,
            refraction_imaginary: 1.0,
            transmit: [1.0, 1.0, 1.0],
            file_output_type: 0,
            link_string: None,
            filename: Some(b"out.mi".to_vec()),
            antialias: 1,
            motion_blur: 0,
            blur_type: 0,
            blur2d_algo: 1,
            blur_samples: 1,
            contour_line_width: 0.5,
            contour_depth: 0.0,
            time_contrast: [0.1, 0.1, 0.1, 0.1],
            grid_size: 1.0,
            photon_flags: [0, 0, 0, 0, 0, 1, 0, 0, 0, 0],
            ph_map_file: Some(Vec::new()),
            cs_filter_type: 0,
            photon_ints: [100, 100, 100, 2, 2, 2],
            photon_reals: [1.1, 0.0, 0.0, 0.0, 0.0],
        }
    }
}

/// The render control (`SI_RenControl`; the fields a setup file carries).
#[derive(Clone, Debug, PartialEq)]
pub struct RenderControl {
    /// +0xfa `RENDERING_TYPE`, an index into [`RENDER_TYPE_NAMES`].
    pub rendering_type: i16,
    /// +0xfe: the render section had a `PICTURE` block; the writer writes one if this is set.
    pub has_picture: i16,
    /// +0x100 `FADE`, +0x104 `FADE_START`, +0x10c `FADE_END`, +0x118 `FADE_COLOUR`,
    /// +0x130 `FADE_TRANSPARENCY`
    pub fade: i16,
    pub fade_start: f32,
    pub fade_end: f32,
    pub fade_colour: [f32; 3],
    pub fade_transparency: f32,
    /// +0x13c `ON_FIELD`, +0x13e `FIRST_FIELD`
    pub on_field: i16,
    pub first_field: i16,
    /// +0x154 `PIXEL_RATIO`
    pub pixel_ratio: f32,
    /// +0x158 `FILTER`, which is also the first number after `ANTIALIAS`.
    pub filter: i16,
    /// +0x15a `ANTIALIAS`: 0 `FIXED`, 1 `ADAPTIVE`. The line is written only for these two.
    pub antialias: i16,
    /// +0x15c, +0x16c: the second number and the real after `ANTIALIAS` (a level and a
    /// threshold, by their values; not read from code).
    pub antialias_level: i16,
    pub antialias_threshold: f32,
    /// +0x178 `FRAME_BUFFER`
    pub frame_buffer: i16,
    /// +0x17c `OUTPUT_FILE`, +0x180 `PRE_RENDERING`, +0x184 `POST_RENDERING`
    pub output_file: Option<Vec<u8>>,
    pub pre_rendering: Option<Vec<u8>>,
    pub post_rendering: Option<Vec<u8>>,
    /// +0x1da `MAX_TRIA_DEPTH`, +0x1dc `MAX_TREE_DEPTH`, +0x1e4 `MAX_RAY_DEPTH`
    pub max_tria_depth: i16,
    pub max_tree_depth: i16,
    pub max_ray_depth: i16,
    /// +0x1e6 `TIME_STATISTICS`, +0x1e8 `TREE_STATISTICS`, +0x1ea `MEMORY_STATISTICS`,
    /// +0x1ec `LINE_STATISTICS`
    pub time_statistics: i16,
    pub tree_statistics: i16,
    pub memory_statistics: i16,
    pub line_statistics: i16,
    /// +0x1f4 `RENDERING_FRAME`: first, last, step.
    pub frames: [i16; 3],
    /// +0x1fc `AMBIENCE`
    pub ambience: [f32; 3],
    /// +0x20c `BACK_CULLING`
    pub back_culling: i16,
    /// +0x210 `MOTION_BLUR`, +0x214 `SHUTTER_SPEED`, +0x21c `MIN_MOVEMENT`
    pub motion_blur: i16,
    pub shutter_speed: f32,
    pub min_movement: f32,
    /// +0x226: the subregion is on. `SUBFRAME` gives it; `SUBREGION_FRAME` and the rendering
    /// type `SUBREGION` raise it.
    pub subregion: i16,
    /// +0x22a `DRAWSUB`, `DRAW_SUBR_ON_RENDER`
    pub draw_subregion: i16,
    /// +0x22c `SUBREGION_FRAME`: four reals (+0x22c … +0x238) and two shorts (+0x23c, +0x23e).
    pub subregion_frame: [f32; 4],
    pub subregion_size: [i16; 2],
    /// +0x240 `TAG_FILE`
    pub tag_file: i16,
    /// +0x24c `Z_CHANNEL`
    pub z_channel: i16,
    /// +0x250 `DITHERING`: two ints and a real.
    pub dithering: [i32; 2],
    pub dithering_value: f32,
    /// +0x25c `FOG`, +0x260 `FOG_DENSITY`, +0x264 `FOG_BASE`, +0x268 `FOG_THICKNESS`,
    /// +0x26c `FOG_START`, +0x270 `FOG_END`, +0x284 `FOG_COLOUR`
    pub fog: i16,
    pub fog_density: f32,
    pub fog_base: f32,
    pub fog_thickness: f32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_colour: [f32; 3],
    /// +0x2a0
    pub mental_ray: MentalRay,
    /// `GLOBAL_SWITCHES`, by index of [`GLOBAL_SWITCH_NAMES`] (+0x584 … +0x5aa).
    pub switches: [i16; 13],
    /// +0x59a `RESOLUTION`
    pub resolution: [i16; 2],
}

/// The keywords of `GLOBAL_SWITCHES` in the order written, with the offset of each in the
/// render control.
pub const GLOBAL_SWITCH_NAMES: [(&str, u16); 13] = [
    ("GLG_MATERIAL", 0x584), ("GLG_2D_TXT", 0x586), ("GLG_3D_TXT", 0x588), ("GLG_REFLECT", 0x58a),
    ("GLG_REFRACT", 0x58c), ("GLG_SHADOW", 0x58e), ("MR_AREA_LIGHT", 0x59e), ("MR_DISPLACEMENT", 0x5a4),
    ("MR_VOLUME", 0x5aa), ("MR_LENS", 0x5a6), ("MR_OUTPUT", 0x5a8), ("MR_SHADOW_MAP", 0x5a0),
    ("MR_MB_SHADOW_MAP", 0x5a2),
];

impl Default for RenderControl {
    /// `SI_renControlInit` (0x082912e4, in part) with the fade and fog records it shares
    /// `SI_fadeInit` (0x08291298, in part) `SI_fogInit` (0x0829122c, in part); the fields above.
    fn default() -> Self {
        RenderControl {
            rendering_type: 4,
            has_picture: 1,
            fade: 0,
            fade_start: 20.0,
            fade_end: 100.0,
            fade_colour: [0.0; 3],
            fade_transparency: 0.0,
            on_field: 0,
            first_field: 0,
            pixel_ratio: 1.0,
            filter: 2,
            antialias: 1,
            antialias_level: 2,
            antialias_threshold: 0.05,
            frame_buffer: 1,
            output_file: Some(b"default".to_vec()),
            pre_rendering: Some(Vec::new()),
            post_rendering: Some(Vec::new()),
            max_tria_depth: 10,
            max_tree_depth: 30,
            max_ray_depth: 1,
            time_statistics: 0,
            tree_statistics: 0,
            memory_statistics: 0,
            line_statistics: 0,
            frames: [1, 100, 1],
            ambience: [0.3, 0.3, 0.3],
            back_culling: 1,
            motion_blur: 0,
            shutter_speed: 1.0,
            min_movement: 5.0,
            subregion: 0,
            draw_subregion: 1,
            subregion_frame: [0.2; 4],
            subregion_size: [0, 0],
            tag_file: 0,
            z_channel: 0,
            dithering: [0, 0],
            dithering_value: 10.0,
            fog: 0,
            fog_density: 0.2,
            fog_base: -1.0,
            fog_thickness: 5.0,
            fog_start: 20.0,
            fog_end: 10000.0,
            fog_colour: [0.7, 0.7, 0.7],
            mental_ray: MentalRay::default(),
            switches: [1; 13],
            resolution: [500, 424],
        }
    }
}

/// `PREVIEW` (the record at 0x08781d00, which is +0x500 of the render control; offsets
/// below are within the record) and the renderer the preview uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    /// The two numbers after `PREVIEW` (+0x1e, +0x20): the size of the preview picture.
    pub size: [i16; 2],
    /// `SI_prevRenderer` (0x0869eef8): 1 `PREVIEW_TYPE_SI`, 2 `PREVIEW_TYPE_MR`, 3
    /// `PREVIEW_TYPE_MR_2X`. The rendering types `MENTAL_RAY` and `MENTAL_RAY_2X` set it too.
    pub renderer: i16,
    /// +0x08 … +0x12: `PRW_MATERIAL`, `PRW_2D_TXT`, `PRW_3D_TXT`, `PRW_REFLECT`,
    /// `PRW_REFRACT`, `PRW_SHADOW`.
    pub switches: [i16; 6],
    /// +0x22 `PRW_AREA_LIGHT`, +0x24 `PRW_SHADOW_MAP`, +0x28 `PRW_DISPLACEMENT`,
    /// +0x2a `PRW_LENS`, +0x2c `PRW_OUTPUT`, +0x2e `PRW_VOLUME`
    pub area_light: i16,
    pub shadow_map: i16,
    pub displacement: i16,
    pub lens: i16,
    pub output: i16,
    pub volume: i16,
    /// +0x30 `XY_PIXEL_RATIO`
    pub xy_pixel_ratio: f32,
    /// +0x34 `PREVIEW_IN_WINDOW`
    pub in_window: i16,
    /// +0x3c `SI_ACTIVE`
    pub si_active: i16,
    /// +0x40 `SAMPLE_TYPE`, +0x44 `SAMPLE_SIZE`, +0x48 `MAX_FILTER`
    pub sample_type: i32,
    pub sample_size: i32,
    pub max_filter: i32,
    /// +0x4c `DIV_THRESHOLD`
    pub div_threshold: [f32; 4],
    /// +0x5c `MR_ACTIVE`, +0x5e `ADAPTIVE`
    pub mr_active: i16,
    pub adaptive: i16,
    /// +0x60 `RGB_THRESHOLD`
    pub rgb_threshold: [f32; 4],
    /// +0x70 `FILTER_TYPE`, +0x74 `WIDTH`, +0x78 `HEIGHT`
    pub filter_type: i32,
    pub width: f32,
    pub height: f32,
    /// +0x7c `MIN_SAMPLES`, +0x80 `MAX_SAMPLES`
    pub min_samples: i32,
    pub max_samples: i32,
}

const PREVIEW_SWITCH_NAMES: [&str; 6] =
    ["PRW_MATERIAL", "PRW_2D_TXT", "PRW_3D_TXT", "PRW_REFLECT", "PRW_REFRACT", "PRW_SHADOW"];

impl Default for Preview {
    /// The preview record is the part of the render control from +0x500 on, and starts as the
    /// control's initialiser leaves it `SI_renControlInit` (0x082912e4, in part) (the fields
    /// above; the first three `DIV_THRESHOLD` values and the pixel ratio are not set there
    /// and stay zero). The preview renderer is a global of its own with the static initial
    /// value 1.
    fn default() -> Self {
        Preview {
            size: [500, 424],
            renderer: 1,
            switches: [1; 6],
            area_light: 1,
            shadow_map: 1,
            displacement: 1,
            lens: 1,
            output: 1,
            volume: 1,
            xy_pixel_ratio: 0.0,
            in_window: 0,
            si_active: 0,
            sample_type: 1,
            sample_size: 2,
            max_filter: 2,
            div_threshold: [0.0, 0.0, 0.0, 0.05],
            mr_active: 0,
            adaptive: 1,
            rgb_threshold: [0.2; 4],
            filter_type: 1,
            width: 2.0,
            height: 2.0,
            min_samples: -2,
            max_samples: 0,
        }
    }
}

/// A key of a function curve (36 bytes; the fields the key reader fills).
#[derive(Clone, Debug, PartialEq)]
pub struct FcurveKey {
    /// +0x00 `TIME`, +0x04 `VALUE`
    pub time: f32,
    pub value: f32,
    /// +0x08 `RIGHT_SLOPE`, +0x18 `LEFT_SLOPE`
    pub right_slope: f32,
    pub left_slope: f32,
    /// +0x0c `FLAG`
    pub flag: i16,
    /// +0x0e `INTERPOLATION`: 0 constant, 1 linear, 2 spline. The original leaves it as the
    /// allocator returned it when the key has no such line; here that is 0.
    pub interpolation: i16,
    /// +0x1c `KEYTYPE`: 0 free form, 1 key frame, 2 key path, 3 raw data.
    pub key_type: i16,
    /// +0x1e `SPLINE_INDEX`
    pub spline_index: i16,
}

impl Default for FcurveKey {
    /// `SI_fkeyInitialize` (0x0820bc50, in part), the fields above.
    fn default() -> Self {
        FcurveKey { time: 0.0, value: 0.0, right_slope: 0.0, left_slope: 0.0, flag: 0, interpolation: 0, key_type: 0, spline_index: -1 }
    }
}

/// A function curve as the value readers parse it (the fields they fill).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fcurve {
    /// +0x18 `PRE_EXTRAPOLATION`, +0x1a `POST_EXTRAPOLATION`
    pub pre_extrapolation: i16,
    pub post_extrapolation: i16,
    /// +0x1c `FLAGS`
    pub flags: i16,
    /// +0x1e `INTERPOLATION`. A new curve of no particular kind starts with the preferred
    /// interpolation, `PREF_FCV_INTERP_TYPE` `SI_fcurveAllocate` (0x0820a810, in part)
    /// `SI_fcurveInitialize` (0x0820a9d0, in part).
    pub interpolation: i16,
    /// +0x08, +0x0c `KEYS`
    pub keys: Vec<FcurveKey>,
}

// ------------------------------------------------------------------------------------------
// values that may be function curves
// ------------------------------------------------------------------------------------------

/// A key of a function curve [soft 0x083dd170 fcvkeyAsciiRead]: keyword and value pairs from
/// the word in the token buffer on. `FLAG` ends the key (the next token is read and the
/// function returns) if the file version is at most 2.02; the version looked at is a global
/// of this file's own (0x08818150) that nothing ever sets, so it is 0 and `FLAG` always ends
/// the key: `SPLINE_INDEX` and `KEYTYPE` after it are never reached. `KEYTYPE` ends the key
/// too; so does any word that is none of the keywords (without reading on). False if the
/// file ends first.
fn fcvkey_read(r: &mut Reader, key: &mut FcurveKey) -> bool {
    *key = FcurveKey::default();
    loop {
        if r.is("TIME") {
            key.time = r.float();
        } else if r.is("VALUE") {
            key.value = r.float();
        } else if r.is("LEFT_SLOPE") {
            key.left_slope = r.float();
        } else if r.is("RIGHT_SLOPE") {
            key.right_slope = r.float();
        } else if r.is("INTERPOLATION") {
            if !r.in_token() {
                return false;
            }
            if r.is("CONSTANT") {
                key.interpolation = 0;
            } else if r.is("LINEAR") {
                key.interpolation = 1;
            } else if r.is("SPLINE") {
                key.interpolation = 2;
            } else {
                r.message("INTERPOLATION");
            }
        } else if r.is("FLAG") {
            key.flag = r.short();
            let file_version = 0.0f64; // si_fileVersion: never written
            if !(file_version > 2.02) {
                r.in_token();
                return true;
            }
        } else if r.is("SPLINE_INDEX") {
            key.spline_index = r.short();
        } else if r.is("KEYTYPE") {
            if !r.in_token() {
                return false;
            }
            if r.is("FREEFORM") {
                key.key_type = 0;
            } else if r.is("KEYFRAME") {
                key.key_type = 1;
            } else if r.is("KEYPATH") {
                key.key_type = 2;
            } else if r.is("RAWDATA") {
                key.key_type = 3;
            } else {
                r.message("KEYTYPE");
            }
            r.in_token();
            return true;
        } else {
            return true;
        }
        if !r.in_token() {
            return false;
        }
    }
}

/// A function curve [soft 0x083dd508 si_fcurveAsciiRead], from the word `CURVE_TYPE` in the
/// token buffer on: the curve's type (a word, skipped), optional settings, then `KEYS`, a
/// count and that many keys, which end the curve.
///
/// The original builds the curve in a record it allocates for the purpose and then returns
/// without handing the record to anyone: nothing of it reaches the program. The curve is
/// kept in [`super::ReadInfo::lost_fcurves`] so that this can be seen.
///
/// The result is the original's: 1 when the curve was read to its end, 0 when the file ended
/// inside it. A count above the bytes left in the file is refused (the original allocates
/// for it; with no bytes left each key would be empty).
pub(crate) fn fcurve_read(r: &mut Reader, s: &Setup) -> Result<i16, SetupError> {
    let mut first = true;
    let mut fcv = Fcurve { interpolation: s.pref.fcv_interp_type, ..Fcurve::default() };
    let done = loop {
        if r.is("CURVE_TYPE") {
            if !r.in_token() {
                break 0;
            }
        } else if r.is("KEYS") {
            r.in_ivalue_na();
            let n = r.ivalue;
            if !r.in_token() {
                break 0;
            }
            if n > 0 {
                if n as usize > r.bytes_left() {
                    return Err(SetupError::CountTooLarge { keyword: "KEYS", count: n });
                }
                for _ in 0..n {
                    let mut key = FcurveKey::default();
                    fcvkey_read(r, &mut key);
                    fcv.keys.push(key);
                }
            }
            break 1;
        } else if r.is("PRE_EXTRAPOLATION") {
            fcv.pre_extrapolation = r.short();
        } else if r.is("POST_EXTRAPOLATION") {
            fcv.post_extrapolation = r.short();
        } else if r.is("FLAGS") {
            fcv.flags = r.short();
        } else if r.is("INTERPOLATION") {
            if !r.in_token() {
                break 0;
            }
            if r.is("CONSTANT") {
                fcv.interpolation = 0;
            } else if r.is("LINEAR") {
                fcv.interpolation = 1;
            } else if r.is("SPLINE") {
                fcv.interpolation = 2;
            } else {
                r.message("INTERPOLATION");
            }
        } else {
            if first {
                r.in_token();
            }
            // (a curve that got no keys is disposed of here)
            break 1;
        }
        if !r.in_token() {
            break 0;
        }
        first = false;
    };
    r.info.lost_fcurves.push(fcv);
    Ok(done)
}

/// One value [soft 0x083dc83c si_valueAsciiRead]: the next token as a real, or, if it is the
/// word `CURVE_TYPE`, a function curve (which leaves the value untouched). After a number
/// the following token is read: the caller finds the next keyword in the buffer.
///
/// `read_first` is the function's last argument: with it 0 the token already in the buffer
/// is taken instead of reading one (the converter's material reader does that for the
/// second value of a pair). The value is stored only where `dst` points; with no `dst` it is
/// read and dropped.
pub(crate) fn value_ascii_read(r: &mut Reader, s: &Setup, dst: Option<&mut f32>, read_first: bool) -> Result<i16, SetupError> {
    if read_first && !r.in_token() {
        return Ok(0);
    }
    if r.is("CURVE_TYPE") {
        return fcurve_read(r, s);
    }
    let v = crate::cnum::atof(r.token()) as f32;
    if let Some(d) = dst {
        *d = v;
    }
    r.in_token();
    Ok(1)
}

fn value_read(r: &mut Reader, s: &Setup, dst: &mut f32) -> Result<i16, SetupError> {
    value_ascii_read(r, s, Some(dst), true)
}

/// Three values [soft 0x083dc9ac si_value3AsciiRead]: three reals, or three function curves
/// one after the other if the first token is `CURVE_TYPE`. The second and third real go
/// through locals that are uninitialised in the original if the file ends there; here they
/// are 0. Each real is stored only if its pointer is set (here: all three or none).
pub(crate) fn value3_ascii_read(r: &mut Reader, s: &Setup, dst: Option<&mut [f32; 3]>) -> Result<i16, SetupError> {
    if !r.in_token() {
        return Ok(0);
    }
    if r.is("CURVE_TYPE") {
        let mut ret = fcurve_read(r, s)?;
        ret |= fcurve_read(r, s)?;
        ret |= fcurve_read(r, s)?;
        return Ok(ret);
    }
    let a = crate::cnum::atof(r.token()) as f32;
    let (mut b, mut c) = (0.0f32, 0.0f32);
    r.in_fvalue(&mut b);
    r.in_fvalue(&mut c);
    if let Some(d) = dst {
        *d = [a, b, c];
    }
    r.in_token();
    Ok(1)
}

fn value3_read(r: &mut Reader, s: &Setup, dst: &mut [f32; 3]) -> Result<i16, SetupError> {
    value3_ascii_read(r, s, Some(dst))
}

// ------------------------------------------------------------------------------------------
// reader
// ------------------------------------------------------------------------------------------

/// The `PICTURE` block of the render section [soft 0x083dcd40 pictureAsciiRead]: the picture
/// format of the scene camera, with the aspect of a custom-aspect format or the size of a
/// custom-size one. It starts at the word in the token buffer and ends at the first word
/// that is none of its four keywords, where it gives the camera the aspect ratio of the
/// format. If the very first word is none of them, one more token is read first. False if
/// the file ends inside the block.
fn picture_read(r: &mut Reader, s: &mut Setup) -> bool {
    s.render.has_picture = 1;
    let mut first = true;
    loop {
        if r.is("FORMAT_TYPE") {
            if !r.in_token() {
                return false;
            }
            match FORMAT_NAMES.iter().position(|n| r.is(n)) {
                Some(i) => s.layout.scene_camera_mut().format = i as i32,
                None => r.message("FORMAT_TYPE"),
            }
        } else if r.is("ASPECT") {
            let cam = s.layout.scene_camera_mut();
            r.in_fvalue(&mut cam.format_aspect);
            s.format_aspect[FORMAT_CUSTOM_ASPECT] = cam.format_aspect;
        } else if r.is("WIDTH") {
            let cam = s.layout.scene_camera_mut();
            r.in_fvalue(&mut cam.format_width);
            // (the height the ratio is taken with is the one of before)
            if cam.format_height != 0.0 {
                s.format_aspect[FORMAT_CUSTOM_SIZE] = cam.format_width / cam.format_height;
            }
        } else if r.is("HEIGHT") {
            let cam = s.layout.scene_camera_mut();
            r.in_fvalue(&mut cam.format_height);
            // no test for zero here: the ratio may come out infinite or not a number
            s.format_aspect[FORMAT_CUSTOM_SIZE] = cam.format_width / cam.format_height;
        } else {
            if first {
                r.in_token();
            }
            let format = s.layout.scene_camera().format;
            match s.format_aspect.get(format as usize) {
                Some(&a) => s.layout.scene_camera_mut().aspect = a,
                // (the original reads past the table for a format number from
                // PREFERENCE_PICT_FMT that is out of range)
                None => r.info.deviations.push("PICTURE: format outside the table"),
            }
            return true;
        }
        if !r.in_token() {
            return false;
        }
        first = false;
    }
}

/// A mental ray shader [soft 0x083e5ee8 shaderAsciiRead], from the word in the token buffer
/// on: `UDF_NAME`, `UDF_ACTIVE`, `UDF_INTERNAL`, `UDF_ORDER`, until another word. A zeroed
/// record is made if the caller has none (`SI_udfAllocate`, 0x0854c2f4, which also enters it
/// in a global list of shaders; the list is not reproduced). An empty name leaves a name
/// that is already there.
fn shader_read(r: &mut Reader, udf: &mut Option<Udf>) {
    let u = udf.get_or_insert_with(Udf::default);
    loop {
        if r.is("UDF_NAME") {
            if let Some(name) = r.string_read() {
                u.name = Some(name);
            }
        } else if r.is("UDF_ACTIVE") {
            r.in_token();
            if r.is("ACTIVE") {
                u.active = 1;
            } else if r.is("INACTIVE") {
                u.active = 0;
            } else {
                r.message("UDF_ACTIVE");
            }
        } else if r.is("UDF_INTERNAL") {
            r.in_token();
            if r.is("INTERNAL") {
                u.internal = 1;
            } else if r.is("NON_INTERNAL") {
                u.internal = 0;
            } else {
                r.message("UDF_INTERNAL");
            }
        } else if r.is("UDF_ORDER") {
            u.order = r.short() as i32;
        } else {
            return;
        }
        if !r.in_token() {
            return;
        }
    }
}

/// A count of shaders and the array for them (`MR_OUTPUT_NUM`, `MR_LENS_NUM`): the original
/// allocates four bytes per shader for whatever number the file gives. A negative count
/// gives no array here, and one above the bytes left in the file is refused.
fn shader_array(r: &mut Reader, keyword: &'static str, count: i32) -> Result<Vec<Option<Udf>>, SetupError> {
    if count > 0 && count as usize > r.bytes_left() {
        return Err(SetupError::CountTooLarge { keyword, count });
    }
    Ok(vec![None; count.max(0) as usize])
}

/// Puts a shader at the place its order says (order 0 and 1 both mean the first). The
/// original does not compare the order with the size of the array; here a shader that does
/// not fit is dropped.
fn place_shader(r: &mut Reader, list: &mut [Option<Udf>], shader: Option<Udf>) {
    let Some(u) = shader else { return };
    let at = if u.order != 0 { (u.order as i64 - 1) as usize } else { 0 };
    match list.get_mut(at) {
        Some(slot) if u.order >= 0 => *slot = Some(u),
        _ => r.info.deviations.push("shader order outside the shader array"),
    }
}

/// `ACTIVE` is 1, `INACTIVE` 0; anything else leaves the field and shows a message.
fn active(r: &mut Reader, keyword: &'static str, dst: &mut i16) {
    onoff(r, keyword, dst, "ACTIVE", "INACTIVE", true);
}

/// `YES` is 1, `NO` 0; anything else leaves the field and shows a message.
fn yes_no(r: &mut Reader, keyword: &'static str, dst: &mut i16) {
    onoff(r, keyword, dst, "YES", "NO", true);
}

fn onoff(r: &mut Reader, keyword: &'static str, dst: &mut i16, on: &str, off: &str, complain: bool) {
    r.in_token();
    if r.is(on) {
        *dst = 1;
    } else if r.is(off) {
        *dst = 0;
    } else if complain {
        r.message(keyword);
    }
}

/// A real kept if it lies in 0 to 1, replaced by `fallback` otherwise (and for a NaN).
fn unit_or(v: f32, fallback: f32) -> f32 {
    if v >= 0.0 && v <= 1.0 {
        v
    } else {
        fallback
    }
}

/// The render section [soft 0x083e0754 renderAsciiRead], from `RENDERING_TYPE` in the token
/// buffer on: keyword and value lines in any order.
///
/// The section ends in one of three ways. `DITHERING` is its last line: after its three
/// numbers the result is 1 and the caller reads the next token. A word that is none of the
/// keywords gives 0 with that word left in the buffer, and so does the end of the file.
///
/// The link strings of mental ray are emptied first; `PICTURE` is marked absent until one is
/// read.
pub(crate) fn render_read(r: &mut Reader, s: &mut Setup) -> Result<i16, SetupError> {
    let session = r.session;
    s.render.has_picture = 0;
    s.render.mental_ray.link_string = None;
    let mut ok = false;
    loop {
        let mut need = true;
        let c = &mut s.render;
        if r.is("RENDERING_TYPE") {
            r.in_token();
            if r.is("SUBREGION") {
                c.rendering_type = 4;
                c.subregion = 1;
                c.draw_subregion = 1;
            } else if (r.is("MENTAL_RAY") && !session.mental_ray_19) || r.is("MENTAL_RAY_2X") {
                c.rendering_type = 10;
                s.preview.renderer = 3;
                s.mi2_output = 1;
            } else if r.is("MENTAL_RAY") {
                c.rendering_type = 9;
                s.preview.renderer = 2;
                s.mi2_output = 0;
            } else {
                match RENDER_TYPE_NAMES.iter().position(|n| r.is(n)) {
                    Some(i) => c.rendering_type = i as i16,
                    None => r.message("RENDERING_TYPE"),
                }
            }
        } else if r.is("RENDERING_FRAME") {
            for x in c.frames.iter_mut() {
                r.in_svalue(x);
            }
        } else if r.is("SUBREGION_FRAME") {
            c.subregion = 1;
            for x in c.subregion_frame.iter_mut() {
                r.in_fvalue(x);
            }
            for x in c.subregion_size.iter_mut() {
                r.in_svalue(x);
            }
        } else if r.is("DRAW_SUBR_ON_RENDER") {
            yes_no(r, "DRAW_SUBR_ON_RENDER", &mut c.draw_subregion);
        } else if r.is("FRAME_BUFFER") {
            active(r, "FRAME_BUFFER", &mut c.frame_buffer);
        } else if r.is("OUTPUT_FILE") {
            if let Some(name) = r.string_read() {
                c.output_file = Some(name);
            }
        } else if r.is("RESOLUTION") {
            for x in c.resolution.iter_mut() {
                r.in_svalue(x);
            }
        } else if r.is("PICTURE") {
            r.in_token();
            picture_read(r, s);
            need = false;
        } else if r.is("PIXEL_RATIO") {
            r.in_fvalue(&mut c.pixel_ratio);
        } else if r.is("FILTER") {
            r.in_svalue(&mut c.filter);
        } else if r.is("MAX_RAY_DEPTH") {
            r.in_svalue(&mut c.max_ray_depth);
        } else if r.is("MAX_TRIA_DEPTH") {
            r.in_svalue(&mut c.max_tria_depth);
        } else if r.is("MAX_TREE_DEPTH") {
            r.in_svalue(&mut c.max_tree_depth);
        } else if r.is("PRE_RENDERING") {
            if let Some(name) = r.string_read() {
                c.pre_rendering = Some(name);
            }
        } else if r.is("POST_RENDERING") {
            if let Some(name) = r.string_read() {
                c.post_rendering = Some(name);
            }
        } else if r.is("TIME_STATISTICS") {
            yes_no(r, "TIME_STATISTICS", &mut c.time_statistics);
        } else if r.is("MEMORY_STATISTICS") {
            yes_no(r, "MEMORY_STATISTICS", &mut c.memory_statistics);
        } else if r.is("TAG_FILE") {
            yes_no(r, "TAG_FILE", &mut c.tag_file);
        } else if r.is("TREE_STATISTICS") {
            yes_no(r, "TREE_STATISTICS", &mut c.tree_statistics);
        } else if r.is("LINE_STATISTICS") {
            yes_no(r, "LINE_STATISTICS", &mut c.line_statistics);
        } else if r.is("ANTIALIAS") {
            r.in_token();
            if r.is("FIXED") {
                c.antialias = 0;
            } else if r.is("ADAPTIVE") {
                c.antialias = 1;
            } else {
                r.message("ANTIALIAS");
            }
            r.in_svalue(&mut c.filter);
            r.in_svalue(&mut c.antialias_level);
            r.in_fvalue(&mut c.antialias_threshold);
        } else if r.is("BACK_CULLING") {
            active(r, "BACK_CULLING", &mut c.back_culling);
        } else if r.is("FADE") {
            active(r, "FADE", &mut c.fade);
        } else if r.is("FADE_START") {
            let mut v = s.render.fade_start;
            value_read(r, s, &mut v)?;
            s.render.fade_start = v;
            need = false;
        } else if r.is("FADE_END") {
            let mut v = s.render.fade_end;
            value_read(r, s, &mut v)?;
            s.render.fade_end = v;
            need = false;
        } else if r.is("FADE_COLOUR") {
            let mut v = s.render.fade_colour;
            value3_read(r, s, &mut v)?;
            s.render.fade_colour = v;
            need = false;
        } else if r.is("FADE_TRANSPARENCY") {
            let mut v = s.render.fade_transparency;
            value_read(r, s, &mut v)?;
            s.render.fade_transparency = v;
            need = false;
        } else if r.is("FOG") {
            active(r, "FOG", &mut c.fog);
        } else if r.is("FOG_START") {
            let mut v = s.render.fog_start;
            value_read(r, s, &mut v)?;
            s.render.fog_start = v;
            need = false;
        } else if r.is("FOG_END") {
            let mut v = s.render.fog_end;
            value_read(r, s, &mut v)?;
            s.render.fog_end = v;
            need = false;
        } else if r.is("FOG_COLOUR") {
            let mut v = s.render.fog_colour;
            value3_read(r, s, &mut v)?;
            s.render.fog_colour = v;
            need = false;
        } else if r.is("FOG_DENSITY") {
            let mut v = s.render.fog_density;
            value_read(r, s, &mut v)?;
            s.render.fog_density = v;
            need = false;
        } else if r.is("FOG_BASE") {
            let mut v = s.render.fog_base;
            value_read(r, s, &mut v)?;
            s.render.fog_base = v;
            need = false;
        } else if r.is("FOG_THICKNESS") {
            let mut v = s.render.fog_thickness;
            value_read(r, s, &mut v)?;
            s.render.fog_thickness = v;
            need = false;
        } else if r.is("AMBIENCE") {
            let mut v = s.render.ambience;
            value3_read(r, s, &mut v)?;
            s.render.ambience = v;
            need = false;
        } else if r.is("MOTION_BLUR") {
            r.in_svalue(&mut c.motion_blur);
        } else if r.is("SHUTTER_SPEED") {
            r.in_fvalue(&mut c.shutter_speed);
        } else if r.is("MIN_MOVEMENT") {
            r.in_fvalue(&mut c.min_movement);
        } else if r.is("SUBFRAME") {
            active(r, "SUBFRAME", &mut c.subregion);
        } else if r.is("DRAWSUB") {
            active(r, "DRAWSUB", &mut c.draw_subregion);
        } else if r.is("ON_FIELD") {
            yes_no(r, "ON_FIELD", &mut c.on_field);
        } else if r.is("FIRST_FIELD") {
            r.in_svalue(&mut c.first_field);
        } else if r.is("DITHERING") {
            r.in_ivalue(&mut c.dithering[0]);
            r.in_ivalue(&mut c.dithering[1]);
            r.in_fvalue(&mut c.dithering_value);
            return Ok(1);
        } else if r.is("Z_CHANNEL") {
            yes_no(r, "Z_CHANNEL", &mut c.z_channel);
        } else if r.is("TEXTURE") {
            shade_setup_read(r, Some(&mut s.render_shade));
            need = false;
        } else if r.is("SHADE_SETUP_GLB") {
            shade_setup_glb_read(r, &mut s.render_shade_glb, session);
            need = false;
        } else {
            match mental_ray_line(r, s)? {
                Line::Read => {}
                Line::ReadAhead => need = false,
                // after a list shader the loop goes on without reading and without
                // looking at `need`: the same as not needing a token
                Line::Unknown => return Ok(0),
                Line::EndOfFile => return Ok(0),
            }
        }
        if need {
            ok = r.in_token();
        }
        if !ok {
            return Ok(0);
        }
    }
}

/// What a line of the render section did with the token buffer.
enum Line {
    /// Values consumed; the next keyword is still to be read.
    Read,
    /// The next keyword is already in the buffer.
    ReadAhead,
    /// Not a keyword of the section.
    Unknown,
    /// The file ended where a shader should start.
    EndOfFile,
}

/// The `MR_…` lines of the render section (part of [soft 0x083e0754 renderAsciiRead]).
fn mental_ray_line(r: &mut Reader, s: &mut Setup) -> Result<Line, SetupError> {
    let m = &mut s.render.mental_ray;
    if r.is("MR_MAX_SAMPLE") {
        r.in_ivalue(&mut m.max_sample);
    } else if r.is("MR_MIN_SAMPLE") {
        r.in_ivalue(&mut m.min_sample);
    } else if r.is("MR_FILTER_TYPE") {
        r.in_ivalue(&mut m.filter_type);
    } else if r.is("MR_FILTER_WIDTH") {
        r.in_fvalue(&mut m.filter_width);
    } else if r.is("MR_FILTER_HEIGHT") {
        r.in_fvalue(&mut m.filter_height);
    } else if r.is("MR_THREADS") {
        r.in_ivalue(&mut m.threads);
    } else if r.is("MR_VERBOSE") {
        active(r, "MR_VERBOSE", &mut m.verbose);
    } else if r.is("MR_NICE") {
        r.in_ivalue(&mut m.nice);
    } else if r.is("MR_CONTOUR") {
        active(r, "MR_CONTOUR", &mut m.contour);
    } else if r.is("MR_CONTLINEW") {
        r.in_fvalue(&mut m.contour_line_width);
    } else if r.is("MR_CONTDEPTH") {
        r.in_fvalue(&mut m.contour_depth);
    } else if r.is("MR_NORMAL") {
        active(r, "MR_NORMAL", &mut m.normal);
    } else if r.is("MR_ZPIC") {
        active(r, "MR_ZPIC", &mut m.zpic);
    } else if r.is("MR_TRACER") {
        active(r, "MR_TRACER", &mut m.tracer);
    } else if r.is("MR_SHADOW") {
        active(r, "MR_SHADOW", &mut m.shadow);
    } else if r.is("MR_FACE") {
        r.in_token();
        if r.is("MR_BOTH") {
            m.face = 3;
        } else if r.is("MR_FRONT") {
            m.face = 1;
        } else if r.is("MR_BACK") {
            m.face = 2;
        } else {
            r.message("MR_FACE");
        }
    } else if r.is("MR_RAY_REFLECTED") {
        r.in_ivalue(&mut m.ray_reflected);
    } else if r.is("MR_RAY_REFRACTED") {
        r.in_ivalue(&mut m.ray_refracted);
    } else if r.is("MR_RAY_SHADOW") {
        r.in_ivalue(&mut m.ray_shadow);
    } else if r.is("MR_ACCEL_METHOD") {
        r.in_ivalue(&mut m.accel_method);
    } else if r.is("MR_ACCEL_METHOD_2X") {
        r.in_ivalue(&mut m.accel_method_2x);
    } else if r.is("MR_PRIMARY_RAY") {
        r.in_ivalue(&mut m.primary_ray);
    } else if r.is("MR_SECONDARY_RAY") {
        r.in_ivalue(&mut m.secondary_ray);
    } else if r.is("MR_BSPDEPTH") {
        r.in_ivalue(&mut m.bsp_depth);
    } else if r.is("MR_BSPDEPTH_2X") {
        r.in_ivalue(&mut m.bsp_depth_2x);
    } else if r.is("MR_BSPSIZE") {
        r.in_ivalue(&mut m.bsp_size);
    } else if r.is("MR_BSPSIZE_2X") {
        r.in_ivalue(&mut m.bsp_size_2x);
    } else if r.is("MR_GRIDSIZE") {
        r.in_fvalue(&mut m.grid_size);
    } else if r.is("MR_MEMORY_LIMIT") {
        r.in_ivalue(&mut m.memory_limit);
    } else if r.is("MR_GAMMA") {
        r.in_fvalue(&mut m.gamma);
    } else if r.is("MR_SHUTTER") {
        r.in_fvalue(&mut m.shutter);
    } else if r.is("MR_DITHERING") {
        r.in_ivalue(&mut m.dithering);
    } else if r.is("MR_FILE_OUTPUT_TYPE") {
        r.in_ivalue(&mut m.file_output_type);
    } else if r.is("MR_FILE_OUTPUT") {
        // these two take any other word in silence
        onoff(r, "MR_FILE_OUTPUT", &mut m.file_output, "ACTIVE", "INACTIVE", false);
    } else if r.is("MR_FILE_OUTPUT_PER_FRAME") {
        onoff(r, "MR_FILE_OUTPUT_PER_FRAME", &mut m.file_output_per_frame, "ACTIVE", "INACTIVE", false);
    } else if r.is("MR_FILENAME") {
        // an empty name gives the default one
        m.filename = Some(r.string_read().unwrap_or_else(|| b"out.mi".to_vec()));
    } else if r.is("MR_DESATURATE") {
        active(r, "MR_DESATURATE", &mut m.desaturate);
    } else if r.is("MR_ADAPTIVE") {
        active(r, "MR_ADAPTIVE", &mut m.adaptive);
    } else if r.is("MR_THRESHOLD") {
        let mut v = s.render.mental_ray.threshold;
        value3_read(r, s, &mut v)?;
        s.render.mental_ray.threshold = v;
        return Ok(Line::ReadAhead);
    } else if r.is("MR_ATMOS_SHADER") {
        if !r.in_token() {
            return Ok(Line::EndOfFile);
        }
        shader_read(r, &mut m.atmos_shader);
        return Ok(Line::ReadAhead);
    } else if r.is("MR_OUTPUT_NUM") {
        r.in_ivalue(&mut m.output_num);
        m.output_shaders = shader_array(r, "MR_OUTPUT_NUM", m.output_num)?;
    } else if r.is("MR_RENDER_SHADER") {
        if !r.in_token() {
            return Ok(Line::EndOfFile);
        }
        let mut u = None;
        shader_read(r, &mut u);
        place_shader(r, &mut m.output_shaders, u);
        return Ok(Line::ReadAhead);
    } else if r.is("MR_LENS_NUM") {
        r.in_ivalue(&mut m.lens_num);
        m.lens_shaders = shader_array(r, "MR_LENS_NUM", m.lens_num)?;
    } else if r.is("MR_LENS_SHADER") {
        if !r.in_token() {
            return Ok(Line::EndOfFile);
        }
        let mut u = None;
        shader_read(r, &mut u);
        place_shader(r, &mut m.lens_shaders, u);
        return Ok(Line::ReadAhead);
    } else if r.is("MR_ANTIALIAS") {
        m.antialias = r.next_is("ACTIVE") as i16;
    } else if r.is("MR_MOTION_BLUR") {
        m.motion_blur = r.next_is("ACTIVE") as i16;
    } else if r.is("MR_BLUR_TYPE") {
        r.in_token();
        if r.is("MR_BLUR_3D") {
            m.blur_type = 0;
        } else if r.is("MR_BLUR_2D") {
            m.blur_type = 1;
        } else if r.is("MR_BLUR_MVONLY") {
            m.blur_type = 2;
        } else {
            r.message("MR_BLUR_TYPE");
        }
    } else if r.is("MR_BLUR2D_ALGO") {
        r.in_token();
        if r.is("MR_BLUR2D_UNIF") {
            m.blur2d_algo = 0;
        } else if r.is("MR_BLUR2D_LINEAR") {
            m.blur2d_algo = 1;
        } else if r.is("MR_BLUR2D_GAUSS") {
            m.blur2d_algo = 2;
        } else {
            r.message("MR_BLUR2D_ALGO");
        }
    } else if r.is("MR_BLUR_SAMPLES") {
        r.in_ivalue(&mut m.blur_samples);
    } else if r.is("MR_TIME_CONTRAST") {
        // four reals through one local (so the end of the file repeats the last one), each
        // replaced by 0.1 if it is outside 0 to 1
        let mut v = 0.0f32;
        for x in m.time_contrast.iter_mut() {
            r.in_fvalue(&mut v);
            *x = unit_or(v, 0.1);
        }
    } else if r.is("MR_REFRACTION_REAL") {
        r.in_fvalue(&mut m.refraction_real);
    } else if r.is("MR_REFRACTION_IMAGINARY") {
        r.in_fvalue(&mut m.refraction_imaginary);
    } else if r.is("MR_TRANSMIT") {
        let mut v = s.render.mental_ray.transmit;
        value3_read(r, s, &mut v)?;
        s.render.mental_ray.transmit = v;
        return Ok(Line::ReadAhead);
    } else if r.is("MR_LINK_STRING") {
        // Each line is appended to those before, a newline between. The original joins them
        // in a buffer of 1,999 bytes on its stack without measuring; here the string grows.
        if let Some(line) = r.string_read() {
            let mut all = Vec::new();
            if let Some(old) = &m.link_string {
                all.extend_from_slice(old);
                all.push(b'\n');
            }
            all.extend_from_slice(&line);
            if all.len() > 0x7ce {
                r.info.deviations.push("MR_LINK_STRING: longer than the buffer the lines are joined in");
            }
            m.link_string = Some(all);
        }
    } else if r.is("MR_PH_MAP_FILE") {
        m.ph_map_file = r.string_read();
    } else if r.is("MR_CS_FILTER_TYPE") {
        r.in_svalue(&mut m.cs_filter_type);
    } else if let Some(i) = PHOTON_FLAG_NAMES.iter().position(|n| r.is(n)) {
        r.in_svalue(&mut m.photon_flags[i]);
    } else if let Some(i) = PHOTON_INT_NAMES.iter().position(|n| r.is(n)) {
        r.in_ivalue(&mut m.photon_ints[i]);
    } else if let Some(i) = PHOTON_REAL_NAMES.iter().position(|n| r.is(n)) {
        r.in_fvalue(&mut m.photon_reals[i]);
    } else {
        return Ok(Line::Unknown);
    }
    Ok(Line::Read)
}

/// `GLOBAL_SWITCHES` [soft 0x083e00b4 renderGlobalsAsciiRead]: thirteen keywords with `YES`
/// or `NO`; any other word after one of them shows a message and leaves the switch.
pub(crate) fn render_globals_read(r: &mut Reader, c: &mut RenderControl) {
    r.in_token();
    loop {
        let Some(i) = GLOBAL_SWITCH_NAMES.iter().position(|(n, _)| r.is(n)) else { return };
        yes_no(r, GLOBAL_SWITCH_NAMES[i].0, &mut c.switches[i]);
        if !r.in_token() {
            return;
        }
    }
}

/// `PREVIEW` [soft 0x083e4d68 previewAsciiRead]: two numbers, the word `PRW_GLOBALS` (without
/// it the section ends at once), then keyword lines. Numbers out of their range are replaced
/// by a fixed value, with a message. The two sample counts are kept within two of each
/// other: each one read pulls the other along.
pub(crate) fn preview_read(r: &mut Reader, s: &mut Setup) {
    let session = r.session;
    let p = &mut s.preview;
    r.in_svalue(&mut p.size[0]);
    r.in_svalue(&mut p.size[1]);
    r.in_token();
    if !r.is("PRW_GLOBALS") {
        return;
    }
    r.in_token();
    // locals the numbers go through: uninitialised in the original if the file ends there
    let mut fv = 0.0f32;
    let mut iv = 0i32;
    loop {
        let mut bad = false;
        if r.is("PREVIEW_TYPE_SI") {
            p.renderer = 1;
        } else if (r.is("PREVIEW_TYPE_MR") && !session.mental_ray_19) || r.is("PREVIEW_TYPE_MR_2X") {
            p.renderer = 3;
            if s.render.rendering_type == 9 {
                s.render.rendering_type = 10;
            }
            s.mi2_output = 1;
        } else if r.is("PREVIEW_TYPE_MR") {
            p.renderer = 2;
            if s.render.rendering_type == 10 {
                s.render.rendering_type = 9;
            }
            s.mi2_output = 0;
        } else if let Some(i) = PREVIEW_SWITCH_NAMES.iter().position(|n| r.is(n)) {
            yes_no(r, PREVIEW_SWITCH_NAMES[i], &mut p.switches[i]);
        } else if r.is("XY_PIXEL_RATIO") {
            r.in_fvalue(&mut fv);
            if fv >= 0.0 {
                p.xy_pixel_ratio = fv;
            } else {
                bad = true;
                p.xy_pixel_ratio = 0.0;
            }
        } else if r.is("PREVIEW_IN_WINDOW") {
            r.in_token();
            p.in_window = r.is("YES") as i16;
            bad = !r.is("YES") && !r.is("NO");
        } else if r.is("SI_ACTIVE") {
            r.in_token();
            p.si_active = r.is("YES") as i16;
            bad = !r.is("YES") && !r.is("NO");
        } else if r.is("SAMPLE_TYPE") {
            r.in_ivalue(&mut iv);
            if (iv as u32) <= 2 {
                p.sample_type = iv;
            } else {
                bad = true;
                p.sample_type = 0;
            }
        } else if r.is("SAMPLE_SIZE") {
            r.in_ivalue(&mut iv);
            if (iv.wrapping_sub(1) as u32) <= 4 {
                p.sample_size = iv;
            } else {
                bad = true;
                p.sample_size = 2;
            }
        } else if r.is("MAX_FILTER") {
            r.in_ivalue(&mut iv);
            if (iv.wrapping_sub(1) as u32) <= 9 {
                p.max_filter = iv;
            } else {
                bad = true;
                p.max_filter = 2;
            }
        } else if r.is("DIV_THRESHOLD") {
            for (x, fallback) in p.div_threshold.iter_mut().zip([0.2, 0.2, 0.2, 0.05]) {
                r.in_fvalue(&mut fv);
                *x = unit_or(fv, fallback);
            }
        } else if r.is("MR_ACTIVE") {
            r.in_token();
            p.mr_active = r.is("YES") as i16;
            bad = !r.is("YES") && !r.is("NO");
        } else if r.is("ADAPTIVE") {
            r.in_token();
            p.adaptive = !r.is("NO") as i16;
            bad = !r.is("YES") && !r.is("NO");
        } else if r.is("RGB_THRESHOLD") {
            for (x, fallback) in p.rgb_threshold.iter_mut().zip([0.2, 0.2, 0.2, 0.05]) {
                r.in_fvalue(&mut fv);
                *x = unit_or(fv, fallback);
            }
        } else if r.is("FILTER_TYPE") {
            r.in_ivalue(&mut iv);
            if (iv as u32) <= 8 {
                p.filter_type = iv;
            } else {
                bad = true;
                p.filter_type = 0;
            }
        } else if r.is("WIDTH") {
            r.in_fvalue(&mut fv);
            if fv >= 0.0 {
                p.width = fv;
            } else {
                bad = true;
                p.width = 2.0;
            }
        } else if r.is("HEIGHT") {
            r.in_fvalue(&mut fv);
            if fv >= 0.0 {
                p.height = fv;
            } else {
                bad = true;
                p.height = 2.0;
            }
        } else if r.is("MIN_SAMPLES") {
            r.in_ivalue(&mut iv);
            if iv < -2 {
                bad = true;
                iv = -2;
            }
            p.min_samples = iv;
            if (p.max_samples.wrapping_sub(iv) as u32) > 2 {
                p.max_samples = iv.wrapping_add(2);
            }
        } else if r.is("MAX_SAMPLES") {
            r.in_ivalue(&mut iv);
            if iv < 0 {
                bad = true;
                iv = 0;
            }
            p.max_samples = iv;
            if (iv.wrapping_sub(p.min_samples) as u32) > 2 {
                p.min_samples = iv.wrapping_sub(2);
            }
        } else if r.is("PRW_AREA_LIGHT") {
            yes_no(r, "PRW_AREA_LIGHT", &mut p.area_light);
        } else if r.is("PRW_DISPLACEMENT") {
            yes_no(r, "PRW_DISPLACEMENT", &mut p.displacement);
        } else if r.is("PRW_VOLUME") {
            yes_no(r, "PRW_VOLUME", &mut p.volume);
        } else if r.is("PRW_LENS") {
            yes_no(r, "PRW_LENS", &mut p.lens);
        } else if r.is("PRW_OUTPUT") {
            yes_no(r, "PRW_OUTPUT", &mut p.output);
        } else if r.is("PRW_SHADOW_MAP") {
            yes_no(r, "PRW_SHADOW_MAP", &mut p.shadow_map);
        } else {
            return;
        }
        if bad {
            r.message("PREVIEW");
        }
        if !r.in_token() {
            return;
        }
    }
}

// ------------------------------------------------------------------------------------------
// writer
// ------------------------------------------------------------------------------------------

/// The `PICTURE` block [soft 0x083be7ac pictureAsciiWrite]: the format's name (none for a
/// number outside the table), and the aspect or the size for the two custom formats.
fn picture_write(o: &mut Out, s: &Setup) {
    let cam = s.layout.scene_camera();
    o.set_indent(1);
    o.key("FORMAT_TYPE");
    if let Some(name) = FORMAT_NAMES.get(cam.format as usize) {
        o.word(name);
    }
    if cam.format == FORMAT_CUSTOM_ASPECT as i32 {
        o.key("ASPECT");
        o.g(cam.format_aspect);
    } else if cam.format == FORMAT_CUSTOM_SIZE as i32 {
        o.key("WIDTH");
        o.g(cam.format_width);
        o.key("HEIGHT");
        o.g(cam.format_height);
    }
    o.set_indent(-1);
}

/// [soft 0x083c162c SI_shaderAsciiWrite]. The order is written only for shaders of kinds 6
/// and 10.
fn shader_write(o: &mut Out, u: &Udf) {
    o.key("UDF_NAME");
    o.quoted(u.name.as_deref());
    o.key("UDF_ACTIVE");
    o.choice(u.active != 0, "ACTIVE", "INACTIVE");
    o.key("UDF_INTERNAL");
    o.choice(u.internal != 0, "INTERNAL", "NON_INTERNAL");
    if u.kind == 10 || u.kind == 6 {
        o.key("UDF_ORDER");
        o.int(u.order);
    }
}

/// The render section and the global switches [soft 0x083bea34 renderAsciiWrite]
/// [soft 0x083c51b0 renderGlhwAsciiWrite]. `BACK_CULLING` is written twice. The `ANTIALIAS`
/// line is left out if its kind is neither fixed nor adaptive. The values that could be
/// function curves are always written as numbers.
pub(crate) fn render_write(o: &mut Out, s: &Setup) {
    let c = &s.render;
    o.set_indent(1);
    o.raw("\n");
    o.word("RENDERING_TYPE");
    if let Some(name) = RENDER_TYPE_NAMES.get(c.rendering_type as u16 as usize).filter(|_| c.rendering_type >= 0) {
        o.word(name);
    }
    o.key("RENDERING_FRAME");
    for x in c.frames {
        o.int(x as i32);
    }
    if c.subregion != 0 {
        o.key("SUBREGION_FRAME");
        for x in c.subregion_frame {
            o.g(x);
        }
        for x in c.subregion_size {
            o.int(x as i32);
        }
    }
    o.key("DRAW_SUBR_ON_RENDER");
    o.choice(c.draw_subregion != 0, "YES", "NO");
    o.key("FRAME_BUFFER");
    o.choice(c.frame_buffer != 0, "ACTIVE", "INACTIVE");
    o.key("OUTPUT_FILE");
    o.quoted(c.output_file.as_deref());
    o.key("RESOLUTION");
    o.int(c.resolution[0] as i32);
    o.int(c.resolution[1] as i32);
    if c.has_picture != 0 {
        o.key("PICTURE");
        picture_write(o, s);
    }
    o.key("PIXEL_RATIO");
    o.g(c.pixel_ratio);
    o.key("FILTER");
    o.int(c.filter as i32);
    o.key("PRE_RENDERING");
    o.quoted(c.pre_rendering.as_deref());
    o.key("POST_RENDERING");
    o.quoted(c.post_rendering.as_deref());
    o.key("MAX_RAY_DEPTH");
    o.int(c.max_ray_depth as i32);
    o.key("MAX_TRIA_DEPTH");
    o.int(c.max_tria_depth as i32);
    o.key("MAX_TREE_DEPTH");
    o.int(c.max_tree_depth as i32);
    o.key("TIME_STATISTICS");
    o.choice(c.time_statistics != 0, "YES", "NO");
    o.key("Z_CHANNEL");
    o.choice(c.z_channel != 0, "YES", "NO");
    o.key("MEMORY_STATISTICS");
    o.choice(c.memory_statistics != 0, "YES", "NO");
    o.key("TAG_FILE");
    o.choice(c.tag_file != 0, "YES", "NO");
    o.key("TREE_STATISTICS");
    o.choice(c.tree_statistics != 0, "YES", "NO");
    o.key("BACK_CULLING");
    o.choice(c.back_culling != 0, "ACTIVE", "INACTIVE");
    mental_ray_write(o, &c.mental_ray);
    shade_setup_write(o, &s.render_shade);
    shade_setup_glb_write(o, &s.render_shade_glb);
    o.key("LINE_STATISTICS");
    o.choice(c.line_statistics != 0, "YES", "NO");
    if (c.antialias as u16) <= 1 {
        o.key("ANTIALIAS");
        o.word(if c.antialias == 0 { "FIXED" } else { "ADAPTIVE" });
        o.int(c.filter as i32);
        o.int(c.antialias_level as i32);
        o.g(c.antialias_threshold);
    }
    o.key("BACK_CULLING");
    o.choice(c.back_culling != 0, "ACTIVE", "INACTIVE");
    o.key("FADE");
    o.choice(c.fade != 0, "ACTIVE", "INACTIVE");
    o.key("FADE_START");
    o.g(c.fade_start);
    o.key("FADE_END");
    o.g(c.fade_end);
    o.key("FADE_COLOUR");
    o.g3(&c.fade_colour);
    o.key("FADE_TRANSPARENCY");
    o.g(c.fade_transparency);
    o.key("FOG");
    o.choice(c.fog != 0, "ACTIVE", "INACTIVE");
    o.key("FOG_START");
    o.g(c.fog_start);
    o.key("FOG_END");
    o.g(c.fog_end);
    o.key("FOG_COLOUR");
    o.g3(&c.fog_colour);
    o.key("FOG_DENSITY");
    o.g(c.fog_density);
    o.key("FOG_BASE");
    o.g(c.fog_base);
    o.key("FOG_THICKNESS");
    o.g(c.fog_thickness);
    o.key("AMBIENCE");
    o.g3(&c.ambience);
    o.key("MOTION_BLUR");
    o.int(c.motion_blur as i32);
    o.word("SUBFRAME");
    o.choice(c.subregion != 0, "ACTIVE", "INACTIVE");
    o.word("DRAWSUB");
    o.choice(c.draw_subregion != 0, "ACTIVE", "INACTIVE");
    o.key("SHUTTER_SPEED");
    o.g(c.shutter_speed);
    o.key("MIN_MOVEMENT");
    o.g(c.min_movement);
    o.key("ON_FIELD");
    o.choice(c.on_field != 0, "YES", "NO");
    o.key("FIRST_FIELD");
    o.int(c.first_field as i32);
    o.key("DITHERING");
    o.int(c.dithering[0]);
    o.int(c.dithering[1]);
    o.g(c.dithering_value);
    o.raw("\n");
    o.raw("\n");
    o.word("GLOBAL_SWITCHES");
    for (i, (name, _)) in GLOBAL_SWITCH_NAMES.iter().enumerate() {
        o.key(name);
        o.choice(c.switches[i] != 0, "YES", "NO");
    }
    o.set_indent(-1);
    o.raw("\n");
}

/// The mental ray lines of the render section [soft 0x083bff2c mentalRayRenderAsciiWrite].
///
/// An empty or missing file name is written as `out.mi`. The link strings are written one
/// line each. The six photon ints are printed with `%.7g` (through a double): above seven
/// digits they lose their tail and come back different.
fn mental_ray_write(o: &mut Out, m: &MentalRay) {
    let act = |o: &mut Out, k: &str, v: i16| {
        o.key(k);
        o.choice(v != 0, "ACTIVE", "INACTIVE");
    };
    let int = |o: &mut Out, k: &str, v: i32| {
        o.key(k);
        o.int(v);
    };
    let real = |o: &mut Out, k: &str, v: f32| {
        o.key(k);
        o.g(v);
    };
    int(o, "MR_MAX_SAMPLE", m.max_sample);
    int(o, "MR_MIN_SAMPLE", m.min_sample);
    int(o, "MR_THREADS", m.threads);
    act(o, "MR_VERBOSE", m.verbose);
    int(o, "MR_NICE", m.nice);
    act(o, "MR_CONTOUR", m.contour);
    real(o, "MR_CONTLINEW", m.contour_line_width);
    real(o, "MR_CONTDEPTH", m.contour_depth);
    act(o, "MR_NORMAL", m.normal);
    act(o, "MR_ZPIC", m.zpic);
    act(o, "MR_TRACER", m.tracer);
    act(o, "MR_SHADOW", m.shadow);
    o.key("MR_FACE");
    o.word(match m.face {
        3 => "MR_BOTH",
        1 => "MR_FRONT",
        _ => "MR_BACK",
    });
    int(o, "MR_RAY_REFLECTED", m.ray_reflected);
    int(o, "MR_RAY_REFRACTED", m.ray_refracted);
    int(o, "MR_RAY_SHADOW", m.ray_shadow);
    int(o, "MR_ACCEL_METHOD", m.accel_method);
    int(o, "MR_ACCEL_METHOD_2X", m.accel_method_2x);
    int(o, "MR_PRIMARY_RAY", m.primary_ray);
    int(o, "MR_SECONDARY_RAY", m.secondary_ray);
    int(o, "MR_BSPSIZE", m.bsp_size);
    int(o, "MR_BSPSIZE_2X", m.bsp_size_2x);
    int(o, "MR_BSPDEPTH", m.bsp_depth);
    int(o, "MR_BSPDEPTH_2X", m.bsp_depth_2x);
    real(o, "MR_GRIDSIZE", m.grid_size);
    int(o, "MR_MEMORY_LIMIT", m.memory_limit);
    real(o, "MR_GAMMA", m.gamma);
    real(o, "MR_SHUTTER", m.shutter);
    int(o, "MR_FILTER_TYPE", m.filter_type);
    real(o, "MR_FILTER_WIDTH", m.filter_width);
    real(o, "MR_FILTER_HEIGHT", m.filter_height);
    int(o, "MR_DITHERING", m.dithering);
    act(o, "MR_DESATURATE", m.desaturate);
    act(o, "MR_ADAPTIVE", m.adaptive);
    int(o, "MR_FILE_OUTPUT_TYPE", m.file_output_type);
    act(o, "MR_FILE_OUTPUT", m.file_output);
    act(o, "MR_FILE_OUTPUT_PER_FRAME", m.file_output_per_frame);
    o.key("MR_FILENAME");
    match &m.filename {
        Some(name) if !name.is_empty() => o.quoted(Some(name)),
        _ => o.quoted(Some(b"out.mi")),
    }
    o.key("MR_THRESHOLD");
    o.g3(&m.threshold);
    if let Some(u) = &m.atmos_shader {
        o.key("MR_ATMOS_SHADER");
        shader_write(o, u);
    }
    if m.output_num > 0 {
        int(o, "MR_OUTPUT_NUM", m.output_num);
    }
    for u in m.output_shaders.iter().flatten() {
        o.key("MR_RENDER_SHADER");
        shader_write(o, u);
    }
    if m.lens_num > 0 {
        int(o, "MR_LENS_NUM", m.lens_num);
    }
    for u in m.lens_shaders.iter().flatten() {
        o.key("MR_LENS_SHADER");
        shader_write(o, u);
    }
    act(o, "MR_ANTIALIAS", m.antialias);
    act(o, "MR_MOTION_BLUR", m.motion_blur);
    o.key("MR_BLUR_TYPE");
    o.word(match m.blur_type {
        0 => "MR_BLUR_3D",
        1 => "MR_BLUR_2D",
        _ => "MR_BLUR_MVONLY",
    });
    o.key("MR_BLUR2D_ALGO");
    o.word(match m.blur2d_algo {
        0 => "MR_BLUR2D_UNIF",
        1 => "MR_BLUR2D_LINEAR",
        _ => "MR_BLUR2D_GAUSS",
    });
    int(o, "MR_BLUR_SAMPLES", m.blur_samples);
    o.key("MR_TIME_CONTRAST");
    for x in m.time_contrast {
        o.g(x);
    }
    real(o, "MR_REFRACTION_REAL", m.refraction_real);
    real(o, "MR_REFRACTION_IMAGINARY", m.refraction_imaginary);
    o.key("MR_TRANSMIT");
    o.g3(&m.transmit);
    if let Some(all) = &m.link_string {
        // strtok(…, "\n"): empty lines vanish
        for line in all.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
            o.key("MR_LINK_STRING");
            o.quoted(Some(line));
        }
    }
    for (i, name) in PHOTON_FLAG_NAMES.iter().enumerate() {
        int(o, name, m.photon_flags[i] as i32);
    }
    o.key("MR_PH_MAP_FILE");
    o.quoted(m.ph_map_file.as_deref());
    int(o, "MR_CS_FILTER_TYPE", m.cs_filter_type as i32);
    for (i, name) in PHOTON_INT_NAMES.iter().enumerate() {
        o.key(name);
        o.g64(m.photon_ints[i] as f64);
    }
    for (i, name) in PHOTON_REAL_NAMES.iter().enumerate() {
        real(o, name, m.photon_reals[i]);
    }
}

/// `PREVIEW` [soft 0x083c4378 previewAsciiWrite]. It raises the indent level twice and lowers
/// it once.
pub(crate) fn preview_write(o: &mut Out, s: &Setup) {
    let p = &s.preview;
    let yn = |o: &mut Out, k: &str, v: i16| {
        o.key(k);
        o.choice(v != 0, "YES", "NO");
    };
    o.raw("\n");
    o.set_indent(0);
    o.word("PREVIEW");
    o.set_indent(1);
    o.nl();
    o.int(p.size[0] as i32);
    o.int(p.size[1] as i32);
    o.key("PRW_GLOBALS");
    o.set_indent(1);
    o.nl();
    o.word(match p.renderer {
        1 => "PREVIEW_TYPE_SI",
        2 => "PREVIEW_TYPE_MR",
        _ => "PREVIEW_TYPE_MR_2X",
    });
    for (i, name) in PREVIEW_SWITCH_NAMES.iter().enumerate() {
        yn(o, name, p.switches[i]);
    }
    o.key("XY_PIXEL_RATIO");
    o.g(p.xy_pixel_ratio);
    yn(o, "PREVIEW_IN_WINDOW", p.in_window);
    yn(o, "SI_ACTIVE", p.si_active);
    o.key("SAMPLE_TYPE");
    o.int(p.sample_type);
    o.key("SAMPLE_SIZE");
    o.int(p.sample_size);
    o.key("MAX_FILTER");
    o.int(p.max_filter);
    o.key("DIV_THRESHOLD");
    for x in p.div_threshold {
        o.g(x);
    }
    yn(o, "MR_ACTIVE", p.mr_active);
    yn(o, "ADAPTIVE", p.adaptive);
    o.key("RGB_THRESHOLD");
    for x in p.rgb_threshold {
        o.g(x);
    }
    o.key("FILTER_TYPE");
    o.int(p.filter_type);
    o.key("WIDTH");
    o.g(p.width);
    o.key("HEIGHT");
    o.g(p.height);
    o.key("MIN_SAMPLES");
    o.int(p.min_samples);
    o.key("MAX_SAMPLES");
    o.int(p.max_samples);
    yn(o, "PRW_AREA_LIGHT", p.area_light);
    yn(o, "PRW_DISPLACEMENT", p.displacement);
    yn(o, "PRW_VOLUME", p.volume);
    yn(o, "PRW_LENS", p.lens);
    yn(o, "PRW_OUTPUT", p.output);
    yn(o, "PRW_SHADOW_MAP", p.shadow_map);
    o.nl();
    o.raw("\n");
    o.raw("\n");
    o.set_indent(-1);
}
