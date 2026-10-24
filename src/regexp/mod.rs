//! ECMAScript regular expression pattern parsing, independent of the VM.
//!
//! Matching and JavaScript RegExp objects are separate layers. Patterns and
//! diagnostics use UTF-16 offsets, including when the input has lone surrogates.

pub mod ir;
pub mod parse;
