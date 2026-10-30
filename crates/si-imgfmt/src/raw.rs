// SPDX-License-Identifier: MIT
//! Headerless pictures: one file per plane ("raw": `.r`, `.g`, `.b`, `.a`) and interleaved
//! bytes in one file ("rgb": `.rgb`, `.rgba`). Original source file `raw.c`.
//!
//! Neither kind has a header: the caller supplies width and height (the import dialogs ask the
//! user for them), and the reader takes `width × height` bytes per plane, first row first.
//! Nothing is checked. Reading stops at nothing either: past the end of a file `getc` returns
//! −1, and the reader stores that as the byte 0xff, so a short file gives a picture whose
//! tail is 0xff, with success reported.
//!
//! How the files are found is the opening routine's business (`SI_softOpen`, not ported here):
//! given a name and an extension it opens `name + extension` unless the name already ends in
//! that extension; **when reading**, if `name + extension` does not exist it opens the bare
//! name instead; a name starting with `-` means standard input or output. The functions here
//! take what that routine returned: the bytes of the file, or `None` where it found nothing.
//! None of the readers asks it to display an error; the writers do.

use si_file::Stream;
use si_picture::Picture;

/// The extensions of the four plane files, in the order they are read and written.
pub const RAW_EXTENSIONS: [&str; 4] = [".r", ".g", ".b", ".a"];
/// The extension of the interleaved file without and with alpha.
pub const RGB_EXTENSION: &str = ".rgb";
pub const RGBA_EXTENSION: &str = ".rgba";

