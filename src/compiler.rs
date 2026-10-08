//! Small end-to-end driver. Analysis and emission remain independently usable.

use std::fmt;

use crate::{AnalysisError, ParseError, analyze, emit_js, parse};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompileError {
    Parse(ParseError),
    Analysis(AnalysisError),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(f),
            Self::Analysis(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CompileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Parse(error) => error,
            Self::Analysis(error) => error,
        })
    }
}

pub fn compile(source: &str) -> Result<String, CompileError> {
    let syntax = parse(source).map_err(CompileError::Parse)?;
    let program = analyze(source, &syntax).map_err(CompileError::Analysis)?;
    Ok(emit_js(&program))
}
