//! Install a [`NamespaceArtifact`] in place of analyzing its namespace
//! (docs/compiler-cache.md, "Cache contract" and "Representation and
//! relocation"): the inverse of [`crate::artifact_capture`].
//!
//! [`install`] appends the artifact's tables to the program at the source
//! frontier where analysis would have run, so `NamespaceAnalysis::finish`,
//! application preparation and emission see the program an analysis would
//! have left:
//!
//! - local references are offsets into the artifact's own tables, which land
//!   after the program's;
//! - an earlier namespace's global is found by namespace and name in the
//!   analysis's name index, and an earlier namespace by name in
//!   [`InstallIndex`], never by scanning the program;
//! - source indices become the current program's, and the patches to earlier
//!   globals are applied;
//! - the deferred units are registered again in the compilation session at
//!   the next unit IDs, prepared, with the compile-time JavaScript that names
//!   unit IDs rewritten, and the session's registrations replayed with them;
//! - the macro host gets the namespace's `ns` declarations, its Vars'
//!   compile-time bindings and its macros, whose functions are evaluated only
//!   when a later expansion needs them.
//!
//! Nothing is analyzed and no macro executes. Everything that can refuse the
//! artifact is checked before the program changes, so a refused artifact
//! leaves the compilation as it was and its source is analyzed instead.

use std::collections::HashMap;

use crate::analyzer::{self as ir, GlobalId, Program};
use crate::compilation_session::{BundleRecord, InstalledRegistration};
use crate::compiler_data::{Datum as SourceDatum, DatumKind as SourceDatumKind, Name};
use crate::macro_host::{MacroHost, MacroImport, SourceBindingKind};
use crate::namespace_artifact::*;

/// Indexes over the program that installation resolves names against. Each
/// covers the program's table up to where it was last extended, so building
/// them costs one pass over the program however many namespaces install.
#[derive(Default)]
pub(crate) struct InstallIndex {
    namespaces: HashMap<String, usize>,
    indexed_namespaces: usize,
    js_globals: HashMap<Vec<String>, usize>,
    indexed_js_globals: usize,
    lazy_namespaces: HashMap<String, usize>,
    indexed_lazy_namespaces: usize,
    /// The generated-name numbers the compilation holds: what it allocated
    /// and the windows it installed ([`crate::generated_names`]).
    taken: crate::generated_names::Taken,
    /// Where the allocators stood when the compilation last began
    /// allocating for itself: at its start, or after the last installation.
    fresh_from: Option<crate::generated_names::Counters>,
    /// The `gensym` spellings of the installed namespaces.
    gensyms: std::collections::HashSet<Vec<u16>>,
}

impl InstallIndex {
    /// Note where the compilation's own allocations begin.
    pub(crate) fn begin_generated_names(&mut self, from: crate::generated_names::Counters) {
        self.fresh_from = Some(from);
    }

    /// Hold what the compilation allocated since it last began allocating.
    fn take_fresh(&mut self) {
        let now = crate::generated_names::current();
        if let Some(from) = self.fresh_from.replace(now) {
            self.taken.take(&from, &now);
        }
    }

    fn extend(&mut self, program: &Program) {
        for (index, namespace) in program
            .namespaces
            .iter()
            .enumerate()
            .skip(self.indexed_namespaces)
        {
            self.namespaces.entry(namespace.clone()).or_insert(index);
        }
        self.indexed_namespaces = program.namespaces.len();
        for (index, global) in program
            .js_globals
            .iter()
            .enumerate()
            .skip(self.indexed_js_globals)
        {
            self.js_globals.entry(global.path.clone()).or_insert(index);
        }
        self.indexed_js_globals = program.js_globals.len();
        for (index, namespace) in program
            .lazy_namespaces
            .iter()
            .enumerate()
            .skip(self.indexed_lazy_namespaces)
        {
            self.lazy_namespaces
                .entry(namespace.clone())
                .or_insert(index);
        }
        self.indexed_lazy_namespaces = program.lazy_namespaces.len();
    }
}

/// The analysis an artifact installs into.
pub(crate) struct Target<'a> {
    pub program: &'a mut Program,
    pub names: &'a mut crate::name_index::NameIndex,
    pub index: &'a mut InstallIndex,
    /// The source index the namespace takes.
    pub source_index: usize,
    /// The compilation's warnings, which the namespace's join.
    pub warnings: &'a mut Vec<ir::AnalysisWarning>,
}

/// The source the loader resolved for the namespace, which the artifact
/// must describe, and the compilation it installs into.
pub(crate) struct InstallSource<'a> {
    pub namespace: &'a str,
    pub path: &'a str,
    pub text: &'a str,
    pub entry: bool,
    /// The compilation's analysis options, which must be the artifact's.
    pub options: ir::AnalysisOptions,
    /// The control that cancels the compilation, if any, which installation
    /// checks besides the compile-time session's.
    pub control: Option<&'a crate::evaluator::ExecutionControl>,
}

/// How many expressions installation converts between checks for
/// cancellation.
const CHECK_INTERVAL: usize = 4096;

/// Why an artifact was not installed.
#[derive(Debug)]
pub(crate) enum InstallError {
    /// It does not fit the compilation, which it left unchanged: the
    /// namespace is analyzed instead.
    Refused(String),
    /// Registering what it declares failed after the program changed, as
    /// the same registration would have failed the namespace's analysis.
    Failed(String),
}

