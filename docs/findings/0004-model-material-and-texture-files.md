# 0004 — Model, material and texture files: the prototype-and-token format

Date: 2026-10-13. Binary: Linux `soft`, sha256 `ef21a382c3325d42…`. Source files: `binary.c`
(readers), `binary_b.c` (writers), `b_matio.c` (materials, textures, shader references),
`utilio.c` (prototype table, skipping). Rust: `crates/si-hrc`; check binary `hrc-check`.
The readers also exist on the shared memory image (`crates/si-hrc/src/image/`, check
binaries `hrc-emucheck` and `hrc-imagecheck`): see "The readers on the memory image" and
its second pass at the end (every object kind of a model is on the image since the
second), which also correct statements of this note. The writers are on the image since
the third pass, measured against files the original wrote (`re/tools/si-hrc/savedcheck`).

How to read this note: everything is **read from code** unless marked otherwise. "Page" means
the vendor's SDK pages `fileFormats/hrc*.htm`, which describe version 3.5 and are used here for
the *names* of fields; "corpus" means the 7,122 sample files on the discs (1,905 `.hrc`, 4,552
`.mtr`, 338 binary `.txt`, 327 `.t3D`; by content 1,690, 2,064, 281 and 125 distinct files).
Field names that are neither in the page nor obvious from the code are given as offsets
(`x404`).

## The four files

All four have the same layout:

| Part | Content |
|---|---|
| header | the 88-byte header of finding 0002 |
| tag | 4 bytes |
| prototype block | only if the tag was `PROT`; then another 4-byte tag follows |
| record | one record, if the tag is the one the reader expects |
| icon | optionally, the appended icon of finding 0003 |

| File | Tag | Record | Magic written | Reader, writer |
|---|---|---|---|---|
| `.hrc` model | `HRCH` | the root `<Hierarchy>` node | general 0x5380f634 | `SI_fdmodelBinaryRead` 0x083ec740, `SI_fdmodelBinaryWrite` 0x083ec3f0 |
| `.mtr` material | `MTRL` | one `<Material>` | 0x5380f640 | `SI_materialReadBinaryFile` 0x083e8b14, `SI_materialWriteBinaryFile` 0x083e8a04 |
| `.txt` 2D texture | `TXMP` | one `<Texture>` | 0x5380f643 | `SI_textureReadBinaryFile` 0x083e8d30, `SI_textureWriteBinaryFile` 0x083e8c28 |
| `.t3D` 3D texture | `TX3D` | one `<3D texture>` | 0x5380f644 | `SI_texture3DReadBinaryFile` 0x083e8f50, `SI_texture3DWriteBinaryFile` 0x083e8e48 |

- No reader compares the magic with the kind of file; any magic the header reader accepts will
  do. That is why the corpus's 366 materials (versions 2.0 to 2.49) and 246 3D textures (2.02
  to 2.6) under the general magic read like the others. The dedicated magics appear from 2.6.
- The model reader stops if the header is refused. The other three ignore the header reader's
  result: on a refused header they carry on at the start of the file with a version of −1,
  find no tag and read nothing.
- A material *file* never holds the material's textures: the writer puts two zero counts where
  a material inside a model has its texture lists. In the corpus no material file has any.
- The writers always emit a prototype block. The readers do not need one (see "Inline tokens").
- A model is written from its root with all its descendants; the root's own brothers are not
  written (the brother flag of the root is always 0).

## The prototype block

A *prototype* is the list of tokens a file stores for one kind of record. The block is

```
repeat:  short id (0 ends the block)
         short count
         short token[count]     the last one is 0; the count includes it
```

`si_prototypeBinaryRead` (0x083fdc40) keeps one list per id in a table of 28 pointers: ids 3
and 5 to 31. A list for any other id is read and discarded; a second list for an id replaces
the first. The prototypes may come in any order, and any may be missing.

| Id | Prototype | Id | Prototype | Id | Prototype |
|---|---|---|---|---|---|
| 3 | Spline | 13 | Material | 22 | Envelop vertex |
| 5 | Hierarchy | 14 | Texture | 23 | Cluster |
| 6 | Patch | 15 | 3D texture | 24 | Metaball |
| 7 | Face | 16, 17 | stored, never used | 25 | Metasystem |
| 8 | Mesh | 18 | Spline key | 26 | Metagroup |
| 9 | Edge | 19 | Dyna_matter | 27 | Metaunblend |
| 10 | Polygon | 20 | Joint | 28 | Udf |
| 11 | Poly_node | 21 | Envelop | 29, 30 | Nurbs curve, surface |
| 12 | Vertex | | | 31 | Nurbs composite curve |

Ids 16 and 17 occur only in files of versions 2.0 to 2.02 (lists `1 2 3 4 5` and
`1 2 3 4 5 6`); no reader in this build looks at them.

A record is then simply the data of its prototype's tokens, in the list's order, with nothing
between them. A reader walks the list and switches on each token.

### The three kinds of token

A token is 16 bits. Its high byte says whether a reader that does not know it can skip it
(`si_fileSkipField`, 0x083ff174):

