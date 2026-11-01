//! Immutable compiler data. No evaluator, native heap, or executable unit owns
//! these values; cloning a form retains only its datum arena and source text.
use crate::parser::{CollectionKind, LineIndex};
use std::{
    fmt,
    ops::Range,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// A name's exact UTF-16 components, without reader resolution or validation.
/// Names constructed by a host need not be valid source token spellings.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Name {
    namespace: Option<Arc<[u16]>>,
    name: Arc<[u16]>,
}

// ASCII's exact-size byte iterator lets Arc allocate the final slice directly.
// EncodeUtf16's unknown exact length otherwise requires a temporary Vec.
fn name_utf16(text: &str) -> Arc<[u16]> {
    if text.is_ascii() {
        text.bytes().map(u16::from).collect()
    } else {
        text.encode_utf16().collect()
    }
}

impl Name {
    pub fn new(namespace: Option<&str>, name: &str) -> Self {
        Self {
            namespace: namespace.map(name_utf16),
            name: name_utf16(name),
        }
    }

    pub fn from_utf16(namespace: Option<&[u16]>, name: &[u16]) -> Self {
        Self {
            namespace: namespace.map(Arc::from),
            name: Arc::from(name),
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
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
    MacroExpansion(Arc<Name>),
    Quote,
    SyntaxQuote,
    Unquote,
    UnquoteSplicing,
    Deref,
    VarQuote,
    Metadata,
    AnonymousFunction,
    AnonymousArgument,
    DataReader(Arc<Name>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormNode {
    pub kind: FormKind,
    pub metadata: Option<FormId>,
    pub(crate) compiler_payload: Option<FormId>,
    pub origin: Origin,
}

/// Compact source identity independent of evaluator owner generations.
/// A process identity, which namespace artifacts never retain
/// ([`crate::generated_names`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SourceId(u64);
static NEXT_SOURCE: AtomicU64 = AtomicU64::new(1);
struct SourceFile {
    id: SourceId,
    name: String,
    text: String,
    lines: OnceLock<LineIndex>,
    line_offset: usize,
}
/// Source text retained directly by compiler forms and diagnostics.
#[derive(Clone)]
pub struct Source(Arc<SourceFile>);
impl Source {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self::with_line_offset(name, text, 0)
    }
    pub(crate) fn with_line_offset(
        name: impl Into<String>,
        text: impl Into<String>,
        line_offset: usize,
    ) -> Self {
        Self(Arc::new(SourceFile {
            id: SourceId(NEXT_SOURCE.fetch_add(1, Ordering::Relaxed)),
            name: name.into(),
            text: text.into(),
            lines: OnceLock::new(),
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
        self.line_index()
            .line_column(offset)
            .and_then(|(line, column)| Some((line.checked_add(self.0.line_offset)?, column)))
    }
    pub(crate) fn line_index(&self) -> &LineIndex {
        self.0.lines.get_or_init(|| LineIndex::new(self.text()))
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
    fn view(&self) -> SourceSpanView<'_> {
        SourceSpanView { source: &self.source, bytes: &self.bytes }
    }
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

/// Borrow the source and range without creating another source owner.
#[derive(Clone, Copy)]
pub(crate) struct SourceSpanView<'a> {
    source: &'a Source,
    bytes: &'a Range<usize>,
}

impl<'a> SourceSpanView<'a> {
    pub(crate) fn source(self) -> &'a Source {
        self.source
    }
    pub(crate) fn bytes(self) -> Range<usize> {
        self.bytes.clone()
    }
    fn to_span(self) -> SourceSpan {
        SourceSpan { source: self.source.clone(), bytes: self.bytes() }
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
    String(Arc<[u16]>),
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
pub struct Datum(Arc<DatumNode>);
impl fmt::Debug for Datum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind().fmt(f)
    }
}
/// Shallow inspection of immutable compiler data.
#[derive(Clone, Debug, PartialEq)]
pub enum DatumView {
    Nil,
    Boolean(bool),
    Integer(i64),
    Double(f64),
    Ratio { numerator: i64, denominator: i64 },
    Character(u16),
    String(Arc<[u16]>),
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
        Self(Arc::new(DatumNode {
            kind,
            metadata: None,
        }))
    }
    pub fn with_metadata(&self, metadata: Datum) -> Self {
        Self(Arc::new(DatumNode {
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
        Arc::as_ptr(&self.0) as usize
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

/// Owned provenance for one executed macro expansion. It survives heap teardown.
#[derive(Clone, Debug)]
pub struct ExpansionOrigin {
    pub macro_name: Name,
    pub call: SourceSpan,
    pub definition: Option<SourceSpan>,
    pub helpers: Vec<Name>,
}

/// Most generated arenas use one source file. Retain it once, while mixed-source
/// arenas keep independently owned spans. Neither representation retains input
/// form arenas, and public spans acquire their own source ownership on demand.
enum FormSpans {
    SingleSource { source: Source, ranges: Box<[Range<usize>]> },
    Individual(Box<[SourceSpan]>),
}

impl FormSpans {
    fn len(&self) -> usize {
        match self {
            Self::SingleSource { ranges, .. } => ranges.len(),
            Self::Individual(spans) => spans.len(),
        }
    }
    fn view(&self, id: usize) -> SourceSpanView<'_> {
        match self {
            Self::SingleSource { source, ranges } => SourceSpanView { source, bytes: &ranges[id] },
            Self::Individual(spans) => spans[id].view(),
        }
    }
}

#[derive(Default)]
enum FormSpanBuilder {
    #[default]
    Empty,
    SingleSource { source: Source, ranges: Vec<Range<usize>> },
    Individual(Vec<SourceSpan>),
}

impl FormSpanBuilder {
    fn push(&mut self, span: SourceSpanView<'_>) {
        match self {
            Self::Empty => {
                *self = Self::SingleSource {
                    source: span.source().clone(), ranges: vec![span.bytes()],
                };
            }
            Self::SingleSource { source, ranges }
                if Arc::ptr_eq(&source.0, &span.source().0) => ranges.push(span.bytes()),
            Self::SingleSource { .. } => {
                let Self::SingleSource { source, ranges } = std::mem::take(self) else {
                    unreachable!()
                };
                let mut spans: Vec<_> = ranges.into_iter()
                    .map(|bytes| SourceSpan { source: source.clone(), bytes }).collect();
                spans.push(span.to_span());
                *self = Self::Individual(spans);
            }
            Self::Individual(spans) => spans.push(span.to_span()),
        }
    }
    fn finish(self) -> FormSpans {
        match self {
            Self::Empty => FormSpans::Individual(Box::default()),
            Self::SingleSource { source, ranges } => FormSpans::SingleSource { source, ranges: ranges.into() },
            Self::Individual(spans) => FormSpans::Individual(spans.into()),
        }
    }
}

struct Forms {
    nodes: Box<[FormNode]>,
    data: Box<[Datum]>,
    spans: FormSpans,
    expansions: Vec<ExpansionOrigin>,
}
/// Immutable semantic arena, retaining owned compiler data and source provenance.
/// Children are source ordered, including alternating map keys/values. Datum
/// storage order must never determine executable map evaluation order.
#[derive(Clone)]
pub struct Form {
    forms: Arc<Forms>,
    id: FormId,
    // Native values retain exact occurrences. Only compiler import uses the
    // legacy scalar-position view, without replacing a Datum or its span.
    #[cfg(feature = "bbp-macros")]
    compiler_spans: Option<Arc<std::collections::HashMap<FormId, SourceSpan>>>,
}

/// An occurrence in a borrowed semantic arena. Recursive compiler reads do
/// not need an owned Form (or a temporary vector of owned child forms).
#[derive(Clone, Copy)]
pub(crate) struct FormView<'a> {
    owner: &'a Form,
    id: FormId,
}

impl<'a> FormView<'a> {
    pub(crate) fn at(self, id: FormId) -> Option<Self> {
        self.owner.forms.nodes.get(id.0).map(|_| Self { id, ..self })
    }
    pub(crate) fn node(self) -> &'a FormNode {
        &self.owner.forms.nodes[self.id.0]
    }
    pub(crate) fn data(self) -> &'a Datum {
        &self.owner.forms.data[self.id.0]
    }
    pub(crate) fn span(self) -> SourceSpanView<'a> {
        self.owner.forms.spans.view(self.id.0)
    }
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn compiler_span(self) -> SourceSpanView<'a> {
        self.compiler_span_override().map(SourceSpan::view).unwrap_or_else(|| self.span())
    }
    #[cfg(feature = "bbp-macros")]
    fn compiler_span_override(self) -> Option<&'a SourceSpan> {
        self.owner.compiler_spans.as_ref()
            .and_then(|spans| spans.get(&self.id))
    }
    pub(crate) fn children(self) -> impl ExactSizeIterator<Item = Self> + DoubleEndedIterator + 'a {
        let ids: &'a [FormId] = match &self.node().kind {
            FormKind::Scalar => &[],
            FormKind::Collection { children, .. } => children,
        };
        ids.iter().map(move |&id| Self { id, ..self })
    }
    /// Retain the occurrence, including metadata, provenance and compiler
    /// span overrides, only when the caller stores it beyond this traversal.
    pub(crate) fn to_form(self) -> Form {
        self.owner.at(self.id).expect("form occurrence")
    }
}

