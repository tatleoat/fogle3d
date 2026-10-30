// SPDX-License-Identifier: MIT
//! The flock setup file (`.flk`, tag `SFLK`): reader and writer (finding 0006).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source file: `particle_dial.c`.
//!
//! The file holds the parameters of the Flock ("particle") effect, one keyword per line
//! between `Particle` and `End`. Like the channel setup reader this one reads the stream with
//! C's `fscanf` after the ASCII header ([`crate::chn::scan`]).

use crate::chn::scan::{fmt_f, fmt_s, Arg, CFile};
use crate::token::read_asc_header;

/// The version the writer puts in the ASCII header: a constant of its own (0x086612fc), equal
/// to `si_file::VERSION_OUTPUT`.
pub const HEADER_VERSION: f32 = 4.0;

/// The flock parameters: the fields of the program's one `SI_ParticleStatus` record
/// (`si_particleStatus`, 0x08822040) that the file holds, each named after its keyword.
/// What the parameters do is not read from code; the names are the file's.
#[derive(Clone, Debug, PartialEq)]
pub struct Flock {
    /// +0x00: incremented by each read of a file; zero or less means "still the defaults".
    pub loaded: i32,
    /// +0x04 `Name`
    pub name: Option<Vec<u8>>,
    /// +0x08 `Gravity`
    pub gravity: f32,
    /// +0x0c `Up`
    pub up: [f32; 3],
    /// +0x18 `SourceSeed`
    pub source_seed: i32,
    /// +0x1c `DestSeed`
    pub dest_seed: i32,
    /// +0x20 `SeedOffset`
    pub seed_offset: i32,
    /// +0x24 `Number`
    pub number: i32,
    /// +0x28 `SourceVert`
    pub source_vert: i32,
    /// +0x2c `DestVert`
    pub dest_vert: i32,
    /// +0x30 `Shapes`
    pub shapes: i32,
    /// +0x34 `CreateMotionPaths`
    pub create_motion_paths: i32,
    /// +0x38 `Start`
    pub start: i32,
    /// +0x3c `Stop`
    pub stop: i32,
    /// +0x40 `Repetition`
    pub repetition: i32,
    /// +0x44 `Offset`
    pub offset: i32,
    /// +0x48 `Extrapolation`
    pub extrapolation: i32,
    /// +0x4c `Hide`
    pub hide: i32,
    /// +0x50 `PreHide`
    pub pre_hide: i32,
    /// +0x54 `TrackDest`
    pub track_dest: i32,
    /// +0x58 `Sample`
    pub sample: i32,
    /// +0x5c `Fixed`
    pub fixed: i32,
    /// +0x60 `MaterialInterp`
    pub material_interp: i32,
    /// +0x68 … +0x74: the integer of `JitterScale`, `JitterRotate`, `JitterTrans`,
    /// `JitterShape`, in that order.
    pub jitter_on: [i32; 4],
    /// +0x78 … +0xa4: the three numbers of each.
    pub jitter: [[f32; 3]; 4],
    /// +0xac `BackTrackMaterial`
    pub back_track_material: i32,
    /// +0xb0 `BackTrackDest`
    pub back_track_dest: i32,
}

impl Default for Flock {
    fn default() -> Self {
        Self::new()
    }
}

impl Flock {
    /// The parameters the program starts with `SI_initParticle` (0x084c0450; not counted as ported: only the
    /// fields kept here).
    pub fn new() -> Flock {
        Flock {
            loaded: 0,
            name: Some(b"flock".to_vec()),
            gravity: 0.0,
            up: [0.0, 1.0, 0.0],
            source_seed: 17,
            dest_seed: 17,
            seed_offset: 0,
            number: 10,
            source_vert: 0,
            dest_vert: 0,
            shapes: 2,
            create_motion_paths: 0,
            start: 1,
            stop: 30,
            repetition: 1,
            offset: 0,
            extrapolation: 1,
            hide: 0,
            pre_hide: 0,
            track_dest: 0,
            sample: 0,
            fixed: 0,
            material_interp: 0,
            jitter_on: [0; 4],
            jitter: [[0.05; 3]; 4],
            back_track_material: 1,
            back_track_dest: 0,
        }
    }
}

