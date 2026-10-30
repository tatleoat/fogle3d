// SPDX-License-Identifier: MIT
//! Icons beyond the appended-icon reader and writer of `lib.rs`: making an icon from a
//! picture, with the scaling the program uses, and the look-up order of the browsers.
//!
//! Sources are functions of the Linux `soft` binary, cited as `[soft 0xADDRESS name]`.
//! Source file: `geticon.c`.

use crate::{icon_offset, read_mixed, read_pict_chunk, read_rgba, write_icon, OpenPicture, Picture, PictureError, PictureRgba, MAX_ALLOC};
use si_file::Stream;
use si_x87::{x, Round, F80};

/// The longer side of an icon made from a picture, in pixels (the constant 75.0, stored
/// three times: 0x08673138, 0x0867313c, 0x08673140).
pub const ICON_MAX: i16 = 75;

/// The size of the icon made from a `width × height` picture.
///
/// A picture of at most 75 × 75 keeps its size. Otherwise the longer side becomes 75 and the
/// other `(shorter / longer) × 75`, truncated; a square or upright picture is 75 high.
/// The arithmetic is done in the x87 unit, quotient first (class: model).
///
/// `at_least_one` is the difference between the three routines that share this:
/// `SI_iconLoadFromPicRGBA` raises a result of 0 to 1 (when the picture's own side is above
/// 0); the two that read a picture file do not, so a 800 × 5 picture gives them an icon of
/// 75 × 0.
pub fn icon_size(width: i16, height: i16, at_least_one: bool) -> (i32, i32) {
    let (mut iw, mut ih) = (width as i32, height as i32);
    let scaled = |short: i16, long: i16| ((F80::from_i32(short as i32) / F80::from_i32(long as i32)) * x(75.0f32)).to_i32(Round::Zero);
    if width > ICON_MAX || height > ICON_MAX {
        if height < width {
            iw = ICON_MAX as i32;
            ih = scaled(height, width);
            if at_least_one && ih == 0 && height > 0 {
                ih = 1;
            }
        } else {
            ih = ICON_MAX as i32;
            iw = scaled(width, height);
            if at_least_one && iw == 0 && width > 0 {
                iw = 1;
            }
        }
    }
    (iw, ih)
}

/// The source index all three routines take for icon position `i` of `n` from a side of
/// `size`: `size × (i / (n + 1))`, truncated (x87, class: model). Note the `n + 1`: the last
/// source rows and columns are never reached, and position 0 maps to 0.
fn sample(i: i32, n: i32, size: i16) -> i32 {
    ((F80::from_i32(i) / F80::from_i32(n + 1)) * F80::from_i32(size as i32)).to_i32(Round::Zero)
}

/// Makes the icon of a picture file, as planes [soft 0x085c39d0 SI_pictReadAsIcon].
///
/// The file is read row by row into four row buffers; icon row `i` is taken from source row
/// `height × (i / (icon height + 1))`, which is reached by reading on until that row is in
/// the buffers, and icon column `x` from source column `width × (x / (icon width + 1))`.
/// Nothing is averaged. The result has all four planes and the header's ratio, fields and
/// spare word.
///
/// When a row cannot be read, the icon row being made is still filled from whatever the row
/// buffers hold and the icon ends there. Two places where the original uses memory it never
/// set, and this has zeros:
/// - the icon rows after that point;
/// - the alpha plane of a picture without alpha (the alpha row buffer is never written).
pub fn pict_read_as_icon(data: &[u8]) -> Result<Picture, PictureError> {
    let mut file = OpenPicture::open(data)?;
    let (w, h) = (file.picture.width, file.picture.height);
    let mut rows: [Option<Vec<u8>>; 4] = std::array::from_fn(|_| Some(vec![0u8; w.max(0) as usize]));
    let (iw, ih) = icon_size(w, h, false);
    let (uw, uh) = (iw.max(0) as usize, ih.max(0) as usize);
    let mut planes: [Vec<u8>; 4] = std::array::from_fn(|_| vec![0u8; uw * uh]);
    let mut current = -1i32;
    let mut ok = true;
    for i in 0..ih {
        let want = sample(i, ih, h);
        while current != want {
            ok = file.scanline_read(&mut rows);
            if !ok {
                break;
            }
            current += 1;
        }
        for xi in 0..iw {
            let sx = sample(xi, iw, w) as usize;
            for (plane, row) in planes.iter_mut().zip(&rows) {
                plane[i as usize * uw + xi as usize] = row.as_ref().and_then(|r| r.get(sx).copied()).unwrap_or(0);
            }
        }
        if !ok {
            break;
        }
    }
    let [r, g, b, a] = planes;
    Ok(Picture { width: iw as i16, height: ih as i16, planes: [Some(r), Some(g), Some(b), Some(a)], ..file.picture })
}