impl Form {
    pub(crate) fn view(&self) -> FormView<'_> {
        FormView { owner: self, id: self.id }
    }
    /// Leaf provenance must not pin the surrounding syntax arena: interned
    /// keywords and last-seen scalar values can outlive their input collection.
    pub(crate) fn compact_leaf(&self) -> Self {
        let node = self.node();
        if !matches!(node.kind, FormKind::Scalar)
            || node.metadata.is_some()
            || node.compiler_payload.is_some()
        {
            return self.clone();
        }
        let mut leaf = Self::from_parts(
            vec![node.clone()],
            vec![self.data()],
            vec![self.span()],
            FormId(0),
        );
        Arc::get_mut(&mut leaf.forms).unwrap().expansions = self.expansions().to_vec();
        #[cfg(feature = "bbp-macros")]
        if let Some(span) = self.compiler_spans.as_ref().and_then(|s| s.get(&self.id)) {
            leaf.compiler_spans = Some(Arc::new(std::collections::HashMap::from([(
                FormId(0),
                span.clone(),
            )])));
        }
        leaf
    }
    pub(crate) fn from_parts(
        nodes: Vec<FormNode>,
        data: Vec<Datum>,
        spans: Vec<SourceSpan>,
        id: FormId,
    ) -> Self {
        Self::from_parts_with_spans(nodes, data, FormSpans::Individual(spans.into()), id)
    }
    fn from_parts_with_spans(
        nodes: Vec<FormNode>,
        data: Vec<Datum>,
        spans: FormSpans,
        id: FormId,
    ) -> Self {
        assert_eq!(nodes.len(), data.len());
        assert_eq!(nodes.len(), spans.len());
        Self {
            forms: Arc::new(Forms {
                nodes: nodes.into(),
                data: data.into(),
                spans,
                expansions: vec![],
            }),
            id,
            #[cfg(feature = "bbp-macros")]
            compiler_spans: None,
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
            spans: &mut Vec<Range<usize>>,
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
            spans.push(span.bytes());
            id
        }
        let (mut nodes, mut values, mut spans) = (vec![], vec![], vec![]);
        let id = add(data, &span, &origin, &mut nodes, &mut values, &mut spans);
        Self::from_parts_with_spans(nodes, values,
            FormSpans::SingleSource { source: span.source, ranges: spans.into() }, id)
    }
    pub fn expansions(&self) -> &[ExpansionOrigin] {
        &self.forms.expansions
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
            #[cfg(feature = "bbp-macros")]
            compiler_spans: self.compiler_spans.clone(),
        })
    }
    pub fn data(&self) -> Datum {
        self.forms.data[self.id.0].clone()
    }
    pub fn span(&self) -> SourceSpan {
        self.forms.spans.view(self.id.0).to_span()
    }
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn compiler_span(&self) -> SourceSpan {
        self.compiler_span_override().unwrap_or_else(|| self.span())
    }
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn compiler_span_override(&self) -> Option<SourceSpan> {
        self.compiler_spans
            .as_ref()
            .and_then(|s| s.get(&self.id))
            .cloned()
    }
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn has_compiler_spans(&self) -> bool {
        self.compiler_spans.is_some()
    }
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn with_compiler_spans(
        mut self,
        spans: std::collections::HashMap<FormId, SourceSpan>,
    ) -> Self {
        self.compiler_spans = (!spans.is_empty()).then(|| Arc::new(spans));
        self
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

/// Pure form construction used by the reader and compiler bootstrap transforms.
/// These operations do not execute language code or allocate evaluator roots.
#[derive(Clone, Copy, Default)]
pub(crate) struct DataBuilder;
impl DataBuilder {
    pub(crate) fn nil(&self) -> Datum {
        Datum::new(DatumKind::Nil)
    }
    pub(crate) fn boolean(&self, v: bool) -> Datum {
        Datum::new(DatumKind::Boolean(v))
    }
    pub(crate) fn integer(&self, v: i64) -> Datum {
        Datum::new(DatumKind::Integer(v))
    }
    pub(crate) fn double(&self, v: f64) -> Datum {
        Datum::new(DatumKind::Double(v))
    }
    pub(crate) fn character(&self, v: u16) -> Datum {
        Datum::new(DatumKind::Character(v))
    }
    pub(crate) fn string(&self, v: impl AsRef<[u16]>) -> Datum {
        Datum::new(DatumKind::String(Arc::from(v.as_ref())))
    }
    pub(crate) fn symbol(&self, v: Name) -> Datum {
        Datum::new(DatumKind::Symbol(v))
    }
    pub(crate) fn keyword(&self, v: Name) -> Datum {
        Datum::new(DatumKind::Keyword(v))
    }
    pub(crate) fn compiler_literal(&self, kind: CompilerLiteralKind, payload: &Datum) -> Datum {
        Datum::new(DatumKind::CompilerLiteral {
            kind,
            payload: payload.clone(),
        })
    }
    fn collection(&self, kind: CollectionKind, values: &[Datum]) -> Result<Datum, String> {
        Ok(Datum::new(DatumKind::Collection {
            kind,
            children: values.into(),
        }))
    }
    pub(crate) fn list(&self, v: &[Datum]) -> Result<Datum, String> {
        self.collection(CollectionKind::List, v)
    }
    pub(crate) fn vector(&self, v: &[Datum]) -> Result<Datum, String> {
        self.collection(CollectionKind::Vector, v)
    }
    pub(crate) fn set(&self, v: &[Datum]) -> Result<Datum, String> {
        self.collection(CollectionKind::Set, v)
    }
    pub(crate) fn map(&self, v: &[(Datum, Datum)]) -> Result<Datum, String> {
        self.collection(
            CollectionKind::Map,
            &v.iter()
                .flat_map(|(k, v)| [k.clone(), v.clone()])
                .collect::<Vec<_>>(),
        )
    }
    pub(crate) fn with_meta(&self, v: &Datum, m: &Datum) -> Result<Datum, String> {
        if !matches!(
            v.kind(),
            DatumKind::Symbol(_) | DatumKind::Collection { .. }
        ) {
            return Err("invalid metadata target".into());
        }
        Ok(v.with_metadata(m.clone()))
    }
    pub(crate) fn assoc(&self, v: &Datum, k: &Datum, value: &Datum) -> Result<Datum, String> {
        let mut entries = v.entries()?;
        if let Some((_, v)) = entries.iter_mut().find(|(key, _)| key.value_eq(k)) {
            *v = value.clone()
        } else {
            entries.push((k.clone(), value.clone()))
        }
        let result = self.map(&entries)?;
        Ok(match v.metadata() {
            Some(meta) => result.with_metadata(meta),
            None => result,
        })
    }
}
impl Datum {
    pub(crate) fn elements(&self) -> Result<Vec<Datum>, String> {
        match self.kind() {
            DatumKind::Collection {
                kind: CollectionKind::List | CollectionKind::Vector | CollectionKind::Set,
                children,
            } => Ok(children.to_vec()),
            _ => Err("expected collection".into()),
        }
    }
    pub(crate) fn entries(&self) -> Result<Vec<(Datum, Datum)>, String> {
        match self.kind() {
            DatumKind::Collection {
                kind: CollectionKind::Map,
                children,
            } => Ok(children
                .as_chunks::<2>()
                .0
                .iter()
                .map(|[k, v]| (k.clone(), v.clone()))
                .collect()),
            _ => Err("expected map".into()),
        }
    }
    pub(crate) fn get(&self, key: &Datum) -> Result<Option<Datum>, String> {
        Ok(self
            .entries()?
            .into_iter()
            .find(|(k, _)| k.value_eq(key))
            .map(|(_, v)| v))
    }
    pub(crate) fn value_eq(&self, other: &Self) -> bool {
        use DatumKind as D;
        match (self.kind(), other.kind()) {
            (D::Nil, D::Nil) => true,
            (D::Boolean(a), D::Boolean(b)) => a == b,
            (D::Integer(a), D::Integer(b)) => a == b,
            (D::Double(a), D::Double(b)) => a == b,
            (D::Integer(a), D::Double(b)) | (D::Double(b), D::Integer(a)) => *a as f64 == *b,
            (
                D::Ratio {
                    numerator: a,
                    denominator: b,
                },
                D::Ratio {
                    numerator: c,
                    denominator: d,
                },
            ) => i128::from(*a) * i128::from(*d) == i128::from(*c) * i128::from(*b),
            (D::Character(a), D::Character(b)) => a == b,
            (D::String(a), D::String(b)) => a == b,
            (D::Symbol(a), D::Symbol(b)) | (D::Keyword(a), D::Keyword(b)) => a == b,
            (
                D::CompilerLiteral {
                    kind: a,
                    payload: b,
                },
                D::CompilerLiteral {
                    kind: c,
                    payload: d,
                },
            ) => a == c && b.value_eq(d),
            (
                D::Collection {
                    kind: a,
                    children: b,
                },
                D::Collection {
                    kind: c,
                    children: d,
                },
            ) => {
                if b.len() != d.len() {
                    return false;
                }
                match (a, c) {
                    (CollectionKind::Map, CollectionKind::Map) => {
                        b.as_chunks::<2>().0.iter().all(|[k, v]| {
                            d.as_chunks::<2>()
                                .0
                                .iter()
                                .any(|[x, y]| k.value_eq(x) && v.value_eq(y))
                        })
                    }
                    (CollectionKind::Set, CollectionKind::Set) => {
                        b.iter().all(|v| d.iter().any(|x| v.value_eq(x)))
                    }
                    (
                        CollectionKind::List | CollectionKind::Vector,
                        CollectionKind::List | CollectionKind::Vector,
                    ) => b.iter().zip(d.iter()).all(|(a, b)| a.value_eq(b)),
                    _ => false,
                }
            }
            _ => false,
        }
    }
    pub(crate) fn value_hash(&self) -> u64 {
        use DatumKind as D;
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match self.kind() {
            D::Nil => 0u8.hash(&mut h),
            D::Boolean(v) => {
                1u8.hash(&mut h);
                v.hash(&mut h)
            }
            D::Integer(v) => {
                2u8.hash(&mut h);
                (*v as f64).to_bits().hash(&mut h)
            }
            D::Double(v) => {
                2u8.hash(&mut h);
                (if *v == 0.0 { 0.0 } else { *v }).to_bits().hash(&mut h)
            }
            D::Ratio {
                numerator,
                denominator,
            } => {
                2u8.hash(&mut h);
                (*numerator as f64 / *denominator as f64)
                    .to_bits()
                    .hash(&mut h)
            }
            D::Character(v) => {
                3u8.hash(&mut h);
                v.hash(&mut h)
            }
            D::String(v) => {
                4u8.hash(&mut h);
                v.hash(&mut h)
            }
            D::Symbol(v) => {
                5u8.hash(&mut h);
                v.hash(&mut h)
            }
            D::Keyword(v) => {
                6u8.hash(&mut h);
                v.hash(&mut h)
            }
            D::CompilerLiteral { kind, payload } => {
                7u8.hash(&mut h);
                kind.hash(&mut h);
                payload.value_hash().hash(&mut h)
            }
            D::Collection { kind, children } => match kind {
                CollectionKind::Map => {
                    8u8.hash(&mut h);
                    children
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .fold(0, |acc, [k, v]| {
                            acc ^ (k.value_hash().wrapping_mul(31) ^ v.value_hash())
                        })
                        .hash(&mut h)
                }
                CollectionKind::Set => {
                    9u8.hash(&mut h);
                    children
                        .iter()
                        .fold(0, |acc, c| acc ^ c.value_hash())
                        .hash(&mut h)
                }
                _ => {
                    10u8.hash(&mut h);
                    for child in children {
                        child.value_hash().hash(&mut h)
                    }
                }
            },
        }
        h.finish()
    }
}

// Only live allocation identities enter this temporary map, never user names
// or language equality keys. The input Form keeps all allocations alive until
// lookup finishes. Mix aligned addresses into both bucket bits and control tags
// with the same scheme as the native bridge's identity tables.
#[derive(Default)]
struct ProvenanceIdentityHasher(u64);

impl std::hash::Hasher for ProvenanceIdentityHasher {
    fn finish(&self) -> u64 {
        self.0 ^ (self.0 >> 32)
    }
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }
    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }
}

