# 0010 — Elements, dictionaries and models: the in-memory data model

Date: 2026-10-13. Binary: Linux `soft`, sha256 `ef21a382c3325d42…`. Source files read:
`elemdict.c`, `relations.c`, `reldict.c` (the look-ups; the resource-file reader only as
evidence), `attdict.c`, `enmdict.c`, `init_dict.c`, and from `model.c`, `model_b.c` and the
geometry files the allocation, initialisation, disposal and dump routines. Rust: `crates/si-types`.
Scripts: `re/tools/si-types/`.

Every field below is marked **read** (code was read that uses it and shows what it means),
**named** (the meaning comes from a function name, a file token, the vendor's attribute table or
a manual; the code confirms only offset and width) or **unknown** (offset and width seen, or the
bytes never touched; meaning not established). The field tables at the end are printed from the
Rust definitions, whose offsets and sizes are asserted at compile time, so the tables and the
code cannot drift apart.

## Method

1. **Size** from the allocator: the argument of `SI_malloc` / `SI_calloc`, the stride of the
   array that holds the records, or the size of a global of that type in the symbol table.
2. **Fields** from every function that touches the structure. `re/tools/si-types/sitypes.py
   fields <Type>` streams the Ghidra export and tabulates, for parameters and locals of the given
   type, each access: offset, width, read or write, and the functions involved. Ghidra knows the
   parameter types from the mangled names but not the structures, so an access is spelled with a
   byte offset and is easy to collect. (`regen.sh` reruns every table in 18 seconds.)
3. **Names**, from four sources, in this order of weight:
   - the program's own dump and print routines, which label fields (`SI_modelDump`,
     `SI_reldictDump`, the description-file writers);
   - **the vendor's attribute table.** The program ships a text file, `Relations.rsrc`, that
     lists every attribute of every kind of element with a numeric id, a name and a description
     (564 ids). The routines of `edparams.c` (`SI_modelGetParam`, `SI_…AttrGetValue`, …) switch
     on those ids and each case reads one field. Joining the two (`params.py`) names a field
     with code as witness: id 268 is "WireType: Element's display type", and the case for 268
     reads the word at +0x20 of the element;
   - the binary writers of the model file, which write a structure's fields in the order of the
     file's tokens (`ioseq.py`), paired with the token names of the vendor's file-format pages;
   - function names (`SI_elemdictSchemSetPosX` sets one float: the schematic x position).

## What the allocator leaves in memory

`SI_malloc` (0x080d0ee8) is `malloc` plus two marker bytes: it writes 0x6f (`o`) into the first
byte of the block and 0x6b (`k`) into the last, and clears nothing. `SI_calloc` is `calloc`
(returning null for a count of zero). So a field that an allocator does not set holds whatever the
heap held. Each allocator ported to Rust lists the fields it leaves unset; they are zero there.
Models, meshes, patches, faces, splines, NURBS records, materials, graf nodes and the element
array all come from `SI_malloc`.

## The picture

```
SI_elemDicts ──► array of SI_ElemDict (0x70 each); SI_nbElemDicts of them; elemDictExists[i] says
                 whether slot i is live. Dictionary 0 is the scene. SI_dbed is a separate one for
                 the database browser. SI_procData[0] is the index of the current dictionary.

SI_ElemDict ──► lists: array of SI_ElemList (0x40 each), one per chapter, 36 of them
SI_ElemList ──► elems: array of __SI_Elem (0x84 each); a deleted element keeps its slot
__SI_Elem   ──► node:     SI_DbListNode (0x34): the element's name in the parts of a file name
            ──► data:     the object itself: __SI_Model, SI_Camera, light, __SI_Material, …
            ──► aux_data: for objects on screen, the __SI_GrafNode
            ──► forward, backward: two SI_Relations, each six SI_RelList, each an array of
                          SI_ARelation (0x20) naming another element by (chapter, index)

__SI_Model  ──► father / brother / son: the hierarchy of nodes
            ──► defs[nb_defs], definition: the geometry (SI_Mesh, SI_Patch, SI_Face, SI_Spline,
                          __SI_NurbsCrv, SI_NurbsSrf, metaball records) by kind
            ──► materials[nb_materials], textures (linked list), texture3d
            ──► elem_id: index of its element in the MODELS chapter
```

Three global dictionaries describe the *kinds* of things, and are filled at start-up from
`Relations.rsrc`: the relation dictionary `SI_RD` (which relations exist between which
chapters), the attribute dictionary `SI_ATD` (which attributes an element has, as a tree of
groups) and the enum dictionary `SI_EVD`.

Start-up (`SI_initDictionaries`, 0x0810824c): initialise `SI_RD`; add the first dictionary and
make it current (`SI_procData[0]` = its index, `SI_procData[1]` = 0); load its 36 chapters
into `SI_cid`; create the default scene description; read `Relations.rsrc` from the working
directory, else from the directory given, and exit with a message if neither can be read;
then initialise the database dictionary `SI_dbed` and load its chapters into `SI_dbcid`.

### Who owns what

- A dictionary owns its chapter array; a chapter owns its name, its suffix string, its element
  array and its sort tables (`SI_elemdictDispose`, `SI_elemlistDispose`).
- An element owns its name record, its icon, its suffix string and its relation arrays. It does
  **not** own `data`: the dictionary code never frees an object, and disposing of a chapter
  leaves them all.
- A relation entry owns its relation data, on the source side only.
- A model node owns its name, its definitions, its lattices, fittings, dynamics and envelope
  records, clusters, user data, and its render data. Materials and the 3D texture are shared:
  `SI_modelDisposeNode` disposes of one only when `SI_materialStillUsed` /
  `SI_texture3DStillUsed` say no other node uses it (those two were not read). No reference
  count was found in any of the structures read.
- Disposing of a model disposes of its sons and, optionally, its following brothers, then
  unlinks it from its father's list (`SI_modelDispose`).

## Identifying an element

An element is **(dictionary, chapter index, element index)**. Both indices are plain array
indices: `SI_elemdictGetElem(ed, chap, i)` is `ed->lists[chap].elems[i]`, refused when `i` is out
of range or the slot's status is 2 (deleted).

| | dictionary | chapter | element | further |
|---|---|---|---|---|
| SDK `SAA_Elem` | `ed`: an `int` | `chapid` | `elemid` | `groupid`, `user_data` |
| internal `SI_ElemIdentity` | none: the current one | +0 | +4 | +8, +0xc: indices inside the element |
| internal `SI_ARelation` | none: the same one | 16 bits at +0x10 | +4 | list index at +0x12 |

- The SDK's `ed` is an `int` (`typedef int SAA_ElmDct`): by its use it is the index into
  `SI_elemDicts`, the same number `SI_procData[0]` holds. The internal identity has no
  dictionary because the attribute routines always work on the current one.
- `__SI_Elem` is **not** the SDK's `SAA_Elem`. `SAA_Elem` is a handle (five words); `__SI_Elem`
  is the 132-byte record the handle designates.
- Indices are stable: deleting an element sets its status to 2 and clears its pointers, and the
  slot is reused by the next `SI_elemdictAddElem` (first deleted slot wins). Nothing is compacted,
  so the indices stored in relations stay valid.
- What the SDK's `groupid` corresponds to was not verified. The internal candidates are the
  relation list index (0 to 5) and the element's `root_id`.
- A model node knows its element through `elem_id` (+0x354) and the element knows the node
  through `data`; the element's `aux_data` is the node's graf node.

## Chapters

`SI_chapInfo` (0x086842c0, 432 bytes) is 36 records of three words: chapter name, file suffix,
and the four-character tag of the chapter's ASCII files. `SI_elemdictLoadChapters` adds the 36
chapters to a dictionary in table order, so a chapter's row is also its index, and writes each
index into a `SI_ChapIds` (36 words; `SI_cid` at 0x08781660 for the scene, `SI_dbcid` for the
database). The first 33 words of `SI_ChapIds` are, in order, the SDK's `SAA_ChapIds`; the SDK
header stops before the last three.

| Index | Chapter | Suffix | Tag | `SI_ChapIds` word (SDK name) |
|---|---|---|---|---|
| 0 | SCENES | dsc | SDSC | 3 (ChapDsc) |
| 1 | ANIMATION | ani | SANI | 0 (ChapAni) |
| 2 | CAMERAS | cam | SCAM | 1 (ChapCam) |
| 3 | CLUSTERS | cls | CLUS | 2 (ChapCls) |
| 4 | PICTURES | pic | | 4 (ChapImg) |
| 5 | LATTICES | lat | SLAT | 5 (ChapLat) |
| 6 | LIGHTS | lig | SLIG | 6 (ChapLig) |
| 7 | MATERIALS | mtr | SMAT | 7 (ChapMat) |
| 8 | MODELS | hrc | | 8 (ChapMod) |
| 9 | NSELECTIONS | (empty) | | 9 (ChapNsl) |
| 10 | PALETTES | pal | | 10 (ChapPal) |
| 11 | RENDER_PICTURES | pic | | 11 (ChapRim) |
| 12 | SETUP_SOFT | sts | SSET | 22 (ChapSts) |
| 13 | SETUP_CHANNELS | chn | SCHN | 23 (ChapStc) |
| 14 | SETUP_FLOCK | flk | SFLK | 24 (ChapSfk) |
| 15 | SHAPES | shp | | 21 (ChapShp) |
| 16 | STRUCT_KEYS | key | SKEY | 25 (ChapSky) |
| 17 | TEXTURES2D | txt | ST2D | 26 (ChapT2d) |
| 18 | TEXTURES3D | t3D | ST3D | 27 (ChapT3d) |
| 19 | WAVES | wav | SWAV | 28 (ChapWav) |
| 20 | EXPRESSIONS | exp | | 29 (ChapExp) |
| 21 | MATERIAL_SHADERS | shd | SHAT | 12 (ChapShdMat) |
| 22 | SHADOW_SHADERS | shd | SHAD | 16 (ChapShdSha) |
| 23 | VOLUME_SHADERS | shd | SHVL | 15 (ChapShdVol) |
| 24 | TEXTURE2D_SHADERS | shd | SHT2 | 13 (ChapShdT2d) |
| 25 | TEXTURE3D_SHADERS | shd | SHT3 | 14 (ChapShdT3d) |
| 26 | LIGHT_SHADERS | shd | SHIG | 17 (ChapShdLig) |
| 27 | CAMERA_SHADERS | shd | SHAM | 18 (ChapShdCam) |
| 28 | OUTPUT_SHADERS | shd | SHUT | 19 (ChapShdOut) |
| 29 | ENVIRON_SHADERS | shd | SHNV | 20 (ChapShdEnv) |
| 30 | CHANNEL_DRIVERS | (empty) | | 30 (ChapChnDrv) |
| 31 | OUTSIDE_DB_FILES | all | | 31 (ChapOut) |
| 32 | SPREADSHEET_SETUP | spd | | 32 (ChapSpd) |
| 33 | AUDIO | aiff | | 33 (not in the SDK) |
| 34 | MATERIAL_PHOTON_SHADERS | shd | SHAP | 34 (not in the SDK) |
| 35 | LIGHT_PHOTON_SHADERS | shd | SHIP | 35 (not in the SDK) |