/// What a read reports besides the parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadInfo {
    /// The ASCII header's version; −1.0 without an accepted header (the reader then starts
    /// over from the first byte). The reader does not use it.
    pub version: f32,
    pub header_tag: Option<u32>,
    /// The word `Particle` was found.
    pub particle: bool,
    /// The reader stopped at the word `End` (otherwise at the end of the file).
    pub end_word: bool,
    /// The lines the original prints to `stderr`, one per word it does not know.
    pub messages: Vec<Vec<u8>>,
    /// Nothing but white space is left after the last byte the reader took.
    pub all_tokens_consumed: bool,
    /// The file ended before `End`: the reader wanted a word or a value and there was none.
    pub read_past_end: bool,
    /// Words longer than the buffer the original reads them into (80 bytes for a keyword, 40
    /// for the name). The strings are kept whole here.
    pub overruns: usize,
    /// File offset after the last byte the reader took.
    pub end: usize,
}

/// The reader has no failing case: whatever the file holds, the original returns.
pub type FlkError = std::convert::Infallible;

/// Where an integer keyword's value goes.
type IntField = fn(&mut Flock) -> &mut i32;

/// The integer keywords in the order the reader tests them (and the writer writes them).
/// `SourceSeed`, `DestSeed`, `Gravity` and `Up` come between the fourth and the fifth, `Name`
/// before the first, the four `Jitter…` after the last.
const INT_KEYWORDS: [(&str, IntField); 19] = [
    ("Number", |p| &mut p.number),
    ("Shapes", |p| &mut p.shapes),
    ("SourceVert", |p| &mut p.source_vert),
    ("DestVert", |p| &mut p.dest_vert),
    ("Start", |p| &mut p.start),
    ("Stop", |p| &mut p.stop),
    ("Repetition", |p| &mut p.repetition),
    ("Offset", |p| &mut p.offset),
    ("SeedOffset", |p| &mut p.seed_offset),
    ("Hide", |p| &mut p.hide),
    ("PreHide", |p| &mut p.pre_hide),
    ("Extrapolation", |p| &mut p.extrapolation),
    ("Sample", |p| &mut p.sample),
    ("Fixed", |p| &mut p.fixed),
    ("TrackDest", |p| &mut p.track_dest),
    ("BackTrackDest", |p| &mut p.back_track_dest),
    ("MaterialInterp", |p| &mut p.material_interp),
    ("BackTrackMaterial", |p| &mut p.back_track_material),
    ("CreateMotionPaths", |p| &mut p.create_motion_paths),
];
const JITTER_KEYWORDS: [&str; 4] = ["JitterScale", "JitterRotate", "JitterTrans", "JitterShape"];

/// Reads a flock setup file into the program's defaults, as loading one into a freshly
/// started program does [soft 0x084c03d0 SI_particleSetupGet].
pub fn read(data: &[u8]) -> Result<(Flock, ReadInfo), FlkError> {
    let mut p = Flock::new();
    let info = read_into(&mut p, data);
    p.loaded += 1;
    Ok((p, info))
}

/// Reads the parameters [soft 0x084bf860 particleReadParameters].
///
/// After the ASCII header (whose result is ignored) words are skipped up to `Particle`. Then
/// each word is a keyword followed by its values, up to the word `End` or the end of the
/// file. The keywords are tested in one fixed order, each test followed by the read of the
/// next word, so a file in the writer's order is read in one pass; any other order takes
/// more passes and gives the same result. A word that is no keyword is reported ("Invalid
/// keyword in particle file: …") and skipped. A value that does not scan leaves the
/// parameter as it was, and what could not be scanned is then taken as the next word.
///
/// `Name` takes one word; a name with a space in it, which the writer writes whole, comes
/// back cut at the space, and its rest is reported as an invalid keyword.
pub fn read_into(p: &mut Flock, data: &[u8]) -> ReadInfo {
    let mut f = CFile::new(data);
    let header = read_asc_header(&mut f.f);
    let mut info = ReadInfo {
        version: header.map_or(-1.0, |h| h.1),
        header_tag: header.map(|h| h.0),
        particle: false,
        end_word: false,
        messages: Vec::new(),
        all_tokens_consumed: false,
        read_past_end: false,
        overruns: 0,
        end: 0,
    };
    parameters(&mut f, p, &mut info);
    info.read_past_end = !info.end_word;
    info.all_tokens_consumed = f.f.only_blanks_left();
    info.end = f.tell();
    info
}

