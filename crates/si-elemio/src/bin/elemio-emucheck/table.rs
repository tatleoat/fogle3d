// SPDX-License-Identifier: MIT
//! From a vector file's routine name to the port: one line per function.

use crate::{Args, Ret};
use si_elemio::image::anim as n;
use si_elemio::image::bsceneio as b;
use si_elemio::image::byteswap as w;
use si_elemio::image::camfct as c;
use si_elemio::image::elemdict as e;
use si_elemio::image::fcurve as f;
use si_elemio::image::headers as h;
use si_elemio::image::lights as l;
use si_elemio::image::outside::Outside;
use si_elemio::image::shader as s;
use si_elemio::image::utilio as u;
use si_mem::Mem;

pub fn run(name: &str, m: &mut Mem, o: &mut dyn Outside, a: &Args) -> Option<Ret> {
    Some(match name {
        // byteswap.c
        "SI_swap2byte" => w::swap2byte(m, a.p("p")).into(),
        "SI_swap4byte" => w::swap4byte(m, a.p("p")).into(),
        "SI_swap8byte" => w::swap8byte(m, a.p("p")).into(),
        "validate_float" => w::validate_float(m, a.p("p")).into(),
        "SI_byteswap_fread" => w::byteswap_fread(m, o, a.p("buffer"), a.v("size"), a.v("count"), a.f("stream")).into(),
        "SI_float_fread" => w::float_fread(m, o, a.p("buffer"), a.v("size"), a.v("count"), a.f("stream")).into(),
        "SI_fix_SI_FileChunk" => w::fix_file_chunk(m, a.p("chunk")).into(),
        "SI_FileChunk_fread" => w::file_chunk_fread(m, o, a.p("buffer"), a.v("size"), a.v("count"), a.f("stream")).into(),
        // headers.c
        "SI_sceneversioncheckExists" => h::sceneversioncheck_exists(m).into(),
        "SI_sceneversioncheckExec" => h::sceneversioncheck_exec(m, o).into(),
        "SI_dbfileReadBinHeader" => h::dbfile_read_bin_header(m, o, a.f("stream"), a.p("version"), a.p("comment")).into(),
        // the element dictionary's readers
        "SI_elemdictReadCamera" => e::elemdict_read_camera(m, o, a.p("path")).into(),
        "SI_elemdictReadLight" => e::elemdict_read_light(m, o, a.p("path")).into(),
        "SI_elemdictReadLattice" => e::elemdict_read_lattice(m, o, a.p("path"), a.s("branch"), a.p("model")).into(),
        // fcv_fct.c, fkey_fct.c, fcurveio.c
        "SI_fkeyInitialize" => f::fkey_initialize(m, a.p("key")).into(),
        "SI_fcurveInitialize" => f::fcurve_initialize(m, a.p("curve")).into(),
        "SI_fcurveAllocate" => f::fcurve_allocate(m, a.i("kind"), a.p("owner")).into(),
        "SI_fcurveDispose" => f::fcurve_dispose(m, a.p("place")).into(),
        "SI_fcvConvertAngleUnitToDegree" => f::fcv_convert_angle_unit_to_degree(m, a.p("curve")).into(),
        "fcvkeyBinaryRead" => f::fcvkey_binary_read(m, o, a.f("stream"), a.p("key")).into(),
        "SI_fcurveBinaryRead" => f::fcurve_binary_read(m, o, a.f("stream"), a.p("place"), a.p("owner")).into(),
        "SI_staticvalBinaryRead" => f::staticval_binary_read(m, o, a.f("stream"), a.p("place")).into(),
        // hierani.c, physicalio.c, dictin.c, actions.c, and the action lists of utilio.c
        "SI_actionsInit" => n::actions_init(m, a.p("list")).into(),
        "SI_actionsAddElemTime" => n::actions_add_elem_time(m, o, a.p("list"), a.p("name"), a.v("start"), a.v("end"), a.i("cycles"), a.v("flags")).into(),
        "SI_hieranimInit" => n::hieranim_init(m, a.p("anim"), a.i("chapter")).into(),
        "SI_hieranimAllocate" => n::hieranim_allocate(m, a.i("chapter")).into(),
        "SI_hieranimAddElem" => n::hieranim_add_elem(m, a.p("anim"), a.p("name"), a.i("father")).into(),
        "SI_hieranimAddItem" => n::hieranim_add_item(m, a.p("anim"), a.i("owner"), a.v("kind") as u16, a.v("rel_id"), a.v("occ_id") as u16, a.v("flag") as u16, a.p("data")).into(),
        "si_hieranimBinaryRead" => n::hieranim_binary_read(m, o, a.p("dict"), a.p("anim"), a.f("stream")).into(),
        "SI_elemdictReadAnimation" => n::elemdict_read_animation(m, o, a.p("path"), a.f("stream"), a.p("data"), a.p("kind"), a.p("owner"), a.p("rel_id"), a.p("occ_id"), a.s("old"), a.p("flag")).into(),
        "SI_actionsElemBinaryRead" => n::actions_elem_binary_read(m, o, a.f("stream"), a.p("name"), a.p("name2"), a.p("start"), a.p("end"), a.p("clip_start"), a.p("clip_end"), a.p("cycles"), a.p("flag10"), a.p("interpolation"), a.p("path"), a.p("selected")).into(),
        "SI_actionsBinaryRead" => n::actions_binary_read(m, o, a.p("anim"), a.f("stream"), a.s("which")).into(),
        "SI_elemdictReadAnimations" => n::elemdict_read_animations(m, o, a.p("dict"), a.p("path")).into(),
        // utilio.c, filetoken.c
        "SI_stringBinaryRead" => u::string_binary_read(m, o, a.f("stream")).into(),
        "si_fileSkipField" => u::file_skip_field(m, o, a.f("stream"), a.s("token")).into(),
        "si_valueBinaryRead" => u::value_binary_read(m, o, a.f("stream"), a.p("value"), a.p("fcurve"), a.p("owner")).into(),
        "si_value3BinaryRead" => u::value3_binary_read(m, o, a.f("stream"), a.p("a"), a.p("b"), a.p("c"), a.p("fa"), a.p("fb"), a.p("fc"), a.p("owner")).into(),
        "si_vectorBinaryRead" => u::vector_binary_read(m, o, a.f("stream"), a.p("vector")).into(),
        // b_matio.c, headers.c, binary.c
        "si_dbfileIsOldUdfVersion" => s::dbfile_is_old_udf_version(a.v("version")).into(),
        "readOldUDF" => s::read_old_udf(m, o, a.f("stream"), a.p("protos")).into(),
        "si_udfRead" => s::udf_read(m, o, a.f("stream"), a.p("protos")).into(),
        "si_userDataRead" => s::user_data_read(m, o, a.f("stream"), a.p("first")).into(),
        // light_fct.c, mat_global.c, algebra.c
        "SI_lightGetGrafType" => l::light_get_graf_type(m, a.p("light")).into(),
        "SI_matrixCopy" => l::matrix_copy(m, a.p("from"), a.p("to")).into(),
        "si_initSunDefault" => l::init_sun_default(m, o, a.p("light")).into(),
        "si_lightInitialize" => l::light_initialize(m, o, a.p("light"), a.i("kind")).into(),
        "SI_lightAllocate" => l::light_allocate(m, o, a.i("kind")).into(),
        "SI_newLight" => l::new_light(m, o, a.i("kind")).into(),
        // cam_fct.c
        "SI_resetCamProjPlane" => c::reset_cam_proj_plane(m, o, a.p("camera")).into(),
        "SI_resetCamera" => c::reset_camera(m, o, a.p("camera"), a.i("mode")).into(),
        "SI_newCamera" => c::new_camera(m, o, a.p("name"), a.i("mode")).into(),
        // bsceneio.c
        "si_mapLineColour" => b::map_line_colour(m, a.s("colour"), a.s("unused")).into(),
        "SI_mapIndexToWireColour" => b::map_index_to_wire_colour(m, a.s("a"), a.s("b")).into(),
        "oldpositionBinaryRead" => b::oldposition_binary_read(m, o, a.f("stream"), a.p("position"), a.p("fcurve"), a.p("model"), a.p("owner"), a.v("kind")).into(),
        "SI_cameraBinaryRead" => b::camera_binary_read(m, o, a.f("stream"), a.p("camera")).into(),
        "SI_latticeBinaryRead" => b::lattice_binary_read(m, o, a.f("stream"), a.p("model"), a.s("branch"), a.p("fcurve"), a.p("owner")).into(),
        "SI_lightBinaryRead" => b::light_binary_read(m, o, a.f("stream"), a.p("lights")).into(),
        _ => return None,
    })
}
