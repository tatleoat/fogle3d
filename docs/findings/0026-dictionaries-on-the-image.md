# 0026 — The dictionaries on the image

Date: 2026-10-13. Binary: Linux `soft`, sha256 `ef21a382c3325d42…`. Package: `crates/si-dict`,
`re/tools/si-dict/`, scratch in `re/out/si-dict/`. Source files of the original: `elemdict.c`,
`enmdict.c`, `init_dict.c`, `updlist.c`, `toposort.c`, `reldict.c`, `relations.c`, `attdict.c`
(309 functions, about 71 KB of code).

The element, relation and enum dictionaries are the centre of the program's state. They had
been ported twice: on owned Rust stores (`si-types`, `si-scene`; parts in `si-ascii`, `si-db`,
`si-expr`) and, for 19 functions, on a raw image (`si-motion`). This package is the one form
every crate is meant to call: on `si-mem`'s image (finding 0024), records in place at the
original's layout, each function taking the image and the original's arguments and making the
original's allocation calls. The earlier ports are untouched and still in use.

How to read this note: a statement about the original is **read from code** (the decompiled
listing, and the disassembly where the listing is doubtful) and, for every function in the
tables of section 3, **measured**: the function was stepped through in the emulation harness
(finding 0011) on made-up dictionaries and the port compared with what it left in memory.
*[name]* marks a meaning taken from a symbol or a string; *[guess]* a guess.

## Results at a glance

| | |
|---|---|
| After the third pass at `si-scene` (section 11) | **240 of 309**, and two routines of `text.c`: all 52 of `attdict.c` and the element sort (`elemdictCompareElems`, `SI_elemlistSortChapterElements`, with `SI_strGetSeqNum` and `SI_strStripDigit`). 43,715 new cases in two vector directories of their own: 43,481 identical, 234 identical in the defined part, **0 different**. The rows below are the state of the first hand-over and still hold for the first vector directory, whose check output is unchanged byte for byte |
| Functions on the image | **186 of 309**: `elemdict.c` 121 of 125, `enmdict.c` 9 of 10, `updlist.c` 4 of 5, `toposort.c` 18 of 19, `relations.c` 7 of 8, `reldict.c` 27 of 88, `init_dict.c` 0 of 2, `attdict.c` 0 of 52 |
| Of them ported before, on stores or in `si-motion` | 148; **new: 38** |
| Emulation, `dict-emucheck` | 186 routines, 15,726 cases: **15,217 identical**, 346 identical in every word the original defines (the rest of those cases is memory the original never set), **0 different**, 163 not compared (160 the original stopped on a wild read or write, 2 where the result hangs on memory nobody wrote, 1 where the original returns a word nobody set) |
| What "identical" covers per case | the return value; every buffer of the case, changed or not; every block the routine allocated, with address, size and every byte the original wrote, and no byte written that the original left alone; the blocks freed, in order; every call to a routine outside the eight files, with arguments; text printed; and that nothing else in the image changed |
| Mutation switch (`EMUCHECK_MUTATE=1`) | 14,584 of the 15,563 compared cases differ; the other 979 are calls that return nothing and change nothing (a look-up that sets no cursor, a disposal of an empty record) |
| `si-motion`'s 19 dictionary functions on the same vectors (`DICT_EARLIER=motion`) | 2,784 cases: 2,525 identical, 10 identical in the defined part, **0 different**, 249 not comparable there (section 4.1) |
| Corpus, `dict-check` | 343 scene descriptions, 334 loaded completely (the 334 of finding 0018); their dictionaries (40,025 element slots, 38,730 forward relation entries) copied into images; **5,130,470 questions** of 29 kinds asked of the image functions and of the store-based ports: **0 disagreements**, no panic, no fault of the image; 4.9 s, 42 MB. (Since the first pass at `si-scene` the binary is that crate's, section 10.1; same numbers) |
| Differences found in the earlier ports | none that the emulated original contradicts, on these vectors and this corpus (section 4.3 lists where they knowingly answer differently and what the image form does there) |
| Globals declared and held against the binary | 33 (`check_globals.py`: 0 wrong); 8 string literals, the 36 rows of the chapter table with their 72 strings and 18 outside routine addresses (`check_rodata.py`: 62 checked, 0 wrong) |

## 1. What is on the image

### 1.1 The crate

`crates/si-dict` depends on `si-mem` and `si-types` (and on `si-x87` for the vector reader of
its check binary). Record layouts are `si-types`' (`Elem`, `ElemList`, `ElemDict`,
`DbListNode`, `Relations`, `RelList`, `ARelation`, `RelDict`, `RelRule`, `SortTbl`, `UpdList`,
`ChapIds`, `ChapInfo`, `ElemIdentity`, the enum records), used through `si-mem`'s typed access
(`m[p].field`); what `si-types` lacks is laid out here with `record!` (section 5).

| Module | Original | Functions | Content |
|---|---|---:|---|
| `elemdict` | `elemdict.c` | 42 | dictionaries, chapters, elements: creation, the look-ups by index, object and callback, the sort table of `SI_LOAD_OPTIM`, hierarchy information, deletion, disposal |
| `elemrel` | `elemdict.c` | 30 | relations of elements: adding and removing at both ends, the walks over related elements with their cursors and marks, counts |
| `elemname` | `elemdict.c` | 17 | name records, look-ups by name, renaming, new names, versions |
| `elemmisc` | `elemdict.c` | 32 | the global array of dictionaries, chapter loading, the clean-ups a scene load ends with, copies between dictionaries, the small look-ups on top of the walks |
| `relations` | `relations.c` | 7 | one element's relation lists: allocation, ordered insertion, removal |
| `reldict` | `reldict.c` | 27 | the relation dictionary: rules, the sorted index, every look-up |
| `updlist` | `updlist.c` | 4 | update lists: nodes and arguments |
| `toposort` | `toposort.c` | 18 | the topological sort into an update list (whole scene, one element, by marked elements, by curves), the cycle check, the hierarchical order of a chapter |
| `enmdict` | `enmdict.c` | 9 | the enum dictionary |
| `globals`, `rodata`, `records` | | | the files' globals at their addresses; the literals and the chapter table they read; records `si-types` has not |
| `outside` | | | the trait to what is not on the image (section 6) |
| `libc` | | | the C library's string routines and `qsort` on the image (section 9) |

### 1.2 The rules every function follows

- **Signature.** The image, then the original's arguments in the original's order: pointers
  as `Ptr32<T>`, numbers at their C width (`i16` for `short`). A result the original writes
  through a pointer is written through that pointer in the image, also when the caller is
  another function of this crate that could have taken a value. (A few look-ups have, beside
  the cited function, a crate-private form that returns the values: used where the original's
  caller passes the addresses of its own locals, which nothing else can see.)
- **Allocation.** Every `SI_malloc`, `SI_realloc`, `SI_calloc`, `malloc`, `calloc`, `strdup`
  and `free` is made, with its size, in its order. Because `si-mem`'s allocator is the
  harness's, the heap after a call is the emulated original's block for block, and the check
  compares it. Sizes and growth steps were read again from the original for every function
  that had been ported on stores; they are in section 1.3.
- **Memory the original does not set stays unset.** A block from `SI_malloc` holds the
  image's fill beyond the first and last byte; fields a routine leaves alone are left alone.
  The check requires it: a byte the original did not write must still hold the fill.
- **Reads the original makes without a test are made without a test.** Several look-ups read
  the status of the element an entry names at whatever `SI_elemdictGetElem` answered: for a
  missing element that is address 0x48, a recorded fault of the image (the original dies).
  Reads one record before or after an array are made as the original makes them.
- **Statics are globals of the image.** The two look-up caches (`last_prd`/`last_relid`/
  `last_index` of the relation dictionary, `last_pel`/`last_data`/`last_index` of
  `SI_elemdictFindElemByData`), the serial-number counter, the new-name counter and buffer and
  the five buffers of `SI_dbScnGetListNode` are at their addresses, so a second call sees what
  the first left, across crates.
- **Constants read by address are stored by the crate** (`rodata::store`): 8 string literals
  and `SI_chapInfo` with its 72 strings, each held against the binary by
  `re/tools/si-dict/check_rodata.py`. (The chapter table and its strings lie in `.data`, not
  in `.rodata`.)
- **Left out, with the project's standing reasons:** the counter `SI_secureLicAlloc`
  (0x08679660) that the original bumps before every allocation (`si-app` describes it as a
  word of the program's self-check; the allocation wrappers clear it again, so no vector
  shows it changed and nothing had to be excused in the check), and the
  product-tier flag: the three "add a relation" routines call `checkNSEStatus` only when
  `SI_lightLicense` is set, the two removal routines call `SI_edSetEditable` under the same
  flag, and `topoSort` subtracts `SI_lightLicense − SI_lightLicense2` from a cursor. Both
  words are 0 in the binary's data; the port takes that path. `checkNSEStatus` itself is
  ordinary code and is ported and measured.

### 1.3 Sizes and growth, as read again

| Array | First allocation | Growth | When |
|---|---|---|---|
| dictionaries (`SI_elemDicts`) and their flags (`elemDictExists`) | `SI_malloc(0x70)`, `SI_malloc(2)` | `SI_realloc(…, (n + 1) × 0x70)`, `(n + 1) × 2` | every `SI_elemdictAdd` |
| chapters of a dictionary | `SI_malloc(0x40)` | `SI_realloc(…, (n + 1) × 0x40)` | every new chapter |
| elements of a chapter | `SI_malloc(0x84)` | `SI_realloc(…, (n + 1) × 0x84)` | every addition that reuses no deleted slot |
| a chapter's sort table (`SI_LOAD_OPTIM` only) | `SI_calloc(1, 12)`; index array `SI_malloc(4)` | `SI_realloc(…, (n + 1) × 4)`, the new word −1 | with the elements |
| the six lists of one direction | `SI_malloc(0x90)` | never | when an element is added with lists |
| entries of a list | `SI_malloc(0x20)` | `SI_realloc(…, (n + 1) × 0x20)` when count = capacity | before the entry is examined: a refused entry leaves the room and the counted capacity |
| rules of the relation dictionary | `SI_malloc(0x50)`; sort table `SI_calloc(1, 12)` and `SI_malloc(4)` | `(n + 1) × 0x50`; `(n + 1) × 4` | every rule |
| nodes of an update list | `SI_malloc(0x2c)` | `(n + 1) × 0x2c` | every node; the end mark of the fast hierarchy update is one more reallocation that does not count |
| arguments of a node | `SI_malloc(0x14)` | `(n + 1) × 0x14` | every argument |
| enum types, values of a type | `SI_malloc(12)` | `(n + 1) × 12` | every addition |
| sorted chapter indices; a chapter's sorted elements | `calloc(n, 4)` (the C library's, not the wrapper) | made anew | each sort; the old array of chapter indices is not freed |
| hierarchical order and counts of a chapter | `SI_calloc(n, 4)` twice | made anew | each sort; the old count array is freed only when there was an old order array |

No array keeps spare room. A list that loses its last entry frees its block.

### 1.4 Behaviour of the original worth knowing

Read from the code, and each reached by the vectors of section 3 (so the port's agreement on
it is measured), unless marked *[read only]*.

- `si_relAddRelation` makes room and counts the capacity before it decides: a refused entry
  (the same element again under a one-rule relation; beyond the fan-out on the source side)
  leaves capacity one above the count.
- `SI_elemdictSetDeleteElem` with its sixth argument set and more than one entry in the
  element's backward lists only removes the entries that name the element given as fifth
  argument, with their counterparts, and returns: the element is not deleted.
- `SI_elemdictFindElemByData` has three paths that do not answer alike when two elements in
  use share an object: the plain scan answers the first; the path that rebuilds a stale sort
  table answers the **last**; bisection in a valid table answers the first (only the first
  enters the table).
- `SI_elemdictGetElemId` tests the address against the chapter's array with unsigned
  arithmetic: for a chapter without slots the upper bound wraps and every address passes; the
  index is then the product of the compiler's exact division by 0x84, not a rounded quotient.
