// SPDX-License-Identifier: MIT
//! `custom_util.c`: the services of the custom effects (lists of effect records, value lists, dso
//! data, copying, lookups); finding 0041, "seventh continuation".

use si_mem::{clib, Mem, Outside, Routine};

pub const FGETC: Routine = Routine::new("plt:fgetc", 0x080c_3828);
pub const FCLOSE: Routine = Routine::new("plt:fclose", 0x080c_4958);
pub const SI_GET_EVENT: Routine = Routine::new("SI_getEvent", 0x0839_4058);
pub const SI_GL_SELECT: Routine = Routine::new("SI_gLSelect", 0x0832_ef0c);
pub const SI_FLASH_OBJECT: Routine = Routine::new("SI_flashObject", 0x0823_c9d8);
pub const SI_FCURVE_REMOVE: Routine = Routine::new("SI_fcurveRemove", 0x081f_f0a0);
pub const SI_NEXT_GRAFNODE: Routine = Routine::new("SI_nextGrafnode", 0x0833_53d4);
pub const SI_SHAPE_ADD_KEY: Routine = Routine::new("SI_shapeAddKey", 0x0824_2088);
pub const SI_ATTDICT_GET_ATT_GRP_BY_PATH: Routine = Routine::new("SI_attdictGetAttGrpByPath", 0x0812_0430);
pub const SI_ATTDICT_ATT_GRP_GET_ATTRIBUTE: Routine = Routine::new("SI_attdictAttGrpGetAttribute", 0x0811_f7cc);
pub const SI_ATTDICT_ATTRIBUTE_IS_ANIMATABLE: Routine = Routine::new("SI_attdictAttributeIsAnimatable", 0x0812_05fc);
pub const SI_ATTDICT_ATT_GRP_GET_NB_ATTRIBUTES: Routine = Routine::new("SI_attdictAttGrpGetNbAttributes", 0x0811_f7b4);
pub const SI_FCURVE_ALLOCATE: Routine = Routine::new("SI_fcurveAllocate", 0x0820_a810);
pub const SI_FKEY_FRAME_CREATE: Routine = Routine::new("SI_fkeyFrameCreate", 0x0820_b1b0);
pub const SI_FCURVE_ADD_KEY: Routine = Routine::new("SI_fcurveAddKey", 0x081f_f400);

pub const SI_PROC_DATA: u32 = 0x0867_9314;
pub const SI_GRAF_LISTS: u32 = 0x0867_9498;
pub const SI_FRAME_START: u32 = 0x0878_2050;
pub const SI_CUS_VAL_NUM_CALLBACK: u32 = 0x086e_49e8;
pub const SI_CUS_FCURVE_DIRTY: u32 = 0x086e_49e0;
pub const SI_ATD: u32 = 0x0878_1438;
/// The five custom status records (`SI_cusMotion`, `SI_cusModel`, `SI_cusMatter`, `SI_cusTools`, `SI_cusActor`).
pub const CUS_MOTION: u32 = 0x0882_2c60;
pub const CUS_MODEL: u32 = 0x0882_2b00;
pub const CUS_MATTER: u32 = 0x0882_29a0;
pub const CUS_TOOLS: u32 = 0x0882_2dc0;
pub const CUS_ACTOR: u32 = 0x0882_2840;

/// [soft 0x08547a30 si_cusLineRead]: reads one line into `buf` (at most `n` characters, no terminator
/// kept; a carriage return ends the line like a line feed). `buf` when a line ended or a character was
/// read, else 0 (end of file with nothing read).
pub fn si_cus_line_read(m: &mut Mem, o: &mut dyn Outside, fp: u32, buf: u32, n: i32) -> u32 {
    let mut i: i32 = 0;
    let mut status = 0;
    while i < n {
        let c = o.call(m, FGETC, &[fp]).eax as i32;
        if c == 10 || c == 13 {
            status = 1;
            break;
        }
        if c == -1 {
            status = -1;
            break;
        }
        m.set_u8(buf.wrapping_add(i as u32), c as u8);
        i += 1;
    }
    m.set_u8(buf.wrapping_add(i as u32), 0);
    if status == 1 || i > 0 {
        buf
    } else {
        0
    }
}

fn is_white(c: u8) -> bool {
    matches!(c, 9 | 10 | 13 | 32)
}

/// [soft 0x08547aac cusNextString]: copies the next blank-separated word of `src` to `dst`; the
/// position after it, or 0 when there was none.
pub fn cus_next_string(m: &mut Mem, src: u32, dst: u32) -> u32 {
    let mut p = src;
    while is_white(m.u8(p)) {
        p = p.wrapping_add(1);
    }
    let mut k: u32 = 0;
    loop {
        let c = m.u8(p);
        if is_white(c) || c == 0 {
            break;
        }
        m.set_u8(dst.wrapping_add(k), c);
        p = p.wrapping_add(1);
        k += 1;
    }
    m.set_u8(dst.wrapping_add(k), 0);
    if k == 0 {
        0
    } else {
        p
    }
}

/// [soft 0x08547b74 si_cusNumStrings]: the number of blank-separated words of `s` (the original copies
/// each into a 0x50-byte stack buffer; a longer word overruns it, here the buffer is made long enough).
pub fn si_cus_num_strings(m: &mut Mem, s: u32) -> i32 {
    let size = clib::strlen(m, s).wrapping_add(1).max(0x50);
    let buf = m.local(size);
    let mut n = 0;
    let mut p = s;
    loop {
        p = cus_next_string(m, p, buf);
        if p == 0 {
            break;
        }
        n += 1;
    }
    m.release(size);
    n
}

/// [soft 0x08547b04 si_cusGetMdl]: waits for events until a click (event 5, or 0 with +6 = 0x65 not set)
/// selects a model or an icon (selection kind 2 or 3), flashes it and answers its +4 word; 0 on
/// another kind of event, or a 0x65 key.
pub fn si_cus_get_mdl(m: &mut Mem, o: &mut dyn Outside) -> u32 {
    loop {
        let ev = o.call(m, SI_GET_EVENT, &[]).eax;
        let kind = m.u32(ev);
        if kind == 5 || kind == 0 {
            if m.u16(ev.wrapping_add(6)) == 0x65 {
                return 0;
            }
        } else {
            return 0;
        }
        let mut found = 0;
        if m.u32(ev) == 5 {
            let a = m.u32(ev.wrapping_add(0xc));
            let sel = o.call(m, SI_GL_SELECT, &[a]).eax;
            if sel != 0 && m.u32(sel).wrapping_sub(2) <= 1 {
                found = m.u32(sel.wrapping_add(4));
                o.call(m, SI_FLASH_OBJECT, &[sel, 5]);
            }
        }
        if found != 0 {
            return found;
        }
    }
}

/// [soft 0x08547bb0 SI_cusClosePipes]: for every command of kind 0x1b with a pipe (+0x18 >= 0) closes
/// both streams (+0x10, +0x14), clears them and sets the pipe to -1.
pub fn si_cus_close_pipes(m: &mut Mem, o: &mut dyn Outside, effect: u32) {
    if effect == 0 {
        return;
    }
    let mut c = m.u32(effect.wrapping_add(0xf8));
    while c != 0 {
        if m.u16(c) == 0x1b && (m.u32(c.wrapping_add(0x18)) as i32) >= 0 {
            let a = m.u32(c.wrapping_add(0x10));
            o.call(m, FCLOSE, &[a]);
            let b = m.u32(c.wrapping_add(0x14));
            m.set_u32(c.wrapping_add(0x10), 0);
            o.call(m, FCLOSE, &[b]);
            m.set_u32(c.wrapping_add(0x14), 0);
            m.set_u32(c.wrapping_add(0x18), u32::MAX);
        }
        c = m.u32(c.wrapping_add(0x30));
    }
}

/// [soft 0x0854a850 si_cusDsoDataCreate]: a new 0x44-byte dso record (counts 0, pointers 0, +0x34 = 1,
/// +0x38 = 0x32, +0x3a = 1); 0 when out of memory.
pub fn si_cus_dso_data_create(m: &mut Mem) -> u32 {
    let p = m.si_malloc(0x44);
    if p == 0 {
        return 0;
    }
    for off in [0u32, 0xc, 4, 0x14, 8, 0x10, 0x1c, 0x18, 0x20, 0x24] {
        m.set_u32(p.wrapping_add(off), 0);
    }
    m.set_u16(p.wrapping_add(0x30), 0);
    m.set_u32(p.wrapping_add(0x34), 1);
    m.set_u16(p.wrapping_add(0x38), 0x32);
    m.set_u16(p.wrapping_add(0x3a), 1);
    m.set_u16(p.wrapping_add(0x3c), 0);
    m.set_u32(p.wrapping_add(0x40), 0);
    p
}

/// [soft 0x0854b134 si_cusValueDelete]: frees the name (+0), the text (+0x18), removes the four fcurves
/// (cells +0x2c..+0x38) and frees the chain at +0x3c (linked by +0x14), then the value.
pub fn si_cus_value_delete(m: &mut Mem, o: &mut dyn Outside, v: u32) {
    let name = m.u32(v);
    if name != 0 {
        m.free(name);
    }
    let text = m.u32(v.wrapping_add(0x18));
    if text != 0 {
        m.free(text);
    }
    for k in 0..4u32 {
        let cell = v.wrapping_add(0x2c).wrapping_add(4 * k);
        if m.u32(cell) != 0 {
            o.call(m, SI_FCURVE_REMOVE, &[cell]);
        }
    }
    let mut p = m.u32(v.wrapping_add(0x3c));
    while p != 0 {
        let next = m.u32(p.wrapping_add(0x14));
        m.free(p);
        p = next;
    }
    if v != 0 {
        m.free(v);
    }
}

/// [soft 0x0854b104 si_cusValueListDelete]: deletes every value of the list (linked by +0x44).
pub fn si_cus_value_list_delete(m: &mut Mem, o: &mut dyn Outside, list: u32) {
    let mut p = list;
    while p != 0 {
        let next = m.u32(p.wrapping_add(0x44));
        si_cus_value_delete(m, o, p);
        p = next;
    }
}

/// [soft 0x0854af90 si_cusValueListCreate]: a new 0x50-byte value (type word -1, +8 = -1, links and
/// fcurve cells 0, +0x4c = -1); 0 when out of memory.
pub fn si_cus_value_list_create(m: &mut Mem) -> u32 {
    let p = m.si_malloc(0x50);
    if p == 0 {
        return 0;
    }
    m.set_u32(p.wrapping_add(0x44), 0);
    m.set_u32(p.wrapping_add(8), u32::MAX);
    m.set_u32(p, 0);
    m.set_u32(p.wrapping_add(0x18), 0);
    m.set_u16(p.wrapping_add(4), 0xffff);
    for off in [0x38u32, 0x34, 0x30, 0x2c, 0x3c, 0x40] {
        m.set_u32(p.wrapping_add(off), 0);
    }
    m.set_u16(p.wrapping_add(0x48), 0);
    m.set_u32(p.wrapping_add(0x4c), u32::MAX);
    p
}