/// Install `artifact` as the next source and return its body's root.
pub(crate) fn install(
    target: Target<'_>,
    artifact: &NamespaceArtifact,
    source: &InstallSource<'_>,
    macro_host: &mut MacroHost,
) -> Result<ir::ExprId, InstallError> {
    let Target {
        program,
        names,
        index,
        source_index,
        warnings,
    } = target;
    // A cancelled or failed compilation stays failed: a hit never stands in
    // for the work it stopped.
    if let Some(reason) = macro_host.halted(source.control) {
        return Err(InstallError::Failed(reason));
    }
    index.extend(program);
    index.take_fresh();
    let resolved = resolve(
        program,
        names,
        index,
        source_index,
        artifact,
        source,
        macro_host,
    )
    .map_err(InstallError::Refused)?;
    Installer {
        program,
        names,
        index,
        artifact,
        source,
        resolved,
        bases: Bases::default(),
        warnings,
    }
    .run(macro_host)
    .map_err(InstallError::Failed)
}

/// Hold the names an installed artifact generated, and keep every later
/// allocation of this compilation from producing one
/// ([`crate::generated_names`]).
fn reserve_generated_names(
    names: &GeneratedNames,
    index: &mut InstallIndex,
    macro_host: &MacroHost,
) -> Result<(), String> {
    index.taken.take(&names.from, &names.to);
    index.gensyms.extend(names.gensyms.iter().cloned());
    crate::generated_names::reserve(&names.to);
    index.fresh_from = Some(crate::generated_names::current());
    match macro_host.session_handle() {
        Some(session) => session.reserve_gensyms(&names.gensyms, names.gensym_to),
        None => Ok(()),
    }
}

/// What the artifact names outside itself, resolved before anything changes.
struct Resolved {
    source_index: usize,
    namespaces: Vec<usize>,
    globals: Vec<GlobalId>,
    /// Each external global's metadata when the namespace began, which a
    /// `(var x)` of it shares ([`VarMetadata::Global`]).
    metadata: Vec<Option<ir::ExprId>>,
    /// The namespace's own name is not yet in the program.
    new_namespace: bool,
    /// The session's next unit ID and bundle count.
    first_unit: usize,
    /// The warning kinds, as the compiler spells them.
    warning_kinds: Vec<&'static str>,
}