/// Makes the icon of a picture file, packed [soft 0x085c3d64 pictReadAsIconRGBA].
///
/// The same sizes and the same sampling formulas as [`pict_read_as_icon`], with these
/// differences, all reproduced:
/// - Rows are stored bottom-up and made from the last icon row down. The row counter starts
///   at `height − 1` and goes down by one per row read, so when it equals the wanted index
///   the buffers hold the row **one above** it (counted bottom-up): the icon is the picture
///   sampled one source row too high.
/// - A row that cannot be read ends the icon without filling the row being made.
/// - The fourth byte of every pixel is 0, and the word at offset 10 is 0.
///
/// A picture one row high reads nothing (the counter starts on the wanted index) and then
/// tests a result it never set; its icon comes from unset memory. Here it is black.
pub fn pict_read_as_icon_rgba(data: &[u8]) -> Result<PictureRgba, PictureError> {
    let mut file = OpenPicture::open(data)?;
    let (w, h) = (file.picture.width, file.picture.height);
    let mut rows: [Option<Vec<u8>>; 4] = std::array::from_fn(|_| Some(vec![0u8; w.max(0) as usize]));
    let (iw, ih) = icon_size(w, h, false);
    let (uw, uh) = (iw.max(0) as usize, ih.max(0) as usize);
    let mut out = PictureRgba { width: iw as i16, height: ih as i16, ratio: file.picture.ratio, fields: file.picture.fields, pad: 0, data: vec![0u8; 4 * uw * uh] };
    let mut current = h as i32 - 1;
    let mut ok = true;
    for i in (0..ih).rev() {
        let want = sample(i, ih, h);
        while current != want {
            ok = file.scanline_read(&mut rows);
            if !ok {
                break;
            }
            current -= 1;
        }
        if !ok {
            break;
        }
        for xi in 0..iw {
            let sx = sample(xi, iw, w) as usize;
            let at = 4 * (i as usize * uw + xi as usize);
            for c in 0..3 {
                out.data[at + c] = rows[c].as_ref().and_then(|r| r.get(sx).copied()).unwrap_or(0);
            }
        }
    }
    Ok(out)
}

/// Makes an icon from a packed picture in memory [soft 0x085c4084 SI_iconLoadFromPicRGBA].
///
/// Sizes as [`icon_size`] with a minimum of 1; ratio, `fields` and the word at offset 10 are
/// copied. Icon row `i` wants source row `height × (i / (icon height + 1))`, but the row
/// pointer is advanced once more than that index (it starts one step behind and moves
/// before it is used), so the row actually taken is **the one after**: source row 0 is never
/// used, and a picture one row high is read one row past its end (zeros here). Columns as
/// in the other two routines. All four bytes of a pixel are copied.
pub fn icon_from_rgba(p: &PictureRgba) -> PictureRgba {
    let (iw, ih) = icon_size(p.width, p.height, true);
    let (uw, uh) = (iw.max(0) as usize, ih.max(0) as usize);
    let mut out = PictureRgba { width: iw as i16, height: ih as i16, ratio: p.ratio, fields: p.fields, pad: p.pad, data: vec![0u8; 4 * uw * uh] };
    let w = p.width.max(0) as usize;
    for i in 0..ih {
        let row = sample(i, ih, p.height) as usize + 1;
        for xi in 0..iw {
            let from = 4 * (row * w + sample(xi, iw, p.width) as usize);
            if let Some(px) = p.data.get(from..from + 4) {
                let at = 4 * (i as usize * uw + xi as usize);
                out.data[at..at + 4].copy_from_slice(px);
            }
        }
    }
    out
}

/// Reads the icon at the end of a database file as a packed picture
/// [soft 0x085c365c fileIconRgbaReadF].
///
/// The layout is that of [`crate::read_icon`]; rows are stored bottom-up, the fourth byte is
/// the icon's alpha, and the word at offset 10 is 0. Unlike the plane reader this one gives
/// up on the first row that does not decode: a damaged icon is no icon.
///
/// An icon with a negative size is refused here (the original computes with it), and so is
/// one whose size could not be allocated or whose rows the file is too short to hold (the
/// original fails on reading them).
pub fn read_icon_rgba(data: &[u8]) -> Option<PictureRgba> {
    let off = icon_offset(data)?;
    if off > data.len() {
        return None;
    }
    let mut s = Stream::new(data);
    s.seek(off);
    let head = read_pict_chunk(&mut s)?;
    if head.width < 0 || head.height < 0 {
        return None;
    }
    let (w, h) = (head.width as usize, head.height as usize);
    // a row is two run sequences: at least 6 bytes when it has pixels
    if w * h == 0 || 4 * w * h > MAX_ALLOC || h.saturating_mul(6) > data.len() - s.tell() {
        return None; // an empty icon is no icon in the original either: nothing is allocated for it
    }
    let mut out = PictureRgba { width: head.width, height: head.height, ratio: head.ratio, fields: head.fields, pad: 0, data: vec![0u8; 4 * w * h] };
    let mut rows: [Vec<u8>; 4] = std::array::from_fn(|_| vec![0u8; w]);
    for y in (0..h).rev() {
        let (rgb, a) = rows.split_at_mut(3);
        let mut three: Vec<&mut [u8]> = rgb.iter_mut().map(|p| p.as_mut_slice()).collect();
        if !read_mixed(&mut s, w, &mut three, 0, false) || !read_mixed(&mut s, w, &mut [a[0].as_mut_slice()], 0, false) {
            return None;
        }
        for xi in 0..w {
            for (c, row) in rows.iter().enumerate() {
                out.data[4 * (y * w + xi) + c] = row[xi];
            }
        }
    }
    Some(out)
}