/// [soft 0x0854b000 si_cusValueListCopy]: a copy of the value list (names and texts duplicated; the
/// fcurves, +0x2c.. and the chain at +0x3c are not copied); 0 when the list is empty or out of memory
/// (a partial copy is deleted).
pub fn si_cus_value_list_copy(m: &mut Mem, o: &mut dyn Outside, list: u32) -> u32 {
    let mut ok = true;
    let mut head = 0;
    let mut prev: u32 = 0;
    let mut src = list;
    while src != 0 {
        let n = si_cus_value_list_create(m);
        if n == 0 {
            ok = false;
        }
        if ok {
            if head == 0 {
                head = n;
            } else {
                m.set_u32(prev.wrapping_add(0x44), n);
            }
            prev = n;
            let name = m.u32(src);
            if name != 0 {
                let d = clib::strdup(m, name);
                m.set_u32(n, d);
                if d == 0 {
                    ok = false;
                }
            }
            if ok {
                let text = m.u32(src.wrapping_add(0x18));
                if text != 0 {
                    let d = clib::strdup(m, text);
                    m.set_u32(n.wrapping_add(0x18), d);
                    if d == 0 {
                        ok = false;
                    }
                }
            }
            if ok {
                let w = m.u16(src.wrapping_add(4));
                m.set_u16(n.wrapping_add(4), w);
                let v = m.u32(src.wrapping_add(8));
                m.set_u32(n.wrapping_add(8), v);
                let w = m.u16(src.wrapping_add(0xc));
                m.set_u16(n.wrapping_add(0xc), w);
                let v = m.u32(src.wrapping_add(0x10));
                m.set_u32(n.wrapping_add(0x10), crate::through_x87(v));
                for off in [0x14u32, 0x1c, 0x20, 0x24, 0x28, 0x4c] {
                    let v = m.u32(src.wrapping_add(off));
                    m.set_u32(n.wrapping_add(off), v);
                }
            }
        }
        src = m.u32(src.wrapping_add(0x44));
        if !ok {
            break;
        }
    }
    if !ok {
        si_cus_value_list_delete(m, o, head);
        head = 0;
    }
    head
}

/// [soft 0x0854a8d4 si_cusDsoDataCopy]: copies a dso record (the two int arrays of counts +4 and +0x14
/// and the two record arrays of 0x14 bytes for +0xc and +0x1c are duplicated). 1, or 0 for a null
/// argument or out of memory (the destination is then partly filled).
pub fn si_cus_dso_data_copy(m: &mut Mem, src: u32, dst: u32) -> i32 {
    if src == 0 || dst == 0 {
        return 0;
    }
    let w = m.u32(src);
    m.set_u32(dst, w);
    m.set_u32(dst.wrapping_add(0x10), 0);
    m.set_u32(dst.wrapping_add(8), 0);
    let n = m.u32(src.wrapping_add(4));
    m.set_u32(dst.wrapping_add(4), n);
    if (n as i32) > 0 {
        let p = m.si_malloc(n << 2);
        m.set_u32(dst.wrapping_add(8), p);
        if p == 0 {
            return 0;
        }
        let s = m.u32(src.wrapping_add(8));
        clib::memcpy(m, p, s, n << 2);
    }
    let n = m.u32(src.wrapping_add(0xc));
    m.set_u32(dst.wrapping_add(0xc), n);
    if (n as i32) > 0 {
        let p = m.si_malloc(n.wrapping_mul(0x14));
        m.set_u32(dst.wrapping_add(0x10), p);
        if p == 0 {
            return 0;
        }
        let s = m.u32(src.wrapping_add(0x10));
        clib::memcpy(m, p, s, n.wrapping_mul(0x14));
    }
    m.set_u32(dst.wrapping_add(0x20), 0);
    m.set_u32(dst.wrapping_add(0x18), 0);
    let n = m.u32(src.wrapping_add(0x14));
    m.set_u32(dst.wrapping_add(0x14), n);
    if (n as i32) > 0 {
        let p = m.si_malloc(n << 2);
        m.set_u32(dst.wrapping_add(0x18), p);
        if p == 0 {
            return 0;
        }
        let s = m.u32(src.wrapping_add(0x18));
        clib::memcpy(m, p, s, n << 2);
    }
    let n = m.u32(src.wrapping_add(0x1c));
    m.set_u32(dst.wrapping_add(0x1c), n);
    if (n as i32) > 0 {
        let p = m.si_malloc(n.wrapping_mul(0x14));
        m.set_u32(dst.wrapping_add(0x20), p);
        if p == 0 {
            return 0;
        }
        let s = m.u32(src.wrapping_add(0x20));
        clib::memcpy(m, p, s, n.wrapping_mul(0x14));
    }
    for off in [0x24u32, 0x28, 0x2c, 0x34, 0x40] {
        let v = m.u32(src.wrapping_add(off));
        m.set_u32(dst.wrapping_add(off), v);
    }
    for off in [0x30u32, 0x38, 0x3a] {
        let v = m.u16(src.wrapping_add(off));
        m.set_u16(dst.wrapping_add(off), v);
    }
    m.set_u16(dst.wrapping_add(0x3c), 0);
    1
}

/// [soft 0x0854a7c0 si_cusFindModelByName]: walks the graphics list of the current scene for a model
/// node (kind 2 or 3) whose name is `name`; the model, or (kept) the last model looked at, 0 when none.
pub fn si_cus_find_model_by_name(m: &mut Mem, o: &mut dyn Outside, name: u32) -> u32 {
    let cell = m.local(4);
    m.set_u32(cell, 0);
    let mut found = 0;
    loop {
        let idx = m.u32(m.u32(SI_PROC_DATA).wrapping_add(4));
        let list = m.u32(SI_GRAF_LISTS).wrapping_add(idx.wrapping_mul(0x24));
        o.call(m, SI_NEXT_GRAFNODE, &[list, cell]);
        let node = m.u32(cell);
        if node == 0 {
            break;
        }
        if m.u32(node).wrapping_sub(2) > 1 {
            continue;
        }
        found = m.u32(node.wrapping_add(4));
        let n = m.u32(found.wrapping_add(4));
        if clib::strcmp(m, n, name) == 0 {
            break;
        }
    }
    m.release(4);
    found
}

pub const SI_MODEL_UPDATE_DEPENDENCY: Routine = Routine::new("SI_modelUpdateDependency", 0x0846_f200);
pub const SI_MODEL_ALLOCATE: Routine = Routine::new("SI_modelAllocate", 0x0847_7cf4);
pub const SI_MODEL_CREATE_PARENTSHIP: Routine = Routine::new("SI_modelCreateParentship", 0x0847_7c90);

/// [soft 0x08548cc4 si_cusCreateShape]: adds a shape key to the model (0 when its +0x34 word is 0),
/// sets its dirty flags (+10 bit 4, +0x3fa bit 4) and, with `children`, does the same down the first
/// child (+0x2c0), else the next sibling (+0x2c4). 1 when done.
pub fn si_cus_create_shape(m: &mut Mem, o: &mut dyn Outside, model: u32, children: i16) -> i32 {
    if m.u16(model.wrapping_add(0x34)) == 0 {
        return 0;
    }
    o.call(m, SI_SHAPE_ADD_KEY, &[model, 0, 0]);
    let b = m.u8(model.wrapping_add(10));
    m.set_u8(model.wrapping_add(10), b | 0x10);
    let b = m.u8(model.wrapping_add(0x3fa));
    m.set_u8(model.wrapping_add(0x3fa), b | 0x10);
    if children == 0 {
        return 1;
    }
    let mut next = m.u32(model.wrapping_add(0x2c0));
    if next == 0 {
        next = m.u32(model.wrapping_add(0x2c4));
        if next == 0 {
            return 1;
        }
    }
    si_cus_create_shape(m, o, next, children)
}

/// [soft 0x08548d38 si_cusTransferSrt]: copies scale, rotation and translation (+0x100..+0x114 and
/// +0x158..+0x160) from `src` to `dst`, then the same down both children (+0x2c0) and siblings
/// (+0x2c4) that exist in both. 1, or -1 when a recursive call fails.
pub fn si_cus_transfer_srt(m: &mut Mem, src: u32, dst: u32) -> i32 {
    for off in [0x100u32, 0x104, 0x108, 0x10c, 0x110, 0x114, 0x158, 0x15c, 0x160] {
        let v = m.u32(src.wrapping_add(off));
        m.set_u32(dst.wrapping_add(off), v);
    }
    for off in [0x2c0u32, 0x2c4] {
        let (a, b) = (m.u32(src.wrapping_add(off)), m.u32(dst.wrapping_add(off)));
        if a != 0 && b != 0 && (si_cus_transfer_srt(m, a, b) as i32) < 0 {
            return -1;
        }
    }
    1
}

/// [soft 0x08548e54 cusRecursiveHrcReplicate]: for `src` a new model of its name (+4) made a child of
/// `parent`, then the same for its children (+0x2c0, under `parent`) and siblings (+0x2c4, under the new model).
pub fn cus_recursive_hrc_replicate(m: &mut Mem, o: &mut dyn Outside, src: u32, parent: u32) {
    if src == 0 {
        return;
    }
    let name = m.u32(src.wrapping_add(4));
    let n = o.call(m, SI_MODEL_ALLOCATE, &[name]).eax;
    o.call(m, SI_MODEL_CREATE_PARENTSHIP, &[parent, n]);
    let c = m.u32(src.wrapping_add(0x2c0));
    if c != 0 {
        cus_recursive_hrc_replicate(m, o, c, parent);
    }
    let s = m.u32(src.wrapping_add(0x2c4));
    if s != 0 {
        cus_recursive_hrc_replicate(m, o, s, n);
    }
}

/// [soft 0x08548e10 si_cusHrcReplicate]: a new model of the name of `src` with a copy of the hierarchy
/// below it (+0x2c4).
pub fn si_cus_hrc_replicate(m: &mut Mem, o: &mut dyn Outside, src: u32) -> u32 {
    if src == 0 {
        return 0;
    }
    let name = m.u32(src.wrapping_add(4));
    let n = o.call(m, SI_MODEL_ALLOCATE, &[name]).eax;
    let s = m.u32(src.wrapping_add(0x2c4));
    if s != 0 {
        cus_recursive_hrc_replicate(m, o, s, n);
    }
    n
}

/// [soft 0x0854a3ec cusPatchTransfer]: copies the control points (16 bytes each, `w * h` of them,
/// the array at +0x1a4) of one patch to another.
pub fn cus_patch_transfer(m: &mut Mem, src: u32, dst: u32) {
    let mut s = m.u32(src.wrapping_add(0x1a4));
    let mut d = m.u32(dst.wrapping_add(0x1a4));
    let mut n = m.i16(src.wrapping_add(0xc)).wrapping_mul(m.i16(src.wrapping_add(0xe)));
    while n != 0 {
        for k in 0..4u32 {
            let v = m.u32(s.wrapping_add(4 * k));
            m.set_u32(d.wrapping_add(4 * k), v);
        }
        s = s.wrapping_add(16);
        d = d.wrapping_add(16);
        n = n.wrapping_sub(1);
    }
}

/// [soft 0x085495d0 si_cusFreeCommandList]: for each command of the effect's list (+0x30 of the list
/// record): kinds 0x18 0x19 0x37 0x42 0x46 take one use off the model they name (+0x352) and update its
/// dependency; kinds 0x20 0x21 take one off the model at +0x1c (+0x350); then frees the commands and
/// the three texts (+4 +8 +0xc) and the +0x24 block when its count (+0x20) is positive.
pub fn si_cus_free_command_list(m: &mut Mem, o: &mut dyn Outside, list: u32) {
    if list == 0 {
        return;
    }
    let mut c = m.u32(list.wrapping_add(0x30));
    while c != 0 {
        match m.u16(c) {
            0x18 | 0x19 | 0x37 | 0x42 | 0x46 => {
                let q = m.u32(c.wrapping_add(0x28));
                if q != 0 {
                    let model = m.u32(q.wrapping_add(0x1c));
                    if model != 0 {
                        let v = m.u16(model.wrapping_add(0x352)).wrapping_sub(1);
                        m.set_u16(model.wrapping_add(0x352), v);
                        o.call(m, SI_MODEL_UPDATE_DEPENDENCY, &[model, 0]);
                    }
                }
            }
            0x20 | 0x21 => {
                let model = m.u32(c.wrapping_add(0x1c));
                if model != 0 {
                    let v = m.u16(model.wrapping_add(0x350)).wrapping_sub(1);
                    m.set_u16(model.wrapping_add(0x350), v);
                }
            }
            _ => {}
        }
        c = m.u32(c.wrapping_add(0x30));
    }
    let mut c = m.u32(list.wrapping_add(0x30));
    while c != 0 {
        for off in [4u32, 8, 0xc] {
            let p = m.u32(c.wrapping_add(off));
            if p != 0 {
                m.free(p);
            }
        }
        if (m.u32(c.wrapping_add(0x20)) as i32) > 0 {
            let p = m.u32(c.wrapping_add(0x24));
            if p != 0 {
                m.free(p);
            }
        }
        let next = m.u32(c.wrapping_add(0x30));
        m.free(c);
        c = next;
    }
    m.set_u32(list.wrapping_add(0x30), 0);
}

