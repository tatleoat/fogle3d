# 0045 Particles and waves (si-dynamics)

## Where it stands

Ported in crate si-dynamics (files particle_*.rs, wave_*.rs); every number below is identical / cases, 0 different anywhere, and
=1 / =calls changed every function that writes or calls (details in the dated sections and in the spec files).
- particle_mf (0x084c4590..0x084c48d8) vector/matrix helpers; particle_data (12 functions 0x084c2f30..0x084c3f64); particle_init,
  particle_fcrv, particle_mrg (SI_initParticle 40/40); particle_util: five originals 300/300 each, AddPre/PostAnimHideKeys and
  MakePeriodic 600/600, DataAllocate/Free, MeshToTri 792/800 (8 fault in the original), ComputePaths 800/800, particleWriteData
  323 + 77 given-lib of 400, DuplicateMesh 600/600; si_particleMaterialCopy not checkable (malloc addresses).
- particle_build: particleCreateHrcAnimation [soft 0x084c0e24] 559/600 + 23 given-lib + 6 defined, particleCreateShapeAnimation
  [soft 0x084c1de8] 566/600 + 20 + 3 (point mode, no materials exercised; spec particle_crt.spec).
- wave_fcrv (10 functions) 600/600 each (two have faulting cases); wave_init: SI_initWave, si_waveAllocate, si_waveInitHorizProf
  600/600, si_waveAmpCreate 486/600; wave_util 876..900/900 (waveFreezeMdl 876/900); wave_mn: SI_waveFreeze 574/600,
  SI_waveDetachItem 565/600, particleHandleHideKeys 600/600; waveGetNextWave 400/400, modelGetWaveName 200/200.
- Wave files (wavefile.spec, 300 each): si_waveReadWave 296/300, owaveBinaryWrite, SI_dbwaveAsciiWrite, SI_getWaveByName,
  SI_waveDispose, SI_modelUnlinkAllWaves, SI_waveAttachByName 300/300; SI_owaveBinaryRead 247, SI_dbwaveAsciiRead 253,
  SI_waveDuplicate 220; SI_waveChangeProfile 279/400 (rest fault in the original).
- Wave menu items (wavemn.spec, 600 each): SI_waveCreateItem [soft 0x084c67a0], SI_waveEditItem [soft 0x084c69a8],
  SI_waveAttachItem [soft 0x084c6b34]; waveSetDg 400/400, waveGetDg 400/400 (wavedg.spec).
- Dialog files: SI_particleSetupSave [soft 0x084c034c] 100/100, SI_particleSetupDefault [soft 0x084c0418] 100/100.
- 2026-10-29: SI_particleSetupReset [soft 0x084c0438] 100/100 and SI_particleAnimation [soft 0x084c05d0] 1800/1800 (the menu
  item; stand-ins for the dialog, the builders, the hide keys and the graph list; =1 changes 153/600, =calls 600/600);
  see "2026-10-29 particle menu item".
- si-boolean routing: the five SI_SPLIT_FACE sites call `split::si_split_face`; 750 cases = 657 identical, 93 not compared.
Not ported: si_dialogParticle 0x084be3c0, particleSetDg 0x084beae8, particleGetDg 0x084bf0c0, si_dialogWave 0x084ca1f0 (dialog
bodies, user interface). si_waveCreateIconMdl 0x084c61b8 is not a bare icon: it builds a model with a mesh from the binary's tables
and sets the wave's icon; it is ported in si-scene (wave.rs, `wave_create_icon_mdl`) and is a stand-in here; not re-ported.
Wiring left to other crates: scnMenu (0x0823701c, si-anim) still calls SI_particleAnimation through a hook; si-view's
SI_PARTICLE_SETUP_RESET (SI_sceneDeleteAllNotInHistory) still a call by address.
Check: re/tools/si-dynamics/run.sh (gen/check/mutate), genwavemn.sh, genwavefile.sh, genpanim.sh; dynamics-checkkit.

## 2026-10-28 read from code

- SI_grafLists (0x08679498) and SI_procData (0x08679314) are pointer variables: the graph list is
  `*SI_grafLists + (*SI_procData)[1] * 0x24`.
- path2_xfm keeps the previous point's up vector when the up argument is null; the check found this.
- libc drand48 default state in the emulator is 0x1234ABCD330E; Lcg48 reproduces it.
- Program constants at 0x08661300 (64 bytes) are needed for tri_sample and path2 checks.
- Guessed names: particle status +0x34 (model flag), +0xb4/+0xb8/+0xbc (source, destination, third item).
- Hooks in ParticleCalls point at routines ported in other crates (71 routing-debt pairs).

## 2026-10-28 tooling

- specgen.py answers accept @BUF, {a|b|c} choices, rand32*K and ANS/OUT1/OUT2 (words stored through o args);
  blk has share=1; arg kind filew. specgen_particle.py (SPECGEN_PARTICLE, SPECGEN_STATUS) adds the globals and the
  SI_nodesAllocate stand-in; the Rust side uses an effect() stand-in in dynamics-checkkit. chk.sh NAME... prints
  plain, MUTATE=1 and MUTATE=calls results.
