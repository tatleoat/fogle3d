// SPDX-License-Identifier: MIT
//! Expressions on disc (`express_io.c` and the four file-level functions of
//! `physicalio.c`), finding 0013.
//!
//! A scene keeps all its expressions in one text file in the EXPRESSIONS chapter
//! (`<name>.exp`). The file starts with the tag `SEXP` and a version, then holds one block
//! per expression:
//!
//! ```text
//! EXPRESSION <element name>
//! <type> <parameter> ret_el
//!     (
//!        arg1 ,
//!        arg2
//!     )
//! {
//!     <variable> = <text> ;
//!     return( <text> );
//! }
//! ```
//!
//! The reader takes the block apart with the token reader of the other text files
//! (finding 0006), glues `<type> ret_el.<parameter> = <text>` together and hands that to the
//! parser; the writer prints a tree with references written as `arg<n>`.

// adapter: not in place; the file reader and writer on value trees; `crate::image::io` reads into the image
// through this one.

use crate::env::Dict;
use crate::format::{get_expr_arg_list, get_ftype_name, si_get_expr_node_format, ArgType};
use crate::parse::{Parser, Undefined, VarDef};
use crate::token;
use crate::tree::*;
use si_ascii::AsciiFile;

/// The tag of an expression file.
pub const TAG: u32 = u32::from_be_bytes(*b"SEXP");

/// The name every stored expression gives its own target.
pub const RET_EL: &[u8] = b"ret_el";

#[derive(Clone, Debug, PartialEq)]
pub enum ReadError {
    /// The file ends inside a block. The original's loops there do not test their reads: it
    /// would read empty tokens for ever.
    EndlessAtEof(&'static str),
    Undefined(Undefined),
}

impl From<Undefined> for ReadError {
    fn from(e: Undefined) -> Self {
        ReadError::Undefined(e)
    }
}

/// What one block gave.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    /// The tree, or `None` if the text did not pass the check (a null pointer).
    pub expr: Option<Expression>,
    /// The text handed to the parser.
    pub text: Vec<u8>,
    /// The variables as read.
    pub vars: Vec<VarDef>,
    /// The argument names: `ret_el` first, then those of the list. `None` for an empty list.
    pub args: Option<Vec<Vec<u8>>>,
    /// If the check failed: the variable at fault (−1: the main text), the offset in its
    /// text and the parser's message.
    pub error: Option<(i32, isize, String)>,
}

fn next(f: &mut AsciiFile, what: &'static str) -> Result<(), ReadError> {
    if !f.token_read() && f.token().is_empty() {
        return Err(ReadError::EndlessAtEof(what));
    }
    Ok(())
}

/// Reads one block from after its `EXPRESSION <name>` line
/// [soft 0x08255bbc SI_expressionRead].
///
/// The original copies the type and the parameter into 100-byte buffers and formats the
/// start of the text into 252 bytes, without checks; Rust has no such limits.
pub fn expression_read(f: &mut AsciiFile, parser: &mut Parser, dict: &dyn Dict) -> Result<Block, ReadError> {
    let mut block = Block::default();
    f.token_read();
    let kind = f.token().to_vec();
    f.token_read();
    let param = f.token().to_vec();
    // "%s %s.%s =" and a blank
    let mut text = kind;
    text.push(b' ');
    text.extend_from_slice(RET_EL);
    text.push(b'.');
    text.extend_from_slice(&param);
    text.extend_from_slice(b" = ");
    f.token_reach("(", b';');
    f.token_read();
    if !f.is(")") {
        let mut args = vec![RET_EL.to_vec()];
        loop {
            let mut name = f.token().to_vec();
            if name.last() == Some(&b',') {
                name.pop();
            }
            args.push(name);
            next(f, "argument list")?;
            if f.is(",") {
                next(f, "argument list")?;
            }
            if f.is(")") {
                break;
            }
        }
        block.args = Some(args);
    }
    f.token_reach("{", b'}');
    f.token_read();
    // variables, up to a token that starts with `return`
    while !f.token().starts_with(b"return") {
        if f.token().is_empty() {
            return Err(ReadError::EndlessAtEof("variables"));
        }
        let name = f.token().to_vec();
        f.token_reach("=", b'=');
        f.token_read();
        let mut def: Option<Vec<u8>> = None;
        while !f.is(";") {
            let d = def.get_or_insert_with(Vec::new);
            d.extend_from_slice(f.token());
            d.push(b' ');
            next(f, "variable definition")?;
        }
        block.vars.push(VarDef { name, text: def });
        f.token_read();
    }
    // the returned text: tokens up to the parenthesis that closes `return(`, both of which
    // become blanks
    let mut depth = f.token().contains(&b'(') as i32;
    f.token_read();
    loop {
        let mut tok = f.token().to_vec();
        if tok.is_empty() {
            return Err(ReadError::EndlessAtEof("returned text"));
        }
        let mut done = false;
        for c in tok.iter_mut().filter(|c| **c == b'(') {
            if depth == 0 {
                *c = b' ';
            }
            depth += 1;
        }
        for c in tok.iter_mut().filter(|c| **c == b')') {
            depth -= 1;
            if depth < 1 {
                *c = b' ';
                done = true;
            }
        }
        text.extend_from_slice(&tok);
        text.push(b' ');
        f.token_read();
        if done {
            break;
        }
    }
    f.token_reach("}", b';');
    match parser.valid_custom_express(dict, &text, &block.vars, 0, 1)? {
        None => {
            block.expr = Some(parser.gen_cus_expr_parsing_tree(dict, &text, &block.vars, 0, block.args.as_deref())?);
        }
        Some((var, at)) => block.error = Some((var, at, parser.last_error_msg.clone())),
    }
    block.text = text;
    Ok(block)
}