/// [soft 0x0854a2e8 SI_cusRemoveEffect]: unlinks (from the list at the status' +0x14c, linked by +0xfc)
/// the first effect whose +0xc is `model` and returns it; 0 when none or a null argument.
pub fn si_cus_remove_effect(m: &mut Mem, status: u32, model: u32) -> u32 {
    if status == 0 || model == 0 {
        return 0;
    }
    let mut prev = m.u32(status.wrapping_add(0x14c));
    while m.u32(prev.wrapping_add(0xfc)) != 0 {
        let e = m.u32(prev.wrapping_add(0xfc));
        if m.u32(e.wrapping_add(0xc)) == model {
            let next = m.u32(e.wrapping_add(0xfc));
            m.set_u32(prev.wrapping_add(0xfc), next);
            m.set_u32(e.wrapping_add(0xfc), 0);
            return e;
        }
        prev = e;
    }
    0
}

/// [soft 0x0854b1e0 SI_cusContextInit]: fills a 0x20-byte context (+0 `a3`, +4 `a2`, +8 `a5`, +0xc and
/// +0x10 the first two words of `SI_procData`'s record, +0x14/+0x18 0, +0x1c `a4`). 1 when `p` is not null.
pub fn si_cus_context_init(m: &mut Mem, p: u32, a2: u32, a3: u32, a4: u32, a5: u32) -> i32 {
    if p == 0 {
        return 0;
    }
    m.set_u32(p, a3);
    let pd = m.u32(SI_PROC_DATA);
    m.set_u32(p.wrapping_add(0x1c), a4);
    m.set_u32(p.wrapping_add(4), a2);
    let (w0, w1) = (m.u32(pd), m.u32(pd.wrapping_add(4)));
    m.set_u32(p.wrapping_add(0xc), w0);
    m.set_u32(p.wrapping_add(0x10), w1);
    m.set_u32(p.wrapping_add(8), a5);
    m.set_u32(p.wrapping_add(0x14), 0);
    m.set_u32(p.wrapping_add(0x18), 0);
    1
}

/// [soft 0x0854a3d0 cusFreeDialog]: deletes the value list of a dialog record (+0x54).
pub fn cus_free_dialog(m: &mut Mem, o: &mut dyn Outside, dialog: u32) {
    let l = m.u32(dialog.wrapping_add(0x54));
    si_cus_value_list_delete(m, o, l);
}

/// [soft 0x0854aa64 si_cusDsoDataDelete]: frees the four arrays (+0x10 +0x20 +8 +0x18, each cleared)
/// and the record.
pub fn si_cus_dso_data_delete(m: &mut Mem, d: u32) {
    if d == 0 {
        return;
    }
    for off in [0x10u32, 0x20, 8, 0x18] {
        let p = m.u32(d.wrapping_add(off));
        if p != 0 {
            m.free(p);
            m.set_u32(d.wrapping_add(off), 0);
        }
    }
    m.free(d);
}

/// [soft 0x0854a348 SI_cusDeleteEffect]: closes the pipes, frees the dialog's values (+0x10 record),
/// the command list (+0xc8), the dso data (+0x104), the name (+4) and the text (+0x10c), the effect.
pub fn si_cus_delete_effect(m: &mut Mem, o: &mut dyn Outside, e: u32) {
    if e == 0 {
        return;
    }
    si_cus_close_pipes(m, o, e);
    cus_free_dialog(m, o, e.wrapping_add(0x10));
    si_cus_free_command_list(m, o, e.wrapping_add(200));
    let d = m.u32(e.wrapping_add(0x104));
    if d != 0 {
        si_cus_dso_data_delete(m, d);
    }
    for off in [4u32, 0x10c] {
        let p = m.u32(e.wrapping_add(off));
        if p != 0 {
            m.free(p);
        }
    }
    m.free(e);
}

/// [soft 0x0854abe4 si_cusDialogDsoDataDelete]: does nothing.
pub fn si_cus_dialog_dso_data_delete() {}

/// [soft 0x0854abec si_cusGetIconName]: the text (+4) of the first command of kind 0x1e of the effect, else 0.
pub fn si_cus_get_icon_name(m: &mut Mem, e: u32) -> u32 {
    if e != 0 {
        let mut c = m.u32(e.wrapping_add(0xf8));
        while c != 0 {
            if m.u16(c) == 0x1e {
                return m.u32(c.wrapping_add(4));
            }
            c = m.u32(c.wrapping_add(0x30));
        }
    }
    0
}

/// [soft 0x0854acf4 si_cusStatusGetEffectByIcon]: the effect of the status whose +0xc is `icon`, else 0.
pub fn si_cus_status_get_effect_by_icon(m: &mut Mem, status: u32, icon: u32) -> u32 {
    if status == 0 || icon == 0 {
        return 0;
    }
    let mut e = m.u32(status.wrapping_add(0x14c));
    loop {
        e = m.u32(e.wrapping_add(0xfc));
        if e == 0 {
            return 0;
        }
        if m.u32(e.wrapping_add(0xc)) == icon {
            return e;
        }
    }
}

/// [soft 0x0854ac2c SI_cusGetEffectByIcon]: looks in the motion, model, actor, tools and matter status
/// records in that order; the effect, with the record it was found in stored through `out`.
pub fn si_cus_get_effect_by_icon(m: &mut Mem, icon: u32, out: u32) -> u32 {
    if icon == 0 || out == 0 {
        return 0;
    }
    for s in [CUS_MOTION, CUS_MODEL, CUS_ACTOR, CUS_TOOLS, CUS_MATTER] {
        let e = si_cus_status_get_effect_by_icon(m, s, icon);
        if e != 0 {
            m.set_u32(out, s);
            return e;
        }
    }
    0
}

/// [soft 0x0854adf4 SI_cusStatusGetEffectByName]: the effect called `name` in any record of the
/// status (records linked by +0x120 from +0x148, effects by +0xfc from +0x100), else 0.
pub fn si_cus_status_get_effect_by_name(m: &mut Mem, status: u32, name: u32) -> u32 {
    if status == 0 || name == 0 {
        return 0;
    }
    let mut r = m.u32(status.wrapping_add(0x148));
    while r != 0 {
        let mut e = m.u32(r.wrapping_add(0x100));
        while e != 0 {
            let n = m.u32(e.wrapping_add(4));
            if clib::strcmp(m, n, name) == 0 {
                return e;
            }
            e = m.u32(e.wrapping_add(0xfc));
        }
        r = m.u32(r.wrapping_add(0x120));
    }
    0
}

/// [soft 0x0854ad34 SI_cusGetEffectByName]: as SI_cusGetEffectByIcon by name; `*out` is cleared first.
pub fn si_cus_get_effect_by_name(m: &mut Mem, name: u32, out: u32) -> u32 {
    m.set_u32(out, 0);
    if name == 0 {
        return 0;
    }
    for s in [CUS_MOTION, CUS_MODEL, CUS_ACTOR, CUS_TOOLS, CUS_MATTER] {
        let e = si_cus_status_get_effect_by_name(m, s, name);
        if e != 0 {
            m.set_u32(out, s);
            return e;
        }
    }
    0
}

/// [soft 0x0854ae7c si_cusEffectIsProcessBased]: 1 when the effect has a command that is neither kind
/// 0x1e nor kind 1.
pub fn si_cus_effect_is_process_based(m: &mut Mem, e: u32) -> i32 {
    if e != 0 {
        let mut c = m.u32(e.wrapping_add(0xf8));
        while c != 0 {
            let k = m.u16(c);
            if k != 0x1e && k != 1 {
                return 1;
            }
            c = m.u32(c.wrapping_add(0x30));
        }
    }
    0
}

/// [soft 0x0854aec4 SI_cusGetReleaseTitle]: the address of the release title text "3Dv4.00.1".
pub fn si_cus_get_release_title() -> u32 {
    0x086e_4624
}

/// [soft 0x0854aed0 si_cusConcatStrings]: a new block holding the strings (null ones skipped) one after
/// the other; 0 when out of memory.
pub fn si_cus_concat_strings(m: &mut Mem, strings: &[u32]) -> u32 {
    let mut total: u32 = 0;
    for &s in strings {
        if s != 0 {
            total = total.wrapping_add(clib::strlen(m, s));
        }
    }
    let p = m.si_malloc(total.wrapping_add(1));
    if p != 0 {
        m.set_u8(p, 0);
        for &s in strings {
            if s != 0 {
                clib::strcat(m, p, s);
            }
        }
    }
    p
}

/// [soft 0x08547cbc SI_cusCreateFcurves]: for every animatable attribute of the effect class' attribute
/// group that a value of the effect's list (+0x64) names, makes the value's fcurve (+0x2c) when it has
/// none and, for a value of type 1, a first key at the start frame. 1, or 0 when the effect has no dso
/// data or no class path or an fcurve cannot be made.
pub fn si_cus_create_fcurves(m: &mut Mem, o: &mut dyn Outside, e: u32) -> i32 {
    use crate::custom_fcv::si_cus_effect_class_get_pathname;
    if e == 0 {
        return 0;
    }
    let dso = m.u32(e.wrapping_add(0x104));
    if dso == 0 || m.u32(dso) == 0 {
        return 0;
    }
    let cell = m.local(4);
    m.set_u32(cell, 0);
    let class = m.u32(e.wrapping_add(0xc));
    let ok = si_cus_effect_class_get_pathname(m, o, class, cell) as u16;
    if ok == 0 {
        m.release(4);
        return 0;
    }
    let path = m.u32(cell);
    let grp = o.call(m, SI_ATTDICT_GET_ATT_GRP_BY_PATH, &[SI_ATD, path]).eax;
    if path != 0 {
        m.free(path);
        m.set_u32(cell, 0);
    }
    m.release(4);
    if grp != 0 {
        let mut i: i32 = 0;
        while i < o.call(m, SI_ATTDICT_ATT_GRP_GET_NB_ATTRIBUTES, &[grp]).eax as i32 {
            let att = o.call(m, SI_ATTDICT_ATT_GRP_GET_ATTRIBUTE, &[SI_ATD, grp, i as u32]).eax;
            if att != 0 && o.call(m, SI_ATTDICT_ATTRIBUTE_IS_ANIMATABLE, &[att]).eax as u16 != 0 {
                let mut v = m.u32(e.wrapping_add(0x64));
                while v != 0 {
                    let (a, b) = (m.u32(att.wrapping_add(0x20)), m.u32(v));
                    if clib::strcmp(m, a, b) == 0 {
                        if m.u32(v.wrapping_add(0x2c)) == 0 {
                            let fc = o.call(m, SI_FCURVE_ALLOCATE, &[0x236, e]).eax;
                            m.set_u32(v.wrapping_add(0x2c), fc);
                            if fc == 0 {
                                return 0;
                            }
                            if m.u16(v.wrapping_add(4)) == 1 {
                                let start = m.u32(SI_FRAME_START);
                                let val = m.u32(v.wrapping_add(0x10));
                                let ftype = m.i16(fc.wrapping_add(0x1e)) as i32 as u32;
                                let key = o.call(m, SI_FKEY_FRAME_CREATE, &[start, val, 0, 0, ftype]).eax;
                                let fc = m.u32(v.wrapping_add(0x2c));
                                o.call(m, SI_FCURVE_ADD_KEY, &[fc, key]);
                                if (m.u32(SI_CUS_VAL_NUM_CALLBACK) as i32) > 0 {
                                    let fc = m.u32(v.wrapping_add(0x2c));
                                    m.set_u16(fc.wrapping_add(0x62), 0);
                                    let d = m.u32(SI_CUS_FCURVE_DIRTY).wrapping_sub(1);
                                    m.set_u32(SI_CUS_FCURVE_DIRTY, d);
                                }
                            }
                        }
                        break;
                    }
                    v = m.u32(v.wrapping_add(0x44));
                }
            }
            i += 1;
        }
    }
    1
}

