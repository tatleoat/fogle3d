# What our programs read from the user's Softimage|3D 4.0 install

Release model (owner, 2026-10-29): the engine and the Blender add-on depend on the user's own local Softimage|3D 4.0
install, as a recompilation needs the original ROM. Nothing of Softimage ships with our build. This page lists what
is read from the install, as paths relative to the **install root**, and how the root is found. It is our own
description; it holds no content of the files.

The install root is the directory the product was installed to: the one that holds `Creative-Rsrc/`. In the
development copy (`re/bin/si3d40-ia/installed`, three platform trees side by side: `irix/soft3d`, `linux/soft3d`,
`nt/soft3d/data1`) any of them, or the parent that holds all three, works: the files below are found by name when
they are not at the usual place. The three copies of the files in sections 1 and 2 are byte-identical across
platforms (checked for the relations file and the default picture).

## 1. Needed by every render (`render-frames`, and the add-on's Blender-scene render and the tools' direct .dsc render)

| Path relative to the root | What it is for |
|---|---|
| `Creative-Rsrc/3D/rsrc/Relations.rsrc` | the relation dictionary (the table of relation kinds, their parameters and evaluation rules) every scene load and every evaluation is built from. The renderer's start-up reads it once per scene. |
| `Creative-Rsrc/3D/rsrc/noIcon.pic` | the program's stand-in picture, used when a texture or icon picture is missing; read beside the relations file. Its directory is also the program's resource directory (`SI_resdir`). |
| `Creative-Tools/3D/bin/soft` (the program file of the Linux build, 8,472,833 bytes; in the dev copy `linux/soft3d/Creative-Tools/3D/bin/soft`) | the original program's own data tables and message texts (the renderer's word tables, sampling patterns, set-up texts, error messages): our programs read them from it at run time instead of carrying copies (crates/si-rt, docs/design/runtime-data.md). It is opened once, read as data and checked by length and our own checksum; nothing in it is run. Found at the usual places (`Creative-Tools/3D/bin/soft`, `linux/soft3d/...`, `soft3d/...` under the root), else by name and size anywhere under the root. A missing file or another build (the IRIX or NT program, another version) stops `render-frames` and `si-import-dump` at start with exit status 2 and a message that names `SI_INSTALL` and the preference; the add-on's install check lists it the same way. Also needed by blend2db's render (section 2) and the importer (`si-import-dump`). |
| `Creative-Rsrc/3D/rsrc/Softimage.fnt` | the picture-insert (incrustation) font: the start-up reads `<SI_resdir>/Softimage.fnt` (the global `SI_softFont` names `/Softimage.fnt`). Optional: no batch render turns the insert on (only the options dialog writes `SI_incrust`), and a missing file only leaves the font words zero. |

## 2. Needed only by `blend2db` (the add-on's "This Blender scene" source)

| Path | What it is for |
|---|---|
| any `.../SETUP_SOFT/MODEL-Gears.1-0.sts` (in the dev copy `nt/db/data1/WorkBook/databases/WorkBook/SETUP_SOFT/`) | a stock render set-up file in the sample database "WorkBook" (Databases CD); blend2db starts every database it writes from it and patches the render options into the copy. It sits in the sample databases, so those must be installed. |
| the files of section 1 | same use as above (the new database is rendered with them; render-frames finds the program file through `SI_INSTALL`, which the add-on sets to the install root, since its argument is then the new database). |

## 3. The scene databases (user data, found in the install or elsewhere)

The tools' direct .dsc render (no UI) renders a `.dsc` from a database under the root (`.../SCENES/<name>.dsc`), found by a
recursive search of the root. A database is a directory of chapters; per scene the renderer reads the scene
description and the chapter files it lists:

