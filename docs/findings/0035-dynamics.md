# 0035 — Dynamics: collisions (`collisions.c`) in si-dynamics

Date: 2026-10-28. Binary: Linux `soft`, sha256 `ef21a382c3325d42…`. Package 26 (`crates/si-dynamics`,
`re/tools/si-dynamics/`, scratch `re/out/si-dynamics/`).

## Where it stands

`collisions.c` complete (19, `collisions.rs`); `contact.c` complete (4: `contact.rs`, `contact_bbox.rs`); `invdynamic.c` complete
(10 of 10): `invdynamic.rs` (CheckPtFanEffect, BboxFaceFanFrce, ConstrFrce), `invdynamic_fan.rs` (BboxFanFrce 0x0849a0cc),
`invdynamic_wind.rs` (BboxWindFrce 0x0849b9ec), `invdynamic_mesh.rs` (MeshWindFrce 0x0849c3f0), `invdynamic_node.rs`
(NodeFanFrce 0x0849b730, si_invdynaGetFanFrce 0x0849b844, NodeWindFrce 0x0849ccbc, si_invdynaGetWindFrce 0x0849cdc8);
`dynamic.c`: 16 of 17 in `dynamic.rs` (si_modelDumpDynaInfo is a printf dump, left) (the six integrators plus PrepareSimulationData 0x08496d88, EvaluateCollisions 0x0849727c, SimulationStep 0x08499290, SimulationFrame 0x0849934c, mymodelBranchUpdate 0x084969a8, dynaEvaluateConstraints 0x08496f94, dynaFastbandInbound 0x084981f8, dynaFastbandOutbound 0x084988f4, dynaSlowbandInbound 0x0849739c; environmentAnimationPreUpdate 0x084968f8 ported, only its no-animation path checked). Checks run the original in the x87
emulator on random vectors (`re/tools/si-dynamics/run.sh gen|check`, `chk.sh NAME` prints plain / `=1` / `=calls`
as identical/cases, different, not compared). 0 different anywhere. Per function (1500 cases unless noted):
- all identical: resolveLinearSystemRG, clipt, lineBoxLiangBarskyClipping, surfaceOfContactComputeArea, checkBsphBplnCollision,
  detectDynaBboxcollision, contactInitialSpeedArrange, resolveCollision, si_resolveMultipleCollision, booleanCollisionTest,
  si_vectorClean, the dynamic.c ports, linePolygonClipping, invdynaGetBboxFaceFanFrce, invdynaGetBboxFanFrce,
  invdynaGetBboxWindFrce, invdynaGetMeshWindFrce (all 1500/1500).
- invdynaGetNodeFanFrce 1241 / NodeWindFrce 1251 / si_invdynaGetFanFrce 1217 / si_invdynaGetWindFrce 1249 identical, 0
  different; the rest not compared (the original reads a null model: a kind-5 node with a model-1 kind other than 2
  makes the box routine read mdl2 = 0). Mutation `=1`: every compared case with an output differs (BboxFan 653, BboxWind
  1242, Mesh 807, NodeFan 564, NodeWind 869, wrappers all); `=calls`: Mesh 791, NodeWind 138, wrappers 939 / 924
  differ (the routines without a reachable hook: not applicable).
- si_dynaEvaluateContactStability 1371 + 129 defined; SphereSupport / BboxSupport see the older lines in the ledger of
  this finding (identical or defined, 0 different; not compared where the original stops).
