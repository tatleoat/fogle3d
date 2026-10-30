# Animation and physics from Blender scenes (unit anim1, 2026-10-29)

Goal: a Blender scene rendered by the engine moves and deforms in the Softimage renderer as it does
in Cycles. That covers armatures, shape keys, lattices, cloth, soft body, rigid body, geometry nodes, particles, fluid
and hair, in F12 stills and in animation renders, with motion blur. A scene where objects only move keeps its database
byte for byte, on the path it used before.

Blender evaluates everything. Blender's physics (cloth, soft body, fluid, particles) cannot be re-run with the program's
own dynamics: the solvers differ, so the pictures would not match Blender's. The database therefore carries what Blender
computed, in the forms the program already reads: function curves for motion, shape animation for deformation, and one
model per instance.

Code:
- `blender-addon/softimage3d/animexport.py`: classes, routes, sampling, the side file, the range run.
- Hooks in `export.py`: instances, hair curves, hidden instancers, the range route in `render_blender_scene`, and blur
  shapes.
- blend2db `re/tools/builder2/blend2db/src/shapes.rs`, plus the shaped-model branch and the hold-fill of matrices in
  `build.rs`.
- Tests: `re/tools/anim1/run_anim_test.sh <mode>` (`tests/anim_test.py`).
- Regressions: `re/tools/anim1/regress.sh` and `compare_regress.py`.

## Object classes

The add-on samples each candidate object's evaluated geometry at every frame the database needs. A candidate is a mesh
with modifiers, shape keys or animated mesh data, or any curve, surface, metaball, text or Curves (hair) object. It then
compares two things from frame to frame:
- the topology signature: vertex count, loop count, and a blake2b hash of the loop vertices and polygon loop starts;
- the local vertex positions.

Triangles are not part of the signature. Blender re-triangulates a deformed quad along its shorter diagonal, so a hash
over triangles would call a bending cloth "topology changes".

| Class | Decided by | Carried as | Examples tested |
|---|---|---|---|
| static | not a candidate, matrix constant | the still database, no curve | floor, sphere |
| transform-only | matrix changes, geometry constant | function curves of the model's SRT (relation 15000), the existing path | rigid body cubes, keyed objects |
| deforming | same signature every frame, positions differ | Softimage shape animation: one shape per sampled frame (below) | armature, shape keys, cloth, soft body, geometry-nodes wave, hair |
| topology changes | signature differs between frames, or the object is missing in some frames | a database per frame (window route) | metaballs merging, fluid meshes, remesh, booleans |
| instances | `inst.is_instance` (particles, geometry-node instances, collection instances) | one model per instance, keyed `instancer>object#persistent_id`, with its own curves | instanced particles |
| hair curves | Curves object | camera-facing ribbons, one quad strip per curve, deforming class | Curves hair |

The instance key stays the same from frame to frame. The object's own name would give every particle its source
object's motion. When an instance is missing from some frames (a particle born late or dead early), blend2db fills those
frames with the nearest present matrix. A scene whose instance set changes goes to the window route, so a frame never
shows a particle that is not alive. An instancer hidden in renders (a particle emitter with "Show Instancer" off) is
not exported.

Not carried, each reported as a "not carried" warning of the render:
- volume objects;
- legacy particle-system hair (`ParticleSystem` type HAIR, drawn as strands by Blender's renderers only), with a note
  to convert it to a Curves object;
- grease pencil.

Volume shaders on meshes and the world volume are ignored without a warning.

## Routes (how the frames are exported)

An animation render (Render Animation) with frame mode "Scene range" samples the whole range once at the first frame
Blender asks for, then decides one route for the render job:

- **plain**: no candidate deforms, so the existing path is kept untouched. Each Blender frame gets the range database
  with function curves, and pure-transform scenes stay byte-identical.
- **range**: at least one object deforms and no topology changes.
  - The still is the first frame's evaluated scene, and the deforming objects become shape animation.
  - The range is cut into contiguous chunks, about 3 per renderer process and fewer frames when a chunk's shapes would
    pass the memory budget. Each chunk is a database holding only the shapes of its own frames, plus the shutter frames
    when blur is on.
  - blend2db builds the chunks in parallel. `chunk_child` then runs the renderer over them, K processes at a time
    (`RENDER_RANGE`), and every Blender frame is served from that one run.
  - A chunk of one frame is widened to two frames, because blend2db reads a single animation entry as a still.
- **window**: the topology changes, the instances change, or the camera moves while motion blur is on. Every frame is
  rendered alone with its own evaluated geometry, as F12 does.

An F12 still, a frame mode other than "Scene range", or fields render the frame alone. With motion blur on, the
deforming objects get the shapes of the shutter's frames (`blur_shapes`), so a deformation inside the shutter is blurred.

### The shape side file

The add-on writes one file per deforming object: b"SISHP1", then u32 frames, u32 vertices and u32 triangles (all
little-endian), then i32 frame numbers. After that, for each frame, come f32 positions (vertices x 3) and f32 corner
normals (triangles x 3 x 3), in Blender world space. The triangles are the still's, in the order of the object's `tris`
list. Each still triangle corner is matched to a loop of the sample by (polygon, vertex) through a sorted-key search
(`still_loops`); a mismatch sends the scene to the window route.

blend2db turns every sampled frame into one shape, in the layout the corpus's shape scenes use (MOTION-sparkles,
AnimResample-Kiss):
- a SHAPES element `<model>Shp`, with relation 200 from the model;
- an animation element of chapter SHAPES whose type-8 curve holds the shape index less one, one linear key per frame,
  with relation 15010;