/// [soft 0x0854b230 si_cusEffectCreate]: a new zeroed 0x11c-byte effect record with its defaults (kind
/// ids -1 at +2 and +0x10, "latest" link +0x110 pointing at itself, +0x100 set to 1); 0 when out of memory.
pub fn si_cus_effect_create(m: &mut Mem) -> u32 {
    let p = m.si_calloc(1, 0x11c);
    if p == 0 {
        return 0;
    }
    m.set_u16(p, 0);
    m.set_u16(p.wrapping_add(2), 0xffff);
    m.set_u32(p.wrapping_add(4), 0);
    m.set_u16(p.wrapping_add(8), 0);
    m.set_u32(p.wrapping_add(0xc), 0);
    m.set_u16(p.wrapping_add(0x10), 0xffff);
    for off in [0x18u32, 0x1c, 0x14, 0x64, 0xb4, 0xf8, 0xfc] {
        m.set_u32(p.wrapping_add(off), 0);
    }
    m.set_u32(p.wrapping_add(0x100), 1);
    for off in [0x104u32, 0x108, 0x10c] {
        m.set_u32(p.wrapping_add(off), 0);
    }
    m.set_u32(p.wrapping_add(0x110), p);
    m.set_u32(p.wrapping_add(0x114), 0);
    m.set_u16(p.wrapping_add(0x118), 0);
    p
}

