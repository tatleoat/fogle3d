# Fogle3D: a render engine for Blender (extension id `fogle3d`)

**Notice.** Fogle3D is an unofficial, independent recompilation of a renderer. It is not affiliated with or endorsed by
the makers of Softimage 3D. It requires your own licensed copy of Softimage 3D 4.0 and ships none of that product's
assets or code. "Softimage 3D 4.0" appears here only as the required third-party program and as the name of the scene
file format Fogle3D imports.

Render engine id `FOGLE3D`: Render Properties > Render Engine > Fogle3D. It renders the open Blender scene: `blend2db`
turns it into a database and the port's `render-frames` program (`re/tools/si-render/pictures`) renders that. A
Softimage 3D scene (.dsc) is worked with through **File > Import > Softimage 3D scene (.dsc)** only (and by dropping a
.dsc on the window); there is no way to render a .dsc directly from the Render Properties. (Tools such as the fidelity
sweeps can still render an installed .dsc directly through `session.set_tool_scene` / env `SI3D_TOOL_SCENE`; no panel,
menu or operator reaches that.) The source folder is still `blender-addon/softimage3d` (internal name); the bundle
stages it as `fogle3d`. Design notes: `re/out/lead/blender/design.md`, parallel contract: `parallel-render-contract.md`.

Files saved with the engine id `SOFTIMAGE3D` (before the rename) open with the engine set to Fogle3D: a load handler
rewrites the old id.

## Build the programs

From the repository root (a shared machine: through `re/tools/lead/slot.sh build --`, address space capped):

```
# the renderer. The manifest's own [profile.release] is the shipping profile: fat LTO, one codegen unit,
# no target-cpu (the binary runs on any x86-64). 5-12 % less CPU than the workspace default profile, same pictures.
CARGO_TARGET_DIR=$PWD/target/addon \
  cargo build --release --manifest-path re/tools/si-render/pictures/Cargo.toml --bin render-frames
# the same profile for anything built from the workspace itself is `cargo build --profile dist` (root Cargo.toml).

# the Blender-scene translator (own workspace, builds in about 15 s)
CARGO_TARGET_DIR=$PWD/target/addon cargo build --release --offline --manifest-path re/tools/builder2/blend2db/Cargo.toml
```

Result: `target/addon/release/render-frames` and `target/addon/release/blend2db`.

### x87 backend (hardware by default on x86-64)

Every build of the engine below uses the processor's own x87 unit on x86-64 (byte-identical to the software model, which
is used on every other architecture); to force the software model build with `SI_X87=soft` (the reference; pictures are
byte-identical either way), and `SI_X87=hw` demands the hardware path (a build error off x86-64):

```
SI_X87=soft CARGO_TARGET_DIR=$PWD/target/addon-soft \
  cargo build --release --manifest-path re/tools/si-render/pictures/Cargo.toml --bin render-frames
```

Point the preference "render-frames (hardware x87)" at a differently built binary only if you want two builds side by side.

## Requires your own Softimage|3D 4.0

The add-on and the engine ship **no Softimage file** (no databases, resources, shaders, fonts or textures). You need your
own installed copy of Softimage|3D 4.0 (the directory that holds `Creative-Rsrc/`), and the add-on reads everything it
needs from there at run time. Set it in Preferences > Add-ons > Fogle3D > **Softimage 3D 4.0 install path** (file
browser). For tools and scripts the environment variable `SI_INSTALL` does the same and wins over the preference; the
scene's own "Install dir" field (default: env `SI3D_INSTALLED`) wins over both. If the path is not set, is not a
directory, or lacks a needed file, the preferences panel and the render's error text say what is missing (by file
name). The files read are listed in `docs/install-requirements.md`. The release zip comes from
`re/tools/pkg/check_bundle.sh`, which also fails if anything from an install ended up in it; the programs inside it
(`bin/render-frames`, `bin/blend2db`) are used when the program preferences are empty.