- invdynaCheckPtFanEffect 1208 + 250 defined + 42 nc; si_invdynaGetConstrFrce 1500/1500 (dyn7: the real si_dynaLinearSpeedCompute now ported, spec gives it a state).
Lessons: (1) kit x87 mislabels a register base after a join: check loads at jump targets against `kit dis --raw`. (2) hand-
written stores in a template must assign the x87tpl captures; captures assigned on one path only are stale on the other:
`@pre` in the template re-assigns them all (x87tpl.py now also adds phi expressions to the capture analysis). (3) `gen X f32`
fills element 0 of an array only: use `rec.*` with integer range text. (4) the harness lists only buffers the routine
modified, so low coverage reads as "identical": check with the mutation counts. (5) `#variant` vectors hang the checkkit; the
aliases mdl2 == mdl3 / R0 stay ungenerated (coverage gap). (6) a SPECGEN_VECOUT stand-in must give the same vector in both
passes, whatever the number of calls: specgen_globals.py now seeds it from the case and the call's first two arguments.
Hooks (`DynaCalls`, outside.rs): SI_matrix*/SI_mesh* callees, SI_computeBoolean, SI_modelComputeBounding (si-model, no dependency),
si_constraintPositionGet and OrientationGet, SI_meshPolygonCentroid / VolumeSurface, answered from the vector.
- dynamic2.spec (drivers): mymodelBranchUpdate, PrepareSimulationData, SimulationStep, SimulationFrame 2500 / 2500 / 2000 / 1500 identical, 0 different; `=1` differs in all but the cases that change nothing (255 / 242 of the first two, 0 of the others); `=calls` all differ (Frame: 104 cases with no call). EvaluateCollisions 1500/1500 but only the walk and sub-kinds that call nothing are generated (sub-kinds 1-3 call the ported resolvers; mutations flag nothing there). The fcurve branch of environmentAnimationPreUpdate (SI_fcurveValue in st0) is not generated.
dynaEvaluateConstraints 2500/2500 (kinds 0/10 with the orientation hook, dynamic2.spec; kind 4 records 2500/2500, dynamic3.spec; the kinds 13/6/7 only with their guards off), dynaFastbandInbound 2000/2000 (dynamic4.spec, no stand-ins; `=1` all 2000 differ; the calculeReaction branch is not generated: it needs settings short +0x38 or a successor body). The out-argument kind of a stand-in is per file (`outside` lines are global): files per kind.
dynaFastbandOutbound 2500/2500 (dynamic5.spec), dynaSlowbandInbound 2500/2500 incl. flag != 0 (obj null: resolver sub-kinds 1-3 and non-null entries still ungenerated) and the model == fc variant `_self` (SPECGEN_ALIAS, mkspec_slowself.py) 2500/2500; `=1` all differ.
settings.c (settings.rs, settings_geom.rs, settings_rec.rs, settings_dyn.rs) 43 of 51 (dyn9 added 16, see below); 27 of 51 before: the dyn7 15 plus (dyn8, 1500 cases each, 0 different, `=1` differs) chnvarSetDefault, jntvarSetDefault, jntcteCompute, chncteCompute, matterCopy, jntpropCopy, chnvarCopy, ObstacleBoundingUpdate, AlgoTypeSet, SetChnListPtrs, HierarchyCheckEmptyPhyProps, HierarchyCheckPhyPropsConsistancy; deltat/chncte/jntvar/jntcte are now direct calls in dynamic.rs (hooks removed).
si_modelDumpDynaInfo 0x08496254 is a printf-only debug dump: not ported.
Left (env1 2026-10-29: playback, dyn_constr bounding and existence now checked, see the last section): settings.c (8 of 51), dyn_constr GL draw routines, the rest of dynamicio; route-stability and SI_modelComputeBounding (settings_geom.rs) still hooks.
Tools (kit candidates): x87tpl.py + regen.py (x87 listing to Rust template expander; `@pre`, phi captures, f32 globals), bfan_prep.py /
bwind_prep.py + bfan.tpl / bwind.tpl, mkspec_nodes.py (model-graph spec generator for the node walkers), markuninit*.py, specgen_globals.py
(pokes globals; SPECGEN_FANMDL=2 sets wMdl / wAmpl / wDir), dbgbreak.py, addglobals.py, gen_multi.py, gen_fcs.py, c2rs.py, bool.spec, dynamic.spec, contact*.spec,
invdynamic.spec, invdynamic_node.spec (generated).

## 2026-10-28 — what was done

Read from the code (decompile plus disassembly, the x87 association of every float expression checked against the
instructions): all 18 ported functions. Guess: the field names of the model / contact records (e.g. the
surface-of-contact record: count, points, floats) are ours; layouts come from offsets used.

Lesson worth keeping: vector globals added with `addglobals.py` are only seen by the port; the emulated original
reads the program image. A global such as the alternating `si_firstStepOfDynamicFrame` (0x086d7140) therefore
never reached the original, and resolveCollision looked wrong for its first-step blocks while the arithmetic was
fine (hand-simulated x87 stack matched). `specgen_globals.py` pokes fixed-address globals into every case before the
original runs (`SPECGEN_GLOBALS="0x086d7140:u16=0|1"`); `gen_multi.py` builds the pointer-array records of
si_resolveMultipleCollision that a specgen block cannot hold.

Tools in `re/tools/si-dynamics/`: c2rs.py and draft.py (first Rust draft from the decompile), x87expr.py (x87
expression association), st0_to_eax.py (st0 results compared as eax), addglobals.py, specgen_globals.py,
gen_multi.py, run.sh. Known tool faults: the x87 tool conflates memory slots and has a register bug at 0x084a6910;
`kit dis` truncates function tails; Ghidra's x87 association is unreliable.

