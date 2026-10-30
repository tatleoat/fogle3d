# Run-time data: the original's tables and messages, read from the user's install

Unit rtdata1, 2026-10-30. Release model (owner, 2026-10-29): the user supplies their own Softimage|3D 4.0 install, as a
recompiled game asks for its ROM, and nothing of the original ships with Fogle3D. Some ported routines need data of
the original byte for byte: parser tables, numeric tables, the texts of messages they hand on or print. Until now
those were literals in our Rust. The release audit (`re/tools/release/release.py audit`) counts them as hits:
category 3 (16 or more of the original's messages in one source file), category 4 (literal tables found in the
original's data; hex blobs), category 6 (messages and data windows inside a built program of the add-on bundle).

This note describes the mechanism that replaces such literals (crate `si-rt`), the first two files converted with
it, and, file by file, how to convert every remaining hit. It names files, lines, addresses, lengths and our own
checksums only; it quotes nothing of the original.

## 1. The mechanism: `crates/si-rt`

A crate no longer holds the bytes. It names them by where the Linux `soft` program has them (a virtual address and
a length, the same addresses every `[soft 0x...]` citation uses) and `si-rt` reads them out of the user's copy of
that program the first time they are wanted.

```rust
// a table: one static per range, with our checksum of it
pub static YACC: si_rt::Table = si_rt::Table::new("si_custom.shdb_yacc", 0x0866_3940, 0xc00, 0xaf0f_6d05_f9bd_2b64);
let bytes: &'static [u8] = YACC.get();          // checked once, then one atomic load
let s: &'static [u8] = si_rt::message(addr);    // the C string at an address (no zero)
let s: &'static [u8] = si_rt::message_at(addr); // the C string a `char *` at addr points to
let s: &'static str = si_rt::text(addr);        // the same as &str, for our own formatting and printing
let w: u32 = si_rt::word(addr);                 // a 32-bit word
```

* **Lookup order.** The directories a program gave with `si_rt::add_root(dir)` (its install argument), then the
  directory the environment variable `SI_INSTALL` names (the same variable `si-install`, the add-on and the release
  audit use), then, for development only, `re/bin/si3d40-ia/installed` above the working directory or the program.
  In each it tries `Creative-Tools/3D/bin/soft`, `linux/soft3d/Creative-Tools/3D/bin/soft` and
  `soft3d/Creative-Tools/3D/bin/soft`, then searches up to ten levels for a file named `soft` of the expected
  length (no symbolic links followed; a path holding `/linux/` first). Only names and sizes are looked at during the
  search; the one file read is the program. No other file of the install is opened.
* **Build check.** The file must be 8,472,833 bytes with FNV-1a-64 sum `0xd913_24dd_bca9_e849` (our numbers for the
  Linux build every address refers to: `si_rt::LINUX_SOFT`). A `Table` also checks its own range against its own
  checksum, so a wrong address in a crate is caught at the first use, not as a wrong picture.
* **Cost.** The program (8.1 MB) is read once per process and kept; a table or message is a slice of it
  (`&'static [u8]`, no copy). Loading and checking take 11 ms (release build). A crate decodes a table once (a
  `static Table`, a `LazyLock<[T; N]>`), so hot loops pay nothing.
* **Thread safety.** `si_rt::program()` loads under a `OnceLock`; every later call returns the same result.
* **Errors.** `si_rt::Error` (`NotFound`, `WrongBuild`, `Unreadable`, `Range`, `Sum`, `AlreadyLoaded`); its
  `Display` is the text for the user. The plain accessors (`get`, `message`, `text`, `word`, `table`) panic with
  it and name the slot; the `try_` forms return it. Without an install the text is (our own wording):

  > Softimage|3D 4.0 install not found: no program file `soft` of the Linux build (8472833 bytes) in
  >   - (the directories looked in)
  > Fogle3D reads some of the original program's own data from your Softimage|3D 4.0 install when it runs (none of
  > it ships with Fogle3D): set the environment variable SI_INSTALL to the directory the product was installed to
  > (the one holding Creative-Tools/ and Creative-Rsrc/), or, in Blender, the add-on preference "Softimage 3D 4.0
  > install path"

  A file of another build (the IRIX `soft` is 13,513,284 bytes) gives "... is not the Softimage|3D 4.0 Linux build
  Fogle3D was made for (13513284 bytes, expected 8472833); ..." with the same advice.