- wave.spec / genwave.sh (wave checks): SPECGEN_WAVE adds a wave, icon, model and wave-list world (the arguments
  0x08a10000 / 0x08a00000 become the buffers); SPECGEN_WAVEAMP makes the model a spline; SPECGEN_WAVEMN adds
  SI_procData / SI_grafLists with a selected node. Fixed-address globals above the image are unmapped: use buffers.
  particleHandleHideKeys lives in particles.spec (frame globals 0x08782040 must be declared).

## 2026-10-28 file routines (read from code, tooling, guesses)

- Read from code: the readers open `<name>.wav` (binary) or the name as given (ascii) with mode "r"; the keyword `Wave` starts
  a wave; the icon locals lie at the original's stack addresses (the frame is 0x5c bytes, the rotation vector passed to
  SI_matrixXyzRotation sits at a fixed address the check compares); the status words at 0x08822120 (counter) and
  0x088221a8 are written by the readers; the error word 0x086cebb0 is set to 4 by SI_dbwaveAsciiWrite without a wave.
- Tooling: specgen.py has the arg kinds `filer` (a FILE read by the routine, bytes from `ftext NAME TEMPLATE` lines;
  several lines are alternatives) and `cstr` (a fixed NUL-terminated string buffer, alternatives with `|`), and `ftext
  file:PATH ...` for files the routine opens itself. Templates: `\n`, `{i LO..HI [$v]}`, `{f LO..HI [$v]}`, `{w a|b}`,
  `{r N TEXT}`. specgen_particle.py: SPECGEN_FILES=1 appends written named files to the case's stdout as
  "\n--PATH--\n"+content (like gen_write.py); SPECGEN_WAVEFILE=1|2 is the wave-list world (mode 2 hides buffer addresses
  from the readers). The checker places the text of a `f=` field in the arena, the emulator does not: movefile.py moves
  it behind the other fields so the layouts agree. The check binary includes tools-checkkit's files.rs and a Streams
  wrapper that sends the port's print() to the files model; `write_to` was added to files.rs and its `emit` no longer
  panics when no stream was opened (both backward compatible).
- Guesses: name pool wa..wd; a wave without icon (1 case in 4 returns 0) is the stand-in's choice; the originals'
  uninitialised return values are not reproduced (the port returns the last wave, or 0).
- Menu items with picks: `wavemn.spec` + `genwavemn.sh` (SPECGEN_WAVEMN=2 model nodes, =3 wave nodes; pick events wev0..2, nodes wpk0/wpk1,
  SI_getEvent / SI_gLSelect answered in sequence). Done: Create/Edit/AttachItem with a si_dialogWave stand-in body (edits +0x10, +0x14, +0x28
  from a hash of words +0x2c..+0x34; cancel when hash&7 == 0) and `create_icon` using the world's `wicon`.

## 2026-10-28 dialogs and wave menu items

- Read from code: the wave dialog maps +8 (type) to toggles 6/7/8, +0x30 to toggle 0x11, +0x28 to toggle 0x12, +0xc to toggles
  0x13/0x1b/0x1c, floats +0x18/+0x1c/+0x20/+0x24 to fields 0x14/0x15/0x18/0x19 ("%.4f"), ints +0x10/+0x14 to fields 0x16/0x17 ("%d"),
  name field 4, a second text field 0x1e. SI_particleSetupSave opens the file "w", writes through particleWriteParameters, flushes,
  tests ferror (error word 1 for open, 2 for write); SI_particleSetupDefault tests the status word <= 0.
- Guesses: the dialog edit formula above is the stand-in's; usleep is stubbed by its address (0x080c37b8, a PLT thunk the spec cannot
  name); SI_getTexedFloat is answered in st0 and the stand-in logs it as a 4-byte word (a 10-byte word is read by the check
  binary as the library's maths tape).
- Kit candidates: st0-answering stand-ins for non-libm routines; PLT thunks nameable in `outside`/`stub`.

## 2026-10-29 particle menu item

- Read from code (SI_particleAnimation 0x084c05d0, listing and decompile): the item opens the dialog, reads the meshes, needs the
  repetition count (status +0x40) and particle count (+0x24) above 0; the count comes from the node behind +0xb4 / +0xb8 when
  +0x28 / +0x2c are set; one repetition uses the animation model itself, several hang each repetition model under a node named
  "null"; button 0x65 from the second repetition on halts and shortens the count (restored at the end); the hide keys are keyed
  per model or per son depending on +0x34; then time = frame / rate (SI_frameToSec, run inline), the models are updated and
  the views redrawn. SI_particleSetupReset sets the first word of the status record to 0.
- Guess: none; the stand-in answers of the check (dialog 0 / negative, builders giving trees of 0..5 sons) are made up.
- The 80-byte message buffer is at ebp-0x50, so the port reserves a 4-byte frame word before its local (pointer arguments
  to the status line are compared by address).
- Check: particle_anim.spec + genpanim.sh (world SPECGEN_PANIM of specgen_particle.py): SI_particleAnimation 1800/1800
  identical (151 reach "Computation halted!"), SI_particleSetupReset 100/100. Mutation on the first 600: =1 changes 153,
  =calls 600; SetupReset =1 100/100.
- Hooks added to ParticleCalls: si_dialog_particle, si_particle_get_meshes, si_particle_create_animation,
  particle_handle_hide_keys, si_metaball_get_parent_system. The item is reached from the scene menu, not from evaluation.