/// The next word, with `%s` into the original's 80-byte buffer. False at the end of the file.
fn next_word(f: &mut CFile, word: &mut Vec<u8>, info: &mut ReadInfo) -> bool {
    let ok = f.scanf("%s", &mut [Arg::Str(word)]) == 1;
    if ok && word.len() >= 80 {
        info.overruns += 1;
    }
    ok
}

/// The body of [soft 0x084bf860 particleReadParameters] after the header.
fn parameters(f: &mut CFile, p: &mut Flock, info: &mut ReadInfo) {
    let mut word = Vec::new();
    loop {
        if !next_word(f, &mut word, info) {
            break;
        }
        if word == b"Particle" {
            info.particle = true;
            break;
        }
    }
    if !next_word(f, &mut word, info) {
        return;
    }
    // The original scans a name into 40 bytes of stack and the two seeds into one temporary,
    // and copies them to the record whether or not the scan stored anything. After a failed
    // scan the record therefore gets what the stack held: the value of the previous `Name` or
    // seed statement of this read, or garbage if there was none. Here "none" is an empty name
    // and a seed of 0.
    let mut name: Vec<u8> = Vec::new();
    let mut seed: i32 = 0;
    // `unknown`: the last pass did not end on `JitterShape`. `invalid`: and this pass has
    // matched nothing so far.
    let mut unknown = false;
    let mut invalid = false;
    loop {
        if invalid {
            let mut line = b"Invalid keyword in particle file: ".to_vec();
            line.extend_from_slice(&word);
            line.push(b'\n');
            info.messages.push(line);
            if !next_word(f, &mut word, info) {
                return;
            }
        }
        if unknown {
            invalid = true;
        }
        // every test below that matches reads its values and then the next word, which the
        // tests after it see
        macro_rules! matched {
            () => {
                invalid = false;
                if !next_word(f, &mut word, info) {
                    return;
                }
            };
        }
        if word == b"Name" {
            f.scanf("%s", &mut [Arg::Str(&mut name)]);
            if name.len() >= 40 {
                info.overruns += 1;
            }
            p.name = Some(name.clone());
            matched!();
        }
        // the integer keywords up to DestVert, then the seeds, Gravity and Up, then the rest
        for (i, (keyword, field)) in INT_KEYWORDS.iter().enumerate() {
            if i == 4 {
                if word == b"SourceSeed" {
                    f.scanf("%d", &mut [Arg::Int(&mut seed)]);
                    p.source_seed = seed;
                    matched!();
                }
                if word == b"DestSeed" {
                    f.scanf("%d", &mut [Arg::Int(&mut seed)]);
                    p.dest_seed = seed;
                    matched!();
                }
                if word == b"Gravity" {
                    f.scanf("%f", &mut [Arg::Float(&mut p.gravity)]);
                    matched!();
                }
                if word == b"Up" {
                    let [mut x, mut y, mut z] = p.up;
                    f.scanf("%f %f %f", &mut [Arg::Float(&mut x), Arg::Float(&mut y), Arg::Float(&mut z)]);
                    p.up = [x, y, z];
                    matched!();
                }
            }
            if word == keyword.as_bytes() {
                f.scanf("%d", &mut [Arg::Int(field(p))]);
                matched!();
            }
        }
        for (i, keyword) in JITTER_KEYWORDS.iter().enumerate() {
            if word == keyword.as_bytes() {
                let [mut x, mut y, mut z] = p.jitter[i];
                f.scanf("%d %f %f %f", &mut [Arg::Int(&mut p.jitter_on[i]), Arg::Float(&mut x), Arg::Float(&mut y), Arg::Float(&mut z)]);
                p.jitter[i] = [x, y, z];
                if i == 3 {
                    unknown = false;
                }
                matched!();
            } else if i == 3 {
                unknown = true;
            }
        }
        if word == b"End" {
            info.end_word = true;
            return;
        }
    }
}

