//! Select CLJC branches structurally, retaining original node locations.
use crate::{AnalysisError, AnalysisErrorKind, CollectionKind, NodeId, NodeKind, SyntaxTree};

pub(crate) fn select_cljs(
    source: &str,
    tree: &mut SyntaxTree,
    discards: &mut Vec<NodeId>,
) -> Result<(), AnalysisError> {
    struct Selection<'a> {
        source: &'a str,
        tree: &'a mut SyntaxTree,
        suppressed: Vec<std::ops::Range<usize>>,
        discards: std::collections::VecDeque<NodeId>,
        selected_discards: Vec<NodeId>,
    }
    impl Selection<'_> {
        fn error(&self, id: NodeId, message: &'static str) -> AnalysisError {
            AnalysisError {
                span: self.tree.nodes[id.0].span.clone(),
                kind: AnalysisErrorKind::InvalidForm(message),
            }
        }
        fn discard_before(
            &mut self,
            end: usize,
            input: &mut std::collections::VecDeque<NodeId>,
            collection: bool,
        ) -> Result<(), AnalysisError> {
            while let Some(&id) = self.discards.front() {
                let span = self.tree.nodes[id.0].span.clone();
                if span.start >= end {
                    break;
                }
                self.discards.pop_front();
                if self.suppressed.iter().any(|s| s.contains(&span.start)) {
                    continue;
                }
                let NodeKind::Discard { form } = self.tree.nodes[id.0].kind else {
                    unreachable!("parser retains discard prefix frames")
                };
                let selected = self.one(form, input, collection)?;
                self.selected_discards.push(selected);
            }
            Ok(())
        }
        fn next(
            &mut self,
            input: &mut std::collections::VecDeque<NodeId>,
            collection: bool,
        ) -> Result<Option<NodeId>, AnalysisError> {
            while let Some(&id) = input.front() {
                self.discard_before(self.tree.nodes[id.0].span.start, input, collection)?;
                let Some(id) = input.pop_front() else {
                    break;
                };
                let nodes = self.form(id, input, collection)?;
                if let Some((&first, rest)) = nodes.split_first() {
                    // A splice shares the pending stream with the enclosing reader
                    // prefix: that prefix owns its first value, not the whole splice.
                    for &id in rest.iter().rev() {
                        input.push_front(id);
                    }
                    return Ok(Some(first));
                }
            }
            Ok(None)
        }
        fn one(
            &mut self,
            id: NodeId,
            input: &mut std::collections::VecDeque<NodeId>,
            collection: bool,
        ) -> Result<NodeId, AnalysisError> {
            input.push_front(id);
            self.next(input, collection)?
                .ok_or_else(|| self.error(id, "reader prefix requires one selected form"))
        }
        fn forms(
            &mut self,
            ids: &[NodeId],
            collection: bool,
            end: usize,
        ) -> Result<Vec<NodeId>, AnalysisError> {
            let mut input = ids.iter().copied().collect();
            let mut result = Vec::new();
            while let Some(id) = self.next(&mut input, collection)? {
                result.push(id);
            }
            self.discard_before(end, &mut input, collection)?;
            Ok(result)
        }
        fn form(
            &mut self,
            id: NodeId,
            input: &mut std::collections::VecDeque<NodeId>,
            collection: bool,
        ) -> Result<Vec<NodeId>, AnalysisError> {
            let kind = match self.tree.nodes[id.0].kind.clone() {
                NodeKind::Conditional { splicing, branches } => {
                    let NodeKind::Collection { children, .. } =
                        self.tree.nodes[branches.0].kind.clone()
                    else {
                        unreachable!()
                    };
                    let mut selected = None;
                    for pair in children.as_chunks::<2>().0.iter() {
                        let feature = &self.source[self.tree.nodes[pair[0].0].span.clone()];
                        if selected.is_none() && matches!(feature, ":cljs" | ":default") {
                            selected = Some(pair[1]);
                        } else {
                            // A branch's leading discards are read under the
                            // same suppression as its retained value.
                            self.suppressed.push(
                                self.tree.nodes[pair[0].0].span.end
                                    ..self.tree.nodes[pair[1].0].span.end,
                            );
                        }
                    }
                    let Some(selected) = selected else {
                        return Ok(vec![]);
                    };
                    let selected = self.one(selected, &mut Default::default(), collection)?;
                    if !splicing {
                        return Ok(vec![selected]);
                    }
                    if !collection {
                        return Err(
                            self.error(id, "reader conditional splicing requires a collection")
                        );
                    }
                    return match self.tree.nodes[selected.0].kind.clone() {
                        NodeKind::Collection {
                            kind: CollectionKind::List | CollectionKind::Vector,
                            children,
                        } => Ok(children),
                        _ => Err(self.error(
                            id,
                            "reader conditional splicing requires a sequential value",
                        )),
                    };
                }
                NodeKind::Collection { kind, children } => {
                    let children = self.forms(&children, true, self.tree.nodes[id.0].span.end)?;
                    if kind == CollectionKind::Map && !children.len().is_multiple_of(2) {
                        return Err(self.error(id, "map requires an even number of selected forms"));
                    }
                    NodeKind::Collection { kind, children }
                }
                NodeKind::Prefix { kind, form } => {
                    let form = self.one(form, input, collection)?;
                    self.tree.nodes[id.0].span.end = self.tree.nodes[form.0].span.end;
                    NodeKind::Prefix { kind, form }
                }
                NodeKind::Metadata { metadata, target } => {
                    let metadata = self.one(metadata, &mut Default::default(), collection)?;
                    let target = self.one(target, input, collection)?;
                    self.tree.nodes[id.0].span.end = self.tree.nodes[target.0].span.end;
                    NodeKind::Metadata { metadata, target }
                }
                NodeKind::Tagged { tag, form } => {
                    let form = self.one(form, input, collection)?;
                    self.tree.nodes[id.0].span.end = self.tree.nodes[form.0].span.end;
                    NodeKind::Tagged { tag, form }
                }
                NodeKind::NamespacedMap {
                    auto_resolve,
                    namespace,
                    map,
                } => NodeKind::NamespacedMap {
                    auto_resolve,
                    namespace,
                    map: self.one(map, &mut Default::default(), collection)?,
                },
                kind => kind,
            };
            self.tree.nodes[id.0].kind = kind;
            Ok(vec![id])
        }
    }
    let roots = tree.roots.clone();
    discards.sort_by_key(|id| tree.nodes[id.0].span.start);
    let mut selection = Selection {
        source,
        tree,
        suppressed: vec![],
        discards: discards.iter().copied().collect(),
        selected_discards: vec![],
    };
    selection.tree.roots = selection.forms(&roots, false, source.len())?;
    *discards = selection.selected_discards;
    Ok(())
}

