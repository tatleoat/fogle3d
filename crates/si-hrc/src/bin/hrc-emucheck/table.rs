// SPDX-License-Identifier: MIT
//! From a routine's name to its port on the image, with the arguments of a case.

use crate::{Args, Ret};
use si_hrc::image::{chunks, geom, io, material, matio, mesh, meta, node, nurbs, patch, proto, shape, spline, texture, udf, userdata, Outside, Routine};
use si_mem::Mem;

pub fn run(name: &str, m: &mut Mem, o: &mut dyn Outside, a: &Args) -> Option<Ret> {
    Some(match name {
        // byteswap.c, filetoken.c, utilio.c, headers.c
        "SI_byteswap_fread" => io::byteswap_fread(m, o, a.p("ptr"), a.v("size"), a.v("n"), a.file()).into(),
        "SI_float_fread" => io::float_fread(m, o, a.p("ptr"), a.v("size"), a.v("n"), a.file()).into(),
        "SI_Vector_fread" => io::vector_fread(m, o, a.p("ptr"), a.v("size"), a.v("n"), a.file()).into(),
        "SI_RGB_fread" => io::rgb_fread(m, o, a.p("ptr"), a.v("size"), a.v("n"), a.file()).into(),
        "SI_RGBA_fread" => io::rgba_fread(m, o, a.p("ptr"), a.v("size"), a.v("n"), a.file()).into(),
        "SI_FileChunk_fread" => io::file_chunk_fread(m, o, a.p("ptr"), a.v("size"), a.v("n"), a.file()).into(),
        "SI_stringBinaryRead" => {
            // A string of 260 to 263 bytes overwrites the original's saved frame pointer and
            // nothing else: the routine still returns, and the harness, which is its only
            // caller here, does not use that register. The port marks such a string with a
            // fault (its caller in the program would go on with a wrong frame).
            let bytes = a.file_bytes();
            let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            if (260..264).contains(&len) {
                return Some(Ret::Skip("a string that overwrites the original's saved frame pointer: it returns to the harness all the same, the port faults"));
            }
            io::string_binary_read(m, o, a.file()).into()
        }
        "si_fileSkipField" => io::file_skip_field(m, o, a.file(), a.s("token")).into(),
        "si_dbfileIsOldUdfVersion" => io::is_old_udf_version(a.f("version")).into(),
        "SI_dbfileReadBinHeader" => io::read_bin_header(m, o, a.file(), a.p("version"), a.p("text")).into(),
        "si_prototypeAllocate" => proto::prototype_allocate(m).into(),
        "si_prototypeBinaryRead" => proto::prototype_binary_read(m, o, a.file(), a.t("protos")).into(),
        "si_prototypeDispose" => proto::prototype_dispose(m, a.t("pp")).into(),
        // udf_fct.c
        "SI_udfAllocate" => udf::udf_allocate(m, o).into(),
        "SI_udfIsActive" => udf::udf_is_active(m, a.t("udf")).into(),
        "SI_udfSetActive" => udf::udf_set_active(m, o, a.t("udf"), a.s("active")).into(),
        "SI_udfInitializeUdf" => udf::udf_initialize_udf(m, o, a.t("dst"), a.t("src")).into(),
        "SI_udfCopy" => udf::udf_copy(m, o, a.t("udf")).into(),
        "SI_udfDispose" => udf::udf_dispose(m, o, a.t("udf")).into(),
        // binary.c and model.c: user data
        "si_userDataRead" => userdata::user_data_read(m, o, a.file(), a.t("list")).into(),
        "SI_userDataCopyFromSetup" => userdata::user_data_copy_from_setup(m, a.t("src"), a.t("dst")).into(),
        // t2d_fct.c, t3d_fct.c, mat_fct.c, algebra.c
        "SI_matrixCopy" => texture::matrix_copy(m, a.p("src"), a.p("dst")).into(),
        "SI_textureInitialize" => texture::texture_initialize(m, a.t("t")).into(),
        "SI_textureAllocate" => texture::texture_allocate(m).into(),
        "SI_updTxt2dEffects" => texture::upd_txt2d_effects(m, a.t("t")).into(),
        "textureTransfer" => texture::texture_transfer(m, o, a.t("src"), a.t("dst")).into(),
        "SI_textureCopy" => texture::texture_copy(m, o, a.t("src")).into(),
        "SI_texture3DInitialize" => texture::texture3d_initialize(m, a.t("t")).into(),
        "SI_texture3DAllocate" => texture::texture3d_allocate(m).into(),
        "texture3DTransfer" => texture::texture3d_transfer(m, o, a.t("src"), a.t("dst")).into(),
        "SI_texture3DCopy" => texture::texture3d_copy(m, o, a.t("src")).into(),
        "SI_materialInitialize" => material::material_initialize(m, a.t("mat")).into(),
        "SI_materialAllocate" => material::material_allocate(m).into(),
        // b_matio.c
        "readOldUDF" => matio::read_old_udf(m, o, a.file(), a.t("protos")).into(),
        "si_udfRead" => matio::udf_read(m, o, a.file(), a.t("protos")).into(),
        "si_textureBinaryRead" => matio::texture_binary_read(m, o, a.file(), a.t("protos")).into(),
        "si_texture3DBinaryRead" => matio::texture3d_binary_read(m, o, a.file(), a.t("protos")).into(),
        "si_materialBinaryRead" => matio::material_binary_read(m, o, a.file(), a.t("protos")).into(),
        "SI_materialReadBinaryFile" => matio::material_read_binary_file(m, o, a.p("name")).into(),
        "SI_textureReadBinaryFile" => matio::texture_read_binary_file(m, o, a.p("name")).into(),
        "SI_texture3DReadBinaryFile" => matio::texture3d_read_binary_file(m, o, a.p("name")).into(),
        // mesh_b.c, obj_global.c: the mesh's allocators
        "SI_meshAllocate" => geom::mesh_allocate(m, o).into(),
        "SI_vertexAllocate" => geom::vertex_allocate(m, a.v("n") as i32).into(),
        "SI_polygonAllocate" => geom::polygon_allocate(m, a.v("n") as i32).into(),
        "SI_polygonNodeAllocate" => geom::polygon_node_allocate(m, a.v("n") as i32).into(),
        "SI_edgeAllocate" => geom::edge_allocate(m, a.v("n") as i32).into(),
        "SI_setCurrentModel" => mesh::set_current_model(m, a.p("node")).into(),
        // binary.c: the model reader
        "meshBinaryRead" => mesh::mesh_binary_read(m, o, a.file(), a.t("protos"), a.p("node"), a.p("map"), a.s("keep")).into(),
        "SI_modelAllocate" => node::model_allocate(m, a.p("name")).into(),
        "SI_modelAddDefinition" => node::model_add_definition(m, o, a.p("node"), a.v("kind"), a.p("geometry")).into(),
        "SI_klusterAllocate" => node::kluster_allocate(m).into(),
        "modelBinaryReadNode" => node::model_binary_read_node(m, o, a.file(), a.p("node"), a.t("protos"), a.s("mode")).into(),
        "SI_fdmodelBinaryRead" => node::fdmodel_binary_read(m, o, a.file(), a.p("node"), a.s("mode")).into(),
        // second pass. byteswap.c, spline.c, face.c, nurbs.c: what the readers below call
        "SI_double_fread" => io::double_fread(m, o, a.p("ptr"), a.v("size"), a.v("n"), a.file()).into(),
        "SI_splineInit" => spline::spline_init(m, a.p("s")).into(),
        "SI_splineAllocate" => spline::spline_allocate(m).into(),
        "SI_splineKeyAllocate" => spline::spline_key_allocate(m, a.v("n") as i32).into(),
        "SI_splineTransfer" => spline::spline_transfer(m, a.p("src"), a.p("dst")).into(),
        "SI_splineDispose" => spline::spline_dispose(m, a.p("pp")).into(),
        "SI_splineCopy" => spline::spline_copy(m, a.p("src")).into(),
        "SI_bezierShrink" => spline::bezier_shrink(m, o, a.p("src")).into(),
        "SI_faceAllocate" => spline::face_allocate(m).into(),
        "SI_nurbsCrvAllocate" => nurbs::nurbs_crv_allocate(m).into(),
        "SI_nurbsPointsAllocate" => nurbs::nurbs_points_allocate(m, a.v("n") as i32).into(),
        "si_knotsAllocate" => nurbs::knots_allocate(m, a.v("n") as i32).into(),
        "SI_nurbsCrvInit" => nurbs::nurbs_crv_init(m, a.p("dst"), a.p("src")).into(),
        "SI_nurbsCrvDispose" => nurbs::nurbs_crv_dispose(m, a.p("pp")).into(),
        // binary.c: the geometries other than the mesh
        "splineBinaryRead" => spline::spline_binary_read(m, o, a.file(), a.t("protos")).into(),
        "nurbsCrvBinaryRead" => nurbs::nurbs_crv_binary_read(m, o, a.file(), a.t("protos")).into(),
        "faceBinaryRead" => spline::face_binary_read(m, o, a.file(), a.t("protos")).into(),
        "SI_nurbsSrfAllocate" => nurbs::nurbs_srf_allocate(m).into(),
        "nurbsCompBinaryRead" => nurbs::nurbs_comp_binary_read(m, o, a.file(), a.t("protos")).into(),
        "nurbsSrfBinaryRead" => nurbs::nurbs_srf_binary_read(m, o, a.file(), a.t("protos")).into(),
        "SI_3doubles_fread" => io::three_doubles_fread(m, o, a.p("ptr"), a.v("size"), a.v("n"), a.file()).into(),
        "SI_metaballAllocate" => meta::metaball_allocate(m, a.p("model")).into(),
        "SI_metaSystemAllocate" => meta::meta_system_allocate(m).into(),
        "SI_metaSystemAddGroup" => meta::meta_system_add_group(m, a.p("sys"), a.p("name"), a.p("group")).into(),
        "metaballBinaryRead" => meta::metaball_binary_read(m, o, a.file(), a.t("protos"), a.p("model")).into(),
        "metaSystemBinaryRead" => meta::meta_system_binary_read(m, o, a.file(), a.t("protos"), a.p("model")).into(),
        "SI_patchAllocate" => patch::patch_allocate(m).into(),
        "patchBinaryRead" => patch::patch_binary_read(m, o, a.file(), a.t("protos")).into(),
        // the chunks that hang on a node, and what they call
        "SI_matrix3Copy" => chunks::matrix3_copy(m, a.p("src"), a.p("dst")).into(),
        "SI_Matrix_fread" => io::matrix_fread(m, o, a.p("ptr"), a.v("size"), a.v("n"), a.file()).into(),
        "SI_dynamatterSetDefault" => chunks::dynamatter_set_default(m, a.p("p")).into(),
        "dynageometrySetDefault" => chunks::dynageometry_set_default(m, a.p("p")).into(),
        "SI_dynamatterAllocate" => chunks::dynamatter_allocate(m).into(),
        "SI_dynajntpropSetDefault" => chunks::dynajntprop_set_default(m, a.p("p")).into(),
        "SI_dynajntpropAllocate" => chunks::dynajntprop_allocate(m).into(),
        "dynamatterReadBinaryChunk" => chunks::dynamatter_read_binary_chunk(m, o, a.file(), a.t("protos")).into(),
        "dynajntpropReadBinaryChunk" => chunks::dynajntprop_read_binary_chunk(m, o, a.file(), a.p("node"), a.t("protos")).into(),
        "SI_klusterAddToList" => chunks::kluster_add_to_list(m, o, a.p("node"), a.p("name"), a.s("tell")).into(),
        "clustersReadBinaryChunk" => chunks::clusters_read_binary_chunk(m, o, a.file(), a.p("node"), a.t("protos")).into(),
        "SI_generalMatrixAllocateDbl" => chunks::general_matrix_allocate_dbl(m, a.v("rows") as i32, a.v("cols") as i32).into(),
        "SI_invKinAllocate" => chunks::inv_kin_allocate(m).into(),
        "SI_invKinRootAllocate" => chunks::inv_kin_root_allocate(m, a.p("ik"), a.v("joints") as i32, a.s("other")).into(),
        "SI_invKinReadBinaryChunk" => chunks::inv_kin_read_binary_chunk(m, o, a.file()).into(),
        "SI_envelopInfoAllocate" => chunks::envelop_info_allocate(m).into(),
        "SI_envelopListAllocate" => chunks::envelop_list_allocate(m, a.p("model")).into(),
        "SI_envelopVertAllocate" => chunks::envelop_vert_allocate(m).into(),
        "si_envelopVertListDispose" => chunks::envelop_vert_list_dispose(m, a.p("pp")).into(),
        "SI_envelopInfoDispose" => chunks::envelop_info_dispose(m, a.p("pp")).into(),
        "envelopVertReadBinaryChunk" => chunks::envelop_vert_read_binary_chunk(m, o, a.file(), a.p("matrices"), a.v("index") as i32, a.t("protos")).into(),
        "envelopReadBinaryChunk" => chunks::envelop_read_binary_chunk(m, o, a.file(), a.p("node"), a.t("protos")).into(),
        // binary.c: the shape file reader (gen_shape.py)
        "SI_shapeBinaryRead" => shape::shape_binary_read(m, o, a.p("name"), a.p("model")).into(),
        _ => return None,
    })
}

/// The effect of an outside routine whose stand-in in the harness has one (see
/// `_fake_rebuild` in `re/tools/si-hrc/hrclib.py`): the made-up rebuild of a mesh.
pub fn outside_effect(m: &mut Mem, routine: &'static Routine, args: &[u32]) {
    if routine.name != "SI_meshUpdateNodes" {
        return;
    }
    let mesh = args[0];
    let (verts, n) = (m.u32(mesh), m.u32(mesh + 0x14));
    if verts != 0 && 0 < n && n <= 200 {
        let table = m.malloc(4);
        let block = m.malloc((n + 1) * 0x14);
        m.set_fill_bytes(block, 0, (n + 1) * 0x14);
        m.set_u32(table, block);
        m.set_u32(mesh + 8, table);
        for i in 0..n {
            let star = m.malloc(24);
            for (k, v) in [((i + 1) % n) as i32, i as i32 + 1, -1, ((i + 2) % n) as i32, -(i as i32 + 1), -1].into_iter().enumerate() {
                m.set_i32(star + 4 * k as u32, v);
            }
            m.set_u32(verts + i * 0x14 + 0xc, star);
            m.set_u16(verts + i * 0x14 + 0x10, 2);
        }
    }
}
