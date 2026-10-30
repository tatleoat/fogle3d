# Speed against Blender (unit speed1, 2026-10-29)

Our measurements only. Method and raw rows: `re/tools/speed1/` (`bench.py`, `run_lane.sh`, `matrix.sh`, `aggregate.py`); the full table is in
`re/out/speed1/table.md` (untracked). One frame through the add-on's Blender-scene render (scene export, blend2db, render-frames,
picture decode, hand-over to Blender), headless Blender 5.2.0, 22-core shared VM at load 6-18, Cycles on CPU (64 samples, denoise off,
4 bounces), no GPU used anywhere. Medians of 3 where there are 3 runs; the 1920x1080 rows of textured/mirror/glass/area for some lanes are single runs.
The renderer binary is the release build (already fat LTO, one codegen unit; a separate "dist" profile build gave the same speed).
Add-on default = 16 band threads (cores - 1, cap 16). Hardware x87 (`SI_X87=hw`, expected to become the default) is a separate build.

## Where it stands (unit speed2, 2026-10-29)

The textured scene's unattributed time (item 1 below) is found and removed; the textured rows are now below Cycles 64 at both sizes.
Same renderer binary before and after (pinned copy), same add-on otherwise, 16 band threads, medians of 3, load 5-6:

| scene | size | before | after | Cycles 64 (speed1) |
|---|---|---|---|---|
| textured subdivided monkey | 960x540 | 3.41 | 1.01 | 2.02 |
| textured subdivided monkey | 1920x1080 | 4.64 | 2.35 | 7.59 |
| cube + floor + sun | 960x540 | 0.58 | 0.37 (median of 4, load 9) | 1.84 |

("Before" is lower than speed1's 4.59 / 10.29 because the renderer in the tree had already become faster; the add-on part is what changed here.)
Where the time was, all in the add-on's Python and in blend2db, none in Blender itself (profiled span by span and with cProfile,
`re/tools/speed2/run_prof.sh`):

1. **Animation sampling, 1.95 s (55 % of the 960 frame).** `export.sample_range` samples every frame of the scene's range (250 in a
   factory scene) even for one still, and each sample asked for the material's diffuse colour through `_material`, which also averages
   the texture's pixels (one million floats read back and summed per frame). Now `_diffuse` returns the same value without the texture
   mean: 1.95 s to 0.005 s.
2. **scene.json writing, 0.35 s.** `json.dump` to a file uses Python's pure-Python encoder (about 850,000 small writes for 7.6 MB);
   `json.dumps` gives the same text with the C encoder: 0.14 s.
3. **Looking for the install three times per render, 0.21 s (every scene).** `install.find` walked the whole install (about 17,000
   directories) for each of three checks; a hit is now remembered while the file still exists: 0.07 s on the first render, 0 after.
4. **blend2db refitting each light on every animation frame, 0.17 s.** The light fit is now redone only when the light's colour, energy,
   position or direction changes from the frame before: blend2db 0.22 s to 0.05 s.
5. **Mesh export loops, 0.11 s.** `scene_to_dict` read vertices, polygons, triangles, normals and UVs one Python item at a time; it now
   reads each in bulk (`foreach_get`) and converts to the same lists: 0.19 s to 0.06 s.

