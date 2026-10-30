# 0025 — NURBS curves and surfaces: records, copies, span search, evaluation, the normal, knot insertion, sampling

Date: 2026-10-13 (first pass), 2026-10-14 (second pass: section 13). Binary: Linux `soft`,
sha256 `ef21a382c3325d42…`. Source files of the original: `nrb_b.c` (0x084db5a0, 33
functions), `eval.c` (0x084d3550, 33), `nrb_geometry.c` (0x08501d90, 8), `nrb_misc.c`
(0x084e8f60, 22), `nrb_convert.c` (0x084df320, 19). Rust: `crates/si-nurbs`.
Scripts: `re/tools/si-nurbs/` (`emu_gen.py`, `emu_run.sh`, `lst.sh`, `sym.sh`).

How to read this note: **[read]** is read from the instructions (disassembly; the
decompiler's output served as a map only, and was wrong about argument positions in every
routine that returns a structure); **[measured]** is a count from a run anyone can repeat
(section "Checks"); **[name]** is taken from a function name, a file keyword, an SDK header
or a manual; **[guess]** is a guess. Unmarked statements about ported code are [read] and
[measured] both: the port was written from the listing and then compared bit for bit with
the emulated original (finding 0011).

**State after the second pass: 59 functions ported and cited.** New: the conversion of an
untrimmed surface to the renderer's triangles, `SI_nurbs2Triangle`,
`TriangulateNonTrimmedNurbsSrf`, `repeatKnotsInSampling`,
`EvaluateNonTrimmedNurbsFromBaseGeometry` (`nrb_convert.c`, 4 of 19), `getPointInterval`
(`nrb_misc.c`, the first step of the rest of package 14), and a second form on
the memory image of the six evaluators they call; checked against the original stepped on
the surfaces of the dumped sample scenes and on made-up ones (section 13.6: 10,896 cases, 10,889 identical, 0 different, 7 the original did not finish or did not define), and
bound into the renderer, where the 5 sample pictures that lacked their NURBS surfaces are now
byte for byte the original's (finding 0012, section 22). The first pass's checks give what
they gave.

**State after the first pass: 54 functions ported and cited** (`eval.c` 23 of 33, `nrb_b.c` 21 of 33, `nrb_misc.c`
10 of 22, `nrb_geometry.c` 0 of 8), **all 54 compared with the emulated original**: 72,531
cases, 70,923 bit-identical, **none different**, 1,608 not plainly identical (489 the
original did not finish, 233 hostile inputs the port refuses, 886 undefined in part in the
original and identical in every defined word). Section 11 says what is not ported.

## Where it stands (2026-10-29)

Each bullet is one package step; the section named has the routines, addresses and check numbers. Common to all:
`NURBS_PARTLY=1 nurbs-trimcheck <dir>` also compares heap blocks that hold unwritten malloc data in their defined part;
mutation `EMUCHECK_MUTATE=1` flips a stored value, `EMUCHECK_MUTATE=calls` the call log, and every compared case differs
under both unless a bullet says otherwise. Every checked function has 0 different cases.

- nurbsgap1 (last section): `SI_nurbsSrfValueAndNormal` now makes the original's temporary curve extractions; knot margin words; trim set 0 different (was 10), cmfe_nface 0 different (was 4).
- First pass and package 13/14/15 (sections 11 to 15): `eval.c`, `nrb_b.c`, `nrb_misc.c`, nine routines of `nrb_point.c`, the
  trim-line group (`trimLineOp.c` 43 of 43, vec40: 59,800 cases) and 13 + 9 routines of `curveFit.c` (`fit.rs`, `fit2.rs`).
  `si_nurbsCurveInterBldLinFromComp (0x085085e8)` is not ported (needs `SI_nurbsCrvFlatten`, `SI_nurbsCompGetClosedNC/OpenNC`).
- nurbsfix1, nurbsgeom1/2 (sections 16, 17, 23: `blend.rs`, `blend2.rs`, all of `blend.c`), routing8 (section 22, `routed.rs`).
- nurbsgeom3 (section 18, `geom3.rs`): four `nrb_b.c` surface routines, `nurbsFrenetCoord`, `SI_nurbsCrvIndexHit` (5,200 cases).
- nurbsgeom4 (section 19): `SI_nurbsSrfInverse`, the bridges `SI_nurbsCrvKnotInsert`, `SI_nurbsSrfKnotInsert(U|V)`,
  `SI_nurbsSrfSwapUV` (`bridge.rs`), `si_nurbsClampFactor`: 5,600 cases, 0 different.
- nurbsgeom5 (section 20, `geom5.rs`): `updateStartEnd`, `dirtyCurves`, `markDirtyCcrvs`, the Transfer/ApplyFunc2Points routines:
  6,000 cases (vec90), 5,912 identical, 88 not compared (the original traps).
- nurbsgeom6 (section 21, `elev.rs`, `conv6.rs`): the elevate routines and 7 of the convert group, vec100.
- tri1 (section 30, `sort.rs`): all 15 routines of `sort.c`; 4,000 cases, 3,963 identical, 37 not compared; mutation `calls` has no effect.
- nurbsgeom7 (section 24, `conv6.rs`, `geom7.rs`): `checkValidOpenLinear`, `nurbsSrfAveragePointsDistance`,
  `SI_nurbsDistanceToValueFrenet`, `SI_spline2NurbsCrvKeepCurve`, `initNurbsPointsVKnotsFromBspline`.
- nurbsgeom8/9 (sections 25, 27, `geom8.rs`, `geom9.rs`): `nurbsCrvAddKnotNonUniform`, `openNormal`, `fitPlaneToSamples`,
  `SI_nurbsSrfConvertToLinear` (cited), `updateNurbsEvaluationMatrix` (585 of 700 identical, 0 different).
- nurbstess1 (section 26, `flatten.rs`, `chgknots.rs`): `SI_nurbsCrvFlatten`, `nrb_CRVsubdivc`, `nrb_subdivc`, the two
  `ChangeKnotsSeq` routines (700 cases each). Not done: `SI_nurbsSrfSplit`, `SI_nurbsCrvSplit`, `SI_nurbsSrfInsertKnot`,
  `si_OLDnurbsSrfChangeKnotsSeq`, the grid/line group (0x084e35bc..0x084e55a4), `TriangulateTrimmedNurbsSrf`.
- nurbsgeom10 (sections 28, 29, `geom10.rs`, `routed.rs`): `partitionLinearData` (700 of 700 identical); `nurbsCrvAddKnotNonUniform` routed.
- nurbsgeom11 (section 32, `geom11.rs`): `fitNurbsPoints`, `SI_fitNurbsCurve`, `SI_nurbsUpdateFrenet` (now routed in si-nurbs and si-deform).
- nurbs13 (section 35, `geom13.rs`, `geom13sys.rs`): `SI_skin2Nurbs`, `SI_skin2NurbsFarin` and the four closed-curve helpers cited (1,000 + 1,000 + 1,600 cases; Farin has 6 flagged words, explained there); `SI_createSkinFromNurbs` checked in section 36 (2,000 cases, 0 different); the 6 Farin flags are settled there.
- last1 (last section, `close.rs`): `SI_nurbsSrfClose` and `SI_nurbsConvertToFace` ported; `SI_createSkinFromNurbs` now calls the first directly. Checks and open hooks in the last section.
- Hooks still answered 0 or 1: `si_trmSrfHitInd (0x084e497c)`, `SI_findNURBSPathFcurveOwner (0x0821f7e4)`,
  `SI_deleteFcurvePathKey (0x0821ff50)`, `SI_addFcurvePathKey (0x08220008)`.
- nurbstess2 (section 31, `split.rs`, `tess2.rs`): `SI_nurbsCrvSplit`, `SI_nurbsSrfSplit`, `SI_nurbsSrfNoTriangles`, `SI_nurbsCrvSamples`, `si_nurbsSrfSampleTrims` cited (1,300 cases, 0 different); `SI_nurbsSrfInsertKnot` and `si_OLDnurbsSrfChangeKnotsSeq` partial (11 refusals); Flatten stand-ins replaced in the checkers.
- Not done: `nrb_effect.c` (modeller/UI commands, among them `SI_nurbsSrfAddRowCol 0x084efa2c`), `patchCardinalConvertToNurbsSrf`,
  `SI_patchConvertToNurbsSrf` (hooks: `SI_skin2NurbsFarin 0x08146210`, `SI_createSkinFromNurbs 0x08142ff0`,
  `SI_skin2Nurbs 0x08144c4c`, `SI_besselEndNurbsCrv 0x08147e80`).

## 1. The records

All sizes and offsets are of the 32-bit build; a `double` inside a record is aligned to 4.
`si-types` has `NurbsDir` (0x24), `NurbsCrv` (100), `NurbsPoint` (0x24), `NurbsSrf` (0xdc);
finding 0010 has their full tables. What this package establishes, corrects or adds:

### A direction (`SI_NurbsDir`, 0x24 bytes; once in a curve, twice in a surface)

| Offset | Width | Field | Meaning | Evidence |
|---|---|---|---|---|
| +0x00 | 32 | order | degree + 1 | [read] |
| +0x04 | 32 | point count | | [read] |
| +0x08 | 32 | knot count | **points + order − 2** when open, **points + 1** when closed (section 2) | [read] from the span search and the evaluators; [measured] on every curve and surface of the sample models |
| +0x0c | pointer | knots | doubles | [read] |
| +0x10, +0x18 | 2 doubles | range start, end | the parameter range; every span search clamps to it. Equal to `knots[order − 2]` and `knots[count − order + 1]` (open), the first and last knot (closed), by `si_nurbsDirInverse` and `SI_nurbsSrfExtractCurveSeg`, which write it so | [read] |
| +0x20 | 16 | closed | tested as zero or not; never used as a number | [read] |

That the type's name is `SI_NurbsDir` is [read] from the signatures of `si_nurbsInterv`,
`si_knotsInitSequence`, `si_nurbsDirInverse` (finding 0010 had it as inferred).

### A control point (`SI_NurbsPoint`, 0x24 bytes)

x, y, z, w as doubles and a 32-bit flag word. **x, y and z are stored multiplied by the
weight** (homogeneous coordinates): every evaluator blends the four doubles alike and
divides by the blended w at the end [read]. A point from `SI_nurbsPointsAllocate` is
(0, 0, 0, 1) with flag 0. The flag word travels with the point wherever a whole record is
copied, so the "homogeneous point" evaluators return one (section 4).

### A curve (`__SI_NurbsCrv`, 100 bytes): corrections to `si-types`

| Offset | Width | `si-types` name | What it is | Evidence |
|---|---|---|---|---|
| +0x24 | 32 | `param` | copied by every copy; `SI_nurbsSrfExtractCurveU/V` give the new curve the **surface's** +0x4c | [read] |
| +0x34 | 32 | `step` | the extraction gives the new curve the surface's step of that direction (+0x74 for a curve in V, +0x70 for one in U) | [read] |
| +0x48 | 32 | `unk_48` | cleared by `SI_nurbsCrvCopy` and by `SI_nurbsCrvDispose` (in the record it then frees); not copied by `SI_nurbsCrvInit`. A pointer to something the curve does not own is a [guess] | [read] |
| +0x4c | 16 | `keyframed` | when not 0, `SI_nurbsCrvInverse` calls `SI_findNURBSPathFcurveOwner` and `SI_invertFcurvePathKeys`: the curve is a path with keys tied to its points, as for splines (finding 0020) | [read] |
| +0x50 | 32 | `length_data[0]` | **number of entries** of the two tables that follow | [read] `SI_nurbsCrvCopy`, `Init`, `Dispose` |
| +0x54 | pointer | `length_data[1]` | table of doubles, that many | [read]; that it holds parameters is a [guess] until `SI_nurbsCurveUpdateLength` is read |
| +0x58 | pointer | `length_data[2]` | table of doubles, that many | [read]; cumulated lengths is a [guess] |
| +0x5c | pointer | `unk_5c[0]` | a block `SI_nurbsCrvDispose` frees; never copied | [read] |
| +0x60 | 32 | `unk_5c[1]` | 0 from the allocator; not copied | [read] |

Not copied by `SI_nurbsCrvCopy`: +0x48 (cleared), +0x5c, +0x60. Not copied by
`SI_nurbsCrvInit` either, nor +0x30 (`closing`).

### A surface (`SI_NurbsSrf`, 0xdc bytes): corrections to `si-types`

| Offset | Width | `si-types` name | What it is | Evidence |
|---|---|---|---|---|
| +0x48 | 32 | `nb_points` | total; **point (i in U, j in V) is at index `j × nu + i`: V is the major direction** | [read] every indexing routine |
| +0x70, +0x74 | 32 each | `u_step`, `v_step` | exchanged by `SI_nurbsSrfSwapUV` | [read] |
| +0x78, +0x7c | 32 each | `u_nb_curves`, `v_nb_curves` | display curves; +0x7c multiplies the **U** knot count in `SI_nurbsSrfNoVlines`, +0x78 the V count in `SI_nurbsSrfNoUlines` | [read] |
| +0x80 | 32 | `lines0` (a pointer) | the **number of display lines** | [read] `SI_nurbsSrfSwapUV` allocates that many 20-byte records |
| +0x84 | pointer | `lines1` | the display lines, **20 bytes each** | [read] |
| +0x88 … +0x8e | 4 × 16 | capping flags | exchanged in pairs by the swap: +0x88 with +0x8c, +0x8a with +0x8e | [read] |
| +0x90, +0x94 | 32 each | `unk_90` | exchanged by the swap; the file's "closing in U, V" tokens (finding 0004) by position | [read]; meaning [name] |
| +0xd8 | pointer | `unk_c8[4]` | root of a `GenTree` of Bézier patches (`SI_genTreeDestroy(…, SI_bezPatchDestroy)` in the disposal): the bounding-box hierarchy the intersection code builds. Hidden (cleared and restored) around `SI_nurbsSrfDisposeLines` by the two list disposals | [read]; purpose [name] |

`SI_nurbsSrfCopy` does not copy: the display lines (+0x80, +0x84), +0xb4 to +0xd8.

### A composite curve (`SI_NurbsCompCrv`, 0x18 bytes; new: `si_nurbs::own::CompCrv`)

A trim loop or a projected curve of a surface: a list of curves.

