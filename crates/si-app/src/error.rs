// SPDX-License-Identifier: MIT
//! `SI_error.c`: the error service. A numbered error has a type and a `printf` format in a
//! table; the service formats it and sends it to the standard output, the standard error
//! stream or a handler, unless its type is below the filter.

use crate::env::{Bytes, World};
pub use crate::error_table::{ERROR_FORMAT, ERROR_TYPE};
use crate::state::{App, ERROR_TO_HANDLER, ERROR_TO_STDERR, ERROR_TO_STDOUT};

/// An argument of an error message. The table's formats use `%s` and `%d` only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FmtArg {
    S(Bytes),
    D(i32),
}

/// `vsprintf` for the two conversions the error formats use. A conversion without an
/// argument left prints nothing here (the original would read whatever follows on the
/// stack).
pub fn format_message(format: &[u8], args: &[FmtArg]) -> Bytes {
    let mut out = Vec::new();
    let mut next = args.iter();
    let mut i = 0;
    while i < format.len() {
        let c = format[i];
        if c == b'%' && i + 1 < format.len() {
            match format[i + 1] {
                b's' => {
                    match next.next() {
                        Some(FmtArg::S(s)) => out.extend_from_slice(s),
                        Some(FmtArg::D(d)) => out.extend_from_slice(d.to_string().as_bytes()),
                        None => {}
                    }
                    i += 2;
                    continue;
                }
                b'd' => {
                    match next.next() {
                        Some(FmtArg::D(d)) => out.extend_from_slice(d.to_string().as_bytes()),
                        Some(FmtArg::S(_)) | None => {}
                    }
                    i += 2;
                    continue;
                }
                b'%' => {
                    out.push(b'%');
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// What [`App::si_error`] did besides printing: the call of the handler, when the output
/// mode is "handler" (the handler is a function pointer of the caller's; the port hands
/// its arguments back instead of calling through it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandlerCall {
    pub code: i32,
    pub kind: i32,
    pub text: Bytes,
}

impl<W: World> App<W> {
    /// [soft 0x081417d0 SI_error]: reports error `code` with its arguments and returns the
    /// code. The formatted text is handed to `printf` once more **as a format**, so a `%`
    /// in an argument is interpreted again: `%%` becomes `%` here; any other conversion
    /// would read stray stack words in the original and is printed as it stands.
    /// A code outside the table reads past it in the original; here it panics.
    pub fn si_error(&mut self, code: i32, args: &[FmtArg]) -> (i32, Option<HandlerCall>) {
        let (_, kind, format) = ERROR_FORMAT[code as usize];
        if kind < self.g.error.filter {
            return (code, None);
        }
        let text = format_message(format, args);
        let mut handler = None;
        match self.g.error.output {
            ERROR_TO_STDOUT => {
                let head = [b"SI_".as_slice(), ERROR_TYPE[kind as usize], b": "].concat();
                self.w.out(&head);
                self.w.out(&format_message(&text, &[]));
                self.w.out(b"\n");
            }
            ERROR_TO_STDERR => {
                let head = [b"SI_".as_slice(), ERROR_TYPE[kind as usize], b": "].concat();
                self.w.err(&head);
                self.w.err(&format_message(&text, &[]));
                self.w.err(b"\n");
            }
            ERROR_TO_HANDLER => handler = Some(HandlerCall { code, kind, text }),
            _ => {}
        }
        (code, handler)
    }

    /// [soft 0x081418e4 SI_errorGetMsg]: the format of an error code.
    pub fn error_get_msg(&self, code: i32) -> &'static [u8] {
        ERROR_FORMAT[code as usize].2
    }

    /// [soft 0x081418f8 SI_errorGetType]: the type of an error code (0 status, 1 message,
    /// 2 warning, 3 error, 4 mode).
    pub fn error_get_type(&self, code: i32) -> i32 {
        ERROR_FORMAT[code as usize].1
    }

    /// [soft 0x0814190c SI_errorSetHandler]: sets the handler, returns the old one.
    pub fn error_set_handler(&mut self, handler: u32) -> u32 {
        std::mem::replace(&mut self.g.error.handler, handler)
    }

    /// [soft 0x08141924 SI_errorGetHandler].
    pub fn error_get_handler(&self) -> u32 {
        self.g.error.handler
    }

    /// [soft 0x08141930 SI_errorSetOutputMode]: sets where errors go, returns the old mode.
    pub fn error_set_output_mode(&mut self, mode: i32) -> i32 {
        std::mem::replace(&mut self.g.error.output, mode)
    }

    /// [soft 0x08141948 SI_errorGetOutputMode].
    pub fn error_get_output_mode(&self) -> i32 {
        self.g.error.output
    }

    /// [soft 0x08141954 SI_errorSetFilter]: sets the lowest type reported, returns the old
    /// one.
    pub fn error_set_filter(&mut self, filter: i32) -> i32 {
        std::mem::replace(&mut self.g.error.filter, filter)
    }

    /// [soft 0x0814196c SI_errorGetFilter].
    pub fn error_get_filter(&self) -> i32 {
        self.g.error.filter
    }
}
