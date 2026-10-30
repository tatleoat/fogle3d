# Softimage nodes in the Blender add-on (unit nodes1, 2026-10-29)

Goal: a Blender user sets Softimage materials, textures and the Softimage world with nodes, the way Stecyk3D does it for
Bryce. When no Softimage node is used, shading stays Blender's Principled BSDF, and the database for that scene stays
byte for byte what it was before this unit. That is checked: the exporter JSON, render options and every database file
match the base add-on and the old blend2db, and the speed3 gate gives the same delivery in all 22 cases.

Code: `blender-addon/softimage3d/nodes.py` (sockets, nodes, enums), `material_nodes.py` (tree walking, the records the
exporter sends, faults, builders, conversion), `node_menu.py` (Add > Softimage), `material_ops.py` (operators),
`material_panels.py` (Material and World tabs); blend2db `re/tools/builder2/blend2db/src/simat.rs` and build.rs.

## Node set (Shader Editor, Add > Softimage, shown only while the engine is Softimage 3D)

| Node | Output | What it holds (the record it fills) |
|---|---|---|
| Softimage Material | Material (shader), into Material Output > Surface | shading model (Constant 1, Lambert 3, Phong 4, Blinn 5; 2, 6 and 7 are left out: 2 is an undocumented quick diffuse, 6 and 7 fall through to garbage), ambient, diffuse and specular colours, specular decay (exponent), reflectivity, transparency, refraction index, static blur (on, width, decay); four texture inputs and one 3D texture input |
| Softimage Texture | Texture | one 2D texture record: image, scope (Local on the material, relation 401; Global on the model, relation 400), method (XY, XZ, YZ, UV, cylinder, sphere, reflection map ray-traced first, reflection map without rays), interpolate, mask (blend by alpha 1, by intensity 2, plain blend 3, multiply 4), blend, channel (alpha or intensity drives reflectivity, transparency and bump), factors (ambient, diffuse, specular, reflectivity, transparency, bump, reflection map), black-and-white key, U/V scale, offset, repeat, alternate (mirror) repeats, swap UV |
| Softimage 3D Texture | Texture | one 3D texture record: marble, wood or cloud; spacing, taper, strength, iterations, power, five-colour ramp with three inner stops, mask, blend, channel, factors, scale, rotation, translation |
| Softimage World | World (shader), into World Output > Surface | fit ambience (on: fitted as without the node), ambience (the set-up's AMBIENCE), background (a scene-linear colour), depth fade (FADE_*), layer fog (FOG_*) |

The sockets are our own (colour, value, texture) and coloured differently from Blender's, so a Softimage texture links
only into a Softimage Material. Material, texture and fade/fog colours are Softimage values, as stored in the file,
with no colour management (COLOR_GAMMA). The World's background is a linear colour, like Blender's world colour.
`poll` accepts shader trees only.

A node the exporter cannot use turns red and draws its fault on the node. The faults are:
- a texture without an image, or one whose image has no pixels;
- a texture linked into nothing, or into an input that does not take it;
- a link on a Softimage Material's value input (its value is read, not the link);
- a Softimage Material that is not on the Material Output's Surface;
- a material node in a world tree, or the World in a material tree.

The Material tab shows the same faults. Reroutes are followed and muted links and nodes are skipped.

The Softimage Material also holds two hidden values, `fit_colour` and `fit_key`. Convert sets them so that the lights are
still fitted to the Principled's linear base colour while the node shows the converted Diffuse. A Diffuse changed by
hand no longer equals `fit_key`, and then its own value is used.

## Export priority

1. If the Material Output (the active one, else the first) is fed by a Softimage Material, the exporter sends an `si`
   record with every field above. blend2db writes the material record from it as is, with no Principled formulas:
   - local textures: TEXTURES2D elements with relation 401 (MATERIALS);
   - global textures: relation 400 (MODELS), the old texture route, so objects are still split per global texture;
   - a 3D texture: a TEXTURES3D element with relation 501.

   A transparent texture (si transparency factor not 0) raises the ray depth as a transparent material does.
2. Otherwise the Principled path, unchanged: the same JSON, the same blend2db code, the same database.
3. Two kinds of links around the Principled's image texture, which the old exporter did not read, are now carried as
   overrides on that texture's record (see "Linked inputs"). A scene without them exports as before.

The world: a Softimage World on the World Output gives the background, fade and fog. The lights and the ambience are
always fitted with the background colour as the world light, as the Background node's colour is without the node. With
"Fit ambience" off, the set ambience replaces the fitted one (`render_options["ambience"]`, written as AMBIENCE). The
lights stay as fitted, so a larger ambience brightens the picture. Without a Softimage World, the Background colour
times its strength is fitted as before.

## Linked inputs on standard nodes

| Link | Softimage | Status |
|---|---|---|
| Image Texture Color > Base Color | 2D texture, diffuse and ambient | as before (every image node is read) |
| Image Texture Alpha > Principled Alpha | the picture's alpha is the opacity: mask Blend, blend 1, channel alpha, transparency factor -1 (opacity = alpha) | carried; the cut-out matches Cycles at rmse 0.104 against 0.354 without it, and the ray depth is raised |
| Mapping (Point) between UV coordinates (UV Map node or Texture Coordinate UV) and the image | U/V repeat | carried when the X and Y scales are whole and positive, location and rotation are 0, nothing is linked into the Mapping and the image extension is Repeat: rmse 0.114 against 0.256 without |
| Image Texture > Roughness, Specular, Normal Map, Bump | - | not carried (the linked socket's value is used, as before) |
| Other Mapping set-ups (moves, turns, fractional or negative scales, Texture or Vector type, Generated/Object coordinates) | - | not carried |
| Closest interpolation | - | not carried; the texture keeps pixel interpolation 1, so existing scenes stay identical |
| Anything else | - | not carried; Convert names a material's linked Principled inputs in a warning |

## Operators

- New Softimage material (`si3d.new_material`): puts a Softimage Material on the active material's output, or on a new
  material. The nodes that fed the output stay in the tree, unlinked.
- Convert Blender shader (`si3d.convert_shader`): the active material's Principled BSDF (or its own colour without one)
  and first image texture become Softimage nodes. Their values are what the exporter derives from them now, with the
  same formulas and the link overrides above, so the database does not change. The Principled stays in the tree,
  unlinked.
- Softimage settings to nodes (`si3d.settings_to_nodes`): Convert for every material without Softimage nodes, and a
  Softimage World (fit ambience on) for every world.
- Softimage world (`si3d.new_world`): a Softimage World on the scene's world, or on a new world, that renders as the
  world does now.
- Add Softimage texture (`si3d.add_texture`, 2D or 3D): adds a texture node on the next free texture input.

## Material and World tabs

With our engine, the Material tab (SI3D_PT_material) shows:
- with a Softimage Material: the node's faults, its shading and values, each texture in a sub-panel (image, scope,
  method, mask, blend and details), the 3D texture, and the Add texture buttons;
- without one: what the Principled path reads (base colour, roughness, metallic, alpha, IOR, transmission, emission), the
  texture it uses, and the Convert, New and Settings-to-nodes buttons.

The World tab (SI3D_PT_world) draws the Softimage World, or the Background colour and strength and the Softimage world
button. Blender's material slot and world selector panels are enabled for our engine (COMPAT_ENGINES).

## Not carried (first version)

- Animation of Softimage-node values: a Softimage material is sampled per frame like any other model, but its colour is
  not keyed.
- Glow, UDF shaders, crop, the projection rotation and translation of 2D textures, more than four local textures per
  material, node groups.
- The links listed as not carried above.

## Tests

`re/tools/nodes1/run_nodes_test.sh [ui|identity|render|links|all]` (blender-addon/tests/test_nodes.py, own headless
Blender) gives 69 checks:
- ui (42): registration, poll, sockets, faults, reroute and mute, node, panel and menu drawing through a checking fake
  layout, the operators, converted values, unregister and register again.
- identity (12): the exporter JSON and render options match the base export.py. Old and new blend2db write identical
  databases from the same JSON. A Principled scene and its conversion to nodes give identical databases (19 of 19 and
  21 of 21 files).
- render (7): the conversion renders with 0 differing pixels; a local texture, a 3D texture, fog, fade, a set ambience
  and the background each change the picture, with the expected relation and set-up words.
- links (8): Mapping repeats and Alpha cut-outs are read and match Cycles more closely, and each converts to an
  identical database.
