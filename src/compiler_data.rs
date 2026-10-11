//! Immutable compiler data. No evaluator, native heap, or executable unit owns
//! these values; cloning a form retains only its datum arena and source text.
use crate::parser::{CollectionKind, LineIndex};
use std::{
    cell::OnceCell,
    fmt,
    ops::Range,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

/// A name's exact UTF-16 components, without reader resolution or validation.
/// Names constructed by a host need not be valid source token spellings.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Name {
    namespace: Option<Rc<[u16]>>,
    name: Rc<[u16]>,
}

impl Name {
    pub fn new(namespace: Option<&str>, name: &str) -> Self {
        Self {
            namespace: namespace.map(|s| s.encode_utf16().collect()),
            name: name.encode_utf16().collect(),
        }
    }

    pub fn from_utf16(namespace: Option<&[u16]>, name: &[u16]) -> Self {
        Self {
            namespace: namespace.map(Rc::from),
            name: Rc::from(name),
        }
    }

    pub fn namespace(&self) -> Option<&[u16]> {
        self.namespace.as_deref()
    }

    pub fn name(&self) -> &[u16] {
        &self.name
    }
}

/// Compile-time representations for values owned by the JavaScript runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerLiteralKind {
    /// Exact reader integer identity until JavaScript Number emission.
    BigInteger,
    Regex,
    Instant,
    Uuid,
    JsArray,
    JsObject,
    Queue,
}

/// An index local to one immutable semantic arena, not a syntax NodeId.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FormId(pub usize);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormKind {
    Scalar,
    /// Map children alternate key/value in source order. They are never recovered
    /// from runtime map iteration, which has no evaluation-order contract.
    Collection {
        kind: CollectionKind,
        children: Box<[FormId]>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Origin {
    Source,
    SuppliedData,
    MacroExpansion(Rc<Name>),
    Quote,
    SyntaxQuote,
    Unquote,
    UnquoteSplicing,
    Deref,
    VarQuote,
    Metadata,
    AnonymousFunction,
    AnonymousArgument,
    DataReader(Rc<Name>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormNode {
    pub kind: FormKind,
    pub metadata: Option<FormId>,
    pub(crate) compiler_payload: Option<FormId>,
    pub origin: Origin,
}

/// Compact source identity independent of every interpreter owner/generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SourceId(u64);
static NEXT_SOURCE: AtomicU64 = AtomicU64::new(1);
struct SourceFile {
    id: SourceId,
    name: String,
    text: String,
    lines: OnceCell<LineIndex>,
    line_offset: usize,
}
/// Source text retained directly by compiler forms and diagnostics.
#[derive(Clone)]
pub struct Source(Rc<SourceFile>);
impl Source {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self::with_line_offset(name, text, 0)
    }
    pub(crate) fn with_line_offset(
        name: impl Into<String>,
        text: impl Into<String>,
        line_offset: usize,
    ) -> Self {
        Self(Rc::new(SourceFile {
            id: SourceId(NEXT_SOURCE.fetch_add(1, Ordering::Relaxed)),
            name: name.into(),
            text: text.into(),
            lines: OnceCell::new(),
            line_offset,
        }))
    }
    pub fn id(&self) -> SourceId {
        self.0.id
    }
    pub fn name(&self) -> &str {
        &self.0.name
    }
    pub fn text(&self) -> &str {
        &self.0.text
    }
    pub fn line_column(&self, offset: usize) -> Option<(usize, usize)> {
        self.0
            .lines
            .get_or_init(|| LineIndex::new(self.text()))
            .line_column(offset)
            .and_then(|(line, column)| Some((line.checked_add(self.0.line_offset)?, column)))
    }
    pub fn span(&self, bytes: Range<usize>) -> Option<SourceSpan> {
        self.text().get(bytes.clone())?;
        Some(SourceSpan {
            source: self.clone(),
            bytes,
        })
    }
}
impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Source")
            .field("id", &self.id())
            .field("name", &self.name())
            .finish()
    }
}
#[derive(Clone, Debug)]
pub struct SourceSpan {
    source: Source,
    bytes: Range<usize>,
}
impl SourceSpan {
    pub fn source(&self) -> &Source {
        &self.source
    }
    pub fn bytes(&self) -> Range<usize> {
        self.bytes.clone()
    }
    pub fn text(&self) -> &str {
        &self.source.text()[self.bytes.clone()]
    }
}

