# 0043: The spreadsheet and query modules (si-sprd)

## Where it stands

Package 54 (wave B), crate `si-sprd`, on the shared memory image.

- sprd_fct.c: all 41 functions ported and checked (2 of the 3 entries elsewhere:
  `SI_spdIsPredefined` 0x085a4e18 lives in si-scene).
- sprd_io.c: the two writers ported and checked (`sprdSetupGroupWrite`, `SI_sprdSetupWrite`).
  The two readers (`sprdSetupGroupRead` 0x085a7c88, `SI_sprdSetupRead` 0x085a825c) are not
  ported: the decompile of the first is a tangle of jumps into the middle of itself
  (0x085a8000, 0x085a8220) and needs reading from the disassembly.
- spreadsheet.c: 49 of 70 functions ported and checked (29 earlier + 20 now): `SI_spdCreate`
  0x083c8494, `spdAllocate` 0x083c9384, `spdAllocateRow` 0x083c94c8, `spdDispose` 0x083c956c,
  `spdDisposeRow` 0x083c96ec, `SI_spdDelete` 0x083c86dc, the texted cell routines (text, int,
  float, get and set), `SI_spdSetCheckCell`, `SI_spdSetColorCell`, `SI_spdSetColType`,
  `SI_spdSetCellEditable`, `SI_spdSetCellValid`, `clearAllSelection`, `cellSetSelectFlag`,
  `SI_spdSetSelectionFlag`. NOT ported (21): drawing (`SI_spdDraw`, `spdCellDraw`,
  `SI_spdDrawCell`, `spdDrawAppDirtyCells`), the events and selection treatment (`SI_spdAction`,
  `SI_spdSelection`, `SI_spdToggle`, `spdTreatCol/Row/Cell`, `spdActivateCell`,
  `handleMultipleCells`, `SI_spdEdition`, `SI_spdScrollbarCB`, `toggleSelectedCheck`,
  `setSelectedCheck`), `spdGetCellCoords`, `SI_spdDetectCell/Part`, `SI_spdWinBigEnough`,
  `si_sprdWriteResultContent`.
- Check (new 20, dir `re/out/si-sprd/vec2`, 60 cases each): 1200 cases, 1152 identical, 0
  different, 12 not compared (original read past the array in `SI_spdSetSelectionFlag` with
  row == rows). `SI_spdCreate` only 29 of 60 authoritative (log10 libm replay not available in
  the other 31); `SI_spdSetTextedCellF` 5 inf/nan cases are printf-nonfinite notes.
  Earlier set (`re/out/si-sprd/vec`): 21320 cases, 0 different.
- Mutation (new set): `EMUCHECK_MUTATE=1` changes the verdict of 7 of 20 functions (Delete,
  SetCellEditable, SetCellValid, cellSetSelectFlag, clearAllSelection, spdDispose,
  spdDisposeRow); for the other 13 it changes nothing (coverage: those routines only copy values
  and the switch perturbs only compare/loop results they do not use). `calls` changes 19 of 20;
  `cellSetSelectFlag` calls nothing outside, so no change there.

## Layouts read from the code

Group setup 0x50 bytes (name, query at +0x0c, sort list +0x28, field list +0x34, sub groups
+0x40/+0x44); setup 0x78 bytes (group at +8, element list at +0x58); clause 0x24; sort and field
item 0x14; element 0x18; sheet and cell (0x38 bytes) as in the head of
`crates/si-sprd/src/spreadsheet.rs`.

## 2026-10-28: first port

Everything above was written from the dossiers and checked against the emulated original.
Stand-ins (recorded, not ported here): the expression parser and its data
(`SI_getClauseFromQuery`, `SI_checkAllocStr`, `SI_genCusExprParsingTree`, `si_exprInitRelData`,
`SI_expressDataDispose`, `SI_exprRelDataDispose`, `SI_edElementAttCompValue`,
`SI_getExprOperStr`, `SI_validCustomExpress`), the dictionaries (`SI_enmdictEnumCodeGetByValue`,
`SI_attdictAttributeGetByIndex`, `SI_reldictGetRelName`), `SI_setIndent`, `fprintf`, `fopen`,
`fclose`. The `.rodata` texts of sprd_fct.c are written by `seed_text`. The licence
allocation counter (0x08679660) starts at 0 on both sides.

Weak spots of the vectors: `sprdQueryDuplicate` and the removal routines stop in about one case in
ten (the stand-in clause maker builds nothing); the sprintf path of `SI_sprdGenClauseStr` is
partly stopped in the emulator.

## 2026-10-28 (second step): sheet routines

Hooks (called through `Outside`, not reproduced): spdDrawAppDirtyCells (0x083c7abc), getfont
(0x0860ec74), font (0x0860ec44), strwidth (0x0860f084), getheight (0x0860eca4),
si_textedPerformCalc (0x0838eb68), SI_spdDrawCell (0x083c7d0c). `spdAllocateRow` types array is
u32 per column (stride 4). `SI_spdCreate` truncates log10(rows) (x87 round toward zero) and
calls strwidth twice when the header is not wider than the minimum. `SI_spdSetSelectionFlag`
keeps dead branches for negative row/col. `SI_spdSetTextedCell` returns 0 in its calc branch.
`strtod_text` handles decimal, inf, nan only (hex floats differ). Callbacks stored by address
use the dummy `cb` workaround (argument `v_cb` equal to the trap address); the kit lacks names
for callbacks stored in memory. Generator: `re/tools/si-sprd/gen_sheet2.py` with `sheetlib.py`.
Check: `sprd-checkkit re/out/si-sprd/vec2`.
Not done: sprd_io readers (0x085a7c88, 0x085a825c) and the clause-maker stand-in improvement.

## How to check

    (ulimit -v 6000000; timeout 3000 re/tools/lead/slot.sh emu -- uv run --with unicorn python3 -I re/tools/si-sprd/gen_fct.py re/out/si-sprd/vec 300)
    (same with gen_io.py and gen_sheet.py)
    re/tools/lead/slot.sh build -- cargo build --release -p si-sprd
    (ulimit -v 2000000; target/release/sprd-checkkit re/out/si-sprd/vec)   # EMUCHECK_MUTATE=1 | calls