type DatumOccurrences = std::collections::HashMap<
    usize,
    FormId,
    std::hash::BuildHasherDefault<ProvenanceIdentityHasher>,
>;

impl Form {
    /// A pure generated form keeps every reused input datum's exact provenance.
    pub(crate) fn from_expansion(data: Datum, original: &Form, name: Name) -> Self {
        fn collect(f: FormView<'_>, originals: &mut DatumOccurrences) {
            originals.insert(f.data().identity(), f.id);
            for child in f.children() {
                collect(child, originals);
            }
            if let Some(meta) = f.node().metadata.and_then(|id| f.at(id)) {
                collect(meta, originals);
            }
            if let Some(payload) = f.node().compiler_payload.and_then(|id| f.at(id)) {
                collect(payload, originals);
            }
        }
        // Every entry belongs to this borrowed arena. Keep only occurrence IDs;
        // temporary owned handles would clone its Arcs at every visited edge.
        let mut originals = DatumOccurrences::default();
        collect(original.view(), &mut originals);
        Self::from_expansion_lookup(
            data,
            original,
            ExpansionOrigin {
                macro_name: name,
                call: original.span(),
                definition: None,
                helpers: vec![],
            },
            |identity| originals.get(&identity).map(|&id| FormView { owner: original, id }),
        )
    }
    pub(crate) fn from_bridge_expansion<S: std::hash::BuildHasher>(
        data: Datum,
        original: &Form,
        expansion: ExpansionOrigin,
        originals: &std::collections::HashMap<usize, Form, S>,
    ) -> Self {
        Self::from_expansion_lookup(data, original, expansion, |identity| {
            originals.get(&identity).map(Form::view)
        })
    }