/// Reader scalar distinctions, not a numeric evaluator. Emission converts all
/// numeric kinds to JavaScript Number. UTF-16 is never converted through UTF-8.
#[derive(Clone, Debug)]
pub enum DatumKind {
    Nil,
    Boolean(bool),
    Integer(i64),
    Double(f64),
    Ratio {
        numerator: i64,
        denominator: i64,
    },
    Character(u16),
    String(Rc<[u16]>),
    Symbol(Name),
    Keyword(Name),
    Collection {
        kind: CollectionKind,
        children: Box<[Datum]>,
    },
    CompilerLiteral {
        kind: CompilerLiteralKind,
        payload: Datum,
    },
}
struct DatumNode {
    kind: DatumKind,
    metadata: Option<Datum>,
}
/// Owned finite readable data. Shared subtrees have identity but no runtime roots.
#[derive(Clone)]
pub struct Datum(Rc<DatumNode>);
impl fmt::Debug for Datum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind().fmt(f)
    }
}
/// Shallow inspection of compiler data, independently of native ValueView.
#[derive(Clone, Debug)]
pub enum DatumView {
    Nil,
    Boolean(bool),
    Integer(i64),
    Double(f64),
    Ratio { numerator: i64, denominator: i64 },
    Character(u16),
    String(Rc<[u16]>),
    Symbol(Name),
    Keyword(Name),
    List(usize),
    Vector(usize),
    Map(usize),
    Set(usize),
    CompilerLiteral(CompilerLiteralKind),
}
impl Datum {
    pub fn new(kind: DatumKind) -> Self {
        Self(Rc::new(DatumNode {
            kind,
            metadata: None,
        }))
    }
    pub fn with_metadata(&self, metadata: Datum) -> Self {
        Self(Rc::new(DatumNode {
            kind: self.kind().clone(),
            metadata: Some(metadata),
        }))
    }
    pub fn kind(&self) -> &DatumKind {
        &self.0.kind
    }
    pub fn metadata(&self) -> Option<Datum> {
        self.0.metadata.clone()
    }
    pub(crate) fn identity(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    pub fn compiler_literal_data(&self) -> Option<Datum> {
        match self.kind() {
            DatumKind::CompilerLiteral { payload, .. } => Some(payload.clone()),
            _ => None,
        }
    }
    pub fn inspect(&self) -> DatumView {
        match self.kind() {
            DatumKind::Nil => DatumView::Nil,
            DatumKind::Boolean(v) => DatumView::Boolean(*v),
            DatumKind::Integer(v) => DatumView::Integer(*v),
            DatumKind::Double(v) => DatumView::Double(*v),
            DatumKind::Ratio {
                numerator,
                denominator,
            } => DatumView::Ratio {
                numerator: *numerator,
                denominator: *denominator,
            },
            DatumKind::Character(v) => DatumView::Character(*v),
            DatumKind::String(v) => DatumView::String(v.clone()),
            DatumKind::Symbol(v) => DatumView::Symbol(v.clone()),
            DatumKind::Keyword(v) => DatumView::Keyword(v.clone()),
            DatumKind::CompilerLiteral { kind, .. } => DatumView::CompilerLiteral(*kind),
            DatumKind::Collection { kind, children } => match kind {
                CollectionKind::List | CollectionKind::AnonymousFn => {
                    DatumView::List(children.len())
                }
                CollectionKind::Vector => DatumView::Vector(children.len()),
                CollectionKind::Map => DatumView::Map(children.len() / 2),
                CollectionKind::Set => DatumView::Set(children.len()),
            },
        }
    }
}

struct Forms {
    nodes: Box<[FormNode]>,
    data: Box<[Datum]>,
    spans: Box<[SourceSpan]>,
}
/// Immutable semantic arena, retaining owned compiler data and source provenance.
/// Children are source ordered, including alternating map keys/values. Datum
/// storage order must never determine executable map evaluation order.
#[derive(Clone)]
pub struct Form {
    forms: Rc<Forms>,
    id: FormId,
}
impl Form {
    pub(crate) fn from_parts(
        nodes: Vec<FormNode>,
        data: Vec<Datum>,
        spans: Vec<SourceSpan>,
        id: FormId,
    ) -> Self {
        assert_eq!(nodes.len(), data.len());
        assert_eq!(nodes.len(), spans.len());
        Self {
            forms: Rc::new(Forms {
                nodes: nodes.into(),
                data: data.into(),
                spans: spans.into(),
            }),
            id,
        }
    }
    /// Construct a form directly from owned data, without a host or source reader.
    pub fn from_data(data: Datum, span: SourceSpan, origin: Origin) -> Self {
        fn add(
            data: Datum,
            span: &SourceSpan,
            origin: &Origin,
            nodes: &mut Vec<FormNode>,
            values: &mut Vec<Datum>,
            spans: &mut Vec<SourceSpan>,
        ) -> FormId {
            let kind = match data.kind() {
                DatumKind::Collection { kind, children } => FormKind::Collection {
                    kind: *kind,
                    children: children
                        .iter()
                        .map(|child| add(child.clone(), span, origin, nodes, values, spans))
                        .collect(),
                },
                _ => FormKind::Scalar,
            };
            let metadata = data
                .metadata()
                .map(|m| add(m, span, origin, nodes, values, spans));
            let compiler_payload = data
                .compiler_literal_data()
                .map(|p| add(p, span, origin, nodes, values, spans));
            let id = FormId(nodes.len());
            nodes.push(FormNode {
                kind,
                metadata,
                compiler_payload,
                origin: origin.clone(),
            });
            values.push(data);
            spans.push(span.clone());
            id
        }
        let (mut nodes, mut values, mut spans) = (vec![], vec![], vec![]);
        let id = add(data, &span, &origin, &mut nodes, &mut values, &mut spans);
        Self::from_parts(nodes, values, spans, id)
    }
    pub fn id(&self) -> FormId {
        self.id
    }
    pub fn node(&self) -> &FormNode {
        &self.forms.nodes[self.id.0]
    }
    pub fn nodes(&self) -> &[FormNode] {
        &self.forms.nodes
    }
    pub fn at(&self, id: FormId) -> Option<Self> {
        self.forms.nodes.get(id.0).map(|_| Self {
            forms: self.forms.clone(),
            id,
        })
    }
    pub fn data(&self) -> Datum {
        self.forms.data[self.id.0].clone()
    }
    pub fn span(&self) -> SourceSpan {
        self.forms.spans[self.id.0].clone()
    }
    pub fn children(&self) -> Vec<Self> {
        match &self.node().kind {
            FormKind::Scalar => vec![],
            FormKind::Collection { children, .. } => {
                children.iter().map(|&id| self.at(id).unwrap()).collect()
            }
        }
    }
    pub fn metadata(&self) -> Option<Self> {
        self.node().metadata.and_then(|id| self.at(id))
    }
}
impl fmt::Debug for Form {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Form")
            .field("id", &self.id())
            .field("data", &self.data())
            .field("span", &self.span())
            .finish()
    }
}

/// Owned reader diagnostic: native exception/heap handles cannot escape through
/// this error during the staged reader migration.
#[derive(Clone, Debug)]
pub struct ReadError {
    pub message: String,
    pub span: SourceSpan,
    pub related: Option<SourceSpan>,
}
impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (line, column) = self
            .span
            .source()
            .line_column(self.span.bytes().start)
            .unwrap();
        write!(
            f,
            "{}:{line}:{column}: {}",
            self.span.source().name(),
            self.message
        )
    }
}
impl std::error::Error for ReadError {}

/// Source identity and spelling retained independently of runtime roots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticLocation {
    pub source: String,
    pub bytes: Range<usize>,
    pub line: usize,
    pub column: usize,
    pub text: String,
}
impl DiagnosticLocation {
    pub fn from_span(span: &SourceSpan) -> Self {
        let (line, column) = span.source().line_column(span.bytes().start).unwrap();
        Self {
            source: span.source().name().into(),
            bytes: span.bytes(),
            line,
            column,
            text: span.text().into(),
        }
    }
}
