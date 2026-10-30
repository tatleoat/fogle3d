# 0037 boolean (package 19, boolean.c) - crate si-boolean

## Where it stands

- Unit: `boolean.c`, 75 functions. Crate `si-boolean` (`boolean.rs`, plus `split.rs`, `clean.rs`, `mesh.rs` for the seven
  functions of the last batch), checkers `boolean-checkkit` (recorded stand-ins) and `split-checkkit` (the original's own
  constructors run with their Rust routing).
- Ported and checked: 75 of 75. The first 68 on 150 cases each with 0 different (details per batch below). Last batch
  (cases = identical / equal on defined bytes / different / not compared): si_splitFace 150 = 60/66/0/24 (24 where the
  original itself stopped or wrote past a buffer); si_cleanUpEdges 600 = 598/0/0/2 (budget); si_cleanUpFaces 800 =
  769/0/0/31 (budget); si_generateFaceList 400/0/0/0; si_buildMesh2 400; si_buildMesh 400; SI_computeBoolean 400 =
  109/291/0/0 (all equal on defined bytes). Mutation `=1` makes every case differ; `=calls` differs where the function
  makes outside calls (si_cleanUpFaces, si_generateFaceList, si_buildMesh2, si_buildMesh, SI_computeBoolean) and shows
  nothing on si_splitFace and si_cleanUpEdges, which call nothing outside.
- Unreached bytes (kit coverage): splitFace 0, cleanUpEdges 90 of 1572, cleanUpFaces 38 of 1708, generateFaceList 14,
  buildMesh2 33 of 2105, buildMesh 27, SI_computeBoolean 83 of 1373 (the setjmp-nonzero error path).
- Not checked: the longjmp error path of SI_computeBoolean (the harness records `_setjmp` with one fixed answer, 0);
  the tessellation path of si_buildMesh2 runs only with recorded answers for SI_triangulateContour.
- Routing debt: the five `SI_SPLIT_FACE` call sites (dummyInsert, classifyRegion, si_buildBsp, classifyFaces, xxxx) still
  call the recorded stand-in (`split_answers`), not `si_split_face`; their emulator checks are unchanged. Other hooks
  (the six bool_util constructors are now ported in util.rs and wired;
  SI_malloc, SI_nodeGetUV/SetUV, SI_matrixZAlign, SI_triangulateContour, SI_polygonAllocate/Initialize, SI_nodesCopy) are
  answered by recorded stand-ins; `kit routing-debt --crate si-boolean` is strict 0 pairs.
- bool2 (2026-10-29), `ops.rs`, checker `ops-checkkit`, vectors `re/tools/si-boolean/gen_ops.py` (60 cases each; identical /
  equal on defined bytes / different / not compared): SI_meshClipPlanes 22/38/0/0; SI_meshCutPlanes 2/53/0/5;
  si_copyModelMat 60/0/0/0; SI_animBooleanUpdate 48/12/0/0. Mutation `=1` differs in every case with an output; `=calls`
  differs wherever the function calls outside. Unreached: animBooleanUpdate 49 of 1330 bytes, meshCutPlanes 39 of 1316,
  meshClipPlanes 2. Run: `gen_ops.py OUTDIR 60`, then `ops-checkkit OUTDIR`.
- bool3 (2026-10-29), `util.rs` (bool_util.c, all 6) and `merge.rs` (`SI_meshMerge`), checkers `util-checkkit`, `merge-checkkit`,
  vectors `gen_util.py` / `gen_merge.py` (cases = identical / equal on defined bytes / different / not compared): the six
  constructors 900 = 900/0/0/0 (150 each; mutation `=1` differs in all 900; `=calls` nothing, they call nothing outside);
  SI_meshMerge 150 = 96/54/0/0 (mutation `=1` and `=calls` differ in 150). Unreached in SI_meshMerge: the too-many-elements
  message (73 bytes), a few single branches. The constructors' routing debt is settled (`ops::bool_util_stand_in` and the
  checkers' route call the ports). Not ported on purpose (modeller UI/command, outside the renderer): displayMergeDialog,
  SI_mergePerform, SI_dialogAnimBoolean, SI_dialogInfoAnimBoolean, displayBooleanDialog, doBoolean, SI_booleanPerform.