## Install and set up

Install the bundle zip `fogle3d-addon.zip` from `re/tools/pkg/check_bundle.sh` (Edit > Preferences > Get Extensions > Install from
Disk). The extension lands in its own folder `fogle3d`; an older `softimage3d_engine` entry is a different extension and
should be disabled. Preferences > Add-ons > Fogle3D:

| Preference | Meaning | Default |
|---|---|---|
| Softimage 3D 4.0 install path | directory of your own install (required; env `SI_INSTALL` overrides) | empty |
| render-frames | path of the renderer program (the scene's own "render-frames" field, if filled, wins; its default is env `SI3D_RENDER_FRAMES`) | empty |
| Hardware x87 build, render-frames (hardware x87) | use the separately set binary instead (the default build is already hardware on x86-64) | off |
| blend2db | path of `blend2db` (scene field default: env `SI3D_BLEND2DB`) | empty |
| Processes (animation) | renderer processes for an animation render; 0 = all cores but one, at most 16 | 0 |
| Band threads | threads inside one frame (`RENDER_THREADS`); 0 = all cores but one for a still image (fewer when memory is short), 1 for an animation | 0 |
| Memory per process | address-space cap per renderer process (`ulimit -v`), MB; 0 = automatic: 2000 plus about 600 per million pixels of the picture and 128 per band thread, at most three quarters of the machine's memory (a 4K still with 16 band threads gets about 9-12 GB) | 0 |

Env overrides when the preference is 0: `SI3D_PROCESSES`, `SI3D_BAND_THREADS`, `SI3D_MEM_MB`. Render Properties >
Fogle3D holds the per-scene settings (install dir, value mapping, anti-aliasing, switches, keep directory, and
Memory (MB), which overrides the preference for one scene). With the automatic cap, an animation runs on fewer
processes when their caps together would pass three quarters of the machine's memory. A render that runs out of its
cap stops with "Fogle3D: the renderer ran out of memory ..." naming the cap and the picture size.

## Speed

- **Animation render** (Render > Render Animation) of a scene that carries no state from frame to frame: the frames of
  Blender's range are cut into chunks and up to K renderer processes run at a time (`RENDER_RANGE=a:b`; every
  picture is byte-identical to the file a one-process run writes). Scenes with expressions, constraints, particles,
  dynamics (relation ids in `parallel.STATEFUL`) run in one process over the whole scene range, as before.
- **Still image** (F12): one process, `RENDER_THREADS` band threads (crates/si-render `bands.rs`, byte-identical to one
  thread); a stateless scene renders the single frame (`RENDER_RANGE=n:n`), a stateful one its whole range.
- Cancel (Esc): every renderer process runs in a session of its own and the whole process group is signalled (SIGTERM,
  then SIGKILL after 3 s). Progress ("N/M frames done (K processes)") comes from the pictures that arrive.

## Rendered viewport

Set the 3D view's shading to Rendered (header > Viewport Shading, or Z > Rendered) with the engine set to Fogle3D.
The view shows the Softimage renderer's picture; Blender stays usable while it renders (the programs run as child
processes watched from a timer, never on Blender's main thread). Code: `softimage3d/viewport.py`.

- **Camera.** In camera view (Numpad 0) the picture is the scene camera's at the render resolution, smaller only when
  the camera frame on screen is smaller; at full size it is the F12 picture, float for float. In a free view the
  engine makes a camera from the view (perspective or orthographic, lens and clipping from the view) at the region's
  size divided by Blender's viewport pixel size (the scene's Pixel Size, Automatic: 1 unless the interface is scaled up).
- **Steps.** First a small picture (a quarter or an eighth of the size, shown as blocks), then the full size; one
  more step between when the last picture took over 0.5 s.
- **Changes.** While you orbit, move objects or edit settings, a small picture is started at most every 0.25 s and
  shown when it is done; 0.08 s after the last change the steps run for the newest state. A step for an older state
  is stopped (only the processes this viewport started: SIGTERM, then SIGKILL after 1 s) and its folder removed.