fn item(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(s);
    out.push(b' ');
}

/// What `fprintf("%s ")` prints for a text that may be a null pointer.
fn or_null(s: Option<Vec<u8>>) -> Vec<u8> {
    s.unwrap_or_else(|| b"(null)".to_vec())
}

/// Writes one block without its `EXPRESSION` line [soft 0x082561c4 SI_expressionWrite].
pub fn expression_write(out: &mut Vec<u8>, dict: &dyn Dict, expr: &Expression) -> Result<(), Undefined> {
    // a null root faults in the original
    let root = expr.root.as_deref().ok_or(Undefined::StalePointer(30))?;
    match root.left.as_deref().and_then(|t| t.leaf.as_deref()) {
        None => {
            // no target: a model's first translation parameter
            item(out, b"MOD");
            item(out, &dict.name_from_fcurve(0x8b));
        }
        Some(leaf) => {
            // the original reads the leaf as a reference whatever its kind
            let NopData::Arg(a) = &leaf.data else { return Err(Undefined::StalePointer(31)) };
            item(out, token::get_obj_type_from_chapid(a.chapter).as_bytes());
            if a.fcv == FCV_CUSTOM {
                item(out, &or_null(a.custom_name.clone()));
            } else {
                item(out, &get_ftype_name(dict, a));
            }
        }
    }
    item(out, RET_EL);
    out.push(b'\n');
    item(out, b"   ");
    item(out, b"(");
    out.push(b'\n');
    if let Some(args) = get_expr_arg_list(expr) {
        for (i, a) in args.iter().enumerate() {
            item(out, b"      ");
            item(out, a);
            if i + 1 < args.len() {
                item(out, b",");
            }
            out.push(b'\n');
        }
    }
    item(out, b"   ");
    item(out, b")");
    out.push(b'\n');
    item(out, b"{");
    out.push(b'\n');
    for v in &expr.vars {
        item(out, b"   ");
        item(out, &v.name);
        item(out, b"=");
        item(out, &or_null(si_get_expr_node_format(dict, expr, v.expr.as_deref(), ArgType::Number, 0)?));
        item(out, b";");
        out.push(b'\n');
    }
    let rhs = si_get_expr_node_format(dict, expr, root.right.as_deref(), ArgType::Number, 0)?;
    item(out, b"   ");
    item(out, b"return(");
    item(out, &or_null(rhs));
    item(out, b");");
    out.push(b'\n');
    item(out, b"}");
    out.push(b'\n');
    Ok(())
}

/// The line that precedes a block: an empty line, `EXPRESSION`, the element's name.
fn write_block_head(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b'\n');
    item(out, b"EXPRESSION");
    item(out, name);
    out.push(b'\n');
}

/// Reads every block of an expression file whose name `wanted` accepts
/// [soft 0x0810ff94 SI_elemdictReadExpressions]. The original accepts the names of the
/// elements the scene description listed in its EXPRESSIONS chapter and stores each tree in
/// its element; a block with another name is passed over token by token.
///
/// The header reader does not know the tag `SEXP` (finding 0002): it rewinds, and the first
/// token, tag and version together, is passed over like any other.
pub fn read_expressions(
    data: &[u8],
    parser: &mut Parser,
    dict: &dyn Dict,
    wanted: &mut dyn FnMut(&[u8]) -> bool,
) -> Result<Vec<(Vec<u8>, Block)>, ReadError> {
    let mut f = AsciiFile::new(data);
    si_ascii::read_asc_header(&mut f);
    let mut out = Vec::new();
    while f.token_reach("EXPRESSION", b';') {
        f.token_read();
        let name = f.token().to_vec();
        if wanted(&name) {
            let block = expression_read(&mut f, parser, dict)?;
            out.push((name, block));
        }
    }
    Ok(out)
}

/// Reads the first block of an expression file [soft 0x08110090 SI_expressionLoad]: the
/// dialog's "load an expression".
pub fn expression_load(data: &[u8], parser: &mut Parser, dict: &dyn Dict) -> Result<Option<Block>, ReadError> {
    let mut f = AsciiFile::new(data);
    si_ascii::read_asc_header(&mut f);
    if !f.token_reach("EXPRESSION", b';') {
        return Ok(None);
    }
    f.token_read();
    Ok(Some(expression_read(&mut f, parser, dict)?))
}

/// Writes an expression file [soft 0x08110710 SI_elemdictWriteExpressions]: the header and
/// a block for each element given. The original takes the elements of the EXPRESSIONS
/// chapter that are not deleted, are roots and hold a tree, in the chapter's order.
pub fn write_expressions(
    dict: &dyn Dict,
    version: f32,
    exprs: &[(&[u8], &Expression)],
) -> Result<Vec<u8>, Undefined> {
    let mut out = Vec::new();
    si_file::write_asc_header(&mut out, version, TAG);
    for (name, expr) in exprs {
        write_block_head(&mut out, name);
        expression_write(&mut out, dict, expr)?;
    }
    Ok(out)
}

/// Writes a file with one expression [soft 0x08110898 SI_writeAnExpression]: the dialog's
/// "save".
pub fn write_an_expression(dict: &dyn Dict, version: f32, name: &[u8], expr: &Expression) -> Result<Vec<u8>, Undefined> {
    write_expressions(dict, version, &[(name, expr)])
}