- Run: generate with `re/tools/lead/slot.sh emu -- uv run --with unicorn python3 -I re/tools/si-boolean/gen_boolean.py OUTDIR 150 '^(regex)$'`,
  check with `boolean-checkkit OUTDIR` / `split-checkkit OUTDIR`, mutate with `EMUCHECK_MUTATE=1` / `calls`.

## 2026-10-29 bool2 (callers of the boolean engine)

Ported with the cite given after the vectors matched: [soft 0x083158b0 SI_meshClipPlanes], [soft 0x08316ba0 SI_meshCutPlanes],
[soft 0x0830bc30 si_copyModelMat], [soft 0x0830a7b0 SI_animBooleanUpdate].

Read from code: the plane tree array (0x30 bytes per plane, face slot at +0x24 cleared), the cut loop (side 3 front
dropped / side 4 split with `*pfront` kept for clipping; for cutting sides 1 and 2 go to the output chain), the plane
positions in 80 bits (`pos += (max-min)/(n+1)`), the unit-normal templates and the d-globals at 0x086b99b8/d8/f8, the
mesh fields carried to the result (+0xc, +0x10, +0x20), the material array handling (count short at +0x206, array at
+0x20c, four words at +0xdb0 cleared in copies), and the animated-boolean flow with its setup struct (0x9c bytes).
Guessed: field names only. The setup words at +8 and +0x10 are stack leftovers in the original (+8 holds the material
mode); the port writes the mode and 0.
Method: the boolean-internal constructors, si_splitFace and SI_malloc run as the original's own code in the emulator and
are routed to the Rust ports in `ops-checkkit` (shared `route.rs` with `split-checkkit`); the mesh and model library,
the matrices and the materials are recorded stand-ins; `free` and `SI_calloc` are the allocator's own. Counts below 0
for the cut planes make the original run away (read faults), so the vectors use 0 to 3. The UNINIT cases are the
original writing heap bytes that come from memory nobody wrote; the check compares only the defined bytes there. The 5
not-compared cut cases are ones where the port's own result depends on such memory, as the original's does.
Merge (`SI_meshMerge`, `SI_mergePerform`) is not started: it is a modeller command.

## 2026-10-28 last batch (si_splitFace and the six drivers)

Ported with the cite given after the vectors matched: [soft 0x08313b74 si_splitFace], [soft 0x08312038 si_cleanUpEdges],
[soft 0x0831265c si_cleanUpFaces], [soft 0x08312f58 si_generateFaceList], [soft 0x083110c4 si_buildMesh2],
[soft 0x08311900 si_buildMesh], [soft 0x08315348 SI_computeBoolean].

Read from code (decompile and listing): control flow, the layouts below and the constants. Guessed: only field names.
Layouts: mesh +0 points (stride 0x14), +4 polygons (stride 0x18), +8 edge table, +0x14/+0x18/+0x1c counts; polygon
node stride 0x24; face fields from +0x54 as in the first batch; the SI_BoolSetup offsets used by SI_computeBoolean;
the face id counter at 0x086b9858; the collision-mode byte `SI_actorCollisionMode` at 0x086d7142.
Constants (hard-coded, they are not in the vector memory): 1e-6 at 0x08657c30, 5e-7 at 0x08657bf8, 1e-3 at 0x08657c00,
2.5e-13 at 0x08657c38. The one bug the checks found in si_splitFace was a tolerance read as the wrong constant (1e-6).
Method: the original's own constructors (si_createEdgeNode etc.) run in the emulator for the splitFace vectors; the
Rust side routes them in `split-checkkit` so both sides build the same scene. Import stand-ins (`mallopt`, `signal`,
`_setjmp`, `getenv`) cannot be in the outside list; they are recorded with fixed answers. Each freed buffer needs a
distinct recorded answer.

