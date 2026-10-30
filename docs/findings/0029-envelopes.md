# 0029 — Envelopes: the skin a skeleton deforms (two passes)

Date: 2026-10-13 (first pass); second pass 2026-10-14, sections 13 to 16; third pass (the
envelopes of a loaded scene, wired into the load and the update) 2026-10-14, section 17.
Binary: Linux `soft`,
sha256 `ef21a382c3325d42…`. Package: `crates/si-deform`
(modules added beside the lattice package of finding 0028), `re/tools/si-deform/`, scratch in
`re/out/si-deform/env/`. Source files of the original: `envelop.c` (0x084b1bc0, 24 functions,
24,215 bytes), `envelopset.c` (0x084b7a90, 28 functions, 15,597 bytes), `envtools.c` (0x084bb7b0,
39 functions, 11,230 bytes): 91 functions, about 50 KB (sizes counted to the next symbol).

How to read this note: a statement about the original is **read from code** (the instruction
listing of every function ported; the decompiled text was not used) and, for every function with
case counts in section 8, **measured**: the function was stepped through in the emulation harness
(finding 0011) on made-up scenes and the port compared with what it left in memory. *[name]*
marks a meaning taken from a symbol, *[skim]* a description from a quick reading of a listing
that was not ported, *[guess]* a guess.

## Results at a glance

Fourth pass (section 18, 2026-10-28): 12 more functions (the earlier envelope format, parts, display colours) cited, 0 different in 19,800 cases.
Fifth pass (section 19, 2026-10-28, deform3): `SI_envelopUpdateInitDef` and `SI_envelopCut` ported and cited (4,000 and 3,000 cases, 0 different); the scene images dumped again, `SI_envelopRebuild`, `envelopDataConvert`, `SI_envelopRelationSet` re-compared (120 cases, 0 different). Every function of the crate except the nine interactive or drawing ones is now ported.

The first pass's, as they stood after it. The second pass's are in section 13: 68 of the 91
functions on the image, checked and cited, and one of `iktools.c`.

| | |
|---|---|
| Functions mapped | all 91: what each does, its callers, its kind (section 8) |
| On the image, checked and cited | **48 of 91**, 23,173 bytes of original code: 19 of `envelop.c` (everything the scene update reaches), 22 of `envtools.c`, 7 of `envelopset.c` |
| Left for the second pass | 43 (section 10), 4 of them drawing, 1 a dialog request |
| Emulation, `deform-emucheck` on `env/vec` | 48 routines, **75,640 cases: 49,607 identical, 25,705 identical given the C library's `sin`, `cos`, `pow`, `asin`, `acos`, 0 different**, 328 not compared |
| Mutation switches | `EMUCHECK_MUTATE=1`: 63,817 cases differ; the 11,523 that stay identical return nothing and change no memory. `EMUCHECK_MUTATE=calls`: 12,875 differ, the cases in which the port calls out with an argument |
| Instructions the cases reached | 6,674 instructions in the 48 routines; **2 not reached**: `SI_envelopInfoAllocate`'s answer to no memory and a jump of `envelopSkel` for skin kinds that `dampTabCreate` has already refused |
| The lattice package's vectors, re-run | `deform-emucheck re/out/si-deform/vec` after all changes: **85,176 cases, 77,144 identical, 6,869 given the library, 0 different, 1,163 not compared**, finding 0028's numbers |
| Globals declared and held against the binary | 19 in the crate, 10 of them new (`check_globals.py si-deform`: 1 complaint, the known false alarm about `SI_identite`, finding 0028 section 12) |
| Outside routines named, held against the symbol table | 80 in the crate, 15 of them new (`check_outside.py`: 0 wrong, with all 120 citations and 98 mentions of an address) |
| Constants taken from the binary's data | 28 new (`check_env_data.py`: 0 wrong) |
| Unit tests | 3 new (made-up data), 6 in the crate |

No routine of the three files ported here tests a licence or product-tier flag.

## 1. The modules, and how the two packages are kept apart

| Module | Functions | Content |
|---|---:|---|
| `env_records` | | the six records as `record!` layouts (section 2), the envelope words |
| `env_globals` | | nine of the ten globals of section 2.5 (`totwght` is declared in `envelopset`), each block with its own `init` |
| `env_outside` | | the 15 outside routines the new code calls, by name and address (one more `outside_env!` block printed by `env_outside_table.py`); the trait is the crate's one `outside::Outside` |
| `envtools` | 22 | the records' allocation, copies and disposal; the questions asked of a node (level, skeleton root, local skin, main global node, active); the working definition; the update marks |
| `envelop` | 19 | the deformation: rest matrices, the walk over a skeleton, the two ways a bone moves its points, the bulge profiles, rigidly carried nodes |
| `envelopset` | 7 | where a point lies along a bone (`compRatio`), the assignment's two lists, one weight changed and the others balanced |

Every function takes the image and the original's arguments; `SI_malloc`, `SI_calloc` and
`free` are made with the original's sizes in the original's order. Records are `record!`
layouts of this crate (`si-types` has `EnvSkelJnt` as an empty type and nothing for the
others); the geometry of a skin is addressed by the named offsets of `records::geom` (finding
0028) and a few more in `envelop` (polygon count +0x18, polygon flag word +0x14); the model
node is `si_types::model::Model`. The ten routines of `algebra.c` the files call are `si-math`'s
ports.

**Kept apart from the lattice package.** Nothing of the lattice package's sources was changed
beyond additions: `lib.rs` gained the module lines, the check binary a second dispatch table
(`table_env.rs`, consulted when `table.rs` has no row) and the calls of the new `init`s,
`gen.py` three module names and the rule that a module with stand-ins of its own (`EXTRA`) gets
an emulator session of its own (so the lattice modules' cases are made exactly as before). The
envelope vectors, coverage, reports and logs live in `re/out/si-deform/env/`; the lattice
package's `re/out/si-deform/vec/` was not regenerated, and `check_env.sh` re-runs the check
binary on it and holds the totals against finding 0028's (section 9). `cite.py` is given both
reports together, so that neither package's citations are judged by the other's.

## 2. Records

### 2.1 The bone's record (`__SI_EnvSkelJnt`, 0xa0 bytes from `SI_malloc`), at `Model` +0x324

| Offset | Field | As the code uses it |
|---|---|---|
| +0x00 | level | depth below the skeleton's root: 0 for the root; −1 at creation and for a node taken off. Every walk compares it with the depth it expects and passes over a node that does not match, sons included |
| +0x04 | lists | one list per skin this bone moves points of (2.2), chained both ways |
| +0x08 | rest inverse | the inverse of the bone's matrix in the skeleton's rest position (section 4); identity at creation |
| +0x48, +0x54, +0x60 | three vectors | 0 at creation; `envAutoSet` parks the rest rotation at +0x54 |
| +0x6c | blend length | how far along the bone, in bone lengths, the bend around the joint reaches; clamped to 0 … 0.5 where used |
| +0x70 | inner profile | `SI_EnvDefCrvPrm` (2.3), the side the joint bends toward |
| +0x88 | outer profile | the same for the other side |

### 2.2 Lists and vertex entries

`envlist` (0x10): +0 the skin's node, +4 its vertex entries, +8 previous list, +0xc next list.
`envvert` (0x14): +0 the point's number in the skin's geometry, +4 its place along the bone (in
bone lengths, from `compRatio`), +8 its distance from the bone, +0xc the bone's weight on the point
in percent (100 at creation), +0x10 the next entry. **"Which joints weigh on a vertex" is not
stored per vertex**: each bone has, per skin, the list of the points it moves with their weights,
and a point's bones are found by walking all bones' lists for the skin.

### 2.3 The deformation profile (`SI_EnvDefCrvPrm`, 0x18, passed by value)

+0 kind (0 none, 1 exponential, 2 cosine), +4 a drawn table of 100 pairs (position, value) from
`SI_malloc(0x320)` or null, +8 and +0xc the range of positions it acts on, +0x10 an amplitude
(nothing below 0.0001), +0x14 a scale. 0, null, 0, 1, 0, 1 at creation.

### 2.4 Other records and node fields

- `editvert` element (0x4c): +0 point number, +4 summed weight (float), +8 a matrix (the weighted
  sum of the bones' matrices for the point), +0x48 next. Built only while points are edited by
  hand (section 3.5).
- The rest transformation at `Model` +0x328: scaling, rotation (three angles), translation.
- `Model` +0x31c, the **envelope word** (names ours): 4 a chain's first joint whose chain root
  carries the lists (local skeleton) or a global skeleton's root; 5 a local skin (a son of the
  joint whose chain deforms it); 6 a global skin (the skeleton is among its sons); 7 a node
  carried rigidly by one bone; 8 a local skin made automatically (its chain's rest rotations are
  parked during the update); 9 any other bone; 10 a global skeleton's root; 11 the root of a
  local skeleton that is no chain.
- `Model` +0x08 (16 bits): 0x0002 root seen, 0x0004 rest matrices made for this frame, 0x4000
  bone moved since the last reset. `Model` +0x0c not 0: the skin is not deformed. `Model`
  +0x32c: nine floats, the bone's pose (scaling, angles, translation) at the last reset.

### 2.5 Globals (new)

| Global | Address | Use |
|---|---|---|
| `SI_glbAnim` | 0x08781480 | +0x06 inside an animation step; +0x2e asked before bending around a chain's first joint; +0x38 a sequence plays |
| `SI_glbManip` | 0x08781560 | +0 classical scaling (1); +0x20 "keep whole turns" for the angle extraction |
| `SI_viewerUpdOpt` | 0x08679678 | bit 1 set around the update of a chain's joints |
| `SI_interactive`, `SI_fastIntenv` | 0x0867953e, 0x0867953c | a patch skin is not tessellated while dragging |
| `SI_mdlPnt` | 0x0878897c | the point under the cursor while editing |
| `editvert`, `damptab`, `curvect` | 0x086d72fc, 0x086d7300, 0x086d7304 | `envelop.c`'s statics (sections 3.5, 3.1, 3.4) |
| `totwght` | 0x086d7328 | `envelopset.c`'s running sum of a point's weights |

## 3. How a skin vertex is moved

### 3.1 The call tree of a frame

`animEnvelopUpdate` (`motionUpdate.c`) asks `SI_envSkelSetFlag` and `SI_envelopNeedUpdate`
(section 5) and calls **`SI_envelopCompUpdate(skin)`**:

1. `SI_envelopSkelRoot(skin)`: the skeleton's root (a local skin: the first father that is not a
   skin; a global skin: a son of level 0 with word 4 or 10, searched upwards).
2. `SI_envelopSkelUpdate(root)` (section 4), unless a sequence plays and the root is marked done
   for this frame.
3. **`envelopSkel(skin, root, 0)`**: `damptab` (one word per point, 0) is made; the skin gets a
   working definition copied from its first stored one (`SI_envelopCopyDef`); `back` = inverse of
   the skin's global matrix; `envelopChain` walks the skeleton from the root at level 0 (and its
   brothers for a global skin). For each bone of the right level: a joint moves its points with
   `envelopJoint`, any other bone with `envelopSkelMdl` (a joint without blend takes that path too
   while a sequence plays). Afterwards points no bone moved are copied back from the first stored
   definition (`unassVertCopy`), the mesh's normals are recomputed where points moved (or the patch
   tessellated, or the NURBS marked), `SI_modelTouchDefinition`, `damptab` freed.
4. A local skin's parked rest rotations are put back (`envAutoSet`) and the root's matrices
   rebuilt; a rigid node's matrices are rebuilt.

### 3.2 One bone, one point: `envelopSkelMdl`

For each entry of the bone's list for this skin with weight ≥ 0.01: the point p is read **from
the first stored definition** (the rest shape; a NURBS point divided by its weight, in doubles,
rounded to float). M = skin's global matrix × the bone's rest inverse × the bone's global matrix ×
`back` (three `SI_matrixMultiply`, each rounding to float). p′ = p × M, per coordinate
float(((y·M1j + x·M0j) + z·M2j) + M3j) for x, the z term added to the sum of the other two for y
and z (one rounding each). With weight ≥ 99.99 the point becomes p′; otherwise
**float(float((p′ − p) × (w × 0.01)) + base)** per coordinate, the difference and the share in
the register, where base is the point as
another bone already left it in this pass (its `damptab` word set) or else p. So a point's final
place is p + Σ wᵢ/100 · (p × Mᵢ − p) over its bones in walk order: **linear blending of the bones'
transformations, applied as a sum of displacements**, and a weight of 99.99 or more replaces
whatever earlier bones did. Then `damptab` = 1. A spline key of type 1 also moves its two tangent
points by M whole, whatever the weight; a NURBS point goes through `SI_nurbsCrvMovePoint` or
`SI_nurbsSrfMovePoint` (with the working point's own weight). A patch point's fourth word becomes
1.0. A mesh vertex moved for the first time marks the polygons of its star stale (bit 0x400), and
`envelopMeshCompNormals` recomputes those polygons' normals and their vertices' corner normals.

### 3.3 A joint: `envelopJoint`

The same fetch and store, with M built per point:

- A, B: the joint's and its son's blend lengths (0 … 0.5); a, a′: the joint's and its son's turn
  about z away from rest (present z angle minus the rest one, `fprem` by 2π, rounded to double
  then float, moved into −π … π); J, J′: the bone's global rotation matrix (with the global
  translation in its last row) turned back by −a (−a′) about z.
- r = the point's place along the bone (from its entry, +4). q = p in the joint's rest frame
  (skin matrix × rest inverse, then the same product written out as in 3.2).
- **Near the joint** (A > 0.0001, r ≤ A): t = float(1 − r/A) at most 1; the bone's matrix is J
  turned by float(a − a·0.5·t) about the bone's z axis through q's place along it (translation x
  plus q.x before, minus after), rows scaled by the joint's global scaling. At the joint the skin
  turns half the joint's angle, at r = A all of it. (Not for a chain's first joint when
  `SI_glbAnim` +0x2e is 0, the word is 4, or classical scaling is on.)
- **Near the next joint** (r ≥ 1 − B): t = float(1 − float((1 − r)/B)), q taken with the son's
  rest inverse, J′ turned by float(0.5·a′·t), scaled by the son's scaling: the skin starts into
  the next turn. Not when the son is the effector.
- **Bulge**: for 0 < r < 1 `compDef` answers a sideways displacement, added to the y
  translation of the rest-frame matrix: the profile on the bend's inner or outer side (chosen by
  the sign of sin(a′) against the point's side), yDef(profile, place in the profile's range,
  float(p·(1 − q)·0.15 + q)) × float(|side| / √(along² + side²)) × float(amplitude × …) × scale ×
  side, with p, q the two angles as fractions of a quarter turn. `yDef` is a drawn table (nearest
  of 100 samples, no interpolation), an exponential profile (`pow`, split at 0.6) or a cosine one
  (split at 0.5).
