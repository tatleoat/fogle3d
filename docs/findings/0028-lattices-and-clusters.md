# 0028 — Lattices, clusters of points and the fitting records

Date: 2026-10-13. Binary: Linux `soft`, sha256 `ef21a382c3325d42…`. Package: `crates/si-deform`
(new), `re/tools/si-deform/`, scratch in `re/out/si-deform/`. Source files of the original:
`lattice.c` (0x0843d910, 21 functions, 10,873 bytes), `klusters.c` (0x0843a9b0, 46 functions,
12,090 bytes), `klusters_b.c` (0x0843d8f0, 1 function, 32 bytes), `fitting.c` (0x0843a130, 10
functions, 2,176 bytes): 78 functions, about 25 KB (sizes counted to the next symbol).

How to read this note: a statement about the original is **read from code** (for every function
ported, the instruction listing; the decompiled text was used only to find one's way) and, for
every function with case counts in section 8, **measured**: the function was stepped through in
the emulation harness (finding 0011) on made-up records and the port compared with what it left
in memory. *[name]* marks a meaning taken from a symbol or a dialog's text; *[guess]* a guess.
The dialogs and the drawing routine are described from their listings and are not measured.

## Results at a glance

| | |
|---|---|
| Functions mapped | all 78: what each does, its callers, its kind (section 8) |
| On the image, checked and cited | **72 of 78**: 20 of `lattice.c`, 41 of `klusters.c`, the one of `klusters_b.c`, all 10 of `fitting.c`: everything that is not drawing or a dialog |
| Left behind the trait | 6: `SI_latticeDraw` (drawing) and the five dialog routines of `klusters.c` (section 10) |
| Emulation, `deform-emucheck` | 72 routines, **85,176 cases: 77,144 identical, 6,869 identical given the C library's `sin` and `cos`, 0 different**, 1,163 not compared |
| What "identical" covers per case | the return value; every buffer of the case, changed or not; every block allocated, with size and every byte the original wrote, and no byte written that the original left alone; the blocks freed, in order; every call to a routine outside the crate, with arguments, in order; the library's mathematics called with the same argument bits in the same order; and that nothing else in the image changed |
| Mutation switches | `EMUCHECK_MUTATE=1` (a bit of the result and of the first byte changed): 68,158 cases differ; the 16,076 that stay identical return nothing and change no memory (what they do is their outside calls, or nothing). `EMUCHECK_MUTATE=calls` (a bit of the first argument of every outside call): 31,460 differ, every case in which the port calls out with an argument |
| Instructions the cases reached | 6,943 instructions in the 72 routines; **1 not reached** (a jump in `SI_klusterActivate` that only a negative state word takes) |
| Globals declared and held against the binary | 9 (`check_globals.py si-deform`: 1 complaint, a false alarm about the identity matrix, section 12) |
| Outside routines named, held against the symbol table | 65, of which the ported code calls 51 (`check_outside.py`: 0 wrong, with every citation and every mention of an address in the crate) |
| Constants taken from the binary's data | 8 (`check_data.py`: 0 wrong) |
| Unit tests | 3 (made-up data) |

## 1. The crate

`crates/si-deform` depends on `si-mem` (the image), `si-types` (`Model`, `GrafList`, `GrafNode`
and the dictionary types of three globals), `si-x87` (the register model, the library's
mathematics with replay, the vector reader) and `si-math` (the ports of the seven routines of
`algebra.c` these files call: `SI_bsplineInterpolate`, `SI_bsplineTangentInterpolate`,
`SI_matrixCopy`, `SI_matrixTransposeInverse`, `SI_matrixXyzRotation`, `SI_matrixInverse`,
`SI_matrixMultiply`). Every function takes the image and the original's arguments; `SI_malloc`,
`SI_calloc`, `SI_realloc`, `strdup` and `free` are made with the original's sizes in the
original's order. There is no store of the crate's own. A local of the original whose address is
passed on, or that the original may read without having set it, is a local of the image's stack.

| Module | Functions | Content |
|---|---:|---|
| `lattice` | 20 | the record; rest layout, copy, disposal; the deformation of a point (with and without tangents) and of a geometry; random displacement, blending of two lattices; the lattice sets of a node |
| `klusters` | 42 | the cluster record and a node's chain; tagging and listing tagged points; activation and the disposal marks; renumbering after the node's points changed; copies, merging and renaming with the dictionary |
| `fitting` | 10 | the two deformation records, their matrices, copies, disposal |
| `records` | | the four records as `record!` layouts, the cluster states, the geometry offsets |
| `globals`, `outside` | | the globals at their addresses; the one trait to everything that is not in the crate |

Which layouts are which: `Model`, `GrafList`, `GrafNode` are `si-types` layouts. `Lattice`,
`SplFitting`, `PtchFitting`, `Kluster` are `record!` layouts of this crate (`si-types` has them
as empty types). The geometry records (mesh, patch, spline, face, NURBS curve and surface) and
the dictionary records two cluster routines touch are addressed by **named offsets**
(`records::geom`, `klusters::dict`).

No routine of the four files tests a licence or product-tier flag.

## 2. Records

### 2.1 The lattice (`SI_Lattice`, 0x44 bytes from `SI_malloc`)

| Offset | Field | As the code uses it |
|---|---|---|
| +0x00 | active | 16 bits; not 0: the lattice deforms. 1 at creation |
| +0x02, +0x04, +0x06 | nx, ny, nz | signed 16 bits: control points per axis |
| +0x08 | interpolation, three 32-bit words | per axis: 0 linear, **anything else** a cubic B-spline (the code only tests for 0) |
| +0x14 | points | nx·ny·nz vectors from `SI_calloc`; point (i, j, k) is at index (i·ny + j)·nz + k: x outermost |
| +0x18 | flags | one 16-bit word per point from `SI_calloc`; bit 0 = tagged |
| +0x1c, +0x28 | min, max | the resting box |
| +0x34 | step | (max − min) / (n − 1) per axis: the resting distance of the points |
| +0x40 | | 0 from `latticeAllocate`, left unwritten by `SI_latticeInitialize`; read by no routine of the file |

A node has two sets (finding 0015): +0x4c count and +0x50 array for the lattices that act on the
node alone, +0x54 and +0x58 for those that act on the branch. Each set is an array of lattice
pointers; **only the first lattice of a set deforms**: every routine here tests and hands out
element 0 only. The others are what `si_latticeAddKey` adds and `SI_modelDeleteLatticeAnimation`
frees *[from those names and from `SI_latticeInterpolate`'s one caller, `animLatticeUpdate`: the
keys of the lattice's animation, blended into the first; their code was not read for this note]*.

### 2.2 The cluster (`kluster`, 0x20 bytes)

| Offset | Field | As the code uses it |
|---|---|---|
| +0x00 | name | the pointer `SI_klusterAddToList` was handed (not copied); freed with the cluster |
| +0x04 | count | points |
| +0x08 | | 0 at creation; cleared again by `SI_modelKlusterResetCluInd` |
| +0x0c | numbers | ascending point numbers, one word each; for a face (kind 2) **two words a point**: curve and point, the outline of a face made of splines being curve −1 |
| +0x10 | state | 0 active, 1 not active (creation), 2 to be disposed, 3 active and 4 not active with "animation to be deleted" |
| +0x14 | | never written in the file |
| +0x18 | shape keys | the cluster animation's record; `SI_klustersAddVector` walks a chain of keys from its +0x18 (each: +0 the points, three vectors a point; +4 the next key) |
| +0x1c | next | |

A node's clusters hang from +0x348. Three more cluster records, each beside an array of user
data chains, carry the per-vertex (+0x408, +0x40c), per-polygon (+0x410, +0x414) and per-edge
(+0x418, +0x41c) user data.

### 2.3 The fitting records

`SI_SplFitting` (0xa8 bytes): +0 active (16 bits), +4 the node whose curve is followed, +8 the
frames along that curve (built by the curve's package, freed with the record), +0xc scale,
+0x18 roll (radians), +0x1c translation, +0x28 the matrix, +0x68 its transposed inverse.

`SI_PtchFitting` (0xac bytes): +0 active, +4 the node whose surface is followed, +8 scale, +0x14
rotation (three angles), +0x20 translation, +0x2c the matrix, +0x6c its transposed inverse
(**not set at creation**: `SI_pfittingAllocate` leaves it as the block came).

In both the two bytes at +2 are never written. A node has a node and a branch deformation of
each kind: +0x60, +0x64 (spline) and +0x2d8, +0x2dc (patch).

### 2.4 Geometry, as these files reach it

Read from `SI_latticeTransform`, `klusterGetIndexes` and `klusterTagCVs`; they agree with finding
0015. Mesh: +0 vertices (0x14 bytes: position, +0xc star, +0x10 star length, +0x12 flag byte),
+4 polygons (0x18 bytes: +0xc corners, +0x10 how many), +0x14 vertex count, +0x20 a flag byte
whose bit 4 says "normals are carried". Star entry: 12 bytes, the third word a polygon number or
−1. Corner: 0x24 bytes, +0 vertex number, +8 normal. Patch: +0xc, +0xe the counts, +0x1a4 points
(16 bytes each), +0x1a8 a 16-bit flag word per point. Spline: +0xc type, +0x10 key count, +0x14
keys (0x30 bytes: three vectors, the point in the middle; +0x2c flag byte). Face: +4 the outline
(a spline in place), +0x78 the number of holes (signed 16 bits), +0x7c the holes (splines, 0x74
bytes each), +0x80 the same as NURBS curves (0x64 bytes each), +0x84 "made of splines". NURBS
curve: +4 point count, +0x2c points, +0x3b flag byte; surface: +0x48, +0x54, +0x5b. Control
point: 0x24 bytes, x, y, z, w as doubles, +0x20 flag byte. **Bit 0 of a point's flag byte is the
tag.**

### 2.5 Globals

| Global | Address | Use here |
|---|---|---|
| `SI_procData` | 0x08679314 | the process record: +0 current dictionary, +4 the scene's graph list |
| `SI_elemDicts` | 0x0867931c | the current dictionary's address (`+ index × 0x70`) is handed to the editing layer; `SI_klusterListMerge` reads its chapter array (+4) |
| `SI_cid` | 0x08781660 | chapter numbers: [0], [2] (clusters), [5] (lattices), [8] (models) |
| `SI_grafLists` | 0x08679498 | `SI_patchRemoveFitting` walks the scene's list |
| `SI_glbDuplic` | 0x087814c0 | +0x20, 16 bits: "duplicate every lattice with its animation" |
| `SI_identite` | 0x086dc160 | the identity matrix every new matrix is copied from |
| `SI_ATD`, `SI_RD` | 0x08781438, 0x087815f0 | their addresses are handed on |
| `workStr` | 0x0881b220 | the file's static text buffer (`SI_klusterListAssignToModel`) |

## 3. How a lattice deforms a point

`SI_latticeValue(lattice, j, k, in, out, axis)` is called with `axis` = 2 and calls itself down
to axis 0. On each level, for its axis a (2 = z, 1 = y, 0 = x):

1. **Locate.** q = (in[a] − min[a]) / step[a], the subtraction and the division in the register
   (`in`, `min`, `step` are floats). The cell is q truncated toward zero (the rounding control
   is switched to "toward zero" for one `fistl`); **of that 32-bit integer the low 16 bits are
   kept**. The fraction is f = float(q − cell). A point below the box has a negative fraction
   and cell 0 or less (truncation, not floor). A NaN or a quotient beyond 32 bits gives the
   integer 0x80000000, hence cell 0.
2. **Collect.** With linear interpolation on this axis: the values at cell and cell + 1.
   Otherwise: at cell − 1, cell, cell + 1, cell + 2. A value on level 2 is the result of level 1
   with that z cell; on level 1 the result of level 0 with that y cell; on level 0 it is the
   control point `latticeVirtualKey(lattice, x cell, y cell, z cell)`. So a point costs 8
   control points when all axes are linear and 64 when all are B-splines.
3. **Blend.** Linear: out = float(v0 + (v1 − v0) × f) per coordinate, difference, product and
   sum in the register, one rounding. Otherwise `SI_bsplineInterpolate(out, f, v−1, v0, v1, v2)`
   (the uniform cubic B-spline of `algebra.c`, finding 0008: weights built from floats rounded
   at every step, the sum times the double constant 0.1666667).

So the interpolation is nested z outside, y in the middle, x inside, and **every level rounds
its result to float** before the next level blends it. Cell numbers are 16-bit on the y and z
levels and wrap as such; on the x level the sign-extended cell and its neighbours are 32-bit.

`latticeVirtualKey(lattice, i, j, k, out)` makes the box endless. Each number is clamped into
0 … n − 1 and how far it lay outside is kept as a positive integer e per axis. The answer starts
as the clamped point P. Then for each axis whose number was negative, and after those for each
axis whose number was n or more, in the order x, y, z: answer = float((P − Q) × e + answer),
where Q is P's neighbour one step **inward** along that axis; difference and product stay in the
register, the answer is rounded at each axis. The control points are thus continued beyond every
face in a straight line through the last two, and a corner cell adds the continuations of its
axes (not a bilinear one). The neighbour's address is computed without a check: with one point
along an axis it reads a point that is not the axis's (or memory in front of the array).

`latticeValueTangent(lattice, j, k, in, out, coordsys, axis)` is the same walk with 32-bit cells
throughout (the truncated integer is used whole), and returns beside the point three vectors:
the derivatives of the answer by the x, y and z cell coordinate. Level 0 produces the point, the
x derivative (linear: float(v1 − v0); B-spline: `SI_bsplineTangentInterpolate`) and copies the
point into the y and z slots. Level 1 blends the point and the x and z slots like the point and
differentiates the y slot (float(second − first), or the B-spline tangent of the four); level 2
blends point, x and y slots and differentiates the z slot. Differentiating a slot that started
as a copy of the point is what makes it the derivative.

## 4. Deforming a geometry: `SI_latticeTransform(lattice, kind, source, destination)`

Called by `SI_modelReadjustDefinition` (the scene update, finding 0015: the node's own lattice,
then each branch lattice on the way up to `latdep`) and by the "freeze" command. Source and
destination are two definitions of the same shape (they may be the same).

| Kind | What is done |
|---|---|
| 4 mesh, flag bit 4 clear | `SI_latticeValue` per vertex; then `SI_meshComputesPolygonNormal(dst, −1)` and `SI_meshComputesVertexNormal(dst, −1)` |
| 4 mesh, flag bit 4 set | `latticeValueTangent` per vertex; normals carried (below); then `SI_meshComputesPolygonNormal(dst, −1)` only |
| 1 patch | `SI_latticeValue` per control point (nu × nv, 16 bytes apart); then `SI_patchComputeSurface(dst, 0)` |
| 6 spline; 2 face made of splines | per key the point; for a spline of type 1 also its two tangent points (three calls a key, in memory order). A face: the outline, then each hole; nothing at all when the hole count is negative |
| 9 NURBS curve; 2 face made of NURBS | per control point: (x, y, z) / w in doubles, rounded to float, through `SI_latticeValue`; the result, widened to doubles, goes to `SI_nurbsCrvMovePoint(dst, index, x, y, z)`. A curve then gets bits 0x60 set in its byte at +0x3b |
| 10 NURBS surface | the same with `SI_nurbsSrfMovePoint(dst, index, x, y, z, w)`, w being **the destination point's** weight; bits 0x60 in the byte at +0x5b |
| others | nothing |

**Carried normals.** For a vertex the three tangent vectors, each divided by the lattice's step
of its axis (float(tangent / step)), are the upper-left 3 × 3 of a matrix whose rest is the
identity: the Jacobian of the deformation. N is its transposed inverse (`SI_matrixTransposeInverse`:
the identity when there is no inverse). Every corner of every polygon in the vertex's star whose
vertex number is this vertex gets: n′ = float(((n.y·N[1][c] + n.x·N[0][c]) + n.z·N[2][c]) + N[3][c])
per coordinate c, written to the destination corner; then, if the new length exceeds 0.0001,
n′ is scaled by (old length / new length), the ratio kept in the register and each product
rounded. Lengths are float(sqrt((x² + y²) + z²)). **A carried normal keeps its length, whatever
that was**, and a normal that collapses is left as transformed.

## 5. The rest of `lattice.c`

- `SI_latticeInitialize(nx, ny, nz, ix, iy, iz, min, max)`: along an axis where |max − min| <
  0.0001 the box becomes float(min − 0.005) … float(max + 0.005). step = float((max − min) /
  (n − 1)) (for n = 1 a division by zero: infinity or NaN, stored). The points are laid out x
  outermost; each coordinate starts at `min` and the step is added once per point **as a float
  sum rounded every time**, so the last point of an axis need not be `max`. `SI_latticeReset`
  does the same from the record's own fields.
- `SI_latticeCopy`, `SI_latticeDispose`, `SI_latticeTransfer` (points and flags from one to
  another of the same counts), `SI_latticeInterpolate(dst, t, a, b)` (float(a + (b − a) × t) per
  coordinate; 0 when the counts differ), `SI_latticeRandomize(lattice, amount, times, tagged)`
  (float((drand48() − 0.5) × amount + coordinate), three random numbers a round).
- `SI_modelLattice(node, &which)`: the first lattice of the branch set if active (which = 1),
  else the first of the node set if active (0), else null **and `which` is not written**.
  `SI_latticeCopyFromSetup` goes on with that unset 16-bit local when the source has no active
  lattice (the harness sees it: section 9).
- `SI_isLatticeDeformed(node)`: for a node that depends on itself, its own node lattice or its
  own branch lattice; otherwise a walk from the node to the father of the node it depends on,
  asking at each step whether the node *that step's node depends on* has an active branch
  lattice.
- `SI_modelDeleteILattice`, `SI_modelDeleteLatticeAnimation` (frees every lattice of the set but
  the first), `SI_modelSwapLattice` (undo: exchanges the set with an array and points the set's
  element at the incoming first lattice), `SI_latticeCopyFromSetup` (duplicate: the whole active
  set with its animation when `SI_glbDuplic` + 0x20 is set, else the active lattice alone).

## 6. Clusters

- **The chain**: `SI_klusterAddToList` appends; `klusterDispose(node, cluster, full)` unlinks a
  cluster (one that is not in the chain is left alone) and frees name, numbers and record, after
  three calls that take its centre relation, its animation and its element away (`full`) or two
  that free its curves and key data. `SI_klusterDisposeAll`, `SI_klusterDisposeEmpty` (also the
  three user data clusters, with their arrays), `SI_clusterCount`, the look-ups by name, by
  number (not counting state 2) and by equality (`klsEqual`: same count, same name, same words).
- **Tagged points**: `klusterGetIndexes(node, out, count_only)` lists the points whose flag byte
  has bit 0, from the stored definition numbered `cur_def` (a face: `SI_faceGetIndexes` on the
  first definition). `klusterTagCVs(node, cluster, definition)` does the reverse in one walk:
  a point's bit is set when it is the next number of the cluster's ascending list and cleared
  otherwise (so a list that is not ascending tags only its ascending prefix).
  `klusterAddToModel(node, name, create)` makes a cluster of the tagged points or replaces the
  active cluster's list, and brings the shape keys in step through `klusterCompare` (a merge of
  the old and the new list: one call per point that went or came).
- **The active cluster and the marks**: `SI_klusterActivate(node, cluster, tag, clear)` moves the
  states (0 ↔ 1, 3 ↔ 4) and, with `tag`, tags the cluster's points in every stored definition
  and in the working one (definition numbers 0 to `nb_defs` inclusive), or calls
  `SI_tagClear(clear)` for no cluster. `klusterSetToDispose`, `klusterResetDisposeFlags` and
  `SI_klusterDisposeFlagged` are the undoable deletion of the info dialog: mark, take back,
  carry out.
- **The centre**: `klusterGetCOG` sums the points `SI_elemGetVertex` answers for (a float sum,
  rounded at every addition) and divides by the cluster's count, not by the number found.
  `SI_klusterCnsSetData(node, centre, out)`: the node's global matrix times the inverse of the
  centre object's; when that has no inverse the product is made from a local nobody set.
- **Renumbering** (`SI_indexModifPropagate` calls these after points were added or removed):
  `klusterMoveIndexes(node, cluster, user data, map)` replaces each number by `map[number]`; a
  negative entry removes the point from the keys and from the cluster (`klusterRemoveIndex`:
  the tail moves down, `SI_realloc` shrinks the array while points are left, the parallel user
  data array follows); then `SI_quicksort` sorts, and when the cluster has keys the sort's noted
  exchanges are repeated on them.
- **Copies and the dictionary**: `SI_modelklusterListDuplicate` builds a copy of the chain while
  the node keeps the old one. `SI_klusterListAssignToModel` hangs a chain on a node, relates each
  cluster's element to the node's (relation 0x2bc) and names it node name + cluster name.
  `SI_klusterListMerge(a, b)` appends b to a after renaming every cluster of b whose name a
  cluster of a has to name + `_`, in the cluster, its element's name record, the related element
  (relation 0x3a9a) and the prefix of every element of the first chapter in the same hierarchy.
  `SI_modelklusterListCvtToFace` turns number lists into (curve, number) pairs in place.

## 7. The fitting records

`SI_fittingUpdateMatrix`: s = float(sin(roll)), c = float(cos(roll)) (the library, on the roll
widened to double, sine first). Seven elements are written, the rest of the matrix stays:
[0][0] = float(c·sx), [0][2] = float(sx·(−s)), [1][1] = sy, [2][0] = float(s·sz), [2][2] =
float(c·sz), row 3 = translation. `SI_pfittingUpdateMatrix`: row r of
`SI_matrixXyzRotation(rotation)` times the r-th scaling factor, row 3 the translation, the
fourth column left alone. Both end with `SI_matrixTransposeInverse`. Copies (`fittingCopy`,
`pfittingCopy`) copy the parameters and recompute; a spline fitting's copy has frames of its own
built (`SI_nurbsUpdateFrenet` for a node of kind 9, else `si_splineUpdateFrenet`). Disposal
first removes the node's curves of type 0xdc/0xdd (spline, node/branch) or 0xe6/0xe7 (patch).
`SI_patchRemoveFitting(surface)` walks the scene's graph list and resets every patch fitting
that follows the surface. `si_deformationCopyFromSetup` marks the attributes 0x6b to 0x8a as
animated and copies the active deformation of each kind into the same place of the copy.

## 7a. Behaviour of the original worth knowing

Read from the listings; each is reproduced, and reached by cases unless it says otherwise.

- **Renumbering a face's cluster removes the wrong words.** `klusterMoveIndexes` hands
  `klusterRemoveIndex` the *place* of the point to remove; for a face (two words a point) that
  routine takes the number as a *word* index. So for the point at place i it deletes the two
  words from word i on: for an even i the pair i/2, for an odd i half of one pair and half of
  the next, and in the odd case its last move reads one word behind the array (in the cases:
  the header of the next heap block). Only faces; the other kinds are right.
- **`SI_modelLattice` leaves `which` unset** when the node has no active lattice, and
  `SI_latticeCopyFromSetup` then chooses the set, and passes on the set number, from that unset
  local (and, without the Duplicate setting, calls `SI_latticeCopy(null)`, which reads through
  null).
- **No `SI_calloc` of a lattice is checked**: `SI_latticeInitialize` and `SI_latticeCopy` write
  the points through whatever came back.
- **A lattice with one point along an axis** has step (max − min) / 0, and `latticeVirtualKey`
  reads a "neighbour" that is another axis's point or memory in front of the array.
- **Truncation, not floor**: a point below the box by less than one step is in cell 0 with a
  negative fraction (linear: the straight continuation; B-spline: the spline evaluated outside
  0 … 1), and only from one whole step below does the virtual point take over.
- **`SI_latticeInterpolate` tests its count for zero only**: a negative product of the three
  counts would run through memory (the cases do not ask for it).
- **`SI_pfittingAllocate` does not set the second matrix**; it is defined after the first
  `SI_pfittingUpdateMatrix`.
- **`SI_klusterCnsSetData`** multiplies with a local nobody set when the centre object's matrix
  has no inverse (`SI_matrixInverse` leaves its result alone then).
- **`klusterGetCOG`** divides by the cluster's count even when `SI_elemGetVertex` refused some
  of the points.
- **`klusterDispose`** reads through null when the node has no cluster at all and the cluster
  given is not null (the cases stop there).

## 8. The map of the four files

Columns: size in bytes to the next symbol; kind; callers (the file's own first); cases /
identical / identical given the library / different (the rest of a routine's cases were not
compared).

| Function | Address | Size | Kind | What it does | Callers | Check |
|---|---|---:|---|---|---|---|
| `SI_isLatticeDeformed` | 0x0843d910 | 120 | record keeping | whether a lattice acts on a node's geometry: its own sets, or a branch lattice on the way up to the node it depends on | SI_dialogModelSetCluster, SI_polyTransform, SI_tagAll, SI_tagClear and 5 more | 1500 / 1500 / 0 / 0 |
| `latticeAllocate` | 0x0843d988 | 156 | record keeping | a lattice record without points | SI_latticeCopy | 4 / 4 / 0 / 0 |
| `SI_latticeInitialize` | 0x0843da24 | 588 | computation | a new lattice at rest in a box: widens a flat box, computes the step, lays the points out | si_dialogGetLattice | 2000 / 2000 / 0 / 0 |
| `SI_latticeReset` | 0x0843dc70 | 196 | computation | puts every point back to rest, clears the flags | si_resetLattice | 1000 / 1000 / 0 / 0 |
| `SI_latticeCopy` | 0x0843dd34 | 300 | record keeping | a copy of a lattice with points and flags of its own | SI_latticeCopyFromSetup, modelHistLatSnapShot, si_latticeAddKey | 800 / 800 / 0 / 0 |
| `SI_latticeCopyFromSetup` | 0x0843de60 | 400 | record keeping | gives a duplicated node the lattices of its source (all of the active set with animation, or the active one) | SI_modelCopyFromSetup, connectExpandFromInstance | 2000 / 1854 / 0 / 0 |
| `SI_latticeDispose` | 0x0843dff0 | 108 | record keeping | frees a lattice and clears the pointer to it | SI_modelDeleteILattice, SI_modelDeleteLatticeAnimation, SI_modelDisposeNode, SI_modelRemoveLattice and 1 more | 300 / 300 / 0 / 0 |
| `latticeValueTangent` | 0x0843e05c | 2080 | computation | a deformed point and the three tangents of the deformation there | SI_latticeTransform, latticeValueTangent | 3000 / 2856 / 0 / 0 |
| `SI_latticeValue` | 0x0843e87c | 1144 | computation | a deformed point: the interpolation, z outermost, then y, then x | SI_latticeTransform, SI_latticeValue | 3000 / 2850 / 0 / 0 |
| `latticeVirtualKey` | 0x0843ecf4 | 1028 | computation | the control point at a cell, continued in a straight line outside the box | SI_latticeValue, latticeValueTangent | 2500 / 2339 / 0 / 0 |
| `SI_latticeRandomize` | 0x0843f0f8 | 348 | computation | moves all or the tagged control points at random | si_randomizeSelection | 1200 / 1200 / 0 / 0 |
| `SI_latticeDraw` | 0x0843f254 | 708 | drawing | draws the lattice: three nests of line strips | si_drawGrafnode | behind the trait |
| `SI_latticeInterpolate` | 0x0843f518 | 172 | computation | one lattice's points between those of two others | animLatticeUpdate | 1000 / 1000 / 0 / 0 |
| `SI_latticeTransform` | 0x0843f5c4 | 2392 | computation | deforms a geometry of any kind into the working definition; carries mesh normals | SI_modelReadjustDefinition, modelNodeFreezeLattice | 4000 / 4000 / 0 / 0 |
| `SI_modelLattice` | 0x0843ff1c | 76 | record keeping | a node's active lattice and which set it is of | SI_latticeCopyFromSetup, SI_createVertexOnMesh, SI_doAddEdgToMesh, SI_dragPoint and 46 more | 600 / 600 / 0 / 0 |
| `SI_modelNodeLattice` | 0x0843ff68 | 32 | record keeping | the first lattice of the node set if active | SI_mdltag, SI_modelReadjustDefinition, SI_modelSaveHistoryGeometry, SI_polyTransform and 10 more | 300 / 300 / 0 / 0 |
| `SI_modelTreeLattice` | 0x0843ff88 | 32 | record keeping | the first lattice of the branch set if active | SI_mdltag, SI_modelCreateParent, SI_modelReadjustDefinition, SI_modelSaveHistoryGeometry and 13 more | 300 / 300 / 0 / 0 |
| `SI_modelDeleteLatticeAnimation` | 0x0843ffa8 | 260 | record keeping | removes a set's animation and every lattice of the set but the first | SI_modelDisposeNode, SI_modelRemoveLattice, SI_modelResetFcv | 1200 / 1200 / 0 / 0 |
| `SI_modelDeleteILattice` | 0x084400ac | 200 | record keeping | takes the lattice at an index out of a set | si_latticeRemoveKey | 1500 / 1500 / 0 / 0 |
| `SI_modelSwapLattice` | 0x08440174 | 400 | record keeping | exchanges a set with an array of lattices (undo) | SI_historyUndoRedo | 1500 / 1500 / 0 / 0 |
| `SI_latticeTransfer` | 0x08440304 | 133 | record keeping | copies points and flags between lattices of the same counts | si_latticeAddKey | 800 / 800 / 0 / 0 |
| `SI_klusterAllocate` | 0x0843a9b0 | 72 | record keeping | a cluster record without points | SI_klusterAddToList, SI_subelUserDataCopyFromSetup, modelBinaryReadNode, modelBinaryReadNodeLite | 4 / 4 / 0 / 0 |
| `SI_klusterGetTrueIndex` | 0x0843a9f8 | 92 | record keeping | the point number at a place of a cluster (pairs for a face) | klusterGetCOG, klusterMoveIndexes, cnsClusterCtrSetPtsLocation, cnsFPntObjSetPtsLocation and 1 more | 1000 / 947 / 0 / 0 |
| `klusterGetCOG` | 0x0843aa54 | 212 | computation | the centre of a cluster's points | SI_dialogModelSetCluster | 1200 / 1200 / 0 / 0 |
| `SI_klusterAddToList` | 0x0843ab28 | 100 | record keeping | a new cluster at the end of a node's chain | SI_modelklusterListDuplicate, klusterAddToModel, clustersReadBinaryChunk | 600 / 600 / 0 / 0 |
| `klusterAllocateIndexes` | 0x0843ab8c | 44 | record keeping | room for the numbers of n points | SI_modelklusterListDuplicate, klusterAddToModel | 60 / 60 / 0 / 0 |
| `klusterCenterRelationDelete` | 0x0843abb8 | 300 | record keeping | removes the relation between a cluster and its centre object | klusterDispose | 1000 / 1000 / 0 / 0 |
| `klusterDispose` | 0x0843ace4 | 268 | record keeping | takes a cluster out of the chain and frees it | SI_klusterDisposeAll, SI_klusterDisposeEmpty, SI_klusterDisposeFlagged | 2000 / 1935 / 0 / 0 |
| `SI_klusterDisposeAll` | 0x0843adf0 | 60 | record keeping | disposes every cluster of a node | SI_dialogNurbsCrvClose, SI_dialogNurbsSrfClose, SI_modelConvertForSumatra, SI_modelDisposeNode and 7 more | 800 / 800 / 0 / 0 |
| `klusterGetIndexes` | 0x0843ae2c | 536 | record keeping | counts and lists a node's tagged points | klusterAddToModel, modelTagCount | 2500 / 2500 / 0 / 0 |
| `klusterGetDistinctName` | 0x0843b044 | 112 | record keeping | a name cls<n> no cluster of the node has | SI_dialogModelSetCluster | 600 / 600 / 0 / 0 |
| `SI_klusterGetByName` | 0x0843b0b4 | 96 | record keeping | the cluster of a name | klusterActivateByName, SI_clusterAnimBinaryRead, clusterAnimNodeAsciiRead, si_edUpdateClusterPointers | 800 / 800 / 0 / 0 |
| `klusterGetByNumber` | 0x0843b114 | 52 | record keeping | the n-th cluster not marked for disposal | SI_dialogClusterInfo | 600 / 600 / 0 / 0 |
| `klusterTagCVs` | 0x0843b148 | 880 | record keeping | tags exactly a cluster's points in one definition | SI_klusterActivate | 3000 / 3000 / 0 / 0 |
| `SI_klusterGetActive` | 0x0843b4b8 | 56 | record keeping | the node's active cluster | SI_dialogModelSetCluster, klusterAddToModel, SI_clusterActive, SI_itemSetClusterConstraint and 15 more | 500 / 500 / 0 / 0 |
| `SI_klusterActivate` | 0x0843b4f0 | 240 | record keeping | makes one cluster the active one and tags its points | SI_dialogClusterInfo, klusterActivateByName, klusterAddToModel, SI_nselectionSelectContent and 1 more | 2500 / 2500 / 0 / 0 |
| `klusterActivateByName` | 0x0843b5e0 | 44 | record keeping | the same by name (no caller) | none | 600 / 600 / 0 / 0 |
| `SI_klusterDisposeFlagged` | 0x0843b60c | 116 | record keeping | carries out the marks: animation deleted, cluster disposed | SI_dialogClusterInfo, SI_historyUndoRedo | 1200 / 1200 / 0 / 0 |
| `klusterSetToDispose` | 0x0843b680 | 60 | record keeping | marks a cluster | SI_dialogClusterInfo | 300 / 300 / 0 / 0 |
| `klusterResetDisposeFlags` | 0x0843b6bc | 76 | record keeping | takes every mark back | SI_dialogClusterInfo | 500 / 500 / 0 / 0 |
| `klusterCompare` | 0x0843b708 | 200 | record keeping | brings a cluster's shape keys in step with a new point list | klusterAddToModel | 1500 / 1500 / 0 / 0 |
| `klusterAddToModel` | 0x0843b7d0 | 264 | record keeping | makes a new cluster, or the active one, of the tagged points | SI_dialogModelSetCluster | 2500 / 2357 / 0 / 0 |
| `modelTagCount` | 0x0843b8d8 | 28 | record keeping | how many points of a node are tagged | SI_dialogModelSetCluster | 600 / 600 / 0 / 0 |
| `SI_klusterCnsSetData` | 0x0843b8f4 | 100 | computation | the matrix a centre constraint keeps: model × inverse of the centre | SI_dialogModelSetCluster, SI_itemSetConstraint, SI_sceneConstrUpdate | 1500 / 1422 / 0 / 0 |
| `SI_isClusterNameUnique` | 0x0843b958 | 84 | record keeping | whether no other cluster has a name | SI_dialogClusterInfo, SI_dialogModelSetCluster, SI_generalAttrSetValue | 800 / 800 / 0 / 0 |
| `SI_dialogModelSetCluster` | 0x0843b9ac | 1132 | dialog | the "set cluster" dialog over the selected nodes | mdlMenu, scnMenu | behind the trait |
| `klusterPrepareTextLine` | 0x0843be18 | 260 | dialog | one line of the cluster list of the info dialog | SI_dialogClusterInfo | behind the trait |
| `klusterInfoUpdateItems` | 0x0843bf1c | 248 | dialog | fills the info dialog's items for a cluster | SI_dialogClusterInfo | behind the trait |
| `SI_dialogClusterInfo` | 0x0843c014 | 2496 | dialog | the cluster info dialog | dialogFaceInfo, dialogMeshInfo, dialogNurbsCurveInfo, dialogNurbsSurfaceInfo and 4 more | behind the trait |
| `SI_klusterListMerge` | 0x0843c9d4 | 668 | record keeping | appends one chain of clusters to another, renaming clashes in the dictionary | dialogPatchMerge, si_dialogSplineMerge | 2000 / 1961 / 0 / 0 |
| `SI_modelklusterListDuplicate` | 0x0843cc70 | 252 | record keeping | a copy of a node's chain of clusters | SI_dialogNurbsCrvClose, SI_dialogNurbsSrfClose, SI_historyUndoRedo, SI_intersectionInfo and 9 more | 1200 / 1200 / 0 / 0 |
| `SI_klusterListAssignToModel` | 0x0843cd6c | 640 | record keeping | gives a node a chain and names the clusters' elements after the node | SI_dialogNurbsCrvClose, SI_dialogNurbsSrfClose, SI_intersectionInfo, SI_nurbsSrfAddRowCol and 7 more | 1500 / 1500 / 0 / 0 |
| `klusterRemoveIndex` | 0x0843cfec | 340 | record keeping | takes one point out of a cluster and its parallel user data | klusterMoveIndexes | 2000 / 1980 / 0 / 0 |
| `klusterSortIndexes` | 0x0843d140 | 156 | record keeping | sorts a cluster's numbers, repeating the exchanges on the keys | klusterMoveIndexes | 600 / 600 / 0 / 0 |
| `klusterMoveIndexes` | 0x0843d1dc | 140 | record keeping | renumbers a cluster after the node's points were renumbered | SI_klusterEdgeMoveIndexes, SI_klusterMoveIndexes, SI_klusterPolygonMoveIndexes | 2000 / 1916 / 0 / 0 |
| `SI_klusterPolygonMoveIndexes` | 0x0843d268 | 60 | record keeping | the same for a mesh's per-polygon user data | SI_indexPolygonPropagate | 800 / 800 / 0 / 0 |
| `SI_klusterEdgeMoveIndexes` | 0x0843d2a4 | 60 | record keeping | the same for per-edge user data | SI_indexEdgePropagate | 800 / 800 / 0 / 0 |
| `SI_klusterMoveIndexes` | 0x0843d2e0 | 100 | record keeping | the same for every cluster and the per-vertex user data | SI_indexModifPropagate | 1500 / 1420 / 0 / 0 |
| `SI_klusterDisposeEmpty` | 0x0843d344 | 440 | record keeping | disposes clusters and user data clusters without points | SI_indexModifPropagate | 1500 / 1500 / 0 / 0 |
| `SI_modelKlusterResetCluInd` | 0x0843d4fc | 60 | record keeping | clears a word of every cluster of an animated node | animClustersUpdate | 500 / 500 / 0 / 0 |
| `SI_modelklusterListCvtToFace` | 0x0843d538 | 208 | record keeping | turns point lists into (curve, point) pairs when a curve becomes a face | si_createFace | 1000 / 1000 / 0 / 0 |
| `SI_clustersApplyTransformation` | 0x0843d608 | 60 | record keeping | hands every cluster to the keys' transformation | SI_modelCompensateMatrix | 300 / 300 / 0 / 0 |
| `SI_klustersAddVector` | 0x0843d644 | 168 | computation | moves every shape key of a chain by a vector | si_dialogSplineMerge | 800 / 800 / 0 / 0 |
| `SI_klusterCountVertexRef` | 0x0843d6ec | 180 | record keeping | in how many clusters a point is | cnsClusterCtrSetPtsLocation | 1200 / 1200 / 0 / 0 |
| `klsEqual` | 0x0843d7a0 | 136 | record keeping | whether two clusters have the same name and points | SI_findCluster | 1500 / 1500 / 0 / 0 |
| `SI_findCluster` | 0x0843d828 | 92 | record keeping | the first cluster of a chain equal to one given | SI_historyUndoRedo | 1200 / 1200 / 0 / 0 |
| `WrathStayGhosted` | 0x0843d884 | 102 | dialog | greys six items of the info dialog out | SI_dialogClusterInfo | behind the trait |
| `SI_clusterCount` | 0x0843d8f0 | 32 | record keeping | how many clusters a node has | SI_modelBinaryWriteNode | 200 / 200 / 0 / 0 |
| `SI_fittingAllocate` | 0x0843a130 | 140 | record keeping | a new spline deformation record | fittingCopy, SI_scnRelationsFromElemDict, si_splineFittingCreate | 4 / 4 / 0 / 0 |
| `fittingCopy` | 0x0843a1bc | 168 | record keeping | a copy of one, with frames of its own | si_deformationCopyFromSetup | 800 / 0 / 800 / 0 |
| `SI_fittingUpdateMatrix` | 0x0843a264 | 152 | computation | its matrix from scale, roll and translation, and the transposed inverse | fittingCopy, SI_modelSetParam, SI_modelUpdateDep, animSplineDefUpdate and 6 more | 2000 / 0 / 2000 / 0 |
| `SI_fittingDispose` | 0x0843a2fc | 136 | record keeping | frees one and clears the pointer | SI_modelRemoveFitting, SI_modelSplineFittingReset | 400 / 400 / 0 / 0 |
| `SI_pfittingAllocate` | 0x0843a384 | 132 | record keeping | a new patch deformation record | pfittingCopy, SI_scnRelationsFromElemDict, si_patchFittingCreate | 4 / 4 / 0 / 0 |
| `pfittingCopy` | 0x0843a408 | 124 | record keeping | a copy of one | si_deformationCopyFromSetup | 800 / 0 / 800 / 0 |
| `SI_pfittingUpdateMatrix` | 0x0843a484 | 144 | computation | its matrix from scale, rotation and translation, and the transposed inverse | pfittingCopy, SI_modelSetParam, SI_modelUpdateDep, animPatchDefUpdate and 6 more | 2000 / 0 / 2000 / 0 |
| `SI_pfittingDispose` | 0x0843a514 | 108 | record keeping | frees one and clears the pointer | SI_modelPatchFittingReset, SI_modelRemoveFitting | 400 / 400 / 0 / 0 |
| `SI_patchRemoveFitting` | 0x0843a580 | 128 | record keeping | takes every patch deformation onto a surface off the scene | SI_graflistDeleteGrafnode, SI_modelDeleteDefinition, convertPatchToNURBS, dialogPatchConversion | 1500 / 1500 / 0 / 0 |
| `si_deformationCopyFromSetup` | 0x0843a600 | 944 | record keeping | gives a duplicated node the deformations of its source | SI_modelCopyFromSetup | 1500 / 231 / 1269 / 0 |

78 functions. lattice.c: 8 computation, 1 drawing, 12 record keeping; 20 checked | klusters.c: 3 computation, 5 dialog, 38 record keeping; 41 checked | klusters_b.c: 1 record keeping; 1 checked | fitting.c: 2 computation, 8 record keeping; 10 checked

## 9. Checking by emulation

After `si-view`'s pattern (finding 0027, section 6). `re/tools/si-deform/gen.py` drives the
harness of `re/tools/emu` through `deformlib.py` (the conventions), `dworld.py` and `kworld.py`
(made-up records); `gen_lattice.py`, `gen_transform.py`, `gen_fitting.py`, `gen_klusters.py`,
`gen_kedit.py` hold one generator per routine. A case is one call; every buffer has the
harness's address in the image (`si_mem::load_case_at`); each case runs twice, on memory holding
a pattern and its complement. Differences from `si-view`'s check:

- a stand-in can answer through several locals of the caller, each with several words, 32-bit
  or 16-bit (`SI_elemGetVertex` stores a vector and a 16-bit flag);
- the library's `sin` and `cos` are in the vector with their argument bits and are replayed
  (`si_x87::trig`); such a case counts as "given the library" (finding 0011);
- `drand48` is the harness's exact stand-in; the check answers with the same generator from the
  same seed through the trait's second method;
- a `float` argument and an `SI_Vector` passed by value are words of the case.

**The cases**: lattices of 2 to 4 points an axis (3% with a single point along an axis), at
rest or with moved points, with every mix of linear and B-spline axes, a few with odd steps or
special floats; points inside the box, on its faces, outside, and wild (zeros, infinities, NaNs,
huge). Geometry of every kind with one to seven points, source and working definition separate
or the same; meshes with both settings of the normal bit and stars that name the vertex's
polygons, −1 entries and polygons the vertex is not in. Nodes with one to four clusters in every
state, with and without names and keys, cluster lists that are and are not ascending subsets of
the geometry's points; renumbering maps with removed points; dictionaries with elements in use
and deleted, in and outside a hierarchy.

### 9.1 Numbers

Run of 2026-10-13, `re/tools/si-deform/check.sh 1.0`.

| File | Routines | Cases | Identical | Given the library | Different | Not compared |
|---|---:|---:|---:|---:|---:|---:|
| `lattice.c` | 20 | 28,504 | 27,903 | 0 | 0 | 601 |
| `klusters.c` | 41 | 47,064 | 46,502 | 0 | 0 | 562 |
| `klusters_b.c` | 1 | 200 | 200 | 0 | 0 | 0 |
| `fitting.c` | 10 | 9,408 | 2,539 | 6,869 | 0 | 0 |
| **all** | **72** | **85,176** | **77,144** | **6,869** | **0** | **1,163** |

| Mutation run | Identical | Different | Not compared |
|---|---:|---:|---:|
| `EMUCHECK_MUTATE=1` | 16,076 | 68,158 | 942 |
| `EMUCHECK_MUTATE=calls` | 52,734 | 31,460 | 982 |

Not compared: 690 cases agree in every defined word but carry the harness's note that the rest depends on
memory nobody wrote; in 284 the original stopped (a made-up record that asks for a read or write
of no memory, a string freed that a stand-in had answered); in 109 the original's outside calls
depend on memory nobody wrote (`SI_latticeCopyFromSetup` without an active lattice); in 80 the
port's two runs disagree with each other, which says its result depends on memory nobody wrote
where the harness's two runs happened not to show it (section 9.2).

**Which instructions the cases reached** is noted while the vectors are made (a block hook on
the routine; `dcov.py` sets the assembler's padding aside): 6,943 instructions in the 72
routines, 1 not reached (above). The count is of bytes in blocks the emulator reports, an upper
bound of what was reached; the answers to "no memory" share their instructions with the normal
exits, so the count cannot show that they were never taken (they were not: the harness's
allocator never fails).

### 9.2 What the checks found

**No port had to be mended**: on its first comparison every one of the 72 routines agreed with
the emulated original in every case compared, with the four cases of the second row below as
the one exception, and those are not the port's. The x87 reading of the listing (which operand an
`fsubrp` or `fdivrp` of the disassembler subtracts from which, where a value is rounded) held on
every case. What the checks did find was around the ports:

| Found by | Fault | Kind |
|---|---|---|
| seven cases of `klusterRemoveIndex` differing ("the port changed bytes outside every buffer") | the generator asked for a negative index. The original then writes **in front of** the array; the harness reports a write behind a buffer only, so the original's write was invisible and the port's was counted against it | a limit of the harness, met through the generator. The generator now puts the array inside a larger buffer for these cases (one point, so nothing is reallocated): the wild write is then seen on both sides and agrees |
| the harness stopping in half of `SI_latticeValue`'s first cases | made-up lattices with one point along an axis read a neighbour in front of the first buffer of the case: no memory | generator: a buffer in front, and such lattices made rare |
| the harness stopping in a quarter of `klusterAddToModel`'s cases | nodes without any stored definition | generator |
| `check_globals.py` | `SI_glbDuplic` declared as 0x60 bytes (the distance to the next symbol, as finding 0019 has it); the symbol table says 0x58 | a size taken from a note instead of the table. Fixed |
| four cases differing at full scale (`klusterMoveIndexes` 1 of 2,000, `SI_klusterMoveIndexes` 3 of 1,500): one word of a heap block, original 8, port 0 | the read one word behind a face's array (section 7a). In the emulator that word held what an earlier case's allocation had left there, the same in both of the harness's runs, so the harness could not mark it undefined; in the image heap memory beyond the last block is zero. Neither value is the original's "result": it is the next block's future header | the two-pattern method's known limit (finding 0011), met in the heap. `deform-emucheck` now gives heap memory no block covers the pattern as well; the port's two runs then disagree and the case is counted "not compared" (80 such cases in all). The port was not changed |
| three generators failing on their first run | a keyword passed twice, a point list of the wrong shape | tool |

## 10. Not ported, not checked

Nothing is written and unchecked: every function in the crate is cited, and
`re/tools/si-deform/cite.py` rewrites a citation into `` `name` (0xADDRESS); unchecked `` when
the check's report does not carry it.

Not ported (6 of 78), all behind the trait by the rule of the pass:

| Function | Bytes | What it is |
|---|---:|---|
| `SI_latticeDraw (0x0843f254)` | 708 | drawing: three nests of line strips (`bgnline`, `v3f`, `endline`), along x for every (y, z), along y, along z. After every point whose running number has the low byte 0xff it calls `bgnline`, sends the point again and calls `endline` *[guess: the old GL's limit of 256 points a strip]* |
| `SI_dialogModelSetCluster (0x0843b9ac)` | 1,132 | the "set cluster" dialog (resource 0x1a95) over the selected model nodes: refuses a node a lattice deforms, counts tagged points, asks for a name (default `cls<n>`), makes the cluster (`klusterAddToModel`) and, with the "centre" switch, a null object `cluster_ctr` at the cluster's centre constrained to it (relation 0xf7) |
| `klusterPrepareTextLine (0x0843be18)`, `klusterInfoUpdateItems (0x0843bf1c)`, `SI_dialogClusterInfo (0x0843c014)`, `WrathStayGhosted (0x0843d884)` | 260, 248, 2,496, 102 | the cluster info dialog: the list line of a cluster (`%-20s %c`, the name cut to 19 characters), the items for the chosen one, the dialog's loop, and greying six items out |

Not compared, of what is ported: the cases in which the original stopped (a made-up record
that asks for a read or write of no memory); the cases whose result depends on memory nobody
wrote (`which` in `SI_latticeCopyFromSetup` without an active lattice, a read in front of a
point array, the unset inverse in `SI_klusterCnsSetData`); the answers to "no memory", which the
harness's allocator never gives; signalling NaNs (finding 0011).

Not determined: what the word at +0x40 of a lattice and the words at +8 and +0x14 of a cluster
are for (no routine of these files reads them); whether interpolation codes other than 0 and 1
occur (the code treats every non-zero code as the B-spline).

## 11. The outside world

One trait, `si_deform::outside::Outside`: `call(m, &Routine, &[u32]) -> u32`, the callee by name
and address, the arguments as the 32-bit words the original pushes (a `short` as its 16 bits, a
`double` as two words), the answer what the original leaves in `eax`; and `drand48()`, whose
state is the C library's and therefore not in the image. It has the shape of `si-view`'s and of
`si_mem::Outside`'s proposal (finding 0024, 11.5). 65 routines are named, each written
`name (0xADDRESS)`: 51 that the ported code calls (the editing layer between a node and the
dictionary, the dictionaries, the model tree, mesh normals, the patch surface, the NURBS point
movers, the frames along a curve, cluster animation, sorting) and 14 of drawing and dialogs for
the six routines of section 10. `re/tools/si-deform/outside_table.py` is the one source of the
list: it prints the Rust block with every address looked up in the symbol table, and the
generators take their stand-ins from it.

## 12. Wanted in shared files

- **`si-types`**: `Lattice`, `SplFitting`, `PtchFitting`, `Kluster` are empty types there; the
  four `record!` layouts of `si-deform::records` can move in (then `Model`'s `lattices0/1`,
  `spline_fit`, `patch_fit`, `klusters` and the three user data clusters need no cast).
  `Model::unk_5c` is copied with the lattices on duplication. The geometry offsets of section
  2.4 belong with the mesh, spline and NURBS records when those are typed.
- **Duplicates to reconcile** (each ported again here from the listing and measured):
  `SI_modelLattice` (twice in `si-model`), `SI_modelNodeLattice`, `SI_modelTreeLattice`
  (`si-model`); `SI_klusterAllocate` (twice in `si-hrc`), `SI_clusterCount` (`si-hrc`).
  `si-view` names `SI_klusterGetActive`, `SI_klusterActivate` and `SI_patchRemoveFitting` in its
  trait and can call these ports; so can the callers in `model.c` (`SI_modelReadjustDefinition`
  calls `SI_latticeTransform`, `klusterCopyFromSetup` the two list routines) and `SI_tagClear`,
  `SI_tagAll` and the transformations, which ask `SI_isLatticeDeformed`.
- **`re/tools/si-mem/check_globals.py`**: `SI_identite` (a matrix in `.data` that is not zero)
  cannot be declared without a complaint: the `globals!` form takes scalar initial values only.
  Wanted: `("si-deform", "SI_IDENTITE")` in its `BUILT` table (as `si-hrc` needs for the same
  global), or an initial value for arrays. `re/tools/si-deform/check_data.py` holds the 64 bytes
  against the binary meanwhile.
- **`re/tools/emu`**: a write in front of a buffer is not reported (section 9.2); only a write
  up to 64 bytes behind one is. Heap memory beyond the last block is not refilled between cases
  and passes, so a read behind a heap block can meet the same stale bytes twice and pass for
  defined.
- **`si-mem`**: under a pattern fill, heap memory no block covers stays zero (and a page never
  touched counts as zero in a snapshot: finding 0024, 11.8). `deform-emucheck` writes the
  pattern there itself; `Mem` could.
- **Finding 0019** gives `SI_glbDuplic` as 0x60 bytes; the symbol's size is 0x58.

## 13. `deformation.c`: the touch after the animation (2026-10-14)

Of `deformation.c` (the deformation menus' commands and the touches that tell the update a
deformation changed) only what the render's update reaches is ported, in
`si_deform::deformation`:

- `SI_postAnimTouch(model, branch)` [0x08153618, 80 bytes]: calls `SI_modelReadjustDefinition`
  on the node; for a branch (the `short`'s 16 bits not 0) it then walks the subtree with
  `SI_modelNextSon(model, son)` from a null start and readjusts every node it answers, until
  null. It returns nothing and writes no memory of its own.

The two callees are new outside routines (`outside_table.py`'s `DEFORMATION` table, stand-ins
of `gen_deformation.py`'s session only, so the other generators run as before);
`check_outside.py`: 0 wrong. The render's update host routes them to si-model's ports
(finding 0023 §26).

**Check** (`gen_deformation.py`, new, in `check.sh`'s first generator process): 1,500 cases,
nodes with 0 to 20 sons, some of them made-up addresses, and the `short` as a stack word with
and without bits above its 16. `deform-emucheck`: **1,500 of 1,500 identical**.
`EMUCHECK_MUTATE=calls`: 1,500 different. `EMUCHECK_MUTATE=1` shows nothing: the routine
returns nothing and writes nothing, so what it does is its calls. One unit test.

## 14. Why the lattice scenes match now (2026-10-14)

Finding 0023 §26 left the six lattice scenes different. Before this change the loader
answered `SI_modelUpdateDependency` 0, so a lattice-deformed model never got a working
definition of its own: +0x48 was the stored definition. The load's `SI_sceneRefresh` ran
`SI_latticeTransform` on the stored points, and the render's update ran it again on points
the lattice had already moved.

With the routine routed (finding 0018 §22), each deformed model gets its own copy, and the
readjusting starts from the untouched stored points every time. That brought five of the
six scenes to the original's first frame: `lattice-deformation`,
`CopyGeometry-LatticeSphere`, `MODEL-IceCream`, `MOTION-FLEX2` and `MOTION-FLEX3`.

`MOTION-LatticeDeformation` also needed `SI_modelUpdateSonState`'s own call of
`SI_modelReadjustDefinition`, routed in the render's update (finding 0023 §27). That call
readjusts a node held by another node's branch deformation when its `def_matrix` changes.
While it was answered 0, the ball was drawn 25 px low; now 30 of 30 frames are identical.
Nothing in this crate changed for either fix.

## 15. Why `MOTION-FLEX3` still differs: not the lattice (2026-10-24)

`MOTION-FLEX3` had 27 of 30 frames different at 6086f5f. Measured with the pictures of the
earlier whole-frames run (`re/out/lead/nfaces`) and `motion-evalcheck --only FLEX3`
(`re/out/si-deform/flex3/base`):

- The lattice is right. A picture-difference map of frames 4 and 30 shows the same silhouette
  and shading in both; only the colour differs, in the green and blue planes (frame 4: 469
  pixels, red 2, green 469, blue 469, largest difference 2; frame 30: 2,215 pixels, largest
  48). Frames 1 to 3 are identical; the difference grows with the frame.
- `motion-evalcheck`: all 40 dumps of the original (frames 1, 8, 15, 22, 29) identical. Its
  list of objects holds models, not materials, so no dump differs.
- The scene's expressions (`EXPRESSIONS/MOTION-FLEX3.1-0.exp`, read as text) are five: a
  rotation, a scaling and a lattice node's rotation from `arg1.rotz`, and **`expr4` and
  `expr5`, which set a material's diffuse green and blue to `1.0 - arg1.rotz / 180`**. The
  update's tally has `SI_materialSetParam` (0x080f5964, `edparams.c`, 557 bytes, si-expr's
  row of the owed list in finding 0023) answered 0, twice a frame (12 calls over the five
  frames and the load): the material keeps its loaded colour. The original's sphere tints
  towards cyan as `rotz` grows.
- So the routine is `SI_materialSetParam`, in si-expr (its `SI_generalAttrGetValue` read-side
  callees `SI_modelAttrGetValue` 0x080f6dd0, `SI_materialsAttrGetValue` 0x080f9674 and
  `SI_dummyAttrGetValue` 0x080fabf0 are also answered 0, 12 times each; whether they matter
  here was not checked: the three other expressions, which read the same `rotz`, give the
  right lattice and scaling). Nothing in si-deform is involved; no code of this crate changed.
  Porting it (a 557-byte switch over the parameter number storing a float into the
  material, then `SI_materialSetDirty(material, 0x40)`) and routing it in the update is the
  step to take; the owed-list row of finding 0023 (the table of unported routines) names it.

## How to run

```bash
re/tools/si-deform/check.sh 1.0          # build, unit tests, vectors, check, both mutation runs, citations, globals, addresses, constants, the map, coverage
# or step by step:
export CARGO_TARGET_DIR=$PWD/target/si-deform
(ulimit -v 8000000; cargo test -p si-deform; cargo build --release -p si-deform)
E="uv run --with unicorn python3 -I"
(ulimit -v 6000000; timeout 3000 $E re/tools/si-deform/gen.py re/out/si-deform/vec 1.0)        # all; or names, or a module: gen_lattice gen_transform gen_fitting gen_klusters gen_kedit gen_deformation
(ulimit -v 2000000; timeout 1800 target/si-deform/release/deform-emucheck re/out/si-deform/vec)
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 1800 target/si-deform/release/deform-emucheck re/out/si-deform/vec)
(ulimit -v 2000000; EMUCHECK_MUTATE=calls timeout 1800 target/si-deform/release/deform-emucheck re/out/si-deform/vec)
python3 -I re/tools/si-deform/cite.py re/out/si-deform/check.txt     # citations follow the report
python3 -I re/tools/si-mem/check_globals.py si-deform
python3 -I re/tools/si-deform/check_outside.py                        # every address the crate names
python3 -I re/tools/si-deform/check_data.py                           # the constants taken from the binary's data
python3 -I re/tools/si-deform/dmap.py re/out/si-deform/check.txt      # the table of section 8
(ulimit -v 6000000; $E re/tools/si-deform/dcov.py -v)                 # instructions no case reached
python3 -I re/tools/si-deform/outside_table.py                        # the outside! block, addresses from the symbol table
python3 -I re/tools/si-deform/dfuncs.py                               # functions, sizes, callers
re/tools/si-deform/dl.sh SI_latticeValue                              # a compact instruction listing
```

## Leftovers pass, 2026-10-29 (unit misc2)

Remaining unported in si-deform: `klusters.c` dialogs (`SI_dialogModelSetCluster`, `SI_dialogClusterInfo`, `klusterPrepareTextLine`, `klusterInfoUpdateItems`, `WrathStayGhosted`), `SI_latticeDraw`, and `envtools.c` `SI_envJntAssBoundCreate/Draw`, `SI_envJntAreaLimDraw` (caller `si_envJntAssBound` is a skin dialog). All dialogs or drawing; skipped as not render. Lattice and envelope evaluation are ported. No code added.
