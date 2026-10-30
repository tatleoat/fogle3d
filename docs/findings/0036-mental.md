# 0036 — The mental ray translator, `mi_patch.c` (package si-mental)

## Where it stands

Package 63, crate `si-mental` (`crates/si-mental`, tools in `re/tools/si-mental/`, vectors in
`re/out/si-mental/`). `mi_patch.c` is complete (20 functions); `mi_poly.c` is complete (11 of 11); `mi_material.c` is 13 of 14 written and compared.
Checks: identical / different / not compared, generated cases against the original in the emulator.

mi_patch.c: the 16 functions listed in the diary entry below at 1500 cases each (convert_to_bezier
1481/9 defined-different/10 not compared; print_patch 1499/0/1), plus print_nurbs_geometry
0x085867d0 1500/0/0, mi2_print_nurbs_geometry 0x0858a2d4 1500/0/0, mi2_print_patch 0x0858b9b0
899/0/1 (900 cases), mi2_print_patch_geometry 0x0858b914 900/0/0.

mi_poly.c (module `mi_poly.rs`, specs `mipcv.spec`, `mipoly1.spec`, `mipoly2.spec`):
print_compressed_vector 0x08590928 300/0/0; calculate_motion_vector 0x085907ac 579/0/21,
motion_vector 0x08590848 565/0/35, mi2_motion_vector 0x085908a8 578/0/22 (not compared: the original
faults on a null block); texture3d_point 0x0859076c 300/0/0; prepare_tcoords_array 0x08590314
800/0/0; calc_poly_textverts 0x0858fb50, calc_tri_textverts 0x0858fecc, texture2d_point 0x08590468
600/0/0 each. Mutation (first value, then first argument of every call): every function turns
different except where nothing depends on the value (a function with no calls cannot be mutated in
the "calls" mode: calculate_motion_vector stays identical there).