- **Status.** The view's text info (overlays on) says "Fogle3D: rendering WxH", "WxH in N s", or what is wrong
  (no install path, a frame outside the scene's range, a renderer error).
- **Colour.** The picture is handed to Blender as scene-linear floats; Blender's view transform (Color Management)
  is applied by Blender, as for F12.

Limits: the viewport picture is a still (no motion blur; interlaced fields woven as F12 weaves them); the view uses
Blender's viewport depsgraph, so modifiers use their viewport levels (F12 uses the render levels); the render
region border of the view (Ctrl+B in the view) is not used.

Speed (960x540 free view, own Linux VM): first pixels 0.09 s (cube) / 0.14 s (5 spheres) / 0.43 s (textured
subdivided monkey); full picture 0.22 / 0.37 / 0.57 s. A 1.5 s orbit starts 6 renders.

## Z pass

Tick Properties > View Layer > Passes > Data > Z. The engine switches the set-up's `Z_CHANNEL` on (a patched hard-link
copy of the database when the scene has it off), the renderer writes `<name>.<frame>.Zpic` beside the picture (finding
0012 section 31: big-endian float32, no header, camera-space z, negative in front of the camera, -100000 where
nothing was hit) and the engine delivers Blender's Depth pass as `-z` (1e10 where nothing was hit). A render region
reads the file's region columns (full-width lines); pictures and Depth are unaffected by the extra output.

## Softimage nodes (materials, textures, world)

With the engine set to Fogle3D, the Shader Editor's Add menu has a Softimage entry with four nodes:
- Material: shading model, colours, specular decay, reflectivity, transparency, refraction index, static blur;
- Texture: a 2D texture, local to the material or global to the model, with its method, mask, channel, factors and UV
  scale, offset and repeat;
- 3D Texture: marble, wood or cloud;
- World: ambience, background colour, depth fade, layer fog.

A Softimage Material on the Material Output is exported as it is set. Without one, the Principled BSDF path renders
exactly as before (the same database byte for byte). A node the exporter cannot use turns red and shows why.

The Material and World tabs mirror the nodes. They also hold the operators:
- Convert Blender shader, and Softimage settings to nodes for the whole file: they make nodes holding what the engine
  derives now, so the render does not change;
- New Softimage material and Softimage world;
- Add Softimage texture.

On the Principled path the engine now also reads an image's Alpha linked into the Principled Alpha (a cut-out) and a
UV Mapping with whole X/Y scales (texture repeats). Roughness and normal-map links are not read. Details:
docs/design/softimage-nodes.md.

## Animation and physics

Blender evaluates everything that moves or deforms, and the database carries the result:
- armatures, shape keys, lattices, hooks, cloth, soft body, rigid body, geometry nodes, particles, fluid and hair;
- objects that only move get function curves, as before, and a scene with no deforming object builds the same database
  byte for byte;
- an object whose mesh deforms with the same topology becomes Softimage shape animation (one shape per frame,
  blend2db `shapes.rs`), which the program blends and motion-blurs itself;
- instances (particles, geometry-node instances, collection instances) become one model each;
- hair curves (Curves objects) become camera-facing ribbons.

An animation render with frame mode "Scene range" samples the range once. The deforming objects then go into a few
chunk databases, rendered by K processes in one run. When the topology or the set of particles changes (fluid,
metaballs, remesh, particles born and dying), every frame is rendered from a database of its own. F12 renders the frame
alone. With motion blur on, the deforming objects get the shutter's shapes, so the deformation is blurred too.

Not carried:
- volumes and particle-system hair, each reported as a "not carried" warning (convert particle hair to a Curves object);
- shader displacement and procedural textures, which are not read.

