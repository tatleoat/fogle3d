# 0039 - Model converters: owave2soft.c, osoft2vert.c, osoft2wave.c (package 58, crate si-tools)

## Where it stands

- Crate `crates/si-tools`: `owave2soft.rs`, `osoft2vert.rs`, `osoft2wave.rs`, `omovie2soft.rs`,
  `overt2soft.rs`, `stdio.rs` (strings and C-library hooks); check binary `tools-checkkit` (`si-checkkit`) with a
  stream model in `src/bin/tools-checkkit/files.rs`; vectors from `re/tools/si-tools/gen_tools.py`, `gen_write.py`
  and `gen_read.py`, kept in `re/out/si-tools/vec` and `vec2` (not committed).
- 2026-10-29: [soft 0x081b8584 si_movieToSoftimage], [soft 0x081b8540 si_modelSetDefinition], [soft 0x081b9740
  pmeshReadVertigo] ported, checked with `gen_read.py` (600 cases each): 600, 600 and 564 identical (+5 equal on defined
  bytes), 0 different, 31 not compared; mutation `=1` and `=calls` hold. Not ported: `si_printMessage` (hook),
  `si_dialogModelReadVertigo`, `readVertMdl` (file dialog). Details in the last section.
- 49 functions ported and checked: 37 of `owave2soft.c` (list helpers, materials, `readLine`,
  `readElement`, `readMaterialFile`, the converters, `readWavefrontFile`, `si_objectReadWavefront`), all 4 of `osoft2vert.c` (`headerWriteVertigo`,
  `pmeshWriteVertigo`, `showObject` 0x081b9694, `si_dialogModelWriteVertigo`) and 8 of
  `osoft2wave.c` (`copyMtl`, `checkMtl`, `writeMaterial`, `si_checkMeshTriangulate`,
  `si_putWavefrontHeader`, `si_writeWavefrontMesh`, `si_writeWavefrontPatch`,
  `si_writeWavefrontCurve`).
- Check: `owave2soft` 4,700 cases, 4,661 identical, 0 different, 39 not compared (unchanged by
  the dispose hooks becoming recorded calls). `osoft2vert` 400 cases, 0 different (4 not
  compared; 17 equal under the "printf of non-finite" rule). `osoft2wave` 1,600 cases, 0
  different (copyMtl, checkMtl, triangulate, header, curve 200 identical each; material 188 and
  mesh 194, patch 198 identical, the rest equal only as non-finite printf text). Mutation
  `EMUCHECK_MUTATE=1` differs on every value or output routine; `=calls` differs where calls
  carry arguments (the osoft2wave routines record only `time`, `ctime`,
  `SI_maxNbPolWarning`, so `=calls` shows nothing there).
- `readWavefrontFile` (0x081c1e04): 500 cases = 428 identical, 41 equal on defined bytes, 0 different, 31 not compared
  (the original stopped on an unmapped read); 549 of 5837 bytes unreached (allocation failures and error exits).
  `si_objectReadWavefront` (0x081c1d40): 300 cases = 249 identical, 25 defined, 0 different, 26 not compared; 0 of 194
  bytes unreached. Mutation `=1` and `=calls` differ on every compared case of both.
- Not ported: none of the unit.
- Hooks (unported callees, recorded): the 26 of `owave2soft.c` (si_printMessage 0x081b92b4,
  SI_materialTransfer 0x08284504, SI_modelAllocate 0x08477cf4, the mesh, patch, spline and
  model allocate/dispose/update family; libc fgets, feof, fclose), `osoft2vert.c`:
  pushviewport 0x0860fdcc, viewport 0x0860fe90, SI_clearSelect 0x08335034, popviewport
  0x0860fde4, SI_foolDisplay 0x08323a70, SI_findGlData 0x0832ee68, swapbuffers 0x08611804,
  si_objectFindNextActive 0x081baa68, SI_statusDisplayMessage 0x083cca94,
  SI_dialogFileNameGet 0x083827a0, SI_textListDispose 0x0838260c; `osoft2wave.c`: time
  0x080c4328 and ctime 0x080c4998 (absent imports, recorded), SI_maxNbPolWarning 0x08448afc.
- Not reproduced: uninitialised stack or registers, malloc failure of the file's own
  allocations, a name over 252 bytes overrunning the 256-byte sprintf buffer in
  `convertWFModel2Soft`, `si_writeWavefrontPatch` with a patch type other than 1 to 3 (prints
  an uninitialised buffer).

