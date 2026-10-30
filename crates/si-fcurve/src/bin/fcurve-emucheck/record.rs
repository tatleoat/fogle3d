// SPDX-License-Identifier: MIT
//! Between a vector's fields and the crate's records: the 100-byte `fcurve`, the 36-byte
//! key, the globals, and the calls into the rest of the program.
use si_fcurve::{Env, Fcurve, Host, Key};
use si_x87::vectors::{Case, Field, Verdict, Word};
use std::cell::RefCell;
use std::rc::Rc;

/// What [`curve_same`] answers when the original left a negative key count.
pub const NEGATIVE_COUNT: &str = "negative key count";

/// Words per key in a `keys` or `k` field: time, value, right slope, slope type,
/// interpolation, curve pointer, tag, padding, left slope, flags, spline index, the word at
/// +0x20, padding.
pub const KEY_WORDS: usize = 13;

/// Self-test, as in `math-emucheck`: with `EMUCHECK_MUTATE` set, every comparison is handed
/// a port result with one bit flipped and must report a difference.
pub fn mutate() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("EMUCHECK_MUTATE").is_some())
}

fn val(w: &Word) -> u128 {
    w.value().unwrap_or(0)
}

fn fl(w: &Word) -> f32 {
    f32::from_bits(val(w) as u32)
}

pub fn key_from(w: &[Word]) -> Key {
    Key {
        time: fl(&w[0]),
        value: fl(&w[1]),
        right_slope: fl(&w[2]),
        slope_type: val(&w[3]) as u16 as i16,
        interp: val(&w[4]) as u16 as i16,
        tagged: val(&w[6]) as u16 as i16,
        left_slope: fl(&w[8]),
        flags: val(&w[9]) as u16,
        spline_index: val(&w[10]) as u16 as i16,
        field_20: val(&w[11]) as u16 as i16,
    }
}

/// The curve a case hands to the routine: the record `curve` and the first `n` keys of `keys`.
pub fn curve_from(c: &Case, curve: &str, keys: &str) -> Fcurve {
    let w = c.input(curve).words();
    let n = val(&w[3]) as u32 as i32;
    let has_array = !matches!(w[2], Word::Value { value: 0, .. });
    let keys = if has_array {
        let kw = c.input(keys).words();
        Some((0..n.max(0) as usize).map(|i| key_from(&kw[KEY_WORDS * i..KEY_WORDS * (i + 1)])).collect())
    } else {
        None
    };
    let mut name = [0u8; 34];
    for (i, b) in name.iter_mut().enumerate() {
        *b = val(&w[15 + i]) as u8;
    }
    Fcurve {
        kind: val(&w[0]) as u32 as i32,
        owner: val(&w[1]) as u32,
        keys,
        last_time: fl(&w[4]),
        last_value: fl(&w[5]),
        pre_extrap: val(&w[6]) as u16 as i16,
        post_extrap: val(&w[7]) as u16 as i16,
        flags: val(&w[8]) as u16 as i16,
        interp: val(&w[9]) as u16 as i16,
        active: val(&w[10]) as u16 as i16,
        range_start: fl(&w[12]),
        range_end: fl(&w[13]),
        active_copy: val(&w[14]) as u16 as i16,
        name,
        cache: val(&w[49]) as u32 as i32,
        aux: None,
        min_value: fl(&w[51]),
        max_value: fl(&w[52]),
        field_60: val(&w[53]) as u16 as i16,
        dirty: val(&w[54]) as u16 as i16,
    }
}

/// What the port's hosts were asked, in order, in the notation of a vector's recorded calls.
#[derive(Default)]
pub struct Log {
    pub calls: Vec<String>,
    pub printed: String,
    /// What `SI_dialogRequest2` answered in the vector (the stub's value).
    pub dialog_answer: i16,
}

struct Recorder(Rc<RefCell<Log>>);

impl Host for Recorder {
    fn fast_set_dirty(&mut self) {
        self.0.borrow_mut().calls.push("SI_fastSetDirty".into());
    }
    fn status_message(&mut self, text: &str, kind: i16) {
        self.0.borrow_mut().calls.push(format!("SI_statusDisplayMessage({text:?},{kind})"));
    }
    fn replace_locked_key(&mut self) -> i16 {
        let mut log = self.0.borrow_mut();
        log.calls.push("SI_dialogRequest2".into());
        log.dialog_answer
    }
    fn printf(&mut self, text: &str) {
        self.0.borrow_mut().printed.push_str(text);
    }
    fn scngl_delete(&mut self, _curve: &Fcurve) {
        self.0.borrow_mut().calls.push("si_fcvScnglDelete".into());
    }
    fn save_history(&mut self, _curve: &Fcurve) {
        self.0.borrow_mut().calls.push("SI_fcurveSaveHistory".into());
    }
}

fn global_i32(c: &Case, name: &str) -> i32 {
    c.input(name).values()[0] as u32 as i32
}