- a constant scaling curve (relation 15000), so the program counts the model as animated.

The positions are in world space, so a shaped model has no transformation curve. The program blends the two shapes
around each time itself. It does this at every frame and at every step of its motion-blur walk, so deformation blur
comes from the program, not from us.

### Motion blur of deformation

The blur walk asks how far each model moves (the program's `maxPsMovement`). The answer is -1, meaning "not moving",
when the model has a different number of render vertices at shutter start and end.

The renderer makes one render vertex per run of polygons around a vertex, between marked edges. It marks an edge whose
corner normals on its two sides differ by more than 0.01. A flat-shaded mesh that bends therefore gains render vertices
as it bends: a subdivided cube's side is coplanar quads with one normal until they fold. The first blur test
(`run_anim_test.sh blur`, a box bent by an armature) gave a blurred F12 picture equal to the sharp one, 0 pixels
differing.

blend2db now takes the union over all definitions (the still and every shape) of the edges whose sides differ by more
than 0.005, and gives those edges the user discontinuity bit (0x08) in every definition. The renderer marks those edges
first, so the render-vertex count is the same at every time. Such an edge is shaded hard even where its normals were
within 0.005..0.01, which looks the same.

## Cost

Measured headless at 240x180 on this VM, the renderer at 1 thread per process. The animation numbers include
sampling, the database builds and all renderer processes.

| Test | Route | Frames | Seconds | Per frame |
|---|---|---|---|---|
| cloth on a sphere | range | 40 | 0.97 | 0.024 |
| soft body | range | 30 | 0.57 | 0.019 |
| shape key + armature | range | 10 | 0.34 | 0.034 |
| geometry-nodes wave | range | 12 | 0.40 | 0.033 |
| hair (Curves) | range | 6 | 0.57 | 0.095 |
| rigid body cubes | plain (existing path) | 40 | 2.11 | 0.053 |
| instanced particles | plain | 20 | 1.53 | 0.077 |
| metaballs (topology changes) | window | 12 | 1.05 | 0.088 |

**Timing mode** (`run_anim_test.sh timing`): a cube on a floor, two point lights, 640x480, frames 1..24. The machine had a
load average of about 7 on 22 cores while it ran.

| Scene | Softimage 3D animation, per frame | Cycles 16 samples, no bounces | Cycles 128 samples, 12 bounces | EEVEE |
|---|---|---|---|---|
| cube moving (plain route) | 0.126 s (24 frames 3.01 s) | 0.37 s | 1.39 s | 0.57 s (first EEVEE frame, shaders compiled) |
| cube twisted by a simple deform modifier, 8 cuts (range route) | 0.089 s (24 frames 2.12 s) | 0.31 s | 1.47 s | 0.34 s |

The Softimage numbers are one animation render divided by its frames, with up to 16 renderer processes at a time. The
Blender numbers are one frame each. Per frame, our animation is faster than EEVEE here. The range route is faster than
the plain one on the same cube because it samples the range once.

Memory cost:
- side file: 12 bytes per vertex plus 36 bytes per triangle, per frame;
- renderer: about 0x14 bytes per vertex plus 0xc0 bytes per triangle, per shape it holds;
- the chunk length keeps one database under 700 MB, or half the memory per process if that is lower.

Sampling is Blender's own evaluation: one `frame_set` and one `to_mesh` per candidate per frame. The physics caches are
filled by that pass.

The plain route keeps the existing path's cost. The range database is rebuilt for each Blender frame, so a range of N
frames samples N x N frames. That is unchanged by this unit.

## Comparison and recommendation

| Method | Topology changes | Deformation blur | Cost | Pictures vs F12 still |
|---|---|---|---|---|
| A. A database per frame (snapshot of the evaluated scene) | yes | only through shutter shapes (`blur_shapes`) | a blend2db build and a renderer load per frame | identical (the same database) |
| B. Shape animation in a few chunk databases (range route) | no | yes, by the program | one sampling pass, a few builds, K parallel processes | mean 0.4 to 3.1/255 apart (light fit, below) |
| C. A point-cache file read by the renderer | yes | yes | small | would need a reader the program does not have, a change to the original renderer; rejected |
| D. Re-simulating in the program's dynamics | no | yes | large | different solvers, does not match Blender; rejected |

Recommendation, as implemented:
- B for deforming objects with constant topology;
- A (window route) when the topology or the instance set changes;
- the existing path untouched for transform-only scenes;
- shapes over the shutter for blurred stills and window frames.

**Light fit.** blend2db fits the lights and ambience against Blender's (`fit::fit_light`, `fit_ambient`) using the
still's geometry. Every chunk database of a range shares one still, the range's first frame, so later frames are lit
by a fit made on the first-frame geometry. An F12 still of the same frame fits on its own geometry. The two pictures are therefore a little
apart: 0 pixels differ at the first frame, and the mean difference is 0.4 to 3.1/255 at later frames. The geometry is the
same: the experiment `re/tools/anim1/shape_experiment.py` renders a shaped database and per-frame baked databases with 0
pixels differing when the still is the same frame. Fitting once per range keeps an animation from flickering.

## Blender-only effects

| Effect | Mapping | Status |
|---|---|---|
| Volumes (Volume objects, Principled Volume, world volume) | No volume model in the renderer. A world volume's density and colour could become the set-up's layer fog or depth fade (the Softimage World node sets them by hand). | not carried; Volume objects get a warning |
| Hair shading | Curves become ribbons a pixel wide or more, with the object's material as a flat Softimage material: Principled base inputs, or the viewport colour for a Hair BSDF. No strand lighting (Kajiya-Kay or Marschner). | geometry carried, shading approximated |
| Displacement | Geometry displacement (Displace modifier, geometry nodes, subdivision) is in the evaluated mesh and carried. Shader displacement (Material Output > Displacement, adaptive subdivision) is not: bake it into a Displace modifier on a subdivided copy. | modifier and geometry nodes yes; shader displacement no |
| Procedural shaders (Noise, Voronoi, Wave, colour ramps) | Not read; the material path reads Principled base inputs and image textures. Planned bake: Cycles bake of Base Color (DIFFUSE colour pass) into a UV image per object and material, which the image-texture path then exports. It costs a bake per object, and per frame when the shader is animated, so it should be an explicit operator, not part of every render. | not implemented (design only) |

## Tests

Each test runs `re/tools/anim1/run_anim_test.sh <mode>`, all at 240x180 with Cycles at 16 samples. For each checked
frame it makes one animation render (Render Animation through the engine), a Cycles reference and an F12 still.

| Mode | Scene | Route | Overlap with Cycles (still / anim) | Change, first to last frame |
|---|---|---|---|---|
| cloth | cloth plane falling on a sphere, frames 1/20/40 | range | 0.95 / 0.95, 0.98 / 0.99, 0.98 / 0.99 | centroid move Cycles (-2.2,36.3), ours (-1.9,35.7) |
| softbody | soft-body icosphere falling | range | 0.97-0.98 | (0.0,70.1) vs (0.0,69.6) |
| rigid | rigid-body cubes falling | plain | 0.95-0.97 | (-1.7,69.5) vs (-1.7,69.3) |
| skarm | shape key + armature (bone turns about Z) | range | 0.97 | (-7.5,1.7) vs (-7.4,1.0) |
| gn | geometry-nodes wave on a grid | range | 0.97-0.98 | IoU 0.53 vs 0.53 |
| particles | instanced cubes from an emitter | plain | 0.92-0.94 | (-12.7,56.5) vs (-12.3,57.9) |
| fluid | metaballs (topology changes) | window | 0.94-0.95 | IoU 0.00 vs 0.00 |
| hair | Curves hair swaying | range | 0.94-0.96 | (17.6,-0.1) vs (18.1,-0.3) |
| blur | box bent by an armature, shutter 1 | range + F12 blur | 0.89 (blurred, both) | 1479 / 3632 pixels differ from sharp (F12 / anim); F12 vs anim 0.03/255 |

Overlap is the overlap of the red-object masks.

**Regressions** (`re/tools/anim1/regress.sh <suite> <mode>`, outputs under `re/out/anim1/regress/`): each mode was run
twice, with this add-on and blend2db and with those of main 651e7a5 (`BASE_ADDON`, `BASE_B2D`, outputs under
`regress-base/`). `compare_regress.py` then compares every database file and picture of the paired runs.

Totals, over builder2 shadow, calib, texture, anim and ortho plus blender3's 12 modes: 939 files compared, 914
identical, 25 different. Every .pic is identical. All 25 different files are the test sphere's `.hrc`, and in each of
those 25 pairs Blender gave the sphere's polygons in a different order. The same happens between runs of the baseline
code itself. Where the order is the same, the file is identical.

builder2 has no mode "ambient"; the ambient rows are part of calib. blender1 `run_test.sh`: 0 of 28,552 pixels differ.
pkg `run_install_test.sh`: pictures identical 3 of 3.