## 2026-10-28

### Read from code, from names, guessed

- Read from code (decompile and listing, every cited function): all control flow, record
  offsets and constants. Record layouts, as byte offsets in the image: wavefront list entry
  0x1c, list header 0x60, vertex-list block 0x3e88, element block 0x1f48, group name 100
  bytes, smoothing-group entry 0x5c, material 0xdcc with the next pointer at 0x6e4. Field
  names are ours, from use.
- From names: that the unit converts Wavefront .obj/.mtl to the program's model nodes (file
  name, "WF" strings, the token set v, vn, vt, f, g, s, mtllib, usemtl, o, l, p).
- Guessed: nothing that changes behaviour. The meaning of some element-block fields is a guess.

### Method

Worlds are made with `worlds` from `re/tools/emu` (`gen_tools.py`), the routine is stepped in
the emulator, and `tools-checkkit` runs the Rust on the same loaded image. Pointers stored by
a routine into the heap point at fixed globals, since heap bytes are compared as bytes.
`readLine` and `readMaterialFile` take a FILE; the check binary has a small FILE model
(thread-local, fgets/feof/fclose) and a mini fscanf (`%s`, `%f`, `%d`, `%f %f %f`, identified by
the address of the format string). Allocator failure of the hooked allocators (mesh, vertex,
spline, key, patch) is injected in the converter vectors.

Case counts (all identical unless noted): 24 small routines 100 each (the dispose family,
list helpers, material, smoothing-group, `addNewMdl`, `setMdlType`, `copyLine`, `findFather`,
`compareInt`); `readElement` 300; `cleanGroupName` 200; `cleanHierarchy` 300; `convertPoint`,
`convertLine`, `convertCurve`, `convertMesh` 150 each; `convertPatch` 150 (116 identical, 34 not
compared); `convertWFModel2Soft` 300 (295, 5 not compared; kinds 0, 3, 4, 5 only, the mesh and
patch dispatch is not reached, those converters are checked on their own); `readLine` 150;
`readMaterialFile` 300 (both mutation switches differ on all 300; error branches only partly
covered). Route 2 (oracle corpus): not used.

### Behaviour worth knowing

- `readMaterialFile` uses the statement variable before the first `newmtl` (an uninitialised
  register in the original); Rust uses 0. A .mtl whose last statement ends in a newline makes
  the final `%s` scan fail at end of file with the previous token left in the buffer, so the
  original prints "Invalid material file" (level 3) and returns the previous list.
- `readLine` can leave a stale 1000-byte stack buffer when fgets returns null at a piece
  boundary; not reproduced.
- `getListAtPosition` and `getVertexAtPosition` with a position of -1000 or below index
  before the block; not reproduced.
- The emulator's x87 does not quiet signalling NaNs (README "What to trust"). Rust
  `x87_f32_copy` quiets them as hardware does (bit 22); this cannot be checked, so the
  generator never produces a signalling NaN.
- Message text built by sprintf is not compared; `si_printMessage` is logged as two words.
- `convertPatch` dereferences the material list without a null test, so a null list stops
  the original (34 cases not compared).

### Limits of the check tool

- The dispose hooks that receive `&local` (SI_modelDisposeNode, SI_modelDispose, SI_meshDispose,
  SI_patchDispose, SI_splineDispose) are now recorded calls: `Kit::normalise_stack_args()` maps the
  emulator's `57ac57ac` local to the image's, and the totals stayed identical.
- `si-checkkit` has no stream support: `files.rs` is a thread-local model of the case's `FILE`
  arguments (`f=FILE(r)`/`FILE(w)`, a second written stream `g`, named files `file:<path>`),
  with fgets, feof, fclose, fopen, fseek, ftell, fscanf, sscanf (`%s %d %f`, white space),
  fprintf and sprintf (`%d %i %u %x %X %c %s %f`, flags, width, precision, `l`/`h`), all read
  from the image's format strings.
- Written files are compared through the output `stdout`: the model prints the bytes of `f`,
  then `"\n==g==\n"` and the bytes of `g` (when the case has one), then `"\n--<path>--\n"` and each
  named file written; `gen_write.py` builds the original's `stdout` the same way. A `FILE`
  stored in a record (the writer object) is the emulator's handle 0x6f000300 for the first
  `FILE` argument and 0x6f000400 for the second; `files::HANDLE0` is that value.
