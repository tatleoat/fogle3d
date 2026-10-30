# 0032 — The camera (`sys_camera.c`) and quick stretch (`qstretch.c`) on the image

Date: 2026-10-14. Binary: Linux `soft`, sha256 `ef21a382c3325d42…`. Package: si-camera
(`crates/si-camera`, `re/tools/si-camera/`, scratch `re/out/si-camera/`), and the quick
stretch's update in `crates/si-kine/src/image/qstretch.rs`.

Why: `motion-evalcheck` (finding 0017, section 14) held 135 of the original's 93,275
`evalScene` dumps not identical in the working tree of this day. 80 of them were cameras
(30 filed as "camera up vector", 50 as "position limits", which are cameras whose roll alone
differs) and 24 hung on quick stretch (12 filed as such in `ACTOR-octopusII` 1-0 and 2-0, 12
filed as "expression" in `EXPRESSIONS-simple_attractor` 1-0, 2-0 and `simple_repulser`).
After this package: **93,244 of 93,275**; the 104 are all identical.

Read from the disassembly (`objdump` of the binary; the decompiler's output only to find
one's way) unless said otherwise. Nothing here is a guess unless it says so.

## 1. What the source file holds

`sys_camera.c`, 0x080d3b60 … 0x080d7704, 38 functions (the module map's count; it includes
four small unnamed ones). By what calls them:

| Who calls | Functions |
|---|---|
| the animation update (`updNodeTransformUpdate`, relation 1117) | `SI_camMat` |
| the viewer and the display (`si_drawGrafnode`, `SI_cameraDrawWire`, `SI_setCamPerspView`, `SI_setCamOrthoView`, `SI_refreshAllControls`) | `SI_camMat`, `SI_setCamView` |
| the mouse's moves (`doTransSGI`, `doRotSGI`, the schematic's drag) | `SI_addCamTrans`, `SI_addCitTrans`, `SI_addCamRot`, `SI_displayCamTrans`, `SI_displayCitTrans`, `SI_displayCamRot` |
| the menus of the four modules | `SI_selectInterest`, `SI_selectCamera`, `SI_hideCamera`, `SI_showCamera`, `SI_setCamera` (the camera dialog, 5,170 bytes, with `updateDof`, `camGetSetting` and five depth-of-field helpers), `SI_pictureFormatDialog`, `SI_cameraReset`, `SI_camAndScnToOrigin` |
| the windows and their files | `SI_cameraTransfer`, `SI_camIdConvert`, `SI_camGetDefaultFromId`, `SI_camAspSet`, `SI_changeCameraAspect` (`camAspRefresh`) |
| start-up | `SI_initCamDefault` (`resetCamDefault`, `SI_camReset`) |
| the shaders' dialog | `mrUdfAddToDict`, `SI_shaderFcurveUpdateFromDialog` |

The renderer's camera path does not go through this file: the native renderer's camera is
`camera.c`'s `SI_camera` (finding 0012, si-render); its command `-t` calls
`SI_camAndScnToOrigin` (not ported here).

## 2. The viewing matrix: `SI_camMat` (0x080d62d4)

`SI_camMat(camera, matrix)`: the camera record (`SI_Camera`, 0x13c bytes, laid out as
si-elemio's camera reader has it: position +0x10, interest +0x1c, roll +0x30 in radians) and
sixteen floats to fill. In order:

1. R = `SI_matrixZRotation(roll)`, from the roll as the call finds it, into a local.
2. d = interest − position, each difference kept in the register; the three are stored as
   floats, but the test uses the unrounded ones: unless 0.0001 (a double, 0x08645700) <
   (d_y² + d_x²) + d_z², the matrix is `SI_identite` and only its translation is set (step
   7). A NaN takes this way.
3. d (the stored floats) is normalised (`SI_vectorNormalize`). The up vector u: when
   0.0001 > |d_x| + |d_z| (looking straight up or down), (0, 0, −1) for 0.0001 > d_y and
   `SI_zAxis` (copied as words) for d_y > 0.0001. A third case, d_y equal to the double
   0.0001, would leave u as the stack holds it; no float equals that double, so it does not
   occur. Otherwise the axis a = d × `SI_yAxis` is normalised, the output matrix becomes
   `SI_matrixAxisRotation(π/2, a)` (π/2 the float at 0x0864570c) and u = d through it
   (`SI_vectorMultiplyMatrix`): d turned a quarter towards Y.
4. **The up-vector constraint** (relation 1117 `CamUpVector`): the camera's element
   (`SI_edGetElemFromCamera`); the model element related to it by 1117 in its forward list
   1 (`SI_elemdictFindRelatedElem`, chapter MODELS, `SI_cid` + 0x20); the current relation
   entry (`SI_elemdictGetCurRelation`) and its activation curve (`SI_edGetFcvFromRelation`,
   asked for whether or not it is used). When the entry's 16-bit word +0x14 is 1 and there
   is no curve or `SI_fcurveIntegerValue(curve, t, 0, 1)` > 0, with t =
   float(`SI_frameCurrent`) ÷ `SI_frameRate` rounded to a float (the original sets the
   rounding mode round `frndint`, which changes nothing on an integer): v = the model's
   global translation (+0x1b8) − position, stored as floats and normalised; c = d × v, **not
   normalised**; the output matrix becomes `SI_matrixAxisRotation(π/2, c)`; w = d through it;
   **the roll becomes `SI_vectorAngle(u, w)`, negated when c · u < 0**, the dot product formed
   as (c_x u_x + c_y u_y) + c_z u_z and kept at 80 bits (spilled as ten bytes across the
   call); a NaN is not negative.
5. s = d × u. The matrix is `SI_identite`; rows 0, 1, 2 become s, u and −d (each moved
   through the unit; −d by `fchs`).
6. matrix = R × matrix (`SI_matrixMultiply`): **the roll of step 1**, not the one step 4
   wrote. A constrained camera's matrix therefore lags its roll by one call; the update
   discards the matrix (a local), so what the update leaves is the roll.
7. Row 3's first three floats = the position, copied as words into locals and from there
   through the unit.

The three cross products are all compiled alike: each component is the difference of two
exact products in the register, rounded to a float once it is stored
((a_y b_z − a_z b_y, a_z b_x − a_x b_z, a_x b_y − a_y b_x)). The update's step for 1117
(`updNodeTransformUpdate`) calls `SI_camMat(camera, local)`; si-motion has had that call as
an outside routine (`cam_mat`).

**What the evalScene dumps show.** The 80 camera dumps were all the roll: the update left
the file's roll where the original had set it from the up-vector model (−0.0000 against
−0.0198 in the five motion-capture scenes, 17.2175 and others in the three
`CONSTRAINT-Camera…` scenes). The cause label "position limits" of 50 of them came from the
tool's order of causes, which puts the limited object first.