    fn from_expansion_lookup<'a>(
        data: Datum,
        original: &Form,
        expansion: ExpansionOrigin,
        original_for: impl Fn(usize) -> Option<FormView<'a>>,
    ) -> Self {
        fn add<'a>(
            data: Datum,
            original: &Form,
            original_for: &impl Fn(usize) -> Option<FormView<'a>>,
            name: &Name,
            nodes: &mut Vec<FormNode>,
            values: &mut Vec<Datum>,
            spans: &mut FormSpanBuilder,
        ) -> FormId {
            let reused = original_for(data.identity());
            let span = if let Some(form) = reused { form.span() } else { original.view().span() };
            let origin = reused
                .map(|f| f.node().origin.clone())
                .unwrap_or_else(|| Origin::MacroExpansion(Arc::new(name.clone())));
            let kind = match data.kind() {
                DatumKind::Collection { kind, children } => FormKind::Collection {
                    kind: *kind,
                    children: children
                        .iter()
                        .map(|d| add(d.clone(), original, original_for, name, nodes, values, spans))
                        .collect(),
                },
                _ => FormKind::Scalar,
            };
            let metadata = data
                .metadata()
                .map(|d| add(d, original, original_for, name, nodes, values, spans));
            let compiler_payload = data
                .compiler_literal_data()
                .map(|d| add(d, original, original_for, name, nodes, values, spans));
            let id = FormId(nodes.len());
            nodes.push(FormNode {
                kind,
                metadata,
                compiler_payload,
                origin,
            });
            values.push(data);
            spans.push(span);
            id
        }
        let (mut nodes, mut values, mut spans) = (vec![], vec![], FormSpanBuilder::default());
        let id = add(
            data,
            original,
            &original_for,
            &expansion.macro_name,
            &mut nodes,
            &mut values,
            &mut spans,
        );
        let mut result = Self::from_parts_with_spans(nodes, values, spans.finish(), id);
        let forms = Arc::get_mut(&mut result.forms).unwrap();
        forms.expansions = original.expansions().to_vec();
        forms.expansions.push(expansion);
        #[cfg(feature = "bbp-macros")]
        {
            // Compiler-owned seed expansions also preserve the native mapping
            // view of every reused input, without changing its public span.
            let spans = result
                .forms
                .data
                .iter()
                .enumerate()
                .filter_map(|(i, data)| {
                    original_for(data.identity())
                        .and_then(FormView::compiler_span_override)
                        .map(|span| (FormId(i), span.clone()))
                })
                .collect();
            result = result.with_compiler_spans(spans);
        }
        result
    }
}