`SCENES/*.dsc`, `SETUP_SOFT/*.sts`, `MODELS/*.hrc`, `MATERIALS/*.mtr`, `LIGHTS/*.lig`, `CAMERAS/*.cam`, `ANIMATION/*.ani`,
`TEXTURES2D/*.txt`, `TEXTURES3D/*.t3D`, `SHAPES/*.shp`, `CLUSTERS/*.cls`, `EXPRESSIONS/*.exp`, `LATTICES/*.lat`,
`SPREADSHEET_SETUP/*.spd`, `WAVES/*.wav`, `SETUP_FLOCK/*.flk`, `SETUP_CHANNELS/*.chn`, shader files `*.shd`, each chapter's
`Chapter.rsrc`, and texture pictures `PICTURES/<name>.pic` of the same database (looked up by the picture's name in the
database's `PICTURES` chapter, then in the working directory; a missing picture falls back to `noIcon.pic`).

Measured: a strace of `render-frames` rendering the first frame of all 343 sample scenes in the dev copy (RENDER_FIRST_ONLY)
opened files only of these kinds, plus the two of section 1; one `execve` (the program itself). Nothing under
`Creative-Fonts/`, no `.so`/DSO, no icon or bitmap library, no preferences or set-up files of the user's home.
The user's own databases outside the install work the same way (the add-on's scene field takes any directory that
holds `SCENES/`; the root must still hold the files of section 1).

## 4. Not read (so not required), and why

- Shader/DSO libraries (`Custom-Shader`, `mr21`, ... `*.so`, `*.dll`): the renderer has its own ports of the shaders it
  needs; nothing is loaded or executed from the install. Never exec files from the install.
- Fonts (`Creative-Fonts`), icons, dialog resources, default scenes: only the modeller/UI would use them.
- The licence: the engine has no licence manager; the licence file and its path are never read.

## 5. How the root is found today and after pkg1

| Where | Before | Now |
|---|---|---|
| `render-frames <dir>` (Rust) | the command-line directory, searched recursively | the argument; else env `SI_INSTALL`; else (development only) `re/bin/si3d40-ia/installed` found upward from the working directory or the program. Validated first (section 1); a missing or wrong directory prints the list of what is missing and exits 2. Code: `re/tools/pkg/si-install` (`locate`), used by `re/tools/si-render/pictures` (`install_args`, wired by `build.rs`). |
| the program file (section 1), in `render-frames` and `si-import-dump` (Rust) | (the original's tables and messages were compiled in) | `crates/si-rt`: the root the install argument resolved to; else env `SI_INSTALL`; else (development only) the dev copy found upward from the working directory or the program. Read and checked once at start (`si_rt::check_or_exit`); missing or another build: the reason on standard error and exit status 2. When the argument is not the install (a database blend2db built, a scene overlay) the add-on sets `SI_INSTALL` to the install root in the child's environment. |
| `blend2db build <scene.json> <out> [<root>]` (Rust) | five explicit paths | the old five-argument form still works; the short form takes root (argument, `SI_INSTALL`, dev copy) and derives template, template scene and resource directory (section 2). |
| Blender add-on (Python) | scene field "Install dir" (default env `SI3D_INSTALLED`), found `Relations.rsrc` with a preference for `/linux/` | `blender-addon/softimage3d/install.py`: scene field if set; else env `SI_INSTALL`; else the add-on preference **"Softimage 3D 4.0 install path"**; validated against the same list; the render stops with the missing-file text, and the preferences panel shows it. |
| tests and checks under `re/tools/*` | `re/bin/si3d40-ia/installed` hard-coded in shell scripts | unchanged (they pass the directory explicitly, which still wins). |
| oracle (`re/tools/oracle`) | mounts of the install | unchanged: the oracle is a dev tool and is not part of the release. |

## 6. What the release contains, and the check

The release is `fogle3d-addon.zip`: the add-on's Python files plus our three compiled programs in `bin/`
(`render-frames`, `blend2db`, `si-import-dump`). `re/tools/pkg/check_bundle.py` (`check_bundle.sh`) builds and stages it
under `re/out/pkg/` and fails if any file or zip member has a Softimage asset name or extension, has the sha256 of a file
of the install, contains a 64-byte window of the key assets, or holds the original program's data or messages (the
release audit's bundle category: a 64-byte window of the Linux `soft`'s data sections, or 16 or more of its messages in
one file). Run it before every release build.
Known limit: the check compares against the install it is given (`SI_INSTALL`, else the dev copy); it cannot know files
of installs it has not seen.
