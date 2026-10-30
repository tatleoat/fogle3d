# 0042 mental ray translator, main part (mi_mr_main.c)

Our own description of the first part of the mental ray scene writer. Module: `crates/si-mental/src/mi_mr_main.rs`.
Every function takes the memory image and an `Outside` (other-unit callees answered by the checker, named by the original symbol).


## Where it stands
- Rounds 1-6 (mi_mr_main.c, mi_material.c, mi_light.c, mi_camera.c and others, detail below): every ported function was compared against the emulated original, all plain runs 0 different.
- Round 7 (mi_texture.c) ported in `mi_texture.rs`, checked with `re/tools/mrmain6/tex1..tex14.spec` (cases / identical / different / not compared; mutation switch `=1` and `=calls` count differing cases):
  - preprocess_texture (0x08594050) 600/600/0/0; buildDefaultPicName (0x08593f34) 60/60/0/0; generateTextureName (0x08591ea4) 120/120/0/0.
  - destroy_2D_texture_list (0x08591ff0) 80/80; cleanup_2D_texture_list (0x0859219c) 80/80; dump_2D_texture_list (0x08592228) 120/120.
  - setFileName (0x08593cd0) 300/300 (frame <= 0 with kind 2 duplicates an uninitialised stack string in the original, excluded); static init (0x08594474) 60/60; global constructor (0x085944a0) 40/40.
  - find_program (0x08593ab4) 300/300; SI_init_MRSoftColor3D (0x08591144) 500/500.
  - add_2D_texture (0x08591ca0) 500 cases, 330 identical, 0 different, 170 not compared (original reaches realloc of an unknown block, strcmp or a stub).
  - print_model_2D_defs (0x08591b34) 300/300; add2DMaps (0x085912cc) 400/400; print_3D_map (0x08593260) 600/600; print_model_3D_maps (0x08593090) 400/400.
  - print_2D_texture_map (0x08591380) 600 cases, 456 identical, 0 different, 144 not compared (original writes a global the case does not declare).
- Round 8 (2026-10-28), specs `tex15..tex22.spec`, vectors in `re/out/mrmain6/main`; plain run, mutation `=1`, `=calls` (cases differing):
  - SI_init_MRSoftColor (0x08590a20) 900/900/0/0 (mutation 900, 125, 125); print_2D_map (0x085928b8) 1000/1000 (357, 357); print_model_2D_maps (0x0859233c) 600/600 (251, 93).
  - print_2D_texture_defs (0x085916fc) 700/700 (477, 191); print_2D_texture_defs_preview (0x08591938) 900/900 (329, 309).
  - print_material (0x0857cfd0) 1500/1500/0/0 (mutation 1228, 1228); print_texture_names (0x0859425c) 600/600/0/0 (364, 394).
  - precompute_mi_texture (0x0859390c) 800 cases: 503 identical, 4 defined, 0 different, 293 not compared (fatal=1 with a missing or short picture: sfatal never returns in the original, the case runs on into fclose(NULL)); mutation 507 / 6 differ.
  - SI_init_MRSoftMaterial (0x0857cdfc) was already ported and checked; `kit` listed it unported only because its doc comment lacked the `[soft ...]` tag, now added.
- Round 9 (2026-10-29), cases / identical / different / not compared (mutation `=1`, `=calls` cases differing):
  - mi2_tools.c in `mi2_tools.rs`, spec `mi2tools1.spec`: print_transform_mat (0x08597ff0) 300/300 (0, 300), print_instance (0x085980b8) 300/300 (0, 300), print_default_instance (0x08598134) 100/100 (0, 100), print_group (0x08598178) 600/600 (600, 325).
  - mr_cmdline.c in `mr_cmdline.rs`, spec `cmdline.spec`: addCoreOn (0x0842c854) 500/500 (0, 0), si_mrfileBuildCmdline (0x0842c7d0) 500/500 (371, 0); SI_strListAppend and SI_strListToString are stand-ins (the latter answers "L<count>").
  - si_dumpUsageInfo (0x08581da8) ends in exit(2), which the kit skips; `mrmain-checkkit DIR --exit-cases` compares stderr text, globals and the exit: 6/6 identical (6, 6).
  - print_2D_texture_map (0x08591380) now 600/600: the 144 were undeclared string-slot globals, declared in `tex14.spec`. add_2D_texture (0x08591ca0) now 500/500 (401, 68): realloc of a static block is answered by `memrealloc2`; kind 3 (null file name) crashes the original in strcmp and is excluded.
  - findbound (0x08594978) 300/300 and textureVert (0x085944c0) 900: 456 identical + 444 given-lib, 0 different, now cited.