* **Programs.** A program that needs the data calls, at its start, `si_rt::add_root(<its install root>)` and
  `si_rt::check_or_exit("<name>")`: on failure it prints `<name>: <text>` and exits with status 2, before any work.
* **Tests.** A test that needs the data begins with `si_rt::skip_without_install!();`: without an install (CI) it
  prints why and passes as skipped; with `SI_RT_REQUIRE` set to anything it fails instead (for a machine that must
  have the install). Unit tests on made-up data use `si_rt::Program::unchecked` and `si_rt::elf::Image`.
* **Tool.** `si-rt check [dir]` (where the program is, its build, its segments), `si-rt sum <addr> <len> [dir]`
  (our checksum of a range, as the Rust literal a `Table` takes), `si-rt sums <file> [dir]` (one per
  `<name> <addr> <len>` line). It never prints the bytes.
* **Limits.** Only the Linux build is known. The IRIX and NT programs have other addresses (NT is a PE file, not
  ELF); serving them would need a second address map per build, or the search-by-recipe approach of the Bryce
  engine's substrate (`../bryce-engine/docs/substrate.md`: find each table by a pattern and verify its checksum).
  Neither is needed while the release asks for the Linux install. `si-rt` depends on no other crate, so every
  workspace (root, `re/tools/pkg/si-install`, the pictures, import1 and blend2db workspaces) can use it by path.

## 2. First slice: two files converted

| file | before | after | checks |
|---|---|---|---|
| `crates/si-custom/src/shdb_data.rs` | seven hex blobs, 82 KB of hex (audit: one table hit and seven hex-blob hits) | seven `Table`s: 0x086e4c80 0x1140, 0x086e2000 0x2c80, 0x086fc700 0x500, 0x086df000 0x3000, 0x086636d0 0x50, 0x08663940 0xc00, 0x08664600 0xc49c; `store` writes them as before | the seven old literals equal the program's ranges byte for byte; `custom-checkkit` over all 470 vector directories of `re/out/si-custom`, before and after: same output in every directory (100,946 cases: 99,017 identical, 798 defined, 489 different, 614 not compared, before and after); `cusrel1-check`: 1,500 of 1,500 identical, before and after |
| `crates/si-app/src/error_table.rs` | 150 message formats and 5 type names as literals (audit: 110 messages) | `ERROR_FORMAT_TABLE` (0x08686a60, 1,800 bytes: 150 entries of code, type, text pointer), `ERROR_TYPE_TABLE` (0x08687198, 20 bytes); `ERROR_FORMAT` and `ERROR_TYPE` are `LazyLock`s with the same types as before, so `error.rs` is unchanged | `app-emucheck`, before and after: same output (6,913 cases, 6,795 identical, 118 not compared; SI_errorGetMsg 150/150, SI_errorGetType 150/150); a unit test holds the 150 texts (4,763 bytes) and the 5 names (34 bytes) to the checksums of the old literals |

Both crates' tests pass with the install and pass as skipped without it (a copy of the crates outside the
repository, `SI_INSTALL` unset); `custom-checkkit` and `cusrel1-check` call `check_or_exit` first. Neither file is
on the render path, so `render-frames` needs nothing at start for this slice: six first frames (Boolean2D-Gears,
Jitter-Top, Magnet-Balloon, ACTOR-JumpingCube, MATTER-WhatstheMatter1, Duplicator-Necklace) are byte-identical
between the build before and after (12 of 12 files) and identical to the oracle (6 of 6).

## 3. Recipes