- M = rest-frame matrix × (bent matrix, or the joint's global matrix) × `back`.

### 3.4 Rigid nodes (word 7)

`envelopSkel` gathers in `curvect` the node's scaling, rest angles and rest translation; each bone
with a list for the node gives its pose (`envelopJntAxisMap`: the node's rest matrix through the
bone's rest inverse and present matrix, decomposed with `SI_matrixExtractTransformations`), all of
it at weight ≥ 99.99, else (new − rest) × weight/100 added to rotation and translation. The node's
rotation matrix, angles (`SI_matrixExtractXyzRotation`) and translation are then set relative to
its father.

### 3.5 While points are edited by hand

With `edit` set the working definition is left alone: only tagged points and the point under the
cursor take part, and each bone's M × weight/100 is summed per point in the `editvert` chain
(`SI_envelopUpdateInitDef`, second pass, uses it to carry an edited point back to rest).

## 4. Rest matrices: `SI_envelopSkelUpdate`, `envUpdateMatrix`, `envUpdateJointState`

A bone's rest matrix is its local rest matrix times its father's rest matrix; the local one is,
for a joint or effector, the rotation of its rest angles (identity for word 4) with **its present
translation**; for any other bone the rest rotation with rows scaled by the rest scaling and the
rest translation. Its inverse (`SI_matrixInverse`; left as it was when there is none) goes to the
record. The walk follows first sons one level down and brothers at the same level. The root of a
global skeleton gets its rest matrix from the skeleton's father as it stands now (rotation × the
father's global rotation, rows scaled by rest × the father's scaling, rest translation through
the father's global matrix; a rigid father's own rest transformation instead); a chain whose first
joint has word 4 is solved first (`SI_modelSpecialUpdateSonState`) and the first joint's own
global matrix is the father's. While a sequence plays the root is marked (bit 2) so that the rest
matrices are made once a frame.

## 5. The update marks

`SI_envSkelResetFlag(dictionary)` clears bits 1, 2 and 0x4000 of every bone in the models'
chapter and notes its pose at +0x32c (`SI_calloc(1, 0x24)` when missing). `SI_envSkelSetFlag`
marks (0x4000) a bone whose scaling, angles or translation moved by 0.0001 or more since then, and
everything below it; `SI_envelopNeedUpdate` answers 1 when a marked bone has points of the skin.
`SI_envelopCompensate` adds a vector to the rest translation of the skeletons under a node.

## 6. Assignment and weights (as far as this pass goes)

(The second pass ported and measured the assignment and the weight editing: section 14
replaces the *[skim]* below.)

- **`compRatio(point, start, end, &dist)`** (ported): t = float(((point − start)·u) / |u|²) with
  u = end − start rounded to float, the place of the point along the bone (not clamped); dist =
  the distance to the nearest point of the segment. A bone shorter than 0.01 (|u|² < 0.0001):
  dist = |point − start|, t = 0.
- **`SI_envelopModifyWeight`, `SI_envelopVertWeightBalance`** (ported): one bone's weight on a
  point is set, added to, or raised by a percentage, clamped to 0 … 100, while the other bones'
  entries are collected and the sum kept in `totwght`; then, if the sum exceeds 100, each other
  weight w becomes float(w − w × float((sum − 100)/(sum − new))).
- *[skim]* **`envToJntAssign`** (not ported): for a point and each bone of a list, `compRatio`;
  a distance not above 0 becomes 0.0001; `vrtInJntBound` tests the bone's bounding volume; a bone
  whose volume holds the point gets an entry with a weight from `pow(distance, 4)` and its inverse
  is summed (inverse fourth-power weighting); otherwise the bones are kept sorted by distance and
  the nearest gets an entry of weight 100. `envAssign` runs it for every point of a skin and makes
  the dictionary relation; `SI_envelopSkelAssign`, `SI_envelopReassign`, `SI_envelopAddNewParts`
  are the commands around it.

## 7. Behaviour of the original worth knowing

Read from the listings; each is reproduced and reached by cases unless it says otherwise.

- **The rest shape is always the first stored definition**, and a point no bone moves is copied
  back from it every frame (`unassVertCopy`).
- **Weights are not normalised by the deformation**: entries are applied in walk order as
  displacement shares; a weight ≥ 99.99 discards earlier bones' work on the point; weights below
  0.01 are ignored.
- **A joint's rest matrix uses the joint's present translation**, not its rest translation
  (`envUpdateJointState`).
- **`SI_envelopInfoDispose` frees only the first list record** of a bone; the vertex entries of
  all lists are freed. `envelopInfoTreeDispose` without a skin does the same.
- **`envelopInfoCopy` gives the copied lists no skin** (their first word is 0).
- **`envelopSkel` with a skin matrix that has no inverse** uses a matrix local nobody set; in the
  emulator those words are stack addresses, i.e. NaNs, and the skin's points become NaN. The port
  fills that local with NaNs carrying the image's fill (`unset_matrix`), so the cases that depend
  on it are counted "not compared".
- **A spline whose working definition is of type 1 while its first stored one is not** gets its
  tangent points from locals the original never set (the port writes zeros there; no case asks).
- **`envAutoSet` and `envelopChain` follow first sons and joints without tests for null**; a
  chain whose walk meets a node without record reads through null (the cases stop there).
- **`envelopJoint` uses the son's blend only when the son has a record**, and reads the son's
  global matrices whether or not.

## 8. The map of the three files

Columns: size in bytes to the next symbol; kind; callers (the file's own first); cases /
identical / identical given the library / different (the rest of a routine's cases were not
compared).

| Function | Address | Size | Kind | What it does | Callers | Check |
|---|---|---:|---|---|---|---|
| `editVertAllocate` | 0x084b1bc0 | 68 | record keeping | a new element of the chain of matrices gathered while points are edited | envelopJoint, envelopSkelMdl | 4 / 4 / 0 / 0 |
| `editVertDispose` | 0x084b1c04 | 60 | record keeping | frees that chain | SI_envelopUpdateInitDef | 200 / 200 / 0 / 0 |
| `dampTabCreate` | 0x084b1c40 | 164 | record keeping | the table of one word per skin point: moved yet in this pass or not | SI_envelopUpdateInitDef, envelopSkel | 800 / 800 / 0 / 0 |
| `unassVertCopy` | 0x084b1ce4 | 568 | record keeping | puts the points no bone moved back to their rest place | envelopSkel | 1500 / 1500 / 0 / 0 |
| `envelopMeshCompNormals` | 0x084b1f1c | 208 | record keeping | recomputes the normals of the polygons and corners whose points moved | envelopSkel | 1500 / 1500 / 0 / 0 |
| `envUpdateJointState` | 0x084b1fec | 304 | computation | a bone's rest matrix from its father's, and its inverse into the record | envUpdateMatrix | 2000 / 217 / 1783 / 0 |
| `envUpdateMatrix` | 0x084b211c | 124 | computation | the rest matrices of a branch of bones, level by level | SI_envelopSkelUpdate, envUpdateMatrix | 2000 / 493 / 1501 / 0 |
| `envAutoSet` | 0x084b2198 | 268 | record keeping | parks or restores the rest rotations of a chain under an automatic local envelope | SI_envelopCompUpdate, SI_envelopHierUpdate, SI_envelopSkelUpdate, SI_envelopUpdateInitDef | 1500 / 1446 / 0 / 0 |
| `expFct` | 0x084b22a4 | 88 | computation | power with the profile's guards (0 for a base not above 0, 1 for an exponent not above 0) | yDef | 1200 / 427 / 773 / 0 |
| `yDef` | 0x084b22fc | 340 | computation | the value of a deformation profile: drawn table, exponential or cosine | compDef | 2500 / 1534 / 966 / 0 |
| `compDef` | 0x084b2450 | 636 | computation | how far a point is pushed sideways by a bending joint (the bulge or crease) | envelopJoint | 3000 / 268 / 2732 / 0 |
| `envelopJoint` | 0x084b26cc | 6960 | computation | moves the points of a joint's list: blended rotation near the joint and the next, plus the bulge | envelopChain | 6000 / 0 / 5993 / 0 |
| `envelopSkelMdl` | 0x084b41fc | 4796 | computation | moves the points of a bone's list by the bone's matrix and the weight | envelopChain | 4000 / 3994 / 2 / 0 |
| `envelopJntAxisMap` | 0x084b54b8 | 624 | computation | the pose a bone gives a node it carries rigidly | envelopChain | 2000 / 650 / 1350 / 0 |
| `envelopChain` | 0x084b5728 | 364 | computation | walks a skeleton and lets each bone of the right level move its points | envelopChain, envelopSkel | 2500 / 1036 / 1456 / 0 |
| `envelopSkel` | 0x084b5894 | 864 | computation | deforms one skin by its skeleton: working definition, walk, normals or surface, clean-up | SI_envelopCompUpdate, SI_envelopHierUpdate, SI_envelopUpdateInitDef | 3000 / 1249 / 1737 / 0 |
| `SI_envelopSkelUpdate` | 0x084b5bf4 | 1564 | computation | the rest matrices of a whole skeleton, under its father as it stands | SI_envelopCompUpdate, SI_envelopHierUpdate, SI_envelopUpdateInitDef | 3000 / 313 / 2687 / 0 |
| `SI_envelopCompUpdate` | 0x084b6210 | 192 | computation | the entry of the scene update: rest matrices, then the skin | SI_envelopReassign, SI_modelSetCurrentDefinition, SI_modelSubstituteIDefinition, SI_modelSwapGeometry and 8 more | 3000 / 197 / 2788 / 0 |
| `SI_envelopHierUpdate` | 0x084b62d0 | 260 | computation | deforms every skin of a skeleton's root | SI_dynamicSimulationCompute, SI_envelopReassign, SI_envelopRebuild, si_dialogEnvJntArea and 4 more | 2500 / 583 / 1914 / 0 |
| `oldenvelopJoint` | 0x084b63d4 | 1412 | computation | the deformation of the earlier envelope format *[name]* | oldenvelopChain | second pass |
| `oldenvelopChain` | 0x084b6958 | 140 | computation | its walk *[name]* | oldenvelopChain, si_oldenvelopReset | second pass |
| `si_oldenvelopReset` | 0x084b69e4 | 192 | computation | resets a skin of the earlier format through it *[name]* | SI_envelopRebuild | second pass |
| `envVrtResetEditFlag` | 0x084b6aa4 | 248 | record keeping | clears the marks of the points edited by hand *[name]* | SI_envelopUpdateInitDef | second pass |
| `SI_envelopUpdateInitDef` | 0x084b6b9c | 3814 | computation | moves an edited point's rest position back through the gathered matrices *[name]* | SI_dragPoint, SI_polyTransform, si_editPointCoordinates, si_nurbsEditWeight and 3 more | second pass |
| `skelJntAllocate` | 0x084b7a90 | 56 | record keeping | a new element of a list of bones used while assigning *[name]* | envToJntAssign, envelopAssignSkelSet, envelopSkelBuild | 4 / 4 / 0 / 0 |
| `skelJntDispose` | 0x084b7ac8 | 64 | record keeping | frees such a list *[name]* | SI_envelopAddNewParts, SI_envelopAddNewSkelParts, SI_envelopReassign, SI_envelopSkelAssign and 1 more | 300 / 300 / 0 / 0 |
| `vrtlstAllocate` | 0x084b7b08 | 36 | record keeping | a new element of a list of vertices *[name]* | SI_envelopModifyWeight | 4 / 4 / 0 / 0 |
| `vrtlstDispose` | 0x084b7b2c | 60 | record keeping | frees such a list *[name]* | SI_envelopVertWeightBalance | 300 / 300 / 0 / 0 |
| `vrtInJntBound` | 0x084b7b68 | 3500 | computation | whether a point lies inside a bone's bounding volume *[name]* | envToJntAssign | second pass |
| `compRatio` | 0x084b8914 | 436 | computation | where a point lies along a bone: place and distance *[name]* | SI_envelopVertDistMinMaxCompute, SI_envelopVertWeightUpdate, SI_envelopWeightCreateReference, envToJntAssign | 3000 / 3000 / 0 / 0 |
| `envToJntAssign` | 0x084b8ac8 | 1516 | computation | assigns one point to the bones nearest to it, with weights *[name]* | envAssign | second pass |
| `envAssign` | 0x084b90b4 | 1668 | computation | assigns every point of a skin, local or global mode *[name]* | SI_envelopAddNewParts, SI_envelopReassign, SI_envelopSkelAssign | second pass |
| `envelopListBuild` | 0x084b9738 | 272 | record keeping | the lists of skins a skeleton gets *[name]* | SI_envelopAddNewParts, SI_envelopSkelAssign, envelopListBuild | second pass |
| `envelopSkelBuild` | 0x084b9848 | 400 | record keeping | the list of bones of a skeleton *[name]* | SI_envelopAddNewParts, SI_envelopReassign, envelopSkelBuild | second pass |
| `SI_envelopSkelSet` | 0x084b99d8 | 376 | record keeping | makes a branch a skeleton: records and words *[name]* | SI_envelopAddNewSkelParts, SI_envelopSkelSet, si_envelopSkeletonSet | second pass |
| `SI_envelopSkelReset` | 0x084b9b50 | 236 | record keeping | takes the skeleton records off a branch *[name]* | SI_envelopSkelReset, si_envelopSkeletonSet | second pass |
| `envelopAssignSkelSet` | 0x084b9c3c | 948 | record keeping | gives a branch's bones their records and lists for a skin *[name]* | SI_envelopAddNewSkelParts, SI_envelopSkelAssign, envelopAssignSkelSet | second pass |
| `SI_envelopSkelAssign` | 0x084b9ff0 | 408 | record keeping | makes a node a skin of a skeleton and assigns its points *[name]* | si_autoEnvelopSet, si_chainAttach, si_envelopeAttach | second pass |
| `SI_envelopReassign` | 0x084ba188 | 492 | record keeping | assigns a skin's points anew *[name]* | SI_envelopNewVertAssign, si_envelopReassignAuto | second pass |
| `SI_envelopVertWeightBalance` | 0x084ba374 | 116 | computation | brings a point's weights to a sum of 100 *[name]* | SI_envelopVertWeightUpdate, SI_envelopWeightUpdate | 1500 / 1500 / 0 / 0 |
| `vertWeightUpdate` | 0x084ba3e8 | 436 | computation | changes one point's weight on the bones of a branch *[name]* | SI_envelopVertWeightUpdate, SI_envelopWeightUpdate, vertWeightUpdate | second pass |
| `SI_envelopWeightCreateReference` | 0x084ba59c | 200 | record keeping | a vertex entry for a point a bone did not have *[name]* | vertWeightUpdate | second pass |
| `SI_envelopVertWeightUpdate` | 0x084ba664 | 596 | computation | sets a point's weight on one bone and balances the others *[name]* | envelopWghtUpdate, si_envelopReassignMan | second pass |
| `SI_envelopWeightUpdate` | 0x084ba8b8 | 120 | computation | the same for many points *[name]* | envelopWghtUpdate, si_envelopReassignMan | second pass |
| `SI_envelopVertDistMinMaxCompute` | 0x084ba930 | 364 | computation | the nearest and farthest distance of a skin's points from a bone *[name]* | envelopWghtUpdate | second pass |
| `envPrecInListFind` | 0x084baa9c | 128 | record keeping | the list of a skin in a branch *[name]* | SI_envelopAddNewParts, envPrecInListFind | second pass |
| `SI_envelopAddNewParts` | 0x084bab1c | 632 | record keeping | assigns the points of parts added to a skin *[name]* | SI_doFather, SI_duplicAndConnectGrafnode | second pass |
| `SI_envelopAddNewSkelParts` | 0x084bad94 | 464 | dialog | adds bones to a skeleton, asking the user first *[name]* | SI_doFather | second pass |
| `SI_envelopNewVertAssign` | 0x084baf64 | 44 | record keeping | assigns new points of a skin after a modelling command *[name]* | SI_modelConvertForSumatra, SI_nurbsSrfAddRowCol, SI_nurbsSubdivision, convertPatchToNURBS and 7 more | second pass |
| `SI_envelopVrtColSet` | 0x084baf90 | 1024 | drawing | colours a skin's vertices by their bones for display *[name]* | SI_envelopVertWeightUpdate, envAssign, jntAssgnVrtColSet, si_dialogEnvWeight | second pass |
| `SI_envelopVrtColGet` | 0x084bb390 | 860 | drawing | the display colour of a vertex by its bones *[name]* | meshFunc, nurbsCrvFunc, nurbsSrfFunc, patchFunc and 2 more | second pass |
| `SI_envelopModifyWeight` | 0x084bb6ec | 189 | computation | changes one vertex entry's weight and notes it *[name]* | vertWeightUpdate | 2000 / 2000 / 0 / 0 |
| `SI_envelopLevel` | 0x084bb7b0 | 36 | record keeping | a node's level in its skeleton, -1 for none | SI_envSkelResetFlag, SI_envSkelSetFlag, SI_envelopCopy, SI_envelopCut and 28 more | 600 / 600 / 0 / 0 |
| `SI_envGetRootFromEnvElem` | 0x084bb7d4 | 140 | record keeping | the top node of the envelope a skin belongs to | si_dialogEnvWeight, updateJntList | 1500 / 1500 / 0 / 0 |
| `SI_envLocalGet` | 0x084bb860 | 80 | record keeping | the local skin under a chain's first joint | SI_envelopRebuild, SI_envelopSkelUpdate, envAutoSet, si_envelopReassignAuto and 1 more | 1500 / 1338 / 0 / 0 |
| `SI_skelRootGet` | 0x084bb8b0 | 156 | record keeping | the root of the skeleton a bone belongs to | SI_envelopAddNewSkelParts, SI_envelopHierUpdate, SI_envelopSkelUpdate, si_envelopReassignMan and 1 more | 2000 / 1951 / 0 / 0 |
| `SI_mainGlobEnv` | 0x084bb94c | 104 | record keeping | whether a node is the main node of a global envelope | SI_envGetRootFromEnvElem, SI_envelopCopy, SI_envelopCut, SI_envelopRebuild and 14 more | 1500 / 1500 / 0 / 0 |
| `SI_envelopActive` | 0x084bb9b4 | 116 | record keeping | whether a skin's envelope is switched on (a relation in the dictionary) | SI_dragPoint, SI_isModelDeformed, SI_modelReadjustDefinition, SI_polyTransform and 8 more | 800 / 800 / 0 / 0 |
| `SI_envelopVertAllocate` | 0x084bba28 | 56 | record keeping | a new vertex entry, weight 100 | SI_envelopVertCopy, oEnvelopVertReadBinaryChunk, SI_envelopWeightCreateReference, envToJntAssign and 1 more | 4 / 4 / 0 / 0 |
| `SI_envelopListAllocate` | 0x084bba60 | 52 | record keeping | a new list of a skin's points for a bone | SI_oEnvelopReadBinaryChunk, envelopDataConvert, envelopInfoCopy, envelopRebuildList and 4 more | 20 / 20 / 0 / 0 |
| `SI_envelopInfoAllocate` | 0x084bba94 | 264 | record keeping | a new bone record | SI_oEnvelopReadBinaryChunk, envelopDataConvert, envelopInfoCopy, SI_envelopSkelSet and 2 more | 4 / 4 / 0 / 0 |
| `SI_envelopVertCopy` | 0x084bbb9c | 96 | record keeping | a copy of a chain of vertex entries | envelopInfoCopy, wghtListCreate, wghtListRestore | 600 / 600 / 0 / 0 |
| `envelopInfoCopy` | 0x084bbbfc | 384 | record keeping | a copy of a bone record, with or without its lists | SI_envelopCopy | 1200 / 1200 / 0 / 0 |
| `oEnvelopVertReadBinaryChunk` | 0x084bbd7c | 172 | record keeping | reads vertex entries of the earlier file format *[name]* | SI_oEnvelopReadBinaryChunk | second pass |
| `SI_oEnvelopReadBinaryChunk` | 0x084bbe28 | 212 | record keeping | reads a bone record of the earlier file format *[name]* | modelBinaryReadNode, modelBinaryReadNodeLite | second pass |
| `si_envelopVertListDispose` | 0x084bbefc | 60 | record keeping | frees a chain of vertex entries | SI_envelopInfoDispose, envelopDispose, envelopInfoTreeDispose, SI_envelopReassign and 1 more | 300 / 300 / 0 / 0 |
| `envelopDispose` | 0x084bbf38 | 84 | record keeping | takes one list out of a bone's chain and frees it | envelopInfoTreeDispose | 800 / 800 / 0 / 0 |
| `SI_envelopInfoDispose` | 0x084bbf8c | 176 | record keeping | frees a bone record (its first list only) | SI_envelopSkelReset, SI_modelDisposeNode, modelBinaryReadNode, modelBinaryReadNodeLite | 1000 / 1000 / 0 / 0 |
| `envelopInfoTreeDispose` | 0x084bc03c | 232 | record keeping | takes a skin, or all, off the bones of a branch | SI_envelopCut, SI_envelopRelax, envelopInfoTreeDispose | 2000 / 1994 / 0 / 0 |
| `SI_envelopSkelRoot` | 0x084bc124 | 288 | record keeping | the root of the skeleton that deforms a skin | SI_envelopRebuild, SI_envelopRelax, SI_envelopTmpReset, SI_envelopAddNewParts and 13 more | 2500 / 2500 / 0 / 0 |
| `SI_envelopDispDef` | 0x084bc244 | 140 | record keeping | gives up a skin's working definition | animEnvelopUpdate, envAssign, si_oldenvelopReset | 1200 / 1200 / 0 / 0 |
| `SI_envelopRelax` | 0x084bc2d0 | 488 | record keeping | frees a skin from its skeleton *[name]* | SI_envelopCut, SI_envelopRelax, SI_deleteSelection, supraDelete | second pass |
| `SI_envelopCut` | 0x084bc4b8 | 424 | record keeping | the same when a node is cut from its father *[name]* | SI_modelCutFather | second pass |
| `SI_envelopCopy` | 0x084bc660 | 388 | record keeping | what the copy of a node keeps of its master's envelope | SI_modelCopy, SI_modelCopyNode, SI_modelExpandAllWithoutGeometry | second pass (ported in si-rensetup) |
| `SI_envelopSkelPosReset` | 0x084bc7e4 | 1248 | computation | puts a skeleton back into its rest pose *[name]* | SI_envelopSkelPosReset, SI_envelopTmpReset, si_envelopReset | second pass |
| `SI_envelopTmpReset` | 0x084bccc4 | 280 | computation | the same for the time of an assignment *[name]* | SI_envelopAddNewParts, SI_envelopReassign, envelopWghtUpdate, si_envelopReassignMan | second pass |
| `envelopDataConvert` | 0x084bcddc | 560 | record keeping | converts records of the earlier format *[name]* | SI_envelopRebuild, envelopDataConvert | second pass |
| `envelopCheckVrtInd` | 0x084bd00c | 180 | record keeping | drops vertex entries whose number is no point of the skin *[name]* | envelopRebuildList | second pass |
| `envelopReassignPtr` | 0x084bd0c0 | 128 | record keeping | points lists at a skin anew *[name]* | envelopReassignPtr, envelopRebuildList | second pass |
| `envelopRebuildList` | 0x084bd140 | 352 | record keeping | rebuilds a branch's lists after a load *[name]* | SI_envelopRebuild, envelopRebuildList | second pass |
| `SI_envelopRebuild` | 0x084bd2a0 | 776 | record keeping | rebuilds a skin's envelope after a load or a copy *[name]* | SI_envelopRebuild, SI_duplicAndConnectGrafnode, SI_fdmodelBinaryRead, SI_modelSymmetry and 1 more | second pass |
| `SI_envelopRelationSet` | 0x084bd5a8 | 288 | record keeping | makes the dictionary relations of the envelopes after a load *[name]* | SI_dbLoadElementsFromDict | second pass |
| `SI_envelopCopyDef` | 0x084bd6c8 | 196 | record keeping | gives a skin a working definition of its own | animEnvelopUpdate, envAssign, envelopSkel, si_cleanupSelection and 1 more | 1500 / 1500 / 0 / 0 |
| `SI_envelopRenum` | 0x084bd78c | 272 | record keeping | renumbers vertex entries after a skin's points were renumbered *[name]* | SI_envelopRenum, SI_indexModifPropagate | second pass |
| `SI_envelopCompensate` | 0x084bd89c | 100 | computation | moves the skeletons under a node by a vector | doTransSGI | 600 / 600 / 0 / 0 |
| `SI_envelopNeedUpdate` | 0x084bd900 | 228 | record keeping | whether a skin has to be deformed anew: a moved bone with points of it | SI_envelopNeedUpdate, animEnvelopUpdate | 2500 / 2496 / 4 / 0 |
| `SI_envSkelSetFlag` | 0x084bd9e4 | 508 | record keeping | marks the bones of a skeleton that moved since the last reset | SI_envSkelSetFlag, animEnvelopUpdate | 3000 / 2999 / 1 / 0 |
| `SI_envSkelResetFlag` | 0x084bdbe0 | 268 | record keeping | clears every bone's marks and notes its pose | SI_sceneConstrUpdate, SI_updListUpdate, si_envelopReset | 1200 / 1182 / 18 / 0 |
| `SI_envJntAssBoundCreate` | 0x084bdcec | 1040 | record keeping | makes the bones' assignment volumes as display nodes *[name]* | SI_envJntAssBoundCreate, si_envJntAssBound | second pass |
| `SI_envJntAssBoundDraw` | 0x084be0fc | 344 | drawing | draws the assignment volumes *[name]* | si_drawGrafnode | second pass |
| `SI_envJntAreaLimDraw` | 0x084be254 | 360 | drawing | draws the limits of a joint's blend *[name]* | si_drawGrafnode | second pass |

91 functions. envelop.c: 17 computation, 7 record keeping; 19 checked (18452 bytes) | envelopset.c: 10 computation, 1 dialog, 2 drawing, 15 record keeping; 7 checked (957 bytes) | envtools.c: 3 computation, 2 drawing, 34 record keeping; 22 checked (3764 bytes). Checked in all: 48 functions, 23173 bytes.

## 9. Checking by emulation

After finding 0028's pattern: `gen.py` drives the harness through `deformlib.py`; the envelope
worlds are built by `eworld.py` (nodes in trees, bone records with lists, skins with every kind of
geometry, whole scenes of a skin and its skeleton in five arrangements: a global skin with plain
bones, with a chain, with a rigidly carried node beside the skeleton, a local skin under a chain's
first joint, a local skin under a root of word 11), and `gen_envtools.py`, `gen_envelop.py`,
`gen_envskin.py`, `gen_envset.py` hold one generator per routine. The composite routines run with
everything of the three files they call **running in the emulator too** (nothing of the package
is a stand-in), so `SI_envelopCompUpdate`'s cases compare the whole frame update of a skin.
`deform-emucheck` (the lattice package's binary) checks them; what "identical" covers is finding
0028's list.

### 9.1 Numbers

Run of 2026-10-13, `re/tools/si-deform/check_env.sh 1.0`.

| File | Routines | Cases | Identical | Given the library | Different | Not compared |
|---|---:|---:|---:|---:|---:|---:|
| `envelop.c` | 19 | 42,204 | 16,411 | 25,682 | 0 | 111 |
| `envelopset.c` | 7 | 7,108 | 7,108 | 0 | 0 | 0 |
| `envtools.c` | 22 | 26,328 | 26,088 | 23 | 0 | 217 |
| **all** | **48** | **75,640** | **49,607** | **25,705** | **0** | **328** |

| Mutation run | Identical | Different | Not compared |
|---|---:|---:|---:|
| `EMUCHECK_MUTATE=1` | 11,523 | 63,817 | 300 |
| `EMUCHECK_MUTATE=calls` | 62,467 | 12,875 | 298 |

Not compared: 285 cases in which the original stopped (a made-up scene that sends a walk through
null: a chain whose first sons lead to a node without record, a skin under a chain's first joint
placed where the walk meets it); 28 in which the port's two runs disagree, all of them the skin
matrix without inverse of section 7 (the original's result depends on stack contents too, which
the harness cannot see when both its runs leave the same stack addresses there); 11 that agree in
every defined word and carry the harness's note that the rest depends on memory nobody wrote; 4
whose outside calls depend on such memory. "Given the library": every case that makes a rotation
matrix (`SI_matrixXyzRotation`, `SI_matrixZRotation`: `sin`, `cos`), takes angles out of a matrix
(`asin`, `acos`) or calls `pow`; 23 more are inputs with a signalling NaN (finding 0011).

Instructions reached are noted while the vectors are made (finding 0028, 9.1): 6,674 in the 48
routines, 2 not reached (above). Every branch of `envelopJoint` (1,839 instructions) and
`envelopSkelMdl` (1,170) is reached.

### 9.2 What the checks found

**No port had an arithmetic fault**: on its first comparison every one of the 48 routines
agreed with the emulated original in every case compared, `envelopJoint` and `envelopSkelMdl`
included. The x87 readings (the mathematics package's symbolic reader on each routine, held
against a labelled listing whose register subtractions and divisions are decoded from the
instruction bytes) held. What the checks did find:

| Found by | Fault | Kind |
|---|---|---|
| 72 cases of two routines "no buffer `v_0`" | a buffer named `v_0`: the check reads every name beginning `v_` as an argument | generator. Renamed |
| a quarter of `envAutoSet`'s and of `SI_envelopHierUpdate`'s first cases stopping | a skin placed as a chain joint's first son, so that the walk down first sons met it; lists naming a made-up skin that `SI_envelopHierUpdate` then deforms | generator |
| coverage: 4 instructions of `envAutoSet`, the rigid-node tail of `envelopSkel`, a branch of `SI_envelopSkelUpdate` and one of `envelopChain` not reached | no chain without joints; a rigid node only as the main node of its envelope (which `envelopSkel` refuses); no effector without record | generator: those scenes added |
| 1, then 3 cases of `SI_envelopCompUpdate` differing (original `7feeffcc`) | a skin matrix without inverse: the original multiplies by a local nobody set, whose words are stack addresses, i.e. NaNs; the port's local held what its own earlier locals had left, finite, the same in both runs, so the result was 0 instead of NaN and not flagged | port, at a spot where it cannot reproduce the original: the local is now filled with NaNs carrying the image's fill (`unset_matrix`) and those cases count as not compared |
| 2 cases differing in a NURBS point's low word after that | the sixteen NaNs had sixteen payloads; which one an x87 operation hands on depends on the payloads, and in 1 case of 8 the three bits that survive into a double's low word were the same in both runs | port: one payload for all sixteen, so the two runs always differ there |
| review, before the first run | `SI_envelopCopyDef` read the first stored definition before the kind's switch; the original reads it only for the five kinds that copy | port |

## 10. Not ported, for the second pass

(As written after the first pass. The second pass took items 1 to 3 and three routines of
item 4: sections 13 to 16.)

Nothing is written and unchecked: every function in the new modules is cited; `cite.py` turns a
citation into `` `name` (0xADDRESS); unchecked `` when the reports do not carry it.

Not ported (43): `envelop.c` 5, `envelopset.c` 21, `envtools.c` 17, each written
`name (0xADDRESS)` in `env_outside.rs`' table or the module docs where the ported code meets it.
What the second pass should take first, in this order:

1. **`envToJntAssign (0x084b8ac8)`, `vrtInJntBound (0x084b7b68)`, `envAssign (0x084b90b4)`**
   (6,684 bytes): the automatic assignment by distance and bounding volume, on top of the ported
   `compRatio` and lists.
2. **`SI_envelopSkelAssign (0x084b9ff0)`, `envelopListBuild (0x084b9738)`,
   `envelopSkelBuild (0x084b9848)`, `envelopAssignSkelSet (0x084b9c3c)`, `SI_envelopSkelSet
   (0x084b99d8)`, `SI_envelopSkelReset (0x084b9b50)`, `SI_envelopReassign (0x084ba188)`**: making a
   skeleton and a skin and assigning (local and global modes).
3. **`vertWeightUpdate (0x084ba3e8)`, `SI_envelopVertWeightUpdate (0x084ba664)`,
   `SI_envelopWeightUpdate (0x084ba8b8)`, `SI_envelopWeightCreateReference (0x084ba59c)`,
   `SI_envelopVertDistMinMaxCompute (0x084ba930)`**: weight editing over the ported helpers.
4. **`SI_envelopRebuild (0x084bd2a0)`, `envelopRebuildList (0x084bd140)`, `envelopCheckVrtInd
   (0x084bd00c)`, `envelopReassignPtr (0x084bd0c0)`, `SI_envelopRelationSet (0x084bd5a8)`,
   `SI_envelopRenum (0x084bd78c)`, `SI_oEnvelopReadBinaryChunk (0x084bbe28)`,
   `oEnvelopVertReadBinaryChunk (0x084bbd7c)`, `envelopDataConvert (0x084bcddc)`**: what a loaded
   scene needs before its first frame, so that a skinned character from a file deforms.
5. `SI_envelopUpdateInitDef (0x084b6b9c)`, `envVrtResetEditFlag (0x084b6aa4)` (editing points of
   a skin), `SI_envelopSkelPosReset (0x084bc7e4)`, `SI_envelopTmpReset (0x084bccc4)`,
   `SI_envelopRelax (0x084bc2d0)`, `SI_envelopCut (0x084bc4b8)`, `envPrecInListFind (0x084baa9c)`,
   `SI_envelopAddNewParts (0x084bab1c)`, `SI_envelopNewVertAssign (0x084baf64)`,
   `SI_envJntAssBoundCreate (0x084bdcec)`, the old format's `oldenvelopJoint (0x084b63d4)`,
   `oldenvelopChain (0x084b6958)`, `si_oldenvelopReset (0x084b69e4)`, and `SI_envelopCopy
   (0x084bc660)`, which `si-rensetup` has.
6. Behind the trait for good: drawing `SI_envJntAssBoundDraw (0x084be0fc)`, `SI_envJntAreaLimDraw
   (0x084be254)`, the display colours `SI_envelopVrtColSet (0x084baf90)`, `SI_envelopVrtColGet
   (0x084bb390)`, and `SI_envelopAddNewSkelParts (0x084bad94)`, which asks the user
   (`SI_dialogRequest2`).

Not compared, of what is ported: cases in which the original stopped (a made-up scene that leads
a walk through null); the cases whose result depends on memory nobody wrote (the skin matrix
without inverse, section 7); signalling NaNs (finding 0011).

Not determined: what the vectors at +0x48 and +0x60 of the bone record are for (no ported
routine reads them); the meaning of `SI_glbAnim` +0x2e (named after its use only).

## 11. The outside world

The crate's one trait. The new code calls 15 routines not called before (the definition copies
and disposals of five geometry packages, `SI_modelUpdateSonState`,
`SI_modelSpecialUpdateSonState`, `SI_modelTouchDefinition`, `SI_skeChainRoot`,
`SI_elemdictGetRelationFromIds`) and 6 the lattice package named already (the two NURBS point
movers, the two mesh normal routines, `SI_patchComputeSurface`, `SI_edGetElemFromModel`). `env_outside_table.py` is
the one source of the list; `gen.py` holds the stand-ins against `env_outside.rs`.

## 12. Wanted in shared files

- **`si-types`**: `EnvSkelJnt` is an empty type; `env_records::EnvInfo` (with `DefCrv`),
  `EnvList`, `EnvVert`, `EditVert`, `InitSrt` can move in, and `Model` +0x32c (`unk_32c`) is the
  noted pose (nine floats), +0x08 carries the three envelope bits of section 2.4.
- **Duplicates to reconcile** (each ported again here from the listing and measured):
  `SI_envelopLevel`, `SI_mainGlobEnv`, `SI_envelopVertAllocate`, `SI_envelopListAllocate`,
  `SI_envelopInfoAllocate`, `SI_envelopVertCopy`, `envelopInfoCopy` (`si-rensetup`; the first two
  allocators also `si-hrc`). `si-rensetup`'s names `ratio`/`thickness` for the vertex entry's +4
  and +8 are, by `compRatio`'s use, the place along the bone and the distance from it.
  `SI_envelopCompUpdate` is what `motionUpdate.c`'s `animEnvelopUpdate` and `model.c`'s callers
  can call.
- **`re/tools/si-mem/check_globals.py`**: the `SI_identite` complaint (finding 0028, section 12).
- **`deform-emucheck`**: a case is compared up to its first difference only; when hunting a
  difference it would help to print them all.

## 13. Second pass: results

Run of 2026-10-14, `re/tools/si-deform/check_env2.sh 1.0` (vectors made afresh).

| | |
|---|---|
| On the image, checked and cited | **21 functions this pass**: 20 of the three files (15 of `envelopset.c`, 5 of `envtools.c`; 13,720 bytes) and `SI_matrixOrthoNormalInverse` of `iktools.c`. With the first pass: **68 of the 91**, 36,893 bytes; 23 left (section 16) |
| Emulation, `deform-emucheck` on `env2/vec` | 21 routines, **45,000 cases: 40,051 identical, 4,598 identical given the C library, 0 different**, 351 not compared |
| Not compared | all 351 are cases in which the original faulted (a read through null, section 15.3); **in every one the port faults too, on a read, after the same outside calls** |
| Mutation switches | `EMUCHECK_MUTATE=1`: 33,772 differ; the 10,877 that stay identical return nothing and change no byte. `EMUCHECK_MUTATE=calls`: 21,521 differ. `worldlib.mutation_held`: held for all 21 |
| Instructions the cases reached | 4,255 in the 21 routines, **6 not reached**: 3 dead (a pointer tested just after it was stored; a link into a list head that is null there) and 3 of `SI_envelopTmpReset` (a global skeleton's root with a chain root as a brother, which the scenes never make) |
| Earlier vectors, re-run with the changed check | lattices `vec/`: 85,176 / 77,144 / 6,869 / 0 / 1,163 (finding 0028's); envelopes, first pass, `env/vec`: 75,640 / 49,607 / 25,705 / 0 / 328 (section 9's). Their faulted cases (245 and 285): the port faults too, after the same calls |
| Globals | 28 in the crate, 9 new (`skel`, `SI_AssgnJntsDefNb` = 2, `SI_select3WM`, `SI_select6WM`, `SI_select7WM`, `SI_selectStep`, `SI_unselectWM`, `SI_objectWM`, `SI_edSumatraTo3x`); `check_globals.py`: the known `SI_identite` complaint only |
| Outside routines | 88 named, 8 new (`SI_meshPolygonCentroid`, `SI_findGlData`, `SI_modelGlobalState`, `SI_modelSetRotation`, `SI_dialogEnvelopInit`, `SI_modelGetVertexPtr`, `SI_elemGetNbVertices`, and `SI_envelopVrtColSet` of `envelopset.c` behind the trait); `check_outside.py` and `worldlib.check_names`: 0 wrong with all 141 citations |
| Constants from the binary's data | 39, 11 new (`check_env_data.py`: 0 wrong) |
| Unit tests | none new (6) |

| Function | Address | Bytes | File | Cases | Identical | Given the library | Different | Not compared |
|---|---|---:|---|---:|---:|---:|---:|---:|
| `vrtInJntBound` | 0x084b7b68 | 3500 | envelopset.c | 3000 | 3000 | 0 | 0 | 0 |
| `envToJntAssign` | 0x084b8ac8 | 1516 | envelopset.c | 2500 | 1644 | 788 | 0 | 68 |
| `envAssign` | 0x084b90b4 | 1668 | envelopset.c | 1500 | 1132 | 322 | 0 | 46 |
| `envelopListBuild` | 0x084b9738 | 272 | envelopset.c | 2000 | 2000 | 0 | 0 | 0 |
| `envelopSkelBuild` | 0x084b9848 | 400 | envelopset.c | 2000 | 1947 | 53 | 0 | 0 |
| `SI_envelopSkelSet` | 0x084b99d8 | 376 | envelopset.c | 2000 | 1915 | 52 | 0 | 33 |
| `SI_envelopSkelReset` | 0x084b9b50 | 236 | envelopset.c | 2000 | 1930 | 58 | 0 | 12 |
| `envelopAssignSkelSet` | 0x084b9c3c | 948 | envelopset.c | 2500 | 2437 | 60 | 0 | 3 |
| `SI_envelopSkelAssign` | 0x084b9ff0 | 408 | envelopset.c | 2000 | 1394 | 495 | 0 | 111 |
| `SI_envelopReassign` | 0x084ba188 | 492 | envelopset.c | 2000 | 406 | 1582 | 0 | 12 |
| `vertWeightUpdate` | 0x084ba3e8 | 436 | envelopset.c | 3000 | 2984 | 2 | 0 | 14 |
| `SI_envelopWeightCreateReference` | 0x084ba59c | 200 | envelopset.c | 1500 | 1498 | 2 | 0 | 0 |
| `SI_envelopVertWeightUpdate` | 0x084ba664 | 596 | envelopset.c | 2500 | 2471 | 1 | 0 | 28 |
| `SI_envelopWeightUpdate` | 0x084ba8b8 | 120 | envelopset.c | 1500 | 1499 | 1 | 0 | 0 |
| `SI_envelopVertDistMinMaxCompute` | 0x084ba930 | 364 | envelopset.c | 1500 | 1498 | 2 | 0 | 0 |
| `SI_envelopSkelPosReset` | 0x084bc7e4 | 1248 | envtools.c | 3000 | 2312 | 664 | 0 | 24 |
| `SI_envelopTmpReset` | 0x084bccc4 | 280 | envtools.c | 2000 | 1492 | 508 | 0 | 0 |
| `envelopCheckVrtInd` | 0x084bd00c | 180 | envtools.c | 2000 | 1997 | 3 | 0 | 0 |
| `envelopReassignPtr` | 0x084bd0c0 | 128 | envtools.c | 2000 | 1998 | 2 | 0 | 0 |
| `envelopRebuildList` | 0x084bd140 | 352 | envtools.c | 3000 | 2997 | 3 | 0 | 0 |
| `SI_matrixOrthoNormalInverse` | 0x084ad170 | 164 | iktools.c | 1500 | 1500 | 0 | 0 | 0 |

Section 8's map still reads "second pass" in the check column of these 20; this table is
their check. The ports are in `envelopset.rs` (15) and `envtools.rs` (6).

## 14. What the second pass's routines do

Read from the listings and measured (section 15) like the first pass's. *[name]* and *[guess]*
as at the top.

### 14.1 The automatic assignment (`vrtInJntBound`, `envToJntAssign`, `envAssign`)

**Volumes.** A bone's sons whose algorithm is 18 to 29 are its assignment volumes; other sons
are passed over. The point is taken into the frame of the node a volume names at +0x44 (the
volume itself in every case made; `SI_matrixInverse` of its global matrix) and multiplied by
the volume's local scaling per axis; the volume is then of unit size, with half extents 0.5:

| Algorithm | Shape | Being inside it |
|---|---|---|
| 18 | box: \|x\|, \|y\|, \|z\| ≤ 0.5 × scaling (not rounded) | keeps |
| 19 | round cylinder along x: \|x\| ≤ float(0.5 sx), float(√(y² + z²)) ≤ float(0.5 sy) | keeps |
| 20 | ball: float(√((x² + y²) + z²)) ≤ float(sx × 0.5) | keeps |
| 21, 23, 25 | elliptic cylinder along x: \|x\| ≤ a, y²/b² + z²/c² ≤ 1 (a, b, c = float(0.5 × scaling)) | 21 refuses, 23 keeps, 25 takes |
| 22, 24, 26 | ellipsoid: (x²/a² + y²/b²) + z²/c² ≤ 1 | 22 refuses, 24 keeps, 26 takes |
| 27, 28, 29 | a mesh (the son's definition) | 27 refuses, 28 keeps, 29 takes |

A bone with *keeping* volumes refuses a point inside none of them; a point inside a
*refusing* volume is refused; a point inside a *taking* volume and not refused is the bone's
(answer 1; `*out` = refused). A mesh volume: the box its points span (**grown, within one
call, by every mesh volume met so far**, from ±1e8) must hold the point; then for each polygon
the unit vector from the point to its centroid (`SI_meshPolygonCentroid`) against the
polygon's normal: a negative dot product means the point is beyond that face. **That mark is
never cleared within a call**: once one mesh volume finds the point beyond a face, every later
mesh volume of the bone leaves its flag as it was. A volume whose matrix has no inverse is
tested in the previous volume's frame (the local keeps it; for the first, what the stack
held).

**One point** (`envToJntAssign`): the bones are the list `skel` but chain roots, effectors and
roots of word 11. For each, `compRatio` from its global translation to its far end's (a
joint's: its first son's) gives the place along it and the distance (at least 0.0001).
Method 2, *by volumes*: every bone that takes the point gets a vertex entry with weight
float(d⁴); with any such bone, the weights become float(float(100 / Σ 1/d⁴) / d⁴) (**inverse
fourth power of the distance, normalised to 100**). With none, the point is shared the same
way among the `SI_AssgnJntsDefNb` (2 in the binary) nearest bones that do not refuse it.
Any other method: the nearest bone (the first of equal distances) gets the point, weight 100.
A bone whose share is above 50 gives the point its display colour (its display node's word
at +0x12 masked by `SI_select7WM`), unless a scene is being converted (`SI_edSumatraTo3x`).

**A skin** (`envAssign`): a rigidly carried node (word 7) is one point, number −1, at its
global translation. Otherwise the skin's element is related to itself (relation 0xcf, *the
envelope is on*), every point of its current stored definition (patch, mesh, spline key's
own point, NURBS control point divided by its weight) is taken through its global matrix,
assigned and coloured (`SI_envelopVrtColSet`), and the working definition is made anew.

### 14.2 Making a skeleton and its skins

- `SI_envelopSkelSet`: every node of a branch with word 0 or 1 that is no volume becomes a
  bone (word 9, a new record, its present local transformation noted as its rest
  transformation) and, but chain roots and effectors, gets the next colour of the cycle
  (`SI_select3WM`, + `SI_selectStep` each bone, back to `SI_select3WM` after `SI_select6WM`)
  on its display node. `SI_envelopSkelReset` undoes it for bones of word 9 whose record has
  no lists.
- `SI_envelopSkelAssign(skin, root, limit, method)`: a skin of word 5 to 8 gets a list (word 7:
  a rest transformation from `SI_modelGlobalState` instead), its sons of a fitting kind become
  skins of the same word (`envelopListBuild`); the skeleton's bones down to `limit` levels get
  records, words and one list per skin (`envelopAssignSkelSet`): at the top of a local
  skeleton a chain root gets 9 and its first joint 4, another root 11; at the top of a global
  one a chain root 4, another root 10; below, 9. Every bone (and the volumes under it, whose
  rest transformations are noted) goes into `skel`, every skin is assigned, `skel` is freed.
  **The list of skins built on the way is not freed.**
- `envelopSkelBuild` builds `skel` alone (for `SI_envelopReassign`).

### 14.3 Rest pose and assigning anew

`SI_envelopSkelPosReset(node, what, auto, brothers)` puts a skeleton into its rest pose
(`what` 0), parks its present pose in the records (+0x48, +0x54, +0x60: so **the record's
three vectors are the parked scaling, rotation and translation**, which section 10 left
undetermined) and takes the rest pose (1), or brings the parked pose back (2). A joint changes
only its angles (with `auto`, a chain under an automatic local skin, the parked joint takes
zeros); an effector its angles, and when its father's inverse kinematics record has its first
word set (finding 0022: the effector keeps its orientation) its rotation is made its global
one again (`SI_matrixOrthoNormalInverse`, `SI_modelSetRotation`); any other bone its scaling,
angles and translation. With `what` 0 volumes are put to rest too. Everything is copied word
for word. `SI_envelopTmpReset(skin, what)` does it for a skin's skeleton and rebuilds the
matrices (`SI_modelSpecialUpdateSonState` for a chain, `SI_modelUpdateSonState`).

`SI_envelopReassign(skin, hier)`: the method comes from the user (`SI_dialogEnvelopInit`;
nothing on 0) unless a scene is being converted (then 2); the skeleton is put at rest, every
bone loses its entries for the skin (with `hier`: for each skin of the root's lists whose
display node has its word at +0x08 set *[guess: selected]*), the skin is assigned again, the
pose is brought back and the skins deformed (`SI_envelopHierUpdate` or
`SI_envelopCompUpdate`).

### 14.4 Editing weights

`SI_envelopVertWeightUpdate(skin, node, target, point, weight, method, by_distance, reach,
scale)` takes the point from the skin's first stored definition (`SI_modelGetVertexPtr`) into
global space; **by distance, the weight is weight × scale × (reach − distance from the target
bone), the factor at least 0.0001** (a fall-off with distance). `vertWeightUpdate` walks the
skeleton level by level: the point's first entry in each bone's list for the skin goes through
`SI_envelopModifyWeight` (first pass: the target's entry set, added to or raised by a
percentage, clamped to 0 … 100; the others gathered); the target without an entry gets one
(`SI_envelopWeightCreateReference`, methods 0 and 1 only). Then `SI_envelopVertWeightBalance`
scales the others down to a sum of 100, and the point takes `SI_objectWM` (no target) or the
target's colour (new weight above 50). `SI_envelopWeightUpdate` does the same for a rigidly
carried node (its rest translation, number −1; the weight argument's own slot is the amount
the walk changes). `SI_envelopVertDistMinMaxCompute` widens a range by one point's distance
from a bone (a NaN widens nothing).

### 14.5 After a load: the lists (`envelopRebuildList`, `envelopReassignPtr`, `envelopCheckVrtInd`)

A loaded bone's lists have no skins. `envelopRebuildList(skin, root, 0)`: for each bone whose
record has the level expected, the **last** list takes the skin (not for word 7) and the
skin's sons of the same word take the lists before it, walking back through the `prev` links
from the last (`envelopReassignPtr`: the first list without skin found). Entries numbered
outside the skin's points (`SI_elemGetNbVertices` of the skin's element) are unlinked and
freed (`envelopCheckVrtInd`). A chain root or a root of word 11 whose first son has a record
then takes lists like its son's: **its first list record is freed and the rest of its old chain
is left in memory**, and new empty lists, one per list of the son, are chained.