## 3. Depth of field (`findhyperfocal` … `findoutofocus`)

The camera dialog's five helpers, 0x080d3d9c … 0x080d4098, are **the same instructions** as
`camera.c`'s `findHyperfocal` … `findOutoFocus` (0x0827299c …; finding 0012, section 8,
si-render), instruction for instruction, each copy reading its own copy of the eight
constants (0x08645580 … 0x086455b8 here, 0x08654490 … 0x086544c8 there, the same values: the
float nearest 1e-6, the doubles 0.1, 0.01, 0.001, 0.0001, 0.00001, 0.85 and the float 12).
Two copies of one static source. Ported again here as value functions (`si_camera::dof`), as
finding 0012 describes them; the lead may reconcile the duplicates.

## 4. Moves: `SI_addCamTrans` (0x080d676c), `SI_addCitTrans` (0x080d67e8)

The two differ only in the field: the vector is copied (as words) into a local; when the
translation preference `SI_glbManip` + 0x0c (`PREF_TRANSLATION`) is 0 it is turned into the
camera's axes (`SI_camMat` into a local matrix, its translation cleared,
`SI_vectorMultiplyMatrix` in place); each of the three floats of the position (+0x10) or the
interest (+0x1c) gets the vector's float added in the register and stored rounded. With the
preference 0 a move can therefore change the roll too (step 4 above).

## 5. Quick stretch: `SI_animQuickStretchUpdate` (0x085a144c)

