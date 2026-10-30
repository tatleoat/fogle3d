# Import fidelity: the .dsc rendered directly vs. the imported Blender scene (units fidelity1, fidelity2)

Status: the combination checked 2026-10-30 (final1). The full sweep is extra2's of 2026-10-29 over all 334 corpus
scenes, reconverted with every fix below but sky2's (re/out/extra2/sweep-all); sky2's and extra1/extra2's fixes
together were re-measured on 28 scenes, those either of them touches and five controls (re/out/final1/sweep).
fidelity2's final sweep of 2026-10-29 is the earlier column.

## What is measured

Every corpus scene (334) converted to a .blend by `re/tools/import1/convert_scenes.py` is rendered twice by the
Fogle3D engine in headless Blender:

- **reference**: the engine's own direct render of the .dsc (tool-only, `session.set_tool_scene`; not in any panel) (full frame, the scene's resolution);
- **blender**: the Blender scene (F12), the converted objects, lights, materials and world handed through
  export.py and blend2db to the same renderer.

`re/tools/fidelity1/sweep.py` scores the pair. Until fidelity1's last run the blender lane was rendered at 320 px on
the long side and the reference box-averaged down to it. The final run uses `--size 0`: both pictures at the scene's
own resolution, so nothing is resampled. Colours are compared times alpha (black background).

Metrics:

- coverage (alpha > 0.5 and some colour), overlap of the covered pixels (IoU), RMSE of the colour (fidelity1);
- `shape` (fidelity2): the share of the pixels either picture covers (alpha > 0.5, dark or not) that the other covers
  within one pixel;
- `bad` (fidelity2): the share of those pixels whose colour differs by more than 0.1 in any channel from every pixel
  of the other picture within one pixel.

The two fidelity2 metrics forgive a one-pixel shift of an edge and count dark covered pixels, which fidelity1's mask
dropped (IK-PageTurn's near-black floor made it "partial").

| band | rule |
|---|---|
| failed | a lane gave an error |
| empty | both coverages < 0.002 (nothing to see in either) |
| blank | blender coverage < 0.1 × the reference's, or the blender spread < 0.012 while the reference's is ≥ 0.03 |
| match | `bad` ≤ 0.05 |
| wrong colours | `bad` > 0.05 and `shape` ≥ 0.9 |
| partial | `shape` < 0.9 |

The baseline and finish1 columns were banded with fidelity1's rule (match: overlap ≥ 0.9 and RMSE ≤ 0.06; wrong
colours: overlap ≥ 0.9; partial: the rest). The final run is given under both rules.

## Band counts

| band | baseline | after finish1 merge | final, fidelity1 rule | final, fidelity2 rule | extra2 | combined |
|---|---|---|---|---|---|---|
| match | 97 | 197 | 279 | 266 | 300 | 297 |
| wrong colours | 99 | 23 | 7 | 33 | 4 | 6 |
| partial | 101 | 80 | 18 | 5 | 0 | 1 |
| blank | 5 | 4 | 0 | 0 | 0 | 0 |
| empty | 6 | 6 | 30 | 30 | 30 | 30 |
| failed | 26 | 24 | 0 | 0 | 0 | 0 |

- **Baseline**: the owner's .blend files (re/out/blend-scenes) with the extension as installed before finish1, at
  320 px (re/out/fidelity1/sweep-base). Of the 26 failures, 24 are scenes without a mesh; the other 2 were bad_alloc
  under a 2 GB memory cap (sweep.py now uses 6 GB).
- **After finish1 merge**: the same files, with finish1's light pass-through, back-face culling, region and depth
  handling, at 320 px (re/out/fidelity1/sweep-merged).
- **Final**: every scene reconverted with fidelity1's and fidelity2's fixes (re/out/fidelity2/blend-final), measured
  at the scene's resolution (re/out/fidelity2/sweep-final, table.tsv there). 118 scenes are pixel-identical
  (RMSE 0).
- **extra2**: every scene reconverted with extra1's and extra2's fixes as well, without sky2's
  (re/out/extra2/sweep-all), fidelity2 rule. 129 pixel-identical.
- **Combined**: extra2's table with the 28 rows final1 re-measured on blends converted with sky2's, extra1's and
  extra2's fixes together (re/out/final1/sweep; references made afresh, pixel-identical to fidelity1's, 28 of 28): 24
  match, 3 wrong colours, 1 partial there. None is worse than in extra2's sweep but Env_Sunrise, Env_Sunset and
  Env_Moonscape, which differ by design (open cause 3; sky1 measured the same `bad`); the other 25 keep their band
  with an equal or lower rmse (DYNAMICS-Stairs now pixel-identical: 130 in all). The other 306 scenes were not
  re-measured.