## 2026-10-28 (dyn7) - settings.c leaves and hierarchy bounding

Read in full: the x87 order of every function below (kit x87 + dis); ported with 80-bit sums and f32 stores where the original stores.
- Wired `si_dynaLinearSpeedCompute` into collisions.rs and invdynamic.rs (trait hook removed); ConstrFrce 1500/1500 (was 211 + defined).
- `dynaSlowbandInbound` now 2500/2500 with flag != 0 (the entry record's object word is null; before it was random garbage) and the
  `_self` variant (model is its own first body, via SPECGEN_ALIAS). Not generated: resolver sub-kinds 1-3, non-null entries.
- Ported (1500-2500 cases each, 0 different; `=1` differs in all cases with an output): si_dynachnDeltatCompute 0x0849e0f0,
  inertiaTensorElemBoxFactors 0x0849e13c, inertiaTensorElemBox 0x0849e2d8, inertiaTensorTranslate 0x0849e438,
  modelBoundingBoxTransfer 0x0849e4dc, hierarchyBoundingCompute 0x0849e808, hierarchyNodeOverallBoundingCompute 0x0849e850,
  hierarchyOverallBoundingCompute 0x0849ebc4, hierNodeOverallInertiaTensorCompute 0x0849ec10, hierarchyOverallInertiaTensorCompute 0x0849ef7c.
- Original bug kept: hierarchyOverallInertiaTensorCompute stores the second tensor's off-diagonals into the first tensor's
  words (+0xc, +0x18, +0x1c) after filling them with the first sums. Checked against the emulated original.
- Guess/limit: SI_modelComputeBounding is a do-nothing hook in the spec (its effect on the model box is not covered).
- Lesson: constants read from the image (0x08660b30..) must be added to every vec of a function that reaches them, not only the leaf's.
- Full run of re/out/si-dynamics/vec: 148040 cases, 133683 identical, 4302 given-lib, 2314 defined, 0 different, 7741 not compared.

## 2026-10-28 (dyn8)

Read from the listings and checked against the original in the emulator (specs `settings4.spec`, generator `mkspec_rec.py`).
- main's `cargo check --workspace` failure was an unimported `x(..)` in dynamics-checkkit (now `si_x87::x`).
- si-dynamics now depends on si-model, si-scene, si-types (no cycle today; one would appear if si-scene ever needs si-dynamics).
- SI_dynachnvarCopy keeps an original bug: its two 3x3 matrix copies at +0x70 and +0x94 copy from the destination record into the source.
- SetChnListPtrs allocates with SI_malloc, which marks the first byte 0x6f and the last 0x6b; plain malloc shows as a false difference. Only the first child gets rec+0x25c: the sibling loop jumps past the store.
- Prepare spec: a null model head faults in Rel2Abs in the original, so the head is always set.
- Spec pitfalls: do not name blocks `p_*` (collides with pointer arguments); count layout padding words against the offsets; zero unblocked pointer fields with `gen X.f u32 choice=0`; SPECGEN_ALIAS builds model trees.
- Guess: the two HierarchyCheck routines are reproduced from the listing; their callers are not ported, so the meaning of the flag argument is inferred.
- Not done: the rest of settings.c, dyn_constr, dynamicio, playback; the SI_modelComputeBounding hook.
- Tool: `dbgfault.py` prints eip and address of unmapped accesses while generating vectors.

## 2026-10-29 (dyn9)

Specs `settings5.spec`, `constr.spec`, `play.spec` (generators `mkspec_dyn.py`, `mkspec_constr.py`, `mkspec_play.py`); every function 1500 cases, 0 different. `=1` / `=calls` are identical out of 1500.
- settings.c, 16 more (`settings_dyn.rs`): [soft 0x0849f40c SI_modelKineStateRecordState] (460/460), [soft 0x0849fa88 SI_dynajntvarAllocate] (0/1500), [soft 0x0849fad8 SI_dynachnvarAllocate] (0/1500), [soft 0x0849fb58 dynageometryTransfer] (865 plain, 635 defined), [soft 0x084a02b8 SI_modelVolume] (0/991), [soft 0x084a04bc dynaGeometryModelUpdate] (90/1173), [soft 0x084a072c dynaGeometryHierarchyNodeCompute], [soft 0x084a08a8 dynaGeometryHierarchyCompute], [soft 0x084a0920 SI_dynaGeometryUpdate] (1147, 353 defined), [soft 0x084a0b30 SI_dynachnGeometryUpdate], [soft 0x084a10c4 SI_dynaLinkRootToGravity], [soft 0x084a1160 SI_modelDynaInfoCopy] (850, 650 defined), [soft 0x084a1248 SI_modelDynaInfoDispose], [soft 0x084a15a4 SI_dynaPhyPropsSetNewRoot], [soft 0x084a16f8 SI_dynaPhyPropsValidateCopyDelete], [soft 0x084a1794 SI_dynaPhyPropsValidateParent]. The five hierarchy bounding/inertia routines were re-run (1500 each, 0 different).
- dyn_constr.c (`constr.rs`), 14 ported: [soft 0x084957cc si_conslist_find_priority], [soft 0x0849585c si_conslist_find_prev_constraint], [soft 0x084958d0 si_conslist_constr_allocate], [soft 0x08495a98 si_conslist_constr_complete_allocation], [soft 0x08495ad8 si_conslist_add_constraint], [soft 0x08495b74 conslist_dispose_one_constraint], [soft 0x08495c20 conslist_dispose_constraint], [soft 0x08495d90 si_conslist_dispose], [soft 0x08496060 si_dyna_constraint_relax_from_data], [soft 0x084960cc si_modeldyna_constr_list_clean], [soft 0x08495e5c si_dyna_constraint_get], [soft 0x08495544 si_constraint_position_get], [soft 0x08495624 si_constraint_orientation_get], dyna_def_constraint_check_existence (0x08495dfc, ported, not checked: needs a graphics-list chain). Mutation `=1` / `=calls` differ in all compared cases except where the argument or call cannot matter (void or no call). Not ported: the GL draw routines and SI_dynaConstraintBounding (0x08495384).
- dyn_playback.c (`playback.rs`): [soft 0x0849d3b0 SI_dynaSimulationCheckExistence], [soft 0x0849d514 simulationRemoveElemFrames], [soft 0x0849d760 SI_dynaSimulationDispose], [soft 0x0849d8f8 SI_dynaTransitionCrvDispose], [soft 0x0849da48 SI_simseginfoAddTransition], [soft 0x0849daa0 fcvFindKeyFrame], [soft 0x0849db20 SI_simulatedValuesGet]; SI_simseginfoRecordTransition (0x0849da08) is partial. Not ported (many hooks): simulationRawFcrvSaveElemFrame (0x0849ddb8), SI_simulationRawFcrvSaveKeys (0x0849e030, still a hook). SI_frameToSec is computed natively (frame / SI_frameRate).
- dynamicio: [soft 0x08499460 odynamic_write_binary_chunk] (1500 identical, `=calls` 611).
- Lessons: (1) SI_malloc marks the first byte 0x6f and the last 0x6b, and uninitialised heap bytes stay arbitrary: these give "defined" instead of "identical" cases. (2) A zero-argument outside routine must be declared with kind `u`. (3) Every global the original writes must be poked with SPECGEN_GLOBALS (single words, `ADDR:u32=V0|V1`), otherwise the case is not compared (`GW:` note). (4) Count layout offsets with the model's real size (the `PM` alg word is at 0x2ec). (5) The checker compares eax/ax only, not st0.
- Routing debt (settled by routing5, 2026-10-29, see the last section): the DynaCalls hooks whose targets exist now (si_constraint_position_get, si_constraint_orientation_get, si_dyna_constraint_get, si_conslist_constr_allocate, si_conslist_add_constraint, si_conslist_dispose, simulation and transition-curve dispose) are still outside calls; the app route table must map them to constr.rs / playback.rs.
- Full run, vec: 220040 cases, 207091 identical, 4302 given-lib, 3952 defined, 0 different, 4695 not compared. The stale directory vec6f (generated 22:07, before the vec6s regeneration) shows 69 different in dynaSlowbandInbound; vec6s and vec both show 0 different for it. Over all five dirs: 224340 cases, 211091 identical, 69 different (all in vec6f).

## 2026-10-29 (routing5): the DynaCalls hooks are wired

`crates/si-dynamics/src/routed.rs` (`Routed`, `ROUTED`) wraps an `Outside` and answers 14 addresses with this crate's own ports,
forwarding the rest. This is wiring, it reproduces no function. The 8 asked for: 0x08495544 si_constraintPositionGet,
0x08495624 si_constraintOrientationGet, 0x08495e5c SI_dynaConstraintGet, 0x084958d0, 0x08495ad8, 0x08495d90 (conslist allocate,
add, dispose), 0x0849d760, 0x0849d8f8 (the two disposes); plus Rel2Abs, Abs2Rel, JointsUpdate and the three band transfers.
Env `DYNAMICS_NO_ROUTING` switches it off, `DYNAMICS_ROUTING_SKIP=0xADDR,..` skips single addresses.
- Proof: the old dynaEvaluateConstraints vectors recorded stand-ins for the orientation callee, so they were regenerated with the
  spec minus its `outside si_constraintOrientationGet` line (the real callee runs; `p2bc` block enlarged to 130 floats, the first
  try read past the block). `DYNAMICS_ROUTED=1` in dynamics-checkkit lets position, orientation, Rel2Abs, Abs2Rel, JointsUpdate
  and fastband outbound run as ports: 7500 of 7500 identical, 0 different; `DYNAMICS_ROUTED=mut` (flips one output bit) makes
  5931 differ. Old stand-in vectors: 1000 of 1000 identical. Specs in `re/tools/routing5/`, vectors in `re/out/routing5/`.
- Not proven by an integrated check: conslist*, DynaConstraintGet, the disposes, slowband inbound (they allocate or call libc);
  only the dispatch test and their own port vectors.
- Existing dirs vec, vec4, vec6f, vec6s: 224340 cases, 69 different, all dynaSlowbandInbound in the stale vec6f, as before.
- si-dynamics has no dependents, so this cannot change a rendered frame.

## 2026-10-29 (env1): playback leftovers and dyn_constr checked

Read from code (listings, 1500 cases each, `chk.sh`-style plain / `=1` / `=calls`; vectors in `re/out/env1/vec`):
- `SI_simseginfoRecordTransition` (0x0849da08): listing read, already ported; 1500/1500 identical, 0 different; `=1` 272 differ (the rest change nothing); no calls. Cited in full form.
- `simulationRawFcrvSaveElemFrame` (0x0849ddb8), new in `playback.rs`: 1500/1500 identical, `=1` 1011 differ (489 change nothing), `=calls` 1500 differ.
- `SI_simulationRawFcrvSaveKeys` (0x0849e030), new in `playback.rs`, wired in `routed.rs`: 1418 identical, 0 different, 82 not compared (the original faults on a null simulation record at +0x304); `=1` 1418 and `=calls` 1418 differ. A rotation key time is 57.29577951308232 (double at 0x08660ae0) times the float, rounded once to float.
- New `DynaCalls` hooks, answered 0 until wired: `si_elemdict_find_related_elem` (0x0813508c), `si_model_anim_plot_trajectory` (0x0820d9e0, unported, mot_plot), `si_fkey_create` (0x0820b248), `si_fcurve_add_key_after` (0x081ff588).
- `SI_dynaConstraintBounding` (0x08495384): 2500/2500 identical, `=1` 1684 differ (816 write nothing), `=calls` no calls. Cases 5/6/8, 16/17, 9, 7, 10; 11 to 15 write nothing. The larger length is stored as a float and reloaded before the divisions.
- `dynaDefConstraintCheckExistence` (0x08495dfc): 1500/1500 identical (graphics-list chain world, `SPECGEN_CEXIST=1`); `=1` 1500 differ; no calls.
Guesses: none left in these. The GL draw routines of dyn_constr stay unported (display only).
Tools: `mkspec_play2.py` + `play2.spec` + `genplay2.sh` (the `pbworkFkey` global 0x086d7144 is a case buffer: `SPECGEN_PBWORK=always|mixed` in `specgen_globals.py`);
`mkspec_constr2.py` + `constr2.spec` + `genconstr2.sh` (`SPECGEN_CEXIST=1` builds SI_procData / SI_grafLists with a node chain). dynamics-checkkit runs a real
`SI_fkeyCopy` (malloc 0x24, defaults, ten fields) for the two save entries (thread-local `NATIVE_FKEY`), as the original does. Avoid signalling NaNs in generated words
(the emulator keeps the payload where hardware quiets it).

## Leftovers pass, 2026-10-29 (unit misc2)

Remaining unported in si-dynamics: `dyn_constr.c` drawing (`SI_dynaCdmDraw`, `drawCharacter`, `drawFleche`, `drawQSControl`, `drawFan`, `drawNail`, `SI_dynaConstraintDraw`), `si_modelDumpDynaInfo` (0x08496254, no caller in the binary), `particleGetDg/particleSetDg/si_dialogParticle`, `si_dialogWave`: drawing, dumps and dialogs; skipped as not render. `si_waveCreateIconMdl` (0x084c61b8) builds the wave's icon model; it is reproduced in si-scene (`wave.rs`, cited there in the bare form, so the ledger lists it as mentioned). Changing that citation to the full form is an edit in si-scene; the function is an editor icon and not render.
