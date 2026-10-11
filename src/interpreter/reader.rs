//! Contextual semantic construction over the existing syntax parser.
use super::{
    Context, ExecutableUnit, HandleError, Name, RootedValue, Source, SourceSpan, ValueView,
};
use crate::parser::{
    CollectionKind, Node, NodeId, NodeKind, NumberKind, Parser, PrefixKind, SymbolicValue,
    SyntaxTree,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    ops::Range,
    rc::Rc,
};

pub use crate::compiler_data::{FormId, FormKind, FormNode, Origin};

pub(super) struct Forms {
    nodes: Box<[FormNode]>,
    // Constants and provenance indexed by FormId. One collector root owns both.
    storage: ExecutableUnit,
}

/// Immutable, owned reader form with source-ordered children and ordinary data.
/// All handles survive dropping the parser/input/context or resetting the owner.
#[derive(Clone)]
pub struct Form {
    forms: Rc<Forms>,
    id: FormId,
}
impl Form {
    pub(super) fn reuse_identity(&self) -> (super::UnitId, FormId) {
        (self.forms.storage.id(), self.id)
    }
    pub(super) fn source_length(&self) -> usize {
        let storage = &self.forms.storage;
        let source = storage.code.locations[self.id.0].source;
        storage
            .owner
            .store
            .borrow()
            .sources
            .get(source.0)
            .expect("form source")
            .text
            .len()
    }
    pub(super) fn weak_forms(&self) -> std::rc::Weak<Forms> {
        Rc::downgrade(&self.forms)
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
    pub fn data(&self) -> RootedValue {
        self.forms
            .storage
            .constant(self.id.0)
            .expect("form constant")
    }
    pub fn span(&self) -> SourceSpan {
        self.forms.storage.span(self.id.0).expect("form provenance")
    }
    pub(super) fn diagnostic_location(&self) -> super::DiagnosticLocation {
        let storage = &self.forms.storage;
        super::DiagnosticLocation::new(
            &storage.owner.store.borrow(),
            &storage.code.locations[self.id.0],
        )
    }
    pub fn children(&self) -> Vec<Self> {
        match &self.node().kind {
            FormKind::Scalar => Vec::new(),
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
            .field("id", &self.id)
            .field("data", &self.data())
            .field("span", &self.span())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedSyntax {
    SyntaxQuote,
    Unquote,
    UnquoteSplicing,
    ReaderConditional,
    NamespacedMap,
    TaggedLiteral,
    AutoResolvedKeyword,
    Regex,
    BigInteger,
    Decimal,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LowerErrorKind {
    Unsupported(UnsupportedSyntax),
    /// Syntax accepted by the code reader but outside the restricted EDN mode.
    EdnSyntax(&'static str),
    UnsupportedEdnValue,
    NumericOverflow,
    InvalidMetadataTarget,
    InvalidCompilerLiteral(&'static str),
    InvalidMetadata,
    DuplicateMapKey,
    DuplicateSetMember,
    InvalidAnonymousArgument,
    AnonymousParameterLimit,
    InvalidSyntax,
    InvalidSplice,
    MissingReaderForm,
    MissingAlias(Box<Name>),
    UnknownTag(Box<Name>),
    DataReader {
        tag: Box<Name>,
        error: Box<super::NativeError>,
    },
    WrongOwner,
    FormNestingLimit,
    Resource(super::ResourceError),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadErrorKind {
    Syntax(crate::parser::ErrorKind),
    Lower(LowerErrorKind),
}
#[derive(Clone, Debug)]
pub struct ReadError {
    pub kind: ReadErrorKind,
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
            "{}:{line}:{column}: {:?}",
            self.span.source().name(),
            self.kind
        )
    }
}
impl std::error::Error for ReadError {}

/// Bounds expansion of `%n`, whose output can greatly exceed its input size.
#[derive(Clone, Copy, Debug)]
pub struct ReaderLimits {
    pub max_anonymous_parameters: usize,
}

/// Host configuration contains no runtime roots or captured callbacks.
#[derive(Clone)]
pub(super) struct ReaderConfig {
    pub(super) compiler: bool,
    features: HashSet<Name>,
    tags: HashMap<Name, &'static super::NativeDefinition>,
    default: Option<&'static super::NativeDefinition>,
}
impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            compiler: false,
            features: HashSet::from([Name::new(None, "clj")]),
            tags: HashMap::new(),
            default: None,
        }
    }
}
impl Default for ReaderLimits {
    fn default() -> Self {
        Self {
            max_anonymous_parameters: 1024,
        }
    }
}

/// Explicit EDN handlers. These are per-reader roots, never ambient code-reader
/// registrations. Handlers receive data; the fallback receives tag and data.
#[derive(Clone, Default)]
pub struct EdnOptions {
    pub readers: HashMap<Name, RootedValue>,
    pub default: Option<RootedValue>,
}

/// Pull reader over the shared syntax parser. EOF is None, literal nil is Some.
/// Like Parser, the reader is fused after the first syntax or semantic failure.
/// The source is borrowed only by this cursor, never by its returned forms.
pub struct FormReader<'src> {
    context: Context,
    source: &'src Source,
    parser: Parser<'src>,
    discarded: usize,
    finished: bool,
    started: bool,
    limits: ReaderLimits,
    edn: Option<Rc<EdnOptions>>,
}
/// Owned incremental cursor; pass the context on each call so namespace changes
/// and reader policy updates take effect between forms. Failure and EOF fuse it.
/// Returned forms and captured spans retain the original source independently.
pub struct SourceReader {
    source: Source,
    offset: usize,
    finished: bool,
    started: bool,
    limits: ReaderLimits,
    edn: Option<Rc<EdnOptions>>,
}
impl SourceReader {
    pub fn position(&self) -> usize {
        self.offset
    }
    pub fn source(&self) -> &Source {
        &self.source
    }
    pub fn read_form(&mut self, context: &Context) -> Result<Option<Form>, ReadError> {
        self.read_form_with_source(context)
            .map(|result| result.map(|(form, _)| form))
    }
    /// Capture the exact consumed bytes, including leading whitespace, comments
    /// and discards; unlike printing the resulting value this preserves spelling.
    pub fn read_form_with_source(
        &mut self,
        context: &Context,
    ) -> Result<Option<(Form, SourceSpan)>, ReadError> {
        if !Rc::ptr_eq(&context.owner, &self.source.owner) {
            return Err(ReadError {
                kind: ReadErrorKind::Lower(LowerErrorKind::WrongOwner),
                span: self.source.span(self.offset..self.offset).unwrap(),
                related: None,
            });
        }
        if self.finished {
            return Ok(None);
        }
        let start = self.offset;
        let mut reader = context
            .reader_with_limits(&self.source, self.limits)
            .unwrap();
        reader.parser = Parser::at_offset(self.source.text(), start).retaining_discards();
        reader.started = self.started;
        reader.edn = self.edn.clone();
        let result = reader.read_form();
        self.started = true;
        self.offset = reader.parser.position();
        self.finished = !matches!(result, Ok(Some(_)));
        result
            .map(|result| result.map(|form| (form, self.source.span(start..self.offset).unwrap())))
    }
}

impl Context {
    /// Replace the reader feature set (keyword names). The default is `:clj`;
    /// an empty set selects only `:default`. Existing readers see updates.
    pub fn set_reader_features(&mut self, features: impl IntoIterator<Item = Name>) {
        self.invalidate_analysis();
        self.readers.borrow_mut().features = features.into_iter().collect();
    }
    /// Register an explicit tag handler receiving one already-read data value.
    /// No Var, constructor, class, filesystem or network lookup is performed.
    pub fn register_data_reader(&mut self, tag: Name, handler: &'static super::NativeDefinition) {
        self.invalidate_analysis();
        self.readers.borrow_mut().tags.insert(tag, handler);
    }
    pub fn remove_data_reader(&mut self, tag: &Name) {
        self.invalidate_analysis();
        self.readers.borrow_mut().tags.remove(tag);
    }
    /// Fallback receives the tag symbol and already-read data value.
    pub fn set_default_data_reader(&mut self, handler: Option<&'static super::NativeDefinition>) {
        self.invalidate_analysis();
        self.readers.borrow_mut().default = handler;
    }
    fn reader_context(&self) -> Self {
        Self {
            owner: self.owner.clone(),
            evaluation: super::evaluation::Scratch::default(),
            analysis_scratch: super::evaluation::AnalysisScratch::default(),
            reuse: super::reuse::AnalysisReuse::default(),
            reuse_revision: self.reuse_revision.clone(),
            active_calls: self.active_calls,
            active_expressions: self.active_expressions,
            accounting: self.accounting.clone(),
            namespaces: self.namespaces.clone(),
            readers: self.readers.clone(),
            macros: super::macros::MacroState::default(),
            loading: self.loading.clone(),
            builtin_var_mutation: self.builtin_var_mutation.clone(),
            policy: self.policy.clone(),
            var_remap: self.var_remap.clone(),
            #[cfg(test)]
            evaluation_trace: Vec::new(),
        }
    }
    // Temporary native storage adapter; deleted by light-cljs-115.
    pub(crate) fn native_form_adapter(
        &self,
        nodes: Vec<FormNode>,
        values: &[RootedValue],
        spans: &[SourceSpan],
        id: FormId,
    ) -> Form {
        let storage = self
            .executable_unit(values, spans)
            .expect("native adapter owner");
        Form {
            forms: Rc::new(Forms {
                nodes: nodes.into(),
                storage,
            }),
            id,
        }
    }
    /// Supply owned data as a form, for embedding and later macro expansion.
    /// No printing, parsing or source arena is involved. Collection order is
    /// the supplied value's order (map/set traversal is unspecified).
    pub fn form_from_data(
        &self,
        value: &RootedValue,
        origin: &SourceSpan,
    ) -> Result<Form, ReadError> {
        self.data_form_with_origin(value, origin, None)
    }
    fn data_form_with_origin(
        &self,
        value: &RootedValue,
        origin: &SourceSpan,
        expansion: Option<(&Form, Name, Option<&RootedValue>)>,
    ) -> Result<Form, ReadError> {
        let _scope = self.accounting.enter();
        let mut lower = Lowerer::new(self, origin.source(), &[], &[], ReaderLimits::default());
        if !Rc::ptr_eq(&self.owner, &value.owner)
            || !Rc::ptr_eq(&self.owner, &origin.source().owner)
        {
            return Err(lower.error(LowerErrorKind::WrongOwner, origin.bytes()));
        }
        if let Some((call, name, alias)) = expansion {
            lower.data_origin = Origin::MacroExpansion(Rc::new(name));
            let mut pending = vec![call.id()];
            while let Some(id) = pending.pop() {
                lower.resource_check(origin.bytes())?;
                let original = call.at(id).unwrap();
                if let FormKind::Collection { children, .. } = &original.node().kind {
                    self.charge_allocation(children.len() as u64)
                        .map_err(|e| lower.error(LowerErrorKind::Resource(e), origin.bytes()))?;
                    pending.extend(children.iter().copied());
                }
                pending.extend(original.node().metadata);
                pending.extend(original.node().compiler_payload);
                if let Some(id) = original.data().heap_id() {
                    lower.supplied.insert(id, original);
                }
            }
            // &form receives a metadata-annotated copy. Preserve the original
            // form/provenance for that alias as well as the native root itself.
            if let Some(id) = alias.and_then(RootedValue::heap_id) {
                lower.supplied.insert(id, call.clone());
            }
        }
        let mut result = lower
            .data_form(value, origin.bytes(), 0)
            .map(|id| lower.finish(id));
        if let Err(error) = self.accounting.check() {
            match &mut result {
                Err(failure) => {
                    if !matches!(&failure.kind, ReadErrorKind::Lower(LowerErrorKind::DataReader { error: native, .. })
                        if matches!(native.as_ref(), super::NativeError::Invocation(e) if e.error().class() == super::ErrorClass::Control))
                    {
                        failure.kind = ReadErrorKind::Lower(LowerErrorKind::Resource(error));
                    }
                }
                Ok(form) => {
                    return Err(ReadError {
                        kind: ReadErrorKind::Lower(LowerErrorKind::Resource(error)),
                        span: form.span(),
                        related: None,
                    });
                }
            }
        }
        result
    }
    pub(crate) fn macro_form(
        &self,
        value: &RootedValue,
        call: &Form,
        name: Name,
    ) -> Result<Form, ReadError> {
        self.data_form_with_origin(value, &call.span(), Some((call, name, None)))
    }
    // Temporary compiler macro alias adapter; deleted by light-cljs-115.
    pub(crate) fn macro_form_with_alias(
        &self,
        value: &RootedValue,
        call: &Form,
        name: Name,
        alias: &RootedValue,
    ) -> Result<Form, ReadError> {
        self.data_form_with_origin(value, &call.span(), Some((call, name, Some(alias))))
    }
    pub fn source_reader(&self, source: Source) -> Result<SourceReader, HandleError> {
        if !Rc::ptr_eq(&self.owner, &source.owner) {
            return Err(HandleError::WrongOwner);
        }
        Ok(SourceReader {
            source,
            offset: 0,
            finished: false,
            started: false,
            limits: ReaderLimits::default(),
            edn: None,
        })
    }
    // I/O has already charged buffered source bytes; resume the shared reader
    // with full-source byte provenance instead of printing/reparsing values.
    pub(super) fn read_at(
        &self,
        source: &Source,
        offset: usize,
        end: usize,
        edn: Option<&Rc<EdnOptions>>,
    ) -> (Result<Option<Form>, ReadError>, usize) {
        let mut reader = self.reader(source).unwrap();
        reader.parser = Parser::at_offset(&source.text()[..end], offset).retaining_discards();
        reader.started = true;
        reader.edn = edn.cloned();
        let result = reader.read_form();
        (result, reader.parser.position())
    }
    pub fn reader<'src>(&self, source: &'src Source) -> Result<FormReader<'src>, HandleError> {
        self.reader_with_limits(source, ReaderLimits::default())
    }
    pub fn reader_with_limits<'src>(
        &self,
        source: &'src Source,
        limits: ReaderLimits,
    ) -> Result<FormReader<'src>, HandleError> {
        if !Rc::ptr_eq(&self.owner, &source.owner) {
            return Err(HandleError::WrongOwner);
        }
        Ok(FormReader {
            context: self.reader_context(),
            source,
            parser: Parser::new(source.text()).retaining_discards(),
            discarded: 0,
            finished: false,
            started: false,
            limits,
            edn: None,
        })
    }

    /// Restricted data reader using the same parser, semantic arena and limits.
    /// EOF is None; code-reader tags/features/namespace aliases are not inherited.
    pub fn edn_reader<'src>(
        &self,
        source: &'src Source,
        options: EdnOptions,
    ) -> Result<FormReader<'src>, HandleError> {
        self.check_edn_options(&options)?;
        let mut reader = self.reader(source)?;
        reader.edn = Some(Rc::new(options));
        Ok(reader)
    }
    pub fn edn_source_reader(
        &self,
        source: Source,
        options: EdnOptions,
    ) -> Result<SourceReader, HandleError> {
        self.check_edn_options(&options)?;
        let mut reader = self.source_reader(source)?;
        reader.edn = Some(Rc::new(options));
        Ok(reader)
    }
    fn check_edn_options(&self, options: &EdnOptions) -> Result<(), HandleError> {
        if options
            .readers
            .values()
            .chain(options.default.iter())
            .any(|value| !Rc::ptr_eq(&value.owner, &self.owner))
        {
            return Err(HandleError::WrongOwner);
        }
        Ok(())
    }

    /// Lower a retained syntax root without printing/reparsing it. The tree must
    /// correspond to this exact source. Discards already removed by ordinary
    /// `parse` cannot be validated here; use `reader` for full reader validation.
    pub fn lower_form(
        &self,
        source: &Source,
        tree: &SyntaxTree,
        root: NodeId,
    ) -> Result<Form, ReadError> {
        self.lower_with_aliases(source, tree, root, None, &[], &[])
    }
    /// Compiler-only reader view: aliases are data, never interpreter namespace
    /// mutations. Runtime .cljs inputs do not allow reader conditionals.
    pub(crate) fn lower_compiler_form(
        &self,
        source: &Source,
        tree: &SyntaxTree,
        root: NodeId,
        aliases: &[(String, String)],
        discarded: &[NodeId],
        globals: &[(String, String, String)],
    ) -> Result<Form, ReadError> {
        self.lower_with_aliases(source, tree, root, Some(aliases), discarded, globals)
    }
    fn lower_with_aliases(
        &self,
        source: &Source,
        tree: &SyntaxTree,
        root: NodeId,
        aliases: Option<&[(String, String)]>,
        discarded: &[NodeId],
        globals: &[(String, String, String)],
    ) -> Result<Form, ReadError> {
        let _scope = self.accounting.enter();
        let span = &tree.nodes[root.0].span;
        let discarded = discarded
            .iter()
            .copied()
            .filter(|id| *id != root && span.contains(&tree.nodes[id.0].span.start))
            .collect::<Vec<_>>();
        let mut lower = Lowerer::new(
            self,
            source,
            &tree.nodes,
            &discarded,
            ReaderLimits::default(),
        );
        lower.compiler_sets = aliases.is_some();
        lower.compiler_globals = globals
            .iter()
            .map(|(local, ns, name)| (Name::new(None, local), Name::new(Some(ns), name)))
            .collect();
        if let Some(aliases) = aliases {
            lower.aliases = Some(
                aliases
                    .iter()
                    .map(|(a, n)| (Name::new(None, a), Name::new(None, n)))
                    .collect(),
            );
        }
        if !Rc::ptr_eq(&self.owner, &source.owner) {
            return Err(lower.error(LowerErrorKind::WrongOwner, 0..0));
        }
        let mut result = lower
            .required(&mut Input::nodes([root]), 0, 0..0)
            .map(|id| lower.finish(id));
        if let Err(error) = self.accounting.check() {
            match &mut result {
                Err(failure) => {
                    if !matches!(&failure.kind, ReadErrorKind::Lower(LowerErrorKind::DataReader { error: native, .. })
                        if matches!(native.as_ref(), super::NativeError::Invocation(e) if e.error().class() == super::ErrorClass::Control))
                    {
                        failure.kind = ReadErrorKind::Lower(LowerErrorKind::Resource(error));
                    }
                }
                Ok(form) => {
                    return Err(ReadError {
                        kind: ReadErrorKind::Lower(LowerErrorKind::Resource(error)),
                        span: form.span(),
                        related: None,
                    });
                }
            }
        }
        result
    }
}
impl FormReader<'_> {
    pub fn read_form(&mut self) -> Result<Option<Form>, ReadError> {
        let _scope = self.context.accounting.enter();
        if self.finished {
            return Ok(None);
        }
        self.context
            .checkpoint(super::PollPoint::Read)
            .map_err(|e| {
                self.finished = true;
                ReadError {
                    kind: ReadErrorKind::Lower(LowerErrorKind::Resource(e)),
                    span: self.source.span(0..0).unwrap(),
                    related: None,
                }
            })?;
        if !self.started {
            self.started = true;
            if let Err(error) = self
                .context
                .charge_allocation(self.source.text().len() as u64)
            {
                self.finished = true;
                return Err(ReadError {
                    kind: ReadErrorKind::Lower(LowerErrorKind::Resource(error)),
                    span: self.source.span(0..self.source.text().len()).unwrap(),
                    related: None,
                });
            }
        }
        if self.edn.is_some() {
            self.parser.limit_nesting(self.context.edn_nesting_budget());
        }
        let mut lower = Lowerer::new(&self.context, self.source, &[], &[], self.limits);
        lower.edn = self.edn.clone();
        lower.syntax = Syntax::Parser(&mut self.parser);
        lower.parser_discards = self.discarded;
        let result = (|| {
            let mut input = Input {
                top_level: true,
                ..Input::default()
            };
            let form = lower.read_next(&mut input, 0, false)?;
            lower.discard_before(usize::MAX, 0)?;
            Ok(form.map(|id| lower.finish(id)))
        })();
        self.discarded = self.parser.discarded().len();
        let result = result.map_err(|mut error: ReadError| {
            if self.edn.is_some()
                && error.kind == ReadErrorKind::Syntax(crate::parser::ErrorKind::NestingLimit)
            {
                error.kind = ReadErrorKind::Lower(LowerErrorKind::Resource(
                    self.context
                        .accounting
                        .fail(super::ResourceError::EvaluationDepthExceeded),
                ));
            }
            error
        });
        let result = if let Err(error) = self.context.accounting.check() {
            let mut failure = result.err().unwrap_or_else(|| ReadError {
                kind: ReadErrorKind::Lower(LowerErrorKind::Resource(error)),
                span: self.source.span(0..0).unwrap(),
                related: None,
            });
            if !matches!(&failure.kind, ReadErrorKind::Lower(LowerErrorKind::DataReader {
                error: native, ..
            }) if matches!(native.as_ref(), super::NativeError::Invocation(e) if e.error().class() == super::ErrorClass::Control))
            {
                failure.kind = ReadErrorKind::Lower(LowerErrorKind::Resource(error));
            }
            Err(failure)
        } else {
            result
        };
        if !matches!(result, Ok(Some(_))) {
            self.finished = true;
        }
        result
    }
}

