//! Conservative, optional application preparation over Rust semantic IR.
//! Compile-time unit emission never enters this boundary.
use crate::analyzer::{CoreFnId, ExprId, ExprKind, GlobalId, Program};
use crate::call_shapes::{CallShapes, FunctionShape};
use crate::core_operations::{Calls, Purity, Type};
use crate::inference::{Definition, Inference, ProgramFacts, TypeRole};
use crate::program_fragment::children;
use std::collections::HashMap;

#[cfg(test)]
thread_local! { static PREPARATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// How much whole-program work application preparation does before emission.
/// Levels are ordered: each one includes everything the previous one does.
/// See docs/advanced-optimization.md for the roadmap behind `Advanced`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OptimizationLevel {
    /// Emit every definition and call as analyzed. This output is the
    /// differential oracle that every other level must agree with.
    None,
    /// Conservative preparation: remove discarded definitions with inert
    /// initializers and specialize eager top-level calls. The default.
    #[default]
    Prepare,
    /// Release mode. It accepts the deliberate semantic differences listed
    /// under "Advanced-mode contract" in docs/advanced-optimization.md. So far
    /// it prepares like `Prepare` except that it also removes definitions whose
    /// initializers only call pure core operations, branch, bind locals or
    /// construct program types, and does not treat retained `js*` as an
    /// escape. Emission reads runtime bindings directly and embeds just the
    /// runtime units the program reaches (`crate::runtime_slice`); core
    /// operations the program may replace read through a cell. Types inferred
    /// over the prepared program ([`crate::inference`]) let emission drop
    /// `truth()` and write core operators, typed `=`, `count`, `nth` and record
    /// lookups directly. Calls of the program's own functions name the method
    /// they run ([`crate::call_shapes`]). Constants and dead branches fold
    /// ([`crate::folding`]) and single-use and small pure functions are
    /// inlined ([`crate::inlining`]) before those analyses. Later passes of
    /// that design hook in through [`OptimizationLevel::advanced`].
    Advanced,
}
impl OptimizationLevel {
    /// Every level from weakest to strongest, for differential tests.
    pub const ALL: [Self; 3] = [Self::None, Self::Prepare, Self::Advanced];

