// SPDX-License-Identifier: MIT
//! `tools-checkkit <directory of vector files>`: `owave2soft.c` on `si-checkkit` against the
//! emulated original (vectors of `re/tools/si-tools/gen_tools.py`; findings 0039 and 0011).

use si_checkkit::Kit;
use si_tools::owave2soft::*;
use si_tools::{omovie2soft, osoft2vert::*, osoft2wave, overt2soft, stdio};

mod files;

fn main() {
    Kit::new(&[
        ("si_disposeMaterial", |m, _o, a| { si_dispose_material(m, a.p::<()>("pp").addr()); ().into() }),
        ("si_disposeNameList", |m, _o, a| { si_dispose_name_list(m, a.p::<()>("pp").addr()); ().into() }),
        ("disposeVertexList", |m, _o, a| { dispose_vertex_list(m, a.p::<()>("pp").addr()); ().into() }),
        ("disposeData", |m, _o, a| { dispose_data(m, a.p::<()>("pp").addr()); ().into() }),
        ("disposeDataLine", |m, _o, a| { dispose_data_line(m, a.p::<()>("pp").addr()); ().into() }),
        ("disposeGroupName", |m, _o, a| { dispose_group_name(m, a.p::<()>("pp").addr()); ().into() }),
        ("resetModel", |m, _o, a| { reset_model(m, a.p::<()>("wf").addr()); ().into() }),
        ("disposeWFList", |m, _o, a| { dispose_wf_list(m, a.p::<()>("pp").addr(), a.i("all") as i16); ().into() }),
        ("getListAtPosition", |m, _o, a| get_list_at_position(m, a.p::<()>("list").addr(), a.i("pos")).into()),
        ("getVertexAtPosition", |m, _o, a| get_vertex_at_position(m, a.p::<()>("list").addr(), a.i("pos")).into()),
        ("addListData", |m, o, a| add_list_data(m, o, a.p::<()>("wf").addr(), a.i("a"), a.i("b")).into()),
        ("addVertex", |m, o, a| add_vertex(m, o, a.p::<()>("pl").addr(), a.p::<()>("v").addr()).into()),
        ("findFather", |m, _o, a| find_father(m, a.i("n"), a.p::<()>("g").addr()).into()),
        ("compareInt", |m, _o, a| compare_int(m, a.p::<()>("a").addr(), a.p::<()>("b").addr()).into()),
        ("readElement", |m, o, a| read_element(m, o, a.p::<()>("wf").addr(), a.p::<()>("s").addr()).into()),
        ("cleanGroupName", |m, o, a| clean_group_name(m, o, a.p::<()>("first").addr()).into()),
        ("cleanHierarchy", |m, o, a| { clean_hierarchy(m, o, a.p::<()>("pmodel").addr()); ().into() }),
        ("addNewMdl", |m, o, a| add_new_mdl(m, o, a.p::<()>("pp").addr()).into()),
        ("setMdlType", |m, o, a| set_mdl_type(m, o, a.p::<()>("wf").addr(), a.i("kind")).into()),
        ("copyLine", |m, o, a| copy_line(m, o, a.p::<()>("dst").addr(), a.p::<()>("src").addr(), a.i("dir"), a.i("mode"), a.i("skip") as i16).into()),
        ("updateGroupName", |m, o, a| update_group_name(m, o, a.p::<()>("pg").addr(), a.p::<()>("names").addr(), a.i("a"), a.i("b")).into()),
        ("updateSmoothingGroup", |m, o, a| update_smoothing_group(m, o, a.p::<()>("pl").addr(), a.p::<()>("name").addr(), a.i("a")).into()),
        ("allocateDefaultMaterial", |m, o, _a| allocate_default_material(m, o).into()),
        ("allocDefaultWFMaterial", |m, o, _a| alloc_default_wf_material(m, o).into()),
        ("addMaterial", |m, o, a| add_material(m, o, a.p::<()>("lib").addr(), a.p::<()>("name").addr(), a.p::<()>("plist").addr()).into()),
        ("copyMaterial", |m, o, a| copy_material(m, o, a.i("idx"), a.p::<()>("used").addr(), a.p::<()>("plist").addr()).into()),
        ("convertPoint", |m, o, a| convert_point(m, o, a.p::<()>("model").addr(), a.p::<()>("wf").addr()).into()),
        ("convertLine", |m, o, a| convert_line(m, o, a.p::<()>("model").addr(), a.p::<()>("wf").addr()).into()),
        ("convertCurve", |m, o, a| convert_curve(m, o, a.p::<()>("model").addr(), a.p::<()>("wf").addr()).into()),
        ("convertPatch", |m, o, a| convert_patch(m, o, a.p::<()>("model").addr(), a.p::<()>("wf").addr(), a.p::<()>("used").addr()).into()),
        ("convertMesh", |m, o, a| convert_mesh(m, o, a.p::<()>("model").addr(), a.p::<()>("wf").addr(), a.p::<()>("used").addr(), a.p::<()>("smooth").addr(), a.i("order") as i16, a.i("base")).into()),
        ("convertWFModel2Soft", |m, o, a| convert_wf_model2soft(m, o, a.p::<()>("name").addr(), a.p::<()>("groups").addr(), a.p::<()>("wf").addr(), a.p::<()>("used").addr(), a.p::<()>("smooth").addr(), a.i("order") as i16).into()),
        ("readWavefrontFile", |m, o, a| {
            files::begin_open(a);
            read_wavefront_file(m, o, a.p::<()>("path").addr(), a.p::<()>("pgroups").addr(), a.p::<()>("pwf").addr(), a.p::<()>("plib").addr(), a.p::<()>("psmooth").addr()).into()
        }),
        ("si_objectReadWavefront", |m, o, a| {
            files::begin_open(a);
            si_object_read_wavefront(m, o, a.p::<()>("path").addr(), a.i("order") as i16).into()
        }),
        ("readLine", |m, o, a| {
            files::begin(a);
            read_line(m, o, files::HANDLE0).into()
        }),
        ("readMaterialFile", |m, o, a| {
            files::begin(a);
            read_material_file(m, o, a.p::<()>("name").addr(), files::HANDLE0, a.p::<()>("prev").addr()).into()
        }),
        ("headerWriteVertigo", |m, o, a| {
            files::begin(a);
            header_write_vertigo(m, o, files::HANDLE0, a.p::<()>("name").addr());
            files::emit(o);
            ().into()
        }),
        ("pmeshWriteVertigo", |m, o, a| {
            files::begin(a);
            let r = pmesh_write_vertigo(m, o, a.p::<()>("model").addr(), a.p::<()>("mesh").addr(), a.p::<()>("name").addr());
            files::emit(o);
            r.into()
        }),
        ("copyMtl", |m, _o, a| { osoft2wave::copy_mtl(m, a.p::<()>("dst").addr(), a.p::<()>("src").addr()); ().into() }),
        ("checkMtl", |m, _o, a| osoft2wave::check_mtl(m, a.p::<()>("mtl").addr(), a.p::<()>("plist").addr()).into()),
        ("writeMaterial", |m, o, a| {
            files::begin(a);
            osoft2wave::write_material(m, o, files::HANDLE0, a.p::<()>("mtl").addr());
            files::emit(o);
            ().into()
        }),
        ("si_checkMeshTriangulate", |m, _o, a| osoft2wave::si_check_mesh_triangulate(m, a.p::<()>("mesh").addr()).into()),
        ("si_putWavefrontHeader", |m, o, a| {
            files::begin(a);
            osoft2wave::si_put_wavefront_header(m, o, a.p::<()>("obj").addr(), a.i("mode"));
            files::emit(o);
            ().into()
        }),
        ("si_writeWavefrontMesh", |m, o, a| {
            files::begin2(a);
            osoft2wave::si_write_wavefront_mesh(m, o, a.p::<()>("model").addr(), a.p::<()>("mesh").addr(), a.p::<()>("obj").addr());
            files::emit(o);
            ().into()
        }),
        ("si_writeWavefrontPatch", |m, o, a| {
            files::begin2(a);
            osoft2wave::si_write_wavefront_patch(m, o, a.p::<()>("model").addr(), a.p::<()>("patch").addr(), a.p::<()>("obj").addr());
            files::emit(o);
            ().into()
        }),
        ("si_writeWavefrontCurve", |m, o, a| {
            files::begin2(a);
            osoft2wave::si_write_wavefront_curve(m, o, a.p::<()>("model").addr(), a.p::<()>("spline").addr(), a.p::<()>("obj").addr());
            files::emit(o);
            ().into()
        }),
        ("0x081b9694", |m, o, a| { show_object(m, o, a.p::<()>("model").addr()); ().into() }),
        ("si_dialogModelWriteVertigo", |m, o, _a| {
            files::begin(_a);
            si_dialog_model_write_vertigo(m, o);
            files::emit(o);
            ().into()
        }),
        ("si_modelSetDefinition", |m, _o, a| omovie2soft::si_model_set_definition(m, a.p::<()>("model").addr(), a.i("kind"), a.p::<()>("def").addr()).into()),
        ("si_movieToSoftimage", |m, o, a| {
            files::begin_open(a);
            omovie2soft::si_movie_to_softimage(m, o, a.p::<()>("path").addr(), a.i("verbose") as i16).into()
        }),
        ("pmeshReadVertigo", |m, o, a| {
            files::begin_open(a);
            overt2soft::pmesh_read_vertigo(m, o, a.p::<()>("name").addr()).into()
        }),
        ("checkSmoothingGroup", |m, o, a| { check_smoothing_group(m, o, a.p::<()>("lists").addr(), a.p::<()>("mesh").addr(), a.i("base")); ().into() }),
    ])
    // the argument is the address of the caller's local: it is not compared (the kit does not map
    // the emulator's local address to the image's)
    .setup(stdio::init_rodata)
    .normalise_stack_args()
    .route(files::route)
    .run();
}
