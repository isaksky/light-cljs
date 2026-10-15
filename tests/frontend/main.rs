//! Tokenizer, parser, analyzer and namespace integration tests, linked into a single
//! test binary. Add a suite as `tests/frontend/<name>.rs` and register it below.

// Temporary directories removed when their test ends, however it ends.
#[path = "../support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

mod analyzer;
mod boundaries;
mod corpus;
mod full_lexer;
mod namespaces;
mod parser;
mod parser_corpus;
mod parser_reference;
mod tokenizer;