// Streaming lookahead only determines how many syntax roots a prefix needs.
// It never constructs values, resolves names, validates metadata or calls tags.
// Closed collections/conditional branches are barriers: malformed operands in
// them are diagnosed by the semantic reader, never completed from later input.
struct BufferedProbe<'a> {
    parser: Parser<'a>,
    text: &'a str,
    features: &'a HashSet<Name>,
    discarded: Vec<NodeId>,
    next_discard: usize,
    parser_discarded: usize,
    needs_more: bool,
}
impl BufferedProbe<'_> {
    fn read_root(&mut self) -> Result<Option<NodeId>, crate::parser::ParseError> {
        let result = self.parser.read_form()?;
        self.discarded
            .extend_from_slice(&self.parser.discarded()[self.parser_discarded..]);
        self.parser_discarded = self.parser.discarded().len();
        self.discarded[self.next_discard..].sort_by_key(|id| self.parser.nodes()[id.0].span.start);
        Ok(result)
    }

    fn skip_inside(&mut self, end: usize) {
        while self
            .discarded
            .get(self.next_discard)
            .is_some_and(|id| self.parser.nodes()[id.0].span.start < end)
        {
            self.next_discard += 1;
        }
    }
    fn next(
        &mut self,
        pending: &mut VecDeque<NodeId>,
        required: bool,
        external: bool,
        depth: usize,
    ) -> Result<bool, crate::parser::ParseError> {
        if depth >= 128 {
            return Ok(true);
        } // semantic depth diagnostic
        loop {
            let id = match pending.pop_front() {
                Some(id) => id,
                None if required && external => match self.read_root()? {
                    Some(id) => id,
                    None => {
                        self.needs_more = true;
                        return Ok(false);
                    }
                },
                None => return Ok(false),
            };
            let node = self.parser.nodes()[id.0].clone();
            if let Some(&discard) = self.discarded.get(self.next_discard)
                && self.parser.nodes()[discard.0].span.start < node.span.start
            {
                self.next_discard += 1;
                pending.push_front(id);
                pending.push_front(discard);
                self.next(pending, true, external, depth + 1)?;
                continue;
            }
            match node.kind {
                NodeKind::Prefix { form, .. } | NodeKind::Tagged { form, .. } => {
                    pending.push_front(form);
                    self.next(pending, true, external, depth + 1)?;
                    return Ok(true);
                }
                NodeKind::Metadata { metadata, target } => {
                    pending.push_front(target);
                    pending.push_front(metadata);
                    self.next(pending, true, external, depth + 1)?;
                    self.next(pending, true, external, depth + 1)?;
                    return Ok(true);
                }
                NodeKind::Conditional { branches, .. } => {
                    let NodeKind::Collection { children, .. } =
                        &self.parser.nodes()[branches.0].kind
                    else {
                        return Ok(true);
                    };
                    let selected = children.as_chunks::<2>().0.iter().position(|pair| {
                        let feature = &self.text[self.parser.nodes()[pair[0].0].span.clone()];
                        feature == ":default" || self.features.contains(&name(&feature[1..]))
                    });
                    let chosen = if let Some(index) = selected {
                        let mut branch = children[index * 2 + 1..].iter().copied().collect();
                        // Earlier branches are closed; their discards cannot
                        // consume the selected branch's pending operands.
                        let start = self.parser.nodes()[children[index * 2 + 1].0].span.start;
                        self.skip_inside(start);
                        self.next(&mut branch, false, false, depth + 1)?
                    } else {
                        false
                    };
                    self.skip_inside(node.span.end);
                    if chosen {
                        return Ok(true);
                    }
                }
                _ => {
                    self.skip_inside(node.span.end);
                    return Ok(true);
                }
            }
        }
    }
}
impl Context {
    pub(super) fn edn_nesting_budget(&self) -> usize {
        self.accounting
            .depths()
            .1
            .min(64)
            .saturating_sub(self.active_expressions)
    }
    pub(super) fn buffered_form_end(
        &self,
        text: &str,
        offset: usize,
        edn: bool,
    ) -> Result<(usize, bool), crate::parser::ParseError> {
        let readers = self.readers.borrow();
        let mut probe = BufferedProbe {
            parser: Parser::at_offset(text, offset).retaining_discards(),
            text,
            features: &readers.features,
            discarded: Vec::new(),
            next_discard: 0,
            parser_discarded: 0,
            needs_more: false,
        };
        if edn {
            probe.parser.limit_nesting(self.edn_nesting_budget());
        }
        if let Some(root) = probe.read_root()? {
            probe.next(&mut VecDeque::from([root]), false, true, 0)?;
        } else {
            // A line can contain only discards. Their conditional/prefix
            // operands may still require the next line before semantic reading.
            while let Some(&discard) = probe.discarded.get(probe.next_discard) {
                probe.next_discard += 1;
                probe.next(&mut VecDeque::from([discard]), true, true, 0)?;
            }
        }
        Ok((probe.parser.position(), probe.needs_more))
    }
}

