# 0027 — The graph list

Date: 2026-10-13. Binary: Linux `soft`, sha256 `ef21a382c3325d42…`. Package: `crates/si-view`
(new), `re/tools/si-view/`, scratch in `re/out/si-view/`. Source files of the original:
`graflist.c` (0x0832bfd0 to 0x08339a6e, 119 functions, 55,771 bytes; this pass),
`graphsort.c` (31), `nselections.c` (16), `displayMode.c` (25): 191 functions, about 79 KB,
three passes planned. Sections 1 to 9 are the first pass; **section 10 is the second**
(2026-10-13, the same day): 99 of the 191 functions are on the image now, with three
routines of `db2scn.c` that build the list from a loaded scene.

The graph list is the list of scene nodes every view draws from and the render commands
walk: one node per model node, camera, light, wave, material and texture. The render set-up
(finding 0023, 17.5) had to make a list up because nothing built one. This pass puts the
list itself on the shared memory image (finding 0024): the array of lists, the nodes, their
order, the look-ups, selection state and display type.

How to read this note: a statement about the original is **read from code** (the decompiled
text and, for every function ported, the instruction listing) and, for every function with
case counts in section 4, **measured**: the function was stepped through in the emulation
harness (finding 0011) on made-up scenes and the port compared with what it left in memory.
*[name]* marks a meaning taken from a symbol; *[guess]* a guess. Section 3's account of the
scene loader is read from the decompiled text of a routine of another package and is not
measured.

## Results at a glance