## 2026-10-28 first batch

Ported with a cite only after the vectors matched: [soft 0x0830c990 fatal_error], [soft 0x08315180 si_get_mem],
[soft 0x0830c9a8 get_point], [soft 0x0830cec4 find_point_id], [soft 0x0830cf10 get_edge], [soft 0x0830cfe4 find_edge_id],
[soft 0x0830d02c reset_edge_id], [soft 0x0830d064 nb_res_features], [soft 0x0830d0d4 extract_face],
[soft 0x0830d0f4 reset_face], [soft 0x0830d154 reset_list_of_faces], [soft 0x0830d17c open_object],
[soft 0x0830d978 find_edge_node], [soft 0x0830ee8c mark_selected_face], [soft 0x0830eecc find_last_face_of_list],
[soft 0x0830eef4 merge_list_of_faces], [soft 0x0830ef2c merge_two_lists_of_faces], [soft 0x0830f20c set_status],
[soft 0x08310374 remove_face_from_list], [soft 0x0830ff44 reverse_face], [soft 0x08310100 reverse_tree],
[soft 0x08310894 set_merge_dist].

Layouts (read from code): face (plane a,b,c,d at 0/8/0x10/0x18, min 0x20, max 0x38, id 0x50, result id 0x54, count 0x5c,
status 0x64 with 1 unknown / 2 inside / 3 outside, flag 0x65, contours 0x68, regA 0x6c, regB 0x70, prev 0x74, next 0x78);
contour (first node +0, next +8); edge node (visit +0, end byte +4, edge +8, prev +0xc, next +0x10); edge, 0x7c bytes
(mark +0, p0 +8, p1 +0xc, faceA +0x10, faceB +0x14, four attribute vectors from 0x18, next +0x78); point, 0x5c bytes
(x,y,z, dist +0x48, plane +0x50, result id +0x54, count +0x58, side +0x5a, flag +0x5b); point block 0x934 bytes (25
points from +0x30, prev +0x92c, next +0x930); edge block 0x3078 bytes (100 edges, prev +0x3070, next +0x3074);
tree node (kind +0, 1 = leaf; plane doubles at +4; faces +0x24, back +0x28, front +0x2c). Globals are at 0x086b9800..0x086b9858,
0x087ba880, 0x087ba8a0, 0x087ba940 (mergeDist), 0x087ba948 (oper), 0x087ba960 (plList, 50 entries of 0x20 bytes).

Notes for the next porter: in the decompile `(byte)(a<b | NaN | EQ)==1` means a<b and `(byte)(...)-1 < 0x40` means a<=b.
Sums of products run in 80-bit (use si_x87::F80); stores to f64 locals round. Uninitialised stack reads exist in
addPtToEdge, computeStatus and splitEdge (the point copy in getPoint): Rust uses zeros, and the vectors must not depend on them.
getPoint copies 0x5c bytes from a stack temp, so only x,y,z are defined afterwards. resetFace/resetListOfFaces leave a stray
eax; the check treats their result as none. si_getMem returns 0 after the longjmp hook returns (documented in the code).
Constants read so far: 5e-5 / -5e-5 (0x08657b50..ba0), 1e-6 (b48), 5e-7 (b40, bd8 for computeStatus), 1e-4 (b90).

Kit candidate: `re/tools/si-boolean/gen_boolean.py` has a scene builder (sparse layouts, pooled blocks, globals declared
by default) that other pointer-graph units could reuse.

## 2026-10-28 second batch

