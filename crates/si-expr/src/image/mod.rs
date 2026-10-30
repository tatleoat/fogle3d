// SPDX-License-Identifier: MIT
//! The expression code on the shared memory image (finding 0024; finding 0013, section 12).
//!
//! The value form ([`crate::eval`], [`crate::fct`], [`crate::tree`]) keeps its trees as
//! Rust values and reaches the scene through [`crate::scene::Scene`]. Here the same
//! algorithm works on the original's records in `si-mem`'s image: the trees
//! (`SI_Expression`, `expr_node`, `SI_ExpNopNode` and the leaf records), the relation's
//! data (`SI_ExprRelData`), the update's node, the elements of the dictionary (`si-dict`),
//! the function curves (`si-motion`'s bridge) and the models' parameters. Every function
//! takes the image, an outside world ([`si_mem::Outside`], for what is not on the image)
//! and the original's arguments, and makes the original's allocation calls.
//!
//! | Module | Original source | What |
//! |---|---|---|
//! | [`rec`] | | records, globals, their initial values |
//! | [`calls`] | | the callees that are not on the image |
//! | [`eval`] | `exp_fct.c`, `exp_parsing.c`, `edparams.c` | `SI_animExpressionEvaluate`, `SI_expressionEvaluate`, `si_evalExprExec` and their helpers |
//! | [`elem`] | `edanim.c`, `edparams.c`, `attrib.c` | a parameter of an element read and written; a model's visibility; the `General_Element` attributes |
//! | [`regen`] | `exp_parsing.c`, `exp_fct.c`, `express_io.c` | references tied to the relation, the relation's data, the random tables |
//! | [`exio`] | `express_io.c` | an expression added to or given up from the dictionary |
//! | [`tree`] | | value-form trees into the image and back (adapter for the parser) |
//! | [`io`] | `physicalio.c` | an expression file read into the dictionary (adapter for the reader) |
//!
//! The update calls `SI_animExpressionEvaluate(node, time)` ([`anim_expression_evaluate`]).
//! What a host must do around it: [`rec::init_globals`] once at start-up (the evaluator's
//! statics start at −1 and 0), and after a scene is loaded the expressions read
//! ([`io::elemdict_read_expressions`]) and tied to their relations
//! ([`regen::expressions_regenerate`]), as the loader does before the topological sort.

pub mod attrget;
pub mod calls;
pub mod elem;
pub mod exio;
pub mod express_dial;
pub mod eval;
pub mod io;
pub mod lexrt;
pub mod misc;
pub mod modelparam;
pub mod params;
pub mod parsing;
pub mod qstretch_misc;
pub mod rec;
pub mod regen;
pub mod tree;

pub use eval::{anim_expression_evaluate, expression_evaluate};