// Compiler semantic reader. Construction uses owned data only, never evaluation.
use crate::compiler_data::{
    CompilerLiteralKind, DataBuilder, Datum, DatumView, Form, FormId, FormKind, FormNode, Name,
    Origin, ReadError, Source, SourceSpan,
};
use crate::parser::{Node, NumberKind, PrefixKind, SymbolicValue};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::Range,
    sync::Arc,
};
#[derive(Debug)]
pub enum LowerErrorKind {
    InvalidSyntax,
    FormNestingLimit,
    MissingReaderForm,
    InvalidSplice,
    MissingAlias(Box<Name>),
    UnknownTag(Box<Name>),
    NumericOverflow,
    InvalidMetadata,
    InvalidMetadataTarget,
    DuplicateMapKey,
    InvalidAnonymousArgument,
    AnonymousParameterLimit,
    InvalidCompilerLiteral(&'static str),
    Unsupported(UnsupportedSyntax),
}
#[derive(Debug)]
pub enum UnsupportedSyntax {
    ReaderConditional,
    Regex,
}
struct Anonymous {
    unique: Option<u64>,
    names: HashMap<i64, Datum>,
    rest: Option<Datum>,
    max: usize,
}
enum InputForm {
    Syntax(NodeId),
}
#[derive(Default)]
struct Input {
    pending: VecDeque<InputForm>,
}
impl Input {
    fn nodes(nodes: impl IntoIterator<Item = NodeId>) -> Self {
        Self {
            pending: nodes.into_iter().map(InputForm::Syntax).collect(),
        }
    }
}
struct Syntax<'a>(&'a [Node]);
impl Syntax<'_> {
    fn nodes(&self) -> &[Node] {
        self.0
    }
}
struct Limits {
    max_anonymous_parameters: usize,
}
struct Lowerer<'a> {
    data_reader: Option<&'a DataReader<'a>>,
    bindings: Option<&'a BindingResolver<'a>>,
    builder: DataBuilder,
    source: &'a Source,
    syntax: Syntax<'a>,
    view: &'a NamespaceView,
    splices: HashSet<FormId>,
    discarded: Vec<NodeId>,
    next_discard: usize,
    suppress_arguments: usize,
    anonymous: Option<Anonymous>,
    gensyms: HashMap<Name, Datum>,
    limits: Limits,
    nodes: Vec<FormNode>,
    values: Vec<Datum>,
    spans: Vec<SourceSpan>,
}
impl Lowerer<'_> {
    fn reader_equal(&self, a: FormId, b: FormId) -> bool {
        // Source spelling carries reader numeric categories; Datum equality is
        // the canonical JS data model and may already equate distinct categories.
        let number = |id: FormId| {
            self.syntax.nodes().iter().find_map(|node| {
                if node.span == self.spans[id.0].bytes()
                    && let NodeKind::Number(kind) = node.kind
                {
                    crate::reader_literals::number_key(self.spans[id.0].text(), kind)
                } else {
                    None
                }
            })
        };
        if let (Some(a), Some(b)) = (number(a), number(b)) {
            return a == b;
        }
        match (&self.nodes[a.0].kind, &self.nodes[b.0].kind) {
            (
                FormKind::Collection {
                    kind: ak,
                    children: ac,
                },
                FormKind::Collection {
                    kind: bk,
                    children: bc,
                },
            ) => {
                if ac.len() != bc.len() {
                    return false;
                }
                match (ak, bk) {
                    (
                        CollectionKind::List | CollectionKind::Vector,
                        CollectionKind::List | CollectionKind::Vector,
                    ) => ac
                        .iter()
                        .zip(bc.iter())
                        .all(|(&a, &b)| self.reader_equal(a, b)),
                    (CollectionKind::Map, CollectionKind::Map) => {
                        ac.as_chunks::<2>().0.iter().all(|[a, v]| {
                            bc.as_chunks::<2>().0.iter().any(|[b, w]| {
                                self.reader_equal(*a, *b) && self.reader_equal(*v, *w)
                            })
                        })
                    }
                    (CollectionKind::Set, CollectionKind::Set) => ac
                        .iter()
                        .all(|&a| bc.iter().any(|&b| self.reader_equal(a, b))),
                    _ => false,
                }
            }
            _ => self.values[a.0].value_eq(&self.values[b.0]),
        }
    }
    fn reader_namespace(&self, alias: Option<&Name>) -> Option<Name> {
        match alias {
            None => Some(self.view.namespace.clone()),
            Some(a) => self.view.aliases.get(a).cloned().or_else(|| {
                // An interactive session's namespaces are addressable by
                // their own names, as if required.
                let session = self.view.session_namespaces.as_ref()?;
                (a != &self.view.namespace && session.contains(&String::from_utf16_lossy(a.name())))
                    .then(|| a.clone())
            }),
        }
    }
    fn syntax_quote_name(&self, name: &Name) -> Name {
        self.view.qualify_with(name, self.bindings)
    }
    fn error(&self, kind: LowerErrorKind, bytes: Range<usize>) -> ReadError {
        ReadError {
            message: format!("Lower({kind:?})"),
            span: self
                .source
                .span(bytes)
                .unwrap_or_else(|| self.source.span(0..0).unwrap()),
            related: None,
        }
    }
    fn resource_check(&self, _span: Range<usize>) -> Result<(), ReadError> {
        Ok(())
    }
    fn collection_error(&self, _error: String, span: Range<usize>) -> ReadError {
        self.error(LowerErrorKind::InvalidSyntax, span)
    }
    fn finish(self, id: FormId) -> Form {
        Form::from_parts(self.nodes, self.values, self.spans, id)
    }
    fn read_next(
        &mut self,
        input: &mut Input,
        depth: usize,
        _expand_splices: bool,
    ) -> Result<Option<FormId>, ReadError> {
        loop {
            let Some(next) = input.pending.pop_front() else {
                return Ok(None);
            };
            let InputForm::Syntax(id) = next;
            let span = self.syntax.nodes()[id.0].span.clone();
            if self
                .discarded
                .get(self.next_discard)
                .is_some_and(|d| self.syntax.nodes()[d.0].span.start < span.start)
            {
                input.pending.push_front(InputForm::Syntax(id));
                self.discard_before_input(span.start, depth, input)?;
                continue;
            }
            if matches!(self.syntax.nodes()[id.0].kind, NodeKind::Conditional { .. }) {
                return Err(self.error(
                    LowerErrorKind::Unsupported(UnsupportedSyntax::ReaderConditional),
                    span,
                ));
            }
            return self.lower(id, depth, input).map(Some);
        }
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
    fn add(
        &mut self,
        value: Datum,
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
    fn scalar(&mut self, value: Datum, span: Range<usize>, origin: Origin) -> FormId {
        self.add(value, FormKind::Scalar, None, span, origin)
    }
    fn symbol(&mut self, text: &str, span: Range<usize>, origin: Origin) -> FormId {
        let value = self.builder.symbol(name(text));
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
        if kind == CollectionKind::Map {
            if kind == CollectionKind::Map && !children.len().is_multiple_of(2) {
                return Err(self.error(LowerErrorKind::InvalidSyntax, span));
            }
            let mut seen: HashMap<u64, Vec<usize>> = HashMap::new();
            let step = 2;
            for i in (0..values.len()).step_by(step) {
                let bucket = seen.entry(values[i].value_hash()).or_default();
                if let Some(&first) = bucket
                    .iter()
                    .find(|&&j| self.reader_equal(children[i], children[j]))
                {
                    let mut e = self.error(
                        LowerErrorKind::DuplicateMapKey,
                        self.spans[children[i].0].bytes(),
                    );
                    e.related = Some(self.spans[children[first].0].clone());
                    return Err(e);
                }
                bucket.push(i);
            }
        }
        let value = match kind {
            CollectionKind::List => self.builder.list(&values),
            CollectionKind::Vector => self.builder.vector(&values),
            CollectionKind::Map => self.builder.map(
                &values
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| (p[0].clone(), p[1].clone()))
                    .collect::<Vec<_>>(),
            ),
            CollectionKind::Set => self.builder.set(&values),
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
            let mark = self.nodes.len();
            input.pending.push_front(InputForm::Syntax(id));
            let result = self.required(input, depth, self.syntax.nodes()[id.0].span.clone());
            result?;
            self.nodes.truncate(mark);
            self.values.truncate(mark);
            self.spans.truncate(mark);
            self.splices.retain(|id| id.0 < mark);
        }
        Ok(())
    }
    fn source_scalar(&mut self, node: &Node) -> Result<Option<Datum>, ReadError> {
        let span = node.span.clone();
        let text = &self.source.text()[span.clone()];
        let scalar = match &node.kind {
            NodeKind::Nil => Some(self.builder.nil()),
            NodeKind::Boolean(v) => Some(self.builder.boolean(*v)),
            NodeKind::Symbol => Some(self.builder.symbol(name(text))),
            NodeKind::Keyword {
                auto_resolve: false,
            } => Some(
                self.builder
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
                    self.builder
                        .keyword(Name::from_utf16(Some(namespace.name()), raw.name())),
                )
            }
            NodeKind::Regex => Some(
                self.builder.compiler_literal(
                    crate::compiler_data::CompilerLiteralKind::Regex,
                    &self
                        .builder
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
                Some(self.builder.string(value.to_utf16(self.source.text())))
            }
            NodeKind::Character(v) => Some(self.builder.character(*v as u16)),
            NodeKind::SymbolicValue(v) => Some(self.builder.double(match v {
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
            NodeKind::Discard { form } => {
                input.pending.push_front(InputForm::Syntax(*form));
                self.required(input, depth + 1, span)
            }
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
                DatumView::Symbol(name) => (Some(name), false),
                DatumView::Keyword(name) => (Some(name), true),
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
                    self.builder.keyword(name)
                } else {
                    self.builder.symbol(name)
                };
                if let Some(metadata) = self.values[key.0].metadata() {
                    value = self
                        .builder
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
        let tag = name(tag_text);
        input.pending.push_front(InputForm::Syntax(form));
        let data = self.required(input, depth + 1, span.clone())?;
        let span = span.start..span.end.max(self.spans[data.0].bytes().end);
        if let Some(reader) = self.data_reader {
            let location = self.source.span(span.clone()).unwrap();
            if let Some(value) =
                reader(&tag, &self.values[data.0], &location).map_err(|message| ReadError {
                    message,
                    span: location.clone(),
                    related: None,
                })?
            {
                let form = Form::from_data(value, location, Origin::DataReader(Arc::new(tag)));
                let offset = self.nodes.len();
                for node in form.nodes() {
                    let mut node = node.clone();
                    if let FormKind::Collection { children, .. } = &mut node.kind {
                        for child in children.iter_mut() {
                            child.0 += offset;
                        }
                    }
                    node.metadata = node.metadata.map(|id| FormId(id.0 + offset));
                    node.compiler_payload = node.compiler_payload.map(|id| FormId(id.0 + offset));
                    self.nodes.push(node);
                }
                for index in 0..form.nodes().len() {
                    let child = form.at(FormId(index)).unwrap();
                    self.values.push(child.data());
                    self.spans.push(child.span());
                }
                return Ok(FormId(form.id().0 + offset));
            }
        }
        {
            use crate::compiler_data::CompilerLiteralKind as K;
            let tag_name = String::from_utf16_lossy(tag.name());
            if tag.namespace().is_none()
                && matches!(tag_name.as_str(), "inst" | "uuid" | "js" | "queue")
            {
                let value = self.values[data.0].clone();
                let kind = match (tag_name.as_str(), value.inspect()) {
                    ("inst", DatumView::String(ref text)) => {
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
                            self.builder
                                .compiler_literal(K::Instant, &self.builder.double(millis as f64)),
                            span,
                            Origin::Source,
                        ));
                    }
                    ("uuid", DatumView::String(ref text)) => {
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
                            self.builder.compiler_literal(
                                K::Uuid,
                                &self.builder.string(uuid.encode_utf16().collect::<Vec<_>>()),
                            ),
                            span,
                            Origin::Source,
                        ));
                    }
                    ("js", DatumView::Vector(_)) => K::JsArray,
                    ("js", DatumView::Map(_)) => {
                        for (key, _) in value
                            .entries()
                            .map_err(|e| self.collection_error(e, span.clone()))?
                        {
                            if !matches!(key.inspect(), DatumView::String(_))
                                && !matches!(key.inspect(), DatumView::Keyword(ref n) if n.namespace().is_none())
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
                    ("queue", DatumView::Vector(_)) => {
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
                    self.builder.compiler_literal(kind, &value),
                    span,
                    Origin::Source,
                );
                self.nodes[id.0].compiler_payload = Some(data);
                return Ok(id);
            }
        }
        Err(self.error(LowerErrorKind::UnknownTag(Box::new(tag)), span))
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
        (matches!(self.values[head.0].inspect(), DatumView::Symbol(n) if n == Name::new(Some("clojure.core"), name)))
            .then(|| children.get(1).copied())
    }
    fn quote_operand(&mut self, operand: Option<FormId>, span: Range<usize>) -> FormId {
        operand.unwrap_or_else(|| self.scalar(self.builder.nil(), span, Origin::SyntaxQuote))
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
        } else if let DatumView::Symbol(mut name) = self.values[id.0].inspect() {
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
                    let number = crate::generated_names::next_syntax_quote();
                    let mut text = name.name()[..name.name().len() - 1].to_vec();
                    text.extend(format!("__{number}__auto__").encode_utf16());
                    let value = self.builder.symbol(Name::from_utf16(None, &text));
                    self.gensyms.insert(name, value.clone());
                    value
                }
            } else {
                if !special {
                    name = self.syntax_quote_name(&name);
                }
                self.builder.symbol(name)
            };
            let symbol = self.scalar(symbol, span.clone(), Origin::SyntaxQuote);
            self.quote_call("quote", vec![symbol], span.clone())?
        } else if matches!(
            self.values[id.0].inspect(),
            DatumView::CompilerLiteral(
                CompilerLiteralKind::JsArray | CompilerLiteralKind::JsObject
            )
        ) {
            // A #js literal is atomic syntax-quote data. Quote it explicitly so
            // emission preserves the descriptor instead of constructing a host value.
            return self.quote_call("quote", vec![id], span);
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
                    .any(|name| key.inspect() == DatumView::Keyword(Name::new(None, name)))
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
                            .any(|(k, _)| k.inspect() == DatumView::Keyword(Name::new(None, key)))
                        {
                            continue;
                        }
                        let key = self.scalar(
                            self.builder.keyword(Name::new(None, key)),
                            span.clone(),
                            Origin::SyntaxQuote,
                        );
                        let value = self.scalar(
                            self.builder.integer(value as i64),
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
    fn number(&self, text: &str, kind: NumberKind, span: Range<usize>) -> Result<Datum, ReadError> {
        let overflow = || self.error(LowerErrorKind::NumericOverflow, span.clone());
        if kind == NumberKind::Ratio {
            let (n, d) = text.split_once('/').ok_or_else(overflow)?;
            if let Some(exact) = crate::reader_literals::integral_ratio_text(n, d) {
                return Ok(if let Ok(n) = exact.parse::<i64>() {
                    self.builder.integer(n)
                } else {
                    self.builder.compiler_literal(
                        crate::compiler_data::CompilerLiteralKind::BigInteger,
                        &self
                            .builder
                            .string(exact.encode_utf16().collect::<Vec<_>>()),
                    )
                });
            }
        }
        if kind != NumberKind::Ratio {
            if matches!(kind, NumberKind::Integer | NumberKind::BigInteger)
                && let Some(n) = integer(if kind == NumberKind::BigInteger {
                    text.strip_suffix('N').unwrap()
                } else {
                    text
                })
                .and_then(|n| i64::try_from(n).ok())
            {
                return Ok(self.builder.integer(n));
            }
            if matches!(kind, NumberKind::Integer | NumberKind::BigInteger) {
                let text = if kind == NumberKind::BigInteger {
                    text.strip_suffix('N').unwrap()
                } else {
                    text
                };
                let exact = crate::reader_literals::integer_key(text);
                return Ok(self.builder.compiler_literal(
                    crate::compiler_data::CompilerLiteralKind::BigInteger,
                    &self
                        .builder
                        .string(exact.encode_utf16().collect::<Vec<_>>()),
                ));
            }
            return crate::analyzer::number(text, kind)
                .map(|v| self.builder.double(v))
                .ok_or_else(overflow);
        }
        crate::analyzer::number(text, kind)
            .map(|v| self.builder.double(v))
            .ok_or_else(overflow)
    }

    fn metadata(
        &mut self,
        metadata: FormId,
        target: FormId,
        span: Range<usize>,
    ) -> Result<FormId, ReadError> {
        let meta_span = self.spans[metadata.0].bytes();
        let metadata = match self.values[metadata.0].inspect() {
            DatumView::Keyword(_) => {
                let value = self.scalar(
                    self.builder.boolean(true),
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
            DatumView::Symbol(_) | DatumView::String(_) => {
                let tag = self.scalar(
                    self.builder.keyword(Name::new(None, "tag")),
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
            DatumView::Map(_) => metadata,
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
            .builder
            .with_meta(&self.values[target.0], &self.values[metadata.0])
            .map_err(|_failure| {
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
        self.anonymous = Some(Anonymous {
            unique: Some(crate::generated_names::next_anonymous()),
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
                self.builder.symbol(Name::new(
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
        let n = if rest {
            -1
        } else if index.is_none() {
            1
        } else {
            text.strip_prefix('%')
                .and_then(|s| {
                    integer(s).and_then(|n| i64::try_from(n).ok()).or_else(|| {
                        let (a, b) = s.split_once('/')?;
                        let a: i64 = a.parse().ok()?;
                        let b: i64 = b.parse().ok()?;
                        (b != 0 && a % b == 0).then(|| a / b)
                    })
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
        // The canonical reader registers %& and %-1 at the same index.
        // Other nonpositive indices have names in the body, but never add
        // positional parameter bindings.
        let value = if n == -1 {
            args.rest
                .get_or_insert_with(|| {
                    self.builder.symbol(Name::new(
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
                    self.builder.symbol(Name::new(
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

/// Stable compiler binding identity captured at the declaration frontier.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BindingId(pub usize);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    Value,
    Macro,
    Protocol,
    Type,
    Record,
}
#[derive(Clone, Debug)]
pub struct ResolvedBinding {
    pub id: BindingId,
    pub kind: BindingKind,
    pub name: Name,
}
/// Immutable definition-site name resolution. It owns no evaluator or roots.
#[derive(Clone, Debug)]
pub struct NamespaceView {
    pub namespace: Name,
    pub aliases: HashMap<Name, Name>,
    /// In an interactive session, its namespaces, which resolve as aliases
    /// of themselves after `aliases`.
    pub(crate) session_namespaces: Option<crate::analyzer::SessionNamespaces>,
    pub refers: HashMap<Name, Name>,
    pub excludes: HashSet<Name>,
    pub bindings: HashMap<Name, ResolvedBinding>,
    pub declaration_frontier: usize,
    pub source: crate::compiler_data::SourceId,
    core_bindings: std::rc::Rc<HashMap<Name, ResolvedBinding>>,
}
impl NamespaceView {
    pub fn new(namespace: &str, source: &Source) -> Self {
        thread_local! {
            static CORE: std::rc::Rc<HashMap<Name, ResolvedBinding>> = std::rc::Rc::new(core_public_bindings().enumerate().filter_map(|(index, (name, kind))| {
                if crate::CoreFnId::resolve(name).is_none() && !crate::compiler_names::core_macro(name) && !crate::compiler_names::is_definition_macro("cljs.core", name) { return None; }
                let qualified = Name::new(Some("cljs.core"), name);
                let kind = if crate::CoreFnId::resolve(name).is_none() && (crate::compiler_names::core_macro(name) || crate::compiler_names::is_definition_macro("cljs.core", name)) { BindingKind::Macro } else { kind };
                Some((qualified.clone(), ResolvedBinding { id: BindingId(usize::MAX - index), kind, name: qualified }))
            }).collect());
        }
        let core_bindings = CORE.with(Clone::clone);
        Self {
            namespace: Name::new(None, namespace),
            aliases: HashMap::new(),
            session_namespaces: None,
            refers: HashMap::new(),
            excludes: HashSet::new(),
            bindings: HashMap::new(),
            core_bindings,
            declaration_frontier: 0,
            source: source.id(),
        }
    }
    /// Inspect binding identity/kind without demanding a value or initializer.
    pub fn binding(&self, name: &Name) -> Option<&ResolvedBinding> {
        self.bindings
            .get(name)
            .or_else(|| self.bindings.get(&self.qualify(name)))
            .or_else(|| self.core_bindings.get(&self.qualify(name)))
    }
    pub fn qualify(&self, name: &Name) -> Name {
        self.qualify_with(name, None)
    }
    pub(crate) fn qualify_with(&self, name: &Name, bindings: Option<&BindingResolver<'_>>) -> Name {
        if let Some(resolved) = bindings.and_then(|lookup| lookup(name)) {
            return resolved;
        }
        if let Some(binding) = self.bindings.get(name) {
            return binding.name.clone();
        }
        if let Some(resolved) = self.refers.get(name) {
            return resolved.clone();
        }
        if let Some(ns) = name.namespace() {
            return self
                .aliases
                .get(&Name::from_utf16(None, ns))
                .map(|ns| Name::from_utf16(Some(ns.name()), name.name()))
                .unwrap_or_else(|| name.clone());
        }
        let text = String::from_utf16_lossy(name.name());
        if !self.excludes.contains(name)
            && (crate::compiler_names::core_macro(&text)
                || crate::compiler_names::is_definition_macro("cljs.core", &text)
                || crate::compiler_names::is_declaration_macro(&text)
                || core_metadata(&text).is_some()
                || self
                    .core_bindings
                    .contains_key(&Name::new(Some("cljs.core"), &text)))
        {
            return Name::from_utf16(
                Some(&"cljs.core".encode_utf16().collect::<Vec<_>>()),
                name.name(),
            );
        }
        if [
            "AssertionError",
            "Exception",
            "String",
            "ArithmeticException",
            "Integer",
            "Number",
            "Double",
            "Object",
        ]
        .contains(&text.as_str())
        {
            return Name::new(None, &format!("java.lang.{text}"));
        }
        if text.contains('.') {
            if !text.starts_with('.') && text.ends_with('.') {
                let base = self.qualify_with(&Name::new(None, &text[..text.len() - 1]), bindings);
                let mut suffix = base.name().to_vec();
                suffix.push(46);
                return Name::from_utf16(base.namespace(), &suffix);
            }
            return name.clone();
        }
        Name::from_utf16(Some(self.namespace.name()), name.name())
    }
}
/// The pinned public declaration universe. Availability comes from the compiler registry.
pub(crate) fn core_public_bindings() -> impl Iterator<Item = (&'static str, BindingKind)> {
    include_str!("core_public_bindings.tsv")
        .lines()
        .filter_map(|line| {
            let (name, kind) = line.split_once('\t')?;
            Some((
                name,
                match kind {
                    "Macro" => BindingKind::Macro,
                    "Protocol" => BindingKind::Protocol,
                    "Type" => BindingKind::Type,
                    "Record" => BindingKind::Record,
                    _ => BindingKind::Value,
                },
            ))
        })
}
/// Source metadata is resolution data; querying it cannot initialize a runtime.
pub fn core_metadata(name: &str) -> Option<&'static str> {
    include_str!("core_var_metadata.tsv")
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .find_map(|(n, m)| (n == name).then_some(m))
}
pub fn lower_form(
    source: &Source,
    tree: &SyntaxTree,
    root: NodeId,
    view: &NamespaceView,
    discards: &[NodeId],
) -> Result<Form, ReadError> {
    lower_form_with_reader(source, tree, root, view, discards, None, None)
}
pub(crate) type BindingResolver<'a> = dyn Fn(&Name) -> Option<Name> + 'a;
pub(crate) type DataReader<'a> =
    dyn Fn(&Name, &Datum, &crate::compiler_data::SourceSpan) -> Result<Option<Datum>, String> + 'a;
pub(crate) fn lower_form_with_reader(
    source: &Source,
    tree: &SyntaxTree,
    root: NodeId,
    view: &NamespaceView,
    discards: &[NodeId],
    data_reader: Option<&DataReader<'_>>,
    bindings: Option<&BindingResolver<'_>>,
) -> Result<Form, ReadError> {
    let span = &tree.nodes[root.0].span;
    let mut discarded = discards
        .iter()
        .copied()
        .filter(|id| *id != root && span.contains(&tree.nodes[id.0].span.start))
        .collect::<Vec<_>>();
    discarded.sort_by_key(|id| tree.nodes[id.0].span.start);
    let mut lower = Lowerer {
        data_reader,
        bindings,
        builder: DataBuilder,
        source,
        syntax: Syntax(&tree.nodes),
        view,
        splices: HashSet::new(),
        discarded,
        next_discard: 0,
        suppress_arguments: 0,
        anonymous: None,
        gensyms: HashMap::new(),
        limits: Limits {
            max_anonymous_parameters: 1024,
        },
        nodes: vec![],
        values: vec![],
        spans: vec![],
    };
    let id = lower.required(&mut Input::nodes([root]), 0, span.clone())?;
    lower.discard_before(span.end, 0)?;
    Ok(lower.finish(id))
}
pub fn read_form(source: &Source, view: &NamespaceView) -> Result<Option<Form>, ReadError> {
    let mut parser = crate::Parser::new(source.text()).retaining_discards();
    let root = parser.read_form().map_err(|e| ReadError {
        message: format!("Syntax({:?})", e.kind),
        span: source.span(e.span).unwrap(),
        related: e.related.map(|s| source.span(s).unwrap()),
    })?;
    let Some(root) = root else { return Ok(None) };
    let discards = parser.discarded().to_vec();
    let tree = SyntaxTree {
        nodes: parser.into_nodes(),
        roots: vec![root],
    };
    lower_form(source, &tree, root, view, &discards).map(Some)
}

#[cfg(test)]
mod namespace_snapshot_tests {
    use super::*;
    #[test]
    fn snapshots_keep_frontier_kinds_aliases_excludes_and_exact_names() {
        let source = Source::new("definition.cljs", "`[a/x + same]");
        let mut view = NamespaceView::new("definition", &source);
        view.aliases
            .insert(Name::new(None, "a"), Name::new(None, "library"));
        view.excludes.insert(Name::new(None, "+"));
        view.bindings.insert(
            Name::new(None, "same"),
            ResolvedBinding {
                id: BindingId(7),
                kind: BindingKind::Macro,
                name: Name::new(Some("definition"), "same"),
            },
        );
        view.declaration_frontier = 8;
        let captured = view.clone();
        view.aliases
            .insert(Name::new(None, "a"), Name::new(None, "later"));
        view.declaration_frontier = 9;
        assert_eq!(
            captured.qualify(&Name::new(Some("a"), "x")),
            Name::new(Some("library"), "x")
        );
        assert_eq!(
            captured.qualify(&Name::new(None, "+")),
            Name::new(Some("definition"), "+")
        );
        assert_eq!(captured.declaration_frontier, 8);
        assert_eq!(
            captured.bindings[&Name::new(None, "same")].kind,
            BindingKind::Macro
        );
        assert_eq!(captured.source, source.id());
        let escaped = Name::from_utf16(None, &[0xd800, 0x0078]);
        let result = captured.qualify(&escaped);
        assert_eq!(result.name(), &[0xd800, 0x0078]);
        assert_eq!(
            result.namespace(),
            Some(Name::new(None, "definition").name())
        );
        assert!(read_form(&source, &captured).unwrap().is_some());
        assert!(core_metadata("+").is_some());
    }
}