* **A. Data written into the memory image at its own address** (`globals.rs`, `data.rs`: lists of `(address,
  bytes)` copied into `Mem` by a `store_data`). Keep the addresses, drop the bytes: `m.set_bytes(addr,
  si_rt::message(addr))` and the zero after it, or, better, one `Table` per contiguous range of `.data`/`.rodata`
  copied whole (the original's initialised memory, padding included). Checksum each range with `si-rt sums`.
* **B. A table of numbers** (u8/u16/u32/f32/f64 arrays). A `static Table` with address, byte length and checksum;
  decode once into the type the code uses: `LazyLock<[f64; 40]>` from `chunks_exact(8)` and `from_le_bytes`, and so
  on. Keep the `const` length the code indexes with.
* **C. A message our own code prints or formats.** `si_rt::text(addr)` where the Rust literal is the original's C
  string; where our literal is a Rust rewording of a `printf` format (`{}` for `%s`), take the original's format
  with `si_rt::message` and format it with the port's own `printf` (`si_render::img::sprintf`,
  `si_tools::stdio::sprintf`, `si_custom::cfmt::sprintf`), so the text stays the original's to the byte.
* **D. A lookup table of names** (`si-file`'s magic types). Keep the keys (four-character tags are not messages), put
  the address of each name beside it, resolve with `si_rt::try_text` when asked; return the error to the caller
  rather than `None`, so a missing install is not mistaken for an unknown type.
* **E. Tests that compare against the original's text** (`assert_eq!(x, Some("..."))`). Compare against
  `si_rt::text(addr)` after `skip_without_install!()`, or against the length and our checksum of the text.
* **F. A list that carries its own address** (`fcvlists_data.rs`: `(addr, &[i32])`). `Table::new(name, addr,
  4 * n, sum)`, decoded to `i32`.

For each file: measure first (`re/tools/rtdata1/locate.py` gives every literal's address, pointers to it and
checksum; `si-rt sums` the ranges), compare the old literals with the ranges (equal byte for byte, or the conversion
is wrong), convert, then run the crate's own check before and after on the same vectors and compare the outputs
(`re/tools/rtdata1/checkkit_all.sh` for a checkkit over every vector directory), and the scenes that use the file
(`re/tools/rtdata1/scenes.sh`). Generators that wrote the literal files must stop writing them (section 6).

## 4. Which programs need the install at start

| program (shipped in the add-on) | needs it after the full conversion | where to call it |
|---|---|---|
| `render-frames` (`re/tools/si-render/pictures`) | yes: `store_data` of si-rensetup and si-render runs for every scene | `install_args()` in `src/frames.rs`, after `si_install::locate` returns: `si_rt::add_root(&i.root); si_rt::check_or_exit("render-frames");` (also on the two-argument and `rules` paths) |
| `si-import-dump` (`re/tools/import1`) | yes: the scene loader links the same data | `run()` in `src/main.rs`, after `si_install::locate`, the same two calls |
| `blend2db` (`re/tools/builder2/blend2db`) | no: none of the original's messages or data windows are in it | |

On failure both print the text of section 1 and exit with status 2; the add-on shows the program's output. The add-on
passes `render-frames` a temporary database directory, not the install, so it must also set `SI_INSTALL` in the
child's environment to the resolved install root (`session.install_root`): in `runner.run_render_frames` and in
`importer`'s call of `si-import-dump`. `si-install`'s inventory (`docs/install-requirements.md`, its `ITEMS`, the
add-on's `install.check`) should list the program file `Creative-Tools/3D/bin/soft` as required for rendering.

## 5. Conversion guide, hit by hit

Lines are of the source tree (the release tree's lines are one more: its SPDX line). Addresses are of the Linux
`soft`; lengths in bytes. "In the bundle" says whether the file's hits are inside the shipped programs (section 7).

### Category 3: blocks of the original's messages in code

| file (lines) | messages | where in `soft` | recipe | in the bundle |
|---|---|---|---|---|
| `crates/si-rensetup/src/native/globals.rs` (`STRINGS` from 172; messages 173-228) | 28 (69 strings in all) | 47 spans, 0x0807d3da .. 0x086a6ace; also the word table at lines 131-134 (68 bytes from 0x0869ec78) | A | yes, 28 in both programs |
| `crates/si-render/src/data.rs` (`TEXTS` from 237; messages 241-311) | 21 (44 strings) | 21 spans, 0x0807e86f .. 0x086a4660 | A | yes, 21 in both |
| `crates/si-file/src/lib.rs` (`magic_type`, 150-184; test 352-355) | 35 | 0x086cebc4 .. 0x086cf1c0, one string per entry, no pointer table [soft 0x083fb328 SI_dbfileMagicType] | D, E | no (used by `si-check`) |
| `crates/si-anim/src/scene1.rs` (239, 247-248, 491, 673, 784, 791, 922, 924, 980-981, 984-985) | 16 (31 strings) | 23 spans, 0x0807df26 .. 0x08698541; also a word table at lines 427-429 (136 bytes from 0x08698280) | C, B | no |
| `crates/si-app/src/error_table.rs` | 110 | | done (section 2) | no |
| `crates/si-app/src/sysmain.rs` (35, 120-121, 311, 351, 370-371, 446, 504, 729, 733, 745, 906) | 16 (42 strings) | 18 spans, 0x08644c80 .. 0x0867cc20 | C | no |
| `crates/si-app/src/usage_text.rs` (6-98) | 71 (85 strings) | 21 spans, 0x08679ea0 .. 0x0867b480 | A or C (the usage screen: print the original's texts in order) | no |
| `crates/si-app/src/image/batch.rs` (190, 498, 509, 518, 527, 556, 560, 590, 647, 817-818, 915, 960, 998, 1021-1022) | 17 (29 strings) | 12 spans, 0x08678fdc .. 0x086c7420 | C | no |

### Category 4: tables of the original's data

| file (lines) | tables (address, length) | recipe | in the bundle |
|---|---|---|---|
| `crates/si-custom/src/shdb_data.rs` | seven hex blobs | done (section 2) | no |
| `crates/si-anim/src/fcvlists_data.rs` (2-11) | ten lists of i32 that carry their own address: 0x08695be0, 0x086959e0, 0x08695920, 0x086958c0, 0x08695880, 0x0869597c, 0x08695a80, 0x08695ac0, 0x08695b20, 0x08695ce0; length four bytes per entry | F | no |
| `crates/si-expr/src/yacc_tables.rs` (15, 17, 19, 22-23) | u8 302 at 0x08650060; u16 162 at 0x086501a0; u16 162 at 0x08650260; u16 252 at 0x08650320 | B | yes, one window |
| `crates/si-gl/src/data.rs` (15-17, 22-26, 31-33, 37-39, 52-116, 120-191) | u16 52 at 0x08674038; u8 48 at 0x086ffa00; u8 32 at 0x086fee40; u8 32 at 0x086fee60; u8 1,024 at 0x086fef20 (the font); u16 1,704 at 0x086ff340 | B | no |
| `crates/si-mental/src/mi2control_yacc.rs` (66, 68, 70, 72, 76, 80, 82; messages from 35) | u8 283 at 0x0865df00; u16 96 at 0x0865e020; u16 96 at 0x0865e080; u16 102 at 0x0865e0e0, 0x0865e160, 0x0865e1e0, 0x0865e260; nine strings at 0x0869ad94 and 0x086d30e5 .. 0x086d316c | B, C | yes, one message |
| `crates/si-render/src/data.rs` (`TABLES`, 151-159 and 163-166) | 320 at 0x0869c680 (the forty doubles `POISSON`); 112 at 0x086a2ec0 | A (whole range) | yes, six windows |
| `crates/si-render/src/shd_list.rs` (19-29) | u8 152 at 0x086a2e20 | B | no |
| `crates/si-scene/src/wave_tables.rs` (7-27, 31-43, 47-72, 76-100, 104-162, 166-230, 234-266, 270-302, 306-333, 337-365) | ten u32 tables: 240 at 0x086d8760, 192 at 0x086d8860, 300 at 0x086d8120, 384 at 0x086d8260, 696 at 0x086d8920, 1,024 at 0x086d8be0, 384 at 0x086d83e0, 512 at 0x086d8560, 324 at 0x086d8fe0, 448 at 0x086d9140 | B | no |
| `crates/si-ui/src/globals.rs` (502-505) | u16 126 at 0x086be9c0 | B | no |

`re/out/rtdata1/locate.tsv` (local, not shipped) has every literal of these files with its line, address, the
pointers to it and our checksum; `python3 -I re/tools/rtdata1/locate.py <file.rs>` remakes it for any file.

### Category 6: the bundle

`render-frames` holds 131 of the original's messages and 9 of `soft`'s 64-byte data windows, `si-import-dump` 143 and
9 (`re/tools/rtdata1/bundle_map.py` traces each to the source literals that hold it; `re/out/rtdata1/
bundle_map.before.tsv`). The two block files of the render path account for 49 of the 131 (globals.rs 28, data.rs
21). The other 82 are spread over 37 files of the render path, one to eleven each (a message held in more than one
file counts in each): si-expr `parse.rs` 11, `token.rs` 9, `eval.rs` 7; si-render 32 in 13 files (`rle.rs` 6,
`renutil_image.rs` 4, `rnd_main.rs` 4, `licence.rs` 3, `materials.rs` 3, `rle_image.rs` 3, `camera.rs` 2, `update.rs`
2, `initialize.rs`, `jitter.rs`, `jitter_image.rs`, `lights.rs`, `matter_image.rs` 1 each); si-model 10 in 4 files
(`model_image/tree.rs` 5, `model_image.rs` 3, `mesh_polygon.rs`, `nurbs_prim_image.rs`); si-scene `load.rs` 7,
`animrel.rs` 1; si-ascii `chn.rs` 4, `srel.rs` 3, `reldata.rs` 1; si-elemio `image/globals.rs` 4; si-math `matrix.rs`
4 (6 in `si-import-dump`); si-rensetup `native/world.rs` 4; si-motion `animcopy.rs` 3; si-fcurve `eval.rs` 2; si-hrc
`image/globals.rs` 2; si-dict `elemsort.rs`, `toposort.rs`, si-kine `image/qscalc.rs`, si-nurbs `trimmed.rs`,
si-mental `mi_texture.rs`, `mi2control_yacc.rs` (one each). The nine windows: the `POISSON` doubles (held twice, in si-render `data.rs` and `shade.rs` lines 46-49), the
table at 0x086a2ec0, one yacc table window of si-expr, and three long messages (si-model `model_image/tree.rs`,
si-elemio `image/globals.rs`, si-mental `mi_texture.rs`).

The bundle category counts a whole program, so it passes only when fewer than 16 messages are left in each program:
converting the block files is not enough. Each of the scattered messages is an error or diagnostic the ported routine
prints or hands on (recipe C, or A where it is written into the image), so the conversion is mechanical but touches
about forty files of other packages; each package converts its own, with its checks before and after. A test
literal that holds the same text (si-expr `tests.rs`, si-fcurve `tests.rs`) is recipe E.

## 6. Generators that wrote literal files

These must stop emitting the bytes (emit the `Table` declarations, addresses and checksums instead) or be retired,
or the next regeneration brings the hits back: `re/tools/si-custom/gen_data.py` (shdb_data.rs),
`re/tools/si-app/gen_tables.py` (error_table.rs, usage_text.rs), `re/tools/si-anim/lift_fcvlists.py`
(fcvlists_data.rs), `re/tools/si-gl/gen_data.py` (si-gl data.rs), `re/tools/si-scene/wave_tables.py`
(wave_tables.rs). The checkers that hold a file's literals against the program (`re/tools/si-rensetup/
check_strings.py`, `re/tools/si-render/check_data.py`, `re/tools/si-mem/check_globals.py`, `re/tools/si-ui/
check_globals.py`) then check the addresses and checksums instead. `crates/si-expr/src/yacc_tables.rs` is made by
`re/tools/si-expr/yacc_check.py` from the grammar we recovered, which checks it equal to the original's tables;
whether output generated that way may ship (or be generated at build time from the
grammar) is the owner's decision; until then recipe B.

## 7. Checking a conversion with the audit before it is committed

`make_release.sh` builds the release tree from a committed ref only. `re/tools/rtdata1/overlay.py --base
re/out/release/fogle3d --dest <dir> <changed paths> -- [--quick] [--bundle-dir <dir>]` copies the last release tree,
puts the working files over it (with the SPDX line the make step adds, new files listed as included by the
manifest), and runs the audit on it. For a full audit (the walk of `re/out` included) the destination must be outside
`re/out`, or the audit finds the copy itself; the script refuses it without `--quick`.

Unit rtdata1's run (overlay of the files of section 2, the base being the release tree of 57d6c85): 15 hits in the
tree's counted categories, down from 24; the eight of `shdb_data.rs` and the one of `error_table.rs` are gone, the
other fifteen are those of section 5. The bundle is unchanged (neither file is linked into the shipped programs).