fn resolve(
    program: &Program,
    names: &crate::name_index::NameIndex,
    index: &InstallIndex,
    source_index: usize,
    artifact: &NamespaceArtifact,
    source: &InstallSource<'_>,
    macro_host: &MacroHost,
) -> Result<Resolved, String> {
    artifact.validate()?;
    // Warnings, errors and array checks depend on the analysis options, so
    // an artifact of other options would report what this build does not.
    let policy = crate::artifact_capture::analysis_policy(source.options);
    if artifact.header.policy != policy {
        return Err(format!(
            "it was compiled under {:?}, not {policy:?}",
            artifact.header.policy
        ));
    }
    admit(artifact.accounting.as_ref(), macro_host)?;
    // Generated names stay one per identity: an artifact whose window
    // overlaps numbers this compilation allocated or installed, as one
    // captured elsewhere can, or that spells a gensym an installed namespace
    // spells, would share a name with another identity.
    let generated = &artifact.names;
    if let Some(allocator) = index.taken.overlap(&generated.from, &generated.to) {
        return Err(format!(
            "its {allocator} names overlap ones the compilation already holds"
        ));
    }
    if let Some(name) = generated
        .gensyms
        .iter()
        .find(|name| index.gensyms.contains(*name))
    {
        return Err(format!(
            "its gensym {} is an installed namespace's",
            String::from_utf16_lossy(name)
        ));
    }
    let header = &artifact.header;
    if header.namespace != source.namespace {
        return Err(format!(
            "the artifact is of {}, not {}",
            header.namespace, source.namespace
        ));
    }
    if header.path != source.path || header.source_length as usize != source.text.len() {
        return Err("the artifact describes another source file".into());
    }
    if header.entry != source.entry {
        return Err("the artifact was captured with another entry namespace".into());
    }
    let new_namespace = match index.namespaces.get(&header.namespace) {
        Some(&position) if position == source_index => false,
        Some(_) => return Err("the namespace is already in the program".into()),
        None if program.namespaces.len() == source_index => true,
        None => return Err("the program's namespaces are not its sources".into()),
    };
    let namespaces = artifact
        .references
        .namespaces
        .iter()
        .map(|name| {
            index
                .namespaces
                .get(name)
                .copied()
                .filter(|&position| position < source_index)
                .ok_or_else(|| format!("namespace {name} is not loaded before it"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut globals = Vec::with_capacity(artifact.references.globals.len());
    let mut metadata = Vec::with_capacity(artifact.references.globals.len());
    for external in &artifact.references.globals {
        let id = *names
            .get(&(external.namespace.clone(), external.name.clone()))
            .ok_or_else(|| format!("{}/{} is not declared", external.namespace, external.name))?;
        globals.push(id);
        metadata.push(program.globals[id.0].metadata);
    }
    // An external global ends declared as the capture saw it: by an earlier
    // namespace, or by this one's patch.
    let mut declared = globals
        .iter()
        .map(|id| program.globals[id.0].declared)
        .collect::<Vec<_>>();
    for patch in &artifact.patches {
        match patch.global {
            GlobalRef::External(external) => declared[external as usize] = patch.flags.declared,
            GlobalRef::Own(_) => return Err("a patch names the namespace's own global".into()),
        }
    }
    for (external, declared) in artifact.references.globals.iter().zip(declared) {
        if external.declared != declared {
            return Err(format!(
                "{}/{} is declared differently than when the namespace was captured",
                external.namespace, external.name
            ));
        }
    }
    for global in &artifact.declarations.globals {
        let key = (global.namespace.clone(), global.name.clone());
        if names.get(&key).is_some() {
            return Err(format!(
                "{}/{} is already a global of the program",
                global.namespace, global.name
            ));
        }
        if let Some(kind) = global.binding {
            let previous = macro_host
                .tables
                .source_bindings
                .get(&format!("{}/{}", global.namespace, global.name));
            if previous.is_some_and(|previous| *previous != source_binding(kind)) {
                return Err(format!(
                    "{}/{} is bound differently at compile time",
                    global.namespace, global.name
                ));
            }
        }
    }
    let check_var = |metadata_ref: &Option<VarMetadata>| match metadata_ref {
        Some(VarMetadata::Global(GlobalRef::External(external)))
            if metadata[*external as usize].is_none() =>
        {
            Err("a var form shares metadata an earlier global no longer has".to_owned())
        }
        Some(VarMetadata::Global(GlobalRef::Own(_))) => {
            Err("a var form shares the metadata of the namespace's own global".to_owned())
        }
        _ => Ok(()),
    };
    for expr in &artifact.ir.exprs {
        if let ExprKind::Var { metadata, .. } = &expr.kind {
            check_var(metadata)?;
        }
    }
    // Each unit was registered by its own root, in unit order.
    let rooted = artifact
        .units
        .registrations
        .iter()
        .filter_map(|registration| match registration {
            Registration::Root { unit, .. } => *unit,
            Registration::Bundle { .. } => None,
        })
        .enumerate()
        .all(|(index, unit)| unit.0 as usize == index);
    let roots = artifact
        .units
        .registrations
        .iter()
        .filter(|registration| matches!(registration, Registration::Root { unit: Some(_), .. }))
        .count();
    if !rooted || roots != artifact.units.units.len() {
        return Err("the deferred units are not registered in unit order".into());
    }
    let own_globals = artifact.declarations.globals.len();
    if artifact
        .units
        .units
        .iter()
        .map(|unit| &unit.view)
        .chain(artifact.declarations.macros.iter().map(|m| &m.view))
        .any(|view| view.own_globals as usize > own_globals)
    {
        return Err("a registration view names globals the namespace lacks".into());
    }
    for declaration in &artifact.declarations.macros {
        let key = format!("{}/{}", header.namespace, declaration.name);
        if macro_host
            .tables
            .source_bindings
            .get(&key)
            .is_some_and(|previous| *previous != SourceBindingKind::Macro)
        {
            return Err(format!("{key} is bound differently at compile time"));
        }
        let (start, end) = declaration.definition.bytes;
        if start > end || end as usize > source.text.len() {
            return Err(format!(
                "macro {} has no place in its source",
                declaration.name
            ));
        }
        if declaration.definition.source != source.path {
            return Err(format!(
                "macro {} was defined in another source",
                declaration.name
            ));
        }
    }
    let session = macro_host.session_handle();
    let first_unit = match &session {
        Some(session) => session.next_unit(),
        None if artifact.units.units.is_empty()
            && artifact.units.registrations.is_empty()
            && artifact.declarations.macros.is_empty() =>
        {
            0
        }
        None => return Err("the compilation has no compile-time session".into()),
    };
    let warning_kinds = artifact
        .diagnostics
        .warnings
        .iter()
        .map(|warning| warning_kind(&warning.kind))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Resolved {
        source_index,
        namespaces,
        globals,
        metadata,
        new_namespace,
        first_unit,
        warning_kinds,
    })
}

/// Whether the compilation's remaining budgets and its depth limits admit
/// the logical work the artifact recorded, as they admitted it when the
/// namespace was analyzed (docs/compiler-cache.md, "Loading, diagnostics,
/// and storage"). The counts are those of the namespace alone, so they are
/// charged on top of what the compilation used; the depths are the
/// namespace's own, so they meet the limits directly.
fn admit(accounting: Option<&Accounting>, macro_host: &MacroHost) -> Result<(), String> {
    let accounting = accounting.ok_or("it records no accounting of its work")?;
    let limits = &macro_host.limits;
    let left = |limit: usize, used: usize| limit.saturating_sub(used) as u64;
    let expansions = left(limits.expansions, macro_host.engine.expansions.get());
    if accounting.expansions > expansions {
        return Err(format!(
            "its {} macro expansions exceed the {expansions} the compilation has left",
            accounting.expansions
        ));
    }
    let nodes = left(
        limits.generated_nodes,
        macro_host.engine.generated_nodes.get(),
    );
    if accounting.node_peak > nodes {
        return Err(format!(
            "its {} generated nodes need room the compilation's {nodes} lack",
            accounting.node_peak
        ));
    }
    if accounting.conversion_peak > limits.generated_nodes as u64 {
        return Err(format!(
            "its data readers converted {} nodes, more than the limit of {}",
            accounting.conversion_peak, limits.generated_nodes
        ));
    }
    let depth = limits.expansion_depth.min(64) as u64;
    if u64::from(accounting.macro_depth) > depth || u64::from(accounting.unit_depth) > depth {
        return Err(format!(
            "its expansions nest deeper than the depth limit of {depth}"
        ));
    }
    if accounting.analysis_depth as usize > crate::macro_host::GENERATED_NESTING_LIMIT {
        return Err("its generated forms nest deeper than analysis allows".into());
    }
    Ok(())
}

/// Charge the compilation's budgets with an admitted artifact's counts,
/// once: the budgets only shrink, as its analysis would have shrunk them.
fn charge(accounting: &Accounting, macro_host: &MacroHost) {
    let expansions = accounting.expansions as usize;
    macro_host
        .engine
        .expansions
        .set(macro_host.engine.expansions.get() + expansions);
    let charged = &macro_host.engine.work.charged_expansions;
    charged.set(charged.get() + expansions);
    macro_host
        .engine
        .generated_nodes
        .set(macro_host.engine.generated_nodes.get() + accounting.generated_nodes as usize);
}

/// The compiler's spelling of a warning kind an artifact names by its text.
/// Each distinct kind is kept once for the process, and only so many, so
/// artifacts cannot grow the set without bound.
fn warning_kind(kind: &str) -> Result<&'static str, String> {
    static KINDS: std::sync::Mutex<Option<std::collections::HashSet<&'static str>>> =
        std::sync::Mutex::new(None);
    if kind.is_empty()
        || kind.len() > 64
        || !kind
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(format!(
            "its warning kind {kind:?} is not one the compiler reports"
        ));
    }
    let mut kinds = KINDS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let kinds = kinds.get_or_insert_with(Default::default);
    if let Some(&known) = kinds.get(kind) {
        return Ok(known);
    }
    if kinds.len() >= 256 {
        return Err("its warning kinds are more than the compiler reports".into());
    }
    let known: &'static str = Box::leak(kind.to_owned().into_boxed_str());
    kinds.insert(known);
    Ok(known)
}

fn source_binding(kind: BindingKind) -> SourceBindingKind {
    match kind {
        BindingKind::Value => SourceBindingKind::Value,
        BindingKind::Macro => SourceBindingKind::Macro,
    }
}

/// Where the artifact's own tables start in the program.
#[derive(Default)]
struct Bases {
    exprs: usize,
    locals: usize,
    recur_targets: usize,
    strings: usize,
    named_literals: usize,
    globals: usize,
    lazy_targets: usize,
    js_globals: Vec<ir::JsGlobalId>,
}

struct Installer<'a> {
    program: &'a mut Program,
    names: &'a mut crate::name_index::NameIndex,
    index: &'a mut InstallIndex,
    artifact: &'a NamespaceArtifact,
    source: &'a InstallSource<'a>,
    resolved: Resolved,
    bases: Bases,
    warnings: &'a mut Vec<ir::AnalysisWarning>,
}

