// SPDX-License-Identifier: MIT
//! From a vector file's routine name to the port: one line per function.

use crate::{Args, Ret};
use si_dict::outside::{Outside, Routine};
use si_dict::*;
use si_mem::Mem;

/// What an outside routine does to memory besides answering: the stand-ins of
/// `re/tools/si-dict/dictlib.py` that have a body (`BODIES`), repeated on the image. The
/// answer is the address the stand-in got, so it must come out the same here.
pub fn outside_effect(m: &mut Mem, routine: &'static Routine, args: &[u32], answer: u32) -> Result<(), String> {
    let put = |m: &mut Mem, n: u32, data: &[u8]| {
        let a = m.malloc(n);
        m.set_fill_bytes(a, 0, n);
        m.set_bytes(a, data);
        a
    };
    let got = match routine.name {
        "SI_dbListNodeAllocate" => {
            let node = put(m, 0x34, b"");
            for (off, text) in [(8u32, &b"\0"[..]), (0x18, b"1\0"), (0x1c, b"0\0")] {
                let s = put(m, text.len() as u32, text);
                m.set_u32(node + off, s);
            }
            node
        }
        "SI_dbFormatFullName" => put(m, 5, b"full\0"),
        "SI_generalAttrGetValue" => {
            // the made-up reading function of dictlib.py's `_attr_get_value`: the value's
            // type and number are the third and fourth word of the element's identity
            let (kind, number) = (m.u32(args[0] + 8), m.u32(args[0] + 12));
            m.set_u32(args[2], kind);
            m.set_u32(args[2] + 4, 0x77);
            m.set_u32(args[2] + 8, number);
            return Ok(());
        }
        "dbfilenameExpand" => {
            // the made-up split of dictlib.py's `split_name`
            let name = m.cstr(args[0]);
            let cut = |a: usize, b: usize| name[a.min(name.len())..b.min(name.len())].to_vec();
            let parts = [cut(0, 1), cut(1, 4), name.len().to_string().into_bytes(), b"0".to_vec(), b"x".to_vec()];
            for (k, part) in parts.iter().enumerate() {
                m.put_cstr(args[1 + k], part);
            }
            return Ok(());
        }
        _ => return Ok(()),
    };
    if got == answer {
        Ok(())
    } else {
        Err(format!("{}: the stand-in's block is at {answer:08x} in the original, {got:08x} here", routine.name))
    }
}