## 15. Checking the second pass

### 15.1 How

As finding 0011 ("Writing a check for a routine") asks: `gen2.py` drives a
`worldlib.Session(strict=True)` (refill, guards, read-only code, poisoned stack) with heap
dumps bounded at 1 MB a case (none came near it); the stand-ins are a `worldlib.Outside` held
against the crate's two tables of outside routines and the binary's symbol table. The first
pass and the lattice package keep `gen.py` and its session, so their vectors are made as
before. **No new world builder**: the assignment's and skeletons' cases use the worlds of
`gen_envassign.py` and `gen_envskel.py` (written by an earlier attempt at this pass); the
others use the first pass's scenes (`gen_envelop._scene`, `eworld.scene`) with what the
scenes leave at 0 filled in (`gen_envreset.extras`: local scaling, inverse kinematics records
on chains, an effector's rotation, now and then a volume). The composite routines run
everything of the package they call in the emulator too: `SI_envelopSkelAssign`'s cases
include the assignment of every point, `SI_envelopReassign`'s the rest pose, the assignment
and the whole deformation of section 3.

Two additions to the check (`deform-emucheck`, this package's binary):

- a case in which the original faulted is run on the port under `OnFault::Stop`; the report
  says whether the port faulted too, on the same kind of access, after the same outside calls
  (it is still not compared);
- `SI_modelSetRotation` reads the angles its caller put in a local. Its stand-in stores back
  the words it found there, so the vector carries them, and the check holds the port's words
  against them (`READ_THROUGH`). Shown live: with the port's angle moved by 0.001, 662 cases of
  `SI_envelopSkelPosReset` and 508 of `SI_envelopTmpReset` differ.

"Given the library": `pow` (the weights d⁴ of the assignment by volumes), `sin`, `cos`,
`asin`, `acos` (rotation matrices and angles in the rest pose and the final deformation), and
inputs with a signalling NaN (words copied by integer moves; finding 0011).

### 15.2 Numbers per file

| File | Routines | Cases | Identical | Given the library | Different | Not compared | Differ with `MUTATE=1` | with `MUTATE=calls` |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `envelopset.c` | 15 | 31,500 | 27,755 | 3,418 | 0 | 327 | 25,715 | 14,977 |
| `envtools.c` | 5 | 12,000 | 10,796 | 1,180 | 0 | 24 | 6,557 | 6,544 |
| `iktools.c` | 1 | 1,500 | 1,500 | 0 | 0 | 0 | 1,500 | 0 |
| **all** | **21** | **45,000** | **40,051** | **4,598** | **0** | **351** | **33,772** | **21,521** |

Cases that stay identical under `MUTATE=1` change nothing: most are a walk that meets no bone
of the level asked, a list without the skin, a weight whose point no bone has, a dialog
answered 0. Generating takes about 4 minutes, the three checks about 30 seconds.

### 15.3 Where the original faulted

Every one is the original reading through a null pointer, reached by an input the worlds make
on purpose, a few percent of the cases. Whether the program's callers can produce those inputs
is not determined (section 16).

| Read through null | Where | Cases |
|---|---|---:|
| the display node of a bone (`SI_findGlData` answered 0): **the answer is never tested** | `envToJntAssign`+0x524, +0x5b5; `envelopAssignSkelSet`+0x13a; `SI_envelopSkelSet`+0x82; `SI_envelopSkelReset`+0x5b | 66 |
| the nearest bone, when the methods other than 2 find no bone that can take the point (only chain roots, effectors, roots of word 11, or `limit` 0) | `envToJntAssign`+0x55b (under `envAssign`, `SI_envelopSkelAssign`, `SI_envelopReassign` too) | 185 |
| the far end of a joint without sons | `envToJntAssign`+0x90 | 8 |
| a null node handed in: `SI_envelopSkelSet` tests it and reads its brother anyway; `SI_envelopSkelPosReset` the same | `SI_envelopSkelSet`+0x14f; `SI_envelopSkelPosReset`+0x84, +0x4aa | 50 |
| the target bone, when weighting by distance without one | `SI_envelopVertWeightUpdate`+0xdf | 28 |
| the record of a node without one, matched by level −1 | `vertWeightUpdate`+0x73 | 14 |
| | | **351** |

### 15.4 What the checks found

**No port had a fault**: the 9 routines of the earlier attempt (cited before any check had
run), the 3 it had written and cited without vectors (citations withdrawn first), and the 9
written in this pass agreed with the emulated original on every compared case at their first
check. What the checks found was in the scaffolding:

| Found by | Fault | Kind |
|---|---|---|
| reading the earlier attempt's work | `SI_matrixOrthoNormalInverse`, `SI_envelopSkelPosReset`, `SI_envelopTmpReset` cited with no vector made | citations withdrawn, restored after their check |
| the brief | its generators ran in the package's own non-strict session | moved to `worldlib.Session(strict=True)`; the assignment's 9 routines gave the same counts |
| `EMUCHECK_MUTATE=1` on the earlier vectors | 7,755 of 19,232 compared cases changed nothing (`envelopListBuild` 1,730 of 2,000): weak worlds | generator: the top node made one the routine takes; then 16,018 of 19,227 changed something |
| the stops of `SI_envelopReassign` (182 of 2,000) | a list naming a made-up address that the final update deforms; a bone of the scenes left out of the display-node answers; an added volume naming no frame at +0x44 | generator; 12 stops left, all the original's own |
| one case of `SI_matrixOrthoNormalInverse` differing | a signalling NaN in a random input: the emulator keeps it through `fld`/`fst`, a real unit and the port make it quiet (finding 0011) | generator: no signalling NaN made |
| reading the listing of `SI_envelopSkelPosReset` | the angles handed to `SI_modelSetRotation` through a local were not compared | the check: `READ_THROUGH` |

## 16. What is left after the second pass

**Not ported (23 of the 91)**, each written `name (0xADDRESS)` where ported code meets it, in
section 10's order:

- item 4, the rest of what a loaded scene needs: `SI_envelopRebuild (0x084bd2a0)`,
  `SI_envelopRelationSet (0x084bd5a8)`, `SI_envelopRenum (0x084bd78c)`,
  `SI_oEnvelopReadBinaryChunk (0x084bbe28)`, `oEnvelopVertReadBinaryChunk (0x084bbd7c)`,
  `envelopDataConvert (0x084bcddc)`;
- item 5: `SI_envelopUpdateInitDef (0x084b6b9c)`, `envVrtResetEditFlag (0x084b6aa4)`,
  `SI_envelopRelax (0x084bc2d0)`, `SI_envelopCut (0x084bc4b8)`, `envPrecInListFind
  (0x084baa9c)`, `SI_envelopAddNewParts (0x084bab1c)`, `SI_envelopNewVertAssign (0x084baf64)`,
  `SI_envJntAssBoundCreate (0x084bdcec)`, `oldenvelopJoint (0x084b63d4)`, `oldenvelopChain
  (0x084b6958)`, `si_oldenvelopReset (0x084b69e4)`, `SI_envelopCopy (0x084bc660)` (in
  `si-rensetup`);
- item 6, behind the trait for good: `SI_envJntAssBoundDraw (0x084be0fc)`,
  `SI_envJntAreaLimDraw (0x084be254)`, `SI_envelopVrtColSet (0x084baf90)` (called by the ported
  assignment and weight routines through the trait), `SI_envelopVrtColGet (0x084bb390)`,
  `SI_envelopAddNewSkelParts (0x084bad94)`.

**Better checked on the real program's state than on made-up memory** (the lead's note of
2026-10-14; AGENTS.md, "The real program is the reference"). The made-up worlds are the
first pass's scenes, and every stand-in answer is ours:

- the loading routines `envelopRebuildList`, `envelopReassignPtr`, `envelopCheckVrtInd`, and
  everything left in item 4: their input is what the loader builds (lists without skins,
  element dictionary, real point counts from `SI_elemGetNbVertices`);
- the whole assignment, `SI_envelopSkelAssign`, `SI_envelopReassign`, `envAssign`,
  `envelopAssignSkelSet`, `SI_envelopSkelSet`, `SI_envelopSkelReset`: they colour display nodes
  of the graph list (`SI_findGlData`, answered by our stand-in), relate elements in the
  dictionary, and ask the user (`SI_dialogEnvelopInit`);
- `SI_envelopSkelPosReset`, `SI_envelopTmpReset`: real chains with their inverse kinematics
  records, and `SI_modelSetRotation`, `SI_modelUpdateSonState`,
  `SI_modelSpecialUpdateSonState` run for real instead of answered;
- `SI_envelopVertWeightUpdate`, `SI_envelopVertDistMinMaxCompute`: the point comes from
  `SI_modelGetVertexPtr`, a stand-in here.

Well served by made-up inputs: `vrtInJntBound` (pure geometry, 1,049 instructions all
reached), `SI_matrixOrthoNormalInverse`, `envToJntAssign`'s arithmetic, `vertWeightUpdate`,
`SI_envelopWeightCreateReference`, `SI_envelopWeightUpdate`, `envelopListBuild`,
`envelopSkelBuild`.

**Not determined.** Whether the program can reach the original's reads through null of
section 15.3 (a bone without a display node; a skeleton with no bone that can take a point,
assigned by nearest bone; a joint without a son; weighting by distance without a target
bone): the emulator shows what the routine does then, not whether a caller lets it happen.
What `SI_dialogEnvelopInit`'s answers other than 2 (by volumes) and 0 (cancel) are called
(the code treats every other value as "nearest bone"). What the display node's word at +0x08
means (read by `SI_envelopReassign` with `hier`; *[guess]* selected). The meaning of
`SI_glbAnim` +0x2e (section 10, unchanged). **Determined this pass**: the bone record's vectors
at +0x48 and +0x60 (section 10) are the parked scaling and translation (14.3).

**Wanted in shared files** (beyond section 12):

- duplicates to reconcile: `SI_matrixOrthoNormalInverse` (`iktools.c`; `si-kine` ports it);
  the global `SI_objectWM` (`si-ui` declares it as `i16`, here `u16`);
- `si-types`: `jntlist` and `vertlist` (`envelopset::JntList`, `VertList`) can move in with the
  other envelope records; the graph node's word at +0x08 and colour word at +0x12 are read here;
- the harness (`re/tools/emu`): nothing. The check's two additions are in this package's
  binary: a case in which the original faulted is run on the port under `OnFault::Stop`, and
  an outside routine that reads through a local (`READ_THROUGH`) has its words compared. Both
  could serve other packages' checks.

## 17. Third pass: the envelopes of a loaded scene (2026-10-14)

The question (finding 0013, 13.4): Beauty_shot's 1,600 differing dumps came from the loaded
state, not from a routine. The original's model reader leaves two things the port's did not:
each spline's step matrix, and the skeleton's envelope records rebuilt. With both in the
load and the deformation in the update, **every dump of the six scenes named in 13.4 is the
original's**, and the corpus goes from 91,488 to 93,140 identical dumps of 93,275.

### 17.1 Ported (2 functions, read from the listings; measured, 17.4)

| Function | Address | Bytes | What it does |
|---|---|---:|---|
| `SI_envelopRebuild` | 0x084bd2a0 | 776 | after a model file is read (or a branch copied): converts the earlier format's chains, gives each skeleton's lists their skins, deforms the skins once, gives volumes and rigid nodes a rest transformation |
| `envelopDataConvert` | 0x084bcddc | 560 | numbers the levels of a chain of the earlier format and gives its root and effector lists like their neighbours' |

Both in `envtools.rs`; `si_oldenvelopReset (0x084b69e4)` (files below version 2.6) is
behind the trait (`env_outside.rs`, new), reached by no rebuild of the corpus. With the two:
**70 of the 91 functions on the image, checked and cited**; 21 left (section 16's list less
the two).

**`SI_envelopRebuild(node, all, old)`**, read from code. For each node, before its first son
and its next brother (both visited with the same flags; brothers by recursion, so a node's
last step runs after its whole branch and its younger brothers):

1. a chain root (a null of algorithm 1 or 2) without record whose first son's record has
   level 0 is converted (`envelopDataConvert(node, 0)`). This is the case of every chain of
   the version-3.0 skins: `si-hrc`'s token-17 reader disposes the chain root's record below
   3.49089 (finding 0004), the first joint keeps its record of level 0;
2. a node of level 0 finds its skin: a chain root whose first son has word 4, its local skin
   (`SI_envLocalGet`; made word 5 when it has none); word 4, its father (made 6 when none);
   word 10, its father; word 11, its local skin. **No skin, or another word: the routine
   returns at once, the branch below and the brothers not visited.** The lists get their
   skins (`envelopRebuildList(skin, node, 0)`), and a local skeleton (the chain case, word 11)
   deforms its skins (`SI_envelopHierUpdate`, after `si_oldenvelopReset(node)` when `all` and
   `old`);
3. a node of another level whose algorithm is 18 to 29 (a volume) and whose father has a
   record gets a rest transformation (`SI_calloc(1, 0x24)`: its local scaling, angles,
   translation) when it has none;
4. after the branch: for the main node of a global envelope (`SI_mainGlobEnv`) with a
   skeleton (`SI_envelopSkelRoot`), without `all` its matrices from no father
   (`SI_modelUpdateSonState(null, node, 1)`) and the skins deformed; with `all`,
   `si_oldenvelopReset(root)` when `old`, the skins deformed (`SI_envelopHierUpdate(root)`),
   and a rigid node (word 7) without rest transformation gets one from its global state
   (`SI_modelGlobalState` into a new `SI_calloc(1, 0x24)`).

`SI_fdmodelBinaryRead` calls it with `all` 1 and `old` = version below 2.6 (finding 0004).

**`envelopDataConvert(node, level)`**, read from code: a chain root whose first son's record
has a level other than 0, or any chain root when `level` ≤ 0, becomes word 9 (if it had no
word), gets a new record (`SI_envelopInfoAllocate`, written without a test) of `level` and one
empty list (`SI_envelopListAllocate(null)`) per list of its first son, and a rest
transformation from its local one when it has none; its first son is converted at `level` + 1.
A joint (kind 5) with a record: word 9 unless 4, its level `level`; first son at `level` + 1.
An effector (a null of algorithm 3 or 4) with a record: word 9, level `level`, one empty list
per list of its **father's** record (chained after the lists it may have: a record that has
lists already is written through null, as in the original); first son at `level` + 1.
Anything else stops the descent. The next brother is converted at `level` when `level` > 0.
Every son and father is read without a test for null.