/// Where a raw picture is read from.
#[derive(Clone, Copy, Debug)]
pub enum RawSource<'a> {
    /// What opening `name.r`, `name.g`, `name.b`, `name.a` gave (mode `rb`).
    Files([Option<&'a [u8]>; 4]),
    /// A name starting with `-`: every one of the four opens yields standard input, so the
    /// planes are read one after another from the same stream, and alpha always "exists".
    Stdin(&'a [u8]),
}

/// The cases in which the original gives up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawError {
    /// A plane file could not be opened: red, green or blue for a raw picture (index 0 to 2;
    /// a missing alpha file is not an error), the `.rgba` file for an interleaved one.
    CannotOpen(usize),
    /// Not a case of the original: `width × height` is negative. The original passes the
    /// product to its allocator and then counts it down to zero through the whole 32-bit
    /// range, writing as it goes; it cannot survive that.
    NegativeSize,
    /// Not a case of the original: more than 64 million pixels. The size comes from the
    /// caller, not from the file, and the original allocates it whatever the files hold.
    TooBig,
    /// Not a case of the original: the interleaved writer was given a picture without a red,
    /// green or blue plane. The original reads through the null pointer.
    MissingPlane,
    /// Not a case of the original: a plane holds fewer than `width × height` bytes. The
    /// original writes that many bytes from the pointer whatever is there.
    ShortPlane,
}

impl RawError {
    /// The text the original shows the user: none in every case. The readers open their files
    /// with the opener's error display off and report failure by return value alone (the
    /// import dialogs then say "Cannot read raw picture file …" or "Cannot find ….rgb");
    /// see [`rgb_unshown_message`] for a text the interleaved reader builds and discards.
    pub fn message(&self, _name: &str) -> String {
        String::new()
    }
}

/// The text [`read_rgb`]'s original formats into a local buffer when neither file opens, and
/// then never passes to anything: `program` is the global program name.
pub fn rgb_unshown_message(program: &str, name: &str) -> String {
    format!("{program}: Cannot open either {name}, {name}.rgb, or {name}.rgba")
}

/// `width × height` as the original computes it (a 32-bit product of two 16-bit values).
fn count(width: i16, height: i16) -> Result<usize, RawError> {
    let n = width as i32 * height as i32;
    if n < 0 {
        return Err(RawError::NegativeSize);
    }
    if n as usize > si_picture::MAX_ALLOC / 4 {
        return Err(RawError::TooBig);
    }
    Ok(n as usize)
}

/// A plane of `n` bytes read with `getc`, end of file stored as 0xff. For `n` = 0 there is no
/// plane: the program's allocator returns a null pointer for a size of 0.
fn getc_plane(s: &mut Stream, n: usize) -> Option<Vec<u8>> {
    // every byte is assigned, so the original's uninitialised allocation never shows
    (n != 0).then(|| (0..n).map(|_| s.getc().unwrap_or(0xff)).collect())
}

/// Reads a raw picture: one file per plane [soft 0x085d7b94 SI_pictureReadRaw]
/// (wrapped unchanged by [soft 0x085d7b20 readRawPicture]).
///
/// The caller's record supplies width and height. The ratio is set to 1.0 and `fields` to 3
/// before anything is opened. Red, green and blue are required, in that order; alpha is taken
/// if its file opens and is absent otherwise. Each plane is `width × height` bytes from its
/// file; whatever the file lacks is 0xff. Extra bytes in a file are ignored.
///
/// On failure the original leaves the planes read so far in the caller's record (nobody frees
/// them) and never reaches the alpha slot, which keeps whatever the caller had there.
/// `pad` is not touched by the original; it is 0 here. A size of zero pixels gives a picture
/// with no planes at all.
pub fn read_raw(width: i16, height: i16, source: RawSource) -> Result<Picture, RawError> {
    let n = count(width, height)?;
    let mut pic = Picture { width, height, ratio: 1.0, fields: 3, pad: 0, planes: Default::default() };
    match source {
        RawSource::Files(files) => {
            for (i, file) in files.iter().enumerate() {
                match file {
                    Some(data) => pic.planes[i] = getc_plane(&mut Stream::new(data), n),
                    None if i < 3 => return Err(RawError::CannotOpen(i)),
                    None => {}
                }
            }
        }
        RawSource::Stdin(data) => {
            let mut s = Stream::new(data);
            for plane in pic.planes.iter_mut() {
                *plane = getc_plane(&mut s, n);
            }
        }
    }
    Ok(pic)
}

/// Writes a raw picture [soft 0x085d7dbc SI_pictureWriteRaw] (wrapped unchanged by
/// [soft 0x085d7b3c writeRawPicture]): for each plane the picture has, in the order red,
/// green, blue, alpha, the bytes of one file (`name` + [`RAW_EXTENSIONS`], mode `wb`): the
/// plane as it is. An absent plane writes no file. There is no header, so width and height
/// are lost.
///
/// The original fails only when one of the files cannot be created (the opener then shows
/// the system's error text); files written before that stay. With a name starting with `-`
/// all planes go to standard output one after another: concatenate the result.
pub fn write_raw(pic: &Picture) -> Result<[Option<Vec<u8>>; 4], RawError> {
    let n = count(pic.width, pic.height)?;
    let mut out: [Option<Vec<u8>>; 4] = Default::default();
    for (i, plane) in pic.planes.iter().enumerate() {
        if let Some(p) = plane {
            out[i] = Some(p.get(..n).ok_or(RawError::ShortPlane)?.to_vec());
        }
    }
    Ok(out)
}

/// Reads an interleaved picture [soft 0x085d7fa4 SI_pictureReadRgb] (wrapped unchanged by
/// [soft 0x085d7b58 readRgbPicture]).
///
/// The caller's record supplies width and height, and its alpha slot doubles as a request:
/// `want_alpha` is "that slot is not null" (the import dialog stores its with-alpha flag
/// there). `rgb` and `rgba` are what opening `name.rgb` and `name.rgba` gives.
///
/// - Without the request, and if `.rgb` opens: three bytes per pixel, red, green, blue.
///   The alpha slot stays null.
/// - Otherwise (alpha requested, in which case `.rgb` is not even tried, or `.rgb` missing):
///   `.rgba` is opened; four bytes per pixel, red, green, blue, alpha. If it does not open
///   the reader fails.
///
/// Ratio 1.0, `fields` 3, set first. Bytes past the end of the file are 0xff.
pub fn read_rgb(width: i16, height: i16, want_alpha: bool, rgb: Option<&[u8]>, rgba: Option<&[u8]>) -> Result<Picture, RawError> {
    let n = count(width, height)?;
    let mut pic = Picture { width, height, ratio: 1.0, fields: 3, pad: 0, planes: Default::default() };
    let (data, k) = match (want_alpha, rgb, rgba) {
        (false, Some(d), _) => (d, 3),
        (_, _, Some(d)) => (d, 4),
        _ => return Err(RawError::CannotOpen(3)),
    };
    if n == 0 {
        return Ok(pic); // the allocator gives null pointers for a size of 0
    }
    let mut s = Stream::new(data);
    let mut planes: Vec<Vec<u8>> = (0..k).map(|_| Vec::with_capacity(n)).collect();
    for _ in 0..n {
        for p in planes.iter_mut() {
            p.push(s.getc().unwrap_or(0xff));
        }
    }
    for (slot, p) in pic.planes.iter_mut().zip(planes) {
        *slot = Some(p);
    }
    Ok(pic)
}

/// Writes an interleaved picture [soft 0x085d81c4 SI_pictureWriteRgb] (wrapped unchanged by
/// [soft 0x085d7b74 writeRgbPicture]).
///
/// `with_alpha` false: the `.rgb` file, three bytes per pixel. True: the `.rgba` file, four
/// bytes per pixel, the fourth being the alpha plane's byte or **0xff if the picture has no
/// alpha plane**. The original fails only if the file cannot be created.
pub fn write_rgb(pic: &Picture, with_alpha: bool) -> Result<Vec<u8>, RawError> {
    let n = count(pic.width, pic.height)?;
    let [Some(r), Some(g), Some(b), a] = &pic.planes else { return Err(RawError::MissingPlane) };
    if [Some(r), Some(g), Some(b), a.as_ref().filter(|_| with_alpha)].iter().flatten().any(|p| p.len() < n) {
        return Err(RawError::ShortPlane);
    }
    let mut out = Vec::with_capacity(n * if with_alpha { 4 } else { 3 });
    for i in 0..n {
        out.extend_from_slice(&[r[i], g[i], b[i]]);
        if with_alpha {
            out.push(a.as_ref().map_or(0xff, |a| a[i]));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(alpha: bool) -> Picture {
        let (w, h) = (5usize, 3usize);
        let plane = |m: usize| Some((0..w * h).map(|i| (i * m + m) as u8).collect::<Vec<u8>>());
        Picture { width: 5, height: 3, ratio: 1.0, fields: 3, pad: 0, planes: [plane(1), plane(7), plane(13), if alpha { plane(29) } else { None }] }
    }

    fn files(out: &[Option<Vec<u8>>; 4]) -> [Option<&[u8]>; 4] {
        [out[0].as_deref(), out[1].as_deref(), out[2].as_deref(), out[3].as_deref()]
    }

    #[test]
    fn raw_round_trip() {
        for alpha in [false, true] {
            let p = sample(alpha);
            let out = write_raw(&p).unwrap();
            assert_eq!(out[3].is_some(), alpha);
            assert_eq!(out[0].as_ref().unwrap().len(), 15);
            assert_eq!(read_raw(5, 3, RawSource::Files(files(&out))).unwrap(), p);
        }
    }

    #[test]
    fn raw_missing_files() {
        let out = write_raw(&sample(true)).unwrap();
        for i in 0..3 {
            let mut f = files(&out);
            f[i] = None;
            assert_eq!(read_raw(5, 3, RawSource::Files(f)).unwrap_err(), RawError::CannotOpen(i));
        }
        // a picture without a red plane writes no .r file, and such a set cannot be read back
        let mut p = sample(false);
        p.planes[0] = None;
        let out = write_raw(&p).unwrap();
        assert!(out[0].is_none() && out[1].is_some());
        assert_eq!(read_raw(5, 3, RawSource::Files(files(&out))).unwrap_err(), RawError::CannotOpen(0));
    }

    #[test]
    fn raw_short_files_fill_with_ff() {
        let p = sample(true);
        let out = write_raw(&p).unwrap();
        for cut in 0..=15 {
            let f = [out[0].as_deref().map(|d| &d[..cut]), out[1].as_deref(), out[2].as_deref(), out[3].as_deref()];
            let got = read_raw(5, 3, RawSource::Files(f)).unwrap();
            let red = got.planes[0].as_ref().unwrap();
            assert_eq!(&red[..cut], &p.planes[0].as_ref().unwrap()[..cut]);
            assert!(red[cut..].iter().all(|&b| b == 0xff) && red.len() == 15);
            assert_eq!(got.planes[1], p.planes[1]);
        }
    }

    #[test]
    fn raw_stdin_reads_four_planes_in_sequence() {
        let p = sample(true);
        let out = write_raw(&p).unwrap();
        let all: Vec<u8> = out.iter().flatten().flatten().copied().collect();
        assert_eq!(read_raw(5, 3, RawSource::Stdin(&all)).unwrap(), p);
        // three planes on standard input: alpha is still made, all 0xff
        let got = read_raw(5, 3, RawSource::Stdin(&all[..45])).unwrap();
        assert_eq!(got.planes[3], Some(vec![0xff; 15]));
        for cut in 0..all.len() {
            let got = read_raw(5, 3, RawSource::Stdin(&all[..cut])).unwrap();
            let flat: Vec<u8> = got.planes.iter().flatten().flatten().copied().collect();
            assert_eq!(&flat[..cut], &all[..cut]);
            assert!(flat[cut..].iter().all(|&b| b == 0xff) && flat.len() == 60);
        }
    }

    #[test]
    fn sizes() {
        // zero pixels: no planes (the allocator returns null for a size of 0), success
        let got = read_raw(0, 7, RawSource::Files([Some(&[1, 2]), Some(&[]), Some(&[]), None])).unwrap();
        assert_eq!(got.planes, [None, None, None, None]);
        assert_eq!((got.ratio, got.fields), (1.0, 3));
        assert_eq!(read_rgb(7, 0, true, None, Some(&[1, 2, 3, 4])).unwrap().planes, [None, None, None, None]);
        // two negative sizes multiply to a positive count, as in the original
        let got = read_raw(-2, -3, RawSource::Stdin(&[9; 24])).unwrap();
        assert_eq!((got.width, got.height, got.planes[0].as_ref().unwrap().len()), (-2, -3, 6));
        assert_eq!(read_raw(-2, 3, RawSource::Stdin(&[])).unwrap_err(), RawError::NegativeSize);
        assert_eq!(read_rgb(2, -3, false, Some(&[]), None).unwrap_err(), RawError::NegativeSize);
    }

    #[test]
    fn rgb_round_trip_and_alpha_default() {
        let p = sample(false);
        let rgb = write_rgb(&p, false).unwrap();
        assert_eq!(rgb.len(), 45);
        assert_eq!(&rgb[..6], &[1, 7, 13, 2, 14, 26]);
        assert_eq!(read_rgb(5, 3, false, Some(&rgb), None).unwrap(), p);
        // no alpha plane: the .rgba file carries 0xff, which comes back as a plane
        let rgba = write_rgb(&p, true).unwrap();
        assert_eq!(&rgba[..8], &[1, 7, 13, 0xff, 2, 14, 26, 0xff]);
        let got = read_rgb(5, 3, true, None, Some(&rgba)).unwrap();
        assert_eq!(got.planes[3], Some(vec![0xff; 15]));
        assert_eq!(got.planes[..3], p.planes[..3]);
        let q = sample(true);
        let rgba = write_rgb(&q, true).unwrap();
        assert_eq!(read_rgb(5, 3, true, None, Some(&rgba)).unwrap(), q);
        // writing .rgb from a picture with alpha drops it
        assert_eq!(write_rgb(&q, false).unwrap(), rgb);
    }

    #[test]
    fn rgb_file_choice() {
        let p = sample(true);
        let rgb = write_rgb(&p, false).unwrap();
        let rgba = write_rgb(&p, true).unwrap();
        // no request: .rgb wins when both exist
        assert_eq!(read_rgb(5, 3, false, Some(&rgb), Some(&rgba)).unwrap().planes[3], None);
        // no request and no .rgb: falls back to .rgba
        assert_eq!(read_rgb(5, 3, false, None, Some(&rgba)).unwrap(), p);
        // request: .rgb is never looked at
        assert_eq!(read_rgb(5, 3, true, Some(&rgb), Some(&rgba)).unwrap(), p);
        assert_eq!(read_rgb(5, 3, true, Some(&rgb), None).unwrap_err(), RawError::CannotOpen(3));
        assert_eq!(read_rgb(5, 3, false, None, None).unwrap_err(), RawError::CannotOpen(3));
        assert_eq!(RawError::CannotOpen(3).message("x"), "");
        assert_eq!(rgb_unshown_message("soft", "x"), "soft: Cannot open either x, x.rgb, or x.rgba");
    }

    #[test]
    fn rgb_truncation_at_every_byte() {
        let p = sample(true);
        for (with_alpha, k) in [(false, 3usize), (true, 4)] {
            let bytes = write_rgb(&p, with_alpha).unwrap();
            for cut in 0..=bytes.len() {
                let got = if with_alpha { read_rgb(5, 3, true, None, Some(&bytes[..cut])) } else { read_rgb(5, 3, false, Some(&bytes[..cut]), None) }.unwrap();
                for i in 0..15 * k {
                    let v = got.planes[i % k].as_ref().unwrap()[i / k];
                    assert_eq!(v, if i < cut { bytes[i] } else { 0xff });
                }
            }
        }
    }

    #[test]
    fn rgb_writer_needs_colour_planes() {
        let mut p = sample(true);
        p.planes[1] = None;
        assert_eq!(write_rgb(&p, true).unwrap_err(), RawError::MissingPlane);
        let mut p = sample(true);
        p.planes[3].as_mut().unwrap().pop();
        assert_eq!(write_rgb(&p, true).unwrap_err(), RawError::ShortPlane);
        assert!(write_rgb(&p, false).is_ok()); // alpha is not looked at for .rgb
        assert_eq!(write_raw(&p).unwrap_err(), RawError::ShortPlane);
    }
}