The update of relation 239 (`QStretch`) is not a step of the update list's dispatcher: the
geometry step leaves post info 3 (si-motion's `geom.rs`), and the model's geometry update
(−200) calls `SI_modelReadjustDefinition (0x0846e340)` (`model.c`, si-model), which among
much else (points' transfer, waves, lattices, fittings, cluster centres, envelopes) does, for
a model element related by 239 in its forward list 2: the update node of 239 for the model
(`SI_edFindUpdateNode(&dict->updList, model, 239, NULL)`); when that node, its arguments, its
16-bit +0x20 and its first argument's 16-bit +0x0c are set and `SI_checkActiveTime(node,
time, &t)` answers, `SI_animQuickStretchUpdate(node, t)`. Its time is `SI_updateTime` when the
16-bit `SI_updateTimeSet` is set, else float(`SI_frameCurrent`) ÷ `SI_frameRate` (`fild`,
`fdiv` of the float, stored as a float).

`SI_animQuickStretchUpdate(node, time)`:

1. Two local matrices are copied from `SI_identite`; nothing for a null node or a node
   without a model (+8). The model's quick-stretch element and data:
   `SI_QuickStretchGetElem (0x085a2428)`, `qstretch_misc.c` — `SI_edGetElemFromModel`, the
   element related by 239 in forward list 2 of chapter MODELS (`SI_elemdictFindRelatedElem`,
   its index to the caller's pointer), and the data of that relation entry
   (`SI_elemdictGetCurRelation` + 0x18, read without a test of the entry); nothing without
   one. The data's 16-bit word +0x122 is kept for step 8.
2. Twice over the node's arguments from the second on: an active argument (+0x0c) of the
   ANIMATION chapter (`SI_cid`'s first word) with a curve gives the curve's value (1 for a
   curve without keys, else `SI_fcurveValue(curve, time, 1)` at the register's width). In
   the first pass the kinds 0xf5 + k (k < 12) store it as a float at data + 0x0c + 0x18 k and
   then that float × 0.005 (a double, 0x08672b60); in the second the kinds 0xe9 + k store it
   at data + 0x10 + 0x18 k and then that ÷ |the float at + 0x0c + 0x18 k|. Every such curve
   is evaluated in both passes, whatever its kind.
3. While `SI_playBackRun` (16 bits) is set: nothing when float(trunc(float(`SI_frameStart`
   + 2.0))) ÷ `SI_frameRate` > time (the truncation is a `frndint` toward zero under a
   rounding mode chosen by the sign).
4. A model with definitions whose current one (+0x48) is still the stored one (+0x38 at
   +0x36) gets a copy of it as the current one: `SI_patchCopy`, `SI_faceCopy`,
   `SI_meshCopy`, `SI_splineCopy`, `SI_nurbsCrvCopy`, `SI_nurbsSrfCopy` for kinds 1, 2, 4, 6,
   9, 10, nothing for the others.
5. Two vectors, zero; when the data's 16-bit mode (+0x120) is set, every active argument of
   the MODELS chapter (a control: relation 238 `QSControl` makes these) of algorithm 0x10 gives
   the first, of 0x11 the second: `SI_yAxis` through the 3 × 3 corner of the control's
   transposed global rotation (`SI_matrixTranspose`), each component ((t₀ y₀ + t₁ y₁) + t₂ y₂)
   kept in the register and multiplied by |s| s (s the control's y scaling, +0x104), then
   stored as a float.
6. **The model's kinematic record** (+0x2f0, `__SI_KineInfo`, 0x60 bytes) is allocated when
   it has none: `SI_modelKineStateAllocate (0x0849f9f8)` = `SI_malloc(0x60)` and
   `SI_modelKineStateSetDefault (0x0849f37c)` (`settings.c`: position zero, the orientation
   `SI_matrix3Copy(SI_matrix3ID)`, the velocities and accelerations zero, each vector written
   z, y, x). Nothing more without a record. When `SI_reldictGetRelChapGrp` knows the
   attributes 90 and 91 (`LinVel`, `LinAcc`; both asked before either is tested) and
   `SI_getFcvTriplet` finds all six of their curves, the six values go to +0x30 … +0x44; the
   same for 92 and 93 (`AngVel`, `AngAcc`) to +0x48 … +0x5c.
7. Up the fathers (+0x2bc) that have quick stretch too (`SI_QuickStretchGetElem` with a
   second data pointer, which must not be null): the last element found; with mode 1 one
   step only (the flag is set from the **model's** data, not the father's).
8. `SI_QuickStretchCalculation (0x0859e280)` twice, with (the current dictionary, the
   element, the node's 16-bit first word, another element, &a buffer pointer, the two
   matrices, the first vector, the second vector, the record, null, the word +0x122): the
   other element is the one step 7 found and then the element itself when the mode is 1,
   the other way round otherwise. A buffer the calculation leaves is given to `free`.
9. A patch model (kind 1) gets `SI_patchUpdateSurface` of its first definition and, when
   different, of its current one.

**What the dumps hang on.** In `octopusII` the two controls (`vctrl1`, algorithm 0x10, and
`rvctrl1`, 0x11) are constrained by 238 to the body; the constraint's orientation
(`constrGetParamOrientation`, si-motion) is the body's kinematic record's linear (+0x30) or
angular (+0x48) velocity, zero when the body has no record. The port's body never had one
(of the update's routines only this one allocates it; the model reader does only with a
dynamics chunk, through `SI_modelKineStateAllocate`), so the controls kept the file's angles
(and the printed ±180°, from the sign of a zero in the matrix). With the record allocated, si-model's `SI_modelKineStateCompute`
(already routed) fills it at every 239 step and the controls turn as the original's. An
experiment that only allocates the record (a stand-in, removed) already made all 90 dumps of
the two `octopusII` scenes identical; the curves of 90 … 93 occur in no sample scene. The
twelve `simple_attractor`/`simple_repulser` dumps, filed as "expression" (the label of the
step that drives their objects), become identical with the same routing; each of the three
scenes has one node of 239. That their expressions read the quick-stretched node's record is
a guess: which value they take was not traced.

**Not ported: `SI_QuickStretchCalculation`** (12,651 bytes: the deformation of the copied
geometry: the vertices through the two vectors, the matrices, `SI_matrixInverse`,
`SI_matrixAxisRotation`, the vertices' get and set, the bounding box, `SI_modelUpdateSonState`
on the root, a `SI_calloc`'d work buffer). `evalScene` prints no vertex, so no dump shows it;
the renderer would.

## 6. The Rust

- `crates/si-camera` (new): `mat.rs` `SI_camMat`; `trans.rs` `SI_addCamTrans`,
  `SI_addCitTrans`; `dof.rs` the five helpers (values); `globals.rs` the globals read
  (`SI_procData`, `SI_elemDicts`, `SI_cid` + 0x20, `SI_frameCurrent`, `SI_frameRate` with
  `globals!`; `SI_identite`, `SI_xAxis`, `SI_yAxis`, `SI_zAxis` stored by `init_vectors`, which
  a loaded image needs: `init` would also write `SI_procData` and `SI_elemDicts` and so must
  not be called after a load); `outside.rs` the five callees in other files as
  `si_mem::outside_calls!` (the first crate on the proposal of finding 0024, section 11.5).
  Math on values is si-math's.
- `crates/si-kine/src/image/qstretch.rs`: `SI_animQuickStretchUpdate`,
  `SI_QuickStretchGetElem`, `SI_modelKineStateAllocate`, `SI_modelKineStateSetDefault`; the
  original's frame as one block of image locals at the original's offsets; eleven routines
  added to si-kine's `outside::routines!` (`SI_QuickStretchCalculation`, `SI_fcurveValue`,
  `SI_getFcvTriplet`, `SI_reldictGetRelChapGrp`, the six copies, `SI_patchUpdateSurface`).
  Where the original reads through a null data pointer (a 239 entry without data) the image
  records the fault.
- Where the port knowingly differs: none in what is compared. A signalling NaN in a
  camera's fields is quieted where the original moves it through the unit (as a real unit
  does; the harness's x87 does not, finding 0011), so the cases with one were left out of
  the vectors (they were the only 72 differences of the first run).

## 7. Checks

**By emulation on dumped sample scenes** (`re/tools/si-camera/scenecheck`,
`camera-scenecheck`; finding 0011's method). `dump` builds each scene as si-emuworld builds
one (the port's loader, stage `dict`), stores si-camera's vectors of `.data`, and writes the
image with an entry per camera record (or, with `--qs`, per update node of 239). The
generators run the original routine on each image with every callee as the binary has it
(nothing stubbed but what is said), `strict`; `check` builds every scene again, holds its
image against the hashes, and for every case puts the image back, stores the case's buffers
(at their fixed addresses: `SI_frameCurrent`, `SI_frameRate`, the camera's nine floats from
+0x10, `SI_glbManip` + 0x0c, `SI_playBackRun`, `SI_frameStart`, the model's +0x2f0), runs the
port with the library's recorded answers (`si_x87::trig::replay`) and the ports of other
crates as callees, and compares every byte of the image both changed and the case's outputs.

| Routine(s) | Scenes | Cases | Identical | Different | `EMUCHECK_MUTATE=1` | `=calls` |
|---|---|---|---|---|---|---|
| `SI_camMat` (`gen_cammat.py`) | 36 | 1,808 | 1,808 (all given the library) | 0 | 1,808 different | 1,808 different |
| `SI_addCamTrans`, `SI_addCitTrans` (`gen_camtrans.py`) | 36 | 864 | 864 | 0 | 828 different (36 change nothing) | 504 different (the cases with library calls; 360 have none) |
| `SI_animQuickStretchUpdate` with `SI_QuickStretchGetElem`, `SI_modelKineStateAllocate`, `…SetDefault` (`gen_qstretch.py`) | 8 | 372 | 372 | 0 | 372 different | 292 different (the cases with stubbed calls; 80 return at the playback test) |

The 36 camera scenes: the eight with relation 1117 and every eleventh scene of the manifest
(`re/tools/si-camera/pick.txt`). Per camera: the manifest's frames on the camera as loaded,
then made-up positions, interests and rolls (random; straight up and down and round the
threshold of |x| + |z|; round the threshold of the squared distance; equal; zero;
infinities; quiet NaNs; huge and tiny coordinates; a NaN, a huge and a π roll; a rate of 0,
a negative and a huge frame). Of the 1,808: 362 through the up-vector constraint (it ran
`acos`), 1,315 through the quarter turn of step 3, 493 vertical or identity. The callees ran
on the image on both sides: si-scene's `SI_edGetElemFromCamera` and `SI_edGetFcvFromRelation`,
si-dict's `SI_elemdictFindRelatedElem` (its search globals `last_index`, `last_relid` are among
the bytes compared) and `SI_elemdictGetCurRelation`, si-motion's `SI_fcurveIntegerValue`.

Quick stretch: the eight scenes whose update list has a 239 node (20 nodes; 13 in
`QSTRETCH-All_Effects_Demo`). Per node: the manifest's frames as loaded (196 cases allocate
the record: the harness's `heap0` is compared with the port's block, and the pointer to it
taken as the same), the same frames with a made-up record already in place, and the playback
test on both sides of its limit. Stubbed in the emulator and compared as recorded calls:
`SI_QuickStretchCalculation` (its stand-in also records the floats of the two vectors and
the two matrices it is handed, so step 5's arithmetic is compared), the six copies,
`SI_patchUpdateSurface`. `SI_relGetFcvType`, which the port reaches through si-motion's
`SI_getFcvTriplet` and nothing has on the image, is answered from Relations.rsrc (90: 221,
91: 224, 92: 227, 93: 230); no sample scene has those curves, so it decides nothing here.

**Values** (`dof.spec`, `camera-emucheck`): the five helpers, 2,000 cases each and 2,000 of
`findcoc` on positive lenses: **12,000 identical**, 0 different; `EMUCHECK_MUTATE=1` 12,000
different.

**si-kine's own checks keep their numbers**: `kine-emucheck` 137,855 / 84,086 / 52,359 / 0
/ 1,410; `--image` 43,145 / 16,251 / 25,832 / 0 / 1,062 (274 files); `kine-corpus` 1,906 files,
1,200 chains, nothing changed; 21 unit tests (si-camera 4).

## 8. Against `evalScene`

Measured on a private copy of `motion-evalcheck` (`re/out/si-camera/evalcheck`, not
tracked; `re/out/si-camera/routing.diff` against `re/tools/si-motion/evalcheck`):

| Run | Identical / last digit / differing / missing | Camera dumps not identical | Quick stretch (octopusII) | attractor/repulser |
|---|---|---|---|---|
| before (`--no-camera --no-qstretch`), stand-ins on or off | 93,140 / 0 / 135 / 0 | 80 | 12 | 12 |
| `SI_camMat` routed | 93,220 / 0 / 55 / 0 | 0 | 12 | 12 |
| quick stretch routed | 93,164 / 0 / 111 / 0 | 80 | 0 | 0 |
| **both** | **93,244 / 0 / 31 / 0** | 0 | 0 | 0 |
| both, `--no-standins` | 93,220 / 0 / 55 / 0 | 0 | 12 | 12 |

The 31 left: inverse kinematics 15 (`DYNATUTOR10-Nail`), wave or cluster 8 (`LumberJack`),
boolean 5, dynamics 3. The quick stretch is reached only through a stand-in of the tool for
the part of `SI_modelReadjustDefinition` above (section 5), so `--no-standins` turns it off;
`SI_camMat` needs no stand-in.

**The routing for the real tool** (`routing.diff`; the package name of the private copy
excepted):

- `Cargo.toml`: `si-camera = { path = "../../../../crates/si-camera" }`.
- `host.rs`, `forward2`: `"cam_mat" if crate::camera::on() => { crate::camera::cam_mat(m,
  counts, i(&a[0]), i(&a[1])); Ret::default() }` before the fall-through to `kine.rs`;
  `"model_readjust_definition" if crate::qstretch::on() && STANDINS… => {
  crate::qstretch::readjust(m, counts, missed, i(&a[0])); … }`; `startup` ends with
  `si_camera::globals::init_vectors(m)` (before the load: its refresh runs the update); a
  public `lookups(counts)` for `Lookups`.
- `camera.rs` (new): the outside world of `SI_camMat` — `SI_edGetElemFromCamera` → si-scene's
  `edtools::ed_get_elem_from_camera`, `SI_elemdictFindRelatedElem` and `…GetCurRelation` →
  si-dict, `SI_edGetFcvFromRelation` → si-scene's `world::ed_get_fcv_from_relation`,
  `SI_fcurveIntegerValue` → si-motion's `fcv::integer_value`.
- `qstretch.rs` (new): the readjust stand-in (section 5); `kine.rs`: `SI_fcurveValue` →
  `si_motion::fcv::value`, `SI_reldictGetRelChapGrp` → si-dict, `SI_getFcvTriplet` →
  `si_motion::utils::get_fcv_triplet` with `SI_relGetFcvType` answered as above, the six
  copies answered with the source (a stand-in: with `SI_QuickStretchCalculation` not
  ported nothing is deformed, so the copy would only be thrown away).
- `main.rs`: `--no-camera`, `--no-qstretch`.

**What si-model's `SI_modelReadjustDefinition` must do for the update** (so that the stand-in
can go): the whole routine on the image, of which the quick stretch's part is the one above;
the update's host then routes `model_readjust_definition` to it, and it calls si-kine's
`anim_quick_stretch_update(m, o, node, t)` with an outside world that answers as `kine.rs`
does now. It also needs `SI_edFindUpdateNode` and `SI_checkActiveTime`, which si-motion has.

**Done (later on 2026-10-14, by the lead).** si-model's eighth pass put
`SI_modelReadjustDefinition` on the image (finding 0015). It is routed now in the tool and
in the renderer's update (finding 0023, section 25), and the stand-in is gone.

- The tool's `qstretch.rs` routes the whole routine, its calls out answered as the
  renderer's update host answers them.
- The six copies `kine.rs` answered with the source now go through
  `si_scene::envside::envelope_call`.
- A face or NURBS model whose working definition is its stored one is left as it is and
  counted: the copy it would need (`SI_faceCopy`, `SI_nurbsCrvCopy`, `SI_nurbsSrfCopy`) is
  on no crate's image. No sample scene reaches it.
- `--no-qstretch` (or `--no-readjust`) answers the routine 0 as before.

`evalScene`: **93,244 / 0 / 31 / 0** with the tool's stand-ins and **also without them**
(was 93,220 without); 93,220 with `--no-readjust`. Still answered 0 inside it:
`SI_QuickStretchCalculation` (232 calls over the corpus, from si-kine) and
`SI_removeTriangle` (116 from the readjusting, 172 from si-model's setters). The
renderer's camera goes through `SI_camMat` the same way (finding 0023, section 25).

## 9. Not ported, not determined

- `sys_camera.c`: 30 of 38 functions are not ported (the menus' and dialogs' routines,
  `SI_setCamView`, `SI_addCamRot` and the displays, the resets and defaults,
  `SI_cameraTransfer`, `SI_camIdConvert`, `SI_camGetDefaultFromId`, `SI_camAspSet`,
  `SI_camAndScnToOrigin`, the shader dialog's two).
- `qstretch.c`: `SI_QuickStretchCalculation` (section 5).
- The fill of the kinematic record from the curves of attributes 90 … 93 is ported but met
  no case (no sample scene has such curves); `SI_relGetFcvType` is answered by a table.
- Signalling NaNs: argued from si-x87, not checked (finding 0011).
- The time `SI_modelReadjustDefinition` computes and the gate in front of the call: no
  longer a stand-in. The routine itself is si-model's port, checked on scenes (finding
  0015, eighth pass: 1,600 of 1,600).

## 10. How to run

```bash
export CARGO_TARGET_DIR=$PWD/target/si-camera
E="uv run --with unicorn python3 -I"
(ulimit -v 8000000; cargo test -p si-camera -p si-kine)
(ulimit -v 8000000; cargo build --release -p si-camera -p si-kine)
(ulimit -v 8000000; cargo build --release --manifest-path re/tools/si-camera/scenecheck/Cargo.toml)
# values
(ulimit -v 6000000; timeout 900 $E re/tools/emu/emu.py gen re/tools/si-camera/dof.spec -o re/out/si-camera/dofvec -n 2000)
(ulimit -v 2000000; [EMUCHECK_MUTATE=1] target/si-camera/release/camera-emucheck re/out/si-camera/dofvec)
# scenes
(ulimit -v 2000000; timeout 900 target/si-camera/release/camera-scenecheck dump re/out/si-camera/scenes --pick re/tools/si-camera/pick.txt)
(ulimit -v 2000000; timeout 900 target/si-camera/release/camera-scenecheck dump re/out/si-camera/qscenes --qs)
(ulimit -v 6000000; timeout 3000 $E re/tools/si-camera/gen_cammat.py re/out/si-camera/scenes re/out/si-camera/vec)
(ulimit -v 6000000; timeout 3000 $E re/tools/si-camera/gen_camtrans.py re/out/si-camera/scenes re/out/si-camera/tvec)
# quick stretch (section 11): the update's cases with SI_QuickStretchCalculation running
(ulimit -v 6000000; timeout 3000 $E re/tools/si-camera/gen_qstretch.py re/out/si-camera/qscenes re/out/si-camera/qvec_calc --calc)
# quick stretch: SI_QuickStretchCalculation called directly, every argument chosen
(ulimit -v 6000000; timeout 6000 $E re/tools/si-camera/gen_qscalc.py re/out/si-camera/qscenes re/out/si-camera/qcvec [--cover re/out/si-camera/qcvec.cover])
for d in vec tvec qvec_calc qcvec; do (ulimit -v 2000000; [EMUCHECK_MUTATE=1|calls] timeout 900 target/si-camera/release/camera-scenecheck check re/out/si-camera/$d); done
# evalScene, with the routing of section 8 applied to a copy of motion-evalcheck
(ulimit -v 2000000; timeout 3000 target/si-camera/release/camera-evalcheck [--no-camera] [--no-qstretch] [--no-standins])
```

`qvec_calc` and `qcvec` replace section 7's `qvec` (`gen_qstretch.py` without `--calc`, the
calculation stubbed). Since section 11 the port runs the calculation that stub stood in for,
so `check` gives `qvec` 80 identical and 292 different, by design; it is no longer run.
`check` picks the port by each case's routine: `qvec_calc`'s cases run the port's
`SI_animQuickStretchUpdate` (`qs::run_case`), `qcvec`'s the port's calculation alone
(`qs::run_calc_case`, `scenecheck/src/qs.rs`).

`re/tools/si-camera/consts.py` prints the binary's words at addresses (float and double).

## 11. The deformation: `SI_QuickStretchCalculation` (0x0859e280)

Later on 2026-10-14, by the lead working alone. Ported in full as si-kine's
`image::qscalc::quick_stretch_calculation` (`crates/si-kine/src/image/qscalc.rs`), which
`SI_animQuickStretchUpdate` (`qstretch.rs`) now calls in place of the outside routine. The
steps, and which values are stored as floats and which stay in the register, are in that
file's doc comments and not repeated here. This section replaces the "Not ported" paragraph
of section 5, the `qstretch.c` line of section 9 and the 232 calls answered 0 of section 8.

**Size.** 12,748 bytes, from 0x0859e280 to `SI_animQuickStretchUpdate` at 0x085a144c (its
`ret` is at 0x085a144b). Section 5's 12,651 is Ghidra's function body, which leaves out the
97 bytes of alignment padding described under "Coverage" below.

**What it does, in short.** Its arguments are `(dict, elem, number, other, &buffer,
matrix1, matrix2, vector1, vector2, kine, -, tagged_only)`, and the update calls it twice
per 239 node. A null `*buffer` gets `SI_calloc(1, 12)`. When `other` is `elem`, that pass
ends there. With another element it deforms one point and keeps it in the buffer.

On the other passes, the four vectors of the kinematic record each drive three of the
twelve groups of the relation data:

- the vectors are the linear velocity (+0x30) and acceleration (+0x3c), and the angular
  velocity (+0x48) and acceleration (+0x54). A velocity below 1e-4 in every component is
  replaced by `vector1` or `vector2`; otherwise it is stretched by them.
- the relation data is `SI_RelQSData`, 0x124 bytes. A group is 0x18 bytes: the enable word,
  the option, three words that leave x, y or z alone, the amount and the threshold.
- the groups pull, bulge, swell and twist. They are set up in the frame of the reference
  node (`other`'s model, else the element's), with its bounding box
  (`SI_modelComputeBoundingEL`) and its scaling clamped to ±1e-4.

Each point of the geometry (`SI_elemGetNbVertices`, `SI_elemGetVertex`) is moved through
the two matrices and, with another element, into that element's frame. It then gets the sum
of the enabled groups' displacements, unless `tagged_only` is set and the point is not
tagged, and is put back (`SI_elemSetVertex`). Both models' roots are first brought up to
date (`SI_modelUpdateSonState`). An enabled group whose option is neither 0 nor 1 prints
"Unexpected default" on `stderr` (`fprintf`) and adds nothing.

In the scene cases, the update always passes the element with itself: the first call only
allocates, and the second deforms every point. This is read from the coverage below, where
no scene case reaches the test of 0x0859e2b2 or any path with another element. That no
father of a quick-stretched model in the samples has quick stretch itself is the likely
reason (a guess, not traced).

**Callees.** The vertex accessors and the box are si-model's (`model_image::points`,
`model_image::bounds`), the root walk is si-model's, and the matrices are si-math's. They
are reached through five new routines in si-kine's `outside.rs`. Two hosts route them the
same way: the renderer's update (`crates/si-rensetup/src/native/motion.rs`) and
`motion-evalcheck` (`re/tools/si-motion/evalcheck/src/kine.rs`). Neither host has a standard
error, so the `fprintf` is counted and not printed.

Both hosts also route `SI_relGetFcvType (0x0810f6c8)` to si-scene's
`dbvsfcv::rel_get_fcv_type` on the attribute dictionary:

- in the tool, it replaces the table of section 7 (90: 221, 91: 224, 92: 227, 93: 230);
- in the renderer, it had answered 0 (80 calls on the first frames). The renderer's comment
  says that without it, `QSTRETCH-All_Effects_Demo`'s record kept the velocities of the
  positions.

**Checks.** There are two kinds of case, both in `camera-scenecheck check`, and both run the
original with every callee as the binary has it:

- **Through the update** (`gen_qstretch.py --calc`, `re/out/si-camera/qvec_calc`): the 372
  cases of section 7, with `SI_QuickStretchCalculation` running instead of being stubbed. The
  six copies stay stubbed but now answer their source instead of 0, so the deformation works
  on the stored definition.
  The port's `SI_animQuickStretchUpdate` calls the port's calculation, and every byte of the
  image both changed is compared, the vertices included.
- **Called directly** (`gen_qscalc.py`, new; `re/out/si-camera/qcvec`; case kind
  `run_calc_case` in `scenecheck/src/qs.rs`). The same eight scenes are used, 300 cases each
  and 600 for `QSTRETCH-All_Effects_Demo`. Every argument is chosen:
  - the pair: the element with itself, another quick-stretched element with any model
    element, a null element or a null other, or an element without quick stretch;
  - the relation data's twelve groups, made up, options other than 0 and 1 included;
  - a null or a filled buffer;
  - the matrices, the vectors and the kinematic record (zero, tiny, of every size, or none);
  - `tagged_only`, with made-up tag bits;
  - a tiny reference scaling, and changed local rotations and translations.

  Words the harness found undefined are left out of the comparison ("identical where
  defined"; none occurred).

| Cases | Scenes | Cases | Identical | Different | `EMUCHECK_MUTATE=1` | `=calls` |
|---|---|---|---|---|---|---|
| through the update (`qvec_calc`) | 8 | 372 | **372** (292 with library calls given or stubbed calls compared) | 0 | not run | not run |
| called directly (`qcvec`) | 8 | 2,700 | **2,700** (1,856 with library calls given or recorded calls compared) | 0 | 2,573 different | 1,856 different (the cases with calls; 844 have none) |

The 127 cases the memory switch leaves identical are those in which the port writes
nothing: no block, no buffer word and no image byte, so the switch has nothing to flip (read
from `run_calc_case`). They are the early returns.

si-kine's own checks (`kine-emucheck`, `--image`, `kine-corpus`) printed the same bytes as
the morning's runs (`re/out/si-camera/qc-kine-*.txt` against `kine-*.txt`).

**Verified on a clean worktree** by the lead (`re/out/lead/qscalc/verify.log`: HEAD
`e897455` with only this work's 16 files copied in):

- the workspace builds with no warnings, and its 1,227 tests pass (si-kine 22, one of them
  new; si-rensetup 23);
- `camera-scenecheck check`: `qvec_calc` 372 of 372 identical; `qcvec` 2,700 of 2,700, with
  2,573 different under `EMUCHECK_MUTATE=1` and 1,856 under `=calls`;
- `kine-emucheck`, `kine-emucheck --image` and `kine-corpus` print byte for byte what they
  printed before;
- `motion-evalcheck`: 93,244 of 93,275 identical, unchanged;
- section 7's stubbed set `qvec` now gives 80 identical, 292 different. This is by design:
  those cases stub `SI_QuickStretchCalculation`, and the port now runs it (a typical first
  difference: the port calls `sin`, where the original, its calculation stubbed, made no
  further call). `qvec_calc` replaces it.

**Coverage** (the harness's `cover`: the bytes of the routine in no block the emulator
reported, across all cases and scenes):

- **Through the update:** 6,809 of 12,748 bytes are never reached, in 52 runs (the 19:03
  run: `gen_qstretch.py --calc`'s 372 cases with the harness's `cover`, in scratch). The
  earlier 5,739 bytes in 20 runs (`re/out/lead/qs_cover.txt`, 18:45) was wrong: it counted
  as reached the `other` ≠ `elem` set-up at 0x0859e988, a branch no scene case can take.
  What these cases miss:
  - every path with another element (the null-other test, `other`'s element, the second
    root, the frame changes there and back);
  - the two accelerations' set-up and per-point blocks (no case has a positive acceleration
    with one of its groups enabled);
  - the scale clamps;
  - the first pass's one point;
  - the tagged-only test;
  - the "Unexpected default" messages.
- **With the direct calls:** 97 bytes in 22 runs (`re/out/si-camera/qcvec.cover`). These
  bytes are all alignment padding, which no flow can execute:
  - each run is one filler instruction (`nop`, `mov esi,esi`, `lea esi,[esi+0]` or
    `lea esi,[esi+eiz*1+0]`, 1 to 7 bytes);
  - each sits directly after an unconditional `jmp` (22 of 22) and ends on a 16-byte
    boundary, the label the code jumps to (22 of 22);
  - no branch of the routine, and no `jmp` or `call` anywhere in the binary's disassembly,
    targets a byte of them;
  - Ghidra's body (12,651 bytes) is the 12,748 less exactly these 97.

  | Run | Bytes | Filler | After the `jmp` at | Next label |
  |---|---|---|---|---|
  | 0x0859e9f9 | 7 | `lea esi,[esi+eiz*1+0]` | 0x0859e9f7 | 0x0859ea00, the linear velocity's set-up in the element's own frame |
  | 0x0859ee7d | 3 | `lea esi,[esi+0]` | 0x0859ee7b | 0x0859ee80, the angular velocity's set-up in the element's own frame |
  | 0x0859f389, 0x0859f3e9, 0x0859f439 | 7, 7, 7 | `lea esi,[esi+eiz*1+0]` | 0x0859f387, 0x0859f3e7, 0x0859f434 | the ±1e-4 clamp of `other`'s scaling, x, y, z |
  | 0x0859f48e, 0x0859f4d9, 0x0859f529 | 2, 7, 7 | `mov esi,esi`, `lea …` | 0x0859f48c, 0x0859f4d7, 0x0859f527 | the clamp of the model's scaling, x, y, z |
  | 0x0859f59f | 1 | `nop` | 0x0859f59d | 0x0859f5a0, the points of a pass that did not allocate |
  | 0x0859f9dc, 0x0859fb4d, 0x0859fcef | 4, 3, 1 | `lea`, `lea`, `nop` | 0x0859f9da, 0x0859fb4b, 0x0859fced | per point, linear velocity: group 0; group 1; group 1's `fprintf` |
  | 0x0859ffad, 0x085a011d, 0x085a02bf | 3, 3, 1 | `lea`, `lea`, `nop` | 0x0859ffab, 0x085a011b, 0x085a02bd | linear acceleration: group 3; group 4; group 4's `fprintf` |
  | 0x085a036a | 6 | `lea` | 0x085a0368 | linear acceleration: group 5, option 1 |
  | 0x085a070e, 0x085a09ca | 2, 6 | `mov esi,esi`, `lea` | 0x085a070c, 0x085a09c8 | angular velocity: the `fprintf`s of groups 6 and 7 |
  | 0x085a0c6a, 0x085a0efa, 0x085a0fcf, 0x085a10b9 | 6, 6, 1, 7 | `lea`, `lea`, `nop`, `lea` | 0x085a0c68, 0x085a0ef8, 0x085a0fca, 0x085a10b7 | angular acceleration: group 9, option 0; group 10 (two places); group 10's `fprintf` |

  Every instruction of the routine therefore ran in some case. One caveat: the harness's
  `cover` is an upper bound of what ran (its doc: a reported block may run on past a taken
  branch). A count of block entries, which cannot over-count, was made once in scratch
  (`qc_paths.py`). It found every one of 30 named paths and all 12 `fprintf`s entered: the
  paths by 49 to 1,423 of the 2,700 cases, and the messages by 21 to 43.

**Against `evalScene`** (`motion-evalcheck`, `re/out/si-camera/qc-eval`): **93,244 / 0 / 31
/ 0**, unchanged. `evalScene` prints no vertex. The 232 calls of
`SI_QuickStretchCalculation` that were answered 0 are gone from the list of routines not on
the image; nothing else in it changed.

**The pictures, first frames** (`render-frames`, `RENDER_FIRST_ONLY=1`, against the
oracle; `re/out/si-camera/pics/oracle_{before,after2}.tsv`, `before` = HEAD):

| Run | Identical | Different pixels | Identical but the icon | No port picture |
|---|---|---|---|---|
| HEAD | 235 | 36 | 17 | 8 |
| with the deformation and `SI_relGetFcvType` routed | **241** | 30 | 17 | 8 |

None is worse. Six scenes became identical:

| Scene | Pixels that differed at HEAD |
|---|---|
| `EXPRESSIONS-simple_attractor.1-0` | 125 |
| `EXPRESSIONS-simple_attractor.2-0` | 579 |
| `EXPRESSIONS-simple_repulser.1-0` | 506 |
| `ACTOR-octopusII.1-0` | 30,084 |
| `ACTOR-octopusII.2-0` | 30,084 |
| `QSTRETCH-All_Effects_Demo.1-0` | 366 |

`QSTRETCH-All_Effects_Demo.1-0` became identical only with `SI_relGetFcvType` routed: in
the run without it (`oracle_after.tsv`), 7,876 pixels differed.

Other changes in the run's tallies (`check_before.txt`, `check_after2.txt`):

- the 40 calls of the calculation answered 0, in each of the two update tallies, are gone;
- `rel_get_fcv_type` is answered 80 times;
- 20 more `SI_modelUpdateSonState`;
- 20 more blocks freed at the renderer's exit.

**Triangles filed:** 574,124 → 574,054. This is `SI_stats` + 0x00, the triangles that
passed the window test (si-math's `YBins::accept`). Two scenes account for all of it:

| Scene | Triangles filed at HEAD | Now |
|---|---|---|
| `EXPRESSIONS-simple_attractor.2-0` | 156 | 126 |
| `EXPRESSIONS-simple_repulser.1-0` | 156 | 116 |

Both scenes' pictures went from different to identical. `QSTRETCH-All_Effects_Demo` files
2,808 at HEAD and now. In the run without `SI_relGetFcvType` it filed 2,768, with the wrong
picture.

The original's count is not in the tables. The oracle compares pixels only, and the
original's `-R` runs print no render statistics (`re/out/oracle/runs/…/log.txt`). The pixels
cannot decide it either, because a triangle that fails the window test leaves no pixel in
either program. That the deformed points take 30 and 40 triangles out of the window is
therefore a plausible reading, not a measured one. Reading `SI_stats` + 0x00 (0x087911e0)
after the original's render, in the in-process harness of finding 0031, would decide it.

**The pictures, every frame:** every frame of every scene's range, compared with all 25,507
of the oracle's frames (as `all_frames.sh`, HEAD `e897455` then the tree with this work,
measured by the lead): 21,286 → **21,882 byte-identical**. HEAD's own run gives the 21,286
committed at `5eea4e9`. Scenes identical on every frame: 226 → **231**. None worse: frame by
frame, the only change is 596 frames that went from different pixels to identical.

| Scene | Identical frames, HEAD → now |
|---|---|
| `EXPRESSIONS-simple_attractor.1-0` | 0 → 100 of 100 |
| `EXPRESSIONS-simple_attractor.2-0` | 0 → 100 of 100 |
| `EXPRESSIONS-simple_repulser.1-0` | 0 → 100 of 100 |
| `ACTOR-octopusII.1-0` | 0 → 100 of 100 |
| `ACTOR-octopusII.2-0` | 0 → 100 of 100 |
| `QSTRETCH-All_Effects_Demo.1-0` | 0 → 96 of 100 |

The four frames of `QSTRETCH-All_Effects_Demo` still different (4, 5, 7 and 8) differ in 2 or 3
of their 212,000 pixels, each by 1. The rest of the run is unchanged: 940 frames with no port
picture, 17 identical but the icon, 15 with different lines. Different pixels went from 3,249
to 2,653. The tables are `re/out/si-camera/pics/frames_after.{sum,tsv,scenes.tsv}`, and HEAD's
are `frames_before.*`.

**How to run:** section 10's commands, which now generate and check `qvec_calc`
(`gen_qstretch.py --calc`) and `qcvec` (`gen_qscalc.py`; `--cover FILE` writes the bytes no
direct call reached) in place of the stubbed `qvec`. The coverage through the update has no
tracked option: it was taken in scratch, by a wrapper that runs `gen_qstretch.py --calc` with
the harness's `cover`.

## 12. Whether quick stretch is on: `SI_quickStretchActive` (0x085a23c0)

2026-10-14, by agents run one at a time for the lead. 96 bytes of `qstretch_misc.c`, ported
in si-kine as `image::qstretch::quick_stretch_active` (`qstretch.rs`). The update asks it
before it takes a model as not deformed. The steps:

1. The model's element, from `SI_edGetElemFromModel(model, &index)` (the index into a
   local). No element: 0.
2. The forward entry of relation 239 that names that element itself:
   `SI_elemdictGetRelationFromIds(dictionary, element, 239, the MODELS chapter, index, -1)`.
   The dictionary is the current one, `SI_procData[0] × 0x70 + SI_elemDicts`. No entry: 0.
3. Otherwise the entry's 16-bit word at +0x14, sign-extended.

The look-up is `si-kine`'s new outside routine `SI_ELEMDICT_GET_RELATION_FROM_IDS`
(0x08134e9c). Both hosts answer it with si-dict's `elemrel::get_relation_from_ids`: the
render's update (`native/motion.rs`) and `motion-evalcheck` (`kine.rs`). The render's update
host routes `quick_stretch_active` to the port (finding 0023 §28); before, it was answered 0.

**Checks.** `re/tools/si-kine/emu_image.py`'s new target `SI_quickStretchActive` makes 1,000
cases on made-up scene worlds. Some models have no element. The stand-in for the look-up
answers null now and then; otherwise it answers one of the element's four made-up entries,
whose word +0x14 is chosen from 0, 1, 2, 0xFFFF, 0x8000 and 0x7FFF, with a made-up word
above it that must not be read. `kine-emucheck --image`:

| Vectors | Cases | Identical | Different | `EMUCHECK_MUTATE=1` | `=calls` |
|---|---:|---:|---:|---:|---:|
| `re/out/si-kine/vec/SI_quickStretchActive.vec` | 1,000 | **1,000** | 0 | 1,000 different | 1,000 different |

The whole of `re/out/si-kine/vec`, now 395 files: 138,855 cases, 0 different, both without
and with `--image`. Every other function's row is byte for byte that of section 11's runs.

**Effect.** In the first-frame run the update asks it 31 times. No picture changed in finding
0023 §28's runs: the three quick-stretch scenes stay identical on every frame (100, 1 and 100
frames).

Verified on a clean worktree of 34bdd92 with only these files
(`re/out/lead/shapes/verify.log`): no warnings; 1,230 tests pass, one of them new in
si-kine.

```bash
(ulimit -v 6000000; uv run --with unicorn python3 -I re/tools/si-kine/emu_image.py -o re/out/si-kine/vec --only SI_quickStretchActive)
(ulimit -v 2000000; target/release/kine-emucheck --image re/out/si-kine/vec)   # EMUCHECK_MUTATE=1 / =calls: every case differs
```