print_mesh_geometry 0x0858cc30 (spec `pmesh.spec`): 1480 identical / 0 different / 120 not authoritative
(the original reads memory nobody wrote), of 1600 cases. Mutation, value and calls: 132 unchanged, 1468 different
in both modes (the 132 are cases that return at the start or print nothing: null dict/element/model, no polygon).
Limits of the check: a single polygon per case (nested blocks of an array element are only generated for
element 0); sdebug is compared by its first four words (the fifth, nverts, of the non-blur call is not);
the printers are compared by format address plus argument words handed to the stand-ins, not by text;
the mesh copy/inverse/dispose, findbound, normalize_vector, the matrix routines, the motion vectors and the
related-element lookups are stand-ins; the texture builders run for real in both. One case seed past 1600
makes the emulator run out of memory (a runaway in the original's hole loop); the spec is capped at 1600.

print_triangles 0x0858e6a8 (spec `ptri.spec`): 1485 identical + 77 same with uninitialised stack words / 0 different / 38 not
compared (the original faults), of 1600 cases. Mutation (value and calls): 1441 different, 121 unchanged (early returns).
Limit: one triangle per case, so the multi-triangle counters and the every-tenth-line text are not exercised.
`gen.py` fix: a variadic stub written `name=0:p,p,w,w` no longer crashes the parser.

mi_material.c (module `mi_material.rs`, specs `matspec/{sel,fog,init,light,maps,ext,softmat}.spec` from `mkmat.py`, vectors by
`genmat.sh NAME`, check `chkmat.sh` + `sum.sh`, binary `mat-checkkit`): 13 of 14 compared, identical / different / not compared:
check_selective_lights 0x0857f558 600/0/0; init_materials 0x0857cdc0 600/0/0; SI_init_MRSoftMaterial 0x0857cdfc 1200/0/0;
mi_print_soft_fog 0x08580430 400/0/0 (bug found: the last printf pushes the double at 0x08790d00 first, the port had the
order reversed; 271 of 400 differed before the fix); print_light_list 0x0857f2c0 1500/0/0; print_default_material 0x0857eab0
1500/0/0; print_3d_map 0x0857f030 1500/0/0; print_2d_map 0x0857ebc8 2500/0/0; print_extension 0x0857faf4 1000/0/0;
print_mat_with_extension 0x0857f92c 2000/0/0; flagModelWithSelectiveLight 0x0857fbcc 1500/0/0; print_all_material
0x0857f5e0 2412/0/88 (the original faults on an unmapped read in some layouts; not chased); mi_print_soft_material
0x0857fcd8 3000/0/0 (the last argument is a text written instead of the light list when non-zero, not a flag).
Mutation (value, then first argument of every call): void functions cannot change in value mode; in calls mode all
13 turn different (print_default_material 1353/1500, print_2d_map 1664/2500, SI_init_MRSoftMaterial 1114/1200,
print_all_material 2412/2500: the rest return early).
Limits: printers compared by format address plus argument words, not text; sprintf buffers (texture names) are not
compared except as pointers; the related-element walkers, GetElem, SI_emptyString, need_texture, SI_textureInitialize
(zero-fills 0x1d8 bytes in both) and print_material are stand-ins. To redo: `re/tools/si-mental/genmat.sh <name>`
then `re/tools/si-mental/sum.sh`.
mi_texture.c, 9 of 33 compared (mi_texture.rs, spec `matspec/tex.spec`): SI_initTxtSeqCounter 20/0/0, need_texture 400/0/0, need_3D_texture 400/0/0, find_empty_entry 400/0/0, lookup_2D_texture_list 600/0/0, mi2_lookup_2D_texture_list 600/0/0, calc_2D_matrix 1500/0/0, calc_3D_matrix 1500/0/0, print_matrix 800/0/0. Mutation: value mode turns all but print_matrix (void) different; calls mode turns calc_*, print_matrix different (leaf lookups make no calls). Globals: N3DTEX 0x086f2ccc, NTEX 0x086f2cc8, TEXLIST 0x086f2cc0 (0x18-byte entries). Routines init_matrix/rotate_matrix are effect-hook stand-ins.

Still to write: print_material 0x0857cfd0 (6.8 KB, read, not ported) and 24 mi_texture.c functions (buildDefaultPicName, setFileName, preprocess_texture, precompute_mi_texture, SI_init_MRSoftColor, SI_init_MRSoftColor3D, generateTextureName, add_2D_texture, print_model_2D_defs, add2DMaps, print_2D_texture_map, print_3D_map, print_model_3D_maps, print_2D_map, print_model_2D_maps, print_2D_texture_defs(_preview), destroy/cleanup/dump_2D_texture_list, find_program, print_texture_names, two static-init stubs).

Where the check cannot see the text: the printers are compared by the address of the format string
and the argument words handed to the SI_mrpipeFPRINTF / SI_mrpipeFPUTC stand-ins, not by rendered
text, so float formatting (`%g`-style rounding of the doubles) is not matched here; NaN weights and
points are not exercised in the NURBS specs. In the texture builders the real tcoords array is
placed at 0x08829800 (declared words) so its contents are compared. Not covered there: the model's
mesh fallback `**(model+0x38)`, the vertex-less kind-4 case of texture2d_point (the original prints
uninitialised stack), and the reallocation in prepare_tcoords_array (its own spec covers it).

Stand-ins answered by effect (same on both sides): vector_X_matrix, SI_matrixCopy 0x08516224,
invert_matrix 0x0859883c, motion_vector / mi2_motion_vector, SI_txtProjectionInfo 0x0829b584,
SI_mapTcoordCompute 0x082b3090, SI_meshGetTxtUVDomain 0x08453668, SI_nodeGetUV 0x084534d8,
SI_rvtxGetUV 0x0826d510. Hooks answered 0 (other units): SI_mrpipeFPRINTF 0x0842c68c, SI_mrpipeFPUTC
0x0842c610, textureVert 0x085944c0, updateTxtSupport 0x08594c04, si_getMatNameExtension 0x08595024,
sfatal 0x08598ba0, swarning 0x08598c44, sdebug 0x08598cdc, SI_nurbsSrfInverse 0x084dcaf8,
SI_patchSurface 0x0847f89c; getenv is recorded.

## 2026-10-28 — mi_patch.c, the printers and the Bezier conversion

Read from the disassembly of the Linux `soft`; the decompiler only to find one's way.

What the code does. The file writes a model's patches and NURBS surfaces in the mental ray scene
language. A patch is first copied and inverted when the model's scale (product of the three scale
factors, against the global handedness flag and bit 3 of the definition flags) mirrors it. Its
control points are turned into Bezier form by `convert_to_bezier`: a closed patch is first
opened by `close_patch`, then the U and then the V direction are multiplied by the B-spline or the
Cardinal basis matrix (tension/3 written into the Cardinal matrix, a static table at 0x086f2040) by
`convert_bez_u` and `convert_bez_v`. A patch with too few points calls `sfatal`, which the
original does not return from; the check restricts the point counts to 4..7 so the wrapped
16-bit counts that follow a returning `sfatal` do not occur. `print_patch` then transforms every
vertex with `vector_X_matrix`, grows the global vertex array (`si_Gbl_vert`, one record of 0x4c
bytes) with `SI_memtrackRealloc` up to the `mb_nb_max_vert` limit, prints the `v` lines and the
`m` (motion) numbers, the material names with `si_getMatNameExtension`, and the trim value
`max(Q.0x1c, Q.0x1e) / (A + B) + 1e-6` (A, B = 1 or 3 by the two kinds). When `no_out` is set the
whole print section is skipped and the copies stay allocated (as in the original).

