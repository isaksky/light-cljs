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

struct Forms {
    nodes: Box<[FormNode]>,
    data: Box<[Datum]>,
    spans: Box<[SourceSpan]>,
    expansions: Vec<ExpansionOrigin>,
}
/// Immutable semantic arena, retaining owned compiler data and source provenance.
/// Children are source ordered, including alternating map keys/values. Datum
/// storage order must never determine executable map evaluation order.
#[derive(Clone)]
pub struct Form {
    forms: Arc<Forms>,
    id: FormId,
}
impl Form {
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
        leaf
    }
    pub(crate) fn from_parts(
        nodes: Vec<FormNode>,
        data: Vec<Datum>,
        spans: Vec<SourceSpan>,
        id: FormId,
    ) -> Self {
        assert_eq!(nodes.len(), data.len());
        assert_eq!(nodes.len(), spans.len());
        Self {
            forms: Arc::new(Forms {
                nodes: nodes.into(),
                data: data.into(),
                spans: spans.into(),
                expansions: vec![],
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

impl Form {
    /// A pure generated form keeps every reused input datum's exact provenance.
    pub(crate) fn from_expansion(data: Datum, original: &Form, name: Name) -> Self {
        fn collect(f: &Form, originals: &mut std::collections::HashMap<usize, Form>) {
            originals.insert(f.data().identity(), f.clone());
            for child in f.children() {
                collect(&child, originals);
            }
            if let Some(meta) = f.metadata() {
                collect(&meta, originals);
            }
            if let Some(payload) = f.node().compiler_payload.and_then(|id| f.at(id)) {
                collect(&payload, originals);
            }
        }
        let mut originals = std::collections::HashMap::new();
        collect(original, &mut originals);
        Self::from_bridge_expansion(
            data,
            original,
            ExpansionOrigin {
                macro_name: name,
                call: original.span(),
                definition: None,
                helpers: vec![],
            },
            &originals,
        )
    }
    pub(crate) fn from_bridge_expansion(
        data: Datum,
        original: &Form,
        expansion: ExpansionOrigin,
        originals: &std::collections::HashMap<usize, Form>,
    ) -> Self {
        fn add(
            data: Datum,
            original: &Form,
            originals: &std::collections::HashMap<usize, Form>,
            name: &Name,
            nodes: &mut Vec<FormNode>,
            values: &mut Vec<Datum>,
            spans: &mut Vec<SourceSpan>,
        ) -> FormId {
            let reused = originals.get(&data.identity());
            let span = reused.unwrap_or(original).span();
            let origin = reused
                .map(|f| f.node().origin.clone())
                .unwrap_or_else(|| Origin::MacroExpansion(Arc::new(name.clone())));
            let kind = match data.kind() {
                DatumKind::Collection { kind, children } => FormKind::Collection {
                    kind: *kind,
                    children: children
                        .iter()
                        .map(|d| add(d.clone(), original, originals, name, nodes, values, spans))
                        .collect(),
                },
                _ => FormKind::Scalar,
            };
            let metadata = data
                .metadata()
                .map(|d| add(d, original, originals, name, nodes, values, spans));
            let compiler_payload = data
                .compiler_literal_data()
                .map(|d| add(d, original, originals, name, nodes, values, spans));
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
        let (mut nodes, mut values, mut spans) = (vec![], vec![], vec![]);
        let id = add(
            data,
            original,
            originals,
            &expansion.macro_name,
            &mut nodes,
            &mut values,
            &mut spans,
        );
        let mut result = Self::from_parts(nodes, values, spans, id);
        let forms = Arc::get_mut(&mut result.forms).unwrap();
        forms.expansions = original.expansions().to_vec();
        forms.expansions.push(expansion);
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
};

#[cfg(test)]
mod tests {
    use super::Source;

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