/// The version and stem length `SI_cusEffectGetVersion` finds in a name's bytes.
fn parse_version(bytes: &[u8]) -> (i32, u32) {
    if let Some(d) = bytes.iter().rposition(|&c| c == b'.') {
        if bytes.get(d + 1) == Some(&b'v') && bytes.get(d + 2).is_some() {
            // strtoul(digits, &end, 10): blanks, a sign, then decimal digits (the C library's reading)
            let rest = &bytes[d + 2..];
            let mut i = 0;
            while i < rest.len() && matches!(rest[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
                i += 1;
            }
            let neg = rest.get(i) == Some(&b'-');
            if matches!(rest.get(i), Some(b'-' | b'+')) {
                i += 1;
            }
            let first = i;
            let mut v: i128 = 0;
            while i < rest.len() && rest[i].is_ascii_digit() {
                v = (v * 10 + (rest[i] - b'0') as i128).min(1i128 << 100);
                i += 1;
            }
            let (v, used) = if i == first { (0i128, 0usize) } else { (if neg { -v } else { v }, i) };
            let r: i32 = if v.abs() > u32::MAX as i128 { u32::MAX as i32 } else { v as u32 as i32 };
            if used == rest.len() && r >= 0 {
                return (r, d as u32);
            }
        }
    }
    (0, bytes.len() as u32)
}

/// [soft 0x0854b2f4 SI_cusEffectGetVersion]: the version a name carries as its last `.vN` suffix (N all
/// digits, not negative); 0 when it has none. `stem` (when not null) receives the length of the name
/// without the suffix (the whole length when there is none).
pub fn si_cus_effect_get_version(m: &mut Mem, name: u32, stem: u32) -> i32 {
    let bytes = m.cstr(name);
    let (v, s) = parse_version(&bytes);
    if stem != 0 {
        m.set_u32(stem, s);
    }
    v
}

/// [soft 0x0854b4d8 cusEffectStatusGetLatest]: the "latest version" link (+0x110) of the first effect in
/// the status' groups (+0x148, next group at +0x120, effect chain at +0x100, next effect at +0xfc)
/// whose name starts with the first `stem` characters of `name` and has exactly that stem length; else 0.
pub fn cus_effect_status_get_latest(m: &mut Mem, status: u32, name: u32, stem: u32) -> u32 {
    let mut found = 0;
    let mut g = m.u32(status.wrapping_add(0x148));
    while g != 0 {
        let mut e = m.u32(g.wrapping_add(0x100));
        while e != 0 && found == 0 {
            let en = m.u32(e.wrapping_add(4));
            if clib::strncmp(m, en, name, stem) == 0 {
                let (_, t) = parse_version(&m.cstr(en));
                if t == stem {
                    found = m.u32(e.wrapping_add(0x110));
                }
            }
            e = m.u32(e.wrapping_add(0xfc));
        }
        g = m.u32(g.wrapping_add(0x120));
        if found != 0 {
            break;
        }
    }
    found
}

/// [soft 0x0854b4a4 si_cusEffectStatusGetLatest]: `cusEffectStatusGetLatest` for the stem length of `name`.
pub fn si_cus_effect_status_get_latest(m: &mut Mem, status: u32, name: u32) -> u32 {
    let (_, stem) = parse_version(&m.cstr(name));
    cus_effect_status_get_latest(m, status, name, stem)
}

/// [soft 0x0854b404 si_cusEffectGetLatest]: the latest-version link for `name` among the model, motion,
/// actor, matter and tools effects, in that order; 0 when none.
pub fn si_cus_effect_get_latest(m: &mut Mem, name: u32) -> u32 {
    let (_, stem) = parse_version(&m.cstr(name));
    for st in [CUS_MODEL, CUS_MOTION, CUS_ACTOR, CUS_MATTER, CUS_TOOLS] {
        let r = cus_effect_status_get_latest(m, st, name, stem);
        if r != 0 {
            return r;
        }
    }
    0
}

/// [soft 0x0854b564 SI_cusGetStatusByName]: the custom status record (motion, model, matter, tools,
/// actor, tested in that order) whose name (at +2) equals `name`; 0 for none or a null name.
pub fn si_cus_get_status_by_name(m: &mut Mem, name: u32) -> u32 {
    if name == 0 {
        return 0;
    }
    for st in [CUS_MOTION, CUS_MODEL, CUS_MATTER, CUS_TOOLS, CUS_ACTOR] {
        if clib::strcmp(m, name, st.wrapping_add(2)) == 0 {
            return st;
        }
    }
    0
}

/// Copies the string at `src[off]` (when set) into the new command's field `off` (libc `strdup`).
fn dup_field(m: &mut Mem, src: u32, dst: u32, off: u32) {
    let s = m.u32(src.wrapping_add(off));
    if s != 0 {
        let d = clib::strdup(m, s);
        m.set_u32(dst.wrapping_add(off), d);
    }
}

/// [soft 0x08547e40 SI_cusCopyEffect]: copies the effect `src` to the record `dst`: the plain fields, a
/// copy of the name and directory, a new dso data copied from the source's and a copy of the value list;
/// the commands are then copied one by one. With `all` clear every command is copied as it is. With it
/// set the commands that run on the effect itself keep their kind, the ones that name a model (icon
/// reads of kinds 4 to 8 and 0x12, 9 and 10, 0x1e) become the shape-bound kinds 0x20 or 0x21 and share
/// the model (its use count at +0x350 grows), the type record (0xd) becomes 0x22 and gives its
/// two words at +0x20 to the copy, the process record (0x1b) copies its words, and kinds that
/// are listed nowhere are left out. The name links are resolved at the end.
pub fn si_cus_copy_effect(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32, all: i16) {
    let a = |off: u32| src.wrapping_add(off);
    let b = |off: u32| dst.wrapping_add(off);
    let k = m.u16(src);
    m.set_u16(dst, k);
    let k = m.u16(a(2));
    m.set_u16(b(2), k);
    let s = m.u32(a(4));
    let d = clib::strdup(m, s);
    m.set_u32(b(4), d);
    let k = m.u16(a(8));
    m.set_u16(b(8), k);
    let v = m.u32(a(0xc));
    m.set_u32(b(0xc), v);
    let v = m.u32(a(0x100));
    m.set_u32(b(0x100), v);
    m.set_u32(b(0xf8), 0);
    let v = m.u32(a(0x108));
    m.set_u32(b(0x108), v);
    let s = m.u32(a(0x10c));
    let d = clib::strdup(m, s);
    m.set_u32(b(0x10c), d);
    let v = m.u32(a(0x110));
    m.set_u32(b(0x110), v);
    let v = m.u32(a(0x114));
    m.set_u32(b(0x114), v);
    let k = m.u16(a(0x118));
    m.set_u16(b(0x118), k);
    let dso = si_cus_dso_data_create(m);
    m.set_u32(b(0x104), dso);
    let sdso = m.u32(a(0x104));
    si_cus_dso_data_copy(m, sdso, dso);
    for off in [0x14u32, 0x18, 0x1c, 0xc0, 0xc4] {
        let v = m.u32(a(off));
        m.set_u32(b(off), v);
    }
    let k = m.u16(a(0x10));
    m.set_u16(b(0x10), k);
    let l = m.u32(a(0x64));
    let c = si_cus_value_list_copy(m, o, l);
    m.set_u32(b(0x64), c);
    m.set_u32(b(0xb4), 0);
    let mut c = m.u32(a(0xf8));
    while c != 0 {
        if all == 0 {
            let n = crate::custom_init::si_cus_new_cmd(m, dst);
            for off in [4u32, 8, 0xc] {
                dup_field(m, c, n, off);
            }
            let k = m.u16(c);
            m.set_u16(n, k);
        } else {
            let kind = m.u16(c);
            match kind {
                2 | 0xc | 0xe | 0xf | 0x10 | 0x11 | 0x14..=0x19 | 0x1d | 0x1f | 0x23..=0x30 | 0x37..=0x46 | 0x49 | 0x4b | 0x4c | 0x4d => {
                    let n = crate::custom_init::si_cus_new_cmd(m, dst);
                    m.set_u16(n, kind);
                    for off in [4u32, 8, 0xc] {
                        dup_field(m, c, n, off);
                    }
                }
                4..=8 | 0x12 | 9 | 10 => {
                    let n = crate::custom_init::si_cus_new_cmd(m, dst);
                    m.set_u16(n, if kind == 0x12 || kind == 9 || kind == 10 { 0x21 } else { 0x20 });
                    let model = m.u32(c.wrapping_add(0x1c));
                    if kind <= 8 || kind == 0x12 {
                        m.set_u32(n.wrapping_add(0x1c), model);
                        if model != 0 {
                            let u = m.u16(model.wrapping_add(0x350)).wrapping_add(1);
                            m.set_u16(model.wrapping_add(0x350), u);
                        }
                    } else {
                        if model != 0 {
                            let u = m.u16(model.wrapping_add(0x350)).wrapping_add(1);
                            m.set_u16(model.wrapping_add(0x350), u);
                        }
                        m.set_u32(n.wrapping_add(0x1c), model);
                    }
                    for off in [4u32, 8, 0xc] {
                        dup_field(m, c, n, off);
                    }
                }
                0xd => {
                    let n = crate::custom_init::si_cus_new_cmd(m, dst);
                    m.set_u16(n, 0x22);
                    let v = m.u32(c.wrapping_add(0x1c));
                    m.set_u32(n.wrapping_add(0x1c), v);
                    for off in [4u32, 8, 0xc] {
                        dup_field(m, c, n, off);
                    }
                    for off in [0x20u32, 0x24] {
                        let v = m.u32(c.wrapping_add(off));
                        m.set_u32(n.wrapping_add(off), v);
                        m.set_u32(c.wrapping_add(off), 0);
                    }
                }
                0x1b => {
                    let n = crate::custom_init::si_cus_new_cmd(m, dst);
                    m.set_u16(n, kind);
                    for off in [4u32, 8, 0xc] {
                        dup_field(m, c, n, off);
                    }
                    for off in [0x10u32, 0x14, 0x1c, 0x18] {
                        let v = m.u32(c.wrapping_add(off));
                        m.set_u32(n.wrapping_add(off), v);
                    }
                }
                0x1e => {
                    let n = crate::custom_init::si_cus_new_cmd(m, dst);
                    m.set_u16(n, 0x20);
                    let v = m.u32(c.wrapping_add(0x1c));
                    m.set_u32(n.wrapping_add(0x1c), v);
                    for off in [4u32, 8, 0xc] {
                        dup_field(m, c, n, off);
                    }
                }
                _ => {}
            }
        }
        c = m.u32(c.wrapping_add(0x30));
    }
    crate::custom_init::si_cus_resolve_name_links(m, o, dst);
}

/// [soft 0x08547c14 si_cusAddMotionEffect]: copies the effect `eff` (it needs a model, +0xc) into a new
/// record linked into the status' motion effect chain (+0x14c): behind the last effect when `at` is
/// negative, else behind the `at`-th, and marks the source as used (+0x10 = 1, +0x100 = 1). Returns the new
/// record, or null without a model.
pub fn si_cus_add_motion_effect(m: &mut Mem, o: &mut dyn Outside, status: u32, eff: u32, at: i32) -> u32 {
    if m.u32(eff.wrapping_add(0xc)) == 0 {
        return 0;
    }
    let mut p = m.u32(status.wrapping_add(0x14c));
    if at < 0 {
        while m.u32(p.wrapping_add(0xfc)) != 0 {
            p = m.u32(p.wrapping_add(0xfc));
        }
    } else {
        let mut n = at;
        let mut nx = m.u32(p.wrapping_add(0xfc));
        while nx != 0 && n > 0 {
            p = nx;
            nx = m.u32(p.wrapping_add(0xfc));
            n -= 1;
        }
    }
    let after = m.u32(p.wrapping_add(0xfc));
    let new = m.malloc(0x11c);
    m.set_u32(p.wrapping_add(0xfc), new);
    m.set_u32(new.wrapping_add(0xfc), after);
    m.set_u16(eff.wrapping_add(0x10), 1);
    m.set_u32(eff.wrapping_add(0x100), 1);
    si_cus_copy_effect(m, o, eff, new, 1);
    new
}

pub const SI_MATERIAL_DISASSOCIATE: Routine = Routine::new("SI_materialDisassociate", 0x0828_5b64);
pub const SI_MATERIAL_STILL_USED: Routine = Routine::new("SI_materialStillUsed", 0x0828_5db0);
pub const SI_ED_DISPOSE_MATERIAL: Routine = Routine::new("SI_edDisposeMaterial", 0x0810_2114);
pub const SI_MATERIAL_COPY: Routine = Routine::new("SI_materialCopy", 0x0828_4888);
pub const SI_GRAFLIST_ADD_DATA: Routine = Routine::new("SI_graflistAddData", 0x0832_d240);
pub const SI_ED_MODEL_SET_MATERIAL: Routine = Routine::new("SI_edModelSetMaterial", 0x0810_0c34);
pub const SI_COPY_MATERIAL_SHADERS: Routine = Routine::new("SI_copyMaterialShaders", 0x082a_6808);
pub const SI_TXT2D_COPY: Routine = Routine::new("SI_txt2dCopy", 0x082a_dfbc);
pub const SI_ED_MODEL_SET_LTEXTURE2D: Routine = Routine::new("SI_edModelSetLTexture2D", 0x0810_126c);
pub const SI_COPY_T2D_SHADER: Routine = Routine::new("SI_copyT2DShader", 0x082a_6864);
pub const SI_TXT3D_COPY: Routine = Routine::new("SI_txt3dCopy", 0x082b_4008);
pub const SI_ED_MODEL_SET_LTEXTURE3D: Routine = Routine::new("SI_edModelSetLTexture3D", 0x0810_0f88);
pub const SI_COPY_T3D_SHADER: Routine = Routine::new("SI_copyT3DShader", 0x082a_68cc);
pub const SI_ED_DISPOSE_TEXTURE2D: Routine = Routine::new("SI_edDisposeTexture2D", 0x0810_253c);
pub const SI_ED_DISPOSE_TEXTURE3D: Routine = Routine::new("SI_edDisposeTexture3D", 0x0810_28d0);
pub const SI_TEXTURE3DG_DISASSOCIATE: Routine = Routine::new("SI_texture3Dgdisassociate", 0x082b_5658);
pub const SI_TEXTURE3D_STILL_USED: Routine = Routine::new("SI_texture3DStillUsed", 0x082b_56c0);
pub const SI_ED_MODEL_SET_GTEXTURE2D: Routine = Routine::new("SI_edModelSetGTexture2D", 0x0810_1130);
pub const SI_ED_MODEL_SET_GTEXTURE3D: Routine = Routine::new("SI_edModelSetGTexture3D", 0x0810_0e4c);

/// The graphic list of the current process (`SI_grafLists[SI_procData->index]`, 0x24 bytes each).
pub(crate) fn graf_list(m: &mut Mem) -> u32 {
    let idx = m.u32(m.u32(SI_PROC_DATA).wrapping_add(4));
    m.u32(SI_GRAF_LISTS).wrapping_add(idx.wrapping_mul(0x24))
}

/// [soft 0x085481d4 si_cusTransferMaterial]: makes the materials of `dst` copies of the ones of `src`
/// (model +0x20c / count +0x206): drops the old ones (disassociated, disposed when no longer used), copies
/// each material with its 2D texture chain, its current texture and its 3D texture and shaders, registers
/// them with the graphic list, then does the same for the two children (+0x2c0, +0x2c4) that both models
/// have. -1 when a model is missing or the new list can not be allocated, else 1.
pub fn si_cus_transfer_material(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32) -> i32 {
    if src == 0 || dst == 0 {
        return -1;
    }
    let mut i = m.u16(dst.wrapping_add(0x206)) as i16 as i32;
    loop {
        i -= 1;
        if i < 0 {
            break;
        }
        let arr = m.u32(dst.wrapping_add(0x20c));
        let mat = m.u32(arr.wrapping_add(i as u32 * 4));
        o.call(m, SI_MATERIAL_DISASSOCIATE, &[dst, mat, 1, 1]);
        if o.call(m, SI_MATERIAL_STILL_USED, &[mat]).eax as u16 == 0 {
            o.call(m, SI_ED_DISPOSE_MATERIAL, &[mat, 1]);
        }
    }
    m.set_u16(dst.wrapping_add(0x206), 0);
    m.set_u32(dst.wrapping_add(0x20c), 0);
    let cnt = m.u16(src.wrapping_add(0x206));
    let n = cnt as i16 as i32;
    if n > 0 {
        let arr = m.malloc((n as u32) << 2);
        m.set_u32(dst.wrapping_add(0x20c), arr);
        if arr == 0 {
            return -1;
        }
        m.set_u16(dst.wrapping_add(0x206), cnt);
        for i in 0..n as u32 {
            let sarr = m.u32(src.wrapping_add(0x20c));
            let smat = m.u32(sarr.wrapping_add(i * 4));
            let newm = o.call(m, SI_MATERIAL_COPY, &[smat]).eax;
            let darr = m.u32(dst.wrapping_add(0x20c));
            m.set_u32(darr.wrapping_add(i * 4), newm);
            let gl = graf_list(m);
            o.call(m, SI_GRAFLIST_ADD_DATA, &[gl, newm, 0xe, 0]);
            o.call(m, SI_ED_MODEL_SET_MATERIAL, &[dst, newm]);
            let darr = m.u32(dst.wrapping_add(0x20c));
            let sarr = m.u32(src.wrapping_add(0x20c));
            let (dm, sm) = (m.u32(darr.wrapping_add(i * 4)), m.u32(sarr.wrapping_add(i * 4)));
            o.call(m, SI_COPY_MATERIAL_SHADERS, &[dm, sm]);
            let mut prev = 0u32;
            let dm = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add(i * 4));
            m.set_u32(dm.wrapping_add(0x6f4), 0);
            let mut count: i16 = 0;
            let sm = m.u32(m.u32(src.wrapping_add(0x20c)).wrapping_add(i * 4));
            let mut t = m.u32(sm.wrapping_add(0x6ec));
            while t != 0 {
                let c = o.call(m, SI_TXT2D_COPY, &[t]).eax;
                let dm = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add(i * 4));
                if prev == 0 {
                    m.set_u32(dm.wrapping_add(0x6ec), c);
                } else {
                    m.set_u32(prev.wrapping_add(0x16c), c);
                }
                m.set_u32(c.wrapping_add(0x16c), 0);
                let gl = graf_list(m);
                o.call(m, SI_GRAFLIST_ADD_DATA, &[gl, c, 0xf, 0]);
                let dm = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add(i * 4));
                o.call(m, SI_ED_MODEL_SET_LTEXTURE2D, &[dm, c]);
                let sh = m.u32(t.wrapping_add(0x198));
                if sh == 0 {
                    m.set_u32(c.wrapping_add(0x198), 0);
                } else {
                    o.call(m, SI_COPY_T2D_SHADER, &[c, sh]);
                }
                let sm = m.u32(m.u32(src.wrapping_add(0x20c)).wrapping_add(i * 4));
                if m.u32(sm.wrapping_add(0x6f4)) == t {
                    let dm = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add(i * 4));
                    m.set_u32(dm.wrapping_add(0x6f4), c);
                }
                count = count.wrapping_add(1);
                prev = c;
                t = m.u32(t.wrapping_add(0x16c));
            }
            let dm = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add(i * 4));
            m.set_u16(dm.wrapping_add(0x6e8), count.wrapping_sub(1) as u16);
            let sm = m.u32(m.u32(src.wrapping_add(0x20c)).wrapping_add(i * 4));
            let t3 = m.u32(sm.wrapping_add(0x6fc));
            if t3 == 0 {
                m.set_u32(dm.wrapping_add(0x6fc), 0);
            } else {
                let c3 = o.call(m, SI_TXT3D_COPY, &[t3]).eax;
                let dm = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add(i * 4));
                m.set_u32(dm.wrapping_add(0x6fc), c3);
                let gl = graf_list(m);
                o.call(m, SI_GRAFLIST_ADD_DATA, &[gl, c3, 0x10, 0]);
                o.call(m, SI_ED_MODEL_SET_LTEXTURE3D, &[newm, c3]);
                let sm = m.u32(m.u32(src.wrapping_add(0x20c)).wrapping_add(i * 4));
                let sh = m.u32(m.u32(sm.wrapping_add(0x6fc)).wrapping_add(0x14c));
                if sh == 0 {
                    m.set_u32(c3.wrapping_add(0x14c), 0);
                } else {
                    o.call(m, SI_COPY_T3D_SHADER, &[c3, sh]);
                }
            }
            let dm = m.u32(m.u32(dst.wrapping_add(0x20c)).wrapping_add(i * 4));
            let b = m.u8(dm.wrapping_add(0xda4)) | 0x80;
            m.set_u8(dm.wrapping_add(0xda4), b);
        }
    }
    if m.u32(dst.wrapping_add(0x210)) != 0 && n > 0 {
        let first = m.u32(m.u32(dst.wrapping_add(0x20c)));
        m.set_u32(dst.wrapping_add(0x210), first);
    }
    for off in [0x2c0u32, 0x2c4] {
        let (s, d) = (m.u32(src.wrapping_add(off)), m.u32(dst.wrapping_add(off)));
        if s != 0 && d != 0 && si_cus_transfer_material(m, o, s, d) < 0 {
            return -1;
        }
    }
    1
}