Two quirks of the suffix each chapter is given (`SI_edGetSuffixFromChapter` through
`SI_chapInfoOffsetFromChapId`, a chain of 35 comparisons against the scene's `SI_cid`): the
chain does not test the NSELECTIONS id, so that chapter's suffix string is `???`, not the empty
string of the table; and PICTURES answers with the row of RENDER_PICTURES (the same suffix).
Chapter kind (+0x18 of the chapter) is 1, except 0 for NSELECTIONS and CHANNEL_DRIVERS and 2 for
OUTSIDE_DB_FILES.

## The life of an element

- **Added** by `SI_elemlistAddElem(list, name record, object, second object, description id,
  work id, with relation lists, reuse a deleted slot)`. It returns the index. A new slot grows the
  array by one record (no spare capacity is kept). The record gets the default wire colour
  (`SI_select7WM + SI_color1`), wire type 2 (visible), dirty 1, new-flag 2, its own index as
  hierarchy root, and, when asked, two sets of six empty relation lists. The three sequence
  fields at +0x74 to +0x7f are not set.
- `SI_elemdictAddElem` is that with relation lists and slot reuse, after marking both update
  lists of the dictionary dirty. For a model added while the hierarchy flag (+0x6e) is clean it
  saves the model relations (two routines of `edtools.c`); otherwise it sets the flag.
- **Deleted** by `SI_elemdictSetDeleteElem`: its relations are removed from both ends and,
  when the caller asks for the slot to be given up, the name record and icon are freed, `node`,
  `data`, `aux_data` are cleared, status becomes 2 and new-flag 0. The slot stays.
- **Found** by index, by object pointer (`…FindElemByData`: linear, or by bisection in a table
  sorted by pointer value when the environment variable `SI_LOAD_OPTIM` is set and the global
  `SI_useElemListSortTbl` is on), by second object, by name, or by a caller's test.
- The `new_flag` (+0x46) is 2 at creation; `SI_elemdictCopyElements` gives its copies 1, and
  the description reader looks an element up by (work id, description id) among those with
  flag 1 (`elemdictGetElemFromWorkId`). Deleting sets 0.

One portability scar: `SI_elemlistClearLowByteWorkFlag` and `…LowByteWorkFlagAND` address the
byte at +0x5b of the 32-bit work flag at +0x58. That is the low byte on the big-endian machines
the code was written for; in this little-endian build it is the high byte, and the code was not
changed.

## Relations

A relation joins a **source** element to a **destination** element and has an id (its kind).
It is stored twice: an `SI_ARelation` in a *forward* list of the source and one in a *backward*
list of the destination. Each entry names the other end: chapter (16 bits), element index, and
the number of the list that holds the matching entry there. Only the source's entry carries the
relation's data pointer.

- Each element has six lists per direction (`si_relAllocate` allocates 6 × 0x18 bytes). Which
  list a relation goes into is given by the relation's rule ("group", source and destination).
- A list is kept ordered by **rank** (`si_relAddRelation` inserts before the first entry of
  higher rank; among equal ranks, relations of one kind stay together). A rule also has a
  **fan-out**; the insertion on the source side refuses an entry beyond it. A relation kind with
  a single rule is refused if the same pair is already related, except for six ids (20000, 10000,
  20010, 246, 247, 551).
- An entry may own an element of the ANIMATION chapter (+8, −1 for none), deleted with the
  relation. By the names of the routines that set it (`si_relationActiveRangeSetKey`,
  `relationCheckAddActiveFcv`) it is the curve of the relation's active range; not read.
- Hierarchy is a relation like any other: a son is the source, its father the destination.
  `SI_elemdictUpdateHierInfo(ed, chapter, list, relation id)` finds the roots (elements with
  the relation on their backward side only) and walks down the backward lists, writing each
  element's root index (+0x50) and depth (+0x54).

The **relation dictionary** (`SI_RelDict`, global `SI_RD`, 16 bytes) is an array of 80-byte
rules. A rule: relation id, name, source and destination chapter, source and destination list,
rank, fan-out, a display value for the dope sheet, two "active" flags, a render flag, an
increment mode, and a block copied from a custom effect's description. Rules of one id are
consecutive; "rule number n" of an id is the n-th of them. Look-up by id goes through an index
sorted by id, rebuilt after any addition, holding one entry per id (its first rule), with the
last look-up cached in three statics. `SI_reldictDump` prints a rule as
`name(id) chp[src->dst] grp[src->dst] rk() fo() dd() act() actdep()`, which names those fields.

Three of the look-ups read outside the rule array when given an id that is not in the
dictionary (`si_reldictGetRelSub` and `SI_reldictGetRelNbRules` start one rule before it;
`SI_reldictGetMaxFanout` accepts an index equal to the count). The Rust answers "not found"
there and says so.

## Attributes and enums

`SI_AttributeDict` (global `SI_ATD`, 28 bytes): a tree of groups (`__SI_AttribGroup`, 40 bytes:
name, condition, the indices of its attributes, sub-groups, and the two functions that get and
set an attribute of the group on an element) over one flat array of attributes (`SI_Attribute`,
100 bytes). An attribute is addressed by a path (`SI_AttGrpIndex`: a depth, up to seven 16-bit
indices, and a component number): top-level group, sub-groups, then position in the group.
An attribute has a value type, an access mode (bit value 2 = writable, 4 = animatable), a range,
and for each of its components a parameter id and a short name: these ids are the ones the
`Get`/`Set` routines switch on, and the ones animation curves are attached to.

`SI_AttributeValue` is the SDK's `SAA_AttributeValue`: a type word and an 8-byte union. For an
enumerated attribute the two halves of the union hold the enum type's index and the value.

`SI_EnumValDict` (global `SI_EVD`): an array of types, each an array of (number, code, name).

## Models

`__SI_Model` is 0x430 bytes. `SI_modelDump` prints its core with labels, which is where these
names come from: `father`, `brother`, `son`, `name`, `type`, `nbdef`, `curdef`, `latdep`,
`definition`, `data[i]`, and the transformation fields.

- **Hierarchy**: `father` (+0x2bc), `brother` (+0x2c0, next son of the same father), `son`
  (+0x2c4, first son). `SI_modelCreateParentship` appends at the end of the father's list and
  refuses a node that has a father or is an ancestor.
- **Kind** (+0x30): 0 null, 1 patch, 2 face, 4 mesh, 5 joint, 6 spline, 7 metaball system,
  8 metaball, 9 NURBS curve, 10 NURBS surface: the SDK's `SAA_ModelType`.
- **Geometry**: `defs` (+0x38) is an array of `nb_defs` pointers, one per stored shape;
  `definition` (+0x48) is the one in use, `cur_def` its number. The file writer normally saves
  `defs[0]`.
- **Transformation**: local scaling `scal` (+0x100), rotation angles `vrot` (+0x10c), rotation
  matrix `mrot` (+0x118), translation `trans` (+0x158); the same in global terms: `gscl`, `grot`,
  `gtrs`, and the global matrix `gmat` (+0x1c4); and a third set labelled `hscl`, `hrot`, `htrs`
  (+0xa8 to +0xff) whose role beside the other two is not read yet. Vectors are 12 bytes.
- **Appearance**: an array of material pointers (+0x20c, count at +0x206), a linked list of 2D
  textures (+0x21c, linked through the texture's word at +0x16c), one 3D texture (+0x224).
- The remaining third of the record is deformation (lattices, spline and patch fittings,
  clusters, envelope), dynamics, inverse kinematics, per-vertex, per-polygon and per-edge user
  data, render data, and rendering flags (visibility, motion blur, matte, caustics, global
  illumination).

`SI_Vector` is **three floats** (12 bytes): its file reader and writer step by 12 and the matrix
routines index by 12. The SDK's `SAA_Vector` has four (`x, y, z, w`); a conversion happens at the
SDK boundary. (Finding 0001 lists the two as mirrors; they are not the same layout.)

## Geometry

- **Mesh** (`SI_Mesh`, 0x54): three arrays. Vertices are 20 bytes (position, a flag, and a
  small array of vectors). Polygons are 24 bytes (corner array, corner count, material number,
  flag). A corner is 36 bytes (vertex index, normal, u, v, a colour, and a pointer to further
  uv pairs when the model has more than one uv set). Edges are 20 bytes, numbered from 1, kept
  in blocks of 1024 behind a table of block pointers. The mesh also carries the discontinuity
  angle (60 degrees by default) and a flag word (0x804 by default).
- **Spline** (`SI_Spline`, 0x74): name, type, closed, tension, step, and an array of 48-byte
  keys, each three vectors (previous, position, next) and three 16-bit values.
- **Face** (`SI_Face`, 0x88): an embedded spline for the outline and an array of splines for
  holes; or, when the face is made of NURBS curves, an array of 100-byte curves.
- **Patch** (`SI_Patch`, 0x204): type, point count, closed, tension, step and curve count for
  U and V; the points as 16-byte records; six matrices; computed surface arrays.
- **NURBS** curve (`__SI_NurbsCrv`, 100) and surface (`SI_NurbsSrf`, 0xdc): the surface holds
  the same nine-field block twice, once per direction, which a curve holds once (order, point
  count, knot count, knot pointer, range, closed). Control points are 36 bytes: four doubles
  and a word. Doubles sit at offsets that are multiples of 4, not 8, as the i386 ABI allows.

## Graf nodes

`__SI_GrafNode` (0x24) is the node of the doubly linked list of things on screen (`SI_GrafList`,
0x24; the global array `SI_grafLists`, indexed by the second word of `SI_procData`). It carries a kind (2 model root,
3 other model node, 4 camera, 6 light, 5 and 7 the nodes placed in front of those, 0xb wave,
0xe material, 0xf and 0x10 textures), the object pointer, selection state, display type and wire
colour. The element keeps copies of the last three for when there is no graf node; the
attribute routines read the graf node's when there is one.

## How much of each structure is established

Bytes, not fields. A structure embedded in another is counted field by field, and no better
than the embedding itself is established.

| Structure | Original | Size | Fields | Read | Named | Unknown |
|---|---|---|---|---|---|---|
| `Elem` | `__SI_Elem` | 0x84 (132) | 34 | 72.7% | 23.5% | 3.8% |
| `ElemDict` | `SI_ElemDict` | 0x70 (112) | 16 | 26.8% | 35.7% | 37.5% |
| `ListOptions` | `(part of SI_ElemDict)` | 0x18 (24) | 10 | 0.0% | 16.7% | 83.3% |
| `ElemList` | `SI_ElemList` | 0x40 (64) | 17 | 37.5% | 40.6% | 21.9% |
| `DbListNode` | `SI_DbListNode` | 0x34 (52) | 15 | 46.2% | 3.8% | 50.0% |
| `Relations` | `SI_Relations` | 0x8 (8) | 2 | 100.0% | 0.0% | 0.0% |
| `RelList` | `SI_RelList` | 0x18 (24) | 7 | 66.7% | 0.0% | 33.3% |
| `ARelation` | `SI_ARelation` | 0x20 (32) | 10 | 75.0% | 12.5% | 12.5% |
| `RelDict` | `SI_RelDict` | 0x10 (16) | 4 | 75.0% | 0.0% | 25.0% |
| `RelRule` | `(entry of SI_RelDict)` | 0x50 (80) | 25 | 45.0% | 47.5% | 7.5% |
| `SortTbl` | `SI_SortTbl` | 0xc (12) | 3 | 100.0% | 0.0% | 0.0% |
| `UpdList` | `SI_UpdList` | 0xc (12) | 3 | 66.7% | 33.3% | 0.0% |
| `ChapIds` | `SI_ChapIds` | 0x90 (144) | 36 | 100.0% | 0.0% | 0.0% |
| `ChapInfo` | `(entry of SI_chapInfo)` | 0xc (12) | 3 | 66.7% | 33.3% | 0.0% |
| `ElemIdentity` | `SI_ElemIdentity` | 0x10 (16) | 4 | 50.0% | 25.0% | 25.0% |
| `AttributeValue` | `SI_AttributeValue` | 0xc (12) | 3 | 100.0% | 0.0% | 0.0% |
| `AttGrpIndex` | `SI_AttGrpIndex` | 0x14 (20) | 3 | 100.0% | 0.0% | 0.0% |
| `AttributeDict` | `SI_AttributeDict` | 0x1c (28) | 8 | 57.1% | 21.4% | 21.4% |
| `AttribGroup` | `__SI_AttribGroup` | 0x28 (40) | 11 | 80.0% | 10.0% | 10.0% |
| `Attribute` | `SI_Attribute` | 0x64 (100) | 27 | 26.0% | 56.0% | 18.0% |
| `EnumValDict` | `SI_EnumValDict` | 0x8 (8) | 2 | 100.0% | 0.0% | 0.0% |
| `EnumValType` | `SI_EnumValType` | 0xc (12) | 3 | 100.0% | 0.0% | 0.0% |
| `EnumVal` | `(entry of SI_EnumValType)` | 0xc (12) | 3 | 100.0% | 0.0% | 0.0% |
| `Model` | `__SI_Model` | 0x430 (1072) | 128 | 49.1% | 26.3% | 24.6% |
| `Vector` | `SI_Vector` | 0xc (12) | 3 | 100.0% | 0.0% | 0.0% |
| `GrafNode` | `__SI_GrafNode` | 0x24 (36) | 11 | 77.8% | 16.7% | 5.6% |
| `GrafList` | `SI_GrafList` | 0x24 (36) | 8 | 11.1% | 27.8% | 61.1% |
| `Mesh` | `SI_Mesh` | 0x54 (84) | 26 | 35.7% | 50.0% | 14.3% |
| `Vertex` | `(vertex of SI_Mesh)` | 0x14 (20) | 4 | 70.0% | 0.0% | 30.0% |
| `Polygon` | `(polygon of SI_Mesh)` | 0x18 (24) | 6 | 41.7% | 0.0% | 58.3% |
| `PolyNode` | `SI_Node` | 0x24 (36) | 7 | 88.9% | 0.0% | 11.1% |
| `Edge` | `(edge of SI_Mesh)` | 0x14 (20) | 6 | 50.0% | 0.0% | 50.0% |
| `Face` | `SI_Face` | 0x88 (136) | 8 | 26.5% | 7.4% | 66.2% |
| `Spline` | `SI_Spline` | 0x74 (116) | 21 | 20.7% | 5.2% | 74.1% |
| `SplineKey` | `(key of SI_Spline)` | 0x30 (48) | 8 | 0.0% | 87.5% | 12.5% |
| `Patch` | `SI_Patch` | 0x204 (516) | 49 | 9.3% | 35.3% | 55.4% |
| `NurbsDir` | `SI_NurbsDir (inferred)` | 0x24 (36) | 8 | 50.0% | 44.4% | 5.6% |
| `NurbsCrv` | `__SI_NurbsCrv` | 0x64 (100) | 16 | 30.0% | 34.0% | 36.0% |
| `NurbsPoint` | `SI_NurbsPoint` | 0x24 (36) | 5 | 77.8% | 22.2% | 0.0% |
| `NurbsSrf` | `SI_NurbsSrf` | 0xdc (220) | 39 | 40.0% | 29.1% | 30.9% |
| `Material` | `__SI_Material` | 0xdcc (3532) | 23 | 1.8% | 0.3% | 97.9% |
| `LclMap` | `__SI_LclMap` | 0x1d8 (472) | 3 | 0.8% | 0.0% | 99.2% |
| `Texture3D` | `__SI_Texture3D` | 0x158 (344) | 24 | 47.7% | 2.3% | 50.0% |
| `Camera` | `SI_Camera` | 0x13c (316) | 7 | 0.0% | 17.7% | 82.3% |
| `Light` | `light` | 0x240 (576) | 9 | 0.3% | 11.5% | 88.2% |

The three structures this note is mainly about: `__SI_Elem` 72.7% read, 23.5% named, 3.8%
unknown; `SI_ElemDict` 26.8% / 35.7% / 37.5% (the unknown part is the two copies of the
browser's display options); `__SI_Model` 49.1% / 26.3% / 24.6%.

The scene objects at the end of the table (material, 2D texture, camera, light) are a first pass:
their sizes are established, and only the fields met on the way are placed.

## Ported to Rust (`crates/si-types`)

106 functions, each cited at its Rust counterpart. Layout records carry the original's offsets;
where the original owns an array, the Rust keeps a `Vec` beside the record.

| Source file | Functions |
|---|---|
| `elemdict.c` (42 of 125) | `SI_elemdictInit`, `SI_elemdictInitElem`, `SI_elemlistInit`, `SI_elemdictAdd`, `SI_elemdictRemove`, `SI_elemdictGetNb`, `SI_elemdictExists`, `SI_elemdictAddList`, `SI_elemdictFindListByName`, `SI_elemdictLoadChapters`, `SI_elemdictSortChapNames`, `elemdictCompareChapNames`, `SI_elemdictGetChapName`, `SI_elemlistAddElem`, `SI_elemdictAddElem`, `SI_elemlistGetElem`, `SI_elemdictGetElem`, `SI_elemlistCountValidElems`, `SI_elemdictSetDirty`, `SI_elemdictSchemSetPosX`, `SI_elemdictSchemSetPosY`, `si_elemdictSetFlag`, `SI_elemlistClearWorkFlag`, `SI_elemlistClearLowByteWorkFlag`, `SI_elemlistLowByteWorkFlagAND`, `SI_elemdictSetNewFlag`, `SI_elemdictSetSelection`, `SI_elemdictIsElemUsed`, `SI_elemdictFindElemByAuxData`, `SI_elemdictFindElemByDelData`, `SI_elemdictFindElemByDelAuxData`, `SI_elemdictFindElemByCB`, `SI_elemdictUpdateHierInfo`, `elemUpdateHierInfo`, `SI_elemdictCompareElemIdent`, `SI_elemdictGetCurRelationIndex`, `SI_elemdictSetCurRelationIndex`, `SI_dbSortTblSetDirty`, `SI_elemlistDisposeElements`, `SI_elemlistDispose`, `SI_elemdictDispose`, `relBackUpDispose` |
| `relations.c` (3 of 8) | `si_relAllocate`, `si_relCheck`, `si_relAddRelation` |
| `reldict.c` (21 of 88) | `SI_reldictInit`, `SI_reldictAddRelation`, `SI_reldictDispose`, `findElemInRelSortTbl`, `addElemInRelSortTbl`, `SI_reldictGetRelIndexFromId`, `SI_reldictGetRelName`, `si_reldictGetRelSub`, `SI_reldictGetRelNbRules`, `SI_reldictGetRelAttributes`, `SI_reldictGetRelRenderFlag`, `SI_reldictGetRelIncmodeFlag`, `SI_reldictGetRelChapGrp`, `reldictGetRelDopeDisp`, `SI_reldictGetRelRank`, `si_reldictGetRelUpdlattr`, `SI_reldictIsRelActive`, `SI_reldictGetRelIdByName`, `SI_reldictCheckUnCompleteRelation`, `SI_reldictGetMaxFanout`, `SI_reldictGetRelIndexFromName` |
| `attdict.c` (13 of 52) | `si_attdictInitAttribute`, `attdictInitAttribGroup`, `SI_attdictInit`, `si_attdictAllocateFcurveArray`, `si_attdictAddAttribute`, `si_attdictAddAttribGroup`, `si_attdictAddAttribSubGroup`, `attdictAttribGroupDispose`, `SI_attdictDispose`, `SI_attdictAttributeGetByIndex`, `SI_attdictCompareGrpIndex`, `SI_attdictAttributeIsAnimatable`, `SI_attdictAttribGetDefWidth` |
| `enmdict.c` (9 of 10) | all but `SI_enmdictDump` |
| `model_b.c` (2 of 4) | `SI_modelAllocate`, `SI_modelCreateParentship` |
| `mesh_b.c`, `obj_global.c` | `SI_meshAllocate`, `SI_vertexAllocate`, `SI_polygonAllocate`, `SI_polygonNodeAllocate`, `SI_edgeAllocate` |
| `spline.c`, `face.c`, `patch.c`, `nrb_b.c` | `SI_splineInit`, `SI_splineAllocate`, `SI_faceAllocate`, `SI_patchAllocate`, `SI_nurbsCrvAllocate`, `SI_nurbsSrfAllocate` |
| `graflist.c` | `grafnodeAllocate`, `graflistInsertGrafnode` |
| `dbexternal.c`, `updlist.c` | `SI_chapInfoOffsetFromChapId`, `SI_edGetSuffixFromChapter` (needed by the chapter loader), `SI_updlistInit` |

Where the Rust knowingly differs, the code says so at the spot: fields the original leaves
uninitialised are zero; the three out-of-bounds reads of the relation look-ups answer "not
found"; `SI_elemdictAddElem` reports that the model relations must be saved instead of calling
the two routines of `edtools.c` that do it; a cycle of hierarchy relations, which makes the
original recurse without end, stops.

Check: `cargo test -p si-types`. The offsets of all 668 fields and the sizes of all 45 records
are asserted at compile time; 33 unit tests (made-up data) cover the ported functions.

## Not determined

- **Elements**: what the 16-bit value at +0x28 means; the third sequence field (+0x7c) and what
  braces instead of brackets mean in a sequence's frame range; five fields of the name record
  (`SI_DbListNode` +0, +4, +8, +0xc, +0x30).
- **Dictionary**: the ten fields of the browser's display options (two copies, 48 bytes); the
  16-bit value at +0x6c.
- **Relations**: `SI_RelList` +0x10 and +0x14; `SI_ARelation` +0x14 and +0x16; `SI_RelDict` +4;
  the meaning of the two update-attribute values and of the custom-effect block of a rule.
- **Sizes not established**: `SI_ElemIdentity` (no allocation of it was found; 16 bytes are the
  fields seen), the element's icon record, the attribute condition record.
- **Model**: 24.6% of the bytes. The largest pieces: two matrices at +0x238 and +0x278 that are
  set to identity at creation and that no function reaches through a typed pointer; the role of
  the `h…` transformation set; the bit meanings of the three flag words (+0x08, +0x0a, +0x3fa);
  and some twenty single words listed in the table.
- **Geometry**: the vectors hanging off a mesh vertex and the three words at the start of a
  polygon; two words of an edge (probably its two polygons: not read); the spline's matrix; four
  of the patch's six matrices; the tail of the NURBS records.
- **Materials, 2D textures, cameras, lights**: almost everything; they belong to later packages.
- What the SDK's `SAA_Elem.groupid` maps to.

## Close-out of the 2026-10-24 round (written 2026-10-28)

`crates/si-dict` gained `dictin.rs` (`SI_elemdictRestoreActions`), `hieranim.rs` (`SI_hieranimDispose`, `GetName`,
`GetType`) and `reldictops.rs` (the `SI_reldictGet*` look-ups and five `lookForExisting*`/`searchAttdictByFcvName`
search routines); `si-scene`'s loader now calls the first one on the image and records its three calls into the
actions code (`SI_actionsTmpActivateAll`, `SI_actionsAddMasterKeysToHA`, `SI_actionsInitPathAnim`) through its outside
world. Check (`dict-emucheck`, emulator, specs from `re/tools/si-dict/gen_rdops.py`, output
`re/out/si-dict/check_rdops.txt`): 29 routines, 12,400 cases: 11,151 identical, 365 identical in every word the
original defines (the rest of the answer is uninitialised memory there), 0 different, 884 not compared (the original
stopped). Mutation switch (`mutate_rdops.txt`): 383 identical, 10,768 different, so the check does see changes.
`SI_reldictCusOpDump` (0x0813eae0), an empty function, is not in the check and is written in the uncited form.

## Not ported, and why

- `SI_elemdictSetDeleteElem` and the removal of relations (`relations.c`): they call the
  disposal of relation data and of name records, and an editing routine of `edtools.c`.
- `SI_elemdictAddCompleteRelation` and the three other routines that add a relation to both
  ends: read, but each calls `checkNSEStatus` under the product-tier flag `SI_lightLicense`,
  which was left alone.
- The fifteen `SI_elemdictFind…RelatedElem…` routines and the look-ups by name: read in part.
- `SI_elemdictFindElemByData` with its sorted table and three-static cache; the 15-key element
  sort (`elemdictCompareElems`).
- The reader of `Relations.rsrc` (about 45 functions of `reldict.c`) and the table that attaches
  the get/set functions to attribute groups (`SI_attdictSetFctsPointers`).
- `init_dict.c`: read; `SI_initDictionaries` needs the resource reader and the database layer.
- `model.c`: nothing yet. `SI_modelDisposeNode` and `SI_modelCopyNode` were read for the layout,
  but each calls twenty or more routines of other files (materials, textures, lattices,
  dynamics, envelopes, clusters); they can be ported once those exist.

## Field tables

Printed by `cargo run --release -p si-types --bin si-types-table`. "attribute N" in the evidence
is parameter id N of `Relations.rsrc`; "token" is a field of the structure's record in a model
file; "the dump" is the structure's own dump routine.

### `Elem` — `__SI_Elem`, 0x84 bytes

Size: SI_elemlistAddElem allocates 0x84 bytes for the first element and grows the array by 0x84; every element loop steps by 0x84. Read 72.7% · named 23.5% · unknown 3.8%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `Ptr32<DbListNode>` | `node` | read | Name record. SI_elemlistAddElem stores its SI_DbListNode argument here; SI_elemdictSetDeleteElem frees it with SI_dbListNodeFree. |
| 0x004 | 4 | `Ptr32<Opaque>` | `data` | read | The object the element stands for (__SI_Model, SI_Camera, light, __SI_Material, __SI_LclMap, __SI_Texture3D, fcurve, ... by chapter). SI_elemdictFindElemByData compares it; 46 functions read it as a model. |
| 0x008 | 4 | `Ptr32<Opaque>` | `aux_data` | read | Second object: for scene objects the __SI_GrafNode (grafnodeElemAssoc stores it, SI_elemdictFindElemByAuxData compares it). |
| 0x00c | 4 | `f32` | `schem_x` | read | Position in the schematic view: SI_elemdictSchemSetPosX; attribute 269 (SchPos); first value of the SCHEM line. |
| 0x010 | 4 | `f32` | `schem_y` | read | SI_elemdictSchemSetPosY; attribute 270; second value of the SCHEM line. |
| 0x014 | 4 | `f32` | `schem_width` | named | si_schemSetElemWidth returns it as the stored width when the element is not being laid out. |
| 0x018 | 4 | `i32` | `schem_collapse` | read | Attribute 271 (SchCollapse) in SI_schemAttrGetValue; third value of the SCHEM line. |
| 0x01c | 4 | `i32` | `wire_color` | read | Attribute 267 (WireColor): SI_generalAttrSetValue stores SI_color1 + value + SI_select7WM; written as WIRECOL; its low 16 bits are copied to the graf node. Set to SI_grafnodeDefColor() at creation. |
| 0x020 | 4 | `i32` | `wire_type` | read | Attribute 268 (WireType: 0 hidden, 1 ghosted, 2 visible); 2 at creation; second value of the DISPLAY line; copied to the graf node. |
| 0x024 | 4 | `i32` | `selection` | read | First value of the DISPLAY line; grafnodeElemAssoc copies it to the graf node's selection state (SelType: 0 none, 1 node, 2 inherited, 3 branch). |
| 0x028 | 2 | `i16` | `dope_state` | unknown | Read by dopeRelUpdate, which passes on the values 3 and 4; meaning not established. |
| 0x02a | 1 | `u8` | `schem_flags` | named | Bit 0 is written as the word USR on the SCHEM line; bit 1 is tested by si_schemSetElemWidth. Cleared as 16 bits at creation. |
| 0x02b | 1 | `u8` | `pad_2b` | unknown | Cleared with schem_flags; no other access found. |
| 0x02c | 16 | `[f32; 4]` | `schem_bbox` | named | Four floats written by SI_schemComputeElemBBox; the element sort uses [2]-[0] and [3]-[1] as extents. |
| 0x03c | 4 | `Ptr32<f32>` | `root_srt` | read | Nine floats printed as the SRT line by dbWriteRootModelSRT, which then frees them. |
| 0x040 | 2 | `i16` | `dirty` | read | SI_elemdictSetDirty; 1 at creation. |
| 0x042 | 2 | `i16` | `selected` | named | SI_elemdictSetSelection sets it in every element; returned as attribute 265 (Selection) when the element has no graf node. The topological sort also uses it as a mark. |
| 0x044 | 2 | `i16` | `shared` | named | Written by dialogSetShare; SI_generalAttrSetValue refuses to rename an element when it is non-zero. |
| 0x046 | 2 | `i16` | `new_flag` | read | SI_elemdictSetNewFlag rewrites it; 2 at creation, 1 after a copy between dictionaries or a load, 0 when deleted. |
| 0x048 | 2 | `i16` | `status` | read | 0 = in use, 2 = deleted (the slot may be reused by SI_elemlistAddElem). Every look-up tests it. |
| 0x04a | 2 | `i16` | `subst` | read | Substitution number: SI_dbfilenameChangePrecision counts it up or down and prints it into the name record. |
| 0x04c | 4 | `i32` | `dsc_id` | read | Sixth argument of SI_elemdictAddElem: the scene description the element was read for; elemdictGetElemFromWorkId matches it together with work_id. |
| 0x050 | 4 | `i32` | `root_id` | read | Index of the root of the element's hierarchy (its own index at creation): SI_elemdictUpdateHierInfo. SI_elemdictSetDeleteElem stores the element's own index. |
| 0x054 | 4 | `i32` | `hier_level` | read | Depth in the hierarchy, 0 for a root: SI_elemdictUpdateHierInfo; attribute 266 (HierLevel). |
| 0x058 | 4 | `i32` | `work_flag` | read | si_elemdictSetFlag, SI_elemlistClearWorkFlag. The 'low byte' routines address the byte at +0x5b, the low byte on the big-endian machines the code was written for and the high byte here. |
| 0x05c | 4 | `i32` | `work_id` | read | Seventh argument of SI_elemdictAddElem: the element's number in the description file being read; the writer stores the running count here. |
| 0x060 | 8 | `Relations` | `forward` | read | Relations of which this element is the source (SI_elemdictAddCompleteRelation adds the entry carrying the data here). |
| 0x068 | 8 | `Relations` | `backward` | read | Relations of which this element is the destination. |
| 0x070 | 4 | `Ptr32<ElemIcon>` | `icon` | read | Freed by SI_dbIconFree. |
| 0x074 | 4 | `i32` | `seq_first` | read | First frame of a picture sequence: si_dbFormatSequenceFrames prints [first - last]. Not set by SI_elemlistAddElem. |
| 0x078 | 4 | `i32` | `seq_last` | read | Last frame of the sequence. |
| 0x07c | 2 | `i16` | `seq_unknown` | unknown | Read by SI_dbListPrepareIcons. |
| 0x07e | 2 | `i16` | `seq_braces` | named | Non-zero makes si_dbFormatSequenceFrames print braces instead of brackets; what the two forms distinguish is not read. |
| 0x080 | 4 | `CStr32` | `suffix` | named | A string freed with the element; the element sort (key 12) uses it in place of the chapter's file suffix. |

### `ElemDict` — `SI_ElemDict`, 0x70 bytes

Size: SI_elemdictAdd grows the global array by 0x70; the symbol SI_dbed, one dictionary, is 0x70 bytes. Read 26.8% · named 35.7% · unknown 37.5%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `nb_lists` | read | Number of chapters. |
| 0x004 | 4 | `Ptr32<ElemList>` | `lists` | read | Array of chapters. |
| 0x008 | 4 | `Ptr32<i32>` | `chap_sort` | read | Chapter indices ordered by name (SI_elemdictSortChapNames). |
| 0x00c | 12 | `UpdList` | `upd_list` | named | Disposed with SI_updlistDispose; its third word is set to 1 by every routine that adds or removes an element or a relation. |
| 0x018 | 24 | `ListOptions` | `options` | named | Browser options; the copy written when the dictionary is a scene. |
| 0x030 | 24 | `ListOptions` | `options_db` | named | The copy written when the dictionary is the database's. |
| 0x048 | 4 | `Ptr32<DbInfo>` | `dbinfo` | named | SI_dbLoadElementsFromDict stores a SI_DbInfo pointer. |
| 0x04c | 12 | `UpdList` | `topo_list` | named | Used by the SI_elemdictTopoSort routines; disposed with SI_updlistDispose. Its third word is set like upd_list's. |
| 0x058 | 2 | `i16` | `is_scene` | read | Third argument of SI_elemdictLoadChapters: 1 for a scene dictionary, 0 for the database dictionary SI_dbed. |
| 0x05a | 2 | `i16` | `rel_dirty` | named | Set to 1 whenever a relation is added or removed. |
| 0x05c | 4 | `i32` | `nb_fwd_backup` | read | Length of fwd_backup (SI_edAllocMdlRelBackUp, relBackUpDispose). |
| 0x060 | 4 | `Ptr32<Relations>` | `fwd_backup` | read | One Relations per model element: a saved copy of forward relations. |
| 0x064 | 4 | `i32` | `nb_back_backup` | read | Length of back_backup. |
| 0x068 | 4 | `Ptr32<Relations>` | `back_backup` | read | Saved copy of backward relations. |
| 0x06c | 2 | `i16` | `untold` | unknown | 0 at creation; written by SI_edSwapUnToldRel and dbCheckExtraUnToldRelation. |
| 0x06e | 2 | `i16` | `hier_dirty` | named | 1 at creation and after every change; SI_elemdictAddElem saves model relations only while it is 0. |

### `ListOptions` — `(part of SI_ElemDict)`, 0x18 bytes

Size: SI_elemdictInit copies six words from +0x18 to +0x30. Read 0.0% · named 16.7% · unknown 83.3%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `sort_type` | named | 6 at creation; SI_elemdictLoadChapters sets 9, the value the element sort treats as 'by full name'. Written by dialogListOptions. |
| 0x004 | 2 | `i16` | `unk_04` | unknown | 3 at creation; dialogListOptions, dbListFormatItemString. |
| 0x006 | 2 | `i16` | `unk_06` | unknown | 0 at creation; dbListCollapseAll writes it. |
| 0x008 | 2 | `i16` | `unk_08` | unknown | 1 at creation; SI_dialogDbExchange. |
| 0x00a | 2 | `i16` | `unk_0a` | unknown | 0 at creation, 1 after SI_elemdictLoadChapters; the list builders read it. |
| 0x00c | 4 | `i32` | `unk_0c` | unknown | -1 at creation; SI_dialogDatabases writes it. |
| 0x010 | 2 | `i16` | `unk_10` | unknown | 0 at creation. |
| 0x012 | 2 | `i16` | `unk_12` | unknown | 0 at creation. |
| 0x014 | 2 | `i16` | `unk_14` | unknown | 1 at creation; SI_dbMFilesDelete and others read it. |
| 0x016 | 2 | `[u8; 2]` | `pad_16` | unknown | No access found. |

### `ElemList` — `SI_ElemList`, 0x40 bytes

Size: SI_elemdictAddList allocates 0x40 bytes for the first chapter and grows the array by 0x40. Read 37.5% · named 40.6% · unknown 21.9%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `CStr32` | `name` | read | Chapter name: stored by SI_elemdictAddList, compared by SI_elemdictFindListByName. |
| 0x004 | 4 | `i32` | `nb_elems` | read | Number of slots in elems, deleted ones included. |
| 0x008 | 4 | `Ptr32<Elem>` | `elems` | read | Array of nb_elems elements. |
| 0x00c | 4 | `Ptr32<i32>` | `sort` | read | Element indices in sorted order, built by SI_elemlistSortChapterElements. |
| 0x010 | 4 | `Ptr32<i32>` | `hier_order` | named | Built by SI_elemdictSortAnimElements; the hierarchical sort reads element indices through it. |
| 0x014 | 4 | `Ptr32<i32>` | `hier_count` | named | Built with hier_order; the hierarchical sort reads a run length per root from it. |
| 0x018 | 2 | `i16` | `kind` | named | 1 at creation; SI_elemdictLoadChapters sets 0 for NSELECTIONS and CHANNEL_DRIVERS (chapters with no files) and 2 for OUTSIDE_DB_FILES. |
| 0x01a | 2 | `[u8; 2]` | `pad_1a` | unknown | No access found. |
| 0x01c | 4 | `i32` | `unk_1c` | unknown | Written by txtlistTransferSelection2ElemList. |
| 0x020 | 4 | `i32` | `unk_20` | unknown | Cleared at creation only. |
| 0x024 | 4 | `i32` | `unk_24` | unknown | Read by dbGetIconData. |
| 0x028 | 4 | `CStr32` | `subdir` | named | Current sub-directory: dbSetSubDirectory; freed with the chapter. |
| 0x02c | 4 | `Ptr32<CStr32>` | `subdirs` | named | nb_subdirs strings filled by SI_dbChapterGetSubDirs. |
| 0x030 | 4 | `i32` | `nb_subdirs` | named | Length of subdirs and subdir_aux. |
| 0x034 | 4 | `Ptr32<Ptr32<Opaque>>` | `subdir_aux` | named | Parallel to subdirs, each entry freed with the chapter. |
| 0x038 | 4 | `Ptr32<SortTbl>` | `sort_tbl` | read | Only when SI_LOAD_OPTIM is set: a table for finding an element by its data pointer. |
| 0x03c | 4 | `CStr32` | `suffix` | read | The chapter's file suffix, copied by SI_elemdictLoadChapters from the chapter table. |

### `DbListNode` — `SI_DbListNode`, 0x34 bytes

Size: SI_dbListNodeAllocate allocates 0x34 bytes. Read 46.2% · named 3.8% · unknown 50.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `unk_00` | unknown | 0 at creation. |
| 0x004 | 4 | `Ptr32<Opaque>` | `unk_04` | unknown | 0 at creation; freed by SI_dbListNodeFree. |
| 0x008 | 4 | `CStr32` | `str_08` | unknown | An empty string at creation; sort key 1 compares it. |
| 0x00c | 4 | `CStr32` | `str_0c` | unknown | Second argument of SI_dbScnGetListNode when not empty; sort key 2 compares it. |
| 0x010 | 4 | `CStr32` | `prefix` | read | Attribute 261 (Prefix). |
| 0x014 | 4 | `CStr32` | `name` | read | Attribute 262 (Name). |
| 0x018 | 4 | `CStr32` | `version` | read | Attribute 263 (Revision), parsed with strtol; the string 1 at creation. |
| 0x01c | 4 | `CStr32` | `subst` | read | Attribute 264 (Subst), parsed with strtol; the string 0 at creation. |
| 0x020 | 4 | `i32` | `time` | read | time(NULL) at creation; sort key 6. |
| 0x024 | 4 | `CStr32` | `fullname` | read | Result of SI_dbFormatFullName; sort key 9. |
| 0x028 | 2 | `i16` | `unk_28` | unknown | 0 at creation. |
| 0x02a | 2 | `i16` | `unk_2a` | unknown | 0 at creation. |
| 0x02c | 2 | `u16` | `mode` | named | Sort key 11 tests (mode & 0xf000) == 0x4000, the directory test of a file mode. |
| 0x02e | 2 | `[u8; 2]` | `pad_2e` | unknown | No access found. |
| 0x030 | 4 | `i32` | `unk_30` | unknown | Sort key 13 compares it as an integer. Not set at creation. |

### `Relations` — `SI_Relations`, 0x8 bytes

Size: embedded twice in __SI_Elem, 8 bytes apart; arrays of them step by 8 in relBackUpDispose. Read 100.0% · named 0.0% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `nb_groups` | read | Number of lists; si_relAllocate sets 6. |
| 0x004 | 4 | `Ptr32<RelList>` | `groups` | read | Array of nb_groups lists (si_relAllocate allocates 0x90 bytes). |

### `RelList` — `SI_RelList`, 0x18 bytes

Size: si_relAllocate initialises six of them in 0x90 bytes, stepping by 0x18. Read 66.7% · named 0.0% · unknown 33.3%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `nb` | read | Number of relations. |
| 0x004 | 4 | `i32` | `capacity` | read | si_relAddRelation grows the array by one entry when nb equals it. |
| 0x008 | 4 | `i32` | `cur` | read | A cursor: SI_elemdictGetCurRelationIndex and SI_elemdictSetCurRelationIndex. |
| 0x00c | 4 | `Ptr32<ARelation>` | `rels` | read | Array of relations, kept ordered by rank. |
| 0x010 | 4 | `i32` | `unk_10` | unknown | Cleared with the list; no other access found. |
| 0x014 | 2 | `i16` | `flag` | unknown | 1 whenever the list is reset. |
| 0x016 | 2 | `[u8; 2]` | `pad_16` | unknown | No access found. |

### `ARelation` — `SI_ARelation`, 0x20 bytes

Size: si_relAddRelation allocates 0x20 bytes for the first and grows by 0x20. Read 75.0% · named 12.5% · unknown 12.5%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `relid` | read | Relation id, a key of the relation dictionary. |
| 0x004 | 4 | `i32` | `elemid` | read | Index of the element at the other end. |
| 0x008 | 4 | `i32` | `active_elem` | read | -1, or the index of an element of the ANIMATION chapter that belongs to the relation and is deleted with it. |
| 0x00c | 4 | `i32` | `serial` | named | Tenth argument of si_relAddRelation, -1 from the plain entry points; the 'serial relation' routines match on it. |
| 0x010 | 2 | `i16` | `chapid` | read | Chapter of the element at the other end. |
| 0x012 | 2 | `i16` | `group` | read | Index of the list, on the other element, that holds the matching entry. |
| 0x014 | 2 | `i16` | `flag` | unknown | Last argument of SI_elemdictAddCompleteRelation (1 from SI_elemdictAddRelation). |
| 0x016 | 2 | `i16` | `unk_16` | unknown | 0 at creation; tested by si_relAddRelation when a serial id is given. |
| 0x018 | 4 | `Ptr32<Opaque>` | `data` | read | Relation data, on the source side only; disposed by SI_elemdictDisposeRelationData. |
| 0x01c | 4 | `i32` | `rank` | read | Rank of the relation's rule; si_relAddRelation inserts by ascending rank. |

### `RelDict` — `SI_RelDict`, 0x10 bytes

Size: the symbol SI_RD is 0x10 bytes; SI_reldictInit sets four words. Read 75.0% · named 0.0% · unknown 25.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `nb` | read | Number of rules. |
| 0x004 | 4 | `i32` | `unk_04` | unknown | -1 at creation; no other access found. |
| 0x008 | 4 | `Ptr32<RelRule>` | `rules` | read | Array of rules; the rules of one relation id are consecutive. |
| 0x00c | 4 | `Ptr32<SortTbl>` | `sort` | read | Rule indices ordered by relation id, built on first look-up. |

### `RelRule` — `(entry of SI_RelDict)`, 0x50 bytes

Size: SI_reldictAddRelation allocates 0x50 bytes for the first and grows by 0x50. Read 45.0% · named 47.5% · unknown 7.5%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `relid` | read | Relation id. |
| 0x004 | 4 | `CStr32` | `name` | read | Relation name (SI_reldictGetRelName). |
| 0x008 | 2 | `i16` | `src_chap` | read | Chapter of the source; printed as chp[a->b] by SI_reldictDump. -10 marks an incomplete relation. |
| 0x00a | 2 | `i16` | `dst_chap` | read | Chapter of the destination. |
| 0x00c | 2 | `i16` | `src_group` | read | List index on the source; printed as grp[a->b]. |
| 0x00e | 2 | `i16` | `dst_group` | read | List index on the destination. |
| 0x010 | 2 | `i16` | `rank` | read | Printed as rk(); SI_reldictGetRelRank. |
| 0x012 | 2 | `i16` | `fanout` | read | Printed as fo(). |
| 0x014 | 2 | `i16` | `dope_disp` | read | Printed as dd(); reldictGetRelDopeDisp. |
| 0x016 | 2 | `i16` | `upd_attr0` | named | First value of si_reldictGetRelUpdlattr. |
| 0x018 | 2 | `i16` | `upd_attr1` | named | Second value of si_reldictGetRelUpdlattr. |
| 0x01a | 2 | `i16` | `active` | read | Printed as act(); SI_reldictIsRelActive(…, 0); 1 at creation. |
| 0x01c | 2 | `i16` | `active_dep` | read | Printed as actdep(); SI_reldictIsRelActive(…, 1); 1 at creation. |
| 0x01e | 2 | `[u8; 2]` | `pad_1e` | unknown | No access found. |
| 0x020 | 4 | `CStr32` | `cus_name` | named | First word copied from the SI_cusRelInfo argument (custom effects), null without one; SI_reldictGetRelIndexFromName compares it as a string. |
| 0x024 | 24 | `[i32; 6]` | `cus_info` | named | The next six words of SI_cusRelInfo; zero without one. |
| 0x03c | 2 | `i16` | `cus_3c` | named | From SI_cusRelInfo. |
| 0x03e | 2 | `i16` | `max_fanout_dst` | read | SI_reldictGetMaxFanout(…, non-zero); 1 without custom info. |
| 0x040 | 2 | `i16` | `max_fanout_src` | read | SI_reldictGetMaxFanout(…, 0); 1 without custom info. |
| 0x042 | 2 | `i16` | `cus_42` | named | From SI_cusRelInfo. |
| 0x044 | 2 | `i16` | `cus_44` | named | From SI_cusRelInfo. |
| 0x046 | 2 | `[u8; 2]` | `pad_46` | unknown | No access found. |
| 0x048 | 2 | `i16` | `render` | read | SI_reldictGetRelRenderFlag. |
| 0x04a | 2 | `[u8; 2]` | `pad_4a` | unknown | No access found. |
| 0x04c | 4 | `i32` | `incmode` | read | SI_reldictGetRelIncmodeFlag. |

### `SortTbl` — `SI_SortTbl`, 0xc bytes

Size: allocated with SI_calloc(1, 0xc) in SI_reldictAddRelation and SI_elemlistAddElem. Read 100.0% · named 0.0% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `count` | read | Number of entries in table. |
| 0x004 | 4 | `Ptr32<i32>` | `table` | read | Indices into the array, ordered by key; a new slot holds -1. |
| 0x008 | 4 | `i32` | `valid` | read | 0 when the table must be rebuilt (every addition clears it). |

### `UpdList` — `SI_UpdList`, 0xc bytes

Size: two of them lie 12 bytes before their neighbours in SI_ElemDict; SI_elemdictInit sets three words in each. Read 66.7% · named 33.3% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `nb` | read | Number of nodes (SI_updlistDispose loops over it). |
| 0x004 | 4 | `Ptr32<UpdElem>` | `nodes` | read | Array of 0x2c-byte nodes. |
| 0x008 | 4 | `i32` | `dirty` | named | 1 at creation and after every change to the dictionary. |

### `ChapIds` — `SI_ChapIds`, 0x90 bytes

Size: the symbols SI_cid and SI_dbcid are 0x90 bytes; SI_elemdictLoadChapters writes words 0 to 35. Read 100.0% · named 0.0% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `ani` | read | ANIMATION. |
| 0x004 | 4 | `i32` | `cam` | read | CAMERAS. |
| 0x008 | 4 | `i32` | `cls` | read | CLUSTERS. |
| 0x00c | 4 | `i32` | `dsc` | read | SCENES. |
| 0x010 | 4 | `i32` | `img` | read | PICTURES. |
| 0x014 | 4 | `i32` | `lat` | read | LATTICES. |
| 0x018 | 4 | `i32` | `lig` | read | LIGHTS. |
| 0x01c | 4 | `i32` | `mat` | read | MATERIALS. |
| 0x020 | 4 | `i32` | `model` | read | MODELS. |
| 0x024 | 4 | `i32` | `nsl` | read | NSELECTIONS. |
| 0x028 | 4 | `i32` | `pal` | read | PALETTES. |
| 0x02c | 4 | `i32` | `rim` | read | RENDER_PICTURES. |
| 0x030 | 4 | `i32` | `shd_mat` | read | MATERIAL_SHADERS. |
| 0x034 | 4 | `i32` | `shd_t2d` | read | TEXTURE2D_SHADERS. |
| 0x038 | 4 | `i32` | `shd_t3d` | read | TEXTURE3D_SHADERS. |
| 0x03c | 4 | `i32` | `shd_vol` | read | VOLUME_SHADERS. |
| 0x040 | 4 | `i32` | `shd_sha` | read | SHADOW_SHADERS. |
| 0x044 | 4 | `i32` | `shd_lig` | read | LIGHT_SHADERS. |
| 0x048 | 4 | `i32` | `shd_cam` | read | CAMERA_SHADERS. |
| 0x04c | 4 | `i32` | `shd_out` | read | OUTPUT_SHADERS. |
| 0x050 | 4 | `i32` | `shd_env` | read | ENVIRON_SHADERS. |
| 0x054 | 4 | `i32` | `shp` | read | SHAPES. |
| 0x058 | 4 | `i32` | `sts` | read | SETUP_SOFT. |
| 0x05c | 4 | `i32` | `stc` | read | SETUP_CHANNELS. |
| 0x060 | 4 | `i32` | `sfk` | read | SETUP_FLOCK. |
| 0x064 | 4 | `i32` | `sky` | read | STRUCT_KEYS. |
| 0x068 | 4 | `i32` | `t2d` | read | TEXTURES2D. |
| 0x06c | 4 | `i32` | `t3d` | read | TEXTURES3D. |
| 0x070 | 4 | `i32` | `wav` | read | WAVES. |
| 0x074 | 4 | `i32` | `exp` | read | EXPRESSIONS. |
| 0x078 | 4 | `i32` | `chn_drv` | read | CHANNEL_DRIVERS. |
| 0x07c | 4 | `i32` | `out` | read | OUTSIDE_DB_FILES. |
| 0x080 | 4 | `i32` | `spd` | read | SPREADSHEET_SETUP. |
| 0x084 | 4 | `i32` | `audio` | read | AUDIO. |
| 0x088 | 4 | `i32` | `shd_mat_photon` | read | MATERIAL_PHOTON_SHADERS. |
| 0x08c | 4 | `i32` | `shd_lig_photon` | read | LIGHT_PHOTON_SHADERS. |

### `ChapInfo` — `(entry of SI_chapInfo)`, 0xc bytes

Size: 432 bytes for 36 chapters; SI_edGetSuffixFromChapter indexes it in steps of three words. Read 66.7% · named 33.3% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `CStr32` | `name` | read | Chapter name, also the name of its directory in a database. |
| 0x004 | 4 | `CStr32` | `suffix` | read | File suffix (SI_edGetSuffixFromChapter). |
| 0x008 | 4 | `u32` | `tag` | named | Four characters, or 0: the tag that starts the chapter's ASCII files (SDSC, SCAM, …) as a big-endian word. |

### `ElemIdentity` — `SI_ElemIdentity`, 0x10 bytes

Size: not established: no allocation with this type was found; 16 bytes are the fields seen through parameters of the type. Read 50.0% · named 25.0% · unknown 25.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `chapid` | read | Chapter index; every attribute routine passes it to SI_elemdictGetElem. |
| 0x004 | 4 | `i32` | `elemid` | read | Element index. |
| 0x008 | 4 | `i32` | `sub_id` | named | Read by the control-vertex, fcurve-key and action attribute routines: an index inside the element. |
| 0x00c | 4 | `i32` | `sub_id2` | unknown | Copied by the spreadsheet routines; meaning not read. |

### `AttributeValue` — `SI_AttributeValue`, 0xc bytes

Size: the SDK structure under the i386 ABI: a type and an 8-byte union; the attribute routines write +0, +4 and +8. Read 100.0% · named 0.0% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `value_type` | read | Copied from the attribute's value type. |
| 0x004 | 4 | `u32` | `value` | read | The value (boolean, char, short, int, float bits or string pointer); for an enumerated attribute, the enum type index. |
| 0x008 | 4 | `u32` | `value2` | read | For an enumerated attribute, the value; with value, the two halves of a double. |

### `AttGrpIndex` — `SI_AttGrpIndex`, 0x14 bytes

Size: the fields seen: a short count, shorts up to +0x10, and a word at +0x10; an attribute embeds one in 0x14 bytes. Read 100.0% · named 0.0% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 2 | `i16` | `depth` | read | Number of path entries (SI_attdictAttributeGetByIndex). |
| 0x002 | 14 | `[i16; 7]` | `path` | read | path[0] is a group of the dictionary, the next ones sub-groups, the last (path[depth-1]) an attribute of the group reached. Seven entries is what fits before value_index. |
| 0x010 | 4 | `i32` | `value_index` | read | Which component of the attribute; checked against its number of values. |

### `AttributeDict` — `SI_AttributeDict`, 0x1c bytes

Size: the symbol SI_ATD is 0x1c bytes. Read 57.1% · named 21.4% · unknown 21.4%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `nb_groups` | read | Number of top-level groups. |
| 0x004 | 4 | `Ptr32<AttribGroup>` | `groups` | read | Array of groups. |
| 0x008 | 4 | `Ptr32<Attribute>` | `attributes` | read | All attributes, in one array. |
| 0x00c | 4 | `i32` | `nb_attributes` | read | Length of attributes. |
| 0x010 | 4 | `Ptr32<Opaque>` | `fcv_index` | named | Built by SI_attdictSortAttribFromFcurveIds; freed with the dictionary. |
| 0x014 | 4 | `i32` | `unk_14` | unknown | 0 at creation. |
| 0x018 | 2 | `i16` | `dirty` | named | 1 at creation and after every addition. |
| 0x01a | 2 | `[u8; 2]` | `pad_1a` | unknown | No access found. |

### `AttribGroup` — `__SI_AttribGroup`, 0x28 bytes

Size: si_attdictAddAttribGroup allocates 0x28 bytes for the first and grows by 0x28. Read 80.0% · named 10.0% · unknown 10.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `CStr32` | `name` | read | Group name. |
| 0x004 | 2 | `i16` | `flag` | unknown | 1 at creation, 0 for the first group of the dictionary. |
| 0x006 | 2 | `[u8; 2]` | `pad_06` | unknown | No access found. |
| 0x008 | 4 | `i32` | `rel_type` | named | Fourth argument of si_attdictAddAttribGroup, declared SI_RelType. |
| 0x00c | 4 | `i32` | `nb_attribs` | read | Number of attributes of the group. |
| 0x010 | 4 | `Ptr32<i32>` | `attribs` | read | Their indices in the dictionary's attribute array. |
| 0x014 | 4 | `Ptr32<AttribCond>` | `cond` | read | Condition under which the group applies (the COND(...) of the resource file). |
| 0x018 | 4 | `Ptr32<Code>` | `get_value` | read | Function that reads an attribute of this group; SI_dummyAttrGetValue at creation. |
| 0x01c | 4 | `Ptr32<Code>` | `set_value` | read | Function that writes one; SI_dummyAttrSetValue at creation. |
| 0x020 | 4 | `i32` | `nb_subs` | read | Number of sub-groups. |
| 0x024 | 4 | `Ptr32<AttribGroup>` | `subs` | read | Array of sub-groups. |

### `Attribute` — `SI_Attribute`, 0x64 bytes

Size: si_attdictAddAttribute allocates 100 bytes for the first and grows by 100. Read 26.0% · named 56.0% · unknown 18.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `unk_00` | unknown | 0 at creation. |
| 0x004 | 2 | `i16` | `unk_04` | unknown | 0 at creation. |
| 0x006 | 2 | `[u8; 2]` | `pad_06` | unknown | No access found. |
| 0x008 | 20 | `AttGrpIndex` | `index` | named | The attribute's own path: the fields are cleared in the pattern of SI_AttGrpIndex, and SI_attdictUpdateIndexes fills them. |
| 0x01c | 4 | `i32` | `id` | read | The id column of the resource line; 20000 when the line gives xx. |
| 0x020 | 4 | `CStr32` | `name` | read | Attribute name. |
| 0x024 | 4 | `CStr32` | `description` | named | The quoted text ending the resource line. |
| 0x028 | 2 | `i16` | `access` | read | From the mode column: bit 1 (value 2) set = writable (the Set routines refuse otherwise); value 4 = animatable (SI_attdictAttributeIsAnimatable). |
| 0x02a | 2 | `i16` | `scope` | named | From the scope column. |
| 0x02c | 4 | `i32` | `value_type` | read | Copied into every SI_AttributeValue produced for the attribute; 1 at creation. |
| 0x030 | 2 | `i16` | `enum_type` | read | For an enumerated attribute, the index of its enum type. |
| 0x032 | 2 | `[u8; 2]` | `pad_32` | unknown | No access found. |
| 0x034 | 8 | `[u32; 2]` | `range_low` | named | Lower bound from the range column (reldictConvertRangeToken); 0 at creation. |
| 0x03c | 2 | `i16` | `range_low_flag` | named | Belongs to the lower bound; 1 at creation. |
| 0x03e | 2 | `[u8; 2]` | `pad_3e` | unknown | No access found. |
| 0x040 | 8 | `[u32; 2]` | `range_high` | named | Upper bound from the range column; 0 at creation. |
| 0x048 | 2 | `i16` | `range_high_flag` | named | Belongs to the upper bound; 1 at creation. |
| 0x04a | 2 | `[u8; 2]` | `pad_4a` | unknown | No access found. |
| 0x04c | 2 | `i16` | `group` | named | From the group column; 5 by default. |
| 0x04e | 2 | `i16` | `rank` | named | From the rank column; 50 by default. |
| 0x050 | 4 | `i32` | `incmode` | named | From the inc column; 3 by default. |
| 0x054 | 2 | `i16` | `visible` | named | From the vis column. |
| 0x056 | 2 | `i16` | `nb_values` | read | Number of components (the # column); length of the two arrays below. |
| 0x058 | 4 | `Ptr32<i16>` | `fcv_ids` | read | Parameter id of each component; -1 at creation. |
| 0x05c | 4 | `Ptr32<CStr32>` | `fcv_names` | read | Short name of each component. |
| 0x060 | 2 | `i16` | `unk_60` | unknown | 0 at creation. |
| 0x062 | 2 | `[u8; 2]` | `pad_62` | unknown | No access found. |

### `EnumValDict` — `SI_EnumValDict`, 0x8 bytes

Size: the symbol SI_EVD is 8 bytes. Read 100.0% · named 0.0% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `nb_types` | read | Number of types. |
| 0x004 | 4 | `Ptr32<EnumValType>` | `types` | read | Array of types. |

### `EnumValType` — `SI_EnumValType`, 0xc bytes

Size: si_enmdictAddEnumType allocates 0xc bytes for the first and grows by 0xc. Read 100.0% · named 0.0% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `nb_values` | read | Number of values. |
| 0x004 | 4 | `Ptr32<EnumVal>` | `values` | read | Array of values. |
| 0x008 | 4 | `CStr32` | `name` | read | Type name. |

### `EnumVal` — `(entry of SI_EnumValType)`, 0xc bytes

Size: si_enmdictAddValue allocates 0xc bytes for the first and grows by 0xc. Read 100.0% · named 0.0% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `id` | read | The number. |
| 0x004 | 4 | `CStr32` | `code` | read | The short code (SI_enmdictEnumCodeGetByValue returns it). |
| 0x008 | 4 | `CStr32` | `name` | read | The long name (SI_enmdictEnumValGetNameById returns it). |

### `Model` — `__SI_Model`, 0x430 bytes

Size: SI_modelAllocate allocates 0x430 bytes and its last store is the word at +0x42c. Read 49.1% · named 26.3% · unknown 24.6%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `Ptr32<Opaque>` | `block_00` | unknown | A heap block freed with the node in SI_modelDisposeNode; null at creation. |
| 0x004 | 4 | `CStr32` | `name` | read | Labelled name in the dump; token 1; SI_modelAllocate duplicates its argument, or the text obj<n>. |
| 0x008 | 2 | `u16` | `flags_08` | unknown | Bit flags, 0 at creation; SI_envSkelSetFlag, SI_polySelection and SI_propagateTransform set bits in both bytes. |
| 0x00a | 2 | `u16` | `update_flags` | named | 0x3e at creation; every routine that sets scaling, rotation or translation ORs bits in; the readers and writers clear it once the matrices are rebuilt. |
| 0x00c | 2 | `i16` | `bbox_valid` | named | SI_bboxAttrGetValue tests it for zero before returning the bounding box; 0 at creation. |
| 0x00e | 2 | `[u8; 2]` | `pad_0e` | unknown | No access found. |
| 0x010 | 12 | `Vector` | `bbox_min` | read | Attributes 274 to 276 (BBox minimum). |
| 0x01c | 12 | `Vector` | `bbox_max` | read | Attributes 277 to 279 (BBox maximum). |
| 0x028 | 8 | `[f32; 2]` | `unk_28` | unknown | Two floats, 0 at creation, written by SI_modelComputeBoundingCP. |
| 0x030 | 4 | `i32` | `kind` | read | Labelled type in the dump; token 2; compared with 1, 2, 4, 6, 7, 8, 9, 10 to choose the geometry routines. |
| 0x034 | 2 | `u16` | `nb_defs` | read | Labelled nbdef: number of definitions. |
| 0x036 | 2 | `u16` | `cur_def` | read | Labelled curdef: index of the current definition. |
| 0x038 | 4 | `Ptr32<Ptr32<Opaque>>` | `defs` | read | Labelled data: one pointer per definition; the writer saves the first as token 3. |
| 0x03c | 4 | `Ptr32<Opaque>` | `org_def` | named | Written by SI_modelCopyOrgDef and by the shape and cluster readers. |
| 0x040 | 2 | `i16` | `serial` | read | Value of a counter at allocation, the number in the default name. |
| 0x042 | 2 | `[u8; 2]` | `pad_42` | unknown | No access found. |
| 0x044 | 4 | `Ptr32<Model>` | `latdep` | read | Labelled latdep; the node itself at creation; the writer uses its global matrix to save geometry in another node's space. |
| 0x048 | 4 | `Ptr32<Opaque>` | `definition` | read | Labelled definition: the current geometry (SI_Mesh, SI_Patch, SI_Face, SI_Spline, SI_NurbsSrf, ... by kind). |
| 0x04c | 2 | `u16` | `nb_lattices0` | read | Length of lattices0. |
| 0x04e | 2 | `[u8; 2]` | `pad_4e` | unknown | No access found. |
| 0x050 | 4 | `Ptr32<Ptr32<Lattice>>` | `lattices0` | read | Lattices of set 0, each disposed with SI_latticeDispose. |
| 0x054 | 2 | `u16` | `nb_lattices1` | read | Length of lattices1. |
| 0x056 | 2 | `[u8; 2]` | `pad_56` | unknown | No access found. |
| 0x058 | 4 | `Ptr32<Ptr32<Lattice>>` | `lattices1` | read | Lattices of set 1. |
| 0x05c | 2 | `i16` | `unk_5c` | unknown | 0 at creation; SI_latticeCopyFromSetup and si_modelSetLattice write it. |
| 0x05e | 2 | `[u8; 2]` | `pad_5e` | unknown | No access found. |
| 0x060 | 8 | `[Ptr32<SplFitting>; 2]` | `spline_fit` | read | Spline deformations, reset with SI_modelSplineFittingReset(model, 0 or 1). |
| 0x068 | 64 | `Matrix` | `def_matrix` | named | Identity at creation; SI_modelUpdateSonState stores the node's global matrix, or a product with it, here, and SI_modelReadjustDefinition multiplies the definition by it. |
| 0x0a8 | 12 | `Vector` | `hscl` | read | Labelled hscl; 1, 1, 1 at creation. |
| 0x0b4 | 64 | `Matrix` | `hrot` | read | Labelled hrot; identity at creation. |
| 0x0f4 | 12 | `Vector` | `htrs` | read | Labelled htrs. |
| 0x100 | 12 | `Vector` | `scal` | read | Labelled scal; token 4; attributes 1 to 3 (local scaling). |
| 0x10c | 12 | `Vector` | `vrot` | read | Labelled vrot; token 5; attributes 4 to 6 (local rotation angles). |
| 0x118 | 64 | `Matrix` | `mrot` | read | Labelled mrot: the rotation as a matrix. |
| 0x158 | 12 | `Vector` | `trans` | read | Labelled trans; token 6; attributes 139 to 141 (local translation). |
| 0x164 | 8 | `[f32; 2]` | `unk_164` | unknown | 0 at creation; written by doRotSGI. |
| 0x16c | 12 | `Vector` | `gscl` | read | Labelled gscl; attributes 1 to 3 when asked in global terms. |
| 0x178 | 64 | `Matrix` | `grot` | read | Labelled grot. |
| 0x1b8 | 12 | `Vector` | `gtrs` | read | Labelled gtrs; attributes 139 to 141 in global terms. |
| 0x1c4 | 64 | `Matrix` | `gmat` | read | Labelled gmat: the node's global matrix. |
| 0x204 | 2 | `i16` | `unk_204` | unknown | 0 at creation; pushMaterial and SI_modelClearRenderData write it. |
| 0x206 | 2 | `i16` | `nb_materials` | read | Length of materials; written as the count of token 7. |
| 0x208 | 2 | `i16` | `cur_material` | named | SI_materialSetCurrent. |
| 0x20a | 2 | `[u8; 2]` | `pad_20a` | unknown | No access found. |
| 0x20c | 4 | `Ptr32<Ptr32<Material>>` | `materials` | read | The node's materials; a polygon's material number indexes it. |
| 0x210 | 4 | `Ptr32<Opaque>` | `unk_210` | unknown | Written by materialEditor and si_cusTransferMaterial. Not set at creation. |
| 0x214 | 2 | `u16` | `unk_214` | unknown | Written by mdlInitTxtCnt. Not set at creation. |
| 0x216 | 2 | `i16` | `unk_216` | unknown | 0 at creation; mdlInitTxtCnt, SI_texture2Dgdisassociate. |
| 0x218 | 4 | `Ptr32<LclMap>` | `texture_218` | unknown | A 2D texture pointer beside the list head; SI_txtCopyFromSetup writes it. |
| 0x21c | 4 | `Ptr32<LclMap>` | `textures` | read | First 2D texture of the node; the list continues through the texture's own link; token 8. |
| 0x220 | 4 | `Ptr32<Opaque>` | `unk_220` | unknown | Written by si_cusTransferTexture. Not set at creation. |
| 0x224 | 4 | `Ptr32<Texture3D>` | `texture3d` | read | The node's 3D texture; token 9. |
| 0x228 | 4 | `i32` | `ren_count0` | named | Render data rebuilt by SI_collect and cleared by SI_modelClearRenderData: a count. |
| 0x22c | 4 | `Ptr32<Opaque>` | `ren_vertices` | named | Render vertices (allocExtraRenVertices); freed with the node. |
| 0x230 | 4 | `i32` | `ren_count1` | named | Render data: a second count. |
| 0x234 | 4 | `Ptr32<Opaque>` | `ren_triangles` | named | Render triangles; freed with the node. |
| 0x238 | 64 | `Matrix` | `mat_238` | unknown | Identity at creation; no other access found. |
| 0x278 | 64 | `Matrix` | `mat_278` | unknown | Identity at creation; no other access found. |
| 0x2b8 | 2 | `i16` | `visible` | read | SI_modelSetHiddenParam: 1 when the element's wire type is 'visible', else 0. |
| 0x2ba | 2 | `[u8; 2]` | `pad_2ba` | unknown | No access found. |
| 0x2bc | 4 | `Ptr32<Model>` | `father` | read | Labelled father. |
| 0x2c0 | 4 | `Ptr32<Model>` | `brother` | read | Labelled brother: next son of the same father. |
| 0x2c4 | 4 | `Ptr32<Model>` | `son` | read | Labelled son: first son. |
| 0x2c8 | 4 | `Ptr32<Opaque>` | `path` | named | SI_modelSetPath. |
| 0x2cc | 8 | `[u32; 2]` | `unk_2cc` | unknown | 0 at creation; no other access found. |
| 0x2d4 | 4 | `Ptr32<Opaque>` | `shape_data` | named | Written by SI_shapeBinaryRead; freed with the node. |
| 0x2d8 | 8 | `[Ptr32<PtchFitting>; 2]` | `patch_fit` | read | Patch deformations, reset with SI_modelPatchFittingReset(model, 0 or 1). |
| 0x2e0 | 2 | `i16` | `shape_interp` | named | Token 12 (shape interpolation). |
| 0x2e2 | 2 | `i16` | `unk_2e2` | unknown | 0 at creation; SI_shapeBinaryRead writes it. |
| 0x2e4 | 2 | `i16` | `unk_2e4` | unknown | 0 at creation. |
| 0x2e6 | 2 | `[u8; 2]` | `pad_2e6` | unknown | No access found. |
| 0x2e8 | 4 | `Ptr32<Opaque>` | `waves` | named | SI_waveAddMdl, SI_modelUnlinkAllWaves. |
| 0x2ec | 4 | `i32` | `algo_type` | read | Attribute 178 (AlgoType); token 16. In memory 2 and 12 exist beside the file's values. |
| 0x2f0 | 4 | `Ptr32<KineInfo>` | `kine_info` | named | Attributes 221 to 232 (velocities and accelerations) are read through it. |
| 0x2f4 | 4 | `Ptr32<DynaMatterInfo>` | `dyna_matter` | read | Attributes 179 to 181 (elasticity, roughness) are read through it; token 14. |
| 0x2f8 | 4 | `Ptr32<DynaJntInfo>` | `dyna_jnt` | named | Token 19 (joint properties) is present when it is not null. |
| 0x2fc | 4 | `Ptr32<DynaConstrInfo>` | `dyna_constr` | named | SI_conslistAddConstraint. |
| 0x300 | 4 | `Ptr32<DynaJntVars>` | `dyna_jnt_vars` | named | SI_modelDynaInfoCopy. |
| 0x304 | 4 | `Ptr32<DynaChnVars>` | `dyna_chn_vars` | named | Attribute 182 (RootCons) is read through it; token 18 says whether it is null. |
| 0x308 | 4 | `Ptr32<Opaque>` | `unk_308` | unknown | 0 at creation; freed by SI_modelDynaInfoDispose. |
| 0x30c | 12 | `[Ptr32<Model>; 3]` | `dyna_links` | named | Three model pointers, null at creation, set by SI_dynaPhyPropsSetNewRoot and SI_dynamicChainsUpdate. |
| 0x318 | 4 | `Ptr32<InvKin>` | `inv_kin` | read | Disposed with SI_invKinDispose; token 20. |
| 0x31c | 4 | `i32` | `envelop` | named | First value of token 17, written as 16 bits. |
| 0x320 | 4 | `Ptr32<Opaque>` | `unk_320` | unknown | 0 at creation; written by SI_modelCopyNode and skeTubingUpdate. |
| 0x324 | 4 | `Ptr32<EnvSkelJnt>` | `env_skel_jnt` | read | Disposed with SI_envelopInfoDispose; second part of token 17. |
| 0x328 | 4 | `Ptr32<Vector>` | `init_srt` | read | Three vectors written as token 25 (initial transformation) when not null; freed with the node. |
| 0x32c | 4 | `Ptr32<Opaque>` | `unk_32c` | unknown | Freed with the node; SI_envSkelResetFlag writes it. |
| 0x330 | 4 | `Ptr32<Opaque>` | `extra_tcoords` | named | allocExtraTCoord; cleared by SI_modelClearRenderData. |
| 0x334 | 4 | `i32` | `unk_334` | unknown | Written by SI_textureVerts. Not set at creation. |
| 0x338 | 2 | `i16` | `unk_338` | unknown | Written by isMoving. Not set at creation. |
| 0x33a | 2 | `[u8; 2]` | `pad_33a` | unknown | No access found. |
| 0x33c | 4 | `Ptr32<Opaque>` | `unk_33c` | unknown | 0 at creation; saveInitialPositions and the mental ray writers write it. |
| 0x340 | 4 | `f32` | `unk_340` | unknown | Written by isMoving. Not set at creation. |
| 0x344 | 4 | `Ptr32<Cluster>` | `cluster` | read | Cluster animation, disposed with SI_clusterDispose. |
| 0x348 | 4 | `Ptr32<Kluster>` | `klusters` | named | SI_klusterAddToList, SI_klusterDisposeAll. |
| 0x34c | 4 | `Ptr32<UserNode>` | `user_data` | read | List of user data blocks, linked through their word at +0x10; token 22. |
| 0x350 | 2 | `i16` | `unk_350` | unknown | 0 at creation; read by SI_graflistDeleteGrafnode. |
| 0x352 | 2 | `i16` | `unk_352` | unknown | 0 at creation; read by SI_isModelDeformed. |
| 0x354 | 4 | `i32` | `elem_id` | read | Index of the node's element in the MODELS chapter (SI_elemdictAddModel stores what SI_elemdictAddElem returns); -1 at creation. |
| 0x358 | 8 | `[u32; 2]` | `unk_358` | unknown | 0 at creation; no other access found. |
| 0x360 | 64 | `Matrix` | `cscl_matrix` | named | Identity at creation; filled by SI_modelUpdateCScalingInfo. |
| 0x3a0 | 12 | `Vector` | `cscl` | named | 1, 1, 1 at creation; SI_modelUpdateCScalingInfo, SI_updateScalingMethod. |
| 0x3ac | 12 | `Vector` | `unk_3ac` | unknown | 0 at creation; SI_modelSpecialUpdateSonState writes it, the inverse kinematics routines read it. |
| 0x3b8 | 24 | `[Vector; 2]` | `comp_angles` | named | 0 at creation; SI_modelCompensateLocalCoord, compensateAngleUpdate. |
| 0x3d0 | 24 | `[Vector; 2]` | `comp_keys` | named | 0 at creation; keyCompensateInfoUpdate reads them. |
| 0x3e8 | 4 | `Ptr32<CompInfoList>` | `comp_info` | read | Disposed with SI_classicMdlCompInfoDispose. |
| 0x3ec | 2 | `i16` | `visibility` | named | Token 23; 7 at creation. |
| 0x3ee | 2 | `i16` | `motion_blur` | named | Token 24. |
| 0x3f0 | 4 | `Ptr32<Opaque>` | `gl_list` | named | modelCreateGlist, SI_glCallDispList. |
| 0x3f4 | 4 | `f32` | `matte` | read | Attribute 371 (NMatte); token 26; 1.0 at creation. |
| 0x3f8 | 2 | `i16` | `garbage_matte` | read | Attribute 538 (GMatte); token 27. |
| 0x3fa | 2 | `u16` | `update_flags2` | named | 0x3e at creation; set together with update_flags. |
| 0x3fc | 4 | `Ptr32<Opaque>` | `unk_3fc` | unknown | 0 at creation; read by plmDraw. |
| 0x400 | 2 | `i16` | `unk_400` | unknown | 1 at creation; no other access found. |
| 0x402 | 2 | `i16` | `unk_402` | unknown | 0 at creation; edSetMasterFlag writes it. |
| 0x404 | 2 | `i16` | `caustic` | read | Bit 0: attribute 620 (CsTrans); bit 1: attribute 631 (CsReceiv). |
| 0x406 | 2 | `i16` | `global_illum` | read | Bit 0: attribute 632 (GITrans); bit 1: attribute 633 (GIReceiv). |
| 0x408 | 4 | `Ptr32<Kluster>` | `vertex_ud_cluster` | read | Per-vertex user data: the cluster of tagged vertices (token 29). |
| 0x40c | 4 | `Ptr32<Ptr32<UserNode>>` | `vertex_ud` | read | Per-vertex user data (token 30); disposed with its cluster by SI_subelUserDataDispose. |
| 0x410 | 4 | `Ptr32<Kluster>` | `polygon_ud_cluster` | read | Per-polygon user data: cluster (token 31). |
| 0x414 | 4 | `Ptr32<Ptr32<UserNode>>` | `polygon_ud` | read | Per-polygon user data (token 32). |
| 0x418 | 4 | `Ptr32<Kluster>` | `edge_ud_cluster` | read | Per-edge user data: cluster (token 33). |
| 0x41c | 4 | `Ptr32<Ptr32<UserNode>>` | `edge_ud` | read | Per-edge user data (token 34). |
| 0x420 | 2 | `i16` | `wf_display` | read | Attribute 676 (WFDisplay). |
| 0x422 | 2 | `[u8; 2]` | `pad_422` | unknown | No access found. |
| 0x424 | 4 | `i32` | `nb_uv` | read | Number of texture coordinate sets of a mesh, 1 at creation; the mesh writer stores it and each polygon node carries nb_uv - 1 extra pairs. |
| 0x428 | 4 | `Ptr32<Opaque>` | `uv_domains` | read | nb_uv - 1 entries of 8 bytes, each a pointer (freed with the node) and a count. |
| 0x42c | 4 | `Ptr32<Opaque>` | `unk_42c` | unknown | 0 at creation; freed with the node; SI_collect writes it. |

### `Vector` — `SI_Vector`, 0xc bytes

Size: SI_Vector_fread and SI_Vector_fwrite step through arrays by 0xc and swap three words per item; SI_vectorMultiplyMatrix indexes by 0xc. Read 100.0% · named 0.0% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `f32` | `x` | read | First float. |
| 0x004 | 4 | `f32` | `y` | read | Second float. |
| 0x008 | 4 | `f32` | `z` | read | Third float. |

### `GrafNode` — `__SI_GrafNode`, 0x24 bytes

Size: grafnodeAllocate allocates 0x24 bytes. Read 77.8% · named 16.7% · unknown 5.6%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `kind` | read | What data is: 2 the root node of a model, 3 another node of it, 4 a camera, 6 a light, 5 and 7 the nodes created in front of those, 0xb a wave, 0xe a material, 0xf and 0x10 textures (SI_graflistAddData). |
| 0x004 | 4 | `Ptr32<Opaque>` | `data` | read | The object: a __SI_Model for kinds 2 and 3. |
| 0x008 | 4 | `i32` | `selection` | read | Selection state: 0 none, 1 node, 2 inherited, 3 branch; the element's attribute 265 when a graf node exists. |
| 0x00c | 4 | `i32` | `wire_type` | read | Display type (0 hidden, 1 ghosted, 2 visible), 2 at creation; attribute 268. |
| 0x010 | 2 | `i16` | `nb_sub` | read | For a model node, its number of sub-models (SI_modelNbSubModels); 1 for the node in front of a camera or light. |
| 0x012 | 2 | `i16` | `wire_color` | read | Wire-frame colour index: SI_select7WM + SI_color1 when added; attribute 267. |
| 0x014 | 2 | `i16` | `unk_14` | unknown | 0 at creation; SI_sceneKeySetColor writes it. |
| 0x016 | 2 | `u16` | `flags` | named | Bit 0 clear = selectable (attribute 567); bit 0x1000 is tested when a relation is removed. |
| 0x018 | 4 | `Ptr32<Code>` | `func` | named | A function pointer set by SI_updateGnFunc and called when the node is drawn. |
| 0x01c | 4 | `Ptr32<GrafNode>` | `next` | read | Next node (graflistInsertGrafnode). |
| 0x020 | 4 | `Ptr32<GrafNode>` | `prev` | read | Previous node. |

### `GrafList` — `SI_GrafList`, 0x24 bytes

Size: SI_grafListAllocate allocates 0x24 bytes; the global array SI_grafLists is indexed in steps of 0x24. Read 11.1% · named 27.8% · unknown 61.1%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 2 | `i16` | `multi` | named | 0 = one active node at a time: SI_graflistAddData then unselects the previous one. |
| 0x002 | 2 | `[u8; 2]` | `pad_02` | unknown | No access found. |
| 0x004 | 4 | `Ptr32<GrafNode>` | `first` | read | Head of the list (graflistInsertGrafnode). |
| 0x008 | 4 | `Ptr32<GrafNode>` | `active` | named | The node SI_graflistSetActive made active. |
| 0x00c | 2 | `i16` | `unk_0c` | unknown | 0 at creation. |
| 0x00e | 2 | `i16` | `unk_0e` | unknown | 1 at creation. |
| 0x010 | 16 | `[u32; 4]` | `unk_10` | unknown | Partly cleared at creation; not read. |
| 0x020 | 4 | `Ptr32<GrafNode>` | `active2` | named | Used in place of active when that is null. |

### `Mesh` — `SI_Mesh`, 0x54 bytes

Size: SI_meshAllocate allocates 0x54 bytes. Read 35.7% · named 50.0% · unknown 14.3%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `Ptr32<Vertex>` | `vertices` | read | Array of nb_vertices vertices (SI_vertexAllocate). |
| 0x004 | 4 | `Ptr32<Polygon>` | `polygons` | read | Array of nb_polygons polygons (SI_polygonAllocate). |
| 0x008 | 4 | `Ptr32<Ptr32<Edge>>` | `edges` | read | Table of blocks of 1024 edges; edge n (counted from 1) is entry n & 0x3ff of block n >> 10. |
| 0x00c | 4 | `f32` | `angle` | read | Attribute 319 (PolyDisc, the discontinuity angle); token angle; 60.0 at creation. |
| 0x010 | 4 | `f32` | `cos_angle` | named | Set at creation to the cosine of 1.04719755 radians (60 degrees), as a float. |
| 0x014 | 4 | `i32` | `nb_vertices` | read | Attribute 314 (PolyNbVert). |
| 0x018 | 4 | `i32` | `nb_polygons` | read | Attribute 312 (PolyNbPoly). |
| 0x01c | 4 | `i32` | `nb_edges` | read | Attribute 313 (PolyNbEdg). |
| 0x020 | 2 | `u16` | `flags` | read | Token flag; bit 1 is attribute 318 (PolyIsAutom), bit 2 attribute 317 (PolyProNor); 0x804 at creation. |
| 0x022 | 2 | `[u8; 2]` | `pad_22` | unknown | No access found. |
| 0x024 | 4 | `i32` | `nb_display` | named | Length of display (SI_meshOptimizeDisplay). |
| 0x028 | 4 | `Ptr32<i32>` | `display` | named | Integers built by SI_meshOptimizeDisplay and used by the drawing routines. |
| 0x02c | 4 | `i32` | `unk_2c` | unknown | 0 at creation. |
| 0x030 | 4 | `Ptr32<MeshListNode>` | `polygon_list0` | named | A polygon list (SI_meshCopyPolygonLists, SI_meshUpdatePolygonLists). |
| 0x034 | 4 | `Ptr32<MeshListNode>` | `polygon_list1` | named | A second polygon list. |
| 0x038 | 2 | `i16` | `mr_view` | named | Third of nine values written after the edges; by position the patch's view-dependent flag. 0 at creation. |
| 0x03a | 2 | `[u8; 2]` | `pad_3a` | unknown | No access found. |
| 0x03c | 4 | `f32` | `mr_edge` | named | 5.0 at creation: with the next two the same triple (5, 0.1, 45) as the patch's and the surface's approximation settings. |
| 0x040 | 4 | `f32` | `mr_ucurv` | named | 0.1 at creation. |
| 0x044 | 4 | `f32` | `mr_vcurv` | named | 45.0 at creation. |
| 0x048 | 2 | `i16` | `mr_min` | named | 1 at creation. |
| 0x04a | 2 | `i16` | `mr_max` | named | 5 at creation. |
| 0x04c | 2 | `i16` | `mr_rec` | named | 0 at creation. |
| 0x04e | 2 | `[u8; 2]` | `pad_4e` | unknown | No access found. |
| 0x050 | 2 | `i16` | `mr_approx` | named | 2 at creation; second of the nine values written after the edges. |
| 0x052 | 2 | `i16` | `mr_first` | unknown | 0 at creation; first of the nine values written after the edges. |

### `Vertex` — `(vertex of SI_Mesh)`, 0x14 bytes

Size: SI_vertexAllocate allocates records of 0x14 bytes. Read 70.0% · named 0.0% · unknown 30.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 12 | `Vector` | `pos` | read | Token position. |
| 0x00c | 4 | `Ptr32<Vector>` | `vectors` | unknown | An array of nb_vectors vectors, copied with the vertex by SI_meshCopy; what they are is not read. |
| 0x010 | 2 | `u16` | `nb_vectors` | unknown | Length of vectors. |
| 0x012 | 2 | `i16` | `flag` | read | Token flag, written after the position. |

### `Polygon` — `(polygon of SI_Mesh)`, 0x18 bytes

Size: SI_polygonAllocate allocates records of 0x18 bytes. Read 41.7% · named 0.0% · unknown 58.3%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 12 | `Vector` | `vec_00` | unknown | Three words copied with the polygon; not written to files. |
| 0x00c | 4 | `Ptr32<PolyNode>` | `nodes` | read | Array of nb_nodes corners (SI_polygonNodeAllocate). |
| 0x010 | 2 | `u16` | `nb_nodes` | read | Number of corners; written before them. |
| 0x012 | 2 | `i16` | `material` | read | Index into the node's materials; the reader resets it to 0 when out of range. |
| 0x014 | 2 | `i16` | `flag` | read | Token flag, written after the material in newer files. |
| 0x016 | 2 | `[u8; 2]` | `pad_16` | unknown | No access found. |

### `PolyNode` — `SI_Node`, 0x24 bytes

Size: SI_polygonNodeAllocate allocates records of 0x24 bytes. Read 88.9% · named 0.0% · unknown 11.1%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `vertex` | read | Index of the vertex; -1 at creation. |
| 0x004 | 4 | `i32` | `unk_04` | unknown | 0 at creation; not written to files. |
| 0x008 | 12 | `Vector` | `normal` | read | Token normal. |
| 0x014 | 4 | `f32` | `u` | read | Token uv, first value. |
| 0x018 | 4 | `f32` | `v` | read | Token uv, second value. |
| 0x01c | 4 | `[u8; 4]` | `color` | read | Token colour: four bytes, written as one word. 0xffb2b2b2 at creation. |
| 0x020 | 4 | `Ptr32<[f32; 2]>` | `extra_uv` | read | nb_uv - 1 further pairs, nb_uv being the model's. |

### `Edge` — `(edge of SI_Mesh)`, 0x14 bytes

Size: SI_edgeAllocate allocates blocks of 0x400 records of 0x14 bytes. Read 50.0% · named 0.0% · unknown 50.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `v0` | read | First vertex; -1 at creation. |
| 0x004 | 4 | `i32` | `v1` | read | Second vertex; -1 at creation. |
| 0x008 | 4 | `i32` | `unk_08` | unknown | -1 at creation; copied with the edge, not written to files. |
| 0x00c | 4 | `i32` | `unk_0c` | unknown | -1 at creation; copied with the edge, not written to files. |
| 0x010 | 2 | `i16` | `unk_10` | unknown | 0 at creation. |
| 0x012 | 2 | `i16` | `flag` | read | Token flag. |

### `Face` — `SI_Face`, 0x88 bytes

Size: SI_faceAllocate allocates 0x88 bytes. Read 26.5% · named 7.4% · unknown 66.2%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `CStr32` | `name` | named | Read as a string by SI_faceDump; freed with the face; null at creation. |
| 0x004 | 116 | `Spline` | `spline` | read | The outline when is_spline is set: initialised with SI_splineInit, written with splineBinaryWrite. |
| 0x078 | 2 | `i16` | `nb_holes` | read | Attribute 310 (FaceNbHole): number of holes; number of curves when the face is made of NURBS curves. |
| 0x07a | 2 | `[u8; 2]` | `pad_7a` | unknown | No access found. |
| 0x07c | 4 | `Ptr32<Spline>` | `holes` | read | Array of nb_holes splines. |
| 0x080 | 4 | `Ptr32<NurbsCrv>` | `curves` | read | Array of nb_holes NURBS curves when is_spline is 0. |
| 0x084 | 2 | `i16` | `is_spline` | read | Attribute 311 (FaceIsSpline); 1 at creation. |
| 0x086 | 2 | `[u8; 2]` | `pad_86` | unknown | No access found. |

### `Spline` — `SI_Spline`, 0x74 bytes

Size: SI_splineAllocate allocates 0x74 bytes; a face embeds one in 0x74 bytes and keeps its holes 0x74 apart. Read 20.7% · named 5.2% · unknown 74.1%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `CStr32` | `name` | read | Token name; SI_splineAllocate sets spl<n>. |
| 0x004 | 2 | `i16` | `unk_04` | unknown | 0x10 at creation; SI_splineTouch writes it. |
| 0x006 | 2 | `i16` | `phantom` | read | Attribute 309 (SplPhantom); token phantom. |
| 0x008 | 2 | `i16` | `closed` | read | Attribute 306 (SplClose); token close. |
| 0x00a | 2 | `[u8; 2]` | `pad_0a` | unknown | No access found. |
| 0x00c | 4 | `i32` | `kind` | read | Attribute 305 (SplTyp); token type, written as 16 bits. |
| 0x010 | 2 | `i16` | `nb_keys` | read | Attribute 304 (SplNbPoint). |
| 0x012 | 2 | `[u8; 2]` | `pad_12` | unknown | No access found. |
| 0x014 | 4 | `Ptr32<SplineKey>` | `keys` | read | Array of nb_keys keys. |
| 0x018 | 4 | `i32` | `unk_18` | unknown | 0 at creation. |
| 0x01c | 4 | `f32` | `tension` | read | Attribute 307 (SplTension). |
| 0x020 | 2 | `i16` | `step` | read | Attribute 308 (SplStep); 10 at creation. |
| 0x022 | 2 | `[u8; 2]` | `pad_22` | unknown | No access found. |
| 0x024 | 64 | `Matrix` | `matrix` | unknown | Identity at creation. |
| 0x064 | 2 | `i16` | `unk_64` | unknown | 2 at creation; read by SI_splineAddPoint. |
| 0x066 | 2 | `i16` | `unk_66` | unknown | 1 at creation; read by SI_splineAddPoint. |
| 0x068 | 2 | `u16` | `nb_lengths` | named | SI_splineUpdateLength. |
| 0x06a | 2 | `[u8; 2]` | `pad_6a` | unknown | No access found. |
| 0x06c | 4 | `Ptr32<Opaque>` | `lengths` | named | Table built by SI_splineUpdateLength; freed with the spline. |
| 0x070 | 2 | `i16` | `unk_70` | unknown | 0 at creation; SI_checkSplineToFcurve writes it. |
| 0x072 | 2 | `[u8; 2]` | `pad_72` | unknown | No access found. |

### `SplineKey` — `(key of SI_Spline)`, 0x30 bytes

Size: splineBinaryWrite steps through the keys by 0x30. Read 0.0% · named 87.5% · unknown 12.5%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 12 | `Vector` | `prev` | named | Token previous: first vector written. |
| 0x00c | 12 | `Vector` | `pos` | named | Token position. |
| 0x018 | 12 | `Vector` | `next` | named | Token next. |
| 0x024 | 4 | `[u8; 4]` | `unk_24` | unknown | Not written to files. |
| 0x028 | 2 | `i16` | `constraint` | named | Token constraint. |
| 0x02a | 2 | `i16` | `segment` | named | Token segment. |
| 0x02c | 2 | `i16` | `flag` | named | Token flag. |
| 0x02e | 2 | `[u8; 2]` | `pad_2e` | unknown | No access found. |

### `Patch` — `SI_Patch`, 0x204 bytes

Size: SI_patchAllocate allocates 0x204 bytes. Read 9.3% · named 35.3% · unknown 55.4%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `unk_000` | unknown | 0 at creation. |
| 0x004 | 4 | `i32` | `u_type` | read | Attribute 290 (PtchSplnTyp, U). |
| 0x008 | 4 | `i32` | `v_type` | read | Attribute 291. |
| 0x00c | 2 | `u16` | `nb_u` | read | Attribute 288 (PtchNbPoint, U). |
| 0x00e | 2 | `u16` | `nb_v` | read | Attribute 289. |
| 0x010 | 2 | `i16` | `u_closed` | read | Attribute 292 (PtchClose, U). |
| 0x012 | 2 | `i16` | `v_closed` | read | Attribute 293. |
| 0x014 | 4 | `f32` | `u_tension` | read | Attribute 294 (PtchTension, U). |
| 0x018 | 4 | `f32` | `v_tension` | read | Attribute 295. |
| 0x01c | 2 | `u16` | `u_step` | read | Attribute 296 (PtchStep, U); 3 at creation. |
| 0x01e | 2 | `u16` | `v_step` | read | Attribute 297; 3 at creation. |
| 0x020 | 2 | `u16` | `u_nb_curves` | read | Attribute 298 (PtchNbCrv, U); 1 at creation. |
| 0x022 | 2 | `u16` | `v_nb_curves` | read | Attribute 299; 1 at creation. |
| 0x024 | 128 | `[Matrix; 2]` | `basis` | named | Two matrices filled by SI_patchUpdateMatrix and read by SI_patchValue. Not set at creation. |
| 0x0a4 | 256 | `[Matrix; 4]` | `matrices` | unknown | Four matrices, identity at creation. |
| 0x1a4 | 4 | `Ptr32<[f32; 4]>` | `points` | read | nb_u × nb_v points of 16 bytes each; the writer stores the first three floats of each. |
| 0x1a8 | 4 | `Ptr32<i16>` | `tags` | read | One 16-bit value per point; the last token. |
| 0x1ac | 2 | `i16` | `unk_1ac` | unknown | 0 at creation. |
| 0x1ae | 2 | `[u8; 2]` | `pad_1ae` | unknown | No access found. |
| 0x1b0 | 4 | `Ptr32<f32>` | `surface0` | named | Built by SI_patchUpdateSurface; freed with the patch. |
| 0x1b4 | 4 | `[u16; 2]` | `surface0_dims` | named | Set with surface0. |
| 0x1b8 | 4 | `Ptr32<f32>` | `surface1` | named | Built by SI_patchUpdateSurface; freed with the patch. |
| 0x1bc | 4 | `[u16; 2]` | `surface1_dims` | named | Set with surface1. |
| 0x1c0 | 2 | `i16` | `unk_1c0` | unknown | 0 at creation; SI_patchComputesSubdivision writes it. |
| 0x1c2 | 2 | `[u8; 2]` | `pad_1c2` | unknown | No access found. |
| 0x1c4 | 4 | `Ptr32<Opaque>` | `subdiv0` | named | SI_patchComputesSubdivision; freed with the patch. |
| 0x1c8 | 4 | `Ptr32<Opaque>` | `subdiv1` | named | SI_patchComputesSubdivision; freed with the patch. |
| 0x1cc | 2 | `i16` | `u_bottom` | read | Attribute 300 (PtchBottom, U). |
| 0x1ce | 2 | `i16` | `u_top` | read | Attribute 302 (PtchTop, U). |
| 0x1d0 | 2 | `i16` | `v_bottom` | read | Attribute 301 (PtchBottom, V). |
| 0x1d2 | 2 | `i16` | `v_top` | read | Attribute 303 (PtchTop, V). |
| 0x1d4 | 2 | `i16` | `unk_1d4` | unknown | 0 at creation; the last of nine 16-bit tokens written after the tensions. |
| 0x1d6 | 2 | `i16` | `unk_1d6` | unknown | 0 at creation. |
| 0x1d8 | 4 | `Ptr32<i16>` | `modif0` | named | SI_patchUpdateSurface; freed with the patch. |
| 0x1dc | 4 | `Ptr32<i16>` | `modif1` | named | SI_patchUpdateSurface; freed with the patch. |
| 0x1e0 | 2 | `i16` | `unk_1e0` | unknown | Written by SI_dragPoint and the tag routines. Not set at creation. |
| 0x1e2 | 2 | `[u8; 2]` | `pad_1e2` | unknown | No access found. |
| 0x1e4 | 4 | `i32` | `unk_1e4` | unknown | 0 at creation; patchUpdateDef writes it. |
| 0x1e8 | 2 | `i16` | `mr_view` | named | Second of the eight approximation tokens of newer files; 0 at creation. |
| 0x1ea | 2 | `[u8; 2]` | `pad_1ea` | unknown | No access found. |
| 0x1ec | 4 | `f32` | `mr_edge` | named | Third approximation token; 5.0 at creation. |
| 0x1f0 | 4 | `f32` | `mr_ucurv` | named | Fourth; 0.1 at creation. |
| 0x1f4 | 4 | `f32` | `mr_vcurv` | named | Fifth; 45.0 at creation. |
| 0x1f8 | 2 | `i16` | `mr_min` | named | Sixth; 1 at creation. |
| 0x1fa | 2 | `i16` | `mr_max` | named | Seventh; 10 at creation. |
| 0x1fc | 2 | `i16` | `mr_rec` | named | Eighth; 0 at creation. |
| 0x1fe | 2 | `[u8; 2]` | `pad_1fe` | unknown | No access found. |
| 0x200 | 2 | `i16` | `mr_approx` | named | First approximation token; 1 at creation. |
| 0x202 | 2 | `[u8; 2]` | `pad_202` | unknown | No access found. |

### `NurbsDir` — `SI_NurbsDir (inferred)`, 0x24 bytes

Size: a surface holds the same nine fields at +0 and +0x24. Read 50.0% · named 44.4% · unknown 5.6%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `i32` | `order` | read | Degree + 1 (attributes 321, 327, 328 return order - 1); 4 at creation. |
| 0x004 | 4 | `i32` | `nb_points` | read | Attributes 320, 325, 326. |
| 0x008 | 4 | `i32` | `nb_knots` | read | Length of knots. |
| 0x00c | 4 | `Ptr32<Double>` | `knots` | read | Knot vector. |
| 0x010 | 8 | `Double` | `range_start` | named | Token range, first value. |
| 0x018 | 8 | `Double` | `range_end` | named | Token range, second value. |
| 0x020 | 2 | `i16` | `closed` | read | Attributes 322, 329, 330. |
| 0x022 | 2 | `[u8; 2]` | `pad_22` | unknown | No access found. |

### `NurbsCrv` — `__SI_NurbsCrv`, 0x64 bytes

Size: SI_nurbsCrvAllocate allocates 100 bytes; a face keeps its curves 100 bytes apart. Read 30.0% · named 34.0% · unknown 36.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 36 | `NurbsDir` | `dir` | read | Order, counts, knots, range and closed flag. |
| 0x024 | 4 | `i32` | `param` | read | Attribute 324 (CrvParam, the parameterisation); written before the knots. |
| 0x028 | 2 | `i16` | `unk_28` | unknown | 0 at creation; copied by SI_nurbsCrvCopy. |
| 0x02a | 2 | `[u8; 2]` | `pad_2a` | unknown | No access found. |
| 0x02c | 4 | `Ptr32<NurbsPoint>` | `points` | read | Array of dir.nb_points control points. |
| 0x030 | 4 | `i32` | `closing` | named | Token closing; 1 at creation. |
| 0x034 | 4 | `i32` | `step` | read | Attribute 323 (CrvStep); 10 at creation. |
| 0x038 | 4 | `u32` | `flags_38` | unknown | 0 at creation; the point-editing routines set bits in its last byte. |
| 0x03c | 8 | `[u32; 2]` | `unk_3c` | unknown | 0 at creation; copied as one 8-byte value. |
| 0x044 | 2 | `i16` | `unk_44` | unknown | 0 at creation; SI_nurbsCrvCut writes it. |
| 0x046 | 2 | `[u8; 2]` | `pad_46` | unknown | No access found. |
| 0x048 | 4 | `i32` | `unk_48` | unknown | 0 at creation; read by the proportional-editing routines. |
| 0x04c | 2 | `i16` | `keyframed` | named | Token keyframed, the last one written. |
| 0x04e | 2 | `i16` | `unk_4e` | unknown | 0 at creation. |
| 0x050 | 12 | `[u32; 3]` | `length_data` | named | Three words set by SI_nurbsCurveUpdateLength. |
| 0x05c | 8 | `[u32; 2]` | `unk_5c` | unknown | 0 at creation. |

### `NurbsPoint` — `SI_NurbsPoint`, 0x24 bytes

Size: the writers step through control points by 0x24: four doubles and a word. Read 77.8% · named 22.2% · unknown 0.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 8 | `Double` | `x` | read | First of four doubles written per point. |
| 0x008 | 8 | `Double` | `y` | read | Second. |
| 0x010 | 8 | `Double` | `z` | read | Third. |
| 0x018 | 8 | `Double` | `w` | named | Fourth: the weight (attribute 539). |
| 0x020 | 4 | `i32` | `flag` | read | A word written after the doubles. |

### `NurbsSrf` — `SI_NurbsSrf`, 0xdc bytes

Size: SI_nurbsSrfAllocate allocates 0xdc bytes. Read 40.0% · named 29.1% · unknown 30.9%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 36 | `NurbsDir` | `u` | read | The U direction. |
| 0x024 | 36 | `NurbsDir` | `v` | read | The V direction. |
| 0x048 | 4 | `i32` | `nb_points` | read | Total number of control points: the bound of the writer's loop. |
| 0x04c | 4 | `i32` | `param` | read | Attribute 339 (SrfParam). |
| 0x050 | 2 | `i16` | `unk_50` | unknown | 0 at creation; set by the revolution and extrusion routines. |
| 0x052 | 2 | `[u8; 2]` | `pad_52` | unknown | No access found. |
| 0x054 | 4 | `Ptr32<NurbsPoint>` | `points` | read | Array of nb_points control points. |
| 0x058 | 4 | `i32` | `unk_58` | unknown | 0 at creation; si_handleAutoBorder writes it. |
| 0x05c | 2 | `i16` | `border` | named | Token border. |
| 0x05e | 2 | `[u8; 2]` | `pad_5e` | unknown | No access found. |
| 0x060 | 4 | `i32` | `nb_trims` | read | Number of trim curves. |
| 0x064 | 4 | `Ptr32<Ptr32<NurbsCompCrv>>` | `trims` | read | The trim curves (composite curves). |
| 0x068 | 4 | `i32` | `nb_projected` | read | Number of projected curves (SI_nurbsSrfGetProjCrv); written last. |
| 0x06c | 4 | `Ptr32<Ptr32<NurbsCompCrv>>` | `projected` | read | The projected curves. |
| 0x070 | 4 | `i32` | `u_step` | read | Attribute 331 (SrfStep, U); 3 at creation. |
| 0x074 | 4 | `i32` | `v_step` | read | Attribute 332; 3 at creation. |
| 0x078 | 4 | `i32` | `u_nb_curves` | read | Attribute 333 (SrfNbCrv, U); 1 at creation. |
| 0x07c | 4 | `i32` | `v_nb_curves` | read | Attribute 334; 1 at creation. |
| 0x080 | 4 | `Ptr32<Opaque>` | `lines0` | named | Display lines (SI_nurbSrfUpdateLines, SI_nurbsSrfDisposeLines). |
| 0x084 | 4 | `Ptr32<Opaque>` | `lines1` | named | Display lines. |
| 0x088 | 2 | `i16` | `u_bottom` | read | Attribute 335 (SrfBottom, U). |
| 0x08a | 2 | `i16` | `u_top` | read | Attribute 337 (SrfTop, U). |
| 0x08c | 2 | `i16` | `v_bottom` | read | Attribute 336. |
| 0x08e | 2 | `i16` | `v_top` | read | Attribute 338. |
| 0x090 | 8 | `[i32; 2]` | `unk_90` | unknown | 1 and 1 at creation; written after the cap flags. |
| 0x098 | 2 | `i16` | `mr_approx` | named | 1 at creation; by position and default the patch's approximation flag. |
| 0x09a | 2 | `[u8; 2]` | `pad_9a` | unknown | No access found. |
| 0x09c | 2 | `i16` | `mr_view` | named | 0 at creation. |
| 0x09e | 2 | `[u8; 2]` | `pad_9e` | unknown | No access found. |
| 0x0a0 | 4 | `f32` | `mr_edge` | named | 5.0 at creation. |
| 0x0a4 | 4 | `f32` | `mr_ucurv` | named | 0.1 at creation. |
| 0x0a8 | 4 | `f32` | `mr_vcurv` | named | 45.0 at creation. |
| 0x0ac | 2 | `i16` | `mr_min` | named | 1 at creation. |
| 0x0ae | 2 | `i16` | `mr_max` | named | 10 at creation. |
| 0x0b0 | 2 | `i16` | `mr_rec` | named | 0 at creation. |
| 0x0b2 | 2 | `[u8; 2]` | `pad_b2` | unknown | No access found. |
| 0x0b4 | 4 | `[i16; 2]` | `unk_b4` | unknown | 0 at creation; the trimming dialogs write the first. |
| 0x0b8 | 16 | `[Double; 2]` | `unk_b8` | unknown | 0 at creation; read as doubles by nurbsSrfFunc. |
| 0x0c8 | 20 | `[u32; 5]` | `unk_c8` | unknown | 0 at creation; two of them are read as generic lists by the trim drawing code. |

### `Material` — `__SI_Material`, 0xdcc bytes

Size: SI_materialAllocate allocates 0xdcc bytes; the symbol SI_defaultMaterial is 0xdcc bytes. Read 1.8% · named 0.3% · unknown 97.9%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `CStr32` | `name` | read | Written first by si_materialBinaryWrite, as a string. |
| 0x004 | 26 | `[u8; 0x1a]` | `unk_004` | unknown | Not placed. |
| 0x01e | 2 | `i16` | `unk_01e` | unknown | Written by si_materialBinaryWrite as 16 bits. |
| 0x020 | 8 | `[u8; 8]` | `unk_020` | unknown | Not placed. |
| 0x028 | 2 | `i16` | `shading` | named | Second value written by si_materialBinaryWrite: the shading model. |
| 0x02a | 2 | `[u8; 2]` | `pad_02a` | unknown | Not placed. |
| 0x02c | 4 | `f32` | `unk_02c` | unknown | Third value written by si_materialBinaryWrite. |
| 0x030 | 4 | `[u8; 4]` | `unk_030` | unknown | Not placed. |
| 0x034 | 16 | `[f32; 4]` | `ambient` | read | Attributes 45 to 47 (MatAmbient) are the first three floats. |
| 0x044 | 16 | `[f32; 4]` | `diffuse` | read | Attributes 48 to 50 (MatDiffuse). |
| 0x054 | 16 | `[f32; 4]` | `specular` | read | Attributes 51 to 53 (MatSpecular). |
| 0x064 | 4 | `f32` | `spec_decay` | read | Attribute 54 (MatSpecDecay). |
| 0x068 | 1600 | `[u8; 0x640]` | `unk_068` | unknown | Not placed. |
| 0x6a8 | 4 | `f32` | `reflect` | read | Attribute 55 (MatReflect). |
| 0x6ac | 12 | `[u8; 0xc]` | `unk_6ac` | unknown | Not placed. |
| 0x6b8 | 4 | `f32` | `transp` | read | Attribute 57 (MatTransp). |
| 0x6bc | 32 | `[u8; 0x20]` | `unk_6bc` | unknown | Not placed. |
| 0x6dc | 4 | `f32` | `refract` | named | Written after reflectivity and transparency by si_materialBinaryWrite, where the file has the refraction index. |
| 0x6e0 | 1756 | `[u8; 0x6dc]` | `unk_6e0` | unknown | Not placed. |
| 0xdbc | 4 | `Ptr32<Opaque>` | `unk_dbc` | unknown | Tested by si_materialBinaryWrite. |
| 0xdc0 | 4 | `[u8; 4]` | `unk_dc0` | unknown | Not placed. |
| 0xdc4 | 4 | `Ptr32<Opaque>` | `user_data` | named | A user-data list tested by si_materialBinaryWrite. |
| 0xdc8 | 4 | `[i16; 2]` | `unk_dc8` | unknown | Written by si_materialBinaryWrite as two 16-bit values. |

### `LclMap` — `__SI_LclMap`, 0x1d8 bytes

Size: SI_textureAllocate allocates 0x1d8 bytes; the symbol SI_defaultMap is 0x1d8 bytes. Read 0.8% · named 0.0% · unknown 99.2%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 364 | `[u8; 0x16c]` | `unk_000` | unknown | Not placed. |
| 0x16c | 4 | `Ptr32<LclMap>` | `next` | read | Next 2D texture of the same model: SI_modelBinaryWriteNode and SI_modelDisposeNode walk the list through it. |
| 0x170 | 104 | `[u8; 0x68]` | `unk_170` | unknown | Not placed. |

### `Texture3D` — `__SI_Texture3D`, 0x158 bytes

Size: SI_texture3DAllocate allocates 0x158 bytes. Read 47.7% · named 2.3% · unknown 50.0%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 16 | `[u8; 0x10]` | `unk_000` | unknown | Not placed. |
| 0x010 | 4 | `f32` | `spacing` | read | Attribute 69 (T3DSpacing). |
| 0x014 | 4 | `[u8; 4]` | `unk_014` | unknown | Not placed. |
| 0x018 | 4 | `f32` | `strength` | read | Attribute 68 (T3DStrength). |
| 0x01c | 4 | `u32` | `iteration` | named | Attribute 405 (T3DIteration) reads this word and the one at +0x120. |
| 0x020 | 4 | `f32` | `power` | read | Attribute 70 (T3DPower). |
| 0x024 | 4 | `[u8; 4]` | `unk_024` | unknown | Not placed. |
| 0x028 | 4 | `f32` | `blend` | read | Attribute 155 (T3DBlend). |
| 0x02c | 4 | `[u8; 4]` | `unk_02c` | unknown | Not placed. |
| 0x030 | 80 | `[[f32; 4]; 5]` | `colors` | read | Attributes 76 to 90 and 156 to 160 (T3DColor0 to T3DColor4, red, green, blue, alpha). |
| 0x080 | 12 | `[f32; 3]` | `factors` | read | Attributes 73 to 75 (T3DFact1 to T3DFact3). |
| 0x08c | 4 | `[u8; 4]` | `unk_08c` | unknown | Not placed. |
| 0x090 | 12 | `Vector` | `scale` | read | Attributes 91 to 93 (T3DScale). |
| 0x09c | 12 | `Vector` | `rotation` | read | Attributes 94 to 96 (T3DRotation). |
| 0x0a8 | 12 | `Vector` | `transl` | read | Attributes 97 to 99 (T3DTransl). |
| 0x0b4 | 84 | `[u8; 0x54]` | `unk_0b4` | unknown | Not placed. |
| 0x108 | 4 | `f32` | `ambient` | read | Attribute 100 (T3DAmbient). |
| 0x10c | 4 | `f32` | `diffuse` | read | Attribute 101 (T3DDiffuse). |
| 0x110 | 4 | `f32` | `specular` | read | Attribute 102 (T3DSpecular). |
| 0x114 | 4 | `f32` | `reflection` | read | Attribute 103 (T3DReflection). |
| 0x118 | 4 | `f32` | `transp` | read | Attribute 104 (T3DTransp). |
| 0x11c | 4 | `[u8; 4]` | `unk_11c` | unknown | Not placed. |
| 0x120 | 4 | `u32` | `iteration2` | named | Read with iteration for attribute 405. |
| 0x124 | 52 | `[u8; 0x34]` | `unk_124` | unknown | Not placed. |

### `Camera` — `SI_Camera`, 0x13c bytes

Size: SI_newCamera allocates 0x13c bytes; the symbol si_paintCamera is 0x13c bytes. Read 0.0% · named 17.7% · unknown 82.3%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 4 | `CStr32` | `name` | named | Read as a string by three functions; freed by cameraFree. |
| 0x004 | 12 | `[u8; 0xc]` | `unk_004` | unknown | Five 16-bit values written by SI_cameraTransfer; not placed. |
| 0x010 | 12 | `Vector` | `position` | named | SI_addCamTrans adds a translation to these three floats. |
| 0x01c | 12 | `Vector` | `interest` | named | SI_addCitTrans adds a translation to these three floats. |
| 0x028 | 248 | `[u8; 0xf8]` | `unk_028` | unknown | Not placed. |
| 0x120 | 24 | `[f32; 6]` | `frustum` | named | SI_camSetFrustum and SI_camGetFrustum. |
| 0x138 | 4 | `Ptr32<Opaque>` | `user_data` | named | Passed by address to the camera's binary reader as a user-data list. |

### `Light` — `light`, 0x240 bytes

Size: SI_lightAllocate allocates 0x240 bytes; the symbol SI_defaultLight is 0x240 bytes. Read 0.3% · named 11.5% · unknown 88.2%.

| Offset | Size | Type | Field | Confidence | Evidence |
|---|---|---|---|---|---|
| 0x000 | 30 | `[u8; 0x1e]` | `name` | named | An inline string: si_lightInitialize copies the eight bytes of the text default here; 30 bytes is the room before kind. |
| 0x01e | 2 | `i16` | `kind` | read | The second argument of si_lightInitialize, which chooses defaults for 1, 2, 3 and 4: the resource file's LigType (point, spot, infinite, sun). |
| 0x020 | 4 | `[u8; 4]` | `unk_020` | unknown | Not placed. |
| 0x024 | 12 | `[f32; 3]` | `color` | named | 1, 1, 1 at initialisation; SI_lightSetParam writes it. |
| 0x030 | 4 | `[u8; 4]` | `unk_030` | unknown | 0 at initialisation. |
| 0x034 | 12 | `Vector` | `position` | named | SI_addLigTrans adds a translation to these three floats. |
| 0x040 | 24 | `[u8; 0x18]` | `unk_040` | unknown | Six floats, 0 at initialisation. |
| 0x058 | 12 | `Vector` | `interest` | named | SI_addLitTrans adds a translation to these three floats. |
| 0x064 | 476 | `[u8; 0x1dc]` | `unk_064` | unknown | Not placed. |
