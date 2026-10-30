// SPDX-License-Identifier: MIT
//! The expression language of Softimage|3D 4.0: the formulas a user attaches to an animatable
//! parameter (finding 0013).
//!
//! Sources are functions of the Linux `soft` binary (sha256 `ef21a382c3325d42…`), cited as
//! `[soft 0xADDRESS name]`. Source files: `exp_lex.c` (scanner), `exp_yacc.c` (parser),
//! `exp_token.c` (word tables), `exp_parsing.c` (tree, printer, evaluator), `exp_fct.c`
//! (built-in functions), `express_io.c` (reading and writing).

pub mod tree;
pub mod env;
pub mod lex;
#[rustfmt::skip]
pub mod lex_tables;
pub mod parse;
pub mod token;
#[rustfmt::skip]
pub mod yacc_tables;
pub mod format;
pub mod io;
pub mod fct;
pub mod rand;
pub mod scene;
pub mod value;
pub mod eval;
pub mod util;
pub mod image;
#[cfg(test)]
mod tests;