| | |
|---|---|
| `graflist.c` mapped | all 119 functions: what each does, its callers, its kind (section 4) |
| Functions on the image, checked and cited | **72 of 119**, 12,936 bytes of the original (of 55,967 counted to the next symbol): every routine of list keeping but four, every query but one, all of selection and display type; none of drawing and picking (section 7) |
| Emulation, `view-emucheck` | 72 routines, **83,625 cases: 80,047 identical, 0 different**, 3,578 not compared: the original stopped on a wild read or write the made-up scene asks for (20 of them ran out of the instruction budget) |
| What "identical" covers per case | the return value; every buffer of the case, changed or not; every block allocated, with address, size and every byte the original wrote, and no byte written that the original left alone; the blocks freed, in order; every call to a routine outside the crate, with arguments, in order; and that nothing else in the image changed |
| Mutation switches | `EMUCHECK_MUTATE=1` (a bit of the result and of the first byte changed): 73,555 of the 80,047 compared cases differ; the other 6,492 return nothing and change nothing. `EMUCHECK_MUTATE=calls` (a bit of the first argument of every outside call): 41,200 differ, every case in which the port calls out with an argument |
| Instructions the cases reached | 4,367 instructions in the 72 routines; **18 not reached** (the answers to "no memory", which the harness's allocator never gives, and one instruction of dead code), and 147 more that only the reduced product reaches (behind `SI_lightLicense`, left out of the port) |
| Globals declared and held against the binary | 20 (`check_globals.py`: 0 wrong) |
| Outside routines named, held against the symbol table | 102 (`check_outside.py`: 0 wrong, with every citation and every mention of an address in the crate) |
| Unit tests | 3 (made-up data; one walks an object into the list, cuts and links a branch and holds the chain's order) |
| **After the second pass** (section 10) | **102 routines cited** (78 of `graflist.c`, 10 of `nselections.c`, 11 of `graphsort.c`, 3 of `db2scn.c`): **130,325 cases, 125,605 identical, 0 different**, 4,720 not compared; the rows above are the first pass's and came out again unchanged |

## 1. The crate

`crates/si-view` depends on `si-mem`, `si-types` (the records `GrafNode`, `GrafList`,
`Model`, `Elem`) and `si-x87` (the vector reader of the check binary, and the x87 register
for two float computations). Every function takes the image and the original's arguments;
`SI_malloc`, `SI_realloc`, `strdup` and `free` are made with the original's sizes in the
original's order. There is no store of the crate's own.

| Module | Functions | Content |
|---|---:|---|
| `graflist::list` | 26 | the array of lists; a node's creation, insertion, removal; the walks; the look-ups by object and by name; the hit distance |
| `graflist::flags` | 24 | the selection tests and counts; display type, bounding box and wire-frame switches behind the display menus; selectability |
| `graflist::select` | 4 | `SI_selectGrafnode`, `SI_unselectGrafnode`, `SI_toggleGrafnode`, `SI_graflistSelectData` |
| `graflist::events` | 6 | the selection commands of the menus on the scene's own list; `SI_graflistDeleteSelection` |
| `graflist::edit` | 12 | `SI_graflistAddData`, `SI_graflistLinkModel`, `SI_graflistCutModel`; cutting an object's connections; disposal; `SI_graflistDeleteGrafnode` and the quick-stretch controls |
| `globals`, `records` | | the globals at their addresses; the process record; names for flag bits, selection states and kinds |
| `outside` | | the one trait to everything that is not in the crate (section 8) |

Left out with the project's standing reason: **`SI_lightLicense` (0x08679320)**, the
product-tier flag. Five routines read it (`SI_selectGrafnode`, `SI_toggleGrafnode`,
`SI_toggleSelect`, `SI_graflistLinkModel`, `SI_graflistCutModel`); in each the port takes the
path of the flag being 0, a comment at the spot says so, and the flag is neither declared nor
read. What the reduced product does there is described in section 5 from the listing, not
ported and not emulated (the cases leave the flag as the binary has it: 0).

## 2. Records and globals

### 2.1 The graph node (`__SI_GrafNode`, 0x24 bytes)

`si_types::scene::GrafNode` has the layout; what this pass establishes about the fields,
each read from the code of the routines named and reached by their cases:

| Offset | Field | As the code uses it |
|---|---|---|
| +0x00 | kind | 2 a model's root node, 3 a model node below a root, 4 a camera, 5 the node in front of a camera, 6 a light, 7 the node in front of a light, 8, 0x13, 0x14 lights that have one node only (by the light's own type word: 1, 3, 4), 9 a curve (`SI_grafnodeDispose` hands it to `SI_fcurveRemove`), 0xb a wave, 0xc a named selection (handed to `SI_nselectionSelectContent`), 0xd made by the loader without an object, 0xe a material, 0xf a 2D texture, 0x10 a 3D texture, 0x12 an animation file (`SI_edDisposeAnimFile`), 1 a spline whose points `si_grafnodeTraversePnts` walks *[the last from the drawing code, not ported]*; 0xa is refused by `SI_selectGrafnode` |
| +0x04 | object | what the node stands for. Kinds 2 and 3: a `__SI_Model`. Kinds 6, 7, 8, 0x13, 0x14: a light, whose record begins with its name. Kinds 4, 5: the camera (both nodes the same) |
| +0x08 | selection | 0 none, 1 as a node, 3 as a branch, 2 inherited from a branch above |
| +0x0c | display type | 0 hidden, 1 ghosted, 2 visible (2 at creation). `SI_graflistToggleGhostSelection` computes 3 minus its low 16 bits |
| +0x10 | count | 16 bits, unsigned in every walk: how many nodes after this one belong to it. A model node: its descendants. The node in front of a camera or light: 1 |
| +0x12 | wire colour | `SI_select7WM` + `SI_color1` at creation, the root's for the nodes of its hierarchy |
| +0x14 | | 0 at creation; not touched by any routine ported |
| +0x16 | flags | 0x0001 not selectable (`SI_setSelectability`); 0x0800 tested with a manipulation global before `SI_setUItoPrefFlags`; 0x1000 not editable: the list keeps such a node in its second "active" place, and the reduced product keeps its selection in 0x2000 (node), 0x4000 (branch), 0x8000 (inherited) instead of +0x08. The display menus and counts look at those three bits in the full product too |
| +0x18 | function | not touched by any routine ported (`SI_adjustGnFunc`, `SI_updateGnFunc` set it) |
| +0x1c, +0x20 | next, previous | the chain |

### 2.2 The list (`SI_GrafList`, 0x24 bytes)

| Offset | As the code uses it |
|---|---|
| +0x00 | 16 bits: 0 single selection, else multiple (`SI_graflistSetMultiSelection`, `SI_graflistCheckMulti`) |
| +0x02 | two bytes no routine writes (`grafListInit` leaves them) |
| +0x04 | head of the chain |
| +0x08 | the active node; +0x20 the active node when it is not editable. Setting one clears the other (`SI_graflistSetActive`) |
| +0x0c, +0x0e | 16-bit words, 0 and 1 at creation; not read by any routine ported |
| +0x10 … +0x1c | four words cleared at creation; `graflistDelete` clears +0x10 again; +0x14 and +0x1c are what the render commands hand to `SI_image` (finding 0023) |

Lists live in one array, `*SI_grafLists`, 0x24 bytes each, grown by one record per
`SI_grafListAdd` (`SI_realloc` to `(n + 1) × 0x24`; the flags array `grafListExists` to
`(n + 1) × 2`). A view names its list by index (`__SI_Grafport` +0x3c, read by the drawing
code); **the scene's list is the one whose index is the second word of the process record**.
`SI_grafListRemove` clears a list and drops its flag but never shrinks the array.

### 2.3 The process record and the globals

`*SI_procData` (`SI_procDataStorage`, 12 bytes): +0 the index of the current element
dictionary, +4 the index of the scene's graph list, +8 the scene's camera.

| Global | Address | Use here |
|---|---|---|
| `SI_grafLists`, `SI_nbGrafLists`, `grafListExists` | 0x08679498, 0x08679494, 0x086bae64 | the array of lists, its length, a 16-bit flag per list |
| `SI_procData` | 0x08679314 | the process record (points to 0x08782108) |
| `schemNoRefresh`, `SI_schemSlideItem` | 0x086bae60, 0x086bb424 | "do not lay the schematic view out now" and its opposite (1 in the binary) |
| `SI_select7WM`, `SI_color1` | 0x08781638, 0x08781354 | their sum is the default wire colour |
| `lastSelected`, `multipleSelected` | 0x086bae68, 0x086bae6c | whether a second node was selected since the status line last looked |
| `hitSize` | 0x086bacc0 | squared hit distance, 0x32 in the binary |
| `SI_scngl` | 0x08695e9c | a pointer to the scene's list kept beside the array (`SI_graflistCheckMulti` writes both) |
| `SI_elemDicts`, `SI_cid` | 0x0867931c, 0x08781660 | the current dictionary's address is handed to the editing layer; rows 6 (lights), 8 (models) and 9 (the chapter whose elements stand for others through their first forward list: named selections) are read |
| `SI_glbManip` | 0x08781560 | +0x18 a node the manipulation code refers to, +0x08, +0x0c, +0x14 mode words stepped back when that node is deleted, +0x1c "clicks select plain nodes in multiple selection", +0x32 "tell the preference flags" |
| `SI_glbSchem` | 0x087fb960 | +0x00 layout direction (0, 1), +0x3c, +0x3e default colour of new nodes, +0x48 a schematic view is laid out |
| `SI_uniqueNameFlag`, `SI_edSumatraTo3x`, `SI_historyActive`, `SI_standaloneRenderer` | 0x08679448, 0x08680e50, 0x0867965c, 0x08679400 | handed on or tested as switches |

## 3. How a node comes to be in the list, and in what order

**The chain's order carries the hierarchy.** A root node is followed at once by the nodes of
its hierarchy, depth first, and its count says how many they are; a count of a node inside
the hierarchy covers its own descendants. Nothing else records which node hangs under which
(`si_graflistFindParent` recovers the parent from the counts alone).

1. **Every object enters through `SI_graflistAddData`, at the head of the chain.** So the
   chain is, group by group, the reverse of the order of arrival.
2. **A model** arrives as its root (kind 2); the routine walks the model tree with
   `SI_modelNextNode` (the son, else the brother, else the nearest brother of a father) and
   chains one node of kind 3 per model node behind the one before. `SI_modelNextNode` does
   not know where the hierarchy ends: a root that has a brother link, or a node that is not
   a root, is followed into its brothers *[read from that routine's listing; it belongs to
   `model_b.c`]*.
3. **A camera or a light** arrives as two nodes, the node in front (kind 5 or 7, count 1)
   and the object's own (4 or 6) behind it. Lights of the other three types, waves,
   materials and textures are one node; a material or texture the editing layer refuses is
   taken out again and freed.
4. **Changing the hierarchy moves the branch**: `SI_graflistLinkModel` takes a root with
   its nodes out of the chain and puts it right behind the new parent's node, as kind 3,
   and raises the count of every ancestor; `SI_graflistCutModel` puts a branch at the head
   as kind 2 and lowers them.
5. **When a scene is emptied** `graflistDelete` deletes every node and then adds the
   scene's camera again, so the camera's two nodes are the chain's tail from then on.
   **When a scene is loaded**, `SI_graflistBuildFromElemDict (0x08109ec8)` (`db2scn.c`;
   ported and measured in the second pass: section 10.3 has the whole account) goes
   through the dictionary chapter by
   chapter and element by element in index order and, for each new element, adds a model
   whose father is null with kind 2 (its hierarchy follows as in 2), a material with 0xe, a
   texture with 0xf or 0x10, a named selection with 0xc and an animation file with 0x12;
   cameras and lights are not added there but looked up (`SI_findGlData`): they entered
   the list when they were read.

So for a loaded scene the chain is: the last root model of the MODELS chapter first, each
root followed by its hierarchy depth first, the first root last; around them the other
kinds by the same rule; the camera at the tail. That agrees with what finding 0023 (16.3)
read out of the render commands ("the chain's head is the last root of the list").

**What the render commands may call now.** `si_view::graflist`:
`graf_list_create(m)` once; `scene_list(m)` for the scene's list;
`graflist_add_data(m, o, list, object, kind, select)` for each object in the loader's
order; then the walks they already make by hand (`m[list].first`, `m[node].next`) or
`next_grafnode`, and `find_gl_data`. The `Outside` they hand in must answer two routines
truthfully, `SI_modelNextNode (0x08478340)` and `SI_modelNbSubModels (0x0846d5ec)` (`si-model`'s
image module of today, `model_image/tree.rs`, names both; it was not read for this note), and may answer 0 to the rest for a list
that only has to be walked (the editing layer's `SI_edAdd…` routines give the nodes their
elements; `SI_edAddMaterial` and the two texture routines must answer non-zero or the node
is refused). The unit test `objects_enter_at_the_head_and_hierarchies_stay_together` is
such a use.

## 4. The map of `graflist.c`

Kinds: *keeping* (the list and its nodes: making, chaining, deleting), *query*, *selection*,
*visibility* (display type, bounding box, wire frame: the display menus), *drawing*,
*interface* (picking with the mouse, projections for hit tests). Callers are counted from
`re/out/linux/soft.calls.tsv`; "own file" are those inside `graflist.c`. The table is
printed by `re/tools/si-view/vmap.py` from `graflist_map.tsv` (our descriptions) and the
check's report.

| Address | Function | Bytes | Callers (own file) | Kind | What it does | Checked |
|---|---|---:|---:|---|---|---|
| 0x0832bfd0 | `SI_schematicNoRefresh` | 36 | 9 (2) | keeping | sets the switch that stops the schematic view's own layout while nodes come and go, and its opposite | 60 cases, 60 identical, 0 different |
| 0x0832bff4 | `SI_grafnodeDefColor` | 24 | 3 (0) | query | the wire colour a new node gets: the sum of two 16-bit globals | 60 cases, 60 identical, 0 different |
| 0x0832c00c | `SI_grafnodeGetName` | 80 | 2 (0) | query | the name of what a node stands for, by kind | 300 cases, 300 identical, 0 different |
| 0x0832c05c | `SI_grafnodeChangeName` | 152 | 0 (0) | keeping | renames what a node stands for (model, camera); for a wave it writes through a null pointer | 400 cases, 336 identical, 0 different |
| 0x0832c0f4 | `si_graflistFindParent` | 56 | 0 (0) | query | the node a node hangs under, going by the chain and the counts alone | 2000 cases, 2000 identical, 0 different |
| 0x0832c12c | `SI_graflistFindRoot` | 24 | 2 (1) | query | the root node (kind 2) of a model node, backwards along the chain | 1500 cases, 1451 identical, 0 different |
| 0x0832c144 | `SI_grafListExists` | 44 | 5 (0) | query | whether a list index is inside the array and flagged | 300 cases, 300 identical, 0 different |
| 0x0832c170 | `grafListInit` | 76 | 2 (2) | keeping | an empty list record | 20 cases, 20 identical, 0 different |
| 0x0832c1bc | `SI_grafListCreate` | 72 | 4 (0) | keeping | makes the array of lists with its first list, once | 200 cases, 200 identical, 0 different |
| 0x0832c204 | `SI_grafListAdd` | 152 | 3 (0) | keeping | one more list at the end of the array | 500 cases, 500 identical, 0 different |
| 0x0832c29c | `SI_grafListAllocate` | 40 | 1 (1) | keeping | a list record from SI_malloc, initialised | 20 cases, 20 identical, 0 different |
| 0x0832c2c4 | `SI_grafListRemove` | 92 | 1 (0) | keeping | clears a list that is not the current one and drops its flag | 1500 cases, 1462 identical, 0 different |
| 0x0832c320 | `SI_graflistClear` | 76 | 2 (1) | keeping | frees the nodes of a list (not their objects) | 500 cases, 500 identical, 0 different |
| 0x0832c36c | `graflistDelete` | 284 | 1 (1) | keeping | deletes every node with its object, model nodes first, then puts the camera back into the list | 1500 cases, 1379 identical, 0 different |
| 0x0832c488 | `SI_graflistSetActive` | 56 | 8 (6) | keeping | stores the list's active node, in the second place when it is not editable | 800 cases, 800 identical, 0 different |
| 0x0832c4c0 | `removeChapterData` | 272 | 1 (1) | keeping | disposes of the objects of one dictionary chapter (curves, expressions, animation, user functions) | 1500 cases, 1479 identical, 0 different |
| 0x0832c5d0 | `SI_sceneDeleteAllNotInHistory` | 704 | 1 (0) | keeping | empties the scene: chapter data, elements, the graph list, then the default description | 1500 cases, 1426 identical, 0 different |
| 0x0832c890 | `grafnodeAllocate` | 96 | 1 (1) | keeping | a node from SI_malloc: all zero, display type 2 | 20 cases, 20 identical, 0 different |
| 0x0832c8f0 | `graflistInsertGrafnode` | 60 | 1 (1) | keeping | chains a node behind another or at the head | 1500 cases, 1500 identical, 0 different |
| 0x0832c92c | `SI_grafnodeCutConnections` | 128 | 8 (1) | keeping | removes animation, expressions, channels, constraints, path and curves of a node's object | 1000 cases, 1000 identical, 0 different |
| 0x0832c9ac | `SI_graflistRemoveGrafnode` | 116 | 15 (2) | keeping | takes a node out of the chain and flags its element deleted | 2000 cases, 2000 identical, 0 different |
| 0x0832ca20 | `pathRelaxFromData` | 52 | 1 (1) | keeping | deletes the path of a node whose model is one | 300 cases, 300 identical, 0 different |
| 0x0832ca54 | `SI_grafnodeDispose` | 120 | 5 (0) | keeping | disposes of what a node stands for, by kind | 600 cases, 600 identical, 0 different |
| 0x0832cacc | `sceneGraflistRemoveFcurves` | 140 | 1 (1) | keeping | takes the curves of a model or light out of the scene's curve lists | 500 cases, 500 identical, 0 different |
| 0x0832cb58 | `grafnodeDeleteAnimation` | 88 | 1 (1) | keeping | removes the animation of a node's element | 300 cases, 300 identical, 0 different |
| 0x0832cbb0 | `grafnodeDeleteExpressions` | 120 | 1 (1) | keeping | removes the expressions on and through a node's element | 300 cases, 300 identical, 0 different |
| 0x0832cc28 | `deleteQStretchCtrls` | 296 | 1 (1) | keeping | deletes the two control nodes a quick-stretch relation (0xee) hangs on a model | 1500 cases, 1351 identical, 0 different |
| 0x0832cd50 | `SI_graflistDeleteGrafnode` | 1264 | 21 (3) | keeping | deletes a node and the nodes below it with their objects and every relation, by kind | 4000 cases, 3881 identical, 0 different |
| 0x0832d240 | `SI_graflistAddData` | 1236 | 127 (1) | keeping | puts an object into a list: its node at the head, a hierarchy's nodes behind it | 4000 cases, 3953 identical, 0 different |
| 0x0832d714 | `SI_graflistSelectData` | 72 | 2 (0) | selection | selects the node of an object | 800 cases, 769 identical, 0 different |
| 0x0832d75c | `SI_graflistDeleteNode` | 952 | 3 (1) | keeping | the Delete command for one node: its geometry only, or the node cut out of its hierarchy and deleted | 3000 cases, 2880 identical, 0 different |
| 0x0832db14 | `SI_graflistDeleteSelection` | 48 | 2 (0) | keeping | deletes a selected node, as a branch when it is selected as one | 300 cases, 300 identical, 0 different |
| 0x0832db44 | `SI_grafNodeIsSelected` | 40 | 8 (7) | query | selection word 1 or 3 | 200 cases, 200 identical, 0 different |
| 0x0832db6c | `SI_grafNodeIsWrathSelected` | 36 | 8 (7) | query | the same from the flags of a node that is not editable | 200 cases, 200 identical, 0 different |
| 0x0832db90 | `SI_graflistCountSelected` | 124 | 34 (1) | query | number of selected nodes, and the first | 2000 cases, 2000 identical, 0 different |
| 0x0832dc0c | `SI_graflistCountSelHierarchies` | 104 | 3 (0) | query | number of hierarchies with something selected | 2000 cases, 1960 identical, 0 different |
| 0x0832dc74 | `SI_graflistTypeCount` | 132 | 28 (0) | query | number of selected nodes of one or two kinds, and the last | 2000 cases, 2000 identical, 0 different |
| 0x0832dcf8 | `graflistAllVisible` | 40 | 0 (0) | visibility | display type 2 for every node | 300 cases, 300 identical, 0 different |
| 0x0832dd20 | `SI_graflistUnselectAll` | 80 | 9 (1) | selection | clears every node's selection and the list's active node | 500 cases, 500 identical, 0 different |
| 0x0832dd70 | `SI_graflistSetMultiSelection` | 132 | 14 (2) | selection | switches a list between single and multiple selection | 2000 cases, 2000 identical, 0 different |
| 0x0832ddf4 | `SI_graflistUnghostAll` | 44 | 4 (0) | visibility | ghosted nodes become visible | 600 cases, 600 identical, 0 different |
| 0x0832de20 | `SI_graflistToggleGhostSelection` | 68 | 4 (0) | visibility | selected nodes swap between ghosted and visible | 1500 cases, 1500 identical, 0 different |
| 0x0832de64 | `SI_graflistGhostUnselected` | 56 | 4 (0) | visibility | visible nodes that are not selected become ghosts | 1500 cases, 1500 identical, 0 different |
| 0x0832de9c | `SI_graflistGhostHidden` | 52 | 4 (0) | visibility | hidden nodes become ghosts | 600 cases, 600 identical, 0 different |
| 0x0832ded0 | `SI_unboundGlAll` | 76 | 4 (0) | visibility | no model drawn as its bounding box | 500 cases, 500 identical, 0 different |
| 0x0832df1c | `instanceComputeBoundingBox` | 124 | 2 (2) | visibility | bounding boxes of a hierarchy, masters of instances first | 1000 cases, 1000 identical, 0 different |
| 0x0832df98 | `SI_toggleBoundGlSelected` | 124 | 4 (0) | visibility | flips "drawn as bounding box" for the selected models | 1000 cases, 1000 identical, 0 different |
| 0x0832e014 | `SI_boundGlAll` | 76 | 4 (0) | visibility | every model drawn as its bounding box | 500 cases, 500 identical, 0 different |
| 0x0832e060 | `SI_unhideGlAll` | 88 | 4 (0) | visibility | shows every hidden node, through its element | 800 cases, 800 identical, 0 different |
| 0x0832e0b8 | `SI_toggleHideGlSelected` | 400 | 4 (0) | visibility | hides or shows the selected nodes, and unselects what it hid | 2000 cases, 1822 identical, 0 different |
| 0x0832e248 | `SI_graflistSetForceWireframeAll` | 64 | 4 (0) | visibility | sets "always wire frame" on every model | 500 cases, 500 identical, 0 different |
| 0x0832e288 | `SI_graflistResetForceWireframeAll` | 64 | 4 (0) | visibility | clears it | 500 cases, 500 identical, 0 different |
| 0x0832e2c8 | `SI_graflistToggleForceWireframeSelected` | 64 | 4 (0) | visibility | flips it for the selected models | 800 cases, 800 identical, 0 different |
| 0x0832e308 | `SI_hideGlUnselected` | 100 | 4 (0) | visibility | hides every node that is not selected | 800 cases, 800 identical, 0 different |
| 0x0832e36c | `SI_hideGlGhosted` | 164 | 4 (0) | visibility | hides every ghosted node and unselects everything | 1000 cases, 661 identical, 0 different |
| 0x0832e410 | `SI_setSelectability` | 516 | 4 (0) | selection | sets, clears or flips "not selectable" on models, cameras, lights, waves, named selections | 2500 cases, 2500 identical, 0 different |
| 0x0832e614 | `SI_drawGraflist` | 148 | 5 (0) | drawing | draws a view's list, unselected and selected nodes in one or two passes |  |
| 0x0832e6a8 | `drawGraflist` | 1984 | 1 (1) | drawing | one pass over the list: write masks, grid, each visible node, axes |  |
| 0x0832ee68 | `SI_findGlData` | 76 | 140 (10) | query | the node of an object, by object address and kind | 2000 cases, 2000 identical, 0 different |
| 0x0832eeb4 | `SI_stateGlData` | 40 | 3 (0) | query | the selection word of an object's node | 800 cases, 800 identical, 0 different |
| 0x0832eedc | `SI_autoGlSelect` | 48 | 3 (0) | interface | rectangle selection in a view that is not the dope sheet |  |
| 0x0832ef0c | `SI_gLSelect` | 28 | 51 (1) | interface | the node under the mouse |  |
| 0x0832ef28 | `SI_gLSelectCheckNSE` | 28 | 17 (0) | interface | the same, nodes that are not editable included |  |
| 0x0832ef44 | `SI_gLSelectSelected` | 1060 | 6 (2) | interface | picks the nearest node under the mouse by drawing the list in selection mode |  |
| 0x0832f368 | `computeWaveMatrix` | 84 | 2 (2) | drawing | a wave's matrix: its icon's, times the model's it is attached to |  |
| 0x0832f3bc | `twoConesIconDraw` | 680 | 2 (2) | drawing | the two-cone icon of a constraint |  |
| 0x0832f664 | `SI_jointIconPositionsGet` | 480 | 4 (2) | drawing | the two points a joint's constraint icons are drawn at |  |
| 0x0832f844 | `prefAxisConstrDraw` | 1028 | 1 (1) | drawing | the preferred-axis constraint of a joint |  |
| 0x0832fc48 | `upVectorConstrDraw` | 964 | 1 (1) | drawing | the up-vector constraint of a joint |  |
| 0x0833000c | `directionConstrDraw` | 1092 | 1 (1) | drawing | the direction constraint of a model |  |
| 0x08330450 | `SI_setAreaRect` | 336 | 5 (1) | drawing | the four corners of a rectangular area light |  |
| 0x083305a0 | `SI_setAreaDisk` | 320 | 5 (1) | drawing | centre and normal of a disc area light |  |
| 0x083306e0 | `si_drawAreaRect` | 260 | 1 (1) | drawing | draws a rectangular area light |  |
| 0x083307e4 | `si_drawAreaSphere` | 260 | 1 (1) | drawing | draws a spherical area light |  |
| 0x083308e8 | `si_drawAreaDisk` | 360 | 1 (1) | drawing | draws a disc area light |  |
| 0x08330a50 | `si_drawGrafnode` | 4596 | 9 (6) | drawing | draws one node by kind: model geometry, camera, light, wave, with its icons and constraints |  |
| 0x08331c44 | `drawLightCone` | 1304 | 1 (1) | drawing | the cone of a spot light |  |
| 0x0833215c | `si_grafnodeTraversePnts` | 4480 | 4 (1) | drawing | hands every point of a node's object to a callback (auto-zoom, framing) |  |
| 0x083332dc | `splineTraversePnts` | 1004 | 1 (1) | drawing | the same for a spline's points |  |
| 0x083336c8 | `bboxTraversePnts` | 184 | 1 (1) | drawing | the eight corners of a box |  |
| 0x08333780 | `nullTraversePnts` | 176 | 1 (1) | drawing | the six points of a null's icon |  |
| 0x08333830 | `SI_unselectGrafnode` | 328 | 18 (7) | selection | unselects a node, or its branch, or its skeleton chain | 3000 cases, 2522 identical, 0 different |
| 0x08333978 | `SI_selectedString` | 1152 | 1 (0) | interface | the text of the status line: what is selected | 2500 cases, 2364 identical, 0 different |
| 0x08333df8 | `SI_selectGrafnode` | 1468 | 30 (5) | selection | selects a node, its branch, its model or its chain | 4000 cases, 3723 identical, 0 different |
| 0x083343b4 | `SI_toggleGrafnode` | 440 | 4 (1) | selection | selects a node that is not selected, unselects one that is | 3000 cases, 2808 identical, 0 different |
| 0x0833456c | `SI_vectorDistancectCam` | 312 | 3 (0) | query | distance of a point from a camera, along its view direction or straight | 2000 cases, 2000 identical, 0 different |
| 0x083346a4 | `SI_initProj` | 72 | 4 (0) | interface | projects the active node's points for hit tests |  |
| 0x083346ec | `SI_hitProj` | 36 | 4 (0) | interface | the projected point under a position |  |
| 0x08334710 | `SI_hitProjDst` | 360 | 10 (2) | interface | the same with the squared distance |  |
| 0x08334878 | `SI_screenToWorld` | 1344 | 21 (0) | interface | a screen position as a point in space |  |
| 0x08334db8 | `si_clearAll` | 328 | 4 (0) | drawing | clears a view through the object planes' write mask |  |
| 0x08334f00 | `clearUnselect` | 308 | 0 (0) | drawing | clears the unselected planes |  |
| 0x08335034 | `SI_clearSelect` | 12 | 6 (1) | drawing | nothing: answers 0 | 5 cases, 5 identical, 0 different |
| 0x08335040 | `SI_myclearSelect` | 124 | 12 (0) | drawing | walks the active view windows around that empty routine | 600 cases, 600 identical, 0 different |
| 0x083350bc | `SI_graflistHitSelectionOnGrafNode` | 712 | 8 (1) | interface | which part of a node (patch, curve, edge) is under a position |  |
| 0x08335384 | `SI_graflistHitSelection` | 80 | 2 (0) | interface | the same for the node under the mouse |  |
| 0x083353d4 | `SI_nextGrafnode` | 40 | 11 (0) | query | the walk over a list with a cursor | 600 cases, 600 identical, 0 different |
| 0x083353fc | `SI_graflistLinkModel` | 560 | 7 (0) | keeping | moves a root's branch behind its new father in the chain | 3000 cases, 2683 identical, 0 different |
| 0x0833562c | `SI_graflistCutModel` | 408 | 7 (2) | keeping | moves a branch to the head of the chain as a root | 2500 cases, 2262 identical, 0 different |
| 0x083357c4 | `SI_graflistHiliteData` | 552 | 4 (0) | drawing | draws an object's node highlighted into the front buffer |  |
| 0x083359ec | `SI_grafNodeNbHilite` | 516 | 0 (0) | drawing | the same for an array of nodes |  |
| 0x08335bf0 | `SI_getHitSize` | 20 | 1 (0) | query | the squared hit distance | 20 cases, 20 identical, 0 different |
| 0x08335c04 | `SI_setHitSize` | 20 | 6 (0) | query | sets it | 20 cases, 20 identical, 0 different |
| 0x08335c18 | `SI_grafnodeInitProjection` | 3788 | 17 (2) | interface | projects every point of a node into the view's hit table |  |
| 0x08336ae4 | `SI_splineHitSub` | 808 | 3 (0) | interface | which sub-curve of the active node is under the mouse |  |
| 0x08336e0c | `graflistCheckUniqueName` | 208 | 0 (0) | query | whether no other node of a kind carries a name | 2000 cases, 2000 identical, 0 different |
| 0x08336edc | `SI_itemGetByName` | 208 | 0 (0) | query | the object of a kind with a name | 2000 cases, 1839 identical, 0 different |
| 0x08336fac | `SI_grafnodeGetByName` | 196 | 0 (0) | query | the node of a kind with a name | 2000 cases, 1827 identical, 0 different |
| 0x08337070 | `rectSelect` | 812 | 1 (1) | interface | selects or toggles every node drawn inside a rectangle |  |
| 0x0833739c | `setPickViewport` | 852 | 2 (2) | interface | viewport and projection for picking inside a rectangle |  |
| 0x083376f0 | `SI_allSelect` | 244 | 4 (0) | selection | the Select All command | 2000 cases, 1976 identical, 0 different |
| 0x083377e4 | `SI_clearSelection` | 124 | 7 (0) | selection | the Clear Selection command | 1500 cases, 836 identical, 0 different |
| 0x08337860 | `SI_graflistCheckMulti` | 332 | 2 (0) | selection | sets single or multiple selection by what is selected (after a load) | 1500 cases, 1500 identical, 0 different |
| 0x083379ac | `SI_toggleSelect` | 332 | 4 (0) | selection | the Toggle Selection command | 2000 cases, 2000 identical, 0 different |
| 0x08337af8 | `SI_graflistHitGrafnone` | 424 | 1 (0) | interface | which sphere of a metaball system is under the mouse |  |
| 0x08337ca0 | `SI_getNearestCenterProjection` | 1336 | 1 (0) | interface | the node whose centre projects nearest to a position |  |
| 0x083381d8 | `SI_getNearestPointProjection` | 5060 | 8 (0) | interface | the nearest projected point of the nodes of a list |  |
| 0x0833959c | `modelUnlinkAllLight` | 172 | 1 (1) | keeping | clears a model's slot in the selective-light arrays of the lights that name it | 500 cases, 500 identical, 0 different |
| 0x08339648 | `meshGetPickedEdge` | 1063 | 1 (1) | interface | which edge of a mesh is under a position |  |

| Kind | Functions | Bytes | Ported and checked |
|---|---:|---:|---:|
| keeping | 29 | 7868 | 29 |
| query | 19 | 1784 | 19 |
| selection | 11 | 4068 | 11 |
| visibility | 16 | 1604 | 16 |
| drawing | 25 | 21580 | 2 |
| interface | 19 | 19063 | 1 |
| all | 119 | 55967 | 78 |

(The table is as of the second pass.) The package's other files, and the three routines of
`db2scn.c`:

| File | Address | Function | Bytes | What it does | Checked |
|---|---|---|---:|---|---|
| db2scn.c | 0x08109cb0 | `elemdictCompleteHierarchy` | 216 | gives the nodes below a root that has just entered the list their elements, then updates the hierarchy information, the prefixes and the model's internals in the dictionary | 800 cases, 688 identical, 0 different |
| db2scn.c | 0x08109d88 | `grafnodeElemAssoc` | 48 | ties a graph node and its element; with the switch the node takes the element's wire colour, selection and display type | 300 cases, 300 identical, 0 different |
| db2scn.c | 0x08109ec8 | `SI_graflistBuildFromElemDict` | 1469 | puts the new elements of a dictionary into the scene's list, chapter by chapter | 2500 cases, 2315 identical, 0 different |
| nselections.c | 0x08346050 | `nselectionCount` | 84 | how many named selections the scene's list has (all, or the selected ones), and the last | 600 cases, 600 identical, 0 different |
| nselections.c | 0x083460a4 | `SI_nselectionAllocate` | 76 | a named selection record: 12 bytes, the name not copied | 200 cases, 200 identical, 0 different |
| nselections.c | 0x083460f0 | `SI_nselectionCreate` | 212 | makes a named selection or tag: record, graph node (kind 0xc), name record, element | 1200 cases, 1200 identical, 0 different |
| nselections.c | 0x083461c4 | `nselectionDispose` | 132 | removes the relations of a named selection, frees its name, then frees the pointer it was handed | 400 cases, 400 identical, 0 different |
| nselections.c | 0x08346248 | `nselectionReplace` | 860 | relates a named selection to every selected node (and cluster, and curve); deletes it when nothing is selected | 2500 cases, 2429 identical, 0 different |
| nselections.c | 0x083465a4 | `SI_nselectionSelectContent` | 992 | selects, unselects or toggles the nodes, curves and clusters a named selection holds | 3000 cases, 2806 identical, 0 different |
| nselections.c | 0x08346984 | `SI_dialogNamedSelections` | 764 | [name] the dialog that lists the named selections a node belongs to |  |
| nselections.c | 0x08346c80 | `buildNameSelectionTextList` | 416 | [name] fills that dialog's list from a relation list |  |
| nselections.c | 0x08346e20 | `SI_dialogSetNamedSelection` | 1136 | [name] the dialog that makes or replaces a named selection |  |
| nselections.c | 0x08347290 | `SI_tagMdlRm` | 104 | frees the tag numbers of every model of the list | 600 cases, 600 identical, 0 different |
| nselections.c | 0x083472f8 | `SI_tagMdlUpdate` | 372 | gives every model the numbers of the tags it carries; answers the number of tags | 1500 cases, 1500 identical, 0 different |
| nselections.c | 0x0834746c | `tagValid` | 108 | the number of a tag among the tags of the NSELECTIONS chapter | 1000 cases, 1000 identical, 0 different |
| nselections.c | 0x083474d8 | `SI_checkTags` | 164 | prints every model's tag numbers (debugging; no caller) |  |
| nselections.c | 0x0834757c | `SI_nselectionUpdate` | 1004 | after a load: marks tags, turns the curve records of a scene file into relations to curves, removes the relations to whole animations | 2500 cases, 2392 identical, 0 different |
| nselections.c | 0x08347968 | `SI_nselReadSpecialRelationData` | 460 | [name] reads a relation's curve record from a scene file |  |
| nselections.c | 0x08347b34 | `SI_nselWriteSpecialRelationData` | 479 | [name] writes it |  |
| graphsort.c | 0x08339a70 | `si_isPrimaryMatterRelation` | 64 | whether a shared material or texture hangs under this element in the schematic view | 1000 cases, 1000 identical, 0 different |
| graphsort.c | 0x08339ab0 | `getPrimaryMatterRelationParent` | 80 | the model a material or texture is related to by 0x12c | 600 cases, 600 identical, 0 different |
| graphsort.c | 0x08339b00 | `schemElemRelFlag` | 332 | clears the layout mark of the sons of an element on one side | 2000 cases, 2000 identical, 0 different |
| graphsort.c | 0x08339c4c | `si_schemElemFlag` | 140 | clears the layout mark of an element and everything below it; makes its position relative to its parent's | 2000 cases, 2000 identical, 0 different |
| graphsort.c | 0x08339cd8 | `elemRelGraphSortInit` | 336 | prepares the sons of an element on one side for a layout | 2000 cases, 2000 identical, 0 different |
| graphsort.c | 0x08339e28 | `elemGraphSortInit` | 152 | prepares an element and everything below it: relative position, width 0, marked | 2000 cases, 2000 identical, 0 different |
| graphsort.c | 0x08339ec0 | `relGraphSortInitElement` | 196 | one element of a chapter before a layout, by the schematic view's switches | 2000 cases, 2000 identical, 0 different |
| graphsort.c | 0x08339f84 | `relGraphSortInitChapter` | 128 | every element in use of a chapter before a layout | 1500 cases, 1500 identical, 0 different |
| graphsort.c | 0x0833a004 | `si_schemSortInit` | 280 | prepares the chapters the schematic view shows | 2000 cases, 2000 identical, 0 different |
| graphsort.c | 0x0833a11c | `si_schemResetUserPos` | 180 | forgets the user's own placing of every element | 500 cases, 500 identical, 0 different |
| graphsort.c | 0x0833a1d0 | `schemCompareElems` | 236 | the order of two sons by their position (qsort's comparison) | 2000 cases, 2000 identical, 0 different |
| graphsort.c | 0x0833a2bc | `schemSetElemRelPosX` | 988 | [name] positions of the sons of an element across the layout direction |  |
| graphsort.c | 0x0833a698 | `si_schemSetElemPosX` | 128 | [name] the same for one element |  |
| graphsort.c | 0x0833a718 | `schemSetElemRelLevelPos` | 688 | [name] positions of the sons along the layout direction, by level |  |
| graphsort.c | 0x0833a9c8 | `si_schemSetElemLevelPos` | 224 | [name] the same for one element |  |
| graphsort.c | 0x0833aaa8 | `schemSetElemRelWidth` | 468 | [name] the width the sons of an element take |  |
| graphsort.c | 0x0833ac7c | `si_schemSetElemWidth` | 248 | [name] the width of an element with what is below it |  |
| graphsort.c | 0x0833ad74 | `schemChapterSetHierarchies` | 324 | [name] lays out the hierarchies of one chapter |  |
| graphsort.c | 0x0833aeb8 | `schemSetHierarchies` | 300 | [name] lays out the hierarchies of every chapter |  |
| graphsort.c | 0x0833afe4 | `SI_schematicSort` | 700 | [name] the layout of the whole schematic view |  |
| graphsort.c | 0x0833b2a0 | `schemChapterArrangeHierarchies` | 712 | [name] places the hierarchies of one chapter side by side |  |
| graphsort.c | 0x0833b568 | `schemArrangeHierarchies` | 764 | [name] the same for all chapters |  |
| graphsort.c | 0x0833b864 | `SI_schemComputeElemBBox` | 456 | [name] the box of an element with what is below it |  |
| graphsort.c | 0x0833ba2c | `schemComputeElemRelBBox` | 452 | [name] the box of the sons of an element |  |
| graphsort.c | 0x0833bbf0 | `schemChapterComputeHierarchiesBBox` | 388 | [name] the boxes of the hierarchies of one chapter |  |
| graphsort.c | 0x0833bd74 | `SI_schemComputeHierarchiesBBox` | 536 | [name] the boxes of every hierarchy (the routine four routines of graflist.c call) |  |
| graphsort.c | 0x0833bf8c | `schemElemRelOper` | 464 | [name] an operation (move, collapse, …) on the sons of an element |  |
| graphsort.c | 0x0833c15c | `si_schemElemOper` | 400 | [name] the same on an element and everything below it |  |
| graphsort.c | 0x0833c2ec | `schemUpdateHierarchy` | 752 | [name] lays one hierarchy out again |  |
| graphsort.c | 0x0833c5dc | `SI_schematicHierOperation` | 600 | [name] an operation on the hierarchy an element belongs to |  |
| graphsort.c | 0x0833c834 | `schemCollapseNslElem` | 828 | [name] collapses or expands a named selection's element |  |

| File | Functions | Bytes | Ported and checked | Bytes |
|---|---:|---:|---:|---:|
| db2scn.c | 3 | 1733 | 3 | 1733 |
| nselections.c | 16 | 7363 | 10 | 3944 |
| graphsort.c | 31 | 12544 | 11 | 2124 |

Eight functions have no caller in the binary: `SI_grafnodeChangeName`,
`si_graflistFindParent`, `graflistAllVisible`, `clearUnselect`, `SI_grafNodeNbHilite`,
`graflistCheckUniqueName`, `SI_itemGetByName`, `SI_grafnodeGetByName`. Six of them are ported
all the same (they are small, and they say how names and counts are meant); the two that
draw are not.

## 5. Behaviour of the original worth knowing

Read from the listings; each is reached by the cases of section 4 unless marked.

- **`SI_graflistRemoveGrafnode` leaves the removed node's forward link standing** and
  clears only its backward link; it frees nothing. A node whose backward link is null and
  that is not the list's head is taken for "not in a list" and left alone.
  `SI_graflistAddData`'s clean-up after a failed allocation and `SI_graflistDeleteGrafnode`
  rely on the forward link of a removed node.
- **`SI_graflistDeleteGrafnode` frees no node.** It disposes of objects through the editing
  layer and unchains; the node records are kept for undo (`SI_grafnodeSaveHistoryDelete`)
  or simply dropped. `SI_graflistClear` is the only routine of the file that frees nodes,
  and it frees nothing else.
- **`SI_selectGrafnode`'s last test reads the wrong node's flags.** After a branch was
  selected the routine decides whether to note "a second node is selected" by the "not
  editable" flag of the node its walk ended on: the node *after* the branch. At the chain's
  end that is a read through null (the original dies there; the made-up lists that end in a
  model's branch show it as stopped cases). The program's own lists end with the camera's
  two nodes.
- **`SI_selectGrafnode` on a joint selects its chain**: mode 1 on a model of kind 5 (or on
  an effector, kind 0 with algorithm 3 or 4, whose father is one) becomes mode 3: the
  fathers are climbed to the first model of kind 0, that model's node is given the
  selection word of the node that was clicked, and the click then acts on it. A second
  click turns "node" into "branch". Under the manipulation switch at +0x1c, in multiple
  selection, a click selects every node of the branch as a plain node instead.
- **`SI_unselectGrafnode` on anything selected as 2 or 3 reads the object as a model**,
  whatever the node's kind (it looks for an effector to unselect the whole chain from its
  root).
- **`SI_graflistTypeCount` answers the last matching node, `SI_graflistCountSelected` the
  first.**
- **`SI_graflistCountSelHierarchies` trusts the counts**: it steps `count + 1` nodes per
  hierarchy without a test for the chain's end.
- **`SI_grafnodeGetName` returns nothing for kinds it does not know** (9, 10, 0xd, 0x11,
  0x12 and anything outside 2 to 0x14): the result register is left as the caller had it.
  The port answers null. *[not compared: the cases use the kinds with an answer]*
- **`SI_grafnodeChangeName` for a wave or a named selection writes through a null
  pointer**: it frees the object, clears its register and stores the copy of the name at
  address 4. It has no caller.
- **`graflistCheckUniqueName` takes a camera's record as its name**: for kind 4 it compares
  the text at the object's address, which for a camera is a pointer. Lights are right
  (their record begins with the text).
- **`SI_graflistToggleForceWireframeSelected` and `SI_toggleBoundGlSelected` flip bit 0 of
  a byte**, where the "all" routines of the same switches write the 16-bit word as 0 or 1.
- **`SI_hideGlGhosted` unselects every selected node**, hidden or not;
  `SI_toggleHideGlSelected` only those it left hidden, and none when its second argument
  is set.
- **An element of the chapter of groups stands for its members**: `SI_setSelectability`
  and `SI_toggleHideGlSelected`, meeting a selected node whose element is in chapter
  `SI_cid[9]`, act on the graph node of every element named in that element's first
  forward relation list (and only on those that are not selected themselves).
- **`SI_graflistAddData` for a light in single selection** leaves the node in front
  unselected, selects the light's own node and makes that one the active node; in multiple
  selection both get 1.
- **`SI_graflistLinkModel` and `SI_graflistDeleteGrafnode` pass a float nobody set** to
  `si_schematicRightReadjustX` when the schematic view's layout direction is neither 0 nor
  1. For 0 and 1 the distance is the element's box size along the direction, minus or plus
  0.5 or 1.5, each step rounded once to a float on the x87 unit (reproduced with `si-x87`,
  and identical on every case).
- **`deleteQStretchCtrls` deletes whatever the related element's graph node is, also
  null**, and asks for "the first related element" twice rather than the first and the
  next: the first deletion is what makes the second question answer the other control.
- **The reduced product** (`SI_lightLicense` set; read, not ported): a node that is not
  editable keeps its selection in flags 0x2000, 0x4000 and 0x8000; `SI_graflistLinkModel`
  does not select such a parent; `SI_graflistCutModel` calls `SI_edSetEditable` for the
  father's element when the cut node is not editable.

## 6. Checking by emulation

### 6.1 How a case is made and compared

After `si-dict`'s pattern (finding 0026, section 2). `re/tools/si-view/gen.py` drives the
harness of `re/tools/emu` through `viewlib.py` (the conventions) and `vworld.py` (made-up
scenes); `gen_list.py`, `gen_flags.py`, `gen_select.py`, `gen_edit.py` hold one generator
per routine. A case is one call. Its world is a set of named buffers: the array of lists
(one buffer, as the original has one block), every node a buffer of its own (the routines
free and unchain them one by one), every model, light, element and string a buffer;
globals are buffers at their own addresses. `view-emucheck` loads the case with
`si_mem::load_case_at`, which gives every buffer the harness's address, runs the port and
compares (the list is in the table at the top). Each case runs twice, on memory holding a
pattern and its complement.

**Outside routines** are replaced in the emulator by stand-ins that write down the call,
its arguments and the answer the generator chose; the port must make the same calls in the
same order with the same arguments and gets the same answers through the trait. Three
conventions of this package:

- an argument that is the address of a local of the caller is written `57ac57ac`; the port
  must pass an address of the image's stack there, and what the stand-in stored through it
  follows as an entry `name.out(values…)`, which the check stores through the port's
  address;
- a `short` argument is compared in its 16 bits;
- an argument the original takes from stack memory nobody set is written as unwritten and
  not compared (one case: the float of section 5).

No stand-in changes memory otherwise. Two stand-ins compute their answer from the emulated
memory (`SI_modelNextNode`, `SI_modelNbSubModels`: the model tree of the case), so that
`SI_graflistAddData` builds real hierarchies.

**The scenes**: one to three lists of which one is used; up to five groups, each a model
hierarchy up to four levels deep, a camera pair, a light pair or a single node of any other
kind; selection words and flags by chance and, for the selection routines, consistent
branches (3 on a root, 2 below) in a third of the cases; skeleton chains (a root of kind 0,
joints, effectors); the active node present or not, in the right place for its flag or the
wrong one; counts that do not fit the chain; lists that end in the camera's nodes (as the
program's do) or not; names equal, empty, and the very string a model carries.

**Which instructions the cases reached** is noted while the vectors are made (a block hook
on the routine; `vcov.py` sets the assembler's padding aside). Looking at that list twice
added cases: `SI_selectGrafnode`'s named-selection exit, its second look-up when the
chain's root has no node, and its "another node is active" branch were not reached by the
first generator.

### 6.2 Numbers

Run of 2026-10-13, `re/tools/si-view/check.sh 1.0` (about 6 minutes; 803 MB of
vector files).

| Module | Routines | Cases | Identical | Different | Not compared |
|---|---:|---:|---:|---:|---:|
| `graflist::list` | 26 | 21,125 | 20,640 | 0 | 485 |
| `graflist::flags` | 24 | 25,300 | 24,743 | 0 | 557 |
| `graflist::select` | 4 | 10,800 | 9,822 | 0 | 978 |
| `graflist::events` | 6 | 7,900 | 7,212 | 0 | 688 |
| `graflist::edit` | 12 | 18,500 | 17,630 | 0 | 870 |
| **all** | **72** | **83,625** | **80,047** | **0** | **3,578** |

| Mutation run | Identical | Different | Not compared |
|---|---:|---:|---:|
| `EMUCHECK_MUTATE=1` | 6,492 | 73,555 | 3,578 |
| `EMUCHECK_MUTATE=calls` | 38,847 | 41,200 | 3,578 |

Per routine: the last column of the table in section 4 (cases, identical, different; the rest of a routine's cases were not compared).

Not compared: in 3,578 cases the original did not return (a made-up list that asks for
a read or write through null: a count that runs past the chain's end, a branch at the
chain's tail in `SI_selectGrafnode`, a node of kind 3 without a root before it, a null name
handed to `strcmp`, an object that is not a model read as one; 20 of them ran out of
the instruction budget).

### 6.3 What the checks found

**No port had to be mended.** On its first comparison every routine agreed with the emulated
original in every case, with one exception that is not the port's: three cases of
`deleteQStretchCtrls` (the float nobody set, below). What the checks did find was in what I
had written around the ports:

| Found by | Fault | Kind |
|---|---|---|
| `check_outside.py`, on its first run | six of the 101 outside routines had addresses I had written down without looking them up (`SI_fcvScnglBDelete`, `SI_cusDeleteModel`, `SI_cusDeleteIcon`, `SI_cusElemCallbackCallAll`, `SI_cusElemCallbackUpdateEffectsForDeletion`, `SI_modelUnlinkAllWaves`), and two of them the wrong source file | an address from memory is not an address. Fixed before any use; the script now holds every address the crate mentions against the symbol table |
| the generator stopping | a stand-in's answer was logged in a form the harness's formatter does not take | tool |
| the harness: 11 to 14% of two routines' cases stopped inside a stand-in | the generators of one module answered with a buffer address before the session was known to them | generator |
| the count of stopped cases (27% of `SI_graflistLinkModel`, 55% of `SI_clearSelection`) | the generator picked parents that are no model nodes, and gave nodes that are no model nodes the selection of a branch | generator: the cases were valid but said little |
| three cases of `deleteQStretchCtrls` differing | the float nobody set (section 5): in a nested call the stack holds what earlier code left, the same in both of the harness's runs, so the harness cannot mark it | a limit of the two-pattern method (finding 0011 names it); the stand-in now writes the argument down as unwritten under that condition |
| the instruction list | three paths of `SI_selectGrafnode` and two exits of `SI_toggleGrafnode` not reached | generator: cases added |
| a unit test | its own outside world answered 1 where an element's address was expected | test |

## 7. Not ported, not checked

Nothing is written and unchecked: every function in the crate is cited, and
`re/tools/si-view/cite.py` rewrites a citation into `` `name` (0xADDRESS); unchecked `` when
the check's report does not carry it.

Not compared, of what is ported: the answers to "no memory" (`SI_grafListAdd`'s −1,
`SI_graflistAddData`'s clean-up of a half-built hierarchy: 18 instructions in all,
one of them dead code in `SI_grafnodeGetByName`); the kinds for which `SI_grafnodeGetName`
returns nothing; the cases in which the original stopped; the reduced product's paths.

Not ported, of `graflist.c` (41 functions after the second pass, which took the six
routines this table listed first: section 10):

| Functions | Bytes | Why |
|---|---:|---|
| drawing: 23 functions | 21,444 | `si_drawGrafnode` (4,596 bytes), `drawGraflist`, the constraint and light icons, the point traversals for auto-zoom: the GL layer's calls in sequence; third pass, with the views |
| interface: the other 18 functions | 17,911 | picking (`SI_gLSelectSelected`, `rectSelect`, the hit tests, the nearest-point projections): need the view windows and the GL layer's selection mode; third pass |

`graphsort.c`, `nselections.c` and `displayMode.c` were not started in the first pass; the
second took 10 routines of `nselections.c` and 11 of `graphsort.c` (section 10.7 lists what
is left).

**What the next pass should take first** (written at the end of the first pass; section
10.7 has the list for the third): `SI_graflistDeleteNode`, then the three routines
that empty a scene (with them `SI_grafListCreate` to `graflistDelete` is the whole life of
a list); `nselections.c` (16 functions; `SI_nselectionSelectContent` is the one callee of
the selection routines that is still behind the trait and belongs to this package); then
`graphsort.c`, whose `SI_schemComputeHierarchiesBBox` and `SI_schematicHierOperation` four
routines here call.

## 8. The outside world

One trait, `si_view::outside::Outside`, with one method, `call(m, &Routine, &[u32]) -> u32`:
the callee by name and address, the arguments as the 32-bit words the original pushes
(a `short` as its 16 bits), the answer what the original leaves in `eax`. It has the shape
of `si-dict`'s and of `si_mem::Outside`'s proposal (finding 0024, 11.5), so that it can
become that one when the lead decides. 102 routines are named, each written
`name (0xADDRESS)` with its source file, of which the ported code calls 77: the editing
layer between the list and the dictionary (`edtools.c`, 21), the dictionaries (`si-dict`
has `SI_elemdictGetElem`, `SI_elemdictFindRelatedElem` and its "next" on the image), the
model tree, history, animation, expressions, constraints, custom effects, the schematic
view, the windows; the rest are named for the routines of section 7.

## 9. Wanted in shared files

- **`si-types`, `GrafNode`**: `nb_sub` and `wire_color` are 16-bit words the original only
  ever treats as unsigned (`movzwl`); they are `i16` there and cast here. `flags`: the bits
  of section 2.1 (the comment names 0x0001 and 0x1000 only). `kind`: 8, 9, 0xc, 0xd, 0x12,
  0x13, 0x14 have no constant. **`GrafList`**: `unk_10` is four words of which +0x14 and
  +0x1c go to `SI_image`; `multi` is the selection mode. **`Model`**: `bbox_valid` (+0x0c)
  is the "draw as bounding box" switch of the display menu; `unk_350` is the count that
  makes `SI_cusDeleteModel` be called.
- **`si-types`' `graflist_insert` and `GrafNode::allocate`** cite `graflistInsertGrafnode`
  and `grafnodeAllocate` on a Rust slice; **`si-rensetup`'s `native/graflist.rs`** cites
  `SI_findGlData`. All three are here on the image, written again from the listings and
  measured: three duplicates to reconcile.
- **`si-rensetup`**: its bridge can now build the list with `graflist_add_data` in the
  loader's order (section 3) instead of making one up.
- **`re/tools/emu`**: a stand-in's result cannot be logged as a pointer (`("p", value)`
  works for arguments only; `viewlib.py` resolves it itself); a stand-in has no way to say
  "this argument has no defined value" short of writing `UNWRITTEN` into the log by hand;
  a float local that nobody set is not marked in a nested call (section 6.3).
- **`check_globals.py`**: `SI_glbManip` is declared here as its whole 0x3c bytes, as
  `si-dict` does; `si-rensetup`'s 4-byte declaration of it is the one finding 0024 (11.7)
  already lists.
- **`fn.py`**: an instruction listing with names would serve every package; this one has
  its own copy of `si-rensetup`'s (`vlst.py`), a compact form (`vl.sh`) and a jump-table
  reader (`vjt.py`).

## 10. Second pass (2026-10-13): building the list from a scene, deleting, named selections, the schematic hierarchy

What sections 1 to 9 say stands, except where this section says otherwise (the table of
section 4 and the lists of section 7 are brought up to date in place). Everything below is
**read from the instruction listings and measured** unless marked.

### 10.1 What was added

| Source file | Functions | Ported, checked and cited now | This pass |
|---|---:|---:|---|
| `graflist.c` | 119 | **78** | `SI_graflistDeleteNode`, `graflistDelete`, `removeChapterData`, `SI_sceneDeleteAllNotInHistory`, `SI_selectedString`, `SI_vectorDistancectCam` |
| `nselections.c` | 16 | **10** | `nselectionCount`, `SI_nselectionAllocate`, `SI_nselectionCreate`, `nselectionDispose`, `nselectionReplace`, `SI_nselectionSelectContent`, `SI_tagMdlRm`, `SI_tagMdlUpdate`, `tagValid`, `SI_nselectionUpdate` |
| `graphsort.c` | 31 | **11** | `si_isPrimaryMatterRelation`, `getPrimaryMatterRelationParent`, `schemElemRelFlag`, `si_schemElemFlag`, `elemRelGraphSortInit`, `elemGraphSortInit`, `relGraphSortInitElement`, `relGraphSortInitChapter`, `si_schemSortInit`, `si_schemResetUserPos`, `schemCompareElems` |
| `displayMode.c` | 25 | 0 | |
| **the package** | **191** | **99** | 27 |
| `db2scn.c` (the loader's file; three routines) | | 3 | `grafnodeElemAssoc`, `elemdictCompleteHierarchy`, `SI_graflistBuildFromElemDict` |

30 functions, about 11.5 KB of the original (3,676 + 3,944 + 2,124 + 1,733 bytes counted to
the next symbol). New modules of `crates/si-view`: `db2scn`, `graflist::scene`,
`graflist::status`, `nselections`, `graphsort`, `wired`; `graflist::edit` has
`SI_graflistDeleteNode`. `grafnodeHidden`, which the module map lists at `graphsort.c`'s
last address, is the first function of `hidden.c` and not this package's (a first version
of this pass ported it as "one `ret`"; the emulation stopped in both of its cases and the
port was thrown away).

### 10.2 Numbers

`re/tools/si-view/check.sh 1.0` regenerated every vector (the generators' shared library
changed), about ten minutes; 1.6 GB of vector files.

| | Routines | Cases | Identical | Different | Not compared |
|---|---:|---:|---:|---:|---:|
| First pass | 72 | 83,625 | 80,047 | 0 | 3,578 |
| Second pass | 30 | 46,700 | 45,558 | 0 | 1,142 |
| **All** | **102** | **130,325** | **125,605** | **0** | **4,720** |

**The first pass's totals came out again**, row for row (all 72 rows of the report equal),
and its 72 vector files are byte for byte what they were (checksums taken before the
regeneration, `re/out/si-view/vec_first_pass.md5`).

| Second pass by file | Routines | Cases | Identical | Different | Not compared |
|---|---:|---:|---:|---:|---:|
| `db2scn.c` | 3 | 3,600 | 3,303 | 0 | 297 |
| `graflist.c` | 6 | 12,000 | 11,528 | 0 | 472 |
| `nselections.c` | 10 | 13,500 | 13,127 | 0 | 373 |
| `graphsort.c` | 11 | 17,600 | 17,600 | 0 | 0 |

| Mutation run | Identical | Different | Not compared |
|---|---:|---:|---:|
| `EMUCHECK_MUTATE=1`, all | 17,541 | 108,064 | 4,720 |
| the same, second pass | 11,049 | 34,509 | 1,142 |
| `EMUCHECK_MUTATE=calls`, all | 64,241 | 61,364 | 4,720 |
| the same, second pass | 25,394 | 20,164 | 1,142 |

(The cases that stay identical under the first switch return nothing and change no memory;
under the second they make no outside call with an argument.)

Instructions: 8,123 in the 102 routines, **29 not reached** (the 18 of the first pass; ten
of `SI_selectedString` that are dead code: the inline `strlen` of the line it has just
emptied; one of `nselectionReplace` behind a `free` the harness stops at, 10.4), and 153
more that only the reduced product reaches (six of them the call of `SI_checkFcurveNSel` in
`SI_nselectionSelectContent`). Globals declared: 30 (`check_globals.py`: 0 wrong). Outside
routines named: 135, with 102 citations and 297 mentions of an address (`check_outside.py`:
0 wrong). Constants the crate writes into the image or uses in place of a read of the
binary: 7 (`check_data.py`, new: 0 wrong). Unit tests: 5.

Not compared (1,142): the original stopped on a wild read or write the made-up case asks
for (an element without the object its chapter expects, a count past a chain's end, the
`free` of 10.4), ran out of the instruction budget (a list `graflistDelete` cannot empty),
wrote past a made-up record, or made calls that depend on memory nobody wrote.

### 10.3 How a loaded scene becomes the list

`SI_graflistBuildFromElemDict(dictionary, relation dictionary, chapter ids, loaded)`
(0x08109ec8) walks **every chapter in index order and in it every element in index order**
and looks only at elements whose "new" word (+0x46) is 1:

| Chapter | What happens |
|---|---|
| MODELS | an element whose model has no father: the model enters as a root (`SI_graflistAddData`, kind 2, selected or not as `SI_cusReadingAnimModel` says), its hierarchy behind it; node and element are tied; `SI_modelUpdateDependency`. When `loaded` is 0, `elemdictCompleteHierarchy` gives the other nodes their elements (`SI_edAddModel`) with `SI_addInDictionary` set meanwhile |
| SCENES | a node of kind 0xd without an object |
| NSELECTIONS, TEXTURES3D, ANIMATION | the object enters as kind 0xc, 0x10, 0x12 |
| MATERIALS | kind 0xe; the material gets the element's index at +0xda8 and 1 at +0xdac |
| TEXTURES2D | kind 0xf; a texture without a name gets a copy of the element's; index at +0x190, 7 at +0x194 |
| CAMERAS | **nothing is added**: the node of the process record's camera is looked up (kind 5 for a root element, 4 for one below a root) and tied; a root element with `loaded` 0 goes to `SI_edCreateObjectFromInterest` |
| LIGHTS | **nothing is added**: the light gets the index at +0x170; its node is looked up by the light's type (+0x1e: 1 kind 8, 3 kind 0x13, 4 kind 0x14, 2 kind 7 for a root element and 6 below one) and tied; for kind 6 with `loaded`, the element's name is copied over the light's (30 bytes); a node found as kind 7 with `loaded` 0 goes to `SI_edCreateObjectFromInterest` |
| WAVES | the node is looked up (kind 0xb) and tied |
| others | nothing |

Then the MODELS chapter once more: every new element whose model **has** a father is tied
to its model's node, always with the display state.

"Tied" (`grafnodeElemAssoc`): the element's word at +8 becomes the node, and with `loaded`
the node takes the element's wire colour (16 bits), selection and display type.

**The order it produces**: each addition goes to the head, so the chain is the reverse of
the walk: the last root of the last chapter that adds first, every root followed by its
hierarchy depth first; cameras, lights and waves stay where they were (they entered the
list when they were read), which for the program's own list is the tail.

**What the caller must have in the image**: the dictionary with its chapters and elements
(`si-dict`'s layout; the cases of the check build it as `si-types` lays it out, without
depending on `si-dict`); every element's object, and for materials, 2D textures and lights
objects large enough for the words above; the name record of an element whose name is
read; the chapter ids; `SI_procData` with the camera and the list's index (`SI_grafLists`
may be null: the routine makes the array); and for `loaded` 0 the current dictionary in
`SI_elemDicts` with `SI_cid` naming the MODELS chapter. Its `Outside` must answer
`SI_modelNextNode` and `SI_modelNbSubModels` truthfully (section 3) and may answer 0 to
`SI_cusReadingAnimModel`; `SI_edAddMaterial` and the two texture routines must answer
non-zero. The unit test `a_loaded_dictionary_becomes_the_list` is such a use.

A local of the routine holds the kind a camera or light is looked up as. A light whose type
is none of 1 to 4 is looked up with **whatever that local holds**: the kind of the last
camera, light or root model, or, before any, a word of the stack nobody wrote. The port
keeps the local on the image's stack and does not set it either.

### 10.4 Behaviour of the original worth knowing

- **`SI_graflistDeleteNode` composes a node's transformation with its father's before it
  cuts it loose** (a node of kind 3, when `SI_hrchCpst` is set, 1 in the binary): each
  local scaling factor times the father's global one; the rotation matrix through
  `SI_matrixMultiplySub3` (itself, the father's global rotation) and
  `SI_matrixExtractXyzRotation`; the translation as the point it is times the father's
  global matrix, `((y·m10 + x·m00) + z·m20) + m30` and likewise for the other two, all
  three from the old coordinates. Every product and sum is on the x87 unit and rounded
  once, at the store (`si-x87`; identical on every case, special values included). The two
  matrix routines are `si-math`'s and behind the trait. The node's element is read without
  a test for null.
- **Without "branch", a model node that has a son loses only its geometry** and stays; a
  light's own node (6) deletes the node in front of it.
- **`graflistDelete` deletes the model nodes first**, then whatever is at the head until
  the list is empty, and puts the camera back **with both nodes hidden** (display type 0);
  the camera record's words at +0xa0 and +0xa4 are cleared without a test for null.
- **`removeChapterData` does not clear the object of a curve** (an ANIMATION element above
  level 0): it hands `SI_fcurveRemove` the address of the element's object word. The
  shadow shaders' chapter is not among the shader chapters it disposes of.
- **`SI_selectedString` uses no `sprintf`**: `strcat` behind an inline `strlen` each time,
  into a static line of 0x400 bytes; a piece that would pass 0x3ff is left out, the rest
  still follows. The line is `prefix`, `" Multi"`, `": "`, the element's prefix, `"-"`, its
  name, `"[Br]"`.
- **`SI_vectorDistancectCam` answers two different things**: with the camera's word at
  +0x0c, the straight distance, rounded to a float; otherwise the distance along the view
  direction, **left unrounded in the register** (80 bits compared).
- **`nselectionDispose` frees the pointer it was handed**, not the record that points to.
  Its one caller, `nselectionReplace` when an existing selection is replaced while nothing
  is selected, hands it the address of a local: a `free` of a stack address. The harness
  stops there (71 cases) and the image records the fault; what the C library of the time
  did is not known here. When a *new* selection finds nothing selected, the node is deleted
  and the record is not disposed of at all.
- **`SI_nselectionAllocate` does not copy the name** and leaves the type word unwritten
  for a type that is neither 0 nor 1.
- **Selecting a named selection** clears the node's own selection, switches both lists to
  multiple selection when it holds more than one node or more than one curve, and acts on
  each relation of the element's first forward list: a curve's node in the curve list
  (made on the spot when it has none), a cluster made active, anything else the element's
  graph node. A named selection among the content selects its own content in turn.
- **`SI_nselectionUpdate` reads a curve's type through null** when the number in a
  relation's record is larger than the curves the element has.
- **The schematic hierarchy** (`graphsort.c`): an element's sons are the relations 0x6e,
  0x456 and 0x83e of its backward lists and 0x4e22, 0x12c, 0x190, 0x191, 0x1f4, 0x1f5,
  0x2bc, 0xc8, 0xd2, 0xd7 and 0x3a98 to 0x3aa2 of its forward lists; a material or texture
  with several users hangs under the first. `elemGraphSortInit` marks an element only
  **after** its sons: a cycle of such relations never ends. Positions are made relative to
  the parent's with one rounding each. `schemCompareElems` answers 0 for a NaN.

### 10.5 The seam: four routines stay calls by address

The first pass's routines call four routines that were ported after them:
`SI_graflistDeleteNode` and `SI_nselectionSelectContent` (ported now), and
`SI_schemComputeHierarchiesBBox` and `SI_schematicHierOperation` (`graphsort.c`, not
ported). The first pass's vectors answer them with recorded stand-ins. So that those
vectors stay valid and its totals reproduce, **the ports keep calling the four through the
trait, every caller alike**, and `si_view::wired::Wired` closes the seam: an `Outside` that
hands the four to their ports (with itself as their outside world) and everything else to
the world it wraps. A program runs the crate under `Wired`. In the check, a generator
module names the seam routines that run themselves in its cases (`REAL`); its vector files
begin with a line `# real: …`, and for those files the check runs the port where the others
replay the recorded call. `gen_delete` and `gen_nsel` do that: `SI_nselectionSelectContent`
is measured with the selection routines calling it again, on the original's side and on
the port's. The unit test `the_seam_is_closed_by_wired` shows both ways.

Not measured as a whole: the first pass's selection routines with the real
`SI_nselectionSelectContent` under them (each side of that call is measured separately).

### 10.6 What the checks found in this pass's own first versions

| Found by | Fault | Kind |
|---|---|---|
| the emulation: 11 of 250 cases of `nselectionReplace` different | the port disposed of the record in both "nothing selected" branches; the listing does it only in the "removed" branch (a jump over the call was misread) | **port**, mended from the listing |
| the emulation: 6 of 60 cases of `getPrimaryMatterRelationParent` different | the port answered null when its output pointer was null; the original answers the element and only skips the store (a jump target misread) | **port**, mended from the listing |
| the emulation stopping in every case | `grafnodeHidden` taken for a one-byte routine of `graphsort.c` (10.1) | **reading the module map**; port removed |
| reading the binary's data before declaring | `SI_addInDictionary` written down as 0 at start; the binary has 1 | global; mended before use |
| the check: "the port changed 2 bytes outside every buffer" | two globals side by side changed in one run of bytes | **check binary**: the rule now tests byte by byte |
| the check: 3 of 2,500 builder cases different | a made-up material smaller than a material: the original's write 0xda8 bytes in landed where the harness does not look, the image saw the port's | generator: no chapter is shared with the materials' any more |
| the check: 8 of 2,500 cases of `nselectionReplace`, "the port faulted" | made-up element addresses where the deletion reads an element's box | generator: elements that are memory |
| a generator stopping at full scale | `vworld`'s scenes can give two lights one name | generator: such a scene is made again (the first pass's cases never met it) |
| the instruction list | the inline `strlen` of `SI_selectedString` starts differently for each alignment of the string; the harness aligns every buffer | generator: strings 0 to 3 bytes into their buffer |
| a 51 MB vector file at a tenth of the cases | a case that loops until the budget records every call | generator: the record of a stopped case is cut short |

Every other routine agreed with the emulated original on its first comparison.

### 10.7 Not ported, not checked

Nothing is written and unchecked (`cite.py`: 102 cited, 0 unchecked).

| Functions | Bytes | Why |
|---|---:|---|
| `graflist.c`: drawing, 23 functions | 21,444 | third pass, with the views (section 7) |
| `graflist.c`: picking and projection, 18 functions | 17,911 | third pass |
| `nselections.c`: `SI_dialogNamedSelections`, `buildNameSelectionTextList`, `SI_dialogSetNamedSelection` | 2,316 | the two dialogs and the list one of them shows: the dialog layer's calls in sequence; not read beyond their names |
| `nselections.c`: `SI_nselReadSpecialRelationData`, `SI_nselWriteSpecialRelationData` | 939 | `fscanf` and `fprintf` on a stream: the image has no streams yet (finding 0024, 11.4) |
| `nselections.c`: `SI_checkTags` | 164 | a debugging print-out through `printf`; no caller |
| `graphsort.c`: the layout, 20 functions | 10,420 | positions, widths, boxes, arranging, the operations on a hierarchy (`SI_schemComputeHierarchiesBBox`, `SI_schematicHierOperation`, `SI_schematicSort` among them): float layout code, **not read**; the pass ended before them |
| `displayMode.c`, 25 functions | 5,163 | not started |

Not compared, of what is ported: the answers to "no memory"; the reduced product's paths;
what follows the `free` of 10.4 in `nselectionReplace`; `SI_graflistBuildFromElemDict`'s
look-up of a light of an unknown type before any camera, light or root model set the local
(the cases have it, and the garbage of both sides finds no node).

**What the third pass should take first**: the 20 layout routines of `graphsort.c`, from
`SI_schemComputeElemBBox` and `si_schemElemOper` up to `SI_schemComputeHierarchiesBBox`
and `SI_schematicHierOperation` (with them the seam of 10.5 closes for good and the first
pass's deleting and linking routines can be measured with the real thing under them);
then `displayMode.c`; then drawing and picking with the views.

### 10.8 Wanted in shared files (beyond section 9)

- **`si-types`, `Model`**: `unk_358` is two words, the number of tags the model carries
  and the array of their numbers (`SI_tagMdlUpdate`). `scal`, `vrot`, `mrot`, `trans`,
  `gscl`, `grot`, `gmat` are used here as the comments there say.
- **`si-types`**: the records `NamedSelection` (12 bytes) and `NSelFcvData` (16 bytes) are
  in `crates/si-view/src/records.rs`; the camera's fields at +0x0a, +0x0c, +0x10 to +0x30,
  +0xa0, +0xa4, +0x138, a light's at +0x1e and +0x170, a material's at +0xda8 and +0xdac
  and a 2D texture's at +0x18, +0x190, +0x194 are addressed by named offsets in the
  modules that use them.
- **`si-rensetup`**: the bridge can build its list with
  `si_view::db2scn::graflist_build_from_elem_dict` now (10.3).
- **`si-math`**: `SI_matrixMultiplySub3`, `SI_matrixExtractXyzRotation`,
  `SI_vectorNormalize`, `SI_vectorMultiplyMatrix` and the three rotation matrices are
  called through the trait with addresses of the image; whoever wires them needs them on
  the image's memory.
- **`re/out/linux/soft.modules.tsv` and `vfuncs.py`**: a file's end address is the next
  file's first function when no padding lies between (`graphsort.c` and `grafnodeHidden`);
  `vmap2.py` leaves such a function out, `vfuncs.py` still lists it.
- **`vworld.py`** (this package's): `Scene` names a light after a made-up address that two
  groups can share; left as it is so that the first pass's vectors stay byte for byte, and
  worked round in `gen_nsel.py`.
- **`re/tools/emu`**: a write more than 64 bytes past a buffer is not reported (10.6); a
  made-up address around 0x7e000 was readable in the emulator and not in the image (the
  cases no longer use one where it is read).

## 11. Third pass (2026-10-15): `func.c`, the chooser of a node's drawing routine

`SI_updateGnFunc` (0x08176618, `func.c`) is on the image in `crates/si-view/src/func.rs`
[soft 0x08176618 SI_updateGnFunc]. Read from the code:

- A node of kind 2 or 3 holds a model (+0x14); the model's kind (+0x30) picks the routine
  stored at node +0x18: 0 none (null), 1 `patchFunc` (0x08172140), 2 `faceFunc`
  (0x0816e290), 4 `meshFunc` (0x08170628), 6 `splineFunc` (0x08172d14), 8 `ballFunc`
  (0x081738f8), 9 `nurbsCrvFunc` (0x08175734), 10 `nurbsSrfFunc` (0x08173af0); any other kind
  leaves the routine as it was. A node of kind 9 takes `SI_fcurveFunc` (0x0820490c). Any
  other node kind is left alone. The routines are stored as addresses, never called; they are
  not ported (the drawing stays behind the trait, section 7).
- For a patch model, the routine is stored first, then each of the `nb_defs` definitions
  (+0x34, unsigned 16 bits) goes through `SI_patchUpdateSurface (0x0847cee4)`, in order. The
  original counts with a signed 16-bit index compared with the count as a 32-bit number, so
  a count above 0x8000 never ends; reproduced (the loop stops at a fault of the image).
  `SI_patchUpdateSurface` is a new outside address (`si-spline` has it); `check_outside.py`
  holds it against the binary (140 outside routines, 0 wrong).

Check: `gen_func.py` makes 3,000 cases on made-up nodes (every node kind and model kind that
matters, counts of 0 to 7 definitions with up to 3 more pointers than the count, null and
wild pointers among them). The routine under test runs itself and its callee is a stand-in:
`viewlib.Session` gained the option `own` (a module's `OWN`), because the stand-ins of the
seam would otherwise hide `SI_updateGnFunc`, which is also one of them. `SI_patchUpdateSurface`
is now a stand-in in every session of this package.

| | |
|---|---|
| `SI_updateGnFunc` | **3,000 of 3,000 identical**, none different, none not compared |
| `EMUCHECK_MUTATE=1` | 733 differ (the others are cases where nothing is stored) |
| `EMUCHECK_MUTATE=calls` | 174 differ (the cases that call `SI_patchUpdateSurface`) |
| Whole package, vectors made again with the change | 103 routines, 133,325 cases, 128,605 identical, 0 different, 4,720 not compared; mutation 108,797 differ on memory and 61,538 on the call sequence |
| Regression | the 102 older routines' lines (cases, identical, different, not compared) are identical to the figures of the second pass in plain and in both mutation runs |

Still owed: `graflist/edit.rs` (`graflist_delete_node`, which calls it after clearing a node's
selection and flags) still reaches `SI_updateGnFunc` through the Outside hook `SI_UPDATE_GN_FUNC`, as
the vectors of the first two passes were made with a stand-in for it. This is owed: route it
to `func::update_gn_func` (in `Wired`, and in the checks of callers). Until then the callers'
checks hold, and the ported routine runs only in its own check.
`SI_adjustGnFunc (0x08176700)`, the same for a new node, is not ported.

## 12. Fourth pass (2026-10-28): `hidden.c`, the hidden-line view (first seventeen of 33)

Source: `hidden.c`, 33 functions, about 35 KB; package 41 of wave B. Code in
`crates/si-view/src/hidden/` (`geom.rs`: fixed-point and x87 geometry; `lists.rs`: the
chunked lists and their release; `edges.rs`, `tree.rs`, `x87stack.rs`). Check by the same route as the other passes: the original
is stepped in `re/tools/emu` for the cases `re/tools/si-view/gen_hidden.py` makes
(`gen.py ... gen_hidden`), `view-emucheck` runs the port on the same world and compares
memory, heap, outside calls and return. (The queue named this finding 0018; the graph list's
checker is this one.)

### 12.1 Where it stands

| function | address | cases | identical | defined part | different |
|---|---|---|---|---|---|
| vctXMat | 0x08344fd0 | 1500 | 1500 | 0 | 0 |
| triNrmLong | 0x0833f478 | 1500 | 1500 | 0 | 0 |
| triangleOrgNrm | 0x0833f5d4 | 1500 | 1500 | 0 | 0 |
| edgeIntersection | 0x0833f728 | 1500 | 1366 | 134 | 0 |
| edgeTestVertices | 0x08343c70 | 1500 | 1500 | 0 | 0 |
| edgInterTri2 | 0x0833fc5c | 1500 | 1500 | 0 | 0 |
| insertVector | 0x08343874 | 200 | 200 | 0 | 0 |
| insertOrgVector | 0x08343950 | 100 | 100 | 0 | 0 |
| insertEdge (hidden.c) | 0x08343a18 | 300 | 300 | 0 | 0 |
| edgeFreeMem | 0x08344fa0 | 200 | 200 | 0 | 0 |
| allFree | 0x08340504 | 200 | 200 | 0 | 0 |
| inlistStoreTriangle | 0x083405e8 | 300 | 300 | 0 | 0 |
| lookInsertEdge | 0x0833f19c | 1500 | 1500 | 0 | 0 |
| edgeIntersectTriangle | 0x0833f884 | 1500 | 1500 | 0 | 0 |
| intritreeStoreTriangle | 0x08340674 | 1500 | 1500 | 0 | 0 |
| inedgtreeStoreTriangle | 0x083409e4 | 1500 | 1500 | 0 | 0 |
| xferList | 0x0834108c | 800 | 800 | 0 | 0 |

Total 17,100 cases: 16,966 identical, 134 identical in the defined part, 0 different, 0 not
compared (17 of 33 functions). The other `insertEdge` (0x0830d834) is a
different routine of another file.

Mutation, once per function: `EMUCHECK_MUTATE=1` changes every function but `edgeFreeMem`
(10,097 of 10,300 cases flagged different; allFree 3 cases stay identical, those with nothing
to free). `EMUCHECK_MUTATE=calls` changes the routines that call outside (vctXMat 235,
edgInterTri2 255, insertVector, insertEdge, allFree 200, inlistStoreTriangle 127) and nothing
in the pure ones (triNrmLong, triangleOrgNrm, edgeTestVertices, edgeIntersection,
insertOrgVector). `edgeFreeMem` stores nothing and calls nothing outside; its only effect is
the free order, which the mutations do not touch. Coverage evidence: chains of 0 to 8 blocks
(200 cases) so the recursion is reached at every depth up to eight.

Mutation of the five added: `=1` changes lookInsertEdge 1500, edgeIntersectTriangle 1500,
intritreeStoreTriangle 1500, inedgtreeStoreTriangle 1500, xferList 637 of 800 (the rest are
empty lists). `=calls` changes lookInsertEdge 14, edgeIntersectTriangle 224 (the error path,
15% of the cases are degenerate on purpose), intritreeStoreTriangle 1352, and nothing in
inedgtreeStoreTriangle and xferList (pure, or calling only the ported neighbour; coverage
line: the chain of their effects is covered by `=1`). Single versus double rotations of the
trees were not counted separately.

### 12.2 What was read from the code

- Coordinates are fixed point: `vctXMat` multiplies a float vector by a 4 x 4 matrix, divides
  by the fourth coordinate and scales by 2^29, truncating to long. A fourth coordinate of
  1e-5 or less prints to `fp` and sets `Error` to 4. All arithmetic is in extended registers;
  the `static` scratch doubles (`tmp.*`, `a0.*`, ...) are stored at their addresses in the
  original's order and width, because the check compares memory.
- `edgeIntersectTriangle`-style determinants are rounded to double at the points the original
  spills them (`edgeIntersection` keeps partial sums in double stack slots; `edgInterTri2`
  keeps all in registers). The three-message error path of `edgInterTri2` sets `Error` to 3.
- Lists: a vector list holds 0x2000 entries of 12 bytes to a chunk (0x4000 edges of 24 bytes);
  chunk arrays grow by `SI_malloc`/`SI_realloc`, chunks come from `SI_chunkMalloc` on
  `chunkHid` (0x086bafc0), except the original-vector list (plain `SI_malloc`). Edge records
  come from a pool of blocks of 1000 records (0x5dc4 bytes) linked through the first word;
  `edgeFreeMem` frees that chain deepest first.
- x87 method: the long determinant functions are written instruction by instruction on a
  small register-stack model (`hidden/x87stack.rs`), generated from the listing by
  `re/tools/si-view/x87gen.py`. Pitfalls found: `fsubp st(i),st` is st(i) - st0 and `fsubrp`
  is st0 - st(i); `fabs`/`fcomp` leave a value to pop; double constants in rodata (-1.0,
  -1e-5 and others) are not in the emulated memory image, so the port carries them as
  literals.
- `inlistStoreTriangle` stores eight triangle numbers per 0x24-byte block.

### 12.3 Not compared

The doubles that `fprintf` prints on the error paths are not part of the recorded call (only
the stream and the format are logged). The 134 "defined part" cases of `edgeIntersection`
read scratch memory the original leaves uninitialised.

### 12.4 Not yet ported (16 of 33)

analyzeTriangles (0x0833ff50; read, the screen-grid sizing tail not yet), edgeCutMiddle
(0x08343ff8), edgeCutHalf (0x08343dd0), cutEdge (0x0834445c), cutParallelEdge (0x083445d0),
triangleSort (0x08344b0c), analyzeScene (0x08341184), mainHideLines (0x08344f68), initPlanes
(0x0833eb08), grafnodeHidden (0x0833cb70), hlDraw, hlMove, hlLineDraw, sceneryDraw,
findGrafportForHiddenLine, SI_hiddenRefresh. The chunk routing pairs are not wired. Callees in other files stay hooks: clipPlane, displayMode,
draw, graflist, lindraw, pre_render, soft, softdraw, softvertex, status, tablet.

### 12.5 How to run this pass

```bash
export CARGO_TARGET_DIR=$PWD/target/slot-N
E="uv run --with unicorn python3 -I"
(ulimit -v 6000000; timeout 1500 $E re/tools/si-view/gen.py re/out/si-view/hidden/vec 1.0 gen_hidden)
(ulimit -v 2000000; timeout 900 $CARGO_TARGET_DIR/release/view-emucheck re/out/si-view/hidden/vec)   # EMUCHECK_MUTATE=1 | calls
```
Do not run `cite.py` with a report that covers only this pass: it rewrites every citation of
the crate that the report does not list (it did once, and was undone from the committed
files). Use `kit cited si-view` instead.

## How to run

```bash
re/tools/si-view/check.sh 1.0          # build, vectors, check, both mutation runs, citations, globals, addresses, constants, totals by pass, coverage (about ten minutes)
# or step by step:
export CARGO_TARGET_DIR=$PWD/target/si-view
(ulimit -v 8000000; cargo test -p si-view; cargo build --release -p si-view)
E="uv run --with unicorn python3 -I"
(ulimit -v 6000000; timeout 3000 $E re/tools/si-view/gen.py re/out/si-view/vec 1.0)            # all; or names, or a module: gen_list gen_flags gen_select gen_edit gen_build gen_delete gen_text gen_nsel gen_sort gen_func
(ulimit -v 2000000; timeout 1800 target/si-view/release/view-emucheck re/out/si-view/vec)
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 1800 target/si-view/release/view-emucheck re/out/si-view/vec)
(ulimit -v 2000000; EMUCHECK_MUTATE=calls timeout 1800 target/si-view/release/view-emucheck re/out/si-view/vec)
python3 -I re/tools/si-view/cite.py re/out/si-view/check.txt       # citations follow the report
python3 -I re/tools/si-mem/check_globals.py si-view
python3 -I re/tools/si-view/check_outside.py                        # every address the crate names
python3 -I re/tools/si-view/check_data.py                           # the constants the crate writes into the image
python3 -I re/tools/si-view/vpass.py re/out/si-view/check.txt       # the totals of each pass
python3 -I re/tools/si-view/vmap2.py re/out/si-view/check.txt       # the table of the other files (section 4)
(cd re/out/si-view/vec && md5sum -c ../vec_first_pass.md5 | grep -vc ': OK$')   # the first pass's vectors: 0 changed
(ulimit -v 6000000; $E re/tools/si-view/vcov.py -v)                 # instructions no case reached
python3 -I re/tools/si-view/vmap.py re/out/si-view/check.txt        # the table of section 4
python3 -I re/tools/si-view/vfuncs.py graflist.c                    # functions, sizes, callers
re/tools/si-view/vl.sh SI_graflistAddData                           # a compact instruction listing
```
