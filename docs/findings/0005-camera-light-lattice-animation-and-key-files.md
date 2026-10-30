# 0005 — Camera, light, lattice, animation, struct key, cluster and shape files

Date: 2026-10-13. Binary: Linux `soft`, sha256 `ef21a382c3325d42…`. Static reading; the
readers of camera, light, lattice, animation and struct key files have since been written a
second time on the shared memory image and held against the emulated original (the last
section but one, which also corrects three statements of the sections before it; each is
marked there).
Source files of the original: `bsceneio.c` (the program's camera, light and lattice readers),
`b_structio.c` (their writers, and a second set of readers used by the stand-alone converter),
`fcurveio.c` (function curves, keys, static values), `utilio.c` (values, positions, curve and
action writers, prototype block), `binary.c` / `binary_b.c` (shape and cluster files), and the
element dictionary's and the hierarchy animation's files (the functions that open each file).
Rust: `crates/si-elemio` (the value form in `src/*.rs`, the image form in `src/image/`).
Checks: `cargo run --release -p si-elemio --bin elem-check -- <dir>` (the value form);
`re/tools/si-elemio/check.sh` (the image form: emulation and corpus).

How to read this note. "From code" means read from the decompiled function and, where the
decompiler's rendering was ambiguous, from the disassembly. Field keywords in `CAPITALS` are the
ones the vendor's own converter `bin2ascii` prints for that field: its ASCII writers
(`SI_cameraAsciiWrite`, `SI_lightAsciiWrite`, `SI_latticeAsciiWrite`, `fcurveAsciiWrite`,
`fcvkeyAsciiWrite`, `staticvalAsciiWrite`, `si_elemdictWriteAscAnimations`) were read from the
disassembly of the unstripped Linux `bin2ascii` and paired with the record offsets they load.
Enum value names (`SAA_…`) are from the SDK headers `SAA_genTypes.h` and `SAA_gen.h`. Fields with
no name from either source are given by their offset in the record.

## The seven files

| File | Magic | On the discs | Content after the 88-byte header |
|---|---|---|---|
| `.cam` camera | 0x5380f636 | 358 (328 distinct) | one token stream |
| `.lig` light | 0x5380f639 | 229 (185) | one token stream |
| `.lat` lattice | 0x5380f638 | 11 (9) | token streams of several lattices, one closing zero |
| `.ani` animation | 0x5380f635 | 449 (436) | element list, items (function curves), two action lists |
| `.key` struct keys | 0x5380f646 | 218 (218) | element list, items (static values) |
| `.cls` cluster | 0x5380f637 | 22 (4) | prototype block, one token stream |
| `.shp` shape | 0x5380f642 | 61 (59) | prototype block, geometry records |

None of these files carries an appended icon on the discs. The same file often ships under
several platforms; "distinct" counts contents.

## Tokens

After the header an element file is a stream of 16-bit **tokens**, big-endian, each followed by
the field it announces. Token 0 ends a record. All multi-byte fields are big-endian
(`SI_byteswap_fread`, 0x083f9ba0).

**The size byte.** A token's high byte is the size of its field: 0 = not stated, 1 to 0xfe = that
many bytes, 0xff = a 32-bit size follows the token. A reader that does not know a token calls
`si_fileSkipField` (0x083ff174), which steps over the field by that rule and can do nothing for a
high byte of 0. The newer tokens carry a size (`0x1614`, `0x102a`, `0xff15`); the older ones do
not. Three sizes are wrong as written by this build: `0x1614` (camera projection plane) says 22
bytes and has 32, `0x102a` (light energy) says 16 and has 14, `0x042b` (light exponent) says 4 and
has 6. Each forgets or miscounts the 2-byte inner token of a "value" (below). An older program
skipping these by size would lose its place.

**How the readers read.** Every token and nearly every field is read into a global or static
variable and the result of `fread` is never looked at. A failed read leaves the variable as it
was. Two consequences, both reproduced in `si-elemio`:

- `token` is a separate `static short` in each source file (the decompiler prints one name; the
  addresses differ: `b_structio.c` 0x08817f24, `binary.c` 0x08817f2c, `binary_b.c` 0x08817f30,
  `bsceneio.c` 0x08817f34, `fcurveio.c` 0x08817f3c, `utilio.c` 0x08818160, the element
  dictionary's file 0x08785ba4, the hierarchy animation's file 0x08788120). The shorts, floats
  and integers go through the globals `si_svalue`, `si_fvalue`, `si_ivalue`, `si_lvalue`.
- On a file that ends before its zero token most loops **never end**: the token variable keeps
  the last token and its handler runs again and again. Only the loops that set the token to a
  stop value before each read survive a short file (the animation item header, the action lists).
  The Rust readers stop at the first failed token read and report it (`End::Eof`).
  (Emulated since: two more cases end. A curve whose file ends *between two keys* ends
  normally, the missing keys blank, because a key's closing zero is in the token the curve's
  loop tests; and a camera or light file that ends right after a position with inner token 0,
  because the position reader reads that token into the loop's own variable.)

**Compiled switches.** The readers are `switch` statements on the token. GCC 2.95 compiled the
sparse ones into trees of compares in which many cases are reached only by range (no equality
test), and the decompiler shows the tree. Every token table below was therefore resolved from the
disassembly: `re/tools/si-elemio/switchmap.py` walks the compare tree for all 65,536 token values
and `handlers.py` prints what each handler touches. This is how one dead case was found (light
token `0xff29`, below).

### Sub-records used by several files

| Sub-record | Layout | Reader, writer |
|---|---|---|
| string | bytes up to a zero byte or the end of the file (the reader collects them in a 256-byte stack buffer with no bound) | `SI_stringBinaryRead` 0x084040a0, `SI_stringBinaryWrite` 0x08404104 |
| vector | three floats | `si_vectorBinaryRead` 0x083fd350, `si_vectorBinaryWrite` 0x083fd2e0 |
| value | inner token, then: 1 → one float; 2 → *nothing* (see below); other → skipped by size | `si_valueBinaryRead` 0x083fd564, `si_valueBinaryWrite` 0x083fd4f0 |
| three values | inner token 1 and three floats, otherwise as a value | `si_value3BinaryRead` 0x083fd6a0, `si_value3BinaryWrite` 0x083fd5dc |
| position | inner token: 1 → vector; 3 → 16-bit number and a string: "follow the model of this name"; 2, 4, 5 → nothing read, a message printed | `oldpositionBinaryRead` 0x083f77b0, `si_positionBinaryWrite` 0x083fd76c |
| user data | 32-bit count; per node: 32-bit name length, name, 16-bit word, 32-bit size, data | `si_userDataRead` 0x083ec910, `si_userDataSize` 0x083f3780, `si_userDataWrite` 0x083f3940 |
| custom shader | string name, three 32-bit integers (the first two kept as 16 bits) | `si_udfRead` 0x083e9154, `si_udfWrite` 0x083e9068 |

- **Value, inner token 2.** The value writer, given a function curve, writes inner token 2 and
  the whole curve in place of the float. No caller in these files passes a curve, and the reader
  reads nothing at all for inner token 2: a file with an animated value inline could not be read
  back by this build. Animation lives in the `.ani` file instead.
- **Position, inner token 3.** Only the program's readers (`bsceneio.c`) know it: they put a
  record (owner, kind 4/5 for a camera's position/interest and 6/7 for a light's, the number,
  the name) on a list (`si_cnstrLst`) that the scene loader resolves. The converter's readers use
  `si_positionBinaryRead` (0x083fdbe8), which accepts only a vector. The writer always writes the
  vector. No file on the discs uses token 3.
- **User data.** Nodes named `CUSELEMCB` and `CUSNEXTELEMCB` (plug-in call-back hooks) are not
  written. On reading, if `fileVersion` is below 3.82099 the data block is fetched as *one item
  of `size` bytes* through the byte-swapping read, which reverses items of 2, 4 or 8 bytes: such
  a block comes out byte-reversed on the little-endian builds. From 3.82099 it is fetched as
  `size` single bytes. `fileVersion` is a static of `binary.c`, set by the model, shape and
  cluster readers only, so for a camera or light it is whatever the last such file left. No
  element file on the discs has user data.
- **Custom shader.** For a file version the program calls old (`si_dbfileIsOldUdfVersion`
  0x083fb934: version ≤ 2.67906, or 2.69 ≤ version < 2.999019) the older layout is read and
  dropped: three strings, a 16-bit and a 32-bit number (`readOldUDF` 0x083e9374). An empty name
  gives no shader.

**Version tests.** The version is a float; the constants it is compared with are doubles, and the
compare is done on the x87 at extended precision. So a file marked 2.65 (the float nearest 2.65
is 2.6500001) is *above* the double 2.65, and one marked 2.62 (2.6199999) is *below* 2.62. Every
threshold below is given as the double the code holds.

## Camera file

Reader `SI_cameraBinaryRead` (0x083f789c), called by `SI_elemdictReadCamera` (0x0810fdf0), which
reads into the scene's existing camera: fields the file does not mention keep their values.
Writer `SI_cameraBinaryWrite` (0x083e9480), called by `SI_elemdictWriteCamera` (0x08110498). The
converter's reader `b2aCameraBinaryRead` (0x083e990c) is the same except for positions (above).

| Token | Field | Record offset | Keyword / name | Notes |
|---|---|---|---|---|
| 1, 2 | 16-bit | | | read and dropped; never written |
| 3 | position | +0x10 | `POSITION` | |
| 4 | position | +0x1c | `INTEREST` | the point looked at |
| 5 | value | +0x30 | `ROLL` | radians; the same float as the third of token 6 |
| 6 | three values | +0x28, +0x2c, +0x30 | `YAW_PITCH_ROLL` | |
| 7 | value | +0x34 | `NEAR` | near clipping distance; 0.1 in a new camera |
| 8 | value | +0x38 | `FAR` | 32768 in a new camera |
| 9 | value | +0x70 | `FOCAL_LENGTH` | 50 |
| 10 | value | +0x94 | `F_STOP` | 5.6 |
| 11 | value | +0x74 | `FOCAL_DISTANCE` | 20 |
| 12 | value | +0x40 | `FIELD_OF_VIEW` | radians; π/4 in a new perspective camera. On the discs 0.049 to 2.55 |
| 13 | 16-bit | +0x6c (32-bit) | `DEPTH_OF_FIELD` | 0 `OFF`, 1 `AUTOMATIC`, 2 `CUSTOM` (`SAA_CamDofType`) |
| 14 | value | +0x7c | `NEAR_FOCUS` | 15 |
| 15 | value | +0x84 | `FAR_FOCUS` | 25 |
| 16 | value | +0x8c | `OUT_OF_FOCUS` | 40 |
| 17 | value | +0x98 | `MAXIMUM_COC` | largest circle of confusion; 20 |
| 18 | 16-bit | +0x9c | `ENLARGE` | non-zero `ACTIVE` |
| 19 | value | | | read and dropped; never written |
| 0x14, 0x1614 | 16-bit and five values | +0x108; +0x10c; +0x110, +0x114; +0x118, +0x11c | projection plane: active; distance; width, height; offset x, y (SDK: `SAA_cameraGetProjPlane…`) | written as 0x1614 |
| 0xff15 | 32-bit size, user data | +0x138 | | written only if the camera has user data |

The writer emits tokens 3 to 18 in numeric order, then 0x1614, then 0xff15 if needed, then 0: a
camera file written by this build is exactly 268 bytes without user data. Before the first token
the reader zeroes +0xac and +0xb0 and sets the 16-bit +0xa8 to 1 (meaning not read).

A new camera (`SI_newCamera` 0x080d12dc → `SI_resetCamera` 0x080d10e0) has the defaults in the
table, position (0, 0, 100), interest at the origin, and a projection plane computed by
`SI_resetCamProjPlane` (0x080d1264): distance = focal length, height = 2 · distance ·
tan(field of view / 2), width = height × the factor at +0x64, not active, no offset. The ported
reset matches these; the tangent comes back from the C library on the x87 stack at extended
precision in the original and is a double in Rust, so the last bit of the stored height is not
proven equal (it can differ only when the product lies within about 2⁻⁵² of a rounding
boundary).

**History, from the files.** Files of versions 2.601 to 3.0 (49) have no projection plane
token; from 3.0905 on every file has it. The writer has no version branch.

## Light file

Reader `SI_lightBinaryRead` (0x083f7ce8) via `SI_elemdictReadLight` (0x0810fe78); writer
`SI_lightBinaryWrite` (0x083e9d58) via `SI_elemdictWriteLight` (0x08110570); converter's reader
`b2aLightBinaryRead` (0x083eaa88). The light does not exist until the type token arrives: token 2
allocates it with the defaults of `si_lightInitialize` (0x082819c8) for that type and gives it
the name read under token 1.

| Token | Field | Record offset | Keyword | Notes (default in a new light) |
|---|---|---|---|---|
| 1 | string | +0x00, 30 bytes in the record | `LIGHT_NAME` | |
| 2 | 16-bit | +0x1e | `LIGHT_TYPE` | 1 `POINT`, 2 `SPOT`, 3 `INFINITE`, 4 `SUN` (`SAA_LightType`) |
| 3 | three values | +0x24 | `COLOUR` | red, green, blue (1, 1, 1) |
| 4 | value | +0x84 | `FALL_OFF_START` | (10000) |
| 5 | value | +0x88 | `FALL_OFF_END` | (10000) |
| 6 | value | +0x90 | `SHADOW_FACTOR` | (0.75) |
| 7 | position | +0x34 | `POSITION` | |
| 8 | position | +0x58 | `INTEREST` | |
| 9 | value | +0x70 | `CONE_ANGLE` | radians (0.5236 = 30°); written for spots only |
| 10 | value | +0x74 | `SPREAD_ANGLE` | radians (0.0873 = 5°); written for spots only |
| 11, 15 | value | | | read and dropped; never written |
| 12 | 16-bit | display handle +0x12 | | old line colour, mapped by `si_mapLineColour` 0x083f91c0; never written |
| 13 | 16-bit | display handle +0x0c | | never written |
| 14 | 16-bit | display handle +0x08 | | 1 or 3 also make the handle the active one; never written |
| 16 | 16-bit | +0x8c | `SHADOW_TYPE` | (1) `SAA_LightShadowType`: 1 none, 2 ray traced, 3 depth map, 4 soft |
| 17 | 16-bit | +0x118 (32-bit) | `SHADOW_MAP_RES` | (500) |
| 18 | 16-bit | +0x11c (32-bit) | `SHADOW_FILTER_SIZE` | (10) |
| 19 | 16-bit | +0x124 (32-bit) | `SHADOW_FILTER_STEP` | (2) |
| 20 | value | +0x128 | `SHADOW_FADE_START` | (0.5) |
| 21 | value | +0x12c | `SHADOW_FADE_RATE` | (1.0) |
| 22 | two 16-bit | display handle +0x12 | | wire colour, through `SI_mapIndexToWireColour` 0x083f9278; never written |
| 23 | two 16-bit | +0x15c, +0x160 (32-bit each) | `SELECTIVE` | (0, 1); SDK: selective and include |
| 24 | string | sun +0x00 | `SUN_NAME` | sun only. A new sun is named "Location" |
| 25 | 32-bit | sun +0x04 | `SUN_LATITUDE` | |
| 26 | 32-bit | sun +0x08 | `SUN_LONGITUDE` | |
| 27 | 16-bit | sun +0x0c | `SUN_DAYST` | daylight saving time |
| 28 | 32-bit | sun +0x10 | `SUN_DATE_TIME` | a C `time_t`; a new sun gets the current time |
| 29 | custom shader | +0x178 | | |
| 30 | 32-bit, kept as 16 | +0x182 | `AREA_LIGHT` | (0) |
| 31 | 32-bit | +0x184 | `AREA_TYPE` | (3) `SAA_LightAreaType`: 0 rectangle, 1 sphere, 2 disk, 3 none |
| 32, 33 | 32-bit each | +0x188, +0x18c | `AREA_U_SAMPLE`, `AREA_V_SAMPLE` | (3, 3) |
| 34 | by the area type read so far | +0x190 … | `AREA_GEOMETRY` | type 0: four value triples; 1: one value; 2: a value triple and a value; 3: nothing |
| 35 | 32-bit, kept as 16 | +0x1c0 | `AREA_LOCAL_COORDS` | |
| 36 | three values | +0x1c4 | `AREA_SCALE` | (1, 1, 1) |
| 37 | three values | +0x1d0 | `AREA_ROTATION` | |
| 38 | sixteen values | +0x1e0 | `AREA_ROTATION_MAT` | 4 × 4 by rows (identity) |
| 39 | 32-bit, kept as 16 | +0x1dc | `AREA_SELECTED` | |
| 40 | 32-bit, kept as "≠ 0" | +0x180 | `DIFFUSE` | SDK: diffuse only |
| 41 | custom shader | +0x17c | | a second shader |
| 42, 0x102a | three values | +0x224 | `ENERGY` | (10000 each); SDK: caustic energy |
| 43, 0x042b | value | +0x230 | `EXPONENT` | (2.0); SDK: caustic fall-off exponent |
| 44, 0x042c | 32-bit | +0x234 | `CAUSTIC_PHOTON` | (10000) |
| 45, 0x042d | 32-bit | +0x238 | `GLOBILLUM_PHOTON` | (10000) |
| 46, 0x022e | 16-bit | +0x23c | `CAUSTIC_ENABLE` | |
| 47, 0x022f | 16-bit | +0x23e | `GLOBILLUM_ENABLE` | |
| 0xff28 | 32-bit size, user data | +0x220 | | |

**Order written and version branches** (`SI_versionOutput` against doubles):

1. always: 1, 2, 3, 4, 5, 6, 7, 8; 9 and 10 for a spot; 16 to 21; 23; 24 to 28 for a sun;
2. if version > 2.65: 29, 30, 31, 32, 33, 34, 35, 36, 37, **39, 38**, 40;
3. inside that, unless version < 3.8109: 0x102a, 0x042b, 0x042c, 0x042d, 0x022e, 0x022f;
4. 0xff28 if the light has user data;
5. unless version < 3.8109, and if the light has a second shader: token **0xff29**, a 32-bit
   size (name length + 13) and the shader;
6. 0.

**What the code gets wrong, reproduced as such.**

- *The second shader is lost on reading.* The reader's case for it is the constant −215
  (0xffffff29), but the token is widened without sign (`movzwl`), so the two never compare
  equal; the reader does have a case for the unsized token 41 (0x29), which nothing writes. A
  0xff29 field is stepped over by its size. Both readers have the bug.
- *The sun's name is freed.* The string read under token 24 is stored in the sun record and also
  kept in the reader's "last name" variable, which is freed when the reader returns: the sun
  record is left pointing at freed memory. (Rust keeps the string.)
- Several handlers do not check that the light exists (27, 29, 34, 38, 41) or that it is a sun
  (24 to 28): a stream with those tokens out of place faults in the original. Rust reads the
  field, drops it and flags the read.
- A 30-character name fills its field with no terminator, so the writer's string runs on into
  the type field: one extra byte (the type's low byte) is written on the little-endian builds.

**History, from the files.** Versions up to 2.64933 (14 files) stop after token 23. The two
files marked 2.65 do too, although this build's writer, told to write "2.65", adds the shader
and area tokens (the float 2.65 is above the double): those two were written by a build with a
different test. Versions 3.0 to 3.7 (166 files) have tokens 29 to 39 but no `DIFFUSE` (40);
3.704 and 3.71 (47 files) have it. No file reaches 3.8109, so the six photon tokens occur in none.
No file is a sun, has user data, an inline constraint or a second shader.

The three functions that map old display colours (`si_mapLineColour`, `SI_mapIndexToWireColour`,
`SI_mapWireToIndexColour` 0x083f92bc) are ported as pure functions of the display's colour layout
(six globals); the display handle itself is not modelled, so the reader returns the raw numbers.

## Lattice file

Reader `SI_latticeBinaryRead` (0x083f8d20) via `SI_elemdictReadLattice` (0x08110200); writer
`SI_latticeBinaryWrite` (0x083eb950) via `SI_elemdictWriteLattice` (0x08110988); converter's
reader `b2aLatticeBinaryRead` (0x083ebbb8).

A model has two lists of lattices (acting on the node, acting on the branch); a file holds one
list and its name says which (`…NLat`, `…BLat`). The lattices follow each other with no
separator; the points token closes each, and a single zero token ends the file.

| Token | Field | Record offset | Keyword | Notes |
|---|---|---|---|---|
| 1 | 16-bit | +0x00 | `ACTIVE` | |
| 2 | three 16-bit | +0x02, +0x04, +0x06 | `NB_KEYS` | control points along each axis |
| 3 | three 16-bit | +0x08, +0x0c, +0x10 (32-bit) | `INTERPOLATION` | one code per axis; 0 and 1 occur on the discs. (The SDK's lattice interpolation type is `SAA_AnimInterpType`: linear, cardinal, weight; that the stored code is that enum's value is not checked) |
| 6 | vector | +0x1c | `MINIMUM` | |
| 7 | vector | +0x28 | `MAXIMUM` | |
| 8 | vector | +0x34 | `SCALING` | |
| 4 | one vector per point | +0x14 | `KEYS` | the product of the three counts; completes the lattice |

Written in the order 1, 2, 3, 6, 7, 8, 4. Token 5 does not exist. The reader collects the fields
in local variables and copies them into a new lattice at token 4 without resetting them, so a
later lattice inherits what it does not restate; minimum, maximum, scaling and interpolation are
uninitialised stack if the first lattice lacks them (the value form: zero; emulated since: in
the program they are bytes of the file's own header, see "What the listings and the emulation
added"). A product of zero or less makes
token 4 read nothing and create nothing. Unknown tokens: the program's reader ignores them and
skips nothing; the converter's skips by size.

The converter's own file functions (`SI_latticeWriteBinaryFile` 0x083ec014,
`SI_latticeReadBinaryFile` 0x083ebf60) put, and expect, the four characters `NLAT` between the
header and the stream. The program's do not, and no file on the discs has them: the converter's
reader would misread a database file by four bytes.

## Function curves as stored

This is the stored form the curve evaluator (`fcurve.c`) starts from.

### Key — `SI_Fcvkey`, 36 bytes

Reader `fcvkeyBinaryRead` (0x083facb0), writer `fcvkeyBinaryWrite` (0x083fd7ac). A key is a
token stream of its own, closed by token 0. The reader first blanks the key
(`SI_fkeyInitialize` 0x0820bc50: all zero, +0x1e and +0x20 set to −1; it does not touch +0x0e).

| Token | Type | Record offset | Keyword | Meaning |
|---|---|---|---|---|
| 1 | float | +0x00 | `TIME` | seconds (a key at frame 1 of a 30 fps scene is at 0.0333) |
| 2 | float | +0x04 | `VALUE` | |
| 3 | float | +0x18 | `LEFT_SLOPE` | |
| 4 | float | +0x08 | `RIGHT_SLOPE` | |
| 5 | 16-bit | +0x0e | `INTERPOLATION` | of the segment after the key; codes below |
| 6 | 16-bit | +0x0c | `FLAG` | slope bits (SDK "slope type"): 0 automatic, 1 `SAA_FKSL_USER`, 2 `SAA_FKSL_DOUBLE` (right ≠ left) |
| 8 | 16-bit | +0x1e | `SPLINE_INDEX` | written only when the key type word is exactly 2; −1 otherwise |
| 7 | 16-bit | +0x1c | `KEYTYPE` | low four bits: 0 `FREEFORM`, 1 `KEYFRAME`, 2 `KEYPATH`, 3 `RAWDATA` (`SAA_FKeyType`); the curve code keeps flags in bits 0x10 and 0x20 |
| 9 | 16-bit | +0x20 | | written only when not −1 |

Written in the order 1, 2, 3, 4, 5, 6, [8], 7, [9], 0: 38 bytes for a plain key. Not stored: the
pointer back to the curve (+0x10) and the 16-bit word at +0x14 (the curve reader sets them).
Unknown tokens are ignored and nothing is skipped for them.

Old files: for `SI_versionInput` ≤ 2.64918, a token 7 word with bit 0x10 set is not stored;
instead bit 0x20 of the key type is set. No file on the discs has such a word.

**Interpolation codes** (`SAA_FcurveInterpType`; in brackets the converter's word): 0 constant
[`CONSTANT`], 1 linear [`LINEAR`], 2 Hermite [`SPLINE`], 3 slope 0 [`SLOPE0`], 4 plateau
[`PLATEAU`]. The SDK adds 5 as a "user defined" code valid only for actions.

**Extrapolation codes** (`SAA_FcurveExtrapType`): 0 constant, 1 gradient (`SAA_EXT_GRADT`),
2 cycle.

### Curve — `fcurve`, 100 bytes

Reader `SI_fcurveBinaryRead` (0x083faeb0), writer `SI_fcurveBinaryWrite` (0x083fd9c8).

| Token | Type | Record offset | Keyword | Meaning |
|---|---|---|---|---|
| 1 | 16-bit | +0x00 (32-bit) | `CURVE_TYPE` | which attribute the curve drives: an id of the program's attribute dictionary (names come from that table at run time, `SI_fcvGetNameFromType`; not read here) |
| 2 | 16-bit | +0x0c (32-bit) | number of keys | written when the output version is not above 2.62 |
| 8 | 32-bit | +0x0c | number of keys | written otherwise |
| 3 | 16-bit | +0x18 | `PRE_EXTRAPOLATION` | before the first key |
| 4 | 16-bit | +0x1a | `POST_EXTRAPOLATION` | after the last key |
| 5 | 16-bit | +0x1c | `FLAGS` | 0 in every curve on the discs |
| 6 | 16-bit | +0x1e | `INTERPOLATION` | the curve's default for new segments |
| 7 | the keys | +0x08 | `KEYS` | as many as the count read before it |

Written in the order 1, 8 (or 2), 3, 4, 5, 6, 7, keys, 0. The reader starts from a blank curve
(`SI_fcurveAllocate(0, …)` 0x0820a810) whose interpolation is the program's current default
(`SI_glbAnim` + 0x30); the key array is allocated for the count rounded up to a multiple of eight.
The count must precede token 7. A curve read with no destination is read and dropped.

**What occurs on the discs** (8,495 curves, 1,996,614 keys in the 449 animation files): curve
interpolation 0: 26, 1: 1,235, 2: 7,162, 3: 9, 4: 63; pre-extrapolation 0: 8,449, 1: 3, 2: 43;
post-extrapolation 0: 8,446, 1: 3, 2: 46; key interpolation 0: 2,195, 1: 1,546,769, 2: 447,532,
3: 23, 4: 95; key flag 0: 1,800,017, 1: 188,642, 3: 7,955; key type 0: 1,913,767, 3: 82,847.
Tokens 8 and 9 of a key occur in no file. 179 curves (the files of versions up to 2.62) use the
16-bit count.

### Static value — `SI_HaStaticVal`, 8 bytes

`SI_staticvalBinaryRead` (0x083fb228), `SI_staticvalBinaryWrite` (0x083fb188): token 1, 16-bit
`TYPE` (the curve type the value belongs to); token 2, float `VALUE`; token 0.

### Angles in old files

A file whose header is not accepted (an old, header-less file) has its angle curves in radians;
`SI_fcvConvertAngleUnitToDegree` (0x0820aa60) multiplies value, left slope and right slope of
every key by the double 57.29577951308232 for the curve types 11, 12, 25, 26, 44, 72, 94, 95,
96, 110, 117, 124, 125, 126, 133, 134, 135, 166, 167, 168. So in a file with a header, angle
curves are in degrees. The multiplication is done on the x87: the product is rounded to the
64-bit register and again to a single by the store, which `si_elemio::x87::mul_double_single`
reproduces exactly (a unit test holds a constructed operand pair where this differs from
computing in double precision). No file on the discs is header-less.

## Animation file and struct key file

Both are read by `SI_elemdictReadAnimations` (0x08127760): header, **element list**, **items**,
then two **action lists**. A struct key file is the same thing with static values for items and
no action lists.

### Element list — `si_hieranimBinaryRead` 0x08139094, `si_hieranimBinaryWrite` 0x08138fbc

| Token | Field | Keyword | Meaning |
|---|---|---|---|
| 3 | string | `HIER_CHAPNAME` | the name of the element list (chapter) the elements belong to, e.g. `MODELS`; only as the first token. The reader looks the name up in the element dictionary |
| 1 | string | `HIER_NAME` | an element's name in the database (`prefix-name.version-substitution`) |
| 2 | 16-bit | `HIER_FTH` | index of the element's parent in this list, −1 for the root; adds the element |
| 0 | | `HIER_END` | |

The list gives the hierarchy the animation was saved from: a file is the animation of a model
and its descendants. On the discs: 7,783 elements in the animation files, 4,951 in the key files.

### Item — `SI_elemdictReadAnimation` 0x08110284, `SI_elemdictWriteAnimation` 0x08110a94

A header, then the data.

| Token | Field | Keyword | Meaning |
|---|---|---|---|
| 1 | 16-bit | `OWNER_ID` | index of the owning element in the element list; an item must have it |
| 4 | 16-bit | `ITM_TYPE` | 0 function curve, 1 static value. Written if the output version is above 2.62 |
| 5 | 16-bit | `ACT_RELID` | written (with 6) only if not negative: the id of an action relation |
| 6 | 16-bit | `ACT_OCCID` | |
| 7 | 16-bit | | stored with the item; 1 from the writers of an existing animation, a word of the relation from the scene-side writer. Written if the version is above 2.62 |
| 2 | string | | read and dropped; never written |
| 3 | 16-bit | | read and dropped; never written |
| 0 | | `ANIM_END` | then the curve (type 0) or the static value (type 1); nothing for another type |

A first token of 0, 10 or 99 means there is no further item: 10 opens the first action list, 99
ends a stream written to the program's pipe. The header loop sets its token variable to 0 before
each read, so the end of the file also ends the items. On the discs: 8,495 items in the
animation files, all curves; 44,567 in the key files, all static values. Tokens 2, 3, 5 and 6
occur in no file.

An item is added to its owner's arrays (`SI_hieranimAddItem` 0x08138e0c); an owner index outside
the list drops it.

### Action lists — `SI_actionsBinaryRead` 0x083ff938, `SI_actionsElemBinaryRead` 0x083ff614, `SI_actionsBinaryWrite` 0x083ff218

An action is a named span of time on the animation. The first list is opened by token 10 and
closed by 0x0b; the second is opened by 0x1e and closed by 0x1f. An element of the first list
can name an element of the second (token 0x1b); the program looks the name up there. Each
element is a token stream closed by 0x11:

| Token | Field | Element offset | Meaning (names from the accessor functions `SI_actionsElemSet…`) |
|---|---|---|---|
| 0x0c | string | +0x00 | name; an element must have it |
| 0x12 | float | +0x08 | start, seconds |
| 0x13 | float | +0x0c | end, seconds |
| 0x15 | 16-bit | +0x2c | interpolation code (5 if absent) |
| 0x16, 0x17, 0x18, 0x19 | string each | path record +0x10, +0x0c, +0x18, +0x14 | a path animation record (`SI_PathAnimActs`); written only if the +0x0c string exists |
| 0x1a | function curve | path record +0x08 | |
| 0x14 | 32-bit | +0x24 | number of cycles in the span (0 is read as 1); first list only |
| 0x10 | no field | +0x18 | the flag word is 2 instead of 10 (bit 8 clear); first list only |
| 0x0f | no field | +0x18 bit 1 | selected; first list only |
| 0x1b | string | +0x04 | name of an element of the second list; first list only |
| 0x1c, 0x1d | float each | +0x10, +0x14 | clip start and end; written with 0x1b |
| 0x0d, 0x0e | 32-bit integer each | +0x08, +0x0c | old form of start and end, in frames: divided by the frame rate (30 if the scene's is 0); never written |

- **Half a frame.** The times in the file are half a frame inside the span: the writer stores
  start + h and end − h, the reader takes start − h and end + h, with h = 0.5 / `SI_frameRate`
  (0 if the rate is 0). The scene's frame rate at the moment of loading therefore changes what
  is read, and reading then writing does not always give back the same single.
- **Order.** The reader inserts each element by time (`SI_actionsAddElemTime` 0x081df3e4):
  before the first element whose start + 0.0001 is greater than the new element's *end*. An
  element with start > end or a cycle count ≤ 0 is not added.
- **Selection.** A selected element is selected the way the user would (`SI_actionsElemSelect`
  0x081e46b8): if the selection is in another list, all of that list is deselected first. So a
  selected element in the second list clears the selection read into the first.
- **How the lists start.** The token 10 that opens the first list is consumed by the item reader
  as its "no more items". The 0x1e that opens the second is read by the list reader, is not its
  closing token, and passes through the element reader as an unknown token. Each loop sets its
  token to the stop value before reading, so a file that ends here (most do) gives two empty
  lists.

### Writers

`elemdictSaveHierAnimation` (0x0812ca2c), in its branch for an element that already carries a
hierarchy animation, writes header, element list, every item of every element, and both action
lists — always, even when empty (8 bytes: 10, 0x0b, 0x1e, 0x1f). It writes every item header
with no action numbers and token 7 = 1, whatever the items hold. Its other branch builds the
animation from the scene's relations (`elemdictSaveElementAnimation` 0x0812c34c) and is not
ported: it needs the element dictionary. `SI_elemdictSaveExistingHierAnim` (0x0812cf54) writes
the same without action lists under the struct key magic; the dialog's struct key writer
(`SI_elemdictSaveHierStructKey` 0x0812ce68) produces that layout from the scene and is not
ported either.

**History, from the files.** Of the 449 animation files, 444 have no action list at all; 4 (all
3.71) end with an empty first list (10, 0x0b) and have no second; 1 (3.8) has both, with 12 and 3
elements. The second list is therefore the newer of the two. Versions up to 2.62 (17 files) have
item headers of token 1 alone and 16-bit key counts.

## Cluster file

Reader `SI_clusterAnimBinaryRead` (0x083f1d20), writer `SI_clusterAnimBinaryWrite`
(0x083f68cc). The magic 0x5380f637 is absent from the program's table of file types (finding
0002) but this writer uses it.

A cluster is a named subset of a model's points; a cluster file holds the shape keys of one
cluster. After the header come four bytes which, if they are `PROT`, open a prototype block
(finding 0004); then a token stream:

| Token | Field | Meaning |
|---|---|---|
| 1 | one geometry record of the model's kind | the model's current shape |
| 2 | string | the cluster's name. The model gets a new cluster animation record; if it has a cluster of that name, that cluster gets new, empty key data |
| 3 | 32-bit count, then that many keys | each key: token 4, a 32-bit number of points, and that many positions |
| 5 | 32-bit | key data +0x00 (1 in a new record) |
| 6 | 16-bit | key data +0x08 |
| 7 | 32-bit | cluster animation record +0x04 |
| 8 | 16-bit | cluster animation record +0x08 |

A position is 16 bytes (x, y, z, w) for a patch or NURBS model, 36 bytes (three vectors) for a
face or spline, 12 bytes for a polygon mesh. **The file does not say which**: the reader takes
the kind from the model the file is loaded onto. The writer writes one cluster per file, each key
with the cluster's own number of points.

Quirks, reproduced: a name the model does not have leaves the previous cluster's key data
current, so the count and the two words that follow overwrite that cluster's; tokens 5 to 8
before any name write through null pointers in the original. The reader stores as many positions
as the file says into a buffer sized by the cluster's own count.

**On the discs** all 22 cluster files (4 distinct contents) are empty: header, prototype block,
zero token. The key layout above is therefore from the code alone, unchecked against data. The
meaning of the four words under tokens 5 to 8 has not been read.

## Shape file

Reader `SI_shapeBinaryRead` (0x083f1798), writer `SI_shapeBinaryWrite` (0x083f6314). The magic
0x5380f642 is likewise absent from the program's table of file types.

A shape file holds the *shapes* of one model: alternative definitions of its geometry (the same
patch, mesh, spline … with its points elsewhere), between which shape animation interpolates.
It uses the prototype-and-record format of the model file (finding 0004), not tokens:

| Part | Content |
|---|---|
| header | 88 bytes |
| tag | 4 bytes; if `PROT`, the prototype block follows and then another tag |
| tag | the kind of geometry: `PTCH`, `FACE`, `MESH`, `SPLN`, `NCRV`, `NSRF` |
| count | 16-bit. If negative: (if it is −2, a 16-bit number first, stored in the model at +0x2e2) one definition, which becomes the model's *current* shape; then the count is read again |
| definitions | `count` records of the model's kind, each appended to the model's list of shapes |

- As in a cluster file, **the kind is taken from the model** (model +0x30: 1 patch, 2 face,
  4 mesh, 6 spline, 9 NURBS curve, 10 NURBS surface), not from the file: the tag is only
  compared with the tag for that kind, and a mismatch reads no definitions and fails. For a
  model of any other kind one long field is skipped.
- Any negative count brings a current definition; −2 additionally brings the 16-bit number. The
  number is tested by the routines that interpolate shape weights (0 makes them normalise the
  weights; from code, not otherwise named).
- The writer writes the prototype block for the output version, the tag, then −2, the number
  and the current shape if the model has one, then the number of shapes less one and the shapes
  from the second on: the model's first definition (its base geometry) is in the model file, not
  here. A model left with no current shape after reading gets a copy of its second definition.
- The records are the model file's: spline (`splineBinaryRead` 0x083f00dc) with its keys, face
  (`faceBinaryRead` 0x083f043c: an outline and holes as splines, or as NURBS curves), patch
  (`patchBinaryRead` 0x083f0690), mesh (`meshBinaryRead` 0x083ef514) with vertices, polygons,
  polygon nodes and edges, NURBS curve, surface and composite curve (0x083f2b38, 0x083f2d90,
  0x083f32e4), and their writers. Their field tables are in finding 0004; `si-elemio::geom`
  ports them a second time for these two files (the field names there are from the SDK's file
  format page `hrc7.htm`).

What the readers do to a record after reading it is geometry code and is **not** ported: a
mesh's edges and normals are rebuilt from its polygons, spline and patch matrices are computed,
old Bézier point lists converted, and a face keeps copies of its splines and curves that drop
each spline's name and each curve's closing information. The Rust records are what the file
said. Two consequences for a rewrite: the original would write the rebuilt edge list (Rust
writes the file's), and empty spline names inside a face (Rust keeps the names).

Two version rules in the record readers (`fileVersion`, set from the shape file's header): the
top byte of every polygon node's colour is forced to 0xff in a file below 3.71 (a float
constant); the flag of a NURBS control point is read only from 2.6 (a double) on.

**On the discs** (61 files, 59 distinct): 38 meshes, 18 splines, 2 patches, 1 face (made of NURBS
curves), by distinct content; versions 2.64933 to 3.71. Every file has the count −2, the number
0, a current definition and 2 to 8 further definitions.

## Checks

`elem-check` runs every reader and writer over all files of the seven types under the unpacked
install. For each file: does the reader come to its zero token with the next byte being the end
of the file ("exact end"); read → write as 4.0 → read again, are the two records equal ("round
trip"); and is the file reproduced byte for byte when written again with its own header version
and comment ("identical").

| File | Files | Exact end | Round trip | Identical, own version | Identical body as 4.0 |
|---|---|---|---|---|---|
| camera | 358 | 358 | 358 | 309 | 309 |
| light | 229 | 229 | 229 | 61 | 0 |
| lattice | 11 | 11 | 11 | 11 | 11 |
| animation | 449 | 449 | 449 | 0 (448 but for the list brackets) | 0 |
| struct keys | 218 | 218 | 218 | 218 | 218 |
| cluster | 22 | 22 | 22 | 0 (22 but for the prototype block) | 0 |
| shape | 61 | 61 | 61 | 0 (see below) | 0 |
| **all** | **1,348** | **1,348** | **1,348** | **599** | **538** |

Every file is read to its exact end and every record survives the round trip. For the camera,
light and lattice files the converter's reader was run as well and gives the same record in
every file. Every byte difference is accounted for; the check tries each cause and names the one
that makes the bytes equal:

| Files | Cause | Evidence |
|---|---|---|
| 49 cameras (≤ 3.0) | written before the projection plane token existed; this writer always adds it | identical once those 34 bytes are removed |
| 166 lights (3.0 to 3.7) | written before `DIFFUSE` existed; this writer always adds it above 2.65 | identical once those 6 bytes are removed |
| 2 lights (2.65) | no shader and area block, though the float 2.65 passes this build's "above 2.65" | identical when written as a version below 2.65 |
| all lights, as 4.0 | 4.0 adds the six photon tokens (44 bytes) | |
| 444 animations | the file has no action lists; this writer always writes both, empty (8 bytes) | identical without them |
| 4 animations (3.71) | the file has an empty first list and no second (4 bytes) | identical without it |
| 1 animation (3.8) | action times: start − h + h is not always the same single | identical when read and written with frame rate 0 (no shift) |
| 22 clusters | the prototype block: no file was written by this build | header and token stream identical |
| 61 shapes | the prototype block, as above | 31 files: records identical with this build's writers; 8 (3.704 meshes): identical but for one byte per polygon node, the colour byte the reader forces to 0xff below 3.71 (16,800 bytes a file); 22 (2.64933 and 3.4909 meshes): this build always writes the polygon node's texture coordinates and colour, which those files' node lists (`3 2`, `3 2 4`) lack. They are identical when the records are written by the file's own lists |

The prototype block differs because a build writes the same block into every file, whatever
the file holds, and the block grew with every release: fields this build writes for any output
version (so a 3.71 file lacks hierarchy fields 0xff21, 0xff22, 0x224, material 0x21b to 0x21d,
texture 0x238), thresholds a stamped version passes although the file predates the field
(2.65 again; 3.4909 files have no hierarchy 0xff1c though 3.4909 ≥ 3.49089), and the envelope
list, which the old files have longer.

Assumptions of the check, which the files cannot settle: the scene's frame rate is 30 (the
program's default) for the action lists; the default interpolation of a curve with no token 6 is
2 (every curve has the token); a new sun's time stamp is 0 (no file is a sun); the cluster files
are read as belonging to a mesh (none holds a position).

Unit tests (59, made-up data) cover what the corpus does not exercise: positions that follow a
model, user data and its old byte reversal, the custom shader forms, sun lights, all four area
light forms, the light's version branches and its lost second shader, tokens with size bytes,
truncated streams, counts larger than the file, key path keys, the old key type word, action
lists with path records and selection, cluster keys of each kind, NURBS surfaces and composite
curves, and the x87 product.

## The readers on the memory image (second form, 2026-10-13)

Why: Get Scene builds a scene's dictionaries in `si-mem`'s image (finding 0024) and the
render command and the renderer run on it, but an element's *contents* were not there: this
crate's readers returned Rust values. `crates/si-elemio/src/image/` is the same readers a
second time, written from the listings, **in the image**: a function takes
`&mut si_mem::Mem`, the outside world and the original's arguments, reads and writes the
original's globals at their addresses and makes the original's allocation calls in its
order. What it leaves is the original's record, byte for byte. The value form above stays
as it is (`si-scene` and `si-rensetup` use it).

Everything called "identical" in this section was compared with the original routine
stepped through in the emulation harness (finding 0011). The numbers are of
`re/tools/si-elemio/check.sh` at scale 1.0 on 2026-10-13.

### What is on the image

| File | Source files of the original | Cited | Functions |
|---|---|---:|---|
| `byteswap.rs` | `byteswap.c` | 8 | `SI_swap2byte`, `SI_swap4byte`, `SI_swap8byte`, `validate_float`, `SI_byteswap_fread`, `SI_float_fread`, `SI_fix_SI_FileChunk`, `SI_FileChunk_fread` |
| `utilio.rs` | `utilio.c`, `filetoken.c` | 5 | `SI_stringBinaryRead`, `si_fileSkipField`, `si_valueBinaryRead`, `si_value3BinaryRead`, `si_vectorBinaryRead` |
| `shader.rs` | `b_matio.c`, `headers.c`, `binary.c` | 4 | `si_dbfileIsOldUdfVersion`, `readOldUDF`, `si_udfRead`, `si_userDataRead` |
| `headers.rs` | `headers.c` | 3 | `SI_sceneversioncheckExists`, `SI_sceneversioncheckExec`, `SI_dbfileReadBinHeader` |
| `lights.rs` | `light_fct.c`, `mat_global.c`, `algebra.c` | 6 | `SI_lightGetGrafType`, `SI_matrixCopy`, `si_initSunDefault`, `si_lightInitialize`, `SI_lightAllocate`, `SI_newLight` |
| `camfct.rs` | `cam_fct.c` | 3 | `SI_resetCamProjPlane`, `SI_resetCamera`, `SI_newCamera` |
| `bsceneio.rs` | `bsceneio.c` | 6 | `si_mapLineColour`, `SI_mapIndexToWireColour`, `oldpositionBinaryRead`, `SI_cameraBinaryRead`, `SI_lightBinaryRead`, `SI_latticeBinaryRead` |
| `fcurve.rs` | `fcurveio.c`, `fcv_fct.c`, `fkey_fct.c` | 8 | `SI_fkeyInitialize`, `SI_fcurveInitialize`, `SI_fcurveAllocate`, `SI_fcurveDispose`, `SI_fcvConvertAngleUnitToDegree`, `fcvkeyBinaryRead`, `SI_fcurveBinaryRead`, `SI_staticvalBinaryRead` |
| `anim.rs` | `hierani.c`, `physicalio.c`, `dictin.c`, `actions.c`, the action lists of `utilio.c` | 11 | `SI_actionsInit`, `SI_actionsAddElemTime`, `SI_hieranimInit`, `SI_hieranimAllocate`, `SI_hieranimAddElem`, `SI_hieranimAddItem`, `si_hieranimBinaryRead`, `SI_elemdictReadAnimation`, `SI_actionsElemBinaryRead`, `SI_actionsBinaryRead`, `SI_elemdictReadAnimations` |
| `elemdict.rs` | `physicalio.c` | 3 | `SI_elemdictReadCamera`, `SI_elemdictReadLight`, `SI_elemdictReadLattice` |

**57 functions**, each cited `[soft 0xADDRESS name]` because no case of the check differs and
at least one was compared (`cite.py` sets the citations from the check's report).

Not functions of the original: `outside.rs` (the trait and the 17 routines reached through
it), `host.rs` (`Files`: the C library's streams on byte vectors, behaving as the harness's
stand-ins), `globals.rs` (40 globals and statics, held against the binary by
`check_globals.py`; 10 texts and 33 constants by `check_data.py`), `off.rs` (the offsets).

**Records are addressed by named offsets** (`image::off`), not by `record!`: `si-types` has
no layout for the light, the camera, the lattice, the curve or the animation records, and
none is known beyond what the readers and the allocation routines touch (a `record!` must
name every byte).

**Eight routines of other packages' source files are ported here** because the readers
allocate through them and no crate has them on the image: `SI_fcurveAllocate`,
`SI_fcurveInitialize`, `SI_fcurveDispose`, `SI_fkeyInitialize`,
`SI_fcvConvertAngleUnitToDegree` (`fcv_fct.c`, `fkey_fct.c`; `si-fcurve` has them on
values), `SI_matrixCopy` (`algebra.c`), `SI_actionsInit` and `SI_actionsAddElemTime`
(`actions.c`).

### What a caller hands a reader, and what it finds afterwards

- Once per image: `image::globals::init(&mut m)` (the initial data of the readers' globals
  and the texts they pass on). The world implements `image::Outside`: `call(m, routine,
  args)` for the 17 routines of `outside::ALL` (hand `fread`, `fgetc`, `fopen`, `fclose`,
  `rewind` to an `image::host::Files`; `SI_graflistAddData`, `SI_findGlData`,
  `SI_graflistSetActive` are `si-view`'s, `SI_elemdictFindListByName` is `si-dict`'s), and
  `file_left`.
- A light: `image::elemdict::elemdict_read_light(m, o, path)` → the address of a `light`
  of 0x240 bytes (`off::light`), allocated by the type token with `SI_newLight`, which has
  also asked `SI_graflistAddData` for its graph node; null for a file without a type
  token. A sun's record hangs at +0x16c, shaders at +0x178 and +0x17c, user data at +0x220.
- The camera: the scene's camera must exist (`image::camfct::new_camera(m, o, name, 1)`,
  0x13c bytes) and stand in the third word of the record `SI_procData` points at;
  `elemdict_read_camera(m, o, path)` reads into it and answers it.
- A lattice file: `elemdict_read_lattice(m, o, path, branch, model)`; each lattice (0x44
  bytes with two blocks) is handed to the world's `SI_modelAddILattice (0x08472a0c)`. An
  animation or struct key file: `image::anim::elemdict_read_animations(m, o, dict, path)` →
  an `SI_HierAnim` (`off::hier_anim`): elements of 0x28 bytes with six arrays an item, the
  curves (`off::fcurve`, keys of 0x24 bytes) and static values behind them, two action
  lists.
- Every reader answers `Result<_, NoReturn>`. `Err` means the original would not have come
  back (below); the image then holds what the original had built up to that point, and
  the stream is left open.

### Where the port cannot follow, and what it does instead

The original looks at no result of a read. A port has to stop in four situations
(`image::NoReturn`), and the rule for each is fixed so that the check can hold the port to
it:

- `Endless`: a token loop whose token read failed and left a token that is not the loop's
  end. The static keeps the token, the handler runs again, for ever. The port follows the
  original up to that read and stops; nothing it does differs before it.
- `Stack`: a text of 256 characters or more (`SI_stringBinaryRead` stores the 257th byte
  over its caller's frame pointer), or a field of a negative size with more than 1 KB of
  file left (`si_fileSkipField` reads it in one piece into 1 KB of stack).
- `Count`: a count from the file that the file cannot hold and that is beyond 64 KB
  (`COUNT_LIMIT`): user data sizes, a lattice's points, a curve's keys. Below the limit the
  port allocates as the original does, so a file cut inside a small block leaves what the
  original leaves.
- `Fault`: an access outside memory (a token whose handler stores through a light that was
  never allocated). The image records the fault; the reader stops at its next token.

`SI_secureLicAlloc` (0x08679660) is a counter seven of these routines increment before an
allocation. Its name ties it to the licence scheme, which is not ported: the ports leave it
alone, and the check does not count a write to it as a difference. Nothing was read about
who uses it.

### The check

`re/tools/si-elemio/check.sh` (after `si-view`'s): `gen.py` with the modules `gen_io`,
`gen_light`, `gen_camera`, `gen_lattice`, `gen_fcurve`, `gen_anim` steps each original
routine through the harness on made-up files (every token, each inner form of a value and
of a position, unknown tokens with and without a size, files cut at a made-up byte,
counts beyond the file, statics holding what an earlier read left); `elemio-emucheck`
loads each case into an image (`si_mem::load_case_at`), opens the stream on the same bytes
at the address the harness gave it, runs the port and compares: the result; every buffer
and global, changed or not; every heap block's size and bytes, and that a byte the original
left unwritten is unwritten; the frees in order; the outside calls in order with their
arguments; what was printed; where the stream was left; and that nothing else changed.
Every case runs on two fill patterns on both sides.

Three things are particular to this check:

- **Cases stopped at a loop that cannot end are compared.** For each reader the generator
  hooks the instruction after its loop's token read (12 sites, `elemlib.ENDLESS`, each held
  to lie in its function and to follow a call of `SI_byteswap_fread`) and stops the case
  there when the read gave nothing and left a token that is not zero
  (`STOP:endless:<reader>`). The harness prints memory as it is at that moment; the port
  must stop at the same reader's loop with the same heap, frees, calls and stream. 7,128
  cases are of this kind. Any other stop of the original (a fault, the instruction budget,
  a smashed stack) is not compared.
- **The emulated heap is 1 MB**, and the check's image has the same, so that an allocation
  fails in the port where it failed in the original; a case whose line is longer than
  300,000 characters is left out by the generator (a made-up count made the routine
  allocate hundreds of kilobytes, all of which the line would hold).
- **A signalling NaN moved through the x87 unit is not compared.** The readers copy every
  float with `fld`/`fstp`, which quiets a signalling NaN on a real unit and not in this
  emulator (finding 0011). The generators make none, but a float read in part over an
  earlier one can be one. The port quiets (`si_x87::fmov`) and counts; such a case is
  skipped.

| Routine | Cases | Identical | Identical in the defined part | Different | Not compared |
|---|---:|---:|---:|---:|---:|
| `SI_FileChunk_fread` | 600 | 600 | 0 | 0 | 0 |
| `SI_actionsAddElemTime` | 2,000 | 2,000 | 0 | 0 | 0 |
| `SI_actionsBinaryRead` | 2,495 | 2,260 | 230 | 0 | 5 |
| `SI_actionsElemBinaryRead` | 2,499 | 2,492 | 0 | 0 | 7 |
| `SI_actionsInit` | 30 | 30 | 0 | 0 | 0 |
| `SI_byteswap_fread` | 2,000 | 2,000 | 0 | 0 | 0 |
| `SI_cameraBinaryRead` | 2,968 | 2,926 | 0 | 0 | 42 |
| `SI_dbfileReadBinHeader` | 2,000 | 1,991 | 8 | 0 | 1 |
| `SI_elemdictReadAnimation` | 2,991 | 2,965 | 0 | 0 | 26 |
| `SI_elemdictReadAnimations` | 2,986 | 1,604 | 1,326 | 0 | 56 |
| `SI_elemdictReadCamera` | 1,193 | 1,169 | 0 | 0 | 24 |
| `SI_elemdictReadLattice` | 1,200 | 1,002 | 157 | 0 | 41 |
| `SI_elemdictReadLight` | 1,495 | 1,321 | 49 | 0 | 125 |
| `SI_fcurveAllocate` | 400 | 400 | 0 | 0 | 0 |
| `SI_fcurveBinaryRead` | 3,973 | 3,896 | 0 | 0 | 77 |
| `SI_fcurveDispose` | 500 | 500 | 0 | 0 | 0 |
| `SI_fcurveInitialize` | 50 | 50 | 0 | 0 | 0 |
| `SI_fcvConvertAngleUnitToDegree` | 800 | 800 | 0 | 0 | 0 |
| `SI_fix_SI_FileChunk` | 100 | 100 | 0 | 0 | 0 |
| `SI_fkeyInitialize` | 50 | 50 | 0 | 0 | 0 |
| `SI_float_fread` | 1,000 | 1,000 | 0 | 0 | 0 |
| `SI_hieranimAddElem` | 400 | 384 | 0 | 0 | 16 |
| `SI_hieranimAddItem` | 800 | 800 | 0 | 0 | 0 |
| `SI_hieranimAllocate` | 50 | 50 | 0 | 0 | 0 |
| `SI_hieranimInit` | 50 | 50 | 0 | 0 | 0 |
| `SI_latticeBinaryRead` | 3,000 | 2,136 | 771 | 0 | 93 |
| `SI_lightAllocate` | 200 | 200 | 0 | 0 | 0 |
| `SI_lightBinaryRead` | 4,986 | 4,782 | 0 | 0 | 204 |
| `SI_lightGetGrafType` | 100 | 100 | 0 | 0 | 0 |
| `SI_mapIndexToWireColour` | 800 | 800 | 0 | 0 | 0 |
| `SI_matrixCopy` | 100 | 100 | 0 | 0 | 0 |
| `SI_newCamera` | 200 | 200 | 0 | 0 | 0 |
| `SI_newLight` | 300 | 300 | 0 | 0 | 0 |
| `SI_resetCamProjPlane` | 600 | 600 | 0 | 0 | 0 |
| `SI_resetCamera` | 300 | 300 | 0 | 0 | 0 |
| `SI_sceneversioncheckExec` | 200 | 200 | 0 | 0 | 0 |
| `SI_sceneversioncheckExists` | 50 | 50 | 0 | 0 | 0 |
| `SI_staticvalBinaryRead` | 1,500 | 1,496 | 0 | 0 | 4 |
| `SI_stringBinaryRead` | 1,200 | 1,125 | 0 | 0 | 75 |
| `SI_swap2byte` | 200 | 200 | 0 | 0 | 0 |
| `SI_swap4byte` | 200 | 200 | 0 | 0 | 0 |
| `SI_swap8byte` | 200 | 200 | 0 | 0 | 0 |
| `fcvkeyBinaryRead` | 2,500 | 2,498 | 0 | 0 | 2 |
| `oldpositionBinaryRead` | 2,000 | 1,996 | 0 | 0 | 4 |
| `readOldUDF` | 2,000 | 2,000 | 0 | 0 | 0 |
| `si_dbfileIsOldUdfVersion` | 600 | 600 | 0 | 0 | 0 |
| `si_fileSkipField` | 2,000 | 1,907 | 0 | 0 | 93 |
| `si_hieranimBinaryRead` | 2,000 | 1,999 | 0 | 0 | 1 |
| `si_initSunDefault` | 200 | 200 | 0 | 0 | 0 |
| `si_lightInitialize` | 400 | 400 | 0 | 0 | 0 |
| `si_mapLineColour` | 2,000 | 2,000 | 0 | 0 | 0 |
| `si_udfRead` | 2,500 | 2,374 | 115 | 0 | 11 |
| `si_userDataRead` | 2,376 | 2,200 | 0 | 0 | 176 |
| `si_value3BinaryRead` | 2,000 | 1,996 | 0 | 0 | 4 |
| `si_valueBinaryRead` | 2,000 | 1,997 | 0 | 0 | 3 |
| `si_vectorBinaryRead` | 600 | 596 | 0 | 0 | 4 |
| `validate_float` | 50 | 50 | 0 | 0 | 0 |
| **all (57 routines)** | **69,992** | **66,242** | **2,656** | **0** | **1,094** |

"Identical in the defined part": the case reads memory nobody wrote (the harness's two
passes disagree in some word) and the port agrees in every word that has a value. Not
compared, by reason: the original stopped (a fault, the instruction budget, a stand-in
handed a null pointer) 809; the port does not allocate for a count beyond the file 138; the
port stops where the original overruns a local of its stack 69; a signalling NaN went
through the x87 unit 55; the port's result depends on memory nobody wrote and the harness
could not see that the original's does 23.

The three routines of `cam_fct.c` are "identical given the library": `SI_resetCamProjPlane`
calls the C library's `tan`, whose answer the harness takes from this machine; the check
holds the port to the same argument bits and replays the answer.

Self-tests: `EMUCHECK_MUTATE=1` (one bit of the result and of the first byte the port
changed) makes 67,776 of the 68,921 cases it compares differ; the 1,145 that stay identical
are cases in which the routine returns nothing (it is `void`, or was stopped at a loop that
cannot end) and changes nothing (an owner outside the list, a curve that is no angle, a
null record, an empty file). `EMUCHECK_MUTATE=calls` (one bit of the first argument of
every recorded outside call) makes 11,787 cases differ: the cases that call out with an
argument.

Instructions no case reached (`re/out/si-elemio/coverage/`, an upper bound of what was
reached): 651 bytes in 28 of the 57 routines, most of them the padding between blocks (128
ranges, few longer than 8 bytes). The largest: `SI_lightBinaryRead` 277 of 4,151,
`SI_cameraBinaryRead` 79 of 1,097, `si_udfRead` 41 of 542, `SI_actionsElemBinaryRead` 38 of
802, `SI_latticeBinaryRead` 30 of 933, `SI_fcurveBinaryRead` 20 of 726. The paths among
them, by the listing: the light reader's status line when `SI_newLight` answers null and
the lattice reader's freeing of the vectors when the word block cannot be had (both need
an allocation of a few bytes to fail, which no case arranges); the null-light branches
inside the light's area geometry token, which lie behind an access through that light; and
the unaligned start of `si_udfRead`'s inlined string length, which a block from `malloc`
never takes.

### The corpus

`elemio-imagecheck <dir>`: every sample file of the five kinds is read by the element
dictionary's reader of the image form into an image under `Mem::program()` (the heap that
uses freed room again), and what it built is read back by an adapter
(`src/bin/elemio-imagecheck/back.rs`, `// adapter: not in place`) into the value form's
types and compared with what the value form reads from the same bytes, every field.

| Kind | Files | Returned | Faults of the image | Agree with the value form | Differ |
|---|---:|---:|---:|---:|---:|
| camera | 358 | 358 | 0 | 358 | 0 |
| light | 229 | 229 | 0 | 229 | 0 |
| lattice | 11 | 11 | 0 | 11 | 0 |
| animation | 449 | 449 | 0 | 449 | 0 |
| struct keys | 218 | 218 | 0 | 218 | 0 |
| **all** | **1,265** | **1,265** | **0** | **1,265** | **0** |

With `IMAGECHECK_MUTATE=1` (one bit of the record flipped before it is read back) all
1,265 differ, each kind in the field that was flipped. With `--one-image` all files are
read into one image one after the other, statics carried from file to file as in a
session: the same 1,265 agree, and the heap reaches 77,917,056 bytes at most with 120,072
live blocks of 76,326,569 bytes at the end (135,486 allocations, 15,414 frees; of the
reallocs 225,910 stayed in place and 62,746 moved). Nothing is disposed of between files:
that is what all sample files of these kinds cost at once. A run takes four seconds.

What the corpus run's world supplies in place of routines nobody has on the image, each a
stand-in of the simplest kind and said in `main.rs`: a shader record of 60 cleared bytes
for `SI_udfAllocate`; a list beside the image for `SI_modelAddILattice`; the MODELS
chapter's index for `SI_elemdictFindListByName`; selection as the value form models it for
`SI_actionsElemSelect`; 0 for the graph list's three routines (a light then has no node,
and its old display tokens store nothing; no sample file has them). The frame rate is 30
and the program's curve interpolation 2, as in the value form's check.

### What the listings and the emulation added, or corrected

- **A lattice's unstated fields are bytes of the file's header.** `SI_latticeBinaryRead`
  copies its interpolation codes and three vectors from locals it never sets. Read through
  `SI_elemdictReadLattice`, its frame lies where `SI_dbfileReadBinHeader`'s just lay.
  Stepped through the harness on a file with a marked header
  (`probe_lattice_stack.py`): the second interpolation code is the header's magic number,
  the third the version's bits, and minimum, maximum and scaling are bytes 8 to 43 of the
  comment, unswapped; the first code is a word nobody wrote. (The statement above, "Rust:
  zero", is the value form's; an empty comment gives zeros in the original too.) The image
  form lays the two readers' locals over each other the same way. This was found because
  the check counted 275 cases (23 since) as "the port's result depends on memory nobody wrote, unseen
  by the harness": the harness could not see it because both of its passes left the same
  header bytes there.
- **`oldpositionBinaryRead` reads its inner token into `bsceneio.c`'s own `token`**, the
  loop variable of the reader that called it. A position with inner token 0 as the last
  thing in a file therefore ends the camera or light reader normally.
- **A text ends at the end of the file as at a zero byte** (`SI_stringBinaryRead` tests
  `fgetc` for −1): a file cut inside a name gives the name so far. The 256 bytes are at
  the top of its frame; the 257th store is the low byte of the caller's saved frame
  pointer.
- **`si_fileSkipField`** divides the 32-bit size as a signed number: a negative size is
  not read in 1 KB pieces but asked of `fread` in one piece of up to 4 GB.
- **`si_udfRead`**: with no token list in the prototype block (every camera and light) the
  tokens are 1 to 5 and token 5 returns at once; the test that disposes of a shader named
  `NULL` is reached only by a list that ends in a zero. The name is copied (`strdup`) and
  the read block freed.
- **`SI_dbfileReadBinHeader`** calls the function `scene_version_check` points at (null in
  the binary's data) for a version above the double 4.0001. A file shorter than 88 bytes
  is tested on the bytes that arrived, unswapped, in front of stack contents.
- **`SI_elemdictReadLight`** hands the reader a local it never set: a light file without a
  type token answers a stack word, and a token in front of the type token stores through
  it. **`SI_elemdictReadAnimations`** likewise passes the item reader an occurrence id
  nobody set (stored with every item of a file without token 6: every sample file).
- **A curve whose file ends between two keys ends normally**: a key's closing zero is in
  the token all three readers of `fcurveio.c` share, so the keys still to come return at
  once, blank, and the curve's loop sees the zero. Ending inside a key or among the curve's
  own tokens does not end. A second keys token allocates a second array and loses the
  first.
- **`hierani.c` has a `si_svalue` of its own** (0x08788122) beside its `token`
  (0x08788120); the element list reader does not use the global one. The text under an
  item's token 2 is read and never freed.
- **`SI_actionsAddElemTime`** refuses an element only when its start is *greater* than its
  end: times that are not numbers pass. The place is found by comparing start + 0.0001
  with the new element's end at the x87's precision, not as doubles.
- **`SI_cameraBinaryRead` handed no camera** reads 16-bit words until one is zero, stepping
  over no field (so the "tokens" it meets are whatever the fields hold), and answers 0.

**The ports themselves**: none of the 57 differed from the emulated original in any case at
its first comparison. What the check found was in the checking: 11 cases that differed
were signalling NaNs made by partial reads (the emulator's known limit; now skipped and
counted); cases of megabytes that exhausted the generator (the 1 MB heap); a made-up
shader record with a garbage name that stopped 176 cases of `si_udfRead` (the record is now
as `SI_udfAllocate` leaves it); and the 275 cases above, which were a fact about the
original.

**The value form** agrees with the image form on every field of every sample file. It
differs from the original, as the emulation shows it, in the odd cases this section lists
(a lattice's unstated fields are zero there; it stops at the first failed token read
whatever the token).

### Not ported, not checked

- **Cluster and shape files** (`SI_clusterAnimBinaryRead`, `SI_shapeBinaryRead`): they are
  the model file's geometry records read onto an existing model, whose kind decides the
  record. The geometry readers are being put on the image in `si-hrc` in this same pass;
  these two wait for them and for the model record (`si-model`).
- **The writers**, all of them, and the converter's readers (`b2a…`, `SI_…ReadBinaryFile`).
- Behind the trait, named in `outside.rs` and answered by the world:
  `SI_udfAllocate (0x0854c2f4)`, `SI_udfDispose (0x0854c3c0)` (`udf_fct.c`);
  `SI_modelAddILattice (0x08472a0c)`; `SI_edAddLattice (0x0810383c)`;
  `SI_actionsElemSelect (0x081e46b8)`, `SI_actionsGetNewName (0x081e3f0c)`;
  `SI_statusDisplayMessage (0x083cca94)`; the graph list's three and the dictionary's one,
  which exist in `si-view` and `si-dict`.
- `SI_mapWireToIndexColour` (the writers' side of the old colours) and
  `si_positionBinaryRead` (the converter's) are in the value form only.
- A light's old display tokens (12, 13, 14, 22) are checked against a made-up graph node;
  `off::graf_node` names the three offsets they store (+0x08, +0x0c, +0x12), not
  `si-types`' record.

### Commands

```bash
re/tools/si-elemio/check.sh                 # everything below, about 7 minutes
export CARGO_TARGET_DIR=$PWD/target/si-elemio
(ulimit -v 8000000; cargo test -p si-elemio)                          # 70 unit tests, 11 of them of the image form
(ulimit -v 8000000; cargo build --release -p si-elemio)
E="uv run --with unicorn python3 -I"
(ulimit -v 6000000; timeout 3400 $E re/tools/si-elemio/gen.py re/out/si-elemio/vec 1.0 gen_light)   # one module, or one routine by name
(ulimit -v 2000000; timeout 1800 target/si-elemio/release/elemio-emucheck re/out/si-elemio/vec)
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 1800 target/si-elemio/release/elemio-emucheck re/out/si-elemio/vec)
(ulimit -v 2000000; timeout 900 target/si-elemio/release/elemio-imagecheck re/bin/si3d40-ia/installed [--one-image] [--list])
python3 -I re/tools/si-elemio/cite.py re/out/si-elemio/check.txt      # the citations from the report
python3 -I re/tools/si-mem/check_globals.py si-elemio
python3 -I re/tools/si-elemio/check_outside.py; python3 -I re/tools/si-elemio/check_data.py
(ulimit -v 6000000; timeout 300 $E re/tools/si-elemio/probe_lattice_stack.py)
```

## Open

- The meaning of the fields named by offset: camera +0xa8, +0xac, +0xb0 (reset at every read);
  light +0x14c, +0x150, +0x154 (reset at the type token); the item header's token 7; the key's
  token 9; the cluster's four words; the four strings of a path animation record.
- Tokens that are read and dropped (camera 1, 2, 19; light 11, 15; item 2, 3): what older
  versions stored there.
- The curve type ids: the table is the attribute dictionary's, built at run time.
- `si_getLayoutWindow` (0x083f90c8, in `bsceneio.c`) looks up a window by kind; it belongs to
  the scene file reader and needs the window list. Not ported.
- The reader of the old text cluster format (`SI_oclusterAsciiRead`) and the dispatcher that
  chooses it (`SI_elemdictReadClusters`).