    /// The CLI spelling accepted by `--optimize` and [`str::parse`].
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Prepare => "prepare",
            Self::Advanced => "advanced",
        }
    }
    /// Whether the conservative removal and specialization passes run.
    pub fn prepares(self) -> bool {
        self >= Self::Prepare
    }
    /// Whether release-only passes may run. They must preserve results
    /// relative to `None` except under the advanced-mode contract.
    pub fn advanced(self) -> bool {
        self >= Self::Advanced
    }
}
/// `false` is [`OptimizationLevel::None`] and `true` is
/// [`OptimizationLevel::Prepare`], the meaning of the former boolean option.
impl From<bool> for OptimizationLevel {
    fn from(optimize: bool) -> Self {
        if optimize { Self::Prepare } else { Self::None }
    }
}
impl std::fmt::Display for OptimizationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
/// An optimization level name other than `none`, `prepare` or `advanced`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownOptimizationLevel(pub String);
impl std::fmt::Display for UnknownOptimizationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown optimization level {:?}: expected none, prepare or advanced",
            self.0
        )
    }
}
impl std::error::Error for UnknownOptimizationLevel {}
impl std::str::FromStr for OptimizationLevel {
    type Err = UnknownOptimizationLevel;
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|level| level.name() == name)
            .ok_or_else(|| UnknownOptimizationLevel(name.to_owned()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct ApplicationOptions {
    pub optimize: OptimizationLevel,
    /// Keep these runtime binding identities and all their source generations.
    pub exports: Vec<GlobalId>,
    /// Build-time values for the program's `goog-define` Vars, at every level
    /// ([`crate::defines`]). A define that names none of them, or whose value
    /// has another kind than the default, is ignored and listed by
    /// [`PreparedApplication::rejected_defines`].
    pub defines: Vec<crate::defines::Define>,
    /// At `Advanced`, leave every call of a program function a call
    /// ([`crate::inlining`] off; folding still runs), as Closure's
    /// `inlineFunctions` option does: for debugging and profiling with every
    /// stack frame, and for tests of what the passes after it do with
    /// functions.
    pub no_inlining: bool,
    /// At `Advanced`, leave every expression as analysis and inlining make it
    /// ([`crate::folding`] off; inlining and definition removal still run),
    /// as Closure's `foldConstants` option off does: for debugging, and for
    /// tests that compare what the constant pool holds before folding reads
    /// constants away.
    pub no_folding: bool,
    /// At `Advanced`, keep the output readable: the compiler's names
    /// (`g3_1`, `l7`, `k0`, `rt$add`), the runtime as written, with its
    /// comments, and the emitter's layout, as Closure's `--debug` and pretty
    /// printing do; for debugging, and for tests that read the output. Off,
    /// `Advanced` output is terse ([`crate::terse`]; docs/advanced-optimization.md,
    /// Phase 3). CLI: `--pseudo-names`.
    pub pseudo_names: bool,
}
impl ApplicationOptions {
    /// Options at `optimize` without explicit exports or defines. Accepts a
    /// level or the former boolean (`false` is `None`, `true` is `Prepare`).
    pub fn new(optimize: impl Into<OptimizationLevel>) -> Self {
        Self {
            optimize: optimize.into(),
            exports: Vec::new(),
            defines: Vec::new(),
            no_inlining: false,
            no_folding: false,
            pseudo_names: false,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplicationStats {
    pub removed_definitions: usize,
    pub specialized_calls: usize,
    pub retained_globals: usize,
    /// `Advanced`: `deftype` and `defrecord` forms whose type nothing
    /// instantiates, emitted as a bare constructor or not at all.
    pub uninstantiated_types: usize,
    /// `Advanced`: method entries of types, `reify` and `extend-type` that no
    /// instance can run.
    pub dropped_methods: usize,
    /// `Advanced`: protocol method calls that read their implementation from
    /// the receiver instead of calling the dispatcher.
    pub devirtualized_calls: usize,
    /// `Advanced`: calls of program functions that name the method they run
    /// and call it directly (`crate::call_shapes`).
    pub direct_calls: usize,
    /// `Advanced`: functions with several methods or rest arguments whose
    /// value never escapes, defined without an `arguments.length` dispatcher:
    /// retained globals and, since light-cljs-228, `let` and `letfn` locals.
    pub dispatchers_removed: usize,
    /// `Advanced`: methods of local functions without a dispatcher that
    /// capture nothing, bound once to a constant table entry instead of at
    /// each evaluation of their scope (light-cljs-228).
    pub hoisted_methods: usize,
    /// `Advanced`: parameters of closed functions whose type, joined over the
    /// arguments of every call (`crate::inference`), is narrower than `any`.
    pub typed_parameters: usize,
    /// `Advanced`: calls replaced with the body of the method they run
    /// (`crate::inlining`).
    pub inlined_calls: usize,
    /// `Advanced`: expressions rewritten by constant folding, including
    /// substituted constants, removed bindings and dead branches
    /// (`crate::folding`).
    pub folded_expressions: usize,
}
/// An owned semantic program plus emission decisions computed entirely in Rust.
/// Repeated emission does not rerun preparation or execute compiler callbacks.
pub struct PreparedApplication {
    level: OptimizationLevel,
    /// `Advanced` without [`ApplicationOptions::pseudo_names`]: terse output.
    terse: bool,
    program: Program,
    globals: Vec<bool>,
    calls: HashMap<ExprId, usize>,
    /// `Advanced`: runtime names of the core operations the program assigns.
    replaced_core: Vec<&'static str>,
    /// `Advanced`: the types of the prepared program.
    inference: Option<Inference>,
    /// `Advanced`: protocol method calls that read their implementation from
    /// the receiver's slot, with the slot (`crate::rta`).
    direct_methods: HashMap<ExprId, String>,
    /// `Advanced`: whether the program names `getBasis` (`crate::rta`).
    type_basis: bool,
    /// `Advanced`: the first expression that names an upstream field or
    /// method of the runtime's types (`crate::rta`).
    upstream_member: Option<ExprId>,
    /// `Advanced`: the units that install the record methods whose slots
    /// the program names, each with the first expression naming one
    /// (`crate::rta`).
    record_methods: Vec<(&'static str, ExprId)>,
    /// `Advanced`: the units the core protocol declarations the program names
    /// need, each with the first expression naming one (`crate::rta`).
    protocol_declarations: Vec<(&'static str, ExprId)>,
    /// `Advanced`: the units the members of light.lazy's `Loadable` the
    /// program names need, each with the first expression naming one
    /// (`crate::rta`).
    loadable_members: Vec<(&'static str, ExprId)>,
    /// `Advanced`: lookups of constant keys in map literals with constant keys
    /// (or locals bound to them), with the index of the entry each reads
    /// (`crate::folding::entry_lookups`).
    entry_lookups: HashMap<ExprId, usize>,
    /// `Advanced`: direct calls and dispatcher-free definitions
    /// (`crate::call_shapes`).
    shapes: Option<CallShapes>,
    /// `Advanced`: by expression, the discarded top-level definitions with
    /// inert initializers (`Retained::candidates`), which removal could drop
    /// when unread and a chunked build may therefore move toward their
    /// readers (docs/code-splitting.md, "Items and signatures"). Empty
    /// below `Advanced`, where every form stays in its namespace.
    removable: Vec<bool>,
    /// Defines of the options that did not apply.
    rejected_defines: Vec<crate::defines::DefineError>,
    stats: ApplicationStats,
}
impl PreparedApplication {
    fn emit(
        &self,
        module: bool,
        mapped: bool,
    ) -> (
        String,
        crate::runtime_slice::EmittedRuntime,
        Option<crate::source_map::SourceMapping>,
    ) {
        crate::emitter::emit_application(&self.program, self.application(), module, mapped)
    }
    /// What preparation decided for emission.
    fn application(&self) -> crate::emitter::Application<'_> {
        let advanced = self.level.advanced();
        crate::emitter::Application {
            globals: &self.globals,
            calls: &self.calls,
            direct: advanced.then_some(self.replaced_core.as_slice()),
            inference: self.inference.as_ref().filter(|_| advanced),
            methods: advanced.then_some(&self.direct_methods),
            entries: advanced.then_some(&self.entry_lookups),
            basis: !advanced || self.type_basis,
            upstream_member: self.upstream_member.filter(|_| advanced),
            record_methods: if advanced { &self.record_methods } else { &[] },
            protocol_declarations: if advanced {
                &self.protocol_declarations
            } else {
                &[]
            },
            loadable_members: if advanced {
                &self.loadable_members
            } else {
                &[]
            },
            shapes: self.shapes.as_ref().filter(|_| advanced),
            terse: self.terse,
        }
    }
    /// One JavaScript expression. At `Advanced` it embeds only the runtime
    /// units the program reaches; see [`PreparedApplication::retention`].
    pub fn emit_js(&self) -> String {
        self.emit(false, false).0
    }
    /// One ES module for a bundler; see [`crate::emitter::emit_esm`]. Its
    /// runtime is the one [`PreparedApplication::emit_js`] embeds.
    pub fn emit_esm(&self) -> String {
        self.emit(true, false).0
    }
    /// One ES module per chunk of the program's [`crate::chunks::plan`]
    /// (docs/code-splitting.md, "Phase 2"), in plan order: startup first,
    /// whose default export is the program's value, as
    /// [`PreparedApplication::emit_esm`]'s is. A loadable whose target
    /// another chunk holds imports that chunk, `import("./app.editor.js")`,
    /// unless the target has evaluated before the loadable's chunk runs.
    /// A program of one chunk, one without a loadable whose target another
    /// chunk holds, is one file: [`PreparedApplication::emit_esm`]'s.
    /// At `Advanced` definitions move toward their readers and chunks that
    /// would write no code are dropped ([`crate::chunks::plan`]), unless the
    /// program's forms reach reflection: then every form stays in its
    /// namespace's chunk ([`crate::chunks::namespace_plan`]), so a namespace
    /// the program can look up is never split across chunks.
    /// At `Advanced` each chunk embeds the runtime units of its signature
    /// (docs/code-splitting.md, "Phase 3", runtime units).
    pub fn emit_chunks(&self) -> crate::chunks::ChunkedOutput {
        self.emit_chunks_reported().0
    }
    /// [`PreparedApplication::emit_chunks`] and, for an `Advanced` build of
    /// more than one chunk, which runtime units each chunk embeds and why
    /// (`--size-report` with `--out-dir`). Every other build embeds the
    /// runtime [`PreparedApplication::retention`] reports, in startup.
    pub fn emit_chunks_reported(
        &self,
    ) -> (
        crate::chunks::ChunkedOutput,
        Option<crate::ChunkedRetention>,
    ) {
        let plan = crate::chunks::plan(&self.program, self);
        if let Some(output) = self.write_chunks(&plan, false) {
            return output;
        }
        let plan = crate::chunks::namespace_plan(&self.program, self);
        self.write_chunks(&plan, true)
            .expect("a plan that moves no definition is written")
    }
    /// The files of `plan` and their runtimes' report, or `None` when its
    /// forms reach reflection and it moves a definition out of its
    /// namespace's chunk or drops a chunk. `reflective`: the forms are known
    /// to reach reflection.
    fn write_chunks(
        &self,
        plan: &crate::chunks::ChunkPlan,
        reflective: bool,
    ) -> Option<(
        crate::chunks::ChunkedOutput,
        Option<crate::ChunkedRetention>,
    )> {
        if plan.chunks.len() == 1 {
            let emitted = vec![crate::emitter::EmittedChunk {
                source: self.emit_esm(),
                imports: Vec::new(),
                runtime: Default::default(),
            }];
            let output = crate::chunks::ChunkedOutput::new(&self.program, plan, emitted);
            return Some((output, None));
        }
        // The constant table and the runtime may add chunks to the plan.
        let (plan, emitted) =
            crate::emitter::emit_chunked(&self.program, self.application(), plan, reflective)?;
        let report = self.level.advanced().then(|| {
            let chunks = plan
                .chunks
                .iter()
                .zip(&emitted)
                .map(|(chunk, emitted)| (chunk.name.clone(), &emitted.runtime))
                .collect::<Vec<_>>();
            crate::ChunkedRetention::new(&self.program, self.level, &chunks)
        });
        let output = crate::chunks::ChunkedOutput::new(&self.program, &plan, emitted);
        Some((output, report))
    }
    /// [`PreparedApplication::emit_js`] (the same text) and its source map,
    /// which [`crate::source_map::SourceMapping::to_json`] writes against
    /// the program's sources.
    pub fn emit_js_mapped(&self) -> (String, crate::source_map::SourceMapping) {
        let (code, _, mapping) = self.emit(false, true);
        (code, mapping.expect("a mapped emission"))
    }
    /// [`PreparedApplication::emit_esm`] (the same text) and its source map.
    pub fn emit_esm_mapped(&self) -> (String, crate::source_map::SourceMapping) {
        let (code, _, mapping) = self.emit(true, true);
        (code, mapping.expect("a mapped emission"))
    }
    /// Which runtime units the emitted output embeds, each with its size and
    /// the runtime name and source expression that first demanded it. Below
    /// `Advanced` the output embeds the whole runtime and the report
    /// attributes what it can. `--size-report` prints this table.
    pub fn retention(&self) -> crate::RetentionReport {
        let (_, runtime, _) = self.emit(false, false);
        crate::RetentionReport::new(&self.program, self.level, &runtime)
    }
    pub fn stats(&self) -> &ApplicationStats {
        &self.stats
    }
    /// At `Advanced`, the runtime names of the core operations the program may
    /// replace, sorted: assigned with `set!`, `binding` or `with-redefs`
    /// (directly or through a property) or passed to a `js*` hole, which raw
    /// text could assign. Their reads go through an output-level cell; every
    /// other core operation is read from its unit binding. Empty below
    /// `Advanced`.
    pub fn replaced_core(&self) -> &[&'static str] {
        &self.replaced_core
    }
    /// At `Advanced`, the inferred types of the prepared program (see
    /// [`crate::inference`]), which emission uses to lower truth tests and core
    /// calls. `None` below `Advanced`.
    pub fn inference(&self) -> Option<&Inference> {
        self.inference.as_ref()
    }
    /// At `Advanced`, which calls name their method and which definitions
    /// leave out their dispatcher ([`crate::call_shapes`]). `None` below
    /// `Advanced`.
    pub fn call_shapes(&self) -> Option<&CallShapes> {
        self.shapes.as_ref()
    }
    /// The program this application emits: the analyzed program after
    /// build-time defines, definition removal and call specialization, and at
    /// `Advanced` folding, inlining and rapid type analysis.
    pub fn program(&self) -> &Program {
        &self.program
    }
    /// At `Advanced`, whether `id` is a discarded top-level definition with
    /// an inert initializer, one that removal drops when nothing reads it
    /// (`Retained::candidates`). Removability is movability: a chunked build
    /// moves such a definition toward its readers ([`crate::chunks::plan`]).
    /// False below `Advanced`.
    pub fn removable(&self, id: ExprId) -> bool {
        self.removable.get(id.0).copied().unwrap_or(false)
    }
    /// The level this application was prepared at. Level-specific emission
    /// (for example runtime slicing under `Advanced`) reads it here.
    pub fn level(&self) -> OptimizationLevel {
        self.level
    }
    /// The defines of [`ApplicationOptions::defines`] that did not apply: they
    /// name no `goog-define` of the program or give a value of another kind.
    pub fn rejected_defines(&self) -> &[crate::defines::DefineError] {
        &self.rejected_defines
    }
}

/// Preserve the entry result, explicit exports, and ordered observable effects.
/// Only discarded top-level definitions with provably inert initializers are
/// candidates for removal. Unknown calls, throws, metadata tests, declaration
/// bundles and standalone expressions stay in their original execution order.
pub fn prepare_application(
    program: &Program,
    mut options: ApplicationOptions,
) -> PreparedApplication {
    #[cfg(test)]
    PREPARATIONS.with(|count| count.set(count.get() + 1));
    let level = options.optimize;
    let mut prepared = PreparedApplication {
        level,
        terse: level.advanced() && !options.pseudo_names,
        program: program.clone(),
        globals: vec![false; program.globals.len()],
        calls: HashMap::new(),
        replaced_core: Vec::new(),
        inference: None,
        direct_methods: HashMap::new(),
        entry_lookups: HashMap::new(),
        type_basis: true,
        upstream_member: None,
        record_methods: Vec::new(),
        protocol_declarations: Vec::new(),
        loadable_members: Vec::new(),
        shapes: None,
        removable: Vec::new(),
        rejected_defines: Vec::new(),
        stats: ApplicationStats::default(),
    };
    // Build-time defines replace `goog-define` defaults at every level, as
    // upstream's `:closure-defines` do.
    prepared.rejected_defines = crate::defines::apply(&mut prepared.program, &options.defines);
    // A loadable reads its target only once loaded, so a lazy Var target is a
    // root like an export: kept, with a dispatcher, its arguments unknown.
    options.exports.extend(
        program
            .lazy_targets
            .iter()
            .filter_map(|target| target.global),
    );
    let mut retained = retain(&mut prepared, &options.exports);
    // Folding and inlining (Phase 2 step 8) alternate with definition removal:
    // folding removes dead branches and the reads in them, inlining the calls
    // of single-use and small pure functions, and removal then drops the
    // definitions nothing reads any more.
    if level.advanced() {
        let mut growth = crate::inlining::growth_budget(prepared.program.exprs.len());
        // Literals the program builds at each evaluation stay built, however
        // constant their items become.
        let mut built = crate::folding::built_literals(&prepared.program);
        for round in 0..SIMPLIFY_ROUNDS {
            let folded = if options.no_folding {
                0
            } else {
                crate::folding::fold(&mut prepared.program, &mut built)
            };
            prepared.stats.folded_expressions += folded;
            if round > 0 || folded > 0 {
                retained = retain(&mut prepared, &options.exports);
            }
            if round + 1 == SIMPLIFY_ROUNDS {
                break;
            }
            let inlined = if options.no_inlining {
                0
            } else {
                let shapes = crate::call_shapes::analyze(&prepared.program, None, &options.exports);
                crate::inlining::inline(&mut prepared.program, &shapes, &mut growth, &mut built)
            };
            prepared.stats.inlined_calls += inlined;
            if inlined == 0 && folded == 0 {
                break;
            }
        }
        crate::folding::keep_built(&mut prepared.program, &built);
        retained
            .candidates
            .resize(prepared.program.exprs.len(), false);
    }
    let Retained {
        top,
        candidates,
        raw,
    } = retained;
    if level.prepares() && !raw {
        specialize(&mut prepared, &top);
    }
    if !level.prepares() {
        prepared.globals.fill(true);
    }
    // Release-only passes (docs/advanced-optimization.md, Phases 1-3) run here,
    // after conservative preparation, under `level.advanced()`. So far `Advanced`
    // prepares like `Prepare` except that more initializers are inert (`Inert`)
    // and retained `js*` is not an escape; emission also slices the runtime.
    // Emission reads the runtime directly and keeps a mutable cell for each core
    // operation the program may replace (the scan `Inert` trusts).
    // Type inference (Phase 2 step 3) runs over the prepared program, whose
    // emission lowers truth tests and core calls with it.
    if level.advanced() {
        let mut top = Vec::new();
        flatten(&prepared.program, prepared.program.root, true, &mut top);
        let top: Vec<ExprId> = top.into_iter().map(|(id, _)| id).collect();
        let facts = ProgramFacts::new(&prepared.program, &top);
        prepared.replaced_core = facts.replaced_names();
        // Parameters of closed functions join the arguments of their calls
        // (light-cljs-229). Which functions are closed needs no types, so it is
        // decided here, before inference and over the program that rapid type
        // analysis then rewrites: its removals only drop calls, so the calls
        // found here include every call emission keeps.
        let closed = crate::call_shapes::closed_functions(&prepared.program, &options.exports);
        let inference = Inference::new(&prepared.program, facts, &closed);
        prepared.stats.typed_parameters = inference.typed_parameters();
        // Rapid type analysis (Phase 2 step 4) drops the methods no instance
        // can run, the types nothing instantiates and the definitions only those
        // read, and devirtualizes calls on receivers of one program type.
        let analysis =
            crate::rta::analyze(&prepared.program, &inference, &candidates, &options.exports);
        let rewrite = analysis.apply(&mut prepared.program, &mut prepared.globals, &candidates);
        prepared.direct_methods = analysis.direct_calls().clone();
        prepared.type_basis = analysis.names_basis();
        prepared.upstream_member = analysis.names_upstream_member();
        prepared.record_methods = analysis.named_record_methods().to_vec();
        prepared.protocol_declarations = analysis.named_protocol_declarations().to_vec();
        prepared.loadable_members = analysis.named_loadable_members().to_vec();
        prepared.stats.removed_definitions += rewrite.removed_definitions;
        prepared.stats.uninstantiated_types = rewrite.uninstantiated_types;
        prepared.stats.dropped_methods = rewrite.dropped_methods;
        prepared.stats.devirtualized_calls = rewrite.devirtualized_calls;
        // Call shapes (Phase 2 step 5) over the rewritten program: calls of
        // known functions name their method, and functions whose value never
        // escapes are defined without a dispatcher.
        let shapes =
            crate::call_shapes::analyze(&prepared.program, Some(&inference), &options.exports);
        prepared.stats.direct_calls = shapes.direct_calls();
        prepared.stats.dispatchers_removed = (0..prepared.program.globals.len())
            .filter(|global| {
                prepared.globals[*global]
                    && matches!(
                        shapes.function(GlobalId(*global)),
                        FunctionShape::Entries {
                            dispatcher: false,
                            ..
                        }
                    )
            })
            .count()
            + shapes.local_functions().len();
        prepared.stats.hoisted_methods = shapes.hoisted_methods();
        prepared.shapes = Some(shapes);
        // Lookups of constant keys in maps with constant keys read their entry
        // (light-cljs-221).
        prepared.entry_lookups =
            crate::folding::entry_lookups(&prepared.program, inference.facts());
        prepared.inference = Some(inference);
        prepared.removable = candidates;
    }
    prepared.stats.retained_globals = prepared.globals.iter().filter(|b| **b).count();
    prepared.stats.specialized_calls += prepared.calls.len();
    prepared
}

/// The most rounds of folding, inlining and removal at `Advanced`. A round
/// that inlines nothing ends them; each inlining round is followed by folding
/// and removal.
pub const SIMPLIFY_ROUNDS: usize = 4;

/// What [`retain`] leaves for the passes after it.
struct Retained {
    /// The flattened top-level forms, each with whether it is the result.
    top: Vec<(ExprId, bool)>,
    /// By expression: the discarded top-level definitions with inert
    /// initializers, which may be removed when nothing reads their globals.
    candidates: Vec<bool>,
    /// Below `Advanced`: a retained `js*` made every top-level form a root.
    raw: bool,
}

/// Mark the globals the program reads from its roots (every top-level form
/// but discarded definitions with inert initializers, and the definitions of
/// `exports`), and remove the candidate definitions of unread globals.
/// `Advanced` runs it again after folding and inlining.
fn retain(prepared: &mut PreparedApplication, exports: &[GlobalId]) -> Retained {
    let level = prepared.level;
    let program = &prepared.program;
    let mut top = Vec::new();
    flatten(program, program.root, true, &mut top);
    let mut candidates = vec![false; program.exprs.len()];
    let mut definitions = vec![Vec::new(); program.globals.len()];
    let mut pending = Vec::new();
    let mut inert = Inert::new(program, &top, level.advanced());
    for (position, &(id, result)) in top.iter().enumerate() {
        if let ExprKind::Def {
            global, init, test, ..
        } = program.exprs[id.0].kind
        {
            definitions[global.0].push(id);
            if level.prepares()
                && !result
                && test.is_none()
                && init.is_none_or(|i| inert.initializer(i, position))
            {
                candidates[id.0] = true;
                continue;
            }
        }
        pending.push(id);
    }
    for g in exports {
        if let Some(defs) = definitions.get(g.0) {
            pending.extend(defs);
        }
    }
    let mut visited = vec![false; program.exprs.len()];
    let mut retained = vec![false; program.globals.len()];
    let mut raw = false;
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut visited[id.0], true) {
            continue;
        }
        // Below `Advanced`, retained raw JS is an application escape: every
        // top-level form becomes a root and nothing is specialized. A discarded
        // function containing JS is inert and must not turn its compile-time
        // uses into roots. `Advanced` treats `js*` as an opaque leaf whose holes
        // are ordinary children: hygiene renames every compiler-bound name away
        // from raw text, which can therefore name only its holes and host
        // globals (Phase 2 step 7 of docs/advanced-optimization.md).
        if !raw
            && !level.advanced()
            && matches!(program.exprs[id.0].kind, ExprKind::JsTemplate { .. })
        {
            raw = true;
            pending.extend(top.iter().map(|(id, _)| *id));
        }
        for global in globals(&program.exprs[id.0].kind) {
            if !std::mem::replace(&mut retained[global.0], true) {
                pending.extend(&definitions[global.0]);
            }
        }
        pending.extend(children(program, id));
    }
    prepared.globals = retained;
    for (index, candidate) in candidates.iter().enumerate() {
        if *candidate && !visited[index] {
            prepared.program.exprs[index].kind = ExprKind::Nil;
            prepared.stats.removed_definitions += 1;
        }
    }
    Retained {
        top,
        candidates,
        raw,
    }
}

fn flatten(program: &Program, id: ExprId, result: bool, out: &mut Vec<(ExprId, bool)>) {
    if let ExprKind::Do(items) = &program.exprs[id.0].kind {
        for (index, child) in items.iter().enumerate() {
            flatten(program, *child, result && index + 1 == items.len(), out);
        }
    } else {
        out.push((id, result));
    }
}
/// What an inert expression evaluates to, as far as the calls that consume it
/// need to know: its type in the core operation lattice, whether it is *plain
/// data* (see `crate::core_operations`), which `dispatch` operations hash,
/// compare and traverse without running program code, and for a literal
/// collection the type of its elements, which `invoke` operations pass to
/// their function argument.
#[derive(Clone, Copy)]
struct Shape {
    ty: Type,
    plain: bool,
    elements: Type,
}
impl Shape {
    /// A value of type `ty`, plain when every value of that type is a scalar.
    fn of(ty: Type) -> Self {
        Self {
            ty,
            plain: SCALAR.contains(ty),
            elements: Type::ANY,
        }
    }
    fn opaque(ty: Type) -> Self {
        Self {
            ty,
            plain: false,
            elements: Type::ANY,
        }
    }
    /// A collection of type `ty` with elements of type `elements`.
    fn collection(ty: Type, plain: bool, elements: Type) -> Self {
        Self {
            ty,
            plain,
            elements,
        }
    }
}
const SCALAR: Type = Type::NIL
    .union(Type::BOOLEAN)
    .union(Type::NUMBER)
    .union(Type::STRING)
    .union(Type::KEYWORD)
    .union(Type::SYMBOL);

/// A constructor whose call only stores its arguments: a program type defined
/// once, by a top-level `deftype` or `defrecord` that never gets reassigned,
/// or that type's positional factory (`->T`).
#[derive(Clone, Copy)]
struct Constructor {
    /// Index in the flattened top level of the defining form.
    position: usize,
    factory: bool,
    instance: Type,
}

/// Decides which discarded top-level initializers are inert: evaluating them
/// has no observable effect, cannot throw and runs no program code, so removing
/// an unread definition is unobservable.
///
/// `Prepare` accepts primitive literals, functions, core values, lists and
/// vectors of inert values, and maps and sets with scalar keys. `Advanced`
/// (Phase 2 step 6 of docs/advanced-optimization.md) also accepts local reads,
/// `let`, `if` and `do` over inert parts, metadata on inert values through the
/// `with-meta` row, calls of core operations whose row in
/// `src/core_operations.tsv` is `pure` or `allocates` for the argument types
/// and whose arguments run no code (`-` calls, or `dispatch` over plain data),
/// element-wise `invoke` calls (`mapv`, `filterv`, `every?`, ...) of a `fn`
/// literal or core operation that is inert on the elements of plain
/// collections, and `new` or `->T` of a [`Constructor`] after its definition.
/// `throws` and `effect` rows stay effectful.
///
/// The core rows hold under the program-wide conditions of the
/// `crate::core_operations` module docs, checked here: a core operation that
/// the program may replace (`set!`, `with-redefs`, or a `js*` hole, which raw
/// text could assign) is not trusted, and when the program extends a core
/// protocol to anything but its own types (`extend-type number`, `default`, a
/// runtime collection, `specify!`), no core call is inert.
struct Inert<'p> {
    program: &'p Program,
    advanced: bool,
    core_calls: bool,
    /// Core operations the program may replace, by table row.
    replaced: Vec<bool>,
    constructors: Vec<Option<Constructor>>,
    /// The shape of each `let` binding examined so far; every local has one
    /// binding site.
    locals: Vec<Option<Shape>>,
    /// Index in the flattened top level of the definition being examined.
    position: usize,
}
impl<'p> Inert<'p> {
    fn new(program: &'p Program, top: &[(ExprId, bool)], advanced: bool) -> Self {
        let mut inert = Self {
            program,
            advanced,
            core_calls: false,
            replaced: Vec::new(),
            constructors: Vec::new(),
            locals: Vec::new(),
            position: 0,
        };
        if !advanced {
            return inert;
        }
        // Program-wide, including code that may never run: which globals are
        // defined once, which core operations may be reassigned, and which
        // targets core protocols are extended to.
        let top_ids: Vec<ExprId> = top.iter().map(|(id, _)| *id).collect();
        let facts = ProgramFacts::new(program, &top_ids);
        inert.replaced = CoreFnId::ALL
            .into_iter()
            .map(|core| facts.replaced(core))
            .collect();
        inert.locals = vec![None; program.locals.len()];
        inert.constructors = vec![None; program.globals.len()];
        for global in 0..program.globals.len() {
            if let Definition::Type {
                definition,
                role: role @ (TypeRole::Constructor | TypeRole::Factory),
                record,
                ..
            } = facts.definition(GlobalId(global))
                && let Some(position) = facts.top_position(definition)
            {
                inert.constructors[global] = Some(Constructor {
                    position,
                    factory: role == TypeRole::Factory,
                    instance: if record { Type::RECORD } else { Type::OBJECT },
                });
            }
        }
        inert.core_calls = facts.core_protocols_intact();
        inert
    }

    /// Whether the initializer of the definition at `position` in the
    /// flattened top level is inert.
    fn initializer(&mut self, init: ExprId, position: usize) -> bool {
        self.position = position;
        self.shape(init).is_some()
    }

    /// The shape of `id` when it is inert.
    fn shape(&mut self, id: ExprId) -> Option<Shape> {
        use ExprKind::*;
        let program = self.program;
        Some(match &program.exprs[id.0].kind {
            Nil => Shape::of(Type::NIL),
            Bool(_) => Shape::of(Type::BOOLEAN),
            Number(_) => Shape::of(Type::NUMBER),
            String(_) => Shape::of(Type::STRING),
            Keyword(_) => Shape::of(Type::KEYWORD),
            Symbol(_) => Shape::of(Type::SYMBOL),
            CoreFn(core) => Shape::opaque(core.operation().value),
            LoadableType => Shape::opaque(Type::FN),
            Fn { .. } => Shape::opaque(Type::FN),
            Do(items) => {
                let mut last = Shape::of(Type::NIL);
                for item in items {
                    last = self.shape(*item)?;
                }
                last
            }
            List(items) | Vector(items) => {
                let (mut plain, mut elements) = (true, Type::NEVER);
                for item in items {
                    let item = self.shape(*item)?;
                    plain &= item.plain;
                    elements = elements.union(item.ty);
                }
                let ty = if matches!(program.exprs[id.0].kind, List(_)) {
                    Type::LIST
                } else {
                    Type::VECTOR
                };
                Shape::collection(ty, plain, elements)
            }
            Map(items) | LiteralMap(items) => {
                let mut plain = true;
                for pair in items.as_chunks::<2>().0 {
                    if !scalar(program, pair[0]) {
                        return None;
                    }
                    plain &= self.shape(pair[1])?.plain;
                }
                // Its elements are map entries.
                Shape::collection(Type::MAP, plain, Type::VECTOR)
            }
            Set(items) | LiteralSet(items) => {
                if !items.iter().all(|id| scalar(program, *id)) {
                    return None;
                }
                let mut elements = Type::NEVER;
                for item in items {
                    elements = elements.union(self.shape(*item)?.ty);
                }
                Shape::collection(Type::SET, true, elements)
            }
            // Non-scalar hashing, reader literals, metadata and unknown operations
            // may throw or dispatch user code and are deliberately not pure here.
            _ if !self.advanced => return None,
            Local(local) => self.locals[local.0].unwrap_or(Shape::opaque(Type::ANY)),
            Let { bindings, body } => {
                for (local, init) in bindings {
                    self.locals[local.0] = Some(self.shape(*init)?);
                }
                self.shape(*body)?
            }
            If { test, yes, no } => {
                self.shape(*test)?;
                let (yes, no) = (self.shape(*yes)?, self.shape(*no)?);
                Shape::collection(
                    yes.ty.union(no.ty),
                    yes.plain && no.plain,
                    yes.elements.union(no.elements),
                )
            }
            WithMeta { value, metadata } => {
                let arguments = [self.shape(*value)?, self.shape(*metadata)?];
                Shape::opaque(self.core_call(CoreFnId::WithMeta, &arguments)?)
            }
            Call { callee, args } => {
                if let Some((keys, values)) = hash_map_expansion(program, *callee, args) {
                    return self.hash_map(keys, values);
                }
                let arguments = args
                    .iter()
                    .map(|arg| self.shape(*arg))
                    .collect::<Option<Vec<_>>>()?;
                match program.exprs[callee.0].kind {
                    CoreFn(core) => self.core_call_shape(core, args, &arguments)?,
                    Global(global) => Shape::opaque(self.construct(global, true)?),
                    _ => return None,
                }
            }
            Construct { callee, args } => {
                let Global(global) = program.exprs[callee.0].kind else {
                    return None;
                };
                let instance = self.construct(global, false)?;
                for arg in args {
                    self.shape(*arg)?;
                }
                Shape::opaque(instance)
            }
            _ => return None,
        })
    }

    /// The result type of an inert call of `core` on arguments of these shapes.
    fn core_call(&self, core: CoreFnId, arguments: &[Shape]) -> Option<Type> {
        if !self.core_calls || self.replaced[core.table_row()] {
            return None;
        }
        let types: Vec<_> = arguments.iter().map(|shape| shape.ty).collect();
        let call = core.operation().call(&types);
        let runs_no_code = match call.calls {
            Calls::None => true,
            Calls::Dispatch => arguments.iter().all(|shape| shape.plain),
            Calls::Invoke => false,
        };
        (call.purity <= Purity::Allocates && runs_no_code).then_some(call.returns)
    }

    /// The shape of an inert call of `core` on `args`, whose shapes are
    /// `arguments`. An `invoke` row is inert when the operation calls its first
    /// argument on one element of each further argument, those are plain
    /// collections, and the function argument, a `fn` literal or a core
    /// operation, is inert on their elements (light-cljs-210).
    fn core_call_shape(
        &mut self,
        core: CoreFnId,
        args: &[ExprId],
        arguments: &[Shape],
    ) -> Option<Shape> {
        let types: Vec<_> = arguments.iter().map(|shape| shape.ty).collect();
        let call = core.operation().call(&types);
        if call.calls != Calls::Invoke {
            return Some(Shape::of(self.core_call(core, arguments)?));
        }
        let element_wise = matches!(
            core,
            CoreFnId::Mapv
                | CoreFnId::Filterv
                | CoreFnId::Every
                | CoreFnId::NotEvery
                | CoreFnId::NotAny
                | CoreFnId::Some
                | CoreFnId::GroupBy
                | CoreFnId::Run
        );
        if !element_wise
            || !self.core_calls
            || self.replaced[core.table_row()]
            || call.purity > Purity::Allocates
        {
            return None;
        }
        let collections = arguments.get(1..)?;
        if collections.is_empty() || !collections.iter().all(|shape| shape.plain) {
            return None;
        }
        let elements: Vec<_> = collections
            .iter()
            .map(|collection| Shape::collection(collection.elements, true, Type::ANY))
            .collect();
        let result = self.invoke(args[0], &elements)?;
        Some(match core {
            CoreFnId::Mapv => Shape::collection(Type::VECTOR, result.plain, result.ty),
            CoreFnId::Filterv => Shape::collection(Type::VECTOR, true, elements[0].ty),
            CoreFnId::Some => {
                Shape::collection(result.ty.union(Type::NIL), result.plain, result.elements)
            }
            // The results are the keys of the map it builds.
            CoreFnId::GroupBy if result.plain => Shape::collection(Type::MAP, true, Type::VECTOR),
            CoreFnId::GroupBy => return None,
            _ => Shape::of(call.returns),
        })
    }

    /// The shape of what calling `function`, a `fn` literal or a core
    /// operation, on arguments of these shapes returns, when the call is
    /// inert. A `fn` with one fixed method takes any number of arguments
    /// (missing ones are nil); one with several needs a fixed method of that
    /// many, since a mismatch throws (variadic methods are not followed).
    fn invoke(&mut self, function: ExprId, arguments: &[Shape]) -> Option<Shape> {
        let program = self.program;
        match &program.exprs[function.0].kind {
            ExprKind::CoreFn(core) => {
                let types: Vec<_> = arguments.iter().map(|shape| shape.ty).collect();
                if core.operation().call(&types).calls == Calls::Invoke {
                    return None;
                }
                Some(Shape::of(self.core_call(*core, arguments)?))
            }
            ExprKind::Fn { methods, .. } if !program.macro_functions.contains(&function) => {
                let method = match methods.as_slice() {
                    [method] if method.rest.is_none() => method,
                    _ => methods.iter().find(|method| {
                        method.rest.is_none() && method.params.len() == arguments.len()
                    })?,
                };
                // A call of an `^:async` method returns a Promise.
                if method.asynchronous {
                    return None;
                }
                for (index, param) in method.params.iter().enumerate() {
                    self.locals[param.0] = Some(
                        arguments
                            .get(index)
                            .copied()
                            .unwrap_or(Shape::of(Type::NIL)),
                    );
                }
                self.shape(method.body)
            }
            _ => None,
        }
    }

    /// A `hash-map` call that its compiler macro expanded: the `array` calls
    /// and the `hash-map` row on the interleaved operands, in source order.
    fn hash_map(&mut self, keys: &[ExprId], values: &[ExprId]) -> Option<Shape> {
        if self.replaced[CoreFnId::PersistentHashMap.table_row()] {
            return None;
        }
        let mut operands = [Vec::new(), Vec::new()];
        for (array, operands) in [keys, values].into_iter().zip(&mut operands) {
            for operand in array {
                operands.push(self.shape(*operand)?);
            }
            self.core_call(CoreFnId::Array, operands)?;
        }
        let [keys, values] = operands;
        let pairs: Vec<_> = keys
            .into_iter()
            .zip(values)
            .flat_map(|(k, v)| [k, v])
            .collect();
        Some(Shape::opaque(self.core_call(CoreFnId::HashMap, &pairs)?))
    }

    /// The instance type of `new global` (or of calling it, for a `factory`)
    /// when it only stores its arguments and its definition has run.
    fn construct(&self, global: GlobalId, factory: bool) -> Option<Type> {
        let constructor = self.constructors.get(global.0).copied().flatten()?;
        (constructor.factory == factory && constructor.position < self.position)
            .then_some(constructor.instance)
    }
}

/// The key and value operands of `(PersistentHashMap.fromArrays (array k ...)
/// (array v ...))`, the expansion of a `hash-map` call with arguments
/// (src/compiler_core_macros.cljs), when every key has a value.
fn hash_map_expansion<'p>(
    program: &'p Program,
    callee: ExprId,
    args: &'p [ExprId],
) -> Option<(&'p [ExprId], &'p [ExprId])> {
    let ExprKind::Property { receiver, property } = &program.exprs[callee.0].kind else {
        return None;
    };
    let receiver = &program.exprs[receiver.0].kind;
    if property != "fromArrays"
        || !matches!(receiver, ExprKind::CoreFn(CoreFnId::PersistentHashMap))
    {
        return None;
    }
    let array = |id: &ExprId| match &program.exprs[id.0].kind {
        ExprKind::Call { callee, args }
            if matches!(
                program.exprs[callee.0].kind,
                ExprKind::CoreFn(CoreFnId::Array)
            ) =>
        {
            Some(args.as_slice())
        }
        _ => None,
    };
    let [keys, values] = args else {
        return None;
    };
    let (keys, values) = (array(keys)?, array(values)?);
    (keys.len() == values.len()).then_some((keys, values))
}
fn scalar(program: &Program, id: ExprId) -> bool {
    matches!(
        program.exprs[id.0].kind,
        ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_)
    )
}
/// The globals an expression of this kind reads or defines.
pub(crate) fn globals(kind: &ExprKind) -> Vec<GlobalId> {
    match kind {
        ExprKind::Global(g) | ExprKind::Def { global: g, .. } => vec![*g],
        ExprKind::DefProtocol {
            global, methods, ..
        } => std::iter::once(*global)
            .chain(methods.iter().map(|m| m.global))
            .collect(),
        ExprKind::DefType { global, layout, .. } => std::iter::once(*global)
            .chain(layout.constructor)
            .chain(layout.map_constructor)
            .collect(),
        _ => Vec::new(),
    }
}
fn reachable(program: &Program, root: ExprId) -> Vec<ExprId> {
    let mut pending = vec![root];
    let mut visited = vec![false; program.exprs.len()];
    let mut result = Vec::new();
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut visited[id.0], true) {
            continue;
        }
        result.push(id);
        pending.extend(children(program, id));
    }
    result
}
pub(crate) fn unwrap_do(program: &Program, mut id: ExprId) -> ExprId {
    while let ExprKind::Do(items) = &program.exprs[id.0].kind {
        if items.len() != 1 {
            break;
        }
        id = items[0];
    }
    id
}
pub(crate) fn dynamic(program: &Program, g: GlobalId) -> bool {
    let Some(id) = program.globals[g.0].metadata else {
        return false;
    };
    let ExprKind::Map(items) = &program.exprs[id.0].kind else {
        return true;
    };
    items.as_chunks::<2>().0.iter().any(|pair| {
        let ExprKind::Keyword(name) = program.exprs[pair[0].0].kind else {
            return false;
        };
        let name = &program.named_literals[name.0];
        name.namespace.is_none()
            && program.strings[name.name.0] == "dynamic".encode_utf16().collect::<Vec<_>>()
            && !matches!(
                program.exprs[pair[1].0].kind,
                ExprKind::Nil | ExprKind::Bool(false)
            )
    })
}
fn specialize(prepared: &mut PreparedApplication, top: &[(ExprId, bool)]) {
    let p = &prepared.program;
    let nodes = reachable(p, p.root);
    let mut defs = vec![Vec::new(); p.globals.len()];
    let mut escaped = vec![false; p.globals.len()];
    let mut callees = vec![false; p.exprs.len()];
    for &id in &nodes {
        if let ExprKind::Call { callee, .. } = p.exprs[id.0].kind {
            callees[callee.0] = true;
        }
    }
    for &id in &nodes {
        match &p.exprs[id.0].kind {
            ExprKind::Def {
                global,
                init,
                once: false,
                test: None,
            } => defs[global.0].push(*init),
            ExprKind::Def { global, .. } => escaped[global.0] = true,
            ExprKind::Global(g) if !callees[id.0] => escaped[g.0] = true,
            _ => (),
        }
    }
    // Specialize only eager top-level calls after the sole initialization. Calls
    // inside functions/branches retain late lookup: no initialization dominance
    // or interprocedural effect assumptions are smuggled into emission.
    let mut initialized = vec![false; p.globals.len()];
    let mut core_calls = Vec::new();
    for &(id, _) in top {
        if let ExprKind::Def { global, .. } = p.exprs[id.0].kind {
            initialized[global.0] = true;
        }
        let ExprKind::Call { callee, ref args } = p.exprs[id.0].kind else {
            continue;
        };
        let ExprKind::Global(g) = p.exprs[callee.0].kind else {
            continue;
        };
        if escaped[g.0] || !initialized[g.0] || dynamic(p, g) || defs[g.0].len() != 1 {
            continue;
        }
        let Some(init) = defs[g.0][0] else {
            continue;
        };
        let init = unwrap_do(p, init);
        if let ExprKind::Fn { methods, .. } = &p.exprs[init.0].kind {
            // Multi-arity wrappers publish these canonical fixed arity entries.
            // Keep .call(null) so this/arguments/recur and argument effects match.
            if methods.len() > 1
                && methods
                    .iter()
                    .filter(|m| m.rest.is_none() && m.params.len() == args.len())
                    .count()
                    == 1
            {
                prepared.calls.insert(id, args.len());
            }
        } else if let ExprKind::CoreFn(core) = p.exprs[init.0].kind {
            core_calls.push((id, callee, core));
        }
    }
    for (id, callee, core) in core_calls {
        let mut expr = prepared.program.exprs[callee.0].clone();
        expr.kind = ExprKind::CoreFn(core);
        let callee = ExprId(prepared.program.exprs.len());
        prepared.program.exprs.push(expr);
        if let ExprKind::Call { callee: target, .. } = &mut prepared.program.exprs[id.0].kind {
            *target = callee;
        }
        prepared.stats.specialized_calls += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optimization_levels_parse_print_order_and_convert_from_the_former_boolean() {
        for level in OptimizationLevel::ALL {
            assert_eq!(level.name().parse(), Ok(level));
            assert_eq!(level.to_string(), level.name());
        }
        assert_eq!(
            "Advanced".parse::<OptimizationLevel>(),
            Err(UnknownOptimizationLevel("Advanced".into()))
        );
        assert!(OptimizationLevel::None < OptimizationLevel::Prepare);
        assert!(OptimizationLevel::Prepare < OptimizationLevel::Advanced);
        assert_eq!(
            OptimizationLevel::ALL.map(|l| (l.prepares(), l.advanced())),
            [(false, false), (true, false), (true, true)]
        );
        assert_eq!(OptimizationLevel::from(false), OptimizationLevel::None);
        assert_eq!(OptimizationLevel::from(true), OptimizationLevel::Prepare);
        assert_eq!(
            ApplicationOptions::default().optimize,
            OptimizationLevel::Prepare
        );
        let p = crate::analyze(
            "(def unused 1) 2",
            &crate::parse("(def unused 1) 2").unwrap(),
        )
        .unwrap();
        for level in OptimizationLevel::ALL {
            let prepared = prepare_application(&p, ApplicationOptions::new(level));
            assert_eq!(prepared.level(), level);
            assert_eq!(
                prepared.stats().removed_definitions,
                usize::from(level.prepares())
            );
        }
    }
    #[test]
    fn macro_calls_do_not_prepare_applications_or_recompile_unchanged_units() {
        let source = "(def factor 2) (defn helper [x] (* factor x)) (defmacro twice [x] (helper x)) (twice 21) (twice 21) (twice 21)";
        PREPARATIONS.with(|count| count.set(0));
        let p = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        PREPARATIONS.with(|count| assert_eq!(count.get(), 0));
        for name in ["cljs.user/factor", "cljs.user/helper"] {
            let uses: Vec<_> = p
                .compilation_uses
                .iter()
                .filter(|u| u.name == name)
                .collect();
            assert_eq!(uses.len(), 1, "{name}: {:?}", p.compilation_uses);
            assert_eq!(uses[0].analyses, 1);
            assert_eq!(uses[0].emissions, 1);
        }
        let prepared = prepare_application(&p, Default::default());
        assert_eq!(prepared.stats().removed_definitions, 2);
        let js = prepared.emit_js();
        assert_eq!(prepared.emit_js(), js);
        PREPARATIONS.with(|count| assert_eq!(count.get(), 1));
        assert!(!js.contains("__lightCompileUnit"));
        assert!(!js.contains("__lightCompilerRoots"));
    }
    #[test]
    fn generation_counters_distinguish_compile_demand_from_application_analysis() {
        let source = "(def x 1) (defmacro read-x [] x) (read-x) (read-x) (def x 2) (read-x) (read-x) (def unused 3) 42";
        let p = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        let uses: Vec<_> = p
            .compilation_uses
            .iter()
            .filter(|u| u.name == "cljs.user/x")
            .collect();
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].generation, 1);
        assert_eq!(uses[1].generation, 2);
        for u in uses {
            assert_eq!((u.analyses, u.emissions), (1, 1));
        }
        let unused = p
            .compilation_uses
            .iter()
            .find(|u| u.name == "cljs.user/unused")
            .unwrap();
        assert_eq!((unused.analyses, unused.emissions), (1, 0));
    }
    /// Removed definitions at `Prepare` and `Advanced`.
    fn removed(source: &str) -> [usize; 2] {
        let p = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        [OptimizationLevel::Prepare, OptimizationLevel::Advanced]
            .map(|level| prepare_application(&p, ApplicationOptions::new(level)))
            .map(|prepared| prepared.stats().removed_definitions)
    }
    #[test]
    fn advanced_inertness_checks_replaced_operations_extensions_and_constructors() {
        assert_eq!(removed("(def a (vector 1)) (def b [1]) 42"), [1, 2]);
        // Replaced core operations, including through a property or a raw hole
        // that raw text could assign, are not trusted; other operations are.
        assert_eq!(
            removed("(set! cljs.core/vector list) (def a (vector 1)) 42"),
            [0, 0]
        );
        assert_eq!(
            removed("(with-redefs [vector list] 1) (def a (vector 1)) 42"),
            [0, 0]
        );
        assert_eq!(
            removed("(js* \"~{}\" vector) (def a (vector 1)) 42"),
            [0, 0]
        );
        assert_eq!(removed("(js* \"~{}\" list) (def a (vector 1)) 42"), [0, 1]);
        assert_eq!(
            removed("(def a (hash-map 1 2)) (def b (hash-map 1)) 42"),
            [0, 1]
        );
        assert_eq!(
            removed("(set! (.-fromArrays PersistentHashMap) nil) (def a (hash-map 1 2)) 42"),
            [0, 0]
        );
        // Core protocols extended beyond the program's own types make every
        // core call suspect; the program's own protocols and types do not.
        assert_eq!(
            removed("(extend-type number IHash (-hash [_] 0)) (def a (vector 1)) (def b [1]) 42"),
            [1, 1]
        );
        assert_eq!(
            removed(
                "(defprotocol P (m [x])) (extend-type number P (m [x] x)) (def a (vector 1)) 42"
            ),
            [0, 1]
        );
        assert_eq!(
            removed("(deftype T [x]) (extend-type T IHash (-hash [_] 0)) (def a (vector 1)) 42"),
            [0, 1]
        );
        // Construction after a sole top-level definition only stores fields.
        assert_eq!(
            removed(
                "(defrecord R [x]) (deftype T [x]) (def a (->R 1)) (def b (R. 1)) (def c (T. 1)) 42"
            ),
            [0, 3]
        );
        assert_eq!(
            removed("(defrecord R [x]) (def a (map->R {:x 1})) 42"),
            [0, 0]
        );
        assert_eq!(
            removed("(declare T) (def a (T. 1)) (deftype T [x]) 42"),
            [0, 0]
        );
        assert_eq!(
            removed("(deftype T [x]) (set! T nil) (def a (T. 1)) 42"),
            [0, 0]
        );
        assert_eq!(
            removed("(defrecord R [x]) (set! ->R vector) (def a (->R 1)) (def b (R. 1)) 42"),
            [0, 1]
        );
        // Local reads, `let`, `if`, `do` and metadata over inert parts.
        assert_eq!(
            removed("(def a (let [x (inc 1)] (if (pos? x) (do x [x]) ^:m {:x x}))) 42"),
            [0, 1]
        );
        // `throws` and `effect` rows, calls outside a row's domain and
        // `dispatch` rows over data that is not plain stay.
        assert_eq!(
            removed(
                "(def a (into [] [1])) (def c (inc \"x\")) \
                 (def d (count (vector 1))) (def e (count [(fn [] 1)])) 42"
            ),
            [0, 0]
        );
        // Element-wise `invoke` rows are inert when their function, a `fn`
        // literal or a core operation, is inert on the elements of plain
        // collections (light-cljs-210).
        assert_eq!(
            removed(
                "(def a (mapv inc [1 2])) (def b (filterv (fn [x] (pos? x)) [1 -1])) \
                 (def c (mapv (fn [x y] [x y]) [1] '(:a))) (def d (every? keyword? #{:a})) \
                 (def e (group-by count [[1] [2 3]])) (def f (some #(when (pos? %) [%]) [0 1])) 42"
            ),
            [0, 6]
        );
        // Not when the function may throw on an element, has no method for the
        // arity, is not a literal, or the collection is not plain data.
        assert_eq!(
            removed(
                "(def a (mapv inc [\"x\"])) (def b (mapv (fn ([] 1) ([x y] 2)) [1])) \
                 (def c (mapv identity (range 3))) (defn g [x] x) (def d (mapv g [1])) \
                 (def e (mapv (fn [x] (inc x)) [[1]])) (def f (group-by identity [(fn [])])) \
                 (def h (reduce + [1 2])) 42"
            ),
            [0, 0]
        );
    }
}
