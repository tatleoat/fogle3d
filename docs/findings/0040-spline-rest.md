# 0040 - the rest of splines and patches (bez.c, SI_bezierFitContour)

Related: [0015 models and meshes in memory](0015-models-and-meshes-in-memory.md),
[0011 a check binary with si-checkkit](0011-checking-by-emulation.md), 0020, 0024.

## Where it stands

- renderfin1 (2026-10-28): `SI_splineHit` (0x084906d0, GL picking) stays unported: it needs `SI_setProjectionMatrix` (0x08369d80) and
  `SI_computePersProj` (0x0836a030), view code not ported. `SI_patchDrawNormal`, `SI_patchDrawControlMesh` are drawing.
  The norConstr patch-shell check (finding 0022) saw 5 differences in a freed block; they come from `SI_patchAddRow` along U
  on a Bezier U with `u_step` 1 reading and writing the row before the points array (original padding is stale, the
  port's zero); not a fault of `SI_patchAddRow` / `SI_patchUpdateSurface`, whose live arrays agree (1,500 / 1,500 on 4x5
  patches of types 1/2/3).

- Ported on the shared image (`si-mem`), checked on the emulator with `si-checkkit`:
  the five `bez.c` routines (`si_bezPatchCreate`, `SI_bezPatchDestroy`, `si_bezPatchDump`,
  `si_bezPatchInit`, `si_bezPatchCleanup`) in `crates/si-spline/src/bez_image.rs`, and
  `SI_bezierFitContour` in `crates/si-spline/src/fit_contour_image.rs`.
- Check numbers, 300 cases each: Create, Init, Cleanup, Destroy 300 identical;
  Dump 262 identical, 38 not compared (the original faults on a null point array);
  FitContour 300 compared: 243 identical, 57 identical on the defined part, 0 different.
- Mutation switch (`EMUCHECK_MUTATE=1`): Create/Init/Cleanup 300 different, Destroy 253
  (47 are no-op cases), FitContour 300; Dump only the calls mutation applies (262 different).
- Ported and checked since (wave A, package 3), all in `style=world`, GL calls recorded:
  `patchRead` (0x0847a838) in `patch_read_image.rs` (300 identical, vectors from the worldlib
  generator `re/tools/si-spline/gen_patchread.py`); `SI_patchDraw` (200/200), `SI_patchHit`
  (200/200), `SI_patchDrawModif` (200/200), `SI_patchHitAll` (200/200 with no hit; 175 identical
  and 25 not compared with endpick answering 1) in `patch_draw_image.rs`; `SI_splineDraw` (294 of
  300, 6 not compared), `SI_splineDrawModif` (394 of 400, 6 not compared), `SI_splineDrawStart`
  (291 of 300, 9 not compared), `oldsplineHit` (300/300) in `spline_draw_image.rs`. 0 different
  everywhere; the not compared are original faults on a key count the block does not hold.
- Mutation: `=1` changes every function that writes memory (patchRead 300, HitAll 200/175); the
  draw routines write nothing, so only `=calls` applies: 159..191 of 200 (Patch*), 154 of 300
  (splineDraw), 176 of 400 (Modif), 145 of 300 (Start), 241 of 300 (oldsplineHit); the
  unchanged remainder are cases that call nothing.