- Floats printed by the writers are unrounded 80-bit sums in the original; `printf` of NaN and
  infinity is not compared (the kit's rule), so the writers' generators use finite values.
- `time` and `ctime` are absent imports: `gen_write.py` records them (`time=1012586400:p`,
  `ctime=<address of a text buffer of the case>:p`).

### Kit candidates

A stream route in `si-checkkit` (the `files.rs` model: fopen/fgets/fscanf/sscanf/fprintf/
sprintf/fseek/ftell by format address) and a native comparison of `FILE(w)` and `file:` outputs
instead of the `stdout` trick; `record` rules for absent imports in the vector header; a label
for functions with the same name (`showObject` exists at 0x081b9694 and 0x081bac20, the vector
is labelled by address); normalising heap pointers in comparisons; a signalling-NaN note or
quieting in the emulator's x87.

## 2026-10-28 (later): the writers

- `osoft2vert.c`: the Vertigo `.pol` writer. Each vertex is transformed by the model's matrix
  with every product and sum in the 80-bit register (order read from `kit x87`), and printed as
  a double. `pmeshWriteVertigo` appends ".pol" to the name by `SI_realloc` (the block stays 5
  bytes longer) and always answers 1, also when the file cannot be opened. 100 cases each; the
  dialog function 100 cases, 4 not compared (the original stopped on a null mesh).
- `osoft2wave.c`: `si_writeWavefrontMesh` writes vertices, then one normal line per change of
  normal (a table of the running normal count per face corner, grown in steps of 1,000 entries
  by `SI_malloc`), group names, then per polygon the material (looked up by `checkMtl`, which
  appends a "*" to the name of a material whose values differ and looks again, and writes the
  new ones to the `.mtl` file by `writeMaterial`), `usemtl`, and the face as `v//vn` pairs, a
  continuation line after each 20. The counters at +0x150 and +0x154 of the writer object make
  the numbers of a later object continue. The patch writer sends `res 4 4` and one `cdp`,
  `bsp` or `bzp` line of 16 control point numbers per 4 by 4 piece (step 3 for Bezier); the
  curve writer a `cdc` or `l` line and, for a closed curve, its first three numbers again.
- `si_putWavefrontHeader`: mode 1 writes the copyright line, the time text without its newline,
  the object name and 78 blanks and remembers the position; mode 2 seeks back and writes
  `mtllib`. A seek past the end followed by a write fills the gap with zeros (the stream model
  does the same as the emulator).
- Read from code: all of it (every cited function was read in decompile and listing). Guessed:
  the field names of the writer object and of the patch and spline records, from use.

### 2026-10-28 readWavefrontFile and si_objectReadWavefront

Ported and checked: [soft 0x081c1e04 readWavefrontFile], [soft 0x081c1d40 si_objectReadWavefront].
Read from code (decompile and listing): all of it. The kit disassembler stops `readWavefrontFile` at 0x081c349f (a
`.byte 0x8b` at the end of the listing, 5788 of 5837 bytes); the tail (`strrchr(path, '/')` and the `"<%s>"` name format,
string at 0x08691c41) was read from the binary bytes. Guessed: field names only.
Behaviour: the file is `<path>.obj` (a `sprintf("%s.obj")` into an `SI_malloc(strlen+5)` buffer). On an open error the
original prints `%s : %s.` (level 3) with `strerror(errno)` and returns 0. The default material library is the file named
by the environment variable `WF_MTL_LIB`, read before the first line. The static cells at 0x08788c04 (the open .obj
stream) and 0x08788c08 (the last material stream) are written as the original does. Every line is split into its first
word (the token) and the rest, which stays in the line buffer; the first character selects the case. Every loop that
calls `readElement` also adds one to the line counter (the message text carries it). Lines `l`: the choice of direction
depends on the first and last numbers against the two end numbers of the current spline (checked case by case, two
consecutive `l` lines were needed to expose a swapped pair of direction and mode). `vn` leaves the fourth word of the
stack vertex untouched (a stale word, 0xffffffff after a `v`); `v` sets it. A file with no `o` name gets the name
`<file>` (the part of the path after the last `/` in angle brackets). Error exits (bad vertex, normal, `usemtl` with two
names, allocation failure) close the stream, free the name list and materials and return 0. A path without a `/` and no `o` name makes
the original call `strrchr` and add 1 to null: not reproduced (the vectors always use `dir/file`). The 31 cases where the
original stopped on an unmapped read were not analysed further.
Check notes: the stream model starts empty for these two functions (`files::begin_open`) so the first handle is
the emulator's first `fopen` handle; `__errno_location` answers an address in the string area holding 2 (ENOENT), which
is the only errno value any case reaches; `strerror` and `sleep` are recorded calls.

## 2026-10-29: the text model readers

Ported and checked: [soft 0x081b8584 si_movieToSoftimage], [soft 0x081b8540 si_modelSetDefinition],
[soft 0x081b9740 pmeshReadVertigo]. Read from code (listing; the kit listing of `pmeshReadVertigo` stops three
instructions before its end at 0x081b9e7e, so the last call, `SI_meshInitVertexColors (mesh)` before the return, was
read from the binary bytes; a size 1854 against a real 0x73e bytes of that unit). Guessed: field names only.

`si_movieToSoftimage(path, verbose)`: the file is `objects nodes polygons` (first line, read with `fgets` of 100 bytes
and `sscanf`), one `first last` line per object (polygon numbers from 1), the nodes (`%g %g %g`), then one polygon per
run of numbers that ends at the first number not above 0 (a positive number is a node from 1, a negative one `-n` the
last node n, `0` node -1); only the first four numbers of a run are kept, the count of numbers is the side count (a
triangle gets -1 as fourth node). Objects with a polygon of other than 3 or 4 sides are marked invalid (`last = -1`;
message level 2). A valid object (`last > 0`, so an object that ends at polygon 1 is dropped, as the original does)
becomes a model named `<path><k>`, whose mesh holds the nodes from the smallest non-negative to the largest node number
over its polygons; the polygons are made by `SI_polygonInitialize` (3 or 4 corners, the node numbers minus that
smallest). The second and later models become children of the first (`SI_modelCreateParentship`). Every error exit
closes the stream and frees what is held, in the original's order (the model and mesh leak when `SI_vertexAllocate`
or `SI_polygonAllocate` fails). A path of 76 or more bytes overruns the original's 80-byte name buffer (not reproduced;
the vectors use `dir/mv`). `si_modelSetDefinition` stores the kind at +0x30, a 4-byte list holding the definition at
+0x38, the definition at +0x48 and 1 as 16 bits at +0x34; it answers -2 when the list cannot be made (not reached).