/// The globals of a case as the port's environment, with a host that records.
pub fn env_from(c: &Case) -> (Env, Rc<RefCell<Log>>) {
    let log = Rc::new(RefCell::new(Log { dialog_answer: 1, ..Default::default() }));
    let mut env = Env::new();
    env.frame_rate = c.input("rate").f32();
    env.frame_current = global_i32(c, "frame");
    env.key_tolerance = c.input("tol").f32();
    env.default_interp = global_i32(c, "dinterp") as i16;
    env.key_type_propag_mode = global_i32(c, "propag") as i16;
    env.use_hidden_for_matte = global_i32(c, "hidden") as i16;
    env.cus_val_num_callback = global_i32(c, "ncb");
    env.fcurve_dirty = global_i32(c, "dirty");
    env.cus_fcurve_dirty = global_i32(c, "cusdirty");
    if let Some(f) = c.try_input("standalone") {
        env.standalone_renderer = f.values()[0] as i32;
    }
    // what the stubbed dialog answered in the vector is what the port's host answers
    if let Some(call) = c.calls.iter().find(|call| call.name == "SI_dialogRequest2") {
        log.borrow_mut().dialog_answer = call.result.value().unwrap_or(0) as u16 as i16;
    }
    env.host = Box::new(Recorder(log.clone()));
    (env, log)
}

/// The curve of a case together with its snapshot, when the record's pointer at +0x54 names
/// the buffer `aux`.
pub fn curve_with_aux(c: &Case) -> Fcurve {
    let mut cv = curve_from(c, "curve", "keys");
    if matches!(c.input("curve").words()[50], Word::Ref { .. }) {
        cv.aux = Some(Box::new(curve_from(c, "aux", "auxkeys")));
    }
    cv
}

/// The snapshot the routine left (the record its pointer at +0x54 leads to, `auxrec`, and
/// that record's keys, `auxk`) against the port's.
pub fn aux_same(c: &Case, f: &Fcurve) -> Result<(), String> {
    let has = !matches!(c.after("curve").words()[50], Word::Value { value: 0, .. });
    same("has a snapshot", f.aux.is_some() != mutate(), has)?;
    match &f.aux {
        Some(aux) => curve_same(c, "auxrec", "auxk", aux),
        None => Ok(()),
    }
}

/// A key on its own, as the routine left it in the field `name`, against the port's. Words
/// the routine did not write are not compared.
pub fn lone_key_same(c: &Case, name: &str, k: &Key) -> Result<(), String> {
    let w = c.after(name).words();
    let m = mutate() as u32;
    let port: [(usize, u32, &str); 10] = [
        (0, k.time.to_bits() ^ m, "time"),
        (1, k.value.to_bits(), "value"),
        (2, k.right_slope.to_bits(), "right slope"),
        (3, k.slope_type as u16 as u32, "slope type"),
        (4, k.interp as u16 as u32, "interpolation"),
        (6, k.tagged as u16 as u32, "tag"),
        (8, k.left_slope.to_bits(), "left slope"),
        (9, k.flags as u32, "flags"),
        (10, k.spline_index as u16 as u32 ^ m, "spline index"),
        (11, k.field_20 as u16 as u32, "word +0x20"),
    ];
    for (i, v, what) in port {
        if let Some(o) = w[i].value() {
            same(what, v, o as u32)?;
        }
    }
    Ok(())
}

/// The names of the blocks the routine freed, in order.
pub fn freed(c: &Case) -> Vec<String> {
    c.try_output("freed")
        .map(|f| f.words().iter().map(|w| if let Word::Ref { name, .. } = w { name.clone() } else { "?".into() }).collect())
        .unwrap_or_default()
}

fn same<T: PartialEq + std::fmt::Debug>(what: &str, port: T, original: T) -> Result<(), String> {
    if port == original {
        Ok(())
    } else {
        Err(format!("{what}: port {port:x?}, original {original:x?}"))
    }
}

/// One key of an array against the thirteen words of its record.
pub fn key_words_same(i: usize, k: &Key, w: &[Word]) -> Result<(), String> {
    key_same(i, k, w)
}

/// `port` against a word of the original; a word that has no defined value (the routine
/// did not write it, or it depends on memory nobody wrote) is not compared.
fn word_same(what: &str, port: u32, original: &Word) -> Result<(), String> {
    match original.value() {
        Some(o) if o as u32 != port => Err(format!("{what}: port {port:x}, original {:x}", o as u32)),
        _ => Ok(()),
    }
}