- Round 10 (2026-10-29), cases / identical / different / not compared against the emulated original; mutation `=1` and `=calls` turn every row with something to damage different (rows with no output or no compared call stay identical):
  - mr_messages.c in `mr_messages.rs`, specs `msg1.spec` `msg2.spec`: processMsgBuffer (0x0842bfd4) 600/600, processMessages (0x0842bf6c) 300/300, pipeReadMessages (0x0842bef4) 400/400, si_mrmsgProc_Protected (0x0842be88) 300/300, si_mrmsgProc (0x0842bd70) 300/300 + the setjmp-returns-1 variant 300/300, cleanup (0x0842c0e4) 100/100. The signal handler (0x0842be70) ends in longjmp, the stubbed original falls through and faults: not comparable, its recorded call is read from the vector.
  - mr_translator.c in `mr_translator.rs`, specs `trans1..3.spec`: Protected (0x0842c2ac) 100/100, si_mrtranslatorProc (0x0842c110) 200/200 + two setjmp variants 200/200 each, cleanup (0x0842c308) 200/200; handlers 0x0842c27c and 0x0842c294 as above.
  - mr_main.c remainder in `mr_main.rs`, spec `mm1.spec`: SI_initMentalPipeParms (0x0842b654) 300/300, signal handler (0x0842b734) 10/10, displayMessages (0x0842b75c) 300/300, detectMessages (0x0842b7ec) 200/200, checkUserQuit (0x0842b9bc) 200/200, checkResources (0x0842bbc8) 120/120, computeWindowSizePos (0x0842ba44) 600/600, WaitAfterDone (0x0842b844) 200/200.
  - mi2control.c in `mi2control.rs`, specs `mi2ctl1..6.spec` (written by `mkmi2.py`): writeBuffer 100, forwardSlashes 120, backSlashes 120, getFullPath 100, extractPathSeparator 100, extractSceneAndDbName 300, databaseGetByPath (0x0842d644) 300, initRendering (0x0842d940) 300, SI_mi2ConnectToFile 300, si_mi2OpenScene (0x0842dcb4) 600, the six queries 30-100 each, scene render 300, close/disconnect/exit/lex-yacc/start-parser: all identical, 0 different. si_mi2InitTranslator (0x0842db20) 400: 349 identical, 0 different, 51 not compared (the port's result depends on memory nobody wrote; the cause was not found, getenv answers come from the kit's knob word). Not covered: free of a non-null block held by a global (stand-in refuses unknown blocks).
  - Resolved: mi2_print_model_tree (0x085968c0) 1495 identical + 5 defined, 0 different (the 10 UNINIT:eax were the print_group stand-in's unwritten block, which the original formats with `%s_shadow`; now it starts with "pg", enabled by the word 0x08795fc8 = 0x600dcafe). addToRootList 65 defined: list null with count > 0, so the first made block's earlier words are never written by anyone and realloc copies them (a property of the case, not of the original). mi2_print_light 320 given-lib: libm cos is the host's, the port makes the same calls with the same argument bits; nothing more can be proved.
  - SI_mentalRunTracer (0x0842a8a0) was written from the decompiled flow in mr_tracer.rs; compared in round 11 (below). Tools added: `plt@NAME=@key[!k,..]:kinds` import stubs with custom answers and effects (`xst`, `acc`, `genv`, `ina`, `dbl`, `prep`, `seq`), `mkmi2.py` (string literals injected as globs).
- Round 11 (2026-10-29), cases / identical / different / not compared; mutation `=1` and `=calls` as above:
  - Queue state was stale: mi_calc (0 different in 300-500 cases each, earlier rounds), mr_main and most of mi2control were already compared.
  - si_mi2InitTranslator (0x0842db20) now 400/400/0/0 (mutation 0, 400 differ with `=1`, 0 with `=calls`): the 51 uncompared cases came from the generator (`_fx_genv` padded the made getenv block with zeros, the port writes only the text); padding removed, `mi2ctl4.spec` regenerated.
  - mr_display.c ported in `mr_display.rs` (specs `disp1..6.spec`, written by `mkdisp.py`; the pipe stand-in is `pipet`): swapLongArray (0x0842cd7c) 200/200/0/0, updateFrame (0x0842cb9c) 300/300, pipeReadTileHeader (0x0842cbd8) 500/500, pipeReadTileData (0x0842cc8c) 600/600 + 400/400 on the case's own block, tileDisplay (0x0842cdb4) 600/600, si_mrtileProc_Protected (0x0842c9d8) 1000/1000, si_mrtileProc (0x0842c8c0) 400/400 + setjmp-returns-1 300/300, si_mrtileProcCleanup (0x0842d520) 100/100. Mutation `=1` makes every row differ (cases differing: 134/200, 150/300, 500, 600, 400, 282/600, 1000, 400, 300, 78/100); `=calls`: header 500, data 526/600, data#b 348/400, tileDisplay 600, Protected 1000, Proc 400, Proc#r1 300, the three call-free rows 0. The handler 0x0842c9c0 ends in longjmp: 20 cases all STOP, not comparable. Not ported (pure IRIS GL window drawing, called through `Outside`): ghostFrame (0x0842d068), showFrameCount (0x0842ce90), si_mrwinOpen (0x0842d210), si_mrwinClose (0x0842d370). Whole-directory run of `mrmain-checkkit re/out/mrmain6/main`: 42181 cases, 40716 identical, 0 different.
  - Tool fix: `gen.py` dropped `glob` lines placed before the first `fn` of a spec and did not match labels like `0xADDR#b`; both fixed.
  - mi2control_lex.c: the flex run-time and the scanner in `mi2control_lex.rs`, checked by `mi2lex-checkkit` with vectors from `re/tools/si-mental/gen_mi2lex.py` (read(si_stdInput, buf, 1) is a recorded stand-in handing out a case's bytes): cases / identical / different / not compared (300 each): yy_get_previous_state (0x0842f14c) 289/0/11, yy_try_NUL_trans (0x0842f1b0) 300/0/0, yyunput (0x0842f1fc) 192/0/108, yy_get_next_buffer (0x0842ef50) 301 cases 260 identical, 1 different, 40 not compared, yyinput (0x0842f2a8) 258 identical + 22 defined (unwritten bytes the case never set), 0 different, 20 not compared, mi2_yylex (0x0842e9e0) 113 identical, 1 different, 186 not compared (random transition tables: fatal "no action found" ends in exit(2), wild table reads, budget), and 300/300 identical for create, delete, flush, init, load, switch (275, 25 original faulted), scan_buffer, scan_bytes (286, 14 exit), scan_string, restart, i.e. buffer functions. The 2 different (one in yy_get_next_buffer, one in yylex) are one artifact: a `read` answered -1 makes the original store the end-of-buffer mark at ch_buf[-1], one byte below the case's buffer, which the port's memory refuses. Not-compared rows are originals that stopped (exit, fault, budget) or wrote past a buffer, nothing needs action. Mutation `=1`: 4158 of 4801 differ (the rest are void routines whose effects here do not change what is compared, or not-compared); `=calls`: yy_get_next_buffer 121 differ, mi2_yylex 3 (only the ECHO rule calls fwrite), yyinput 7. The ECHO call is fwrite(yytext, yyleng, 1, yyout); the port had the two counts swapped until the check found it.
  - mi2control_yacc.c: `mi2_yyparse` (0x0842e1d0) in `mi2control_yacc.rs` (bison tables read from the image: 51 states, 48 rules, yylast 0x32), with mi2_yyerror (0x0842e908), output_ok (0x0842e92c), output_error (0x0842e978) from the lex file; `mi2yacc-checkkit`, vectors from `gen_mi2yacc.py` (the scanner and the thirteen si_mi2 commands are recorded stand-ins; token scripts are random walks over the parser's tables with some corrupt tokens): mi2_yyparse 400/400/0/0, mi2_yyerror 400/400, output_ok 400/400, output_error 400/400; mutation `=calls`: 399/400 yyparse differ (one case makes no call), 400 each for the others; `=1` differs for all 400 yyparse rows (the other three produce no value).
  - SI_mentalRunTracer (0x0842a8a0) in `mr_tracer.rs`: 600 cases, 600 identical, 0 different, 0 not compared (mutation `=1` and `=calls`: 600 differ each; 3325 of 3505 bytes of the original executed, the gaps are single branches). `mrtracer-checkkit`, vectors from `re/tools/si-mental/gen_mrtracer.py`: 52 process, thread, pipe, mutex, string-list, GL and C library routines are recorded stand-ins answering from per-case lists (the ones that answer through a local of the caller store through `o` arguments); its ported siblings (checkUserQuit, checkResources, computeWindowSizePos, WaitAfterDone, the cmdline builder, the two cleanups, the translator proc) run natively in the port and as the original's own code. The check found two slips in my generator, not in the port (the third argument of SI_mrthreadCreate is -1; SI_pipeioDestroy also takes the address of a field of the block). The framework names a stand-in by the text before `__`, so `__fxstat` is registered by hand there.
- Not ported in this finding: none of mi_texture.c; the rest of mi_material.c is ported.
- Tools: `re/tools/mrmain6/gen.sh SPEC`, `chk.sh` (all vectors), `mut.sh NAME` (plain, =1, =calls rows), `mkpm.py` (writes tex20.spec), `mkpic.py` (writes tex22.spec).

## Earlier rounds (detail, kept as written)

- Ported and compared against the emulated original, all vectors identical in the plain run (cases): [soft 0x08580cc8 SI_setmi2PipeConnection] 20,
  [soft 0x08580ce0 SI_setmi2Output] 60, [soft 0x08580cfc SI_getmi2Output] 20, [soft 0x08580d84 SI_mi2IsIncremental] 20, [soft 0x08580d94 SI_setmi2Incremental] 60,
  [soft 0x085826dc SI_onoff] 40, [soft 0x08585e20 __static_initialization_and_destruction_0] 80, [soft 0x08585e40 global.constructors.keyed.to.progname] 20,
  [soft 0x08580c3c displayMessages] 200, [soft 0x08580684 findBlur2DShaderLib] 200, [soft 0x085814a8 buildFullPicFileName] 300, [soft 0x08585aa4 _IsScanlineOff] 400,
  [soft 0x085859c4 openMIfile] 300, [soft 0x08581460 cleanup_sequence_mental] 100, [soft 0x08581cf8 disposeAllmovingCopy] 300, [soft 0x08581c78 SI_cleanupTranslator] 300,
  [soft 0x08580700 printHeader] 300, [soft 0x08581dd4 print_scene] 400, [soft 0x08580d0c SI_isMr19Enable] 120, [soft 0x08580da4 SI_spdlUseNamesrvShdDecl] 60,
  [soft 0x08580e28 SI_spdlUseNamesrvShdRef] 100, [soft 0x08580ec4 SI_spdlUseNamesrvImgRef] 60, [soft 0x08585ba0 SI_closeLiveConnection] 300,
  [soft 0x08585b54 SI_initLiveConnection] 100, [soft 0x08580500 SI_init_MRCamera] 363 identical + 219 defined of 600, [soft 0x085808ac mi2_printHeader] 300,
  [soft 0x08580f48 init_sequence_mental] 500, [soft 0x085855c8 mi2_print_view] 40, [soft 0x08581310 sequence_mental] 285 of 300 (15 never end, not compared).
- si_rotateWorld ([soft 0x085823cc]) 600 of 600 given the recorded host sin/cos. mi_calc.c, 0 different in 300 to 500 cases each, aliased variants included:
  vector_plus_vector 0x08598270, vector_minus_vector 0x08598298, vector_X_scalar 0x085982c0, vector_dot_vector 0x085982e8, vector_X_vector 0x08598310,
  normalize_vector 0x0859835c, init_matrix 0x085983ec, vector_X_matrix 0x08598430, matrix_X_matrix 0x0859849c, rotate_matrix 0x085986f4 (400 given-lib),
  invert_matrix 0x0859883c, matrixZAlign 0x08598a60 (each cited [soft ...]).
- Round 3 (main7..main12 specs; cases / identical / given-lib / not compared; none different): [soft 0x08581530 print_frame] 500/495/0/5 (the 5 stop in
  the strdup stand-in), [soft 0x08582054 mi2_print_scene] 500/500 (needs SI_mutexLock/Unlock stubbed), [soft 0x08585c74 SI_translateInLiveConnection] 300/300,
  [soft 0x08585600 SI_customPreviewWriteUDF] 500/500, [soft 0x08584e8c print_option] 600/600, [soft 0x085826f4 print_view] 600: 312 identical + 288 given-lib.
  print_option and print_view were written from disassembly plus decompile (the decompile garbled the argument lists).
- Printers (sdebug, SI_mrpipeFPRINTF, the fprintf family) are compared by format address plus argument words, not rendered text; the Rust image has
  no rodata, so format addresses are the original's. print_frame's sprintf and the later sfatal/swarning/sdebug use `cprintf.rs` (a glibc vfprintf port);
  `printf-check` compares it with host snprintf: 6000 cases, 5998 identical, 2 different (glibc prints `%#g` of 999999.5, precision 6, as "1.e+06").
- st0 workaround: the Kit cannot compare a 10-byte st0 return or consume a recorded st0 answer for non-libm code. libm tan is replayed from the vector
  in the port closure (print_view's 288 given-lib cases, vector_dot_vector); genm.py has an unrecorded `stub NAME=!st0@VALUE`.
- Mutation (EMUCHECK_MUTATE=1 and =calls): every checked function turns different under at least one switch. Coverage lines where a switch changes
  nothing: batch 1 (no calls) and printHeader/cleanup_sequence_mental ignore =1; getenv flag functions ignore =calls (getenv unlogged); SI_init_MRCamera
  =calls (libm skipped); mi2_printHeader =1; vector_dot_vector =1 (st0); mi2_print_view =1 (no arithmetic); print_option =1 (returns nothing, writes
  nothing; =calls kills 600 of 600). Round 3: print_frame =1 494 of 500, mi2_print_scene both 500, SI_translateInLiveConnection =1 300 / calls 257,
  SI_customPreviewWriteUDF =1 500 / calls 383, print_view both 600.
- Bug found by the check: openMIfile printed an empty name for a null path; glibc prints "(null)". Fixed.
- Round 4 (mrmain5; specs in re/tools/mrmain5/, vectors re/out/mrmain5/main; cases / identical, none different): print_camera 0x085839f4 600/287 + 313
  given-lib; sfatal 0x08598ba0, swarning 0x08598c44, sdebug 0x08598cdc 300 each; mr_util.c: SI_checkHighlightedWindow 0x0842c370 200, SI_generateMIPerFrameFileName
  0x0842c40c 400, SI_mrpipeSet 0x0842c578 100, writePipe 0x0842c76c 300, SI_mrpipeFlush 0x0842c5d8 300, SI_mrpipeFPUTC 0x0842c610 500, SI_mrpipeFPRINTF 0x0842c68c 600;
  mi_defparms.c: SI_mentalPrintDefaultDeclare 0x08595420 and mi2_ 0x0859545c 400 each; mi_tools.c: buildShaderName 0x085952f4 300, buildShaderNameWithString
  0x08595380 300, si_checkForTxtGlb 0x08594fb8 200, si_getMatNameExtension 0x08595024 498 + 2 not compared (uninit), updateTxtSupport 0x08594c04 300, dumpTxtSupport
  0x08594cdc 295 + 5 not compared (uninit), findbound 0x08594978 300. Mutation =1 / =calls: every one turns different under a switch (mi_defparms only under =calls).
- Rounds 5 and 6 (mrmain6; specs re/tools/mrmain6/, vectors re/out/mrmain6/main, checker `re/tools/mrmain6/chk.sh`; cases / identical, 0 different everywhere):
  mi_tools.c elemTxtGlbCompare 0x085950d4 400, textureVert 0x085944c0 900 (456 + 444 given-lib); mi_light.c print_light 0x08592e08 900 (632 + 268
  given-lib), si_printDefaultLight 200, print_light_defs 500; mi_model.c print_model_tree 900, print_all_models 500, print_model 1200; mi2_light.c
  SI_init_MRSoftLight 800, mi2_print_light_defs 600, mi2_printDefaultLight 0x085964a8 200, mi2_print_soft_light 400, mi2_print_light 0x08595938 1200
  (880 + 320 libm replay); mi2_model.c modelHasShaderReference 0x08597de4 100, print_empty_light 0x08597dbc / print_empty_mesh 0x08597b98 /
  print_empty_patch 0x08597be0 60 each, addToRootList 0x08596f88 500 (435 identical + 65 defined: the original read uninitialised memory),
  SI_mi2GetModelName 0x08597994 600, print_empty_geometry 0x08597a40 500, si_mi2ModelForwardDefs 0x08597e18 500, mi2_print_all_models 0x08596710 600,
  mi2_print_model_tree 0x085968c0 1500 (1490 identical, 10 not compared: no STOP label shown), mi2_print_model 0x0859709c 1500.
  si_dumpUsageInfo 0x08581da8 is ported (usage text on stderr with the program name from 0x086ef9ac, then exit(2)): the emulator stops with exit 2 in all
  6 cases and the checker skips stopped cases, so 0 of 6 are compared by the tool; the text was compared by hand with the recorded stderr (identical).
  Mutation (=1 / =calls, cases differing): GetModelName 285, all_models 305, print_model 1443, model_tree 1108, print_empty_geometry 345, ForwardDefs
  84 / 374, modelHasShaderReference 100, print_empty_light/mesh/patch 0 / 60 (pointer-only argument, caught by calls), addToRootList 417 / 0.
- Tooling pitfalls (genm6.py customs: fixed-address list/son/relation stand-ins mfirst mnext son nson relfirst relnext relcnt bsnames msons mnsons mrelc treefx
  pmodel modname mstrdup): `gen X#idx` is a word index, `X[idx]` something else; a layout field typed ptr with `choice=` misbehaves (use u32); stubs are global
  per spec (stubbing the function under test is vacuous, so separate spec files); stack-address arguments need `!k` masks after a custom; a route that answers a
  call suppresses its effect (write *out16 inside the route); the Rust Mem has no rodata, so constants such as "MOD_" are literal bytes; stubs that
  dereference arguments need mapped globals (xm0..15 at 0x08796000); son/relation stubs must stay consistent or the original frees garbage.
- Ported code is complete for the work order; remaining in this unit: nothing known.
- Tooling: getenv knob word at 0x08829ff0 (genm.py `genv`); named files via genm.py FILES; genm.py `_logged` exact stand-ins for creat/access/close/unlink/usleep
  patched into `_sti.EXACT` (standins.py untouched); customs genname, pipe30, lightlist, modlist, firstrel, rndbits. Port code embeds string literals.
- Check: `re/tools/si-mental/gen.sh re/tools/si-mental/main7.spec` (main1..main12, calc) then `re/tools/si-mental/chkm.sh`; own checker `crates/si-mental/src/bin/mrmain-checkkit`.


## Round 7 (2026-10-28)
- Read from the code: the texture list is an array of 0x18-byte entries at the list global, with name, map name and flag fields; printers write through fprintf with the exact format strings, SI_malloc marks its block with 'o' first and 'k' last.
- Guesses: none that changed output; unknown callee effects are answered by recorded stubs (add2DMaps, precompute_mi_texture, swarning, SI_mrfileConvertToUnix, SI_renderSceneUpdate).
- Pitfalls: layout pad counts must be exact; glob lines belong to the preceding fn and are repeated per fn; signalling NaN inputs are an emulator artifact; rotate_matrix is replaced by an identity-writing stand-in (`rotid`) mirrored in the checker; cases that read uninitialised stack are not comparable and were excluded by input choice.
- Check numbers are in the list above.

## Round 8 (2026-10-28)
- Read from the code: print_material builds three name strings (the header, the displace/static-blur name and the shadow name, with the header as the fallback) and strdups them only when mental ray 2 output is on; texture surface numbers are counted material slots first, then the element's, with kinds 7 and 8 (environment) numbered 0; the end section runs only with mi2output.
- Guesses: the stale stack words of calls the listing shows without arguments (mi_print_soft_fog takes none; sfatal "can't read header" shows a third word 1, the fread count still on the stack) are reproduced as the checker records them; the empty and one-dash rodata constants are empty strings by their address spacing and by kit `consts`.
- Lessons:
  - A port that formats text with `cprintf` reads format strings from memory, but the Rust image of a check has no rodata: formats must be byte literals and constants passed to `%s` need a stand-in in memory (`spf` and the scratch word in print_material). The symptom was a heap block 0x20 off, from strdup lengths.
  - Two routines differ only in case (print_2D_map 0x085928b8 and print_2d_map 0x0857ebc8); stubs and `Routine` names must use the right one, or the original runs the other routine natively.
  - Native callees must not go through `o.call` when the original runs them natively unlogged (init_matrix, strdup, free): the port calls the Rust function directly; a generator custom that stands in for a routine must write its out pointer in the vector (fr2).
  - `access` is answered by the emulator's stand-in; the vector records it as a stub call with answers 0/1 unrelated to the case's files, so the port replays it. The picture reader runs natively in the original; for the port the checker (`mrmain-checkkit`) reads the case's `file:` inputs, serves fopen and SI_byteswap_fread (fread plus swapping of 2, 4 and 8 byte items).
- Cases not compared: precompute_mi_texture 293 (see above). Nothing else in round 8.