Ported, each held against the original on 150 cases: [soft 0x0830e680 ptIsOnPlane], [soft 0x0830e6fc getPtPos],
[soft 0x0830d3d0 testBox], [soft 0x0830e4f0 si_whichSide], [soft 0x0830ccd0 locatePt], [soft 0x0830cbac findPoint],
[soft 0x0830e588 splitEdge], [soft 0x0830e78c addEdgeToInter], [soft 0x0830e7c4 addInterNode],
[soft 0x0830e8f0 getInterListNode], [soft 0x0830f0d0 classifyPoint], [soft 0x0831019c computeStatus],
[soft 0x08310864 getNbEdges], [soft 0x083107f8 deleteEdge], [soft 0x083108a4 si_setClassifying],
[soft 0x083108b4 si_setCleanup], [soft 0x08315298 si_initBoolean], [soft 0x08310558 divideList],
[soft 0x083106b4 colinear], [soft 0x0830d1bc computeFaceSize], [soft 0x0830d2d0 si_computeFaceGroupSize],
[soft 0x08311ee0 nearPointEdge], [soft 0x08311f90 mergeSimilarEdge], [soft 0x08312d08 computeNormal],
[soft 0x08312e84 computeAveragePt], [soft 0x0830f338 resetRegion].

Float order (from the x87 listing, which differs from the decompile's grouping in getPtPos): plane distance is
`(((x*a + y*b) + z*c) + d)` in 80 bits; sides at +-5e-5 (strictly above: 3 and back, strictly below: 2 and front, else 1 and
dist 0). Squared distances are `(dx*dx + dy*dy) + dz*dz` of unrounded 80-bit differences (nearPointEdge adds `dy*dy + dx*dx`
first, computeNormal's length `(B*B + A*A) + C*C`). Running sums in computeStatus/computeAveragePt/computeNormal are stored
as doubles every iteration, but the final division uses the unrounded last sum. addInterNode divides by the length rounded to
f32 (only when that exceeds 1e-4), stores the key as a double but compares the 80-bit value while inserting.
splitEdge: t = dist / (((b*dy) + (a*dx)) + (c*dz)).

Layouts added: inter node, 0x44 bytes (point +0, key double +0x1c, edge-list tail +0x2c, head +0x38, prev +0x3c, next +0x40,
ints at 0x24/0x28/0x30/0x34 = -1 when fresh), held in 0x10-byte cells (node +0, next cell +8) chained from firstInterList;
face-list cell (face +0, +4 data, prev +8, next +0xc); face group (min +0, max +0x18, first face +0x30).

Caveats: splitEdge's new points keep the callee's stack garbage in bytes 0x18..0x5c (the checker compares only defined bytes,
hence "defined"). computeStatus reads an uninitialised stack word when nbPl is 0 and the answer was 1; the vectors cannot reach
that (classifyPoint answers 1 only after pushing a plane). classifyPoint's plane push writes slot nbPl before testing the cap
(nbPl 50 would write past plList), so vectors keep nbPl below 50. divideList on 7 of 150 made-up scenes faulted in the original
(not compared).

## 2026-10-28 third batch

Ported from the listings and checked against recorded runs of the original: addPtToEdge [soft 0x0830d50c addPtToEdge],
insertEdge [soft 0x0830d834 insertEdge], splitOnEdge [soft 0x0830ea78 splitOnEdge], findOnEdge [soft 0x0830eb58 findOnEdge],
buildFace [soft 0x0830e9c8 buildFace].

Layouts learned. Edge attributes: four 3-double vectors at +0x18, +0x30, +0x48, +0x60; edges of a ring link through +0x78.
Inter-list node: point +0, vector +4..0x1c, key (double) +0x1c, words +0x24/0x28/0x2c and +0x30/0x34/0x38, prev +0x3c,
next +0x40. Face-list cell: face +0, bsp +4, prev +8, next +0xc. Contour: first +0, next +8. Callee shapes: createFaceList
(0x10 bytes: face, tree, 0, 0), createContour (0xc: node, 0, 0), createRegBspNode (0x30: 1, 0, arg), createBoolFace (0x80,
copies the source if given).

Float notes: sums run in 80-bit and round only at stores; squared distances use unrounded differences. Caveats: addPtToEdge
leaves temp vectors uninitialised when a face is null (zeros in Rust, "defined" in the check); findOnEdge dereferences a null
`next` on 29 made-up scenes (not compared).

## 2026-10-28 fourth batch

Ported from the listings, each held against the original on 150 cases (0 different): cleanUpTree [soft 0x083103c8 cleanUpTree]
150 identical; dummyInsert [soft 0x0830efc4 dummyInsert] 150; processRegion [soft 0x0830f234 processRegion] 150;
classifyRegion [soft 0x0830f390 classifyRegion] 132 + 18 not compared; classifyCoincident [soft 0x0830f5e8 classifyCoincident]
145 + 5 not compared; si_buildBsp [soft 0x08313648 si_buildBsp] 137 + 13 not compared; classifyFaces [soft 0x0830f794
classifyFaces] 137 + 13; xxxx [soft 0x0830faec xxxx] 140 + 10; polygonTesselate [soft 0x083108cc polygonTesselate] 150;
computeTriangleBarycentricArea [soft 0x08310be8 computeTriangleBarycentricArea] 150; computeBarycentricColor
[soft 0x08310e74 computeBarycentricColor] 150; computeBarycentricUVCoord [soft 0x08311020 computeBarycentricUVCoord] 150;
si_getResultingFaces [soft 0x08313ab0 si_getResultingFaces] 150; si_releaseMem [soft 0x08315260 si_releaseMem] 150.
(Not compared = the original reads or writes unmapped memory or runs out its budget on a made-up world.)

Mutation: `EMUCHECK_MUTATE=1` makes every compared case differ for cleanUpTree, dummyInsert, classifyRegion, si_buildBsp,
computeBarycentricColor, computeTriangleBarycentricArea, polygonTesselate, classifyFaces (137 of 137) and
si_getResultingFaces; for processRegion (129 of 150: 21 produce nothing), classifyCoincident (131 of 145: 14 produce
nothing) and xxxx (125 of 140: 15 only set statuses of an empty list) the cases that produce anything differ.
computeBarycentricUVCoord and si_releaseMem produce
no value (the first writes through the SI_nodeSetUV stand-in, the second only frees), so only `calls` can differ: all 150
for the first; none for the second (its frees are compared but not mutated). `=calls` differs in every case that reaches a
hook (dummyInsert 22, classifyRegion 6, classifyCoincident 48, classifyFaces 39, xxxx 6, polygonTesselate 95, UV 150).

Layouts and notes. Face group (0x34 bytes in the original, 0x38 in the generator): min +0, max +0x18, first face +0x30.
Region cell (si_createFaceList, 0x10 bytes): face +0, tree +4, prev +8, next +0xc. Leaf tree node: kind 1, region-cell head +4,
inside +8. classifyRegion statuses: union back 2 front 3, intersection/difference back 3 front 2. si_buildBsp: scratch
groups come from SI_malloc, the node from si_getMem (0x30), the axis search takes the first of x,y,z whose box side beats
the best so far (80-bit), the original leaves `group.first.prev` pointing at the part head. xxxx is classifyFaces that keeps
the faces: a cut face moves its two region cells (+0x6c, +0x70) onto the back part and links a fresh cell for the front part
(si_createFaceList(cell.tree, front)); the three lists are joined coincident, back, front. polygonTesselate: points are moved by
the 4x4 float matrix of SI_matrixZAlign as `((x*e0 + y*e4) + z*e8) + e12` in 80 bits, triangles come from
SI_triangulateContour (out-locals count and triangle array), nodes of the old polygon are copied with SI_nodesCopy onto
matching corners; on a failed triangulation the scratch blocks are freed and `*pcount` is not written. The barycentric
functions use 80-bit sums of f32 products; computeBarycentricColor cuts toward zero and clamps only from above (a negative
channel stays negative); computeTriangleBarycentricArea accepts a polygon when each pair of weights sums to at most 1.000001
(0x08657be8) and carries a private copy of the SI_areaTriangle3D arithmetic (si-math owns the port; a dependency would
need a root Cargo.lock change).

Generator lessons: the stand-in for SI_malloc also answers the SI_malloc calls inside si_getMem, so a case whose memory
globals ask for a fresh block eats a spare group and the node lands in the guard gap (OVERRUN); cases for si_buildBsp and
classifyFaces therefore always carve from a given block (`cur(..., carve=True)`).

## 2026-10-28 fifth batch

Ported: [soft 0x0830d9f4 verifyEdge] (`verify_edge`). Layout learned: a point holds its distance along the edge line (double at
+0x48) and a visit id (+0x50, set to count+1 once placed); the interval list uses 0x1c-byte nodes from si_get_mem (edge +0,
lo +4, hi +0xc, prev +0x14, next +0x18) kept sorted by lo; the thresholds are 5e-5 (plane distance), 5e-7 (axis difference)
and 1e-6 (interval merge). The direction vector is x87: squares and sums stored as doubles, the root kept 80-bit, the length
and unit components stored as doubles. One branch (`len < db` in the "start outside, end inside" case) cannot be reached by
construction and is kept for fidelity.
Check: 1500 cases, 1232 identical, 171 defined, 0 different, 97 not compared (a 150-case run: 128/15/0/7).
Mutation: `EMUCHECK_MUTATE=1` makes 143 of 143 compared cases differ; `=calls` changes 0 of 143 because the only hook reached,
si_createEdgeNode, takes no argument. Coverage: 63 of 2812 bytes unreached, all alignment padding plus the dead branch.

## 2026-10-29 bool3 (bool_util.c and SI_meshMerge)

Ported with the cite given after the vectors matched: [soft 0x083170d0 si_createFaceGroup], [soft 0x083170ec si_createEdgeNode],
[soft 0x08317118 si_createContour], [soft 0x08317144 si_createBoolFace], [soft 0x08317208 si_createRegBspNode],
[soft 0x08317234 si_createFaceList], [soft 0x08315c64 SI_meshMerge].

Read from code: the constructors take their block from `si_getMem` (0x14, 0xc, 0x80, 0x30, 0x10, 0x34 bytes) and write only
the named fields, so the rest of a fresh block is whatever the allocator held (compared on defined bytes). `si_createBoolFace`
with a source copies 0x20 words, without one writes the fields listed in `util.rs`; both clear +0x54, +0x74, +0x78.
SI_meshMerge(n, models, mode, dist): sets the merge distance to dist squared (80-bit product stored as a double), converts every
model to a mesh and moves it by the model's matrix at +0x1c4 (with `SI_matrixTransposeInverse` for the normals); the first
mesh gives the result's +0xc and +0x10 floats and flag word, later meshes combine flags (bit 4 if any, bit 2 only if all). A
model without a mesh gives the max-polygons warning and 0. Mode 0 joins the arrays: polygons (0x18 bytes, node blocks of
0x24 with vertex, signed edge, three floats), edges (1-based, paged by 0x400 of 0x14 bytes) and vertices (0x14 with 0xc-byte
nodes), every index shifted by what the earlier meshes hold (vertices, edges, polygons), polygon materials by the earlier
models' material counts (short at model +0x206); node blocks are rounded up to a multiple of four and zeroed. Other modes run
the boolean engine mesh after mesh (classifying from the second), triangulating non-planar meshes, and build the result with
`si_buildMesh`. The mesh array is not freed on the normal path (the original leaks it). The frame of the original is kept as
one local block addressed from `ebp`, because the face group at ebp-0x40 shares its list head with the face-list pointer.
Guessed: field names only. Run: `gen_util.py OUT 150` / `gen_merge.py OUT 150`, then `util-checkkit OUT` / `merge-checkkit OUT`.

## Leftovers pass, 2026-10-29 (unit misc2)

Remaining unported in si-boolean: `SI_mergePerform` (0x0831669c) and `SI_booleanPerform` (0x0830c76c), the modeller commands that open `displayMergeDialog` / `displayBooleanDialog` and then call the engine, and the animated-boolean dialogs `SI_dialogAnimBoolean` / `SI_dialogInfoAnimBoolean`. The engine and the animated-boolean evaluation are ported. Skipped as not render. No code added.