Method. `re/tools/si-mental/gen.py` runs the original in the Unicorn emulator over generated cases
(specs `*.spec`), with stand-ins for the callees: recording stubs, and effect stubs for the
routines that write (`vector_X_matrix`: out = in * 1.5 + 0.25 (k+1); `motion_vector`; realloc capped
at 24 KB; the material lookups). `mental-checkkit` repeats the same effects (`route`/`effect` in
`src/bin/mental-checkkit/main.rs`) and compares the Rust port: return value, memory writes, the
recorded calls and their arguments, the globals written.

Things the check taught.
- Static data the original reads from the image is not in the check's memory: the type-name table
  at 0x086f17c0 (first nine entries are compiled in as `TYPE_NAME_INIT`) and the constants at
  0x08672538..0x08672550 (0xd1b71759 as the divisor of x/5000, 179.0, 3.0, 1e-6) are literals in the
  port. The two basis matrices are the exception: they are globals the spec randomises
  (`glob bs*`, `glob cd*`) because the program may rewrite the Cardinal one.
- A scalar-only spec needs a dummy pointer argument, or the `glob` lines are not applied.
- In `print_patch_texture_ref` the newline counter of the grid loop restarts for every row.
- The owner pointer in `print_patch`'s element must be non-null: the original reads through it.

## 2026-10-28 — print_mesh_geometry

`print_mesh_geometry (mi_poly.rs)` writes the 3-D mesh of a model as a mental ray polygon group. In order:
the mirror test (sign of the scale product, right-hand flag, bit 3 of the model flags) which, when it
holds, swaps in inverted copies of both meshes and gives the originals back at the end; the bounding
widths and centre as floats in globals; the vertex block (plain, or with the material's motion-blur
offsets: one vertex per node); then three passes over the polygons: per-node vectors (normal, texture
coordinates through the real builders, motion vector, vertex colour/alpha, the mental ray 2 colour
modifiers with their clamps), the `v` index lines, and the surface statements with the hole logic of
the original's node walk (a literal copy of its register drift, including the case where it never ends).
The mental ray 2 tail prints the approximation line. Frame: the locals sit at the original's ebp
offsets because the stand-ins record their addresses; the calls of the texture builders reserve the
original's outgoing area (0x24 bytes for `calc_poly_textverts` and `texture2d_point`), which the
check showed by the callee-local addresses.

## 2026-10-28 — print_triangles and the first twelve of mi_material.c

print_triangles keeps the pad words of calc_tri_textverts and texture2d_point (0x24 each) as in print_mesh_geometry;
the byte table at 0x086f2bc6 is not in the check image, so its first entry (a tab) and the rest (space) are literals.
mi_material.c is written bottom-up from the listings: the small helpers first, then the map printers, the light list,
the per-material extension routines and print_all_material. mi_print_soft_fog reads nine floats that live in the
program's zero-filled data (they are set elsewhere, not constants). Two choices worth knowing: the walkers that read
the result of SI_elemdictGetFirstRelatedElem through the pointer argument (flagModelWithSelectiveLight) read the
local, the others use the return value, as the original does; and print_all_material calls
SI_elemdictFindNextRelatedElem even when the first lookup found nothing, as the original does. Not compared: no spec
exists for these twelve yet (state needed: element lists with 0x84-byte entries, the writer statics, the related-element
walkers as stand-ins).
