// SPDX-License-Identifier: MIT
//! The element dictionary's file readers for a camera, a light and a lattice: open the
//! file, read its header, hand the stream to the reader of `bsceneio.c`, close it.
//!
//! When `SI_pipedFiles` is not 0 the stream is `SI_pipe` instead of a file and is left
//! open.

use super::bsceneio::{camera_binary_read, lattice_read_with, light_binary_read};
use super::globals::{IN_VERSION, MODE_CAMERA, MODE_LATTICE, MODE_LIGHT, SI_PIPE, SI_PIPED_FILES, SI_PROC_DATA};
use super::headers::{dbfile_read_bin_header, read_bin_header_at};
use super::off::file_chunk;
use super::outside::{Outside, FCLOSE, FOPEN};
use super::Run;
use si_mem::Mem;

fn open(m: &mut Mem, o: &mut dyn Outside, path: u32, mode: u32) -> u32 {
    if m[SI_PIPED_FILES] == 0 {
        o.call(m, &FOPEN, &[path, mode])
    } else {
        m[SI_PIPE]
    }
}

fn close(m: &mut Mem, o: &mut dyn Outside, stream: u32) {
    if m[SI_PIPED_FILES] == 0 {
        o.call(m, &FCLOSE, &[stream]);
    }
}

/// [soft 0x0810fdf0 SI_elemdictReadCamera]
/// Reads a camera file into the scene's camera (the third word of the record
/// `SI_procData` points at) and answers that camera; 0 when the file cannot be opened.
pub fn elemdict_read_camera(m: &mut Mem, o: &mut dyn Outside, path: u32) -> Run<u32> {
    let stream = open(m, o, path, MODE_CAMERA);
    if stream == 0 {
        return Ok(0);
    }
    dbfile_read_bin_header(m, o, stream, IN_VERSION.addr(), 0);
    let proc_data = m[SI_PROC_DATA];
    let cam = m.u32(proc_data.wrapping_add(8));
    camera_binary_read(m, o, stream, cam)?;
    let proc_data = m[SI_PROC_DATA];
    let cam = m.u32(proc_data.wrapping_add(8));
    close(m, o, stream);
    Ok(cam)
}

/// [soft 0x0810fe78 SI_elemdictReadLight]
/// Reads a light file and answers the light its type token made; 0 when the file cannot be
/// opened.
///
/// The original hands the reader a local it never set: a file without a type token
/// answers whatever the stack held, and a token in front of the type token stores through
/// it. Here the local holds what the image's stack holds (zero in an image of zeros: such
/// a file then answers null and its guarded tokens store nothing).
pub fn elemdict_read_light(m: &mut Mem, o: &mut dyn Outside, path: u32) -> Run<u32> {
    let stream = open(m, o, path, MODE_LIGHT);
    if stream == 0 {
        return Ok(0);
    }
    let sp = m.sp();
    let local = m.local(4);
    dbfile_read_bin_header(m, o, stream, IN_VERSION.addr(), 0);
    let run = light_binary_read(m, o, stream, local);
    let l = m.u32(local);
    m.set_sp(sp);
    run?;
    close(m, o, stream);
    Ok(l)
}

/// [soft 0x08110200 SI_elemdictReadLattice]
/// Reads a lattice file onto a model's node list or branch list and answers the reader's
/// 16-bit result; 0 when the file cannot be opened.
///
/// **What a lattice gets for a field its file does not state.** The lattice reader copies
/// its interpolation codes and its three vectors from locals it never sets, and its frame
/// lies where the header reader's just lay. Stepping the original through the harness
/// (`re/tools/si-elemio/probe_lattice_stack.py`) shows what those locals hold: the second
/// and third interpolation code are the header's magic number and its version (the float's
/// bits), the minimum, the maximum and the scaling are the bytes 8 to 43 of the header's
/// comment, read as floats without swapping, and the first interpolation code is a word
/// nobody wrote. The port lays the two readers' locals over each other the same way, so
/// that the same bytes arrive; the first code is what the image's stack holds.
pub fn elemdict_read_lattice(m: &mut Mem, o: &mut dyn Outside, path: u32, branch: i16, model: u32) -> Run<i32> {
    let stream = open(m, o, path, MODE_LATTICE);
    if stream == 0 {
        return Ok(0);
    }
    let sp = m.sp();
    // the header's 88 bytes, and below them the word that becomes the first code
    let chunk = m.local(file_chunk::SIZE);
    let below = m.local(4);
    read_bin_header_at(m, o, stream, IN_VERSION.addr(), 0, chunk);
    let locals = [below, chunk + 0x10, chunk + 0x1c, chunk + 0x28];
    let run = lattice_read_with(m, o, stream, model, branch, locals);
    m.set_sp(sp);
    let r = run?;
    close(m, o, stream);
    Ok(r as i16 as i32)
}