- `SI_elemdictGetRelation` with index −1, as `topoSort` calls it to climb a hierarchy, answers
  the first entry of the relation that does **not** name the chapter asked for (its counter is
  reset to 0 there, and 0 is −1 + 1). *[read only: the look-up's own vectors ask for indices
  0 to 2; the sort's vectors reach the climb, but no case was made to tell this apart]*
- `topoSort` passes an entry whose other element is not in use, or lies in a chapter of
  another kind, **without moving the list's cursor**; every other skipped entry moves it. The
  "pure dependency" state, once set by one entry of a list, stays set for the entries after it.
- `SI_elemdictTopoSortElement`, asked for all lists of an element none of whose lists takes
  part, returns a word of its stack that nobody set, and clears the dirty word and calls
  `SI_cusResortUpdList` or not by that word. The port answers 0 and does neither.
- `SI_elemdictFindRoot` reads the hierarchical order one word past its end before it tests
  for the end; `SI_reldictCheckRelAnim` looks at one rule beyond the relation's own;
  `si_reldictGetRelSub` and `SI_reldictGetRelNbRules` start one rule before the array for an
  unknown id; `SI_reldictGetMaxFanout` accepts an index equal to the count. The image form
  makes the same reads (the stores answered "not found").
- `SI_elemdictRecursiveSelect` flags as done every element it passes that it does not have
  to go through, and an element it went through only when that element has relation lists.
- `strcmp_underscore` (the comparison behind `SI_compareElemName`) lets an underscore sort
  before a character of the class with bit 0x1000 in the C library's table, which is
  `isxdigit`: digits and the letters a to f, in either case. `ab_` against `abg` is an
  ordinary comparison; `ab_` against `abf` is not.
- `SI_elemdictGetNewNameId` strips the digits at the end of the given name, then takes one
  more than the highest number `strtol` reads behind that stem in any name of the chapter
  (`cube` counts `cube12` as 12 and `cube_x` as 0). With `SI_XSI_LEGACYLOADER` in the
  environment the chapter is not looked at, the routine's static counter runs on and the name
  is `stem_number`. The result is a 500-byte static buffer.
- Blocks the original drops without freeing them: the two validated copies of prefix and
  name in `SI_elemdictChangeName` (on every path that renames), the full name a renamed record
  had, the previous array of `SI_elemdictSortChapNames`, the copy of a chapter name that
  `SI_elemdictLoadChapters` makes for a chapter the dictionary already has. Disposing of a
  dictionary frees neither the chapter names nor any object.
- `SI_elemlistClearLowByteWorkFlag` and `…LowByteWorkFlagAND` address the byte at +0x5b
  (finding 0010's scar), confirmed.

## 2. Checking by emulation

### 2.1 How a case is made and compared

`re/tools/si-dict/gen.py` drives the harness of `re/tools/emu` through `dictlib.py` (the
conventions) and `world.py` (made-up dictionaries); `gen_*.py` hold one generator per routine.
A case is one call. Its world is a set of named buffers: every block the original allocates on
its own is a buffer of its own (the chapter array, each chapter's element array, each
element's two list arrays, each list's entry array, each name record, each string), because
the routines free and reallocate them one by one; globals are buffers at their own addresses.
The vector line carries the buffers, the arguments (`v_name` by value, `p_name` a pointer,
which may point into the middle of a buffer), and after the call: the return value, every
buffer the routine changed, every heap block byte for byte (`heap=u8`), the frees, the
recorded outside calls.

`dict-emucheck` does not rebuild anything. It places the buffers in the image in the case's
order, which gives them the harness's addresses (the arena is laid out the same way), stores
globals at their addresses, runs the port, and compares all of it (the list is in the table
at the top). Three further rules:

- **Two runs.** Each case runs twice, on fresh memory holding a pattern and its complement.
  Both must agree with the original. If they disagree with each other, the port's result
  depends on memory nobody wrote, and so does the original's, unseen by the harness because
  both of its patterns happened to give the same visible result (a rule read one past the
  array whose garbage chapter number was out of range both times). Such a case is not
  compared: 2 cases.
- **Undefined words.** Where the harness marks words as undefined (`UNINIT`: its two runs
  disagree) the case is compared in all other words and counted in its own column: 346 cases,
  nearly all of them a reallocation that copies bytes `SI_malloc` never set.
- **Outside calls.** Routines of other source files are replaced in the emulator by recorded
  stand-ins and reached in the port through the trait of section 6; the check holds every call
  of the port against the next recorded one (name and arguments) and answers as recorded.
  Five stand-ins have a body, the same on both sides: a new name record with its three
  strings, a formatted full name, the name validator (answers its argument), the suffix of a
  chapter (an address of the binary's own table), the split of a file name.

### 2.2 The dictionaries the cases are made of

By the parameters of `world.Dict` and by chance: no chapter, one, four to six; chapters with
no slot, one, several; slots in use, flagged for deletion (status 1), deleted (2); elements
with and without relation lists; lists empty, full (count = capacity) and with room, so that
additions cross the growth step both ways; relations in every list the rules use, with and
without serial numbers, inactive entries, data, owned animation elements, cursors and marks
left by earlier look-ups; a hierarchy that is a forest, and for the sort both acyclic graphs
and graphs with cycles, the internal dependency relations (−102, −103, −110), a chapter
standing in for CHANNEL_DRIVERS and one for WAVES, chapters of kind 0 and 2; names equal,
prefixes of each other, differing in case, with digits at the end, with underscores; a
relation dictionary with no rule, one, all, in another order of ids, with no sort table, a
stale one, a valid one, and the look-up cache empty, right or stale; `SI_LOAD_OPTIM` on and
off with the chapter's table missing, stale or valid.

Scale: the numbers of this note are of `gen.py … 0.3`: between 3 and 600 cases a routine (600
for `topoSort` and `SI_elemdictTopoSort`, 450 for `si_relAddRelation`), 173 MB of vector
files. A dictionary of a case holds up to about twelve elements; more would only make the
files larger.

### 2.3 What the check does not show

- Cases where the original stops (160: a made-up index outside its chapter, a name record
  missing where the routine does not test for it) are not compared up to the stop.
- The stand-ins answer what the generators make up; what the real routines would answer is
  another package's.
- A read of memory nobody wrote that changes nothing is seen by neither side.
- `SI_secureLicAlloc` is counted up by the original and not by the port (section 1.2); no
  case ends with it changed, so the comparison of globals never meets it.

## 3. Function by function

"Earlier port" names the crates that cite the function. "Defined part": identical in every
word the original defines (see 2.1). "Not compared": the original stopped, or one of the two
rare kinds of 2.1. The check's column "identical given the C library's answers" is left out:
it is 0 for every routine (no case here goes through the library's mathematics; the string
routines and `qsort` are exact stand-ins of the harness).

### `elemdict.c`

| Function | Address | Earlier port | On the image | Cases | Identical | Defined part | Different | Not compared |
|---|---|---|---|---:|---:|---:|---:|---:|
| `filenameCheckSuffix` | 0x08131980 | si-db | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_dbScnGetListNode` | 0x081319f0 | si-db | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_dbListNodeCopyInfo` | 0x08131ba8 | si-scene | yes | 60 | 60 | 0 | 0 | 0 |
| `strcmp_underscore` | 0x08131ce4 | si-ascii | yes | 240 | 240 | 0 | 0 | 0 |
| `SI_compareElemName` | 0x08131d60 | si-ascii | yes | 30 | 30 | 0 | 0 | 0 |
| `elemdictCompareElems` | 0x08131d7c | si-scene | no |  |  |  |  |  |
| `SI_elemlistSortChapterElements` | 0x08132330 | si-scene | no |  |  |  |  |  |
| `elemdictCompareChapNames` | 0x08132564 | si-types | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictSortChapNames` | 0x0813259c | none | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictFindListByName` | 0x081325f4 | si-ascii, si-db | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictInitElem` | 0x081326b0 | si-types | yes | 9 | 9 | 0 | 0 | 0 |
| `SI_elemlistAddElem` | 0x081327c4 | si-types | yes | 150 | 150 | 0 | 0 | 0 |
| `SI_elemdictAddElem` | 0x08132a5c | si-types | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictGetChapName` | 0x08132b04 | si-types | yes | 15 | 15 | 0 | 0 | 0 |
| `SI_elemlistGetElem` | 0x08132b1c | si-types | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictGetElem` | 0x08132b50 | si-types, si-motion | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictGetElemId` | 0x08132b94 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictSetDirty` | 0x08132be8 | si-types | yes | 9 | 9 | 0 | 0 | 0 |
| `SI_elemdictSchemSetPosX` | 0x08132bfc | si-types | yes | 9 | 9 | 0 | 0 | 0 |
| `SI_elemdictSchemSetPosY` | 0x08132c0c | si-types | yes | 9 | 9 | 0 | 0 | 0 |
| `si_elemdictSetFlag` | 0x08132c1c | si-types | yes | 9 | 9 | 0 | 0 | 0 |
| `SI_elemlistClearWorkFlag` | 0x08132c30 | si-types | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemlistClearLowByteWorkFlag` | 0x08132c70 | none | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemlistLowByteWorkFlagAND` | 0x08132cac | si-types | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictCusGetRelData` | 0x08132cf0 | none | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictAddCompleteRelation` | 0x08132dc4 | si-scene | yes | 150 | 138 | 0 | 0 | 12 |
| `SI_elemdictAddRelation` | 0x08132f78 | si-scene | yes | 90 | 84 | 0 | 0 | 6 |
| `SI_elemdictAddUnCompleteRelation` | 0x08132fb4 | si-scene | yes | 120 | 119 | 0 | 0 | 1 |
| `SI_elemdictAddSerialRelation` | 0x08133184 | si-scene | yes | 150 | 150 | 0 | 0 | 0 |
| `SI_elemdictFindSerialRelation` | 0x0813335c | si-scene | yes | 240 | 240 | 0 | 0 | 0 |
| `SI_elemdictSetRelToSerialid` | 0x0813354c | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictRemoveRelationPair` | 0x0813362c | si-scene | yes | 90 | 87 | 0 | 0 | 3 |
| `SI_elemdictDisconnectANDConnectRel` | 0x08133780 | si-scene | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictRemoveUncompleteRelPair` | 0x0813382c | si-scene | yes | 90 | 89 | 0 | 0 | 1 |
| `SI_elemdictRemoveRelationId` | 0x08133998 | si-scene | yes | 90 | 83 | 0 | 0 | 7 |
| `SI_elemdictRemoveSerialRelId` | 0x08133a6c | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictRemoveBackwardRelId` | 0x08133b18 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemlistInit` | 0x08133bc8 | si-types | yes | 6 | 6 | 0 | 0 | 0 |
| `SI_elemdictAddList` | 0x08133c40 | si-types, si-ascii | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemlistCountValidElems` | 0x08133cc0 | si-types | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictLoadChapters` | 0x08133cf0 | si-types | yes | 30 | 0 | 30 | 0 | 0 |
| `SI_elemdictFindElemByName` | 0x081341d4 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_dbSortTblSetDirty` | 0x0813425c | si-types | yes | 18 | 18 | 0 | 0 | 0 |
| `addElemInChapSortTbl` | 0x0813427c | none | yes | 90 | 90 | 0 | 0 | 0 |
| `findElemInChapSortTbl` | 0x081342ec | none | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictFindElemByData` | 0x08134364 | none | yes | 240 | 240 | 0 | 0 | 0 |
| `SI_elemdictFindElemByCB` | 0x08134530 | si-types | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictFindElemByAuxData` | 0x081345a0 | si-types | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictFindElemByDelData` | 0x081345e8 | si-types | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictFindElemByDelAuxData` | 0x0813462c | si-types | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictFindElemByPrjName` | 0x08134678 | si-scene | yes | 150 | 150 | 0 | 0 | 0 |
| `SI_elemdictFindElemByNameOnly` | 0x08134770 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictCheckDuplElemName` | 0x08134804 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictFindElemByFullName` | 0x081348b4 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemlistDump` | 0x08134940 | none | no |  |  |  |  |  |
| `SI_elemdictDump` | 0x08134ba8 | none | no |  |  |  |  |  |
| `elemUpdateHierInfo` | 0x08134c34 | si-types | yes | 60 | 56 | 0 | 0 | 4 |
| `SI_elemdictUpdateHierInfo` | 0x08134cc8 | si-types | yes | 90 | 88 | 0 | 0 | 2 |
| `SI_elemdictGetCurRelation` | 0x08134db0 | si-scene, si-motion | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictGetRelation` | 0x08134dec | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictGetRelationFromIds` | 0x08134e9c | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictResetCurrFlag` | 0x08134f78 | si-scene | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictSetCurrRelPointer` | 0x08134fdc | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictFindRelatedElem` | 0x0813508c | si-scene, si-motion | yes | 120 | 120 | 0 | 0 | 0 |
| `SI_elemdictFindNextRelatedElem` | 0x08135154 | si-scene, si-motion | yes | 120 | 120 | 0 | 0 | 0 |
| `SI_elemdictFindRelatedElem2` | 0x08135220 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictFindRangeNextRelatedElem2` | 0x081352ec | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictFindRangeRelatedElem` | 0x081353c4 | si-scene, si-motion | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictFindRangeNextRelatedElem` | 0x08135484 | si-scene, si-motion | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictFindRelatedElemSerial` | 0x08135554 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictFindNextRelatedElemSerial` | 0x08135620 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictRelationsCount` | 0x08135700 | si-scene | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictRangeRelationsCount` | 0x08135770 | si-scene | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictFindSourceRelation` | 0x081357dc | si-scene | yes | 120 | 120 | 0 | 0 | 0 |
| `SI_elemdictSerialRelCount` | 0x0813590c | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictInit` | 0x081359bc | si-types | yes | 6 | 6 | 0 | 0 | 0 |
| `SI_dbIconFree` | 0x08135a8c | none | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictSetDeleteElem` | 0x08135ad4 | si-scene | yes | 180 | 178 | 0 | 0 | 2 |
| `SI_elemdictDeleteChapterElems` | 0x08135c2c | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemlistDisposeElements` | 0x08135ca0 | none | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemlistDispose` | 0x08135fd4 | si-types | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictDispose` | 0x0813606c | si-types | yes | 60 | 60 | 0 | 0 | 0 |
| `relBackUpDispose` | 0x08136128 | si-types | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictChangeDscName` | 0x081361d8 | si-scene | yes | 120 | 120 | 0 | 0 | 0 |
| `SI_elemdictPropagatePrfxAndShare` | 0x08136350 | none | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictChangeName` | 0x08136450 | none | yes | 180 | 178 | 0 | 0 | 2 |
| `SI_elemdictGetNewNameId` | 0x081366a0 | none | yes | 150 | 150 | 0 | 0 | 0 |
| `SI_elemdictGetNewName` | 0x0813697c | none | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictGetNewPrefix` | 0x081369a4 | none | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictCreateDefaultDsc` | 0x081369cc | si-scene | yes | 45 | 45 | 0 | 0 | 0 |
| `elemdictSetVersion` | 0x08136a60 | none | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictSetSubstitution` | 0x08136b50 | none | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictSetNewFlag` | 0x08136c48 | si-types | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictSetSelection` | 0x08136cb0 | none | yes | 18 | 18 | 0 | 0 | 0 |
| `SI_elemdictIsElemUsed` | 0x08136d10 | si-types | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictCheckEmptyDescription` | 0x08136db0 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictAddSelElemsFromDb` | 0x08136e98 | none | yes | 90 | 55 | 9 | 0 | 26 |
| `SI_elemdictMoveElemsFromOutsideChapter` | 0x08137064 | none | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictRemoveAllSubScenes` | 0x0813720c | si-scene | yes | 90 | 89 | 0 | 0 | 1 |
| `SI_elemdictRemoveAllInvalidElems` | 0x081373d0 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictRemoveAllEmptyAnim` | 0x08137490 | none | yes | 90 | 87 | 0 | 0 | 3 |
| `SI_elemdictRecursiveSelect` | 0x0813757c | none | yes | 150 | 150 | 0 | 0 | 0 |
| `si_elemdictResetOriginDB` | 0x08137884 | none | yes | 45 | 45 | 0 | 0 | 0 |
| `si_elemdictSetOriginDB` | 0x08137918 | si-scene | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictSortAnimElements` | 0x08137980 | si-scene | yes | 150 | 150 | 0 | 0 | 0 |
| `SI_elemdictAdd` | 0x08137b00 | none | yes | 18 | 0 | 18 | 0 | 0 |
| `SI_elemdictRemove` | 0x08137bdc | si-types | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictGetNb` | 0x08137c40 | si-types | yes | 12 | 12 | 0 | 0 | 0 |
| `SI_elemdictExists` | 0x08137c70 | si-types | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictCompareElemIdent` | 0x08137c9c | si-types | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictCopyElements` | 0x08137cd0 | none | yes | 60 | 9 | 51 | 0 | 0 |
| `SI_elemdictCopyRenderRelations` | 0x08137ddc | none | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictRelationCheck` | 0x08137f0c | si-scene | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictGetElemFromData` | 0x08137f94 | none | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictFindSon` | 0x08137ff4 | si-scene | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictFindNextSon` | 0x08138030 | si-scene | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictGetElemFromScene` | 0x08138068 | si-scene | yes | 18 | 18 | 0 | 0 | 0 |
| `SI_elemdictGetFirstRelatedElem` | 0x08138094 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictGetNextRelatedElem` | 0x08138104 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictGetNthRelatedElem` | 0x08138190 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictGetCurRelationIndex` | 0x08138224 | si-types | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictSetCurRelationIndex` | 0x0813825c | si-types | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictAddUDFElemRelation` | 0x08138290 | none | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictRemoveUDFElemRelation` | 0x08138384 | none | yes | 90 | 90 | 0 | 0 | 0 |
| `checkNSEStatus` | 0x08138434 | none | yes | 90 | 90 | 0 | 0 | 0 |

### `enmdict.c`

| Function | Address | Earlier port | On the image | Cases | Identical | Defined part | Different | Not compared |
|---|---|---|---|---:|---:|---:|---:|---:|
| `si_enmdictAddValue` | 0x08138520 | si-types | yes | 45 | 45 | 0 | 0 | 0 |
| `si_enmdictAddEnumType` | 0x08138588 | si-types | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_enmdictInit` | 0x081385f4 | si-types | yes | 6 | 6 | 0 | 0 | 0 |
| `SI_enmdictDispose` | 0x0813860c | si-types | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_enmdictDump` | 0x081386fc | none | no |  |  |  |  |  |
| `SI_enmdictEnumValGetByCode` | 0x081387c0 | si-types | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_enmdictEnumCodeGetByValue` | 0x08138834 | si-types, si-expr | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_enmdictEnumTypeGetName` | 0x08138888 | si-types, si-ascii | yes | 60 | 52 | 0 | 0 | 8 |
| `SI_enmdictEnumValGetByAnyCode` | 0x081388d8 | si-types, si-expr | yes | 120 | 120 | 0 | 0 | 0 |
| `SI_enmdictEnumValGetNameById` | 0x081389a4 | si-types | yes | 90 | 90 | 0 | 0 | 0 |

### `init_dict.c`

| Function | Address | Earlier port | On the image | Cases | Identical | Defined part | Different | Not compared |
|---|---|---|---|---:|---:|---:|---:|---:|
| `SI_databaseDirectoryLoad` | 0x08108130 | si-db | no |  |  |  |  |  |
| `SI_initDictionaries` | 0x0810824c | none | no |  |  |  |  |  |

### `updlist.c`

| Function | Address | Earlier port | On the image | Cases | Identical | Defined part | Different | Not compared |
|---|---|---|---|---:|---:|---:|---:|---:|
| `si_updlistAddArgument` | 0x081413e0 | si-scene, si-motion | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_updlistInit` | 0x08141468 | si-types, si-motion | yes | 3 | 3 | 0 | 0 | 0 |
| `si_updlistAddNode` | 0x08141488 | si-scene, si-motion | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_updlistDispose` | 0x08141554 | si-scene, si-motion | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_updlistDump` | 0x081415e0 | none | no |  |  |  |  |  |

### `toposort.c`

| Function | Address | Earlier port | On the image | Cases | Identical | Defined part | Different | Not compared |
|---|---|---|---|---:|---:|---:|---:|---:|
| `elemdictFindFather` | 0x0813feb0 | si-scene | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_elemdictTopoSortDump` | 0x0813fef4 | none | no |  |  |  |  |  |
| `relTopoSortInitChapter` | 0x0813ff44 | si-scene, si-motion | yes | 30 | 30 | 0 | 0 | 0 |
| `SI_relTopoSortInit` | 0x0813ffbc | si-scene, si-motion | yes | 30 | 30 | 0 | 0 | 0 |
| `relCreateArguments` | 0x0813fff8 | si-scene, si-motion | yes | 120 | 117 | 3 | 0 | 0 |
| `topoSort` | 0x081400cc | si-scene, si-motion | yes | 600 | 593 | 7 | 0 | 0 |
| `elemdictTopoSortChapter` | 0x0814070c | si-scene, si-motion | yes | 240 | 233 | 7 | 0 | 0 |
| `SI_elemdictTopoSort` | 0x08140828 | si-scene, si-motion | yes | 600 | 522 | 78 | 0 | 0 |
| `SI_elemdictTopoSortElement` | 0x081409bc | si-motion | yes | 240 | 228 | 11 | 0 | 1 |
| `SI_elemdictTopoSortSelectedElements` | 0x08140b28 | none | yes | 240 | 226 | 14 | 0 | 0 |
| `SI_elemdictCheckCycle` | 0x08140d80 | si-scene, si-motion | yes | 180 | 180 | 0 | 0 | 0 |
| `SI_elemdictUpdListSetDirty` | 0x08140dac | si-scene, si-motion | yes | 6 | 6 | 0 | 0 | 0 |
| `SI_elemdictFindRoot` | 0x08140dc4 | si-scene | yes | 90 | 51 | 0 | 0 | 39 |
| `elemdictSortHier` | 0x08140e54 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictSortHierElements` | 0x08140f30 | si-scene | yes | 150 | 150 | 0 | 0 | 0 |
| `relGetRelationList` | 0x08141060 | none | yes | 90 | 66 | 24 | 0 | 0 |
| `SI_elemdictGetAffectedByRelations` | 0x081410e4 | none | yes | 60 | 47 | 13 | 0 | 0 |
| `SI_elemdictGetAffectsRelations` | 0x08141140 | none | yes | 60 | 46 | 14 | 0 | 0 |
| `SI_elemdictTopoSortElemByFcv` | 0x0814119c | none | yes | 180 | 160 | 20 | 0 | 0 |

### `reldict.c`

| Function | Address | Earlier port | On the image | Cases | Identical | Defined part | Different | Not compared |
|---|---|---|---|---:|---:|---:|---:|---:|
| `SI_reldictAddRelation` | 0x08139b80 | si-types | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_reldictInit` | 0x08139db0 | si-types | yes | 3 | 3 | 0 | 0 | 0 |
| `SI_reldictDispose` | 0x08139dd8 | si-types | yes | 45 | 45 | 0 | 0 | 0 |
| `findElemInRelSortTbl` | 0x08139e88 | si-types, si-ascii | yes | 90 | 90 | 0 | 0 | 0 |
| `addElemInRelSortTbl` | 0x08139ef8 | si-types, si-ascii | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_reldictGetRelIndexFromId` | 0x08139f68 | si-types, si-ascii | yes | 180 | 180 | 0 | 0 | 0 |
| `SI_reldictGetRelName` | 0x0813a0f4 | si-types, si-ascii | yes | 60 | 60 | 0 | 0 | 0 |
| `si_reldictGetRelSub` | 0x0813a12c | si-types, si-ascii | yes | 120 | 111 | 0 | 0 | 9 |
| `SI_reldictGetRelNbRules` | 0x0813a1ac | si-types, si-ascii | yes | 90 | 79 | 0 | 0 | 11 |
| `SI_reldictCheckRelAnim` | 0x0813a1fc | none | yes | 90 | 81 | 0 | 0 | 9 |
| `SI_reldictGetRelAttributes` | 0x0813a28c | si-types, si-ascii | yes | 120 | 108 | 12 | 0 | 0 |
| `SI_reldictCusGetRelAttributes` | 0x0813a30c | si-scene | yes | 120 | 120 | 0 | 0 | 0 |
| `SI_reldictGetRelRenderFlag` | 0x0813a3f4 | si-types, si-ascii | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_reldictGetRelIncmodeFlag` | 0x0813a428 | si-types, si-ascii | yes | 45 | 44 | 1 | 0 | 0 |
| `SI_reldictGetRelChapGrp` | 0x0813a464 | si-types, si-ascii | yes | 60 | 60 | 0 | 0 | 0 |
| `reldictGetRelDopeDisp` | 0x0813a4b4 | si-types, si-ascii | yes | 45 | 45 | 0 | 0 | 0 |
| `SI_reldictGetRelRank` | 0x0813a4e8 | si-types, si-ascii | yes | 45 | 45 | 0 | 0 | 0 |
| `si_reldictGetRelUpdlattr` | 0x0813a51c | si-types, si-ascii | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_reldictIsRelActive` | 0x0813a56c | si-types | yes | 60 | 59 | 1 | 0 | 0 |
| `SI_reldictGetRelIdByName` | 0x0813a5bc | si-types, si-ascii | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_reldictActiveSet` | 0x0813a60c | none | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_reldictNbRelForDeactCount` | 0x0813a714 | none | yes | 60 | 54 | 0 | 0 | 6 |
| `SI_reldictCheckUnCompleteRelation` | 0x0813a770 | si-types, si-ascii | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_reldictDump` | 0x0813a7bc | none | no |  |  |  |  |  |
| `reldictConvertGroupToken` | 0x0813a878 | si-ascii | no |  |  |  |  |  |
| `reldictConvertScopeToken` | 0x0813aa0c | si-ascii | no |  |  |  |  |  |
| `reldictConvertApplicationToken` | 0x0813aa9c | si-ascii | no |  |  |  |  |  |
| `reldictConvertOpCategToken` | 0x0813aae8 | si-ascii | no |  |  |  |  |  |
| `reldictConvertRenderToken` | 0x0813abd8 | si-ascii | no |  |  |  |  |  |
| `reldictConvertIncmodeToken` | 0x0813ac18 | si-ascii | no |  |  |  |  |  |
| `reldictConvertAttribIncmodeToken` | 0x0813ace4 | si-ascii | no |  |  |  |  |  |
| `reldictConvertTypeToken` | 0x0813ad5c | si-ascii | no |  |  |  |  |  |
| `reldictConvertRangeToken` | 0x0813aeb0 | si-ascii | no |  |  |  |  |  |
| `reldictConvertConditionToken` | 0x0813b374 | si-ascii | no |  |  |  |  |  |
| `reldictConvertChapterToken` | 0x0813b58c | si-ascii | no |  |  |  |  |  |
| `reldictConvertAccessModeToken` | 0x0813b5e8 | si-ascii | no |  |  |  |  |  |
| `reldictCusConvertFanoutToken` | 0x0813b644 | si-ascii | no |  |  |  |  |  |
| `reldictCusReadPickMsg` | 0x0813b734 | si-ascii | no |  |  |  |  |  |
| `reldictReadDescription` | 0x0813b910 | si-ascii | no |  |  |  |  |  |
| `reldictCusReadCommonSection` | 0x0813b9f0 | si-ascii | no |  |  |  |  |  |
| `reldictReadOneAttribute` | 0x0813bbd8 | si-ascii | no |  |  |  |  |  |
| `reldictReadOneSection` | 0x0813c330 | si-ascii | no |  |  |  |  |  |
| `reldictReadOneOpResult` | 0x0813c588 | si-ascii | no |  |  |  |  |  |
| `reldictReadOneOpArgument` | 0x0813c63c | si-ascii | no |  |  |  |  |  |
| `reldictReadOpDefinition` | 0x0813ca90 | si-ascii | no |  |  |  |  |  |
| `reldictReadElemsAttributes` | 0x0813cc20 | si-ascii | no |  |  |  |  |  |
| `attdictGetAttribGroup` | 0x0813d098 | si-ascii | no |  |  |  |  |  |
| `reldictReadOneOperatorAttribute` | 0x0813d12c | none | no |  |  |  |  |  |
| `SI_reldictReadOperatorAttributes` | 0x0813d39c | none | no |  |  |  |  |  |
| `SI_reldictReadOneOperator` | 0x0813d75c | none | no |  |  |  |  |  |
| `reldictReadOperators` | 0x0813df94 | si-ascii | no |  |  |  |  |  |
| `reldictReadEnumeratedTypes` | 0x0813e040 | si-ascii | no |  |  |  |  |  |
| `SI_reldictReadNewRelations` | 0x0813e2d0 | si-ascii | no |  |  |  |  |  |
| `SI_reldictReadRelations` | 0x0813e318 | si-ascii | no |  |  |  |  |  |
| `SI_dbLoadRelationDictionary` | 0x0813ea84 | none | no |  |  |  |  |  |
| `SI_reldictCusOpDump` | 0x0813eae0 | none | no |  |  |  |  |  |
| `SI_reldictGetRelIndexFromName` | 0x0813eae8 | si-types | yes | 90 | 90 | 0 | 0 | 0 |
| `SI_reldictGetNbRes` | 0x0813eb7c | none | no |  |  |  |  |  |
| `SI_reldictGetNbArgs` | 0x0813ec80 | none | no |  |  |  |  |  |
| `SI_reldictGetNbDistinctRes` | 0x0813ed60 | none | no |  |  |  |  |  |
| `SI_reldictGetNbDistinctArgs` | 0x0813eeac | none | no |  |  |  |  |  |
| `SI_reldictGetNextDistinctRes` | 0x0813efac | none | no |  |  |  |  |  |
| `SI_reldictGetNextDistinctArg` | 0x0813eff4 | none | no |  |  |  |  |  |
| `SI_reldictGetNbResInstances` | 0x0813f04c | none | no |  |  |  |  |  |
| `SI_reldictGetNbArgInstances` | 0x0813f17c | none | no |  |  |  |  |  |
| `SI_reldictGetResPickMessage` | 0x0813f2f8 | none | no |  |  |  |  |  |
| `SI_reldictGetArgPickMessage` | 0x0813f404 | none | no |  |  |  |  |  |
| `SI_reldictGetResCond` | 0x0813f550 | none | no |  |  |  |  |  |
| `SI_reldictGetArgCond` | 0x0813f654 | none | no |  |  |  |  |  |
| `SI_reldictGetChap` | 0x0813f7a0 | none | no |  |  |  |  |  |
| `SI_reldictGetGrp` | 0x0813f7e8 | none | no |  |  |  |  |  |
| `reldictOpArgInstanceGetGrp` | 0x0813f82c | none | no |  |  |  |  |  |
| `reldictOpResInstanceGetGrp` | 0x0813f880 | none | no |  |  |  |  |  |
| `SI_reldictOpParamInstanceGetGrp` | 0x0813f8bc | none | no |  |  |  |  |  |
| `SI_reldictGetCond` | 0x0813f904 | none | no |  |  |  |  |  |
| `SI_reldictGetScope` | 0x0813f948 | none | no |  |  |  |  |  |
| `SI_reldictGetName` | 0x0813f98c | none | no |  |  |  |  |  |
| `SI_reldictGetMaxFanout` | 0x0813f9cc | si-types | yes | 60 | 50 | 6 | 0 | 4 |
| `SI_reldictIsNewSrc` | 0x0813fa18 | none | yes | 45 | 29 | 14 | 0 | 2 |
| `SI_reldictGetRelRankByIndex` | 0x0813fa44 | none | yes | 45 | 28 | 13 | 0 | 4 |
| `lookForExistingResName` | 0x0813fa74 | none | no |  |  |  |  |  |
| `lookForExistingArgName` | 0x0813fb48 | none | no |  |  |  |  |  |
| `lookForExistingOatName` | 0x0813fbe0 | none | no |  |  |  |  |  |
| `lookForExistingFcurveName` | 0x0813fc58 | none | no |  |  |  |  |  |
| `searchAttdictByFcvName` | 0x0813fd20 | none | no |  |  |  |  |  |
| `token2Uppercase` | 0x0813fd88 | si-ascii | no |  |  |  |  |  |
| `attdictAttributeGetByName` | 0x0813fddc | si-ascii | no |  |  |  |  |  |
| `enmdictEnumValGetByCode` | 0x0813fe30 | si-ascii | no |  |  |  |  |  |

### `relations.c`

| Function | Address | Earlier port | On the image | Cases | Identical | Defined part | Different | Not compared |
|---|---|---|---|---:|---:|---:|---:|---:|
| `si_relGroupDump` | 0x081391a0 | none | no |  |  |  |  |  |
| `si_relAllocate` | 0x081392a0 | si-types | yes | 6 | 6 | 0 | 0 | 0 |
| `si_relRemoveARelation` | 0x0813930c | si-scene | yes | 150 | 150 | 0 | 0 | 0 |
| `si_relRemoveARelationId` | 0x081394b8 | si-scene | yes | 150 | 150 | 0 | 0 | 0 |
| `SI_relRemoveRelations` | 0x081396d8 | si-scene | yes | 90 | 90 | 0 | 0 | 0 |
| `si_relAddRelation` | 0x0813987c | si-types | yes | 450 | 450 | 0 | 0 | 0 |
| `si_relCheck` | 0x08139b24 | si-types | yes | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictGetRelSerialNumber` | 0x08139b68 | si-scene | yes | 6 | 6 | 0 | 0 | 0 |

### `attdict.c`

| Function | Address | Earlier port | On the image | Cases | Identical | Defined part | Different | Not compared |
|---|---|---|---|---:|---:|---:|---:|---:|
| `si_attdictInitAttribute` | 0x0811e7d0 | si-types | no |  |  |  |  |  |
| `attdictInitAttribGroup` | 0x0811e890 | si-types | no |  |  |  |  |  |
| `SI_attdictInit` | 0x0811e8e0 | si-types | no |  |  |  |  |  |
| `si_attdictAllocateFcurveArray` | 0x0811e91c | si-types | no |  |  |  |  |  |
| `si_attdictAddAttribute` | 0x0811e984 | si-types | no |  |  |  |  |  |
| `si_attdictAddAttribGroup` | 0x0811ea5c | si-types | no |  |  |  |  |  |
| `si_attdictAddAttribSubGroup` | 0x0811eae0 | si-types | no |  |  |  |  |  |
| `attdictAttribGroupDispose` | 0x0811eb58 | si-types | no |  |  |  |  |  |
| `SI_attdictDispose` | 0x0811ec1c | si-types | no |  |  |  |  |  |
| `si_attdictAttribSubGroupGetByName` | 0x0811ed88 | si-ascii | no |  |  |  |  |  |
| `SI_attdictAttribGroupGetByName` | 0x0811edf8 | si-ascii | no |  |  |  |  |  |
| `SI_attdictAttribGroupGetByIndex` | 0x0811ee3c | none | no |  |  |  |  |  |
| `SI_attdictAttributeGetByName` | 0x0811eeb4 | none | no |  |  |  |  |  |
| `SI_attdictAttributeGetByIndex` | 0x0811ef0c | si-types | no |  |  |  |  |  |
| `SI_attdictAttributeSetAccessMode` | 0x0811efb4 | none | no |  |  |  |  |  |
| `SI_attdictAttributeGetAccessMode` | 0x0811efe0 | none | no |  |  |  |  |  |
| `SI_attdictEvalCond` | 0x0811f01c | none | no |  |  |  |  |  |
| `attdictEvalGroupCond` | 0x0811f0ec | none | no |  |  |  |  |  |
| `SI_attdictValidateAttr` | 0x0811f118 | none | no |  |  |  |  |  |
| `SI_attdictRangeAdjust` | 0x0811f1c0 | none | no |  |  |  |  |  |
| `SI_attdictSetFctsPointers` | 0x0811f370 | none | no |  |  |  |  |  |
| `SI_attdictAttGrpGetAttrib` | 0x0811f754 | none | no |  |  |  |  |  |
| `SI_attdictAttGrpGetNbAttributes` | 0x0811f7b4 | none | no |  |  |  |  |  |
| `SI_attdictAttGrpGetAttribute` | 0x0811f7cc | none | no |  |  |  |  |  |
| `attdictAttribGroupDump` | 0x0811f7fc | none | no |  |  |  |  |  |
| `SI_attdictDump` | 0x0811fa00 | none | no |  |  |  |  |  |
| `attdictAttribGroupUpdateIndexes` | 0x0811fa78 | none | no |  |  |  |  |  |
| `SI_attdictUpdateIndexes` | 0x0811fb3c | none | no |  |  |  |  |  |
| `SI_attdictCountAnimatedParams` | 0x0811fb84 | si-scene | no |  |  |  |  |  |
| `attdictCompareIds` | 0x0811fbb8 | si-scene, si-expr | no |  |  |  |  |  |
| `attdictInitFcvIndexes` | 0x0811fc14 | si-scene, si-expr | no |  |  |  |  |  |
| `SI_attdictSortAttribFromFcurveIds` | 0x0811fc80 | si-scene, si-expr | no |  |  |  |  |  |
| `attdictBSearchCompareIds` | 0x0811fd10 | si-scene | no |  |  |  |  |  |
| `SI_attdictGetGrpIndexFromFcurve` | 0x0811fd8c | none | no |  |  |  |  |  |
| `SI_attdictFindFcurve` | 0x0811fdfc | si-scene, si-expr | no |  |  |  |  |  |
| `SI_attdictSetFcurveFlag` | 0x0811ff18 | none | no |  |  |  |  |  |
| `SI_attdictGetFcurveFlag` | 0x0811ff64 | none | no |  |  |  |  |  |
| `SI_attdictGetFcurveFromName` | 0x0811ffa8 | si-scene, si-expr | no |  |  |  |  |  |
| `SI_attdictGetGroupCustomFcurve` | 0x081200b4 | si-scene | no |  |  |  |  |  |
| `SI_attdictGetGroupFromFcurve` | 0x081200c0 | si-scene | no |  |  |  |  |  |
| `SI_attdictGetNameFromFcurve` | 0x0812012c | si-scene, si-expr | no |  |  |  |  |  |
| `SI_attdictGetChapidFromFcurve` | 0x08120194 | si-scene | no |  |  |  |  |  |
| `getOldSubdef` | 0x08120214 | si-scene | no |  |  |  |  |  |
| `SI_attdictGetInfoFromFcurve` | 0x08120264 | si-scene | no |  |  |  |  |  |
| `SI_attdictGetFcurveFromAttId` | 0x08120320 | si-scene | no |  |  |  |  |  |
| `SI_attdictGetNbFcurvesForAttId` | 0x0812037c | si-scene | no |  |  |  |  |  |
| `SI_attdictAttribGetDefWidth` | 0x081203d0 | si-types | no |  |  |  |  |  |
| `SI_attdictGetAttGrpByPath` | 0x08120430 | none | no |  |  |  |  |  |
| `SI_attdictGetAttSubGrpByPath` | 0x081204f4 | none | no |  |  |  |  |  |
| `SI_attdictAttGrpGetNbAnimatableAttribs` | 0x081205b4 | none | no |  |  |  |  |  |
| `SI_attdictAttributeIsAnimatable` | 0x081205fc | si-types | no |  |  |  |  |  |
| `SI_attdictCompareGrpIndex` | 0x08120610 | si-types | no |  |  |  |  |  |

### Totals

| Source file | Functions | On the image | of them ported before | new | Checked by emulation | Cases | Identical | Defined part | Different | Not compared |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `elemdict.c` | 125 | 121 | 93 | 28 | 121 | 8991 | 8813 | 108 | 0 | 70 |
| `enmdict.c` | 10 | 9 | 9 | 0 | 9 | 606 | 598 | 0 | 0 | 8 |
| `init_dict.c` | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `updlist.c` | 5 | 4 | 4 | 0 | 4 | 213 | 213 | 0 | 0 | 0 |
| `toposort.c` | 19 | 18 | 13 | 5 | 18 | 3036 | 2805 | 191 | 0 | 40 |
| `reldict.c` | 88 | 27 | 22 | 5 | 27 | 1968 | 1876 | 47 | 0 | 45 |
| `relations.c` | 8 | 7 | 7 | 0 | 7 | 912 | 912 | 0 | 0 | 0 |
| `attdict.c` | 52 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| all | 309 | 186 | 148 | 38 | 186 | 15726 | 15217 | 346 | 0 | 163 |

### Not on the image yet

- `elemdict.c` (4): `elemdictCompareElems` (si-scene), `SI_elemlistSortChapterElements` (si-scene), `SI_elemlistDump`, `SI_elemdictDump`
- `enmdict.c` (1): `SI_enmdictDump`
- `init_dict.c` (2): `SI_databaseDirectoryLoad` (si-db), `SI_initDictionaries`
- `updlist.c` (1): `SI_updlistDump`
- `toposort.c` (1): `SI_elemdictTopoSortDump`
- `reldict.c` (61): `SI_reldictDump`, `reldictConvertGroupToken` (si-ascii), `reldictConvertScopeToken` (si-ascii), `reldictConvertApplicationToken` (si-ascii), `reldictConvertOpCategToken` (si-ascii), `reldictConvertRenderToken` (si-ascii), `reldictConvertIncmodeToken` (si-ascii), `reldictConvertAttribIncmodeToken` (si-ascii), `reldictConvertTypeToken` (si-ascii), `reldictConvertRangeToken` (si-ascii), `reldictConvertConditionToken` (si-ascii), `reldictConvertChapterToken` (si-ascii), `reldictConvertAccessModeToken` (si-ascii), `reldictCusConvertFanoutToken` (si-ascii), `reldictCusReadPickMsg` (si-ascii), `reldictReadDescription` (si-ascii), `reldictCusReadCommonSection` (si-ascii), `reldictReadOneAttribute` (si-ascii), `reldictReadOneSection` (si-ascii), `reldictReadOneOpResult` (si-ascii), `reldictReadOneOpArgument` (si-ascii), `reldictReadOpDefinition` (si-ascii), `reldictReadElemsAttributes` (si-ascii), `attdictGetAttribGroup` (si-ascii), `reldictReadOneOperatorAttribute`, `SI_reldictReadOperatorAttributes`, `SI_reldictReadOneOperator`, `reldictReadOperators` (si-ascii), `reldictReadEnumeratedTypes` (si-ascii), `SI_reldictReadNewRelations` (si-ascii), `SI_reldictReadRelations` (si-ascii), `SI_dbLoadRelationDictionary`, `SI_reldictCusOpDump`, `SI_reldictGetNbRes`, `SI_reldictGetNbArgs`, `SI_reldictGetNbDistinctRes`, `SI_reldictGetNbDistinctArgs`, `SI_reldictGetNextDistinctRes`, `SI_reldictGetNextDistinctArg`, `SI_reldictGetNbResInstances`, `SI_reldictGetNbArgInstances`, `SI_reldictGetResPickMessage`, `SI_reldictGetArgPickMessage`, `SI_reldictGetResCond`, `SI_reldictGetArgCond`, `SI_reldictGetChap`, `SI_reldictGetGrp`, `reldictOpArgInstanceGetGrp`, `reldictOpResInstanceGetGrp`, `SI_reldictOpParamInstanceGetGrp`, `SI_reldictGetCond`, `SI_reldictGetScope`, `SI_reldictGetName`, `lookForExistingResName`, `lookForExistingArgName`, `lookForExistingOatName`, `lookForExistingFcurveName`, `searchAttdictByFcvName`, `token2Uppercase` (si-ascii), `attdictAttributeGetByName` (si-ascii), `enmdictEnumValGetByCode` (si-ascii)
- `relations.c` (1): `si_relGroupDump`
- `attdict.c` (52): `si_attdictInitAttribute` (si-types), `attdictInitAttribGroup` (si-types), `SI_attdictInit` (si-types), `si_attdictAllocateFcurveArray` (si-types), `si_attdictAddAttribute` (si-types), `si_attdictAddAttribGroup` (si-types), `si_attdictAddAttribSubGroup` (si-types), `attdictAttribGroupDispose` (si-types), `SI_attdictDispose` (si-types), `si_attdictAttribSubGroupGetByName` (si-ascii), `SI_attdictAttribGroupGetByName` (si-ascii), `SI_attdictAttribGroupGetByIndex`, `SI_attdictAttributeGetByName`, `SI_attdictAttributeGetByIndex` (si-types), `SI_attdictAttributeSetAccessMode`, `SI_attdictAttributeGetAccessMode`, `SI_attdictEvalCond`, `attdictEvalGroupCond`, `SI_attdictValidateAttr`, `SI_attdictRangeAdjust`, `SI_attdictSetFctsPointers`, `SI_attdictAttGrpGetAttrib`, `SI_attdictAttGrpGetNbAttributes`, `SI_attdictAttGrpGetAttribute`, `attdictAttribGroupDump`, `SI_attdictDump`, `attdictAttribGroupUpdateIndexes`, `SI_attdictUpdateIndexes`, `SI_attdictCountAnimatedParams` (si-scene), `attdictCompareIds` (si-scene, si-expr), `attdictInitFcvIndexes` (si-scene, si-expr), `SI_attdictSortAttribFromFcurveIds` (si-scene, si-expr), `attdictBSearchCompareIds` (si-scene), `SI_attdictGetGrpIndexFromFcurve`, `SI_attdictFindFcurve` (si-scene, si-expr), `SI_attdictSetFcurveFlag`, `SI_attdictGetFcurveFlag`, `SI_attdictGetFcurveFromName` (si-scene, si-expr), `SI_attdictGetGroupCustomFcurve` (si-scene), `SI_attdictGetGroupFromFcurve` (si-scene), `SI_attdictGetNameFromFcurve` (si-scene, si-expr), `SI_attdictGetChapidFromFcurve` (si-scene), `getOldSubdef` (si-scene), `SI_attdictGetInfoFromFcurve` (si-scene), `SI_attdictGetFcurveFromAttId` (si-scene), `SI_attdictGetNbFcurvesForAttId` (si-scene), `SI_attdictAttribGetDefWidth` (si-types), `SI_attdictGetAttGrpByPath`, `SI_attdictGetAttSubGrpByPath`, `SI_attdictAttGrpGetNbAnimatableAttribs`, `SI_attdictAttributeIsAnimatable` (si-types), `SI_attdictCompareGrpIndex` (si-types)

## 4. Against the earlier ports

### 4.1 `si-motion`'s 19 functions, on the same vectors

`si-motion` was moved onto `si-mem`'s image while this package was written, so its
dictionary functions (9 of `toposort.c`, 4 of `updlist.c`, 6 of `elemdict.c`; untyped, by
named offsets) take a case of this package as it is. `dict-emucheck` built with
`--features earlier` and run with `DICT_EARLIER=motion` sends the same vector files through
them; what they reach through their host trait is answered by this crate's functions and by
the case's recorded outside calls.

| Function | Cases | Identical | Defined part | Different | Not compared |
|---|---:|---:|---:|---:|---:|
| `SI_elemdictGetElem` | 45 | 45 | 0 | 0 | 0 |
| `SI_elemdictGetCurRelation` | 60 | 60 | 0 | 0 | 0 |
| `SI_elemdictFindRelatedElem` | 120 | 120 | 0 | 0 | 0 |
| `SI_elemdictFindNextRelatedElem` | 120 | 120 | 0 | 0 | 0 |
| `SI_elemdictFindRangeRelatedElem` | 90 | 90 | 0 | 0 | 0 |
| `SI_elemdictFindRangeNextRelatedElem` | 90 | 90 | 0 | 0 | 0 |
| `SI_updlistInit` | 3 | 3 | 0 | 0 | 0 |
| `si_updlistAddArgument` | 60 | 60 | 0 | 0 | 0 |
| `si_updlistAddNode` | 90 | 90 | 0 | 0 | 0 |
| `SI_updlistDispose` | 60 | 60 | 0 | 0 | 0 |
| `relTopoSortInitChapter` | 30 | 30 | 0 | 0 | 0 |
| `SI_relTopoSortInit` | 30 | 30 | 0 | 0 | 0 |
| `relCreateArguments` | 120 | 0 | 0 | 0 | 120 |
| `topoSort` | 600 | 589 | 0 | 0 | 11 |
| `elemdictTopoSortChapter` | 240 | 231 | 0 | 0 | 9 |
| `SI_elemdictTopoSort` | 600 | 496 | 10 | 0 | 94 |
| `SI_elemdictTopoSortElement` | 240 | 225 | 0 | 0 | 15 |
| `SI_elemdictCheckCycle` | 180 | 180 | 0 | 0 | 0 |
| `SI_elemdictUpdListSetDirty` | 6 | 6 | 0 | 0 | 0 |
| all | 2,784 | 2,525 | 10 | 0 | 249 |

Both ports agree with the emulated original and so with each other. The 249 not compared:
248 cases in which a run of entries got arguments. `si-motion` works out the curve of a
relation entry itself (its own port of `SI_edGetFcvFromRelation (0x080ee6f0)`, which reads
the current dictionary), where this package asks the outside world and the vectors answer by
a stand-in; that is a difference of where the boundary lies, not of behaviour, and those
cases cannot be compared this way (all of `relCreateArguments` among them). 1 case is the
uninitialised return of `SI_elemdictTopoSortElement`, where both ports answer 0.

What the image form adds over `si-motion`'s: typed records instead of offsets, the frees and
blocks compared (its own check compares buffers), and the 167 functions it does not have.

### 4.2 The store-based ports, on the corpus

`dict-check <dir>` (built in `crates/si-scene` since section 10.1; it was this crate's
behind a feature `corpus` when the numbers below were first taken, and gives the same
ones now) loads every scene description under a directory with `si-scene` (which
still works on `si-types`' stores), copies the loaded dictionaries into an image through an
adapter marked `// adapter: not in place` (the blocks are made in an order of the adapter's
own; objects stay the stores' handles, which the dictionary routines never follow), and asks
every read-only question twice. Over `re/bin/si3d40-ia/installed`: 343 descriptions, 334
loaded completely and compared.

| Question | Image function | Store-based port | Asked | Disagree |
|---|---|---|---:|---:|
| rule index by relation id | `reldict::get_rel_index_from_id` | `RelDictStore::get_rel_index_from_id` | 89,848 | 0 |
| number of rules; rule attributes; active flags; name; rank, render, increment mode, dope value | `reldict::…` | `RelDictStore::…` | 444,230 | 0 |
| elements in use per chapter; chapter by name | `elemdict::elemlist_count_valid_elems`, `find_list_by_name` | `ElemListStore::count_valid_elems`, `ElemDictStore::find_list_by_name` | 24,048 | 0 |
| element by index (from −1 to one past the end) | `elemdict::get_elem` | `ElemDictStore::get_elem` | 64,073 | 0 |
| element by object, by second object, slot by either | `elemdict::find_elem_by_data`, `…by_aux_data`, `…by_del_data`, `…by_del_aux_data` | `si_scene::animrel::find_elem_by_data`, `ElemDictStore::…` | 120,075 | 0 |
| element by full name (three forms), by prefix and name, by name only, duplicate of a name | `elemdict::find_elem_by_name`, `elemname::…` | `si_scene::elemdict::…` | 240,150 | 0 |
| element used (both forms); father | `elemmisc::is_elem_used`, `toposort::find_father` | `ElemStore::is_elem_used`, `si_scene::toposort::find_father` | 80,050 | 0 |
| first entry of a relation in a list | `relations::rel_check` | `si_types::store::rel_check` | 508,249 | 0 |
| the walk over related elements (first, then next until none), per list and relation | `elemrel::find_related_elem`, `find_next_related_elem` | `si_scene::elemrel::related_elems` | 508,249 | 0 |
| count of entries, for each of the six lists at the other end | `elemrel::relations_count` | `si_scene::elemdict::relations_count` | 3,049,494 | 0 |
| root and depth of every model | `elemdict::update_hier_info` | `ElemDictStore::update_hier_info` | 334 | 0 |
| hierarchical order and counts of the MODELS chapter | `toposort::sort_hier_elements` | `si_scene::toposort::sort_hier_elements` | 334 | 0 |
| the sorted scene: success, number of steps, and every step with its arguments | `toposort::topo_sort_dict` | `si_scene::toposort::elemdict_topo_sort` | 1,002 | 0 |
| no fault of the image | | | 334 | 0 |
| all | | | 5,130,470 | 0 |

The last-but-one row is the one that matters most: the update list of each of the 334 scenes,
node for node (chapter, list, element, object, relation, active word, relation data, and per
argument chapter, list, element, object, active word), is the same from both ports.

One thing the check itself had to learn: an element's address cannot be turned back into
(chapter, index) by asking `SI_elemdictGetElemId` chapter by chapter, because of the wrapped
range test of section 1.4. The first run reported 25,966 "disagreements" that were this.

### 4.3 Where the earlier ports differ, and which side the original is on

No case was found in which the emulated original contradicts an earlier port on an input that
port defines. The differences are of three kinds.

**(a) Inputs the earlier ports refuse and the original (and the image form) does not.** Each
is said in the earlier port's own comment; the image form now does what the original does,
and the vectors that reach them are in the "defined part" and "not compared" columns:

| Function | Earlier port | Original, and the image form |
|---|---|---|
| `si_reldictGetRelSub`, `SI_reldictGetRelNbRules` (unknown id) | `si-types`: "not found" | start one rule before the array; the vectors show the original's answer when that memory is defined, and a stop when it is not mapped |
| `SI_reldictGetMaxFanout` (index = count) | `si-types`: −1 | reads one rule past the array |
| `SI_reldictGetRelAttributes` (rule number beyond the relation's rules) | `si-types`: the rule at that position if there is one | the same position; past the last rule of the dictionary it is memory behind the array (5 cases, undefined) |
| `SI_elemdictFindRoot` (element not in the order, or no root after it) | `si-scene`: none | reads the order one word past its end first |
| `elemUpdateHierInfo` (a cycle of hierarchy relations) | `si-types`: stops | recurses until the stack is used up; the image form stops at 10,000 levels and records a fault |
| `SI_elemdictTopoSortElement` (no list takes part) | `si-motion`: false | an uninitialised word; the image form 0 |
| the "next related element" look-ups (entry naming a missing element) | `si-scene`, `si-motion`: skip or fault by their own rule | read the status at address 0x48: the original dies; the image records the fault |

**(b) Things the stores cannot hold, which the image form now reproduces and the check
compares.** None of these can be asked of the store ports at all:

- which blocks exist after a call, their sizes and order (every row of section 1.3);
- the bytes a routine leaves unwritten (the three sequence fields of a new element, two bytes
  of each new relation list and each new chapter, the unset words and pad bytes of an update
  node and argument, the six pad bytes of a new rule);
- the frees and their order (a disposal of a dictionary frees, in this order per element:
  icon, suffix, name record through the outside routine, forward entry arrays and lists,
  backward entry arrays and lists);
- the blocks the original drops (section 1.4);
- the three statics of each look-up cache, which make a look-up answer from the last question
  even after the dictionary changed (`si-types` keeps a cache per store "which gives the same
  answers"; with one relation dictionary in the program that is so);
- `SI_elemdictAddElem` calling the two back-up routines of `edtools.c` (`si-types` reports
  "the model relations must be saved" to its caller instead).

**(c) Where the boundary lies.** `si-motion` ports `SI_edGetFcvFromRelation` and
`SI_edGetElemFromModel` (of `edanim.c` and `edtools.c`) into its own crate and reads the
current dictionary from the globals; this package reaches both files through the trait. When
`si-motion` calls this crate, its two functions are what an implementation of the trait hands
those calls to.

## 5. Record fields and globals established or corrected

Records laid out in this crate (`record!`, offsets asserted):

| Record | Offset | Width | Meaning | Evidence |
|---|---|---|---|---|
| `SI_cusRelInfo` (`records::CusRelInfo`, 0x28 as laid out; the true size is not established beyond +0x26) | +0x00 | 4 | name of the custom effect | `SI_reldictAddRelation` copies it to the rule's +0x20, which `SI_reldictGetRelIndexFromName` compares as a string |
| | +0x04 | 6 × 4 | six words | copied to the rule's +0x24 to +0x38; not read in these files |
| | +0x1c | 2 | "new source" | to the rule's +0x3c, which `SI_reldictIsNewSrc` answers |
| | +0x1e, +0x20 | 2, 2 | fan-out limit on the destination side, on the source side | to the rule's +0x3e and +0x40 (`SI_reldictGetMaxFanout`) |
| | +0x22, +0x24 | 2, 2 | two words | to the rule's +0x42 and +0x44 |
| `SI_CustomRelData` (`records::CustomRelData`) | +0x20, +0x24 | 4, 4 | the rule's name; the custom effect's name | `SI_reldictCusGetRelAttributes` looks the rule up by them |
| update node (`records::UpdNode`, 0x2c) | all | | as findings 0017 and 0018 give it | now typed; measured: `si_updlistAddNode` writes neither +0x0c to +0x13 nor the pad bytes at +0x1a and +0x22 |
| update argument (`records::UpdArg`, 0x14) | all | | as findings 0017 and 0018 | measured: the pad at +0x0e is not written |
| `__SI_UDF` (`elemmisc::Udf`) | +0x04, +0x08 | 4, 4 | chapter and index of the element the user data belongs to; a negative index means none | `SI_elemdictAddUDFElemRelation`, `…RemoveUDFElemRelation` |
| `SI_listChapUpdate` (`toposort::ListChapUpdate`, 0x0c) | +0x00, +0x04, +0x08 | 4, 4, 4 | chapter; first item; next chapter | `SI_elemdictTopoSortElemByFcv` |
| its item (`toposort::ListChapItem`, 8) | +0x00, +0x04 | 4, 4 | element index; next item | the same |

Fields of records `si-types` lays out, established, confirmed or to be renamed:

| Record | Offset | Finding 0010 / `si-types` | Now |
|---|---|---|---|
| `SI_RelList` | +0x10 `unk_10` | "cleared with the list" (0018: sort state) | confirmed and measured: 0, 1 being sorted, 2 done; also set by `elemdictSortHier`. Name it `state` |
| `SI_RelList` | +0x14 `flag` | "1 whenever the list is reset" (0017: takes part in sorts) | confirmed: tested by `elemdictTopoSortChapter` and the three other starts of a sort |
| `SI_RelList` | +0x16 | pad | never written by `si_relAllocate` or any reset (measured) |
| `SI_ARelation` | +0x16 `unk_16` | 0018: the current entry | confirmed in 14 look-ups; `si_relAddRelation` also reads it (an entry with the mark ends the scan for a serial number). Name it `current` |
| `SI_RelRule` | +0x3c `cus_3c` | "from SI_cusRelInfo" | what `SI_reldictIsNewSrc` answers. Name it `new_src` |
| `SI_RelRule` | +0x14 `dope_disp` | printed as `dd()` | also: rules whose value is not 0 are the ones `SI_reldictNbRelForDeactCount` sums the fan-outs of |
| `__SI_Elem` | +0x2b `pad_2b` | "cleared with schem_flags" | confirmed: the creation routines clear +0x2a as 16 bits |
| `__SI_Elem` | +0x74 to +0x7f | "not set by SI_elemlistAddElem" | measured: not written; `SI_elemdictInitElem` clears them |
| `__SI_Elem` | +0x70 `icon` → +0x08 | "freed by SI_dbIconFree" | the icon record owns one block, at its +8; nothing else of it is read here |
| `SI_ElemList` | +0x1a `pad_1a` | no access found | not written by `SI_elemlistInit` (measured) |
| `SI_ElemList` | +0x1c `unk_1c` | written by `txtlistTransferSelection2ElemList` | copied from the database dictionary's chapter by `SI_elemdictAddSelElemsFromDb` |
| `SI_ElemList` | +0x18 `kind`, +0x28 `subdir` | named | a chapter of kind 2 hands its sub-directory string to the scene's chapter in `SI_elemdictAddSelElemsFromDb`; `SI_elemdictMoveElemsFromOutsideChapter` copies it into +8 of each moved element's name record |
| `SI_ElemDict` | +0x6c `untold` | unknown | read by `checkNSEStatus`, which does nothing while it is set |
| `SI_ElemDict` | +0x2e, +0x46 | pad of the two option copies | the second is a copy of the first (`SI_elemdictInit` copies 24 bytes); neither is set |
| `SI_DbListNode` | +0x00 | 0018: path of the database | confirmed: `si_elemdictResetOriginDB` compares it as text with a path, `si_elemdictSetOriginDB` stores a path pointer |
| `__SI_GrafNode` | +0x16 flags, bit 0x1000 | 0018: "the selection state is in the bits…" | the same bit (byte +0x17, 0x10) is what `checkNSEStatus` tests on both elements' nodes *[the meaning there: not editable, from the names of the two routines it calls]* |
| `SI_glbManip` | +0x36 | 0018: "the global at 0x08781596" | 16 bits: while 0, every added or removed relation marks the hierarchy dirty, and `SI_elemdictAddElem` saves a model's relations only when it is set |
| `SI_glbManip` | +0x2e | `si-motion`: the hierarchy short cut | confirmed by the sort's vectors |

Globals (all in `globals.rs`, held against the symbol table and `.data` by
`re/tools/si-mem/check_globals.py`):

| Symbol | Address | Type | Content |
|---|---|---|---|
| `SI_elemDicts`, `SI_nbElemDicts`, `elemDictExists` | 0x0867931c, 0x08679318, 0x08683b80 | pointer, int, pointer | the array of dictionaries, its length, one 16-bit flag per slot |
| `SI_procData` | 0x08679314 | pointer, initial value 0x08782108 | points at `SI_procDataStorage` (12 bytes in `.bss`); the first word there, read as 32 bits, is the index of the current dictionary. (Finding 0010 wrote `SI_procData[0]`; it is one more indirection.) |
| `SI_RD`, `SI_ATD`, `SI_EVD` | 0x087815f0, 0x08781438, 0x08781454 | records | the three dictionaries of kinds |
| `SI_cid`, `SI_dbcid`, `SI_dbed`, `SI_chapInfo` | 0x08781660, 0x08781700, 0x087813a0, 0x086842c0 | records | chapter indices of the scene and of the database dictionary, the database dictionary, the chapter table |
| `last_prd.246`, `last_relid.247`, `last_index.248` | 0x08684560, …64, …68 | pointer, int, int; initial 0, 0x7fffffff, −1 | the relation look-up's last question and answer |
| `last_pel.383`, `last_data.384`, `last_index.385` | 0x08683bf8, 0x08683bfc, 0x08787ae0 | pointer, pointer, int | the same for `SI_elemdictFindElemByData` |
| `RelSerialNumber` | 0x08684480 | int | the counter `SI_elemdictGetRelSerialNumber` hands out |
| `glbED`; `glbEL`, `glbHierSort`, `glbSortUp`, `glbSorttype` | 0x08787cf4; 0x08787cf8, …fc, …fe, 0x08787d00 | | what the two `qsort` comparisons read: the dictionary being sorted; the chapter, the hierarchical switch, the direction, the key |
| `num.510`, `newname.509` | 0x08683e28, 0x08787b00 (500 bytes) | | the new-name counter and buffer |
| `proj.246`, `name.247`, `vers.248`, `prec.249`, `suff.250` | 0x087876c0, 0x087878c0 (0x200 each), 0x08787ac0, …ca, …d4 (10 each) | | the five parts `SI_dbScnGetListNode` has a file name split into |
| `SI_useElemListSortTbl` | 0x08679284 | 16 bits | look-ups by object may use a chapter's sort table |
| `SI_viewerUpdOpt` | 0x08679678 | int | bit 2: an update list gets an end mark |
| `SI_glbManip` | 0x08781560 | 0x3c bytes | the two words above |

## 6. What is behind a trait

`outside::Outside` has one method for calls, `call(image, routine, arguments) -> eax`, with
the routine named by a constant that holds its name and its address in the binary and the
arguments as 32-bit stack words in C order: the calling convention of `si_mem::Registry`, so
that an implementation can hand a call to the real function the day it is on the image. Each
routine has a typed wrapper, which is what the ported code calls. `call_addr` is the same for
a function pointer the dictionaries were handed (`SI_elemdictFindElemByCB`'s test). Three
further methods stand for the C library where it is not memory: `getenv`, and the text the
dictionaries print to `stdout` and `stderr`.

| Routine | Address | File | Called by |
|---|---|---|---|
| `SI_getLoadOptim` | 0x080d94b0 | `dbloadsave.c` | `SI_elemlistAddElem`, `SI_elemdictFindElemByData` |
| `SI_grafnodeDefColor` | 0x0832bff4 | `graflist.c` | the two routines that initialise an element |
| `SI_elemdictDisposeRelationData` | 0x08111b7c | | the three removal routines of `relations.c`, for an entry with data |
| `SI_dbListNodeAllocate`, `SI_dbListNodeFree`, `SI_dbFormatFullName`, `SI_dbnameCheckValidity`, `dbfilenameExpand` | 0x0812250c, 0x08122450, 0x081223dc, 0x08121bdc, 0x081228c0 | `dbexternal.c` | everything that makes, frees or renames a name record |
| `SI_edGetSuffixFromChapter`, `SI_edGetChapterFromSuffix` | 0x08120a48, 0x08120a7c | `dbexternal.c` | `SI_elemdictLoadChapters`, `SI_elemdictMoveElemsFromOutsideChapter` |
| `SI_edAllocMdlRelBackUp`, `SI_edMdlRelBackUp` | 0x08105cb0, 0x08105d7c | `edtools.c` | `SI_elemdictAddElem` |
| `SI_edSetUneditable`, `SI_edCheckForUneditable` | 0x081074c0, 0x08107a44 | `edtools.c` | `checkNSEStatus` |
| `SI_dbCheckExtraUnToldRelation` | 0x08104fb8 | `edtools.c` | the four sorts, before and after |
| `SI_cusResortUpdList` | 0x0852dd6c | `custom_dso.c` | the sorts, at the end |
| `SI_edGetFcvFromRelation` | 0x080ee6f0 | `edanim.c` | `relCreateArguments` |
| `time` | the binary's stub | C library | `SI_dbScnGetListNode` |

Not behind the trait although of other files: the three allocation wrappers of
`sys_memory.c` (`si-mem` has them).

## 7. For the crates that will move over

The functions keep the original's names in snake case, grouped by the modules of section
1.1; `re/out/si-dict/tables.md` (made by `report.py`) lists each with its address.

- **`si-scene`.** Its `Dict` (stores plus arena of name records) and `Program` (chapter
  indices, relation dictionary, the globals) become addresses in the image:
  `Ptr32<ElemDict>` for the dictionary, `SI_RD`, `SI_CID` and the other `globals`. An
  `ElemId { chap, index }` stays what it is (the original passes the two integers too);
  where the original passes `__SI_Elem *`, pass `elemdict::get_elem(m, ed, chap, index)`. Its
  own ports of `elemdict.c`, `toposort.c`, `updlist.c` and `relations.c` (with `si-types`' and
  `si-motion`'s: 93 + 13 + 4 + 7 functions) are replaced one for one by the functions here: `si_scene::elemrel::add_complete_relation(d, p, …)`
  → `si_dict::elemrel::add_complete_relation(m, ed, SI_RD, …)`, `related_elems` → the
  `find_related_elem` / `find_next_related_elem` pair, `elemdict_topo_sort(d, p, upd, opt, …)`
  → `toposort::topo_sort_dict(m, o, SI_RD, ed, field!(ed, ElemDict, upd_list), check_only, restricted)`
  with the two option bits it carried in `SortOptions` now the globals `SI_viewerUpdOpt` and
  `SI_glbManip` + 0x2e. Its `Unported` trait and this crate's `Outside` overlap in five
  routines (`SI_dbCheckExtraUnToldRelation`, `SI_cusResortUpdList`, the two back-up routines,
  `SI_elemdictDisposeRelationData`); one implementation can serve both. What `si-scene` must
  write itself to stand on the image: its loaders have to put name records
  (`SI_DbListNode` with its strings as blocks) and objects into the image; today they are
  `si_db::ListNode` values and handles. `dict-check`'s adapter (`to_image`) is that
  conversion for the dictionary part, and moves into `si-scene` as the first step (done,
  section 10.1); it is
  done when the loaders build the records in place and the adapter is gone. The update list
  it keeps as a `Vec` becomes the `SI_UpdList` at +0x0c of the dictionary.
- **`si-motion`.** Already on the image. Its 19 functions in `ed.rs` and `topo.rs` can be
  deleted in favour of this crate's: same image, same addresses, so a call is a cast
  (`si_dict::toposort::topo_sort_dict(m, o, Ptr32::new(rd), Ptr32::new(d), Ptr32::new(ul), check as i16, 0)`).
  Its `TopoHost` trait disappears (three of its six methods are functions of this crate, the
  others are `Outside` routines), and its own `get_fcv_from_relation` becomes what its
  implementation of `Outside` answers for `SI_edGetFcvFromRelation`. Its `off::{elem, list,
  dict, rellist, arel, updnode, updelem}` offsets can go where the typed records serve.
  Section 4.1 is the evidence that nothing changes: 2,535 cases agree.
- **`si-rensetup`.** It walks `si-scene`'s `Dict` to find the models, lights and materials of
  a frame. On the image that is `m[ed].lists.add(chap)`, `elemdict::get_elem`, and the
  relation walks of `elemrel` (materials of a model: relation 300 in forward list 3;
  textures: 400, 500, 401, 501, as finding 0018 lists them); the objects it then reads are at
  `m[e].data`. It needs nothing of this crate beyond the read-only routines the corpus check
  exercised.
- **`si-expr`.** Its scene trait asks for an element by name and for an attribute's value.
  The first half is `elemname::find_elem_by_prj_name` / `find_elem_by_full_name`; the second
  half is the attribute dictionary (`attdict.c`), which is **not** on the image yet (section
  8): until then `si-expr` keeps its trait, with `si-dict` behind the element look-ups.

For all of them: the check binaries get shorter (finding 0024, section 5.3). `dict-emucheck`
is 550 lines for the driver and one line per routine; its driver is not specific to
dictionaries and could serve any crate whose vectors follow the conventions of
`re/tools/si-dict/dictlib.py`.

## 8. Not on the image, not determined

- **`attdict.c`, all 52.** *(Done in the third pass at `si-scene`: section 11.)* The attribute
  dictionary (`SI_ATD`): 13 functions are on stores in
  `si-types`, the curve index in `si-scene` and `si-expr`. Not started here. It is the next
  piece: `si-expr` and the spreadsheet stand on it. Its records are laid out in `si-types`
  (`AttributeDict`, `AttribGroup`, `Attribute`, `AttGrpIndex`, `AttributeValue`); the condition
  record and the 12-byte curve index entry are not.
- **`reldict.c`, 61 of 88.** The reader of `Relations.rsrc` (about 35 functions, 14 KB; a
  parser on `FILE *`, ported as a parser of values in `si-ascii`): on the image it needs the
  C library's stream calls behind the trait and the operator records (`SI_OperDef`,
  `SI_OperResult`), which are not laid out. The 26 look-ups over operators
  (`SI_reldictGetNbRes` to `SI_reldictGetScope`) and the five name searches at the end of the
  file read the same records. `SI_reldictDump`.
- **`elemdict.c`, 4.** *(The comparison and the sort are done: section 11. The two dumps are
  left.)* `elemdictCompareElems` (the 15-key element comparison, 1,460 bytes:
  it calls `sscanf`, `sprintf` and two routines of `text.c`) and
  `SI_elemlistSortChapterElements`, which sorts with it: read, not ported here (`si-scene` has
  both on stores, with `scene-emucheck` vectors). The C library's `qsort` they need is in
  `libc.rs` and is used by `SI_elemdictSortChapNames`. `SI_elemlistDump`, `SI_elemdictDump`.
- **`init_dict.c`, 2.** `SI_initDictionaries` needs the resource reader and the database
  layer; `SI_databaseDirectoryLoad` is `si-db`'s.
- **The dumps** (`SI_enmdictDump`, `SI_updlistDump`, `SI_elemdictTopoSortDump`,
  `si_relGroupDump` and the four above): they only print. The trait has `stdout`/`stderr`;
  what is missing is a `printf` formatter on the image.
- **Not determined:** the true size of `SI_cusRelInfo` and of the icon record; the meaning of
  the six words of a rule's custom block; what `SI_elemdictTopoSortElement` returns in
  practice when no list takes part (it depends on what the stack held); whether any caller
  relies on `SI_elemdictFindElemByData` answering the last of two elements with one object.

## 9. What `si-mem` lacked

Nothing in `si-mem` was edited. Put into this crate instead, for the lead to move or not:

1. **A loader for cases with globals.** `si_mem::load_case` places every field in the arena
   and resolves pointers only among those. A dictionary case has buffers at fixed addresses
   (globals) that arena buffers point to and that point into the arena. `dict-emucheck` does
   the two steps itself (`place` in the case's order, then `load_words` with one `Names`):
   20 lines that belong in `si-mem` as `load_case_at`.
2. **The C library on the image.** `strcmp`, `strncmp`, `strcasecmp`, `strcpy`, `strcat`,
   `strchr`, `strrchr`, `strtol` (base 10) and `qsort` as the harness's stand-ins behave
   (the comparisons answer −1, 0, 1; `qsort` is the merge sort that calls the comparison with
   addresses inside the array, in a fixed order): `si-dict/src/libc.rs`. Every crate on the
   image will need them; `si-mem` has only `strlen`, `cstr`, `strdup`.
3. **`getenv` and the standard streams.** There is no model of the environment or of
   `stdout`; they are methods of this crate's trait. The harness's `getenv` allocates a block
   for the value on every call, so a port that asks must allocate too to keep the heap in
   step; the check's implementation does.
4. **Snapshots under a pattern fill.** `Mem::changed` takes a page that was never touched as
   zero, so with `Fill::Pattern` every fresh heap page reads as changed. The check uses
   `changed_in` for the arena, `.data` and `.bss` and compares the heap block by block.
5. **`si_x87::vectors::Word` against `si_mem::Word`**: still two types, converted by a
   closure, as finding 0024 says.

Two more, met when `si-scene` began to move (finding 0018, section 13.3, has the
arithmetic):

6. **An allocator that reuses.** The image's allocator is the harness's: no block is used
   again and `realloc` always moves. The dictionaries grow each array by one record per
   addition, so building a real scene's dictionary with these functions costs heap in the
   square of a chapter's size: 444 MB for the element array of the corpus's largest
   chapter (2,593 elements), against a heap of 16 MB. Right for a case compared with the
   harness; a load needs either a much larger heap through `Mem::with_map` or a second
   allocation policy in `si-mem`.
7. **More handles.** `si_mem::Handles` has 4,096 addresses per kind; the objects and the
   relation data of a scene, which stay handles until their crates are on the image, are
   more.

Wishes for the harness (`re/tools/emu`), none of them blocking: a case whose two runs agree
by luck on memory nobody wrote is not marked (the check finds those on the port's side, by
running the port on two patterns too); a routine that writes beyond 64 bytes past a buffer is
not marked `OVERRUN`.

## 10. Where the work stands

Done and checked: the 186 functions of the tables; `dict-emucheck`, `dict-check`; the unit
tests (3, made-up data); the two scripts that hold globals, constants and routine addresses
against the binary.

For the next engineer, in this order:

1. `attdict.c` (52). Start from `si-types/src/attdict.rs` for the layouts and
   `si-scene/src/attdict.rs` for the curve index; read every allocation again. The group
   tree (`AttribGroup.subs`) grows like every array of section 1.3 *[expected; not read
   here]*. The get/set function pointers of a group are code addresses (`FnPtr`); keep them
   as addresses.
2. The operator records and look-ups of `reldict.c`, then the resource reader behind a
   stream trait, then `SI_initDictionaries`.
3. `elemdictCompareElems` and the chapter sort: port from the listing, with `si-scene`'s
   `elemsort.rs` as the description; the vectors need `glbEL` and its three companions as
   declared globals and the two `text.c` helpers either ported beside it or as stand-ins.
4. A `printf` on the image, then the eight dumps.
5. Vectors at scale 1.0 for the sort (the generators are deterministic per routine name).

### 10.1 After the first pass at `si-scene` (2026-10-13)

`si-scene` was to move onto this crate in one pass; the pass was stopped early. What it
changed here, and nothing else:

- **The feature `corpus` is gone**, with the optional dependencies on `si-scene` and
  `si-db`: `si-scene` now depends on `si-dict`, not the other way round. `dict-check` is
  `crates/si-scene/src/bin/dict-check.rs`, unchanged but for its place and header; run
  from there it asks the same 5,130,470 questions with 0 disagreements.
- The feature `earlier` (`si-motion`) is as it was; it was not built in that pass.
- No function of this crate changed. `dict-emucheck`'s output is identical to the one of
  the hand-over (186 routines, 15,726 cases, 15,217 identical, 346 in the defined part, 0
  different, 163 not compared; mutation run 14,584 different).
- Seven of the routines of section 6 now exist on the image in `si-scene`
  (`src/dbnode.rs`): `SI_dbListNodeAllocate`, `SI_dbListNodeFree`, `SI_dbFormatFullName`,
  `SI_dbnameCheckValidity`, `dbfilenameExpand`, `SI_edGetSuffixFromChapter` and
  `SI_edGetChapterFromSuffix` (with the three they call, ten functions). They are held
  against `si-db`'s value forms by unit tests, not against the emulated original, and no
  implementation of `Outside` hands calls to them yet.
- `si-scene`'s loader and saver still work on stores; its 72 store-based copies of
  functions of this crate are all still there, because its loader and saver call them.
  Finding 0018, section 13, is the hand-over for that work.

### 10.2 After the second pass at `si-scene` (2026-10-13)

"Get Scene" now builds the scene in the image on top of this crate (finding 0018,
section 14). What that means here:

- **No source file of this crate changed in that pass.** `dict-emucheck`'s output is
  byte for byte `re/out/si-dict/check.txt` (186 routines, 15,726 cases: 15,217 identical,
  346 in the defined part, 0 different, 163 not compared). (`libc.rs` had become a
  re-export of `si_mem::clib` before the pass.)
- **The loader is the first whole client.** Start-up (`reldict::init`, `elemmisc::add`,
  `elemdict::init`, `elemmisc::load_chapters`, `elemname::create_default_dsc`,
  `reldict::add_relation` per rule of `Relations.rsrc`) and the load of all 343 sample
  descriptions run on these functions under `Mem::program()`: no fault of the image in
  any load, the allocator's tables consistent after each, 2.04 MB of heap at most for
  the largest scene (2,709 element slots; 25,258 live blocks), 3.2 s for the corpus.
  The dictionaries they build agree with the ones the store-based loader built in
  5,130,470 questions and field for field (0 differences).
- **`Outside` has a real implementation**: `si_scene::world::Out`. The seven routines
  of `dbexternal.c` it hands on (`SI_dbListNodeAllocate`, `SI_dbListNodeFree`,
  `SI_dbFormatFullName`, `SI_dbnameCheckValidity`, `dbfilenameExpand`,
  `SI_edGetSuffixFromChapter`, `SI_edGetChapterFromSuffix`) are `si-scene`'s
  `dbnode.rs`, each now held against the emulated original (14,760 cases with the three
  they call, 0 different) and cited there. `SI_edGetFcvFromRelation` and
  `SI_elemdictDisposeRelationData` are written on the image in `si_scene::world`,
  unchecked. The two back-up routines and the two of the light product answer 0.
- **45 of `si-scene`'s 72 store copies of functions of this crate are deleted**; the 24
  of them that have vectors in `re/out/si-scene/vec` are checked there through this
  crate's functions on `si_mem::load_case_at` (16,439 of 17,450 cases identical, 0
  different; 15,916 with the store forms). 27 copies remain in `si-scene` because its
  saver (12) or `si-rensetup` (11) still calls them in store form, or those reach them
  (4): the list is in finding 0018, section 14.3.
- **What the loader had to know that section 7 did not say**: `Elem::data`,
  `Elem::aux_data` and `ARelation::data` hold addresses of `si-mem`'s handle range for
  what is not in the image (`si_scene::world`: kinds 4, 5, 6), except a custom effect's
  relation data, which must be a `CustomRelData` record with its two names as blocks.
  The current dictionary is `SI_elemDicts[*SI_procData]`: `SI_edGetFcvFromRelation`
  reaches it that way, so a client that builds a dictionary elsewhere (the adapter
  `si_scene::world::to_image` does) has to make it that record.
- **Not asked of this crate, and still open**: `attdict.c` (section 8); the loader uses
  `si-scene`'s store form of the curve index behind an adapter.

How to run everything of this package:

```bash
export CARGO_TARGET_DIR=$PWD/target/si-dict
(ulimit -v 8000000; cargo test -p si-dict)
(ulimit -v 8000000; cargo build --release -p si-dict --features earlier)
(ulimit -v 8000000; CARGO_TARGET_DIR=$PWD/target/si-scene cargo build --release -p si-scene)   # dict-check
(ulimit -v 6000000; timeout 3400 uv run --with unicorn python3 -I re/tools/si-dict/gen.py re/out/si-dict/vec 0.3)   # about 20 minutes, 173 MB
(ulimit -v 2000000; timeout 600 target/si-dict/release/dict-emucheck re/out/si-dict/vec)                    # 186 routines, about 10 s
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 600 target/si-dict/release/dict-emucheck re/out/si-dict/vec)   # every case that returns or changes something must differ
re/tools/si-dict/motion_links.sh && (ulimit -v 2000000; DICT_EARLIER=motion timeout 600 target/si-dict/release/dict-emucheck re/out/si-dict/vec_motion)   # si-motion's 19 on the same vectors
(ulimit -v 2000000; timeout 900 target/si-scene/release/dict-check re/bin/si3d40-ia/installed)            # the corpus, 5 s
python3 -I re/tools/si-mem/check_globals.py si-dict
python3 -I re/tools/si-dict/check_rodata.py
python3 -I re/tools/si-dict/report.py > re/out/si-dict/tables.md                                           # the tables of section 3
```

Scripts in `re/tools/si-dict/`: `funcs.py` (the functions of a source file with sizes),
`show.py` (decompiled functions, compact), `callees.py` (what the files call outside
themselves), `sym.py` (data symbols and literals by address), `dictlib.py`, `world.py`,
`gen.py`, `gen_*.py` (the vectors), `report.py`, `check_rodata.py`.

## 11. Third pass at `si-scene`: the attribute dictionary and the element sort (2026-10-13)

Done by the engineer of `si-scene` (finding 0018, section 15), who owned this crate for
the pass. **Read from code**: the listings of all 52 functions of `attdict.c`, of
`elemdictCompareElems`, `SI_elemlistSortChapterElements`, `SI_strGetSeqNum` and
`SI_strStripDigit`, and the instruction listings of `SI_attdictEvalCond`,
`SI_attdictValidateAttr`, `SI_attdictRangeAdjust`, `SI_attdictFindFcurve`,
`SI_attdictGetFcurveFromName`, `attdictAttribGroupDump`, the comparison's float keys and
its tail, and the two `text.c` routines. Everything in the tables is **measured**.

### 11.1 What is on the image now

| Module | Source | Functions | New vectors | Cases | Identical | Defined part | Different | Not compared |
|---|---|---:|---|---:|---:|---:|---:|---:|
| `attdict.rs` | `attdict.c` | 52 of 52 | `re/out/si-dict/vec_att` (`gen_att.py`) | 34,815 | 34,815 | 0 | 0 | 0 |
| `elemsort.rs` | `elemdict.c` (2), `text.c` (2) | 4 | `re/out/si-dict/vec_sort` (`gen_sort.py`) | 8,900 | 8,666 | 234 | 0 | 0 |

`re/out/si-dict/tables.md` has the cases function by function (made by `report.py` from
the three check outputs joined). 242 functions are cited in the crate: 240 of the 309 of
the eight source files and the two of `text.c`.

**The first reference is untouched.** `dict-emucheck re/out/si-dict/vec` still prints
`re/out/si-dict/check.txt` byte for byte (186 routines, 15,726 cases). The new vectors
are in directories of their own with references of their own: `vec_att` →
`check_att.txt`, `vec_sort` → `check_sort.txt` (`mutate_att.txt`, `mutate_sort.txt` for
the mutation runs). `gen.py` makes them only when the module is named
(`gen.py re/out/si-dict/vec_att 1.0 gen_att`): a plain run into `vec` makes the seven
first modules as before.

Mutation switch: `vec_att` 34,060 of 34,815 differ (754 are calls that answer nothing and
change nothing: a path that names no attribute, a disposal of an empty dictionary; 1 is
not compared because the flipped bit made the port's two runs disagree); `vec_sort` 8,663
of 8,900 (44 do nothing; 167 are cases whose result the original itself leaves undefined;
26 not compared). The switch now also flips one bit of the first byte a routine prints,
so the two dumps are live under it; on the first vector directory the mutation counts
are unchanged (14,584).

### 11.2 The attribute dictionary: facts the vectors confirm

Records and globals added (`records.rs`, `globals.rs`, `rodata.rs`; `check_globals.py`
39 globals of this crate, 0 wrong; `check_rodata.py` 96 constants and addresses, 0 wrong):

| | |
|---|---|
| `SI_AttribCond`, 12 bytes | +0 the index of an attribute in the dictionary's array, +4 an array of numbers (owned: the group's disposal frees it before the record), +8 how many. The size is what `attdict.c` reads; the record is made by the resource reader |
| `SI_FcvATD_Index`, 12 bytes | +0 attribute index, +4 component, +8 a 16-bit flag (0 when built; the two bytes behind it are never written). `SI_AttributeDict` +0x10 is the array, +0x14 its length |
| `glbATD` 0x08785dc0 | the dictionary whose index is being sorted or searched |
| `last_sort_ind.327` 0x08682788, `last_fcv_id.328` 0x0868278c | `SI_attdictFindFcurve`'s memory of its last answer; both start as -1 |
| `default_name.347` 0x086827b8 | a pointer to the literal `notfound` (0x086827ac) |
| `ATT_FCTS` | the 28 groups `SI_attdictSetFctsPointers` knows, with the code addresses of their reading and writing functions (`BoundingBox` has no writing function: null) |

What the functions do, where it is not what one would guess:

- **`si_attdictInitAttribute`** clears six of the seven entries of the attribute's own
  path; the seventh and the padding keep what the block held.
- **`si_attdictAddAttribute`** initialises the new slot and then copies all 100 bytes of
  the caller's record over it. Both arrays grow as the element dictionary's do: the first
  record from `SI_malloc`, then `SI_realloc` to `(n + 1)` records.
  `si_attdictAllocateFcurveArray` takes its two arrays from `SI_calloc`.
- **`SI_attdictDispose`** frees, per attribute, each component name, the array of
  names, the array of ids, the name and the description, then the groups (list of
  indices, name, the condition's array, the condition, the sub-groups in depth, their
  array), the curve index and the two arrays; every pointer is cleared after its `free`.
- **A path** (`SI_AttGrpIndex`): `path[0]` is a top-level group, `path[1]` to
  `path[depth - 2]` sub-groups, `path[depth - 1]` a position in the group reached. With
  a depth of 1 the one entry is both group and position. Entries are read by offset: a
  depth above 8 reads past the path.
- **`SI_attdictAttGrpGetAttrib`** (the attribute after a given one) does not look at the
  group for that: it copies the given attribute's own path, raises its last entry by one
  and looks that path up. The entry raised is the 16-bit word number `depth` of the copy.
- **`SI_attdictEvalCond`** looks the condition's attribute up in `SI_ATD` (not in a
  dictionary handed in), finds its group by the attribute's own path, requires the
  element to exist in the current element dictionary (`SI_elemDicts[*SI_procData]`), and
  calls the group's reading function through its pointer with the element's identity,
  the path and the address of a local value. The condition holds when the function
  answers a non-zero 16-bit word, the value's type is 3 and its third word is one of the
  condition's numbers. **With a null function pointer the two locals are tested without
  having been set.** The port keeps them on the image's stack where the original has
  them (the value 0x10 bytes below the stack pointer the routine was entered with; the
  two callers above it take 0x20 and 0x30 bytes), so that the address the reading
  function is handed is the original's and a value left by an earlier call is still
  there.
- **`SI_attdictValidateAttr`** does not test the path's first entry against the number
  of groups.
- **`SI_attdictRangeAdjust`**: a bound exists when its word is 1 (2, the "soft" bound of
  the resource file, is no bound here). Float (type 1) and double (type 6) are compared
  with the bound in the x87 unit and replaced when beyond; an unordered comparison
  changes nothing. Integer (4) and 16-bit integer (8) are compared with the bound cut
  toward zero; for type 8 the low 16 bits of the 32-bit result are taken (a bound of
  70000.9 clamps to 4464). Any other type answers 0, but only when the attribute has a
  bound at all: without one the answer is 1 for every type.
- **`SI_attdictFindFcurve`**: the same id as last time answers at once; the id one above
  answers with the next entry when that entry is the next component of the same
  attribute and that component has this id; otherwise `bsearch`, so of several entries
  with one id the one the bisection meets first answers. The remembered position is
  used only while it is below the index's length. The statics are shared by every
  dictionary handed in.
- **`SI_attdictGetFcurveFromName`** compares with the C library's `strcmp` over the
  index in its order, then four names of older versions inline (seven bytes each):
  `hidden` 0xaf, `camhid` 0xb1, `lighid` 0xb0, `t2norm` 0x2b.
- **`SI_attdictGetFcurveFromAttId`** and **`…GetNbFcurvesForAttId`**: with a count of
  attributes below zero the first record is answered unlooked at.
- **`SI_attdictGetAttGrpByPath`** and the sub-group form copy the path with `strdup` and
  free the copy only when a group was found: a path that names nothing leaves a block.
- **`attdictAttribGroupDump`** prints the owner of an attribute as `Elem` when its first
  word is 0, else `Oper`, and the 16-bit word at +4 as `owner_id`: those are the two
  fields `si-types` calls `unk_00` and `unk_04`.

### 11.3 The element sort: facts the vectors confirm

- **`elemdictCompareElems`** reads a version with `sscanf("%d")` into a local it has not
  set: a version that is no number compares as whatever the comparison before left at
  that place of the stack. The port keeps the two locals on the image's stack without
  refilling them. 193 of the 5,000 comparison cases and 41 of the 2,500 sort cases have
  such a word in their result and are compared in the rest.
- The two box keys (14, 15) form the differences of the box's corners in the x87 unit
  and answer the difference cut toward zero (0x80000000 when it does not fit or is no
  number); the two position keys (16, 17) answer -1, 0, 1, and 0 when unordered.
- The pictures key compares the suffixes (the element's own, else the chapter's; only
  when both exist), then the full names with their frame numbers cut off (two `strdup`
  copies, freed after), then the frame numbers; with the C library's `strcmp`, not the
  name comparison.
- The prefix-name key prints into two statics of 512 bytes (`str1` 0x08787d20, `str2`
  0x08787f20); a null name prints as `(null)`.
- Answers that are **not** negated for a descending sort: an entry outside the chapter,
  a freed slot, an element without name record, a null string.
- **`SI_elemlistSortChapterElements`** takes its arrays from the C library's `calloc`,
  not from the program's wrapper. With the hierarchical order it sorts the roots as
  positions in that order and then writes, per root, the entries before it; an entry
  beyond the chapter's size prints the routine's error message and **is written all the
  same**, behind the array (47 of the cases; the port writes it too).
- **`SI_strStripDigit`** cuts a name that ends in a dot at that dot; **`SI_strGetSeqNum`**
  answers -1 when `strtol` converted nothing.

### 11.4 Faults the checks found

In the first versions written in this pass:

1. `SI_attdictEvalCond`, `attdictEvalGroupCond`, `SI_attdictValidateAttr`: the local value
   handed to the group's reading function was at another stack address than the
   original's (978 of 4,500 cases differed in that one argument). Mended by laying the
   three frames out as the original does. All other 49 functions of `attdict.c` agreed
   on their first run.
2. The sort: locals taken with `Mem::local` are filled afresh on every call, so a stale
   version was lost (4 of 2,500 cases). Mended with a frame that is left as it is
   (`si_dict::frame`, in `lib.rs`).

In the generators (not in a port): the four sort globals declared as three adjacent
buffers made the check see a write "outside every buffer" (2,439 false differences);
one case walked the hierarchical order below its start, where the harness does not flag
a read (the bytes before a buffer keep an earlier case's content in both of its runs):
such cases are no longer made.

Against the earlier store forms (`si-scene`'s, now deleted or kept as said in finding
0018): the store comparison read a version that is no number as 0 where the original
keeps a stale word; it formed the box differences in double precision where the
original uses the x87 unit's 64-bit significand; it compared the chapter's suffix with
itself for the pictures key (the element's own suffix was not kept there); and its
hierarchical sort dropped an entry beyond the chapter's size where the original writes
it. None of these shows in the corpus: `dict-check` asks the image sort and the store
sort for every chapter's order three ways (120,075 positions) and they agree.

### 11.5 What changed outside the two new modules

- `outside.rs`: one routine more, `SI_generalAttrGetValue` (0x080f6460), as the stand-in
  the check answers for a group's reading function.
- `lib.rs`: `frame` (above). `libc.rs` is unchanged; `attdict.rs` and `elemsort.rs` take
  `bsearch`, `strtol` and `strncmp` from `si_mem::clib` directly.
- `dict-emucheck`: 56 routines more in the table, the stand-in's effect, the printed
  text under the mutation switch.
- `re/tools/si-dict/`: `gen_att.py`, `gen_sort.py`; `dictlib.py` has the reading
  function's stand-in; `gen.py` keeps the two new modules apart; `check_rodata.py`
  checks `ATT_FCTS`.

### 11.6 Not done, and wishes

- The eight dumps of section 8 other than the attribute dictionary's two
  (`attdictAttribGroupDump` and `SI_attdictDump` are on the image, their text built in
  Rust from the format strings as read).
- `si-types`' 13 functions of `attdict.c` on stores (`si-types/src/attdict.rs`) are now
  a second form of functions that are here; nobody was asked to delete them.
- Wishes for `si-types`: names for `Attribute::unk_00` (owner kind) and `unk_04` (owner
  id) and for `AttributeDict::unk_14` (the length of the curve index); `fcv_index` typed
  as a pointer to the 12-byte entry; `AttribCond` as a record instead of an empty type;
  `range_low` and `range_high` as a float and a kind. `si-ascii` reads the word at +0x54
  as the `#` column and the one at +0x60 as the `vis` column, `si-types` labels +0x54
  `visible`: not settled here (no function of `attdict.c` reads either).
- Wishes for `si-mem`: a stack local that is not refilled (what `si_dict::frame` does);
  `sscanf` and `sprintf` on the image (the sort has a `%d` scanner of its own).
- A wish for the harness: flag a read before a buffer as it flags one behind it.

Commands of this pass (after those of section 10):

```bash
export CARGO_TARGET_DIR=$PWD/target/si-scene
(ulimit -v 8000000; cargo build --release -p si-dict)
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-dict/gen.py re/out/si-dict/vec_att 1.0 gen_att)     # 85 MB, about 90 s
(ulimit -v 6000000; timeout 3000 uv run --with unicorn python3 -I re/tools/si-dict/gen.py re/out/si-dict/vec_sort 1.0 gen_sort)   # 67 MB
(ulimit -v 2000000; timeout 600 target/si-scene/release/dict-emucheck re/out/si-dict/vec | cmp - re/out/si-dict/check.txt)
(ulimit -v 2000000; timeout 900 target/si-scene/release/dict-emucheck re/out/si-dict/vec_att | cmp - re/out/si-dict/check_att.txt)
(ulimit -v 2000000; timeout 900 target/si-scene/release/dict-emucheck re/out/si-dict/vec_sort | cmp - re/out/si-dict/check_sort.txt)
(ulimit -v 2000000; EMUCHECK_MUTATE=1 timeout 900 target/si-scene/release/dict-emucheck re/out/si-dict/vec_att)
cat re/out/si-dict/check.txt re/out/si-dict/check_att.txt re/out/si-dict/check_sort.txt > re/out/si-dict/check_all.txt
python3 -I re/tools/si-dict/report.py re/out/si-dict/check_all.txt > re/out/si-dict/tables.md
python3 -I re/tools/si-mem/check_globals.py si-dict; python3 -I re/tools/si-dict/check_rodata.py
```

## 2026-10-28, the dictionary dumps (unit dict2)

The nine diagnostic printers of the dictionary files are ported in `crates/si-scene/src/dictdump.rs`:
the enumeration dictionary dump (0x081386fc), the relation dictionary dump (0x0813a7bc), the relation
group dump (0x081391a0), the element list dump (0x08134940), the element dictionary dump (0x08134ba8),
the update list dump (0x081415e0), the topological-sort dump (0x0813fef4), the hierarchy animation dump
(0x08138a00) and the custom-operator dump (0x0813eae0, a function that does nothing). They read the dictionary
image and print through the `Outside` text sinks (stderr for the dictionary and list dumps, stdout for the
update list, sort and animation dumps).

Check: `re/tools/si-scene/emu_gen_dict2.py` builds 943 dictionary images; `dict2-emucheck` runs the
original under the emulator and the port on each and compares the printed text and the touched memory.
Result: 926 identical, 0 different, 17 not compared (the original read invalid memory on a generated
image: relation group 9 of 150, update list 7 of 200, topological sort 1 of 100).
Mutation: the shared `EMUCHECK_MUTATE=1` switch changes only the 108 update-list cases that touch the
last-relation-dictionary global (a printer has no other value to alter); `=calls` changes nothing (no
outside calls are made, a coverage line). A third switch `EMUCHECK_MUTATE=text`, local to
`dict2-emucheck`, flips one bit of the first printed byte: 926 of 926 compared cases become different;
the no-op custom-operator dump has nothing to print (3 of 3 stay identical).

Not done in this round: the dictin.c functions (map struct key, replace precision, fix patch, and the live-reached
set-anim-relation and check-paste-fcv, whose hybrid ports in `animrel.rs` stay uncited), the relation
dictionary operator readers, the description file dump, init dictionaries, the physical-IO wrappers and
the relation-data store. Dialog and user-interface functions of these files were skipped.

## 2026-10-28, the rest of the dictionary code (unit dict3)

Ported in `crates/si-scene/src/dictrest.rs`, every outside routine reached through `ByAddr`:
map struct key (0x08128a9c), replace precision (0x081292e0), relation-data allocate (0x08111d58),
store relation data (0x08111dec), load relation dictionary (0x0813ea84), init dictionaries (0x0810824c),
the shader, cluster, setup, channel-setup and particle-setup readers (0x0810ff38, 0x08110144, 0x08111b00,
0x08111b34, 0x08111b64) and fix patch (0x08129430, the padding of a patch grid to multiples of three; the
convert-patch routine 0x080dcb54 stays outside).

Check: `re/tools/si-scene/emu_gen_dict3.py` (1750 cases, made-up worlds, recorded stand-ins), run by
`dict2-emucheck`: 1639 identical, 0 different, 111 not compared. Not compared: store relation data 30 (null
model, the original faults), init dictionaries 10 (the program exits), cluster reader 36 (a name over 1024
bytes smashes the original's frame), fix patch 35 (the original passes a slot of an array it never wrote to
convert-patch). Mutation: `EMUCHECK_MUTATE=1` changes every compared case except the three one-line setup
wrappers (they only call out; coverage line) and the 14 fix-patch cases that return early; `=calls` changes
all but map struct key (58 of 300 only), store relation data (47 of 470) and relation-data allocate (makes no
call; coverage line).
Findings of the check: the emulator copies a signalling NaN through fld/fstp unchanged, real hardware quiets it
(the port quiets it; the vectors use quiet NaNs). A stack local handed to a callee is compared by what it holds
(the static-value pair, the found flag) and texts by CRC-32.
Open: set-anim-relation, check-paste-fcv (hybrids in `animrel.rs`, hierarchy animations there are Rust values,
not image data; stay uncited), the operator readers (0x0813d12c, 0x0813d39c, 0x0813d75c), the description file
dump (0x08131344). Dialog and user-interface functions were skipped.

## 2026-10-28, operator readers, description dump, two setup readers (unit dict4)

Ported in `crates/si-scene/src/dictrest.rs`: read one operator attribute (0x0813d12c), read operator attributes
(0x0813d39c), read one operator (0x0813d75c), the description file dump (0x08131344), the channel-setup getter
(0x08568ab4) and the ascii cluster reader (0x083ddf10). Outside routines go through `ByAddr`; the custom
operator option parser (0x0852a3ac) is not ported and is a recorded stand-in.

Read from code: the "from chapter" of an operator result is the 16-bit value at the results-array pointer plus
8 (no second dereference). The dump writes the ELEMENTS block chapter by chapter, and with relations switched
on a RELATIONS block of 37 (from, to) chapter pairs; the pairs were taken from the call arguments in the
disassembly, not typed by hand. The ascii cluster reader opens `<base>.cls` in the text mode (string at
0x086cbdd9), checks the first word against CLUS, reads one float, then reads node words until one is not
`Cluster_node`; a wrong first word gives the "is not a cluster file" message at level 3.
Not reproduced: the original bumps the licence allocation counter (0x08679660) before the message allocation in
the operator reader; that is licence bookkeeping and stays out, as in the other crates.

Check (`emu_gen_dict3.py`, run by `dict2-emucheck`, vectors in `re/out/si-scene/dict4/vec/`): 1980 cases, 1851
identical, 0 different, 129 not compared (read one operator: 46 where the original dereferences null when an
argument precedes a result at custom-option 0, 83 where it copies names from memory it never set). Per routine:
description dump 120/120, channel-setup getter 60/60, ascii cluster reader 200/200, operator attributes 400/400,
one attribute 500/500, one operator 571 of 700. Mutation: `EMUCHECK_MUTATE=1` and `=text` change all 1851
compared cases; `=calls` changes all but 39 ascii-cluster-reader cases (error paths that call nothing).
Corpus: not reachable. The callers of the operator readers are the store-based `reldictReadOperators` (si-ascii)
and the unported custom-effect loaders, so no file in the corpus drives them on the image.

Not ported: `SI_udfInitialize` (0x08544370, 941 bytes, 22 callees; with the unported udf read and set-dirty
routines) stays for a unit of the user-defined-field code. Hierarchy-animation relations
(`SI_elemdictSetAnimRelation` 0x08128c3c, `elemdictCheckPasteFcv` 0x081287c0) stay uncited: in this port the
hierarchy animations are Rust values in the scene's objects, not records on the image, so a byte-for-byte
check against the original has nothing to compare; the hybrids in `animrel.rs` are described, not cited.
`si-ascii` quoted 0x08111d58 for a capacity table that only reproduces part of the allocator; that comment now
names the function without a citation (the full port is checked here).

## 2026-10-29 (routing5): the cluster node reader is no longer a hook

`ocluster_ascii_read` in `si-scene/src/dictrest.rs` now calls `si_app::image::clusterread::cluster_anim_node_ascii_read`
(the hook CLUSTER_ANIM_NODE_ASCII_READ is kept only for `ocluster_ascii_read_with(.., false)`, which `dict2-emucheck` uses to match
its recorded vectors). si-scene gains the dependency `si-app` (si-app needs only si-x87 and si-mem: no cycle; Cargo.lock edge
si-scene -> si-app). An adapter `AsOutside` lets the dictionary world answer the reader's calls. This is wiring, no function is
newly reproduced. Checks: unit test with a scripted file world (answer 1, reader not asked of the world, model gets a cluster list
at +0x344); `dict2-emucheck dict4/vec` SI_oclusterAsciiRead 200 of 200 identical, total 1980 cases, 1851 identical, 0 different,
129 skipped. No mutation run. Nothing in the render path reaches this reader. The allocators SI_clusterAllocate and
SI_clusterKeyAllocate and SI_klusterGetByName stay world hooks.