impl Installer<'_> {
    fn expr(&self, id: ExprRef) -> ir::ExprId {
        ir::ExprId(self.bases.exprs + id.0 as usize)
    }
    fn exprs(&self, ids: &[ExprRef]) -> Vec<ir::ExprId> {
        ids.iter().map(|&id| self.expr(id)).collect()
    }
    fn optional(&self, id: Option<ExprRef>) -> Option<ir::ExprId> {
        id.map(|id| self.expr(id))
    }
    fn local(&self, id: LocalRef) -> ir::LocalId {
        ir::LocalId(self.bases.locals + id.0 as usize)
    }
    fn locals(&self, ids: &[LocalRef]) -> Vec<ir::LocalId> {
        ids.iter().map(|&id| self.local(id)).collect()
    }
    fn recur(&self, id: RecurRef) -> ir::RecurTargetId {
        ir::RecurTargetId(self.bases.recur_targets + id.0 as usize)
    }
    fn string(&self, id: StringRef) -> ir::StringId {
        ir::StringId(self.bases.strings + id.0 as usize)
    }
    fn named(&self, id: NamedRef) -> ir::NamedLiteralId {
        ir::NamedLiteralId(self.bases.named_literals + id.0 as usize)
    }
    fn global(&self, global: GlobalRef) -> GlobalId {
        match global {
            GlobalRef::Own(index) => GlobalId(self.bases.globals + index as usize),
            GlobalRef::External(index) => self.resolved.globals[index as usize],
        }
    }
    fn optional_global(&self, global: Option<GlobalRef>) -> Option<GlobalId> {
        global.map(|global| self.global(global))
    }
    fn source_index(&self, source: SourceRef) -> usize {
        match source {
            SourceRef::Own => self.resolved.source_index,
            SourceRef::Namespace(index) => self.resolved.namespaces[index as usize],
        }
    }
    fn unit(&self, unit: UnitRef) -> usize {
        self.resolved.first_unit + unit.0 as usize
    }

    fn run(mut self, macro_host: &mut MacroHost) -> Result<ir::ExprId, String> {
        let artifact = self.artifact;
        let namespace = artifact.header.namespace.as_str();
        if let Some(accounting) = &artifact.accounting {
            charge(accounting, macro_host);
        }
        reserve_generated_names(&artifact.names, self.index, macro_host)?;
        if self.resolved.new_namespace {
            self.program.namespaces.push(namespace.to_owned());
        }
        // Prepared units and macros keep the namespaces: shared, not copied.
        self.program.namespaces.freeze();
        let program = &*self.program;
        self.bases = Bases {
            exprs: program.exprs.len(),
            locals: program.locals.len(),
            recur_targets: program.recur_targets.len(),
            strings: program.strings.len(),
            named_literals: program.named_literals.len(),
            globals: program.globals.len(),
            lazy_targets: program.lazy_targets.len(),
            js_globals: Vec::new(),
        };
        let ir = &artifact.ir;
        self.program.strings.extend(ir.strings.iter().cloned());
        let literals = ir
            .named_literals
            .iter()
            .map(|literal| ir::NamedLiteral {
                namespace: literal.namespace.map(|id| self.string(id)),
                name: self.string(literal.name),
            })
            .collect::<Vec<_>>();
        self.program.named_literals.extend(literals);
        let mut js_globals = Vec::with_capacity(ir.js_globals.len());
        for path in &ir.js_globals {
            let id = match self.index.js_globals.get(path) {
                Some(&id) => id,
                None => {
                    let id = self.program.js_globals.len();
                    self.program
                        .js_globals
                        .push(ir::JsGlobal { path: path.clone() });
                    self.index.js_globals.insert(path.clone(), id);
                    id
                }
            };
            js_globals.push(ir::JsGlobalId(id));
        }
        self.index.indexed_js_globals = self.program.js_globals.len();
        self.bases.js_globals = js_globals;
        // The registration views of the namespace's units and macros: the
        // program's globals up to each view's frontier.
        let mut frontiers = artifact
            .units
            .units
            .iter()
            .map(|unit| unit.view.own_globals)
            .chain(
                artifact
                    .declarations
                    .macros
                    .iter()
                    .map(|declaration| declaration.view.own_globals),
            )
            .collect::<Vec<_>>();
        frontiers.sort_unstable();
        frontiers.dedup();
        let mut views = HashMap::new();
        let mut next_view = frontiers.iter().peekable();
        for (index, global) in artifact.declarations.globals.iter().enumerate() {
            while next_view
                .next_if(|&&frontier| frontier as usize == index)
                .is_some()
            {
                self.program.globals.freeze();
                views.insert(index as u32, self.program.globals.clone());
            }
            let binding = ir::GlobalBinding {
                type_descriptor: global.flags.type_descriptor,
                declared: global.flags.declared,
                constant: global.flags.constant,
                define: global.flags.define,
                boolean_tag: global.flags.boolean_tag,
                metadata: self.optional(global.metadata),
                namespace: global.namespace.clone(),
                name: global.name.clone(),
                span: span(global.span),
                source_index: self.source_index(global.source),
            };
            let id = GlobalId(self.program.globals.len());
            self.program.globals.push(binding);
            self.names
                .insert((global.namespace.clone(), global.name.clone()), id);
        }
        if next_view.peek().is_some() {
            self.program.globals.freeze();
            for &frontier in next_view {
                views.insert(frontier, self.program.globals.clone());
            }
        }
        let locals = ir
            .locals
            .iter()
            .map(|local| ir::LocalBinding {
                name: local.name.clone(),
                span: span(local.span),
                source_index: self.source_index(local.source),
            })
            .collect::<Vec<_>>();
        self.program.locals.extend(locals);
        let targets = ir
            .recur_targets
            .iter()
            .map(|target| ir::RecurTarget {
                params: self.locals(&target.params),
                protocol_impl: target.protocol_impl,
                receiver_from_this: target.receiver_from_this,
                used: target.used,
                argument_slots: target
                    .argument_slots
                    .iter()
                    .map(|slot| slot.map(|slot| slot as usize))
                    .collect(),
            })
            .collect::<Vec<_>>();
        self.program.recur_targets.extend(targets);
        let mut exprs = Vec::with_capacity(ir.exprs.len());
        for (converted, expr) in ir.exprs.iter().enumerate() {
            if converted % CHECK_INTERVAL == 0 {
                self.check(macro_host)?;
            }
            exprs.push(ir::Expr {
                kind: self.kind(&expr.kind),
                span: span(expr.span),
                context: expr.context,
                source_index: self.source_index(expr.source),
            });
        }
        self.program.exprs.extend(exprs);
        let declarations = &artifact.declarations;
        for target in &declarations.lazy_targets {
            let lazy_namespace = match self.index.lazy_namespaces.get(&target.namespace) {
                Some(&index) => index,
                None => {
                    let index = self.program.lazy_namespaces.len();
                    self.program.lazy_namespaces.push(target.namespace.clone());
                    self.index
                        .lazy_namespaces
                        .insert(target.namespace.clone(), index);
                    index
                }
            };
            let lazy = ir::LazyTarget {
                site: self.expr(target.site),
                namespace: lazy_namespace,
                global: self.optional_global(target.global),
            };
            self.program.lazy_targets.push(lazy);
        }
        self.index.indexed_lazy_namespaces = self.program.lazy_namespaces.len();
        for patch in &artifact.patches {
            let id = self.global(patch.global);
            let metadata = self.optional(patch.metadata);
            let global = &mut self.program.globals[id.0];
            global.type_descriptor = patch.flags.type_descriptor;
            global.declared = patch.flags.declared;
            global.constant = patch.flags.constant;
            global.define = patch.flags.define;
            global.boolean_tag = patch.flags.boolean_tag;
            if metadata.is_some() {
                global.metadata = metadata;
            }
        }
        for (global, methods) in &declarations.protocols {
            let methods = methods
                .iter()
                .map(|method| self.protocol_method(method))
                .collect();
            let global = self.global(*global);
            self.program.protocols.insert(global, methods);
        }
        for &global in &declarations.record_constructors {
            let global = self.global(global);
            self.program.record_constructors.insert(global);
        }
        for &(local, target, mutable) in &declarations.field_bindings {
            let entry = (self.local(local), (self.recur(target), mutable));
            self.program.field_bindings.insert(entry.0, entry.1);
        }
        for &target in &declarations.type_methods {
            let target = self.recur(target);
            self.program.type_methods.insert(target);
        }
        for &expr in &declarations.macro_functions {
            let expr = self.expr(expr);
            self.program.macro_functions.insert(expr);
        }
        let context = &declarations.reader_context;
        self.program.reader_contexts.insert(
            namespace.to_owned(),
            ir::RuntimeReaderNamespace {
                aliases: context.aliases.clone(),
                refers: context.refers.clone(),
                excludes: context.excludes.clone(),
                macros: context.macros.clone(),
            }
            .into(),
        );
        let root = self.expr(artifact.header.root);
        self.program
            .namespace_entries
            .insert(root, namespace.to_owned());
        self.index.extend(self.program);
        self.check(macro_host)?;
        self.declare(macro_host, &views)?;
        self.check(macro_host)?;
        // The namespace's warnings where its analysis reported them: the
        // analysis sorts the compilation's warnings by source and offset
        // when it ends, stably, and its own were captured in that order.
        let source_index = self.resolved.source_index;
        for (warning, &kind) in artifact
            .diagnostics
            .warnings
            .iter()
            .zip(&self.resolved.warning_kinds)
        {
            self.warnings.push(ir::AnalysisWarning {
                kind,
                message: warning.message.clone(),
                span: span(warning.span),
                source_index,
            });
        }
        self.warnings
            .sort_by_key(|warning| (warning.source_index, warning.span.start));
        Ok(root)
    }

    /// Stop when the compilation is cancelled or its compile-time session
    /// failed, without spending a poll budget: installation runs no
    /// JavaScript.
    fn check(&self, macro_host: &MacroHost) -> Result<(), String> {
        match macro_host.halted(self.source.control) {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }

    fn protocol_method(&self, method: &ProtocolMethod) -> ir::ProtocolMethod {
        ir::ProtocolMethod {
            global: self.global(method.global),
            key: method.key.clone(),
            arities: method.arities.iter().map(|&a| a as usize).collect(),
            variadic: method.variadic.map(|v| v as usize),
        }
    }
    fn method(&self, method: &FnMethod) -> ir::FnMethod {
        ir::FnMethod {
            params: self.locals(&method.params),
            rest: method.rest.map(|id| self.local(id)),
            body: self.expr(method.body),
            target: self.recur(method.target),
            captures: self.locals(&method.captures),
            asynchronous: method.asynchronous,
        }
    }
    fn methods(
        &self,
        methods: &[(MethodDispatch, FnMethod)],
    ) -> Vec<(ir::MethodDispatch, ir::FnMethod)> {
        methods
            .iter()
            .map(|(dispatch, method)| (dispatch.clone(), self.method(method)))
            .collect()
    }
    fn bindings(&self, bindings: &[(LocalRef, ExprRef)]) -> Vec<(ir::LocalId, ir::ExprId)> {
        bindings
            .iter()
            .map(|&(local, init)| (self.local(local), self.expr(init)))
            .collect()
    }
    fn pairs(&self, pairs: &[(ExprRef, ExprRef)]) -> Vec<(ir::ExprId, ir::ExprId)> {
        pairs
            .iter()
            .map(|&(a, b)| (self.expr(a), self.expr(b)))
            .collect()
    }

    fn kind(&self, kind: &ExprKind) -> ir::ExprKind {
        use ir::ExprKind as K;
        match kind {
            ExprKind::Nil => K::Nil,
            ExprKind::Bool(value) => K::Bool(*value),
            ExprKind::Number(value) => K::Number(*value),
            ExprKind::String(id) => K::String(self.string(*id)),
            ExprKind::ReaderLiteral {
                kind,
                items,
                quoted,
            } => K::ReaderLiteral {
                kind: *kind,
                items: self.exprs(items),
                quoted: *quoted,
            },
            ExprKind::DefinitionTest { value, unit } => K::DefinitionTest {
                value: self.expr(*value),
                unit: self.unit(*unit),
            },
            ExprKind::WithMeta { value, metadata } => K::WithMeta {
                value: self.expr(*value),
                metadata: self.expr(*metadata),
            },
            ExprKind::Var {
                definition,
                value,
                symbol,
                metadata,
            } => K::Var {
                definition: *definition,
                value: self.expr(*value),
                symbol: self.named(*symbol),
                metadata: match metadata {
                    None => None,
                    Some(VarMetadata::Expr(id)) => Some(self.expr(*id)),
                    Some(VarMetadata::Global(GlobalRef::External(external))) => {
                        self.resolved.metadata[*external as usize]
                    }
                    Some(VarMetadata::Global(GlobalRef::Own(_))) => {
                        unreachable!("refused by resolve")
                    }
                },
            },
            ExprKind::Keyword(id) => K::Keyword(self.named(*id)),
            ExprKind::Symbol(id) => K::Symbol(self.named(*id)),
            ExprKind::List(items) => K::List(self.exprs(items)),
            ExprKind::Vector(items) => K::Vector(self.exprs(items)),
            ExprKind::Map(items) => K::Map(self.exprs(items)),
            ExprKind::Set(items) => K::Set(self.exprs(items)),
            ExprKind::LiteralMap(items) => K::LiteralMap(self.exprs(items)),
            ExprKind::LiteralSet(items) => K::LiteralSet(self.exprs(items)),
            ExprKind::CoreFn(id) => K::CoreFn(*id),
            ExprKind::Local(id) => K::Local(self.local(*id)),
            ExprKind::Global(id) => K::Global(self.global(*id)),
            ExprKind::JsGlobal(id) => K::JsGlobal(self.bases.js_globals[id.0 as usize]),
            ExprKind::ThisAs { binding, body } => K::ThisAs {
                binding: self.local(*binding),
                body: self.expr(*body),
            },
            ExprKind::JsArguments => K::JsArguments,
            ExprKind::JsTemplate { segments, args } => K::JsTemplate {
                segments: segments.clone(),
                args: self.exprs(args),
            },
            ExprKind::JsIndex { receiver, indices } => K::JsIndex {
                receiver: self.expr(*receiver),
                indices: self.exprs(indices),
            },
            ExprKind::JsDelete { receiver, key } => K::JsDelete {
                receiver: self.expr(*receiver),
                key: self.expr(*key),
            },
            ExprKind::QualifiedMethod { class, method } => K::QualifiedMethod {
                class: self.expr(*class),
                method: method.clone(),
            },
            ExprKind::CopyArguments(id) => K::CopyArguments(self.expr(*id)),
            ExprKind::DefProtocol {
                global,
                methods,
                via_metadata,
            } => K::DefProtocol {
                global: self.global(*global),
                methods: methods
                    .iter()
                    .map(|method| self.protocol_method(method))
                    .collect(),
                via_metadata: *via_metadata,
            },
            ExprKind::ProtocolPredicate {
                protocol,
                value,
                key,
                direct,
            } => K::ProtocolPredicate {
                protocol: self.expr(*protocol),
                value: self.expr(*value),
                key: key.clone(),
                direct: *direct,
            },
            ExprKind::ExtendType {
                instance,
                return_target,
                target,
                methods,
            } => K::ExtendType {
                instance: *instance,
                return_target: *return_target,
                target: self.expr(*target),
                methods: self.methods(methods),
            },
            ExprKind::Reify {
                metadata,
                captures,
                methods,
            } => K::Reify {
                metadata: self.optional(*metadata),
                captures: self.locals(captures),
                methods: self.methods(methods),
            },
            ExprKind::DefType {
                record,
                global,
                methods,
                layout,
            } => K::DefType {
                record: *record,
                global: self.global(*global),
                methods: self.methods(methods),
                layout: Box::new(ir::TypeLayout {
                    constructor: self.optional_global(layout.constructor),
                    body: self.optional(layout.body),
                    masks: self.optional(layout.masks),
                    map_constructor: self.optional_global(layout.map_constructor),
                    fields: layout.fields.clone(),
                }),
            },
            ExprKind::Construct { callee, args } => K::Construct {
                callee: self.expr(*callee),
                args: self.exprs(args),
            },
            ExprKind::Assert {
                test,
                message,
                form,
            } => K::Assert {
                test: self.expr(*test),
                message: self.optional(*message),
                form: self.string(*form),
            },
            ExprKind::Def {
                global,
                init,
                test,
                once,
            } => K::Def {
                global: self.global(*global),
                init: self.optional(*init),
                test: self.optional(*test),
                once: *once,
            },
            ExprKind::SetGlobal { target, value } => K::SetGlobal {
                target: self.expr(*target),
                value: self.expr(*value),
            },
            ExprKind::Binding { bindings, body } => K::Binding {
                bindings: self.pairs(bindings),
                body: self.expr(*body),
            },
            ExprKind::WithOutStr(id) => K::WithOutStr(self.expr(*id)),
            ExprKind::Exists(id) => K::Exists(self.expr(*id)),
            ExprKind::Loadable { target, class } => K::Loadable {
                target: ir::LazyTargetId(self.bases.lazy_targets + target.0 as usize),
                class: self.expr(*class),
            },
            ExprKind::LoadableType => K::LoadableType,
            ExprKind::BindingExists(id) => K::BindingExists(self.expr(*id)),
            ExprKind::BooleanHint(id) => K::BooleanHint(self.expr(*id)),
            ExprKind::Property { receiver, property } => K::Property {
                receiver: self.expr(*receiver),
                property: property.clone(),
            },
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => K::MethodCall {
                receiver: self.expr(*receiver),
                method: method.clone(),
                args: self.exprs(args),
            },
            ExprKind::If { test, yes, no } => K::If {
                test: self.expr(*test),
                yes: self.expr(*yes),
                no: self.expr(*no),
            },
            ExprKind::Do(items) => K::Do(self.exprs(items)),
            ExprKind::Let { bindings, body } => K::Let {
                bindings: self.bindings(bindings),
                body: self.expr(*body),
            },
            ExprKind::LetFn { bindings, body } => K::LetFn {
                bindings: self.bindings(bindings),
                body: self.expr(*body),
            },
            ExprKind::Loop {
                target,
                bindings,
                body,
            } => K::Loop {
                target: self.recur(*target),
                bindings: self.bindings(bindings),
                body: self.expr(*body),
            },
            ExprKind::Recur { target, args } => K::Recur {
                target: self.recur(*target),
                args: self.exprs(args),
            },
            ExprKind::Throw(id) => K::Throw(self.expr(*id)),
            ExprKind::Await(id) => K::Await(self.expr(*id)),
            ExprKind::Try {
                body,
                catches,
                finally,
            } => K::Try {
                body: self.expr(*body),
                catches: catches
                    .iter()
                    .map(|catch| ir::CatchClause {
                        class: self.optional(catch.class),
                        binding: self.local(catch.binding),
                        body: self.expr(catch.body),
                    })
                    .collect(),
                finally: self.optional(*finally),
            },
            ExprKind::Case {
                strict,
                keyword_tests,
                test,
                branches,
                default,
            } => K::Case {
                strict: *strict,
                keyword_tests: *keyword_tests,
                test: self.expr(*test),
                branches: self.pairs(branches),
                default: self.optional(*default),
            },
            ExprKind::Fn { name, methods } => K::Fn {
                name: name.map(|id| self.local(id)),
                methods: methods.iter().map(|method| self.method(method)).collect(),
            },
            ExprKind::Call { callee, args } => K::Call {
                callee: self.expr(*callee),
                args: self.exprs(args),
            },
            ExprKind::InstanceOf { class, value } => K::InstanceOf {
                class: self.expr(*class),
                value: self.expr(*value),
            },
        }
    }

    /// Compile-time JavaScript with each unit ID it names renumbered.
    fn code(&self, code: &RelocatableCode) -> String {
        let mut text = String::with_capacity(code.text.len());
        let mut at = 0;
        for &((start, end), unit) in &code.units {
            text.push_str(&code.text[at..start as usize]);
            text.push_str(&self.unit(unit).to_string());
            at = end as usize;
        }
        text.push_str(&code.text[at..]);
        text
    }

    /// A unit's or macro's prepared JavaScript with the declarations its
    /// registration names: the program's globals up to its view, and those
    /// its own analysis added.
    fn prepared(
        &self,
        code: &RelocatableCode,
        view: &RegistrationView,
        views: &HashMap<u32, crate::arena::Arena<ir::GlobalBinding>>,
    ) -> crate::emitter::DeferredUnit {
        let mut globals = views[&view.own_globals].clone();
        for &added in &view.added {
            globals.push(self.program.globals[self.global(added).0].clone());
        }
        let mut reader_contexts = self.program.reader_contexts.clone();
        let namespace = &self.artifact.header.namespace;
        match &view.reader_context {
            Some(context) => {
                reader_contexts.insert(
                    namespace.clone(),
                    ir::RuntimeReaderNamespace {
                        aliases: context.aliases.clone(),
                        refers: context.refers.clone(),
                        excludes: context.excludes.clone(),
                        macros: context.macros.clone(),
                    }
                    .into(),
                );
            }
            None => {
                reader_contexts.remove(namespace);
            }
        }
        crate::emitter::DeferredUnit::from_parts(
            self.code(code),
            self.program.namespaces.clone(),
            globals,
            reader_contexts,
        )
    }

    /// The namespace's declarations in the macro host and its units and
    /// registrations in the compilation session.
    fn declare(
        &mut self,
        macro_host: &mut MacroHost,
        views: &HashMap<u32, crate::arena::Arena<ir::GlobalBinding>>,
    ) -> Result<(), String> {
        let artifact = self.artifact;
        let namespace = artifact.header.namespace.as_str();
        let declarations = &artifact.declarations;
        macro_host.restore_namespace_declarations(
            namespace,
            declarations
                .imports
                .iter()
                .map(|import| MacroImport {
                    namespace: import.namespace.clone(),
                    alias: import.alias.clone(),
                    refers: import.refers.clone(),
                })
                .collect(),
            declarations.renames.clone(),
            declarations.namespace_imports.clone(),
            declarations.namespace_symbol.as_ref().map(datum),
            declarations.namespace_doc.as_ref().map(datum),
        );
        let bindings = declarations
            .globals
            .iter()
            .map(|global| {
                (
                    &global.namespace,
                    &global.name,
                    global.binding,
                    global.private,
                )
            })
            .chain(artifact.references.globals.iter().map(|external| {
                (
                    &external.namespace,
                    &external.name,
                    external.private.map(|_| BindingKind::Value),
                    external.private,
                )
            }));
        for (namespace, name, binding, private) in bindings {
            match (binding, private) {
                (_, Some(private)) => macro_host.register_runtime_var(namespace, name, private),
                (Some(kind), None) => {
                    macro_host.register_source_binding(namespace, name, source_binding(kind))
                }
                (None, None) => Ok(()),
            }?;
        }
        // Every macro was defined in this source (checked by `resolve`).
        let file = (!declarations.macros.is_empty())
            .then(|| crate::compiler_data::Source::new(self.source.path, self.source.text));
        for declaration in &declarations.macros {
            let Some(span) = file
                .as_ref()
                .and_then(|file| file.span(span(declaration.definition.bytes)))
            else {
                continue;
            };
            macro_host.register_source_binding(
                namespace,
                &declaration.name,
                SourceBindingKind::Macro,
            )?;
            macro_host.install_javascript_macro(
                format!("{namespace}/{}", declaration.name),
                crate::compiler_bootstrap::CompiledMacro::installed(
                    self.prepared(&declaration.code, &declaration.view, views),
                    span,
                    datum(&declaration.metadata),
                    declaration.generation as usize,
                ),
            );
        }
        let Some(session) = macro_host.session_handle() else {
            return Ok(());
        };
        let units = &artifact.units;
        let prepared = units
            .units
            .iter()
            .map(|unit| self.prepared(&unit.code, &unit.view, views))
            .collect();
        let unit_name = |unit: UnitRef| match &units.units[unit.0 as usize].name {
            UnitName::Var(name) => name.clone(),
            UnitName::Declaration => format!("declaration#{}", self.unit(unit)),
            UnitName::Test(owner) => format!("{owner} :test#{}", self.unit(unit)),
        };
        let own_method = format!("method@{}:", self.resolved.source_index);
        let registrations = units
            .registrations
            .iter()
            .map(|registration| match registration {
                Registration::Root {
                    name,
                    unit,
                    test,
                    once,
                    location,
                } => InstalledRegistration::Root {
                    name: unit.map_or_else(|| name.clone(), unit_name),
                    unit: unit.map(|unit| self.unit(unit)),
                    test: test.map(|unit| self.unit(unit)),
                    once: *once,
                    location: location.clone(),
                },
                Registration::Bundle {
                    products,
                    unit,
                    extensions,
                    once,
                    group,
                    location,
                } => InstalledRegistration::Bundle(std::rc::Rc::new(BundleRecord {
                    products: products.clone(),
                    unit: self.unit(*unit),
                    extensions: extensions
                        .iter()
                        .map(|extension| {
                            (
                                extension.protocol.clone(),
                                extension.selector.clone(),
                                match &extension.identity {
                                    ExtensionIdentity::Method(offset) => {
                                        format!("{own_method}{offset}")
                                    }
                                    ExtensionIdentity::Text(text) => text.clone(),
                                },
                            )
                        })
                        .collect(),
                    location: location.clone(),
                    once: *once,
                    group: *group,
                })),
            })
            .collect();
        session.install(prepared, registrations, self.resolved.source_index as u32)
    }
}