/// [soft 0x085485b0 si_cusTransferTexture]: makes the global textures of `dst` (2D chain +0x21c with the
/// current one at +0x220, the 3D one at +0x224) copies of the ones of `src`, the old ones being disposed;
/// marks `dst` (+0xa bit 7) and does the same for the two children (+0x2c0, +0x2c4). -1 when a model is
/// missing or a child fails, else 1.
pub fn si_cus_transfer_texture(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32) -> i32 {
    if src == 0 || dst == 0 {
        return -1;
    }
    while m.u32(dst.wrapping_add(0x21c)) != 0 {
        let t = m.u32(dst.wrapping_add(0x21c));
        o.call(m, SI_ED_DISPOSE_TEXTURE2D, &[t]);
    }
    let t3 = m.u32(dst.wrapping_add(0x224));
    m.set_u32(dst.wrapping_add(0x220), 0);
    if t3 != 0 {
        o.call(m, SI_TEXTURE3DG_DISASSOCIATE, &[dst, t3, 1]);
        if o.call(m, SI_TEXTURE3D_STILL_USED, &[t3]).eax as u16 == 0 {
            o.call(m, SI_ED_DISPOSE_TEXTURE3D, &[t3]);
        }
    }
    m.set_u32(dst.wrapping_add(0x224), 0);
    let mut prev = 0u32;
    let mut t = m.u32(src.wrapping_add(0x21c));
    while t != 0 {
        let c = o.call(m, SI_TXT2D_COPY, &[t]).eax;
        if prev == 0 {
            m.set_u32(dst.wrapping_add(0x21c), c);
        } else {
            m.set_u32(prev.wrapping_add(0x16c), c);
        }
        m.set_u32(c.wrapping_add(0x16c), 0);
        let gl = graf_list(m);
        o.call(m, SI_GRAFLIST_ADD_DATA, &[gl, c, 0xf, 0]);
        o.call(m, SI_ED_MODEL_SET_GTEXTURE2D, &[dst, c]);
        let sh = m.u32(t.wrapping_add(0x198));
        if sh == 0 {
            m.set_u32(c.wrapping_add(0x198), 0);
        } else {
            o.call(m, SI_COPY_T2D_SHADER, &[c, sh]);
        }
        if m.u32(src.wrapping_add(0x220)) == t {
            m.set_u32(dst.wrapping_add(0x220), c);
        }
        prev = c;
        t = m.u32(t.wrapping_add(0x16c));
    }
    let s3 = m.u32(src.wrapping_add(0x224));
    if s3 == 0 {
        m.set_u32(dst.wrapping_add(0x224), 0);
    } else {
        let c3 = o.call(m, SI_TXT3D_COPY, &[s3]).eax;
        m.set_u32(dst.wrapping_add(0x224), c3);
        let gl = graf_list(m);
        o.call(m, SI_GRAFLIST_ADD_DATA, &[gl, c3, 0x10, 0]);
        o.call(m, SI_ED_MODEL_SET_GTEXTURE3D, &[dst, c3]);
        let s3 = m.u32(src.wrapping_add(0x224));
        let sh = m.u32(s3.wrapping_add(0x14c));
        if sh != 0 {
            o.call(m, SI_COPY_T3D_SHADER, &[c3, sh]);
        }
    }
    let b = m.u8(dst.wrapping_add(10)) | 0x80;
    m.set_u8(dst.wrapping_add(10), b);
    for off in [0x2c0u32, 0x2c4] {
        let (s, d) = (m.u32(src.wrapping_add(off)), m.u32(dst.wrapping_add(off)));
        if s != 0 && d != 0 && si_cus_transfer_texture(m, o, s, d) < 0 {
            return -1;
        }
    }
    1
}

pub const SI_PATCH_COPY: Routine = Routine::new("SI_patchCopy", 0x0847_a2b8);
pub const SI_FACE_COPY: Routine = Routine::new("SI_faceCopy", 0x0843_4990);
pub const SI_MESH_COPY: Routine = Routine::new("SI_meshCopy", 0x0844_04e0);
pub const SI_SPLINE_COPY: Routine = Routine::new("SI_splineCopy", 0x0848_aa74);
pub const SI_NURBS_CRV_COPY: Routine = Routine::new("SI_nurbsCrvCopy", 0x084d_bbf4);
pub const SI_NURBS_SRF_COPY: Routine = Routine::new("SI_nurbsSrfCopy", 0x084d_c1a8);
pub const SI_PATCH_UPDATE_SURFACE: Routine = Routine::new("SI_patchUpdateSurface", 0x0847_cee4);
pub const SI_MESH_COMPUTES_POLYGON_NORMAL: Routine = Routine::new("SI_meshComputesPolygonNormal", 0x0845_99f4);
pub const SI_MESH_COMPUTE_HARD_EDGE: Routine = Routine::new("SI_meshComputeHardEdge", 0x0845_a6e4);
pub const SI_MESH_COMPUTES_VERTEX_NORMAL: Routine = Routine::new("SI_meshComputesVertexNormal", 0x0845_9e8c);
pub const SI_MODEL_TOUCH_DEFINITION: Routine = Routine::new("SI_modelTouchDefinition", 0x0846_d530);
pub const SI_FIND_GL_DATA: Routine = Routine::new("SI_findGlData", 0x0832_ee68);
pub const SI_UPDATE_GN_FUNC: Routine = Routine::new("SI_updateGnFunc", 0x0817_6618);
pub const SI_WRATH_UNSUPPORTED_MDL_TYPE: Routine = Routine::new("SI_wrathUnsupportedMdlType", 0x0810_7b68);
pub const SI_ED_GET_ELEM_FROM_MODEL: Routine = Routine::new("SI_edGetElemFromModel", 0x080f_cf20);
pub const SI_ED_SET_UNEDITABLE: Routine = Routine::new("SI_edSetUneditable", 0x0810_74c0);
pub const SI_FACE_TRANSFER_POINTS: Routine = Routine::new("SI_faceTransferPoints", 0x0843_672c);
pub const SI_MESH_TRANSFER_VERTICES: Routine = Routine::new("SI_meshTransferVertices", 0x0844_6d00);
pub const SI_SPLINE_TRANSFER_POINTS: Routine = Routine::new("SI_splineTransferPoints", 0x0849_0664);
pub const SI_SPLINE_TOUCH: Routine = Routine::new("SI_splineTouch", 0x0848_c924);
pub const SI_NURBS_CRV_TRANSFER_POINTS_KNOTS: Routine = Routine::new("SI_nurbsCrvTransferPointsKnots", 0x084e_a00c);
pub const SI_NURBS_SRF_TRANSFER_POINTS_KNOTS: Routine = Routine::new("SI_nurbsSrfTransferPointsKnots", 0x084e_a07c);
pub const SI_SAVEKEY_MODEL: Routine = Routine::new("SI_savekeyModel", 0x0821_9fe0);
pub const SI_CUS_MODEL_DISPOSE_USER_DATA: Routine = Routine::new("si_cusModelDisposeUserData", 0x0853_7230);
pub const SI_CUS_MODEL_ATTACH_USER_DATA: Routine = Routine::new("si_cusModelAttachUserData", 0x0853_70d4);
pub const SI_CUS_MODEL_SEARCH_USER_DATA: Routine = Routine::new("si_cusModelSearchUserData", 0x0853_7154);
const FSCANF_P: Routine = Routine::new("plt:fscanf", 0x080c_4058);
const SI_LIGHT_LICENSE: u32 = 0x0867_9320;
const SI_ELEM_DICTS_P: u32 = 0x0867_931c;

/// [soft 0x085491d0 SI_cusMakeDefinition]: gives a model a definition of its own: when its current geometry
/// (+0x48) is the one at the index +0x36 of its list (+0x38, count +0x34), replaces it by a copy of the
/// first of the list (copy routine by the geometry kind at +0x30: 1 patch, 2 face, 4 mesh (copies the
/// current one), 6 spline, 9 and 10 nurbs curve and surface); with `all` set also for the two children.
pub fn si_cus_make_definition(m: &mut Mem, o: &mut dyn Outside, model: u32, all: u32) {
    let kind = m.u32(model.wrapping_add(0x30));
    if matches!(kind, 1 | 2 | 4 | 6 | 9 | 10) && m.u16(model.wrapping_add(0x34)) != 0 {
        let list = m.u32(model.wrapping_add(0x38));
        let idx = m.u16(model.wrapping_add(0x36)) as u32;
        let cur = m.u32(model.wrapping_add(0x48));
        if cur == m.u32(list.wrapping_add(idx * 4)) {
            let first = m.u32(list);
            let r = match kind {
                1 => o.call(m, SI_PATCH_COPY, &[first]).eax,
                2 => o.call(m, SI_FACE_COPY, &[first]).eax,
                4 => o.call(m, SI_MESH_COPY, &[cur]).eax,
                6 => o.call(m, SI_SPLINE_COPY, &[first]).eax,
                9 => o.call(m, SI_NURBS_CRV_COPY, &[first]).eax,
                _ => o.call(m, SI_NURBS_SRF_COPY, &[first]).eax,
            };
            m.set_u32(model.wrapping_add(0x48), r);
        }
    }
    if all != 0 {
        for off in [0x2c0u32, 0x2c4] {
            let c = m.u32(model.wrapping_add(off));
            if c != 0 {
                si_cus_make_definition(m, o, c, all);
            }
        }
    }
}