/// Writes the parameters [soft 0x084bf540 particleWriteParameters]: the header, `Particle`,
/// one line per keyword with integers as `%d` and numbers as `%f`, `End`. The original's
/// header version is [`HEADER_VERSION`]. A null name prints as `(null)`, as glibc does.
pub fn write(p: &Flock, header_version: f32) -> Vec<u8> {
    let mut out = Vec::new();
    si_file::write_asc_header(&mut out, header_version, u32::from_be_bytes(*b"SFLK"));
    let num = |v: f32| fmt_f(v as f64, 0, 6);
    out.extend_from_slice(b"Particle\n  Name ");
    out.extend_from_slice(fmt_s(&p.name));
    out.push(b'\n');
    let mut line = |s: String| out.extend_from_slice(s.as_bytes());
    line(format!("  Number {}\n", p.number));
    line(format!("  Shapes {}\n", p.shapes));
    line(format!("  SourceVert {}\n", p.source_vert));
    line(format!("  DestVert {}\n", p.dest_vert));
    line(format!("  SourceSeed {}\n", p.source_seed));
    line(format!("  DestSeed {}\n", p.dest_seed));
    line(format!("  Gravity {}\n", num(p.gravity)));
    line(format!("  Up {} {} {}\n", num(p.up[0]), num(p.up[1]), num(p.up[2])));
    line(format!("  Start {}\n", p.start));
    line(format!("  Stop {}\n", p.stop));
    line(format!("  Repetition {}\n", p.repetition));
    line(format!("  Offset {}\n", p.offset));
    line(format!("  SeedOffset {}\n", p.seed_offset));
    line(format!("  Hide {}\n", p.hide));
    line(format!("  PreHide {}\n", p.pre_hide));
    line(format!("  Extrapolation {}\n", p.extrapolation));
    line(format!("  Sample {}\n", p.sample));
    line(format!("  Fixed {}\n", p.fixed));
    line(format!("  TrackDest {}\n", p.track_dest));
    line(format!("  BackTrackDest {}\n", p.back_track_dest));
    line(format!("  MaterialInterp {}\n", p.material_interp));
    line(format!("  BackTrackMaterial {}\n", p.back_track_material));
    line(format!("  CreateMotionPaths {}\n", p.create_motion_paths));
    for (i, keyword) in JITTER_KEYWORDS.iter().enumerate() {
        let [x, y, z] = p.jitter[i];
        line(format!("  {keyword} {}   {}  {}  {}\n", p.jitter_on[i], num(x), num(y), num(z)));
    }
    line("End\n".into());
    out
}

/// What the corpus check finds for one file.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub info: ReadInfo,
    /// Written and read again, the parameters are the same.
    pub same_after_rewrite: bool,
    /// Written with the file's header version, the bytes are the file's.
    pub identical: bool,
}