| Offset | Width | Meaning | Evidence |
|---|---|---|---|
| +0x00 | 16 | the file's "boundary" flag (finding 0004). **Not set by `SI_nurbsCompCopy` and `SI_nurbsCompFromCurve`**: it then holds the allocator's mark 0x6f in its low byte and heap content above; `SI_nurbsSrfTrmCpy`/`ProjCpy` copy it, but only for a composite with at least one curve | [read], [measured] |
| +0x04 | 32 | number of curves | [read] |
| +0x08 | pointer | array of pointers to curves | [read] |
| +0x0c | pointer | the trim line (`SI_trimLine`, the trimming package's): `nrb_b.c` reads four doubles at +0x10, +0x18, +0x20, +0x28 of it and a singly linked list from +0x08 whose nodes start with two doubles and link at +0x14. Not set by `SI_nurbsCompCopy`/`FromCurve` (heap content) | [read] |
| +0x10 | 32 | cleared by every maker; not read in these files | [read] |
| +0x14 | pointer | a block the disposal frees; cleared by every maker | [read] |

A surface's trims are at +0x60 (count) and +0x64 (array of pointers to composites), its
projected curves at +0x68 and +0x6c (finding 0010).

### Globals and statics

| Address | Name | What |
|---|---|---|
| 0x086d9c20 | `left.272` | static of `si_nurbsInterv`: the span index it last returned. 0 at start. Section 3 |
| 0x086d9c0c, 0x086d9c10, 0x086d9c14 | `aj.249`, `dl.250`, `dr.251` | scratch arrays of `SI_nurbsSrfExtractCurveSeg`, grown with `SI_realloc` and never freed |
| 0x086d9c1c, 0x086d9c18 | `nb_aj.253`, `nb_dlr.252` | their sizes |
| 0x0878896c | `SI_gblIndex` | when not null, a table of integers in which the reordering routines record where each control point went (finding 0020): `SI_nurbsCrvInverse` writes both indices of each exchanged pair, `SI_nurbsSrfSwapUV` appends the new index of each point in the old order |

Constants (doubles; the compiler emitted one copy per use, all listed by address):

| Value | Bits | Addresses | Used for |
|---|---|---|---|
| 1e-6 | 0x3eb0c6f7a0b5ed8d | 0x08661660 (`SI_nurbsCrvValue`), 0x08661668 (`SI_nurbsCrvValue4D`), 0x08661670 (`SI_nurbsKnotsCorrectMultAndValue`), 0x08661678 (`SI_nurbsCrvKnotInsert`), 0x08661690 and 0x086616a0 (`SI_nurbsSrfExtractCurveU`, `V`), 0x086616a8 (`SI_nurbsSrfExtractCurveSeg`) | blend factors, the weight, coinciding knots |
| 0.999999 | 0x3feffffde7210be9 | 0x08661688, 0x08661698 (`SI_nurbsSrfExtractCurveU`, `V`) | upper test of a blend factor |
| 1e-5 | 0x3ee4f8b588e368f1 | 0x08661700 (`si_nurbsInterv`) | "at the end of the range" |
| 0.001 | 0x3f50624dd2f1a9fc | 0x086616b0 (`SI_nurbsSrfValueAndNormal`) | a derivative has vanished |
| 0.0001 | 0x3f1a36e2eb1c432d | 0x086616b8 | near the range end; least length of a normal |
| ±0.01 | 0x3f847ae147ae147b | 0x086616c8, 0x086616c0 | step of the search for a derivative |
| 1e-6 | 0x3eb0c6f7a0b5ed8d | 0x086616e0, 0x086616f8 (`SI_SampleKnotVector`, `SI_samplesPerKnot`), 0x08661720, 0x08661730 (`SI_nurbsSrfKnotInsertU`, `V`) | a span too short to sample; the insertions' tests |
| 0.01, 0.1 | | 0x086616d0 and 0x086616d8 (`SI_SampleKnotVector`), 0x086616e8 and 0x086616f0 (`SI_samplesPerKnot`) | spans that count for the mean length; the least mean |
| 0.5 | | 0x08661680 (`SI_nurbsCrvKnotInsert`), 0x08661728, 0x08661738 (`SI_nurbsSrfKnotInsertU`, `V`) | the triangle's level offsets, computed in doubles |

## 2. Knot vectors, parameterisations

**Layout** [read: `si_nurbsInterv`, every evaluator; measured on the corpus]. A direction
of order k with n points holds

- **open**: `n + k − 2` knots. The textbook vector of `n + k` knots (Piegl and Tiller, *The
  NURBS Book*) without its first and last entry: full multiplicity at an end is `k − 1`
  equal knots. The parameter range is `knots[k − 2] ..= knots[n − 1]`; span `left` (with
  `knots[left] ≤ t < knots[left + 1]`) uses the k control points from `left − k + 2`.
- **closed**: `n + 1` knots, one period. The range is `knots[0] ..= knots[n]`; there are n
  spans; a knot index below 0 or above n is reached by subtracting or adding the period
  `knots[n] − knots[0]`, and control point indices wrap modulo n. Nothing requires the knots
  to be uniform.

**The uniform start sequences** (`si_knotsInitSequence(dir, kind)`): with
`d = 1 / ((n − k) + 1)` kept in the register, kind 0 writes `knots[i] = d × ((i − k) + 1)` to
every knot (unclamped; knot `k − 1` is 0); kind 1 writes k zeros, then the same expression up
to index `n − 1`, then ones to the end of the vector; any other kind writes nothing. Kind 1 is
a clamped vector **in the textbook layout** (`n + k` knots), not in the layout the evaluators
read: with the program's knot count it yields one more zero and one fewer one than a clamped
vector has. The routine has **no caller** in the binary (static call graph): it is a
leftover of an earlier layout, like the knot-sequence routines of section 12.

**Parameterisation** (curve +0x24, surface +0x4c; the files' "uniformity" token, finding
0004): carried by every copy and extraction, never read by the routines ported here. Its
values are the business of `SI_knotsCrvReparametrization` (not ported; by its helpers' names
uniform, chord length, centripetal: [name]).

**Reversal** (`si_nurbsDirInverse`): the knot **intervals** are reversed, not the knots:
`Δ[i] = knots[i + 1] − knots[i]` (each rounded to double), then
`knots[i] = knots[i − 1] + Δ[count − i + first − 2]` for i from `first`, where `first` is 2 on
a closed direction whose order is not 3, else 1. The first knot (the first two in the closed
case) keeps its value, so a reversed vector starts where it started and accumulates rounding
from there; the range is then read back from the knots. `SI_nurbsCrvInverse` exchanges the
control points end for end, calls this, and tells the animation package when the curve is
keyframed.

## 3. From a parameter to a span: `si_nurbsInterv(dir, t, open)`

All evaluators call it with `open` = "the direction is not closed".

1. **Clamp.** `t` is raised to the range start when that is above it and lowered to the range
   end when that is below it. A NaN passes both tests unchanged.
2. **Bounds.** `lo = order − 2`, `hi = count − order + 1` when `open` is not 0; else `lo = 0`,
   `hi = count − 1`.
3. **Start.** The static `left` (the last answer) is kept when `lo ≤ left < hi`, else it
   becomes `(hi + lo) / 2` (towards zero).
4. **At the end of the range**: when `|t − knots[hi]|` (the difference in the 80-bit
   register) is **below 1e-5**, the answer is `hi − 1`: the last span. On a closed direction
   it then steps down while `|t − knots[left]|` is still below 1e-5, **with no lower bound**:
   a closed direction whose knots all lie within 1e-5 of its end makes it read in front of
   the array.
5. **Otherwise a bisection**: while `t < knots[left]` or `t ≥ knots[left + 1]`: `hi = left`
   in the first case, `lo = left` in the second, and `left = (hi + lo) / 2`.

Consequences:

- **Exactly on a knot**, the parameter belongs to the span that *starts* there; on a multiple
  knot, to the span after the last of the equal knots. Only the range end is different (the
  last span, by rule 4), and by rule 4 so is everything within 1e-5 below the end: a
  parameter in `(end − 1e-5, end)` that sits in an earlier, very short span is still
  evaluated in the last span, with its own value of t.
- **Outside the range only the search is clamped.** The routine clamps its own copy of the
  parameter; its callers go on with the parameter they were given. What the evaluators make
  of that is in section 4 ("Outside the range"). In particular **a closed curve is not
  periodic in its parameter**: callers wrap, these routines do not.
- A NaN ends rule 5 at once: the answer is the start index, i.e. whatever the last call left.
- **The bisection has no exit of its own.** If the clamped parameter is outside
  `knots[lo] .. knots[hi]` (a range that disagrees with the knots) it never returns
  [measured: the emulator's instruction budget]. The port returns `None`.
- The static makes the routine's answer depend on earlier calls only in those undefined
  cases; it is carried in `si_nurbs::Statics` all the same and compared after every call.

`si_OLDnurbsInterv(dir, t)` (used by the old subdivision routine): no clamp, no tolerance,
no closed case. 0 when t is below the first knot or above the last; else the knots are
scanned down from the last to index `count − order − 1`, and the first one **strictly below**
t gives its index plus 1 (a parameter equal to a knot passes it); if none is, the vector is
scanned up from the start for the first knot above t. It returns one more than
`si_nurbsInterv` would, in the textbook's convention. When `count − order − 1` is negative
the downward scan reads in front of the array.

## 4. A curve's point and derivatives

The method is **de Boor's algorithm** (the triangular scheme on the k control points of the
span; de Boor, *A Practical Guide to Splines*, and algorithm A3.1/A3.2 of *The NURBS Book* in
its effect), in homogeneous coordinates, with derivatives taken by differencing the first
levels of the triangle and the quotient rule for the rational part (*The NURBS Book*,
equation 4.8). Every routine starts the same way:

- `left` = the span (section 3);
- `dl[i] = t − knots[left − i]` and `dr[i] = knots[left + 1 + i] − t` for i in `0 .. k − 1`,
  each **rounded to double**. On a closed direction an index outside the vector goes through
  the period: `dl[i] = ((t − knots[0]) + knots[last]) − knots[left + count − 1 − i]` when
  `i > left`; `dr[i] = ((knots[last] − t) + knots[left + 2 + i − count]) − knots[0]` when
  `left + 1 + i ≥ count`; in the register, rounded once;
- the first control point is `left − k + 2`, plus n when that is negative; the k points are
  taken at successive indices modulo n.

One **blending level** on m pairs: `P[b] = P[b+1] + a × (P[b] − P[b+1])` with
`a = dr[b] / (dl[m − 1 − b] + dr[b])`; `a`, the difference, the product and the sum stay in
the register, each of the four coordinates is rounded to double once. One **difference
level**: `P[b] = (P[b+1] − P[b]) × (m / (dl[m − 1 − b] + dr[b]))`, likewise.

**`SI_nurbsCrvValue(curve, t, nd)`** returns three doubles: the `nd`-th derivative.

1. For each order d from 0 to nd: the k points afresh, d difference levels (on k − 1,
   k − 2, … pairs), then the remaining k − 1 − d blending levels; the first point is the
   homogeneous derivative `A⁽ᵈ⁾` (x, y, z, w).
2. `inv = 1 / w⁽⁰⁾`, kept in the register.
3. `C⁽ᵈ⁾ = (A⁽ᵈ⁾ − Σ_{j<d} binom(d, j) × w⁽ᵈ⁻ʲ⁾ × C⁽ʲ⁾) × inv`, j ascending: `binom × w` in
   the register, each subtraction rounded to double, then the product with `inv` rounded.

Departures from the published algorithm, all [read] and [measured]:

- **Blend factors near 0 or 1 are not blended.** When `1 − a` is below 1e-6 the point is
  left as it is; when `a` is below 1e-6 it becomes a copy of its neighbour (all 36 bytes,
  the flag word too). So a curve evaluated within a millionth of a span's length of a knot
  returns exactly the value at the knot, and the curve has a (tiny) step there.
- **Outside the range the blend factors are clamped to [0, 1], not the parameter.** The two
  tests are one-sided. With a parameter beyond the range end the span is the last one
  (section 3) but `dr` is negative, the factors are below 0, and each such factor copies the
  right neighbour; beyond the start they exceed 1 and each keeps the left one. Far enough
  out every factor does, and the result is the last (first) control point of the end span:
  **the end point of a clamped curve, bit for bit, but for an unclamped or closed curve a
  point that is not on the curve**; just outside, some factors are clamped and some blend,
  which gives neither the end's value nor the polynomial's continuation. The difference
  levels have no such test: a derivative outside the range is the end span's polynomial
  differenced, with the clamped blends on top.
- **A point of zero weight is returned undivided**: `inv` is 1 when `|w⁽⁰⁾|` is not above
  1e-6. (The surface normal routine has no such test: section 6.)
- **A derivative order above the degree is not zero**: the difference levels beyond the
  degree have nothing to do, and the last real derivative is returned again.
- The whole triangle is recomputed for every derivative order (d + 1 passes for d
  derivatives), not built once.
- `si_binom(n, k)` multiplies in 32 bits: `n × (n − 1) × … × (max(k, n − k) + 1)` divided by
  the factorial of the other; it wraps from n = 13 or so and returns nonsense for k > n.

Scratch arrays are on the stack up to order 4 (and up to derivative order 3), from
`SI_calloc`/`SI_nurbsPointsAllocate` above.

**`SI_nurbsCrvValue4D(curve, t)`** is step 1 for d = 0 alone: the homogeneous point, as a
whole control-point record. Its **flag word** is the first span point's, or a later one's
where a factor below 1e-6 copied a point over it.

## 5. A surface's point

**`SI_nurbsSrfExtractCurveSeg(surface, u, v, swap, nd, out)`** is what every surface
evaluator stands on: the **one-span** curve in V that the surface is at the parameter u, valid
for the V span that holds v. It is an open curve of `order_v` points and `2 × order_v − 2`
knots.

1. The spans of u and v (section 3; U first). `dl`, `dr` for U as in section 4, except that
   here the closed-case `dl` is **rounded twice**: `(u − knots[0]) + knots[last]` is stored
   as a double before the last subtraction.
2. The `order_u × order_v` control points of the two spans are gathered; each index wraps to
   0 exactly once, when it reaches the count.
3. De Boor's triangle in U on all rows at once: `nd` difference levels, in which **the
   difference is stored as a double and then multiplied** (two roundings where the curve
   evaluator has one), then the blending levels with the two 1e-6 short cuts of section 4,
   decided once a factor for all rows.
4. The first point of each row is the curve's control point; its knots are the
   `2 × order_v − 2` knots of V from `left_v − order_v + 2`, through the period where the
   index leaves the vector (`(knots[0] + knots[i + count − 1]) − knots[last]` below,
   `(knots[last] + knots[i − count + 1]) − knots[0]` above, rounded once); its range is read
   from them.

The new curve is made with `SI_calloc`, not with the curve allocator: order `order_v`, not
closed, every other field zero (step 0, closing 0, where the allocator gives 10 and 1). When
the caller passes a curve in `out`, its order and knot count are trusted and only its points,
knots and range are written. With `nd` above 0 the result is the curve of the `nd`-th
derivative with respect to U (homogeneous: its "weights" are the derivative of w).
**`swap` = 1** exchanges the surface's directions in place before and after
(`SI_nurbsSrfSwapUV`, which reallocates the point array each time) and the two parameters
once: the result is then the curve in U at v.

- **`SI_nurbsSrfValue(surface, u, v)`** = `SI_nurbsCrvValue(segment(u, v, nd = 0), v, 0)`,
  the segment disposed of afterwards. So a surface point is rounded to double after every
  level in U, then after every level in V, and divided by the weight last; U is always
  reduced first.
- **`SI_nurbsSrfValue4D`** = `SI_nurbsCrvValue4D` of the same segment.

**Whole isoparametric curves.** `SI_nurbsSrfExtractCurveU(surface, u, nd)` returns the whole
curve in V at u (a curve from the allocator, with V's order, counts, knots, range and closed
flag, the surface's parameterisation and V's step count); `SI_nurbsSrfExtractCurveV` is the
mirror image; `SI_nurbsSrfExtractCurve(surface, t, dir, nd)` picks U for `dir` 0, V for
anything else. They run the same triangle row by row, with three differences from the point
evaluators:

- the factors of all levels are computed once and **rounded to double** before use;
- a blend is skipped (the point kept) when the factor is **not below 0.999999**, rather than
  when 1 minus it is below 1e-6. The two tests differ for factors in a band of about 1e-16
  around 0.999999 and for a NaN factor, which here keeps the point and there blends;
- a difference level rounds once (difference times the rounded factor).

So a point of an extracted curve and `SI_nurbsSrfValue` at the same parameters agree only to
rounding [measured on the corpus: section "Checks"].

## 6. The normal: `SI_nurbsSrfValueAndNormal(surface, u, v, normal, a, b)`

Returns the point (three doubles) and writes the normal. `a` and `b` are optional one-span
curves from the caller: the position curve and the curve of the derivative across it.

1. **Curves.** When either `a` or `b` is null, the routine extracts both itself
   (`SI_nurbsSrfExtractCurveSeg` with `nd` 0 and 1, `swap` 0) and evaluates them at v. When
   both are given, it evaluates them **at u**: the caller extracted them with U and V
   exchanged.
2. **Point.** `P = SI_nurbsCrvValue4D(position curve)`; `inv = 1 / P.w` with **no test of
   the weight** (a zero weight gives infinities); the point is `P.xyz × inv`, rounded to
   double. This is the result whatever happens below.
3. **Derivatives.** `across = (D.xyz − D.w × point) × inv` from
   `D = SI_nurbsCrvValue4D(derivative curve)`, all in the register, one rounding a
   coordinate (the quotient rule for a first derivative). `along =
   SI_nurbsCrvValue(position curve, ·, 1)`.
4. **A vanished derivative** (length `√((x² + y²) + z²)` in the register **below 0.001**) is
   searched for nearby. The step in each parameter is +0.01, or −0.01 when the parameter is
   within 0.0001 of that direction's range end (parameter units, whatever the knot scale).
   Up to 99 tries: tries 1 and 2 evaluate the curve already extracted at `v + ` the
   accumulated V step and add a V step; every third try extracts anew at
   (`u +` accumulated U step, `v +` accumulated V step), evaluates there and adds a U step.
   The first derivative that is long enough replaces the vanished one; after 99 tries the
   last one does. For `across`, a re-extraction also recomputes the point and `inv` it is
   formed with; the returned point is not touched.
   - without `a`: `across` is tested and searched for first, then `along`;
   - with `a`: `along` first, **searched for with the formula of `across`** (the derivative
     curve is evaluated in place of the position curve's derivative), then `across`, searched
     for with the formula of `along`. This reads as a slip of the original; it is reproduced.
   Which branch is taken depends on `a` alone, the extraction of step 1 on both: a caller
   passing one curve gets the extraction of "none" and the tests of "a".
5. **Normal.** `across × along` without `a`, `along × across` with it, each coordinate
   rounded to double. Its length `√((y² + x²) + z²)` is **rounded to float**; when that is
   above 0.0001 the three coordinates are divided by it (and rounded to double), otherwise
   the raw cross product is returned: **zero for a surface degenerate around the point, not
   a unit vector**.

So the fallback is a search along the parameters, not a higher derivative, and it is blind
to scale: on a surface whose range is 0 to 1 a step of 0.01 is a hundredth of the surface;
on one whose range is 0 to 1000 it is nothing, and a pole there is given up after 99 tries
with a zero normal.

## 7. Allocation, copies, disposal (`nrb_b.c`)

- `SI_nurbsPointsAllocate(n)`: `SI_malloc(36 n)`, each point (0, 0, 0, 1), flag 0; null for
  n ≤ 0. `si_knotsAllocate(n)`: `SI_calloc(n, 8)`; null for n ≤ 0.
- `SI_nurbsCrvAllocate`, `SI_nurbsSrfAllocate`: `SI_malloc`, every field set but the padding
  (ported in `si-types`; compared here).
- `SI_nurbsCrvCopy`: a new curve; knots only when their count is above 0, points only when
  the source's count is; the two length tables when their count is above 0.
  `SI_nurbsCrvInit(dst, src)`: the same into an existing record, whose old arrays are not
  freed; it leaves `dst`'s points alone when the source has none and clears the length
  fields when the source has no table.
- `SI_nurbsCrvDispose(&curve)`: knots, points, +0x5c, then the two tables (+0x54 before
  +0x58, only when the count is above 0), the record; the caller's pointer is cleared.
- `SI_nurbsCompCopy`, `SI_nurbsCompFromCurve`, `SI_nurbsCompDispose` (curves, the array,
  the trim line's nodes and the line, +0x14, the record).
- `SI_nurbsSrfTrmCpy(dst, src)` disposes of `dst`'s display lines first, copies the border
  flag (+0x5c) and the trims; `SI_nurbsSrfProjCpy` copies the border flag too and the
  projected curves, **without** touching the display lines. With a count of 0 both leave the
  destination's array pointer as it was.
- `SI_nurbsSrfCopy`; `SI_nurbsSrfTrmDispose`, `SI_nurbsSrfProjDispose` (each disposes of the
  display lines with the tree at +0xd8 hidden), `SI_nurbsSrfDispose`; `SI_nurbsSrfAddTrim`
  (a copy of the composite appended; the array is made anew when the count is below 1).
- `SI_nurbsSrfNoVlines` = `(knots_u − (2 (order_u − 2) + 1)) × (+0x7c) + 1` for an open U,
  `(knots_u − 1) × (+0x7c)` for a closed one; `SI_nurbsSrfNoUlines` likewise from V and
  +0x78.
- **`SI_nurbsSrfSwapUV`**: returns 0 and does nothing for a surface without points.
  Otherwise the points go into a new array transposed; the two direction blocks, the steps,
  the curve counts, the capping flags (in pairs) and +0x90/+0x94 are exchanged; without
  trims, the display line records are rotated so that the former V lines come first; for
  each trim, x and y of every control point of its curves are exchanged, and where the
  composite has a trim line, that line's doubles (+0x10 with +0x28, +0x20 with +0x18) and the
  two doubles of each of its nodes, after which the trimming package's `SI_trmCvrRve` is
  called on it; the old point array is freed. Returns 1.

## 8. Knot insertion

The method is **Boehm's knot insertion**, the multiple form (algorithm A5.1 of *The NURBS
Book* in its effect): the triangle of de Boor's algorithm is built r levels deep on the
span's control points and the new control polygon is read off its edges.

**`SI_nurbsKnotsCorrectMultAndValue(dir, &t, left, &r)`**: when `|t − range end|` is below
1e-6 the knot looked at is the **last of the vector** (index `count − 1`), else knot `left`.
When `|t − knot|` is below 1e-6: t becomes that knot exactly; the multiplicity is 1 plus the
number of knots directly below it within 1e-6 of it; r is lowered to
`(order − 1) − multiplicity` when above it. That limit is 0 on a knot of full multiplicity
and **negative** beyond.

**`SI_nurbsCrvKnotInsert(curve, t, r, correct)`**, in place; returns 0 when nothing is
inserted, else 1:

1. on a closed curve a parameter within 1e-6 of the range end becomes the **first knot**;
2. the span (section 3); with `correct` not 0 the correction above. r = 0 returns 0 here.
   **A negative r goes on**: on a knot whose multiplicity already exceeds `order − 1` (which
   the correction turns into a negative count) the routine then *removes* points and knots:
   the counts grow by the negative r and the arrays are rebuilt by the same loops
   [measured: such cases return 1 with fewer points];
3. the triangle: level 0 is the span's k points; level l + 1 is formed from level l with the
   factors and the two 1e-6 short cuts of section 4 (a copy of the left parent when
   `1 − a < 1e-6`, of the right one when `a < 1e-6`, else `right + a × (left − right)`
   rounded to double a coordinate); r levels. **A blended point's flag word is never
   written**: for orders up to 4 the triangle is on the stack and the new point enters the
   curve with stack content in its flag (from order 5 the triangle comes from
   `SI_nurbsPointsAllocate` and the flag is 0) [measured: 921 of 2,000 cases have such a
   word]. Copied points carry their parent's flag;
4. the new points, r more than before: the old ones up to the span's first; the first points
   of levels 1 to r − 1; all of level r; the last points of levels r − 1 down to 1; the old
   ones from the span's last on. On a closed curve the positions run modulo the new count.
   When the span's points wrap round the start of the array (closed, `left < order − 2`)
   the same pieces are laid out from index 0 instead: the tail of the insertion, the old
   points that do not belong to the span, then its head (the bookkeeping is done partly in
   doubles: `c × k − c (c − 1) × 0.5`, truncated). The new array is as long as the highest
   index written plus one;
5. the knots with r copies of t after knot `left`; the range read back from them; **the
   parameterisation word (+0x24) is set to 2**;
6. where `SI_gblIndex` is set, the new index of every old point that kept its place in the
   sequence is written to it.

**`SI_nurbsSrfKnotInsertU(surface, t, r)`**, **`…V`** and the dispatcher
**`SI_nurbsSrfKnotInsert(surface, t, r, dir)`** (0: U) do the same on every row (column) of
the grid at once, with these differences: the correction is always applied; the new array is
`(n + r) × other` whatever is written; all triangles lie in one allocated block, so blended
points have flag 0; the total (+0x48) and the parameterisation word (+0x4c, 2) are updated;
`SI_nurbsSrfDisposeLines` is called at the end. The two direction routines return 16 bits.

## 9. Sampling

Both samplers walk the spans of the range (`order − 2 .. count + 1 − order` open,
`0 .. count − 1` closed) and scale "samples a span" by the span's length: with
`mean` = the mean length of the spans longer than 0.01 (raised to 0.1 when smaller, or when
there is none) and `unit = per / mean` (rounded to double), a span of width w gets
`m = per` samples, or **`4 × per` when `w × unit` exceeds `4 × per`** (through the C
library's `rint` and a truncating conversion, which change nothing on a whole number). So a
long span gets at most four times the samples of an average one, and a short one never
fewer than `per`.

- **`SI_SampleKnotVector(dir, per, &array, &count)`**: the parameters themselves. Spans not
  longer than 1e-6 are skipped (nothing at a multiple knot); every other span gives its
  start knot and, unless the order is 2 and `per` is 1, the m − 1 parameters
  `j × (w / m) + knot` (`w / m` in the register, one rounding each); the last entry is the
  range's end knot. Nothing is done for a null direction or one without knots. The count is
  computed in a first pass as the sum of m (1 where m is 0) plus 1, which agrees with the
  second pass only for m ≥ 0: **a negative `per` makes the array too small** for what is
  written [measured: write faults]. Called by the mesh builders (skin, extrusion,
  revolution), the conversions to polylines and the triangulation of untrimmed surfaces.
- **`SI_samplesPerKnot(dir, per, &array, &count)`**: one double a span: 0 for a span not
  longer than 1e-6, else m (1 where m is 0); returns their sum. Called by the drawing code.
- **`getKnotPosition(out, knots, t, n)`** (static): the n knots copied with t inserted in
  order; returns where: before knot 0 when strictly below it, else before the first later
  knot not below it, else at the end. After the insertion the value carried is
  `knots[n − 1] + 1`, in the register.
- **`SI_nurbsCrvSamples(curve, mode, step, &count, &points)`** (mode 0 only):
  `floor((range end − range start) / step) + 1` points from the range start; the parameter
  is kept **in the register** and grows by `step` a point without being rounded, is lowered
  to the range end when above it, and is rounded to double for each evaluation. Called by
  the trim sampler.

## 10. The small routines of `nrb_misc.c`

Bits 0x60 of the highest byte of a point's flag word (+0x23), of a curve's +0x38 word
(+0x3b) and of a surface's +0x58 word (+0x5b) are set by every routine here that changes
points ("moved"; what clears them is not read).

- `SI_nrbPtsMultiplyMatrix(src, m, dst, n)`: `p × M` with row vectors (finding 0008),
  coordinate q = `((x m₀q + y m₁q) + z m₂q) + w m₃q` in the register, one rounding. The four
  coordinates; flags of `dst` untouched.
- `SI_nurbsCrvApplyMatrix(curve, m, mask)`: the same on every point (with a mask: on the
  points whose flag has one of its bits), each marked moved; the curve is marked whether any
  point was touched or not.
- `SI_nurbsCrvMovePoint(curve, i, x, y, z)`: the point's coordinates become the given
  cartesian ones times its weight; no test of the index; returns 1.
- `SI_nurbsCrvTransferPoints`, `SI_nurbsSrfTransferPoints` (the source's count of whole
  records; the destination marked moved), `SI_nurbsTransferDirKnots` (the source's knots and
  range; counts untouched), `SI_nurbsCrvTransferPointsKnots`,
  `SI_nurbsSrfTransferPointsKnots` (which first disposes of the destination's display lines).
- `SI_nurbsCrvGetCtrlVertex`, `SI_nurbsSrfGetCtrlVertex(…, i, out)`: `SI_CtrlVertex` is
  0x24 bytes: cartesian x, y, z (`coordinate × (1 / w)`, no test of the weight), w, flag.
  Error 0x3b when i is **above** the count: i equal to the count reads one record past the
  points.

## 11. Checks

**Emulation** [measured]. `re/tools/si-nurbs/emu_run.sh`: `emu_gen.py` steps each routine of
the original on made-up curves and surfaces and `nurbs-emucheck` compares results, the
span-search static, and for routines that build or change a curve or surface every word of
the record, knots, points, tables, lists and display lines, plus the order of frees, the
index table and the calls into other packages.

Made-up inputs: orders 2 to 8; open and closed; 0 to 8 points more than the order; knot
vectors uniform, clamped (full multiplicity at the ends), random steps, with multiple knots
inside up to full multiplicity, "Bézier" (every interior knot of full multiplicity), with
steps around the tolerances (1e-7 to 1e-3), shifted and scaled; points random, on a grid, on
a line, repeated, all equal, in a plane, tiny, huge; weights 1, random, powers of two, with
zeros and values around 1e-6, negative; 2% with an infinity, a NaN or a denormal. Parameters:
exactly on a knot, exactly on either range end, a hair either side of those (1e-12 to 1e-3,
including just below and above 1e-5), inside, outside, non-finite. Surfaces also as height
fields, tubes, with poles, flat, collapsed to a point, with coinciding rows or columns; with
trims, trim lines, projected curves and display lines for the record routines. Some 4% of
the inputs are **hostile on purpose** (a range that disagrees with the knots, an order of 1,
a point or knot count one short, a non-finite knot, a closed direction narrower than the
search tolerance, more coinciding knots than the order allows, a negative sample count); the
generator marks them, and only there may the port refuse.

Columns: identical; different; S the original did not finish (instruction budget or fault);
R hostile input the port refuses; U undefined in part in the original (memory it never
wrote) and identical in every defined word.

| Routine | Cases | Identical | Different | S + R + U |
|---|---|---|---|---|
| `si_binom`, `si_knotsAllocate`, `si_knotsInitSequence`, `SI_nurbsPointsAllocate`, `getKnotPosition`, `SI_nurbsKnotsCorrectMultAndValue` | 12,000 | 12,000 | 0 | 0 |
| `SI_nurbsCrvAllocate`, `SI_nurbsSrfAllocate` (no inputs) | 40 | 40 | 0 | 0 |
| `si_nurbsInterv` | 2,000 | 1,919 | 0 | 81 |
| `si_OLDnurbsInterv` | 2,000 | 1,988 | 0 | 12 |
| `si_nurbsDirInverse` | 2,000 | 1,994 | 0 | 6 |
| `SI_nurbsCrvValue` | 2,000 | 1,966 | 0 | 34 |
| `SI_nurbsCrvValue4D` | 2,000 | 1,978 | 0 | 22 |
| `SI_nurbsSrfExtractCurveSeg` | 2,000 | 1,958 | 0 | 42 |
| `SI_nurbsSrfValue` | 2,000 | 1,948 | 0 | 52 |
| `SI_nurbsSrfValue4D` | 2,000 | 1,946 | 0 | 54 |
| `SI_nurbsSrfExtractCurveU`, `V`, `SI_nurbsSrfExtractCurve` | 2,500 | 2,468 | 0 | 32 |
| `SI_nurbsSrfValueAndNormal` | 2,000 | 1,972 | 0 | 28 |
| `SI_SampleKnotVector` | 2,000 | 1,788 | 0 | 212 |
| `SI_samplesPerKnot` | 2,000 | 1,992 | 0 | 8 |
| `SI_nurbsCrvSamples` | 1,000 | 991 | 0 | 9 |
| `SI_nurbsCrvKnotInsert` | 2,000 | 1,063 | 0 | 937 (921 of them U: the flag words of section 8) |
| `SI_nurbsSrfKnotInsertU`, `V`, `SI_nurbsSrfKnotInsert` | 2,991 | 2,915 | 0 | 76 |
| `SI_nurbsCrvCopy`, `SI_nurbsCrvInit`, `SI_nurbsCrvDispose` | 5,000 | 5,000 | 0 | 0 |
| `SI_nurbsCrvInverse` | 2,000 | 1,997 | 0 | 3 |
| `SI_nurbsCompCopy`, `SI_nurbsCompFromCurve`, `SI_nurbsCompDispose` | 3,000 | 3,000 | 0 | 0 |
| `SI_nurbsSrfCopy`, `SI_nurbsSrfTrmCpy`, `SI_nurbsSrfProjCpy`, `SI_nurbsSrfTrmDispose`, `SI_nurbsSrfProjDispose`, `SI_nurbsSrfDispose`, `SI_nurbsSrfAddTrim` | 7,000 | 7,000 | 0 | 0 |
| `SI_nurbsSrfNoVlines`, `SI_nurbsSrfNoUlines` | 1,000 | 1,000 | 0 | 0 |
| `SI_nurbsSrfSwapUV` | 2,000 | 2,000 | 0 | 0 |
| the ten of section 10 | 10,000 | 10,000 | 0 | 0 |
| **total, 54 routines** | **72,531** | **70,923** | **0** | **1,608** (S 489, R 233, U 886) |

No clean input was left uncompared in a routine that returns: every S and R case carries
the generator's hostile mark. Of the 1,980 compared cases of the value-and-normal routine,
404 went through a search for a vanished derivative and 108 of those ended it early
(between 2 and 99 tries), in all four combinations of given curves [measured with
`EMUCHECK_TRACE=1`].
The insertion vectors cover the wrapped layout of a closed curve (section 8, step 4) with
r from 1 to 4.

With `EMUCHECK_MUTATE=1` (one bit of the port's value flipped before each comparison)
71,190 cases differ (the 70,923 and the 886 of column U, less 619), 722 are not compared
(S and R), and 619 stay identical: the cases of `si_knotsInitSequence` whose kind writes
nothing.

**What the comparison corrected.** The arithmetic of every port was right at its first
comparison, the value-and-normal routine with its two searches and the three insertion
routines included. Wrong were three things, each where the port had been written from an
assumption instead of a reading:

- `SI_nurbsSrfProjCpy` was taken for the mirror image of `SI_nurbsSrfTrmCpy` and so called
  the display-line disposal; it does not (it does copy the border flag);
- `si_OLDnurbsInterv`'s downward scan was read as stopping at a knot equal to the parameter;
  it passes it;
- the insertion routines refused a negative insertion count; the original goes on and
  removes points (section 8), which is now reproduced wherever no stale scratch memory ends
  up in the result.

And one statement of this note, found by a unit test rather than by the comparison (the
port was right, the prose was not): that a parameter outside the range is clamped. Only the
span search clamps; section 4 has what happens.

**Corpus** [measured]. `nurbs-check` over the sample models (`si-hrc` reads them): 1,905
model files, 39 with NURBS, no panic. **51 curves** (11 curve nodes, 19 of faces, 21 of
trims and projected curves; 1,450 control points; 9 rational), all of order 4: 44 closed, 7
open. **97 surfaces** (9,573 control points; 37 rational; 10 trims of one curve each, 11
projected curves):

| U × V | Count |
|---|---|
| order 4 closed × order 4 open | 75 |
| order 4 closed × order 2 open | 9 |
| order 4 closed × order 4 closed | 6 |
| order 4 open × order 4 open | 4 |
| order 4 open × order 4 closed | 1 |
| order 3 closed × order 4 open | 1 |
| order 3 open × order 3 open | 1 |

So the corpus has orders 2 to 4 only, and nearly every surface is a closed-by-open tube.
On all 245 directions the layout of section 2 holds: the knot count is `points + order − 2`
(open) or `points + 1` (closed), the range equals `knots[order − 2]` and
`knots[count − order + 1]` (open) or the first and last knot (closed) **bit for bit**, the
knots do not decrease; every surface has `nu × nv` points. Invariants of the port (not
comparisons with the original), all met:

- a clamped curve's value at each range end is the end control point bit for bit (14 of 14
  ends), and so is its value beyond the range (7 of 7 curves);
- a closed curve's value at the range end equals its value at the range start **bit for
  bit** (44 of 44);
- the first derivative agrees with a central difference (1,297 of 1,297 parameters);
- a knot inserted in the middle of every span raises both counts by the number of
  insertions (51 of 51 curves) and leaves the sampled curve within 1e-9 of its size (3,942
  of 3,942 samples; worst 3.9e-16 of the size);
- a surface point equals the homogeneous point divided (15,785 of 15,785); the point of the
  whole curve extracted in V at u, and of the one extracted in U at v, is the surface point
  to 1e-9 of the size (15,785 and 8,250; worst 4.7e-16);
- the normal is a unit vector to 1e-6 at all 15,785 sampled points, none zero; **1,617 of
  them needed the search for a vanished derivative** (the poles of the closed-by-open
  surfaces); where no search was needed it lies within 2.6 degrees of the cross product of
  difference quotients (5,060 of 5,060);
- U and V exchanged twice give the same surface (97 of 97); a copy has the same directions,
  knots, points and list lengths (97 of 97).

## 12. Where the work stands

For the next engineer. Addresses in `soft.modules.tsv`; "read" means from the instructions
unless it says decompiler.

**Ported and compared** (54): section 11's table. Rust: `crates/si-nurbs/src/own.rs` (the
only module that holds what a record points to: curve, composite, surface, their copies and
disposal, the exchange of U and V), `knots.rs` (span searches, start sequences, reversal of
a direction), `eval.rs` (points, derivatives, extraction, the normal), `insert.rs`,
`sample.rs`, `edit.rs` (curve reversal), `misc.rs`. The numerical modules take the views
`Dir`, `CrvRef`, `SrfRef` (a record and slices) or, where a routine reallocates, an
`own::Curve` / `own::Surface`.

**Not ported:**

| What | Functions | Notes |
|---|---|---|
| `eval.c`: the knot-sequence change | `si_nurbsCrvChangeKnotsSeq` (0x084d7c90), `nrb_CRVsubdivc`, `si_nurbsSrfChangeKnotsSeq`, `nrb_subdivc`, `si_OLDnurbsSrfChangeKnotsSeq` (0x084d822c), `SI_nurbsSrfInsertKnot` (0x084d84a4) | **Dead code**: none has a caller in the binary but each other (static call graph). The first four are read from the decompiler (the curve kernel's arithmetic from the instructions), the last two not at all: the **Oslo algorithm** (Cohen, Lyche, Riesenfeld 1980): each new control point from a (span + 1) × (order + 1) table, `(t1 × same row + t2 × row above) × (1 / (t1 + t2))` with `t1 = new knot − old knot[i]`, `t2 = old knot[i + k − r] − new knot`, all in the register (curve); the surface kernel uses one factor `a` with the 1e-6 short cuts and returns the surface with U and V exchanged. They count knots in the **textbook layout** (points = knots − order in the surface routine), which is not the layout of section 2: they predate it. `si_knotsInitSequence` (ported) has no caller either and shows the same older layout |
| `eval.c`: splitting | `SI_nurbsCrvSplit` (0x084d8e08, 1,468 bytes), `SI_nurbsSrfSplit` (0x084d8530, 2,264) | next in line: they stand on `SI_nurbsCrvKnotInsert` / `SI_nurbsSrfKnotInsert`, which are done. Not read |
| `eval.c`: basis matrices | `SI_nurbsMatrixBasisFunction` (0x084d93c4), `SI_nurbsCrvBasisFunctions` | return `SI_GenMatrixDbl` (`si-math` has the type). Not read |
| `nrb_b.c`: surface reversal | `SI_nurbsSrfInverse` (0x084dcaf8, 1,956 bytes), `si_nurbsSrfInverseForConversion` | `si_nurbsDirInverse` and `SI_nurbsCrvInverse` are done. Not read |
| `nrb_b.c`: reparameterisation | `SI_knotsCrvReparametrization`, `SI_knotsSrfReparametrization`, `knotsCrvReparamChordCentri`, `…Quad`, `nurbsSrfAveragePointDelta`, `si_nurbsCrvFillSegmentPoints`, `si_nurbsCrvFillDistancePoints`, `SI_shiftDirKnotsClosed` | these decide what the parameterisation word means. Not read. Since 2026-10-15 `SI_knotsCrvReparametrization`, `knotsCrvReparamChordCentri`, `…Quad` and `si_nurbsCrvFillDistancePoints` are on the image (§13.13) |
| `nrb_b.c`: dumps | `SI_nurbsSrfDump`, `SI_nurbsCrvDump` | `fprintf` of the records; the format strings would name fields. Not read |
| `nrb_geometry.c` | all 8: `SI_nurbsCurveUpdateLength`, `SI_nurbsCurveLength`, `SI_nurbsDistanceToFactor`, `SI_nurbsFactorToDistance`, `SI_nurbsUpdateFrenet`, `nurbsFrenetCoord`, `SI_nurbsDistanceToValueFrenet`, `SI_nurbsCrvAnimUpdate` | the two tables at +0x54/+0x58 of a curve are theirs; `SI_Coordsys` is not laid out. Not read |
| `nrb_convert.c` (second pass) | 15 of 19: the trimmed triangulation `TriangulateTrimmedNurbsSrf` (0x084fd14c, in `trimmed.c`'s company) is called from here; the conversions `SI_nurbsConvertToMesh`, `initNurbsPointsVKnotsFromBspline`, `initNurbsKnotsFromBezier`, `patchCardinalConvertToNurbsSrf`, `SI_patchConvertToNurbsSrf`, `SI_spline2NurbsCrvKeepCurve`, `SI_spline2NurbsCrvKeepCP`, `SI_nurbsCrv2SplineKeepCP`, `SI_nurbsCrvConvertToLinear`, `nurbsCrvConvertToLinearBezier`, `SI_nurbsCrvConvertToBezier`, `SI_nurbsConvertToFace`, `SI_nurbsSrfConvertToLinear`, `nurbsToMeshUpdatePolygonUV`, `getBezierPoint` | section 13. Not read. Since 2026-10-15 `SI_nurbsCrvConvertToLinear` is on the image (§13.13) |
| `nrb_misc.c` | `nurbsDirElevateOrder`, `SI_nurbsCrvElevateDegree` (1,308 bytes), `SI_nurbsSrfElevateDegree`, `SI_nurbsSrfApplyMatrix`, `SI_nurbsSrfMovePoint`, `SI_nurbsSrfMovePointNoCap`, the two `…ApplyFunc2Points`, `updateStartEnd`, `dirtyCurves` (the decompiler gives it `__regparm1`; not verified), `markDirtyCcrvs` (`getPointInterval`: ported in the second pass, 13.7) | the surface matrix and move routines are the curve's plus `markDirtyCcrvs` (which display lines a moved point invalidates); read from the decompiler only |

**Not determined:** the values of the parameterisation word (2 is "set by an insertion";
the rest belongs to the reparameterisation routines); what +0x48 of a curve points to; who
clears the "moved" bits; the layout of a display line record (20 bytes) and of
`SI_trimLine` beyond the fields of section 1; whether any caller relies on the evaluators'
behaviour outside the range; whether a caller passes `SI_nurbsSrfValueAndNormal` only one of
its two curves (the branch exists; `SI_surfaceValueAndTangent` and `nurbsSrfFunc` are the
callers to read).

**Hooks of other crates these can replace** (read from those crates' sources):

| Crate, hook | Now |
|---|---|
| `si-motion` `keyedit.rs`: `nurbs_crv_transfer_points_knots`, `nurbs_srf_transfer_points_knots` | `misc::crv_transfer_points_knots`, `misc::srf_transfer_points_knots` |
| `si-model` `face.rs`: `nurbs_crv_inverse`, `nurbs_crv_apply_matrix`, `nurbs_crv_transfer_points_knots` | `edit::crv_inverse`, `misc::crv_apply_matrix`, `misc::crv_transfer_points_knots` |
| `si-hrc`: the file's NURBS curve, composite and surface | `own::Curve::from_file`, `CompCurve::from_file`, `Surface::from_file` give the in-memory records |
| `si-motion` `path.rs`: `nurbs_curve_update_length`, `nurbs_factor_to_distance` | not yet: `nrb_geometry.c` |
| `si-render` `softtriangle.rs`: `Tessellators::nurbs2triangle` | done in the second pass: `convert::nurbs2triangle` on the image, bound by the renderer's adapter (finding 0012, section 22) |
| `si-model` `face.rs`: `knots_crv_reparametrization`; `nurbs_crv_convert_to_linear`, `nurbs_crv_delete_point`, `nurbs_face_add_point`, `nurbs_crv_draw`; `mesh_util.rs`: `nurbs_convert_to_mesh` | not this pass: reparameterisation is the rest of `nrb_b.c`; the others are `nrb_convert.c`, `nrb_point.c`, `nrb_draw.c`, which stand on `sample::sample_knot_vector` and the evaluators that now exist |

In the other direction, this crate's `own::Host` asks for `SI_nurbsSrfDisposeLines`
(`nrb_draw.c`), `SI_genTreeDestroy` with `SI_bezPatchDestroy`, `SI_trmCvrRve` (trimming),
`SI_gblIndex`, and `SI_findNURBSPathFcurveOwner` with `SI_invertFcurvePathKeys`, which
`si-motion` has (`path::find_nurbs_path_fcurve_owner`).

## 13. Second pass: a surface as the renderer's triangles, on the memory image

Date: 2026-10-14. Rust: `crates/si-nurbs/src/convert.rs` (new), `image.rs` (new); the binding
into the renderer is finding 0012, section 22. Scripts: `re/tools/si-nurbs/gen_convert.py`
(new). Outputs: `re/out/si-nurbs/scenes/` (the dumped scenes), `vec13/` (the vectors),
`nurbs_check13.txt`, `gen13_*.log`, `emucheck13*.txt`.

**Ported and cited: 4 functions of `nrb_convert.c`** (0x084df320, 19 functions; 4 of them
now), and `getPointInterval` of `nrb_misc.c` (13.7): `SI_nurbs2Triangle` (0x084e16f8), `TriangulateNonTrimmedNurbsSrf` (0x084e17f8),
`repeatKnotsInSampling` (0x084e28fc), `EvaluateNonTrimmedNurbsFromBaseGeometry`
(0x084e2ad0). **Given a second form, on the image**: `SI_SampleKnotVector`,
`SI_nurbsCrvValue4D`, `SI_nurbsCrvValue`, `SI_nurbsSrfExtractCurveU`, `…V` and
`SI_nurbsSrfExtractCurve` (`image.rs`). Everything below is read from the instruction
listing (`re/tools/si-render/lst.py`, and `re/tools/si-math/x87sym.py` for the order of the
x87 operations), the decompiler's output served as a map; and confirmed by stepping the
original (section 13.6), unless it says otherwise.

### 13.1 The path, from a node to its triangles

`SI_collect(node)` (finding 0012, section 3b) calls `SI_makeVertices(node, −1)`, which for a
node of kind 10 calls

    SI_nurbs2Triangle(shape, srf, materials, elem_id, &n_verts, &verts, &n_tris, &tris, flag)

with `shape` the node's definition that lends positions (the first of the array at +0x38
when +0x34 is 1, else the second), `srf` its current surface (+0x48), `materials` its
material array (+0x20c), `elem_id` +0x354, the four results at +0x228, +0x22c, +0x230,
+0x234, `flag` −1 [read in `SI_makeVertices`' listing].

- `SI_nurbs2Triangle` does nothing for a null surface or one whose U or V has no points
  (+0x04, +0x28 not above 0) or no knots (+0x0c, +0x30 null). A `flag` above 0 is put in
  both display steps (+0x70, +0x74) of `srf`, and of `shape` when that is another record,
  and put back afterwards (both records get `srf`'s saved values). A surface with trims
  (+0x60 not 0) goes to **`TriangulateTrimmedNurbsSrf` (0x084fd14c; not ported)** with
  `needTrianglesWithIndex` (0x086d9fc0) and `&trianglesWithIndex` (0x086d9fc4); any other to
  `TriangulateNonTrimmedNurbsSrf`. The two counts come back in locals and are then copied.
- **`TriangulateNonTrimmedNurbsSrf(shape, srf, materials, elem_id, &n_verts, &n_tris,
  &verts, &tris)`**:
  1. `SI_SampleKnotVector` of U with the U display step (+0x70) as the samples a span, of V
     with +0x74 (section 9). The texture scale of each direction is `1 / (last − first)` of
     its sample array, rounded to double, **replaced by 1 when its size is below 1e-4**
     (0x086617b8; the test is made on the sign first and then on 1e-4 > s or 1e-4 > −s, so
     a NaN stays).
  2. `repeatKnotsInSampling` on both arrays (below); the new counts go to `uNoPoints`
     (0x088221c0) and `vNoPoints` (0x088221c4), their product to `*n_verts`; the vertices
     are `SI_calloc(n_verts, 0x4c)`.
  3. For each U sample u (index i): when u is above the previous sample (the first is
     compared with the first sample less 1, kept in the register), the curve in V at u,
     `SI_nurbsSrfExtractCurve(srf, u, 0, 0)` (section 5); then the curve of its derivative
     with respect to U, `SI_nurbsSrfExtractCurve(srf, u', 0, 1)`, with u' = u − 0.001
     (0x086617c0) when the next sample equals u, else u. A repeated sample thus has the
     same position curve and the derivative of the piece on its left.
  4. For each V sample v (index j; the first compared with the first sample less 1,
     **rounded to double**): the homogeneous point P = `SI_nurbsCrvValue4D(curve in V, v)`
     when v is above the previous sample **or the previous vertex needed a search for its
     derivative across** (a mark the search sets and every vertex clears); else the last P
     stands (the frame's record: before the first P it is stack content, which no sample
     array of a real surface reaches). `inv = 1 / P.w` in the register; the position p is
     P × inv a coordinate, rounded to double. The vertex's x (+0x08 and +0x20) is **rounded
     straight from the register to float**, y and z (+0x0c, +0x24; +0x10, +0x28) from the
     rounded doubles: a double rounding for two coordinates of three.
  5. **Across**: D = `SI_nurbsCrvValue4D(derivative curve, v')`, v' = v − 0.001 before a
     repeated V sample; across = `inv × (D − D.w × p)` a coordinate in the register (the
     quotient rule). Its squared length is taken from the **unrounded** values; below 1e-6
     (0x086617c8) the search: steps ±0.01 (0x086617d0, 0x086617d8: −0.01 when the
     parameter is within 1e-4 of the range end, strictly, or when the next sample equals
     it, else +0.01; the next sample is read only when the first test fails, and at the
     last sample of an open direction it lies past the array); the test is then made again
     on the rounded doubles; while it holds and up to the 99th try, tries 1 and 2 of every
     three evaluate the derivative curve at v + the accumulated V step with the point and
     `inv` as they stand and add a V step; every third frees the curves the search
     extracted (never the row's two), extracts both anew at u + the accumulated U step,
     takes P, `inv` and p anew from the new position curve at v + the V steps (P stays so
     for the next vertex, which the mark makes take its own), and adds a U step. This is
     section 6's search, written out a second time in this file with the curve of the
     whole row instead of one span.
  6. **Along**: L = `SI_nurbsCrvValue(curve in V, v', 1)`; below 1e-6 (from the doubles) the
     same search on the position curve alone (every third try extracts it anew at u + the
     U steps).
  7. The normal: across × along from the doubles, each coordinate in the register rounded
     to float (+0x14, +0x18, +0x1c); its length the square root of the three floats'
     squares, rounded to float; **divided only when the length is above 1e-4** (compared as
     doubles: the decompiler's `(float)1e-4` is wrong), else left as it is. The texture
     pair: `1 − (u − first u) × scale_u` and `(v − first v) × scale_v` (+0x00, +0x04),
     rounded from the register; +0x40 is 0 (no further pairs), +0x44 is 0xffb2b2b2.
  8. After a row the curve in V is freed unless the next sample equals u (the next row,
     which extracts nothing, uses it); the derivative curve always.
  9. The triangles: **`n_tris = 2 × (V samples − 1) × (U samples − 1)` with the counts
     before the repetition**, `SI_calloc(n_tris, 0x60)`; each record linked to the next
     (+0x5c; the last 0), +0x58 the element number (16 bits), +0x4c the first material
     (`*materials`, or 0 for a null array). Corners, cell by cell, the index of each
     direction moved on by one where a sample equals the next (a crease is stepped over):
     (a, b, c) and (a, c, d) with a = (i, j), b = (i + 1, j), c = (i + 1, j + 1), d = (i, j +
     1), vertex (i, j) at `i × V samples + j` **in the counts after the repetition**.
  10. With a `shape` other than `srf`, `EvaluateNonTrimmedNurbsFromBaseGeometry`; then both
      sample arrays are freed (`free`, the C library's).
- **`repeatKnotsInSampling(dir, &count, &array)`**: the inner knots (open: from `order − 1`
  to `knots − order + 1`; closed: from 0 to `knots − 1`) are walked in runs of knots within
  1e-6 (0x086617e0, absolute difference in the register, strictly) of the run's first; the
  first index of every run at least `order − 1` long is noted in `SI_calloc(knots, 4)`. With
  a note, a new array `SI_calloc(count + notes, 8)` takes each sample once, and twice when
  it lies within 1e-6 of the next noted knot (whose turn then passes); past the last note
  the block's next word, which `SI_calloc` cleared, names knot 0. The old array is freed,
  the count grows. The notes are freed.
- **`EvaluateNonTrimmedNurbsFromBaseGeometry(shape, srf, verts, nu, nv, us, vs)`**: the
  sample arrays brought from `srf`'s ranges to `shape`'s, in place, each `((s − a0) / (a1 −
  a0)) × (b1 − b0) + b0` in the register (U, then V), **when the starts or the ends of the
  two ranges agree to 1e-6** (0x086617e8) as the source's absolute-value macro measures
  them: expanded around `a − b` without parentheses it gives `a − b` when that is not
  negative and **`−a − b`** otherwise [read: the negative case loads a, changes its sign,
  subtracts b]. Then each vertex's +0x20 … +0x28 from `shape`'s curve in V at u
  (extracted for each u above the previous, kept for an equal next one), its homogeneous
  point at v times `1 / w`, rounded straight to float.

What `SI_collect` does with the result is finding 0012, section 3b (the counts to the
statistics, `SI_textureVerts`, the hand rule).

### 13.2 The evaluators on the image (`image.rs`)

A ported function on the image takes the image and the original's addresses and makes the
original's allocation calls (finding 0024). The six routines above are written so: they
read the records and their arrays from the image, keep `left.272` (0x086d9c20) where the
binary has it, compute with the first form (sections 3 to 9, already compared bit for bit),
and allocate in the original's order, scratch included: for an order k above 4
`SI_nurbsCrvValue4D` and `SI_nurbsCrvValue` take `SI_calloc(k − 1, 8)` twice and
`SI_nurbsPointsAllocate(k)` and give them back points first; `SI_nurbsCrvValue` with a
derivative order above 3 also `SI_nurbsPointsAllocate(nd + 1)` and `SI_calloc(nd + 1, 0x18)`,
given back before; the whole-curve extraction allocates the curve, its points (into +0x2c
at once), for a reduced order k above 4 the two distance arrays, a triangle of points and
`SI_calloc((k − 1) × k / 2, 8)` of factors (given back factors, points, distances), then
fills +0x34, +0x24, the counts, `si_knotsAllocate` and the knots, the range and the closed
word. The allocators and `SI_nurbsCrvDispose` on the image are `si-hrc`'s ports, which this
crate calls. Where the first form refuses (a span search that does not end, a read outside
the arrays a record names), the second answers `Halt::Refused`; a trimmed surface answers
`Halt::Unported`.

### 13.3 Which sample scenes have NURBS surfaces

17 of the 343 scene descriptions (render-pictures' and `rensetup-check`'s index, column
"kinds"), 42 surface nodes in the 14 of them under `nt/db/data1` that were dumped
(`scene-dump`, built into this package's target directory from `crates/si-emuworld`
unchanged). In every one the node has one definition and **the definition is the current
surface** (`shape == srf`): `EvaluateNonTrimmedNurbsFromBaseGeometry` is not reached by the
sample scenes. **None of their surfaces has a trim.** Orders: 4 × 4 but for two (3 × 3 in
`Expressions-MOUTH`, 3 × 4 in `MODEL-glass`); display steps 3 but for 2 (`Expressions-MOUTH`,
`General-Skeleton`), 4 × 5 and 10 (`MATTER-depth_of_field`, `MODEL-SoftDrinksCan`).
Over the model files (`nurbs-check`), the conversion on an image: **95 of 97 surfaces
converted, the 2 with trims not** (the trimmed path), none halted; 90,462 vertices, 165,678
triangles, every corner a vertex of the block, every normal of length 1 to 1e-5 (none left
undivided); 3 surfaces have a crease (a repeated sample).

### 13.4 What is not ported on this path

`TriangulateTrimmedNurbsSrf` (0x084fd14c, 1,600 bytes) and the 15 routines of `trimmed.c`
it calls (finding 0012, section 18.5, has their names), the trimming package's line
routines below them. No sample scene renders a trimmed surface; two model files have one.

### 13.5 Oddities worth keeping

- The vertex's x and its y, z are rounded differently (13.1, step 4).
- The triangle count is made from the counts before the repetition, the corner indices from
  the counts after it: on a surface with a crease the grid has more vertices than the cells
  use, and the stepping over equal samples is what keeps the cells off the crease.
- The absolute-value macro of `EvaluateNonTrimmedNurbsFromBaseGeometry` (13.1), and its
  test: the remap is made when either end "agrees" (below 1e-6 as the macro measures). For
  a negative difference the macro gives `−a − b`, which is below 1e-6 whenever `a + b` is
  above −1e-6: with parameters that are not negative, **an end of `srf` below `shape`'s
  always counts as agreeing**. So the samples are mapped (with the formula's rounding, also
  where the ranges are the same) unless at both ends `srf`'s parameter exceeds `shape`'s by
  1e-6 or more, or the parameters are negative. Which of this the authors meant is a guess;
  the test cases of 13.6 shift and scale ranges both ways.
- The search for a vanished derivative reads the sample after the last of a direction
  (past the array) when the last sample is not within 1e-4 of the range end.
- `flag` above 0 restores `shape`'s steps from `srf`'s saved values, not from its own.

### 13.6 Checks

**Emulation** [measured], strict sessions, `re/tools/si-nurbs/gen_convert.py` →
`re/out/si-nurbs/vec13`, compared by `nurbs-emucheck --image` (module `image13`): a case is
loaded into a fresh image (the harness's addresses; its heap handing out blocks in the
harness's order), the port runs on it, and compared are every output buffer word for word
(a pointer into a heap block as the address the port's block of that number has), every
input buffer the original left alone, the vertex and triangle blocks, a new array or curve
read through the routine's results, the answer, and the list of blocks given back, in
order. Surfaces: all 42 of the dumped scenes (copied into the case: record, knots, points;
the record's other pointers null), and made-up ones (`emu_gen.py`'s, never hostile) for
orders 2 to 8, creases and poles.

| Routine | Cases | Identical | Different | Not compared |
|---|---|---|---|---|
| `SI_nurbs2Triangle` (with `TriangulateNonTrimmedNurbsSrf`, `repeatKnotsInSampling` and the evaluators inside), the 42 surfaces of the dumped scenes: flag −1 (as `SI_collect` calls it) with the surface as its own shape and with a null shape, flags 1 and 2, and another surface lending positions | 208 | 208 | 0 | 0 |
| … made-up surfaces (orders 2 to 4, 0 to 8 points more than the order, every knot style of section 11, display steps 1 to 3, flags −1 to 2) | 300 | 296 | 0 | 4 (S: the original faults: 3 writes, 1 read) |
| … made-up surfaces of at most 6 points a direction | 120 | 120 | 0 | 0 |
| `EvaluateNonTrimmedNurbsFromBaseGeometry` alone: the scene surfaces with themselves, with their parameters shifted or scaled, with another surface | 168 | 168 | 0 | 0 |
| `SI_SampleKnotVector` on the image: the 84 directions of the scenes and 400 made-up ones, 1 to 10 samples a span | 1,300 | 1,300 | 0 | 0 |
| `repeatKnotsInSampling`: the same directions with sample arrays on and near their knots | 684 | 683 | 0 | 1 (S: a read the original faults on) |
| `SI_nurbsSrfExtractCurve` (and through it `…U`, `…V`) on the image: the scene surfaces and made-up ones of orders 2 to 4, derivative orders 0 to 2 | 752 | 752 | 0 | 0 |
| `SI_nurbsCrvValue4D` on the image: the curve nodes of the scenes and made-up curves of orders 2 to 8 | 1,557 | 1,557 | 0 | 0 |
| `SI_nurbsCrvValue` on the image, derivative orders 0 to 5 | 1,559 | 1,557 | 0 | 2 (not authoritative) |
| `getPointInterval` (section 13.7) | 4,248 | 4,248 | 0 | 0 |
| **total** | **10,896** | **10,889** | **0** | **7** |

Every compared case was identical at its first complete run (one mistake of the check's own
was mended first: a buffer the original only writes had been looked for among the inputs).
Made-up curves whose closed direction is narrower than the span search's tolerance were left
out of the generator: they are hostile by section 11's definition (the search then reads in
front of the knots, which the first form refuses). Reached in the conversions
(`EMUCHECK_TRACE=1`): 11,824 searches for a vanished derivative across (7,188 of them in the
scene surfaces, at their poles) and 3,980 along (none in the scene surfaces); 239,341 tries
that extracted curves anew (none in the scene surfaces: there the search ends at the first
or second try); 203 of the made-up conversions have a crease (a repeated sample), 178 a
positive flag (and 86 of the scene ones). **`EMUCHECK_MUTATE=1`** (one bit of the first byte compared, or of the
answer): 10,889 different, the 7 not compared. **`EMUCHECK_MUTATE=calls`** (the last block
given back dropped from the port's list, or one added where it gave none back): 10,889
different. (For the routines that give nothing back the second switch shows only that the
list is compared.)

**Corpus** [measured]: section 13.3. **Pictures**: finding 0012, section 22.

### 13.7 `getPointInterval` (0x084ea3e4, `nrb_misc.c`, static)

`getPointInterval(dir, i, &first, &last, &count, &start)`: the knot spans control point i of
a direction bears on; `markDirtyCcrvs` (0x084ea5ec, not ported) calls it for both directions
of a surface whose point moved, to know which display lines to mark. `*start` = `order − 2`
(open) or 0 (closed). Open: `*first` = `order − 2` and `*last` = `order − 1 + i` for i below
`order − 2`, else `*first` = i and `*last` = `i + order − 1`; `*last` lowered to `knots −
order` when above it; `*count` = `*last − *first + 1`. Closed: `*count` = order; `*first` = i,
or `knots − 2` for a negative i; `*last` = `(*first − 1 + order) % (knots − 1)`, C's remainder
(negative for a negative dividend; a closed direction of one knot divides by zero). Every
value is written through its pointer and read back, in that order. Ported in `misc.rs`, on
the image; checked on 4,248 cases (13.6), the directions of the scene surfaces and made-up
ones, every point index and a few outside.

### 13.8 How to run

```bash
export CARGO_TARGET_DIR=$PWD/target/si-nurbs
(ulimit -v 8000000; cargo build --release -p si-nurbs -p si-emuworld --bin nurbs-emucheck --bin nurbs-check --bin scene-dump; cargo test --release -p si-nurbs)
# the scenes with NURBS surfaces, dumped (one directory each; 14 of them, nt/db/data1)
for s in Model-Methane_Molecule.1-0 MODEL-JOJOtheFISH.1-0 …; do (ulimit -v 2000000; timeout 600 target/si-nurbs/release/scene-dump re/bin/si3d40-ia/installed re/out/si-nurbs/scenes/$s --only "/$s.dsc"); done
(ulimit -v 6000000; timeout 7000 uv run --with unicorn python3 -I re/tools/si-nurbs/gen_convert.py re/out/si-nurbs/vec13)   # all groups; the made-up conversions take about half an hour
(ulimit -v 2000000; EMUCHECK_TRACE=1 target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec13 | tail -2)      # 10,896 / 10,889 / 0 / 7
(ulimit -v 2000000; EMUCHECK_MUTATE=1 target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec13 | grep TOTAL)
(ulimit -v 2000000; EMUCHECK_MUTATE=calls target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec13 | grep TOTAL)
(ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck re/out/si-nurbs/vec | tail -2)   # the first pass's: 72,531 / 70,923 / 0 / 1,608
(ulimit -v 2000000; timeout 900 target/si-nurbs/release/nurbs-check re/bin/si3d40-ia/installed | tail -5)   # the conversion's corpus count
```

The first pass's checks after this pass: `nurbs-emucheck re/out/si-nurbs/vec` **72,531 cases,
70,923 identical, 0 different, 1,608 not compared**; with `EMUCHECK_MUTATE=1` 71,190
different, 619 identical, 722 not compared (as before); `nurbs-check`: every line of section
11 as before. 16 unit tests (one new: a flat square of order 2 becomes two triangles on an
image, every block but the two results given back).

### 13.9 Where the work stands (second pass)

`nrb_convert.c`: 4 of 19 ported; `nrb_misc.c`: 11 of 22 (`getPointInterval` new). Next on the renderer's path: the trimmed triangulation
(16 routines, the trimming package below). Next of package 14 in this note's order (section
12): splitting (`SI_nurbsCrvSplit`, `SI_nurbsSrfSplit`), the basis matrices, the surface's
reversal, reparameterisation, `nrb_geometry.c`, the rest of `nrb_misc.c`; and of
`nrb_convert.c` the conversions the modeller and the scene's loader call
(`SI_nurbsConvertToMesh` 0x084df320, `SI_nurbsCrvConvertToLinear` 0x084e0eb4, which
`si-model`'s faces of NURBS curves wait for, `SI_spline2NurbsCrvKeepCurve`,
`SI_patchConvertToNurbsSrf`, …). Not started in this pass.

**Not determined**: what P holds before the first vertex in a case where the first V sample
is not above the first sample less 1 (only an array with a NaN or a value of 2^53 or more
gets there); what `TriangulateTrimmedNurbsSrf` does.

### 13.10 `SI_nurbsCrvMovePoint` on the image (0x084e9eec, `nrb_misc.c`; 2026-10-14)

By agents run one at a time for the lead. The render's update moves a NURBS curve's points
when it blends a face of NURBS curves (si-anim's interpolators, finding 0030; routed in
finding 0023 §28). `image::crv_move_point` is the image form of section 10's routine, with
the same arithmetic as the first form (`misc::crv_move_point`):

- the point's record is at the curve's points (+0x2c) plus i × 0x24, with no test of i (the
  address wraps as the original's 32-bit arithmetic does);
- the weight (+0x18) is read, then bits 0x60 of the flag word's highest byte (+0x23) are set;
- x, y and z are stored, each times the weight in the register and rounded once to
  `double`;
- last, bits 0x60 of the curve's +0x3b are set. It returns 1.

**Checks** (`nurbs-emucheck --image`, `image13.rs`; an answer in `eax` is now compared):

- `gen_convert.py`'s new group `mov` writes `re/out/si-nurbs/vec13/SI_nurbsCrvMovePoint.vec`:
  611 cases, 11 on the NURBS curves of the dumped scenes and 600 on made-up curves. The place
  is made up: zeros of either sign, integers, huge and tiny values, infinities, a quiet NaN.
  In a fifth of the cases the point's weight is too.
- The first pass's 1,000 cases (`re/out/si-nurbs/vec/SI_nurbsCrvMovePoint.vec`) go through the
  image form unchanged.

| Vectors | Cases | Identical | Different | `EMUCHECK_MUTATE=1` | `=calls` |
|---|---:|---:|---:|---:|---:|
| `vec13` (image form) | 611 | **611** | 0 | 611 different | 611 different |
| the first pass's (image form) | 1,000 | **1,000** | 0 | 1,000 different | 1,000 different |

All of `vec13`, now 11 files: 11,507 cases, 11,500 identical, 0 different, the same 7 not
compared; every other row as in 13.6. The first form on `re/out/si-nurbs/vec`: 72,531 /
70,923 / 0 / 1,608, as before. One unit test (new): a point moved on the image is the
point the first form moves, bit for bit, with the point's and the curve's marks.

Verified on a clean worktree of 34bdd92 with only these files
(`re/out/lead/shapes/verify.log`).

```bash
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-nurbs/gen_convert.py re/out/si-nurbs/vec13 mov)
(ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec13 | tail -2)   # 11,507 / 11,500 / 0 / 7
```

### 13.11 A curve's length and the parameter at a distance (`nrb_geometry.c`; 2026-10-15)

By agents run one at a time for the lead. The render's update asks for a NURBS path's length
and for the parameter at a distance along it (the path code, and the tangency constraint
through si-kine; routed in finding 0023 §28). Four of the file's eight routines are ported,
in both forms: the first on `own::Curve` (`geometry.rs`, new) and the second on the memory
image (`image.rs`). The arithmetic is shared; the forms differ only in where a table entry
is read and written.

**The tables.** The count is at +0x50, the parameters at +0x54 and the cumulated lengths at
+0x58, `count` doubles each. `own::Curve` keeps them beside the record. The two readers build
them when the count is 0, as the original does, with `SI_nurbsCurveUpdateLength(curve, 100, 1)`.

- **`SI_nurbsCurveUpdateLength`** (0x08501ed8, 405 bytes; `image::nurbs_curve_update_length`,
  `geometry::curve_update_length`). Nothing for a null curve. With flag 1, bits 0x60 of +0x3b
  are set and the touch below is called. The parameters come from
  `SI_SampleKnotVector(curve, samples)`. The old tables are freed and the new parameters and
  their count stored. The cumulated lengths come from `SI_calloc(count, 8)`: entry 0 is 0, and
  entry i is entry i − 1 plus the chord between the points `SI_nurbsCrvValue(curve, t, 0)` at
  parameters i − 1 and i. Each difference is rounded to a double, the square root and the sum
  are taken in the register, and the entry is stored as it is computed. A curve of one sample
  evaluates its one parameter and does not use the point.
- **`SI_nurbsCrvAnimUpdate`** (0x08501d90, 328 bytes; `image::nurbs_crv_anim_update`), the
  touch. The curve is not tested for null.
  1. A count that is not 0 is cleared and the cumulated lengths freed; the parameters stay.
  2. Bit 0x10 of +0x38 is set.
  3. It walks the current object list (`SI_grafLists[SI_procData[1]]`, along +0x1c). For a
     node of kind 2 or 3: if its model is a NURBS curve (kind 9) whose definition is this
     curve, bit 0x10 of the update bytes +0x0a and +0x3fa is set; each spline deformation
     record (+0x60, +0x64) whose path model's definition is this curve has its table (+8)
     freed.
  4. A keyframed curve (+0x4c is 1) calls the animation package:
     `SI_findNURBSPathFcurveOwner`, `SI_updateFcurveNURBSPathKeys`, `SI_checkNrbCrvToFcurve`,
     `SI_refreshFcurve(0)`. These go through `si_mem::Outside`.
  The first form has only the record's part.
- **`SI_nurbsCurveLength`** (0x08502070, 54 bytes). 0 for a null curve, else the last
  cumulated length, entry `count − 1` (the address wraps as the original's 32-bit arithmetic
  does), as loaded into the unit.
- **`SI_nurbsDistanceToFactor`** (0x08502f4c, 277 bytes). The result is unrounded, as the
  original leaves it in st0.
  1. |d| ≤ 0.0001 (the double at 0x08661e40): the start of the range (+0x10). This is
     decided before the length is asked for.
  2. Within 0.0001 of the length: the end of the range (+0x18).
  3. A closed curve (+0x20 not 0): the length is taken off while d is above it, then added
     while d is below 0, in the register.
  4. Still above the length: the end. Below 0: the start.
  5. Otherwise, the first entry i of the cumulated lengths above d, then the straight line
     between entries i − 1 and i of both tables. For i = 0 the original reads the doubles in
     front of both tables.

The ports refuse (`Halt::Refused`, `geometry::Refusal`) four cases:

- a curve without knots, where the original reads through a null array with a count it never
  set;
- a sampling or evaluation the first form refuses;
- a closed curve whose loops never bring the distance inside the length (the port gives up
  after 2^20 turns; a length of 0 loops for ever);
- no entry above the distance (a NaN distance, say), where the original indexes the tables
  with whatever `esi` held.

**Checks** (`nurbs-emucheck --image`, `image13.rs`). An answer in st0 is compared in all 80
bits, the tables after the call through the curve (`t.lp`, `t.ld`), and the calls into the
animation package by name and argument words.

- `gen_convert.py`'s group `len` writes `re/out/si-nurbs/vec14`. It covers made-up curves and
  the scene curves, with made-up tables: chord lengths, repeated entries, a first entry above
  0, entries out of order, a quiet NaN, counts below the table's size, and a double in front
  of each table where the original reads there. Distances: near 0 and near the length, on
  entries, inside, beyond, several lengths away for the closed curves' loops, ±0, huge and
  tiny values, infinities.
- Group `upd` writes `vec15`. It covers made-up curves (some with old tables) and the scene
  curves; 1 to 100 samples a unit; flag 1, 0 or another value; now and then a keyframed curve
  (the four calls stubbed and recorded); and object lists that are empty or hold up to five
  nodes, whose models and spline deformation records point at this curve or another.

| Vectors | Routine | Cases | Identical | Different | `EMUCHECK_MUTATE=1` | `=calls` |
|---|---|---:|---:|---:|---:|---:|
| `vec14` | `SI_nurbsCurveLength` | 408 | **408** | 0 | 408 different | 408 different |
| `vec14` | `SI_nurbsDistanceToFactor` | 2,000 | **1,999** | 0 | 1,999 different | 1,999 different |
| `vec15` | `SI_nurbsCurveUpdateLength` with the touch | 1,200 | **1,200** | 0 | 1,200 different | 1,200 different |

The one `SI_nurbsDistanceToFactor` case not compared is one the original faults on (a read
of unmapped memory). All of `vec13` (11,507 / 11,500 / 0 / 7) and the first form's vectors
(72,531 / 70,923 / 0 / 1,608) print the same bytes as before. Two unit tests (new) check
that the first form and the image form give the same bits:

- the length and the parameter at a distance, on made-up tables (open and closed, the
  refusals);
- the tables `SI_nurbsCurveLength` builds for a straight line and a cubic, with the touch's
  marks and a keyframed curve's four calls.

Verified on a clean worktree of 924c56f with only these files
(`re/out/lead/nurbslen/verify.log`): no warnings, 1,232 tests pass.

`nrb_geometry.c` now has 4 of 8 ported. Left: `SI_nurbsUpdateFrenet`, `nurbsFrenetCoord`,
`SI_nurbsDistanceToValueFrenet`, `SI_nurbsFactorToDistance`.

```bash
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-nurbs/gen_convert.py re/out/si-nurbs/vec14 len)
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-nurbs/gen_convert.py re/out/si-nurbs/vec15 upd)
(ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec14 | tail -n 3)   # 408/408; 2,000/1,999/0/1
(ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec15 | tail -n 2)   # 1,200/1,200
```

### 13.12 The copies, disposals and point moves the update calls, on the image (2026-10-15)

By agents run one at a time for the lead. The envelope (finding 0029 §17.2), the shapes'
surface blends (finding 0030) and the key edit copy, dispose of and move the points of NURBS
curves and surfaces. Until now `si-nurbs` had these only on values, so the hosts left NURBS
skins as they were (`si_scene::envside::unsupported_skin`, `unsupported_rebuild`). Five
routines now have image forms in `image.rs`, with the helpers they call:

- **`SI_nurbsCrvCopy`** (0x084dbbf4, `crv_copy`): a new curve (`SI_nurbsCrvAllocate`; 0 when
  that fails) with the source's order, counts, range, flags and steps. The knots and points
  go into new blocks (the points when the **source's** count is above 0), +0x48 is cleared,
  and the length tables (+0x50 to +0x58, section 13.11) are copied entry by entry. Doubles go
  through the unit (`fld`, `fstp`: a signalling NaN comes out quiet).
- **`SI_nurbsSrfCopy`** (0x084dc1a8, `srf_copy`): the same for a surface's two directions,
  points, capping words and approximation floats. The trims and projected curves are copied
  by `SI_nurbsSrfTrmCpy` (0x084dbf08) and `SI_nurbsSrfProjCpy` (0x084dc058), each composite
  with its curves (`crv_copy`). The trims' copy first disposes of the new surface's display
  lines.
- **`SI_nurbsSrfDispose`** (0x084dba5c, `srf_dispose`): knots and points freed, then the
  trims (`SI_nurbsSrfTrmDispose`, 0x084db944) and the projected curves
  (`SI_nurbsSrfProjDispose`, 0x084db9d0). Each first disposes of the display lines with the
  bounding-box tree hidden around the call, and each composite goes through
  `SI_nurbsCompDispose` (0x084dbb14). Last, the tree is destroyed, the record freed and `*pp`
  cleared.
- **`SI_nurbsSrfMovePoint`** (0x084e9f2c, `nrb_misc.c`, `srf_move_point`; new): point i of
  +0x54 (not tested; the address wraps) gets the weight through the unit and bits 0x60 of its
  flag byte. x, y and z are each multiplied by the weight in the register and rounded once.
  Then bits 0x60 of +0x5b are set; an untrimmed surface (+0x60 is 0) calls
  `markDirtyCcrvs(srf, i)`; `SI_nurbsCapping` runs; the tree is destroyed. It returns 1.
- **`SI_nurbsSrfTransferPointsKnots`** (0x084ea07c, `srf_transfer_points_knots`): the
  destination's display lines disposed of, then `SI_nurbsSrfTransferPoints` (0x084ea0c4:
  bits 0x60 of +0x5b, the source's points over the destination's), then
  `SI_nurbsTransferDirKnots` (0x084ea108) for U and V.
- Ported for them (new to the ledger): `SI_nurbsSrfDisposeLines` (0x084e5468, `nrb_draw.c`:
  each display line's block, the array, the count, then `SI_genTreeDestroy`) and
  `SI_nurbsCapping` (0x084f00a0, `nrb_point.c` by the symbol table; the code's comments say
  `nurbs.c`): for each capping word set on a direction that is not closed,
  `SI_nurbsSrfForceCapping(srf, direction, 0)`.

Three calls leave these files and go through `si_mem::Outside`: `SI_genTreeDestroy`
(0x08515848, the intersection package's, always with `SI_bezPatchDestroy`), `markDirtyCcrvs`
(0x084ea5ec, static in `nrb_misc.c`) and `SI_nurbsSrfForceCapping` (0x084f0134). None is
ported. `si_scene::envside::NurbsSide` hands them to the host as calls of `si-model`'s
outside world, and the host answers them as it answers any routine it lacks (the update's
hosts: 0, counted).

**Routed** (no function of the original):

- `si_scene::envside::envelope_call` answers the five, with `SI_nurbsCrvMovePoint` and
  si-hrc's `SI_nurbsCrvDispose`, for the load's `SI_envelopRebuild` and the update's
  `SI_envelopCompUpdate`. The guards `unsupported_skin` and `unsupported_rebuild` are removed;
  nothing they refused is unsupported any more (finding 0018 §21, finding 0029 §17.2).
- `si-rensetup`'s update and `motion-evalcheck` route `SI_nurbsSrfMovePoint`, the three
  NURBS-surface interpolators of `shape.c` (`nurbs_srf_interpolate`, `…_cardinal_…`,
  `…_weight_…`) and `SI_nurbsSrfTransferPointsKnots` (the key edit's).
- The quick stretch's copies are still not routed: a NURBS node is left as it is there
  (`qstretch_copy_missing`).

**Checks** (`nurbs-emucheck --image`, `image13.rs`). A new curve or surface is read through
the answer (`n.…`) down to its composites' curves, and the three stubbed calls are compared
by name and argument words. `gen_convert.py`'s group `cpy` writes `re/out/si-nurbs/vec16`.
The routines run whole in the emulator but for the three stubbed calls. Inputs:

- the dumped scenes' surfaces (no trims) and curves;
- made-up surfaces with trims, projected curves (some with length tables), trim lines,
  display lines with or without blocks, and bounding-box tree words;
- point moves on every point of a small surface and on the first, the last and others of a
  larger one, with made-up places and weights (zeros of either sign, integers, huge and tiny
  values, infinities, a quiet NaN).

| Routine | Cases | Identical | Different | `EMUCHECK_MUTATE=1` | `=calls` |
|---|---:|---:|---:|---:|---:|
| `SI_nurbsCrvCopy` | 806 (6 on the scenes' curves) | **806** | 0 | 806 different | 806 different |
| `SI_nurbsSrfCopy` | 684 (84 on the scenes' surfaces) | **684** | 0 | 684 different | 684 different |
| `SI_nurbsSrfDispose` | 642 (42) | **642** | 0 | 642 different | 642 different |
| `SI_nurbsSrfMovePoint` | 1,283 (483) | **1,283** | 0 | 1,283 different | 1,283 different |
| `SI_nurbsSrfTransferPointsKnots` | 642 (42) | **642** | 0 | 642 different | 642 different |
| total | 4,057 | **4,057** | 0 | 4,057 | 4,057 |

Two unit tests (new):

- a 3 × 4 plane copied, a point of the copy moved, the source's points handed back and the
  copy disposed of, all on the image: blocks, marks, the outside calls and the order of the
  frees;
- a curve copied on the image is the curve copied on values, bit for bit.

**In the pictures** (finding 0023 §30): Expressions-MOUTH and Actor-Walk, whose skins are
NURBS surfaces, are now identical on every frame (115 and 50, was 0). First frames: 265 of
296 (was 263). Every frame: 23,753 of 25,507 (was 23,588); none worse. `motion-evalcheck`
93,244 of 93,275, unchanged. **Owed:** `SI_nurbsSrfForceCapping` is answered 0 (2,880 calls
over the evalScene corpus, Actor-Walk's capped skin), yet the frames match.

Verified on a clean worktree of 868501f with only these files
(`re/out/lead/nurbsguard/verify.log`): no warnings, 1,273 tests pass.

```bash
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-nurbs/gen_convert.py re/out/si-nurbs/vec16 cpy)
(ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec16 | tail -n 6)   # 4,057 / 4,057
(ulimit -v 2000000; EMUCHECK_MUTATE=1 target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec16 | tail -n 1)   # 4,057 different; =calls the same
```

### 13.13 A curve made linear and a curve reversed, on the image, for faces of NURBS curves (2026-10-15)

By agents run one at a time for the lead. `SI_face2triangle` makes a node of kind 2 into
triangles through `SI_faceConvertToMesh` (si-model's `face_convert_to_mesh`, on the image).
For a face of NURBS curves (the word +0x84 is 0) that routine makes each contour of order 2
with `SI_nurbsCrvConvertToLinear` and winds it with `SI_faceOrdinatePoints`, which reverses a
contour with `SI_nurbsCrvInverse`. Neither was on the image, so such a face had no triangles
(finding 0012 §18.5). Seven routines now have image forms in `image.rs`:

| Routine | Address | Bytes | `image.rs` |
|---|---|---:|---|
| `SI_nurbsCrvConvertToLinear` (`nrb_convert.c`) | 0x084e0eb4 | 371 | `crv_convert_to_linear` |
| `SI_knotsCrvReparametrization` (`nrb_b.c`) | 0x084de590 | 860 | `knots_crv_reparametrization` |
| `si_nurbsCrvFillDistancePoints` | 0x084ddfdc | 412 | `crv_fill_distance_points` |
| `knotsCrvReparamChordCentri` | 0x084de2e0 | 334 | `knots_crv_reparam_chord_centri` |
| `knotsCrvReparamChordCentriQuad` | 0x084de430 | 352 | `knots_crv_reparam_chord_centri_quad` |
| `SI_nurbsCrvInverse` (first form `edit::crv_inverse`) | 0x084dca24 | 212 | `crv_inverse` |
| `si_nurbsDirInverse` (first form `knots::dir_inverse`) | 0x084dc954 | 205 | `dir_inverse` |

- **`SI_nurbsCrvConvertToLinear(curve)`**: null for a null curve, one without points or
  without knots; a copy (`crv_copy`, §13.12) for a curve of order 2 already. Otherwise the
  curve is sampled (`SI_SampleKnotVector` with its step, +0x34) and a new curve of order 2
  made through the samples (`crv_value`: x, y, z; the weight left at 1, the flag 0; a closed
  curve one sample fewer), step 1; the samples freed; its knots made by
  `SI_knotsCrvReparametrization` (uniform, its +0x24 being 0).
- **`SI_knotsCrvReparametrization(curve)`** makes the knots anew from the parametrisation
  (+0x24): 0 uniform (z = order − (closed × (order − 2) + 1) zeros, then the integers, an open
  curve's last order − 2 repeating the one before); 2 keeps knots it has, else makes them
  from the chords, then moves them by knot 0 when |knot 0| is above 1e-6 and sets a knot
  less than 1e-6 above the one before to it; any other frees the knots and makes them from
  the chords. Then the range from the knots.
- **The chords** (`si_nurbsCrvFillDistancePoints`): each point divided by its weight and
  rounded to `float`, the chord (dx² + dy²) + dz² in the register and its root (twice for
  the centripetal parametrisation, 4), at least the double nearest 0.01; the total summed as
  a `double`; a closed curve's closing chord into the last place, not summed and turned to
  the front. The count and the point pointer are read again at every pair.
  `knotsCrvReparamChordCentri` sums them into knots (an open curve's first order − 2 chords
  summed in the register first); `…Quad` (order 3) takes the mean of two running sums.
- **`SI_nurbsCrvInverse(curve)`** exchanges the points end for end (nine words each through
  a copy on the stack), writes where each went into `SI_gblIndex`'s table when it is set,
  reverses the knots (`si_nurbsDirInverse`: the intervals, then each knot the one before
  plus an interval from the other end, from 2 for a closed direction not of order 3) and,
  for a keyframed curve (+0x4c), calls `SI_findNURBSPathFcurveOwner` and
  `SI_invertFcurvePathKeys` through `si_mem::Outside`. It answers 1.
- **Refused**, never met by the inputs below: the closing chord of a closed curve of one
  point (it reads floats the original never wrote), and a failed allocation of the new curve.

**Routed** (no function of the original): si-render's `adapter::FaceSide` answers
`face_convert_to_mesh`'s calls with these two, si-hrc's `SI_nurbsCrvInit` and
`SI_nurbsCrvDispose`, and si-spline's three; the reversal's two calls into the animation
package and the converter's `SI_showActivity` / `SI_stopActivity` (the busy cursor) answer
0, counted. Finding 0012 §24 has the binding; finding 0023 §33 the pictures.

**Checks** (`nurbs-emucheck --image`, `image13.rs`; a curve changed in place read through
the case's buffer as `o.…`). `gen_convert.py`'s group `lin` writes `re/out/si-nurbs/vec17`,
the group `inv` `re/out/si-nurbs/vec18`. Both run whole in the emulator but for the
reversal's two stubbed calls. Inputs: the dumped scenes' curves (three or four times) and
made-up ones (`emu_gen.py`'s, never hostile) of orders 2 to 8, open and closed; for the
reparametrization every parametrisation 0 to 4, with and without knots; for the conversion
steps 1 to 25; for the reversal `SI_gblIndex` null or a table, the keyframed word now and
then. No signalling NaN anywhere (finding 0011).

| Routine | Cases | Identical | Different | `EMUCHECK_MUTATE=1` | `=calls` |
|---|---:|---:|---:|---:|---:|
| `SI_knotsCrvReparametrization` | 2,508 | **2,508** | 0 | 2,508 different | 2,508 different |
| `SI_nurbsCrvConvertToLinear` | 1,506 | **1,506** | 0 | 1,506 different | 1,506 different |
| `SI_nurbsCrvInverse` | 2,506 | **2,506** | 0 | 2,506 different | 2,506 different |
| total | 6,520 | **6,520** | 0 | 6,520 | 6,520 |

The three helpers are checked inside their callers. The older image vectors (`vec13` to
`vec16`) give the same totals as before. Four unit tests (new): uniform knots made for a
curve without knots; chord knots following the distances, the shortest chord kept; a curve
made linear going through its samples; a curve reversed on the image equal to the curve
reversed on values, bit for bit.

**In the pictures** (finding 0023 §33): `MODEL-flower`, which has faces of NURBS curves, is
now identical (its one frame; 19,638 of 250,000 pixels differed). First frames:
274 of 296 (was 273). Every frame: 24,992 of 25,507 (was 24,991); none worse.
`motion-evalcheck` 93,252 of 93,275, unchanged.

Verified on a clean worktree of 88e7aa5 with only these files
(`re/out/lead/nfaces/verify.log`): no warnings, 1,298 tests pass.

```bash
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-nurbs/gen_convert.py re/out/si-nurbs/vec17 lin)
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-nurbs/gen_convert.py re/out/si-nurbs/vec18 inv)
(ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec17 | tail -n 3)   # 4,014 / 4,014
(ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec18 | tail -n 2)   # 2,506 / 2,506
(ulimit -v 2000000; EMUCHECK_MUTATE=1 target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec18 | tail -n 1)   # 2,506 different; =calls the same
```

### 13.14 A curve multiplied by a matrix, on the image, for `SI_splineRevolutionToPatch` (2026-10-15)

By Sonnet agents run one at a time for the lead. `SI_splineRevolutionToPatch` (0x081977b0)
needs `SI_nurbsCrvApplyMatrix` on the image; its first form is `misc::crv_apply_matrix`
(same arithmetic). One routine, `image.rs`'s `crv_apply_matrix`:

| Routine | Address | `image.rs` |
|---|---|---|
| `SI_nurbsCrvApplyMatrix` (`nrb_misc.c`) | 0x084e9750 | `crv_apply_matrix` |

- **`SI_nurbsCrvApplyMatrix(curve, m, mask)`**: the point pointer (+0x2c) is read once, the
  count (+4) again after each point. A point is transformed when `mask` is 0 or its flag word
  (+0x20) has one of the mask's bits: x, y, z, w are read, then each coordinate q is stored
  as `((x m[0][q] + y m[1][q]) + z m[2][q]) + w m[3][q]` in the register (the floats widened
  exactly, one rounding to `double`) and bits 0x60 of the flag word's highest byte (+0x23)
  are set. Last, bits 0x60 of the curve's +0x3b are set, whether or not a point was
  transformed. Nothing is allocated and nothing is called.

**Check** (`nurbs-emucheck --image`, `image13.rs`; `gen_convert.py`'s group `app` writes
`re/out/si-nurbs/vec19`): 1,508 of 1,508 cases identical; `EMUCHECK_MUTATE=1` 1,508
different; `=calls` 1,508 different. The older image vectors (`vec13` to `vec18`) give the
same figures as before.

```bash
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-nurbs/gen_convert.py re/out/si-nurbs/vec19 app)
(ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec19 | tail -n 2)   # 1,508 / 1,508
```

### 13.15 A surface multiplied by a matrix, and the forced capping, on the image (2026-10-24)

Three routines of the NURBS surface added to `image.rs`, read from the listings, not from
names: `srf_apply_matrix` = [soft 0x084e9644 SI_nurbsSrfApplyMatrix] (`nrb_misc.c`),
`srf_move_point_no_cap` = [soft 0x084e9fac SI_nurbsSrfMovePointNoCap] (a callee of the
capping, no check of its own) and `srf_force_capping` = [soft 0x084f0134
SI_nurbsSrfForceCapping] (it is in `nrb_point.c`, not `nurbs.c`). The surface record, as
the listings address it: nu at +4, nv at +0x28, the count of points at +0x48, the points at
+0x54 (0x24 bytes each: w at +0x18, flag at +0x20), the trim count at +0x60, the dirty byte
at +0x5b. ApplyMatrix transforms the flagged points like the curve's routine (§13.14) and
sets the dirty bits; ForceCapping averages or moves the points of a capped end and calls
`markDirtyCcrvs` (the one call out, stubbed in the check and its calls compared).

Detail read from the listing: in the mean-mode case `1.0/count` stays in an 80-bit register
and is never stored as a float; storing it first made 363 cases differ, keeping it fixed all.

**Check** (`gen_convert.py` group `sam`, `re/out/si-nurbs/vec20`): ApplyMatrix 1,626 of 1,626
identical, ForceCapping 2,172 of 2,172, total 3,798 of 3,798; `EMUCHECK_MUTATE=1` and `=calls`
each make all 3,798 differ. Cases: the 42 surfaces of the dumped scenes Actor-Walk,
Env_Sunrise, Expressions-MOUTH, plus made-up surfaces.

**Effect in the render** (finding 0023 §35): MOTION-jojo_happy_fish_2 goes from 0 of 100
frames identical to 100 of 100; Actor-Walk stays 50 of 50. `cargo test -p si-nurbs`: 25 passed.

```bash
(ulimit -v 6000000; uv run --with unicorn python3 -I re/tools/si-nurbs/gen_convert.py re/out/si-nurbs/vec20 sam)
(ulimit -v 2000000; target/si-nurbs/release/nurbs-emucheck --image re/out/si-nurbs/vec20 | tail -n 2)   # 3,798 / 3,798
```

### 13.16 The point routines of `nrb_point.c`, from the end, on the image (2026-10-28)

Ported on the memory image, each compared with the emulated original (vectors from
`re/tools/si-nurbs/gen_convert.py`, groups `pnt`, `add`, `hit`, written to `re/out/si-nurbs/vec21/`):
[soft 0x084f0328 SI_crvDeleteTagPoints] (312 B), [soft 0x084ef06c SI_nurbsCrvDeletePoint] (279 B),
[soft 0x084ef184 nurbsDirDeleteKnot] (138 B) and [soft 0x084ef214 nurbsCrvDeleteKnotNonUniform]
(107 B) (both static, checked through their callers), [soft 0x084ef288 SI_nurbsSrfDeleteRow] (657 B),
[soft 0x084eeacc SI_nurbsCrvAddPoint] (305 B), [soft 0x084eed8c knotsWithoutMultiplicity] (109 B),
[soft 0x084eedfc SI_srfHitDirAndVal] (513 B) and [soft 0x084ef008 SI_nurbsSrfIndHit] (92 B).

Oddities, all [read] and [measured]:
- `SI_nurbsCrvDeletePoint` guards its index with "0 <= i or i < count", which holds for every
  index; the checks therefore keep indexes to 0..count-1 (outside, the original reads memory
  at random).
- `SI_nurbsSrfDeleteRow` and `SI_srfHitDirAndVal` divide by a count that can be zero; the original
  then stops on the divide error and such cases are counted "not compared".
- `SI_nurbsCrvAddPoint` rounds each weight-times-coordinate product once, from the register; the port
  does the same.
- The knot-delete helpers clamp the index range before moving knots; the clamp is in the port.
- Skipped on purpose: the dialog and event functions of the file (`SI_nurbsSrfAddRowCol`,
  `calculMinMaxAddRowDialog`, `ghostAddRowColDialog`, `refreshNurbsAddRowCol`, `refreshNurbsClose`,
  `ghostClose*Dialog`, `SI_dialogNurbs*Close`, `refreshNurbsCloseSrf`), which cannot be run in the
  emulator; and `SI_nurbsCrvIndexHit (0x084eec00)`, which calls four unported routines.

Check (full run of `vec21`): SI_crvDeleteTagPoints 1506/1506 identical, SI_nurbsCrvAddPoint 1506/1506,
SI_nurbsCrvDeletePoint 1455/1455, SI_nurbsSrfDeleteRow 1626/1626, SI_nurbsSrfIndHit 1238 identical of 1366
(128 not compared), SI_srfHitDirAndVal 1190 of 1365 (175 not compared), knotsWithoutMultiplicity 872/872:
9,393 identical, 0 different, 303 not compared. Mutation: `EMUCHECK_MUTATE=calls` 9,393 of 9,393 differ;
`EMUCHECK_MUTATE=1` 8,894 differ (499 `SI_nurbsSrfIndHit` cases unchanged by it). The checker now compares
recorded 8-byte arguments as two words and reads the result pointer of the original ("r").

Run: `(ulimit -v 6000000; uv run --with unicorn python3 -I re/tools/si-nurbs/gen_convert.py re/out/si-nurbs/vec21 pnt add hit)`, then
`(ulimit -v 2000000; target/slot-e13/release/nurbs-emucheck --image re/out/si-nurbs/vec21)`.

## 14. Trim lines (`trimLineOp.c`, package 15)

What a trim line is, read from the code (our own description). A line (`SI_trimLine`) is a record:
node count at +0, sign (turning direction) at +4, first node at +8, cursor node at +0xc, then the
four doubles of its box. A node (`SI_trimData`, 0x18 bytes) is a point of the surface's parameter
plane (u at +0, v at +8) with a mark at +0x10 and the link at +0x14. The mark pairs the two nodes
where two lines cross (the same non-zero value in both lines); the byte at +0x12 is or-ed with 2 when
`checkAtBorder` snaps a point onto the border, which lands in the mark's third byte. The range of the
surface is global: `uEps`, `vEps`, `uS`, `uE`, `vS`, `vE` (0x08822218 to 0x08822240), the tolerances
`epsU`, `epsV` (0x08822268, 0x08822270) and the flags `uClosed`, `vClosed`.

Ported and checked against the emulated original (vectors in `re/out/si-nurbs/vec30`, generator
`re/tools/si-nurbs/gen_trim.py`, checker `crates/si-nurbs/src/bin/nurbs-trimcheck`): `linerInterp
0x084f0460`, `clearTLnFlag 0x084f04d8`, `bboxesIntsect 0x084f04f8`, `atSameSide 0x084f0ac8`,
`nextNode 0x084f0b58`, `newPt 0x084f0b70`, `fillTheCoefs 0x084f128c`, `resetTLnFlag 0x084f139c`,
`getIntPt 0x084f159c`, `nxtIntsect 0x084f15e8`, `closeLine 0x084f16dc`, `reorderTrimStat 0x084f187c`,
`brdIntsct 0x084f1918`, `forceAtEdge 0x084f26b0`, `atEdge 0x084f2cc0`, `edgVal 0x084f4440`,
`checkAtBorder 0x084f56a0`, `perturbSngPtOnBorder 0x084f5874`, `resTrmCrv 0x084f6240`,
`delTrimLine 0x084f7158`; and, second round: `edgeDir 0x084f44b8`, `moveTrimLine 0x084f52e4`,
`joinTwoLnAtEnd 0x084f6280`, `midJoinTwoLines 0x084f632c`, `trimPeak 0x084f6720`,
`addPtAtEdge 0x084f5fb4`, `travelingIntCrvs 0x084f1998` (it allocates a line through `SI_calloc`: the
checker maps the heap block and compares its words, spec token `heap=ptr`), `addUEgSmp 0x084f2730`,
`addVEgSmp 0x084f2a00`, `trimHalfPeak 0x084f6438`, `trmCrvRevAEdg 0x084f5474`, `trmCrvAEdg 0x084f58f8`,
`handleUCsdMulIntsct 0x084f46e0`, `handleVCsdMulIntsct 0x084f4ce0`, `addCWBoundaryEdge 0x084f2de0`,
`addCCWBoundaryEdge 0x084f38c0`.

Run: `re/tools/lead/slot.sh emu -- bash -c 'uv run --with unicorn python3 -I re/tools/si-nurbs/gen_trim.py
re/out/si-nurbs/vec30 <group>'` with a group of `float list trav trav2 join edge smp rev cw`, then
`nurbs-trimcheck re/out/si-nurbs/vec30`. For the two border walks the groups use the real edge-sample
routines; for the callers of the walks (`moveTrimLine`, `trmCrvAEdg`, the `handle*` pair) the walks are
stubs whose two point records are read through (`deref_stub`, `derefs` in the checker: four words each).

Oddities found in the code:

- `atSameSide` is "not strictly on opposite sides": both signed distances above -1e-5, or both below 1e-5.
- `brdIntsct` answers a `short` that the caller sign-extends; `fillTheCoefs` tests the degenerate edge
  on the unrounded 80-bit `du`, not on the stored double.
- `nextNode` wraps to the **second** node (the last node repeats the first): a line of one node makes
  `nxtIntsect` and `resTrmCrv` read through null (the original stops with an invalid read; 115 + 20 cases
  are not compared).
- `resetTLnFlag` reads an unset local when no node of the first line has mark 0 (taken as null here;
  the vectors always give such a node), and `closeLine` loops for ever on a pair of lines without a
  node carrying the start mark (the emulator's budget ends those cases).
- `checkAtBorder` moves the coordinate along the border first and the other one second, and answers
  the corner the point reached through its `int *` argument.

Check numbers: 22,500 cases, 22,365 identical, 0 different, 135 not
compared. Per function: 1,800 each for the eight float routines except `edgVal` 1,500; `getIntPt` 700,
`nxtIntsect` 785 of 900, `resTrmCrv` 500, `clearTLnFlag` 500, `nextNode` 500, `fillTheCoefs` 900,
`delTrimLine` 400, `reorderTrimStat` 600, `resetTLnFlag` 700, `closeLine` 680 of 700,
`checkAtBorder` 1,000, `perturbSngPtOnBorder` 1,000.

Second-round oddities:

- `addUEgSmp` and `addVEgSmp` truncate toward zero when they find the sample index (the x87 control word
  is set to round-to-zero), and test with the "less or equal or unordered" branch form; the tables at
  0x086db8e4 (u) and 0x086db8e8 (v) are read at an index from absolute coordinate, so a negative
  coordinate reads below the table: the vectors keep range and points non-negative.
- `addPtAtEdge` puts its second insertion's cursor on the first line and then restores the second
  line's cursor (it does not set the second line's).
- `trimPeak` and `trimHalfPeak` read the bit 0 of the byte at +0x11 of a node.
- `handleUCsdMulIntsct`/`handleVCsdMulIntsct`: when the cut node is the line's tail, `ecx.next` is read
  again after the tail's link was overwritten; three locals are used unset (taken as null), and the
  original dereferences null when the minimum or maximum node is the tail (793 and 771 of 1,500 cases
  are not compared for that reason).
- `trmCrvRevAEdg`, `trmCrvAEdg` and the `handle*` pair work on the marks 0x100 and 0x200 and the bytes
  at +0x11 and +0x12; `trmCrvRevAEdg` reads null when no 0x100 node precedes the 0x200 node (171 cases).
- The border walks are one cycle each (clockwise 1, 4, 2, 3; counter-clockwise 1, 3, 2, 4, with 1 = u
  start, 2 = u end, 3 = v start, 4 = v end); the tests on the last edge are not all the same way round:
  counter-clockwise from edge 1 to 3 walks when `q.u` is below `uS + uEgInc`, and from 1 to 4 when `q.u`
  is above `uE - uEgInc`, the other way from every other start edge.

Check numbers, second round: `edgeDir` 1,800 of 1,800; `moveTrimLine` 900; `joinTwoLnAtEnd` 700;
`midJoinTwoLines` 700; `trimPeak` 890 of 900; `addPtAtEdge` 1,500; `travelingIntCrvs` 653 of 700;
`addUEgSmp` 900; `addVEgSmp` 900; `trimHalfPeak` 1,200; `trmCrvRevAEdg` 729 of 900; `trmCrvAEdg` 1,500;
`handleUCsdMulIntsct` 707 of 1,500; `handleVCsdMulIntsct` 729 of 1,500; `addCWBoundaryEdge` 1,495 of 1,500;
`addCCWBoundaryEdge` 1,486 of 1,500; 0 different everywhere, mutation `1` and `calls` each 0 identical.

### Third round: segment intersection, the trim curve and the automatic border

Ported on the image: `si_lnSegsIntsect (0x084f055c)`, `tCrvIntscted (0x084f0bb4)`,
`si_convertTrimLineToTrimCrv (0x084f6cf8)`, `SI_trimCrvToLineUpdate (0x084f71c0)` and
`si_handleAutoBorder (0x084f6e0c)`. Check numbers: `si_lnSegsIntsect` 3,000 of 3,000 (answers 0 to 8
all reached), `tCrvIntscted` 1,500 of 1,500 (counts 0 to 7), `si_convertTrimLineToTrimCrv` 300,
`SI_trimCrvToLineUpdate` 600, `si_handleAutoBorder` 1,434 of 1,500 (the other 66 agree on the defined part
of the new record, whose first half-word is never written, and are counted as not compared); 0 different;
mutation `1` and `calls`: every compared case differs. Run group `seg`, `conv` and `upd` of
`re/tools/si-nurbs/gen_trim.py`.

- The box of a trim line is (minU, maxV, maxU, minV) at +0x10, +0x18, +0x20, +0x28; an empty line starts
  at (+3.4028234663852886e38, -3.4028234663852886e38, -3.4028234663852886e38, +3.4028234663852886e38).
- `si_lnSegsIntsect` stores its two quotients (the intersection's u and v) in locals of the caller only
  when the answer needs them; `tCrvIntscted` keeps those two locals across the whole call, so a later
  segment pair that does not write them reads the last pair's values (0.0 before the first).
  A count of 1 in `tCrvIntscted` is added once whichever of the cases ends the pair.
- `si_convertTrimLineToTrimCrv` makes a linear (order 2) curve of `count - 1` points of the line, the
  closing flag at +0x34 set to 1 and knots 0, 1, 2, ...; the last node (the first again) is not entered. The
  boundary short of the composite is 0 for a positive sign and 1 otherwise.
- `si_handleAutoBorder` never creates a border for a flagged call: a contained new line with a flag sets bit 0
  of the byte at +0x5b and clears the short at +0x5c. The new border line is walked round the four edges
  with `addUEgSmp` and `addVEgSmp` one increment inside the range at each corner.
- The `realloc` of the composite table (a surface that already has composites and gets a border) is not
  checked: the emulator cannot start with a table that came from `malloc`.
- The array vectors of `tCrvIntscted` name the 14 coefficient and box arrays `q3` to `q16`.
- The check stand-ins that really build a curve and append nodes (`SI_trimCrvToLineUpdate`, `si_handleAutoBorder`)
  are marked in a case by an input buffer `realins`; the port's responder does the same work.

### Fourth round: the merger, the small trim routines, the edge detection

Ported on the image: `mergerIntesectedTCrv (0x084f1af0)`, `si_detectEdgeCross (0x084f68e8)` and, in
`trimutils.rs`, `si_delTrimLine (0x084f769c)`, `si_trimDataAllocate (0x084f7818)`, `si_trmDataIns (0x084f7910)`,
`revTrimFg (0x084f79b4)`, `SI_trmCvrRve (0x084f7a0c)`, `si_trmCrvBBox (0x084f7af0)`, `si_getEdgeSteps (0x084f8594)`
and `si_uniqueD (0x084f8444)`. Check numbers: `si_delTrimLine` 400 of 400, `si_trmDataIns` 600, `SI_trmCvrRve` 600,
`revTrimFg` 300, `si_trmCrvBBox` 1,500, `si_getEdgeSteps` 800, `si_uniqueD` 1,500 (all identical); the merger
and the detection as in the top block. Run groups `merge`, `hook`, `uniq`, `detect` (and `join`, `rev` for the
swapped callers) of `re/tools/si-nurbs/gen_trim.py`; `gen_fit.py` (groups `vec`, `cls`, `free`) for `curveFit.c`.

- The merger works through the lines in pairs by `tCrvIntscted`; a hit at the last segment of either line ends it
  with 1, a second hit of one line with 2. A line that meets another is joined (`traveling_int_crvs`) and both
  stale pointers are cleared. Then, for a surface with composites, each composite's line is tested the same
  way against the new lines (flag 1) and the old ones (flag 0). Oddities kept: after a hit against an old line
  the loop index is set to the table count (not the object's), the second loop still runs after the first loop
  stopped, and a joined line's freed pointer is read once more.
- `si_trmCrvBBox` starts the box at the second node, not the first; with no marked node the sign comes from a
  cross product of three consecutive points. `si_getEdgeSteps` keeps the largest gap of its table divided
  by the step count, on the register.
- `si_detectEdgeCross` derives the sample steps from the surface's edge tables (halves, 5% tolerances, 1e-4 of
  those), allocates `uSmp` only for a surface closed in v and `vSmp` for one closed in u, closes every line
  (`si_uniqueD`), and classifies each line by `trmCrvAEdg`: 0 reverse by edge, 2 reverse by the turning
  direction, 3 and 4 the multiple-crossing handlers, 5 and 6 end the routine with 3 and 4.
- `si_uniqueD` returns 0 (same place), 1 (different), 2 (same place across a u seam) or 3 (across a v seam),
  tolerance 1e-5, compared on the register.
- The pointer inside a freed block, and the first words of a freed block, are allocator artefacts and are not
  compared (`freeSegmentList` vectors leave the list head and cells as inputs only).
- `curveFit.c` point vectors: records of 0x24 bytes (x, y, z doubles, the weight 1.0 at +0x18). `classifySample`
  returns 1 when the tolerance is above `(p1 - p2) . (p3 - p1)`.
- Check tooling: a case input `ported` makes `nurbs-trimcheck` answer the four small trim hooks with the ports
  (the original ran the real ones, so no call is logged); a null pointer argument is the value 0 in the
  vector (`lp=ptr` with `[0]`).

### Fifth round (2026-10-28, nurbs5): sample-table padding, trimUtils wiring, curveFit second batch

- The seven earlier differences were the original reading below the edge-sample tables. The generator now puts
  64 zero doubles (`SMP_PAD`) below `us`/`vs`; `vec31` (regenerated): `moveTrimLine` 890 identical, 0 different,
  10 not compared; `trmCrvAEdg` 1,470/1,500, `handleVCsdMulIntsct` 730/1,500 (770 not compared), `handleUCsdMulIntsct` 738/1,500 with 1 different (a logged `si_trmDataIns` call whose first data word differs, original 9e543be8 against ffffffff; not yet diagnosed). Two `si_detectEdgeCross` cases still differ: the
  routine allocates its own sample arrays and reads one word (a malloc chunk-size field, 0x409 apart in the
  mantissa) or a block (a fault 0x40 below the first heap block) below them; the port reads zeros. These are
  undefined memory in the original, not a porting error, but the checker does not exclude them yet.
- `TrimUtilsOutside` (in `trimutils.rs`) answers `si_delTrimLine`, `si_trmDataIns`, `SI_trmCvrRve`,
  `si_trmCrvBBox` from the ports; the product wraps its own `Outside` in it.
- `curveFit.c` second batch (`fit.rs`, checked as in the top block). Power basis: orders 3 and 4 only, with
  the reciprocals kept unrounded on the x87 register (rounding them to double changes the last bit); order 5
  returns 1 and does nothing; other orders free the scratch and return 0 (below order 3 the original frees a
  buffer on its own stack, which the port skips; those cases are not compared). The normal equations are
  filled for orders 3 and 4 only; `plusEqualSegmentNormalEqns` has three column modes; `multVecByNrmlEqns`
  starts the band at column k+i and the point at n-k+1+i. The template point has weight 1.0 at +0x18 and a
  zero word at +0x20. Ported from `[soft 0x084cfae0 nurbsPowerBasis]` and the five others cited at the
  functions.

### Sixth round (2026-10-28, nurbs6): the last three differences explained, curveFit third batch

- `si_detectEdgeCross` (2 different) and `handleU/VCsdMulIntsct` (1 different, then 2 more once better cases ran) were all
  the original reading undefined memory, not porting errors. (a) The edge-sample tables: the original reads a malloc
  chunk-size word or a block 0x40 below a table that `si_detectEdgeCross` callocs itself, or past the top of a table
  the vector placed; the port reads its image. The port now counts such reads (`take_smp_outside_reads`, trimline.rs,
  a checker aid and no part of the original) and `nurbs-trimcheck` classifies a case that differs after such a read as
  not compared (only differing cases, and not under a mutation switch, so mutation sensitivity is kept). (b) The
  `si_trmDataIns` word 9e543be8 against ffffffff: the original passed a data word it never wrote (stack garbage; the
  vector prints it as unwritten, all ones); the port has zeros there. The checker now lets a word the original left
  unwritten match any word of the port in a logged call's arguments. A call that is missing, extra, or differs in any
  written word still differs (mutation `calls` unchanged).
- The high not-compared counts of `handleU/VCsdMulIntsct` (761/770) are not new: vec30 has the same. The cause is the
  generator's random flag lists: the original walks the rotated loop and needs a 0x100 node before each pair of seam
  nodes and room behind the last, else it reads through a null node. `gen_trim.py` now makes 60% of the cases
  well-formed: 327/299 not compared (vec36), same cases identical otherwise.
- `gen_trim.py hook` crashed with "no value for lp of si_delTrimLine": a null pointer is `[0]`, not `None`. Fixed;
  the group (vec35, 6 files): 4,200 of 4,200 identical.
- curveFit third batch, in `fit.rs` (checker `nurbs-trimcheck`, vectors `gen_fit.py ... ev`, `re/out/si-nurbs/vec41`):
  `[soft 0x084d2a60 evaluateSegment]` 413/500 identical, 0 different, 87 not compared (the original frees or writes
  a stack buffer for an order below 3); `[soft 0x084d2bbc computeNurbsEvaluationMatrix]` 700/700;
  `[soft 0x084d1cb8 multVecByEvalMat]` 656/700 (44 the original did not finish). Both mutation switches differ in every
  compared case. Facts: the segment cell is 0x98 bytes (previous +0, next +4, knot +8, first row +0x10, row count
  +0x14); spans of at most 1e-6 are no segment (`0x08661650`); a parameter belongs to the segment while
  `knot + 1e-6 > parameter` on the register; the scratch of an order below 5 lives on the stack. For an order of 5
  or more `evaluateSegment` calls `SI_nurbsMatrixBasisFunction (0x084d93c4)` and `SI_nurbsCrvBasisFunctions
  (0x084d9668)` (not ported; answered 0 in the vectors, so only the failure path is checked); `SI_showActivity` is
  called through the outside once per segment.
- Not yet ported (10 of 26): `fitPlaneToSamples (0x084cefd0)`, `partitionLinearData (0x084ce53c)`,
  `nurbsCrvFromComponents (0x084d1800)`, `multVecByEvalMatTrans (0x084d1f74)`, `allocateFixedData (0x084cf6ec)`,
  `updateKnotVector (0x084d0444)`, `updateNurbsEvaluationMatrix (0x084d2fe0)`, `calculateError (0x084d019c)` (x87 register
  juggling over six compares; read, not yet written), `fitNurbsPoints (0x084d081c)`, `SI_fitNurbsCurve (0x084ce350)`.
  Run: `gen_fit.py <dir> ev`, then `nurbs-trimcheck <dir>`.

## 15. Seventh round (nurbs7): the matrix basis functions and the fitting helpers (`fit2.rs`)

- The two basis-function routines, `SI_nurbsMatrixBasisFunction (0x084d93c4)` and `SI_nurbsCrvBasisFunctions (0x084d9668)`,
  are ported on the image with their own small matrix helpers (`SI_generalMatrixDbl` records: identity, product,
  element copy, disposal; the same arithmetic as `si-math`'s, written on the image because the heap blocks matter).
  `evaluateSegment` now calls them directly for an order of 5 and more, and is checked with the real callees
  (orders 3 to 7).
- `allocateFixedData`, `updateKnotVector`, `calculateError`, `nurbsCrvFromComponents` and `multVecByEvalMatTrans` follow
  the listing instruction by instruction; the numbers are in the status block at the top.
- Two reading mistakes the checks caught: in `calculateError` the zeroing of the sample offset sits before the span
  loop's back-edge target (the offset is set once and keeps pace with the sample index); in `nurbsCrvFromComponents`
  the test of the first record's closing flag reloads the record pointer (the sum loop's moving pointer is not used).
  Lesson for the next porter: when a `mov [local],0` precedes a loop label, check whether the label is after it.
- `updateKnotVector` calls `SI_nurbsCrvKnotInsert (0x084d4130)` on a work record that lives on the stack. The check answers
  1 and changes nothing, so the effect of an insertion is not covered; the product has to wire the call to
  `insert::crv_knot_insert` (first form), whose image form is not ported.
- Heap blocks from `SI_nurbsCrvAllocate` leave padding words unwritten; the check classes a case "agreeing in the defined
  part" (`NURBS_PARTLY=1`) as not compared, never as identical.
- `fit_nurbs_curve (0x084ce350)` is written (not cited, not checked): it needs `partitionLinearData` and `fitNurbsPoints`, which
  are not ported. Left for the next round, in this order: `fitPlaneToSamples (0x084cefd0)` (its x87 tail keeps some
  intermediates unrounded in registers and rounds others through locals; use `kit x87` and the raw listing together),
  `partitionLinearData (0x084ce53c)`, `updateNurbsEvaluationMatrix (0x084d2fe0)`, `fitNurbsPoints (0x084d081c)`, then the
  step 3 group (`si_nurbsCurveInterBldLinFromComp`, `SI_nurbsCrvFlatten`, `SI_nurbsCompGetClosedNC/OpenNC`).

## 16. nurbsgeom1 (2026-10-28): the knot helpers of `blend.c` (`blend.rs`)

Ported on `si_mem::Mem` and compared with the emulated original (`gen_blend.py <dir> kn ml fs mc`, then
`nurbs-trimcheck <dir>`; vectors `re/out/si-nurbs/vec60`). Read from the listings: `reverseKnotSeq (0x084cabf8)`,
`extendEndKnotFromDeltas (0x084cb0b8)`, `extendBeginKnotFromDeltas (0x084cb0f4)`, `buildDeltaVector (0x084cb4e0)`,
`checkMult (0x084cc8dc)`, `mergeKnotSequence (0x084caa20)`, the mesh-list trio `SI_meshAddList (0x084ce274)`,
`SI_meshFreeList (0x084ce2c8)`, `SI_meshCntList (0x084ce328)` (blend.c holds its own copy of them), `insertIntoKnotList
(0x084cc020)`, `si_nurbsCrvFillSegmentPoints (0x084ddf54)`, `SI_nurbsFactorToDistance (0x08503378)`, `insertMissingKnots
(0x084cc664)`, `insertMissingKnotsSrf (0x084cc790)`, `nurbsCrvMakeCompatibleKnots (0x084cc158)`, `SI_nurbsCrvMakeCompatible
(0x084cc29c)`, `SI_nurbsSrfMakeCompatible (0x084cc458)`. All 17 files: 0 different.

- Make-compatible: knots are scaled to a common length held unrounded in a register (`F80`); the merged array is never freed
  by the original, and the port does not free it. Epsilon for "same knot" is 1e-6 at all four constants.
- Hooks answered by stand-ins: `SI_nurbsCrvKnotInsert (0x084d4130)`, `SI_nurbsSrfKnotInsert (0x084d96bc)` (answered 1,
  change nothing) and `SI_nurbsSrfSwapUV (0x084dc5f4)`. The call sequence is compared, the effect of the insertions is not
  (the product wires them to `insert.rs`).
- Vector note: the walks in `insertMissing*` run past the end of a knot array when the wanted knot is above all of them;
  the generator pads each knot buffer with 1e9 so both sides stop at the same place (without it, 64 of 500 surface cases
  differed through the neighbouring heap bytes only).
- Skipped (display or dialog): none in these files. Not yet ported: the extend/merge/blend group of `blend.c`
  (`nurbsCrvExtendEndKnot 0x084cb168`, `SI_nurbsCrvMerge 0x084cb560`, `SI_nurbsCrvBlend 0x084cb730`, the surface forms),
  which call `SI_generalMatrix*`/`SI_nurbsMatrixBasisFunction` (outside this crate: reach by `Outside` only); `nrb_misc.c`
  elevate/convert routines; `fitPlaneToSamples` and the rest of `curveFit.c`.

## 17. nurbsgeom2 (2026-10-28): the extend, merge and blend group of `blend.c` (`blend2.rs`)

Ported on `si_mem::Mem` from the listings (the x87 reading with `kit x87` and `kit dis`) and compared with the emulated
original. Curves: `nurbsCrvExtendEndKnot (0x084cb168)`, `nurbsCrvExtendBeginKnot (0x084cb334)`, `si_nurbsCrvExtendKnot
(0x084cac38)`, `SI_nurbsCrvMerge (0x084cb560)`, `SI_nurbsCrvBlend (0x084cb730)`, `SI_nurbsCrvMergeLinear (0x084cbb0c)`,
`SI_nurbsCrvMergeJoin (0x084cbe80)`. Surfaces: `nurbsSrfExtendEndKnot (0x084ccc98)`, `nurbsSrfExtendBeginKnot
(0x084ccfd0)`, `si_nurbsSrfExtendKnot (0x084cae78)`, `SI_nurbsSrfMerge (0x084cc920)`, `SI_nurbsSrfBlend (0x084cd2a0)`,
`SI_nurbsSrfMergeLinear (0x084cd9b8)`, `SI_nurbsSrfMergeJoin (0x084ce028)`.

- Run: `gen_blend2.py <dir> xk mb ml sm` (groups: single extends and ExtendKnot; Merge and Blend; MergeLinear and
  MergeJoin; the four surface forms), then `NURBS_PARTLY=1 nurbs-trimcheck <dir>`. Vectors: `re/out/nurbsgeom2/vec61` (xk and
  mb, 4,200 cases), `vec62` (ml, 1,000), `vec63` (mb again with a third of the blends at `p3 = p5 = 0, p4 = 0.5`, 1,000),
  `vec64` (sm, 1,600). 7,800 cases: 0 different. Only 221 (the `n = 1` ExtendKnot cases, which do nothing) are plainly
  identical; the other 7,579 are "agree in the defined part": every one carries the pseudo-inverse's freed work vectors,
  which hold unwritten malloc data. Under mutation `1` and under `calls` every case differs (7,800 of 7,800). That is a weaker statement than identical and is reported as such.
- The matrix routines (`SI_generalMatrix*Dbl`) run natively. The pseudo-inverse `greville_inverse` is a copy of `si-kine`'s
  image form, which this crate cannot call (`si-kine` -> `si-model` -> `si-nurbs`); the copy is not cited. Moving that
  routine into a crate below `si-nurbs` would remove the copy.
- Curve and surface `ExtendKnot` differ for even `n`: the curve builds the begin side's delta vector as `(p6, p4)`, the
  surface keeps `(p4, p6)`. Found by the vectors (the first reading assumed the same).
- 80-bit arithmetic of the blend: the weight-divided components of the two joint points stay unrounded on the x87 stack;
  `X = ax*s + bx*t` is never rounded before the product with the weights' sum, `Y` and `Z` are stored as doubles first. The
  curve blend keeps `s = 1 - t` unrounded, the surface blend stores `1 - p4` as a double once. In `MergeLinear` the second
  component of `a`'s end point enters the interpolation through its stored double. In `MergeJoin` the three differences are
  stored as doubles, and the moved x is `((x/w + dx) * w)` with the sum unrounded (y and z rounded).
- `_DAT_08661578` and `_DAT_086615c0`, the blend factor `MergeJoin` passes, are 0.5 (an early port assumed 1.0 and 199 of
  500 cases showed it, through the ill-conditioned begin-extension).
- The surface blend does not write `SI_gblIndex` for the joint column, unlike the curve blend (which writes -1 at the joint
  in `SI_gblIndex` and `SI_gblIndex2[0]`). The joint column index is the stale loop counter of the copy loop. The surface
  forms call `SI_showActivity` through `Outside` once per point (Merge), per row (Blend's last pass, MergeLinear, MergeJoin).
- Not done in this round: wiring `SI_nurbsCrvKnotInsert (0x084d4130)`, `SI_nurbsSrfKnotInsert (0x084d96bc)` and
  `SI_nurbsSrfSwapUV (0x084dc5f4)` to `insert.rs` and `own::srf_swap_uv`. Those work on the owned `Curve`/`Surface` forms; the
  hooks of `blend.rs` and `fit2.rs` get the memory image, so a bridge (image record to owned form and back, with the original's
  free-and-allocate of the arrays) and a vector group comparing the effect are needed first. The stand-ins still answer 1.
  Also not started: the `nrb_b.c` four, `nurbsFrenetCoord`, and the `nrb_misc.c`, `nrb_convert.c`, `nrb_effect.c`,
  `curveFit.c` and `nrb_point.c` leftovers.

## 18. nurbsgeom3 (2026-10-29): the surface routines of `nrb_b.c` and `nurbsFrenetCoord` (`geom3.rs`)

Ported on `si_mem::Mem`; x87 order read with `kit x87` and `kit dis`. [soft 0x084de8ec nurbsSrfAveragePointDelta],
[soft 0x084de178 SI_shiftDirKnotsClosed], [soft 0x084ded40 SI_knotsSrfReparametrization], [soft 0x084dd29c
si_nurbsSrfInverseForConversion], [soft 0x08502904 nurbsFrenetCoord], [soft 0x084eec00 SI_nurbsCrvIndexHit].

- Run: `gen_geom3.py <dir> [ad sh rp ic fr ix]` (one group per routine), then `NURBS_PARTLY=1 nurbs-trimcheck <dir>`. Vectors
  `re/out/nurbsgeom3/vec70`: 900 + 900 + 900 + 700 + 900 + 900 = 5,200 cases, 5,117 identical, 0 different, 83 not compared
  (all `SI_nurbsCrvIndexHit`: the original reads unwritten heap of the sampled knot array when `*idx` lies past its count).
  Mutation `EMUCHECK_MUTATE=1` and `=calls`: 5,200 of 5,200 differ in both.
- `SI_nurbsCrvIndexHit`: on the image, from the ported `SI_SampleKnotVector`, `si_nurbsInterv`,
  `SI_nurbsKnotsCorrectMultAndValue`, `SI_nurbsMatrixBasisFunction`, `SI_nurbsCrvBasisFunctions`. The point is the one of the
  largest basis value of the span, shifted by the larger neighbour; answers 0 (leaking the sampled array, as the original) when
  the snap leaves no insertion. Open curves (+0x20 = 0) mostly; closed ones only as a stray flag.
- Average point delta: the distance is `sqrt(dz^2 + (dy^2 + dx^2))` of the weight-divided coordinates, all unrounded on the
  x87 stack; when `+0x4c == 4` the root is taken twice. A closed direction gets one more entry (average over the other
  direction of the first-to-last distance, never below 0.01 = `_DAT_08661770`) and the entries are shifted right with that one
  first; the closing entry is divided by `nv` in both directions (as read; the U case is not symmetric by intent) [read].
- Reparametrisation: calls `srf_dispose_lines` first; parametrisation 2 builds knots from the net with the averages and
  normalises (1e-6 floor), 0 gives uniform knots, any other value frees the knots and rebuilds them from the net. It returns
  at once when `nu < orderU` or `nv < orderV`.
- Inverse for conversion: mirrors the net rows through a stack copy, writes `SI_gblIndex (0x0878896c)` pairs, keeps the knot
  span as an 80-bit value across the calls; per trim it inverts each curve (`crv_inverse`), maps the points to `w*span - x`,
  negates the line sign, mirrors line ranges and nodes, calls `si_trm_cvr_rve`, reverses the curve list, then reparametrises
  and answers 1.
- `nurbsFrenetCoord`: open curve: past the end the last frame, before the start the first, else a bisection of the
  parameter list (`ldiv` by 2) and a linear blend in 80-bit, each stored as a float; closed: subtract the end while above it,
  add the start (not the range) while below it, at most the end. The three vectors are then made unit when the length
  (root of x^2+y^2+z^2, rounded to a float) exceeds 0.0001 (`_DAT_08661e38`). The sum order is read from the listing, not the
  decompiler's expression. [guess] the add-the-start step is an original bug; kept. Closed cases need start > 0 or the original
  loops without end; the vectors keep it positive. `SI_nurbsUpdateFrenet (0x085020a8)` (null frame array) is not ported; the
  port takes it as a callback and the vectors always give a frame array.
- Not done in this round: the leftovers of `nrb_misc.c`, `nrb_convert.c`, `nrb_effect.c`, `curveFit.c`, `nrb_point.c` and the
  three insertion/swap hooks (`SI_nurbsCrvKnotInsert`, `SI_nurbsSrfKnotInsert`, `SI_nurbsSrfSwapUV`), which still answer 1.
  `SI_generalMatrixGrevilleInverseDbl` stays a copy: moving it needs a crate below `si-nurbs`, and `si-kine` depends on
  `si-model` which depends on `si-nurbs`.

## 19. nurbsgeom4 (2026-10-29): `SI_nurbsSrfInverse`, the knot-insertion and swap bridges (`geom3.rs`, `bridge.rs`)

[soft 0x084dcaf8 SI_nurbsSrfInverse]: like the conversion variant (section 18) on a copy record, but the net is swapped from
index 0 and the trim mirror uses the span `k[n-order+1]-k[order-2]` (open) or `k[n-1]-k[0]` (closed). For a closed direction a
trim point goes through `interv` and becomes `knots[ecx] - (x - copy.knots[idx])`, `ecx` = 1 when idx is 0 and the order is 3
or 4 (U) or 4 (V) [read; the U/V asymmetry is as listed], else `n - idx + order - 4`. Ends with the reparametrisation and
disposes the copy. Vectors `vec80`: 700 cases, all agree on the defined part (the freed copy is unwritten heap, so the
harness counts them as "skipped"; the column `skipped` there means exactly this), mutation `1`/`calls` 700 of 700 differ.

The bridges (`bridge.rs`) read the image record into the owned form (`read_curve`/`read_surface`), run the checked owned
routine, then redo the original's heap traffic: scratch blocks (two `SI_calloc(order-1, 8)`, one points block of
`cross * order * (order+1) / 2`, with the work arrays' contents written), `SI_nurbsPointsAllocate` of the new net, old free,
`SI_calloc` of the new knots, old free, scratch freed (triangle, then the distances), `SI_nurbsSrfDisposeLines`. The curve
routine takes the scratch only for order above 4 (the smaller ones use the stack). `SI_nurbsSrfKnotInsertV` disposes the
display lines BEFORE its scratch, the U routine at the end [read from the call order; the free sequence differs].
[soft 0x084d4130 SI_nurbsCrvKnotInsert], [soft 0x084d96f4 SI_nurbsSrfKnotInsertU], [soft 0x084da6fc SI_nurbsSrfKnotInsertV],
[soft 0x084d96bc SI_nurbsSrfKnotInsert]: vectors `vec81` (1,200 + 800 x 3): 2,892 of 3,600 identical, 0 different, 708 not
compared (706 curve cases agree on the defined part only, the new points block being partly unwritten; 1 + 1 surface cases
where the original itself faults). The 33 first refused cases were all the original's undefined corners, now reproduced:
(1) a negative count (`r < 0`, from a knot of multiplicity above the degree) reads past the triangle: below order 5 that is
the stack area (ten points, zero here, only ever copied unwritten), for surfaces the heap beyond the triangle block, which
the bridge replays step by step against the image (`Scratch.3`), so the words copied are the heap's own; (2) the same
counts write past the end of the new points and knot blocks; the owned vectors grow, the bridge allocates the record's count
and overruns as the original; (3) `interv` on a closed direction whose knots all equal `t` walks off the array start: the
word there is the heap header, not near `t`, so it stops at -1 (`knots.rs`, no other vector changes: vec30/32/41 totals
are the same with and without). The +0x48 total stays `nu*nv` of the new counts. `SI_nurbsSrfInsert*` answers are masked to
16 bits. Mutation `1` and `calls`: 3,599 of 3,600 differ in both (the one not compared is the fault case).
[soft 0x084dc5f4 SI_nurbsSrfSwapUV] on the image: transposes the net (`SI_gblIndex` filled), exchanges the direction blocks
and the paired words (+0x70/+0x74, +0x78/+0x7c, +0x88/+0x8c, +0x8a/+0x8e, +0x90/+0x94), rebuilds the line array with the V
lines first (no trims), swaps x/y of every trim point, box and node and calls `si_trm_cvr_rve`. `vec80`: 700 cases, 682
identical, 0 different, 18 not compared; mutation 700 of 700 differ.
[soft 0x084e6960 si_nurbsClampFactor]: `vec82`, 600 of 600 identical, mutation 600 of 600. Below the range start it stores the
range END (as read), same as above the end.
Generator note: `srf()` in `gen_geom3.py` writes random range doubles; any routine that calls `interv` needs the real range
(`gen_geom4.py` and `g_ic` for the non-conversion routines patch it; with random ranges the original ran past 40M steps).
Not done: the other leftovers of `nrb_misc.c` (`nurbsDirElevateOrder` read: each interior knot written multiplicity+1 times,
tolerance 1e-5), `nrb_convert.c`, `nrb_effect.c`, `curveFit.c`, `nrb_point.c`. Routing the product's hooks to the bridges is a
change in other crates.

## 20. nurbsgeom5 (2026-10-29): display-line marking and the transfer/apply routines of `nrb_misc.c` (`geom5.rs`)

All on the memory image, checked by `nurbs-trimcheck` against the emulated original (`re/out/nurbsgeom5/vec90`, generator
`re/tools/si-nurbs/gen_geom5.py`, groups `us dc md nco nso acf asf`; arms in `bin/nurbs-trimcheck/geom5arms.rs`).

[soft 0x084ea144 updateStartEnd]: line records are (flag, start, end, ...) in a row of `count`; an open direction lowers every start
when `lo` is below it, else raises the end when `hi` is above it; a closed one extends both ends when `hi > lo` and end > start,
else sets start = `lo` and end = `(lo-1) % (nk-1)`. With `nk` = 1 the closed branch divides by zero and the original traps
(65 of 900 cases; the port answers `Err(Refused)`, these are the "not compared"). 835 of 835 other cases identical.
[soft 0x084ea484 dirtyCurves]: record index `((first+i) % (dir.nk-1) [+2-order if open]) * stride + base`; an existing record
with its flag set goes to `updateStartEnd`, one with it clear is filled with `[a-lo0, b-lo0]` and flag 1 over `stride` records;
a missing record steps back to the nearest existing one, then back `stride` records, and is treated the same. 1,177 of 1,200
identical, 23 not compared (budget stops and traps). A record index below 0 makes the original read/write before the line array;
the generator keeps `first >= 0` and `base >= (order-2)*stride` so none occurs.
[soft 0x084ea5ec markDirtyCcrvs]: `getPointInterval` on both directions (column `i % nu - 1`, row `i / nu - 1`), `dirtyCurves` twice
(strides from +0x7c with base 0, then strides from +0x78 with base `NoVlines`), then the flag at record `NoVlines-1` for an open U
with lines, and at record `count-1` (+0x80) for an open V. 1,200 of 1,200 identical. `image::MARK_DIRTY_CCRVS` can now be answered
by `geom5::mark_dirty_ccrvs` (routing change, not made here).
[soft 0x084ea860 nurbsCrvTransferOriginal]: copies closed word, counts, ranges and +0x24, resizes points (count*36) and knots
(count*8) with `SI_realloc`, then the points/knots transfer. 700 of 700 identical. [soft 0x084ec1e4 nurbsSrfTransferOriginal]:
disposes the destination's lines, copies the counts/ranges, frees and reallocates the net and both knot blocks, then
`SI_nurbsSrfTransferPointsKnots`. 600 of 600 identical (calls to `SI_genTreeDestroy` compared).
[soft 0x084e9908 SI_nurbsCrvApplyFunc2Points], [soft 0x084e982c SI_nurbsSrfApplyFunc2Points]: for every source point whose flag word
(+0x20) meets `mask` (0 = all): `func(x/w, y/w, z/w, &dst.x, &dst.y, &dst.z)` (each division as `coordinate * (1/w)`), bits 0x60 at +0x23,
then the move-point routine (curve: native `crv_move_point(dst*src.w)`; surface: `SI_nurbsSrfMovePoint` through `Outside`), finally
bits 0x60 on the owner and (surface) `SI_nurbsCapping`. The callback is stubbed as `nurbsDump` (a no-op) in the vectors, so the check
compares the argument words of every callback call. 700 + 700 of 1,400 identical.
Totals: 6,000 cases, 5,912 identical, 0 different, 88 not compared; mutation `1` and `calls`: 5,912 of 5,912 compared differ.
Run: `NURBS_PARTLY=1 $CARGO_TARGET_DIR/release/nurbs-trimcheck re/out/nurbsgeom5/vec90`.
Not done: `nurbsDirElevateOrder`, the elevate/convert routines of `nrb_convert.c`, `nrb_effect.c`, `curveFit.c`, `nrb_point.c` closers.

## 21. nurbsgeom6 (2026-10-29): the elevate routines and the curve convert group (`elev.rs`, `conv6.rs`)

Ported on the memory image, each checked against the emulated original with vectors `re/out/nurbsgeom6/vec100`
(generator `re/tools/si-nurbs/gen_geom6.py`, groups ed ce se gb ik lb cb kc ks):
`nurbsDirElevateOrder`, `SI_nurbsCrvElevateDegree`, `SI_nurbsSrfElevateDegree` (`elev.rs`); `getBezierPoint`,
`initNurbsKnotsFromBezier`, `nurbsCrvConvertToLinearBezier`, `SI_nurbsCrvConvertToBezier`, `SI_spline2NurbsCrvKeepCP`,
`SI_nurbsCrv2SplineKeepCP` (`conv6.rs`). Written but not checked and not cited: `initNurbsPointsVKnotsFromBspline (0x084df864)`.

Check (7,500 cases, 0 different anywhere): nurbsDirElevateOrder 900/900 identical; getBezierPoint 600/600;
initNurbsKnotsFromBezier 700/700; SI_nurbsCrv2SplineKeepCP 127 identical, 773 partly (UNINIT); the other five files
(SI_nurbsCrvElevateDegree 1200, SI_nurbsSrfElevateDegree 900, nurbsCrvConvertToLinearBezier 500, SI_nurbsCrvConvertToBezier 900,
SI_spline2NurbsCrvKeepCP 900) are all "partly": the port agrees on the defined part, and the original leaves a scratch word, the copy
record or unwritten spline-record fields as uninitialised heap. Mutation `1` and `calls`: all 7,500 differ.

Read versus guessed: everything above was read from the listing. The spline routines (`SI_splineUpdateMatrix 0x0848abd0`,
`SI_splineTouch 0x0848c924`, `SI_splineAddPoint 0x0848acc0`) are called through the `Outside` trait because `si-spline` depends on
`si-nurbs`; the vectors stub them and compare the recorded calls. Keys are 0x30 bytes (position +0xc, in tangent +0, out tangent +0x18).
x87: a key's x is the f32 of an unrounded extended division; y and z go through f64 first; mirrored tangents are the f32 `(a+a)-b`;
the knot step of `initNurbsKnotsFromBezier` is 3/(count-5), kept unrounded. `SI_nurbsCrvConvertToBezier` is only compared for orders 2 and 4
(other orders do not give 3k+1 points and the original then reads unwritten heap).

Run: `(ulimit -v 2000000; NURBS_PARTLY=1 $CARGO_TARGET_DIR/release/nurbs-trimcheck re/out/nurbsgeom6/vec100)`, with
`EMUCHECK_MUTATE=1` and `=calls` for the mutations.
Not done: `SI_nurbsSrfConvertToLinear` (needs a `SI_nurbsSrfValue` stand-in that writes a point), `SI_spline2NurbsCrvKeepCurve`,
`patchCardinalConvertToNurbsSrf`, `SI_patchConvertToNurbsSrf`, `nurbsCrvAddKnotNonUniform`, the closers, the curveFit chain,
`nrb_effect.c`, `SI_nurbsUpdateFrenet`, `SI_nurbsDistanceToValueFrenet`, `si_trmSrfHitInd`, `SI_nurbsSrfAddRowCol`.
## 22. routing8 (2026-10-29): the ported callees behind the hooks, in the checkers and in the product

Nothing new is ported here; the existing ports are put behind the places that used to answer 0, 1 or a stand-in.

`crates/si-nurbs/src/routed.rs` holds `ROUTED`, the addresses (about 45) of the image-level ports that can stand in for an
original callee (the knot-insert and swap bridges, `SI_nurbsSrfInverse`, `si_nurbsClampFactor`, `nurbsFrenetCoord`,
`SI_nurbsCrvIndexHit`, the 14 `blend2` routines, curve/surface copy, dispose and apply-matrix, `SrfValueAndNormal`,
`SrfForceCapping`, `InterCreateP`, `InterComputeRayT` (result in st0), `RayAddInter`, `RayDestroy`, `UpdateBbox`,
`PointOnLine`, the three inter/curveinter sub-routers). `Routed<'a>` wraps any `Outside`: a call whose address is in the list
and not switched off goes to the port, anything else to the inner responder. Switches: `NURBS_NO_ROUTING` (all off),
`NURBS_ROUTING_SKIP=0xADDR,..`, `NURBS_ROUTED_TRACE` (one line per routed call on stderr). `SI_nurbsUpdateFrenet (0x085020a8)`
stays with the inner side (not ported). `tests/routed.rs`: 2 tests (switch off forwards; null-record calls reach the ports).

Where it is wired:
- checkers: `NURBS_ROUTED=1 nurbs-trimcheck <dir>` answers routed callees by `Routed::route` and logs only the rest (an empty
  value counts as set).
- product: `si_scene::envside::envelope_call` and `si_rensetup::native::world::more()` send every `ROUTED` address to
  `Routed` over the model/NURBS side. That gives `SI_modelInverse` (via `SI_nurbsSrfInverse`), the `blend` hooks of the same
  crate's callers and `SrfForceCapping` a path. `0x0850cc48` stays unrouted there (st0 result). No Cargo edge was needed
  (`si-scene` and `si-rensetup` already depend on `si-nurbs`); no Cargo.lock change.

Checks with the real callees (generator `re/tools/routing8/gen_routing8.py`, vectors `re/out/routing8/vec90/91`; only
`SI_genTreeDestroy` is stubbed; column order cases / identical / different; "defined part" = agree in every defined word, the
original leaves new 9-word points partly unwritten):
- `insertMissingKnots` 300: 44 identical, 256 defined-part-same, 0 different. `insertMissingKnotsSrf` 300: 300 identical, 0
  different (real `SI_nurbsSrfKnotInsertU/V`). `SI_nurbsCrvMakeCompatible` 300 (knots forced positive: with negative start
  knots the generator made curves that hang the original): 81 identical, 219 defined-part-same, 0 different.
- Mutation `1` and `calls`, once each: all three above 300 of 300 differ.
- `SI_nurbsSrfMakeCompatible`: directions 1 and 2 were measured per case on the first set: 206 of 206 agree; direction 0
  (as read, it inserts the second direction's merged knots, rescaled, into the first) 45 agree, 49 do not finish in 4 s or
  fault, 3 different. Cause: the parameters lie outside the first direction's knot range, a case `bridge::srf_knot_insert_dir`
  does not follow. Open gap, not fixed. The regenerated sets (`vec91` dir 1/2, `vec92` dir 0) and `updateKnotVector` with
  the real `SI_nurbsCrvKnotInsert` (group `uk`, `vec93`) were queued behind the emulator slots when this was written.
- First frame of the 14 scenes (section 13.3), routing on against `NURBS_NO_ROUTING`: pictures and report text identical
  14 of 14 (`re/tools/routing8/scenes_first_frame.sh`). Only `Actor-Walk` reaches a routed address at all: 960 calls of
  `SrfForceCapping (0x084f0134)`; the other 13 reach none, so for them routing is wired but not exercised.

Skipped because trim6/trim7 still port the callee: `curveOrient` group (0x0850a330, 0x0850a36c, 0x0850a424, 0x0850bf64), the ray
remainder (0x0850dcd8 ff.), the tree group (`SI_genTreeDestroy 0x08515848`, `addCompFromTree`, `CreateSrfComps 0x08504130 /
0x085043c8`, `CompositeCurveDestroy`), `BldLinFromComp 0x085085e8`, `BuildOldInterList 0x085082c8`, `IntersectLists 0x08509de0`,
`computeIntersectionOrientation 0x085090b8`, `SI_nurbsUpdateFrenet 0x085020a8`, `nurbsCrvAddKnotNonUniform 0x084ee4a8`,
`si_trmSrfHitInd 0x084e497c`, `SI_nurbsCrvFlatten 0x084e62f4`, `markDirtyCcrvs`. Not done: real-callee vectors for
`convertToBez`, `interSetup/addIntersection` and the composite routines; `TriangulateTrimmedNurbsSrf (0x084fd14c)` in
`nurbs_on_image` (no sample scene has a trimmed surface); `si-mental` `mi_patch` own table.

## 23. nurbsfix1 (2026-10-29): `SI_nurbsSrfMakeCompatible` direction 0, real-callee vectors (`blend.rs`)

The open gap of section 22 is closed. In direction 0 `SI_nurbsSrfMakeCompatible` swaps u and v, merges the second direction's
knots, rescales them and inserts them into the first direction. The values lie outside that direction's range; the original's
span search clamps internally, but the unclamped value is what gets inserted, so the knot array afterwards is unsorted. The port
already followed all of that. The hang was in `insert_missing` [soft 0x084cc664 insertMissingKnots] and [soft 0x084cc790 insertMissingKnotsSrf] (the shared walk of the knot
helper): the inner walk "advance while the record's knot is below the wanted one and not within 1e-6" ran past the end of the
record's knot array when a wanted knot lay above all of them. The original then reads whatever sits above the heap top (the
emulator's unwritten-memory pattern) and stops at the first double that is not below the wanted value; the port read zeros
forever. Fix: `beyond_heap_top` (address at or above the allocator's top and inside the heap region) ends the walk there and
counts as "not close". Insertions do not depend on the garbage unless it lies within 1e-6 of the wanted value.
Honest limit: the walk-off result is fill-dependent in the original too; on the emulator's fill 2 of 300 direction-0 cases fault
in the original (unmapped read in `insertMissingKnotsSrf`).

Checks (real callees, `NURBS_ROUTED=1 NURBS_PARTLY=1`, per case under a wall-clock budget by `re/tools/nurbsfix1/perline.sh`;
fresh seed `nf1`, generator `re/tools/nurbsfix1/gen_nurbsfix1.py`, vectors `re/out/nurbsfix1/vecA`):
- `SI_nurbsSrfMakeCompatible` directions 0, 1, 2, 300 each: 900 identical, 0 different, no hang. Old direction-0 set (`vec92`):
  298 identical, 0 different, 2 original faults (was 63 hang, 1 different, 234 identical).
- Mutation `1`: 900 of 900 differ; `calls`: 900 of 900 differ.
- `updateKnotVector` with the real `SI_nurbsCrvKnotInsert` (group `uk`, `vecB`): 300 cases, 64 identical, 161 defined-part-same,
  75 not compared (original fault at `updateKnotVector+0x2b4`), 0 different; 331 real insert calls routed. Mutation `1` and
  `calls`: 225 of 225 compared differ.
- Negative-knot sets (`vec90`): `SrfMakeCompatible` 294 identical, 2 different, 4 not compared; `CrvMakeCompatible` 83 identical,
  8 different, 209 not compared. All 10 differences have negative or unsorted knots, where the original reads past its arrays
  (fill-dependent); they hung the port before. Treated as undefined in the original, not as port errors.
- `convertToBez` with real `SrfCopy`, `ApplyMatrix`, `KnotInsert`, `Dispose` and `SI_genTreeDestroy` (group `ctb`, `vecC`, rendercheck
  arm now honours `NURBS_ROUTED=1`): 300 cases, 65 defined-part-same, 0 different, 235 not compared (41 the original wrote past
  a buffer cap, 194 the port's result depends on stack/heap words nobody wrote: the patch-record box and the z cells copy
  uninitialised doubles, so original and port carry different fill). Not claimed as identical; no mutation run for this set.
Not done: real-callee vectors for `interSetup`/`addIntersection`, `si_nurbsTrimConvertToBez`, the composite routines.

## 24. nurbsgeom7 (2026-10-29): the convert leftovers and four `nrb_point.c` / `nrb_geometry.c` routines (`conv6.rs`, `geom7.rs`)

Read from the listings and ported on the memory image, checked against the emulated original (generators `re/tools/si-nurbs/gen_geom6.py`
groups ib sl kk and `gen_geom7.py` groups cv ad df; vectors `re/out/nurbsgeom7/vec110`, `vec111`, `vec118`, `vec120`; `stale/` holds the superseded runs):

- `checkValidOpenLinear (0x084eb194)`: 800 cases, 759 identical, 0 different, 41 not compared (the original reads a knot past its block);
  mutation `1` and `calls` 759 of 759 differ. The compare is against 1e-06 in extended precision.
- `nurbsSrfAveragePointsDistance (0x084ed2f4)`: 700 cases, 0 different, 700 agree on the defined part (the temporary copy the
  original makes holds unwritten words, so none counts as identical); mutation 700 of 700 differ under both switches. The mean
  chord lengths are summed in the original's order (dz squared first), taken as a square root twice for a curve type of 4.
- `SI_nurbsDistanceToValueFrenet (0x08503068)`: 900 cases, 892 identical, 0 different, 8 not compared (the original stops); mutation
  892 of 892 differ under both switches. `SI_nurbsUpdateFrenet (0x085020a8)` is not ported and the vectors always give the frames, so
  the branch that builds them is not covered. Before the start the shift uses the unrounded products, past the end each product is
  rounded to a float first.
- `SI_spline2NurbsCrvKeepCurve (0x084e049c)`: 900 cases, 0 different, 900 agree on the defined part; mutation 900 of 900 differ. The
  stub answers `SI_splineConvertToBezier (0x0848ee64)` with 0 (the port then returns 0), so the tangent form of kind 3 is covered
  only for what follows that call.
- `SI_nurbsSrfConvertToLinear (0x084e1490)`: written (cited since section 26): 24 identical, 613 different, 63 not compared; every difference is
  the block count, because the original's `SI_nurbsSrfValue` allocates and frees three blocks (record, points, knots) per sample and
  the port's value form does not. The surfaces themselves are not reported different; mutation differs in 697 of 700.
- `initNurbsPointsVKnotsFromBspline (0x084df864)`: 300 cases (vec120, budget 3,000,000: 700 cases at 40,000,000 did not finish in
  50 minutes), 187 identical, 0 different, 113 not compared (the original's span search does not end); mutation `1` and `calls`
  187 of 187 differ.

Run: `(ulimit -v 2000000; NURBS_PARTLY=1 $CARGO_TARGET_DIR/release/nurbs-trimcheck re/out/nurbsgeom7/vec111)` and the same on
`vec110`, `vec118`, `vec120`; `EMUCHECK_MUTATE=1` and `=calls` for the mutations.
Lessons for the generators: a buffer named `ax` is taken for the `ax` result (use another name); a pointer field in a record must be
declared `ptr` in its spec or the vector prints the original's addresses; read-only globals (`SI_xAxis` ...) must be declared with
`@0x...` or the port reads zeros.
Not done: `patchCardinalConvertToNurbsSrf (0x084dfbb0)`, `SI_patchConvertToNurbsSrf (0x084e00f8)`, `nurbsCrvAddKnotNonUniform
(0x084ee4a8)`, `openNormal (0x084eb244)`, the other closers, the curveFit chain, `nrb_effect.c`, `SI_nurbsUpdateFrenet`,
`si_trmSrfHitInd`, `SI_nurbsSrfAddRowCol`.

## 25. nurbsgeom8 (2026-10-29): `nurbsCrvAddKnotNonUniform`, `openNormal`, `fitPlaneToSamples` (`geom8.rs`)

Generator `re/tools/si-nurbs/gen_geom8.py` (groups nn, on, fp; env `GEN_N`), arms in `nurbs-trimcheck/geom8arms.rs`, vectors
`re/out/nurbsgeom8/vec130..132`. Each: 900 cases, mutation `1` and `calls` differ in 900 of 900.

- `nurbsCrvAddKnotNonUniform` [soft 0x084ee4a8 nurbsCrvAddKnotNonUniform]: 900 identical. Returns untouched if n < order; frees and
  zeroes the knots when type is not 2, order is 2 or 3, or n == order; otherwise the new knot count is old+1. An open curve at
  index 0 or n-1 rebuilds the end knots from chord lengths (each of x,y,z divided by w, floored at 0.01) and does not insert t.
- `openNormal` [soft 0x084eb244 openNormal]: 897 identical, 3 not compared (the original did not finish). Trims end knots and
  points with a 1e-6 tolerance, scratch from `SI_calloc`, marking via `SI_gblIndex`.
- `fitPlaneToSamples` [soft 0x084cefd0 fitPlaneToSamples]: 900 identical. For n <= 0 or NaN data the original reads stale stack
  for the farthest-sample vector; the port uses zeros. Rounding: centroid sums times 1/n in 80-bit stored as doubles; moments stored
  as doubles each step, yz kept in the register; six-case elimination with 1e-6.
- `srf_value_heap` (`SI_nurbsSrfValue` 0x084d6728 image form: three `SI_calloc` blocks per sample, static scratch growth) is wired
  into `srf_convert_to_linear`. `SI_nurbsSrfConvertToLinear (0x084e1490)`: 24 identical, 613 different, 63 not compared of 700
  (vec133 = copy of nurbsgeom7 vec110). The difference is heap block 7, word 0: the original holds data from a freed block, the port
  holds 0x6f. Mutation `1`/`calls` differ 697 of 700. Resolved in section 26.
- Not done: `updateNurbsEvaluationMatrix`, `partitionLinearData`, `fitNurbsPoints`, `SI_fitNurbsCurve`, nrb_effect.c,
  `patchCardinalConvertToNurbsSrf`, `SI_patchConvertToNurbsSrf`, `si_trmSrfHitInd`, `SI_nurbsSrfAddRowCol`, `SI_nurbsUpdateFrenet`.
  The `nurbsCrvAddKnotNonUniform` hook is not yet routed to the port.
## 26. nurbstess1 (2026-10-29): `SI_nurbsCrvFlatten` and the knot-sequence changers (`flatten.rs`, `chgknots.rs`)

Read from the code: [soft 0x084e62f4 SI_nurbsCrvFlatten] resamples a curve into a new curve whose points are the display
samples. It returns 0 when the point count is below the order. It takes the per-span sample counts from
[soft 0x084d74f0 SI_samplesPerKnot] (image form `samples_per_knot_img`), adds one to the total for an open curve, and makes the
curve, its points and knots (`SI_calloc(total [+1 closed], 8)`) and a scratch of `order` points of 0x24 bytes; a failed
allocation disposes the new curve and returns 0. For each span whose sample count is at least 1e-6 in absolute value the
step is `(k1 - k0) / samples` (kept in the x87 register), knots are `k0 + j * step` stored as doubles, the first point of a
span gets flag bit 0 at +0x22 (and bit 4 when the span is narrower than 1e-6). The scratch is filled with the curve (4D
evaluator) at `order` parameters, turned into forward differences with signed binomial coefficients, and each emitted
point is `(1/w) * coordinate` with the reciprocal unrounded and weight set to 1; the table advances by adding the next row.
The last knot is the range end, an open curve gets a final point; +0x10 and +0x18 are the first and last knot.
Orders above 4 leave the evaluator's three scratch blocks (knot distances, de Boor table) freed with their contents, which a
heap compare sees; `crv_value4d_scratch` reproduces that.

[soft 0x084d7dbc nrb_CRVsubdivc] and [soft 0x084d79cc nrb_subdivc] (the same with a row loop and a lerp
`A + (B - A) * t` with the 1e-6 snaps to either end) build a de Boor triangle of `(left+1) * (order+1)` points in a malloc
scratch and copy the apex into point `idx` of the output record. [soft 0x084d7c90 si_nurbsCrvChangeKnotsSeq] (no caller in
the binary) and [soft 0x084d77f4 si_nurbsSrfChangeKnotsSeq] fill a stack record with the new counts, call the subdivide
routine per new point at `si_nurbsInterv` of each new knot, then replace knots and points; the surface form then swaps the
two direction records (new first direction lands in the second slot) and sets +0x4c to 2.
All four are checked against the emulated original: identical or agreeing on the defined part, none different (numbers in the
bullet). The x87 detail that mattered is none: the F80 register form of the factors matched on the first try.

Guesses and open: the `per` field (+0x34) was not varied in the generator (`gen_tess1.py` fl group: the index set did not take).
The checkers' flatten stand-ins (`gen_curveinter.py`, `gen_trim8.py`, `gen_trim3.py`, `nurbs-curvecheck`) are not yet
replaced; that needs a route for 0x084e62f4 in `routed.rs` and new vectors.

## 27. nurbsgeom9 (2026-10-29): the ConvertToLinear difference and `updateNurbsEvaluationMatrix` (`geom9.rs`)

Generator `re/tools/si-nurbs/gen_geom9.py` (group `un`, env `GEN_N`), arms `nurbs-trimcheck/geom9arms.rs`, vectors
`re/out/nurbsgeom9/vec140`.

- **`SI_nurbsSrfConvertToLinear` (0x084e1490) difference found and fixed.** `SI_nurbsSrfExtractCurveSeg`, which
  `SI_nurbsSrfValue` calls, leaves its working arrays in static scratch blocks it never frees: `dl` and `dr` (order_u - 1 doubles each)
  and `aj` (order_u * order_v points of 36 bytes, entry j * order_u + i). The port kept them in local vectors, so the heap block the
  original later reuses held other data. `geom9::seg_scratch` recomputes them (a copy of the nd = 0 part of `eval::seg_kernel`, which
  belongs to another package) and `geom8::srf_value_heap` writes them into the three static blocks. Result on vec133 (700 cases):
  24 identical, 676 identical in the defined part (the original leaves part of a block unwritten), 0 different; 3 of the 700 never
  finish in the original. Mutation `1` and `calls` differ in all 697 compared. Now cited [soft 0x084e1490 SI_nurbsSrfConvertToLinear].
- **`updateNurbsEvaluationMatrix`** [soft 0x084d2fe0 updateNurbsEvaluationMatrix]: after knots were inserted, sweeps a knot index with
  the segment chain, splits a segment with a new 0x98-byte `SI_calloc` cell when a knot lies more than 1e-6 past its end, then
  re-runs `evaluateSegment` and `computeSegmentNormalEquations` on every cell (`SI_showActivity` through the outside hooks). The
  direction range is kn[order-2] .. kn[n-order+1] (closed: first and last knot). The scratch table is on the heap for order > 4 and
  on the stack for order <= 4; one stack word the original reads uninitialised (the previous new cell) is counted as undefined.
  Order 2 always makes the original fault, so it is not generated. 700 cases: 585 identical (about 450 with split cells), 0 different,
  115 not compared (original faults on hostile input: 88 unmapped reads, 24 unmapped writes, 3 more); mutation `1` and `calls`: 588 differ.
- Next: `partitionLinearData`, `fitNurbsPoints`, `SI_fitNurbsCurve` (all callees ported), then the patch converts, `nrb_effect.c`.


## 28. routing9 (2026-10-29): geom5/geom7/geom8 ports and the knot reparametrisation behind the hooks

Nothing new is ported; existing ports are put behind hooks (`kit routing-debt`, then read call sites).
- `nurbs::routed`: `markDirtyCcrvs (0x084ea5ec)` -> `geom5::mark_dirty_ccrvs` and `nurbsCrvAddKnotNonUniform (0x084ee4a8)` ->
  `geom8::crv_add_knot_non_uniform` join `ROUTED`; a refusal (`Halt`) lets the call go to the wrapped world. Reached in the product through
  `si_scene::envside` and `si_rensetup::native::world` (both already send `ROUTED` addresses to `Routed`).
- `si-deform::routed`: `SI_nurbsDistanceToValueFrenet (0x08503068)` -> `geom7::nurbs_distance_to_value_frenet`, its `SI_nurbsUpdateFrenet (0x085020a8)`
  call going to the wrapped world.
- `si-rensetup` update (`native/motion.rs`, `AnimSide`): `SI_knotsCrvReparametrization (0x084de590)` -> `image::knots_crv_reparametrization`,
  `SI_knotsSrfReparametrization (0x084ded40)` -> `geom3::knots_srf_reparametrization` (they were answered 0, counted); `ExprOutside`:
  `SI_materialSetParam (0x080f5964)` -> `si_expr::image::elem::material_set_param`.
- Not routed (reason): `scnMenu` is the scene window's menu dispatcher, not `si_particle_animation`; `SI_particleSetupReset` in si-view would need
  si-view -> si-dynamics (cycle through si-scene); `si_waveCreateIconMdl` from si-dynamics needs a `si_scene::image::Prog` that its
  `Outside` does not have, and no check of `wave_create_icon_mdl` exists (not cited by this step).
- Checks: `cargo test` si-nurbs, si-deform, si-rensetup pass; `motion-evalcheck` 93,275 of 93,275; first frames 277 identical, 19 identical but the icon, 0 different (of 296).
## 29. nurbsgeom10 (2026-10-29): `partitionLinearData` and the `nurbsCrvAddKnotNonUniform` routing (`geom10.rs`, `routed.rs`)

Generator `re/tools/si-nurbs/gen_geom10.py` (group `pl`, env `GEN_N`), arms `nurbs-trimcheck/geom10arms.rs`, vectors
`re/out/nurbsgeom10/vec150`.

- **`partitionLinearData`** [soft 0x084ce53c partitionLinearData]: splits a curve's sampled points into runs of near-straight and
  bent stretches by the cosine test of `classify_sample`, fits planes through the samples with `fitPlaneToSamples`, and returns
  the run starts and counts in heap arrays allocated in the original's order. The x87 arithmetic runs in F80. The plane-extreme
  indices the original leaves unset are taken as 0 (a quirk, [read]). 700 cases: 700 identical, 0 different, 0 not compared
  (660 return 1, 40 return 0). Mutation `EMUCHECK_MUTATE=1` and `=calls`: 700 of 700 differ each.
- **Routing.** `nurbsCrvAddKnotNonUniform` (hook address 0x084e_e4a8 in `image.rs`; the earlier text wrote 0x084ee4a8) is in
  `routed::ROUTED` and answers through `geom8::crv_add_knot_non_uniform`, returning 0 in eax. `cargo test -p si-nurbs`: all pass.
- **`SI_fitNurbsCurve` (0x084ce350)** exists in `fit2.rs` (`fit_nurbs_curve`, read from the listing: cos(angle * 0.017453314), then
  `partitionLinearData` and `fitNurbsPoints` per run); it is not cited because `fitNurbsPoints` is not ported.
- **`fitNurbsPoints` (0x084d081c)** read, not written: setup (n == 2 copy, n < order temporary direction), the main loop of
  evaluation matrix, normal equations and a conjugate-gradient-like refinement (moments scaled by 1e-4, ratio test against 0.95, 1e-12
  and 1e-6 guards; the `Gaxpy` call after the ratio test passes the z ratio twice because the y ratio is dropped from the register
  stack), `calculateError`, `updateKnotVector`, and the result copy. Next step: write it in `fit2.rs`/`geom10.rs` and check it with the
  real callees, then cite `SI_fitNurbsCurve` as a whole.
## 30. tri1 (2026-10-29): `sort.c`, the quicksorts and the exchange list (`sort.rs`)

All 15 routines of `sort.c` (0x0851ca00 to 0x0851d020) are on the memory image in `crates/si-nurbs/src/sort.rs`: the exchange
list (`swapRecord`, `SI_swapGet`, `SI_swapDispose`), and for integers, floats and C strings a swap, a pivot finder, a partition
and a quicksort (`iswap`, `ifindpivot`, `ipartition`, `SI_quicksort`; `fswap`, `ffindpivot`, `fpartition`, `SI_fquicksort`;
`sswap`, `sfindpivot`, `spartition`, `SI_squicksort`). The quicksorts take an optional parallel array of pointers (integers only)
and an optional flag that notes every exchange in a list of 12-byte records (first index, second index, next) kept in three
globals (`swapList 0x086dc2a8`, `swapEol 0x086dc2a4`, `swapCurr 0x086dc2a0`); `SI_swapGet` hands the records out one by one, and
`SI_swapDispose` frees them.

What the routines do that is not obvious: the pivot finder returns the first index after `lo` whose value differs from the one
at `lo` (the larger of the two is the pivot), or -1 when all are equal; the partitions move two cursors towards each other without
a bound check, so a pivot outside the range makes them run on past the array (the port does the same on the image and stops
only where the original would fault); the float routines compare with the ordered x87 compare, so a NaN is "not less" and "not
greater"; `fswap` goes through the x87 register, so a signalling NaN comes back quiet (the port does the same; the harness x87
does not, so this is unchecked); the string routines call `strcmp`, which the checks stand in for.

Checks (`sort-checkkit re/out/tri1/sort`, vectors from `re/tools/tri1/gen_sort.py` through `slot.sh emu`): 4,000 cases, 3,963
identical, 0 different, 37 not compared (28 in `SI_fquicksort` where the original runs out of budget on NaN arrays, 9 in
`spartition` where it runs past the string stand-in). Mutation `EMUCHECK_MUTATE=1`: 348 identical, 3,615 different (the 348 are
sorted arrays and swaps of an element with itself, which change nothing). `EMUCHECK_MUTATE=calls`: no effect, because these
routines make no recorded calls. Where it is used: si-deform `klusterSortIndexes` still has `SI_quicksort`, `SI_swapGet` and
`SI_swapDispose` as outside stand-ins (`crates/si-deform/src/outside.rs`); wiring them is another package's change.
Guesses: none.
The checkers' flatten stand-ins are replaced in section 27.

## 31. nurbstess2 (2026-10-29): the real Flatten in the checkers, the splits, the insert-knot and sampling routines (`split.rs`, `tess2.rs`, `chgknots.rs`)

Flatten stand-ins. `inter::route` answers 0x084e62f4 with `flatten::crv_flatten` (a refusal answers 0, as the stand-ins did) and
`routed.rs` lists it; the stand-in functions of `nurbs-curvecheck` are gone, and the generators (`gen_curveinter.py`
`mk_real_curve`, `gen_trim8.py`, `gen_trim3.py`) build complete curve records (knots, range, points of x y z w flag, step, the
global at 0x086d9c20) so that the original's own Flatten runs in the emulator. Numbers (`re/out/nurbstess2/t8b`, `t4`, `t3`):
`si_nurbsCurveInterBldLinFromComp` 300 cases, 298 identical, 0 different, 2 not compared (the original writes past a buffer of the case);
`si_nurbsCurveInterCheckFitSelfInter` 300 cases, 135 identical, 165 agree on the defined part (Flatten's malloc output is partly
undefined), 0 different; `SI_nurbsTrimCreateP` and `si_nurbsTrimSetStep` 600 of 600 identical. Mutation `1`: BldLin 298 differ (2 not
compared), CheckFit 300, trim3 511 (89 identical where the port produces no value); `calls`: BldLin 0 (it records no calls), CheckFit 164, trim3 391.
`SetStep` no longer generates step -1: the original's failure path calls the stubbed `SI_nurbsCrvDispose` from inside Flatten, and
the port's does not go through the recorded log. The older `trim7` vectors (`comp3check`) still record Flatten as a stand-in
call; their generator is outside this package.

Read from the code, then checked (vectors `re/out/nurbstess2/vec3`, `vec4`, `vec6`, `vec7`, `vec8`):
- si_OLDnurbsSrfChangeKnotsSeq (0x084d822c) is the surface changer with the old span search: the x, y, z of every old point are
  multiplied by the weight (+0x18) first and divided by it after the new points are installed (F80 products rounded on the store).
- SI_nurbsSrfInsertKnot (0x084d84a4): dir 1 exchanges the directions first, dir 0 after, any other dir never; the new knot
  vector (one entry more, `getKnotPosition` gives the answer) goes through the OLD changer.
- [soft 0x084e550c SI_nurbsSrfNoTriangles]: a trimmed surface (+0x60 above 0) answers `SI_noTrimmedTriangles`; otherwise both directions are
  sampled (`per` at +0x70, +0x74) and the answer is `(nu - 1) * 2 * (nv - 1)`.
- [soft 0x084d7fec SI_nurbsCrvSamples] (heap form `crv_samples_img`): with mode 0, `floor((hi - lo) / step) + 1` samples of the curve value into
  `SI_calloc(count, 0x18)`, the parameter kept in the register and clamped to the range end. [soft 0x084e5228 si_nurbsSrfSampleTrims]
  samples every curve of every trim composite at step 0.002 and stores x, y of samples 1.. in one 24-byte table per composite (+0x14).
- [soft 0x084d8e08 SI_nurbsCrvSplit]: a closed curve whose end is within 1e-6 of t is split at its start; a copy gets t inserted
  `order - 1` times, then an open curve becomes two open curves sharing the cut point, a closed one one open curve (`*second = 0`) with a
  knot vector that starts with `order - 1` zeros and continues with the old knot differences rotated by the span, and points rotated with the first one repeated.
- [soft 0x084d8530 SI_nurbsSrfSplit] does the same in the first direction of a surface (`dir == 1` exchanges before and after) and carries the second
  direction over to each piece. In the closed branch the original first copies the old row `order - 2` (a leftover loop counter) into the same row of
  the new points, then does the real copies; with fewer second-direction rows than that it writes past the block, and the port copies the same words.

Checks (`NURBS_PARTLY=1 nurbs-trimcheck`, columns: cases, identical, agree on the defined part, different):
- `SI_nurbsSrfNoTriangles` (vec3) 200: 200 identical, 0 different. `SI_nurbsCrvSamples` 300 (vec6): 300 identical, 0 different (a mode-1 share does nothing).
  `si_nurbsSrfSampleTrims` 200 (vec6): 200 identical, 0 different.
- `SI_nurbsCrvSplit` 300 (vec7): 300 agree on the defined part, 0 different (the new curves come from malloc; open and closed curves about half and half,
  closed ends within 1e-6 of t included; one case the original faults in is not compared). `SI_nurbsSrfSplit` 300 (vec11): 300 agree on the defined
  part, 0 different (dir 0, 1, 2; the one fault case likewise).
- `si_OLDnurbsSrfChangeKnotsSeq` 200: 199 agree on the defined part, 1 different (refusal); `SI_nurbsSrfInsertKnot` 200: 190 agree, 10 different (refusals).
Cause of the refusals of `si_OLDnurbsSrfChangeKnotsSeq` and `SI_nurbsSrfInsertKnot`: the old span search walks off the knot array when `t` equals the
last knot with repeated knots, where the original reads the neighbouring heap and ends; the port refuses and the case counts as different.
Orders above 4 are not generated for the splits and `SI_nurbsCrvSamples` (the evaluator's freed scratch blocks hold garbage a heap compare sees; with
order 5 45 of 300 sampling cases differed on exactly such words). Mutation (`1` and `calls`, one run per function) differs in every compared case.

Guesses and open: the grid and line group (adjustSmpSteps 0x084e35bc to nurbSrfUpdateLinesFD 0x084e55a4), `nurbsComputeIsoLines (0x084e5918)` and
`TriangulateTrimmedNurbsSrf (0x084fd14c)` are not ported; why the last was uncomparable was not looked into. `old_interv` is not a memory-image form.

## 32. nurbsgeom11 (2026-10-29): `fitNurbsPoints`, `SI_fitNurbsCurve`, `SI_nurbsUpdateFrenet` (`geom11.rs`)

Tools: `re/tools/si-nurbs/gen_geom11.py <outdir> fp|fc|uf` (env `GEN_N`, `GEN_TRIM=1`, `GEN_MAXORD`) writes the vectors,
`nurbs-trimcheck <dir>` (arms in `geom11arms.rs`) compares them. One checker change: the call-log comparison no longer
lists `cos`, `sin`, `acos`, because the port computes them itself (`si_x87::trig`) and the result words are compared.

### `fitNurbsPoints (0x084d081c)` [soft 0x084d081c fitNurbsPoints]

A least-squares fit of a curve to a point list with a conjugate-gradient loop on the control points. Facts the port keeps:
the stop thresholds are 1e-4 times the first moment; the loop breaks when a step improves by less than the 0.95 ratio; the
first Gaxpy gets the dropped y ratio; the step lengths are guarded by 1e-6; n == 2 copies the points; n below the order
builds a temporary direction record; an open curve has its knot vector trimmed in place; a degree-raise tail follows.

Check (vec160, 60 cases without the trim branch): 17 identical, 19 agree on the defined part (malloc words), 0 different,
24 stopped in the original (12 budget, probably endless loops because the stub of `SI_nurbsCrvKnotInsert` changes nothing;
3 reads of an unmapped `e4c` that the original frees on early failure; 9 frees of a stack pointer, not understood).
Trim branch (vec160t, 40 cases): 7 identical, 4 different, 29 skipped. The 4 differ only in word 3 of the direction record
(knot pointer after `updateKnotVector`: original 0x61000000, port 0x61001428), a heap-ordering difference not resolved.
So the trim branch is not verified; the function is cited for the non-trim path. Mutation `1` and `calls`: 36 differ, 24 stopped.

### `SI_fitNurbsCurve (0x084ce350)` [soft 0x084ce350 SI_fitNurbsCurve]

Chains `partitionLinearData`, `fitNurbsPoints`, `nurbsCrvFromComponents` and `cleanupPartition` (all ported). vec160, 80 cases:
0 identical, 10 identical given the recorded libm `cos`, 0 different, 70 skipped (about 53 undefined in part but agreeing in
the defined part, 14 stops of the original, 3 other). Mutation: 66 differ, 14 skipped.

### `SI_nurbsUpdateFrenet (0x085020a8)` [soft 0x085020a8 SI_nurbsUpdateFrenet]

Computes the frame (tangent, normal, binormal) at each control point of a curve and, on a closed curve, spreads the twist.
x87 evaluation order was traced from the listing (the products sum left to right at extended precision, short results go
through the F_SHORT constant). A closed curve takes its twist from the first and last frame positions (as the original does,
not the intuitive first and second). The cross vector flips on the sign tests of the original. Check (vec161, 300 cases,
orders up to 4, open and closed, empty scene lists): 15 identical, 285 identical given libm calls, 0 different; mutation: all
300 differ. Order 5 (vec160, 60 cases): 50 identical, 9 different, the difference being stale words of the freed 4-double scratch
blocks of `SI_nurbsCrvValue` inside `image::crv_value`, not this function; the generator caps the order at 4 (`GEN_MAXORD`).

Routing: `0x085020a8` is in the ROUTED lists of si-nurbs and si-deform; `nurbsFrenetCoord (0x08502904)` and the si-deform
hook `0x08503068` call the port.

### Not ported

`SI_nurbsSrfAddRowCol (0x084efa2c)` is a modal dialog command (event loop, state items, sleeps) that cannot be run headless.
`patchCardinalConvertToNurbsSrf (0x084dfbb0)` and `SI_patchConvertToNurbsSrf (0x084e00f8)` are read but not ported: the first
calls the unported `SI_skin2NurbsFarin`, `SI_createSkinFromNurbs` and model-level allocation; the second depends on the first
for the cubic case. The skin hooks and `SI_besselEndNurbsCrv` (prim2) stay unported.

## 33. nurbs12 (2026-10-29): citations for ported routines, and the rest of the package as not render

Seven routines that earlier sections ported and checked carried their address only as "(0x...)" in the doc comment, so the ledger did
not count them. They now carry `[soft 0x... name]`: [soft 0x084d7dbc nrb_CRVsubdivc], [soft 0x084d79cc nrb_subdivc],
[soft 0x084d7c90 si_nurbsCrvChangeKnotsSeq], [soft 0x084d77f4 si_nurbsSrfChangeKnotsSeq] (`chgknots.rs`; checks in sections 26 and 31),
[soft 0x084d822c si_OLDnurbsSrfChangeKnotsSeq] and [soft 0x084d84a4 SI_nurbsSrfInsertKnot] (section 31: 199 of 200 and 190 of 200
agree, the others are the port's refusal where the old span search walks off the knot array; the comments say so), and
[soft 0x084fd14c TriangulateTrimmedNurbsSrf] (finding 0044: 60 cases, 25 identical, 11 defined-part, 0 different, 24 not compared).
No Rust logic changed; no new checks were run.

Left out as not render (drawing, picking, dialogs, modeller commands, debug dumps): the grid and line group of `nrb_draw.c`
(adjustSmpSteps 0x084e35bc to nurbSrfUpdateLinesFD 0x084e55a4, `nurbsComputeIsoLines` 0x084e5918, the wire-frame iso-line and
trim-line display lists), `SI_nurbsCrvDraw`/`Hit`/`DrawCtrlNet`, `SI_nurbsSrfDraw`/`Hit`/`DrawControlMesh`/`DrawCtrlNet`,
`si_trmSrfHitInd`; `nrb_effect.c` (cut, subdivision, extract, knot dialogs, `nurbsDump`); `SI_nurbsCrvClose`, `SI_nurbsSrfClose`,
`SI_nurbsSrfAddRowCol` and their dialogs; `SI_patchGrid` 0x084890a0 and `SI_nurbsGrid` 0x084893d0 (primitive creation);
`SI_patchConvertToNurbsSrf`, `patchCardinalConvertToNurbsSrf`, `SI_nurbsConvertToFace` (callers are modeller commands);
`SI_nurbsCrvDump`, `SI_nurbsSrfDump`, `si_printTrimLine`, `si_nurbsPrintTrimLine`, `si_printTrimStatus`,
`SI_nurbsSrfHighlightTrimCurve`.
## 34. nurbs15 (2026-10-29): image forms of the earlier knot changer and insertion, the two distance helpers, the trimmed `SI_nurbs2Triangle` (`ins2.rs`, `dist.rs`, `convert2.rs`)

Tools: `re/tools/nurbs15/gen_dist.py` (vectors for the two st0 helpers); the check binary `nurbs15check` (a copy of `nurbs-trimcheck`'s driver with
its own arms, `ins2arms.rs`); the older vectors `re/out/nurbstess2/vec3` are reused for the changer and the insertion
(`NURBS_PARTLY=1 nurbs15check <dir>`; `NURBS_BLOCKS=1` lists the port's heap blocks beside the original's).

- [soft 0x084d81a0 si_OLDnurbsInterv] on the image (`ins2::old_interv_img`) and [soft 0x084d822c si_OLDnurbsSrfChangeKnotsSeq]
  (`old_srf_change_knots_seq_img`), [soft 0x084d84a4 SI_nurbsSrfInsertKnot] (`srf_insert_knot_img`). Read from code: the span search's
  downward scan runs to index `count - order - 1`, which is -1 when the count equals the order, so it reads the word before the knot block; the
  upward scan runs off the end when `t` equals a repeated last knot. The earlier forms refused there (10 of 200 insertion cases). The image
  form reads the words as the original does; only a read outside mapped memory is refused. What those words hold in the original is whatever
  lies beside the block (a heap header or the harness's fill), which a check cannot hold the port to: such a case is not compared
  (`ins2::take_outside_reads`).
  Check: 200 changer cases and 200 insertion cases, 0 different; all 400 are cases whose new point block is partly unwritten, counted as
  "agree on the defined part"; 2 insertion cases read the word before the knots and are not compared. Mutation `1` and `calls`: 400 of 400 differ.
  The product still calls the old forms (`split.rs` and the routed table); switching them to the image forms is a routing change in a file this step did not own.
- [soft 0x0850d31c si_nurbsInterDist2D] and [soft 0x0850dcd8 si_nurbsRayDist3D] (`dist.rs`): 1,500 cases each, all identical in st0 (80 bits),
  mutation 3,000 of 3,000 differ. The listing adds `(x-x')^2 + (y-y')^2` first and the z term last; `inter::ray_dist_3d` (older, used by callers)
  adds z first and can differ in the last bit of the extended result, so callers should use `dist::ray_dist_3d`.
- [soft 0x084fd14c TriangulateTrimmedNurbsSrf] (`trimmed::triangulate_trimmed_nurbs_srf`, already written; cited from `convert2.rs` because
  `trimmed.rs` was not this step's file): vectors from `gen_render.py` group `TriangulateTrimmedNurbsSrf` (100 cases into `re/out/nurbs15/vec`, one generator
  run takes about 12 minutes), `nurbs-rendercheck`. 61 compared: 42 identical, 19 agree on the defined part, 0 different; 39 not compared (38 the original stopped:
  7 budget, 23 unmapped read, 3 left the sandbox, 5 double free of the recorded stand-ins; 1 depends on unwritten memory). Mutation `1`: 61 of 61 differ;
  `calls`: 19 differ (the other 42 record no call the switch can change). The earlier note "why it was uncomparable was not looked into": it was never
  generated; the generator existed. Samples are small surfaces with 0 to 3 trim composites; larger cases hit the budget.
- `convert2::nurbs2triangle_full` ([soft 0x084e16f8 SI_nurbs2Triangle]): as `convert::nurbs2triangle` plus the trimmed branch (calls the above with the
  globals `needTrianglesWithIndex` 0x086d9fc0 and `trianglesWithIndex` 0x086d9fc4). Arguments read from the listing; not run against the original on its own
  (its pieces are checked; the product's `convert::nurbs2triangle` should be replaced by it, a routing change outside this step's files).
- Not done / not render: the `...Dump` and `...Print` routines of trimUtils, composite, curveInter, intersec, ray, trim, curveOrient and nrb_b (debug text),
  `SI_nurbsSrfHighlightTrimCurve` (drawing), the grid/line group `adjustSmpSteps`..`nurbSrfUpdateLinesFD` (display lines), `trimDial.c` (dialogs).
## 35. nurbs13 (2026-10-29): the skin routines of `cross.c` (`geom13.rs`, `geom13sys.rs`)

Ported on the memory image, checked against the emulated original (vectors `re/out/nurbs13/vec170`, `vec171`, made by
`re/tools/si-nurbs/gen_geom13.py`, groups `sk`, `fa`, `hs`; checker arms in `bin/nurbs-trimcheck/geom13arms.rs`):

- `SI_skin2Nurbs (0x08144c4c)` skins 2 or more curves; for 2 or 3 curves it spreads them over four rows with the weights 0.6666
  and 0.3333 (0.333 first for three). 1,000 cases: 34 identical, 966 agree in every defined word (the real `SI_modelAllocate`
  leaves model words unwritten), 0 different; mutation (both switches) 1,000 of 1,000 differ.
- `SI_skin2NurbsFarin (0x08146210)` interpolates the curves: V knots uniform or by chord length, then the open (Bessel) or
  cyclic system per column of control points. 1,000 cases (degree 2, types 0, 2, 3, 4, open and closed, some with equal
  curves to reach the "Cannot skin such a set of curves" path): 41 identical, 953 agree in the defined part, 6 flagged. The 6
  flagged words are doubles whose value equals an address of the emulator's test buffers (e.g. 0x61000000, the port gives the
  same number): the printer shows them as a pointer and the comparison resolves that pointer to the port's buffer. Settled in section 36 (a false alarm, shown by a second layout). Mutation: 1,000 of 1,000 differ. Type 1 (all V knots 0) is left out of the
  vectors: it yields NaN everywhere and the emulator's NaN sign (positive) differs from the hardware's (negative, as the port).
  Degrees other than 2 make the original's system sizes mismatch (heap overrun); not generated.
- `setup_closed_system (0x08147070)`, `setup_closed_input (0x08147648)`, `LU_decomp (0x08147c9c)`, `solve_LU (0x081478d4)`:
  each alone, 400 cases each: 1,541 identical, 59 agree in the defined part (n = 2 reads below the knots), 0 different;
  mutation 1,600 of 1,600 differ. The open-curve helpers are copies of the `si-model` ports (uncited, `geom13sys.rs`);
  Farin covers them.
- `SI_createSkinFromNurbs (0x08142ff0)`: see section 36. `meshSkinFromNurbs`, `skin2NurbsQuad`, `SI_nurbsSrfClose` and
  `SI_modelSubstituteIDefinition` go through `Outside`.

Outside calls the ports make: `SI_modelChangeName`, `SI_modelDispose`, `SI_statusDisplayMessage`, `sleep` (named
"0x080c4468" because the emulator image has no symbol for the import), `SI_genTreeDestroy` (inside the reparametrisation).
Skipped, not render: `patchCardinalConvertToNurbsSrf`, `SI_patchConvertToNurbsSrf` (modeller dialog side).

## 36. route3 (2026-10-29): the product calls nurbs15's forms; no new ports

Routing only, no function added. Each change was followed by a render of NURBS scenes before and after (see "Checks").
- `inter::ray_dist_3d` and `inter::inter_dist_2d` now call `dist::ray_dist_3d` / `dist::inter_dist_2d` ([soft 0x0850dcd8 si_nurbsRayDist3D],
  [soft 0x0850d31c si_nurbsInterDist2D]), so every caller (`raylink.rs`, `curveinter3.rs`, `curveorient2.rs`, `inter.rs`) gets the z term added last.
- `split::srf_insert_knot` and `chgknots::old_srf_change_knots_seq` are now one-line routes to `ins2::srf_insert_knot_img` and
  `ins2::old_srf_change_knots_seq_img`; the old bodies (which refused where the earlier span search leaves the knot array) are removed.
  The product reaches them only through the routed table and the check binaries; no sample scene calls them.
- `SI_nurbs2Triangle` in the product is `convert2::nurbs2triangle_full`: `si-model` `mesh_image::nurbs` (SI_nurbsConvertToMesh, its outside world
  bridged to `si_mem::Outside` by `MemSide`, only `eax` returned) and `si-render` `adapter::nurbs_on_image` (a trimmed surface's evaluator calls go
  to `si_nurbs::routed::Routed`; a call no port answers makes the node refuse, as `Halt::Unported` did before, and the node stays with the hook).
- The map2 past-end read in `SI_removeEdgeFromMesh` was fixed by mesh2; nothing in si-nurbs touches it.

Checks (route3): `render-frames` (release profile of `re/tools/si-render/pictures`, fat LTO) built before the routing and after all of it; every frame of
8 scenes with NURBS surfaces, `cmp` file by file: Actor-Walk 50, Expressions-MOUTH 115, General-Skeleton 1 (all it writes), MATTER-depth_of_field 100,
MODEL-glass 100, MODEL-JOJOtheFISH 100, Model-Methane_Molecule 100, MODEL-SoftDrinksCan 100: 666 files, 0 different. No sample scene has a trimmed surface,
so the trimmed branch is unexercised in the pictures. `cargo test -p si-nurbs`: 26 passed. The old-bodies' own checks (`nurbs-trimcheck` arms) now run the image forms.
## 37. nurbs14 (2026-10-29): `SI_createSkinFromNurbs` checked, the 6 Farin flags settled

`create_skin_from_nurbs` ([soft 0x08142ff0 SI_createSkinFromNurbs], `geom13.rs`) now has vectors (`gen_geom13.py` group `cs`, two
files: `#n` where the stubbed `meshSkinFromNurbs` and `skin2NurbsQuad` answer 0x1234, `#z` where they answer 0; `SI_nurbsSrfClose` and
`SI_modelSubstituteIDefinition` are stubs answering 0, the real `SI_skin2Nurbs`, `SI_skin2NurbsFarin`, `SI_modelAllocate` and
`SI_modelAddDefinition` run). Cases cover mesh and NURBS records (about 20% mesh), 0 to 6 curves, degrees 0, 1, 2, 3, open and closed
records (flag 0 / 1 / 0xFFFF, so the sign extension of the short), fresh model (null slot) and existing model, the `skin` name text
at 0x08687255. Checked: 1,000 + 1,000 cases, 377 identical, 1,623 agree in every defined word (uninitialised model words), 0 different;
mutation `EMUCHECK_MUTATE=1` 2,000 of 2,000 differ, `=calls` 2,000 of 2,000. Check found one real bug in the first
port: it dereferenced the model pointer one time too many to reach the first curve (`*(*models + 0x48)`, the port read
`**models + 0x48`); fixed. The checker's arm reads the stub answers from an input `nz`.

The 6 flagged Farin words were false alarms. The vector printer shows a word as `&buffer` when its number equals a buffer's
address in the emulator; a double's half-word equal to 0x61000000 is then compared against the port's own, different, address of that
buffer. Settled two ways: (1) `GEN_BUF=0x61001000` moves the emulator's test buffers to a base with non-zero low bytes
(the words that matched had 24 low zero bits, so only the first buffer's address could match): the same 1,000 Farin cases, 41
identical, 959 agree in the defined part, 0 different (vectors `re/out/nurbs14/vec_fa_odd`, mutation 1,000 of 1,000 differ);
(2) for the one `&m3+0x196` hit this showed in `#n` (a half-word of a curve's range), `GEN_ADDRS=<file>` logs the emulator's buffer
addresses: m3+0x196 is 0x61006fa6, exactly the port's word, so raw bits are identical. A third base (`0x61003000`) gives no hit at all
(`re/out/nurbs14/vec_cs_b3`). Commands: `GEN_BUF=0x61003000 GEN_N=1000 ... gen_geom13.py <dir> cs` then
`NURBS_PARTLY=1 nurbs-trimcheck <dir>`; vectors of a group made with `fa` take the same `GEN_BUF`.
Outside calls: as section 35, plus `meshSkinFromNurbs 0x08143ae4`, `skin2NurbsQuad 0x08145870`, `SI_nurbsSrfClose 0x084ed69c`,
`SI_modelSubstituteIDefinition 0x08467158` (answered by the world, not ported here).

## 2026-10-29, crv5: the scratch of `SI_nurbsCrvValue` above order 4 (`image::crv_value`)

Read from the listing of [soft 0x084d3550 SI_nurbsCrvValue]: below order 5 the two knot-distance arrays and the de Boor table are
on the stack; from order 5 they are `SI_calloc(order - 1, 8)` twice and `SI_nurbsPointsAllocate(order)`, given back with `free` in the
order table, first array, second array. For a derivative order above 3 two more blocks come from the heap, `SI_nurbsPointsAllocate(nd + 1)`
(the homogeneous derivatives) and `SI_calloc(nd + 1, 0x18)` (the rational derivatives), freed first, in that reverse order. None is
cleared before it is freed, so a heap compare sees what the routine left:

- the first array holds `t - knot` for the left side, the second `knot - t` for the right (the values `knot_distances` makes, one double each);
- the table is refilled with all `order` points of the span (36 bytes each, flag word included) at the start of every derivative order, so
  after the loop it holds the state of the last order `nd` (differences, then blending; a point left alone or copied by the 1e-6 tests
  keeps or takes the whole 36 bytes);
- the derivative block receives, after each order, the table's first point (36 bytes) in row `d`;
- the result block receives, row by row (0x18 bytes: x, y, z), the rational derivative after the quotient-rule subtractions, each
  intermediate stored as a double.

`eval::crv_value_parts` returns these beside the result (`crv_value` is its `value`); `image::crv_value` writes them into the blocks before
giving them back (the 4D form's version is `flatten::crv_value4d_scratch`, section 26). No arithmetic changed.

Checks (the emulated original, heap compared):
- `SI_nurbsUpdateFrenet` order 5, vec160 (60 cases, `NURBS_PARTLY=1 nurbs-trimcheck`): 1 identical, 59 given libm calls, 0 different (was 9 different).
  vec161 (300 cases, orders up to 4): 15 identical, 285 given libm, 0 different, unchanged.
- New vectors `re/out/crv5/vecuf` (`gen_geom11.py uf`, 300 cases, the order now drawn from 2..7 with 5 to 7 in a third of the cases, open and
  closed): 8 identical, 291 given libm, 1 different, 0 skipped. The one is a false difference of the vector format: heap block 403 word 36 is
  a double's low word that happens to equal the harness's address of the knot buffer plus 0x92 (0x61000092); the vector stores it as the
  symbol `&cuk+0x92`, the checker resolves that symbol to the port's own buffer, and the port holds the same number (0x61000092). Mutation `1`:
  300 of 300 differ.
- `nurbs-emucheck` on `re/out/si-nurbs/vec`: `SI_nurbsCrvValue` 2000 cases, 1968 identical, 0 agree on the defined part, 32 different (the
  refusal cases; the file before this step had 34), `SI_nurbsCrvValue4D` 1984/16 (22 before). Those vectors do not compare the heap.
- `nurbs15check`: `dist` 3000 of 3000 identical; `ins` and `vec` are skipped in every case with the old and the new build alike (the stored
  vectors are not usable by that driver here), so they say nothing.
- The path `nd` above 3 (the two extra blocks written) is not covered by a heap compare: no generator calls the routine with a derivative order
  above 3 and then reads the freed blocks. It follows the listing; unchecked.

## 2026-10-29, nurbsgap1: the temporary curves of `SI_nurbsSrfValueAndNormal`, and the knot margin words

Three gaps from the trimmed-surface and extrusion checks, all in the image forms of the surface routines.

### The temporary curve extractions [soft 0x084d6824 SI_nurbsSrfValueAndNormal]

The original does not evaluate the surface directly. Through `SI_nurbsSrfExtractCurveSeg` [soft 0x084d603c SI_nurbsSrfExtractCurveSeg] it extracts a position curve (derivative 0) and a derivative curve (derivative 1), evaluates the two with `SI_nurbsCrvValue4D` and `SI_nurbsCrvValue`, and disposes the curves at the end. When the cross vector is shorter than 0.001 it runs a first search that extracts two further curves per try and, every third try, disposes and re-extracts them. A shorter along vector runs a second search with one curve. The routine disposes the position curve only when the caller gave none and the derivative curve only when the caller gave none, so a given one leaks the other. The extractions allocate (100-byte header, a control block, a knot block) and grow the static scratch blocks beside the extractor, which are never freed. The earlier port left all of this out, so the heap block count and contents differed from the original.

Design: `eval::srf_value_and_normal_with` holds the algorithm once over a small trait (`SvnEnv`: extract, value4d, value, dispose). `ValueEnv` is the pure form (no heap); `image::HeapEnv` calls `geom8::extract_seg_heap` and `geom8::dispose_curve`, which allocate and free in the original's order and write the scratch into the image. `geom9::seg_scratch` is no longer used by it.

### Knot margin words

In degenerate knot vectors (all zeros, span 0 or 1, order 4) the knot distance loop reads the knot array below index 0. The original reads the heap word before the block. `lib.rs` now keeps thread-local margin words (32 each side, read from the image where mapped); `SrfHeld::margins` registers them for the call through a guard. `image::extract` uses the same guard, so the four `createModelFromExtrusion` NURBS-face cases no longer end in `Refused("SI_nurbsSrfExtractCurveV")`.

### Checks

| Check | Before | After |
|---|---|---|
| SI_trimUpdate, native SrfValueAndNormal, 2000 cases | 1836 identical, 10 different, 34 not compared | 1836 identical, 0 different, 34 not compared (130 defined-part) |
| `createModelFromExtrusion` cmfe_nface, 300 cases | 272 identical, 4 different, 24 not compared | 276 identical, 0 different, 24 not compared |
| vec160/161/170/171/133/130-132, crv5, nurbs14, nurbs15check dist (3000) | | unchanged: 0 different (known false alarms only) |
| `nurbs-emucheck` on `re/out/si-nurbs/vec` | 70961 of 72531 identical | the same |

Mutation: with `EMUCHECK_MUTATE=1` the trim set gives 1085 different, with `calls` 1298; cmfe_nface gives 276 and 108. Scripts: `re/out/nurbsgap1/runchecks.sh`.

Open: with the stand-in vectors (`TRIM_SVN=1`) one case of 6000 differs in heap block 44, word 134 (original 02, port 01); the same before and after this change, and not the 32-byte block of `si_nurbsRayTraceSrf2` that trim1 described, which did not reproduce in regenerated sets of 2000 and 6000 cases. Not diagnosed.
## 19. SI_nurbsSrfForceCapping in the render's update: already routed (2026-10-29, forcecap1)

The 2,880 "answered 0" calls of the 9b1c873 census are `motion-evalcheck`'s own host (re/tools/si-motion/evalcheck),
not the render: the same census's render-frames tables show the routine 480 times answered by a port and 0 times
answered 0 on the first frames. In the render the routine (`srf_force_capping` = [soft 0x084f0134
SI_nurbsSrfForceCapping], checked 2,172 of 2,172 in section 13.15) is reached through `si_scene::envside::envelope_call`
(its `si_nurbs::routed::ROUTED` arm) before `si-rensetup`'s `ModelSide`; `ModelSide` has a second arm for it
(`RENSETUP_NO_FORCECAP=1` switches that arm off). Switching only that arm changes nothing: first frames of 296 scenes
277 identical + 19 identical but the icon in both builds, byte-identical pictures; every frame of ZIP_PATCH-teapot,
Glue-SplineSurf, MODEL-JOJOtheFISH, MODEL-SoftDrinksCan, Actor-Walk, Model-Methane_Molecule, SurfaceTag-Torus,
MODEL-glass: all 850 pictures hash-identical with and without the switch. Open: a switch that really disables the port
must guard the ROUTED arm in `si-scene` envside.rs; the evalcheck host could route it the same way.
## 2026-10-29, last1: `SI_nurbsSrfClose` and `SI_nurbsConvertToFace` (`close.rs`)

Read from the listings of [soft 0x084ed69c SI_nurbsSrfClose] and [soft 0x084e1450 SI_nurbsConvertToFace]; both are in `crates/si-nurbs/src/close.rs`
(`srf_close`, `nurbs_convert_to_face`).

`srf_close(srf, how)` works on the second (V) direction: `how` 2 opens a closed V linearly (rows go, the message "To open it linearly you must
have at least %d points." at level 2 when the count is too small) or closes an open one by new rows blended from the last and first rows;
`how` 1 turns a closed V into an open one by inserting the end knots (`SI_nurbsSrfKnotInsertV`, the index list in `SI_gblIndex` propagated
after each) and cutting the net and knots; any other value builds the V parametrisation from the chords of the net (chord-length knots, the
last interval from the corner points, held to 0.01) or trims the end knots with `openNormal`. Every path ends with the display lines
disposed, the V closed flag flipped and `SI_knotsSrfReparametrization`. Intermediates that the original keeps in the x87 register stay at 80 bits
(the blend `x*(1-t) + y*t`, the squared chords). `nurbs_convert_to_face(curve, face)` fills a face outline from a curve (the point loop with the
curve's distance points, `SI_nurbsCrvFillDistancePoints`), null face allowed; the generic tree and the index lists are given back as the original does.

Wiring: `geom13::create_skin_from_nurbs` calls `close::srf_close(srf, 1|0)` directly instead of the hook (the result is discarded as the original
discards it). This path is a modeller operation (skin of curve models), not reached while loading or rendering a scene, so no rendered frame
can change; the 6 NURBS scene frames were not re-rendered and are "not compared" (the tests of the crate pass, 26 of 26).

Outside hooks still answered (they only touch lists and a message line): `SI_indexModifAllocate (0x08176860)`, `SI_indexModifPropagate (0x08176988)`,
`SI_clusterModelUpdate (0x081f1a1c)` (si-modeler depends on si-nurbs, so they cannot be called from here) and `SI_statusDisplayMessage (0x083cca94)`.
`SI_indexModifFree (0x08176a00)` is done inline.

Checks (`re/tools/last1/gen_last1.py`, emulated original, `NURBS_PARTLY=1 nurbs-trimcheck re/out/last1/vec2`, heap compared):
- `SI_nurbsSrfClose`, strictly increasing knots: 500 cases, 408 identical, 2 different, 90 not compared (original ran past its budget).
- `SI_nurbsSrfClose` with the generator's random and repeated knots (`#wild`): 500, 410 identical, 2 different, 88 not compared.
- `SI_nurbsConvertToFace`: 500, 55 identical, 0 different, 445 not compared (the original faults or reads unwritten memory on made-up curves).
- Mutation `1` and `calls`: 0 identical in all three files (every compared case differs).
- The 4 different cases are all `how` = 1: one heap word is +inf in the original and NaN in the port. Not explained: the weights and knots of the
  cases are ordinary, and the knot insertion they call is checked elsewhere. Open.
- The index stubs answer 0, so the propagation after a nonzero allocate is not exercised; that part follows the listing and is not compared.