struct Anonymous {
    unique: Option<u64>,
    names: HashMap<i64, RootedValue>,
    rest: Option<RootedValue>,
    max: usize,
}
enum Syntax<'a, 'src> {
    Nodes(&'a [Node]),
    Parser(&'a mut Parser<'src>),
}
impl Syntax<'_, '_> {
    fn nodes(&self) -> &[Node] {
        match self {
            Self::Nodes(nodes) => nodes,
            Self::Parser(parser) => parser.nodes(),
        }
    }
}
enum InputForm {
    Syntax(NodeId),
    Read(FormId),
}
#[derive(Default)]
struct Input {
    pending: VecDeque<InputForm>,
    top_level: bool,
}
impl Input {
    fn nodes(nodes: impl IntoIterator<Item = NodeId>) -> Self {
        Self {
            pending: nodes.into_iter().map(InputForm::Syntax).collect(),
            top_level: false,
        }
    }
}
struct Lowerer<'a, 'src> {
    context: &'a Context,
    source: &'a Source,
    syntax: Syntax<'a, 'src>,
    parser_discards: usize,
    features: HashSet<Name>,
    aliases: Option<HashMap<Name, Name>>,
    compiler_sets: bool,
    compiler_globals: HashMap<Name, Name>,
    edn: Option<Rc<EdnOptions>>,
    splices: HashSet<FormId>,
    discarded: Vec<NodeId>,
    next_discard: usize,
    suppress_arguments: usize,
    suppress_tags: usize,
    anonymous: Option<Anonymous>,
    gensyms: HashMap<Name, RootedValue>,
    limits: ReaderLimits,
    nodes: Vec<FormNode>,
    values: Vec<RootedValue>,
    spans: Vec<SourceSpan>,
    data_origin: Origin,
    supplied: HashMap<super::HeapId, Form>,
}
impl<'a, 'src> Lowerer<'a, 'src> {
    fn new(
        context: &'a Context,
        source: &'a Source,
        syntax: &'a [Node],
        discarded: &[NodeId],
        limits: ReaderLimits,
    ) -> Self {
        let mut discarded = discarded.to_vec();
        discarded.sort_by_key(|id| syntax[id.0].span.start);
        Self {
            context,
            source,
            syntax: Syntax::Nodes(syntax),
            parser_discards: 0,
            features: context.readers.borrow().features.clone(),
            aliases: None,
            compiler_sets: false,
            compiler_globals: HashMap::new(),
            edn: None,
            splices: HashSet::new(),
            discarded,
            next_discard: 0,
            suppress_arguments: 0,
            suppress_tags: 0,
            anonymous: None,
            gensyms: HashMap::new(),
            limits,
            nodes: Vec::new(),
            values: Vec::new(),
            spans: Vec::new(),
            data_origin: Origin::SuppliedData,
            supplied: HashMap::new(),
        }
    }
    fn reader_namespace(&self, alias: Option<&Name>) -> Option<Name> {
        if let (Some(aliases), Some(alias)) = (&self.aliases, alias) {
            aliases.get(alias).cloned()
        } else {
            self.context.reader_namespace(alias)
        }
    }
    fn syntax_quote_name(&self, name: &Name) -> Name {
        if let Some(resolved) = self.compiler_globals.get(name) {
            return resolved.clone();
        }
        if let (Some(aliases), Some(namespace)) = (&self.aliases, name.namespace()) {
            return aliases
                .get(&Name::from_utf16(None, namespace))
                .map(|ns| Name::from_utf16(Some(ns.name()), name.name()))
                .unwrap_or_else(|| name.clone());
        }
        let resolved = self.context.syntax_quote_name(name);
        if self.compiler_sets
            && resolved.namespace() == Some(Name::new(None, "clojure.core").name())
        {
            Name::from_utf16(Some(Name::new(None, "cljs.core").name()), resolved.name())
        } else {
            resolved
        }
    }
    fn error(&self, kind: LowerErrorKind, bytes: Range<usize>) -> ReadError {
        ReadError {
            kind: ReadErrorKind::Lower(kind),
            span: self
                .source
                .span(bytes)
                .unwrap_or_else(|_| self.source.span(0..0).unwrap()),
            related: None,
        }
    }
    fn resource_check(&self, span: Range<usize>) -> Result<(), ReadError> {
        self.context
            .checkpoint(super::PollPoint::Read)
            .and_then(|()| self.context.charge_allocation(1))
            .map_err(|e| self.error(LowerErrorKind::Resource(e), span))
    }
    fn required(
        &mut self,
        input: &mut Input,
        depth: usize,
        span: Range<usize>,
    ) -> Result<FormId, ReadError> {
        self.read_next(input, depth, false)?
            .ok_or_else(|| self.error(LowerErrorKind::MissingReaderForm, span))
    }
    fn children(&mut self, children: &[NodeId], depth: usize) -> Result<Vec<FormId>, ReadError> {
        let mut input = Input::nodes(children.iter().copied());
        let mut forms = Vec::new();
        while let Some(id) = self.read_next(&mut input, depth, true)? {
            forms.push(id);
        }
        Ok(forms)
    }
    // Keep mode-specific validation and its temporaries off the recursive
    // code-reader frames; the code profile still accepts its established depth.
    fn edn_depth(&self, depth: usize, span: Range<usize>) -> Result<(), ReadError> {
        if self.context.active_expressions.saturating_add(depth)
            >= self.context.accounting.depths().1.min(64)
        {
            return Err(self.error(
                LowerErrorKind::Resource(
                    self.context
                        .accounting
                        .fail(super::ResourceError::EvaluationDepthExceeded),
                ),
                span,
            ));
        }
        Ok(())
    }
    fn edn_tokens(&self, start: usize, end: usize) -> Result<(), ReadError> {
        self.context
            .accounting
            .work((end - start) as u64)
            .map_err(|e| self.error(LowerErrorKind::Resource(e), start..end))?;
        // Code comments #! are outside EDN. Scan only consumed syntax bytes,
        // preserving first-form-only reading and callback ordering.
        for token in crate::Tokenizer::new(&self.source.text()[start..end]) {
            if token.kind == crate::TokenKind::Comment
                && self.source.text()[start + token.span.start..start + token.span.end]
                    .starts_with("#!")
            {
                return Err(self.error(
                    LowerErrorKind::EdnSyntax("dispatch comment"),
                    start + token.span.start..start + token.span.end,
                ));
            }
        }
        Ok(())
    }
    fn edn_node(&self, node: &Node) -> Result<(), ReadError> {
        let span = node.span.clone();
        // @, ` and ~ are EDN constituent errors, not delimiters that
        // permit a successful partial token such as the `a` in a@b.
        if matches!(
            node.kind,
            NodeKind::Symbol
                | NodeKind::Keyword { .. }
                | NodeKind::Number(_)
                | NodeKind::Character(_)
                | NodeKind::SymbolicValue(_)
        ) && self.source.text()[span.end..].starts_with(['@', '`', '~'])
        {
            return Err(self.error(LowerErrorKind::EdnSyntax("invalid token constituent"), span));
        }
        let forbidden = match &node.kind {
            NodeKind::Prefix { .. } => Some("reader prefix"),
            NodeKind::Collection {
                kind: CollectionKind::AnonymousFn,
                ..
            } => Some("anonymous function"),
            NodeKind::Keyword { auto_resolve: true } => Some("auto-resolved keyword"),
            NodeKind::NamespacedMap {
                auto_resolve: true, ..
            } => Some("auto-resolved namespaced map"),
            NodeKind::Regex => Some("regex"),
            _ => None,
        };
        if let Some(feature) = forbidden {
            return Err(self.error(LowerErrorKind::EdnSyntax(feature), span));
        }
        Ok(())
    }
    fn read_next(
        &mut self,
        input: &mut Input,
        depth: usize,
        expand_splices: bool,
    ) -> Result<Option<FormId>, ReadError> {
        if self.edn.is_some() {
            self.edn_depth(depth, 0..0)?;
        }
        if depth >= 128 {
            return Err(self.error(LowerErrorKind::FormNestingLimit, 0..0));
        }
        loop {
            let next = match input.pending.pop_front() {
                Some(next) => next,
                None if input.top_level => {
                    let Syntax::Parser(parser) = &mut self.syntax else {
                        return Ok(None);
                    };
                    let start = parser.position();
                    let root = parser.read_form().map_err(|e| ReadError {
                        kind: ReadErrorKind::Syntax(e.kind),
                        span: self.source.span(e.span).expect("parser span"),
                        related: e
                            .related
                            .map(|s| self.source.span(s).expect("parser related span")),
                    })?;
                    let end = parser.position();
                    self.discarded
                        .extend_from_slice(&parser.discarded()[self.parser_discards..]);
                    self.parser_discards = parser.discarded().len();
                    self.discarded[self.next_discard..]
                        .sort_by_key(|id| parser.nodes()[id.0].span.start);
                    if self.edn.is_some() {
                        self.edn_tokens(start, end)?;
                    }
                    let Some(root) = root else { return Ok(None) };
                    InputForm::Syntax(root)
                }
                None => return Ok(None),
            };
            let id = match next {
                InputForm::Read(id) => return Ok(Some(id)),
                InputForm::Syntax(id) => {
                    let node = self
                        .syntax
                        .nodes()
                        .get(id.0)
                        .ok_or_else(|| self.error(LowerErrorKind::InvalidSyntax, 0..0))?;
                    let span = node.span.clone();
                    let conditional = match node.kind {
                        NodeKind::Conditional { splicing, branches } => Some((splicing, branches)),
                        _ => None,
                    };
                    if self
                        .discarded
                        .get(self.next_discard)
                        .is_some_and(|discard| {
                            self.syntax.nodes()[discard.0].span.start < span.start
                        })
                    {
                        input.pending.push_front(InputForm::Syntax(id));
                        self.discard_before_input(span.start, depth, input)?;
                        continue;
                    }
                    if self.edn.is_some() && conditional.is_some() {
                        return Err(
                            self.error(LowerErrorKind::EdnSyntax("reader conditional"), span)
                        );
                    }
                    if let Some((splicing, branches)) = conditional {
                        if self.aliases.is_some() {
                            return Err(self.error(
                                LowerErrorKind::Unsupported(UnsupportedSyntax::ReaderConditional),
                                span,
                            ));
                        }
                        let Some(id) = self.conditional(splicing, branches, span, depth)? else {
                            continue;
                        };
                        id
                    } else {
                        self.lower(id, depth, input)?
                    }
                }
            };
            if expand_splices && self.splices.contains(&id) {
                let span = self.spans[id.0].bytes();
                let elements = match self.nodes[id.0].kind.clone() {
                    FormKind::Collection {
                        kind: CollectionKind::Map,
                        children,
                    } => {
                        let mut entries = Vec::new();
                        for pair in children.as_chunks::<2>().0.iter() {
                            entries.push(self.collection(
                                CollectionKind::Vector,
                                pair.to_vec(),
                                span.clone(),
                                Origin::Source,
                            )?);
                        }
                        entries
                    }
                    FormKind::Collection { children, .. } => children.to_vec(),
                    _ => return Err(self.error(LowerErrorKind::InvalidSplice, span)),
                };
                for element in elements.into_iter().rev() {
                    input.pending.push_front(InputForm::Read(element));
                }
                continue;
            }
            return Ok(Some(id));
        }
    }
    fn conditional(
        &mut self,
        splicing: bool,
        branches: NodeId,
        span: Range<usize>,
        depth: usize,
    ) -> Result<Option<FormId>, ReadError> {
        self.resource_check(span.clone())?;
        self.discard_before(span.start, depth)?;
        let NodeKind::Collection { children, .. } = self.syntax.nodes()[branches.0].kind.clone()
        else {
            return Err(self.error(LowerErrorKind::InvalidSyntax, span));
        };
        let selected = children.as_chunks::<2>().0.iter().position(|pair| {
            let feature = &self.source.text()[self.syntax.nodes()[pair[0].0].span.clone()];
            feature == ":default" || self.features.contains(&name(&feature[1..]))
        });
        // Read in source order so explicit discards inherit the
        // same handler suppression as their enclosing branch.
        let mut chosen = None;
        for (index, pair) in children.as_chunks::<2>().0.iter().enumerate() {
            let suppress_gap = usize::from(selected.is_some_and(|selected| selected < index));
            self.suppress_tags += suppress_gap;
            self.suppress_arguments += suppress_gap;
            let gap = self.discard_before(self.syntax.nodes()[pair[0].0].span.start, depth + 1);
            self.suppress_arguments -= suppress_gap;
            self.suppress_tags -= suppress_gap;
            gap?;
            if Some(index) == selected {
                let mut branch = Input::nodes(children[index * 2 + 1..].iter().copied());
                chosen = self.read_next(&mut branch, depth + 1, false)?;
            } else {
                self.suppress_tags += 1;
                self.suppress_arguments += 1;
                let mark = self.nodes.len();
                let result = self.read_next(&mut Input::nodes([pair[1]]), depth + 1, false);
                self.suppress_tags -= 1;
                self.suppress_arguments -= 1;
                result?;
                self.nodes.truncate(mark);
                self.values.truncate(mark);
                self.spans.truncate(mark);
                self.splices.retain(|id| id.0 < mark);
            }
        }
        let suppress_trailing = usize::from(selected.is_some());
        self.suppress_tags += suppress_trailing;
        self.suppress_arguments += suppress_trailing;
        let trailing = self.discard_before(span.end, depth);
        self.suppress_arguments -= suppress_trailing;
        self.suppress_tags -= suppress_trailing;
        trailing?;
        let Some(id) = chosen else { return Ok(None) };
        if splicing
            && matches!(
                self.values[id.0].inspect(),
                ValueView::List(_)
                    | ValueView::Seq(_)
                    | ValueView::Vector(_)
                    | ValueView::Map(_)
                    | ValueView::Set(_)
                    | ValueView::Symbol(_)
            )
        {
            self.splices.insert(id);
        }
        Ok(Some(id))
    }
    fn collection_error(&self, error: super::CollectionError, span: Range<usize>) -> ReadError {
        self.error(
            match error {
                super::CollectionError::Resource(e) => LowerErrorKind::Resource(e),
                _ => LowerErrorKind::InvalidSyntax,
            },
            span,
        )
    }
    fn finish(self, id: FormId) -> Form {
        let storage = self
            .context
            .executable_unit(&self.values, &self.spans)
            .expect("same reader owner");
        Form {
            forms: Rc::new(Forms {
                nodes: self.nodes.into(),
                storage,
            }),
            id,
        }
    }
    fn add(
        &mut self,
        value: RootedValue,
        kind: FormKind,
        metadata: Option<FormId>,
        span: Range<usize>,
        origin: Origin,
    ) -> FormId {
        let id = FormId(self.nodes.len());
        self.nodes.push(FormNode {
            kind,
            metadata,
            compiler_payload: None,
            origin,
        });
        self.values.push(value);
        self.spans
            .push(self.source.span(span).expect("validated syntax span"));
        id
    }
    fn scalar(&mut self, value: RootedValue, span: Range<usize>, origin: Origin) -> FormId {
        self.add(value, FormKind::Scalar, None, span, origin)
    }
    fn symbol(&mut self, text: &str, span: Range<usize>, origin: Origin) -> FormId {
        let value = self.context.symbol(name(text));
        self.scalar(value, span, origin)
    }
    fn collection(
        &mut self,
        kind: CollectionKind,
        children: Vec<FormId>,
        span: Range<usize>,
        origin: Origin,
    ) -> Result<FormId, ReadError> {
        let values: Vec<_> = children
            .iter()
            .map(|id| self.values[id.0].clone())
            .collect();
        // Compiler analysis validates literal/quoted duplicates itself and must
        // retain equal executable operands in the source-ordered Form children.
        // Ordinary native reading keeps its strict data-duplicate check.
        if kind == CollectionKind::Map || (kind == CollectionKind::Set && !self.compiler_sets) {
            if kind == CollectionKind::Map && !children.len().is_multiple_of(2) {
                return Err(self.error(LowerErrorKind::InvalidSyntax, span));
            }
            let mut seen: HashMap<u64, Vec<usize>> = HashMap::new();
            let step = if kind == CollectionKind::Map { 2 } else { 1 };
            for i in (0..values.len()).step_by(step) {
                let bucket = seen.entry(values[i].value_hash()).or_default();
                if let Some(&first) = bucket.iter().find(|&&j| values[i].value_eq(&values[j])) {
                    let mut e = self.error(
                        if kind == CollectionKind::Map {
                            LowerErrorKind::DuplicateMapKey
                        } else {
                            LowerErrorKind::DuplicateSetMember
                        },
                        self.spans[children[i].0].bytes(),
                    );
                    e.related = Some(self.spans[children[first].0].clone());
                    return Err(e);
                }
                bucket.push(i);
            }
        }
        let value = match kind {
            CollectionKind::List => self.context.list(&values),
            CollectionKind::Vector => self.context.vector(&values),
            CollectionKind::Map => self.context.map(
                &values
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| (p[0].clone(), p[1].clone()))
                    .collect::<Vec<_>>(),
            ),
            CollectionKind::Set => self.context.set(&values),
            CollectionKind::AnonymousFn => unreachable!(),
        }
        .map_err(|error| self.collection_error(error, span.clone()))?;
        Ok(self.add(
            value,
            FormKind::Collection {
                kind,
                children: children.into(),
            },
            None,
            span,
            origin,
        ))
    }
    fn discard_before(&mut self, end: usize, depth: usize) -> Result<(), ReadError> {
        self.discard_before_input(end, depth, &mut Input::default())
    }
    fn discard_before_input(
        &mut self,
        end: usize,
        depth: usize,
        input: &mut Input,
    ) -> Result<(), ReadError> {
        while let Some(&id) = self.discarded.get(self.next_discard) {
            if self.syntax.nodes()[id.0].span.start >= end {
                break;
            }
            self.next_discard += 1;
            // Discards are read/validated, but Edamame does not include discarded
            // argument placeholders in the surrounding shorthand's parameter set.
            if !self.compiler_sets {
                self.suppress_arguments += 1;
            }
            let mark = self.nodes.len();
            input.pending.push_front(InputForm::Syntax(id));
            let result = self.required(input, depth, self.syntax.nodes()[id.0].span.clone());
            if !self.compiler_sets {
                self.suppress_arguments -= 1;
            }
            result?;
            self.nodes.truncate(mark);
            self.values.truncate(mark);
            self.spans.truncate(mark);
            self.splices.retain(|id| id.0 < mark);
        }
        Ok(())
    }
    fn data_form(
        &mut self,
        value: &RootedValue,
        span: Range<usize>,
        depth: usize,
    ) -> Result<FormId, ReadError> {
        let supplied = value
            .heap_id()
            .and_then(|id| self.supplied.get(&id))
            .cloned();
        let (span, origin) = supplied
            .as_ref()
            .map(|f| (f.span().bytes(), f.node().origin.clone()))
            .unwrap_or((span, self.data_origin.clone()));
        self.resource_check(span.clone())?;
        if self.edn.is_some() {
            self.edn_depth(depth, span.clone())?;
        }
        if depth >= 128 {
            return Err(self.error(LowerErrorKind::FormNestingLimit, span));
        }
        if self.edn.is_some()
            && !matches!(
                value.inspect(),
                ValueView::Nil
                    | ValueView::Boolean(_)
                    | ValueView::Integer(_)
                    | ValueView::Double(_)
                    | ValueView::Ratio { .. }
                    | ValueView::Character(_)
                    | ValueView::String(_)
                    | ValueView::Keyword(_)
                    | ValueView::Symbol(_)
                    | ValueView::List(_)
                    | ValueView::Seq(_)
                    | ValueView::Vector(_)
                    | ValueView::Map(_)
                    | ValueView::Set(_)
            )
        {
            return Err(self.error(LowerErrorKind::UnsupportedEdnValue, span));
        }
        let kind = match value.inspect() {
            ValueView::List(_) | ValueView::Seq(_) | ValueView::LazySeq => {
                Some(CollectionKind::List)
            }
            ValueView::Vector(_) => Some(CollectionKind::Vector),
            ValueView::Map(_) => Some(CollectionKind::Map),
            ValueView::Set(_) => Some(CollectionKind::Set),
            _ => None,
        };
        let kind = if let Some(kind) = kind {
            let values = if let Some(original) = &supplied {
                let FormKind::Collection { children, .. } = &original.node().kind else {
                    unreachable!()
                };
                self.context
                    .charge_allocation(children.len() as u64)
                    .map_err(|e| self.error(LowerErrorKind::Resource(e), span.clone()))?;
                children
                    .iter()
                    .map(|id| original.at(*id).unwrap().data())
                    .collect()
            } else if kind == CollectionKind::Map {
                value
                    .entries()
                    .map_err(|e| self.collection_error(e, span.clone()))?
                    .into_iter()
                    .flat_map(|(k, v)| [k, v])
                    .collect()
            } else {
                value
                    .elements()
                    .map_err(|e| self.collection_error(e, span.clone()))?
            };
            let children = values
                .iter()
                .map(|v| self.data_form(v, span.clone(), depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            FormKind::Collection {
                kind,
                children: children.into(),
            }
        } else {
            FormKind::Scalar
        };
        let metadata = value
            .metadata()
            .map(|m| self.data_form(&m, span.clone(), depth + 1))
            .transpose()?;
        let payload = if let Some(original) = supplied
            .as_ref()
            .and_then(|f| f.node().compiler_payload.and_then(|id| f.at(id)))
        {
            Some(self.data_form(&original.data(), original.span().bytes(), depth + 1)?)
        } else {
            None
        };
        let id = self.add(value.clone(), kind, metadata, span, origin);
        self.nodes[id.0].compiler_payload = payload;
        Ok(id)
    }
    // Scalar validation allocates temporary reader values; keep them off the
    // recursive collection frame so the established 128-level bound stays usable.
    fn source_scalar(&mut self, node: &Node) -> Result<Option<RootedValue>, ReadError> {
        let span = node.span.clone();
        let text = &self.source.text()[span.clone()];
        let scalar = match &node.kind {
            NodeKind::Nil => Some(self.context.nil()),
            NodeKind::Boolean(v) => Some(self.context.boolean(*v)),
            NodeKind::Symbol => Some(self.context.symbol(name(text))),
            NodeKind::Keyword {
                auto_resolve: false,
            } => Some(
                self.context
                    .keyword(name(text.strip_prefix(':').ok_or_else(|| {
                        self.error(LowerErrorKind::InvalidSyntax, span.clone())
                    })?)),
            ),
            NodeKind::Keyword { auto_resolve: true } => {
                let raw = name(
                    text.strip_prefix("::")
                        .ok_or_else(|| self.error(LowerErrorKind::InvalidSyntax, span.clone()))?,
                );
                let alias = raw.namespace().map(|alias| Name::from_utf16(None, alias));
                let namespace = self.reader_namespace(alias.as_ref()).ok_or_else(|| {
                    self.error(
                        LowerErrorKind::MissingAlias(Box::new(alias.unwrap())),
                        span.clone(),
                    )
                })?;
                Some(
                    self.context
                        .keyword(Name::from_utf16(Some(namespace.name()), raw.name())),
                )
            }
            NodeKind::Number(kind) if self.suppress_tags != 0 => {
                Some(self.number(text, *kind, span.clone()).unwrap_or_else(|_| {
                    if *kind == NumberKind::BigInteger
                        && let Some(n) =
                            integer(&text[..text.len() - 1]).and_then(|n| i64::try_from(n).ok())
                    {
                        return self.context.integer(n);
                    }
                    self.context
                        .symbol(Name::new(Some("reader.skipped/number"), text))
                }))
            }
            NodeKind::Regex if self.suppress_tags != 0 => Some(self.context.symbol(Name::new(
                Some("reader.skipped/regex"),
                &span.start.to_string(),
            ))),
            NodeKind::Regex if self.context.readers.borrow().compiler => Some(
                self.context.compiler_literal(
                    super::CompilerLiteralKind::Regex,
                    &self
                        .context
                        .string(text[2..text.len() - 1].encode_utf16().collect::<Vec<_>>()),
                ),
            ),
            NodeKind::Number(kind) => Some(self.number(text, *kind, span.clone())?),
            NodeKind::String(value) => {
                if let crate::parser::StringValue::Source(bytes) = value
                    && self.source.text().get(bytes.clone()).is_none()
                {
                    return Err(self.error(LowerErrorKind::InvalidSyntax, span));
                }
                Some(self.context.string(value.to_utf16(self.source.text())))
            }
            NodeKind::Character(v) => Some(self.context.character(*v as u16)),
            NodeKind::SymbolicValue(v) => Some(self.context.double(match v {
                SymbolicValue::Infinity => f64::INFINITY,
                SymbolicValue::NegativeInfinity => f64::NEG_INFINITY,
                SymbolicValue::NaN => f64::NAN,
            })),
            _ => None,
        };
        Ok(scalar)
    }
    fn lower(&mut self, id: NodeId, depth: usize, input: &mut Input) -> Result<FormId, ReadError> {
        let node = self
            .syntax
            .nodes()
            .get(id.0)
            .ok_or_else(|| self.error(LowerErrorKind::InvalidSyntax, 0..0))?
            .clone();
        let span = node.span.clone();
        self.resource_check(span.clone())?;
        if depth >= 128 || self.source.text().get(span.clone()).is_none() {
            return Err(self.error(LowerErrorKind::InvalidSyntax, span));
        }
        self.discard_before(span.start, depth)?;
        let origin = Origin::Source;
        if self.edn.is_some() {
            self.edn_node(&node)?;
        }
        let scalar = self.source_scalar(&node)?;
        if let Some(value) = scalar {
            self.discard_before(span.end, depth + 1)?;
            return Ok(self.scalar(value, span, origin));
        }
        match &node.kind {
            NodeKind::Collection {
                kind: CollectionKind::AnonymousFn,
                children,
            } => self.anonymous(children, span, depth),
            NodeKind::Collection { kind, children } => {
                let lowered = self.children(children, depth + 1)?;
                self.discard_before(span.end, depth + 1)?;
                self.collection(*kind, lowered, span, origin)
            }
            NodeKind::Prefix {
                kind:
                    PrefixKind::Quote
                    | PrefixKind::Deref
                    | PrefixKind::VarQuote
                    | PrefixKind::Unquote
                    | PrefixKind::UnquoteSplicing,
                form,
            } => {
                let (head, generated) = match node.kind {
                    NodeKind::Prefix {
                        kind: PrefixKind::Quote,
                        ..
                    } => ("quote", Origin::Quote),
                    NodeKind::Prefix {
                        kind: PrefixKind::Deref,
                        ..
                    } => ("clojure.core/deref", Origin::Deref),
                    NodeKind::Prefix {
                        kind: PrefixKind::Unquote,
                        ..
                    } => ("clojure.core/unquote", Origin::Unquote),
                    NodeKind::Prefix {
                        kind: PrefixKind::UnquoteSplicing,
                        ..
                    } => ("clojure.core/unquote-splicing", Origin::UnquoteSplicing),
                    _ => ("var", Origin::VarQuote),
                };
                let head = self.symbol(head, span.clone(), generated.clone());
                input.pending.push_front(InputForm::Syntax(*form));
                let body = self.required(input, depth + 1, span.clone())?;
                let span = span.start..span.end.max(self.spans[body.0].bytes().end);
                self.collection(CollectionKind::List, vec![head, body], span, generated)
            }
            NodeKind::Prefix {
                kind: PrefixKind::SyntaxQuote,
                form,
            } => {
                // Read nested backticks before installing this quote's gensym table.
                input.pending.push_front(InputForm::Syntax(*form));
                let body = self.required(input, depth + 1, span.clone())?;
                let previous = std::mem::take(&mut self.gensyms);
                let result = self.syntax_quote(body, depth + 1);
                self.gensyms = previous;
                result
            }
            NodeKind::Metadata { metadata, target } => {
                input.pending.push_front(InputForm::Syntax(*target));
                input.pending.push_front(InputForm::Syntax(*metadata));
                let metadata = self.required(input, depth + 1, span.clone())?;
                let target = self.required(input, depth + 1, span.clone())?;
                let span = span.start..span.end.max(self.spans[target.0].bytes().end);
                let result = self.metadata(metadata, target, span)?;
                if self.splices.contains(&target) {
                    self.splices.insert(result);
                }
                Ok(result)
            }
            NodeKind::NamespacedMap {
                auto_resolve,
                namespace,
                map,
            } => self.namespaced_map(*auto_resolve, namespace.clone(), *map, span, depth),
            NodeKind::Tagged { tag, form } => self.tagged(*tag, *form, span, depth, input),
            NodeKind::Argument { rest, index } => self.argument(*rest, *index, span),
            kind => {
                let feature = match kind {
                    NodeKind::Regex => UnsupportedSyntax::Regex,
                    _ => return Err(self.error(LowerErrorKind::InvalidSyntax, span)),
                };
                Err(self.error(LowerErrorKind::Unsupported(feature), span))
            }
        }
    }
    fn namespaced_map(
        &mut self,
        auto_resolve: bool,
        namespace: Range<usize>,
        map: NodeId,
        span: Range<usize>,
        depth: usize,
    ) -> Result<FormId, ReadError> {
        let spelling = &self.source.text()[namespace.clone()];
        let namespace = if auto_resolve {
            let alias = (!spelling.is_empty()).then(|| Name::new(None, spelling));
            self.reader_namespace(alias.as_ref()).ok_or_else(|| {
                self.error(
                    LowerErrorKind::MissingAlias(Box::new(alias.unwrap())),
                    namespace.clone(),
                )
            })?
        } else {
            Name::new(None, spelling)
        };
        let NodeKind::Collection {
            kind: CollectionKind::Map,
            children,
        } = self.syntax.nodes()[map.0].kind.clone()
        else {
            return Err(self.error(LowerErrorKind::InvalidSyntax, span));
        };
        let children = self.children(&children, depth + 1)?;
        self.discard_before(span.end, depth + 1)?;
        // SCI rejects raw duplicates first. Qualification collisions then
        // follow map assoc semantics: the later source value wins.
        self.collection(
            CollectionKind::Map,
            children.clone(),
            span.clone(),
            Origin::Source,
        )?;
        let mut rewritten: Vec<FormId> = Vec::with_capacity(children.len());
        for pair in children.as_chunks::<2>().0.iter() {
            let key = pair[0];
            let (raw, keyword) = match self.values[key.0].inspect() {
                ValueView::Symbol(name) => (Some(name), false),
                ValueView::Keyword(name) => (Some(name), true),
                _ => (None, false),
            };
            let key = if let Some(raw) = raw {
                let qualifier = match raw.namespace() {
                    None => Some(namespace.name()),
                    Some([95]) => None,
                    other => other,
                };
                let name = Name::from_utf16(qualifier, raw.name());
                let mut value = if keyword {
                    self.context.keyword(name)
                } else {
                    self.context.symbol(name)
                };
                if let Some(metadata) = self.values[key.0].metadata() {
                    value = self
                        .context
                        .with_meta(&value, &metadata)
                        .map_err(|e| self.collection_error(e, span.clone()))?;
                }
                self.add(
                    value,
                    FormKind::Scalar,
                    self.nodes[key.0].metadata,
                    self.spans[key.0].bytes(),
                    Origin::Source,
                )
            } else {
                key
            };
            if let Some(previous) = rewritten
                .as_chunks::<2>()
                .0
                .iter()
                .position(|p| self.values[p[0].0].value_eq(&self.values[key.0]))
            {
                rewritten[previous * 2] = key;
                rewritten[previous * 2 + 1] = pair[1];
            } else {
                rewritten.extend([key, pair[1]]);
            }
        }
        self.collection(CollectionKind::Map, rewritten, span, Origin::Source)
    }
    fn tagged(
        &mut self,
        tag: NodeId,
        form: NodeId,
        span: Range<usize>,
        depth: usize,
        input: &mut Input,
    ) -> Result<FormId, ReadError> {
        let tag_span = self.syntax.nodes()[tag.0].span.clone();
        let tag_text = &self.source.text()[tag_span.clone()];
        if self.edn.is_some()
            && (tag_span.start != span.start + 1
                || !tag_text.chars().next().is_some_and(char::is_alphabetic))
        {
            return Err(self.error(
                LowerErrorKind::EdnSyntax("tag must immediately follow # and begin with a letter"),
                span,
            ));
        }
        let tag = name(tag_text);
        input.pending.push_front(InputForm::Syntax(form));
        let data = self.required(input, depth + 1, span.clone())?;
        let span = span.start..span.end.max(self.spans[data.0].bytes().end);
        if self.suppress_tags != 0 {
            // This marker cannot be spelled by the syntax parser (its
            // namespace contains a slash and its name is empty), so a
            // suppressed tag cannot collide with ordinary source data.
            let marker = self.scalar(
                self.context
                    .symbol(Name::new(Some("reader.skipped/tag"), "")),
                span.clone(),
                Origin::Source,
            );
            let tag = self.scalar(self.context.symbol(tag), span.clone(), Origin::Source);
            return self.collection(
                CollectionKind::List,
                vec![marker, tag, data],
                span,
                Origin::Source,
            );
        }
        if self.edn.is_none()
            && self.context.readers.borrow().compiler
            && !self.context.readers.borrow().tags.contains_key(&tag)
        {
            use super::CompilerLiteralKind as K;
            let tag_name = String::from_utf16_lossy(tag.name());
            if tag.namespace().is_none()
                && matches!(tag_name.as_str(), "inst" | "uuid" | "js" | "queue")
            {
                let value = self.values[data.0].clone();
                let kind = match (tag_name.as_str(), value.inspect()) {
                    ("inst", ValueView::String(ref text)) => {
                        let text = String::from_utf16(text).map_err(|_| {
                            self.error(
                                LowerErrorKind::InvalidCompilerLiteral(
                                    "invalid built-in tagged literal",
                                ),
                                span.clone(),
                            )
                        })?;
                        let millis = crate::reader_literals::instant(&text).ok_or_else(|| {
                            self.error(
                                LowerErrorKind::InvalidCompilerLiteral("invalid #inst timestamp"),
                                span.clone(),
                            )
                        })?;
                        return Ok(self.scalar(
                            self.context
                                .compiler_literal(K::Instant, &self.context.double(millis as f64)),
                            span,
                            Origin::Source,
                        ));
                    }
                    ("uuid", ValueView::String(ref text)) => {
                        let text = String::from_utf16(text).map_err(|_| {
                            self.error(
                                LowerErrorKind::InvalidCompilerLiteral(
                                    "invalid built-in tagged literal",
                                ),
                                span.clone(),
                            )
                        })?;
                        let uuid = crate::reader_literals::uuid(&text).ok_or_else(|| {
                            self.error(
                                LowerErrorKind::InvalidCompilerLiteral(
                                    "invalid #uuid representation",
                                ),
                                span.clone(),
                            )
                        })?;
                        return Ok(self.scalar(
                            self.context.compiler_literal(
                                K::Uuid,
                                &self.context.string(uuid.encode_utf16().collect::<Vec<_>>()),
                            ),
                            span,
                            Origin::Source,
                        ));
                    }
                    ("js", ValueView::Vector(_)) => K::JsArray,
                    ("js", ValueView::Map(_)) => {
                        for (key, _) in value
                            .entries()
                            .map_err(|e| self.collection_error(e, span.clone()))?
                        {
                            if !matches!(key.inspect(), ValueView::String(_))
                                && !matches!(key.inspect(), ValueView::Keyword(ref n) if n.namespace().is_none())
                            {
                                return Err(self.error(
                                    LowerErrorKind::InvalidCompilerLiteral(
                                        "invalid built-in tagged literal",
                                    ),
                                    span,
                                ));
                            }
                        }
                        K::JsObject
                    }
                    ("queue", ValueView::Vector(_)) => {
                        let empty = self.symbol(
                            "cljs.core.PersistentQueue.EMPTY",
                            span.clone(),
                            Origin::Source,
                        );
                        return self.quote_call("cljs.core/into", vec![empty, data], span);
                    }
                    _ => {
                        return Err(self.error(
                            LowerErrorKind::InvalidCompilerLiteral(
                                "invalid built-in tagged literal",
                            ),
                            span,
                        ));
                    }
                };
                let id = self.scalar(
                    self.context.compiler_literal(kind, &value),
                    span,
                    Origin::Source,
                );
                self.nodes[id.0].compiler_payload = Some(data);
                return Ok(id);
            }
        }
        let (handler, fallback) = if let Some(options) = &self.edn {
            match options.readers.get(&tag) {
                Some(handler) if !matches!(handler.inspect(), ValueView::Nil) => {
                    (handler.clone(), false)
                }
                _ => (
                    options.default.clone().ok_or_else(|| {
                        self.error(
                            LowerErrorKind::UnknownTag(Box::new(tag.clone())),
                            span.clone(),
                        )
                    })?,
                    true,
                ),
            }
        } else {
            let readers = self.context.readers.borrow();
            match readers.tags.get(&tag) {
                Some(handler) => (self.context.native_function(handler), false),
                None => (
                    self.context.native_function(readers.default.ok_or_else(|| {
                        self.error(
                            LowerErrorKind::UnknownTag(Box::new(tag.clone())),
                            span.clone(),
                        )
                    })?),
                    true,
                ),
            }
        };
        let arguments = if fallback {
            vec![
                self.context.symbol(tag.clone()),
                self.values[data.0].clone(),
            ]
        } else {
            vec![self.values[data.0].clone()]
        };
        let mut context = self.context.reader_context();
        if self.edn.is_some() {
            // Carry the lowerer's outstanding traversal frames into callbacks
            // so a nested EDN read cannot restart an independent depth budget.
            if context.active_expressions.saturating_add(depth)
                >= context.accounting.depths().1.min(64)
            {
                return Err(self.error(
                    LowerErrorKind::Resource(
                        context
                            .accounting
                            .fail(super::ResourceError::EvaluationDepthExceeded),
                    ),
                    span,
                ));
            }
            context.active_expressions += depth;
        }
        let result = if self.edn.is_none() {
            let super::Value::Native(definition) = handler.value() else {
                unreachable!()
            };
            context.invoke_native(definition, &arguments)
        } else {
            context
                .invoke(&handler, &arguments)
                .map_err(super::NativeError::from)
        };
        let value = result.map_err(|error| {
            self.error(
                LowerErrorKind::DataReader {
                    tag: Box::new(tag.clone()),
                    error: Box::new(error),
                },
                span.clone(),
            )
        })?;
        let previous = std::mem::replace(&mut self.data_origin, Origin::DataReader(Rc::new(tag)));
        let preserve_splice =
            self.splices.contains(&data) && value.heap_id() == self.values[data.0].heap_id();
        let result = self.data_form(&value, span, depth + 1);
        self.data_origin = previous;
        if preserve_splice && let Ok(id) = result {
            self.splices.insert(id);
        }
        result
    }
    fn quote_call(
        &mut self,
        name: &str,
        args: Vec<FormId>,
        span: Range<usize>,
    ) -> Result<FormId, ReadError> {
        self.resource_check(span.clone())?;
        let head = self.symbol(name, span.clone(), Origin::SyntaxQuote);
        let mut children = vec![head];
        children.extend(args);
        self.collection(CollectionKind::List, children, span, Origin::SyntaxQuote)
    }
    fn unquote(&self, id: FormId, splice: bool) -> Option<Option<FormId>> {
        let FormKind::Collection {
            kind: CollectionKind::List,
            children,
        } = &self.nodes[id.0].kind
        else {
            return None;
        };
        let head = children.first()?;
        let name = if splice {
            "unquote-splicing"
        } else {
            "unquote"
        };
        (self.values[head.0].inspect() == ValueView::Symbol(Name::new(Some("clojure.core"), name)))
            .then(|| children.get(1).copied())
    }
    fn quote_operand(&mut self, operand: Option<FormId>, span: Range<usize>) -> FormId {
        operand.unwrap_or_else(|| self.scalar(self.context.nil(), span, Origin::SyntaxQuote))
    }
    fn syntax_quote(&mut self, id: FormId, depth: usize) -> Result<FormId, ReadError> {
        let span = self.spans[id.0].bytes();
        self.resource_check(span.clone())?;
        if depth >= 128 {
            return Err(self.error(LowerErrorKind::FormNestingLimit, span));
        }
        let result = if let Some(operand) = self.unquote(id, false) {
            self.quote_operand(operand, span.clone())
        } else if self.unquote(id, true).is_some() {
            return Err(self.error(LowerErrorKind::InvalidSplice, span));
        } else if let ValueView::Symbol(mut name) = self.values[id.0].inspect() {
            let special = [
                "&",
                ".",
                "case*",
                "catch",
                "def",
                "deftype*",
                "do",
                "finally",
                "fn*",
                "if",
                "let*",
                "letfn*",
                "loop*",
                "monitor-enter",
                "monitor-exit",
                "new",
                "quote",
                "recur",
                "reify*",
                "set!",
                "throw",
                "try",
                "var",
            ]
            .iter()
            .any(|text| name == Name::new(None, text))
                || name == Name::new(Some("clojure.core"), "import*");
            let symbol = if name.name().last() == Some(&u16::from(b'#')) {
                if let Some(value) = self.gensyms.get(&name) {
                    value.clone()
                } else {
                    // Process-wide freshness also covers independent owners/readers.
                    static NEXT: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(1);
                    let number = NEXT
                        .try_update(
                            std::sync::atomic::Ordering::Relaxed,
                            std::sync::atomic::Ordering::Relaxed,
                            |n| n.checked_add(1),
                        )
                        .expect("syntax quote gensym identities exhausted");
                    let mut text = name.name()[..name.name().len() - 1].to_vec();
                    text.extend(format!("__{number}__auto__").encode_utf16());
                    let value = self.context.symbol(Name::from_utf16(None, &text));
                    self.gensyms.insert(name, value.clone());
                    value
                }
            } else {
                if !special {
                    name = self.syntax_quote_name(&name);
                }
                self.context.symbol(name)
            };
            let symbol = self.scalar(symbol, span.clone(), Origin::SyntaxQuote);
            self.quote_call("quote", vec![symbol], span.clone())?
        } else if let FormKind::Collection { kind, children } = self.nodes[id.0].kind.clone() {
            if kind == CollectionKind::List && children.is_empty() {
                self.quote_call("clojure.core/list", vec![], span.clone())?
            } else {
                let mut pieces = Vec::with_capacity(children.len());
                for child in children.iter().copied() {
                    let child_span = self.spans[child.0].bytes();
                    if let Some(operand) = self.unquote(child, true) {
                        pieces.push(self.quote_operand(operand, child_span));
                    } else {
                        let value = if let Some(operand) = self.unquote(child, false) {
                            self.quote_operand(operand, child_span.clone())
                        } else {
                            self.syntax_quote(child, depth + 1)?
                        };
                        pieces.push(self.quote_call(
                            "clojure.core/list",
                            vec![value],
                            child_span,
                        )?);
                    }
                }
                let joined = self.quote_call("clojure.core/concat", pieces, span.clone())?;
                let seq = self.quote_call("clojure.core/seq", vec![joined], span.clone())?;
                let sequence = self.quote_call("clojure.core/sequence", vec![seq], span.clone())?;
                match kind {
                    CollectionKind::List => sequence,
                    CollectionKind::Vector => {
                        self.quote_call("clojure.core/vec", vec![sequence], span.clone())?
                    }
                    CollectionKind::Map | CollectionKind::Set => {
                        let constructor = if kind == CollectionKind::Set {
                            "clojure.core/hash-set"
                        } else if children.len() < 32 {
                            "clojure.core/array-map"
                        } else {
                            "clojure.core/hash-map"
                        };
                        let constructor =
                            self.symbol(constructor, span.clone(), Origin::SyntaxQuote);
                        self.quote_call(
                            "clojure.core/apply",
                            vec![constructor, sequence],
                            span.clone(),
                        )?
                    }
                    CollectionKind::AnonymousFn => unreachable!(),
                }
            }
        } else {
            id
        };
        if let Some(metadata) = self.nodes[id.0].metadata {
            let entries = self.values[metadata.0]
                .entries()
                .map_err(|e| self.collection_error(e, span.clone()))?;
            let has_user_metadata = entries.iter().any(|(key, _)| {
                !["line", "column", "end-line", "end-column"]
                    .iter()
                    .any(|name| key.inspect() == ValueView::Keyword(Name::new(None, name)))
            });
            if has_user_metadata {
                let metadata = if matches!(
                    self.nodes[id.0].kind,
                    FormKind::Collection {
                        kind: CollectionKind::List,
                        ..
                    }
                ) {
                    let FormKind::Collection { children, .. } = &self.nodes[metadata.0].kind else {
                        unreachable!()
                    };
                    let mut children = children.to_vec();
                    let (line, column) = self.source.line_column(span.start).unwrap();
                    for (key, value) in [("line", line), ("column", column)] {
                        if entries
                            .iter()
                            .any(|(k, _)| k.inspect() == ValueView::Keyword(Name::new(None, key)))
                        {
                            continue;
                        }
                        let key = self.scalar(
                            self.context.keyword(Name::new(None, key)),
                            span.clone(),
                            Origin::SyntaxQuote,
                        );
                        let value = self.scalar(
                            self.context.integer(value as i64),
                            span.clone(),
                            Origin::SyntaxQuote,
                        );
                        children.extend([key, value]);
                    }
                    self.collection(
                        CollectionKind::Map,
                        children,
                        span.clone(),
                        Origin::SyntaxQuote,
                    )?
                } else {
                    metadata
                };
                let metadata = self.syntax_quote(metadata, depth + 1)?;
                return self.quote_call("clojure.core/with-meta", vec![result, metadata], span);
            }
        }
        Ok(result)
    }
    fn number(
        &self,
        text: &str,
        kind: NumberKind,
        span: Range<usize>,
    ) -> Result<RootedValue, ReadError> {
        let overflow = || self.error(LowerErrorKind::NumericOverflow, span.clone());
        if (self.compiler_sets || self.context.readers.borrow().compiler)
            && kind == NumberKind::Ratio
        {
            let (n, d) = text.split_once('/').ok_or_else(overflow)?;
            if let Some(exact) = crate::reader_literals::integral_ratio_text(n, d) {
                return Ok(if let Ok(n) = exact.parse::<i64>() {
                    self.context.integer(n)
                } else {
                    self.context.compiler_literal(
                        super::CompilerLiteralKind::BigInteger,
                        &self
                            .context
                            .string(exact.encode_utf16().collect::<Vec<_>>()),
                    )
                });
            }
        }
        if (self.compiler_sets || self.context.readers.borrow().compiler)
            && kind != NumberKind::Ratio
        {
            if matches!(kind, NumberKind::Integer | NumberKind::BigInteger)
                && let Some(n) = integer(if kind == NumberKind::BigInteger {
                    text.strip_suffix('N').unwrap()
                } else {
                    text
                })
                .and_then(|n| i64::try_from(n).ok())
            {
                return Ok(self.context.integer(n));
            }
            if matches!(kind, NumberKind::Integer | NumberKind::BigInteger) {
                let text = if kind == NumberKind::BigInteger {
                    text.strip_suffix('N').unwrap()
                } else {
                    text
                };
                let exact = crate::reader_literals::integer_key(text);
                return Ok(self.context.compiler_literal(
                    super::CompilerLiteralKind::BigInteger,
                    &self
                        .context
                        .string(exact.encode_utf16().collect::<Vec<_>>()),
                ));
            }
            return crate::analyzer::number(text, kind)
                .map(|v| self.context.double(v))
                .ok_or_else(overflow);
        }
        match kind {
            NumberKind::BigInteger => Err(self.error(
                LowerErrorKind::Unsupported(UnsupportedSyntax::BigInteger),
                span,
            )),
            NumberKind::Decimal => Err(self.error(
                LowerErrorKind::Unsupported(UnsupportedSyntax::Decimal),
                span,
            )),
            NumberKind::Integer => integer(text)
                .and_then(|n| i64::try_from(n).ok())
                .map(|n| self.context.integer(n))
                .ok_or_else(overflow),
            NumberKind::Float => {
                let v = text.parse::<f64>().map_err(|_| overflow())?;
                if v.is_infinite() {
                    return Err(overflow());
                }
                Ok(self.context.double(v))
            }
            NumberKind::Ratio => {
                let (n, d) = text.split_once('/').ok_or_else(overflow)?;
                let n = n.parse::<i128>().map_err(|_| overflow())?;
                let d = d.parse::<i128>().map_err(|_| overflow())?;
                if d <= 0 {
                    return Err(overflow());
                }
                let (mut a, mut b) = (n.unsigned_abs(), d as u128);
                while b != 0 {
                    (a, b) = (b, a % b);
                }
                let gcd = i128::try_from(a).map_err(|_| overflow())?;
                let n = i64::try_from(n / gcd).map_err(|_| overflow())?;
                let d = i64::try_from(d / gcd).map_err(|_| overflow())?;
                self.context.ratio(n, d).map_err(|_| overflow())
            }
        }
    }
    fn metadata(
        &mut self,
        metadata: FormId,
        target: FormId,
        span: Range<usize>,
    ) -> Result<FormId, ReadError> {
        let meta_span = self.spans[metadata.0].bytes();
        let metadata = match self.values[metadata.0].inspect() {
            ValueView::Keyword(_) => {
                let value = self.scalar(
                    self.context.boolean(true),
                    meta_span.clone(),
                    Origin::Metadata,
                );
                self.collection(
                    CollectionKind::Map,
                    vec![metadata, value],
                    meta_span,
                    Origin::Metadata,
                )?
            }
            ValueView::Symbol(_) | ValueView::String(_) => {
                let tag = self.scalar(
                    self.context.keyword(Name::new(None, "tag")),
                    meta_span.clone(),
                    Origin::Metadata,
                );
                self.collection(
                    CollectionKind::Map,
                    vec![tag, metadata],
                    meta_span,
                    Origin::Metadata,
                )?
            }
            ValueView::Map(_) => metadata,
            _ => return Err(self.error(LowerErrorKind::InvalidMetadata, meta_span)),
        };
        let metadata = if let Some(previous) = self.nodes[target.0].metadata {
            let FormKind::Collection { children: old, .. } = &self.nodes[previous.0].kind else {
                unreachable!()
            };
            let FormKind::Collection { children: new, .. } = &self.nodes[metadata.0].kind else {
                unreachable!()
            };
            let mut children = old.to_vec();
            for pair in new.as_chunks::<2>().0 {
                if let Some(i) = children
                    .iter()
                    .step_by(2)
                    .position(|id| self.values[id.0].value_eq(&self.values[pair[0].0]))
                {
                    children[i * 2 + 1] = pair[1];
                } else {
                    children.extend_from_slice(pair);
                }
            }
            self.collection(
                CollectionKind::Map,
                children,
                span.clone(),
                Origin::Metadata,
            )?
        } else {
            metadata
        };
        let value = self
            .context
            .with_meta(&self.values[target.0], &self.values[metadata.0])
            .map_err(|failure| {
                if let super::CollectionError::Resource(error) = failure {
                    return self.error(LowerErrorKind::Resource(error), span.clone());
                }
                let mut error = self.error(
                    LowerErrorKind::InvalidMetadataTarget,
                    self.spans[target.0].bytes(),
                );
                error.related = Some(self.source.span(span.clone()).unwrap());
                error
            })?;
        Ok(self.add(
            value,
            self.nodes[target.0].kind.clone(),
            Some(metadata),
            span,
            Origin::Metadata,
        ))
    }
    fn anonymous(
        &mut self,
        children: &[NodeId],
        span: Range<usize>,
        depth: usize,
    ) -> Result<FormId, ReadError> {
        if self.anonymous.is_some() {
            return Err(self.error(LowerErrorKind::InvalidSyntax, span));
        }
        static NEXT_ANONYMOUS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        self.anonymous = Some(Anonymous {
            unique: self
                .context
                .readers
                .borrow()
                .compiler
                .then(|| NEXT_ANONYMOUS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
            names: HashMap::new(),
            rest: None,
            max: 0,
        });
        // Discarding an entire shorthand still expands/validates that function.
        // Only placeholders individually discarded inside it are suppressed.
        let outer_suppression = std::mem::replace(&mut self.suppress_arguments, 0);
        let body = (|| {
            let body = self.children(children, depth + 1)?;
            self.discard_before(span.end, depth + 1)?;
            Ok(body)
        })();
        self.suppress_arguments = outer_suppression;
        let body: Vec<FormId> = body?;
        let args = self.anonymous.take().unwrap();
        let mut parameters = Vec::new();
        for index in 1..=args.max {
            let value = args.names.get(&(index as i64)).cloned().unwrap_or_else(|| {
                self.context.symbol(Name::new(
                    None,
                    &args
                        .unique
                        .map_or_else(|| format!("%{index}#"), |id| format!("p{index}__{id}#")),
                ))
            });
            parameters.push(self.scalar(value, span.clone(), Origin::AnonymousArgument));
        }
        if let Some(rest) = args.rest {
            parameters.push(self.symbol("&", span.clone(), Origin::AnonymousArgument));
            parameters.push(self.scalar(rest, span.clone(), Origin::AnonymousArgument));
        }
        let head = self.symbol("fn*", span.clone(), Origin::AnonymousFunction);
        let parameters = self.collection(
            CollectionKind::Vector,
            parameters,
            span.clone(),
            Origin::AnonymousFunction,
        )?;
        let body = self.collection(
            CollectionKind::List,
            body,
            span.clone(),
            Origin::AnonymousFunction,
        )?;
        self.collection(
            CollectionKind::List,
            vec![head, parameters, body],
            span,
            Origin::AnonymousFunction,
        )
    }
    fn argument(
        &mut self,
        rest: bool,
        index: Option<NodeId>,
        span: Range<usize>,
    ) -> Result<FormId, ReadError> {
        let text = &self.source.text()[span.clone()];
        if self.suppress_arguments != 0 {
            return Ok(self.symbol(text, span, Origin::Source));
        }
        // The pinned SCI reader treats slash spellings as ordinary symbols,
        // including %1/1; its parameter registration uses decimal integer text.
        if !self.compiler_sets
            && index.is_some_and(|id| {
                matches!(
                    self.syntax.nodes().get(id.0).map(|n| &n.kind),
                    Some(NodeKind::Number(NumberKind::Ratio))
                )
            })
        {
            return Ok(self.symbol(text, span, Origin::Source));
        }
        let n = if rest {
            0
        } else if index.is_none() {
            1
        } else {
            text.strip_prefix('%')
                .and_then(|s| {
                    if self.compiler_sets {
                        integer(s).and_then(|n| i64::try_from(n).ok()).or_else(|| {
                            let (a, b) = s.split_once('/')?;
                            let a: i64 = a.parse().ok()?;
                            let b: i64 = b.parse().ok()?;
                            (b != 0 && a % b == 0).then(|| a / b)
                        })
                    } else {
                        s.parse::<i32>().ok().map(i64::from)
                    }
                })
                .ok_or_else(|| self.error(LowerErrorKind::InvalidAnonymousArgument, span.clone()))?
        };
        if n > 0 && n as u64 > self.limits.max_anonymous_parameters as u64 {
            return Err(self.error(LowerErrorKind::AnonymousParameterLimit, span));
        }
        if self.anonymous.is_none() {
            return Err(self.error(LowerErrorKind::InvalidAnonymousArgument, span));
        }
        let args = self.anonymous.as_mut().unwrap();
        let value = if rest {
            args.rest
                .get_or_insert_with(|| {
                    self.context.symbol(Name::new(
                        None,
                        &args
                            .unique
                            .map_or_else(|| "%&#".into(), |id| format!("rest__{id}#")),
                    ))
                })
                .clone()
        } else {
            args.max = args.max.max(n.max(0) as usize);
            args.names
                .entry(n)
                .or_insert_with(|| {
                    self.context.symbol(Name::new(
                        None,
                        &args
                            .unique
                            .map_or_else(|| format!("%{n}#"), |id| format!("p{n}__{id}#")),
                    ))
                })
                .clone()
        };
        Ok(self.scalar(value, span, Origin::AnonymousArgument))
    }
}
fn name(text: &str) -> Name {
    if text == "/" {
        return Name::new(None, text);
    }
    match text.split_once('/') {
        Some((namespace, name)) => Name::new(Some(namespace), name),
        None => Name::new(None, text),
    }
}
fn integer(text: &str) -> Option<i128> {
    let negative = text.starts_with('-');
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (radix, digits) = if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        (16, hex)
    } else if let Some((radix, digits)) = unsigned.split_once(['r', 'R']) {
        (radix.parse().ok()?, digits)
    } else if unsigned.len() > 1 && unsigned.starts_with('0') {
        (8, unsigned)
    } else {
        (10, unsigned)
    };
    if !(2..=36).contains(&radix) {
        return None;
    }
    let magnitude = i128::from_str_radix(digits, radix).ok()?;
    if negative {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

#[cfg(test)]
mod compiler_ownership_tests {
    use super::*;
    #[test]
    fn compiler_snapshot_releases_the_native_owner_entirely() {
        let context = Context::new();
        let weak_owner = Rc::downgrade(&context.owner);
        let source = context.source("snapshot.clj", "^:kept {:z [1 2] :a 3}");
        let native = context
            .reader(&source)
            .unwrap()
            .read_form()
            .unwrap()
            .unwrap();
        let form = crate::native_form_adapter::own_form(&native).unwrap();
        drop(native);
        drop(source);
        drop(context);
        assert!(
            weak_owner.upgrade().is_none(),
            "compiler snapshot retained a native owner"
        );
        assert_eq!(form.children()[0].span().text(), ":z");
        assert!(form.metadata().is_some());
    }
}
