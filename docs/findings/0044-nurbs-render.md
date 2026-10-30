# 0044 - trimmed NURBS tessellation (trimmed.c) and the render side of si-nurbs

Unit `nurbsrender` (package 13 si-nurbs, Wave A). Aim: a rendering engine for Blender, so the
trimmed-surface triangulation comes first, then NURBS ray tracing (ray_trace.c) and the
composite trim curves (composite.c). Code: `crates/si-nurbs/src/trimmed.rs`.

## Where it stands

Packages 14 / 15 (si-nurbs) after units trim9, trimtri1 and the trim8 follow-up (2026-10-29). Every function of the files below is ported,
cited and held to the emulated original (0 different anywhere; the stale linearTrimCrvsAdjust vectors were regenerated, see "Trim8 follow-up";
the numbers are in the dated sections), except the ones listed as open. Whole `re/out/si-nurbs/vec44`: 82 files, 15580 cases, 14506 identical,
387 defined, 0 different, 681 not compared.
- TriangulateTrimmedNurbsSrf with the original's own callees (Trimtri1): the corpus's two trimmed surfaces (Blend-Plane.1-0, from the
  scene dump) 32 cases, 0 different (32 defined: the vertex records' frame words); made-up surfaces 100 cases, 36 identical, 18
  defined, 0 different, 46 not compared (the original stopped). The pictures cannot show them (Blend-Plane's frames are a sub-region
  without them; 100 of 100 frames identical anyway).
- trimmed.c, 31 functions (`trimmed.rs`, `nurbs-rendercheck`): all. ray_trace.c, 32 functions (`ray_trace.rs`): all. Callees outside the
  file go through `Outside` (`Routine` constants in ray_trace.rs, trimmed.rs): the three bezPatch routines are routed like the originals,
  the rest are recorded stand-ins (`kit routing-debt --crate si-nurbs` lists the free wiring).
- composite.c, 39 functions (`composite.rs`, `composite2.rs`, `composite3.rs` with the generic tree of `crates/si-gen/src/tree.rs`): all but
  `si_nurbsCompositeCurveDump`. trim.c (`trim.rs`, `trimprev.rs`): 32 of 33, open `InterCrvDump`.
- curveInter.c (`curveinter.rs`, `curveinter2.rs`, `curveinter3.rs`): 26 of 29, curveOrient.c (`curveorient.rs`, `curveorient2.rs`): 20 of 21;
  the open ones are the Dump routines. IntersectLists is cited with a caveat (70 of 295 compared, Trim8).
- intersec.c (`inter.rs`): 18 of 21 (open: InterDump, dInter, InterDist2D, which is ported and answers in st0, not compared). ray.c: all
  but RayDump, dRay (debug prints); the search and link group is `raylink.rs` (trim9); RayDist3D is `inter::ray_dist_3d` (st0, not compared).
- trimUtils.c (`trimutils.rs`, `trimutils2.rs`, `trimutils3.rs`, `trimstat.rs`): all but the three print routines and
  SI_nurbsSrfHighlightTrimCurve (a draw-state flag setter on a function-local static; not renderer relevant).
- trimDial.c (60 functions, package 15) is the trim dialog: UI, not ported here.
- Records (our names): uvSample 0x24 (next, prev, mate, u, v, flags, index); SampleCrv 8; SampleCrvSegment 0x18; vStrip 12; GenTreeElem 0x18;
  BezPatch 0x7b0; intersection 0x78; ray 0x60; trim point 0x24 (x, y, ..., +0x20 tag); linear curve record: order +0, count +4, points +0x2c,
  step +0x34, tag +0x50. Arithmetic is the x87's (80-bit sums, rounding at stores, `si_x87::F80`).
- Quirks kept: splitBez* leak when the net size is 5; a closed curve whose knots all lie within 1e-5 of the parameter makes the original
  read in front of the knot array ("not compared"); findNeighborInPlane reads one point past the sample array when the intersection sits on
  the last knot (kept out of the vectors); FindClosest stores an unwritten stack word where the port stores 0 ("defined").
- Pipeline of the trimmed triangulation (outline): SampleKnotVector, CreateDomainBoundary / NewSampleCrv, AddUCritcalPoints, MakeBreakVector,
  AddUKnotsToSampleCrvs, strips, IntersectVStrips, EncloseTrimRegions, AddHorizontalSamples, UnlinkMates, CountVerticesTriangles,
  EvaluateVertices, OutputTriangles, FreeTrimmedTemporaryData.

## Trim1 (2026-10-28): intersections, rays, trim status

Read from the code (`kit show`/`dis`): an intersection record is 0x78 bytes (point +0, direction +0x18, u +0x30, v +0x38,
ray +0x40, t +0x44, in-curve short +0x4c, flags +0x50, link distance +0x64, number +0x6c, previous/next link +0x70/+0x74);
a ray is 0x60 bytes (origin +0, direction +0x18, t +0x30, end point +0x38, count +0x50, generic list of intersections
+0x54). The lists are the generic lists of si-gen; `inter.rs` includes si-gen's `list.rs` by path (a `si-gen` dependency
of si-nurbs would replace that; it needs a Cargo.lock change) and is therefore left out of test builds.
Ported and checked: si_nurbsInterCreate, CreateP, Destroy, ComputeRayT, FindClosest, Link, Unconn, UpdateNextCon,
CrossBndry, ClassifyBndry, CopyFlags, GetInCurve, SetInCurve, AlreadyThere, Copy, CompT, BuildLinearCrv; si_nurbsRayCreate,
CreateP, Copy, GetInters, GetNoInter, Destroy, AddInter, InitRayCast, nurbsRayValidNewRay; trimUtils.c: si_writeStack,
si_readStack, si_popStack, SI_initTrimStatus, si_updateTrimStatus, si_trimStatAllocate, si_trimStatDispose,
si_trimLineAllocate, si_trmDataRem, si_countNumLines, getCrvIndex, updateStepAndOrder, ptsOnBorder, acrossBorder, linkSepCrv.
Guesses and differences: InterFindClosest stores 0 where the original stores an uninitialised stack word when nothing was
nearer (30 of 600 cases "defined"); the list callbacks (InterCopy, InterCompT, InterDestroy) are handed to the generic
list by address and named on the way out (`Named` in inter.rs) so that a check or run-time can route them; the status
copies go through the x87 as 32-bit loads and stores, which the emulator leaves unchanged even for a signalling NaN (a real
x87 would quiet it; not compared). A first reading took the constant at 0x08662098 for 1e-5; it is 0.01 (found by the check).
Not ported: the Dump/Print routines (debug text), trimDial.c (dialogs), SI_nurbsSrfHighlightTrimCurve (drawing),
and, still open, curveInter.c (29), curveOrient.c (21), trim.c (33), ray.c's search/link/connect group,
SI_nurbsSrfGetTrimCrv/GetProjCrv/ExtractTrimCurve/TrimRemvPtsOnBorder (they need curveInter's BldLinFromComp).
Run: `re/tools/lead/slot.sh emu -- uv run --with unicorn python3 -I re/tools/si-nurbs/gen_trim2.py re/out/trim1/vec 600`, then
`nurbs-trim2check re/out/trim1/vec` (EMUCHECK_MUTATE=1|calls for the mutation switches).

## Trim2 (2026-10-29): curveInter.c, first half

`crates/si-nurbs/src/curveinter.rs`, checked by `nurbs-curvecheck` (vectors from `re/tools/si-nurbs/gen_curveinter.py`, 300 cases
per function, 4200 in all). Checked identical (0 different; UvMonSegCreate has 11 of 300 not compared, the original faults on a
null element datum; buildUvMonList 23 of 300, a fault on a wild list): [soft 0x08507550 si_nurbsCurveInterCreate], [soft
0x0850758c si_nurbsCurveInterDestroy], [soft 0x085076b4 si_nurbsSegmentInterCreate], [soft 0x08507778
si_nurbsSegmentInterDestroy], [soft 0x0850789c si_nurbsUvMonSegCreate], [soft 0x0850792c si_nurbsUvMonSegDestroy], [soft
0x08508250 si_nurbsCurveInterUpdateBbox], [soft 0x085089d4 si_nurbsCurveInterBboxInter], [soft 0x08509a44
si_nurbsCurveInterPointOnLine], [soft 0x08509ac0 si_nurbsCurveInterCheckSelfInt], [soft 0x08509c90
si_nurbsCurveInterSelfSegInter], [soft 0x08509848 validBbox], [soft 0x08508840 buildLinearBorder], [soft 0x085084b0
buildUvMonList]. Mutation: `=1` changes every compared case that has output, `=calls` every one with recorded calls.

What the records look like: the curve-inter object (0x4c bytes) holds the segment list (+0), the box (+4 / +0xc min, +0x1c /
+0x24 max, doubles), the points (+0x2c) and the run list (+0x34); a segment (0x88) the two end points, two parameters, its box
(+0x48) and a flag byte (+0x84, bits 0..1 mark the start of a run); a run record (0x3c) its box and the first and last segment
elements. The curve's closed word (+0x20) overlaps the high half of the box's max x, so the vectors use two layouts.
Self-intersection of two segments is `si_nurbsTrimComputeLineInter` (trim.c, trim3) answering the two parameters, then the
neighbour rule: a meeting at the shared end of adjacent segments (within 1e-6) is not an intersection.

Written, not yet checked (flatten needs a heap-built stand-in): si_nurbsCurveInterBldLinFromComp 0x085085e8. Not started:
buildSegList, BuildNew/OldInterList, initCurveInterObj, SegInter, setIntersectionDirection, splitSegment,
computeIntersectionOrientation, crossSegments, IntersectLists, CheckFitSelfInter; all of curveOrient.c; the trimUtils and
ray.c leftovers.

## Trim6 (2026-10-29): curveOrient.c, computeIntersectionOrientation

Read and checked (generator `gen_curveinter.py`, checker `nurbs-curvecheck`, 400 vectors each, 0 different): CurveOrientCreateP
0x0850a330, Destroy 0x0850a36c, inDomainSamp 0x0850c8b0, findPointInCurve 0x0850b63c, ProjPtPara 0x0850bc34, projectPtPers
0x0850c710, projectCrvSamples 0x0850b9b4, reverseCurve 0x0850b384, TraverseNestingTree 0x0850c930, and (600 vectors)
computeIntersectionOrientation 0x085090b8. Guesses: the original's nine arguments are the second curve's element first
(seg_inter passes (e2, e1, ci2, ci1, b, a, d, c, op)); the answer is 3/4 for op 0, else 1/2, by the sign of the last z, with
a 1e-6 threshold; two flags ("first", "pos") are left over between steps and are kept as they are. pointRelativeToPlane
0x0850c88c answers in st0 only, so it is written but not checkable and not cited. IntersectLists: random neighbour links made
most vectors fault after splits; with fresh pieces (links 0) and mixed line-intersection answers 37 of 158 compare (the
original stops or reads unwritten memory in the rest), 28 identical, 9 defined part, 0 different, `=1` changes all 9
compared that have output; kept uncited. The generator has a `GEN_SEED` variable; a huge garbage malloc size in a case
makes the emulator run die with MemoryError, so run IntersectLists in several seeds.

## Trim8 (2026-10-29): the rest of curveOrient.c, BldLinFromComp, BuildOld, two table accessors

Code: `curveorient2.rs` (the ten remaining curveOrient functions), `trimutils2.rs` (SI_nurbsSrfGetTrimCrv 0x084f8b78, SI_nurbsSrfGetProjCrv
0x084f8ae8), si_nurbsCurveInterBldLinFromComp 0x085085e8 and BuildOldInterList 0x085082c8 now cited, `route2` also reaches
computeIntersectionOrientation. Vectors: `re/tools/si-nurbs/gen_trim8.py` (checker `nurbs-curvecheck`) and `gen_trim8_st0.py` (st0 answers,
checker `nurbs-trimcheck`). Numbers (compared = identical + defined; the rest: the original stops on its budget or reads unwritten memory):
- PointInCurve, findNeighborInPlane, findTestPoints: 300 of 300 identical each. findLocForOrientTest 107 of 150, checkBoundaryCross 135 of
  150, setCorrection 146 of 150, findUvCriticalInters 63 of 150, MarkBCross 100 of 100 cases / 60 compared, findNonLinearInter 100 of 100,
  Orient 41 of 86 compared: 0 different in all of them. `EMUCHECK_MUTATE=1` makes every compared case differ (setCorrection / checkBoundaryCross:
  192 of 300 together, the rest do not reach a changed value); `=calls` is unchanged where no recorded callee is reached.
- BldLinFromComp 557 of 600 identical (43 not compared: the original stopped or wrote past a case buffer); GetTrimCrv, GetProjCrv 600 of 600
  each, mutation 600 different. BuildOldInterList 600 cases, every byte the original defines agrees (the objects' own heap blocks hold
  uninitialised bytes, so none is "identical"), mutation 600 different; the `BldLinFromComp` it calls is a stand-in by body.
- SegInter against the real computeIntersectionOrientation (no script): 317 of 400 compared, 0 different. IntersectLists 70 of 295 compared
  over three generations, 0 different (mutation: 29 of 32 compared change) - cited with that caveat.
- st0: pointRelativeToPlane and si_nurbsRayBinarySplitRays 1800 of 1800 identical each (`nurbs-trimcheck`, new entries); mutation 3600 different.
Found: findNeighborInPlane reads pts[idx + 1], one past the sample array when the ray sits on the last knot, so the original's answer depends on
the heap there (3 differing findLoc cases and 2 Orient cases before the generator kept such rays off the last knot, `no_last`). One Orient case
(line 39 of the first run) ends in a read of an already destroyed list element that the original survives (freed heap), not a port fault.
Not done: ray.c RayBinarySearch, peturbBinarySplit, Link/Connect group, trimUtils.c ExtractTrimCurve, RemvPtsOnBorder, divLnTo2Planes,
addImageData, the print routines; pointRelativeToPlane and RayBinarySplitRays were the only st0 ones reachable. Generators cannot be stopped
(slot waits): run them with `timeout`.

## How to run

    re/tools/si-nurbs/render_run.sh N [group ...]      # vectors (emu), build, check; MUT=1|calls; EXTRA=--first-diff
    re/tools/si-nurbs/render_run.sh 300 -               # check only
    generator: re/tools/si-nurbs/gen_render.py; checker: crates/si-nurbs/src/bin/nurbs-rendercheck
    trimmed surfaces of a scene (Trimtri1), real callees:
      scene-dump re/bin/si3d40-ia/installed re/out/trimtri1/scene --only Blend-Plane --rsrc <installed>/irix/soft3d/Creative-Rsrc/3D/rsrc/Relations.rsrc
      (ulimit -v 6000000; re/tools/lead/slot.sh emu -- uv run --with unicorn python3 -I re/tools/si-nurbs/gen_trimscene.py \
          re/out/trimtri1/scene/0000.idx re/out/trimtri1/vecscene rec 1 2 3)
      NURBS_ROUTED=1 nurbs-rendercheck re/out/trimtri1/vecscene      # TRI_REAL=1 gen_render.py ... for made-up surfaces

## Next

composite.c: Dump only. trim.c: InterCrvDump, and the MoveInterToBorder fill-pattern question (Trim7). The remaining work is curveOrient.c
and the curveInter.c/ray.c leftovers (trim6 / trim8).

## 2026-10-28 (nurbsrender5): the intersection recursion, the hierarchy, the conversion

Read in full from the listings and reproduced: addIntersection 0x0851261c, intersectBezT2D 0x08513414, intersectBezS2D
0x08511d40, performBsplit 0x0851224c, updateHierBbox 0x085145ac, buildBboxHier 0x08514058, intersectBboxHier 0x08514848,
convertToBez 0x085149ec, si_nurbsTrimConvertToBez 0x08514ec4 (a pass-through of four arguments), si_nurbsRayTraceSrf2
0x08511240. Also read and done inline (tiny): SI_genTreeElemGetData 0x08515b68, SI_genTreeElemCreate 0x08515b50,
SI_genTreeCreate 0x08515830 (calloc blocks of 0x18 / 0x14), si_bezPatchCreate 0x08503460 (routed in the check).

Check numbers (identical / defined-part only / different / not compared, of N; "defined-part" = the original's heap holds
bytes nobody wrote, here the two uninitialised doubles of a patch box copied from the original's frame; the rest agrees):
- addIntersection 200: 200 / 0 / 0 / 0. MUT=1 and MUT=calls: all 200 differ.
- intersectBezT2D 40: 16 / 0 / 0 / 24; intersectBezS2D 40: 16 / 0 / 0 / 24; performBsplit 40: 32 / 0 / 0 / 8. The not
  compared cases are ones where the original stops (stack of 2 MB used up: the recursion between T2D and S2D never
  narrows when the tolerance equals a box width, see below). MUT=1: all compared cases differ; MUT=calls: all but 1.
- updateHierBbox 80: 80 / 0 / 0 / 0 (MUT=1 all differ). buildBboxHier 80: 0 / 74 / 0 / 0 with 6 more taken from the
  recorded log10 (libm, "given-lib"); MUT=1 all 80 differ. intersectBboxHier 80: 79 / 0 / 0 / 1; MUT=1 all differ, MUT=calls
  17.
- convertToBez 80: 1 / 79 / 0 / 0 (MUT=1 and calls: 80 differ). si_nurbsTrimConvertToBez 80: 4 / 76 / 0 / 0 (same
  mutations). si_nurbsRayTraceSrf2 80: 34 / 46 / 0 / 0 (MUT=1 80 differ, calls 54).

What is a guess or unverified: the stand-ins of the callees outside the file answer from the vector (SrfCopy answers a
prepared surface, KnotInsert and ApplyMatrix change nothing), so convertToBez is held against the original with those
callees replaced, not with their real effect on the copy; the arithmetic of SI_nurbsSrfValueAndNormal reaches the port
only as recorded words. The names of the BezPatch cells are ours.

Behaviour worth knowing: (1) intersectBezS2D tests `(y0 - y1) * 10 < w` where the T2D side tests the width with the
sign the right way round (kept). (2) When the tolerance equals a box width the clip never narrows and S2D / T2D answer
each other until the stack is gone (the generator uses tolerances that are not dyadic to stay out of it). (3) When S2D or
T2D answer directly through the other one the working patch records are not given back (leak, kept). (4) updateHierBbox
resets only the x and y cells of the node's box; the z cells keep what they held. (5) buildBboxHier makes the hierarchy
levels with ceil(log10 n / log10 2) and groups 2 x 2; item index is `u * nv + v` with `nu` the first dimension, and
convertToBez fills `[v span * (u spans) + u span]`, which si_nurbsRayTraceSrf2 hands over with the V count first.

## 2026-10-28 (nurbsrender5): composite.c, first half

Read and ported (`crates/si-nurbs/src/composite.rs`; 80 cases each, all identical to the original unless noted; MUT=1 changes
every compared case): [soft 0x08503830 si_nurbsCompositeCurveCreate], [soft 0x08503878 si_nurbsCompositeCurveDestroy],
[soft 0x08504a2c SI_nurbsCompGetOpenNC] (67 identical + 13 defined-part), [soft 0x08504b28 SI_nurbsCompGetClosedNC],
[soft 0x08505db4 getBorder], [soft 0x08505ea8 nextBorder], [soft 0x08505e18 allCorners], [soft 0x08505ef0 addCorner],
[soft 0x08505f78 insertCorners], [soft 0x08505fec onBorderFlags], [soft 0x08506088 indexDestroy], [soft 0x085060b4 indexEqual],
[soft 0x08507508 insertCornerWrap], [soft 0x08506f94 si_nurbsCompositeCurvePointOnBorder], [soft 0x085039fc SI_nurbsSrfCrvOnBorder],
[soft 0x08503a98 addCompositeNoInter] (MUT=calls 21 differ), [soft 0x08504f10 findOrientation], [soft 0x08504bb0 addCompositeInter]
(MUT=calls 75 differ), [soft 0x08503978 si_nurbsCompositeCurveBuild] (MUT=calls 32 differ), [soft 0x08503c64 rebuildKnotVector],
[soft 0x08505d00 SI_nurbsProjectedCurveDelete] (MUT=1 80 differ, MUT=calls 71 differ). Total 23 functions, 0 different.

Guesses and stand-ins: the callees si_nurbsCurveInterUpdateBbox (0x08508250), si_nurbsCurveInterPointOnLine (0x08509a44),
SI_statusDisplayMessage and SI_nurbsCompDispose are recorded stand-ins of the emulator run (Outside, answers given per case).
The genList routines (create, destroy, iter init / next, element data, insert) and si_nurbsRayDist3D are written natively in the
checker's `route` from the same reading as si-gen's, because si-nurbs may not depend on si-gen; the emulator runs the real ones, so
the heap comparison holds them to the original. Record field names are ours. Not exercised: the "Loop detected in building
composite curve." path of addCompositeInter (100000 steps), and the unreachable "Bad KNOT count" message of rebuildKnotVector
(MUT=calls leaves rebuildKnotVector unchanged because its callees are native in the checker).

Behaviour worth knowing: (1) findOrientation answers 0 when the z of (P1-P0) x (P2-P0) is positive, else 1, in 80 bits.
(2) insertCorners loops forever when a border value is zero (kept; the generator avoids it). (3) addCompositeInter's order check
uses the largest order met so far, and best / best_x live across the outer iterations. (4) PointOnBorder and onBorderFlags compare
with a strict 1e-6. (5) rebuildKnotVector (order > 2) turns a chord of near-zero knot into the *squared* distance (RayDist3D
returns it unrooted), rotates the curve to its first flagged point and clears the point flags.
Open in composite.c: ptsMarkForDelete (needs SI_genListElemOf with indexEqual as callback), Dump, fitFailed, fitHighDegree,
elevateFromLinear, buildHighDegree, addCompFromTree, buildContainHier, pruneContainHier, CreateSrfComps(+Proj), curveInCurve,
CompCurveInCurveExt, linearTrimCrvsAdjust, nurbsSrfLinearizeTrimCrvs, rebuildComp, CompositeCurveRebuild, NoBorderTrimDelete.

## 2026-10-28 (nurbsrender5): trimmed.c finished

[soft 0x085000b0 EvaluateVertices] (80 cases: 5 identical, 75 defined-part, 0 different; MUT=1 and MUT=calls 75 differ, the 5
others have no vertices), [soft 0x08501a78 EvaluateTrimmedNurbsFromBaseGeometry] (80 identical, MUT=1 / MUT=calls 73 differ),
[soft 0x08501700 SI_noTrimmedTriangles] (60 cases: 38 identical, 0 different, 22 not compared because the original stopped:
2 budget, 17 invalid reads, 3 stand-in double frees; MUT=1 38 differ), [soft 0x084fd14c TriangulateTrimmedNurbsSrf] (60 cases:
25 identical, 11 defined-part, 0 different, 24 not compared, original stopped or reading unwritten memory; MUT=1 36 differ,
MUT=calls 11 differ).

Behaviour: the vertex record is 0x4c bytes (s, t, point, normal, point; 7 of its words are never written, hence the
defined-part cases). s = 1 - (u - u0)/(uN - u0) and t = (v - v0)/(vN - v0), 1.0 when the span is under 1e-4; the u / v are shifted by
0.001 for samples with flag 0x400 (u) and 0x20 with a positive break entry (v). CountVerticesTriangles' third word is the triangle
total and its fourth the sample (vertex) total (the earlier doc comment had them the other way round; the arrays are sized
calloc(vertices, 0x4c), calloc(triangles, 0x60), calloc(triangles, 12) for the index triples). EvaluateVertices always answers
1, so TriangulateTrimmedNurbsSrf's "evaluation failed" cleanup is unreachable. The base geometry pass runs only for a base that
differs from the surface; its "absolute difference" of a negative value is -base - srf, kept as read, tested against 1e-6.
SI_noTrimmedTriangles answers the triangle total.

Stand-ins: SI_nurbsSrfValue, SI_nurbsSrfValueAndNormal, SI_nurbsSrfExtractCurve and SI_nurbsCrvDispose are recorded stand-ins
(Outside) in the EvaluateVertices / FromBaseGeometry / Triangulate checks; the u/v mapping branch coverage of FromBaseGeometry was
not measured separately.

## 2026-10-28 (nurbsrender5): composite.c, second half so far

[soft 0x085049d8 fitFailed] (80 identical; MUT=1 80 differ, MUT=calls 80 differ), [soft 0x08506730 nurbsSrfLinearizeTrimCrvs]
(80 identical; MUT=1 80, MUT=calls 66 differ), [soft 0x08505150 curveInCurve] (80 identical; MUT=1 80, MUT=calls 55 differ; the
test point sits within 1e-4 of the 80-bit result), [soft 0x08506424 linearTrimCrvsAdjust] (80 cases: 45 identical, 12
defined-part, 0 different, 23 not compared because the original ran to the 3 M step budget; MUT=1 36 differ, MUT=calls 0
differ because both disposals take stack addresses that the stand-in matches loosely). linearTrimCrvsAdjust: the old curve is
copied into a local whose address is handed to the dispose routine (not the array slot), then the slot receives the new curve.
Corrected by the trim8 follow-up: that reading was wrong (the scratch curve goes through a local, the old curve through its table slot);
these 80 vectors logged every argument as a local, see that section.
Guess: the budget stops come from border-flag combinations the real data never has (insertCorners looping on a zero-length
step); not proved.

Open: ptsMarkForDelete (needs a native genListElemOf with indexEqual as callback), Dump (variadic fprintf cannot be held
against a fixed-arity stand-in), fitHighDegree, elevateFromLinear, buildHighDegree, and the tree-based group (addCompFromTree,
buildContainHier, pruneContainHier, CreateSrfComps(+Proj), CompCurveInCurveExt, rebuildComp, CompositeCurveRebuild,
NoBorderTrimDelete), which needs native routes for the tree routines in the checker.

## Trim3 (2026-10-29): trim.c

Read from the code: a trim record is 0x104 bytes (layout in the module doc of `trim.rs`). The setters, getters, CreateP and
Destroy are plain field moves (SetStep rebuilds nothing, it asks the curve routines through `Outside`). Preview walks the
flattened curve and casts a ray per sample (RayCast, parallelRayPerturb); BuildOrientList groups the linked intersections into
chains (open/closed/loop); the rest of the file are orchestrators over routines of other units (curveInter, curveOrient, ray,
composite), reached through `Outside` with `Routine` constants.

Checked on the emulated original (all in `re/out/trim3/vec`, 0 different):
[soft 0x0850f7d8 si_nurbsTrimSetDirection] [soft 0x0850f8fc si_nurbsTrimSetCenter] [soft 0x0850fa6c si_nurbsTrimSetCoordDirection]
[soft 0x0850faa4 si_nurbsTrimSetCoordCenter] [soft 0x0850f7ac si_nurbsTrimSetSurfaceOp] [soft 0x0850f7c4 si_nurbsTrimSetProjection]
[soft 0x0850fa58 si_nurbsTrimSetTrimming] [soft 0x0850ff50 si_nurbsTrimSetRayList] [soft 0x0850ff68 si_nurbsTrimSetOrientList]
[soft 0x0850ff1c si_nurbsTrimGetRayList] [soft 0x0850ff38 si_nurbsTrimGetOrientList] [soft 0x0850fb2c si_nurbsTrimShowPreview]
[soft 0x0850fadc si_nurbsTrimSetStep] [soft 0x0850f580 SI_nurbsTrimCreateP] [soft 0x0850f678 SI_nurbsTrimDestroy]
[soft 0x08510328 si_nurbsTrimStartAtZero] [soft 0x085103e8 si_nurbsTrimClearInterLinks] [soft 0x08510aac si_nurbsTrimComputeLineInter]
[soft 0x08510c08 initInterSeg] (200 cases each); [soft 0x085104b4 si_nurbsTrimOrientOrientList] [soft 0x0850ff88 si_nurbsTrimConnectRayInter]
[soft 0x08510538 si_nurbsTrimRayCast] [soft 0x08511100 parallelRayPerturb] (100 each); [soft 0x085100f0 si_nurbsTrimBuildOrientList]
[soft 0x08510f6c si_nurbsTrimBuildCompositeCurves] [soft 0x08511094 si_nurbsTrimBuildCompositeCurvesProj]
[soft 0x08510e4c si_nurbsTrimBuildIntersectionLists] [soft 0x0850fb60 si_nurbsTrimPreview] [soft 0x0850fecc si_nurbsTrimProject]
[soft 0x0850fe04 SI_nurbsTrimTrim] (150 each); [soft 0x08510808 si_nurbsTrimMoveInterToBorder] (300: 298 defined part, 2 not compared
because the port's result differs between the two fill patterns; reason not found).

Mutation (`EMUCHECK_MUTATE=1`): 266 of 5550 cases stay identical, all of them functions or cases that produce no compared value
(null trim, setters storing a value the case leaves as it was, nothing written). `=calls`: 3435 identical (functions that record no
call, and cases whose path makes none), 1816 differ, 298 defined part (MoveInterToBorder, no calls). The functions that record
calls differ in every case that makes one, except CreateP and Destroy cases that go through no stand-in.

Guesses and limits. (1) MoveInterToBorder: the third double of the result is stack garbage in the original; the port stores 0 and the
check compares the defined words only. (2) Coverage of Preview: fresh rays carry no intersections, so OrientCreateP, ConnectRayInter's
Trans branch and the peak-replace path are not reached; RayTraceSrf2, SI_nurbsCrvValue, ConnectTrans/NoTrans, CompositeCurveBuild,
CreateSrfComps(+Proj), Srf{Trm,Proj}Dispose, Build{New,Old}InterList, IntersectLists, OrientCreateP/Orient/Destroy,
StatusDisplayMessage, ShowActivity, BuildListFromPreview and TraverseNestingTree are recorded stand-ins (the Rust reaches them with
`o.call`). The surface box handed by stack pointer to Build*InterList is not compared and its words 4, 5, 10, 11 are garbage in the
original. (3) Project and Trim were checked with BuildListFromPreview as a stand-in; Trim's cases keep the orientation list non-empty.
(4) `trim` is compiled out of a test build (it includes si-gen's list.rs by path, like `inter`), so `cargo test -p si-nurbs` builds.

Open: BuildListFromPreview 0x08510608 (needs the generic tree routines SI_genTreeCreate 0x08515830 / InsertS 0x08515cf0 and
InCurveExt 0x08505344 as stand-ins; its first bit-0x10 store goes through a register the function never sets, i.e. the caller's EDI,
which this checker cannot supply), InterCrvDump (debug print), InterBuildLinearCrvCB 0x0850d5b4, and composite.c: addCompFromTree,
buildContainHier, pruneContainHier, CreateSrfComps(+Proj), CompCurveInCurveExt, rebuildComp, CompositeCurveRebuild,
NoBorderTrimDelete, ptsMarkForDelete, fitHighDegree, elevateFromLinear, buildHighDegree. Kit candidates: a Kit option to compare
the defined part of UNINIT cases per file is wanted for MoveInterToBorder-like outputs (it already does, as "defined"); a shared
native genTree port; a helper for the callback-address-as-named-argument boilerplate.

## Trim4 (2026-10-29): curveInter.c, second half

Ported and checked against the emulated original (vectors `re/out/trim4/vec` from `gen_curveinter.py`, checker `nurbs-curvecheck`,
2221 cases): [soft 0x08507c18 buildSegList] (300: 296 identical on the defined words, 4 the original faults on, 0 different; the two
box words it never sets are left as the stack held them, the port writes 0), [soft 0x08508918 initCurveInterObj] (200: 50 identical, 150
identical on the defined words, 0 different), [soft 0x085079f0 si_nurbsCurveInterBuildNewInterList] (150: 48 + 100, 0 different, 2 not
compared), [soft 0x0850a0f8 si_nurbsCurveInterCheckFitSelfInter] (200: 35 + 165, 0 different), [soft 0x08508a30 si_nurbsCurveInterSegInter]
(300: 225 identical, 0 different, 75 not compared; `computeIntersectionOrientation` is a recorded outside answer, not ported),
[soft 0x08508ee4 splitSegment] (300: 245 identical, 55 the original faults on), [soft 0x08508e04 setIntersectionDirection] (300),
[soft 0x08509764 crossSegments] (300; note it takes the cross product of the *second* segment's direction with the first's).
Not cited because not checked well enough: IntersectLists (0x08509de0: 71 vectors, 8 compared, 0 different, 63 where the original faults on
random rings), BuildOldInterList (0x085082c8) and BldLinFromComp (0x085085e8), both written but without a stand-in for them.
Mutation: `EMUCHECK_MUTATE=1` leaves 59 identical (BuildNew's 48 with no output to change, buildUvMonList 5, IntersectLists 6), `=calls`
changes every case that makes a call except BuildNew's 8 with calls the port answers itself. Stand-in `SI_nurbsCrvFlatten` for the
CheckFit vectors builds a heap record (min(count, 8) points) in `flatten_body` (generator) and `flatten` (checker).
Open: computeIntersectionOrientation (0x085090b8), curveOrient.c (21), trimUtils/ray leftovers.
## Trim5 (2026-10-29): composite.c, the non-tree remainder

Read from the code: a comp record is 0x44 bytes (curve +0x30, order +0x38 in the high-degree helpers). Polyline points are 0x24
bytes (x +0, y +8, border flags byte +0x20). ptsMarkForDelete walks a point list and marks points through a list built with the
generic list routines, using indexEqual 0x085060b4 as the compare. elevateFromLinear interpolates with PointOnLine and an
80-bit accumulator; fitHighDegree fits with SI_fitNurbsCurve at a tolerance of (f32 global 0x086874d8)/10 and an angle
discontinuity read from int global 0x086874dc; buildHighDegree builds the curve list, fits, and disposes.
Ported in `composite2.rs`: [soft 0x085060cc ptsMarkForDelete], [soft 0x08507008 elevateFromLinear], [soft 0x085072b8
fitHighDegree], [soft 0x08506b44 buildHighDegree]. Checker `nurbs-rendercheck`, vectors `re/out/trim5/vec` from
`gen_render.py`, runner `re/tools/si-nurbs/trim5_run.sh`. The checker has a native route for SI_genListElemOf 0x0851579c (assumes
the compare is indexEqual).
Check: ptsMarkForDelete 200/200 identical, elevateFromLinear 200/200, fitHighDegree 100/100, buildHighDegree 200/200; 0 different.
Mutation `=1` changes every case; `=calls` changes 144/200, 100/100, 181/200 (ptsMarkForDelete has no recorded calls, every callee
is a native route). Stand-ins: PointOnLine, SI_fitNurbsCurve, SI_nurbsCrvDispose (kind "u", arguments not compared, since the
mixed stack and heap pointers fail the other kinds).
Layout read for the open tree group: GenTree 0x14 (root +0, count +0xc); GenTreeElem 0x18 (data, prev, next, parent, child, tree).
Open and not started: the tree group needs native genTree routes (Create, InsertS/ElemInsert with a callback into the emulated
curveInCurve, ElemDestroy, RemoveSubTree); BuildListFromPreview, InterBuildLinearCrvCB, the MoveInterToBorder fill-pattern
question and the trimLineOp leftovers were not reached this round.

## Trim7 (2026-10-29): composite.c tree group, trim.c preview pair

Code: `crates/si-gen/src/tree.rs` (generic tree), `crates/si-nurbs/src/composite3.rs`, `crates/si-nurbs/src/trimprev.rs`.
Checkers: `gen-checkkit` (`SI_genTree*`) and `nurbs-comp3check`. Run: `re/tools/trim7/tree_run.sh 300 -` and
`re/tools/trim7/comp3_run.sh 200 -` (`MUT=1|calls` for mutation, `EXTRA=--first-diff`). Vectors in `re/out/trim7/vec` and `vecp`.

Generic tree (15 functions, [soft 0x08515830 create], [soft 0x08515b50 elem_create], [soft 0x08515b68 elem_get_data],
[soft 0x08515b7c get_root], [soft 0x08515b98 elem_set_data], [soft 0x08515bb8 elem_replace_data], [soft 0x08515cbc set_flags],
[soft 0x08515cd8 get_flags], [soft 0x0851591c elem_destroy_sub_tree], [soft 0x08515848 destroy], [soft 0x085159a0 elem_destroy],
[soft 0x08515bf0 remove], [soft 0x08515c1c remove_sub_tree], [soft 0x08515d38 elem_insert], [soft 0x08515cf0 insert_s]).
GenTree is 0x14 bytes (root +0, block +4, count +0xc, flags +0x10); GenTreeElem is 0x18 (data, prev +4, next +8, parent +0xc,
child +0x10, tree +0x14). Check: 3940 cases, 3885 identical, 55 defined (all Remove: its return is garbage when nothing is
removed), 0 different. Mutation =1: 3832 different; =calls: 1763 different.

Composite functions, all checked against the emulated original (identical / defined / different / not compared):
- [soft 0x0850588c addCompFromTree] 200/0/0/0. It writes the clamped order 4 back to data+0x38; it reads the environment variable
  named by the string at 0x086db081 (high degree trim) through the libc stub at 0x080c3bb8.
- [soft 0x08505090 buildContainHier] 300 identical. [soft 0x08505ba0 pruneContainHier] 300 identical (its walk starts at the
  given element, with a null guard).
- [soft 0x08506a84 rebuildComp] 100: 95 identical, 5 not compared.
- [soft 0x08504130 si_nurbsCompCurveCreateSrfComps] 200 identical (op from trim+0xfc, surface from *trim; si_validTrimCurve is an
  outside stand-in).
- [soft 0x08505344 si_nurbsCompCurveInCurveExt] 60 identical (polyline at +0x20, four refs at +0x10..0x1c, doubles at +0x30/+0x38).
- [soft 0x085043c8 si_nurbsCompCurveCreateSrfCompsProj] 60: 49 identical, 11 not compared (4 original stopped, 7 original
  wrote past a buffer). Area threshold 1e-6, tolerance divisor 10.0, bbox starts at FLT_MAX; the out.knots leak is mimicked.
- [soft 0x0850687c SI_nurbsCompositeCurveRebuild] 100: 26 identical, 54 defined (garbage box words), 20 not compared. The border
  element test uses the root's next sibling.
- [soft 0x0850679c SI_nurbsNoBorderTrimDelete] 100: 2 identical, 64 defined, 34 not compared.
Composite destroy goes through the CrvDispose stand-in so the call is recorded; `linear_trim_crvs_adjust` in composite.rs now
disposes through the table slot (the old check hid this because its table was on the stack).

trim.c preview pair (trimprev.rs):
- [soft 0x0850d5b4 si_nurbsInterBuildLinearCrvCB] 200: 155 identical, 5 defined, 40 not compared (mostly original read faults).
  Tolerance 1e-4 as in inter.rs; start/end flags come from the bits at +0x50.
- [soft 0x08510608 si_nurbsTrimBuildListFromPreview] 200: 134 identical, 7 defined, 59 not compared (49 faults). MarkBCross
  (0x0850a720, curveOrient.c) is an outside stand-in. When no curve exists yet and the flag-0x10 write is needed the original uses
  the caller's EDI register; the port returns Halt::Refused there.
Records: trim +0 srf, +0x14 src list, +0x20 out list, +0x34 tree, +0x38 int, +0xfc op; Inter +0x30 u, +0x38 v, +0x50 flags,
+0x64 dist, +0x6c flag word, +0x70 prev flag, +0x74 next.

Mutation on the composite checks: =1 differs for 80/100 CompositeCurveRebuild, 66 NoBorderTrimDelete, 191 addCompFromTree,
300 buildContainHier, 28 pruneContainHier (no output to change otherwise), 94 rebuildComp, 60/60 CreateSrfComps and InCurveExt.
=calls: 54, 66, 155, 247, 0 (no recorded calls), 82, 55/60, 38/60; InterBuildLinearCrvCB and BuildListFromPreview mostly make no
outside calls (0 and 19 different). Not re-run on the final N=200 vectors for the last three groups.

Limits and guesses: addCompFromTree's "self-intersection bad" branch is not covered (the Flatten stand-in answers 0).
[soft 0x08510808 MoveInterToBorder] on the trim3 vectors: 300 cases, 298 defined, 0 different, 2 not compared; the probable cause
is that ComputeLineInter leaves u/v untouched when the lines do not cross, so stack garbage reaches the third double (unconfirmed).
InterBuildLinearCrvCB copies a 3-double struct into 2-double locals; whether that overwrites a saved register was not determined.

## Trim9 (2026-10-29): ray.c remainder, trimUtils.c remainder

Checker: `nurbs-trim9check re/out/trim9/vec` (generators `re/tools/si-nurbs/gen_trim9.py`, `gen_trim9b.py`). Code: `raylink.rs`, `trimutils3.rs`.

Read from code:
- ray.c linking: RayBinarySearch, peturbBinarySplit, LinkCurLess/More, ConnectNoTrans, ConnectTrans (si_nurbsRayConnectTrans), addRayToSampling and the six small predicates/inserters. In ConnectTrans the "g equals *pp" branch passes the address of the local element to the cur* helpers. addRayToSampling prints through the global stdout, so worlds reaching it must map that global.
- trimUtils.c: si_divLnTo2Planes does its arithmetic in x87 extended precision and rounds r to f32 in every branch. si_addImageData groups points with a 1.1 gap and a 3.0 spread; it reads one slot past the distance array without effect. SrfExtractTrimCurve and TrimRemvPtsOnBorder call SrfValue, FitNurbsCurve, BldLinFromComp, CrvDispose and StatusDisplayMessage, kept outside and replayed from the log.

Guesses / not done: SI_nurbsSrfHighlightTrimCurve is a flag setter on a static; left unported. Debug Dump/print routines and trimDial.c UI are not renderer work. InterDist2D and RayDist3D (st0 returns) are ported but not compared: si-checkkit has no st0 support.

Check numbers (2460 cases, 18 routines): 2367 identical, 30 defined (LinkCurMore 22, ConnectTrans 5, addRayToSampling 3: original returns an uninitialised stack word where we store 0), 0 different, 63 not compared (original hit the 10M step budget: ConnectTrans 38, addRayToSampling 15, peturb 4, bSearchIsNull 4; RemvPtsOnBorder 2 original faults on unmapped read). Mutation: `EMUCHECK_MUTATE=1` leaves 21 identical of 2460 (curNotLowerUpperNotNext only, a mutation-neutral path); `=calls` changes every case with recorded callee calls (1237 identical are call-free).
## Callee1 (2026-10-29): render ports against the real callees

Aim: replace "answered 0" stand-ins by the original's own callees in the vectors, with the ports routed (`NURBS_ROUTED=1`,
`routed.rs`). Generators: `re/tools/si-nurbs/gen_render.py` (env `COMP_REAL=1` for the composite pair; new group
`addIntersection_real`), `re/tools/callee1/gen_callee1.py` (groups `ai`, `ctbt`). Checker `nurbs-rendercheck` (it now allows the
original's scratch statics of the real SI_nurbsSrfValueAndNormal, `aj`, `dl`, `dr`, `nb_aj`, as global writes).
- addCompositeNoInter / addCompositeInter with the real si_nurbsCurveInterUpdateBbox (1742 routed calls) and
  si_nurbsCurveInterPointOnLine (542): 600 cases, 421 identical, 179 defined part (addCompositeInter: the box the original copies
  has z cells in an uninitialised heap block; the old stand-in wrote 0 there and hid it), 0 different. `=1`: 600 differ; `calls`:
  NoInter 71 differ, the rest unchanged (no recorded call left to change in Inter).
- addIntersection with the real si_nurbsInterCreateP, ComputeRayT, RayAddInter (genList insert and InterAlreadyThere included), a real
  ray record, `SI_nurbsSrfValueAndNormal` still a recorded stand-in (`NURBS_ROUTING_SKIP=0x084d6824`, else the routed port answers it):
  300 of 300 identical, 0 different; `=1` and `calls`: 300 of 300 differ.
- addIntersection with a real surface too (group `ai`): not comparable. The port `image::srf_value_and_normal` is "values only": the
  original extracts curves on the heap and frees them (nine blocks per call), the port makes none, so every later heap block is
  numbered differently (`no buffer heap10`) and with `NURBS_ROUTED=1` the routed port leaves the point and normal locals unwritten
  when it refuses a surface. Open: port the heap effect of SI_nurbsSrfValueAndNormal (calls ExtractCurveSeg, CrvValue, CrvValue4D,
  CrvDispose).
- si_nurbsTrimConvertToBez with real SrfCopy / ApplyMatrix / KnotInsert / Dispose (group `ctbt`): 300 cases, 0 identical, 68 defined
  part, 0 different, 232 not compared (33 the original wrote past a buffer cap, 199 depend on unwritten words, as for convertToBez
  in section 23). `=1`: 267 of 267 compared differ; `calls`: 92 of 160 compared differ.
- interSetup / interCleanup (original runs the real si_bezPatchInit): 300 + 300 identical, 0 different; `=1`: 0 and 4 identical. The
  Rust side's bezPatchInit is still the check's transcription (si-spline depends on si-nurbs).
- IntersectLists / BuildOldInterList coverage not extended: new `gen_trim8.py IntersectListsReal` runs (seeds 3, 5, 7) all died in the
  generator with MemoryError (the garbage-size flake named in Trim2), so the 70 of 295 compared stand.

## Trimtri1 (2026-10-29): the trimmed triangulation against the original's own callees

Aim: show that [soft 0x084fd14c TriangulateTrimmedNurbsSrf] and the trimmed.c pipeline give the original's result when every callee is
the original's own code (no stand-ins), on the corpus's real trimmed surfaces as well as on made-up ones.
- Routing (`routed.rs`, `NURBS_ROUTED=1`): SI_nurbsSrfValue (0x084d6728, `geom8::srf_value_heap`) and SI_nurbsSrfExtractCurve
  (0x084d5268, `image::srf_extract_curve`) are routed, so the render's two trimmed nodes triangulate (whole-corpus first frames: 296
  pictures, 277 identical + 19 identical but the icon, before and after; the 2 refusals are gone). SI_nurbsSrfValueAndNormal
  (0x084d6824) now hands on the caller's two curves (words 7 and 8, `image::srf_value_and_normal_curves`). Read from EvaluateVertices:
  it passes the iso-curves it extracted at the strip's v edges (`dir` 1, for samples with flag 0x20 or 0x40), and the original then
  evaluates them instead of extracting its own. The old arm dropped them, so the port extracted three blocks more per call. Every heap
  block after the first such call then had another number (`wtris` pointer different in 16 of the 18 failing TRI_REAL cases). The other
  callers (ray_trace.rs, si-scene's wave geometry) pass null curves and are not changed.
- Pictures say nothing here. Blend-Plane.1-0 is the only scene with trimmed surfaces (finding 0025's 10 trims are its 7 + 3; the 14
  dumped scenes in re/out/si-nurbs/scenes have none). Its frames are a `SUBREGION 91 0 IN 500 424` crop (149 x 131) of the propeller,
  without either trimmed surface (675 triangles filed per frame). All 100 frames are identical to the oracle's before and after
  this unit's routing (three renders: before the routing, with it, and with the ValueAndNormal fix; `frames_compare.py`, 100 identical each time).
- Scene check, new generator `gen_trimscene.py`: `scene-dump` (si-emuworld) writes Blend-Plane as the port's loader built it. Each trimmed
  surface (record, knots, points, trim and projected composites with their curves) is copied out of that image into a case, and the
  original runs with no stand-ins. Node 0x10042978: 7 trims, 2 projected curves, steps 7/7. Node 0x100a1f48: 3 trims, 1 projected, steps
  5/5. Each with the scene's steps and steps 1, 2, 3, times base null / the surface, materials or none, want 0 / 1: 32 cases. Result: 0
  identical, 32 defined, 0 different, 0 not compared. With its own steps node 0x10042978 gives 4301 vertices and 7061 triangles, as in
  the render. "Defined": the vertex array is the block the original's `UNINIT` note names. Its words 12-15, 17 and 18 of each 0x4c
  record are the original's unwritten frame words (EvaluateVertices copies its whole local record). `EMUCHECK_MUTATE=1`: 32 of 32
  differ. `=calls`: 0 differ, because there is no recorded call left to change (all callees are the original's or routed).
- Made-up surfaces with the real callees (`TRI_REAL=1 gen_render.py <dir> 100 TriangulateTrimmedNurbsSrf`, label
  TriangulateTrimmedNurbsSrfReal), 100 cases. Before the fix: 36 identical, 0 defined, 18 different (16 heap numbering, 2 a stray write
  of `aj`), 46 not compared. The original stopped in all 46: 34 read faults, 6 double frees caught by the harness, 5 budget, 1 left the
  sandbox (XInitThreads). After, regenerated with the statics declared (re/out/trimtri1/vec2): 36 identical, 18 defined (the same frame words), 0 different, 46
  not compared. `EMUCHECK_MUTATE=1`: 54 of 54 compared differ. `=calls`: no change (no recorded calls).
- Scratch statics: SI_nurbsSrfExtractCurveSeg's `aj` 0x086d9c0c, `dl` 0x086d9c10, `dr` 0x086d9c14 (blocks it grows), `nb_dlr`
  0x086d9c18, `nb_aj` 0x086d9c1c, and the span search's `left` 0x086d9c20. gen_trimscene.py and TRI_REAL now declare them as case
  globals (`g086d9c0c_aj` ...), so their final values are compared, and they match. The checker resets them for vectors that do not
  declare them (the original's `GW:` notes).
- Tools: `NURBS_HEAPTRACE=1 nurbs-rendercheck` prints the port's heap blocks and frees, and `re/tools/si-nurbs/heapdiff.py <vec> <line>
  <trace>` finds the first block or free that differs from the original's `heapN=` / `freed=`; that is how the dropped curves were found.
  `NURBS_ROUTED_TRACE=1` now prints each routed call's address and arguments.
- Not this unit's, seen on the way: `re/out/si-nurbs/vec44/linearTrimCrvsAdjust.vec` (2026-10-28) gives 36 different. In those
  vectors the original hands SI_nurbsCrvDispose a stack address, while the port disposes through the table slot (Trim7's change).
  One of the two readings is wrong for one of the two vector sets. Settled by the trim8 follow-up: the vectors were stale, the port is right.
- `cargo test -p si-nurbs`: 28 passed, 0 failed.

## Trim8 follow-up (2026-10-29): linearTrimCrvsAdjust's two disposals, stale vectors

Question: the 2026-10-28 vectors of [soft 0x08506424 linearTrimCrvsAdjust] gave 36 different after Trim7 made the port dispose the old
curve through its table slot. Read from the listing: per list element the original calls SI_nurbsCrvDispose (0x084db870) twice, first
with the address of its local holding the scratch curve (the `SI_nurbsCompGetClosedNC(n + 4, 2, 1)` one), then with `curves + 4 * index`
(the caller's table slot), and only then stores the new curve in that slot. The port (composite.rs) does exactly this; no code change.
Cause: gen_render.py gave the stand-in kind `s`, which logs every argument as `57ac57ac` (a local) whatever it is, so the vectors said
"stack address" for the slot too, and the old port (which copied the old pointer into a local) matched them by accident.
- Generator fix (`gen_render.py`, `crv_dispose_logged_where_it_points`): the stand-in logs a stack address as a local and anything else as
  a pointer. Regenerated with the same seed and count (80): the file is byte-identical to the old one but for the dispose arguments, which
  now read `57ac57ac` 79 times and `&curves`, `&curves+0x4`, `&curves+0x8` 44 / 25 / 10 times. The file replaces
  `re/out/si-nurbs/vec44/linearTrimCrvsAdjust.vec` (old copy in `re/out/si-nurbs/ltca/old`).
- Check (`nurbs-rendercheck`): 80 cases, 45 identical, 12 defined (every byte the original wrote agrees; the rest are words it left
  unwritten in the heap curve records), 0 different, 23 not compared (the original ran to its 3 M step budget).
  `EMUCHECK_MUTATE=1`: 36 different (all compared cases that dispose; the other 21 have no curve to adjust, so no value to change).
  `=calls`: 36 different (before, 0: the local-only logging could not see a changed slot).
- Real callee (`LTCA_REAL=1`, label `linearTrimCrvsAdjust#real`, `re/out/si-nurbs/ltca/real`): the original runs its own SI_nurbsCrvDispose,
  which frees the scratch curve's three blocks and the old curve's points and record (case buffers); `NURBS_ROUTED=1 nurbs-rendercheck`
  with the routed port: 200 cases, 137 identical, 22 defined, 0 different, 41 not compared (budget); `freed=` lists agree. `=1`: 103
  different (every compared case with a curve; 56 have none); `=calls`: no change (no recorded calls left). The real callee cannot tell a
  slot from a local (the slot is overwritten after), which is why the stand-in set is the one that decides.
- Live: not reached; the only caller is SI_nurbsNoBorderTrimDelete (0x0850679c), a trim-deletion edit, not on the render path
  (`kit cover`: not traced). No picture can change (no port code changed).
- Other `s`-kind uses of SI_nurbsCrvDispose (EvaluateVertices, SI_nurbsSrfExtractTrimCurve, the old TriangulateTrimmedNurbsSrf set): their
  listings pass `lea` of a local each time, so `s` is right there; RemvPtsOnBorder's vectors already log heap slots (`&heap1+4k`).
- No regression (no crate change): Trim7 `nurbs-comp3check re/out/trim7/vec` 26 files 5707 cases, 1476 identical, 130 defined, 0 different
  (NoBorderTrimDelete 2/64/0/34, CompositeCurveRebuild 26/54/0/20 as recorded), `vecp` 60: 49/0/0/11, tree 3940: 3885/55/0/0; Trimtri1 scene 32: 0/32/0/0, TRI_REAL
  100: 36/18/0/46; `cargo test -p si-nurbs` 28 passed.
- Run: `(ulimit -v 6000000; re/tools/lead/slot.sh emu -- uv run --with unicorn python3 -I re/tools/si-nurbs/gen_render.py DIR 80
  linearTrimCrvsAdjust)` (`LTCA_REAL=1` for the real callee), then `nurbs-rendercheck DIR` (`NURBS_ROUTED=1` for the real set).