/// [soft 0x085487e0 si_cusSwapGeometry]: exchanges the geometry (+0x48, list +0x38, count +0x34, kind
/// +0x30, index +0x36) of two models, giving the first a definition of its own beforehand when the second
/// already has one and the first does not; resets the self links (+0x44), updates the new geometry of
/// `b` (patches, or the polygon and vertex normals and hard edges of a mesh when `normals` is set), touches
/// the definition, updates the graphic node and, in the reduced edition (`SI_lightLicense`), marks models of
/// an unsupported type uneditable; does the same for the children. 1, or -1 when a child fails.
pub fn si_cus_swap_geometry(m: &mut Mem, o: &mut dyn Outside, a: u32, b: u32, normals: i16) -> i32 {
    let at = |p: u32, off: u32| p.wrapping_add(off);
    if m.u16(at(b, 0x34)) != 0 && m.u16(at(a, 0x34)) != 0 {
        let (bl, bi) = (m.u32(at(b, 0x38)), m.u16(at(b, 0x36)) as u32);
        let (al, ai) = (m.u32(at(a, 0x38)), m.u16(at(a, 0x36)) as u32);
        if m.u32(at(b, 0x48)) != m.u32(bl.wrapping_add(bi * 4)) && m.u32(at(a, 0x48)) == m.u32(al.wrapping_add(ai * 4)) {
            si_cus_make_definition(m, o, a, 0);
        }
    }
    let t48 = m.u32(at(a, 0x48));
    let t38 = m.u32(at(a, 0x38));
    let t34 = m.u16(at(a, 0x34));
    let t30 = m.u32(at(a, 0x30));
    let t36 = m.u16(at(a, 0x36));
    let v = m.u32(at(b, 0x48));
    m.set_u32(at(a, 0x48), v);
    let v = m.u32(at(b, 0x38));
    m.set_u32(at(a, 0x38), v);
    let v = m.u16(at(b, 0x34));
    m.set_u16(at(a, 0x34), v);
    let v = m.u32(at(b, 0x30));
    m.set_u32(at(a, 0x30), v);
    let v = m.u16(at(b, 0x36));
    m.set_u16(at(a, 0x36), v);
    m.set_u32(at(b, 0x48), t48);
    m.set_u32(at(b, 0x38), t38);
    m.set_u16(at(b, 0x34), t34);
    m.set_u32(at(b, 0x30), t30);
    m.set_u16(at(b, 0x36), t36);
    m.set_u32(at(b, 0x44), b);
    m.set_u32(at(a, 0x44), a);
    let kind = m.u32(at(b, 0x30));
    if kind == 1 {
        let g = m.u32(at(b, 0x48));
        o.call(m, SI_PATCH_UPDATE_SURFACE, &[g]);
        let n = m.u16(at(b, 0x34)) as u32;
        let mut i = 0;
        if n != 0 {
            loop {
                let l = m.u32(at(b, 0x38));
                let p = m.u32(l.wrapping_add(i * 4));
                o.call(m, SI_PATCH_UPDATE_SURFACE, &[p]);
                i += 1;
                if i >= m.u16(at(b, 0x34)) as u32 {
                    break;
                }
            }
        }
    } else if kind == 4 && normals != 0 {
        let mesh = m.u32(at(b, 0x48));
        o.call(m, SI_MESH_COMPUTES_POLYGON_NORMAL, &[mesh, 0xffff_ffff]);
        o.call(m, SI_MESH_COMPUTE_HARD_EDGE, &[mesh, 0xffff_ffff]);
        o.call(m, SI_MESH_COMPUTES_VERTEX_NORMAL, &[mesh, 0xffff_ffff]);
        let f = m.u8(mesh.wrapping_add(0x20)) | 2;
        m.set_u8(mesh.wrapping_add(0x20), f);
    }
    o.call(m, SI_MODEL_TOUCH_DEFINITION, &[b]);
    let gl = graf_list(m);
    let node = o.call(m, SI_FIND_GL_DATA, &[gl, b, 2]).eax;
    o.call(m, SI_UPDATE_GN_FUNC, &[node]);
    if m.u16(SI_LIGHT_LICENSE) != 0 {
        for model in [a, b] {
            if o.call(m, SI_WRATH_UNSUPPORTED_MDL_TYPE, &[model]).eax as u16 != 0 {
                let out = m.local(4);
                let elem = o.call(m, SI_ED_GET_ELEM_FROM_MODEL, &[model, out]).eax;
                let pd = m.u32(SI_PROC_DATA);
                let k = m.u32(pd).wrapping_shl(3).wrapping_sub(m.u32(pd)).wrapping_shl(4);
                let dict = k.wrapping_add(m.u32(SI_ELEM_DICTS_P));
                o.call(m, SI_ED_SET_UNEDITABLE, &[dict, elem]);
                m.release(4);
            }
        }
    }
    for off in [0x2c0u32, 0x2c4] {
        let (ca, cb) = (m.u32(at(a, off)), m.u32(at(b, off)));
        if ca != 0 && cb != 0 && si_cus_swap_geometry(m, o, ca, cb, normals) < 0 {
            return -1;
        }
    }
    1
}

/// [soft 0x08548a88 si_cusSaveKeyTransformation]: reads the saved-key lines of a stream for the command
/// kind `kind` until the id -1 (1): each is an id, a model name and three numbers; the model is looked up
/// by name (0 when unknown) and keys are saved on its scale/rotation/translation channels (kinds 0x3f 0x43
/// scale, 0x40 0x44 rotation (the numbers are converted from radians to degrees), 0x41 0x45
/// translation; the first three kinds only save for the first line); other kinds save nothing.
pub fn si_cus_save_key_transformation(m: &mut Mem, o: &mut dyn Outside, fp: u32, kind: i32) -> i32 {
    let id = m.local(4);
    let name = m.local(200);
    let nums = m.local(12);
    let mut saved = false;
    let r = loop {
        o.call(m, FSCANF_P, &[fp, 0x086e_4580, id]);
        if m.u32(id) as i32 == -1 {
            break 1;
        }
        o.call(m, FSCANF_P, &[fp, 0x086e_4586, name]);
        o.call(m, FSCANF_P, &[fp, 0x086e_458c, nums, nums.wrapping_add(4), nums.wrapping_add(8)]);
        let model = si_cus_find_model_by_name(m, o, name);
        if model == 0 {
            break 0;
        }
        let (x, y, z) = (m.u32(nums), m.u32(nums.wrapping_add(4)), m.u32(nums.wrapping_add(8)));
        let (ty, ch, vals) = match kind {
            0x3f | 0x43 => {
                if kind == 0x3f && saved {
                    continue;
                }
                (100u32, [1u32, 2, 3], [x, y, z])
            }
            0x40 | 0x44 => {
                if kind == 0x40 && saved {
                    continue;
                }
                let c = 57.29577951308232f64; // the constant at 0x08663478
                let sc = |v: u32| ((c * f32::from_bits(v) as f64) as f32).to_bits();
                (0x65u32, [4, 5, 6], [sc(x), sc(y), sc(z)])
            }
            0x41 | 0x45 => {
                if kind == 0x41 && saved {
                    continue;
                }
                (0x71u32, [0x8b, 0x8c, 0x8d], [x, y, z])
            }
            _ => continue,
        };
        for k in 0..3 {
            o.call(m, SI_SAVEKEY_MODEL, &[model, 1, ty, ch[k], 1, vals[k], 0, 0]);
        }
        saved = true;
    };
    m.release(12);
    m.release(200);
    m.release(4);
    r
}

/// [soft 0x08548eb8 si_cusTransferUserData]: copies the user data list of `src` (+0x34c; name, size +8,
/// bytes +0xc, link +0x10) to `dst`: an existing entry of the same name is disposed, a positive-sized one is
/// copied byte by byte and attached; then the two children. 1, or -1 when a child fails.
pub fn si_cus_transfer_user_data(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32) -> i32 {
    let mut n = m.u32(src.wrapping_add(0x34c));
    while n != 0 {
        let name = m.u32(n);
        if o.call(m, SI_CUS_MODEL_SEARCH_USER_DATA, &[dst, name]).eax != 0 {
            let name = m.u32(n);
            o.call(m, SI_CUS_MODEL_DISPOSE_USER_DATA, &[dst, name]);
        }
        let size = m.u32(n.wrapping_add(8)) as i32;
        if size > 0 {
            let buf = m.malloc(size as u32);
            let data = m.u32(n.wrapping_add(0xc));
            let mut i = 0i32;
            while i < m.u32(n.wrapping_add(8)) as i32 {
                let b = m.u8(data.wrapping_add(i as u32));
                m.set_u8(buf.wrapping_add(i as u32), b);
                i += 1;
            }
            let name = m.u32(n);
            let size = m.u32(n.wrapping_add(8));
            o.call(m, SI_CUS_MODEL_ATTACH_USER_DATA, &[dst, name, size, buf]);
        }
        n = m.u32(n.wrapping_add(0x10));
    }
    for off in [0x2c0u32, 0x2c4] {
        let (a, b) = (m.u32(src.wrapping_add(off)), m.u32(dst.wrapping_add(off)));
        if a != 0 && b != 0 && si_cus_transfer_user_data(m, o, a, b) < 0 {
            return -1;
        }
    }
    1
}

/// The geometry of a model that `which` selects: its current one (+0x48) or the first of its list (+0x38).
fn geometry_of(m: &mut Mem, model: u32, first: bool) -> u32 {
    if first {
        let l = m.u32(model.wrapping_add(0x38));
        m.u32(l)
    } else {
        m.u32(model.wrapping_add(0x48))
    }
}

/// [soft 0x08548fa0 si_cusTransferCV]: copies the control points of the geometry of `src` (+0x48) to the
/// geometry of `dst` (the first of its list when `first` is set, else its current one) for the kinds patch
/// (also updates the surface), face, mesh (also the polygon normals, and with `keep_edges` clear the hard
/// edges and vertex normals), spline (also touches it), nurbs curve and surface; -1 when the kinds differ
/// or for any other kind; then touches the definition and, with `deep` set, does the same for the children.
pub fn si_cus_transfer_cv(m: &mut Mem, o: &mut dyn Outside, src: u32, dst: u32, keep_edges: i32, first: i16, deep: i16) -> i32 {
    let kind = m.u32(src.wrapping_add(0x30));
    if matches!(kind, 1 | 2 | 4 | 6 | 9 | 10) {
        if m.u32(dst.wrapping_add(0x30)) != kind {
            return -1;
        }
        let sg = m.u32(src.wrapping_add(0x48));
        let dg = geometry_of(m, dst, first != 0);
        match kind {
            1 => {
                cus_patch_transfer(m, sg, dg);
                o.call(m, SI_PATCH_UPDATE_SURFACE, &[dg]);
            }
            2 => {
                o.call(m, SI_FACE_TRANSFER_POINTS, &[sg, dg]);
            }
            4 => {
                o.call(m, SI_MESH_TRANSFER_VERTICES, &[sg, dg]);
                o.call(m, SI_MESH_COMPUTES_POLYGON_NORMAL, &[dg, 0xffff_ffff]);
                if keep_edges == 0 {
                    o.call(m, SI_MESH_COMPUTE_HARD_EDGE, &[dg, 0xffff_ffff]);
                    o.call(m, SI_MESH_COMPUTES_VERTEX_NORMAL, &[dg, 0xffff_ffff]);
                }
            }
            6 => {
                o.call(m, SI_SPLINE_TRANSFER_POINTS, &[sg, dg]);
                o.call(m, SI_SPLINE_TOUCH, &[dg]);
            }
            9 => {
                o.call(m, SI_NURBS_CRV_TRANSFER_POINTS_KNOTS, &[sg, dg]);
            }
            _ => {
                o.call(m, SI_NURBS_SRF_TRANSFER_POINTS_KNOTS, &[sg, dg]);
            }
        }
    }
    o.call(m, SI_MODEL_TOUCH_DEFINITION, &[dst]);
    if deep != 0 {
        for off in [0x2c0u32, 0x2c4] {
            let (a, b) = (m.u32(src.wrapping_add(off)), m.u32(dst.wrapping_add(off)));
            if a != 0 && b != 0 && si_cus_transfer_cv(m, o, a, b, keep_edges, first, deep) < 0 {
                return -1;
            }
        }
    }
    1
}

/// Shared body of the two transfers between a model's data (first of the list, +0x38) and its definition
/// (+0x48): `to_def` selects the direction. Patches also update the target surface, meshes recompute its
/// polygon normals, splines touch it; the children follow when `deep` is set.
fn transfer_data_definition(m: &mut Mem, o: &mut dyn Outside, model: u32, deep: u32, to_def: bool) {
    if model == 0 {
        return;
    }
    let kind = m.u32(model.wrapping_add(0x30));
    let data = |m: &mut Mem| {
        let l = m.u32(model.wrapping_add(0x38));
        m.u32(l)
    };
    let (s, d) = if to_def {
        let s = data(m);
        (s, m.u32(model.wrapping_add(0x48)))
    } else {
        let d = data(m);
        (m.u32(model.wrapping_add(0x48)), d)
    };
    match kind {
        1 => {
            cus_patch_transfer(m, s, d);
            o.call(m, SI_PATCH_UPDATE_SURFACE, &[d]);
        }
        2 => {
            o.call(m, SI_FACE_TRANSFER_POINTS, &[s, d]);
        }
        4 => {
            o.call(m, SI_MESH_TRANSFER_VERTICES, &[s, d]);
            o.call(m, SI_MESH_COMPUTES_POLYGON_NORMAL, &[d, 0xffff_ffff]);
        }
        6 => {
            o.call(m, SI_SPLINE_TRANSFER_POINTS, &[s, d]);
            o.call(m, SI_SPLINE_TOUCH, &[d]);
        }
        9 => {
            o.call(m, SI_NURBS_CRV_TRANSFER_POINTS_KNOTS, &[s, d]);
        }
        10 => {
            o.call(m, SI_NURBS_SRF_TRANSFER_POINTS_KNOTS, &[s, d]);
        }
        _ => {}
    }
    if deep != 0 {
        for off in [0x2c0u32, 0x2c4] {
            let c = m.u32(model.wrapping_add(off));
            if c != 0 {
                transfer_data_definition(m, o, c, deep, to_def);
            }
        }
    }
}

