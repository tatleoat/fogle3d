// SPDX-License-Identifier: MIT
//! Refreshing the scene after curves were edited: the last four functions of `fcv_fct.c`
//! (finding 0009). They are glue between the curve editor's list, the element dictionary and
//! the update list, all of which belong to other source files and appear here as the trait
//! [`SceneUpdate`]; what is reproduced is which calls are made, in what order, with what.

use crate::eval::frame_time;
use crate::{kind, Env, Fcurve};

/// What `SI_createUpdateListFcurves` builds: per chapter, in order of first appearance, the
/// indices of the elements to update, each once (two levels of singly linked lists in the
/// original, type `SI_listChapUpdate`).
pub type UpdateList = Vec<(i32, Vec<i32>)>;

/// The calls into the element dictionary, the update list and the windows.
pub trait SceneUpdate {
    /// For a custom-effect curve: the chapter and index of the element its relation leads to.
    /// The original looks the curve up with `SI_elemdictFindElemByData(dict, SI_cid, curve)`,
    /// takes the element (`SI_elemdictGetElem`), asks `SI_attdictGetInfoFromFcurve` for the
    /// curve type's group and calls `SI_elemdictFindRelatedElem(dict, elem, 0, 0, group, -1,
    /// &chapter, &index)`. `None` where a lookup fails or the chapter comes back as −1.
    fn custom_curve_element(&mut self, curve: &Fcurve) -> Option<(i32, i32)>;
    /// `SI_fcvGetChapidFromType(type)`: the chapter a curve type belongs to, or −1.
    fn chapter_of_type(&mut self, curve_type: i32) -> i32;
    /// `SI_elemdictFindElemByData(dict, chapter, owner)`: the index of the element whose data
    /// is the curve's owner, or −1.
    fn find_elem_by_data(&mut self, chapter: i32, owner: u32) -> i32;
    /// `SI_elemdictTopoSortElemByFcv(&SI_RD, dict, -1, 1, 0, list)`: zero if the dependencies
    /// form a cycle.
    fn topo_sort_by_fcv(&mut self, list: &UpdateList) -> i16;
    /// `SI_updListUpdate(&dict->updList, time)` (the update list at +0x4c of the dictionary).
    fn upd_list_update(&mut self, time: f32);
    /// `SI_itemUpdate(chapter, index, time)`; called while `SI_updateOption` is zero.
    fn item_update(&mut self, chapter: i32, index: i32, time: f32);
    /// `SI_refreshSystem(NULL, NULL, code)`.
    fn refresh_system(&mut self, code: i32);
}

/// The elements whose curves the editor shows [soft 0x0820aca0 SI_createUpdateListFcurves].
/// `curves` are the curves of the list's nodes of type 9 whose word at +8 is non-zero, in
/// list order. A custom-effect curve (type 566) leads to an element through its relation;
/// any other through its type's chapter and its owner.
pub fn create_update_list(curves: &[&Fcurve], scene: &mut dyn SceneUpdate) -> UpdateList {
    let mut list = UpdateList::new();
    for c in curves {
        let (chapter, index) = if c.kind == kind::CUSTOM {
            match scene.custom_curve_element(c) {
                Some(found) => found,
                None => continue,
            }
        } else {
            let chapter = scene.chapter_of_type(c.kind);
            if chapter == -1 {
                continue;
            }
            (chapter, scene.find_elem_by_data(chapter, c.owner))
        };
        if index == -1 {
            continue;
        }
        let at = list.iter().position(|(ch, _)| *ch == chapter).unwrap_or_else(|| {
            list.push((chapter, Vec::new()));
            list.len() - 1
        });
        if !list[at].1.contains(&index) {
            list[at].1.push(index);
        }
    }
    list
}

/// Updates the listed elements for the current frame and redraws
/// [soft 0x0820af2c SI_updateObjectsByFcurves].
///
/// With `SI_updateOption` zero: the elements are sorted by their dependencies (a cycle is
/// reported in the status line) and the scene's update list is run. Otherwise each listed
/// element is updated on its own, with the option switched off meanwhile. The time is
/// `(float)SI_frameCurrent / SI_frameRate`, rounded to `float`. Then the windows are
/// refreshed with the codes 0x37 and 0x17.
pub fn update_objects_by_fcurves(list: &UpdateList, scene: &mut dyn SceneUpdate, env: &mut Env) {
    let option = env.update_option;
    if option == 0 {
        if scene.topo_sort_by_fcv(list) == 0 {
            env.host.status_message(
                " A cycle has been detected in scene dependencies.\n Please Cut or Relax the latest relation you set.",
                3,
            );
        }
        scene.upd_list_update(frame_time(env.frame_current, env).to_f32());
    } else {
        env.update_option = 0;
        for (chapter, indices) in list {
            for &index in indices {
                scene.item_update(*chapter, index, frame_time(env.frame_current, env).to_f32());
            }
        }
        env.update_option = option;
    }
    scene.refresh_system(0x37);
    scene.refresh_system(0x17);
}

/// Frees a list [soft 0x0820b10c SI_deleteUpdateListFcurves].
pub fn delete_update_list(list: UpdateList) {
    drop(list);
}

/// After an edit in the curve editor, if `SI_refreshElemFcvOption` is set: build the list,
/// update, free it [soft 0x0820b16c SI_callRefreshElementByFcurve]. `curves` as for
/// [`create_update_list`] (the original walks the list at `SI_scngl` + 4).
pub fn call_refresh_element_by_fcurve(curves: &[&Fcurve], scene: &mut dyn SceneUpdate, env: &mut Env) {
    if env.refresh_elem_fcv_option != 0 {
        let list = create_update_list(curves, scene);
        update_objects_by_fcurves(&list, scene, env);
        delete_update_list(list);
    }
}
