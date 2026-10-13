//! Build-time defines (light-cljs-214; "Phase 2 folding and inlining extension
//! points" in docs/advanced-optimization.md).
//!
//! `(goog-define DEBUG false)` defines a `^:const` Var whose default is a
//! boolean, number or string literal, as upstream's `goog-define` does. A
//! build replaces the default with [`crate::ApplicationOptions::defines`]
//! (`--define cljs.user/DEBUG=true` on the CLI), at every optimization level,
//! as upstream's `:closure-defines` do. At `Advanced` every read of a constant
//! with a literal value is that value ([`crate::folding`]), so
//! `(when DEBUG ...)` folds away and whatever only that code used is removed.
//!
//! A define names its Var by its qualified name, `ns/name`, and its value has
//! the kind of the default: a boolean define takes `true` or `false`, a number
//! define a number, and a string define any text. [`Define::parse`] reads the
//! CLI's `ns/name=value` against the program; [`apply`] rejects a define that
//! names no `goog-define` of the program or has another kind, and
//! [`crate::PreparedApplication::rejected_defines`] lists those.
use crate::analyzer::{ExprId, ExprKind, GlobalId, Program};
use std::fmt;

/// The value a build gives a `goog-define`.
#[derive(Clone, Debug, PartialEq)]
pub enum DefineValue {
    Boolean(bool),
    Number(f64),
    String(String),
}

impl DefineValue {
    /// The kind of value, as upstream's `@define` types name it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Boolean(_) => "boolean",
            Self::Number(_) => "number",
            Self::String(_) => "string",
        }
    }
}

/// One build-time define: the qualified name (`ns/name`) of a `goog-define`
/// Var and the value that replaces its default.
#[derive(Clone, Debug, PartialEq)]
pub struct Define {
    pub name: String,
    pub value: DefineValue,
}

/// Why a define does not apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefineError {
    /// The text is not `ns/name=value` with a qualified name.
    Syntax(String),
    /// The program defines no `goog-define` of this name.
    Unknown(String),
    /// The value is not of the default's kind.
    Kind {
        name: String,
        expected: &'static str,
        value: String,
    },
}

impl fmt::Display for DefineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(text) => write!(
                f,
                "--define requires NAMESPACE/NAME=VALUE (for example cljs.user/DEBUG=true), got {text:?}"
            ),
            Self::Unknown(name) => write!(f, "no goog-define named {name} in the program"),
            Self::Kind {
                name,
                expected,
                value,
            } => write!(
                f,
                "define {name} requires a {expected} value, got {value:?}"
            ),
        }
    }
}

impl std::error::Error for DefineError {}

impl Define {
    /// Read the CLI's `ns/name=value` for `program`: the value is read as the
    /// kind of the define's default, `true` or `false` for a boolean, a number
    /// (`42`, `-1.5`, `1e3`, `##Inf`, `##-Inf`, `##NaN`) for a number, and the
    /// text as written for a string.
    pub fn parse(program: &Program, text: &str) -> Result<Self, DefineError> {
        let syntax = || DefineError::Syntax(text.to_owned());
        let (name, value) = text.split_once('=').ok_or_else(syntax)?;
        if !name.contains('/') || name.starts_with('/') || name.ends_with('/') {
            return Err(syntax());
        }
        let (_, default) =
            find(program, name).ok_or_else(|| DefineError::Unknown(name.to_owned()))?;
        let kind = |expected| DefineError::Kind {
            name: name.to_owned(),
            expected,
            value: value.to_owned(),
        };
        let value = match program.exprs[default.0].kind {
            ExprKind::Bool(_) => match value {
                "true" => DefineValue::Boolean(true),
                "false" => DefineValue::Boolean(false),
                _ => return Err(kind("boolean")),
            },
            ExprKind::Number(_) => {
                DefineValue::Number(number(value).ok_or_else(|| kind("number"))?)
            }
            _ => DefineValue::String(value.to_owned()),
        };
        Ok(Self {
            name: name.to_owned(),
            value,
        })
    }
}

/// A number as the CLI spells it: decimal or exponent notation, or one of
/// the symbolic values.
fn number(text: &str) -> Option<f64> {
    match text {
        "##Inf" => Some(f64::INFINITY),
        "##-Inf" => Some(f64::NEG_INFINITY),
        "##NaN" => Some(f64::NAN),
        _ if text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E')) =>
        {
            text.parse().ok()
        }
        _ => None,
    }
}

/// The `goog-define` named `ns/name` and its default's literal expression.
fn find(program: &Program, name: &str) -> Option<(GlobalId, ExprId)> {
    let (namespace, local) = name.rsplit_once('/')?;
    let global = program
        .globals
        .iter()
        .position(|g| g.define && g.namespace == namespace && g.name == local)
        .map(GlobalId)?;
    // The definition the program's root reaches; analysis leaves other
    // scaffolding in the arena.
    let mut pending = vec![program.root];
    let mut visited = vec![false; program.exprs.len()];
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut visited[id.0], true) {
            continue;
        }
        if let ExprKind::Def {
            global: defined,
            init: Some(init),
            ..
        } = program.exprs[id.0].kind
            && defined == global
        {
            let literal = crate::application::unwrap_do(program, init);
            return matches!(
                program.exprs[literal.0].kind,
                ExprKind::Bool(_) | ExprKind::Number(_) | ExprKind::String(_)
            )
            .then_some((global, literal));
        }
        pending.extend(crate::program_fragment::children(program, id));
    }
    None
}

/// Replace the default of each define's `goog-define` with its value, and
/// return the defines that name none or whose value has another kind.
pub(crate) fn apply(program: &mut Program, defines: &[Define]) -> Vec<DefineError> {
    let mut rejected = Vec::new();
    for define in defines {
        let Some((_, literal)) = find(program, &define.name) else {
            rejected.push(DefineError::Unknown(define.name.clone()));
            continue;
        };
        let kind = match (&program.exprs[literal.0].kind, &define.value) {
            (ExprKind::Bool(_), DefineValue::Boolean(value)) => ExprKind::Bool(*value),
            (ExprKind::Number(_), DefineValue::Number(value)) => ExprKind::Number(*value),
            (ExprKind::String(_), DefineValue::String(value)) => {
                program.strings.push(value.encode_utf16().collect());
                ExprKind::String(crate::analyzer::StringId(program.strings.len() - 1))
            }
            (default, value) => {
                rejected.push(DefineError::Kind {
                    name: define.name.clone(),
                    expected: match default {
                        ExprKind::Bool(_) => "boolean",
                        ExprKind::Number(_) => "number",
                        _ => "string",
                    },
                    value: match value {
                        DefineValue::Boolean(value) => value.to_string(),
                        DefineValue::Number(value) => value.to_string(),
                        DefineValue::String(value) => value.clone(),
                    },
                });
                continue;
            }
        };
        program.exprs[literal.0].kind = kind;
    }
    rejected
}
