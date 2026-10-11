//! Pull-based syntax parsing. Reader expansion and namespace resolution are deferred.

use std::{fmt, ops::Range};

use crate::{LexError, Token, TokenKind, Tokenizer, literals};

/// An index into one parser/tree's node arena. IDs survive arena reallocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct NodeId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionKind {
    List,
    Vector,
    Map,
    Set,
    AnonymousFn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrefixKind {
    Quote,
    SyntaxQuote,
    Unquote,
    UnquoteSplicing,
    Deref,
    VarQuote,
}

/// Validated spelling, without choosing a numeric runtime representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberKind {
    Integer,
    BigInteger,
    Float,
    Decimal,
    Ratio,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SymbolicValue {
    Infinity,
    NegativeInfinity,
    NaN,
}

/// Unescaped strings borrow by offset. Decoded strings preserve JVM/JS UTF-16,
/// including unpaired surrogate escapes, which cannot be stored in a Rust str.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StringValue {
    Source(Range<usize>),
    Decoded(Vec<u16>),
}

impl StringValue {
    /// Materialize the value using the exact original source buffer.
    pub fn to_utf16(&self, source: &str) -> Vec<u16> {
        match self {
            Self::Source(span) => source[span.clone()].encode_utf16().collect(),
            Self::Decoded(value) => value.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeKind {
    Nil,
    Boolean(bool),
    Symbol,
    Keyword {
        auto_resolve: bool,
    },
    Number(NumberKind),
    String(StringValue),
    /// Raw pattern inside #"..."; compilation belongs to the target runtime.
    Regex,
    Character(char),
    SymbolicValue(SymbolicValue),
    Collection {
        kind: CollectionKind,
        children: Vec<NodeId>,
    },
    Prefix {
        kind: PrefixKind,
        form: NodeId,
    },
    Metadata {
        metadata: NodeId,
        target: NodeId,
    },
    Tagged {
        tag: NodeId,
        form: NodeId,
    },
    Conditional {
        splicing: bool,
        branches: NodeId,
    },
    NamespacedMap {
        auto_resolve: bool,
        namespace: Range<usize>,
        map: NodeId,
    },
    /// `%`, `%&`, or `%<index form>` inside #(). None is the implicit %1;
    /// explicit indices remain numeric nodes until anonymous-function expansion.
    /// Interpretation of ratio indices belongs to the semantic reader profile.
    Argument {
        rest: bool,
        index: Option<NodeId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Range<usize>,
}

/// Owns syntax storage, not the source. Keep the original source alongside it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyntaxTree {
    pub nodes: Vec<Node>,
    pub roots: Vec<NodeId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Lexical(LexError),
    UnexpectedDelimiter(char),
    MismatchedDelimiter { expected: char, found: char },
    UnexpectedEof(&'static str),
    NestingLimit,
    OddMap,
    InvalidNumber,
    InvalidSymbol,
    InvalidKeyword,
    InvalidEscape,
    InvalidCharacter,
    InvalidMetadata,
    InvalidTag,
    InvalidConditional,
    InvalidNamespace,
    InvalidSymbolicValue,
    InvalidArgument,
    NestedAnonymousFunction,
    ReadEvalDisabled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub kind: ErrorKind,
    /// Primary byte span; EOF errors use an empty span at source.len().
    pub span: Range<usize>,
    /// Opening delimiter or prefix responsible for a missing/mismatched form.
    pub related: Option<Range<usize>>,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Lexical(error) => write!(f, "lexical error: {error:?}"),
            ErrorKind::UnexpectedDelimiter(found) => {
                write!(f, "unexpected closing delimiter {found}")
            }
            ErrorKind::MismatchedDelimiter { expected, found } => {
                write!(f, "expected {expected}, found {found}")
            }
            ErrorKind::UnexpectedEof(expected) => {
                write!(f, "unexpected end of input; expected {expected}")
            }
            ErrorKind::NestingLimit => f.write_str("nesting limit exceeded"),
            ErrorKind::OddMap => f.write_str("map requires an even number of forms"),
            ErrorKind::ReadEvalDisabled => f.write_str("reader evaluation (#=) is disabled"),
            ErrorKind::NestedAnonymousFunction => f.write_str("nested #() forms are not allowed"),
            other => write!(f, "invalid reader form: {other:?}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Newline index built separately from parsing, so diagnostics impose no scan
/// or allocation on successful parses. Positions are 1-based lines/byte columns.
#[derive(Clone, Debug)]
pub struct LineIndex {
    starts: Vec<usize>,
    length: usize,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let bytes = source.as_bytes();
        let mut starts = vec![0];
        let mut i = 0;
        while i < bytes.len() {
            let ch = bytes[i];
            i += 1;
            if ch == b'\r' && matches!(bytes.get(i), Some(b'\n' | b'\x0c')) {
                i += 1;
            }
            if matches!(ch, b'\r' | b'\n') {
                starts.push(i);
            }
        }
        Self {
            starts,
            length: source.len(),
        }
    }

    pub fn line_column(&self, offset: usize) -> Option<(usize, usize)> {
        if offset > self.length {
            return None;
        }
        let line = self.starts.partition_point(|&start| start <= offset) - 1;
        Some((line + 1, offset - self.starts[line] + 1))
    }
}

/// Recursive descent over a borrowing scanner with at most one cached token.
/// Siblings use loops; only nested forms recurse. An error terminates the parser.
pub struct Parser<'src> {
    source: &'src str,
    lexer: Tokenizer<'src>,
    lookahead: Option<Token>,
    nodes: Vec<Node>,
    nesting_limit: usize,
    in_anonymous_fn: bool,
    finished: bool,
    retain_discards: bool,
    discarded: Vec<NodeId>,
}

impl<'src> Parser<'src> {
    /// The default and hard maximum nesting limit is 128, including prefixes.
    pub fn new(source: &'src str) -> Self {
        Self::with_nesting_limit(source, 128)
    }

    /// Lower the limit (clamped to 128); zero permits only an empty input.
    pub fn with_nesting_limit(source: &'src str, limit: usize) -> Self {
        Self {
            source,
            lexer: Tokenizer::new(source),
            lookahead: None,
            nodes: Vec::new(),
            nesting_limit: limit.min(128),
            in_anonymous_fn: false,
            finished: false,
            retain_discards: false,
            discarded: Vec::new(),
        }
    }

    // Resume an owned semantic cursor without borrowing a previous parser arena.
    // Offsets are validated by the caller against this exact source.
    pub(crate) fn at_offset(source: &'src str, offset: usize) -> Self {
        let mut parser = Self::new(source);
        parser.lexer.cursor = offset;
        parser
    }

    pub(crate) fn position(&self) -> usize {
        self.lookahead
            .as_ref()
            .map_or(self.lexer.cursor, |t| t.span.start)
    }

    /// Retain discarded subtrees in a side table for semantic reader validation.
    /// Ordinary roots/children still exclude them. Default parsing is unchanged.
    pub fn retaining_discards(mut self) -> Self {
        self.retain_discards = true;
        self
    }

    pub fn discarded(&self) -> &[NodeId] {
        &self.discarded
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn into_nodes(self) -> Vec<Node> {
        self.nodes
    }

    /// Read one retained top-level form. The caller can inspect/analyze it before
    /// requesting the next. After EOF or the first error, returns Ok(None).
    pub fn read_form(&mut self) -> Result<Option<NodeId>, ParseError> {
        if self.finished {
            return Ok(None);
        }
        let mark = self.nodes.len();
        let discarded_mark = self.discarded.len();
        let result = (|| {
            self.skip_discarded(0, false)?;
            if self.peek().is_none() {
                return Ok(None);
            }
            self.read_one(0, false).map(Some)
        })();
        if !matches!(result, Ok(Some(_))) {
            self.finished = true;
            if result.is_err() {
                self.nodes.truncate(mark);
                self.discarded.truncate(discarded_mark);
            }
        }
        result
    }

    fn peek(&mut self) -> Option<&Token> {
        if self.lookahead.is_none() {
            self.lookahead = self.lexer.find(|token| token.kind != TokenKind::Comment);
        }
        self.lookahead.as_ref()
    }

    fn take(&mut self) -> Option<Token> {
        self.peek();
        self.lookahead.take()
    }

    fn error(&self, kind: ErrorKind, span: Range<usize>) -> ParseError {
        ParseError {
            kind,
            span,
            related: None,
        }
    }

    fn add(&mut self, kind: NodeKind, span: Range<usize>) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node { kind, span });
        id
    }

    fn skip_discarded(&mut self, depth: usize, in_collection: bool) -> Result<(), ParseError> {
        while self.peek().is_some_and(|t| t.kind == TokenKind::Discard) {
            let prefix = self.take().unwrap();
            if depth >= self.nesting_limit {
                return Err(self.error(ErrorKind::NestingLimit, prefix.span));
            }
            let mark = self.nodes.len();
            let form = self.required(depth + 1, in_collection, &prefix.span)?;
            // Discard the entire syntax subtree, keeping capacity for later nodes.
            if self.retain_discards {
                self.discarded.push(form);
            } else {
                self.nodes.truncate(mark);
            }
        }
        Ok(())
    }

    fn required(
        &mut self,
        depth: usize,
        in_collection: bool,
        prefix: &Range<usize>,
    ) -> Result<NodeId, ParseError> {
        self.read_one(depth, in_collection).map_err(|mut error| {
            if error.related.is_none() {
                error.related = Some(prefix.clone());
            }
            error
        })
    }

    fn read_one(&mut self, depth: usize, in_collection: bool) -> Result<NodeId, ParseError> {
        self.skip_discarded(depth, in_collection)?;
        let token = self.take().ok_or_else(|| {
            self.error(
                ErrorKind::UnexpectedEof("a form"),
                self.source.len()..self.source.len(),
            )
        })?;
        if depth >= self.nesting_limit {
            return Err(self.error(ErrorKind::NestingLimit, token.span));
        }
        use TokenKind as T;
        let start = token.span.start;
        let mut end = token.span.end;
        let child_depth = depth + 1;
        let kind = match token.kind {
            T::Error(error) => return Err(self.error(ErrorKind::Lexical(error), token.span)),
            T::CloseParen | T::CloseBracket | T::CloseBrace => {
                return Err(self.error(
                    ErrorKind::UnexpectedDelimiter(close_char(token.kind).unwrap()),
                    token.span,
                ));
            }
            T::OpenParen | T::OpenBracket | T::OpenBrace | T::SetStart | T::AnonymousFnStart => {
                return self.collection(token, depth);
            }
            T::Atom if self.in_anonymous_fn && self.source[start..end].starts_with('%') => {
                return self.argument(token, depth, in_collection);
            }
            T::Atom => literals::atom(&self.source[token.span.clone()])
                .map_err(|kind| self.error(kind, token.span.clone()))?,
            T::String => NodeKind::String(
                literals::string(self.source, start + 1..end - 1)
                    .map_err(|span| self.error(ErrorKind::InvalidEscape, span))?,
            ),
            T::Regex => NodeKind::Regex,
            T::Character => NodeKind::Character(
                literals::character(&self.source[start + 1..end])
                    .ok_or_else(|| self.error(ErrorKind::InvalidCharacter, token.span.clone()))?,
            ),
            T::Quote
            | T::SyntaxQuote
            | T::Unquote
            | T::UnquoteSplicing
            | T::Deref
            | T::VarQuote => {
                let kind = match token.kind {
                    T::Quote => PrefixKind::Quote,
                    T::SyntaxQuote => PrefixKind::SyntaxQuote,
                    T::Unquote => PrefixKind::Unquote,
                    T::UnquoteSplicing => PrefixKind::UnquoteSplicing,
                    T::Deref => PrefixKind::Deref,
                    _ => PrefixKind::VarQuote,
                };
                let form = self.required(child_depth, in_collection, &token.span)?;
                end = self.nodes[form.0].span.end;
                NodeKind::Prefix { kind, form }
            }
            T::Metadata => {
                let metadata = self.required(child_depth, in_collection, &token.span)?;
                if !matches!(
                    self.nodes[metadata.0].kind,
                    NodeKind::Keyword { .. }
                        | NodeKind::Symbol
                        | NodeKind::String(_)
                        | NodeKind::Collection {
                            kind: CollectionKind::Map,
                            ..
                        }
                        | NodeKind::NamespacedMap { .. }
                ) {
                    return Err(self.error(
                        ErrorKind::InvalidMetadata,
                        self.nodes[metadata.0].span.clone(),
                    ));
                }
                let target = self.required(child_depth, in_collection, &token.span)?;
                end = self.nodes[target.0].span.end;
                NodeKind::Metadata { metadata, target }
            }
            T::Dispatch => {
                let tag = self.required(child_depth, in_collection, &token.span)?;
                if self.nodes[tag.0].kind != NodeKind::Symbol {
                    return Err(self.error(ErrorKind::InvalidTag, self.nodes[tag.0].span.clone()));
                }
                let form = self.required(child_depth, in_collection, &token.span)?;
                end = self.nodes[form.0].span.end;
                NodeKind::Tagged { tag, form }
            }
            T::ReaderConditional | T::ReaderConditionalSplicing => {
                let splicing = token.kind == T::ReaderConditionalSplicing;
                if splicing && !in_collection {
                    return Err(self.error(ErrorKind::InvalidConditional, token.span));
                }
                let branches = self.required(child_depth, in_collection, &token.span)?;
                let NodeKind::Collection {
                    kind: CollectionKind::List,
                    children,
                } = &self.nodes[branches.0].kind
                else {
                    return Err(self.error(
                        ErrorKind::InvalidConditional,
                        self.nodes[branches.0].span.clone(),
                    ));
                };
                if children.len() % 2 != 0
                    || children.iter().step_by(2).any(|id| {
                        !matches!(
                            self.nodes[id.0].kind,
                            NodeKind::Keyword {
                                auto_resolve: false
                            }
                        ) || matches!(
                            &self.source[self.nodes[id.0].span.clone()],
                            ":else" | ":none"
                        )
                    })
                {
                    return Err(self.error(
                        ErrorKind::InvalidConditional,
                        self.nodes[branches.0].span.clone(),
                    ));
                }
                end = self.nodes[branches.0].span.end;
                NodeKind::Conditional { splicing, branches }
            }
            T::NamespacedMap | T::AutoNamespacedMap => {
                let auto_resolve = token.kind == T::AutoNamespacedMap;
                let namespace = start + if auto_resolve { 3 } else { 2 }..end;
                let spelling = &self.source[namespace.clone()];
                if (!auto_resolve || !spelling.is_empty())
                    && (!literals::symbol(spelling)
                        || spelling.contains('/')
                        || spelling.starts_with(':'))
                {
                    return Err(self.error(ErrorKind::InvalidNamespace, token.span));
                }
                let map = self.required(child_depth, in_collection, &token.span)?;
                if !matches!(
                    self.nodes[map.0].kind,
                    NodeKind::Collection {
                        kind: CollectionKind::Map,
                        ..
                    }
                ) {
                    return Err(
                        self.error(ErrorKind::InvalidNamespace, self.nodes[map.0].span.clone())
                    );
                }
                end = self.nodes[map.0].span.end;
                NodeKind::NamespacedMap {
                    auto_resolve,
                    namespace,
                    map,
                }
            }
            T::SymbolicValue => {
                let mark = self.nodes.len();
                let value = self.required(child_depth, in_collection, &token.span)?;
                let node = &self.nodes[value.0];
                let spelling = &self.source[node.span.clone()];
                let value = match (&node.kind, spelling) {
                    (NodeKind::Symbol, "Inf") => SymbolicValue::Infinity,
                    (NodeKind::Symbol, "-Inf") => SymbolicValue::NegativeInfinity,
                    (NodeKind::Symbol, "NaN") => SymbolicValue::NaN,
                    _ => return Err(self.error(ErrorKind::InvalidSymbolicValue, node.span.clone())),
                };
                end = node.span.end;
                if !self.retain_discards {
                    self.nodes.truncate(mark);
                }
                NodeKind::SymbolicValue(value)
            }
            T::ReadEval => return Err(self.error(ErrorKind::ReadEvalDisabled, token.span)),
            T::Discard | T::Comment => unreachable!("trivia consumed before dispatch"),
        };
        Ok(self.add(kind, start..end))
    }

    fn collection(&mut self, token: Token, depth: usize) -> Result<NodeId, ParseError> {
        let (kind, expected) = match token.kind {
            TokenKind::OpenParen => (CollectionKind::List, ')'),
            TokenKind::OpenBracket => (CollectionKind::Vector, ']'),
            TokenKind::OpenBrace => (CollectionKind::Map, '}'),
            TokenKind::SetStart => (CollectionKind::Set, '}'),
            _ => (CollectionKind::AnonymousFn, ')'),
        };
        let previous_fn = self.in_anonymous_fn;
        if kind == CollectionKind::AnonymousFn {
            if previous_fn {
                return Err(self.error(ErrorKind::NestedAnonymousFunction, token.span));
            }
            self.in_anonymous_fn = true;
        }
        let mut children = Vec::new();
        loop {
            self.skip_discarded(depth + 1, true)?;
            let Some(next) = self.peek() else {
                let mut error = self.error(
                    ErrorKind::UnexpectedEof("a closing delimiter"),
                    self.source.len()..self.source.len(),
                );
                error.related = Some(token.span);
                return Err(error);
            };
            if let Some(found) = close_char(next.kind) {
                let close = self.take().unwrap();
                if found != expected {
                    return Err(ParseError {
                        kind: ErrorKind::MismatchedDelimiter { expected, found },
                        span: close.span,
                        related: Some(token.span),
                    });
                }
                let span = token.span.start..close.span.end;
                // Conditional forms can contribute zero or multiple children.
                // Validate arity after selection/expansion when one is present.
                if kind == CollectionKind::Map
                    && children.len() % 2 != 0
                    && !children.iter().any(|id: &NodeId| {
                        matches!(self.nodes[id.0].kind, NodeKind::Conditional { .. })
                    })
                {
                    return Err(self.error(ErrorKind::OddMap, span));
                }
                self.in_anonymous_fn = previous_fn;
                return Ok(self.add(NodeKind::Collection { kind, children }, span));
            }
            children.push(self.read_one(depth + 1, true)?);
        }
    }

    fn argument(
        &mut self,
        token: Token,
        depth: usize,
        in_collection: bool,
    ) -> Result<NodeId, ParseError> {
        let start = token.span.start;
        // The context-free lexer keeps %&rest / %1:kw together. Re-scan after
        // the % (or %&) so the numeric reader's boundaries apply inside #().
        self.lexer.cursor = start + 1;
        let (rest, index, end) = if token.span.end == start + 1 {
            (false, None, start + 1)
        } else if self.source.as_bytes()[start + 1] == b'&' {
            self.lexer.cursor += 1;
            (true, None, start + 2)
        } else {
            let index = self.required(depth + 1, in_collection, &token.span)?;
            if !matches!(
                self.nodes[index.0].kind,
                NodeKind::Number(NumberKind::Integer | NumberKind::BigInteger | NumberKind::Ratio)
            ) {
                return Err(self.error(
                    ErrorKind::InvalidArgument,
                    start..self.nodes[index.0].span.end,
                ));
            }
            (false, Some(index), self.nodes[index.0].span.end)
        };
        Ok(self.add(NodeKind::Argument { rest, index }, start..end))
    }
}

fn close_char(kind: TokenKind) -> Option<char> {
    match kind {
        TokenKind::CloseParen => Some(')'),
        TokenKind::CloseBracket => Some(']'),
        TokenKind::CloseBrace => Some('}'),
        _ => None,
    }
}

pub fn parse(source: &str) -> Result<SyntaxTree, ParseError> {
    let mut parser = Parser::new(source);
    let mut roots = Vec::new();
    while let Some(form) = parser.read_form()? {
        roots.push(form);
    }
    Ok(SyntaxTree {
        nodes: parser.into_nodes(),
        roots,
    })
}