pub fn run(name: &str, m: &mut Mem, o: &mut dyn Outside, a: &Args) -> Option<Ret> {
    Some(match name {
        // enmdict.c
        "si_enmdictAddValue" => enmdict::add_value(m, a.p("ty"), a.i("id"), a.p("code"), a.p("name")).into(),
        "si_enmdictAddEnumType" => enmdict::add_enum_type(m, a.p("dict"), a.p("name")).into(),
        "SI_enmdictInit" => enmdict::init(m, a.p("dict")).into(),
        "SI_enmdictDispose" => enmdict::dispose(m, a.p("dict")).into(),
        "SI_enmdictEnumValGetByCode" => enmdict::enum_val_get_by_code(m, a.p("dict"), a.i("ty"), a.p("code")).into(),
        "SI_enmdictEnumCodeGetByValue" => enmdict::enum_code_get_by_value(m, a.p("dict"), a.i("ty"), a.i("id")).into(),
        "SI_enmdictEnumTypeGetName" => enmdict::enum_type_get_name(m, a.p("dict"), a.p("name")).into(),
        "SI_enmdictEnumValGetByAnyCode" => enmdict::enum_val_get_by_any_code(m, a.p("dict"), a.p("index"), a.p("code")).into(),
        "SI_enmdictEnumValGetNameById" => enmdict::enum_val_get_name_by_id(m, a.p("dict"), a.i("ty"), a.i("id")).into(),
        // toposort.c
        "elemdictFindFather" => toposort::find_father(m, a.p("elem")).into(),
        "relTopoSortInitChapter" => toposort::rel_topo_sort_init_chapter(m, a.p("list"), a.s("fwd")).into(),
        "SI_relTopoSortInit" => toposort::rel_topo_sort_init(m, a.p("ed"), a.s("fwd")).into(),
        "relCreateArguments" => toposort::rel_create_arguments(m, o, a.p("ed"), a.p("list"), a.i("start"), a.p("count")).into(),
        "topoSort" => toposort::topo_sort(m, o, a.p("rd"), a.p("ed"), a.p("upd"), a.p("elem"), a.p("rels"), a.i("chap"), a.i("index"), a.i("group"), a.i("level"), a.s("check"), a.s("fwd"), a.s("restricted"), a.s("pure")).into(),
        "elemdictTopoSortChapter" => toposort::topo_sort_chapter(m, o, a.p("rd"), a.p("ed"), a.p("upd"), a.p("list"), a.i("chap"), a.s("check"), a.s("roots"), a.s("restricted")).into(),
        "SI_elemdictTopoSort" => toposort::topo_sort_dict(m, o, a.p("rd"), a.p("ed"), a.p("upd"), a.s("check"), a.s("restricted")).into(),
        "SI_elemdictTopoSortElement" => toposort::topo_sort_element(m, o, a.p("rd"), a.p("ed"), a.i("chap"), a.i("index"), a.i("group"), a.s("check")).into(),
        "SI_elemdictTopoSortSelectedElements" => toposort::topo_sort_selected_elements(m, o, a.p("rd"), a.p("ed"), a.i("group"), a.s("untold"), a.s("check")).into(),
        "SI_elemdictCheckCycle" => toposort::check_cycle(m, o, a.p("rd"), a.p("ed")).into(),
        "SI_elemdictUpdListSetDirty" => toposort::upd_list_set_dirty(m, a.p("ed")).into(),
        "SI_elemdictFindRoot" => toposort::find_root(m, a.p("list"), a.i("index"), a.p("root")).into(),
        "elemdictSortHier" => toposort::sort_hier(m, a.p("list"), a.p("elem"), a.p("rels"), a.i("index"), a.i("group"), a.i("level"), a.p("order"), a.p("count"), a.p("pos")).into(),
        "SI_elemdictSortHierElements" => toposort::sort_hier_elements(m, a.p("ed"), a.i("chap")).into(),
        "relGetRelationList" => toposort::rel_get_relation_list(m, o, a.p("ed"), a.p("list"), a.i("chap"), a.i("index"), a.i("group"), a.p("upd")).into(),
        "SI_elemdictGetAffectedByRelations" => toposort::get_affected_by_relations(m, o, a.p("ed"), a.i("chap"), a.i("index"), a.i("group"), a.p("upd")).into(),
        "SI_elemdictGetAffectsRelations" => toposort::get_affects_relations(m, o, a.p("ed"), a.i("chap"), a.i("index"), a.i("group"), a.p("upd")).into(),
        "SI_elemdictTopoSortElemByFcv" => toposort::topo_sort_elem_by_fcv(m, o, a.p("rd"), a.p("ed"), a.i("group"), a.s("untold"), a.s("check"), a.p("chapters")).into(),
        // reldictops.rs, hieranim.rs (finding 0010, section on the dictionary's rest)
        "SI_reldictGetNbRes" => reldictops::get_nb_res(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetNbArgs" => reldictops::get_nb_args(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetNbDistinctRes" => reldictops::get_nb_distinct_res(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetNbDistinctArgs" => reldictops::get_nb_distinct_args(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetNextDistinctRes" => reldictops::get_next_distinct_res(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetNextDistinctArg" => reldictops::get_next_distinct_arg(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetNbResInstances" => reldictops::get_nb_res_instances(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetNbArgInstances" => reldictops::get_nb_arg_instances(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetResPickMessage" => reldictops::get_res_pick_message(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetArgPickMessage" => reldictops::get_arg_pick_message(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetResCond" => reldictops::get_res_cond(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetArgCond" => reldictops::get_arg_cond(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetChap" => reldictops::get_chap(m, a.p("rd"), a.i("index"), a.s("which")).into(),
        "SI_reldictGetGrp" => reldictops::get_grp(m, a.p("rd"), a.i("index"), a.s("which")).into(),
        "SI_reldictGetCond" => reldictops::get_cond(m, a.p("rd"), a.i("index"), a.s("which")).into(),
        "SI_reldictGetScope" => reldictops::get_scope(m, a.p("rd"), a.i("index"), a.s("which")).into(),
        "SI_reldictGetName" => reldictops::get_name(m, a.p("rd"), a.i("index"), a.s("which")).into(),
        "reldictOpArgInstanceGetGrp" => reldictops::op_arg_instance_get_grp(m, a.p("rd"), a.i("index"), a.i("n")).into(),
        "reldictOpResInstanceGetGrp" => reldictops::op_res_instance_get_grp(m, a.p("rd"), a.i("index"), a.i("n")).into(),
        "SI_reldictOpParamInstanceGetGrp" => reldictops::op_param_instance_get_grp(m, a.p("rd"), a.i("index"), a.i("n"), a.s("which")).into(),
        "lookForExistingResName" => reldictops::look_for_existing_res_name(m, a.p::<()>("def").addr(), a.p::<()>("name").addr(), a.i("ty"), a.i("ty2"), a.s("group"), a.s("all")).into(),
        "lookForExistingArgName" => reldictops::look_for_existing_arg_name(m, a.p::<()>("def").addr(), a.p::<()>("name").addr(), a.i("ty"), a.i("ty2")).into(),
        "lookForExistingOatName" => reldictops::look_for_existing_oat_name(m, a.p::<()>("atd").addr(), a.p::<()>("group").addr(), a.p::<()>("name").addr()).into(),
        "lookForExistingFcurveName" => reldictops::look_for_existing_fcurve_name(m, a.p::<()>("atd").addr(), a.p::<()>("group").addr(), a.p::<()>("name").addr()).into(),
        "searchAttdictByFcvName" => reldictops::search_attdict_by_fcv_name(m, a.p::<()>("atd").addr(), a.p::<()>("group").addr(), a.p::<()>("name").addr()).into(),
        "SI_elemdictRestoreActions" => dictin::restore_actions(m, o, a.p("ed")).into(),
        "SI_hieranimDispose" => hieranim::dispose(m, o, a.p::<()>("anim").addr()).into(),
        "SI_hieranimGetName" => hieranim::get_name(m, a.p::<()>("anim").addr(), a.i("index")).into(),
        "SI_hieranimGetType" => hieranim::get_type(m, a.p::<()>("elem").addr(), a.i("index"), 0).into(),
        // reldict.c
        "SI_reldictAddRelation" => reldict::add_relation(m, a.p("rd"), a.p("name"), a.i("relid"), a.i("sc"), a.i("dc"), a.i("sg"), a.i("dg"), a.i("rank"), a.i("fanout"), a.s("dope"), a.i("u0"), a.i("u1"), a.s("dup"), a.p("cus"), a.s("render"), a.i("incmode")).into(),
        "SI_reldictInit" => reldict::init(m, a.p("rd")).into(),
        "SI_reldictDispose" => reldict::dispose(m, a.p("rd")).into(),
        "findElemInRelSortTbl" => reldict::find_elem_in_rel_sort_tbl(m, a.p("rd"), a.i("relid"), a.p("tbl"), a.i("count"), a.p("pos")).into(),
        "addElemInRelSortTbl" => reldict::add_elem_in_rel_sort_tbl(m, a.i("index"), a.p("rd"), a.i("relid"), a.p("tbl"), a.p("count")).into(),
        "SI_reldictGetRelIndexFromId" => reldict::get_rel_index_from_id(m, a.p("rd"), a.i("relid")).into(),
        "SI_reldictGetRelName" => reldict::get_rel_name(m, a.p("rd"), a.i("relid")).into(),
        "si_reldictGetRelSub" => reldict::get_rel_sub(m, a.p("rd"), a.i("relid"), a.i("sc"), a.i("dc"), a.i("sg"), a.i("dg"), a.p("fanout")).into(),
        "SI_reldictGetRelNbRules" => reldict::get_rel_nb_rules(m, a.p("rd"), a.i("relid")).into(),
        "SI_reldictCheckRelAnim" => reldict::check_rel_anim(m, a.p("rd"), a.i("relid"), a.p("fanout"), a.p("cid")).into(),
        "SI_reldictGetRelAttributes" => reldict::get_rel_attributes(m, a.p("rd"), a.i("relid"), a.i("rule"), a.i("unused"), a.p("sc"), a.p("dc"), a.p("sg"), a.p("dg"), a.p("rank"), a.p("fanout")).into(),
        "SI_reldictCusGetRelAttributes" => reldict::cus_get_rel_attributes(m, a.p("rd"), a.p("data"), a.i("rule"), a.p("rank"), a.p("fanout")).into(),
        "SI_reldictGetRelRenderFlag" => reldict::get_rel_render_flag(m, a.p("rd"), a.i("relid")).into(),
        "SI_reldictGetRelIncmodeFlag" => reldict::get_rel_incmode_flag(m, a.p("rd"), a.i("relid")).into(),
        "SI_reldictGetRelChapGrp" => reldict::get_rel_chap_grp(m, a.p("rd"), a.i("relid"), a.p("group")).into(),
        "reldictGetRelDopeDisp" => reldict::get_rel_dope_disp(m, a.p("rd"), a.i("relid")).into(),
        "SI_reldictGetRelRank" => reldict::get_rel_rank(m, a.p("rd"), a.i("relid")).into(),
        "si_reldictGetRelUpdlattr" => reldict::get_rel_updlattr(m, a.p("rd"), a.i("relid"), a.p("a"), a.p("b")).into(),
        "SI_reldictIsRelActive" => reldict::is_rel_active(m, a.p("rd"), a.i("relid"), a.s("dep")).into(),
        "SI_reldictGetRelIdByName" => reldict::get_rel_id_by_name(m, a.p("rd"), a.p("name")).into(),
        "SI_reldictActiveSet" => reldict::active_set(m, a.i("relid"), a.s("value"), a.s("dep")).into(),
        "SI_reldictNbRelForDeactCount" => reldict::nb_rel_for_deact_count(m, a.p("rd"), a.i("relid"), a.i("unused")).into(),
        "SI_reldictCheckUnCompleteRelation" => reldict::check_uncomplete_relation(m, a.p("rd"), a.i("relid")).into(),
        "SI_reldictGetRelIndexFromName" => reldict::get_rel_index_from_name(m, a.p("rd"), a.p("effect"), a.p("name")).into(),
        "SI_reldictGetMaxFanout" => reldict::get_max_fanout(m, a.p("rd"), a.i("index"), a.s("dst")).into(),
        "SI_reldictIsNewSrc" => reldict::is_new_src(m, a.p("rd"), a.i("index")).into(),
        "SI_reldictGetRelRankByIndex" => reldict::get_rel_rank_by_index(m, a.p("rd"), a.i("index")).into(),
        // relations.c
        "si_relAllocate" => relations::rel_allocate(m, a.p("rels")).into(),
        "si_relRemoveARelation" => relations::rel_remove_a_relation(m, o, a.p("ed"), a.p("elem"), a.p("list"), a.i("relid"), a.i("own"), a.i("chap"), a.i("elemid"), a.i("serial")).into(),
        "si_relRemoveARelationId" => relations::rel_remove_a_relation_id(m, o, a.p("ed"), a.p("elem"), a.p("list"), a.i("key"), a.i("a5"), a.i("a6"), a.i("serial"), a.s("by"), a.s("ofwd")).into(),
        "SI_relRemoveRelations" => relations::rel_remove_relations(m, o, a.p("ed"), a.p("elem"), a.i("a3"), a.i("a4"), a.s("back")).into(),
        "si_relAddRelation" => relations::rel_add_relation(m, a.p("rd"), a.p("list"), a.i("relid"), a.i("chap"), a.i("elemid"), a.i("group"), a.i("rank"), a.i("fanout"), a.i("pos"), a.i("serial"), a.s("src"), a.p("data"), a.s("flag")).into(),
        "si_relCheck" => relations::rel_check(m, a.p("rels"), a.i("group"), a.i("relid")).into(),
        "SI_elemdictGetRelSerialNumber" => relations::get_rel_serial_number(m).into(),
        // elemdict.c: relations
        "SI_elemdictCusGetRelData" => elemrel::cus_get_rel_data(m, a.p("rel")).into(),
        "SI_elemdictAddCompleteRelation" => elemrel::add_complete_relation(m, a.p("ed"), a.p("rd"), a.i("src"), a.i("dst"), a.i("relid"), a.i("rule"), a.i("pos"), a.p("data"), a.s("flag")).into(),
        "SI_elemdictAddRelation" => elemrel::add_relation(m, a.p("ed"), a.p("rd"), a.i("src"), a.i("dst"), a.i("relid"), a.i("rule"), a.i("pos")).into(),
        "SI_elemdictAddUnCompleteRelation" => elemrel::add_uncomplete_relation(m, a.p("ed"), a.p("rd"), a.i("sc"), a.i("dc"), a.i("src"), a.i("dst"), a.i("relid"), a.i("rule"), a.i("pos"), a.p("data"), a.s("flag")).into(),
        "SI_elemdictAddSerialRelation" => elemrel::add_serial_relation(m, a.p("ed"), a.p("rd"), a.i("sc"), a.i("dc"), a.i("sg"), a.i("dg"), a.i("src"), a.i("dst"), a.i("relid"), a.i("serial"), a.i("rule"), a.i("pos"), a.p("data"), a.s("flag"), a.s("issrc")).into(),
        "SI_elemdictFindSerialRelation" => elemrel::find_serial_relation(m, a.p("ed"), a.p("elem"), a.i("relid"), a.p("list"), a.i("chap"), a.s("fwd"), a.s("first")).into(),
        "SI_elemdictSetRelToSerialid" => elemrel::set_rel_to_serialid(m, a.p("ed"), a.p("elem"), a.i("relid"), a.i("serial"), a.s("fwd")).into(),
        "SI_elemdictRemoveRelationPair" => elemrel::remove_relation_pair(m, o, a.p("ed"), a.p("rd"), a.i("src"), a.i("dst"), a.i("relid"), a.i("rule"), a.i("unused")).into(),
        "SI_elemdictDisconnectANDConnectRel" => elemrel::disconnect_and_connect_rel(m, o, a.p("ed"), a.p("rd"), a.i("src"), a.p("from"), a.p("to"), a.i("relid")).into(),
        "SI_elemdictRemoveUncompleteRelPair" => elemrel::remove_uncomplete_rel_pair(m, o, a.p("ed"), a.p("rd"), a.i("sc"), a.i("dc"), a.i("src"), a.i("dst"), a.i("relid"), a.i("rule"), a.i("unused"), a.i("serial")).into(),
        "SI_elemdictRemoveRelationId" => elemrel::remove_relation_id(m, o, a.p("ed"), a.p("rd"), a.i("src"), a.i("relid")).into(),
        "SI_elemdictRemoveSerialRelId" => elemrel::remove_serial_rel_id(m, o, a.p("ed"), a.p("rd"), a.i("chap"), a.i("index"), a.i("relid"), a.i("serial")).into(),
        "SI_elemdictRemoveBackwardRelId" => elemrel::remove_backward_rel_id(m, o, a.p("ed"), a.i("chap"), a.i("index"), a.i("relid"), a.i("serial")).into(),
        "SI_elemdictGetCurRelation" => elemrel::get_cur_relation(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group")).into(),
        "SI_elemdictGetRelation" => elemrel::get_relation(m, a.p("ed"), a.p("elem"), a.i("relid"), a.i("chap"), a.i("nth")).into(),
        "SI_elemdictGetRelationFromIds" => elemrel::get_relation_from_ids(m, a.p("ed"), a.p("elem"), a.i("relid"), a.i("chap"), a.i("index"), a.i("serial")).into(),
        "SI_elemdictResetCurrFlag" => elemrel::reset_curr_flag(m, a.p("elem"), a.i("relid")).into(),
        "SI_elemdictSetCurrRelPointer" => elemrel::set_curr_rel_pointer(m, a.p("ed"), a.p("elem"), a.i("relid"), a.p("list")).into(),
        "SI_elemdictFindRelatedElem" => elemrel::find_related_elem(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("relid"), a.i("chap"), a.p("chap"), a.p("index")).into(),
        "SI_elemdictFindNextRelatedElem" => elemrel::find_next_related_elem(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("relid"), a.i("chap"), a.p("chap"), a.p("index")).into(),
        "SI_elemdictFindRelatedElem2" => elemrel::find_related_elem2(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("ogroup"), a.i("relid"), a.i("chap"), a.p("chap"), a.p("index")).into(),
        "SI_elemdictFindRangeNextRelatedElem2" => elemrel::find_range_next_related_elem2(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("ogroup"), a.i("lo"), a.i("hi"), a.i("chap"), a.p("relid"), a.p("index")).into(),
        "SI_elemdictFindRangeRelatedElem" => elemrel::find_range_related_elem(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("lo"), a.i("hi"), a.i("chap"), a.p("relid"), a.p("index")).into(),
        "SI_elemdictFindRangeNextRelatedElem" => elemrel::find_range_next_related_elem(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("lo"), a.i("hi"), a.i("chap"), a.p("relid"), a.p("index")).into(),
        "SI_elemdictFindRelatedElemSerial" => elemrel::find_related_elem_serial(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("relid"), a.i("chap"), a.p("chap"), a.p("index"), a.i("serial")).into(),
        "SI_elemdictFindNextRelatedElemSerial" => elemrel::find_next_related_elem_serial(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("relid"), a.i("chap"), a.p("chap"), a.p("index"), a.i("serial")).into(),
        "SI_elemdictRelationsCount" => elemrel::relations_count(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("ogroup"), a.i("relid"), a.i("chap")).into(),
        "SI_elemdictRangeRelationsCount" => elemrel::range_relations_count(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("lo"), a.i("hi"), a.i("chap")).into(),
        "SI_elemdictFindSourceRelation" => elemrel::find_source_relation(m, a.p("ed"), a.p("elem"), a.i("chap")).into(),
        "SI_elemdictSerialRelCount" => elemrel::serial_rel_count(m, a.p("ed"), a.p("elem"), a.i("relid"), a.p("which"), a.p("pos")).into(),
        // elemdict.c: dictionaries, chapters, elements
        "filenameCheckSuffix" => elemdict::filename_check_suffix(m, a.p("name")).into(),
        "strcmp_underscore" => elemdict::strcmp_underscore(m, a.p("a"), a.p("b")).into(),
        "SI_compareElemName" => elemdict::compare_elem_name(m, a.p("a"), a.p("b")).into(),
        "elemdictCompareChapNames" => elemdict::compare_chap_names(m, a.p("a"), a.p("b")).into(),
        "SI_elemdictSortChapNames" => elemdict::sort_chap_names(m, a.p("ed")).into(),
        "SI_elemdictFindListByName" => elemdict::find_list_by_name(m, a.p("ed"), a.p("name")).into(),
        "SI_elemdictInitElem" => elemdict::init_elem(m, o, a.p("elem")).into(),
        "SI_elemlistAddElem" => elemdict::elemlist_add_elem(m, o, a.p("list"), a.p("node"), a.p("data"), a.p("aux"), a.i("dsc"), a.i("work"), a.s("rels"), a.s("reuse")).into(),
        "SI_elemdictAddElem" => elemdict::add_elem(m, o, a.p("ed"), a.i("chap"), a.p("node"), a.p("data"), a.p("aux"), a.i("dsc"), a.i("work")).into(),
        "SI_elemdictGetChapName" => elemdict::get_chap_name(m, a.p("ed"), a.i("chap")).into(),
        "SI_elemlistGetElem" => elemdict::elemlist_get_elem(m, a.p("list"), a.i("index")).into(),
        "SI_elemdictGetElem" => elemdict::get_elem(m, a.p("ed"), a.i("chap"), a.i("index")).into(),
        "SI_elemdictGetElemId" => elemdict::get_elem_id(m, a.p("ed"), a.i("chap"), a.p("elem")).into(),
        "SI_elemdictSetDirty" => elemdict::set_dirty(m, a.p("elem"), a.i("dirty")).into(),
        "SI_elemdictSchemSetPosX" => elemdict::schem_set_pos_x(m, a.p("elem"), a.f("x")).into(),
        "SI_elemdictSchemSetPosY" => elemdict::schem_set_pos_y(m, a.p("elem"), a.f("y")).into(),
        "si_elemdictSetFlag" => elemdict::set_flag(m, a.p("elem"), a.s("flag")).into(),
        "SI_elemlistClearWorkFlag" => elemdict::elemlist_clear_work_flag(m, a.p("ed"), a.i("chap")).into(),
        "SI_elemlistClearLowByteWorkFlag" => elemdict::elemlist_clear_low_byte_work_flag(m, a.p("ed"), a.i("chap")).into(),
        "SI_elemlistLowByteWorkFlagAND" => elemdict::elemlist_low_byte_work_flag_and(m, a.p("ed"), a.i("chap"), a.s("mask")).into(),
        "SI_elemlistInit" => elemdict::elemlist_init(m, a.p("list")).into(),
        "SI_elemdictAddList" => elemdict::add_list(m, a.p("ed"), a.p("name")).into(),
        "SI_elemlistCountValidElems" => elemdict::elemlist_count_valid_elems(m, a.p("list")).into(),
        "SI_elemdictFindElemByName" => elemdict::find_elem_by_name(m, a.p("ed"), a.i("chap"), a.p("full"), a.p("alias")).into(),
        "SI_dbSortTblSetDirty" => elemdict::sort_tbl_set_dirty(m, a.p("list")).into(),
        "findElemInChapSortTbl" => elemdict::find_elem_in_chap_sort_tbl(m, a.p("list"), a.v("data"), a.p("tbl"), a.i("count"), a.p("pos")).into(),
        "addElemInChapSortTbl" => elemdict::add_elem_in_chap_sort_tbl(m, a.i("index"), a.p("list"), a.v("data"), a.p("tbl"), a.p("count")).into(),
        "SI_elemdictFindElemByData" => elemdict::find_elem_by_data(m, o, a.p("ed"), a.i("chap"), a.p("data")).into(),
        "SI_elemdictFindElemByCB" => elemdict::find_elem_by_cb(m, o, a.p("ed"), a.i("chap"), a.p("test"), a.p::<u8>("arg").addr()).into(),
        "SI_elemdictFindElemByAuxData" => elemdict::find_elem_by_aux_data(m, a.p("ed"), a.i("chap"), a.p("obj")).into(),
        "SI_elemdictFindElemByDelData" => elemdict::find_elem_by_del_data(m, a.p("ed"), a.i("chap"), a.p("obj")).into(),
        "SI_elemdictFindElemByDelAuxData" => elemdict::find_elem_by_del_aux_data(m, a.p("ed"), a.i("chap"), a.p("obj")).into(),
        "elemUpdateHierInfo" => elemdict::elem_update_hier_info(m, a.p("list"), a.p("rels"), a.i("group"), a.i("relid"), a.i("index"), a.i("root"), a.i("level")).into(),
        "SI_elemdictUpdateHierInfo" => elemdict::update_hier_info(m, a.p("ed"), a.i("chap"), a.i("group"), a.i("relid")).into(),
        "SI_elemdictInit" => elemdict::init(m, a.p("ed")).into(),
        "SI_dbIconFree" => elemdict::icon_free(m, a.p("elem")).into(),
        "SI_elemdictSetDeleteElem" => elemdict::set_delete_elem(m, o, a.p("ed"), a.i("chap"), a.p("elem"), a.i("index"), a.i("towards"), a.s("only"), a.s("giveup")).into(),
        "SI_elemdictDeleteChapterElems" => elemdict::delete_chapter_elems(m, o, a.p("ed"), a.i("chap"), a.i("root")).into(),
        "SI_elemlistDisposeElements" => elemdict::elemlist_dispose_elements(m, o, a.p("list"), a.s("keep")).into(),
        "SI_elemlistDispose" => elemdict::elemlist_dispose(m, o, a.p("list"), a.s("keep")).into(),
        "SI_elemdictDispose" => elemdict::dispose(m, o, a.p("ed"), a.s("keep")).into(),
        "relBackUpDispose" => elemdict::rel_back_up_dispose(m, a.p("array"), a.p("count")).into(),
        // elemdict.c: names
        "SI_dbScnGetListNode" => elemname::db_scn_get_list_node(m, o, a.p("name"), a.p("alias"), a.s("strip")).into(),
        "SI_dbListNodeCopyInfo" => elemname::db_list_node_copy_info(m, o, a.p("ed"), a.p("src"), a.p("dst")).into(),
        "SI_elemdictFindElemByPrjName" => elemname::find_elem_by_prj_name(m, a.p("ed"), a.i("chap"), a.p("except"), a.p("prefix"), a.p("name"), a.p("unused"), a.i("root"), a.s("new")).into(),
        "SI_elemdictFindElemByNameOnly" => elemname::find_elem_by_name_only(m, a.p("ed"), a.i("chap"), a.p("except"), a.p("name"), a.i("root"), a.s("new")).into(),
        "SI_elemdictCheckDuplElemName" => elemname::check_dupl_elem_name(m, a.p("ed"), a.i("chap"), a.p("elem")).into(),
        "SI_elemdictFindElemByFullName" => elemname::find_elem_by_full_name(m, a.p("ed"), a.i("chap"), a.p("except"), a.p("full"), a.s("new")).into(),
        "SI_elemdictChangeDscName" => elemname::change_dsc_name(m, o, a.p("ed"), a.i("index"), a.p("name"), a.s("fill"), a.s("repl"), a.p("old")).into(),
        "SI_elemdictPropagatePrfxAndShare" => elemname::propagate_prfx_and_share(m, o, a.p("ed"), a.i("chap"), a.i("root"), a.p("prefix"), a.s("shared")).into(),
        "SI_elemdictChangeName" => elemname::change_name(m, o, a.p("ed"), a.i("chap"), a.p("elem"), a.p("prefix"), a.p("name")).into(),
        "SI_elemdictGetNewNameId" => elemname::get_new_name_id(m, o, a.p("ed"), a.i("chap"), a.p("base"), a.p("id"), a.s("prefix")).into(),
        "SI_elemdictGetNewName" => elemname::get_new_name(m, o, a.p("ed"), a.i("chap"), a.p("base")).into(),
        "SI_elemdictGetNewPrefix" => elemname::get_new_prefix(m, o, a.p("ed"), a.i("chap"), a.p("base")).into(),
        "SI_elemdictCreateDefaultDsc" => elemname::create_default_dsc(m, o, a.p("ed")).into(),
        "elemdictSetVersion" => elemname::set_version(m, o, a.p("ed"), a.i("value")).into(),
        "SI_elemdictSetSubstitution" => elemname::set_substitution(m, o, a.p("ed"), a.i("value")).into(),
        "si_elemdictResetOriginDB" => elemname::reset_origin_db(m, a.p("ed"), a.p("path")).into(),
        "si_elemdictSetOriginDB" => elemname::set_origin_db(m, a.p("ed"), a.p("path")).into(),
        // elemdict.c: the rest
        "SI_elemdictSetNewFlag" => elemmisc::set_new_flag(m, a.p("ed"), a.i("from"), a.i("to")).into(),
        "SI_elemdictSetSelection" => elemmisc::set_selection(m, a.p("ed"), a.s("sel")).into(),
        "SI_elemdictIsElemUsed" => elemmisc::is_elem_used(m, a.p("elem"), a.s("ignore")).into(),
        "SI_elemdictCheckEmptyDescription" => elemmisc::check_empty_description(m, o, a.p("ed")).into(),
        "SI_elemdictAddSelElemsFromDb" => elemmisc::add_sel_elems_from_db(m, o, a.p("ed"), a.p("db"), a.s("unused")).into(),
        "SI_elemdictMoveElemsFromOutsideChapter" => elemmisc::move_elems_from_outside_chapter(m, o, a.p("ed")).into(),
        "SI_elemdictRemoveAllSubScenes" => elemmisc::remove_all_sub_scenes(m, o, a.p("ed")).into(),
        "SI_elemdictRemoveAllInvalidElems" => elemmisc::remove_all_invalid_elems(m, o, a.p("ed")).into(),
        "SI_elemdictRemoveAllEmptyAnim" => elemmisc::remove_all_empty_anim(m, o, a.p("ed"), a.i("chap")).into(),
        "SI_elemdictSortAnimElements" => elemmisc::sort_anim_elements(m, o, a.p("list")).into(),
        "SI_elemdictAdd" => elemmisc::add(m).into(),
        "SI_elemdictRemove" => elemmisc::remove(m, o, a.i("index")).into(),
        "SI_elemdictGetNb" => elemmisc::get_nb(m).into(),
        "SI_elemdictExists" => elemmisc::exists(m, a.i("index")).into(),
        "SI_elemdictCompareElemIdent" => elemmisc::compare_elem_ident(m, a.p("a"), a.p("b")).into(),
        "SI_elemdictCopyElements" => elemmisc::copy_elements(m, o, a.p("src"), a.p("dst")).into(),
        "SI_elemdictCopyRenderRelations" => elemmisc::copy_render_relations(m, a.p("src"), a.p("dst"), a.p("rd")).into(),
        "SI_elemdictRelationCheck" => elemmisc::relation_check(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("relid"), a.i("chap")).into(),
        "SI_elemdictGetElemFromData" => elemmisc::get_elem_from_data(m, o, a.p("ed"), a.i("chap"), a.p("data"), a.p("index")).into(),
        "SI_elemdictFindSon" => elemmisc::find_son(m, a.p("ed"), a.p("elem")).into(),
        "SI_elemdictFindNextSon" => elemmisc::find_next_son(m, a.p("ed"), a.p("elem")).into(),
        "SI_elemdictGetElemFromScene" => elemmisc::get_elem_from_scene(m, a.p("ed"), a.p("index")).into(),
        "SI_elemdictGetFirstRelatedElem" => elemmisc::get_first_related_elem(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("relid"), a.i("chap"), a.p("oelem"), a.p("cursor")).into(),
        "SI_elemdictGetNextRelatedElem" => elemmisc::get_next_related_elem(m, a.p("ed"), a.p("elem"), a.s("fwd"), a.i("group"), a.i("relid"), a.i("chap"), a.p("oelem"), a.p("cursor")).into(),
        "SI_elemdictGetNthRelatedElem" => elemmisc::get_nth_related_elem(m, a.p("ed"), a.p("elem"), a.i("nth"), a.s("fwd"), a.i("group"), a.i("relid"), a.i("chap"), a.p("oelem")).into(),
        "SI_elemdictGetCurRelationIndex" => elemmisc::get_cur_relation_index(m, a.p("elem"), a.s("fwd"), a.i("group")).into(),
        "SI_elemdictSetCurRelationIndex" => elemmisc::set_cur_relation_index(m, a.p("elem"), a.s("fwd"), a.i("group"), a.i("cur")).into(),
        "SI_elemdictAddUDFElemRelation" => elemmisc::add_udf_elem_relation(m, o, a.p("udf"), a.p("ed"), a.p("elem"), a.i("chap"), a.i("serial")).into(),
        "SI_elemdictRemoveUDFElemRelation" => elemmisc::remove_udf_elem_relation(m, o, a.p("udf"), a.p("ed"), a.p("elem"), a.i("chap"), a.i("serial")).into(),
        "SI_elemdictLoadChapters" => elemmisc::load_chapters(m, o, a.p("ed"), a.p("cid"), a.s("scene")).into(),
        "SI_elemdictRecursiveSelect" => elemmisc::recursive_select(m, a.p("ed"), a.i("chap"), a.p("full"), a.i("depth"), a.i("nb"), a.p("chaps")).into(),
        "checkNSEStatus" => elemmisc::check_nse_status(m, o, a.p("ed"), a.p("src"), a.p("dst"), a.i("relid"), a.i("chap")).into(),
        // updlist.c
        "si_updlistAddArgument" => updlist::add_argument(m, a.p("args"), a.p("count"), a.p("rel"), a.p("data"), a.p("adata")).into(),
        "SI_updlistInit" => updlist::init(m, a.p("list")).into(),
        "si_updlistAddNode" => updlist::add_node(m, a.p("list"), a.i("chap"), a.i("elemid"), a.p("elem"), a.i("relid"), a.p("rel"), a.i("group"), a.i("nargs"), a.p("args")).into(),
        "SI_updlistDispose" => updlist::dispose(m, a.p("list")).into(),
        // the element sort of elemdict.c and its two helpers of text.c (vectors of
        // re/tools/si-dict/gen_sort.py, kept in a directory of their own)
        "SI_strGetSeqNum" => elemsort::str_get_seq_num(m, a.p("s")).into(),
        "SI_strStripDigit" => elemsort::str_strip_digit(m, a.p("s")).into(),
        "elemdictCompareElems" => elemsort::compare_elems(m, a.p("a"), a.p("b")).into(),
        "SI_elemlistSortChapterElements" => elemsort::sort_chapter_elements(m, o, a.p("list"), a.i("chap"), a.i("key"), a.s("up"), a.s("hier")).into(),
        // attdict.c (vectors of re/tools/si-dict/gen_att.py, kept in a directory of their own)
        "si_attdictInitAttribute" => attdict::init_attribute(m, a.p("a")).into(),
        "attdictInitAttribGroup" => attdict::init_attrib_group(m, a.p("g")).into(),
        "SI_attdictInit" => attdict::init(m, a.p("atd")).into(),
        "si_attdictAllocateFcurveArray" => attdict::allocate_fcurve_array(m, a.p("a"), a.i("count")).into(),
        "si_attdictAddAttribute" => attdict::add_attribute(m, a.p("atd"), a.p("group"), a.p("src")).into(),
        "si_attdictAddAttribGroup" => attdict::add_attrib_group(m, a.p("atd"), a.p("name"), a.v("cond"), a.i("rel_type")).into(),
        "si_attdictAddAttribSubGroup" => attdict::add_attrib_sub_group(m, a.p("group"), a.p("name"), a.v("cond")).into(),
        "attdictAttribGroupDispose" => attdict::attrib_group_dispose(m, a.p("g")).into(),
        "SI_attdictDispose" => attdict::dispose(m, a.p("atd")).into(),
        "si_attdictAttribSubGroupGetByName" => attdict::attrib_sub_group_get_by_name(m, a.p("g"), a.p("name")).into(),
        "SI_attdictAttribGroupGetByName" => attdict::attrib_group_get_by_name(m, a.p("atd"), a.p("name")).into(),
        "SI_attdictAttribGroupGetByIndex" => attdict::attrib_group_get_by_index(m, a.p("atd"), a.p("index")).into(),
        "SI_attdictAttributeGetByName" => attdict::attribute_get_by_name(m, a.p("atd"), a.p("name")).into(),
        "SI_attdictAttributeGetByIndex" => attdict::attribute_get_by_index(m, a.p("atd"), a.p("index")).into(),
        "SI_attdictAttributeSetAccessMode" => attdict::attribute_set_access_mode(m, a.p("atd"), a.p("index"), a.s("mode")).into(),
        "SI_attdictAttributeGetAccessMode" => attdict::attribute_get_access_mode(m, a.p("atd"), a.p("index"), a.p("out")).into(),
        "SI_attdictEvalCond" => attdict::eval_cond(m, o, a.p("cond"), a.p("ident")).into(),
        "attdictEvalGroupCond" => attdict::eval_group_cond(m, o, a.p("g"), a.p("ident")).into(),
        "SI_attdictValidateAttr" => attdict::validate_attr(m, o, a.p("ident"), a.p("index")).into(),
        "SI_attdictRangeAdjust" => attdict::range_adjust(m, a.p("a"), a.p("v")).into(),
        "SI_attdictSetFctsPointers" => attdict::set_fcts_pointers(m, a.p("atd")).into(),
        "SI_attdictAttGrpGetAttrib" => attdict::att_grp_get_attrib(m, a.p("atd"), a.p("g"), a.p("after")).into(),
        "SI_attdictAttGrpGetNbAttributes" => attdict::att_grp_get_nb_attributes(m, a.p("g")).into(),
        "SI_attdictAttGrpGetAttribute" => attdict::att_grp_get_attribute(m, a.p("atd"), a.p("g"), a.i("i")).into(),
        "attdictAttribGroupDump" => attdict::attrib_group_dump(m, o, a.p("atd"), a.p("g"), a.i("position"), a.i("level")).into(),
        "SI_attdictDump" => attdict::dump(m, o, a.p("atd")).into(),
        "attdictAttribGroupUpdateIndexes" => attdict::attrib_group_update_indexes(m, a.p("atd"), a.p("g"), a.i("level"), a.p("path")).into(),
        "SI_attdictUpdateIndexes" => attdict::update_indexes(m, a.p("atd")).into(),
        "SI_attdictCountAnimatedParams" => attdict::count_animated_params(m, a.p("atd")).into(),
        "attdictCompareIds" => attdict::compare_ids(m, a.p("a"), a.p("b")).into(),
        "attdictInitFcvIndexes" => attdict::init_fcv_indexes(m, a.p("atd"), a.p("base")).into(),
        "SI_attdictSortAttribFromFcurveIds" => attdict::sort_attrib_from_fcurve_ids(m, a.p("atd")).into(),
        "attdictBSearchCompareIds" => attdict::bsearch_compare_ids(m, a.p("a"), a.p("b")).into(),
        "SI_attdictGetGrpIndexFromFcurve" => attdict::get_grp_index_from_fcurve(m, a.p("atd"), a.i("id"), a.p("out")).into(),
        "SI_attdictFindFcurve" => attdict::find_fcurve(m, a.p("atd"), a.i("id")).into(),
        "SI_attdictSetFcurveFlag" => attdict::set_fcurve_flag(m, a.p("atd"), a.i("id"), a.i("flag")).into(),
        "SI_attdictGetFcurveFlag" => attdict::get_fcurve_flag(m, a.p("atd"), a.i("id")).into(),
        "SI_attdictGetFcurveFromName" => attdict::get_fcurve_from_name(m, a.p("atd"), a.p("name")).into(),
        "SI_attdictGetGroupCustomFcurve" => attdict::get_group_custom_fcurve().into(),
        "SI_attdictGetGroupFromFcurve" => attdict::get_group_from_fcurve(m, a.p("atd"), a.i("id")).into(),
        "SI_attdictGetNameFromFcurve" => attdict::get_name_from_fcurve(m, a.p("atd"), a.i("id")).into(),
        "SI_attdictGetChapidFromFcurve" => attdict::get_chapid_from_fcurve(m, a.p("atd"), a.i("id")).into(),
        "getOldSubdef" => attdict::get_old_subdef(m, a.i("id"), a.p("out")).into(),
        "SI_attdictGetInfoFromFcurve" => attdict::get_info_from_fcurve(m, a.p("atd"), a.i("id"), a.p("relid"), a.p("group"), a.p("rule"), a.p("limit")).into(),
        "SI_attdictGetFcurveFromAttId" => attdict::get_fcurve_from_att_id(m, a.p("atd"), a.i("att_id")).into(),
        "SI_attdictGetNbFcurvesForAttId" => attdict::get_nb_fcurves_for_att_id(m, a.p("atd"), a.i("att_id")).into(),
        "SI_attdictAttribGetDefWidth" => attdict::attrib_get_def_width(m, a.p("atd"), a.p("index")).into(),
        "SI_attdictGetAttGrpByPath" => attdict::get_att_grp_by_path(m, a.p("atd"), a.p("path")).into(),
        "SI_attdictGetAttSubGrpByPath" => attdict::get_att_sub_grp_by_path(m, a.p("g"), a.p("path")).into(),
        "SI_attdictAttGrpGetNbAnimatableAttribs" => attdict::att_grp_get_nb_animatable_attribs(m, a.p("atd"), a.p("g")).into(),
        "SI_attdictAttributeIsAnimatable" => Ret::Eax(attdict::attribute_is_animatable(m, a.p("a")) as u32),
        "SI_attdictCompareGrpIndex" => attdict::compare_grp_index(m, a.p("a"), a.p("b")).into(),
        _ => return None,
    })
}

/// The same vectors through the earlier port of 19 of these functions in `si-motion`
/// (`DICT_EARLIER=motion`, feature `earlier`): that crate works on the same image, so its
/// functions take the case as it is. What it reaches through its host trait is answered
/// by this crate's functions and by the case's recorded outside calls. One thing it does
/// itself that the vectors answer by a stand-in: the curve of a relation entry
/// (`SI_edGetFcvFromRelation`); the cases that asked for one are left out by the caller.
#[cfg(feature = "earlier")]
pub fn run_motion(name: &str, m: &mut Mem, o: &mut dyn Outside, a: &Args) -> Option<Ret> {
    use si_dict::outside;
    use si_mem::Ptr32;
    use si_motion::ed::{self, Related};
    use si_motion::topo::{self, Mode, TopoHost};
    struct Host<'a> {
        o: &'a mut dyn Outside,
    }
    impl TopoHost for Host<'_> {
        fn reldict_is_rel_active(&mut self, m: &mut Mem, rd: u32, relid: i32, dep: i32) -> i16 {
            reldict::is_rel_active(m, Ptr32::new(rd), relid, dep as i16) as i16
        }
        fn reldict_get_rel_updlattr(&mut self, m: &mut Mem, rd: u32, relid: i32, pur: u32, pre: u32) {
            reldict::get_rel_updlattr(m, Ptr32::new(rd), relid, Ptr32::new(pur), Ptr32::new(pre));
        }
        fn elemdict_get_relation(&mut self, m: &mut Mem, d: u32, e: u32, relid: i32, chap: i32, nth: i32) -> u32 {
            elemrel::get_relation(m, Ptr32::new(d), Ptr32::new(e), relid, chap, nth).addr()
        }
        fn db_check_extra_un_told_relation(&mut self, m: &mut Mem, d: u32, on: i16, forward: i16, c: i16) {
            outside::si_db_check_extra_untold_relation(self.o, m, Ptr32::<u8>::new(d), on as i32, forward as i32, c as i32);
        }
        fn cus_resort_upd_list(&mut self, m: &mut Mem, list: u32) {
            outside::si_cus_resort_upd_list(self.o, m, Ptr32::<u8>::new(list));
        }
        fn printf_toposort_too_deep(&mut self, _m: &mut Mem) {
            self.o.stdout(b"TopoSort: help! level > 100... \n");
        }
    }
    let p = |n: &str| a.p::<u8>(n).addr();
    let b = |n: &str| a.s(n) != 0;
    // the original stores what it found through two pointers; the earlier port returns it
    let found = |m: &mut Mem, r: Related, first: &str, index: &str| {
        if r.elem != 0 {
            m.set_i32(a.p::<u8>(first).addr(), r.first);
            m.set_i32(a.p::<u8>(index).addr(), r.index);
        }
        Ret::Eax(r.elem)
    };
    Some(match name {
        "SI_elemdictGetElem" => ed::get_elem(m, p("ed"), a.i("chap"), a.i("index")).into(),
        "SI_elemdictGetCurRelation" => ed::get_cur_relation(m, p("elem"), b("fwd"), a.i("group")).into(),
        "SI_elemdictFindRelatedElem" => {
            let r = ed::find_related_elem(m, p("ed"), p("elem"), b("fwd"), a.i("group"), a.i("relid"), a.i("chap"));
            found(m, r, "chap", "index")
        }
        "SI_elemdictFindNextRelatedElem" => {
            let r = ed::find_next_related_elem(m, p("ed"), p("elem"), b("fwd"), a.i("group"), a.i("relid"), a.i("chap"));
            found(m, r, "chap", "index")
        }
        "SI_elemdictFindRangeRelatedElem" => {
            let r = ed::find_range_related_elem(m, p("ed"), p("elem"), b("fwd"), a.i("group"), a.i("lo"), a.i("hi"), a.i("chap"));
            found(m, r, "relid", "index")
        }
        "SI_elemdictFindRangeNextRelatedElem" => {
            let r = ed::find_range_next_related_elem(m, p("ed"), p("elem"), b("fwd"), a.i("group"), a.i("lo"), a.i("hi"), a.i("chap"));
            found(m, r, "relid", "index")
        }
        "SI_updlistInit" => topo::updlist_init(m, p("list")).into(),
        "si_updlistAddArgument" => {
            let mut count = m.i32(p("count"));
            let r = topo::updlist_add_argument(m, p("args"), &mut count, p("rel"), p("data"), p("adata"));
            m.set_i32(p("count"), count);
            r.into()
        }
        "si_updlistAddNode" => topo::updlist_add_node(m, p("list"), a.i("chap"), a.i("elemid"), p("elem"), a.i("relid"), p("rel"), a.i("group"), a.i("nargs"), p("args")).into(),
        "SI_updlistDispose" => topo::updlist_dispose(m, p("list")).into(),
        "relTopoSortInitChapter" => topo::rel_topo_sort_init_chapter(m, p("list"), b("fwd")).into(),
        "SI_relTopoSortInit" => topo::rel_topo_sort_init(m, p("ed"), b("fwd")).into(),
        "relCreateArguments" => {
            let (args, n) = topo::rel_create_arguments(m, p("ed"), p("list"), a.i("start"));
            m.set_i32(p("count"), n);
            args.into()
        }
        "topoSort" => {
            let mode = Mode { check_only: b("check"), forward: b("fwd"), roots: b("restricted") };
            topo::topo_sort(m, &mut Host { o }, p("rd"), p("ed"), p("upd"), p("elem"), p("rels"), a.i("chap"), a.i("index"), a.i("group"), a.i("level"), mode, b("pure")).into()
        }
        "elemdictTopoSortChapter" => topo::elemdict_topo_sort_chapter(m, &mut Host { o }, p("rd"), p("ed"), p("upd"), p("list"), a.i("chap"), b("check"), b("roots"), b("restricted")).into(),
        "SI_elemdictTopoSort" => topo::elemdict_topo_sort(m, &mut Host { o }, p("rd"), p("ed"), p("upd"), b("check"), b("restricted")).into(),
        "SI_elemdictTopoSortElement" => topo::elemdict_topo_sort_element(m, &mut Host { o }, p("rd"), p("ed"), a.i("chap"), a.i("index"), a.i("group"), b("check")).into(),
        "SI_elemdictCheckCycle" => topo::elemdict_check_cycle(m, &mut Host { o }, p("rd"), p("ed")).into(),
        "SI_elemdictUpdListSetDirty" => topo::elemdict_upd_list_set_dirty(m, p("ed")).into(),
        _ => return None,
    })
}