fn key_same(i: usize, k: &Key, w: &[Word]) -> Result<(), String> {
    let m = mutate() as u32;
    word_same(&format!("key {i} time"), k.time.to_bits() ^ m, &w[0])?;
    word_same(&format!("key {i} value"), k.value.to_bits() ^ m, &w[1])?;
    word_same(&format!("key {i} right slope"), k.right_slope.to_bits(), &w[2])?;
    word_same(&format!("key {i} slope type"), k.slope_type as u16 as u32, &w[3])?;
    word_same(&format!("key {i} interpolation"), k.interp as u16 as u32, &w[4])?;
    word_same(&format!("key {i} tag"), k.tagged as u16 as u32, &w[6])?;
    word_same(&format!("key {i} left slope"), k.left_slope.to_bits(), &w[8])?;
    word_same(&format!("key {i} flags"), k.flags as u32, &w[9])?;
    word_same(&format!("key {i} spline index"), k.spline_index as u16 as u32, &w[10])?;
    word_same(&format!("key {i} word +0x20"), k.field_20 as u16 as u32, &w[11])
}

/// The record and key array the routine left (`curve`, and `k`: the array the record then
/// describes) against the port's curve. Not compared: the three pointers (owner is compared
/// as a number), the padding words, each key's pointer to its curve, and any word that has
/// no defined value in the original.
pub fn curve_same(c: &Case, name: &str, karray: &str, f: &Fcurve) -> Result<(), String> {
    let w = c.after(name).words();
    let m = mutate() as u32;
    if (val(&w[3]) as u32 as i32) < 0 {
        return Err(NEGATIVE_COUNT.to_string());
    }
    word_same("type", f.kind as u32, &w[0])?;
    word_same("owner", f.owner, &w[1])?;
    word_same("key count", f.nkeys() as u32 ^ m, &w[3])?;
    word_same("last time", f.last_time.to_bits(), &w[4])?;
    word_same("last value", f.last_value.to_bits() ^ m, &w[5])?;
    word_same("extrapolation before", f.pre_extrap as u16 as u32, &w[6])?;
    word_same("extrapolation after", f.post_extrap as u16 as u32, &w[7])?;
    word_same("flags", f.flags as u16 as u32, &w[8])?;
    word_same("interpolation", f.interp as u16 as u32, &w[9])?;
    word_same("active", f.active as u16 as u32, &w[10])?;
    word_same("range start", f.range_start.to_bits(), &w[12])?;
    word_same("range end", f.range_end.to_bits(), &w[13])?;
    word_same("word +0x2c", f.active_copy as u16 as u32, &w[14])?;
    for (i, b) in f.name.iter().enumerate() {
        word_same("name", *b as u32, &w[15 + i])?;
    }
    word_same("search index", f.cache as u32 ^ m, &w[49])?;
    word_same("least value", f.min_value.to_bits(), &w[51])?;
    word_same("greatest value", f.max_value.to_bits(), &w[52])?;
    word_same("word +0x60", f.field_60 as u16 as u32, &w[53])?;
    word_same("dirty mark", f.dirty as u16 as u32, &w[54])?;
    let n = f.nkeys().max(0) as usize;
    if n > 0 {
        let k: &Field = c.try_output(karray).ok_or_else(|| format!("the original's record has no key array, the port {n} keys"))?;
        let kw = k.words();
        if kw.len() < KEY_WORDS * n {
            return Err(format!("the original has {} keys, the port {n}", kw.len() / KEY_WORDS));
        }
        for (i, key) in f.k().iter().enumerate() {
            key_same(i, key, &kw[KEY_WORDS * i..KEY_WORDS * (i + 1)])?;
        }
    }
    Ok(())
}

/// The globals the routine may count in against the port's environment.
pub fn env_same(c: &Case, env: &Env) -> Result<(), String> {
    let g = |name: &str| c.after(name).written()[0].unwrap_or(0) as u32 as i32;
    same("SI_fcurveDirty", env.fcurve_dirty, g("dirty"))?;
    same("SI_cusFcurveDirty", env.cus_fcurve_dirty, g("cusdirty"))?;
    same("SI_frameCurrent", env.frame_current, g("frame"))?;
    same("SI_frameRate", env.frame_rate.to_bits(), c.after("rate").f32().to_bits())
}

/// The calls the routine made into the rest of the program, and what it printed, against
/// what the port asked of its host.
pub fn host_same(c: &Case, log: &Log) -> Result<(), String> {
    let mut original = Vec::new();
    for call in &c.calls {
        if call.result.bytes() == 10 {
            continue;
        }
        if call.name == "SI_statusDisplayMessage" {
            let text = match &call.args[0] {
                Word::Ref { name, .. } => String::from_utf8_lossy(&c.output(name).u8s()).into_owned(),
                _ => String::new(),
            };
            original.push(format!("SI_statusDisplayMessage({text:?},{})", val(&call.args[1]) as u16 as i16));
        } else {
            original.push(call.name.clone());
        }
    }
    same("calls into the program", &log.calls, &original)?;
    let printed: Vec<u8> = c.try_output("stdout").map(|f| f.u8s()).unwrap_or_default();
    same("printed", log.printed.as_bytes(), &printed[..])
}

pub fn verdict(r: Result<(), String>) -> Verdict {
    match r {
        Ok(()) => Verdict::Same,
        Err(e) => Verdict::Different(e),
    }
}