/// Gives a picture file its icon [soft 0x085c4808 SI_addIconToPic]: the icon of
/// [`pict_read_as_icon`], appended with [`crate::write_icon`]. The error is the message
/// shown, with the file's name: "Cannot read file : %s" when the picture cannot be read.
/// ("Cannot write icon to : %s" is shown when the file cannot be opened for update, which
/// cannot happen on bytes in memory.)
pub fn add_icon_to_pic(file: &mut Vec<u8>, name: &str) -> Result<(), String> {
    let icon = pict_read_as_icon(file).map_err(|_| format!("Cannot read file : {name}"))?;
    write_icon(file, &icon);
    Ok(())
}

/// The icon of a database file by name [soft 0x085c38e8 fileIconRgbaRead]: the appended
/// icon. **A file whose name ends in `.pic` and that has no icon is given one on the spot**
/// (the file on disc is changed by looking at it), and that icon is then read back.
pub fn file_icon_rgba_read(file: &mut Vec<u8>, name: &str) -> Option<PictureRgba> {
    if icon_offset(file).is_none() && name.ends_with(".pic") {
        let _ = add_icon_to_pic(file, name);
    }
    read_icon_rgba(file)
}

/// The stock icon the browsers show for a file with no icon of its own, by the file name's
/// ending [soft 0x085c4284 SI_getSoftIcon]: a picture file of this name in the program's
/// resource directory.
pub fn soft_icon_resource(name: &str) -> Option<&'static str> {
    // tested in this order, the last match winning
    const TABLE: [(&str, &str); 8] = [
        (".hrc", "hrcIcon"),
        (".scn", "scnIcon"),
        (".dsc", "scnIcon"),
        (".txt", "txtIcon"),
        (".t3D", "t3DIcon"),
        (".spl", "splIcon"),
        (".mtr", "mtrIcon"),
        (".pal", "palIcon"),
    ];
    TABLE.iter().rev().find(|(ext, _)| name.ends_with(ext)).map(|(_, res)| *res)
}

