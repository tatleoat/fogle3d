# Notices

## Softimage 3D

"Softimage" and "Softimage 3D" are trademarks of their respective owners. Fogle3D is an unofficial, independent
project; it is not affiliated with, endorsed by or connected to the makers or owners of Softimage 3D. The names are
used only to say which program's scene files Fogle3D reads and which installed program it needs.

Fogle3D contains no part of Softimage 3D: no program, library, database, resource file, shader, font, picture,
texture or documentation of it is in this repository or in the add-on's zip. To use Fogle3D you need your own
licensed copy of Softimage 3D 4.0. The programs read the files they need from your install, on your machine, at run
time ([docs/install-requirements.md](docs/install-requirements.md)); `re/tools/pkg/check_bundle.sh` fails the
add-on build if any file of an install, or part of one, is inside it.

The engine was written by studying how the Linux version of the program behaves. Its comments cite the function of
the original a piece of code reimplements, by name and address in that program; these are references for checking,
not copies of it.

"Blender" is a trademark of the Blender Foundation. Fogle3D is not affiliated with or endorsed by it.

## Licences of this repository

- Everything outside `blender-addon/`: MIT, see [LICENSE](LICENSE).
- The Blender add-on, `blender-addon/`: GPL-3.0-or-later, see
  [blender-addon/softimage3d/LICENSE](blender-addon/softimage3d/LICENSE). It runs inside Blender and uses Blender's
  Python API (`bpy`), which is why it is GPL. The renderer and the other programs are separate programs the add-on
  starts as child processes; the add-on's licence does not extend to them.

Source files carry an `SPDX-License-Identifier` line naming their licence.

## Third-party code

The engine's workspace (`Cargo.toml` at the root) and `render-frames` (`re/tools/si-render/pictures`) use no
third-party crates. `blend2db` (`re/tools/builder2/blend2db`) and `si-import-dump` (`re/tools/import1`) use these
crates from crates.io, which Cargo downloads at build time (they are not in this repository; the compiled programs in
the add-on's zip contain their code):

| Crate | Licence |
|---|---|
| `serde`, `serde_core`, `serde_derive`, `serde_json`, `itoa`, `proc-macro2`, `quote`, `syn` | MIT OR Apache-2.0 |
| `memchr` | Unlicense OR MIT |
| `unicode-ident` | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `zmij` | MIT |

The programs link the Rust standard library (MIT OR Apache-2.0), which is part of the Rust toolchain.
