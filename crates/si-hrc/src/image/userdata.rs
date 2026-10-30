// SPDX-License-Identifier: MIT
//! User data lists (`__SI_UserNode`) on the image: the reader of `binary.c` and the copy
//! of `model.c` that the texture routines use.

use super::globals::FILE_VERSION;
use super::io::{alloc_fits, byteswap_fread};
use super::Outside;
use si_mem::{record, Mem, Ptr32};

record! {
    /// One block of user data (`__SI_UserNode`).
    pub struct UserNode = "__SI_UserNode", size 0x14 {
        0x00 tag: Ptr32<u8>, "the block's name, a string in a block of its own";
        0x04 flag: i16, "the file's flag word";
        0x06 pad06: u16, "never written (the record is allocated zeroed)";
        0x08 size: u32, "bytes of data";
        0x0c data: Ptr32<u8>, "the data, a block of its own";
        0x10 next: Ptr32<UserNode>, "the next block of the list";
    }
}

/// [soft 0x083ec910 si_userDataRead]: a list of blocks in the new form: an int count;
/// per block an int tag length, the tag, a short flag, an int size, the data. In a file
/// below version 3.82099 (the double at 0x0865bc30, against `binary.c`'s `fileVersion`)
/// the data is read as one item of `size` bytes, which the swapping read reverses when
/// the size is 2, 4 or 8; from that version on as `size` single bytes.
///
/// The blocks are linked in file order; the first goes to `*list`, which is not touched
/// when the count is not positive (a list already there is neither freed nor extended).
pub fn user_data_read(m: &mut Mem, o: &mut dyn Outside, file: u32, list: Ptr32<Ptr32<UserNode>>) {
    // the count and the tag length the original reads into
    let frame = m.local(8);
    let (count_at, len_at) = (frame + 4, frame);
    let mut last: Ptr32<UserNode> = Ptr32::NULL;
    byteswap_fread(m, o, count_at, 4, 1, file);
    let mut i: i32 = 0;
    while i < m.i32(count_at) {
        let node: Ptr32<UserNode> = Ptr32::new(m.si_calloc(1, 0x14));
        byteswap_fread(m, o, len_at, 4, 1, file);
        let len = m.u32(len_at);
        let tag = if alloc_fits(m, o, file, len.wrapping_add(1), len) { m.si_calloc(len.wrapping_add(1), 1) } else { 0 };
        m[node].tag = Ptr32::new(tag);
        byteswap_fread(m, o, tag, 1, len, file);
        byteswap_fread(m, o, node.addr() + 4, 2, 1, file);
        byteswap_fread(m, o, node.addr() + 8, 4, 1, file);
        let size = m[node].size;
        let data = if alloc_fits(m, o, file, size, size) { m.si_calloc(size, 1) } else { 0 };
        m[node].data = Ptr32::new(data);
        let (item, n) = if (m[FILE_VERSION] as f64) < 3.82099 { (m[node].size, 1) } else { (1, m[node].size) };
        byteswap_fread(m, o, data, item, n, file);
        if last.is_null() {
            m[list] = node;
        } else {
            m[last].next = node;
        }
        i += 1;
        last = node;
    }
    m.release(8);
}

/// [soft 0x084630dc SI_userDataCopyFromSetup]: appends to the empty list `*dst` a copy of
/// every block of `src` whose tag is neither `CUSELEMCB` nor `CUSNEXTELEMCB` (the two a
/// custom effect hangs on an element for its own bookkeeping). A block's tag is duplicated
/// and its data copied into a new block of the same size (`SI_malloc`, so none for a size
/// of 0).
pub fn user_data_copy_from_setup(m: &mut Mem, src: Ptr32<UserNode>, dst: Ptr32<Ptr32<UserNode>>) {
    let mut src = src;
    let mut last: Ptr32<UserNode> = Ptr32::NULL;
    while !src.is_null() {
        let tag = m[src].tag;
        let skip = !tag.is_null() && {
            let text = m.cstr(tag.addr());
            text == b"CUSELEMCB" || text == b"CUSNEXTELEMCB"
        };
        if !skip {
            let node: Ptr32<UserNode> = Ptr32::new(m.si_calloc(1, 0x14));
            // a block without a tag: the original hands null to `strdup`
            let copy = m.strdup(m[src].tag.addr());
            m[node].tag = Ptr32::new(copy);
            m[node].flag = m[src].flag;
            m[node].size = m[src].size;
            let size = m[src].size;
            let data = m.si_malloc(size);
            m[node].data = Ptr32::new(data);
            let from = m[src].data.addr();
            si_mem::clib::memcpy(m, data, from, size);
            if last.is_null() {
                m[dst] = node;
            } else {
                m[last].next = node;
            }
            last = node;
        }
        src = m[src].next;
    }
}
