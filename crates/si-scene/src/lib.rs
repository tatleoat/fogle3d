// SPDX-License-Identifier: MIT
//! Scene loading and saving through the element dictionary (finding 0018).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `dbloadsave.c`, `dictin.c`, `dictout.c`,
//! `dictin_scn.c`, `dictout_scn.c`, `physicalio.c`, `elemdict.c`, `relations.c`, `reldict.c`,
//! `attdict.c`, `dbvsfcv.c`, `scn2db.c`, `db2scn.c`, `dbexternal.c`.
//!
//! The other crates each read and write one kind of file. This one does what the program
//! does on "Get Scene" and "Save Scene": it reads the scene description into the element
//! dictionary, finds and reads each element file, ties elements together by relations, and
//! writes it all back.
//!
//! **On the memory image** (`si-mem`, finding 0024; the dictionary routines are
//! `si-dict`'s):
//!
//! - [`load`]: "Get Scene": the scene's element, the description applied to the
//!   dictionary, the element files found and read, the steps that follow. The scene is
//!   built in the image ([`load::Scene::mem`]).
//! - [`animrel`]: the curves of the animation files tied to the elements they animate;
//!   [`db2scn`]: the scene's structures tied from the relations on the image
//!   (`SI_scnRelationsFromElemDict`); [`scnrel`]: the same on the handles the saver reads,
//!   clusters, and a model file's hierarchy put together for the writer.
//! - [`dbnode`]: name records made, freed, cleaned and formatted in place: what the
//!   dictionaries ask of the database layer; [`globals`]: the loader's globals.
//! - [`dupnames`]: the duplicate-name check that ends a load and starts a save.
//! - [`dbvsfcv`]: curve ids seen from the database side (the attribute dictionary itself,
//!   `SI_ATD`, is `si-dict`'s and is filled at start-up).
//! - [`world`]: what stands beside the image during a load: the dictionaries' outside
//!   world, the addresses of objects that are not in the image yet, and the two adapters
//!   between the image and the stores.
//! - [`image`]: the contents of the elements in the image: the outside world of the file
//!   readers of `si-hrc` and `si-elemio` and of `si-view`'s list during a load, and the
//!   routines around those readers; [`edtools`]: a model node and its element;
//!   [`matconv`]: the copies the load's material conversion makes; [`custom`]: what a
//!   shader reference's element brings to its parameter arrays.
//! - [`init`]: start-up of the dictionaries in the image (and, for callers not in place,
//!   in stores); [`object`]: what the elements point at.
//!
//! **Still on the stores of [`dict`]**, which a load fills from the image when it ends
//! (finding 0018, section 14, has the list function by function):
//!
//! - [`save`]: the scene description written from the dictionary, sorted and numbered.
//! - [`savescene`]: "Save Scene" from start to end: what is taken back from the scene graph,
//!   names, the dirty marks; [`dictout`]: which elements are written, under which name and
//!   version, by which writer; the animation files rebuilt from the relations.
//! - [`elemsort`]: the order of a chapter's elements, for the description writer (the
//!   form on the image is `si-dict`'s); [`csort`]: the C library's sort.
//! - [`elemrel`], [`elemdict`]: the relation routines the saver and `si-rensetup` still
//!   call in their store form; [`updlist`]: the update list as a value.
//! - [`calls`]: calls into the parts of the program that are not ported, recorded.
//!
//! Checks: `scene-check <dir> [load|save|all|saveall|image]` over the sample databases,
//! `scene-emucheck <dir>` against vectors of the emulation harness
//! (`re/tools/si-scene/emu_gen.py`, `emu_gen_save.py`, `emu_gen_node.py`,
//! `emu_gen_world.py`, `emu_gen_fcv.py`, `emu_gen_dup.py`), and
//! `dict-check <dir>`, which asks `si-dict`'s functions on each loaded scene's image and
//! the store-based ones on its stores the same questions.

pub mod calls;
pub mod csort;
pub mod custom;
pub mod db2scn;
pub mod dbnode;
pub mod dict;
pub mod edname;
pub mod dictdump;
pub mod dictrest;
pub mod elemrel;
pub mod elemsort;
pub mod globals;
pub mod init;
pub mod load;
pub mod matconv;
pub mod object;
pub mod save;
pub mod animrel;
pub mod animrel_img;
pub mod dbvsfcv;
pub mod dictout;
pub mod dispose;
pub mod dupnames;
pub mod edtools;
pub mod edfirst;
pub mod envside;
pub mod image;
pub mod savescene;
pub mod scnrel;
pub mod elemdict;
pub mod t2dpath;
pub mod untold;
pub mod updlist;
pub mod wave;
pub mod wave_geom;
pub mod wave_mdl;
pub mod wave_sys;
pub mod wave_tables;
pub mod wave_update;
pub mod loadfix;
pub mod load_pipe;
pub mod world;

#[cfg(test)]
mod tests;
