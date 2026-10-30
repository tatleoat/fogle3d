# Importing Softimage 3D scenes into Blender (units import1, import2, finish1, fidelity1, fidelity2, 2026-10-29)

Goal: File > Import > Softimage 3D scene (.dsc) turns a scene of the user's own Softimage|3D 4.0 install into
ordinary, editable Blender data: objects in the scene's hierarchy, meshes, curves, the add-on's Softimage materials
and textures, lights, the camera, the world, and keyframes for every frame of the scene's range. The same import
saves scenes as .blend files that open with the Fogle3D render engine already selected.

Nothing of the install is bundled or copied into the add-on. Scenes are read at run time from the install path the
user gives (add-on preference or the scene's Install dir). Textures are PNG copies in a cache folder.

Code:
- `re/tools/import1/` (crate `si-import-dump`, its own workspace): the program that reads the scene.
- `blender-addon/softimage3d/importer.py`: the operator, the menu entry, the drop handler and the builder. The file
  is called `importer.py` because `import` is a Python keyword (`from . import import` cannot be written).
- `re/tools/import1/convert_scenes.py`: the headless .blend converter.
- Tests: `run_import_test.sh` / `test_import.py`, `run_convert.sh`, `run_corpus.sh`.

## Pipeline

1. `si-import-dump --install DIR --frames all --deform SCENE.dsc OUT.json` loads the scene with the port's own
   loader (`si_rensetup`'s scene placement and set-up). The render command then runs up to the point where it hands
   the scene to the renderer. There the dump takes over the renderer's entry: it calls the renderer's start, then
   the renderer's per-frame update for every frame asked. So the scene is exactly what the renderer would draw:
   instance copies expanded, kinematics, expressions, constraints and shape animation evaluated by the port, the
   camera and lights as the renderer reads them.
2. On the first frame the dump writes:
   - the node tree and its geometry (the renderer's own tessellation for patches, faces and NURBS surfaces;
     polygons for meshes);
   - the materials and textures, with the texture coordinates the renderer interpolates.
3. On every frame it writes the matrices of the nodes that move, the camera, the lights and (with `--deform`) the
   vertices of the meshes that change.
4. `importer.py` runs the program in a temporary folder, reads the JSON and builds the Blender data.

The program is found through env `SI3D_IMPORT_DUMP`, then the add-on preference "si-import-dump", then the add-on's
`bin/`. The release zip (`re/tools/pkg/check_bundle.py`) carries it in `bin/` beside render-frames and blend2db, so
File > Import works from the zip alone with only the install path set (unit finish1:
`re/tools/finish1/run_staged_import.sh`).

## Dump format (`"format": "si3d-import-dump"`, version 1)

| Key | Content |
|---|---|
| `scene` | path, database, name |
| `settings` | `frame_first`, `frame_last`, `frame_step`, `frame_rate`, `width`, `height`, `pixel_ratio`, `right_hand`, `back_culling` (the set-up's BACK_CULLING), `ray_depth` (MAX_RAY_DEPTH), `switches` (GLOBAL_SWITCHES: `material`, `textures_2d`, `textures_3d`, `reflection`, `refraction`, `shadows`), `antialias` (`mode` FIXED or ADAPTIVE, `filter`, `level`, `threshold`; null when the set-up has none), all read from the renderer's controls after the scene's set-up is loaded (a scene without a set-up file keeps their start values), `crop` (the set-up's sub-region as pixel cut-offs `left`, `right`, `top`, `bottom` and the region's `width`, `height`; null when off), `environment` (ambience, fog, and the depth `fade`: `on`, `start`, `end`, `color`, `transparency`, as the shading reads them) |
| `models[]` | `id`, `name`, `kind` (-1 dummy, 0 null, 1 patch, 2 face, 4 mesh, 5 joint, 6 spline, 7/8 metaball, 9 NURBS curve, 10 NURBS surface), `father`, `visible`, `algorithm`, `scaling`/`rotation`/`translation`, `global_matrix`, `latdep`, `registered`, `normals_flipped`, `materials`, `texture3d` (the node's own 3D texture, an id in `textures3d`, or null), `geometry` or `geometry_of` (a node that shares another node's definition) |
| `geometry` | `type` `mesh` (`vertices`, `polygons` with -1 between contours, per-corner `normals` (often all zero on a faceted mesh; whatever they hold, the renderer uses each polygon's own normal when the mesh is faceted), `polygon_material`, `hidden`, `uv` per texture), `triangles` (the renderer's triangles, for patches, faces, NURBS surfaces), or `spline` |
| `materials[]`, `textures[]` | the records' fields (shading model, colours, exponent, reflectivity, transparency, refraction; texture method, mask, blend, channel, factors, scales, offsets, repeats, crop, picture `file` and whether it was `found`); a material's `texture3d` is an id in `textures3d` or null |
| `textures3d[]` | the 3D (procedural) texture records at the first frame: `kind`, `spacing`, `taper`, `strength`, `iterations`, `power`, `mask`, `blending`, `channel`, five `colours`, three `factors` (colour stops), `scaling`/`rotation`/`translation`, and the ambient, diffuse, specular, reflection, transparency and roughness factors |
| `camera` | position, interest, roll, fov, near, far, camera/view/perspective matrices, picture window, depth of field |
| `lights[]` | type (1 point, 2 spot, 3 infinite, 4 sun), colour, position, interest, cone, spread, fall-off (switch, start, end), shadow type (1 none, 2 ray traced, 3 and 4 depth maps), umbra, `shadow_map` (the depth map's resolution, filter size, filter step, fade start, fade rate: +0x118, +0x11c, +0x124, +0x128, +0x12c), on, selective, `lit_models` (a selective light: the ids of the models it lights, the ones whose entry in its table holds its value; null otherwise), `default` (Softimage's default light) |
| `default_picture` | the picture the renderer reads in place of a missing one (`<resdir>/noIcon.pic`), or null |
| `frames[]` | per frame: `frame`, `time`, `matrices` (moving nodes only), `camera`, `lights`, `vertices` (deformed meshes only) |
| `animated_models`, `deformed_models` | the node ids that move, and that deform |
| `not_carried`, `notes` | what this scene has that is not carried or is carried differently |
| `shaders[]` | (unit sky1) the scene's mental ray shader elements: every element of a `*_SHADERS` chapter the .dsc lists, read from `<database>/<chapter>/<element>.shd`: `id`, `chapter`, `element`, `udf` (the name the scene's references use), `file`, `found`, `type`, `library` (the library shader it instances, e.g. `Env_Atmosphere.mat`), `library_database`, `keys` (key times, seconds) and `params` (per parameter one value per key time, or its value tokens). Empty in 320 of the 334 corpus scenes |
| `scene_shaders` | (unit sky1) the scene's own references: `atmosphere` (volume shader or null), `output[]`, `lens[]`, read from the renderer's controls, else from the set-up (`"from": "setup"`) |
| shader references | (unit sky1) a material's `shaders` (`material`, `volume`, `shadow`, `photon`), a texture's, 3D texture's or light's `shader`, each `{name, active, shader}` with `shader` an id in `shaders`; absent where the record has none |

Mesh vertices are the current definition's positions (+0x48). These are the positions the renderer draws. A
shape-animated mesh's other definition only supplies the positions textures are projected from (render vertex
+0x20), and the texture coordinates in `uv` already include that.

Axes: Softimage is right-handed, Y up, with row vectors (the translation is in the last row). Blender has Z up and
column vectors. A Softimage point (x, y, z) becomes the Blender point (x, -z, y), and a matrix M becomes
C · Mᵀ · Cᵀ. Units are 1:1. A texture's Blender UV is (u, 1 − v) of the renderer's texture coordinate (u, v), the
coordinate the renderer looks the picture up with (after its mesh loader's `1 − u` and method 4's `1 − u, 1 − v`).
blend2db hands a Blender UV to the database as it is, so the lane's coordinate is the scene's (unit extra2: the UV
was 1 − both before, which mirrored every UV and projected-as-UV picture left to right; Label-Bottle 3888 of 3888
corners, `re/tools/extra2/uvcmp.py`).

## What the importer makes

- **One collection per scene**, named after it, with the child collections `<scene> camera` and `<scene> lights`.
  Objects are parented as the scene's tree has them. Dummy roots are left out.
- **Meshes** get their polygons, smooth or flat normals as the renderer has them, one UV layer per texture, and
  material slots per polygon. A polygon none of whose corners has a normal is flat, with Blender's zero custom
  normal ("the face's own"). Every polygon of a faceted mesh (mesh flag 0x2) is flat too, even when its corners store
  smooth normals (PICTURE-SKELETON's head does), and keeps the scene's winding, which gives its normal. Patches, faces and NURBS surfaces become the renderer's own triangles.
- **Splines** become curves. Joints, NURBS curves and metaballs become empties.
- **Materials** use the Softimage Material and Softimage Texture nodes (unit nodes1), plus a Principled BSDF for
  Blender's own engines. A texture the renderer projects (XY, XZ, YZ, cylinder, sphere) can become a UV texture
  with the renderer's coordinates (option "Projections as UV", on by default), so it stays put when the object moves.
  The first texture of a material does; a later projected one does too when its coordinates are the first's on every
  model that has both (they then share the render UV layer, and blend2db writes both: unit extra2, Torch_Fire's flame
  and its textured shadow). A bump map made UV would bump along the UV frame, not the projection's fixed axes, so
  a planar (XY, XZ, YZ) texture that bumps stays a projection (unit imp3): the SI Texture node keeps the method
  and the projection's scaling, rotation and translation, and blend2db, which writes the vertices in world space,
  folds the object's matrix into that rotation (`simat::texture2d_object_space`; the projection fits each axis to
  the model's bounding box, so a scaling along the projection's axes drops out). The importer does this only where
  the fold is exact for every model carrying the texture (no lattice or deformation, the object's matrix seen
  through the projection's rotation a scaling along its axes); elsewhere the texture becomes UV as before.
  A 3D (procedural) texture becomes a Softimage 3D Texture node on the material node's 3D Texture input, with the
  first frame's values. A node's own 3D texture (the renderer applies it where the material has none) goes on a
  copy of the material, named `<material> +3D texture <name>`; a node that shares a mesh but has another 3D texture
  gets object-linked slots. The renderer evaluates a 3D texture in the model's own space, but blend2db writes the
  vertices in world space, so for "This Blender scene" it moves the object's matrix into the 3D texture's scaling,
  rotation and translation (unit sky2, `simat::texture3d_object_space`). Where that has no exact form (a
  non-uniform scaling under a rotation, IK-PageTurn's grid3, grid7, grid8) the texture stays in world space;
  blend2db.txt names each case.
- **Pictures**: a .pic is converted once to a PNG. It goes to Blender's user data folder under
  `softimage3d/textures`, or to the extension's user folder; the converter puts it in `<out>/textures`. The image
  keeps the .pic's absolute path in `si3d_picture`. Images are never packed. A picture that is not there gets the
  dump's `default_picture`, as the renderer does, with a note (unit extra1: MODEL-TVset's SPARKLES sequence).
- **Camera**: place, aim, roll and lens from the vertical field of view and the picture's aspect, plus near and far.
  It is keyframed per frame when it moves. The depth of field's seven values stay on the camera
  (`si3d_dof_values`); export.py hands them to blend2db as `camera.si_dof` while Blender's own depth of field is off,
  and blend2db writes them into the camera file (unit extra1: OVERVIEW-jewel1 and jewel2).
- **Parenting**: a child under an unevenly scaled, rotated parent has a sheared local matrix that Blender's location,
  rotation and scale cannot hold. The shear goes into the child's parent inverse, so its world matrix is the
  program's global matrix (unit extra1: Blend-Plane).
- **Lights**: point, spot or sun. The colour is the record's colour made linear and divided by its largest
  component. Energy:
  - point and spot lights: 4π²·d²·peak, where d is the distance to the centre of the scene's bounding box;
  - suns: π·peak.

  Spot size is cone + spread; the blend comes from the inner and outer cosines. The Softimage values are kept for
  the engine (see "Light brightness").
- **Selective lights** (a light that lights, and casts shadows on, only some models): Blender light and shadow
  linking. One collection `<light> lit` holds the objects of `lit_models` and is the light's receiver and blocker
  collection. For "This Blender scene", export.py hands a light's receiver collection to blend2db as `lit` (the
  objects included) or `unlit` (only exclusions); blend2db writes the light file's SELECTIVE (on, value 1 for `lit`,
  0 for `unlit`) and one `CHAPTER LIGHTS CHAPTER MODELS` relation 2200 per model, so the renderer's own test
  decides.
- **Animation**: matrices are baked to linear keyframes on every frame of the range (blend2db writes a camera's
  roll as a curve of its own when it changes, unit imp3). Shape animation and deformation
  become one shape key per frame, keyed 0/1 around its frame, up to a budget of 40 M floats.
- **Scene settings**:
  - frame range, step and rate; resolution and pixel aspect (export.py hands a pixel aspect other than 1 to blend2db as the set-up's PIXEL_RATIO, x over y; the renderer widens the camera's view by it, unit fidelity2: the eleven NTSC set-ups at 0.9);
  - film transparent (a Softimage picture has alpha 0 where nothing is hit);
  - view transform Standard, no look, no dither (the engine's pictures are display values);
  - the world as a Softimage World node (ambience, fog, depth fade: switch, start, end, colour, transparency; the
    fade start may be negative, IK-PageTurn's is −2);
  - `si3d_back_culling`, and each material's Backface Culling from the set-up's BACK_CULLING;
  - the set-up's sub-region as the Render Region box, left unticked;
  - the set-up's MAX_RAY_DEPTH, GLOBAL_SWITCHES and ANTIALIAS as the engine's settings Ray depth, Materials, 2D textures, 3D textures, Reflection, Refraction, Shadows and Anti-aliasing (filter, level, threshold): blend2db writes them into the generated set-up instead of its own (depth 2, or 4 with a transparent material; all switches on; ADAPTIVE 2 2 0.05). Unit fidelity2: 154 corpus set-ups have depth 1 (no reflected rays; Thunderstorm's water reflected the island without it), 13 turn switches off, 191 differ in ANTIALIAS; with them carried dist-skull, DYNAMICS-Colorbars, INVKIN-Push, General-Skeleton and Shock3D-Bullet render pixel for pixel as the scene does;
  - render engine Fogle3D (id FOGLE3D), frame mode "One frame per run" (F12 renders the current frame alone). The
    scene's own path stays on the collection (`si3d_scene`); nothing in the render panel can render the .dsc directly
    (owner, 2026-10-29: File > Import is the only avenue for a .dsc). Tools that compare with the engine's own .dsc
    render do it through `session.set_tool_scene` / env `SI3D_TOOL_SCENE`, code only.
- A text block "Softimage import report" lists what was made, what this scene has that is not carried, and notes.

### Drop and menu

- File > Import > Softimage 3D scene (.dsc).
- A .dsc dropped on the 3D viewport or the outliner runs the same operator with the options last used. This is the
  `SI3D_FH_import_dsc` file handler (Blender 4.1+): the operator takes `directory` + `files` and runs at once when
  they are set. Several dropped files are imported one after the other.

### Light brightness

blend2db fits a Softimage colour and fall-off to each Blender light so that the engine's picture matches Cycles.
For an imported light this makes the scene brighter than the Softimage scene (fitted colour / scene colour:
Label-Bottle 1.23, Gears 1.85, CubicMap-House 3.9).

Chosen: a flag the importer sets, not a change to the fit.
- Each imported light keeps `si3d_light` (the record's colour, cone, spread, fall-off, shadow kind, umbra, depth-map values) and `si3d_light_key`
  (the Blender values the import made of them). An animated colour also keeps `si3d_light_frames`, per frame.
- `importer.softimage_light(ob, frame)` returns the Softimage values while type, colour, energy, spot size and
  blend, and shadow switch are as imported, and None as soon as the user changes one of them.
- The exporter and blend2db use it (proposed by unit import2, applied by unit finish1):
  - export.py puts `"si"` on the light dicts of `scene_to_dict` and `sample_frame`;
  - blend2db writes `l["si"]` as it is instead of `fit_light` (colour, fall-off start and end, spot cone and spread,
    umbra as the light's shadow factor), per frame too, and the light file's SHADOW_TYPE and depth-map values from
    the record (unit extra2: a Blender light's shadow switch gives only a ray-traced shadow, kind 2; five corpus
    scenes have depth-map lights: Torch_Fire, OBJECT-FLAMBEAU_MR, Weather, IK-PageTurn, MODEL-TVset).
- A light the user changed goes back to the fit. Results (13 scenes, `re/tools/finish1/import_ab.sh`): the engine's
  picture of the imported scene against the scene's own, rmse 0.0000-0.06 (before: 0.013-0.26); MATTER-SOFT_LIGHT
  0.19 to 0.0001, MODEL-Gears 0.068 to 0.0008.

### Back-face culling

- A material with Settings > Backface Culling on gets `"backface_culling": true` in the scene JSON (export.py; the key
  is left out when it is off, so other scenes' JSON stays as it was).
- The program culls the whole picture or nothing (BACK_CULLING in the set-up). blend2db writes BACK_CULLING ACTIVE when
  every material slot it draws culls, and INACTIVE otherwise, with a "not carried" note naming the mix.
- An imported scene whose set-up culls has Backface Culling on in every material, so it renders culled:
  CONSTRAINT-CameraOnCircleRolling's picture now covers what the scene's own does (overlap 0.035 to 0.988).

### Render region

- Output > Render Region works with the "This Blender scene" source too (unit finish1): the set-up blend2db wrote gets
  the region as the original's SUBREGION_FRAME (`export.patch_generated_setup`, the same patch as the "Softimage
  scene" source's), the renderer writes a picture of the box's size, and Blender gets the box. Crop on or off, stills
  and animation ranges, the Depth pass, and a cancel are covered by `re/tools/finish1/run_region_test.sh`.
- The original's sub-region picture sits one row above the box (both sources; measured: 0 of 30,000 pixels differ
  from the full picture one row up, 14,084 at the box itself).

## Converting scenes to .blend files

```
blender -b --factory-startup --python re/tools/import1/convert_scenes.py -- --scene WorkBook/MODEL-Gears [--scene ...] [--verify]
blender -b --factory-startup --python re/tools/import1/convert_scenes.py -- --all [--out DIR]
blender -b --factory-startup --python re/tools/import1/convert_scenes.py -- --list --scene /SCENES/MOTION-
re/tools/import1/run_convert.sh <same options>     # before the merge: main's add-on + this importer
```

- One .blend per scene, `<scene>.blend`: the .dsc name without its `.1-0` version, with ` (<database>)` added when
  two databases have a scene of that name. The default output folder is `re/out/blend-scenes` (untracked, local use).
- Each scene runs in its own Blender process (8 GB cap, 30 minutes). A failure is logged and skipped. Logs:
  `convert.tsv` and `logs/`.
- The file opens with the Fogle3D engine, the scene's resolution, range and rate,
  the install path in the engine's Install dir, and every 3D viewport looking through the scene camera.
- The viewports are set to Rendered shading when the engine draws the viewport. Blender offers Rendered only for an
  engine with `view_draw` (unit viewport1's viewport); without it the shading stays as the start-up file has it.
- Blender 5.2 reads Rendered shading back as Solid in every file, whatever the engine (tested with EEVEE too), so the
  saved shading alone does not survive. The converter therefore also sets the scene flag `si3d_open_rendered`. The
  importer module registers a persistent `load_post` handler that, when a file with the flag opens in a Blender with a
  window, sets every 3D view back to Rendered. The handler does nothing in a background Blender or for files without
  the flag.
- `--verify` opens each file in a new Blender process and checks:
  - engine, source, objects per type against the import's counts, camera, textures (absolute, not packed, the .pic
    present), viewport shading: the file reads as Solid, the flag and the handler are there, and the handler's view
    switch (`importer.open_rendered_views()`, run by the check because a background Blender skips the handler) sets
    every view to Rendered, looking through the camera;
  - renders as F12 does (`render.render(write_still=True)`), then with the tool-only direct .dsc render (`session.set_tool_scene`), compares the two
    pictures and holds the overlap to the import test's (within 0.01). Results: `verify.tsv`.

## Not carried

Never carried, whatever the scene:
- IK chains, constraints and expressions: baked into keyframes; the rigs are not built.
- Dynamics, particles, custom effects (.so shaders).
- Metaballs and NURBS curves: imported as empties.
- Modelling stacks: only the resulting geometry is carried.
- Patches, faces and NURBS surfaces: the renderer's triangles, not editable surfaces.
- An animated 3D texture: the first frame's values only.
- Texture band and old-factor fields, picture sequences (first picture only): custom properties only. Texture crop is carried since fidelity2 (the Softimage Texture node's Crop and its four pixel limits, blend2db's texture record +0x2e and +0x30..+0x36).
- A material's reflection map that is not in its texture list (a node's own map the renderer makes the material's): carried since fidelity2 as one more texture of the material (Env_Sunset, Env_Moonscape, Fly_Thru, Smoky_Room, Thunderstorm, Matter-News_Logo).
- Light fall-off ranges: custom properties only.

In the 334 corpus scenes (first frame), what the dump reports per scene:

| Scenes | Count | What |
|---|---|---|
| 164 | 5154 | splines (not rendered: carried as curves) |
| 96 | 1737 | skeleton joints (empties with the animation baked; no armature) |
| 35 | 91 | kinematics on a node (baked into per-frame matrices) |
| 6 | 32 | 3D textures of models (built since fidelity1) |
| 3 | 8 | NURBS curves (empties) |
| 2 | 3 | 3D textures of materials (built since fidelity1) |

Rendering differences that stay:
- A scene that mixes culled and drawn-both-sides materials renders unculled (the program has one switch).
- The render region's picture sits one row above the box (see Render region).

## Tests

- `re/tools/import1/run_corpus.sh`: the dump over all 334 scenes. 334 exit 0 in 30 s; 24,163 models, 1,614,345
  render triangles, 4,865 materials, 438 lights, 310 textures.
- `re/tools/import1/run_import_test.sh [--render] [scenes]`, 10 scenes by default:
  - drop-style operator run, objects, meshes, lights, cameras;
  - light values handed out, and withheld after a change;
  - triangles, materials, images, world-space vertices, camera at three frames, animation, deformation;
  - with `--render`: the engine's picture from the imported data against the scene's own, and a re-render with the
    scene's BACK_CULLING when it has it.
- `re/tools/import1/run_convert.sh --scene ... --verify`: the .blend converter and its re-open check.
- `re/tools/finish1/import_ab.sh <tag> <addon parent> <blend2db>`: `run_import_test.sh --render` on 13 scenes with a
  given add-on and blend2db (before/after comparisons).
- `re/tools/finish1/run_staged_import.sh`: the release zip installed as an extension in a clean environment imports
  IK-PageTurn and renders it with its own bundled programs.
- `re/tools/finish1/run_region_test.sh`: the render region with the "This Blender scene" source.

## mental ray shaders: approximations (units sky1, sky2)

The Softimage renderer this project reproduces never ran the mental ray shaders a scene can carry: Softimage handed
them to mental ray, which is not part of the port. Fourteen corpus scenes carry such shaders, all in
mental_ray/MR_Effects. Without them, a sky dome renders as its plain material (grey or flat) and Env_Sunrise's water
shows the static picture the texture names. For the sky and the water, Fogle3D now renders an approximation that
follows what each shader is meant to show. **This is not the bit-accurate path**: the shaders' code is not ported
(the irix/nt binaries are not part of the port), and the look was matched by eye to the shader's preset icons.
These icons are mental ray renders of the three scenes at their first and last key, shipped with the shader library
(re/out/sky1/icons).

| Shader (library) | Where (dump ids) | Intent | Fogle3D |
|---|---|---|---|
| Env_Atmosphere.mat (material shader) | the sky dome: Env_Sunrise mat1 (sphere2), Env_Sunset and Env_Moonscape mat4 (dome "sky") | A physically based sky: Rayleigh and Mie scattering over a planet, sun disc and halo, a moon lit by the sun, stars, a cloud layer, gain. Keyed over the scene: Sunrise goes from pre-dawn to sunrise, Sunset from a high sun to after sunset, Moonscape from twilight to night | approximated |
| Water.tex (2D texture shader) | Env_Sunrise texture 0 (grid6, in place of marbre4) | Moving waves: wave components, trough and crest colours, driving the texture's colour and bump | approximated |
| March_Fractal.vol (volume) | material volume: Bang mat1, City_Sign mat1/2/4/5, Hawaiian_Fog mat1, Thunderstorm mat9; atmosphere: Fly_Thru, Hawaiian_Fog, Spotlights, Sunbeams (inactive in Env_Sunset, Env_Moonscape) | Ray-marched fractal fog: explosions, light beams, mist | not approximated |
| March_Smoke.vol (volume) | atmosphere: Gitane, Smoky_Room | Ray-marched smoke | not approximated |
| lightning.vol (volume) | Thunderstorm mat1 | A lightning bolt | not approximated |
| Fur.out (output shader) | Cow (five instances), Warrior | Fur drawn over the finished picture | not approximated |

The volumes and fur would need a ray marcher or a hair pass in the engine. The import names them in its notes
("mental ray shaders not approximated: ...") and renders the scene as before.

**Opt-in by data.** Everything happens in `blender-addon/softimage3d/mrfx.py`, and only when the import dump's
`shaders` table holds Env_Atmosphere or Water. A scene without them, and a .blend imported before unit sky1, renders
exactly as before. Over the 334 converted corpus files (re/out/fidelity2/blend-final), every picture is identical
with and without this unit (re/out/sky1/ab-main, ab-sky).

**At import** (`mrfx.import_shaders`, called at the end of `importer.Builder.run`):
- The dome objects of an Env_Atmosphere material are flagged `si3d_mr_dome`, excluded from rendering and drawn as
  wire. The shader record (its keyed parameters) is stored on the scene as `si3d_mr_sky` (JSON, with the frame rate).
  Film > Transparent, which the importer turns on for every scene (a Softimage picture is transparent where nothing
  is hit), is turned off again: under mental ray every ray that misses the scene hits the dome, so the sky is the
  background (unit sky2).
- Under mental ray a reflected ray that misses everything hits the dome, so it shows the sky. The importer gives the
  same result in three ways:
  - A reflective material without a reflection map gets a latitude-longitude picture of the sky as its reflection map,
    with method 7 (traced first, the map where nothing is hit): Env_Sunrise's water, for example.
  - An existing method-7 map (Env_Moonscape's CITY-new_york picture) is replaced by the sky.
  - A method-8 map (a map instead of rays, Env_Sunset) is kept.

  The sky picture (`si3d_mr_skymap`) is packed into the .blend.
- A Water texture's picture becomes a generated one (`si3d_mr_water`, 512 × 512, no crop), drawn from the shader's
  parameters. The engine's own texture blend and bump use it as they would use any picture.

**At render** (`export.render_blender_scene`):
- `mrfx.render_textures` rewrites the textures folder's `.raw` of each flagged picture for the frame being rendered:
  the water at that time, and the sky map with the sun, moon and clouds where they are at that time.
- After the engine's frame, `mrfx.render_composite` traces one ray per pixel from the evaluated camera through the
  sky and puts that colour behind the picture (premultiplied: colour plus (1 − alpha) × sky; alpha is 1 afterwards).
  It handles render regions and interlaced fields. It returns the decoded picture (`mrfx.Picture`, which remembers
  the engine's .pic so that the Depth pass still reads the depth file beside it), or None when the scene has no sky
  or film transparency is on; then the engine's .pic goes through untouched and the background stays transparent
  (unit sky2).

**Switching the sky off** (`mrfx.switch_sky(scene, on)`, unit sky2): moves the shader record between `si3d_mr_sky`
and `si3d_mr_sky_off`, shows or hides the dome objects for rendering and sets Film > Transparent to the opposite
(on for the sky off, as the importer leaves any scene), so that F12 renders the engine's own picture with the dome
as the original set-up has it. It returns whether the scene has a sky at all. The import test
(re/tools/import1/test_import.py) switches the sky off before its render checks, which compare against the
Softimage engine.

**The sky model** (`mrfx.Sky`, numpy only): single scattering on a grid of view elevation × azimuth relative to the
sun, with the phase function evaluated per pixel. It adds a sun disc and halo, a moon lit from the sun's side, stars
faded by the sky's brightness, and fractal clouds on a shell at the cloud altitude (coverage clip(2F − 1) of the
fractal F, lit by the sun's colour along the path). Below the horizon it shows the horizon haze fading over a few
degrees, since the planet hides the bodies. The water (`mrfx.water`) is a sum of sine waves with deep-water
dispersion (speed from wavelength), mapped from trough to crest colour with the shader's bias, and with height in
alpha.

Constants that are not the shader's (calibrated by eye against the six icons, re/out/sky1/calib/c9.png):
Rayleigh coefficients twice Earth's (5.8, 13.5, 33.1)·10⁻⁶ /m, Mie 1.5·10⁻⁴ /m per unit dust with g 0.8, sun-path
share 0.03, moon albedo 0.08, moon size 0.4° per unit, ground fade 0.05 rad, cloud ambient 0.004, noise gain 2. These
constants set the overall mood, not the pixels: a different constant gives a different but equally plausible sky.

**Measured** (fidelity sweep, Softimage-engine reference, which has no sky; re/out/sky1/sweep-sky3): the three
scenes now differ from that reference on purpose. Env_Sunrise goes from match (`bad` 0.020) to partial (0.935, the
sky fills the frame), Env_Sunset from match (0.050) to wrong colours (0.563), and Env_Moonscape from match (0.039)
to wrong colours (0.637). The other eleven shader scenes keep their fidelity2 numbers. Frames 1, 30, 60 and 100 of
the three scenes are in re/out/sky1/frames2, and 4K stills in re/out/4k-sky. A frame takes 2 to 3.6 s at the
scene's size, against 0.5 to 1.4 s before.

**Checks** (unit sky2): the import test of the three scenes, with the sky switched off, passes (render overlap
Env_Sunrise 0.996, Env_Sunset 0.976, Env_Moonscape 0.987; re/out/sky2/regress-sky). Their earlier `images` failure
(5 of 8 and 5 of 7 pictures) was the test's own: it counted the dump's textures, and these scenes use one picture in
several textures; it now counts distinct picture files. With the sky behind the picture the Depth pass holds the
engine's depth (Env_Sunset at 25 %: every pixel opaque, 49.7 % with a finite depth, 2.45 to 161; Env_Sunrise and
Env_Moonscape alike), and with Film > Transparent on only the objects are opaque (Env_Sunset 47.7 % of the pixels;
re/tools/sky1/check_passes.py, re/out/sky2/passes).

Limitations:
- In an animation render of deforming objects (one run over the range), the water and sky maps keep the first
  frame's pictures; only the sky behind the picture moves.
- Cycles, EEVEE and the viewport show no sky: no Blender World texture is made.
- The sun is seen but does not light the scene: the scene's own lights do, as in the original set-up.
- The reflection map's orientation was checked by eye only (the water reflects the sky and sun where they are).
