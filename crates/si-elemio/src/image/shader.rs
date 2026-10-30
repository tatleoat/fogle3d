// SPDX-License-Identifier: MIT
//! The custom shader field (`b_matio.c`, with the version test of `headers.c`) and the user
//! data field (`binary.c`), which camera, light, material and model files share.

use super::byteswap::byteswap_fread;
use super::globals::{FILE_VERSION_BINARY, NULL_NAME, SI_VERSION_INPUT};
use super::off::{prototypes, udf, user_node};
use super::outside::{Outside, SI_UDF_ALLOCATE, SI_UDF_DISPOSE};
use super::utilio::{file_skip_field, string_binary_read};
use super::{faults, NoReturn, Run, COUNT_LIMIT};
use si_mem::Mem;

/// The doubles `si_dbfileIsOldUdfVersion` compares with (0x0865c5a0, 0x0865c5a8,
/// 0x0865c5b0).
pub const OLD_UDF_UP_TO: f64 = 2.67906;
pub const OLD_UDF_FROM: f64 = 2.69;
pub const OLD_UDF_BELOW: f64 = 2.999019;
/// The double `si_userDataRead` compares `fileVersion` with (0x0865bc30).
pub const USER_DATA_BYTES_FROM: f64 = 3.82099;

/// [soft 0x083fb934 si_dbfileIsOldUdfVersion]
/// Whether a file of this version holds the old form of a custom shader: a version up to
/// 2.67906, or from 2.69 to below 2.999019. The float is compared with doubles on the x87
/// (exact in `f64`: both operands are doubles' values); not a number is "no".
pub fn dbfile_is_old_udf_version(version_bits: u32) -> u32 {
    let v = f32::from_bits(version_bits) as f64;
    (v <= OLD_UDF_UP_TO || (v >= OLD_UDF_FROM && v < OLD_UDF_BELOW)) as u32
}

/// [soft 0x083e9374 readOldUDF]
/// Reads the old form of a custom shader and drops it: by the prototype block's token list
/// when it has one, else tokens 1 to 5 (three texts, a 16-bit and a 32-bit number).
pub fn read_old_udf(m: &mut Mem, o: &mut dyn Outside, stream: u32, protos: u32) -> Run<()> {
    let sp = m.sp();
    let local4 = m.local(4);
    let local2 = m.local(4);
    let mut index = 0u32;
    let mut token: u16;
    let list = if protos != 0 { m.u32(protos + prototypes::UDF) } else { 0 };
    let mut enter = true;
    if list != 0 {
        token = m.u16(list);
        index = 1;
        enter = token != 0;
    } else {
        token = 1;
    }
    while enter {
        match (token as i16 as i32).wrapping_sub(1) as i16 as i32 as u32 {
            0..=2 => {
                let s = string_binary_read(m, o, stream).inspect_err(|_| m.set_sp(sp))?;
                if s != 0 {
                    m.free(s);
                }
            }
            3 => {
                byteswap_fread(m, o, local2, 2, 1, stream);
            }
            4 => {
                byteswap_fread(m, o, local4, 4, 1, stream);
            }
            _ => {
                file_skip_field(m, o, stream, token as i16).inspect_err(|_| m.set_sp(sp))?;
            }
        }
        // the original reads the block's list pointer again, with no test of the block
        let list = m.u32(protos.wrapping_add(prototypes::UDF));
        if list != 0 {
            token = m.u16(list.wrapping_add(index.wrapping_mul(2)));
            index = index.wrapping_add(1);
        } else {
            token = token.wrapping_add(1);
        }
        if token == 6 {
            token = 0;
        }
        enter = token != 0;
    }
    m.set_sp(sp);
    Ok(())
}