/// [soft 0x08549328 SI_cusTransferToDefinition]: copies the geometry points of the model's first data
/// (list +0x38) to its definition (+0x48); with `deep` set also for the two children.
pub fn si_cus_transfer_to_definition(m: &mut Mem, o: &mut dyn Outside, model: u32, deep: u32) {
    transfer_data_definition(m, o, model, deep, true);
}

/// [soft 0x08549444 si_cusTransferToData]: the other direction, from the definition (+0x48) to the first
/// data of the list (+0x38).
pub fn si_cus_transfer_to_data(m: &mut Mem, o: &mut dyn Outside, model: u32, deep: u32) {
    transfer_data_definition(m, o, model, deep, false);
}

pub const SI_GRAFLIST_CUT_MODEL: Routine = Routine::new("SI_graflistCutModel", 0x0833_562c);
pub const SI_MATRIX_MULTIPLY_SUB3: Routine = Routine::new("SI_matrixMultiplySub3", 0x0851_650c);
pub const SI_MODEL_SAVE_HISTORY_PARENTING: Routine = Routine::new("SI_modelSaveHistoryParenting", 0x080c_a4d4);
pub const SI_MODEL_CUT_FATHER: Routine = Routine::new("SI_modelCutFather", 0x0846_d790);
pub const SI_GRAFLIST_DELETE_GRAFNODE: Routine = Routine::new("SI_graflistDeleteGrafnode", 0x0832_cd50);
pub const SI_MATRIX_EXTRACT_XYZ_ROTATION: Routine = Routine::new("SI_matrixExtractXyzRotation", 0x0851_72b0);
pub const SI_MODEL_NEXT_SON: Routine = Routine::new("SI_modelNextSon", 0x0846_d70c);
pub const SI_MODEL_ROOT: Routine = Routine::new("SI_modelRoot", 0x0847_838c);
pub const SI_GRAFLIST_DELETE_NODE: Routine = Routine::new("SI_graflistDeleteNode", 0x0832_d75c);
pub const SI_ELEMDICT_GET_ELEM_D: Routine = Routine::new("SI_elemdictGetElem", 0x0813_2b50);
const SI_HRCH_CPST: u32 = 0x0867_949c;
const SI_CID_MODELS: u32 = 0x0878_1680;

/// [soft 0x08549570 SI_cusDeleteModel]: removes the effects that use `model` from the five status records.
pub fn si_cus_delete_model(m: &mut Mem, o: &mut dyn Outside, model: u32) {
    for s in [CUS_MOTION, CUS_MODEL, CUS_MATTER, CUS_TOOLS, CUS_ACTOR] {
        cus_status_delete_model(m, o, s, model);
    }
}

/// [soft 0x085496c4 cusStatusDeleteModel]: for each effect of the status whose element list holds a model
/// element (kind 0x20 or 0x21) of `model`, finds its graf node; a model node (kind 3) whose model has a
/// root other than itself is first cut from its father (the history parenting saved unless flag 0x10
/// of the node's byte +0x17 is set, the local scale/rotation/translation folded into the root's when
/// `SI_hrchCpst` is set) and the node is then deleted. The model variable keeps the node's model for
/// the following effects, as the original does.
pub fn cus_status_delete_model(m: &mut Mem, o: &mut dyn Outside, status: u32, model: u32) {
    let mut model = model;
    let mut eff = m.u32(m.u32(status.wrapping_add(0x14c)).wrapping_add(0xfc));
    while eff != 0 {
        let next = m.u32(eff.wrapping_add(0xfc));
        let mut found = false;
        let mut e = m.u32(eff.wrapping_add(0xf8));
        while e != 0 {
            let k = m.u16(e) as i16;
            if (0x20..=0x21).contains(&k) && m.u32(e.wrapping_add(0x1c)) == model {
                found = true;
            }
            e = m.u32(e.wrapping_add(0x30));
            if e == 0 || found {
                break;
            }
        }
        let target = m.u32(eff.wrapping_add(0xc));
        if found && target != 0 {
            let gl = graf_list(m);
            let mut node = o.call(m, SI_FIND_GL_DATA, &[gl, target, 2]).eax;
            if node != 0 {
                if m.u32(node) == 3 {
                    model = m.u32(node.wrapping_add(4));
                    let root = o.call(m, SI_MODEL_ROOT, &[model]).eax;
                    if root != 0 && root != model {
                        let gl = graf_list(m);
                        let rn = o.call(m, SI_FIND_GL_DATA, &[gl, root, 0]).eax;
                        if m.u8(node.wrapping_add(0x17)) & 0x10 == 0 {
                            o.call(m, SI_MODEL_SAVE_HISTORY_PARENTING, &[node, rn, 1]);
                        }
                        for p in [root.wrapping_add(0xa), root.wrapping_add(0x3fa)] {
                            let v = m.u8(p) | 0x20;
                            m.set_u8(p, v);
                        }
                        let mut son = 0;
                        loop {
                            son = o.call(m, SI_MODEL_NEXT_SON, &[root, son]).eax;
                            if son == 0 {
                                break;
                            }
                            for p in [son.wrapping_add(0xa), son.wrapping_add(0x3fa)] {
                                let v = m.u8(p) | 0x20;
                                m.set_u8(p, v);
                            }
                        }
                        if m.u16(SI_HRCH_CPST) != 0 {
                            let at = |b: u32, off: u32| b.wrapping_add(off);
                            for (a, b) in [(0x100u32, 0x16cu32), (0x104, 0x170), (0x108, 0x174)] {
                                let v = m.f32(at(model, a)) * m.f32(at(root, b));
                                m.set_f32(at(model, a), v);
                            }
                            let p = model.wrapping_add(0x118);
                            o.call(m, SI_MATRIX_MULTIPLY_SUB3, &[p, root.wrapping_add(0x178), p]);
                            o.call(m, SI_MATRIX_EXTRACT_XYZ_ROTATION, &[p, model.wrapping_add(0x10c)]);
                            // the three products are summed in x87 extended precision; a double is
                            // used here (the products are exact in it)
                            let (x, y, z) = (m.f32(at(model, 0x158)) as f64, m.f32(at(model, 0x15c)) as f64, m.f32(at(model, 0x160)) as f64);
                            let g = |m: &Mem, o: u32| m.f32(at(root, o)) as f64;
                            let nx = x * g(m, 0x1c4) + y * g(m, 0x1d4) + z * g(m, 0x1e4) + g(m, 0x1f4);
                            let ny = y * g(m, 0x1d8) + x * g(m, 0x1c8) + z * g(m, 0x1e8) + g(m, 0x1f8);
                            let nz = x * g(m, 0x1cc) + y * g(m, 0x1dc) + z * g(m, 0x1ec) + g(m, 0x1fc);
                            m.set_f32(at(model, 0x158), nx as f32);
                            m.set_f32(at(model, 0x15c), ny as f32);
                            m.set_f32(at(model, 0x160), nz as f32);
                        }
                        o.call(m, SI_MODEL_CUT_FATHER, &[model]);
                        let gl = graf_list(m);
                        o.call(m, SI_GRAFLIST_CUT_MODEL, &[gl, node, root, 0]);
                        let gl = graf_list(m);
                        node = o.call(m, SI_FIND_GL_DATA, &[gl, model, 0]).eax;
                    }
                }
                let gl = graf_list(m);
                o.call(m, SI_GRAFLIST_DELETE_GRAFNODE, &[gl, node]);
            }
        }
        eff = next;
    }
}

/// [soft 0x08549998 SI_cusDeleteElement]: deletes the custom-effect nodes related to `elem` (nothing for
/// a null element, or for a model element of the model kind 0xd).
pub fn si_cus_delete_element(m: &mut Mem, o: &mut dyn Outside, elem: u32, p2: u32, p3: u32) {
    if elem == 0 {
        return;
    }
    if p2 == m.u32(SI_CID_MODELS) && m.u32(m.u32(elem.wrapping_add(4)).wrapping_add(0x2ec)) == 0xd {
        return;
    }
    cus_delete_element_direction(m, o, elem, p2, p3, 1);
    cus_delete_element_direction(m, o, elem, p2, p3, 0);
}

/// [soft 0x085499e0 cusDeleteElementDirection]: walks the relation table of `elem` (direction 1: at +0x60,
/// else at +0x68; records of 0x18 bytes, entries of 0x20); each entry of kind 20000 that is linked
/// (word +0xc not -1) names a related custom element (+0x18, or for direction 0 looked up in the
/// target element's own table) whose model node, when it exists, is deleted with its descendants.
pub fn cus_delete_element_direction(m: &mut Mem, o: &mut dyn Outside, elem: u32, _p2: u32, _p3: u32, dir: i16) {
    let table = elem.wrapping_add(if dir == 0 { 0x68 } else { 0x60 });
    let dict = |m: &Mem| {
        let pd = m.u32(SI_PROC_DATA);
        pd_dict(m, pd)
    };
    if m.u32(table.wrapping_add(4)) == 0 {
        return;
    }
    let mut i = 0i32;
    while i < m.u32(table) as i32 {
        let rec = m.u32(table.wrapping_add(4)).wrapping_add((i as u32).wrapping_mul(0x18));
        i += 1;
        let mut j = 0i32;
        while j < m.u32(rec) as i32 {
            let ent = m.u32(rec.wrapping_add(0xc)).wrapping_add((j as u32).wrapping_mul(0x20));
            if m.u32(ent) == 20000 && m.u32(ent.wrapping_add(0xc)) != 0xffff_ffff {
                let mut target = 0u32;
                if dir != 0 {
                    target = m.u32(ent.wrapping_add(0x18));
                } else {
                    let d = dict(m);
                    let other = o.call(m, SI_ELEMDICT_GET_ELEM_D, &[d, m.u16(ent.wrapping_add(0x10)) as i16 as i32 as u32, m.u32(ent.wrapping_add(4))]).eax;
                    if other != 0 && other != 0xffff_ffa0 && m.u32(other.wrapping_add(0x64)) != 0 {
                        let sub = m.u32(other.wrapping_add(0x64)).wrapping_add((m.u16(ent.wrapping_add(0x12)) as i16 as i32 as u32).wrapping_mul(0x18));
                        if (m.u32(sub) as i32) > 0 {
                            let l = m.u32(sub.wrapping_add(0xc));
                            let key = m.u32(ent.wrapping_add(0xc));
                            if m.u32(l) == 20000 && m.u32(l.wrapping_add(0xc)) == key {
                                target = m.u32(l.wrapping_add(0x18));
                            } else {
                                let mut k = 0i32;
                                loop {
                                    k += 1;
                                    if k >= m.u32(sub) as i32 {
                                        break;
                                    }
                                    let c = l.wrapping_add((k as u32).wrapping_mul(0x20));
                                    if m.u32(c) == 20000 && m.u32(c.wrapping_add(0xc)) == key {
                                        target = m.u32(c.wrapping_add(0x18));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                if target != 0 {
                    let d = dict(m);
                    let e2 = o.call(m, SI_ELEMDICT_GET_ELEM_D, &[d, m.u32(SI_CID_MODELS), m.u32(target)]).eax;
                    if e2 != 0 {
                        let gl = graf_list(m);
                        let node = o.call(m, SI_FIND_GL_DATA, &[gl, m.u32(e2.wrapping_add(4)), 2]).eax;
                        if node != 0 {
                            let gl = graf_list(m);
                            o.call(m, SI_GRAFLIST_DELETE_NODE, &[gl, node, 1]);
                            j -= 1;
                        }
                    }
                }
            }
            j += 1;
        }
    }
}

fn pd_dict(m: &Mem, pd: u32) -> u32 {
    let k = m.u32(pd).wrapping_shl(3).wrapping_sub(m.u32(pd)).wrapping_shl(4);
    k.wrapping_add(m.u32(SI_ELEM_DICTS_P))
}