What is left at 960x540 (1.0 s): renderer process 0.50 s (about 0.13 s of it start-up before the band threads), scene.json 0.14 s,
hand-over to Blender 0.14 s (unit speed3's), export 0.08 s, blend2db 0.05 s, the first render's install check 0.07 s.
Output is byte-identical before and after: on the textured scene the .pic and the floats handed to Blender at both sizes (every
repeat), and scene.json (250 animation frames) and the database; 47 renders of the add-on tests in separate processes (texmulti,
pbr, glass, mirror, area, lens, deform_shapekey, deform_armature, blur, dof, trim, fields: .pic and PNG pixels identical, all
RESULT ok); in one process, old against new export, animation sampling and blend2db database in all twelve test modes, 132 of 132
identical (separate processes cannot compare scene.json for pbr, glass and mirror because Blender 5.2's UV sphere gets a different
face order in every process); 39 corpus scenes with 2D textures, first frame through the tool-only direct .dsc render: 39 of 39
identical (.pic and floats handed to Blender). Tools: `re/tools/speed2/` (`run_prof.sh`/`prof.py` stage profiler, `b3_all.sh` and
`cmp_runs.py` add-on tests before/after, `b3_dual.sh` in-process old/new comparison, `abexport.py`, `abdb.sh`, `corpus_ab.sh`).

## Wall time per frame, seconds (ours = add-on default threads; hw = SI_X87=hw build)

| scene | size | ours | ours hw | ours 1 thread | Cycles 64 | Cycles 128 | verdict vs Cycles 64 |
|---|---|---|---|---|---|---|---|
| cube + floor + sun | 960x540 | 0.81 | 0.68 | 2.18 | 1.84 | 2.50 | below Cycles |
| cube + floor + sun | 1920x1080 | 1.80 | 1.81 | 7.29 | 7.44 | 9.88 | below |
| 5 spheres + floor, shadows | 960x540 | 1.67 | 1.67 | 5.56 | 2.26 | 3.28 | below |
| 5 spheres + floor, shadows | 1920x1080 | 4.22 | 5.22 | 18.68 | 8.94 | 13.65 | below |
| textured subdivided monkey | 960x540 | 4.59 | 4.61 | 11.48 | 2.02 | 2.72 | ABOVE Cycles (slower) |
| textured subdivided monkey | 1920x1080 | 10.29 | 7.64 | 31.25 | 7.59 | 10.26 | above (hw: near equal to 128 samples) |
| mirror sphere + 2 cubes (test scene) | 960x540 | 1.55 | 1.69 | 5.16 | 2.27 | 3.40 | below |
| mirror sphere + 2 cubes | 1920x1080 | 3.71 | 3.84 | 18.73 | 8.64 | n/a | below |
| glass sphere + cube (test scene) | 960x540 | 1.81 | 1.48 | 6.21 | 2.37 | 3.63 | below |
| glass sphere + cube | 1920x1080 | 6.71 | 4.11 | 36.12 | 9.41 | n/a | below |
| area light + cube (test scene) | 960x540 | 0.93 | 0.96 | 3.61 | 2.62 | 3.58 | below |
| area light + cube | 1920x1080 | 5.27 | 3.44 | 19.49 | 16.87 | n/a | below |

User CPU seconds (all threads): ours 3-54 s per frame, Cycles 64 samples 33-180 s: ours uses 3-10x less CPU on every scene except textured 1080p (54 s vs 143 s).
On one thread, ours is slower than 22-thread Cycles in wall time on every scene except cube 1080p, and still uses less CPU.
Timings under load vary by 10-30 % between repeats (see min..max in the table); the hw column is within that noise of the soft build
on everything but glass/area 1080p, where the single runs favour hw (4.11 vs 6.71 s, 3.44 vs 5.27 s); treat those two as unconfirmed.

## EEVEE

Not timeable on a GPU without starting a GPU job (needs the owner's OK). EEVEE only ran through Mesa llvmpipe (software GL, CPU):
cube 960x540 18.3 s, spheres 960x540 10.9 s, cube 1080p 20.4 s (one run each, load 23-28, 64 samples). That is the software
rasteriser, slower than every other lane, and says nothing about EEVEE on a GPU. Not measured: GPU EEVEE. Typical published GPU EEVEE
frames for scenes like these are a fraction of a second to about a second at 1080p; this is an assumption, not our number. Against that, the
band "between Cycles and EEVEE" is: below Cycles' 2-17 s and above roughly 1 s. Ours at default threads sits inside that band on cube,
spheres, mirror, glass and area (0.8-6.7 s); on textured it is above Cycles (outside the band, wrong side).

## Where the time goes (ours, default threads, median seconds, e.g. 960x540 / 1920x1080)

| scene | export | blend2db | renderer process | pic decode | hand-over to Blender | rest |
|---|---|---|---|---|---|---|
| cube | 0.00 | 0.00 | 0.42 / 1.07 | 0.01 | 0.15 / 0.60 | 0.24 / 0.11 |
| spheres | 0.04 | 0.09 | 1.07 / 3.08 | 0.05 / 0.12 | 0.15 / 0.59 | 0.28 / 0.30 |
| textured | 0.15 | 0.21 / 0.37 | 1.63 / 4.84 | 0.01 / 0.06 | 0.15 / 0.78 | 2.43 / 4.08 |
| area | 0.00 | 0.00 | 0.52 / 3.00 | 0.12 / 0.53 | 0.13 / 1.19 | 0.16 / 0.54 |

Renderer start-up (database load before the first pixel) is small: about 0.2 s (intercept of one-thread cube time against pixel count).
"Rest" is time inside Blender's render operator outside the five spans measured (Blender's own depsgraph/mesh evaluation and result
handling); its cause on the textured scene is not determined.

## Biggest known time sinks (estimates from the table; no perf: kernel.perf_event_paranoid is 4 here)

1. Unattributed time on the textured scene: 2.4 s at 960x540 (53 % of the 4.59 s frame) and 4.1 s at 1080p (40 %), outside the renderer,
   export, blend2db, decode and hand-over. Removing it would put ours at about 2.2 s (960) and 6.2 s (1080), level with Cycles 64 or just
   under it. Cause not found (a subdivided monkey and a texture file; Cycles pays none of it). Needs a profile of the add-on and Blender call.
   (Found and removed by unit speed2: see "Where it stands" above.)
2. The renderer process itself, 55-90 % of the frame on every 1080p scene (3-6 s at default threads; 17-36 s on one thread). One thread is
   3.3-6x slower than all threads at load 10, so the loop scales; the gain has to come from per-pixel cost. No per-function profile was
   possible, so no figure is given for individual functions.
3. Hand-over to Blender (the Python per-pixel loop in `_deliver`): 0.15 s at 960x540 (18 % of the cube frame, 9-16 % of others) and 0.6 s
   at 1080p (33 % of the cube frame, 14 % of spheres, 9 % of glass). A numpy/bulk conversion would save most of it: about 0.5 s per 1080p frame.
   Picture decode (`sipic.decode`) adds 0.5 s on area at 1080p (10 %).

## Profile (speed4, 2026-10-29)

The renderer process alone: `render-frames` on the databases the add-on exports for five of the scenes above plus the area-light
scene, at 1920x1080, kept by `re/tools/speed4/mkdb.sh` (db in `re/out/speed4/db/`, untracked), one frame each. CPU-time samples from
assess1's sampler (`re/tools/speed4/prof.sh`, 2000 Hz, main thread, so one band thread; `lines.py` for source lines and own time), on
the build before the changes below. Share of the render process's samples (2,400-5,700 per scene):

| scene | ray tracing (s_trace, SI_Intersect, shadow rays) | sampling (adaptive sampler, slices) | shading (SI_shade, lights) | 2D textures | output | set-up, load, geometry, BSP |
|---|---|---|---|---|---|---|
| cube + sun | 36.3 | 33.0 | 28.7 | - | 1.6 | 0.5 |
| 5 spheres | 58.5 | 18.5 | 21.6 | - | 0.4 | 1.0 |
| textured monkey | 68.8 | 15.1 | 11.3 | 2.2 | 0.6 | 2.0 |
| area light | 40.9 | 28.3 | 28.6 | - | 1.8 | 0.5 |
| mirror | 56.0 | 19.2 | 23.6 | - | 0.8 | 0.4 |
| glass | 56.9 | 15.1 | 26.5 | - | 1.1 | 0.4 |

Ranked by inclusive share (one thread): `shade` 64-83 % (it holds the shadow and secondary rays), `lights` 50-72 %, `s_trace` 35-67 %,
`SI_Intersect` (`render::intersect`) 17-49 % (49 % on the textured monkey, 24-28 % elsewhere), `trace::push` (BSP walk) 6-19 %.
By leaf crate, the x87 model (`si-x87`'s soft arithmetic: `mul_f32`, `from_f32`/`to_f32`, `add`, `mul_f64`, ...) is 41-56 % of all
samples on every scene: most of the time is spent computing exactly what the original's x87 code computes, wherever it is called from.
Picture output, set-up and the database load are under 2.5 % together. On 16 band threads the static cut of the frame into bands
(estimated cost per line) left threads idle: the slowest band took 1.4-2.7 times the mean band time, and the frame waited for it.

Two changes, both in `crates/si-render` (finding 0012, section 37). Pictures are byte-identical before and after on every check: 66
corpus first frames at 16 threads (66 identical, 0 different, 0 not compared), 30 motion-blur frames at 16 threads (30/30), the 12
Blender test modes (47 PNGs by their picture chunks and the renderer's 51 `.pic` files: all identical), and every timing run below (the
same hash in all 9 runs per scene). `cargo test -p si-render` and `-p si-mem` pass.

1. **An exact pre-filter ahead of `SI_Intersect`**: a test in doubles, with error bounds, that answers "certain miss" for most
   triangle tests (96 % of the misses in a random test set) and leaves everything else to the exact x87 test.
2. **Work stealing between band threads**: a thread that finishes its band takes the second half of the not yet begun lines of the band
   with the most estimated cost left, under the same start/end comparison that the static cut used.

Renderer process wall time, seconds, median of 3, runs of the three builds interleaved (load1 4.6-9.5 at one thread, 5.2-9.9 at 16;
shared VM, other agents building):

| scene, 1 thread | before | change 1 | change 1 + 2 | change |
|---|---|---|---|---|
| cube + sun | 2.37 | 2.10 | 2.10 | -11 % |
| 5 spheres | 4.36 | 3.69 | 3.67 | -16 % |
| textured monkey | 5.35 | 4.05 | 4.01 | -25 % |
| area light | 3.32 | 2.72 | 2.67 | -20 % |
| mirror | 4.64 | 3.88 | 3.87 | -17 % |
| glass | 5.39 | 4.72 | 4.76 | -12 % |

| scene, 16 threads | before | change 1 | change 1 + 2 | change |
|---|---|---|---|---|
| cube + sun | 0.55 | 0.50 | 0.35 | -36 % |
| 5 spheres | 0.98 | 1.02 | 0.61 | -38 % |
| textured monkey | 1.37 | 0.92 | 0.91 | -34 % |
| area light | 0.72 | 0.60 | 0.45 | -38 % |
| mirror | 1.11 | 1.04 | 0.65 | -41 % |
| glass | 1.20 | 1.03 | 0.76 | -37 % |

Change 2 does nothing on one thread (the same loop runs). User CPU time at 16 threads is 1.8-2.1 times the one-thread time on every
scene (hyper-threads and other load share the cores), so 16 band threads now give a 4.4-6.3x speed-up over one thread here (before:
3.9-4.6x). Not done, by size: the x87 model's arithmetic (41-56 % of the samples; the
same idea as change 1, exact float or double arithmetic where it provably gives the x87 result, would apply to the shading's light loop
and the adaptive sampler's weights; the `SI_X87=hw` build is the other road), and the add-on side above (hand-over, the textured
scene's unattributed time). Tools: `re/tools/speed4/` (`bench.sh` and `median.py` for timing, `gate.sh`, `blender.sh`, `pnghash.py`
and `gatecmp.py` for the identity gates, `prof.sh` and `lines.py` for the profile).

## Correction (x87chk1, 2026-10-29)
The "within noise" hardware reading (speed1) and the "41-56% soft arithmetic" profile (speed4) were both taken on builds that predate the hardware default
(speed1's `SI_X87=hw` build predates the `fx!` wrapping; speed4's worktree predates the default). The shipped build uses the hardware path: 1 thread,
1920x1080, soft -> hardware: cube 4.58 -> 2.21 s, spheres 9.47 -> 3.68, mirror 10.52 -> 3.90, glass 13.16 -> 8.86, area 6.65 -> 2.68, textured 6.98 -> 4.04
(x1.5-2.7), pictures identical in all 36 runs. The renderer's ray/shade profile on the default build has no si-x87 function among its leaves. Re-profile on the default build before more speed work.