/// Readable assertion text from finite compiler data, preserving UTF-16 names.
/// This is diagnostic formatting, never a representation used to transport
/// forms, with one exception: a session directive built from data, not read
/// from source text, is written with it to be read as a namespace header
/// (`compiler::session_syntax`); its metadata is taken from the form itself.
pub(crate) fn diagnostic_text(data: &Datum) -> Vec<u16> {
    fn text(out: &mut Vec<u16>, s: &str) {
        out.extend(s.encode_utf16());
    }
    fn write(out: &mut Vec<u16>, d: &Datum) {
        use DatumKind as D;
        match d.kind() {
            D::Nil => text(out, "nil"),
            D::Boolean(v) => text(out, if *v { "true" } else { "false" }),
            D::Integer(v) => text(out, &v.to_string()),
            D::Double(v) => text(
                out,
                &if v.is_nan() {
                    "##NaN".into()
                } else if *v == f64::INFINITY {
                    "##Inf".into()
                } else if *v == f64::NEG_INFINITY {
                    "##-Inf".into()
                } else if v.fract() == 0.0 {
                    format!("{v:.1}")
                } else {
                    v.to_string()
                },
            ),
            D::Ratio {
                numerator,
                denominator,
            } => text(out, &format!("{numerator}/{denominator}")),
            D::Character(v) => {
                text(out, "\\");
                match *v {
                    10 => text(out, "newline"),
                    13 => text(out, "return"),
                    9 => text(out, "tab"),
                    32 => text(out, "space"),
                    v => out.push(v),
                }
            }
            D::String(v) => {
                text(out, "\"");
                for &c in v.iter() {
                    match c {
                        34 => text(out, "\\\""),
                        92 => text(out, "\\\\"),
                        10 => text(out, "\\n"),
                        13 => text(out, "\\r"),
                        9 => text(out, "\\t"),
                        8 => text(out, "\\b"),
                        12 => text(out, "\\f"),
                        c => out.push(c),
                    }
                }
                text(out, "\"");
            }
            D::Symbol(n) | D::Keyword(n) => {
                if matches!(d.kind(), D::Keyword(_)) {
                    text(out, ":")
                }
                if let Some(ns) = n.namespace() {
                    out.extend(ns);
                    text(out, "/")
                }
                out.extend(n.name());
            }
            D::Collection { kind, children } => {
                let (open, close) = match kind {
                    CollectionKind::Map => ("{", "}"),
                    CollectionKind::Vector => ("[", "]"),
                    CollectionKind::Set => ("#{", "}"),
                    _ => ("(", ")"),
                };
                text(out, open);
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        text(out, " ")
                    }
                    write(out, child)
                }
                text(out, close)
            }
            D::CompilerLiteral { kind, payload } => {
                match kind {
                    CompilerLiteralKind::Regex => text(out, "#"),
                    CompilerLiteralKind::Uuid => text(out, "#uuid "),
                    CompilerLiteralKind::Instant => text(out, "#inst "),
                    CompilerLiteralKind::JsArray | CompilerLiteralKind::JsObject => {
                        text(out, "#js ")
                    }
                    CompilerLiteralKind::Queue => text(out, "#queue "),
                    CompilerLiteralKind::BigInteger => {
                        if let D::String(v) = payload.kind() {
                            out.extend(v.iter().copied())
                        } else {
                            write(out, payload)
                        }
                        text(out, "N");
                        return;
                    }
                }
                write(out, payload);
            }
        }
    }
    let mut out = vec![];
    write(&mut out, data);
    out
}

