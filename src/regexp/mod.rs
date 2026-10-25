//! ECMAScript regular expression parsing and bounded matching, independent of the VM.
//!
//! JavaScript RegExp objects are a separate layer. Patterns and
//! diagnostics use UTF-16 offsets, including when the input has lone surrogates.

pub mod ir;
pub mod matcher;
pub mod parse;

pub mod case_fold;
pub(crate) mod object;
pub mod property;
mod sets;
pub mod string_property;