## Causes fixed

1. **No mesh in the scene (24 "failed").** Curves, nulls or nothing; the reference is empty too. export.py
   `render_blender_scene` now returns an empty picture with an INFO report; these scenes band "empty".
2. **Depth fade (18 set-ups).** The dump writes `settings.environment.fade`; the importer sets the SI World node's
   fade inputs.
3. **3D (procedural) textures.** The dump writes a `textures3d` table and each model's and material's 3D texture;
   the importer builds an SI 3D Texture node (a model's own 3D texture goes on a copy of its material). IK-PageTurn's
   panels, particle-system, TMP-AIKIDO_C.
4. **Faceted meshes (dist-skull, dist-cow, dist-foot, dist-tricera).** A face without corner normals is made flat;
   dist-skull went from RMSE 0.0177 to 0.0005.
5. **Selective lights (10 scenes).** The dump writes each light's `lit_models`; the importer makes Blender light and
   shadow linking; export.py hands it to blend2db ("lit"/"unlit"), which writes SELECTIVE and the light-model
   relations. fidelity2 fixed a crash in export.py `_light_linking` (reading each object's link state).
6. **Texture crop (fidelity2).** The crop rectangle of a 2D texture (Env_Sunset's sky, Torch_Fire's flame) is carried:
   SI Texture node properties, the material record, blend2db's texture file. Env_Sunset `bad` 0.255 to 0.196,
   Torch_Fire RMSE 0.050 to 0.040.
7. **Reflection map (fidelity2).** A material's reflection map picture is carried as a texture of the material.
8. **Set-up ray depth, global switches and anti-aliasing (fidelity2).** blend2db's template set-up has ray depth 2 and
   ADAPTIVE 2 2 0.05; the corpus has 154 set-ups with depth 1, 96 with 2, 13 with switches off (material, 2D and 3D
   textures, reflection, refraction, shadows) and ten different ANTIALIAS lines (143 ADAPTIVE 2 2 0.05, 47 FIXED 2 2,
   41 ADAPTIVE 2 5, 29 FIXED 1 1, 19 FIXED 1 2, others). The dump writes them, the importer sets the new scene
   settings (ray depth, anti-aliasing, six switches), export.py writes them into the set-up words. Thunderstorm's
   water lost the reflections the reference does not have; DYNAMICS-Colorbars, INVKIN-Push, Shock3D-Bullet and
   General-Skeleton became pixel-identical.
9. **Pixel ratio (fidelity2).** Eleven corpus set-ups are NTSC with PIXEL_RATIO 0.9; the renderer widens the
   camera's view by that ratio (the picture's aspect is the resolution's times the pixel ratio), blend2db's template
   has 1. The importer already set Blender's pixel aspect; export.py now hands PIXEL_RATIO (pixel aspect x over y)
   to the set-up. scn2db-REGISTER `bad` 0.274 to 0.001, Warrior 0.194 to 0.001, scn2db-NTSC_BARS_77 from partial to
   match, Env_Sunset 0.196 to 0.050, Env_Moonscape 0.181 to 0.039, Thunderstorm 0.112 to 0.001; all nine with a
   picture now match.
10. **3D textures in the object's space (unit sky2).** Softimage evaluates a 3D texture at the point in the model's
   own space. blend2db writes every mesh's vertices in world space (the object's matrix baked in, the model's
   transformation the identity), so the texture saw world coordinates: on Sand_Gust's sky sphere (scaled 204 ×) a
   pattern 204 times finer, stars instead of clouds. blend2db now moves the object's matrix into the 3D texture's
   own scaling, rotation and translation (`texture3d_object_space`). That is exact when the object's matrix is a
   scaling along its axes, or when the product is still a scaling followed by a rotation (a rotated object whose
   texture is scaled the same along its three axes, for one). A non-uniform scaling, the object's or the
   texture's, under a rotation has no exact form in the texture's transformation, so such an object keeps its
   texture in world space as before (blend2db.txt says which; IK-PageTurn's grid3, grid7 and grid8). Over all 334 scenes (re/out/sky2/ab-t3d3 against re/out/sky1/ab-main) 327 are pixel-identical and
   7 change, `bad` before and after: DYNAMICS-Stairs 0.0038 to 0 (now pixel-identical, 119 in all), Missile 0.0253
   to 0, particle-system 0.0384 to 0, TMP-AIKIDO_C 0.0291 to 0.0282, IK-PageTurn 0.0323 to 0.0315, Sand_Gust 0.280
   to 0.256, Weather 0.0005 to 0.001 (the sky's clouds now show, so the right tree's thin branches over them count
   40 more pixels; RMSE 0.0247 to 0.0211); RMSE goes down in all seven. Bands are unchanged.
11. **Parent shear (extra1).** A child under an unevenly scaled, rotated parent has a sheared local matrix that
   Blender's location, rotation and scale drop; the importer puts the shear into the child's parent inverse.
   Blend-Plane `bad` 0.0238 to 0.0011, Ambulate-the_walk wrong colours to match.
12. **Camera roll sign (extra1).** blend2db wrote the negated roll (`cross(level, up)`); both CONSTRAINT-Camera scenes
   went from partial to pixel-identical.
13. **Camera depth of field (extra1).** The camera's seven depth-of-field values are carried (importer, export.py
   `camera.si_dof`, blend2db's camera file): OVERVIEW-jewel1 and jewel2 match (this was the open cause "blur").
14. **Missing picture (extra1).** The renderer reads `<resdir>/noIcon.pic` for a picture that is not there; the dump
   writes `default_picture`, the importer loads it (MODEL-TVset's SPARKLES sequence).
15. **UV mirrored left to right (extra2).** The importer's UV was 1 − both of the renderer's coordinates; the
   renderer's mesh loader keeps (1 − u, v) of a mesh's pair and its UV method takes 1 − each, so the coordinates
   that came back through the exporter were (1 − u, v): every UV and projected-as-UV picture mirrored in u
   (`re/tools/extra2/uvcmp.py` matches corners by position between the scene's dump and the lane database's dump:
   Label-Bottle 3888 of 3888 corners mirrored before, 3888 the same after). The UV is now (u, 1 − v). This was the
   open causes "high-frequency textures", "XY projection mirrored", TVset's picture and "checker phase": on the 40
   textured scenes nine go up a band (MODEL-TVset, Label-Bottle, Puzzle-PuzzleGame, CubicMap-House, Limit-Fruits,
   MATTER-WhatstheMatter2, Duplicator-Necklace, Motion-Roller_Coaster wrong colours to match; keyboard-MotionCap
   partial to match), none down.
16. **Depth-map shadows (extra2).** blend2db wrote every imported shadow as ray traced (kind 2); it now writes the
   record's kind and the depth map's resolution, filter size and step, and fade (dump `shadow_map`). Five scenes
   have depth-map lights: Torch_Fire, OBJECT-FLAMBEAU_MR, Weather, IK-PageTurn, MODEL-TVset.
17. **A second projected texture (extra2).** Only the first texture of a material became UV and blend2db carried
   only the first global texture. A later projected texture with the same coordinates as the first on every
   model now becomes UV too, and blend2db writes it (Torch_Fire's flame grid: flamme and fire_shade2). With 16,
   the flame's shadow is there: Torch_Fire wrong colours (rmse 0.04) to match (0.0042), OBJECT-FLAMBEAU_MR wrong
   colours (0.0401) to match (0.0068).

## Open, by cause

1. **Thin or tiny pictures.** Skeleton and calibration scenes (EXO_MAG_SOLO-*_CALIB, MC-*, PICTURE-SKELETON) cover
   2 to 7 % of the frame; one-pixel differences of thin limbs dominate `bad`. MC-5_mains' reference is 32 × 27 pixels.
   Look at the pictures rather than trust the band.
2. **Sand_Gust terrain.** The sky is fixed (cause 10: clouds, as in the reference, instead of stars). The terrain
   (grid7) still differs: the reference is dark with a coarse grain at the front right, the conversion has a fine,
   lighter grain all over ("wrong colours"; `bad` 0.234, rmse 0.078 with all fixes combined, `bad` 0.256 with sky2's
   alone). Its 3D texture is now in the object's space; the rest of its material is three 2D textures (TILES.1, UV,
   15 × 15 repeats, roughness 14.35; marbre4 twice, XZ, cropped).
   Not found; the 2D textures' roughness (bump) under the model's scaling of 110 × 133 × 86 is the next suspect.
3. **mental ray environment shaders: Env_Sunrise, Env_Sunset, Env_Moonscape (unit sky1, sky2).** The Softimage
   renderer does not run mental ray shaders (the original's are irix/nt binaries); its picture shows an empty sky and
   dark water. The add-on now draws an approximation of the shaders' intent over the engine's picture when the
   Blender scene is rendered (F12): a physical sky with sun, moon, stars and clouds (Env_Atmosphere.mat), and waves
   (Water.tex); see "mental ray shaders: approximations" in docs/design/softimage-import.md. So these three now
   differ from the reference by design (sky1's sweep, `bad` before and after: Env_Sunrise 0.020 to 0.935, Env_Sunset
   0.050 to 0.563, Env_Moonscape 0.039 to 0.637). With the sky switched off (`mrfx.switch_sky`) they are the
   engine's picture as before; the import test compares them that way. The volume shaders (March_Fractal, March_Smoke,
   lightning) and the fur pass of the other 11 shader scenes are not approximated.
4. **obstacle-cascade.** The rocks sit elsewhere and the water surface is faceted.
5. **A bump-mapped projected texture carried as UV.** The renderer bumps a UV texture along (n × t, t), t the
   direction in which v grows on the triangle, and a planar projection along fixed axes (XY: x and y; XZ: x and z;
   YZ: y and z). Made UV, a projected bump map can bump the other way: MATTER-displacement's bumps are lit from the
   other side (rmse 0.0126 to 0.0256 with the UV fix, still match; the u mirror had cancelled half of it); Missile's
   projected bump maps likewise (0.0335 to 0.0344). With sky2's fixes as well: MATTER-displacement 0.0256 (`bad`
   0.049), Missile 0.0136 (`bad` 0.0001), where sky2's alone gave 0.0126 and 0.0112.
   Carrying the projection itself instead of UV coordinates would fix it: the kept lane database with the texture
   written back as XZ (method 2) renders at rmse 0.0119, bad 0.0044 (UV: 0.0282, 0.0222, same render). Not done:
   blend2db does not write a texture's projection rotation, scaling and translation (the dump's `projection`; the
   identity here) and projects the vertices it writes, so the rule would hold only for an identity projection on a
   model whose written space is the original's.
6. **IK-PageTurn's far floor.** The floor is noIcon.pic tiled 22 × 22; seen at a grazing angle it shows vertical
   streaks in the lane that the reference does not have (top 40 rows: mean level 4.4 against 2.3 of 255). The
   picture is the same pixel for pixel, the texture coordinates are (54 of 54 corners), and the set-up's fade is
   carried except that the SI World node clamps the fade start to 0 (the scene has −2; measured: 0.13 levels
   there). Not found.

## Tools (re/tools/fidelity1)

- `sweep.py`: the two lanes, the metrics and the bands. `--scene` runs merge into the table; `--rescore` re-bands an
  existing table, `--remeasure` recomputes the metrics from the pictures already made; `--size 0` renders the
  blender lane at the scene's resolution.
- `convert.sh OUTDIR [convert_scenes.py options, e.g. --all]`: reconverts corpus scenes with this checkout's add-on
  and the slot-9 programs into OUTDIR (refuses the owner's re/out/blend-scenes).
- `render4k.py`: 4K stills of converted scenes through the engine (`--min-aa`, `--keep`).
- `run_suites.sh`: the engine's headless test suites (blender3, blender1, framemap, nodes1, install, bundle) with this
  checkout, output in re/out/fidelity2/regress.
- `inspect_links.py`: a .blend's light linking.
- `montage.py`: reference | blender sheets. `pic2png.py`: a Softimage .pic as a PNG. `probe_scene.py`.
- Units extra1 and extra2: `re/tools/extra1/keep.py` (render a .blend through the lane and keep the exported scene, the
  database and the picture), `diffpics.py` (reference | lane | difference sheets); `re/tools/extra2/uvcmp.py` (texture
  coordinates of two dumps compared corner by corner), `bandcmp.py` (two sweep tables: band changes and counts),
  `suites.sh` (blender3's 12 modes and nodes1's four with its own output folder).
- Unit final1: `re/tools/final1/same_ref.py` (two reference folders compared picture by picture, run with Blender's
  Python), `suites.sh` (blender3, nodes, framemap, install, viewport logic, legacy engine, blend2db tests and the
  bundle check, with its own output folder).