// Compiler data, and the programs, indexes and snapshots built from it, are
// shared between compile threads by reference (docs/compiler-concurrency.md,
// "Thread safety today"). An `Rc`, `Cell` or `RefCell` added to any of these
// fails every build, not only test builds.
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<Name>();
    send_sync::<Source>();
    send_sync::<Datum>();
    send_sync::<Form>();
    send_sync::<Origin>();
    send_sync::<crate::parser::SyntaxTree>();
    send_sync::<crate::analyzer::Program>();
    send_sync::<crate::application::PreparedApplication>();
    send_sync::<crate::name_index::NameIndex>();
    send_sync::<crate::compilation_session::DefinitionSnapshot>();
    send_sync::<crate::compiler_reader::NamespaceView>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_form_occurrences_preserve_order_identity_and_provenance() {
        let source = Source::new("forms.cljs", "0123456789abcdefghijklmnopqrstuvwxyz");
        let metadata = Datum::new(DatumKind::Keyword(Name::new(None, "tag")));
        let symbol = Datum::new(DatumKind::Symbol(Name::from_utf16(Some(&[110]), &[0xd800])))
            .with_metadata(metadata);
        let literal = Datum::new(DatumKind::CompilerLiteral {
            kind: CompilerLiteralKind::Regex,
            payload: Datum::new(DatumKind::String(vec![120, 0xd800].into())),
        });
        let data = Datum::new(DatumKind::Collection {
            kind: CollectionKind::Map,
            children: vec![
                symbol,
                Datum::new(DatumKind::Collection {
                    kind: CollectionKind::Vector,
                    children: vec![literal, Datum::new(DatumKind::Integer(7))].into(),
                }),
                Datum::new(DatumKind::Keyword(Name::new(None, "last"))),
                Datum::new(DatumKind::Nil),
            ].into(),
        });
        let name = Name::new(Some("macros"), "expand");
        let mut form = Form::from_data(data, source.span(0..1).unwrap(), Origin::MacroExpansion(Arc::new(name.clone())));
        let forms = Arc::get_mut(&mut form.forms).unwrap();
        forms.spans = FormSpans::Individual((0..forms.nodes.len())
            .map(|i| source.span(i..i + 1).unwrap()).collect());
        forms.expansions.push(ExpansionOrigin {
            macro_name: name, call: source.span(2..4).unwrap(),
            definition: Some(source.span(5..7).unwrap()), helpers: vec![Name::new(None, "helper")],
        });
        #[cfg(feature = "bbp-macros")]
        let form = form.with_compiler_spans(std::collections::HashMap::from([
            (FormId(1), source.span(20..22).unwrap()),
            (FormId(3), source.span(23..25).unwrap()),
        ]));
        fn walk(view: FormView<'_>, owned: &Form, retained: &mut Vec<Form>) {
            assert_eq!(view.node(), owned.node());
            assert_eq!(view.data().identity(), owned.data().identity());
            assert_eq!(view.span().source().id(), owned.span().source().id());
            assert_eq!(view.span().bytes(), owned.span().bytes());
            #[cfg(feature = "bbp-macros")]
            assert_eq!(view.compiler_span().bytes(), owned.compiler_span().bytes());
            let saved = view.to_form();
            assert_eq!(saved.node(), owned.node());
            assert_eq!(saved.expansions()[0].macro_name, owned.expansions()[0].macro_name);
            assert_eq!(saved.metadata().map(|m| m.data().identity()), owned.metadata().map(|m| m.data().identity()));
            retained.push(saved);
            let children = owned.children();
            assert_eq!(view.children().len(), children.len());
            for (child, expected) in view.children().zip(children) { walk(child, &expected, retained); }
        }
        let mut retained = Vec::new();
        // Include metadata and compiler payload occurrences as well as tree children.
        for id in 0..form.nodes().len() {
            let occurrence = form.at(FormId(id)).unwrap();
            walk(occurrence.view(), &occurrence, &mut retained);
        }
        drop(form);
        for saved in retained {
            assert_eq!(saved.expansions()[0].helpers[0], Name::new(None, "helper"));
            assert!(!saved.span().text().is_empty());
        }
    }

    #[test]
    fn pure_expansion_preserves_last_occurrence_within_its_input_subtree() {
        let source = Source::new("expansion.cljs", "0123456789abcdefghijklmnopqrstuv");
        let builder = DataBuilder;
        let shared = builder.symbol(Name::from_utf16(None, &[0xd800, 120]));
        let literal = builder.compiler_literal(CompilerLiteralKind::Regex, &shared)
            .with_metadata(shared.clone());
        let root = builder.vector(&[shared.clone(), literal.clone(), shared.clone()])
            .unwrap().with_metadata(shared.clone());
        let outer_data = builder.vector(&[root.clone(), shared.clone()]).unwrap();
        let origins = [Origin::Source, Origin::Quote, Origin::Deref, Origin::SyntaxQuote,
            Origin::Unquote, Origin::Metadata, Origin::AnonymousFunction,
            Origin::AnonymousArgument, Origin::Source];
        let mut nodes: Vec<_> = origins.into_iter().map(|origin| FormNode {
            kind: FormKind::Scalar, metadata: None, compiler_payload: None, origin,
        }).collect();
        nodes[3].metadata = Some(FormId(1));
        nodes[3].compiler_payload = Some(FormId(2));
        nodes[6].kind = FormKind::Collection {
            kind: CollectionKind::Vector,
            children: vec![FormId(0), FormId(3), FormId(4)].into(),
        };
        nodes[6].metadata = Some(FormId(5));
        nodes[8].kind = FormKind::Collection {
            kind: CollectionKind::Vector, children: vec![FormId(6), FormId(7)].into(),
        };
        let mut outer = Form::from_parts(
            nodes,
            vec![shared.clone(), shared.clone(), shared.clone(), literal.clone(),
                 shared.clone(), shared.clone(), root.clone(), shared.clone(), outer_data],
            (0..9).map(|id| source.span(id..id + 1).unwrap()).collect(),
            FormId(8),
        );
        let name = Name::new(Some("test"), "expand");
        Arc::get_mut(&mut outer.forms).unwrap().expansions.push(ExpansionOrigin {
            macro_name: Name::new(None, "earlier"), call: source.span(9..10).unwrap(),
            definition: Some(source.span(10..11).unwrap()), helpers: vec![Name::new(None, "helper")],
        });
        #[cfg(feature = "bbp-macros")]
        let outer = outer.with_compiler_spans(std::collections::HashMap::from([
            (FormId(2), source.span(20..21).unwrap()),
            (FormId(5), source.span(22..23).unwrap()),
            (FormId(7), source.span(24..25).unwrap()),
        ]));
        let mut results = Vec::new();
        // For a literal, payload follows metadata. For the containing vector,
        // root metadata follows all children. The later occurrence in the
        // outer arena is outside both input subtrees and must never win.
        for (input, last_shared) in [(3, 2), (6, 5)] {
            let original = outer.at(FormId(input)).unwrap();
            let fresh = builder.string([0xdfff, 121]);
            let data = builder.list(&[fresh, original.data(), shared.clone(), literal.clone()]).unwrap();
            let result = Form::from_expansion(data, &original, name.clone());
            for (index, datum) in result.forms.data.iter().enumerate() {
                let previous = if datum.identity() == shared.identity() {
                    Some(last_shared)
                } else if datum.identity() == literal.identity() {
                    Some(3)
                } else if datum.identity() == root.identity() {
                    Some(6)
                } else {
                    None
                };
                let expected_span = previous.unwrap_or(input);
                let form = result.at(FormId(index)).unwrap();
                assert_eq!(form.span().source().id(), source.id());
                assert_eq!(form.span().bytes(), expected_span..expected_span + 1);
                assert_eq!(form.node().origin, previous.map(|id| outer.forms.nodes[id].origin.clone())
                    .unwrap_or_else(|| Origin::MacroExpansion(Arc::new(name.clone()))));
                #[cfg(feature = "bbp-macros")]
                assert_eq!(form.compiler_span_override().map(|s| s.bytes()), match previous {
                    Some(2) => Some(20..21), Some(5) => Some(22..23), _ => None,
                });
            }
            assert_eq!(result.expansions().len(), 2);
            assert_eq!(result.expansions()[0].helpers, vec![Name::new(None, "helper")]);
            assert_eq!(result.expansions()[1].macro_name, name);
            assert_eq!(result.expansions()[1].call.bytes(), input..input + 1);
            results.push(result);
        }
        drop((outer, source));
        for result in results {
            assert_eq!(result.expansions()[0].definition.as_ref().unwrap().text(), "a");
            assert!(!result.children()[1].span().text().is_empty());
            assert_eq!(result.children()[2].data().identity(), shared.identity());
        }
    }

    #[test]
    fn pure_expansion_keeps_equal_values_distinct_across_identity_map_growth() {
        let builder = DataBuilder;
        let count = 1024;
        // Equal values with distinct live allocation identities, each reused
        // at a later occurrence with a different span. The map must grow
        // repeatedly without confusing language equality with identity.
        let values: Vec<_> = (0..count).map(|_| builder.integer(7)).collect();
        let source = Source::new("many-occurrences.cljs", "x".repeat(count * 2 + 1));
        let mut data = values.clone();
        data.extend(values.iter().rev().cloned());
        let root = builder.vector(&data).unwrap();
        let mut nodes: Vec<_> = (0..count * 2).map(|_| FormNode {
            kind: FormKind::Scalar,
            metadata: None,
            compiler_payload: None,
            origin: Origin::Quote,
        }).collect();
        nodes.push(FormNode {
            kind: FormKind::Collection {
                kind: CollectionKind::Vector,
                children: (0..count * 2).map(FormId).collect(),
            },
            metadata: None,
            compiler_payload: None,
            origin: Origin::Source,
        });
        data.push(root);
        let original = Form::from_parts(
            nodes,
            data,
            (0..count * 2 + 1).map(|i| source.span(i..i + 1).unwrap()).collect(),
            FormId(count * 2),
        );
        let name = Name::new(None, "identity-test");
        let mut generated = values.clone();
        generated.push(builder.integer(7));
        let expanded = Form::from_expansion(
            builder.list(&generated).unwrap(), &original, name.clone(),
        );
        drop((original, source, generated));
        let children = expanded.children();
        for (i, child) in children[..count].iter().enumerate() {
            let last = count * 2 - 1 - i;
            assert_eq!(child.span().bytes(), last..last + 1);
            assert_eq!(child.span().text(), "x");
            assert_eq!(child.node().origin, Origin::Quote);
            assert_eq!(child.data().identity(), values[i].identity());
        }
        assert_eq!(children[count].span().bytes(), count * 2..count * 2 + 1);
        assert_eq!(children[count].node().origin, Origin::MacroExpansion(Arc::new(name)));
        assert!(values.iter().all(|value| value.identity() != children[count].data().identity()));
    }

    #[test]
    fn generated_spans_keep_source_ownership_after_the_arena_is_dropped() {
        let (owned, weak) = {
            let source = Source::new("single.cljs", "0123456789");
            let weak = Arc::downgrade(&source.0);
            let builder = DataBuilder;
            let payload = builder.string([120, 0xd800]);
            let literal = builder.compiler_literal(CompilerLiteralKind::Regex, &payload)
                .with_metadata(builder.keyword(Name::new(None, "tag")));
            let data = builder.vector(&[literal.clone(), builder.integer(1)]).unwrap();
            let original = Form::from_data(data.clone(), source.span(2..5).unwrap(), Origin::Source);
            let expanded = Form::from_expansion(
                builder.list(&[data, literal]).unwrap(), &original, Name::new(None, "expand"),
            );
            for id in 0..expanded.nodes().len() {
                let occurrence = expanded.at(FormId(id)).unwrap();
                assert_eq!(occurrence.view().span().source().id(), source.id());
                assert_eq!(occurrence.view().span().bytes(), 2..5);
                assert_eq!(occurrence.span().text(), "234");
            }
            (expanded.children()[0].span(), weak)
        };
        assert!(weak.upgrade().is_some());
        assert_eq!(owned.text(), "234");
        assert_eq!(owned.source().line_column(owned.bytes().start), Some((1, 3)));
        drop(owned);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn mixed_source_spans_and_compacted_leaves_keep_exact_source_ownership() {
        let (leaf, weak_a, weak_b, weak_c) = {
            // Equal paths and contents must not merge distinct source identities.
            let a = Source::new("same.cljs", "0123456789");
            let b = Source::new("same.cljs", "0123456789");
            let c = Source::new("other.cljs", "abcdefghij");
            let (weak_a, weak_b, weak_c) = (Arc::downgrade(&a.0), Arc::downgrade(&b.0), Arc::downgrade(&c.0));
            let builder = DataBuilder;
            let da = builder.integer(7);
            let db = builder.symbol(Name::new(None, "argument"));
            let dc = builder.string([0xd800, 120]);
            let fa = Form::from_data(da.clone(), a.span(1..2).unwrap(), Origin::Source);
            let fb = Form::from_data(db.clone(), b.span(3..5).unwrap(), Origin::Quote);
            #[cfg(feature = "bbp-macros")]
            let fb = {
                let id = fb.id();
                fb.with_compiler_spans(std::collections::HashMap::from([
                    (id, b.span(6..8).unwrap()),
                ]))
            };
            let fc = Form::from_data(dc.clone(), c.span(2..4).unwrap(), Origin::Deref);
            let originals = std::collections::HashMap::from([
                (da.identity(), fa.clone()), (db.identity(), fb), (dc.identity(), fc),
            ]);
            let literal = builder.compiler_literal(CompilerLiteralKind::Regex, &dc)
                .with_metadata(db.clone());
            let data = builder.vector(&[da.clone(), db.clone(), dc.clone(), db.clone(), literal])
                .unwrap().with_metadata(dc.clone());
            let result = Form::from_bridge_expansion(data, &fa, ExpansionOrigin {
                macro_name: Name::new(None, "mixed"), call: fa.span(), definition: None, helpers: vec![],
            }, &originals);
            for id in 0..result.nodes().len() {
                let occurrence = result.at(FormId(id)).unwrap();
                let identity = occurrence.data().identity();
                let (source, bytes, origin) = if identity == db.identity() {
                    (&b, 3..5, Origin::Quote)
                } else if identity == dc.identity() {
                    (&c, 2..4, Origin::Deref)
                } else if identity == da.identity() {
                    (&a, 1..2, Origin::Source)
                } else {
                    (&a, 1..2, Origin::MacroExpansion(Arc::new(Name::new(None, "mixed"))))
                };
                assert_eq!(occurrence.span().source().id(), source.id());
                assert_eq!(occurrence.view().span().source().id(), source.id());
                assert_eq!(occurrence.span().bytes(), bytes);
                assert_eq!(occurrence.view().span().bytes(), bytes);
                assert_eq!(occurrence.node().origin, origin);
                #[cfg(feature = "bbp-macros")]
                assert_eq!(occurrence.compiler_span_override().map(|s| s.bytes()),
                    (identity == db.identity()).then_some(6..8));
            }
            (result.children()[1].compact_leaf(), weak_a, weak_b, weak_c)
        };
        // The compacted leaf needs its own source and the macro call's source,
        // but must release the unrelated source from another occurrence.
        assert!(weak_a.upgrade().is_some());
        assert!(weak_b.upgrade().is_some());
        assert!(weak_c.upgrade().is_none());
        assert_eq!(leaf.span().text(), "34");
        assert_eq!(leaf.expansions()[0].call.text(), "1");
        let owned = leaf.span();
        drop(leaf);
        assert!(weak_a.upgrade().is_none());
        assert!(weak_b.upgrade().is_some());
        drop(owned);
        assert!(weak_b.upgrade().is_none());
    }

    #[test]
    fn concurrent_source_line_index_preserves_locations() {
        let text = "a\nbc\r\n\ndef\rλx\n";
        let expected: Vec<_> = (0..text.len() + 3)
            .map(|offset| Source::with_line_offset("oracle.cljs", text, 7).line_column(offset))
            .collect();
        assert_eq!(expected[0], Some((8, 1)));
        assert_eq!(expected[3], Some((9, 2)));
        assert_eq!(expected[7], Some((11, 1)));
        assert_eq!(expected[text.len()], Some((13, 1)));
        assert_eq!(expected[text.len() + 1], None);
        for _ in 0..50 {
            let source = Source::with_line_offset("shared.cljs", text, 7);
            let barrier = std::sync::Barrier::new(4);
            std::thread::scope(|scope| {
                for _ in 0..4 {
                    let (source, barrier, expected) = (&source, &barrier, &expected);
                    scope.spawn(move || {
                        barrier.wait();
                        let found: Vec<_> = (0..expected.len())
                            .map(|offset| source.line_column(offset))
                            .collect();
                        assert_eq!(&found, expected);
                    });
                }
            });
        }
    }
}