/// The icon the browsers show for a file [soft 0x085c4284 SI_getSoftIcon]:
/// 1. the file's appended icon ([`file_icon_rgba_read`], which may add one to a picture);
/// 2. for a name ending in `.pic`, an icon made from the picture ([`pict_read_as_icon_rgba`]);
/// 3. the stock icon for the name's ending ([`soft_icon_resource`]), read from the resource
///    directory with the packed reader; `resource` returns that file's bytes.
pub fn get_soft_icon(file: Option<&mut Vec<u8>>, name: &str, resource: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<PictureRgba> {
    if let Some(file) = file {
        if let Some(icon) = file_icon_rgba_read(file, name) {
            return Some(icon);
        }
        if name.ends_with(".pic") {
            if let Ok(icon) = pict_read_as_icon_rgba(file) {
                return Some(icon);
            }
        }
    }
    let bytes = resource(soft_icon_resource(name)?)?;
    read_rgba(&bytes).ok()
}

/// The icon of a database file for the element browsers [soft 0x085c467c SI_dbFileIconGet]:
/// the appended icon, else, for names ending in `.pic` or `.icn`, an icon made from the
/// picture. No stock icon. While it runs, files that cannot be opened are not reported.
pub fn db_file_icon_get(file: &mut Vec<u8>, name: &str) -> Option<PictureRgba> {
    if let Some(icon) = file_icon_rgba_read(file, name) {
        return Some(icon);
    }
    if name.ends_with(".pic") || name.ends_with(".icn") {
        return pict_read_as_icon_rgba(file).ok();
    }
    None
}

/// The icon of a chapter [soft 0x085c478c SI_dbChapterIconGet]: the picture file of the
/// given name in the resource directory, read with the packed reader. `bytes` is that file.
pub fn db_chapter_icon_get(bytes: &[u8]) -> Option<PictureRgba> {
    read_rgba(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: i16, h: i16) -> Picture {
        let n = w as usize * h as usize;
        let plane = |k: usize| Some((0..n).map(|i| ((i % w as usize) + (i / w as usize) * k) as u8).collect::<Vec<u8>>());
        Picture { width: w, height: h, ratio: 1.0, fields: 3, pad: 0, planes: [plane(1), plane(2), plane(3), plane(4)] }
    }

    #[test]
    fn sizes() {
        assert_eq!(icon_size(75, 75, false), (75, 75));
        assert_eq!(icon_size(640, 480, false), (75, 56)); // 0.75 × 75 = 56.25
        assert_eq!(icon_size(480, 640, false), (56, 75));
        assert_eq!(icon_size(100, 100, false), (75, 75));
        assert_eq!(icon_size(800, 5, false), (75, 0));
        assert_eq!(icon_size(800, 5, true), (75, 1));
        assert_eq!(icon_size(76, 3, false), (75, 2));
    }

    #[test]
    fn small_picture_is_sampled_not_copied() {
        // 4 × 3: rows 3 × (i / 4) = 0, 0, 1; columns 4 × (x / 5) = 0, 0, 1, 2
        let p = picture(4, 3);
        let icon = pict_read_as_icon(&p.write(None, false)).unwrap();
        assert_eq!((icon.width, icon.height, icon.fields), (4, 3, 3));
        assert_eq!(icon.planes[0].as_ref().unwrap(), &[0, 0, 1, 2, 0, 0, 1, 2, 1, 1, 2, 3]);
        assert_eq!(icon.planes[3].as_ref().unwrap()[8], 4);
    }

    #[test]
    fn packed_icon_is_one_row_high() {
        // bottom-up wanted rows for i = 2, 1, 0: 1, 0, 0; the buffers then hold file rows 0, 1, 1
        let p = picture(4, 3);
        let icon = pict_read_as_icon_rgba(&p.write(None, false)).unwrap();
        let red = |row: usize| icon.data[4 * (row * 4 + 2)];
        assert_eq!((red(2), red(1), red(0)), (1, 2, 2));
        assert_eq!(icon.data[3], 0);
    }

    #[test]
    fn icon_from_memory_skips_row_zero() {
        let p = PictureRgba::from_planes(&picture(4, 3)).unwrap();
        let icon = icon_from_rgba(&p);
        // wanted rows 0, 0, 1, taken rows 1, 1, 2 of the bottom-up record = file rows 1, 1, 0
        let red = |row: usize| icon.data[4 * (row * 4 + 2)];
        assert_eq!((red(0), red(1), red(2)), (2, 2, 1));
        let one = PictureRgba::from_planes(&picture(4, 1)).unwrap();
        assert!(icon_from_rgba(&one).data.iter().all(|&b| b == 0)); // read past the end in the original
    }

    #[test]
    fn appended_icon_packed() {
        let mut file = picture(100, 40).write(None, false);
        assert!(read_icon_rgba(&file).is_none());
        let got = file_icon_rgba_read(&mut file, "x.pic").unwrap(); // adds the icon
        assert_eq!((got.width, got.height), (75, 30));
        let (planes, _) = crate::read_icon(&file).unwrap();
        assert_eq!(got.data[4 * (29 * 75 + 5)], planes.planes[0].as_ref().unwrap()[5]); // bottom-up
        assert_eq!(got.data[4 * (29 * 75 + 5) + 3], planes.planes[3].as_ref().unwrap()[5]);
        // any other name: nothing is added
        let mut other = picture(100, 40).write(None, false);
        assert!(file_icon_rgba_read(&mut other, "x.icn").is_none());
        assert!(db_file_icon_get(&mut other, "x.icn").is_some());
        assert!(db_file_icon_get(&mut other, "x.hrc").is_none());
        // a damaged icon is no icon
        let off = icon_offset(&file).unwrap();
        file[off + 12] = 0x7f; // a list of 128 pixels in a row of 75
        assert!(read_icon_rgba(&file).is_none());
    }

    #[test]
    fn stock_icons() {
        assert_eq!(soft_icon_resource("a.hrc"), Some("hrcIcon"));
        assert_eq!(soft_icon_resource("a.dsc"), Some("scnIcon"));
        assert_eq!(soft_icon_resource("a.t3D"), Some("t3DIcon"));
        assert_eq!(soft_icon_resource("a.pic"), None);
        let stock = picture(3, 2).write(None, false);
        let icon = get_soft_icon(None, "m.mtr", &|res| (res == "mtrIcon").then(|| stock.clone())).unwrap();
        assert_eq!((icon.width, icon.height), (3, 2));
    }
}
