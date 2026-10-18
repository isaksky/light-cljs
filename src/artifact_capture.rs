//! Capture a completed namespace as a [`NamespaceArtifact`] where
//! `NamespaceAnalysis::analyze` returns, after its deferred units drained
//! (docs/compiler-cache.md, "Binary artifacts").
//!
//! [`begin`] notes how long each program table is before the namespace's
//! analysis, and keeps cheap shared clones of the tables an analysis may
//! change in place; [`finish`] converts what the analysis appended, and the
//! changes it made to earlier entries, into the artifact's own tables. Only a
//! compilation that asks for artifacts runs either: without capture, the
//! analysis does none of this work and keeps nothing for it.
//!
//! What an artifact cannot hold yet makes capture fail with a reason rather
//! than produce a partial artifact: a deferred unit that was never analyzed
//! or that failed, a reference from
//! the namespace's IR into an earlier namespace's expressions or bindings, or a
//! change to an earlier namespace's protocol, type or reader tables.

use std::collections::HashMap;

use crate::analyzer::{self as ir, GlobalBinding, GlobalId, NamespaceResolution, Program};
use crate::compilation_session::{CompilationSession, RegistrationRecord};
use crate::compiler_data::{Datum as SourceDatum, DatumKind as SourceDatumKind};
use crate::macro_host::{MacroHost, SourceBindingKind};
use crate::namespace_artifact::*;

/// The program and session before a namespace's analysis.
pub(crate) struct CaptureStart {
    exprs: usize,
    locals: usize,
    recur_targets: usize,
    strings: usize,
    named_literals: usize,
    globals: usize,
    lazy_targets: usize,
    namespaces: usize,
    /// How many warnings there were, and those of the namespace's own source
    /// (its `ns` form's): analysis sorts every warning by source and offset,
    /// so the namespace's are found by source, less these.
    warnings: usize,
    own_warnings: Vec<ir::AnalysisWarning>,
    units: usize,
    registrations: usize,
    bundles: usize,
    /// Frozen clones: they share every entry until the analysis writes one.
    globals_before: crate::arena::Arena<GlobalBinding>,
    protocols: crate::shared::Shared<HashMap<GlobalId, Vec<ir::ProtocolMethod>>>,
    record_constructors: crate::shared::Shared<std::collections::HashSet<GlobalId>>,
    field_bindings: crate::shared::Shared<HashMap<ir::LocalId, (ir::RecurTargetId, bool)>>,
    type_methods: crate::shared::Shared<std::collections::HashSet<ir::RecurTargetId>>,
    macro_functions: crate::shared::Shared<std::collections::HashSet<ir::ExprId>>,
    reader_contexts: crate::shared::Shared<
        crate::layered::Layered<String, crate::shared::Shared<ir::RuntimeReaderNamespace>>,
    >,
}

/// Note the program's tables before the next namespace's analysis.
pub(crate) fn begin(
    program: &mut Program,
    source_index: usize,
    warnings: &[ir::AnalysisWarning],
    session: Option<&CompilationSession>,
) -> CaptureStart {
    program.globals.freeze();
    let (units, registrations, bundles) = session.map_or((0, 0, 0), |s| s.capture_marks());
    CaptureStart {
        exprs: program.exprs.len(),
        locals: program.locals.len(),
        recur_targets: program.recur_targets.len(),
        strings: program.strings.len(),
        named_literals: program.named_literals.len(),
        globals: program.globals.len(),
        lazy_targets: program.lazy_targets.len(),
        namespaces: program.namespaces.len(),
        warnings: warnings.len(),
        own_warnings: warnings
            .iter()
            .filter(|warning| warning.source_index == source_index)
            .cloned()
            .collect(),
        units,
        registrations,
        bundles,
        globals_before: program.globals.clone(),
        protocols: program.protocols.clone(),
        record_constructors: program.record_constructors.clone(),
        field_bindings: program.field_bindings.clone(),
        type_methods: program.type_methods.clone(),
        macro_functions: program.macro_functions.clone(),
        reader_contexts: program.reader_contexts.clone(),
    }
}

/// What the loader knows about the namespace that analysis does not.
pub(crate) struct CaptureInput<'a> {
    pub namespace: &'a str,
    pub path: &'a str,
    pub source_length: usize,
    pub entry: bool,
    pub resolution: &'a NamespaceResolution,
    pub aliases: &'a [(String, String)],
    /// Where the generated-name allocators stood before the namespace was
    /// prepared.
    pub names_from: crate::generated_names::Counters,
    pub requires: &'a [String],
    /// The compilation's analysis options, which decide the namespace's
    /// diagnostics.
    pub options: ir::AnalysisOptions,
    /// Where the namespace's logical work began, before it was prepared.
    pub work: crate::macro_host::WorkStart,
    /// The control that cancels the compilation, if any.
    pub control: Option<&'a crate::evaluator::ExecutionControl>,
}

/// The analysis options an artifact records and installation matches.
pub(crate) fn analysis_policy(options: ir::AnalysisOptions) -> AnalysisPolicy {
    AnalysisPolicy {
        warning_mode: options.warning_mode,
        checked_arrays: options.checked_arrays,
        array_checks: match options.array_checks {
            ir::CheckedArrays::Off => ArrayChecks::Off,
            ir::CheckedArrays::Warn => ArrayChecks::Warn,
            ir::CheckedArrays::Error => ArrayChecks::Error,
        },
    }
}