### 17.2 Wiring (no function of the original: hosts)

- **The loader** (`si-scene`, `image::Prog`, finding 0018): `SI_splineUpdateMatrix
  (0x0848abd0)`, which `splineBinaryRead` and `SI_bezierShrink` call for every spline, is
  `si-spline`'s port (`spline_image::spline_update_matrix`): the +0x24 matrices are the
  original's at load (si-scene now depends on si-spline). `SI_envelopRebuild` at the end of
  `SI_fdmodelBinaryRead` is the port above, its calls out through `image::EnvSide`: first the
  ports `si_scene::envside::envelope_call` names (the model tree's updates, the touch, the
  global state, the vertex questions of `si-model`; `si-model`'s mesh copy with its lists of
  selected polygons and the two normal routines on the image; `si-hrc`'s spline copy;
  `si-spline`'s patch copy and surface; `si-kine`'s chain update and chain root; `si-view`'s
  graph node; `si-dict`'s relation; `SI_edGetElemFromModel`), the rest the loader's dispatch.
- **NURBS skins are left as they were** (`envside::unsupported_skin`, `unsupported_rebuild`):
  their working copy and point moves (`SI_nurbsCrvCopy`, `SI_nurbsSrfCopy`,
  `SI_nurbsCrvMovePoint`, `SI_nurbsSrfMovePoint`) are on values only in `si-nurbs`; without
  them the deformation writes through a null working definition. First run without this
  guard: 923 dumps missing in Actor-Walk, Expressions-MOUTH and General-Skeleton (a NURBS
  surface skin, the load faulting 992 times at +0x54 and losing the elements' graph nodes).
  The hosts count what they leave: over the evalScene corpus 3 hierarchies not rebuilt
  (those three scenes; `deform-scenecheck count`) and 8 calls of the update.
  **Since 2026-10-15 both guards are gone** (finding 0025 §13.12): those four routines,
  `SI_nurbsSrfDispose` and `SI_nurbsSrfTransferPointsKnots` now have image forms in
  `si-nurbs`, and `envside::envelope_call` answers them. Nothing the guards refused is
  unsupported any more. So the loader runs `SI_envelopRebuild` and the update's hosts
  (`si-rensetup`, `motion-evalcheck`) run `SI_envelopCompUpdate` on NURBS skins too. What the
  image forms call outside their files (`SI_genTreeDestroy`, `markDirtyCcrvs`,
  `SI_nurbsSrfForceCapping`) goes back to the host through `envside::NurbsSide`, and the
  host answers it as it answers any routine it lacks (the update's hosts: 0, counted). `deform-scenecheck count`: 81 scenes with taken calls, 109
  calls, none refused (was 78, 106 and 3 refused). `motion-evalcheck` stays at 93,244 of
  93,275, with no dump missing. Pictures (finding 0023 §30): Expressions-MOUTH 115 of 115 and
  Actor-Walk 50 of 50 frames identical, was 0. General-Skeleton is still identical but the
  icon. Owed:
  - `SI_nurbsSrfForceCapping` is still answered 0 (2,880 calls over the evalScene corpus,
    Actor-Walk's capped skin), yet those frames match.
  - The three rebuilds are not yet compared on their own: `deform-scenecheck`'s vectors
    (`re/out/si-deform/load/vec`, 17.4) predate this change and do not hold them.
- **The update** (`si-rensetup`'s `native/motion.rs`, `motion-evalcheck`'s host):
  `SI_envelopCompUpdate` is si-deform's port with the same callees; `SI_modelSetTranslation`
  (si-kine's plane constraint) si-model's. The render's update optimises the envelopes
  (`SI_glbAnim` +0x3a set), so `animEnvelopUpdate` first asks `SI_envelopSkelRoot`,
  `SI_envSkelSetFlag`, `SI_envelopNeedUpdate` (and the scene `SI_envSkelResetFlag`,
  `SI_envelopDispDef`, `SI_envelopCopyDef`): routed to the first pass's ports too.
  `RENSETUP_NO_ENVELOPES=1` and `--no-envelopes` / `--no-set-translation` give the earlier
  routing back; `--envelopes`, `--set-translation`, `--spline-matrices` are kept.
- `si_scene::calls::Unported::port_call` (new, default nothing): told before and after the
  loader's dispatch hands a routine to its port; what the check of 17.4 stands on.

### 17.3 Against the original's evalScene and pictures, step by step

`motion-evalcheck`, 93,275 dumps (identical / last digit / differing / missing); **every row
the same with the tool's stand-ins on and off** (`--no-standins`):

| Step | All scenes | Beauty_shot | Man_Box | Man_V3_0 | Missile | IK-PageTurn | FEnvCnsClsObj |
|---|---|---|---|---|---|---|---|
| before (tree as found) | 91,488 / 52 / 1,735 / 0 | 1,332 / 28 / 1,600 | 399 / 12 / 0 | 560 / 12 / 0 | 100 / 0 / 0 | 249 / 0 / 0 | 60 / 0 / 0 |
| 1. spline matrices at load | 91,488 / 52 / 1,735 / 0 | the same | | | | | |
| 2. + `SI_envelopRebuild` at load | 91,488 / 52 / 1,735 / 0 | the same | | | | | |
| 3. + the update's envelope and `SI_modelSetTranslation` | **93,140 / 0 / 135 / 0** | **2,960 / 0 / 0** | **411 / 0 / 0** | **572 / 0 / 0** | 100 / 0 / 0 | 249 / 0 / 0 | 60 / 0 / 0 |

Steps 1 and 2 change no dump alone (as finding 0013 found for the stand-ins); step 3 needs
both. The 135 left: position limits 50, camera up vector 30, inverse kinematics 15,
expressions 12 and quick stretch 12 (the attractors' control, 13.5 of finding 0013), wave or
cluster 8, boolean 5, dynamics 3. The three scenes `--envelopes` alone made worse (Missile
10, IK-PageTurn 144, FEnvCnsClsObj 5) are identical.

Pictures (`re/tools/si-scene/pass8_pictures.sh`, 296 compared, 334 of 334 scenes run, 0
failures): the lead's run 222 identical; the tree as found (si-render's work in it, my wiring
off) 223 (IK-PageTurn's page: not this pass's, the same bytes with the spline matrices on and
off); step 1: 223, **all 334 pictures byte for byte as before**; step 2: **226**
(Ambulate-the_walk, CONSTRAINT_ENV-FEnvCnsClsObj, ACTOR-Cheers identical; Beauty_shot, Cow,
Man_Box, Man_V3_0, Missile changed and still differ); step 3 with only
`SI_envelopCompUpdate` routed: 226, byte for byte as step 2 (the render's update asks the
envelope questions first, answered 0); with them routed: **228** (Beauty_shot, Man_Box
identical; Man_V3_0 10 pixels of 212,000, largest difference 2). No picture got worse.

### 17.4 Checks

**On the loaded scenes' own state** (finding 0011, "A real scene as the world"):
`deform-scenecheck` (`re/tools/si-deform/scenecheck`, a tool package: it links si-scene and
si-emuworld) loads each scene of the evalScene manifest as `motion-evalcheck` does, with the
harness's heap policy, and at every call of the rebuild whose hierarchy has an envelope record
or word writes the image (`dump`: 101 images of 74 scenes, one per hierarchy of a database).
`gen_rebuild.py` runs the original `SI_envelopRebuild(root, 1, 0)` and, for each chain root it
would convert, `envelopDataConvert(node, 0)`, strict session, **no stand-in** (the whole
deformation, the mesh copies with their polygon lists, normals, the model tree's updates run as
the binary has them; `times` answered 0 and recorded). `check` loads the scene again, holds
the image against the vectors' hashes, compares the loader's own call (every byte changed,
every block taken, paired in order and compared byte for byte, the blocks freed), and in a
second load runs `envelopDataConvert` on the same state.

| Routine | Cases | Identical | Different | Changing nothing on either side | `MUTATE=1` differ | `MUTATE=calls` |
|---|---:|---:|---:|---:|---:|---|
| `SI_envelopRebuild` | 101 | **101** | 0 | 90 | 11 (every case that changes memory) | nothing recorded on either side |
| `envelopDataConvert` | 3 | **3** | 0 | 0 | 3 | the same |

The 11 rebuilds that change memory each deform skins (in 10 scenes: Beauty_shot, Man_Box,
Man_V3_0, Cow, Missile, Ambulate-the_walk, IK-PageTurn, Multiped-ANT, FEnvCnsClsObj,
ACTOR-Cheers twice): Beauty_shot's takes 35,864 blocks (the skins' working meshes, 1,913
runs of changed bytes), Man_Box's and Man_V3_0's 14,080, the smallest 11; the three
conversions (Beauty_shot, Man_Box, Man_V3_0) 240, 142, 142. All equal block for block.
**Found on the way**: the first check of the six largest differed by two blocks at the end of
each mesh copy: `SI_meshCopyPolygonLists`, which `si-model`'s mesh copy reached as a hook;
si-model's agent put it on the image during this pass, and with it all agree (the vectors
were made with it running in the harness, no stand-in). Instructions no case reached (`--cover`): in
`SI_envelopRebuild` 172 of 776 bytes (the local skin made word 5, the father made word 6, the
roots of word 10 and 11, both `si_oldenvelopReset` calls, the `all` 0 path, the rigid node's
rest transformation, padding); in `envelopDataConvert` 125 of 560 (the chain root's rest
transformation: every converted root had one from its file; padding).

**Earlier checks, re-run** (`re/out/si-deform/load/`): `deform-emucheck` lattices 85,176 /
77,144 / 6,869 / 0 / 1,163; envelopes `env/vec` 75,640 / 49,607 / 25,705 / 0 / 328
(`MUTATE=1` 63,817 differ, `=calls` 12,875); `env2/vec` 45,000 / 40,051 / 4,598 / 0 / 351
(33,772; 21,521): all as before. `check_outside.py`: 89 routines, 143 citations, 0 wrong;
`check_globals.py si-deform`: the known `SI_identite` complaint only. Unit tests: 7 (one new,
made-up chain).

### 17.5 Not done, not determined

- NURBS skins (17.2): their copy and point moves on the image were `si-nurbs`'s to make;
  done 2026-10-15 (finding 0025 §13.12), the guards removed.
- `si_oldenvelopReset`, `oldenvelopJoint`, `oldenvelopChain` (files below 2.6): no rebuild of
  the corpus reaches them. Ported and checked in section 18.
- `SI_envelopActive` asks `SI_elemdictGetRelationFromIds`, which the loader's dispatch does
  not answer (si-dict has it; `envside` routes it for the rebuild and the update only): with
  the lists now holding their skins after the load, the untold relations of the load reach
  it (43 cases of `scene-worldcheck`, finding 0018). Routing it in `Prog` changes the load's
  relations; not done in this pass.
- What the original's `times`-driven busy cursor (`SI_showActivity`) does in the interactive
  program: answered 0 here, a hook in the port.

## 18. Fourth pass, deform2 (2026-10-28): the earlier envelope format, the parts, the colours

Ported from the instruction listings (read from code) and checked with `deform-emucheck` on
vectors from `re/tools/si-deform/gen2.py` (module `gen_envold`, vectors in `re/out/si-deform/env3/vec`).
12 functions, none unchecked:

| Function | What it does | Cases | Different |
|---|---|---|---|
| [soft 0x084b63d4 oldenvelopJoint] | moves the points one bone holds, in the older format (block at node +0x34c, matrices array at block +0xc) | 2,500 | 0 |
| [soft 0x084b6958 oldenvelopChain] | walks a chain applying the joint step, level by level | 1,500 | 0 |
| [soft 0x084b69e4 si_oldenvelopReset] | finds the chain root, applies the chain, gives each skin of the first son a fresh working definition | 1,500 | 0 |
| [soft 0x084b6aa4 envVrtResetEditFlag] | clears the "edited" mark of every point of a working definition (five kinds) | 800 | 0 |
| [soft 0x084baa9c envPrecInListFind] | finds an entry in a list of the global `prec` | 1,500 | 0 |
| [soft 0x084baf64 SI_envelopNewVertAssign] | status message, then reassigns | 1,000 | 0 |
| [soft 0x084bab1c SI_envelopAddNewParts] | adds new points to the envelope lists (asks the dialog whether global or local) | 1,500 | 0 |
| [soft 0x084bad94 SI_envelopAddNewSkelParts] | the same for a skeleton's parts | 2,000 | 0 |
| [soft 0x084bd78c SI_envelopRenum] | renumbers vertex entries after points were removed | 2,000 | 0 |
| [soft 0x084bc2d0 SI_envelopRelax] | relaxes the weights of selected points | 2,500 | 0 |
| [soft 0x084bb390 SI_envelopVrtColGet] | the display colour a point is marked with (mark in bits 8 to 10 of the point's flag, one of `SI_select1WM` to `SI_select7WM`) | 1,500 | 0 |
| [soft 0x084baf90 SI_envelopVrtColSet] | marks a point with a colour | 1,500 | 0 |

`SI_envelopVrtColSet` and `si_oldenvelopReset` were hooks (outside routines answered by recorded
stand-ins) in the earlier passes; the callers now call them directly and the stand-ins are gone from
`env_outside.rs` and the tool tables, so the vectors of every routine that reached them were made
anew (`env2/vec`: `envAssign`, `SI_envelopReassign`, `SI_envelopSkelAssign`,
`SI_envelopVertWeightUpdate`; the rest were not affected). Three outside routines are new:
`SI_dialogRequest2` (0x08141d5c), `SI_elemdictRemoveRelationId` (0x08133998),
`SI_meshOptimizeDisplay` (0x0845a7c8); `SI_statusDisplayMessage` now has a stand-in with its two
arguments. The old-format block of a node is a new field of the made-up model (`eworld.EMODEL`).

Two generator corrections came out of the check, both because the original stores through a wrong
vertex number into the neighbouring memory, which the port (rightly) does not repeat: the lists of
`si_oldenvelopReset` hold only numbers inside the skin, and `SI_envelopVertWeightUpdate` is not
given a number outside it (a number outside the skin is still given to `vertWeightUpdate`, which
does not run the colour routine).

Numbers (`deform-emucheck`):

- `env3/vec` (12 files): 19,800 cases, 18,461 identical, 1,202 identical given the library, **0
  different**, 137 not compared (the original faulted too and the port after the same outside calls;
  a few whose result rests on memory nobody wrote).
- `env2/vec` (21 files, regenerated): 45,000 cases, 40,056 identical, 4,599 given the library, **0
  different**, 345 not compared.
- The lattice package's vectors: 85,176 cases, 0 different, 1,163 not compared (as in 0028); the first
  pass's: 75,640 cases, 0 different, 328 not compared; `vec_deformation`, `vec_frenet`: 1,500 each, 0 different.
- Mutation on `env3`: `EMUCHECK_MUTATE=1` 14,358 differ (all twelve functions differ); `=calls` 5,345
  differ. The seven with no outside call that takes an argument (`SI_envelopRenum`, `VrtColGet`,
  `VrtColSet`, `envPrecInListFind`, `envVrtResetEditFlag`, `oldenvelopChain`, `oldenvelopJoint`) stay
  identical under `=calls` as they must. On `env2`: `=1` 33,845 differ; `=calls` 21,533 differ.
- Instructions: 1,670 in the twelve, **9 not reached** (6 in `oldenvelopJoint`, 3 in `SI_envelopAddNewParts`).
- Crate: `cargo test -p si-deform` 9 passed; `check_outside.py` 94 routines, 0 wrong; `check_globals.py` 1
  complaint, the known one about `SI_identite`.

`deform-scenecheck check` (section 17.4) reported "not held" for all 104 images at that time: the loader had changed since they were made. They were dumped again in the fifth pass (section 19.1) and the three functions compared.

Not ported, by decision (interactive or drawing, not part of deformation evaluation):
`SI_envJntAssBoundCreate` (0x084bdcec), `SI_envJntAssBoundDraw` (0x084be0fc), `SI_envJntAreaLimDraw`
(0x084be254), `SI_latticeDraw` (0x0843f254), `SI_dialogModelSetCluster` (0x0843b9ac),
`klusterPrepareTextLine` (0x0843be18), `klusterInfoUpdateItems` (0x0843bf1c), `WrathStayGhosted`
(0x0843d884), `SI_dialogClusterInfo` (0x0843c014). `SI_envelopUpdateInitDef` was left undecided here and ported in section 19.

To repeat:

```bash
E="uv run --with unicorn python3 -I"
(ulimit -v 6000000; timeout 3500 $E re/tools/si-deform/gen2.py re/out/si-deform/env3/vec 1.0 gen_envold)
(ulimit -v 2000000; timeout 600 target/si-deform/release/deform-emucheck re/out/si-deform/env3/vec)   # + EMUCHECK_MUTATE=1 / calls
```

## 19. Fifth pass, deform3 (2026-10-28): the scene images again, `SI_envelopUpdateInitDef`, `SI_envelopCut`

### 19.1 The scene images dumped again

`deform-scenecheck dump` was run at the current commit (104 `SI_envelopRebuild` calls in 77 scenes,
2.6 GB; the stale dumps and vectors were deleted), `gen_rebuild.py` made the vectors in four parts
(`--part i/4`), `deform-scenecheck check` compared them. Results in `re/out/si-deform/load/` and `relset/`.

| Routine | Cases | Identical | Different | Not held | Mutation `=1` | Mutation `=calls` |
|---|---:|---:|---:|---:|---|---|
| `SI_envelopRebuild` | 104 | 104 | 0 | 0 | 17 differ; 90 change nothing in memory | no case records a call that takes an argument: every callee runs on both sides (coverage line) |
| `envelopDataConvert` | 3 | 3 | 0 | 0 | (in the line above) | (same) |
| `SI_envelopRelationSet` (`dump --relset`, `gen_relset.py`, 13 scenes) | 13 | 13 | 0 | 0 | 13 differ | nothing recorded to change (coverage line) |

The citations of the three functions stand. The cause of the earlier "not held" was a stale image set
(the images hold the loader's memory, so a loader change makes them unusable): after a change to
`si-scene`'s loader the images have to be dumped again.

### 19.2 `SI_envelopUpdateInitDef` (0x084b6b9c) *[soft 0x084b6b9c SI_envelopUpdateInitDef]*

Read from the listing (x87 shapes through `esym.py`). It carries the points of a skin that were edited by hand
back to the rest definition, through the matrices the bones gathered. In order:

- Nothing happens for a null node, for the word 7 in the node's envelope word, or when the node has no
  skeleton root. Otherwise the rest matrices of the skeleton are brought up to date (`SI_envelopSkelUpdate`).
- Up to two rounds, as long as `dampTabCreate` succeeds: `envelopSkel` deforms the skin (flag 1), then, by
  the skin's kind (1 patch, 4 mesh, 6 spline, 9 NURBS curve, 10 NURBS surface), each point of the
  present definition that is marked (bit 0, the sign bit, 0x20, `SI_mdlPnt` for meshes; `SI_mdlPnt` and 3
  for splines) is carried into the first stored definition.
- A point with no entry in the damping table is copied (a patch point gets 1.0 in w; NURBS points are
  divided by their weight and set through `SI_nurbsCrvMovePoint` / `SI_nurbsSrfMovePoint`). A point with an
  entry is taken through its list of edited-vertex elements: when its weight is under 99.99, `1 - 0.01 * w`
  is added to the four diagonal elements of the element's matrix (stored as single precision), the matrix is
  inverted (`matrixInverse`; a singular matrix keeps the inverse of the earlier point, which starts as NaN), and
  the point is multiplied by it with the original's x87 summation order (80-bit sums, one division by the
  weight, rounded to double only for the call arguments). The table is freed after each round.
- At the end, for the envelope words 5 and 8 on a root whose word is not 11, `envAutoSet(root, 0)` and
  `SI_modelUpdateSonState(father, root, 1)`; then `envVrtResetEditFlag(node)` and `editVertDispose()`.

Two constants it reads are copies of 0.01 (0x08660fb8) and 99.99 (0x08660fb0) in the data section; the port
holds them in `PERCENT_AT` and `FULL_AT`, which `check_env_data.py` holds against the binary (41 constants, 0 wrong).

Generator `re/tools/si-deform/gen_envinit.py` (scenes of `gen_envelop` with a 12 % variant for word 7, points
marked at random), registered in `gen2.py`; vectors `re/out/si-deform/env4/vec`.

| | Cases | Identical | Given the library | Different | Not compared |
|---|---:|---:|---:|---:|---:|
| `SI_envelopUpdateInitDef` | 4,000 | 1,920 | 1,831 | **0** | 249 |

Not compared: 235 where the original stopped on a stand-in stub (`SI_meshCopy`, `SI_nurbsCrvCopy`,
`SI_nurbsSrfCopy`, `SI_patchCopy`, `SI_skeChainRoot`, `SI_splineCopy`), 2 where both sides faulted, 1 where the
outside calls depend on unwritten memory, 10 where the port's result depends on unwritten memory, 1 that agrees
in the defined part. Mutation `=1`: 2,025 differ (the rest return early or have no marked point); `=calls`: 1,158
differ. Instructions: 1,072, 0 not reached.

### 19.3 `SI_envelopCut` (0x084bc4b8) *[soft 0x084bc4b8 SI_envelopCut]*

Called by `SI_modelCutFather` when a node is cut from its father (until now a hook in `si-model`). With no father
nothing happens. With a negative level the envelope is relaxed (`SI_envelopRelax(node, NULL, 0, 1)`) unless the
node is the main global one. At level 0, a node whose word is 11, and a chain root (kind 0, algorithm 1 or 2) whose
son's word is 4, are left alone; a node of word 4 or 10 that is the last such son of its father resets the father's
word 7 to 0 and, for every skin on the father's owner list, clears the skin's word, removes the relation
(`SI_elemdictRemoveRelationId(SI_RD, index, 0xcf)`), updates the dependency (`SI_modelUpdateDependency`) and
recomputes the patch surface (kind 1) or the mesh display (kind 4). Finally the node's info tree is disposed of
(`envelopInfoTreeDispose`), for every level.

Generator: `cut` in `gen_envinit.py` (`gen_envold.world` scenes with a second skin now and then, random words,
kinds and algorithms of the nodes). Vectors `env4/vec/SI_envelopCut.vec`.

| | Cases | Identical | Given the library | Different | Not compared |
|---|---:|---:|---:|---:|---:|
| `SI_envelopCut` | 3,000 | 2,988 | 4 | **0** | 8 (original faulted; the port faulted too after the same outside calls) |

Mutation `=1`: 2,237 differ (755 identical: the early returns); `=calls`: 785 differ. Instructions: 129, **2 not
reached**: a null test on a list element that is never null at that point (0x084bc566, 0x084bc568).

### 19.4 Re-run of the earlier sets and the crate checks

`env/vec` 75,640 cases, 0 different, 328 not compared; `env2/vec` 45,000, 0 different, 345 not compared; `env3/vec`
19,800, 0 different, 137 not compared; the lattice vectors 85,176, 0 different, 1,163 not compared: all as before.
`cargo test -p si-deform` 9 passed; `check_outside.py` 94 routines, 0 wrong; `check_env_data.py` 41 constants, 0
wrong; `check_globals.py si-deform` the known complaint about `SI_identite`. `cite.py` over the reports: 153 functions
cited; the five it lists as unchecked are those compared by other tools (the three of 19.1 by `deform-scenecheck`,
whose report format `cite.py` does not read; `SI_curveDistanceToValueFrenet` and `SI_postAnimTouch` by their own vectors).

Not ported, by decision: the nine functions of section 18. Nothing else of the crate is left unported.

To repeat:

```bash
export CARGO_TARGET_DIR=$PWD/target/slot-5
E="uv run --with unicorn python3 -I"
(ulimit -v 6000000; timeout 3000 $E re/tools/si-deform/gen2.py re/out/si-deform/env4/vec 1.0 gen_envinit)   # or: SI_envelopCut
(ulimit -v 2000000; timeout 600 $CARGO_TARGET_DIR/release/deform-emucheck re/out/si-deform/env4/vec)        # + EMUCHECK_MUTATE=1 / calls
$CARGO_TARGET_DIR/release/deform-scenecheck dump re/out/si-deform/load/dumps       # then gen_rebuild.py, check; dump --relset, gen_relset.py, check --relset
```

## 20. Sixth pass, routing1 (2026-10-28): the outside tables answered by the ports

`crates/si-deform/src/routed.rs` adds `Routed`, a wrapper around any `Outside`. For the routines
that another crate of the workspace has ported, it answers the call by running that port on the
memory image, with the stack-word conventions of the original; every other call goes to the
wrapped outside unchanged. 50 addresses are answered this way (`routed::ROUTED`), and each port's
own outside calls come back through adapter structs (`DictSide`, `ModelSide`, `SplineSide`,
`AnimSide`, `NurbsSide`, `EdSide`, `MotionSide`) so that nested calls are routed too. The environment
variable `DEFORM_NO_ROUTING` switches routing off.

Routed from the ports: the relation dictionary (12), copies and disposals of patch, mesh, spline,
NURBS curve and surface (10), the model tree and meshes (23, among them update-son-state, fitting,
dependency update, polygon and vertex normals), the curve and surface movers and the Frenet
routines (5), `SI_edFcvDispose`, `SI_edGetElemFromModel`, and the three motion routines (copy and
reset of element animation, `SI_modelResetFcv`). `SI_latticeTouch` is answered by this crate's own
`post_anim_touch` (same body as `SI_postAnimTouch`). The reverse direction: `SI_envelopCut`
(0x084bc4b8) is answered by `envtools::envelop_cut`.

Left as hooks, with the reason:

| Routine | Reason |
|---|---|
| `SI_edGetElemFromLattice`, `SI_edGetElemFromCluster` | port only in si-scene, which depends on si-deform: a cycle |
| `SI_dbFormatFullName` | the image form is only in si-scene; si-db works on a `ListNode` value form |
| `SI_dialogRequest2` | si-app's port is a UI request; the engine here is headless |
| `bgnline`, `endline`, `v3f` | si-gl's methods work on a GL context object, not on the memory image |
| `SI_modelPatchFittingReset`, `SI_faceGetIndexes`, `SI_userDataDispose` | only value forms exist (`Models`, `Face`, `Vec<UserNode>`), no image form |
| `SI_envelopCut`'s callers in si-model (`SI_ENVELOP_CUT` hook) and si-view | those crates are dependencies of si-deform: a cycle. si-app has no caller |

Checks. `deform-emucheck` uses the recorded outside, so routing does not touch it; re-run after
the change: lattice vec 85,176 cases (77,144 identical, 6,869 given the library, 0 different, 1,163
not compared), env 75,640 (0 different), env2 45,000 (0), env3 19,800 (0), env4 7,000 (0),
deformation 1,500 and Frenet 1,500 (all identical). The new `re/tools/routing1/scenecheck` is the
scene check with `ROUTED=1`: the port runs over `Routed` on top of an outside that answers 0.
`SI_envelopRebuild`: 103 cases, 103 identical, 0 different; mutation `=1` makes 13 differ (the ones
that change memory). `SI_envelopRelationSet`: 13 of 13 identical, mutation 13 differ. Routed calls
that were really exercised include edGetElemFromModel (912), meshComputesPolygonNormal (35,017),
meshComputesVertexNormal (28,107), nurbsSrfMovePoint (1,206) and the element-dictionary routines.
The lattice, cluster, fitting and motion copy/reset routes are reached by no scene check; they are
covered by the unit tests (`routed::tests`) and by compiling. With `ROUTED=1` the loader's own
compare is skipped, because the restore puts the bytes back but not the allocator's books.

## 21. Seventh pass, routing2 (2026-10-28): the hosts wrapped

The outsides of the hosts are now wrapped in `Routed::new(&mut host)`: `si-scene`'s `EnvSide`
(`SI_envelopRebuild`) and `DeformSide` (`SI_envelopActive`) in `image.rs`, and `si-rensetup`'s
`DeformSide` at its ten call sites in `native/motion.rs` (lattice transform, Frenet distance,
`SI_envelopActive`, `SI_envelopCompUpdate`, `SI_envelopDispDef`, `SI_envelopCopyDef`,
`SI_klusterCountVertexRef`, `SI_postAnimTouch`). The host's own code stays the wrapped world, so
every call no port answers (`SI_edGetElemFromCluster`/`Lattice`, the hooks of section 20) reaches the host as before.

First try: 13 of 296 first frames got worse (animated-boolean was already different; the 13 were
branch-deformation, CONSTRAINT_ENV-FEnvCnsClsObj, CopyGeometry-LatticeSphere, lattice-deformation,
Lockdown-Flag, Magnet-Snake, MODEL-IceCream, MOTION-digestive_system, MOTION-FLEX2, MOTION-FLEX3,
Motion-Fruit_Jelly_Jar, MOTION-jojo_happy_fish_2, WAVE-Balloon). Bisect by address (new switch
`DEFORM_ROUTING_SKIP=0xADDR,...`, `re/tools/routing2/bad.sh`: every one of the 49 other addresses
alone changed nothing, skipping only 0x0846e340 made all six probe scenes identical again):
**`SI_modelReadjustDefinition` (0x0846e340) is a hook again.** Reason: routed it runs
`readjust::readjust_definition` over `Routed`'s `ModelSide`, which does not know the callees the
hosts answer by ports of their own (`SI_waveTransform`, `SI_splineFit`, `SI_patchFit`, NURBS
curve/surface apply-matrix, `SI_checkActiveTime`, quick stretch, `SI_edFindUpdateNode`); those fell to
the wrapped world's answer 0. `si-rensetup`'s `ReadjustSide` has them. Making it routable means moving
those arms behind a crate below `si-deform` (not done). `ROUTED` has 49 addresses now.

Checks after the change (worktree routing2, HEAD 36509f8 plus the edit):
- routed scene check (`ROUTED=1`, after the `SI_envelopCut` arm): `SI_envelopRebuild` 103 cases, 103
  identical, 0 different; `envelopDataConvert` 3 of 3.
- `deform-emucheck` (recorded outside, not touched by the hosts): vec 85,176 cases / 0 different (77,144
  identical, 6,869 given the library, 1,163 not compared); env 75,640 / 0; env2 45,000 / 0; env3 19,800 / 0;
  env4 7,000 / 0.
- first frames, all 296 (`measure.sh PHASE=first`): routed 276 identical, 19 identical but the icon, 1 different
  (animated-boolean); with `DEFORM_NO_ROUTING=1` the same 276/19/1. (With 0x0846e340 routed: 263/19/14.)
- every frame of 15 scenes with envelopes, lattices or deforms (Beauty_shot, Man_V3_0, branch-deformation,
  lattice-deformation, Motion-Fruit_Jelly_Jar, Walk_Run-Sample_Actions, CONSTRAINT_ENV-FEnvCnsClsObj,
  General-Skeleton, Lockdown-Flag, Magnet-Snake, CopyGeometry-LatticeSphere, MODEL-IceCream, MOTION-FLEX2,
  MOTION-FLEX3, MOTION-digestive_system), 1,099 frames against the oracle: routed 1,071 identical, 1 identical but
  the icon, 27 different; without routing exactly the same per scene (the 27 are MOTION-FLEX3 frames, not routing).
  `re/tools/routing2/frames.sh LABEL scene...`.

The pictures do not change because the hosts already answered most of these addresses by the same ports
(`envelope_call`); the wrapping adds the ones they did not (the 49 addresses are reached by the hosts' deform
calls first) and makes the routing uniform. Other crates: `kit routing-debt --crate si-scene` finds 0 pairs.
`--crate si-rensetup` finds ten consts in `native/outside.rs`: `SI_image`, `SI_chunkFreeAll`,
`SI_openRenderWindow`, `SI_closeRenderWindow`, `SI_assignMaxcoc`, `SI_deleteRenderDictionary` are answered in `World::port`
already (or are the crate's own); `SI_getmi2Output`, `SI_spdlUseNamesrvImgRef`, `SI_cleanupTranslator` have ports in
`si-mental`, which `si-rensetup` does not depend on (a `Cargo.toml` and `Cargo.lock` change, a shared file); `getbutton`
is `si-gl`'s method on a context object, not the image. Not done.

## How to run

```bash
# the third pass (section 17):
export CARGO_TARGET_DIR=$PWD/target/si-deform
(ulimit -v 8000000; cargo build --offline --locked --release --manifest-path re/tools/si-deform/scenecheck/Cargo.toml)
(ulimit -v 2000000; timeout 3000 target/si-deform/release/deform-scenecheck dump re/out/si-deform/load/dumps)   # 101 images, 2.4 GB
(ulimit -v 6000000; timeout 7200 uv run --with unicorn python3 -I re/tools/si-deform/gen_rebuild.py \
    re/out/si-deform/load/dumps re/out/si-deform/load/vec [--part I/N])          # a few minutes; --cover
(ulimit -v 2000000; timeout 3000 target/si-deform/release/deform-scenecheck check re/out/si-deform/load/vec re/out/si-deform/load/dumps)
#   EMUCHECK_MUTATE=1 / =calls; DEFORM_SCENECHECK_SHOW=1 prints where two sides' blocks part
(ulimit -v 8000000; cargo build --offline --locked --release --manifest-path re/tools/si-motion/evalcheck/Cargo.toml)
re/tools/si-deform/evalrun.sh <label> [options]    # motion-evalcheck with stand-ins on and off; evalrows.py prints the rows
RENSETUP_BIN=target/si-deform/release/rensetup-check re/tools/si-scene/pass8_pictures.sh <label>

re/tools/si-deform/check_env2.sh 1.0    # second pass: build, unit tests, env2 vectors (gen2.py, strict session),
                                        # check, both mutation runs, the lattice and first-pass vectors
                                        # re-checked against their totals, citations by all three reports,
                                        # globals, addresses, constants, coverage
# the second pass step by step:
export CARGO_TARGET_DIR=$PWD/target/si-deform
E="uv run --with unicorn python3 -I"
(ulimit -v 6000000; timeout 3400 $E re/tools/si-deform/gen2.py re/out/si-deform/env2/vec 1.0)   # [only NAME|MODULE ...]
(ulimit -v 2000000; timeout 1800 target/si-deform/release/deform-emucheck re/out/si-deform/env2/vec)
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 1800 target/si-deform/release/deform-emucheck re/out/si-deform/env2/vec)
(ulimit -v 2000000; EMUCHECK_MUTATE=calls timeout 1800 target/si-deform/release/deform-emucheck re/out/si-deform/env2/vec)
python3 -I re/tools/si-deform/cite.py re/out/si-deform/env2/check_all.txt   # the three reports (check_env2.sh writes it)
(ulimit -v 6000000; $E re/tools/si-deform/dcov.py re/out/si-deform/env2/coverage -v)
re/tools/si-deform/el2.sh NAME                                         # a labelled listing into env2/lst

# the first pass:
re/tools/si-deform/check_env.sh 1.0     # build, unit tests, envelope vectors, check, both mutation runs,
                                        # the lattice package's vectors re-checked, citations, globals,
                                        # addresses, constants, the map, coverage
# or step by step:
export CARGO_TARGET_DIR=$PWD/target/si-deform
(ulimit -v 8000000; cargo test -p si-deform; cargo build --release -p si-deform)
E="uv run --with unicorn python3 -I"
(ulimit -v 6000000; timeout 3400 $E re/tools/si-deform/gen.py re/out/si-deform/env/vec 1.0 gen_envtools gen_envelop gen_envset)
(ulimit -v 6000000; timeout 3400 $E re/tools/si-deform/gen.py re/out/si-deform/env/vec 1.0 gen_envskin)   # a second process
(ulimit -v 2000000; timeout 1800 target/si-deform/release/deform-emucheck re/out/si-deform/env/vec)
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 1800 target/si-deform/release/deform-emucheck re/out/si-deform/env/vec)
(ulimit -v 2000000; EMUCHECK_MUTATE=calls timeout 1800 target/si-deform/release/deform-emucheck re/out/si-deform/env/vec)
(ulimit -v 2000000; timeout 1800 target/si-deform/release/deform-emucheck re/out/si-deform/vec)   # finding 0028's totals
python3 -I re/tools/si-deform/cite.py BOTH_REPORTS        # the two reports concatenated (check_env.sh: env/check_all.txt)
python3 -I re/tools/si-mem/check_globals.py si-deform
python3 -I re/tools/si-deform/check_outside.py
python3 -I re/tools/si-deform/check_env_data.py
python3 -I re/tools/si-deform/emap.py re/out/si-deform/env/check.txt     # the table of section 8
(ulimit -v 6000000; $E re/tools/si-deform/dcov.py re/out/si-deform/env/coverage -v)
python3 -I re/tools/si-deform/env_outside_table.py --write             # the outside_env! block
re/tools/si-deform/el.sh NAME                                          # a labelled listing into env/lst
python3 -I re/tools/si-deform/esym.py NAME                             # the x87 reading aid, compacted
```

```bash
# the routed scene check (section 20):
(ulimit -v 8000000; cargo build --release --manifest-path re/tools/routing1/scenecheck/Cargo.toml)
ROUTED=1 target/slot-5/release/routing-scenecheck check re/out/si-deform/load/vec re/out/si-deform/load/dumps
# the hosts wrapped (section 21): first frames routed / DEFORM_NO_ROUTING=1 (through re/tools/lead/slot.sh emu --)
PHASE=first re/tools/measure/measure.sh LABEL BINDIR x           # BINDIR holds render-frames
re/tools/routing2/frames.sh LABEL scene-substring...            # every frame; DEFORM_ROUTING_SKIP=0xADDR,.. bisects
```

## Where it stands

- renderfin1 (2026-10-28): `SI_envJntAssBoundCreate` (0x084bdcec), `SI_envJntAssBoundDraw`, `SI_envJntAreaLimDraw` stay unported:
  they build or draw display-only volume cubes (flag 0x20), not part of the deformation. The decision of this finding stands.

- Sections 1 to 19: envelope, lattice, scene images ported and checked (see the table above).
- env1 (2026-10-29): `envelopDataConvert` (0x084bcddc) 3/3, `SI_envelopRebuild` (0x084bd2a0) 104/104, `SI_envelopRelationSet` (0x084bd5a8) 13/13 identical, 0 different
  (re-dumped scenes through `deform-scenecheck` into `re/out/env1/{rebuild,relset}`; the old dumps were stale after the loader change; build the scenecheck crate with `--offline`);
  `=1` differs in 3 / 14 / 13; `=calls` has nothing to flip. `SI_postAnimTouch` (0x08153618) and `SI_curveDistanceToValueFrenet` (0x081561dc) re-checked 1500/1500, 0 different; all five now cited in full form.
- Section 20 (routing1): `si_deform::routed::Routed` answers 49 outside addresses from the ports; 0 different in every existing check and in the routed scene check.
- Section 21 (routing2): the hosts (si-scene `EnvSide`/`DeformSide`, si-rensetup `DeformSide`, 10 sites) are wrapped in `Routed::new`. 0 different in the emucheck sets and the routed scene check; first frames 276/19/1 of 296 routed and unrouted; 15 deform scenes (1,099 frames) identical routed and unrouted.
- `SI_modelReadjustDefinition` (0x0846e340) left a hook: routed it loses the hosts' callee ports (section 21). Switches: `DEFORM_NO_ROUTING`, `DEFORM_ROUTING_SKIP=0xADDR,...`.
- Open: the hooks listed in section 20 stay until the cycle or image-form obstacle goes (si-scene's edtools into a crate below si-deform; image forms for the value-only ports; the readjust callees below si-deform). `si-rensetup` could route `SI_getmi2Output` and friends to `si-mental` after a dependency (shared files).