`pmeshReadVertigo(name)`: reads `<name>.pol`: words up to `vertices`, then `( x y z )` groups (blanks, and `#` lines),
counted in a first pass (more than 0x7fff is "too big"); the vertices are read in a second pass over the same bytes
(`ftell`/`fseek`); a third pass counts the `face` runs of 16-bit numbers (a run read up to the end of the stream counts
one number more: `fscanf` answers -1 and the original still increments, which leaves a stale word in the corner list;
the generator ends each file with a word so this does not occur); more than `SI_nbPolMax` polygons is "too big"; the
fourth pass reads them again: a `face` makes a polygon (corner array `SI_calloc(n + 3 - ((n-1) & 3), 0x24)`, node
number minus 1 at the start of each 0x24-byte element), a `hole` appends -1 and its own numbers through
`SI_nodesAllocate (0, &polygon[0xc], count, count + n + 1)`. A run of 256 numbers ends the function with an error
message and no cleanup (the stream and mesh leak). Not reproduced: a file without the word `vertices` loops for ever in
the original (the port answers 0); a `#` line cut by the end of the stream loops for ever (the port goes on); a group
cut by the end of the stream before its `)` is not counted but still read (the vertex array is overrun by one).
The `SI_nodesAllocate` hook has no effect in the check (the vectors give a hole only where the corner array,
rounded to a multiple of four, has room), so the reallocation of the original is not compared.

Check notes: `files.rs` learned `%81s`-style widths, the `h` length (16-bit store) and `fgetc`, `ungetc`. Not compared
(31 of 600 for `pmeshReadVertigo`): 8 budget stops (no `vertices` word, `#` at the end) and 22 unmapped writes (a `hole`
before the first `face` writes before the polygon array; a truncated file); the movie reader had none of 600. Movie
coverage: 519 of 3373 bytes unreached (the `SI_malloc` failures of its own buffers); `pmeshReadVertigo`: 109 of 1882
unreached (more than 0x7fff vertices; the second-pass "too many vertices" message that the first pass always
catches; the `SI_malloc` failure of the path buffer).
Kit candidates: `kit dis` stops a function at the size the analysis recorded, here 0x73e of 0x759 bytes (the last call
was missing); a note in `kit dis` when the next bytes after the end are not padding would have caught it.