/// Reads a file, writes it and reads it again: the corpus check for one file.
pub fn check(data: &[u8]) -> Check {
    let Ok((p, info)) = read(data);
    let out = write(&p, info.version);
    let Ok((p2, _)) = read(&out);
    Check { same_after_rewrite: p2 == p, identical: out == data, info }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made-up parameters, in the writer's layout.
    fn sample() -> Flock {
        Flock {
            loaded: 1,
            name: Some(b"swarm".to_vec()),
            gravity: -9.5,
            up: [0.0, 0.0, 1.0],
            source_seed: 3,
            dest_seed: 4,
            seed_offset: 5,
            number: 123,
            source_vert: 1,
            dest_vert: 2,
            shapes: 6,
            create_motion_paths: 1,
            start: -10,
            stop: 99,
            repetition: 7,
            offset: 8,
            extrapolation: 2,
            hide: 9,
            pre_hide: 10,
            track_dest: 1,
            sample: 11,
            fixed: 12,
            material_interp: 13,
            jitter_on: [1, 0, 1, 0],
            jitter: [[0.1, 0.2, 0.3], [1.0, 2.0, 3.0], [0.125, 0.25, 0.5], [10.0, 20.0, 30.5]],
            back_track_material: 0,
            back_track_dest: 14,
        }
    }

    #[test]
    fn defaults() {
        let p = Flock::new();
        assert_eq!((p.name.as_deref(), p.number, p.shapes, p.stop, p.source_seed, p.up), (Some(&b"flock"[..]), 10, 2, 30, 17, [0.0, 1.0, 0.0]));
        assert_eq!((p.jitter[3][2], p.extrapolation, p.back_track_material, p.loaded), (0.05, 1, 1, 0));
    }

    #[test]
    fn write_then_read() {
        let p = sample();
        let out = write(&p, 3.71);
        let text = String::from_utf8(out.clone()).unwrap();
        assert!(text.starts_with("SFLK3.71\nParticle\n  Name swarm\n  Number 123\n  Shapes 6\n  SourceVert 1\n  DestVert 2\n  SourceSeed 3\n  DestSeed 4\n  Gravity -9.500000\n  Up 0.000000 0.000000 1.000000\n  Start -10\n"));
        assert!(text.ends_with("  CreateMotionPaths 1\n  JitterScale 1   0.100000  0.200000  0.300000\n  JitterRotate 0   1.000000  2.000000  3.000000\n  JitterTrans 1   0.125000  0.250000  0.500000\n  JitterShape 0   10.000000  20.000000  30.500000\nEnd\n"));
        assert_eq!(text.lines().count(), 31);
        let (q, info) = read(&out).unwrap();
        assert_eq!(q, p);
        assert_eq!((info.version, info.particle, info.end_word, info.read_past_end), (3.71, true, true, false));
        // the newline after End is looked at and put back
        assert!(info.all_tokens_consumed && info.messages.is_empty() && info.overruns == 0 && info.end == out.len() - 1);
        let c = check(&out);
        assert!(c.identical && c.same_after_rewrite);
        // the original's own header version
        assert!(write(&p, HEADER_VERSION).starts_with(b"SFLK4\nParticle\n"));
        let mut p = p;
        p.name = None;
        assert!(String::from_utf8(write(&p, 4.0)).unwrap().contains("  Name (null)\n"));
    }

    #[test]
    fn any_order_and_unknown_words() {
        // reverse order: one pass per keyword, same result
        let (p, info) = read(b"SFLK3.71\nintro words Particle\nJitterShape 1 1 2 3\nStop 5\nGravity 2.5\nName a\nEnd\ntrailing\n").unwrap();
        assert_eq!((p.jitter_on[3], p.jitter[3], p.stop, p.gravity, p.name.as_deref()), (1, [1.0, 2.0, 3.0], 5, 2.5, Some(&b"a"[..])));
        assert_eq!((info.end_word, info.read_past_end, info.all_tokens_consumed, info.messages.len(), p.loaded), (true, false, false, 0, 1));
        // unknown words are reported one by one and skipped; values that do not scan leave the
        // parameter alone and are then words themselves
        let (p, info) = read(b"Particle\nBogus 3\nStop x\nNumber 7\nEnd\n").unwrap();
        assert_eq!((p.stop, p.number, info.version, info.header_tag), (30, 7, -1.0, None));
        assert_eq!(info.messages, vec![b"Invalid keyword in particle file: Bogus\n".to_vec(), b"Invalid keyword in particle file: 3\n".to_vec(), b"Invalid keyword in particle file: x\n".to_vec()]);
        // a name takes one word
        let (p, info) = read(b"Particle\n  Name my flock\n  Number 7\nEnd\n").unwrap();
        assert_eq!((p.name.as_deref(), p.number), (Some(&b"my"[..]), 7));
        assert_eq!(info.messages, vec![b"Invalid keyword in particle file: flock\n".to_vec()]);
        // a seed that does not scan takes the last seed scanned
        let (p, _) = read(b"Particle\nSourceSeed 9\nDestSeed x\nEnd\n").unwrap();
        assert_eq!((p.source_seed, p.dest_seed), (9, 9));
        // partial values
        let (p, _) = read(b"Particle\nUp 1 2 Stop 4\nJitterTrans 1 7 End\n").unwrap();
        assert_eq!((p.up, p.stop, p.jitter_on[2], p.jitter[2]), ([1.0, 2.0, 0.0], 4, 1, [7.0, 0.05, 0.05]));
    }

    #[test]
    fn truncated_files() {
        let (p, info) = read(b"").unwrap();
        assert_eq!(p, Flock { loaded: 1, ..Flock::new() });
        assert!(!info.particle && !info.end_word && info.read_past_end && info.all_tokens_consumed);
        let (p, info) = read(b"SFLK3.71\nParticle\n  Name a\n  Number").unwrap();
        assert_eq!((p.name.as_deref(), p.number), (Some(&b"a"[..]), 10));
        assert!(info.particle && !info.end_word && info.read_past_end);
        let (p, info) = read(b"SFLK3.71\nno such word\n").unwrap();
        assert_eq!(p.number, 10);
        assert!(!info.particle && info.read_past_end);
        // the last word of a file with no newline after it is still read
        let (_, info) = read(b"Particle\nStop 3\nEnd").unwrap();
        assert!(info.end_word && !info.read_past_end && info.all_tokens_consumed);
    }
}
