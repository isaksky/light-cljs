//! An arena of pattern nodes. Edges point to earlier nodes, so traversal and
//! destruction need no recursive Rust calls. Capture indices start at one.

use crate::JsString;

/// Index into [`Pattern::nodes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeId(pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    pub flags: super::parse::Flags,
    pub nodes: Vec<Node>,
    pub root: NodeId,
    /// Capture names in opening-parenthesis order; unnamed captures are None.
    pub captures: Vec<Option<JsString>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Disjunction(Vec<NodeId>),
    Alternative(Vec<NodeId>),
    /// A code unit without `u`, a code point (possibly a lone surrogate) with it.
    Literal(u32),
    Dot,
    Assertion(Assertion),
    Lookaround {
        body: NodeId,
        behind: bool,
        negative: bool,
    },
    Group {
        body: NodeId,
        capture: Option<usize>,
    },
    Modifiers {
        body: NodeId,
        enable: Modifiers,
        disable: Modifiers,
    },
    /// One numeric capture, or all candidates for a named reference whose
    /// duplicate declarations are in mutually exclusive alternatives.
    Backreference(Vec<usize>),
    Class(CharacterClass),
    Repeat {
        body: NodeId,
        min: u64,
        max: Option<u64>,
        greedy: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assertion {
    Start,
    End,
    WordBoundary,
    NotWordBoundary,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterClass {
    pub inverted: bool,
    pub members: Vec<ClassMember>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassMember {
    Literal(u32),
    Range(u32, u32),
    Escape(ClassEscape),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassEscape {
    Digit,
    NotDigit,
    Word,
    NotWord,
    Space,
    NotSpace,
}