- NOT ported (3 functions, stopped by the agent's context budget, not by a blocker):
  `SI_splineHit` (0x084906d0; picks per segment, callees pick/endpick/pushviewport (0x0860fdcc)/
  popviewport (0x0860fde4)/SI_setProjectionMatrix, calloc of `2 * nb_keys + 2` shorts),
  `SI_patchDrawNormal` (0x0847e590), `SI_patchDrawControlMesh` (0x08483bb4).
- Weakness: the vertex stand-in logs only the point's address (a key's or a stack local at the
  original's frame address), so the forward-difference floats of types 1 to 3 are checked for
  call order and count, not for value. A record kind that logs the floats behind a pointer
  would close it.
- To check: `re/tools/si-spline/run.sh re/tools/si-spline/specs/NAME.spec [cases]`
  (vectors through the emulator, then `spline-checkkit`); `re/tools/si-spline/mut.sh NAME`
  runs the mutation rows. Specs with globals use `#glob`/`#gbuf` (specgen_g.py).

## 2026-10-28: bez.c and SI_bezierFitContour

Our own description, read from the code and confirmed by the emulator checks.

The bez patch record is 0x7b0 bytes: order in s at +0, order in t at +4, a box of twelve
words at +8, a pointer to the 2D points at +0x98 and to the 3D points at +0x9c, two inline
arrays of up to 25 points of 0x24 bytes at +0xa0 and +0x424, and two heap block pointers at
+0x7a8 and +0x7ac. Create allocates and initialises the record, Init fills it for given
orders, Cleanup frees the heap blocks, Destroy cleans up and frees the record, Dump prints
the orders, box and points through fprintf (doubles as two stack words; the formats are at
0x086dac40..0x086dad69).

`SI_bezierFitContour` allocates a type 1 spline and copies the contour points to a double
buffer. When twice the look-ahead is smaller than the point count minus one, it rotates the
points and flags to start at the first corner and fits each corner run (simplify, then
generate curve); otherwise it fits one Bezier curve with the global fit error. It then
pulls each segment's handles in with the new-point routine when the handle distance sum d3
lies between d1+d2 and four times d3 (80-bit arithmetic, the ratio rounded to double). If
the first and last point lie closer than 10.0 the spline is closed: the previous handle is
copied, the last point deleted, the end handles adjusted. A zero n1 divides by zero in the
original; the port panics likewise.

Guessed, not read: the names of the unnamed fields of the record. Checked on 300 cases;
the 57 "defined part" cases compare only words the original wrote (the rest is allocator
leftover). A manual change of the constants 10.0 and 4.0 gave 40 different, so both
branches are reached.

Shared change needed: `si-checkkit` dependency added to `crates/si-spline/Cargo.toml`
(Cargo.lock updates on build).

## 2026-10-28: patch and spline drawing

Our own description, read from the code and confirmed by the emulator checks.

`patchRead` reads the patch text file: a word-by-word keyword parser (type words linear,
bezier, bspline, cardinal for the u and v directions, point counts, closed flags, steps,
curve counts, tensions, then a point block of `nb_u * nb_v` lines of an index and four
floats, ending at `}`; cardinal sets the tension to 0.5), the strings coming from
`SI_agetString` (0x083fb990) and the numbers from `fscanf`. A first word other than `{` prints
`" { expected\n"`. A file without the closing brace loops forever, as the original does. No
sample scene or installed file of the corpus holds a patch text file, so there is no corpus
check; the vectors are generated files, bad ones included.

The patch display routines walk the two display grids (surface 0 at +0x1b0 with 16-bit
columns and rows at +0x1b4/+0x1b6, surface 1 at +0x1b8 with +0x1bc/+0x1be, 12 bytes a point):
`SI_patchDraw` one line strip per row, `SI_patchHit` the same with `loadname` of a running row
number, `SI_patchDrawModif` strips only over points whose flag word (+0x1d8/+0x1dc) is not
zero, `SI_patchHitAll` a pick per segment (pick, projection matrix, multmatrix, endpick).

The spline routines read the record at +0x8 (closed), +0xc (type), +0x10 (key count), +0x14
(keys of 0x30 bytes: tangent in, position, tangent out, segment at +0x2a, flag at +0x2c),
+0x20 (steps) and +0x24 (matrix). Types 0 and 1 walk the keys, type 1 building four
points (position, tangent out, next tangent in, next position) in the stack frame, multiplying
by the spline's matrix and adding rows 3, 2, 1 of the product `step` times, drawing row 3;
types 2 and 3 slide a window of four positions over the keys. `SI_splineDrawModif` draws only
segments touching a key whose flag has bit 0x20; `SI_splineDrawStart` draws the first piece
and takes exactly one difference step whatever the step count; `oldsplineHit` is the control
polygon with a pick name per key.

Method: the graphics-library calls (pushmatrix, popmatrix, multmatrix, bgnline, endline, v3f,
loadname, initnames, pick, endpick) are recorded in the emulator and answered by the port's
`GlCalls` trait; SI_setProjectionMatrix (0x08369d80) is a recorded hook answered 0, and
SI_agetString and `fscanf` are stand-ins (the checker feeds the case's file through si-ui's
`Files`). The port reproduces the frame addresses of the original's stack locals so the
vertex-call addresses compare.

## Leftovers pass, 2026-10-29 (unit misc2)

Remaining unported in `spline.c`/`patch.c` (si-spline): `SI_splineHit` (0x084906d0, picking: draws each segment into the GL pick buffer with `initnames/pick/endpick`; only caller `SI_graflistHitSelectionOnGrafNode`), `SI_patchDrawNormal` (0x0847e590), `SI_patchDrawControlMesh` (0x08483bb4). Read from code and callers: all are interactive drawing or picking back ends, so skipped as not render; nothing a scene render reaches. No code added.