The lights of the range's databases are fitted on the range's first frame, so a later frame can differ from its F12
still by a mean of 0.4 to 3/255. Details: docs/design/blender-animation-and-physics.md.

## Tests (headless, own Blender process, never an open one)

```
export CARGO_TARGET_DIR=<dir with release/render-frames and release/blend2db>
re/tools/blender1/run_test.sh              # engine against the direct renderer run
re/tools/blenderqol1/run_qol.sh            # progress, cancel, region, alpha pass, settings
re/tools/builder1/run_blender_test.sh cube # and: full           (Blender scene -> database)
re/tools/builder2/run_blender_test.sh <shadow|calib|ambient|texture|anim|ortho>
re/tools/pkg/run_install_test.sh          # install path: unset / wrong / empty -> clear error; preference alone -> byte-identical pictures
re/tools/blender2/run_blender2.sh          # Z pass, parallel K=1/8 and threads 1/8 byte identity + timing, cancel of all groups
re/tools/nodes1/run_nodes_test.sh all      # Softimage nodes: ui, identity (databases unchanged), render, links
re/tools/anim1/run_anim_test.sh <cloth|softbody|rigid|skarm|gn|particles|fluid|hair|blur|timing>   # animation and physics vs Cycles
re/tools/anim1/regress.sh <builder2|blender3|blender1|pkg> <mode>   # the other units' tests, output under re/out/anim1/regress
python3 -I re/tools/viewport1/test_logic.py # Rendered viewport: steps, throttle, stop, camera maths (no Blender)
re/tools/viewport1/run_vp_test.sh <identity|camera|timing|scene>   # viewport floats == F12, cameras, timing, tool-only .dsc render
re/tools/viewport1/run_gui_test.sh         # a real Blender window on Xvfb with Mesa software GL (CPU only, no GPU)
re/tools/viewport1/run_regress.sh          # the suites above (blender3 12 modes, blender1, framemap, install, bundle)
```

## Import Softimage 3D scenes (File > Import, drop, .blend files)

File > Import > Softimage 3D scene (.dsc) reads a scene of your install and turns it into ordinary Blender data:
- objects in the scene's hierarchy;
- meshes, and the renderer's triangles for patches, faces and NURBS;
- Softimage materials and textures (nodes);
- lights, the camera and the world;
- keyframes for the whole range, and one shape key per frame for deforming meshes.

Dropping a .dsc on the 3D viewport or the outliner does the same. The scene is set up for the Fogle3D engine
(resolution, range, frame rate), so F12 renders it. Light brightness: the engine fits
light colours to Cycles, so imported scenes come out brighter until the exporter takes the imported lights' own
values (importer.softimage_light, proposed in re/out/import1/proposed/).

The module is `importer.py` (`import` is a Python keyword). The reading is done by the program `si-import-dump`:

```
cargo build --release --manifest-path re/tools/import1/Cargo.toml     # set it in the preference "si-import-dump" or env SI3D_IMPORT_DUMP
```

Textures: each .pic becomes a PNG in Blender's user data folder (`softimage3d/textures`). The image keeps the .pic's
path in `si3d_picture`; nothing is packed or copied into the add-on. What is not carried is listed in the text
"Softimage import report" and in docs/design/softimage-import.md.

Whole scenes as .blend files that open with the engine selected and the 3D views in Rendered shading, looking through
the camera (one Blender process per scene; failures are logged and skipped). Blender reads Rendered back as Solid, so
the add-on sets it again when such a file opens (scene flag `si3d_open_rendered`):

```
blender -b --factory-startup --python re/tools/import1/convert_scenes.py -- --scene WorkBook/MODEL-Gears --verify
blender -b --factory-startup --python re/tools/import1/convert_scenes.py -- --all --out re/out/blend-scenes
```

Tests: `re/tools/import1/run_import_test.sh [--render] [scenes]`, `re/tools/import1/run_convert.sh ... --verify`,
`re/tools/import1/run_corpus.sh`.