fn span(bytes: Bytes) -> std::ops::Range<usize> {
    bytes.0 as usize..bytes.1 as usize
}

/// Reader data as the compiler holds it.
fn datum(datum: &Datum) -> SourceDatum {
    let name =
        |namespace: &Option<Vec<u16>>, name: &[u16]| Name::from_utf16(namespace.as_deref(), name);
    let kind = match &datum.kind {
        DatumKind::Nil => SourceDatumKind::Nil,
        DatumKind::Boolean(value) => SourceDatumKind::Boolean(*value),
        DatumKind::Integer(value) => SourceDatumKind::Integer(*value),
        DatumKind::Double(value) => SourceDatumKind::Double(*value),
        DatumKind::Ratio {
            numerator,
            denominator,
        } => SourceDatumKind::Ratio {
            numerator: *numerator,
            denominator: *denominator,
        },
        DatumKind::Character(value) => SourceDatumKind::Character(*value),
        DatumKind::String(value) => SourceDatumKind::String(value.as_slice().into()),
        DatumKind::Symbol(namespace, value) => SourceDatumKind::Symbol(name(namespace, value)),
        DatumKind::Keyword(namespace, value) => SourceDatumKind::Keyword(name(namespace, value)),
        DatumKind::Collection { kind, children } => SourceDatumKind::Collection {
            kind: *kind,
            children: children.iter().map(self::datum).collect(),
        },
        DatumKind::CompilerLiteral { kind, payload } => SourceDatumKind::CompilerLiteral {
            kind: *kind,
            payload: self::datum(payload),
        },
    };
    let value = SourceDatum::new(kind);
    match &datum.metadata {
        Some(metadata) => value.with_metadata(self::datum(metadata)),
        None => value,
    }
}