| High byte | Kind | An unknown one is skipped by |
|---|---|---|
| 0 | plain | nothing: it cannot be skipped. The readers ignore that and go on with the next token, so the record is misread from there |
| 1 to 254 | short length | that many bytes |
| 255 | long length | reading a 4-byte length (which follows the token's position in the data) and then that many bytes |

Three things the page does not say:

- **The readers switch on the whole 16-bit value.** The length byte is part of a token's
  identity: 0x021b and 0x001b are two cases (which happen to read the same field), and
  hierarchy 0xff21 (edge user-data cluster) and 0x0221 (a short) are unrelated tokens that
  both occur in files of version 3.83. A known id with an unexpected length byte is unknown.
- **A reader that knows a token ignores its length.** For a short-length token it just reads
  its fields; for a long-length token it reads the 4-byte length itself and, with six
  exceptions, does not look at it. The exceptions are the hierarchy's user-data tokens 0xff1d
  to 0xff22, where a length of 0 or less means "absent" and nothing more is read.
- **Two readers skip wrongly.** The patch reader and the composite-curve reader skip *every*
  unknown token as a long one. A future short-length token in a patch would make this build
  read a length that is not there.

### Inline tokens

If the table has no list for a prototype, its reader takes each token from the file: a short
before every field, 0 ending the record. This is the layout of files older than the prototype
block; none is in the corpus (every file has a block that lists what it uses), and the writers
never produce it. The shader-reference reader is the exception: without a list it assumes
tokens 1 to 5 in order and reads none of them from the file.

### The table this build writes

`SI_prototypeBinaryWrite` (0x083fe0ac) builds the table from the output version (global
`SI_versionOutput`, 4.0), one comparison per group of tokens. With every comparison passing
(tokens in hexadecimal from 0x100):

| Prototype | Tokens written at 4.0 |
|---|---|
| Hierarchy | 1 2 3 12 4 5 6 7 8 9 14 16 17 18 19 20 21 22 23 24 25 41a 21b ff1c ff1d ff1e ff1f ff20 ff21 ff22 ff24 221 222 223 224 10 11 |
| Spline, Spline key | 8 1 7 3 4 5 6; 1 2 3 4 5 6 |
| Patch | 1 to 25 |
| Face | 3 1 2 4 |
| Mesh | 6 2 413 7 8 9 20a 20b 20c 40d 40e 40f 210 211 212 |
| Vertex, Polygon, Poly_node, Edge | 1 4; 2 3 4; 3 2 4 405 ff06; 4 3 |
| Material | 1 2 14 15 3 4 5 6 7 13 11 16 17 18 19 20 21 22 23 218 ff19 ff1a 21b 21c 21d |
| Texture | 1 to 16, 18, 20 to 37, 50, 233 234 235 236 237 ff35 238 |
| 3D texture | 1 2 3 4 5 22 23 24 25 26 10 27 28 11 12 13 14 15 16 17 18 19 21 29 31 32 33 34 35 ff24 |
| Dyna_matter, Joint | 1 to 7; 1 |
| Envelop, Envelop vertex | 1 5 4 7 8 9 11 12 13 14 15 16 17 18 20 21; 1 3 4 6 |
| Cluster, Metaball, Metasystem, Metagroup, Metaunblend | 1 2 3 4; 4 5 6 7; 1 2 3 4 5; 1 2; 1 |
| Udf | 1 2 3 4 |
| Nurbs curve, surface, composite | 1 2 3 4 5 6 7 208; 1 to 25, ff1a; 1 2 |

The prototypes are written in the order 5, 3, 18, 6, 7, 8, 12, 10, 11, 9, 13, 14, 15, 19 to 31.
The thresholds, which date each group of tokens: above 2.62 (texture 32 to 35); above 2.65
(hierarchy 23, 24; patch 17 to 25; polygon 4; material 21 to 23; texture 36, 37, 50, 0x233,
0x234; 3D texture 34, 35; the Udf and the three NURBS prototypes); above 2.69 (curve 6, 7;
surface 8 to 25); above 2.69904 (hierarchy 25); above 2.99903 (face 3 and 4); above 3.0 (curve
0x208); from 3.089 (hierarchy 0x41a, 0x21b); above 3.0905 (surface 0xff1a); from 3.49089
(hierarchy 0xff1c); above 3.5 (texture 0x235 to 0x237); from 3.8109 (hierarchy 0x221, 0x222;
mesh 0x20a to 0x212; material 0xff1a); from 3.8209 (hierarchy 0x223); from 3.93 (hierarchy
0xff24, mesh 0x413, node 0xff06). Not tested at all, so always written by this build: hierarchy
0xff1d to 0xff22 and 0x224, material 0x218, 0xff19, 0x21b to 0x21d, texture 0xff35 and 0x238,
3D texture 0xff24.

The newest files in the corpus are three of version 3.83. No file has hierarchy 0x224, 0x223
or 0xff24, material 0x21b to 0x21d, mesh 0x413 or node 0xff06: **no file in the corpus was
written with this build's table.** `re/tools/si-hrc/protcensus.py` lists every distinct token
list in the corpus with the versions that use it (8 for the hierarchy, 11 for the texture).

## Basic types

Everything is big-endian with no padding. short = 2 bytes, int = 4, float = 4, double = 8;
a string is bytes up to and including a zero byte; "flag" is a short. A vector is 3 floats, a
colour 3 floats (RGB) or 4 (RGBA), a matrix 16 floats.

The readers read each field with `fread` straight into its structure and swap afterwards, only
if the read was complete (`SI_byteswap_fread`, 0x083f9ba0). At the end of a truncated file a
field therefore keeps its old value except for the leading bytes that did arrive, unswapped.
The string reader collects into a 256-byte stack buffer without a bound.

## The records

"Old" marks tokens the readers know and this build's writers never emit; the version is where
the corpus or the writer's thresholds place a token. Counts are followed by that many items.

### Hierarchy (5): one node — `modelBinaryReadNode` 0x083eca24, `SI_modelBinaryWriteNode` 0x083f3c20

| Token | Data | Notes |
|---|---|---|
| 1 | string | name |
| 2 | short | type: 0 null, 1 patch, 2 face, 4 mesh, 5 joint, 6 spline, 7 metasystem, 8 metaball, 9 NURBS curve, 10 NURBS surface |
| 3 | a record of the type's prototype | nothing for types −1, 0, 3, 5; any other unknown type is skipped as a long field |
| 12 | short | shape interpolation; a stored −1 is read as 0 |
| 4, 5, 6 | vector each | scaling, rotation (radians), translation |
| 7 | short count; per item: short external; then a string (external) or a `<Material>` | an external material's name is read and dropped: the node keeps a null entry |
| 8 | the same with `<Texture>` | external ones are dropped |
| 9 | the same with `<3D texture>` | only an inline one in first position is kept |
| 13 (old, before 2.35) | short kind; 1: the IK chunk of token 20; 2 or 12: short flag and, if set, a vector (joint friction); anything else: a long skip | not in the corpus |
| 14 (2.35) | short flag; if set: **one more short (unused)** and a `<Dyna_matter>` | |
| 15 (old) | short envelope type, short flag; if set the fixed chunk: int, vector, vector, int, float × 5, int count, per vertex int, vector, float, float | not in the corpus |
| 16 | short | animation algorithm; the writer stores 2 as 1 and 12 as 0 |
| 17 | short envelope type, short flag; if set an `<Envelop>` | in a file below 3.49089 the envelope of a null with algorithm 1 or 2 (a chain root) is read and then dropped |
| 18 | short flag | the node has chain dynamics |
| 19 | short flag; if set a `<Joint>` | |
| 20 | short flag; if set: int type, int joints, vector | with a positive joint count the type is replaced unless it is 40: 10, 20 or 30 for 1, 2 or more joints |
| 21 (2.6) | short count; `<Cluster>` records | |
| 22 (2.6) | short count; per node: 4 bytes tag, int size, data | see "User data" |
| 23, 24 (above 2.65) | short each | visibility flags (default 7), motion blur |
| 25 (above 2.69904) | short flag; if set 3 vectors | initial scaling, rotation, translation |
| 0x41a (from 3.089) | float | matte, default 1; 0x1a is read the same way |
| 0x21b | short | garbage matte; 0x1b likewise |
| 0xff1c (from 3.49089) | int length; a user-data list | appended to the same list as token 22 |
| 0xff1d, 0xff1f, 0xff21 | int length; if positive: 1 byte, int count, ints, int state | the cluster of the user data on vertices, polygons, edges. **Not a `<Cluster>` record**: the byte is an empty name. The vertex cluster of a face holds 2 ints per entry |
| 0xff1e, 0xff20, 0xff22 | int length; if positive: int count; per component a user-data list | the count equals the cluster's |
| 0xff24 (from 3.93) | int length; for each UV set after the first: int count, ints | ids of the texture domains using the set; the number of sets is mesh token 0x413 |
| 0x221, 0x222 (3.8109), 0x223 (3.8209) | short each | meaning not determined |
| 0x224 | short | 1 sets bit 3 of the node's flag byte |
| 10, 11 | short flag; if set a `<Hierarchy>` | son, brother. Depth-first: a node, its sons, then its brothers |

After a node the reader resets to 0 every polygon material index that is negative or not below
the node's material count. For every inline texture of the node and of its materials it sets
the wrap flags from the shape (`SI_txt2dSetWrap`, 0x082ad9f8): on a face, metasystem or
metaball the UV method 4 becomes 6; a wrapping texture's scale is clamped to −1…1 and kept at
least 0.0001 away from 0.

### Mesh (8), Vertex (12), Polygon (10), Poly_node (11), Edge (9) — `meshBinaryRead` 0x083ef514

| Prototype | Token | Data |
|---|---|---|
| Mesh | 6 | short flag (default 0x804) |
| | 2 | float discontinuity angle in degrees |
| | 7, 8, 9 | int count and the vertices, polygons, edges |
| | 3, 4, 5 (old, to 2.35) | the same lists with a short count |
| | 1 (old) | short: non-zero sets bit 1 of the flag (faceted) |
| | 0x413 (3.93) | int: number of UV sets; kept in the node, default 1 |
| | 0x20a, 0x20b, 0x20c, 0x210, 0x211, 0x212 (3.8109) | short each (defaults 0, 2, 0, 1, 5, 0) |
| | 0x40d, 0x40e, 0x40f | float each (defaults 5, 0.1, 45: the same three as a patch's approximation) |
| Vertex | 1; 4 | vector; short flag |
| Polygon | 2 | short count (unsigned) and the nodes |
| | 3; 4 (above 2.65) | short material index; short flag |
| | 1 (old) | vector: the polygon's normal |
| Poly_node | 3 | int vertex index |
| | 1 (old, to 2.35) | short vertex index |
| | 2 | vector normal |
| | 4 (2.99908) | 2 floats: texture coordinates |
| | 0x405 (3.5002) | 4 bytes colour; below 3.71 the reader forces the first to 0xff |
| | 0xff06 (3.93) | int length; (UV sets − 1) pairs of floats |
| Edge | 4 | 2 ints: its vertices |
| | 1 (old, to 2.35) | 2 shorts |
| | 3 | short flag |
| | 2, 5 | known and empty |

The file's edge list is not what the program keeps: it rebuilds the edges from the polygons and
copies each listed edge's flag to the rebuilt edge with the same vertices. If no node normal was
stored it computes normals. Neither step is ported (see Open).

### Patch (6), Spline (3), Spline key (18), Face (7)

| Prototype | Token | Data |
|---|---|---|
| Patch | 1, 2 | short: type in U, in V (0 linear, 1 Bézier, 2 B-spline, 3 cardinal, 4 NURBS) |
| | 3 | short nu, short nv, nu × nv vectors |
| | 4, 5 | short: closed in U, V; any non-zero value is kept as 1 |
| | 6, 7; 8, 9; 10, 11 | float tension; short step (default 3); short curves (default 1), U then V |
| | 12 to 15; 16 | short capping flags; short faceted |
| | 17 to 24 (above 2.65) | approximation: short method, short view, floats edge, U and V curvature, shorts minimum, maximum, recursion |
| | 25 | nu × nv shorts: tagged points (the counts of token 3) |
| Spline | 8 (2.045) | string name |
| | 1 | short type |
| | 7 | short count (unsigned) and the keys |
| | 2 (old, to 2.02) | short count and one vector per key: positions only |
| | 3, 4, 5, 6 | short closed, float tension, short step (default 10), short phantom |
| Spline key | 1, 2, 3; 4, 5, 6 | vectors: previous tangent, position, next tangent; shorts: constraint, segment, flag |
| Face | 3 (above 2.99903) | short: 1 (default) made of splines, 0 made of NURBS curves |
| | 1 | a `<Spline>`: the outline. Read only for a spline face |
| | 2 | short count and that many `<Spline>` holes. Spline face only |
| | 4 | short count and that many `<Nurbs curve>`. NURBS face only |

A face token that does not apply reads and writes nothing at all, not even a count.

### NURBS (29, 30, 31)

| Prototype | Token | Data |
|---|---|---|
| Curve | 1 | int order |
| | 2 | int count; per point 4 doubles (x, y, z, w) and, from version 2.6, an int flag |
| | 3 | short closed |
| | 4 | int uniformity, **int** count, doubles: the knots |
| | 5 | 2 doubles: start, end |
| | 6, 7 (above 2.69) | int display step (default 10), int closing (default 1) |
| | 0x208 (above 3.0); 8 in 3.0905 | short: key-framed |
| Surface | 1 | 2 ints: order in U, V |
| | 2 | int nu, int nv, nu × nv points as above |
| | 3 | 2 shorts: closed in U, V |
| | 4 | int uniformity; int count and knots in U; int count and knots in V |
| | 5 | 4 doubles: start and end in U, in V |
| | 6 | short border flag |
| | 7 | int count and that many `<Nurbs composite curve>`: the trims |
| | 8, 9; 10, 11 | int display steps (default 3); int wireframe curves (default 1) |
| | 12 to 15 | short capping flags |
| | 16, 17 | int closing in U, V |
| | 18 to 25 | approximation, as patch 17 to 24 |
| | 0xff1a (above 3.0905) | int length, int count, composite curves: curves projected on the surface. 0x1a is the same without the length |
| Composite | 1; 2 | short boundary flag; int count and that many `<Nurbs curve>` |

The length the writer puts in 0xff1a comes from a counting pass that assumes this build's
curve layout: 48 bytes plus 36 per point and 8 per knot for a curve, 6 more per composite.

### Metaballs (24 to 27)

| Prototype | Token | Data |
|---|---|---|
| Metaball | 4, 5 | double: ratio (default 0.5), weight (2) |
| | 6; 7 | 3 doubles: exponent; matrix |
| | 1, 2, 3 (old) | 4 bytes each, read and dropped |
| Metasystem | 1, 2; 3 | float: wireframe, rendering resolution; short show flag |
| | 4 | short count and `<Metagroup>` records |
| | 5 | short count and `<Metaunblend>` records |
| Metagroup | 1; 2 | string name; short count and that many strings (names of metaball models) |
| Metaunblend | 1 | 2 strings: the names of two groups |

Groups, a group's names and unblend pairs are each linked in at the head of their list as they
are read, and written from the head: **each save reverses their order.** The corpus has no
metasystem or metaball at all, so these four readers are unchecked against files.

### Material (13) — `si_materialBinaryRead` 0x083e7f90, `si_materialBinaryWrite` 0x083e8604

| Token | Data | Notes |
|---|---|---|
| 1 | string | name; copied without a bound into a 30-byte field |
| 2 | short | shading model (default 4) |
| 14, 15 | short, float | glow flag and amount |
| 3, 4, 5 | RGB each | ambient, diffuse, specular |
| 6, 7, 13, 11 | float each | specular exponent, transparency, reflectivity, refractive index |
| 16 | short count; per item short external, then a string or a `<Texture>` | |
| 17 | the same with `<3D texture>` | only the first inline one is kept |
| 18, 19, 20 (2.6) | short, float, float | static blur flag, width, decay |
| 21, 22, 23 (above 2.65) | `<Udf>` each | material, volume and shadow shader |
| 0x218 (3.69) | short | shading model again |
| 0xff19 (3.69) | int length; a user-data list | |
| 0xff1a (3.8109) | int length (13 + the name's length); a `<Udf>` | a fourth shader |
| 0x21b, 0x21c, 0x21d | short each | shading model a third time; two shorts, default 1 |
| 8, 10 (old) | float each | not in the corpus |
| 9 (old) | RGB | its mean becomes the reflectivity; not in the corpus |

The shading model is stored three times so that each generation of the program finds a value it
understands. Reading: 2 sets it; 0x218 sets it, and a stored 7 becomes 1 with both extra shorts
set to 2; 0x21b sets it again. Writing: token 2 writes a model of 7 as 1; 0x218 writes 7 unless
both extra shorts are 1; 0x21b writes the model as it is.

### Texture (14) — `si_textureBinaryRead` 0x083e6b90, `si_textureBinaryWrite` 0x083e73f8

| Token | Data | Notes |
|---|---|---|
| 1 | string | name (default `NO_NAME`) |
| 2; 3, 4 | short | animation type; repetitions in U, V |
| 5, 6; 7, 8 | float | scaling in U, V; offset in U, V |
| 9; 10 | short | pixel interpolation; mapping method |
| 11 to 16 | float | ambient, diffuse, specular, roughness, transparency, reflection-map factors |
| 18; 20 | float | environment rotation; blending |
| 21 | short | cropping flag |
| **22, 23, 24, 25** | **2 shorts each** | see below |
| 26, 27, 28 | short | swap U and V; alternate in U, in V |
| 29 | float | reflectivity |
| 30, 31 | short | mask (default 3; written as 3 when it is 4), channel (default 2) |
| 32, 33, 34 (above 2.62) | vector | rotation, scaling, translation |
| 35 | short | black-and-white mapping |
| 36, 37 (above 2.65) | `<Udf>`; short | shader; displacement |
| 50 (3.009) | string | picture file name |
| 0x233, 0x234 (3.4909) | short | hardware flags |
| 0x235, 0x236, 0x237 (3.704; above 3.5 in this build) | short | not determined |
| 0xff35 (3.69) | int length; a user-data list | |
| 0x238 (3.83) | short | the mask again, unchanged |
| 17 (old, to 2.052) | float | not written since |
| 19 (old, 2.1) | short | the mask; also sets the blending to 1 |

**The cropping tokens are an off-by-one in the original that the format has kept.** The four
bounds are consecutive shorts in memory, `umin, vmin, umax, vmax` in the order of their
offsets, and tokens 22 to 25 are read and written with an element count of 2 instead of 1:

| Token | Reads and writes shorts at | i.e. |
|---|---|---|
| 22 | +0x30, +0x32 | bound 0, bound 1 |
| 23 | +0x34, +0x36 | bound 2, bound 3 |
| 24 | +0x32, +0x34 | bound 1, bound 2 |
| 25 | +0x36, +0x38 | bound 3, and the short after the bounds |

So the four tokens take 16 bytes, not 8, every bound but the first is stored twice, and on
reading the later token wins. Reader and writer agree, which is why the files work.

After the record, a texture whose mapping method is 7 or 8 has its blending and its ambient,
diffuse, specular, roughness, transparency and reflectivity factors set to 0.

### 3D texture (15) — `si_texture3DBinaryRead` 0x083e77c8, `si_texture3DBinaryWrite` 0x083e7cf4

| Token | Data | Notes |
|---|---|---|
| 1; 2, 3 | string; short | name; type; iterations |
| 4, 5 | float | strength, spacing |
| 22 to 26 | RGBA each | the five colours of the ramp |
| 10, 27, 28 | float | the three factors between them |
| 11, 12, 13 | vector | scaling, rotation, translation |
| 14 to 19 | float | ambient, diffuse, specular, roughness, transparency, reflection |
| 21, 29, 31 | float | power, taper, blending |
| 32, 33 (2.1) | short | mask (default 3), channel (default 1; 2 in a file below 2.1) |
| 34, 35 (above 2.65) | `<Udf>`; short | shader; displacement |
| 0xff24 (3.69) | int length; a user-data list | |
| 30 (old, 2.1) | short | mask: 1 stays 1, anything else is 3; sets the blending to 1 |
| 7, 8, 9 (old) | RGB | three colours: ramp entries 1, 3 and 5. If 7 was present, entries 2 and 4 become the means of their neighbours and the factors f/2, f, f + (1 − f)/2 from the old factor f. Not in the corpus |
| 6, 20 (old) | float | read and dropped. Not in the corpus |

### Shader reference (28) — `si_udfRead` 0x083e9154, `si_udfWrite` 0x083e9068

Current form: token 1 string name, 2 int active, 3 int internal, 4 int order. An empty name or
the name `NULL` gives no reference (the other fields are still read). **Token 5 ends the
record immediately**; it occurs in the six files of version 2.6991.

In a file of version up to 2.67906, or from 2.69 to below 2.999019 (`si_dbfileIsOldUdfVersion`,
0x083fb934), the record has another form and is read and thrown away (`readOldUDF`,
0x083e9374): tokens 1, 2, 3 strings, 4 a short, 5 an int. A missing reference is written as an
empty name and three zeros.

### Dynamics, joints, envelopes, clusters

| Prototype | Token | Data |
|---|---|---|
| Dyna_matter (19) | 1 to 6 | float: density, roughness, elasticity, rigidity, plasticity, kinetic roughness |
| | 7 (2.49) | int: collision detection level (default −2) |
| Joint (20) | 1 | vector: friction (default 60, 15, 15) |
| Envelop (21) | 1 | int level |
| | 5; 4 | float joint area; int inner curve type |
| | 7, 8, 9, 11 | float: inner curve begin, end, amplitude, scale |
| | 12; 13 to 16 | int outer curve type; float begin, end, amplitude, scale |
| | 17, 18 | short count and floats: the inner and outer curve. The writer always writes 200 |
| | 20; 21 (2.49) | short number of lists; per list int count and `<Envelop vertex>` records |
| | 19 (old, 2.35), 10 (old) | one list with a short count; with an int count |
| | 2 (old, to 3.009) | vector: stored as the *node's* initial rotation |
| | 22, 23, 24 (old, 2.6 to 3.009) | vector: initial scaling, rotation, translation of the node's *parent*, if that is a chain root; otherwise dropped |
| | 3, 6 (old) | vector, float: read and dropped |
| Envelop vertex (22) | 1; 3, 4; 6 (2.6) | int index; float ratio, thickness; float weight (default 100) |
| | 2 (old) | vector, dropped |
| | 5 (old, to 2.65) | matrix. Up to version 2.59 kept for rebuilding the envelope; later dropped |
| Cluster (23) | 1; 2; 3; 4 | string name; int count; that many ints (twice as many on a face); int state |

Reading an envelope gives the node an initial transformation, copied from its transformation,
if it has none yet.

### User data

Two forms share one list per node (and one per material, texture, 3D texture):

- **Old** (hierarchy token 22): a 4-byte tag, an int size, the data. In a file below 3.50009 a
  tag other than `CUSA`, `CUSB`, `CUSC`, `CUSD` (custom-effect data) is byte-reversed and
  prefixed with `_`, and the data is read as one item of `size` bytes, which the swapping read
  reverses when the size is 2, 4 or 8.
- **New** (`si_userDataRead` 0x083ec910): int count; per block int tag length, tag, short flag
  (the page's "big-endian flag"), int size, data. Below 3.82099 the data is again read as one
  item.

The writer sorts the list by tag, not by origin: blocks whose tag starts with `_` go into the
new form (token 0xff1c), all others into the old form with the first four bytes of their tag;
blocks tagged `CUSELEMCB` or `CUSNEXTELEMCB` are never written. One oddity: while writing
0xff1c it compares the tag of the list's *first* block with `__NodeInfo` for every block, and
writes none of them if that matches. The length of a user-data token is 4 plus, per block, the
tag's length, the data's size and 10 (`si_userDataSize` 0x083f3780).

## Where the vendor's pages are wrong or silent

1. Tokens are matched on all 16 bits, and a known token's length is not used (above).
2. Texture tokens 22 to 25 hold two shorts each, not one.
3. Hierarchy token 14 has an extra short between its flag and the `<Dyna_matter>`.
4. Hierarchy tokens 0xff1d and 0xff1f are not `<Cluster>` records, and a length of 0 means
   nothing follows.
5. The knot count of a NURBS curve is an int, not a short.
6. Poly_node token 4 is two floats, not two ints (from the values in the files).
7. Udf token 5 is not mentioned; nor is the older Udf form.
8. Everything after 3.5: material 0x218, 0xff19, 0xff1a, 0x21b to 0x21d; texture 0x235 to
   0x238 and 0xff35; 3D texture 0xff24; mesh 0x20a to 0x212 and 0x413; node 0xff06; hierarchy
   0xff21, 0xff22, 0xff24, 0x221 to 0x224.
9. The "not saved" lists are incomplete: the readers also know hierarchy 13 and 15, envelope 2,
   3, 6, 10, 19, 22 to 24, envelope vertex 2 and 5, material 8 to 10, texture 17 and 19, 3D
   texture 6 to 9, 20 and 30, metaball 1 to 3.

## Checks

`cargo run --release -p si-hrc --bin hrc-check -- [--write] [--list] <dir>` over the unpacked
install (2.3 s). The same binary prints one model's hierarchy (`--tree <file>`), exports its
polygon meshes (`--obj <file> <out.obj>`) and shows where a file's rewrite differs
(`--diff <file> [own]`).

| | `.hrc` | `.mtr` | `.txt` | `.t3D` |
|---|---|---|---|---|
| files | 1,906 | 4,552 | 353 | 327 |
| **read to their exact end** | **1,905** | **4,552** | **338** | **327** |
| … at the end of the file | 1,905 | 4,186 | 308 | 27 |
| … where the appended icon starts | 0 | 366 | 30 | 300 |
| not read | 1 empty file | 0 | 15 text files of other subsystems (no binary header) | 0 |
| tokens skipped as unknown | 0 | 0 | 0 | 0 |

The 1,905 models hold 27,577 nodes: 11,595 nulls, 5,619 meshes, 5,188 splines, 2,360 faces,
2,164 joints, 543 patches, 97 NURBS surfaces, 11 NURBS curves.

**Round trip through the writer** (version 4.0 table): write what was read, read it again,
compare the data, write again.

| | `.hrc` | `.mtr` | `.txt` | `.t3D` |
|---|---|---|---|---|
| written | 1,905 | 4,552 | 338 | 327 |
| read again to the exact end | 1,905 | 4,552 | 338 | 327 |
| same data | 1,833 | 4,552 | 338 | 327 |
| second write gives the same bytes | 1,905 | 4,552 | 338 | 327 |

The 72 models whose data differs all have meshes without stored normals (node prototype `1` in
the 33 models of version 2.1, `3` in 39 models of version 2.65): the reader marks such a mesh for normal computation, the writer
stores (zero) normals, and the second read no longer asks for the computation. The original
computes the normals before writing; that step is not ported, so for these 72 files the
written file is not what the program would write.

**Byte for byte.** With the program's own table: 0 of 7,122, necessarily, since no file in the
corpus carries it. To measure the field mapping all the same, `hrc-check` also writes each file
with the header and the table it came with, through the same per-token code:

| | `.hrc` | `.mtr` | `.txt` | `.t3D` |
|---|---|---|---|---|
| identical to the file (up to the icon) | 1,597 | 4,552 | 335 | 219 |
| uses records whose table has tokens this build never writes | 46 | 0 | 0 | 108 |
| differs | 262 | 0 | 3 | 0 |

- The 108 3D textures are of version 2.1 with token 30. The 46 models: 33 of version 2.1 (mesh
  3, 4, 5 and with them edge 1 and node 1), 4 with envelope tokens 2 and 22 to 24, 3 with Udf
  token 5, 6 with curve token 8.
- Every one of the 265 differing files differs where the reader changes a value, as the
  original does: 216 models below 3.71 store node colours whose first byte is not 0xff; 46
  models store a shape interpolation of −1; 3 textures (one file on three platforms) use
  mapping method 8 with a non-zero ambient factor. `hrc-check` records each such change while
  reading and reports no difference it cannot attribute to one.
- Files of 3.71 and later, where no such change applies, are identical: 527 of 527 models.

## Open

- **Not ported, and called by the readers**: rebuilding a mesh's edges and normals
  (`SI_meshUpdateNodes` 0x08458cf8 and the `SI_meshCompute…` routines), the conversion of an
  old Bézier point list (`SI_bezierShrink`), forced capping of NURBS surfaces, the rebuilding
  of envelopes from files up to 2.59 (`SI_envelopRebuild`), the resolution of metaball group
  names into models, and the byte-order conversion of custom-effect data (`SI_fix_…`; the data
  is kept in file order). A model read here is therefore the file's content, not yet the
  program's state after loading.
- **Not ported from the four source files**: the model reader's second mode (re-reading over an
  existing hierarchy) and its "lite" twin (`modelBinaryReadNodeLite`); the writer options for
  triangulated output and global coordinates; shape files (`SI_shapeBinaryRead`), cluster
  animation files (`SI_clusterAnimBinaryRead`), and the function-curve and actions routines of
  `utilio.c`.
- The version tests inside the record writers were read but only their outcome at 4.0 is
  reproduced; whether the program can be made to write an older version was not looked into.
- Meaning of hierarchy 0x221 to 0x224, mesh 0x20a to 0x212, texture 0x235 to 0x237 and the
  material's two shorts (0x21c, 0x21d).
- The default name of a model (`obj-N`) and of a spline (`spl-N`) comes from running counters;
  every file in the corpus stores its names, so the default is not reproduced.

## Corrected since (2026-10-13, from finding 0015)

The paragraph above on the 72 old models says the original computes their normals before
writing. For this build that is wrong. All 1,009 meshes of those models carry mesh flag 0, and
the vertex-normal routine (`SI_meshComputesVertexNormal`) returns at once unless flag bit 0x2,
0x8 or 0x800 is set. After loading, the program has built their vertex stars, edges, polygon
normals and border bits, and their corner normals are still zero. This is read from the code
and confirmed by emulating the original routine on meshes with flag 0. So zero corner normals
in a re-written file are what this build would hold in memory too. Whether display, rendering
or saving treats zero corner normals specially has not been looked into.
- The two strings of a 2D texture are labelled the wrong way round in this finding and in
  `crates/si-hrc` (`Texture2D.name`, `.filename`): token 1 (record offset +0x14) is the
  picture's path and token 50 (+0x18) is the texture's own name. Evidence: the save renames an
  element by writing +0x18, and only with that reading do 306 of 309 textures re-save
  unchanged (finding 0018). The crate's field names are to be swapped when the shared records
  are next consolidated.

## The readers on the memory image (2026-10-13, image pass)

Finding 0024 decided that the program's state lives in `si-mem`'s image at the original's
layout. This pass ports the readers a second time, from the listing, onto that image:
`crates/si-hrc/src/image/` beside the value form above, which stays as it is (eight crates
use it). A reader takes the image, the outside world and the original's arguments, makes
the original's allocation calls in its order, and leaves the original's records. Everything
in this section is **read from code and emulated** (the harness of finding 0011) unless it
says otherwise.

### What is on the image

58 functions, cited in the code; all were compared with the emulated original.

| File of `src/image/` | Original source | Functions |
|---|---|---|
| `io.rs` | `byteswap.c`, `filetoken.c`, `utilio.c`, `headers.c` | `SI_byteswap_fread`, `SI_float_fread`, `SI_Vector_fread`, `SI_RGB_fread`, `SI_RGBA_fread`, `SI_FileChunk_fread`, `SI_fix_SI_FileChunk`, `SI_swap2byte`, `SI_swap4byte`, `SI_swap8byte`, `validate_float`, `SI_stringBinaryRead`, `si_fileSkipField`, `SI_dbfileReadBinHeader`, `si_dbfileIsOldUdfVersion` |
| `proto.rs` | `utilio.c` | `si_prototypeAllocate`, `si_prototypeBinaryRead`, `si_prototypeDispose` |
| `udf.rs` | `udf_fct.c` | `SI_udfAllocate`, `SI_udfCopy`, `SI_udfInitializeUdf`, `SI_udfIsActive`, `SI_udfSetActive`, `SI_udfDispose` |
| `userdata.rs` | `binary.c`, `model.c` | `si_userDataRead`, `SI_userDataCopyFromSetup` |
| `texture.rs` | `t2d_fct.c`, `t3d_fct.c`, `algebra.c` | `SI_textureInitialize`, `SI_textureAllocate`, `textureTransfer`, `SI_textureCopy`, `SI_updTxt2dEffects`, `SI_texture3DInitialize`, `SI_texture3DAllocate`, `texture3DTransfer`, `SI_texture3DCopy`, `SI_matrixCopy` |
| `material.rs` | `mat_fct.c` | `SI_materialInitialize`, `SI_materialAllocate` |
| `matio.rs` | `b_matio.c` | `si_udfRead`, `readOldUDF`, `si_textureBinaryRead`, `si_texture3DBinaryRead`, `si_materialBinaryRead`, `SI_materialReadBinaryFile`, `SI_textureReadBinaryFile`, `SI_texture3DReadBinaryFile` |
| `geom.rs` | `mesh_b.c`, `obj_global.c` | `SI_meshAllocate`, `SI_vertexAllocate`, `SI_polygonAllocate`, `SI_polygonNodeAllocate`, `SI_edgeAllocate` |
| `mesh.rs` | `binary.c`, `mesh_b.c` | `meshBinaryRead`, `SI_setCurrentModel` |
| `node.rs` | `binary.c`, `model_b.c`, `model.c`, `klusters.c` | `SI_fdmodelBinaryRead`, `modelBinaryReadNode` (both modes), `SI_modelAllocate`, `SI_modelAddDefinition`, `SI_klusterAllocate` |

Five of them (the three swaps, `SI_fix_SI_FileChunk`, `validate_float`) have no vector
file of their own: they run inside every compared case of the reads that call them, where
each swapped byte and the scratch byte `tmp` are compared.

Records: `si-types`' `Material`, `LclMap`, `Texture3D`, `Model` and `Mesh` type the
pointers; their layouts place few fields, so the readers address fields by named offsets
(`material::mat`, `texture::lclmap`, `texture::t3d`, `node::node`, `geom::mesh`), each read
from the listing. `record!`s of this crate: `Prototypes` (0x70: 28 list pointers; slots 3,
18, then 5 to 17 and 19 to 31), `Udf` (0x3c), `UserNode` (0x14). Globals at their addresses
(`globals.rs`, 16 entries): the three versions (`SI_versionInput`, and one `fileVersion`
each in `b_matio.c` 0x08817f20 and `binary.c` 0x08817f28), `tmp`, `SI_pipedFiles`,
`SI_pipe`, `SI_identite`, `defaultMapMethod`, `si_udfList`, `si_numUdf`, `SI_procData`
with its storage, `SI_grafLists`, `currMdl`, the node counter of `SI_modelAllocate`,
`SI_edSumatraTo3x`.

Everything else goes through one trait, `image::Outside`: the C library's streams
(`fopen`, `fclose`, `fread`, `fgetc`, `rewind`, and `cos`), and 48 routines of other
source files by name and address, each written `name (0xADDRESS)` where it is declared
(`re/tools/si-hrc/check_outside.py` holds all 157 name and address pairs of the module
against the function table). `image::Files` is the streams in memory, behaving as the
harness's stand-ins do.

### What the listing and the emulator settled

1. **A version is compared as a float against a double.** `fileVersion < c` loads the
   float and compares it with the 8-byte constant. A file whose version is the float
   nearest to the constant is therefore below it when that float is the smaller one: the
   float 2.1 is 2.0999999, so a 3D texture in a file of version 2.1 gets channel 2. The
   value form narrows the constant to a float first and gets 1 (see "Corpus" below).
2. **A 3D texture's channel hangs on a global the model reader never sets.**
   `si_texture3DBinaryRead` looks at `b_matio.c`'s `fileVersion`. Only the three file
   readers of `b_matio.c` set it; `SI_fdmodelBinaryRead` sets `binary.c`'s. A 3D texture
   inside a model is judged by the version of the last material or texture *file* read
   (0 in a fresh program: channel 2). Likewise `si_userDataRead` always looks at
   `binary.c`'s copy, also inside a material or texture file.
3. **The fifth cropping short is lost.** Token 25 reads two shorts into the working copy
   (+0x36, +0x38); `textureTransfer` copies +0x30 to +0x36 only. The new texture has 0 at
   +0x38 whatever the file holds.
4. **Fields the texture readers copy from stack they never wrote.** `SI_textureInitialize`
   does not set +0x134 (the reflectivity factor), +0x1b8 or +0x1ca, and
   `SI_texture3DInitialize` does not set +0x114 (the reflection factor), yet the transfer
   routines copy them from the working copy. A 2D texture from a file without token 29
   has a reflectivity factor, and the "uses reflectivity" flag at +0x118 derived from it,
   made of what earlier callees left on the stack (emulated: the value is the same in both
   of the harness's runs when the reader is called from a file or material reader, and
   differs between them when it is called alone).
5. **A short read through the scratch word is stored whether or not the read succeeded**
   (2D texture tokens 2, 9, 10, 19, 21, 26 to 28, 30, 31; the counts of a node's 3D
   textures, clusters and old user data): at the end of a file the field gets the last
   short read before.
6. **Two locals of the node reader are never cleared.** The geometry a type's reader
   answers, and the map from the file's edges to the rebuilt ones. A null or a joint whose
   list has token 3 hands `SI_modelAddDefinition` whatever the stack held, if it is not 0;
   a mesh without an edge list frees whatever the map's slot held, if it is not 0; token
   0xff22 hands the map to `SI_indexEdgePropagate` on the same condition. In the emulator
   the geometry slot can hold a return address; a later token 2 that makes the node a mesh
   then has the end of the node write material indices into the program's code (which the
   harness leaves writable: `hrclib.py` detects the store, counts the case as stopped and
   restores the code).
7. **Without a prototype list, the end of the file is an endless loop**: the token keeps
   its value, every further read fails. The port records a fault and ends the record.
8. **`si_fileSkipField` with a negative length** asks `fread` for one item of that size
   into 1,024 bytes of stack; **`SI_stringBinaryRead`** collects into 256 bytes of stack
   without a bound. Up to 259 bytes a string comes out whole; 260 to 263 overwrite only the
   saved frame pointer (the routine still returns: emulated); from 264 the return address.
   The port records a fault from 260.
9. **The list of shader references** (`si_udfList`) grows by one slot whenever none is
   free and never shrinks; a material file with one shader leaves two slots, because the
   three references it stores with an empty name are each allocated and disposed of.
10. **What a truncated file leaves allocated**: the working copy's user data list of a
    texture (the new texture gets copies; the first list is never freed), the shader
    reference and user data of every 3D texture after a material's or node's first, a
    replaced prototype list, the zero-byte block of every `SI_calloc` with a count of 0.
11. **Sons and brothers nest without a bound**, and a "has a son" flag read at the end of
    a file keeps the scratch short's value: a truncated model can recurse until the stack
    is used up. The port stops when the image's stack is.
12. The material reader links the new material at the head of the scene's material list
    (`SI_grafLists[SI_procData[1]]` +0x14, through the material's +0x6e4) before it reads
    anything.

### Emulation check

`hrc-emucheck` (pattern of `dict-emucheck`): a case's buffers and globals are placed in the
image where the harness had them, the file is served by the same in-memory streams under
the same handle, the port runs with the image stopping at the first fault, and the return
value, every buffer, every heap block (size, each written byte, each unwritten byte still
fill), the frees in order, the stream's position and state, and the outside calls with
their arguments are compared. Generators: `re/tools/si-hrc/gen.py` with `gen_io.py`,
`gen_alloc.py`, `gen_matio.py`, `gen_model.py` and `hrclib.py` (records in this build's
token order, in old orders, shuffled with unknown tokens of the three kinds, without
lists, cut at every byte or at a random place; for the file readers a refused header, a
wrong tag, no file, a pipe).

| | Cases | Identical | Identical in the defined words | Different | Not compared |
|---|---:|---:|---:|---:|---:|
| 54 vector files | 12,337 | 8,799 | 2,291 | 0 | 1,247 |
| with `EMUCHECK_MUTATE=1` | 12,337 | 114 | 0 | 10,892 | 1,331 |

Not compared: 698 the original did not finish (a count taken from a cut file runs until
the heap is used up; a pointer taken from stack is followed); 525 in which the port's runs
on two fill patterns and on zeroed memory do not agree where the original left a defined
value, and 21 in which one pattern agrees and the other does not: a word taken from stack
that this call never wrote, which in the original holds what an earlier callee left there
(points 4 to 6; no port without the original's frames can have it); 3 strings of 260 to 263
bytes. In 1,605 of the compared cases the residue bytes of point 4 are left out of the
comparison. The 114 cases the mutation does not flag return nothing and change nothing.
Per routine: `re/out/si-hrc/check.txt`. The node reader's own rows: `modelBinaryReadNode`
600 cases, 149 compared; reading over an existing node 200, 143; `SI_fdmodelBinaryRead`
200, 123; `meshBinaryRead` 700, 629. In these the ten readers that are not on the image
(below) and the rebuild of a mesh are stand-ins; the rebuild's stand-in makes up stars and
edges so that the copying of edge flags runs.

The harness's heap is 256 KB (64 KB for the material and texture readers) in these runs,
and the check's image has the same: with the default 16 MB a count from a cut file made
the emulated original allocate 800,000 blocks, which the harness then dumped byte by byte
(3.6 GB of Python objects before the run was stopped).

### Corpus

`hrc-imagecheck <dir>`: each sample file read by the image form into a fresh
`Mem::program()` image and by the value form; the records in the image are read back
through adapters (marked `// adapter: not in place`) and compared field by field.

| | `.mtr` | `.txt` | `.t3D` | `.hrc` |
|---|---:|---:|---:|---:|
| files with a binary header | 4,552 | 338 | 327 | 1,905 |
| read by the image form | 4,552 | 338 | 327 | 985 (5,567 nodes) |
| faults of the image | 0 | 0 | 0 | 0 |
| stream ends where the value form's reader ends | 4,552 | 338 | 327 | 985 |
| same record | 4,552 | 299 | 150 | 979 |
| differs | 0 | 39 | 177 | 6 |

Every disagreement is the value form's: 177 3D textures of version 2.1 (channel: point 1),
39 textures (the fifth cropping short: point 3), and in the models 4 with a 3D texture
(point 2: the value form uses the model's version) and 2 with a cropped texture. The
largest image: 8.7 MB of heap at its peak, 42,069 live blocks (`ViewPoint/wsavan1h.hrc`);
a material takes 4.7 KB. The 920 models not attempted have a geometry or chunk whose
reader is not on the image (first reason found per file: 367 a spline, 219 a face, 124 an
inverse kinematics chunk, 115 a patch, 40 a dynamics chunk, 33 a NURBS curve or surface,
15 a cluster, 7 an envelope). For the models compared, five fields of each 2D texture that
`SI_txt2dSetWrap` sets are left out on both sides, as are the file's edge list and the
chain dynamics state.

### Not on the image

- Readers of `binary.c`, reached through `Outside` until they are ported: `patchBinaryRead`,
  `faceBinaryRead`, `splineBinaryRead`, `nurbsCrvBinaryRead`, `nurbsSrfBinaryRead`,
  `metaSystemBinaryRead`, `metaballBinaryRead`, `dynamatterReadBinaryChunk`,
  `dynajntpropReadBinaryChunk`, `clustersReadBinaryChunk`, `envelopReadBinaryChunk`; and of
  other files `SI_invKinReadBinaryChunk`, `SI_odynamicReadBinaryChunk`,
  `SI_oEnvelopReadBinaryChunk`. A model that has one of these is not read correctly yet:
  the stand-in reads nothing and the stream is then out of step.
- What the readers call and other packages own: the rebuild of a mesh and its normals,
  `SI_modelUpdateMatrix`, `SI_modelUpdateSonState`, `SI_matrixXyzRotation`,
  `SI_modelChangeName`, `SI_txt2dSetWrap`, `SI_updTxt3dTurb`, `SI_envelopRebuild`, the
  allocators of `settings.c` and `invkin.c`, the custom effects' shader loading and byte
  order routines. After a read the node's matrices, a mesh's edges and normals, a 2D
  texture's wrap flags and a 3D texture's turbulence object are therefore not made.
- The "lite" twins of the model reader, shape and cluster animation files, the writers.

### Corrected since (image pass)

- "3D texture … channel (default 1; 2 in a file below 2.1)": below the double 2.1, which
  the float 2.1 is. The same holds wherever this note says "below" a version: the value
  form's `below` narrows the constant and is wrong for a file whose version is exactly the
  float of a constant that rounds down (2.1 is the one case in the corpus).
- "Token 25 … bound 3, and the short after the bounds": that short is read and then lost.
- "Byte for byte … 335 of 338 textures identical to the file": for the 39 textures whose
  fifth short is not 0 the program would write 0 there.

### Commands

```bash
export CARGO_TARGET_DIR=$PWD/target/si-hrc
(ulimit -v 8000000; cargo test -p si-hrc; cargo build --release -p si-hrc)        # 28 unit tests
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-hrc/gen.py re/out/si-hrc/vec)   # about 25 minutes, 370 MB
(ulimit -v 2000000; timeout 600 target/si-hrc/release/hrc-emucheck re/out/si-hrc/vec)                          # 5 s
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 600 target/si-hrc/release/hrc-emucheck re/out/si-hrc/vec)
(ulimit -v 2000000; timeout 900 target/si-hrc/release/hrc-imagecheck re/bin/si3d40-ia/installed)               # 45 s
python3 -I re/tools/si-mem/check_globals.py si-hrc; python3 -I re/tools/si-hrc/check_outside.py
```

## The readers on the memory image, second pass: the other object kinds (2026-10-13)

The first image pass had materials, textures and meshes on the image and reached the
other readers of `binary.c` through `Outside`. This pass ports them from the listing (the
value form beside them as a guide), with the allocators they call. Everything here is
**read from code and emulated** unless it says otherwise.

### What is on the image now

51 functions more, 109 in all, every one compared with the emulated original.

| File of `src/image/` | Original source | Functions |
|---|---|---|
| `io.rs` (added) | `byteswap.c` | `validate_double`, `SI_double_fread`, `SI_Matrix_fread`, `SI_3doubles_fread` |
| `spline.rs` | `spline.c`, `face.c`, `binary.c` | `SI_splineInit`, `SI_splineAllocate`, `SI_splineKeyAllocate`, `SI_splineTransfer`, `SI_splineCopy`, `SI_bezierShrink`, `SI_splineDispose`, `splineBinaryRead`, `SI_faceAllocate`, `faceBinaryRead` |
| `patch.rs` | `patch.c`, `binary.c` | `SI_patchAllocate`, `patchBinaryRead` |
| `nurbs.rs` | `nurbs.c`, `binary.c` | `SI_nurbsCrvAllocate`, `SI_nurbsSrfAllocate`, `SI_nurbsPointsAllocate`, `si_knotsAllocate`, `SI_nurbsCrvInit`, `SI_nurbsCrvDispose`, `nurbsCrvBinaryRead`, `nurbsCompBinaryRead`, `nurbsSrfBinaryRead` |
| `chunks.rs` | `binary.c`, `klusters.c`, `envtools.c`, `invkin.c`, `settings.c`, `matrix_gen_dbl.c`, `matrix3tools.c` | `clustersReadBinaryChunk`, `SI_klusterAddToList`, `envelopReadBinaryChunk`, `envelopVertReadBinaryChunk`, `SI_envelopInfoAllocate`, `SI_envelopListAllocate`, `SI_envelopVertAllocate`, `SI_envelopInfoDispose`, `si_envelopVertListDispose`, `SI_invKinReadBinaryChunk`, `SI_invKinAllocate`, `SI_invKinRootAllocate`, `SI_generalMatrixAllocateDbl`, `dynamatterReadBinaryChunk`, `SI_dynamatterAllocate`, `SI_dynamatterSetDefault`, `dynageometrySetDefault`, `SI_matrix3Copy`, `dynajntpropReadBinaryChunk`, `SI_dynajntpropAllocate`, `SI_dynajntpropSetDefault` |
| `meta.rs` | `metaball.c`, `binary.c` | `SI_metaballAllocate`, `SI_metaSystemAllocate`, `SI_metaSystemAddGroup`, `metaballBinaryRead`, `metaSystemBinaryRead` |

`validate_double` has no vector file: it runs inside every case of the readers that call
it. Several of these routines exist in other crates in their own forms (`si-spline`,
`si-nurbs`, `si-kine`, `si-rensetup`, `si-math`, `si-deform`, `si-types`' value
allocators); the image ones of `si-spline` and `si-nurbs` cannot be used here because those
crates depend on this one. They are ported again and cited (the lead reconciles duplicates).

Records: `si-types`' `Spline`, `Face`, `Patch`, `NurbsCrv`, `NurbsSrf` type the pointers;
the fields are addressed by named offsets read from the listing (`spline::spline`,
`spline::face`, `patch::patch`, `nurbs::crv`, `nurbs::srf`). Without a layout in
`si-types`, by named offsets only: the composite curve (`nurbs::comp`, 0x18 bytes), the
envelope with its lists and vertices (`chunks::envelop`: 0xa0, 0x10, 0x14), the inverse
kinematics record and the matrix of doubles (`chunks::invkin`: 0x74, 0xc), the dynamics of
matter and of a joint (`chunks::dyna`: 0x84, 0xc), the cluster (`chunks::kluster`, 0x20),
the metaball, metasystem, group, primitive and unblend pair (`meta::ball`, `meta::system`:
0xf8, 0x68, 0x14, 0x14, 0x18). Globals added: the spline counter (`count.225`,
0x086d6960) and `SI_matrix3ID` (0x086dc220, stored by `globals::store_data`).

Still behind `Outside`, each written `name (0xADDRESS)` where it is declared:
`SI_splineUpdateMatrix`, `SI_allDummy`, `SI_patchUpdateMatrix`, `SI_nurbsSrfForceCapping`,
`SI_edAddCluster`, `SI_dynajntvarAllocate`, and from the first pass the matrices of a node,
the rebuild of a mesh, `SI_metaSystemResolveGroupName`, `SI_envelopRebuild`,
`SI_odynamicReadBinaryChunk`, `SI_oEnvelopReadBinaryChunk`, `SI_dynachnvarAllocate`,
`SI_modelKineStateAllocate`. The node reader calls the ported readers directly; it asks
`Outside::stands_in` first (a new method with a default that says no), so that the first
pass's vector files, recorded while those readers were stand-ins, still check it the way
they were made (`hrc-emucheck` says yes for a vector without the word `v_pass`, which the
second pass's generators add).

### What the listing and the emulator settled

1. **Two readers skip every unknown token as a long field.** `patchBinaryRead` and
   `nurbsCompBinaryRead` hand `si_fileSkipField` the token −1, not the token met: any
   unknown token is taken to be followed by a 4-byte length (instruction listing at
   0x083f0aa3; the value form had this right).
2. **NURBS point flags are read when `fileVersion` is not below the double 2.6**
   (0x0865bf30, 0x0865bf38; `flds`, `fcompl`): a file whose version is the float 2.6
   (2.5999999) has no flags. The value form narrows the constant and reads one: it is wrong
   for such a file (none in the corpus). "From version 2.6" in the NURBS table means above
   the float 2.6.
3. **An envelope of a file up to version 2.59** (`fileVersion` not above the double 2.59,
   0x0865bd90) keeps its vertices' matrices in one zeroed array per list, a table of the
   arrays, and a zeroed 0x14-byte record (no tag) **written into the node's user data
   pointer** (+0x34c, replacing the node's list) whose data word is the table.
4. **Token orders that crash the original** (each emulated): an envelope whose list has
   token 10 or 19 before token 21 stores through a null pointer (the last list, a local
   only token 21 sets); a closed old Bézier point list of fewer than three points makes
   `SI_bezierShrink` write through a null key array; 21 without 20 runs on a count the
   stack held. **The cluster reader keeps its cluster in `edi` and sets it only at token 1**:
   a count or state token before the name is stored through whatever the caller left in
   that register. Called from the node reader after old user data (token 22), `edi` points
   into the constant strings `CUSA`…`CUSD` that the inlined comparisons walked, and the
   count lands in the format string `%c%s` behind them (emulated: the original wrote
   0x086ce6dc and 0x086ce6e8).
5. **`SI_Matrix_fread` swaps the sixteen floats whatever arrived**, and
   `SI_3doubles_fread` swaps three doubles per item asked for: at the end of a file a
   metaball's matrix or exponents, or an envelope vertex's matrix, keep their old words
   byte-reversed.
6. **Copies drop and leak**: `SI_splineTransfer` copies no name (a face's outline and holes
   have none) and does not free the keys the destination had; `SI_nurbsCrvInit` does not
   free what the destination pointed to and keeps its point pointer for a source without
   points; `SI_envelopInfoDispose` frees the first list's record only.
7. `SI_envelopListAllocate` and `SI_envelopVertAllocate` do not check their allocation;
   `SI_generalMatrixAllocateDbl` answers null as soon as one row fails and leaves what it
   had allocated.
8. **The node reader has a stack slot of its own for each of these** (instruction listing,
   `-0x3c(%ebp)` and `-0x48(%ebp)` and so on): the lengths of the clusters on components
   (0xff1d, 0xff1f, 0xff21) and of the user data on components (0xff1e, 0xff20, 0xff22),
   the user data's count and a cluster's state, the short of token 0xc in mode 1 and that
   of token 0x224, the lengths of 0xff1c and 0xff24. The first image pass's port shared
   one word between each pair; at the end of a file, where a read fails and the word keeps
   its value, the port then took the other token's length (found by the new node cases:
   a cluster the original allocated and the port did not). Corrected; one case of the
   first pass's vectors that its two runs could not decide is now compared.
9. The metasystem reader reads the counts of groups and of unblend pairs into one local
   short; the inverse kinematics chunk reads its number of joints into a local (at the end
   of a file: what the stack held); a group's primitives and the system's groups and pairs
   are linked at the head.

### Emulation check

`hrc-emucheck` as before; the second pass's vectors are in `re/out/si-hrc/vec2` (generators
`gen_geom.py`, `gen_chunks.py`, `gen_node2.py` beside the first pass's, with
`hrclib.OUTSIDE2`: the readers of `binary.c` run for real, only the routines of other
source files are stand-ins). About 2,000 cases per reader (records in this build's order,
in old orders, shuffled with unknown tokens of the three kinds, without lists, cut at every
token or byte and at random), fewer for the small routines.

| | Cases | Identical | Identical in the defined words | Different | Not compared |
|---|---:|---:|---:|---:|---:|
| 50 vector files of the readers and what they call | 23,206 | 20,960 | 603 | 0 | 1,643 |
| 3 vector files of the node reader (below) | 2,000 | 1,338 | 215 | 0 | 447 |
| all 53 with `EMUCHECK_MUTATE=1` | 25,206 | 82 | 0 | 23,005 | 2,119 |
| the first pass's 54 files, not regenerated | 12,337 | 8,799 | 2,292 | 0 | 1,246 |

Not compared in the 50 files: 1,358 the original did not finish (a count read at the end
of a file from a local the stack filled, then records until the heap is used up; the
crashes of point 4; a key loop past 32,767), 226 in which the port took a value from stack
this call never wrote (in the original what an earlier callee left: the type short of a
spline read inside a face from a cut file, the cluster register of point 4), 59 in which
uninitialised memory steers the routine where the harness's two patterns did not show it.
The 82 cases the mutation does not flag dispose of a null pointer or make a null record a
chain root's: they return nothing and change nothing (29 compared cases become not
compared under the mutation). Per routine:
`re/out/si-hrc/check2.txt`. The readers with the most not compared: the envelope (592 of
2,400: its counts lie in locals, so most cut files leave one of them as the stack held it;
the generator cuts behind the last count three times in four), the cluster reader (233 of
1,500, the register), the face (213 of 2,000).

### The node reader's coverage

The first pass compared 415 of its 1,000 node cases (416 now, point 8): the others hung
on the geometry local and the edge map, which the original never clears. `gen_node2.py`
builds cases in which the routine writes both before it reads them, with every reader of
`binary.c` running: a file's nodes either all have a type with a geometry and the list has
token 2 in front of token 3, or the list has no token 3 and no node is a mesh; every mesh
list has an edge token; token 0xff22 has data only on a mesh node whose mesh came before
it; a cluster's list starts with its name, an envelope's lists come in the working order
(point 4). Nothing is assumed about what the locals hold otherwise. Cut files are plain
files of nulls without the tokens whose counts lie in locals, with every flag and count
that goes through the scratch short 0 (a node's type goes through it too: in a file with
geometry, a cut before a node's first such flag leaves a "has a son" flag that is not 0,
and the original recurses until the heap is used up). The heap is 1 MB for these cases.

| | Cases | Compared | Of which in the defined words | Different | Not compared |
|---|---:|---:|---:|---:|---:|
| `modelBinaryReadNode#whole` (mode 0 and 2) | 1,200 | 874 | 164 | 0 | 326 |
| `modelBinaryReadNode#over2` (mode 1) | 300 | 284 | 31 | 0 | 16 |
| `SI_fdmodelBinaryRead#whole` | 500 | 395 | 20 | 0 | 105 |
| the first pass's three files | 1,000 | 416 | 164 | 0 | 584 |

What remains: 409 cases the original does not finish. Of `#whole`'s, 111 end in the
harness's `fread` storing file bytes through a null pointer and 137 at the instruction
budget: in both a size or count read from data the generator's file does not have in
step with the reader (on purpose in places: an unknown token of a patch or composite
curve is written without the length those readers expect one time in three; the cause
was not determined case by case), or a file cut where a flag keeps a value that is not 0;
the rest are the crashes of point 4 and of old Bézier point lists. 38 cases take a word
from stack that the call never wrote (the type short of a spline inside a face, in a cut
file). In 730 compared cases the texture residue bytes of the first pass are left out.

### Corpus

`hrc-imagecheck`, after each group (`--skip-kind 7,8,10` gives the second line):

| | models read into the image (of 1,905) | agree with the value form | disagree | faults |
|---|---:|---:|---:|---:|
| first image pass (meshes only) | 985 | 979 | 6 | 0 |
| with splines, faces, patches, NURBS curves | 1,682 | 1,674 | 8 | 0 |
| with the chunks of a node (NURBS surfaces and metaballs left out) | 1,878 | 1,866 | 12 | 0 |
| with NURBS surfaces and metaballs: everything | **1,905** (27,577 nodes) | **1,893** | 12 | 0 |

Every model's stream ends where the value form's reader ends. The 12 disagreements are
the two the first image pass found, more of them now that more models are read, and the
emulated original sides with the image form in both: 10 models with a 3D texture whose
channel the value form takes from the model's own version (9 on a node, 1 in a material;
point 2 of the first pass), 2 models with a cropped texture whose fifth short the value
form keeps (point 3). The corpus has no old Bézier point list of type 1 (no spline was
shrunk), no NURBS file of version 2.6, and no metaball. Left out of the comparison on both
sides, as before: the five wrap fields of a 2D texture, a mesh's file edges, the chain
dynamics state. The largest image: 8.7 MB of heap at its peak (`ViewPoint/wsavan1h.hrc`),
51,343 live blocks at most (`Human_Express/…/Man-H_18638P_30.1-0.hrc`).

### What the checks found

- In this pass's ports: no case of the 51 functions differed from the emulated original
  on its first run. Three addresses written from memory for outside routines were wrong
  (`SI_splineUpdateMatrix`, `SI_allDummy`, `SI_nurbsSrfForceCapping`) and were corrected
  from the function table before any case ran (`check_outside.py`: 214 names, 0 wrong).
- In the first image pass's node reader: the shared locals of point 8 (found by the new
  node cases, corrected).
- In the generators: a made-up father pointer that the envelope reader now follows; an
  existing node whose cluster list was filler; whole hierarchies too large for a 256 KB
  heap; counts in locals left unset by cut files. Each showed as stops or differences of
  the original and was changed in the generator, not in the port.
- In the value form: point 2 (a NURBS file of version 2.6 would be read with flags; no
  such file in the corpus); in the corpus only the two disagreements of the first pass.

### Not on the image

- The lite twins (`modelBinaryReadNodeLite`, 74 KB of listing, `SI_fdmodelBinaryReadLite`,
  `SI_omodelBinaryReadLite`) and the writers: not attempted this pass.
- What the readers call in other source files and other packages own (above): the
  matrices of nodes, splines and patches, phantom keys, forced capping, the cluster editor,
  the dynamics' working states, the group names of a metasystem, the rebuild of meshes and
  envelopes, the two chunk readers of the oldest files. After a read these are therefore
  not made.
- The `printf` of the two dynamics chunk readers when their record cannot be allocated is
  not reproduced (no case reaches it: the heap is never that full there).

### Corrected since (second image pass)

- The first image pass's list of readers "reached through `Outside` until they are
  ported" is done, except `SI_odynamicReadBinaryChunk` and `SI_oEnvelopReadBinaryChunk`
  (other source files).
- The NURBS table's "from version 2.6" is "not below the double 2.6" (point 2).
- `hrc-imagecheck` no longer leaves models out; its `unsupported` only serves
  `--skip-kind`.

### Commands (second pass)

```bash
export CARGO_TARGET_DIR=$PWD/target/si-hrc
(ulimit -v 8000000; cargo test -p si-hrc; cargo build --release -p si-hrc)        # 31 unit tests
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-hrc/gen.py re/out/si-hrc/vec2 1 gen_geom)    # 6 minutes
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-hrc/gen.py re/out/si-hrc/vec2 1 gen_chunks)  # 6 minutes
(ulimit -v 6000000; timeout 5400 uv run --with unicorn python3 -I re/tools/si-hrc/gen.py re/out/si-hrc/vec2 1 gen_node2)   # 20 minutes
(ulimit -v 2000000; timeout 600 target/si-hrc/release/hrc-emucheck re/out/si-hrc/vec2)      # second pass
(ulimit -v 2000000; timeout 600 target/si-hrc/release/hrc-emucheck re/out/si-hrc/vec)       # first pass: 12,337 / 8,799 / 2,291 / 0 / 1,247
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 600 target/si-hrc/release/hrc-emucheck re/out/si-hrc/vec2)
(ulimit -v 2000000; timeout 900 target/si-hrc/release/hrc-imagecheck re/bin/si3d40-ia/installed)   # 50 s
python3 -I re/tools/si-mem/check_globals.py si-hrc; python3 -I re/tools/si-hrc/check_outside.py
```

## Third pass (2026-10-14): the thresholds, the value form, the writers on the image

Read from code (the listing; the version tests from the instructions themselves, `objdump`
of the binary as data), measured against files the original wrote (finding 0031, third
pass: `evalScene`'s saved databases; second and third passes: `bin2ascii`'s files written
back from its texts), unless said otherwise.

### The thresholds and the value form

Finding 0006 section 10 found that the value form's `below(v, c)` narrowed the constant to a
float. Every version test of the readers loads the float version into the x87 unit and
compares it with a double (`flds fileVersion`; `fcompl c`), except the poly-node colour test
(3.71, a float constant at 0x0865bcc0). For a file whose version is the float nearest a
constant that rounds down, the two forms disagree. The constants that round down: 2.1,
2.59, 2.6, 2.67906, 2.999019, 3.50009. Those that round up: 2.69, 3.49089, 3.82099. Mended,
everywhere in the value form: `below` (3D texture 2.1, user data 3.82099, old user data
3.50009, envelope of a chain root 3.49089), `is_old_udf_version` (2.67906, 2.69,
2.999019), the envelope's "up to 2.59" and the NURBS point flag's "from 2.6". Unit test
`versions_are_compared_as_floats_against_doubles`.

**Which version a reader consults was the second fault.** The value form judged every
record by the file's own version. The original has three globals (image pass, point 2):
`b_matio.c`'s `fileVersion` (0x08817f20: set only by the three material and texture file
readers, 0 in a fresh program; the 3D texture reader's 2.1 test), `binary.c`'s (0x08817f28:
set only by the model reader; every test in `binary.c` and the user data reader, also inside
a material or texture file), and `SI_versionInput` (0x086793f0: set by every accepted
header, 4.0 at start; the shader reference reader). The value form now carries the three
(`Versions`); `read_file` starts from a fresh program's, `read_file_with` takes and updates
the caller's. So a 3D texture inside a model read in a fresh program gets channel 2,
whatever it stores, as in the original.

The fifth cropping short (image pass, point 3) is now dropped by the value form as by
`textureTransfer`. The writer test below found two more places where the value form kept
what the original's copies drop: **a face's outline and holes have no name**
(`SI_splineTransfer`, 0x0848aa9c, copies none), and **a 2D or 3D texture's internal shader
reference** (token 3 of 1) is not copied (`SI_udfCopy`, 0x0854c70c). No sample file has
either (the counts below did not move); unit test `what_the_readers_copies_leave_out`.

Effect on the checks (the reader records each change as a note; `hrc-check` attributes
every difference to one):

| | before | after |
|---|---:|---:|
| `hrc-imagecheck`, records the two forms agree on: models, `.mtr`, `.txt`, `.t3D` | 1,893, 4,552, 299, 150 | **1,905, 4,552, 338, 327** |
| `hrc-check`, files written with their own table identical: models, `.txt`, `.t3D` | 1,597, 335, 219 | 1,587, 296, 150 |

The 10 models, 39 textures and 69 3D textures that left the second row are exactly those
the original changes on reading: 3D texture channel 2 below `b_matio.c`'s 2.1 (8 models,
2 more together with the node colour), the fifth cropping short (2 models, 39 textures),
channel 2 in 3D texture files of version 2.1 (69). The value form is not retired: nine
crates call it (`si-ascii`, `si-kine`, `si-rensetup`, `si-render`, `si-model`,
`si-spline`, `si-scene`, `si-emuworld`, `si-nurbs`) and the tool
`re/tools/si-motion/evalcheck`. `si-ascii`'s local correction `b2a::texture3d_old_channel`
is no longer needed. A loader that reads several files as one program (`si-scene`)
reproduces the original's 3D texture channels inside models only if it passes one
`Versions` through `read_file_with`.

### The writers on the image

`src/image/wio.rs` (`byteswap.c`, `filetoken.c`, `headers.c`), `proto.rs`
(`SI_prototypeBinaryWrite`), `matwrite.rs` (`b_matio.c`), `modelwrite.rs` (`binary_b.c`,
`binary.c`, `invkin.c`, `klusters.c`): 46 functions. **33 measured and cited**:
`SI_byteswap_fwrite`, `SI_Vector_fwrite`, `SI_RGB_fwrite`, `SI_RGBA_fwrite`,
`SI_FileChunk_fwrite`, `SI_stringBinaryWrite`, `SI_headerBinaryWrite`,
`si_newheaderBinaryWrite`, `SI_dbfileWriteBinHeader`, `SI_prototypeBinaryWrite`,
`si_udfWrite`, `si_textureBinaryWrite`, `si_texture3DBinaryWrite`, `si_materialBinaryWrite`,
`SI_materialWriteBinaryFile`, `SI_textureWriteBinaryFile`, `SI_texture3DWriteBinaryFile`,
`SI_omodelBinaryWrite`, `SI_fdmodelBinaryWrite`, `SI_modelBinaryWriteNode`,
`meshBinaryWrite`, `faceBinaryWrite`, `splineBinaryWrite`, `patchBinaryWrite`,
`nurbsSrfBinaryWrite`, `dynamatterWriteBinaryChunk`, `dynajntpropWriteBinaryChunk`,
`clusterWriteBinaryChunk`, `clustersWriteBinaryChunk`, `SI_clusterCount`,
`envelopWriteBinaryChunk`, `envelopVertWriteBinaryChunk`, `SI_invKinWriteBinaryChunk`
(the swaps they share with the readers were on the image already). **13 written and
unchecked**, no
file the original wrote reaching them: `si_userDataSize`, `si_userDataWrite`,
`userDataClusterSize`, `userDataArrayWrite` (only their empty paths are reached),
`nurbsCrvBinaryWrite`, `nurbsCompBinaryWrite`, `si_metaballBinaryWrite`,
`si_metaSystemBinaryWrite`, `SI_3doubles_fwrite`, `SI_Matrix_fwrite` (all ten held against
the value form's writer by a unit test, below), `setT2dMapDomainID`, `si_filenameCleanup`,
`modelBinaryWriteCurrent`. Behind `Outside` (`modelwrite::ALL`): the conversions and
triangulation of a triangulated save, the copies and transformations of a save in global
coordinates, the custom effects' byte order routines, `SI_cusAddUserData`,
`SI_metaSystemValidateGroups`, `SI_elemdictGetElem`, `SI_statusDisplayMessage`. The
outside world gained `fwrite`, `fputc`, `fflush`, `ferror` (with defaults, so existing
implementations compile) and `Files` serves writing.

What the listing settled:

1. **`SI_versionOutput` (0x086793f4) is never written** (128 reads in the listing, no
   store): every version test of every writer takes the 4.0 branch. The tests come in four
   forms (strictly above, not below, above or NaN, below), kept as read (`wio.rs`).
2. **The writers are fixed sequences**, not walks of a table: what they write is the order
   of the table `SI_prototypeBinaryWrite` writes at 4.0 (the table of the first section).
3. **Every swapping write swaps the record in place**, hands it to `fwrite` and swaps it
   back; the global `tmp` keeps the last byte that passed through it.
4. A model file is a general header (magic 0x5380f634, `SI_versionOutput`, "No
   comment..."), the table, `HRCH`, the root and its sons; `SI_omodelBinaryWrite` and
   `SI_fdmodelBinaryWrite` give the same bytes. The element saver passes 0 as the third
   argument: the node's state short and file name are left alone.
5. **A material or texture file gets the version its caller passes** in its header, the
   body is 4.0's whatever it is (`bin2ascii -k`, below).
6. The node writer's old-form user data: the **first four bytes of the tag** (past the end
   of a shorter tag), the size, the data, the custom effects' data swapped to the file's
   order around the write (`SI_fix_SI_CustomHeader`, `SI_fix_CustomDsoFcurves`,
   `SI_fix_CustomFcurves`; outside). The new form (0xff1c) writes the blocks whose tag
   starts with `_`, and none at all when the list's head block is `__NodeInfo`.
7. Before a node is written, a custom-effect node puts its data into its user data
   (`SI_cusAddUserData`), and each texture coordinate set's texture ids are written into
   the textures' user data as `_UVDomainID_` blocks (`setT2dMapDomainID`): saving changes
   the textures.
8. A face of NURBS curves holds an array of curve records 100 bytes apart; its holes are
   spline records 0x74 apart. The length of a NURBS surface's projected curves (0xff1a)
   comes from a counting pass of the same curve writer.
9. In a triangulated save (`SI_saveTriMesh`, 0x086793e2): patches, faces, meshes and NURBS
   surfaces are written as triangulated meshes (type 4), a metasystem as an untriangulated
   mesh, a metaball as a null (type 0, no shape); splines and NURBS curves as they are.
10. A NURBS writer that fails shows a message and the node writer answers 0; nothing else
    looks at a write's result.

### Against the files the original wrote

`re/tools/si-hrc/saved_pairs.py` pairs every model, material and 2D texture file the
original's `evalScene` saved (36 scenes, four variants; the saved scenes list their elements
in the input scenes' order) with the sample file it was loaded from (8,684 pairs; no 3D
texture file was saved), and adds the 4,464 material and 3D texture files `bin2ascii` wrote
back from its own texts. `hrc-savedcheck` (`re/tools/si-hrc/savedcheck`: a tool package,
since it links `si-model`, which depends on this crate) reads each into a fresh image with
the image readers and writes it with the image writers, in two ways: **from the input**
(the sample file, compared with what the original saved from it) and **fixed point** (the
original's own file read and written again: the writers on the state the original's
writer had, as far as the file holds it). Between reading and writing nothing runs: the
scene's evaluation is not reproduced. Its outside world answers the mesh rebuild after
reading (`SI_meshUpdateNodes` and the normal passes) with `si-model`'s value routines
through the adapter `si-scene` uses (repeated in the tool, marked; without it a mesh read
on the image has no edges), and allocates the chain's and the joint's dynamics states
(zeroed; the writers only ask whether there is one).

| Kind | Variant | Files | Fixed point identical | From the input identical |
|---|---|---:|---:|---:|
| model `.hrc` | plain | 216 | **216** | 32 |
| | `-m` | 216 | **216** | 10 (167 reach a conversion not on the image) |
| | `-z` | 216 | **216** | 32 |
| | `-s` | 888 | **888** | 131 |
| material `.mtr` | plain, `-m`, `-z` | 1,008 each | **1,008** each | 1,007 each |
| | `-s` | 4,104 | **4,104** | 4,100 |
| 2D texture `.txt` | plain, `-m`, `-z`; `-s` | 3 each; 11 | **3** each; **11** | 0 |
| material, `bin2ascii` back | default; `-k` | 2,064; 2,064 | **2,064**; 1,688 | (the same files) |
| 3D texture `.t3D`, `bin2ascii` back | default and lower-case suffix; `-k` of both | 168; 168 | **168**; **168** | |

**Every file the original wrote with a version-4.0 body is reproduced byte for byte as a
fixed point**, 14,980 of 14,980 (the value form's writers give the same counts; its writer
has no "keep the version" option, so its `-k` rows differ in the header). The 376 `-k`
materials that are not are all those whose kept version (2.0 to 2.65) lies in the old
shader-reference range: their header tells the reader to take the 4.0 shader references
the body holds for old-form ones, so the original's own reader cannot read them back to
what was written (the port's reader does what the original's does). With `-m` the
from-input write needs `SI_meshCopy`, `SI_patchConvertToMesh`, `SI_faceConvertToMesh`,
`SI_nurbsConvertToMesh` and `SI_meshTriangulate`: those are other packages' and are not on
the image (`si-model` has `SI_meshCopy`, `SI_meshDispose` and `SI_faceConvertToMesh` on its
own values, and only helpers of `SI_meshTriangulate`; `si-spline` has
`SI_patchConvertToMesh` on values; `SI_nurbsConvertToMesh` and `SI_meta2Mesh` are nowhere).
`-z` saves the same model files as the plain save.

**What separates the original's saved file from its input** is what the program does
between loading and saving, each file classified by the fields the two files read
differently (`re/out/si-hrc/savedcheck_models_list.txt`): transformations at the saved
frame (the animation, 29 models with only these in the plain save); the shapes' points and
normals (deformations by envelopes, shape animation: 21 only these, 100 with
transformations); spline names (the evaluation copies splines, and `SI_splineAllocate`
names a copy from its running counter: `spl5` where the input had `spl7`; 15); the 2D
textures' picture paths, which the load resolves into the database being saved
(`/data/<db>/PICTURES/...`, or `(null)/noIcon` when the picture is not found: 10 models and
every saved `.txt`); animated material colours (1 material, 4 in `-s`). One model per
variant (`Human_Express …/Homme-H_18638P_4`) also differs in its envelope's fields; not
determined whether the load's `SI_envelopRebuild` or the evaluation changes them. None of
these is the writers'; each belongs to `si-scene` (load), `si-anim`/`si-motion`
(evaluation), `si-deform`.

What the saved files reach (`hrc-savedcheck --coverage`): meshes in 1,014 files, splines
274, inverse kinematics 256, faces of splines 118, patches 84, clusters 80, dynamics of
matter and joints 48 each, envelopes 24, initial transformations 24, old-form user data
20, shader references 16, NURBS surfaces 7 (none with trims or projected curves); 7,924
materials, 112 2D textures, 168 3D textures. Not reached: NURBS curves, faces of NURBS
curves, composite curves, metaballs and metasystems, user data lists of materials,
textures and components, clusters of components, texture coordinate sets, global
coordinates.

**Unit test `the_writers_write_back_what_the_readers_read`**: the value form writes the
model of every prototype and token (`tests::everything`), the image readers read it and the
image writers write it again, the bytes must be the same; `material_and_texture_files_write_back`
the same for the three other files. This is where the unchecked writers above run, held
against the value form's writer (two ports of one listing), not against the original.

### Not done this pass

- **The lite readers** (`modelBinaryReadNodeLite` 0x083ee024, `SI_fdmodelBinaryReadLite`
  0x083ec834, `SI_omodelBinaryReadLite` 0x083ec670; called only by
  `dbListPrepareMdlTxtsList`, the database listing of a model's textures). Read so far: the
  lite node reader reads the same tokens through the same type readers with a different
  arrangement (the mesh is read without the edge map), does not update the node's matrices
  at the end (no `SI_modelUpdateMatrix`, `SI_modelUpdateSonState`), and
  `SI_fdmodelBinaryReadLite` does not call `SI_envelopRebuild`. Not ported.
- Emulation of the 13 unchecked writers (the harness records what a routine writes to a
  `FILE(w)` argument; `hrc-emucheck` does not compare written streams yet).
- The shape and cluster animation files of `binary.c`/`binary_b.c`
  (`SI_shapeBinaryRead/Write`, `SI_clusterAnimBinaryRead/Write`).

### Corrected since (third pass)

- "Not on the image: … the writers": on the image (above).
- "The version tests inside the record writers were read but only their outcome at 4.0 is
  reproduced": they are reproduced as read, and 4.0 is the only version this build can
  write (`SI_versionOutput` is never written).
- The first image pass's point 1 and "Corrected since (image pass)": the value form now
  compares as the original does; the disagreements of the corpus table there are gone.

### Commands (third pass)

```bash
export CARGO_TARGET_DIR=$PWD/target/si-hrc
(ulimit -v 8000000; cargo test -p si-hrc; cargo build --release -p si-hrc)        # 36 unit tests
(ulimit -v 8000000; cargo build --release --manifest-path re/tools/si-hrc/savedcheck/Cargo.toml)
(ulimit -v 2000000; timeout 600 python3 -I re/tools/si-hrc/saved_pairs.py)        # re/out/si-hrc/saved_pairs.tsv, 13,148 rows
(ulimit -v 2000000; timeout 1800 target/si-hrc/release/hrc-savedcheck re/out/si-hrc/saved_pairs.tsv)   # 20 min; --form value; --list; --show <file>; --coverage
(ulimit -v 2000000; timeout 600 target/si-hrc/release/hrc-emucheck re/out/si-hrc/vec)    # 12,337 / 8,799 / 2,292 / 0 / 1,246 (unchanged)
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 600 target/si-hrc/release/hrc-emucheck re/out/si-hrc/vec)    # 114 / 0 / 10,892 / 1,331
(ulimit -v 2000000; timeout 600 target/si-hrc/release/hrc-emucheck re/out/si-hrc/vec2)   # 25,206 / 22,298 / 818 / 0 / 2,090 (unchanged)
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 600 target/si-hrc/release/hrc-emucheck re/out/si-hrc/vec2)   # 82 / 0 / 23,005 / 2,119
(ulimit -v 2000000; timeout 900 target/si-hrc/release/hrc-imagecheck re/bin/si3d40-ia/installed)
python3 -I re/tools/si-mem/check_globals.py si-hrc; python3 -I re/tools/si-hrc/check_outside.py
```

Outputs of this pass: `re/out/si-hrc/savedcheck_image.txt`, `savedcheck_value.txt`,
`savedcheck_models_list.txt`, `imagecheck3.txt`, `hrc_check3.txt`, `check3_vec.txt`,
`check3_vec2.txt`.

## Two hooks of the readers answered by the loader (2026-10-14, finding 0029's third pass)

`si-hrc` is unchanged. Two of its readers' calls out are now answered by the scene loader
(`si-scene`'s `image::Prog`, finding 0018, section 21) with ports of other crates, as the
original's reader calls them: `SI_splineUpdateMatrix (0x0848abd0)` in `splineBinaryRead` and
`SI_bezierShrink` (`si-spline`'s port: every spline read has the original's step matrix at
+0x24, which `SI_modelTraversePnts` steps the segments with), and `SI_envelopRebuild
(0x084bd2a0)` at the end of `SI_fdmodelBinaryRead` (`si-deform`'s port, finding 0029,
section 17: it gives back the chain roots' records that token 17 disposes below version
3.49089, and the lists their skins). Re-run with the tree of that pass: `hrc-emucheck vec`
12,337 / 8,799 / 2,292 / 0 / 1,246 (`MUTATE=1` 10,892 differ), `vec2` 25,206 / 22,298 / 818 /
0 / 2,090 (23,005), `hrc-savedcheck` every count as `savedcheck_image.txt` (every 4.0 file
the original saved a fixed point): unchanged.

## The shape file reader on the image (2026-10-15)

By agents run one at a time for the lead.

| Routine | Address | Source | Rust |
|---|---|---|---|
| `SI_shapeBinaryRead` | 0x083f1798 | `binary.c` | `src/image/shape.rs` (new) |

**Read from code** (the listing). A model's shape file (`.shp`, finding 0005) holds the
model's shape keys. `SI_shapeBinaryRead(name, node)` reads them onto the node that is
already in memory, as definitions of the node's own kind:

1. `"%s.shp"` is printed into 256 bytes of stack, and the file opened with `"rb"` (or the
   pipe, when `SI_pipedFiles` is set). No stream: the answer is 0.
2. The header is read and its answer not looked at. Its version becomes `binary.c`'s
   `fileVersion` through the x87 unit (a signalling NaN comes out quiet), also when the
   header was refused and the version is whatever the stack held.
3. A tag; if it is `PROT`, the prototype block and another tag; then a 16-bit count.
4. A negative count brings the **current definition** (+0x3c). After a count of −2 comes a
   short for the weight mode (+0x2e2). Then one record of the node's kind (`patchBinaryRead`,
   `faceBinaryRead`, `meshBinaryRead(stream, table, node, null, 0)`, `splineBinaryRead`,
   `nurbsCrvBinaryRead`, `nurbsSrfBinaryRead`; for any other kind one field is skipped). It
   is stored when it is not null, a patch is also handed to `SI_patchUpdateSurface`, and
   the count is read again.
5. When the tag is the kind's (`PTCH`, `FACE`, `MESH`, `SPLN`, `NCRV`, `NSRF` for kinds 1,
   2, 4, 6, 9, 10), `count` records of the kind are read, and each one that is not null is
   appended with `SI_modelAddDefinition(node, kind, record)`. A node still without a current
   definition gets a copy of the list's second entry (`SI_patchCopy`, `SI_faceCopy`,
   `SI_meshCopy`, `SI_splineCopy`, `SI_nurbsCrvCopy`, `SI_nurbsSrfCopy`; the entry is taken
   without a look at the list's length). Another kind's tag: nothing is read and the answer
   is 0. A kind without a tag: one field is skipped.
6. Last, the node's file name (+0x2d4) is freed and replaced by `name` without
   `SI_objectPath` in front (`si_filenameCleanup`). The stream is closed unless it is the
   pipe, and the prototype table is freed.

A path of 256 bytes or more runs over the buffer into the saved registers and the frame: the
port records a fault and answers 0. The copies and `SI_patchUpdateSurface` belong to other
packages and go through `Outside` (`shape::SI_PATCH_UPDATE_SURFACE` and the five copies);
`SI_splineCopy`, `SI_modelAddDefinition` and the record readers are this crate's.
`si-elemio`'s `shape` module reads the same files into values.

**Emulation check.** `re/tools/si-hrc/gen_shape.py` (new, a part of `gen.py`) makes 600
cases (`re/out/si-hrc/vec3`). Each is a node in memory, of one of the six kinds or another,
and a made-up `.shp` file named for it. Most files are what the writer writes. The others cover: a count of −1; no
current definition (the copy, through a stand-in); another kind's tag; a node of a kind
without a tag; records in old and random token orders; no prototype block; a refused header;
a missing file; the pipe; `SI_objectPath` in front of the name; and files cut anywhere. The
node holds no definition or one.

| Routine | Cases | Identical | Different | Not comparable | `MUTATE=1` differ | `=calls` differ |
|---|---:|---:|---:|---:|---:|---:|
| `SI_shapeBinaryRead` | 600 | **503** | 0 | 97 | 503 | 503 |

Of the 97: the original stopped in 69; in 13 a word taken from stack this call never wrote
leaves the port's runs with different memory; in 15 the port's two runs disagree, because
uninitialised memory steers the routine. `si_filenameCleanup`, unchecked until now, is
reached by these cases.

**Why the shapes were missing on the image.** Until now the scene loader read a shape file
into values only (`si-elemio`'s reader, `Scene::shapes`). The model's node in the image kept
the one definition its model file gave it. The update's `animShapeUpdate` (`si-motion`,
finding 0023 §28) clamps its key indices to the last definition, which was 0. So the linear
and cardinal blends blended definition 0 with itself, and the weighted blends do nothing
without a second definition. A shape-animated model therefore stayed as its model file
stores it. The loader now calls this port for each SHAPES element (finding 0018 §24), so
the keys are the node's definitions 1 … n, as in the original.

### Corrected since

- Third pass, "Not done this pass: the shape and cluster animation files": the shape file
  reader is on the image. `SI_shapeBinaryWrite` and the cluster animation files
  (`SI_clusterAnimBinaryRead/Write`) are not.
- `si_filenameCleanup` was "unchecked (nothing in the checks reaches it)": the cases above
  reach it. It has no vector file of its own.

### Commands

```bash
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-hrc/gen.py re/out/si-hrc/vec3 1.0 gen_shape)   # 600 cases
(ulimit -v 2000000; timeout 600 target/release/hrc-emucheck re/out/si-hrc/vec3)                      # 600 / 503 / 0 / 0 / 97
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 600 target/release/hrc-emucheck re/out/si-hrc/vec3)    # 503 differ; =calls the same
```

Verified on a clean worktree of 62e8d6c with only these files
(`re/out/lead/shapekeys/verify.log`): no warnings; 1,273 tests pass.