/// [soft 0x083e9154 si_udfRead]
/// A custom shader: for a file version the program calls old the old form is read and
/// dropped and the answer is null. Otherwise a record from `SI_udfAllocate (0x0854c2f4)`
/// filled by the prototype block's token list, or by tokens 1 to 5 when the block has none
/// (the name, a copy in a block of its own; two 32-bit numbers kept as 16 bits; a 32-bit
/// number). An empty name disposes of the record. A list that ends in a zero (not in token
/// 5) also disposes of a record named `NULL`.
pub fn udf_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, protos: u32) -> Run<u32> {
    let before = faults(m);
    if dbfile_is_old_udf_version(m[SI_VERSION_INPUT]) & 0xffff != 0 {
        read_old_udf(m, o, stream, protos)?;
        return Ok(0);
    }
    let sp = m.sp();
    let local = m.local(4);
    let mut udf_p = o.call(m, &SI_UDF_ALLOCATE, &[]);
    let mut index: i16 = 0;
    let mut token: u16;
    let list = if protos != 0 { m.u32(protos + prototypes::UDF) } else { 0 };
    let mut enter = true;
    if list != 0 {
        token = m.u16(list);
        index = 1;
        enter = token != 0;
    } else {
        token = 1;
    }
    while enter {
        match (token as i16 as i32).wrapping_sub(1) as i16 as i32 as u32 {
            0 => {
                let s = string_binary_read(m, o, stream).inspect_err(|_| m.set_sp(sp))?;
                if udf_p == 0 || s == 0 || m.u8(s) == 0 {
                    o.call(m, &SI_UDF_DISPOSE, &[udf_p]);
                    udf_p = 0;
                } else {
                    let copy = m.strdup(s);
                    m.set_u32(udf_p + udf::NAME, copy);
                }
                if s != 0 {
                    m.free(s);
                }
            }
            1 => {
                byteswap_fread(m, o, local, 4, 1, stream);
                if udf_p != 0 {
                    let v = m.u32(local);
                    m.set_u16(udf_p + udf::WORD_0C, v as u16);
                }
            }
            2 => {
                byteswap_fread(m, o, local, 4, 1, stream);
                if udf_p != 0 {
                    let v = m.u32(local);
                    m.set_u16(udf_p + udf::WORD_10, v as u16);
                }
            }
            3 => {
                byteswap_fread(m, o, local, 4, 1, stream);
                if udf_p != 0 {
                    let v = m.u32(local);
                    m.set_u32(udf_p + udf::WORD_14, v);
                }
            }
            4 => {
                // token 5 ends the shader at once, without the test of the name below
                m.set_sp(sp);
                return finish(m, before, udf_p);
            }
            _ => {
                file_skip_field(m, o, stream, token as i16).inspect_err(|_| m.set_sp(sp))?;
            }
        }
        // the original reads the block's list pointer again, with no test of the block
        let list = m.u32(protos.wrapping_add(prototypes::UDF));
        if list != 0 {
            token = m.u16(list.wrapping_add((index as i32 as u32).wrapping_mul(2)));
            index = index.wrapping_add(1);
        } else {
            token = token.wrapping_add(1);
            if token == 6 {
                token = 0;
            }
        }
        enter = token != 0;
    }
    m.set_sp(sp);
    if udf_p != 0 {
        let name = m.u32(udf_p + udf::NAME);
        if name != 0 {
            // five bytes compared with "NULL" and its zero, up to the first difference
            let mut same = true;
            for k in 0..5u32 {
                if m.u8(name + k) != m.u8(NULL_NAME + k) {
                    same = false;
                    break;
                }
            }
            if same {
                o.call(m, &SI_UDF_DISPOSE, &[udf_p]);
                udf_p = 0;
            }
        }
    }
    finish(m, before, udf_p)
}

fn finish(m: &Mem, before: u32, udf_p: u32) -> Run<u32> {
    if faults(m) != before {
        return Err(NoReturn::Fault("si_udfRead"));
    }
    Ok(udf_p)
}

/// [soft 0x083ec910 si_userDataRead]
/// The user data of a camera, light or model: a 32-bit count, then per node a 32-bit name
/// length, the name, a 16-bit word, a 32-bit size and the data. The nodes (0x14 bytes each,
/// cleared) are chained in the file's order and the first is stored through `first`.
///
/// For a `fileVersion` (the static of `binary.c`) below 3.82099 the data is read as **one
/// item of `size` bytes**, which the byte-swapping read reverses when the size is 2, 4 or
/// 8; from that version on as `size` bytes.
///
/// Three things the port does not do. The original increments `SI_secureLicAlloc`
/// (0x08679660) before each of its three allocations per node; that counter belongs to the
/// licence scheme, which is not ported, and the port leaves it alone. The count and the
/// name length are locals nobody set when their read fails; here they hold what the
/// image's stack holds. And a name length or data size larger than the file has left (and
/// than [`COUNT_LIMIT`]) is not allocated for: the port stops.
pub fn user_data_read(m: &mut Mem, o: &mut dyn Outside, stream: u32, first: u32) -> Run<()> {
    let before = faults(m);
    let sp = m.sp();
    let count_at = m.local(4);
    let length_at = m.local(4);
    let mut last = 0u32;
    byteswap_fread(m, o, count_at, 4, 1, stream);
    let mut i: i32 = 0;
    while i < m.i32(count_at) {
        let node = m.si_calloc(1, user_node::SIZE);
        byteswap_fread(m, o, length_at, 4, 1, stream);
        let length = m.u32(length_at);
        if length.wrapping_add(1) > COUNT_LIMIT && length > o.file_left(m, stream) {
            m.set_sp(sp);
            return Err(NoReturn::Count("si_userDataRead"));
        }
        let name = m.si_calloc(length.wrapping_add(1), 1);
        m.set_u32(node + user_node::NAME, name);
        let name = m.u32(node + user_node::NAME);
        byteswap_fread(m, o, name, 1, length, stream);
        byteswap_fread(m, o, node + user_node::WORD, 2, 1, stream);
        byteswap_fread(m, o, node + user_node::DATA_SIZE, 4, 1, stream);
        let size = m.u32(node + user_node::DATA_SIZE);
        if size > COUNT_LIMIT && size > o.file_left(m, stream) {
            m.set_sp(sp);
            return Err(NoReturn::Count("si_userDataRead"));
        }
        let data = m.si_calloc(size, 1);
        m.set_u32(node + user_node::DATA, data);
        let version = f32::from_bits(m[FILE_VERSION_BINARY]) as f64;
        let (item, items) = if version < USER_DATA_BYTES_FROM { (m.u32(node + user_node::DATA_SIZE), 1) } else { (1, m.u32(node + user_node::DATA_SIZE)) };
        let data = m.u32(node + user_node::DATA);
        byteswap_fread(m, o, data, item, items, stream);
        if last == 0 {
            m.set_u32(first, node);
        } else {
            m.set_u32(last + user_node::NEXT, node);
        }
        i += 1;
        last = node;
        if faults(m) != before {
            // a node or a block that could not be allocated: the original has died
            m.set_sp(sp);
            return Err(NoReturn::Fault("si_userDataRead"));
        }
    }
    m.set_sp(sp);
    Ok(())
}