/// The logical work of the namespace whose work began at `start`
/// ([`crate::macro_host::WorkMarks`]), or None if part of it could not be
/// measured.
fn accounting(macro_host: &MacroHost, start: crate::macro_host::WorkStart) -> Option<Accounting> {
    let work = &macro_host.work;
    if work.unmeasured.get() {
        return None;
    }
    let nodes = macro_host.generated_nodes.get();
    Some(Accounting {
        expansions: (macro_host.expansions.get() - start.expansions) as u64,
        generated_nodes: (nodes - start.generated_nodes) as u64,
        node_peak: (work.node_peak.get().max(nodes) - start.generated_nodes) as u64,
        macro_depth: work.macro_depth.get() as u32,
        analysis_depth: work.analysis_depth.get() as u32,
        unit_depth: work.unit_depth.get() as u32,
        conversion_peak: work.conversion_peak.get() as u64,
    })
}

/// The analysis that just completed, as the namespace analysis holds it.
pub(crate) struct Completed<'a> {
    pub program: &'a Program,
    pub names: &'a crate::name_index::NameIndex,
    pub root: ir::ExprId,
    pub source_index: usize,
    pub warnings: &'a [ir::AnalysisWarning],
}

pub(crate) fn finish(
    start: &CaptureStart,
    completed: Completed<'_>,
    input: CaptureInput<'_>,
    macro_host: &MacroHost,
) -> Result<NamespaceArtifact, String> {
    let session = macro_host.session_handle();
    // Taken before anything can fail, so a refused capture leaves nothing
    // of this namespace for the next one.
    let (captured_macros, captured_expansions) = session
        .as_ref()
        .map(|s| s.take_captured())
        .unwrap_or_default();
    // A cancelled or failed session leaves no artifact, even of an analysis
    // that returned: what its macros saw may be what the failure left.
    if let Some(reason) = macro_host.halted(input.control) {
        return Err(format!("the compile-time session stopped: {reason}"));
    }
    let program = completed.program;
    if program
        .namespaces
        .get(completed.source_index)
        .map(String::as_str)
        != Some(input.namespace)
    {
        return Err("the namespace's source index is not its position in the program".into());
    }
    if program.namespaces.len() != start.namespaces.max(completed.source_index + 1) {
        return Err("the analysis added another namespace to the program".into());
    }
    let units_end = session
        .as_ref()
        .map_or(start.units, |s| s.capture_marks().0);
    let mut c = Converter {
        program,
        start,
        names: completed.names,
        macro_host,
        source_index: completed.source_index,
        units_end,
        references: References::default(),
        external_globals: HashMap::new(),
        namespaces: HashMap::new(),
        strings: program.strings.tail_from(start.strings).cloned().collect(),
        external_strings: HashMap::new(),
        named_literals: Vec::new(),
        external_named: HashMap::new(),
        js_globals: HashMap::new(),
        js_table: Vec::new(),
    };
    for literal in program.named_literals.tail_from(start.named_literals) {
        let literal = c.literal(literal)?;
        c.named_literals.push(literal);
    }
    let exprs = program
        .exprs
        .tail_from(start.exprs)
        .map(|expr| {
            Ok(Expr {
                kind: c.kind(&expr.kind)?,
                span: bytes(&expr.span),
                context: expr.context,
                source: c.source(expr.source_index),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let locals = program
        .locals
        .tail_from(start.locals)
        .map(|local| LocalBinding {
            name: local.name.clone(),
            span: bytes(&local.span),
            source: c.source(local.source_index),
        })
        .collect::<Vec<_>>();
    let recur_targets = program
        .recur_targets
        .tail_from(start.recur_targets)
        .map(|target| {
            Ok(RecurTarget {
                params: c.locals(&target.params)?,
                protocol_impl: target.protocol_impl,
                receiver_from_this: target.receiver_from_this,
                used: target.used,
                argument_slots: target
                    .argument_slots
                    .iter()
                    .map(|slot| slot.map(|slot| slot as u32))
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let root = c.expr(completed.root)?;
    let declarations = c.declarations(&input)?;
    let patches = c.patches()?;
    let units = match &session {
        Some(session) if session.capturing() => c.units(session)?,
        _ => DeferredUnits::default(),
    };
    let macros = c.macros(input.namespace, captured_macros)?;
    let expansions = captured_expansions
        .into_iter()
        .map(|expansion| {
            let (namespace, name) = expansion
                .key
                .split_once('/')
                .ok_or_else(|| format!("macro key {} has no namespace", expansion.key))?;
            Ok(MacroExpansion {
                macro_namespace: namespace.to_owned(),
                name: name.to_owned(),
                generation: expansion.generation as u32,
                call: bytes(&expansion.call),
                source: c.source(expansion.source_index),
                definition: Site {
                    source: expansion.definition_source,
                    bytes: bytes(&expansion.definition),
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut before = start.own_warnings.clone();
    let own = completed
        .warnings
        .iter()
        .filter(|warning| warning.source_index == completed.source_index)
        .filter(|warning| match before.iter().position(|b| b == *warning) {
            Some(index) => {
                before.swap_remove(index);
                false
            }
            None => true,
        })
        .collect::<Vec<_>>();
    if completed.warnings.len() - start.warnings != own.len() {
        return Err("the analysis reported warnings about another source".into());
    }
    let warnings = own
        .into_iter()
        .map(|warning| Warning {
            kind: warning.kind.to_owned(),
            message: warning.message.clone(),
            span: bytes(&warning.span),
            source: c.source(warning.source_index),
        })
        .collect();
    let mut declarations = declarations;
    declarations.macros = macros;
    let ir = SemanticIr {
        exprs,
        locals,
        recur_targets,
        strings: std::mem::take(&mut c.strings),
        named_literals: std::mem::take(&mut c.named_literals),
        js_globals: std::mem::take(&mut c.js_table),
    };
    let artifact = NamespaceArtifact {
        header: Header {
            namespace: input.namespace.to_owned(),
            path: input.path.to_owned(),
            source_length: input.source_length as u32,
            cljc: input.resolution.cljc,
            source_name: input.resolution.source_name.clone(),
            entry: input.entry,
            root,
            policy: analysis_policy(input.options),
        },
        declarations,
        ir,
        patches,
        units,
        diagnostics: Diagnostics {
            warnings,
            expansions,
        },
        references: c.references,
        // What the namespace's analysis, deferred units included, generated.
        names: {
            let (gensym_to, gensyms) = match &session {
                Some(session) => session.take_gensyms()?,
                None => (0, Vec::new()),
            };
            GeneratedNames {
                from: input.names_from,
                to: crate::generated_names::current(),
                gensym_to,
                gensyms,
            }
        },
        accounting: accounting(macro_host, input.work),
    };
    debug_assert_eq!(artifact.validate(), Ok(()));
    Ok(artifact)
}

/// Earlier globals first, then the namespace's own, each in creation order.
fn order(global: &GlobalRef) -> (bool, u32) {
    match global {
        GlobalRef::External(index) => (false, *index),
        GlobalRef::Own(index) => (true, *index),
    }
}

fn bytes(span: &std::ops::Range<usize>) -> Bytes {
    (span.start as u32, span.end as u32)
}

struct Converter<'a> {
    program: &'a Program,
    start: &'a CaptureStart,
    names: &'a crate::name_index::NameIndex,
    macro_host: &'a MacroHost,
    source_index: usize,
    units_end: usize,
    references: References,
    external_globals: HashMap<usize, u32>,
    namespaces: HashMap<usize, u32>,
    strings: Vec<Vec<u16>>,
    external_strings: HashMap<usize, u32>,
    named_literals: Vec<NamedLiteral>,
    external_named: HashMap<usize, u32>,
    js_globals: HashMap<usize, u32>,
    js_table: Vec<Vec<String>>,
}

impl Converter<'_> {
    fn source(&mut self, source_index: usize) -> SourceRef {
        if source_index == self.source_index {
            return SourceRef::Own;
        }
        let next = self.namespaces.len() as u32;
        let index = *self.namespaces.entry(source_index).or_insert_with(|| {
            let name = self
                .program
                .namespaces
                .get(source_index)
                .cloned()
                .unwrap_or_default();
            self.references.namespaces.push(name);
            next
        });
        SourceRef::Namespace(index)
    }

    fn expr(&self, id: ir::ExprId) -> Result<ExprRef, String> {
        id.0.checked_sub(self.start.exprs)
            .map(|index| ExprRef(index as u32))
            .ok_or_else(|| {
                format!(
                    "the IR refers to expression {} of an earlier namespace",
                    id.0
                )
            })
    }
    fn exprs(&self, ids: &[ir::ExprId]) -> Result<Vec<ExprRef>, String> {
        ids.iter().map(|&id| self.expr(id)).collect()
    }
    fn optional(&self, id: Option<ir::ExprId>) -> Result<Option<ExprRef>, String> {
        id.map(|id| self.expr(id)).transpose()
    }
    fn local(&self, id: ir::LocalId) -> Result<LocalRef, String> {
        id.0.checked_sub(self.start.locals)
            .map(|index| LocalRef(index as u32))
            .ok_or_else(|| format!("the IR refers to binding {} of an earlier namespace", id.0))
    }
    fn locals(&self, ids: &[ir::LocalId]) -> Result<Vec<LocalRef>, String> {
        ids.iter().map(|&id| self.local(id)).collect()
    }
    fn recur(&self, id: ir::RecurTargetId) -> Result<RecurRef, String> {
        id.0.checked_sub(self.start.recur_targets)
            .map(|index| RecurRef(index as u32))
            .ok_or_else(|| {
                format!(
                    "the IR refers to recur target {} of an earlier namespace",
                    id.0
                )
            })
    }
    fn unit(&self, id: usize) -> Result<UnitRef, String> {
        if (self.start.units..self.units_end).contains(&id) {
            Ok(UnitRef((id - self.start.units) as u32))
        } else {
            Err(format!(
                "the namespace refers to deferred unit {id} of another namespace"
            ))
        }
    }
    fn string(&mut self, id: ir::StringId) -> StringRef {
        if let Some(index) = id.0.checked_sub(self.start.strings) {
            return StringRef(index as u32);
        }
        let next = self.strings.len() as u32;
        let index = *self.external_strings.entry(id.0).or_insert_with(|| {
            self.strings.push(self.program.strings[id.0].clone());
            next
        });
        StringRef(index)
    }
    fn literal(&mut self, literal: &ir::NamedLiteral) -> Result<NamedLiteral, String> {
        Ok(NamedLiteral {
            namespace: literal.namespace.map(|id| self.string(id)),
            name: self.string(literal.name),
        })
    }
    fn named(&mut self, id: ir::NamedLiteralId) -> Result<NamedRef, String> {
        if let Some(index) = id.0.checked_sub(self.start.named_literals) {
            return Ok(NamedRef(index as u32));
        }
        if let Some(&index) = self.external_named.get(&id.0) {
            return Ok(NamedRef(index));
        }
        let literal = self.literal(&self.program.named_literals[id.0].clone())?;
        let index = self.named_literals.len() as u32;
        self.named_literals.push(literal);
        self.external_named.insert(id.0, index);
        Ok(NamedRef(index))
    }
    fn js_global(&mut self, id: ir::JsGlobalId) -> JsGlobalRef {
        let next = self.js_table.len() as u32;
        let index = *self.js_globals.entry(id.0).or_insert_with(|| {
            self.js_table
                .push(self.program.js_globals[id.0].path.clone());
            next
        });
        JsGlobalRef(index)
    }
    fn global(&mut self, id: GlobalId) -> Result<GlobalRef, String> {
        if let Some(index) = id.0.checked_sub(self.start.globals) {
            return Ok(GlobalRef::Own(index as u32));
        }
        if let Some(&index) = self.external_globals.get(&id.0) {
            return Ok(GlobalRef::External(index));
        }
        let binding = &self.program.globals[id.0];
        let key = (binding.namespace.clone(), binding.name.clone());
        if self.names.get(&key) != Some(&id) {
            return Err(format!(
                "{}/{} names more than one global",
                binding.namespace, binding.name
            ));
        }
        let index = self.references.globals.len() as u32;
        self.references.globals.push(ExternalGlobal {
            namespace: binding.namespace.clone(),
            name: binding.name.clone(),
            declared: binding.declared,
            private: self
                .macro_host
                .runtime_var(&format!("{}/{}", binding.namespace, binding.name)),
        });
        self.external_globals.insert(id.0, index);
        Ok(GlobalRef::External(index))
    }
    fn optional_global(&mut self, id: Option<GlobalId>) -> Result<Option<GlobalRef>, String> {
        id.map(|id| self.global(id)).transpose()
    }
    fn protocol_method(&mut self, method: &ir::ProtocolMethod) -> Result<ProtocolMethod, String> {
        Ok(ProtocolMethod {
            global: self.global(method.global)?,
            key: method.key.clone(),
            arities: method.arities.iter().map(|&a| a as u32).collect(),
            variadic: method.variadic.map(|v| v as u32),
        })
    }
    fn method(&self, method: &ir::FnMethod) -> Result<FnMethod, String> {
        Ok(FnMethod {
            params: self.locals(&method.params)?,
            rest: method.rest.map(|id| self.local(id)).transpose()?,
            body: self.expr(method.body)?,
            target: self.recur(method.target)?,
            captures: self.locals(&method.captures)?,
            asynchronous: method.asynchronous,
        })
    }
    fn methods(
        &self,
        methods: &[(ir::MethodDispatch, ir::FnMethod)],
    ) -> Result<Vec<(MethodDispatch, FnMethod)>, String> {
        methods
            .iter()
            .map(|(dispatch, method)| Ok((dispatch.clone(), self.method(method)?)))
            .collect()
    }
    fn bindings(
        &self,
        bindings: &[(ir::LocalId, ir::ExprId)],
    ) -> Result<Vec<(LocalRef, ExprRef)>, String> {
        bindings
            .iter()
            .map(|&(local, init)| Ok((self.local(local)?, self.expr(init)?)))
            .collect()
    }
    fn pairs(&self, pairs: &[(ir::ExprId, ir::ExprId)]) -> Result<Vec<(ExprRef, ExprRef)>, String> {
        pairs
            .iter()
            .map(|&(a, b)| Ok((self.expr(a)?, self.expr(b)?)))
            .collect()
    }

    /// A var form's metadata: its own expression, or the one its global had
    /// before this namespace, which the form shares.
    fn var_metadata(
        &mut self,
        value: ir::ExprId,
        metadata: Option<ir::ExprId>,
    ) -> Result<Option<VarMetadata>, String> {
        let Some(metadata) = metadata else {
            return Ok(None);
        };
        if metadata.0 >= self.start.exprs {
            return Ok(Some(VarMetadata::Expr(self.expr(metadata)?)));
        }
        match self.program.exprs[value.0].kind {
            ir::ExprKind::Global(global)
                if global.0 < self.start.globals
                    && self.start.globals_before[global.0].metadata == Some(metadata) =>
            {
                Ok(Some(VarMetadata::Global(self.global(global)?)))
            }
            _ => Err(format!(
                "a var form refers to expression {} of an earlier namespace",
                metadata.0
            )),
        }
    }

    fn kind(&mut self, kind: &ir::ExprKind) -> Result<ExprKind, String> {
        use ir::ExprKind as K;
        Ok(match kind {
            K::Nil => ExprKind::Nil,
            K::Bool(value) => ExprKind::Bool(*value),
            K::Number(value) => ExprKind::Number(*value),
            K::String(id) => ExprKind::String(self.string(*id)),
            K::ReaderLiteral {
                kind,
                items,
                quoted,
            } => ExprKind::ReaderLiteral {
                kind: *kind,
                items: self.exprs(items)?,
                quoted: *quoted,
            },
            K::DefinitionTest { value, unit } => ExprKind::DefinitionTest {
                value: self.expr(*value)?,
                unit: self.unit(*unit)?,
            },
            K::WithMeta { value, metadata } => ExprKind::WithMeta {
                value: self.expr(*value)?,
                metadata: self.expr(*metadata)?,
            },
            K::Var {
                definition,
                value,
                symbol,
                metadata,
            } => ExprKind::Var {
                definition: *definition,
                value: self.expr(*value)?,
                symbol: self.named(*symbol)?,
                metadata: self.var_metadata(*value, *metadata)?,
            },
            K::Keyword(id) => ExprKind::Keyword(self.named(*id)?),
            K::Symbol(id) => ExprKind::Symbol(self.named(*id)?),
            K::List(items) => ExprKind::List(self.exprs(items)?),
            K::Vector(items) => ExprKind::Vector(self.exprs(items)?),
            K::Map(items) => ExprKind::Map(self.exprs(items)?),
            K::Set(items) => ExprKind::Set(self.exprs(items)?),
            K::LiteralMap(items) => ExprKind::LiteralMap(self.exprs(items)?),
            K::LiteralSet(items) => ExprKind::LiteralSet(self.exprs(items)?),
            K::CoreFn(id) => ExprKind::CoreFn(*id),
            K::Local(id) => ExprKind::Local(self.local(*id)?),
            K::Global(id) => ExprKind::Global(self.global(*id)?),
            K::JsGlobal(id) => ExprKind::JsGlobal(self.js_global(*id)),
            K::ThisAs { binding, body } => ExprKind::ThisAs {
                binding: self.local(*binding)?,
                body: self.expr(*body)?,
            },
            K::JsArguments => ExprKind::JsArguments,
            K::JsTemplate { segments, args } => ExprKind::JsTemplate {
                segments: segments.clone(),
                args: self.exprs(args)?,
            },
            K::JsIndex { receiver, indices } => ExprKind::JsIndex {
                receiver: self.expr(*receiver)?,
                indices: self.exprs(indices)?,
            },
            K::JsDelete { receiver, key } => ExprKind::JsDelete {
                receiver: self.expr(*receiver)?,
                key: self.expr(*key)?,
            },
            K::QualifiedMethod { class, method } => ExprKind::QualifiedMethod {
                class: self.expr(*class)?,
                method: method.clone(),
            },
            K::CopyArguments(id) => ExprKind::CopyArguments(self.expr(*id)?),
            K::DefProtocol {
                global,
                methods,
                via_metadata,
            } => ExprKind::DefProtocol {
                global: self.global(*global)?,
                methods: methods
                    .iter()
                    .map(|method| self.protocol_method(method))
                    .collect::<Result<_, _>>()?,
                via_metadata: *via_metadata,
            },
            K::ProtocolPredicate {
                protocol,
                value,
                key,
                direct,
            } => ExprKind::ProtocolPredicate {
                protocol: self.expr(*protocol)?,
                value: self.expr(*value)?,
                key: key.clone(),
                direct: *direct,
            },
            K::ExtendType {
                instance,
                return_target,
                target,
                methods,
            } => ExprKind::ExtendType {
                instance: *instance,
                return_target: *return_target,
                target: self.expr(*target)?,
                methods: self.methods(methods)?,
            },
            K::Reify {
                metadata,
                captures,
                methods,
            } => ExprKind::Reify {
                metadata: self.optional(*metadata)?,
                captures: self.locals(captures)?,
                methods: self.methods(methods)?,
            },
            K::DefType {
                record,
                global,
                methods,
                layout,
            } => ExprKind::DefType {
                record: *record,
                global: self.global(*global)?,
                methods: self.methods(methods)?,
                layout: Box::new(TypeLayout {
                    constructor: self.optional_global(layout.constructor)?,
                    body: self.optional(layout.body)?,
                    masks: self.optional(layout.masks)?,
                    map_constructor: self.optional_global(layout.map_constructor)?,
                    fields: layout.fields.clone(),
                }),
            },
            K::Construct { callee, args } => ExprKind::Construct {
                callee: self.expr(*callee)?,
                args: self.exprs(args)?,
            },
            K::Assert {
                test,
                message,
                form,
            } => ExprKind::Assert {
                test: self.expr(*test)?,
                message: self.optional(*message)?,
                form: self.string(*form),
            },
            K::Def {
                global,
                init,
                test,
                once,
            } => ExprKind::Def {
                global: self.global(*global)?,
                init: self.optional(*init)?,
                test: self.optional(*test)?,
                once: *once,
            },
            K::SetGlobal { target, value } => ExprKind::SetGlobal {
                target: self.expr(*target)?,
                value: self.expr(*value)?,
            },
            K::Binding { bindings, body } => ExprKind::Binding {
                bindings: self.pairs(bindings)?,
                body: self.expr(*body)?,
            },
            K::WithOutStr(id) => ExprKind::WithOutStr(self.expr(*id)?),
            K::Exists(id) => ExprKind::Exists(self.expr(*id)?),
            K::Loadable { target, class } => ExprKind::Loadable {
                target: LazyRef(
                    target
                        .0
                        .checked_sub(self.start.lazy_targets)
                        .ok_or("the IR refers to a lazy target of an earlier namespace")?
                        as u32,
                ),
                class: self.expr(*class)?,
            },
            K::LoadableType => ExprKind::LoadableType,
            K::BindingExists(id) => ExprKind::BindingExists(self.expr(*id)?),
            K::BooleanHint(id) => ExprKind::BooleanHint(self.expr(*id)?),
            K::Property { receiver, property } => ExprKind::Property {
                receiver: self.expr(*receiver)?,
                property: property.clone(),
            },
            K::MethodCall {
                receiver,
                method,
                args,
            } => ExprKind::MethodCall {
                receiver: self.expr(*receiver)?,
                method: method.clone(),
                args: self.exprs(args)?,
            },
            K::If { test, yes, no } => ExprKind::If {
                test: self.expr(*test)?,
                yes: self.expr(*yes)?,
                no: self.expr(*no)?,
            },
            K::Do(items) => ExprKind::Do(self.exprs(items)?),
            K::Let { bindings, body } => ExprKind::Let {
                bindings: self.bindings(bindings)?,
                body: self.expr(*body)?,
            },
            K::LetFn { bindings, body } => ExprKind::LetFn {
                bindings: self.bindings(bindings)?,
                body: self.expr(*body)?,
            },
            K::Loop {
                target,
                bindings,
                body,
            } => ExprKind::Loop {
                target: self.recur(*target)?,
                bindings: self.bindings(bindings)?,
                body: self.expr(*body)?,
            },
            K::Recur { target, args } => ExprKind::Recur {
                target: self.recur(*target)?,
                args: self.exprs(args)?,
            },
            K::Throw(id) => ExprKind::Throw(self.expr(*id)?),
            K::Await(id) => ExprKind::Await(self.expr(*id)?),
            K::Try {
                body,
                catches,
                finally,
            } => ExprKind::Try {
                body: self.expr(*body)?,
                catches: catches
                    .iter()
                    .map(|catch| {
                        Ok(CatchClause {
                            class: self.optional(catch.class)?,
                            binding: self.local(catch.binding)?,
                            body: self.expr(catch.body)?,
                        })
                    })
                    .collect::<Result<_, String>>()?,
                finally: self.optional(*finally)?,
            },
            K::Case {
                strict,
                keyword_tests,
                test,
                branches,
                default,
            } => ExprKind::Case {
                strict: *strict,
                keyword_tests: *keyword_tests,
                test: self.expr(*test)?,
                branches: self.pairs(branches)?,
                default: self.optional(*default)?,
            },
            K::Fn { name, methods } => ExprKind::Fn {
                name: name.map(|id| self.local(id)).transpose()?,
                methods: methods
                    .iter()
                    .map(|method| self.method(method))
                    .collect::<Result<_, _>>()?,
            },
            K::Call { callee, args } => ExprKind::Call {
                callee: self.expr(*callee)?,
                args: self.exprs(args)?,
            },
            K::InstanceOf { class, value } => ExprKind::InstanceOf {
                class: self.expr(*class)?,
                value: self.expr(*value)?,
            },
        })
    }

    fn flags(binding: &GlobalBinding) -> GlobalFlags {
        GlobalFlags {
            type_descriptor: binding.type_descriptor,
            declared: binding.declared,
            constant: binding.constant,
            define: binding.define,
            boolean_tag: binding.boolean_tag,
        }
    }

    fn declarations(&mut self, input: &CaptureInput<'_>) -> Result<Declarations, String> {
        let program = self.program;
        let start = self.start;
        let host = self.macro_host;
        let mut globals = Vec::new();
        for binding in program.globals.tail_from(start.globals) {
            let key = format!("{}/{}", binding.namespace, binding.name);
            globals.push(Global {
                namespace: binding.namespace.clone(),
                name: binding.name.clone(),
                flags: Self::flags(binding),
                metadata: self.optional(binding.metadata)?,
                span: bytes(&binding.span),
                source: self.source(binding.source_index),
                private: host.runtime_var(&key),
                binding: host.source_bindings.get(&key).map(|kind| match kind {
                    SourceBindingKind::Value => BindingKind::Value,
                    SourceBindingKind::Macro => BindingKind::Macro,
                }),
            });
        }
        // Tables keyed by earlier entries may only keep them as they were.
        let mut protocols = Vec::new();
        for (global, methods) in program.protocols.iter() {
            // A protocol on a global an earlier namespace reserved is new
            // here; an earlier protocol must keep its methods.
            match start.protocols.get(global) {
                Some(before) if before == methods => continue,
                Some(_) => {
                    return Err("the namespace changed an earlier protocol's methods".into());
                }
                None => {}
            }
            let methods = methods
                .iter()
                .map(|method| self.protocol_method(method))
                .collect::<Result<Vec<_>, _>>()?;
            protocols.push((self.global(*global)?, methods));
        }
        protocols.sort_by_key(|(global, _)| order(global));
        let mut record_constructors = Vec::new();
        for global in program.record_constructors.iter() {
            if start.record_constructors.contains(global) {
                continue;
            }
            record_constructors.push(self.global(*global)?);
        }
        record_constructors.sort_by_key(order);
        let mut field_bindings = Vec::new();
        for (local, (target, mutable)) in program.field_bindings.iter() {
            if local.0 < start.locals {
                if start.field_bindings.get(local) != Some(&(*target, *mutable)) {
                    return Err("the namespace changed an earlier type's fields".into());
                }
                continue;
            }
            field_bindings.push((self.local(*local)?, self.recur(*target)?, *mutable));
        }
        field_bindings.sort_by_key(|(local, _, _)| local.0);
        let mut type_methods = Vec::new();
        for target in program.type_methods.iter() {
            if target.0 < start.recur_targets {
                if !start.type_methods.contains(target) {
                    return Err("the namespace changed an earlier type's methods".into());
                }
                continue;
            }
            type_methods.push(self.recur(*target)?);
        }
        type_methods.sort_by_key(|target| target.0);
        let mut macro_functions = Vec::new();
        for expr in program.macro_functions.iter() {
            if expr.0 < start.exprs {
                if !start.macro_functions.contains(expr) {
                    return Err("the namespace made an earlier function a macro".into());
                }
                continue;
            }
            macro_functions.push(self.expr(*expr)?);
        }
        macro_functions.sort_by_key(|expr| expr.0);
        for namespace in program.namespaces.range(0..start.namespaces) {
            if namespace != input.namespace
                && start.reader_contexts.get(namespace) != program.reader_contexts.get(namespace)
            {
                return Err(format!(
                    "the namespace changed the reader context of {namespace}"
                ));
            }
        }
        let reader_context = program
            .reader_contexts
            .get(input.namespace)
            .map(|context| ReaderContext {
                aliases: context.aliases.clone(),
                refers: context.refers.clone(),
                excludes: context.excludes.clone(),
                macros: context.macros.clone(),
            })
            .unwrap_or_default();
        let mut lazy_targets = Vec::new();
        for target in program.lazy_targets.iter().skip(start.lazy_targets) {
            lazy_targets.push(LazyTarget {
                site: self.expr(target.site)?,
                namespace: program.lazy_namespaces[target.namespace].clone(),
                global: self.optional_global(target.global)?,
            });
        }
        let declared = host.namespace_declarations(input.namespace);
        Ok(Declarations {
            globals,
            protocols,
            record_constructors,
            field_bindings,
            type_methods,
            macro_functions,
            macros: Vec::new(),
            aliases: input.aliases.to_vec(),
            requires: input.requires.to_vec(),
            refers: input.resolution.refers.clone(),
            excludes: input.resolution.excludes.clone(),
            js_refers: input
                .resolution
                .js_refers
                .iter()
                .map(|(local, refer)| {
                    (
                        local.clone(),
                        match refer {
                            ir::JsRefer::Global(path) => JsRefer::Global(path.clone()),
                            ir::JsRefer::Package(package, export) => {
                                JsRefer::Package(package.clone(), export.clone())
                            }
                        },
                    )
                })
                .collect(),
            reader_context,
            imports: declared
                .imports
                .iter()
                .map(|import| Import {
                    namespace: import.namespace.clone(),
                    alias: import.alias.clone(),
                    refers: import.refers.clone(),
                })
                .collect(),
            renames: declared.renames.to_vec(),
            namespace_imports: declared.namespace_imports.to_vec(),
            namespace_symbol: host.namespace_symbol(input.namespace).map(datum),
            namespace_doc: declared.doc.map(datum),
            lazy_targets,
        })
    }

    /// The earlier globals the analysis changed, as patches.
    fn patches(&mut self) -> Result<Vec<GlobalPatch>, String> {
        let program = self.program;
        let mut patches = Vec::new();
        for index in program
            .globals
            .changed_entries(&self.start.globals_before, self.start.globals)
        {
            let (before, after) = (&self.start.globals_before[index], &program.globals[index]);
            if (
                &before.namespace,
                &before.name,
                &before.span,
                before.source_index,
            ) != (
                &after.namespace,
                &after.name,
                &after.span,
                after.source_index,
            ) {
                return Err(format!(
                    "the namespace changed the identity of {}/{}",
                    before.namespace, before.name
                ));
            }
            let metadata = if after.metadata == before.metadata {
                None
            } else {
                Some(self.expr(after.metadata.ok_or_else(|| {
                    format!(
                        "the namespace removed {}/{}'s metadata",
                        after.namespace, after.name
                    )
                })?)?)
            };
            patches.push(GlobalPatch {
                global: self.global(GlobalId(index))?,
                flags: Self::flags(after),
                metadata,
            });
        }
        Ok(patches)
    }

    /// Compile-time JavaScript with the unit IDs its
    /// `globalThis.__lightCompilerTest(N)` calls name. The code is lexed, so
    /// the same text inside a string, template or comment is not a call.
    fn code(&self, text: String) -> Result<RelocatableCode, String> {
        use crate::js_syntax::Kind;
        let mut units = Vec::new();
        if text.contains("__lightCompilerTest") {
            let tokens =
                crate::js_syntax::lex_section(&text, 0..text.len(), false).map_err(|e| {
                    format!(
                        "compile-time code does not lex at {}: {}",
                        e.offset, e.message
                    )
                })?;
            for window in tokens.windows(6) {
                let [global, dot, name, open, id, close] = window else {
                    unreachable!()
                };
                if global.kind == Kind::Name
                    && global.text(&text) == "globalThis"
                    && dot.text(&text) == "."
                    && name.kind == Kind::Name
                    && name.text(&text) == "__lightCompilerTest"
                    && open.text(&text) == "("
                    && id.kind == Kind::Number
                    && close.text(&text) == ")"
                {
                    let unit = id
                        .text(&text)
                        .parse::<usize>()
                        .map_err(|e| format!("unit reference {}: {e}", id.text(&text)))?;
                    units.push(((id.start as u32, id.end as u32), self.unit(unit)?));
                }
            }
        }
        Ok(RelocatableCode { text, units })
    }

    /// A unit's or macro's view: this namespace's globals before `view`, and
    /// the program globals `added`.
    fn view(
        &mut self,
        view: usize,
        added: &[usize],
        context: Option<&ir::RuntimeReaderNamespace>,
    ) -> Result<RegistrationView, String> {
        Ok(RegistrationView {
            own_globals: view.saturating_sub(self.start.globals) as u32,
            added: added
                .iter()
                .map(|&id| self.global(GlobalId(id)))
                .collect::<Result<_, _>>()?,
            reader_context: context.map(|context| ReaderContext {
                aliases: context.aliases.clone(),
                refers: context.refers.clone(),
                excludes: context.excludes.clone(),
                macros: context.macros.clone(),
            }),
        })
    }

    fn units(&mut self, session: &CompilationSession) -> Result<DeferredUnits, String> {
        let namespace = self.program.namespaces[self.source_index].clone();
        let mut units = Vec::new();
        for (offset, record) in session
            .captured_units(self.start.units)?
            .into_iter()
            .enumerate()
        {
            let id = self.start.units + offset;
            let name = if record.name == format!("declaration#{id}") {
                UnitName::Declaration
            } else if let Some(owner) = record.name.strip_suffix(&format!(" :test#{id}")) {
                UnitName::Test(owner.to_owned())
            } else {
                UnitName::Var(record.name.clone())
            };
            let code = self.code(record.code.map_err(str::to_owned)?)?;
            units.push(Unit {
                name,
                generation: record.generation as u32,
                analyses: record.analyses as u32,
                emitted: record.emitted,
                code,
                view: self.view(
                    record.view_globals,
                    &record.added,
                    record
                        .reader_contexts
                        .as_ref()
                        .and_then(|contexts| contexts.get(&namespace))
                        .map(|context| &**context),
                )?,
                extension_frontier: record.extension_frontier.saturating_sub(self.start.bundles)
                    as u32,
            });
        }
        let own_method = format!("method@{}:", self.source_index);
        let mut registrations = Vec::new();
        for registration in session.captured_registrations(self.start.registrations) {
            registrations.push(match registration {
                RegistrationRecord::Root {
                    name,
                    unit,
                    test,
                    once,
                    location,
                } => Registration::Root {
                    name,
                    unit: unit.map(|id| self.unit(id)).transpose()?,
                    test: test.map(|id| self.unit(id)).transpose()?,
                    once,
                    location,
                },
                RegistrationRecord::Bundle(bundle) => Registration::Bundle {
                    products: bundle.products.clone(),
                    unit: self.unit(bundle.unit)?,
                    extensions: bundle
                        .extensions
                        .iter()
                        .map(|(protocol, selector, identity)| Extension {
                            protocol: protocol.clone(),
                            selector: selector.clone(),
                            identity: match identity
                                .strip_prefix(&own_method)
                                .and_then(|offset| offset.parse().ok())
                            {
                                Some(offset) => ExtensionIdentity::Method(offset),
                                None => ExtensionIdentity::Text(identity.clone()),
                            },
                        })
                        .collect(),
                    once: bundle.once,
                    group: bundle.group,
                    location: bundle.location.clone(),
                },
            });
        }
        Ok(DeferredUnits {
            units,
            registrations,
        })
    }

    fn macros(
        &mut self,
        namespace: &str,
        captured_macros: Vec<crate::compilation_session::CapturedMacro>,
    ) -> Result<Vec<MacroDeclaration>, String> {
        let prefix = format!("{namespace}/");
        let mut macros = Vec::new();
        for captured in captured_macros {
            let Some(name) = captured.key.strip_prefix(&prefix) else {
                return Err(format!("the namespace declared macro {}", captured.key));
            };
            let declaration = self
                .macro_host
                .javascript_macros
                .get(&captured.key)
                .ok_or_else(|| format!("macro {} is not declared", captured.key))?;
            let definition = declaration.definition();
            macros.push(MacroDeclaration {
                name: name.to_owned(),
                generation: declaration.generation() as u32,
                metadata: declaration
                    .metadata
                    .as_ref()
                    .map(datum)
                    .ok_or_else(|| format!("macro {} has no metadata", captured.key))?,
                definition: Site {
                    source: definition.source().name().to_owned(),
                    bytes: bytes(&definition.bytes()),
                },
                code: self.code(captured.code)?,
                view: self.view(captured.view_globals, &[], captured.reader_context.as_ref())?,
            });
        }
        // A later definition of the same name replaced the macro; the
        // artifact keeps the declaration the program ended with, whose
        // generation counts the ones before it.
        let mut kept: Vec<MacroDeclaration> = Vec::new();
        for declaration in macros.into_iter().rev() {
            if !kept.iter().any(|kept| kept.name == declaration.name) {
                kept.push(declaration);
            }
        }
        kept.reverse();
        Ok(kept)
    }
}

/// Reader data as the artifact owns it.
fn datum(source: &SourceDatum) -> Datum {
    let name = |name: &crate::compiler_data::Name| {
        (name.namespace().map(<[u16]>::to_vec), name.name().to_vec())
    };
    let kind = match source.kind() {
        SourceDatumKind::Nil => DatumKind::Nil,
        SourceDatumKind::Boolean(value) => DatumKind::Boolean(*value),
        SourceDatumKind::Integer(value) => DatumKind::Integer(*value),
        SourceDatumKind::Double(value) => DatumKind::Double(*value),
        SourceDatumKind::Ratio {
            numerator,
            denominator,
        } => DatumKind::Ratio {
            numerator: *numerator,
            denominator: *denominator,
        },
        SourceDatumKind::Character(value) => DatumKind::Character(*value),
        SourceDatumKind::String(value) => DatumKind::String(value.to_vec()),
        SourceDatumKind::Symbol(symbol) => {
            let (namespace, name) = name(symbol);
            DatumKind::Symbol(namespace, name)
        }
        SourceDatumKind::Keyword(keyword) => {
            let (namespace, name) = name(keyword);
            DatumKind::Keyword(namespace, name)
        }
        SourceDatumKind::Collection { kind, children } => DatumKind::Collection {
            kind: *kind,
            children: children.iter().map(datum).collect(),
        },
        SourceDatumKind::CompilerLiteral { kind, payload } => DatumKind::CompilerLiteral {
            kind: *kind,
            payload: Box::new(datum(payload)),
        },
    };
    Datum {
        kind,
        metadata: source.metadata().map(|metadata| Box::new(datum(&metadata))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refused capture still takes its namespace's macro and expansion
    /// records, so the next namespace's capture does not see them.
    #[test]
    fn a_refused_capture_leaves_no_records_for_the_next() {
        let mut host = MacroHost::new();
        let session = CompilationSession::new();
        session.begin_capture();
        host.session = Some(session.clone());
        let mut program = crate::analyzer::empty_program();
        let start = begin(&mut program, 0, &[], Some(&session));
        session.capture_macro("lib.refused/m".into(), "rt => null".into(), 0, None);
        session.capture_expansion(crate::compilation_session::CapturedExpansion {
            key: "lib.refused/m".into(),
            generation: 1,
            definition_source: "lib/refused.cljs".into(),
            definition: 0..1,
            call: 2..3,
            source_index: 0,
        });
        let names = crate::name_index::NameIndex::default();
        let resolution = NamespaceResolution::default();
        // The program holds no namespace at source index 0: capture refuses.
        let refused = finish(
            &start,
            Completed {
                program: &program,
                names: &names,
                root: ir::ExprId(0),
                source_index: 0,
                warnings: &[],
            },
            CaptureInput {
                namespace: "lib.refused",
                path: "lib/refused.cljs",
                source_length: 0,
                entry: false,
                resolution: &resolution,
                aliases: &[],
                requires: &[],
                names_from: Default::default(),
                options: Default::default(),
                work: host.begin_namespace_work(),
                control: None,
            },
            &host,
        );
        assert!(refused.is_err());
        let (macros, expansions) = session.take_captured();
        assert!(macros.is_empty() && expansions.is_empty());
    }
}
