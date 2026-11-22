//! Chunked output (docs/code-splitting.md, "What a chunk file contains",
//! "Assignment across chunks" and "Phase 2"): one ES module per chunk of a
//! [`ChunkPlan`], at every level.
//!
//! One [`Emitter`] writes every chunk's forms in plan order, so the constant
//! table, the runtime roots and the `used` set are the program's, as in the
//! single file. Each chunk's forms run in a function of their own, as the
//! single file's do, and what other chunks may read is declared at module
//! level: the runtime (startup's whole runtime below `Advanced`; at
//! `Advanced` each chunk's slice, "Phase 3", runtime units), startup's
//! output-level core cells, each chunk's entries of the constant table, and
//! each chunk's `let` declarations of the globals it owns. The table's
//! entries and the runtime's units are placed once the chunks are written:
//! each entry in the chunk of the roots whose code reads it
//! ([`constant_signatures`]), and each unit in the chunk of the roots whose
//! code and constants need it ([`unit_signatures`]), either of which may be
//! a chunk the plan lacks; a hoisted method stays in its function's chunk.
//! Terse output writes a chunk's units at module level (light-cljs-303):
//! the table that names the program's compiler names names their bindings
//! too, a program read `rt$x` and the binding `x` one name, so the chunk
//! exports the bindings later chunks read and imports those its units and
//! code take from earlier chunks, live and read-only (a unit that assigns
//! another's binding shares its chunk, `RuntimeManifest::sharers`).
//! Readable output, and terse output whose text names a binding that keeps
//! its spelling ([`names_kept_spelling`]), makes a chunk's slice a function
//! of the bindings its units take from earlier chunks, which it imports,
//! that returns the bindings its code and later chunks read.
//!
//! Which names a chunk imports is read off its text: every compiler name
//! (`crate::terse::compiler_name`) that it reads and another chunk declares.
//! The owner adds the name to its exports; a setter name (`g7$`) that a
//! chunk calls to assign another chunk's global makes the owner define and
//! export the setter. The texts are renamed together (one terse frequency
//! table, or one hygiene prefix), and the import, declaration and export
//! lines are written afterwards from the final names, so a name one chunk
//! exports is the name another imports.
use super::*;
use crate::analyzer::LazyTargetId;
use crate::chunks::{ChunkId, ChunkPlan, STARTUP, STARTUP_ROOT, Signature};
use crate::runtime_manifest::UnitId;
use crate::runtime_slice::{ChunkRuntime, ChunkSlice};
use crate::terse::{Text, scanned_names};
use std::collections::{BTreeMap, BTreeSet};

/// What an [`Emitter`] of a chunked build knows about the chunks.
pub(super) struct Chunks<'a> {
    plan: &'a ChunkPlan,
    /// The chunk being written.
    current: ChunkId,
    /// By chunk: the chunks that have evaluated before its forms run, by
    /// chunk index: itself, startup, and what it imports, transitively.
    loaded: &'a [Vec<bool>],
    /// When true, this [`Chunks`] belongs to one parallel emission task whose
    /// `runtime` holds only two slots: `[0]` for startup roots and `[1]` for
    /// `current`'s roots.
    task_mode: bool,
    /// By constant table entry: what placing it needs.
    constants: Vec<Constant>,
    /// While an entry's initializer is written: what it reads.
    initializer: Option<Initializer>,
    /// By chunk: the runtime names its code reads (docs/code-splitting.md,
    /// "Phase 3", runtime units), startup's also those of the namespace
    /// registration, the output-level core cells and what the program names
    /// rather than reads (`Emitter::record_application_roots`). A constant's
    /// initializer reads are its own ([`Constant::runtime`]).
    runtime: Vec<RuntimeRoots>,
    /// The globals the getter of a loadable reads from its target chunk's
    /// module namespace object, each with that chunk, which exports it.
    targets: Vec<(ChunkId, String)>,
    /// Whether a chunk has a loadable whose target may not have evaluated
    /// when the chunk runs, for which startup declares `lazy$`.
    lazy: bool,
    /// The constant table reads by code so far, which order the entries'
    /// first reads for the size report.
    constant_reads: usize,
}

/// A constant table entry of a chunked build (docs/code-splitting.md,
/// "Phase 3", constants): where its declaration is, and what decides its
/// chunk.
struct Constant {
    /// Its declaration, `const k3 = ...;`, in the table's declarations.
    declaration: std::ops::Range<usize>,
    /// For a hoisted method ([`ConstantKey::Method`]), the chunk of its
    /// function, which declares it: its body reads that chunk's globals.
    method: Option<ChunkId>,
    /// The entries its initializer reads: the parts of a composite or a
    /// shape, which are earlier entries.
    parts: Vec<usize>,
    /// The roots whose chunks' code reads it: the union of their
    /// signatures, startup's holding every root.
    readers: Signature,
    /// The runtime names its initializer reads, which the chunk declaring
    /// it needs.
    runtime: RuntimeRoots,
    /// What it holds, for the size report ([`ConstantKey::label`]).
    label: String,
    /// The first read by code, for the size report: its place among all
    /// constant reads by code ([`Chunks::constant_reads`]), the chunk
    /// written then (a plan chunk, before [`ChunkPlan::place`] renumbers
    /// them) and the expression being written.
    first: Option<(usize, ChunkId, Option<ExprId>)>,
}

/// A constant table entry as the size report lists it: what it holds, and
/// its first read by code, by a chunk of the placed plan and the expression
/// being written; an entry only another entry's initializer reads has that
/// entry's.
#[derive(Clone, Debug)]
pub(crate) struct ChunkConstant {
    pub(crate) label: String,
    pub(crate) first: Option<(ChunkId, Option<ExprId>)>,
}

/// What a constant table entry's initializer reads while it is written.
#[derive(Default)]
pub(super) struct Initializer {
    /// The entries it reads, the parts of a composite or a shape.
    pub(super) parts: Vec<usize>,
    /// The runtime names it reads.
    pub(super) runtime: RuntimeRoots,
}

impl Chunks<'_> {
    /// Record constant table entry `number`, whose initializer read
    /// `initializer` and which holds what `label` says. Its readers place it.
    pub(super) fn constant(&mut self, number: usize, initializer: Initializer, label: String) {
        debug_assert_eq!(
            number,
            self.constants.len(),
            "entries are numbered in order"
        );
        let roots = self.plan.roots.len();
        self.constants.push(Constant {
            declaration: 0..0,
            method: None,
            parts: initializer.parts,
            readers: Signature::empty(roots),
            runtime: initializer.runtime,
            label,
            first: None,
        });
    }

    /// Record constant table entry `number`, a hoisted method of a function
    /// of the chunk being written, which declares it: its body's runtime
    /// reads are that chunk's.
    pub(super) fn method(&mut self, number: usize, label: String) {
        self.constant(number, Initializer::default(), label);
        self.constants[number].method = Some(self.current);
    }

    /// Where the declaration of entry `number` is in the table's
    /// declarations.
    pub(super) fn declared(&mut self, number: usize, declaration: std::ops::Range<usize>) {
        self.constants[number].declaration = declaration;
    }

    /// Start recording what an initializer reads, returning what an
    /// enclosing initializer recorded so far.
    pub(super) fn begin_initializer(&mut self) -> Option<Initializer> {
        self.initializer.replace(Initializer::default())
    }

    /// What the initializer just written reads, restoring the recording of
    /// `enclosing`.
    pub(super) fn end_initializer(&mut self, enclosing: Option<Initializer>) -> Initializer {
        std::mem::replace(&mut self.initializer, enclosing).unwrap_or_default()
    }

    /// A read of entry `number`: by the initializer being written, or else
    /// by the code of the chunk being written, at `site`.
    pub(super) fn read_constant(&mut self, number: usize, site: Option<ExprId>) {
        match &mut self.initializer {
            Some(initializer) => initializer.parts.push(number),
            None => {
                let signature = &self.plan.chunks[self.current.0].signature;
                let constant = &mut self.constants[number];
                constant.readers.union(signature);
                constant
                    .first
                    .get_or_insert((self.constant_reads, self.current, site));
                self.constant_reads += 1;
            }
        }
    }

    /// A runtime read, as [`RuntimeRoots::record`] records it: by the
    /// initializer being written, or else by the code of the chunk being
    /// written; by startup's code when `startup`, for what startup declares
    /// for every chunk (an output-level core cell's initial value).
    pub(super) fn runtime_root(
        &mut self,
        name: RuntimeName,
        site: Option<ExprId>,
        used: NameUse,
        startup: bool,
    ) {
        match &mut self.initializer {
            Some(initializer) if !startup => initializer.runtime.record(name, site, used),
            _ => {
                let index = if self.task_mode {
                    usize::from(!startup && self.current != STARTUP)
                } else if startup {
                    STARTUP.0
                } else {
                    self.current.0
                };
                self.runtime[index].record(name, site, used);
            }
        }
    }

    /// Attribute what is written from now on to startup's code: the
    /// namespace registration and the units kept for what the program names.
    pub(super) fn write_startup(&mut self) {
        debug_assert!(self.initializer.is_none());
        self.current = STARTUP;
    }

    /// The chunk a loadable of `target` written in the chunk being written
    /// imports, or `None` when the target is resident: in this chunk or in
    /// one that has evaluated before it.
    pub(super) fn importer(&self, program: &Program, target: LazyTargetId) -> Option<ChunkId> {
        let chunk = self.plan.target_chunk(program, target);
        (!self.loaded[self.current.0][chunk.0]).then_some(chunk)
    }

    /// Whether `written`, the assigned binding of an assignment the chunk
    /// being written makes, is a binding another chunk owns: a global
    /// (`g7`), or an output-level core cell (`core$inc`), which startup
    /// owns. The assignment calls the owner's setter instead.
    pub(super) fn assigns_through_setter(&self, written: &str) -> bool {
        if let Some(cell) = written.strip_prefix("core$") {
            return self.current != STARTUP && !cell.is_empty() && identifier(cell);
        }
        written
            .strip_prefix('g')
            .filter(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|number| number.parse::<usize>().ok())
            .is_some_and(|global| self.plan.global_chunks[global] != self.current)
    }
}

fn identifier(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

impl Emitter<'_, '_> {
    /// A loadable of `target`, whose type `class` reads, in a chunk that may
    /// not have evaluated when the loadable's own chunk runs: an importer of
    /// that chunk and a getter that reads the target from its module
    /// namespace object, which startup's `lazy$` ([`LAZY`]) keys by the
    /// chunk's number in the plan, `lazy$(1, new Loadable(() =>
    /// import("./app.editor.js"), (m) => m.g20))`, or `() => null` for a
    /// namespace target.
    pub(super) fn lazy_loadable(&mut self, target: LazyTargetId, class: ExprId) {
        let chunks = self.chunks.as_mut().expect("a chunked build");
        chunks.lazy = true;
        let chunk = chunks
            .importer(self.program, target)
            .expect("a target in another chunk");
        let specifier = quoted(&format!("./{}", chunks.plan.chunks[chunk.0].name));
        write!(self.output, "lazy$({}, new ", chunk.0).unwrap();
        self.value(class);
        write!(self.output, "(() => import({specifier}), ").unwrap();
        match self.program.lazy_targets[target.0].global {
            Some(global) => {
                self.output.push_str("(m) => m.");
                let start = self.output.len();
                self.global(global);
                let name = self.output[start..].to_owned();
                debug_assert!(owned_global(&name).is_some(), "a loadable reads {name}");
                let chunks = self.chunks.as_mut().unwrap();
                chunks.targets.push((chunk, name));
            }
            None => self.output.push_str("() => null"),
        }
        self.output.push_str("))");
    }

    /// Write the forms of the chunk being written that `id`, a form of the
    /// program's root in `context` and in the namespace of source index
    /// `namespace`, holds, in program order: the top-level forms the plan
    /// assigns it, and the entries of its namespaces, which switch `*ns*`
    /// where the output registers namespaces.
    fn chunk_forms(
        &mut self,
        id: ExprId,
        context: ExprContext,
        namespace: usize,
        sources: &HashMap<&str, usize>,
        forms: &[Option<ChunkId>],
    ) {
        let program = self.program;
        let chunks = self.chunks.as_ref().expect("a chunked build");
        let (plan, current) = (chunks.plan, chunks.current);
        let namespace = program
            .namespace_entries
            .get(&id)
            .map_or(namespace, |name| sources[name.as_str()]);
        let ExprKind::Do(body) = &program.exprs[id.0].kind else {
            if forms[id.0] == Some(current) {
                debug_assert!(
                    context != ExprContext::Return || current == STARTUP,
                    "the program's value is startup's"
                );
                self.emit(id, context);
            }
            return;
        };
        if plan.namespace_chunks[namespace] == current {
            let site = self.site.replace(id);
            self.namespace_entry(id);
            if body.is_empty() && context == ExprContext::Return {
                debug_assert_eq!(current, STARTUP, "the program's value is startup's");
                self.indentation();
                self.output.push_str("return null;\n");
            }
            self.site = site;
        }
        for (index, &form) in body.iter().enumerate() {
            let context = if index + 1 == body.len() {
                context
            } else {
                ExprContext::Discard
            };
            self.chunk_forms(form, context, namespace, sources, forms);
        }
    }
}

/// The global a name the emitter writes binds (`g7`, a method entry `g7_1`,
/// a setter `g7$`), if it is one.
fn owned_global(name: &str) -> Option<usize> {
    let tail = name.strip_prefix('g')?;
    let tail = tail.strip_suffix('$').unwrap_or(tail);
    let number = tail.split('_').next()?;
    (!number.is_empty()
        && tail
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'_'))
    .then(|| number.parse().ok())
    .flatten()
}

/// The binding a setter name assigns: `g7` for `g7$`, `core$inc` for
/// `core$inc$`.
fn assigned_binding(name: &str) -> Option<&str> {
    let binding = name.strip_suffix('$')?;
    (owned_global(binding).is_some()
        || binding
            .strip_prefix("core$")
            .is_some_and(|cell| !cell.is_empty()))
    .then_some(binding)
}

/// The number of the constant table entry a name reads: `k3`, or its lazy
/// cell `c3`.
fn constant_entry(name: &str) -> Option<usize> {
    let tail = name.strip_prefix('k').or_else(|| name.strip_prefix('c'))?;
    (!tail.is_empty() && tail.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| tail.parse().ok())
        .flatten()
}

/// Startup's helper for the loadables whose target is in a chunk that may
/// not have evaluated when the loadable's own chunk runs,
/// `lazy$(1, new Loadable(importer, getter))`, keyed by the target chunk's
/// number in the plan (docs/code-splitting.md, "The loading API"). Its one
/// array `m` holds, by chunk `_`, the module namespace object once an
/// importer of the chunk has imported it. It turns the loadable `_l`'s
/// `importer` field into a property that is `null` once its chunk is
/// recorded there and the importer `_i` before, so `loaded?` of every
/// loadable of a chunk turns true when one of them loads, also of a loadable
/// made after; its `getter` into one that hands the target's getter `_g` the
/// recorded module, so `@l` reads the same fields as a single file's
/// loadable; and gives it a non-enumerable `load` that, while the chunk is
/// not recorded, calls its importer, records the module `_m` and derefs, and
/// once it is, loads as the runtime's `load` `_d` does, on the next
/// microtask like a single file's loadable rather than after an import (the
/// Sol review of light-cljs-353). The fields keep their enumerability
/// and order, so `js-keys` sees what a single file's loadable has. A chunk
/// the host has imported already resolves the import at once, and one that
/// fails to evaluate rejects it and stays unrecorded. The runtime's
/// `Loadable` and `light.lazy/load` stay as a single file has them, so a
/// program without chunks pays nothing for this. A chunk that another
/// chunk's static import evaluated is not recorded until a loadable of it is
/// loaded: chunks carry no load markers (docs/code-splitting.md, "Open
/// questions").
///
/// Terse output keeps the text's local names, and no short name it gives
/// may spell one ([`crate::terse`] excludes every name of the text that is
/// not a compiler name), so the locals are `m`, which the getters
/// `(m) => m.g20` spell already, and names starting with `_`, which
/// [`crate::js_syntax::NameGenerator`] never makes. light-cljs-353 measured
/// 687 bytes for the readable version before it, which kept each chunk's
/// waiting loadables, and single-letter locals costing the other chunks 66
/// bytes of longer names.
const LAZY: &str = "  const lazy$ = ((m) => (_, _l, _i = _l.importer, _g = _l.getter, _d = _l.load) => Object.defineProperties(_l, {
    importer: {get: () => _ in m ? null : _i},
    getter: {value: () => _g(m[_])},
    load: {configurable: 1, writable: 1, value: () => _ in m ? _d.call(_l) : _i().then((_m) => (m[_] = _m, _l.deref()))}
  }))([]);
";

/// One chunk of a chunked build: its ES module, and the chunks it imports,
/// in the order of its import lines.
pub(crate) struct EmittedChunk {
    pub(crate) source: String,
    pub(crate) imports: Vec<ChunkId>,
    /// Its runtime at `Advanced`, for the size report.
    pub(crate) runtime: ChunkRuntime,
    /// The constant table entries it declares, in declaration order, for
    /// the size report.
    pub(crate) constants: Vec<ChunkConstant>,
}

/// One chunk's text as the emitter wrote it, before renaming.
struct Written {
    /// Module-level declarations: the helpers, startup's runtime, core
    /// cells and constant table, and the chunk's hoisted methods.
    head: String,
    /// The embedded runtime factory in `head`.
    runtime: Option<std::ops::Range<usize>>,
    /// The function that runs the chunk's forms, `(() => { ... })()`.
    run: String,
    /// The raw `js*` text in `run`.
    raw: Vec<std::ops::Range<usize>>,
    /// The packages the chunk imports.
    packages: Vec<PackageImport>,
}

/// By chunk of `plan`: the chunks that have evaluated before its forms run,
/// by chunk index: itself, startup, and what it imports, transitively.
fn loaded_chunks(plan: &ChunkPlan) -> Vec<Vec<bool>> {
    let count = plan.chunks.len();
    // Imports precede their importers, so each chunk's prerequisites are
    // known when it is reached.
    let mut loaded = vec![vec![false; count]; count];
    for (chunk, members) in plan.chunks.iter().enumerate() {
        loaded[chunk][chunk] = true;
        loaded[chunk][STARTUP.0] = true;
        for import in &members.imports {
            debug_assert!(import.0 < chunk, "an import precedes its importer");
            let before = loaded[import.0].clone();
            for (loaded, before) in loaded[chunk].iter_mut().zip(before) {
                *loaded |= before;
            }
        }
    }
    loaded
}

/// The signatures of the constant table's entries (docs/code-splitting.md,
/// "Phase 3", constants): an entry's signature is the union of the
/// signatures of the chunks whose code reads it and of the entries whose
/// initializers read it, which are later entries; a hoisted method's is its
/// function's chunk's.
fn constant_signatures(plan: &ChunkPlan, constants: &[Constant]) -> Vec<Signature> {
    let mut signatures = constants
        .iter()
        .map(|constant| match constant.method {
            Some(chunk) => plan.chunks[chunk.0].signature.clone(),
            None => constant.readers.clone(),
        })
        .collect::<Vec<_>>();
    for (entry, constant) in constants.iter().enumerate().rev() {
        let (earlier, rest) = signatures.split_at_mut(entry);
        for &part in &constant.parts {
            debug_assert!(part < entry, "k{entry} reads a later entry, k{part}");
            earlier[part].union(&rest[0]);
        }
    }
    signatures
}

/// The runtime units of an `Advanced` chunked build and their signatures
/// (docs/code-splitting.md, "Phase 3", runtime units), in [`UnitId`] order:
/// the units the runtime names each chunk's code reads keep, with the chunk's
/// signature, and those each constant's initializer reads keep, with the
/// constant's ([`crate::runtime_slice::unit_signatures`]). A signature
/// holding startup's root is every root, as startup's chunk's is.
fn unit_signatures(
    plan: &ChunkPlan,
    runtime: &[RuntimeRoots],
    constants: &[Constant],
    constant_signatures: &[Signature],
) -> Vec<(UnitId, Signature)> {
    let roots = plan.roots.len();
    let words = Signature::empty(roots).0.len();
    let demands = runtime
        .iter()
        .zip(&plan.chunks)
        .map(|(names, chunk)| (names, chunk.signature.0.as_slice()))
        .chain(
            constants
                .iter()
                .zip(constant_signatures)
                .map(|(constant, signature)| (&constant.runtime, signature.0.as_slice())),
        )
        .collect::<Vec<_>>();
    crate::runtime_slice::unit_signatures(&demands, words)
        .into_iter()
        .enumerate()
        .filter(|(_, words)| words.iter().any(|&word| word != 0))
        .map(|(unit, words)| {
            let signature = Signature(words);
            let signature = if signature.contains(STARTUP_ROOT) {
                Signature::full(roots)
            } else {
                signature
            };
            (UnitId(unit), signature)
        })
        .collect()
}

/// One top-level item collected for a chunk in a single pre-pass over
/// `program.root`.
#[derive(Clone, Copy)]
enum ChunkFormItem {
    NamespaceEntry { id: ExprId, empty_return: bool },
    Form { id: ExprId, context: ExprContext },
}

struct ChunkCollector<'a> {
    program: &'a Program,
    plan: &'a ChunkPlan,
    sources: &'a HashMap<&'a str, usize>,
    forms: &'a [Option<ChunkId>],
}

impl ChunkCollector<'_> {
    fn collect(
        &self,
        id: ExprId,
        context: ExprContext,
        namespace: usize,
        out: &mut [Vec<ChunkFormItem>],
    ) {
        let namespace = self
            .program
            .namespace_entries
            .get(&id)
            .map_or(namespace, |name| self.sources[name.as_str()]);
        let ExprKind::Do(body) = &self.program.exprs[id.0].kind else {
            if let Some(chunk) = self.forms[id.0] {
                debug_assert!(
                    context != ExprContext::Return || chunk == STARTUP,
                    "the program's value is startup's"
                );
                out[chunk.0].push(ChunkFormItem::Form { id, context });
            }
            return;
        };
        let target = self.plan.namespace_chunks[namespace];
        let empty_return = body.is_empty() && context == ExprContext::Return;
        if empty_return {
            debug_assert_eq!(target, STARTUP, "the program's value is startup's");
        }
        if self.program.namespace_entries.contains_key(&id) || empty_return {
            out[target.0].push(ChunkFormItem::NamespaceEntry { id, empty_return });
        }
        for (index, &form) in body.iter().enumerate() {
            let context = if index + 1 == body.len() {
                context
            } else {
                ExprContext::Discard
            };
            self.collect(form, context, namespace, out);
        }
    }
}

/// Output of one parallel chunk/sub-slice emission task.
struct ChunkTaskOutput<'a> {
    chunk: ChunkId,
    output: String,
    raw_segments: Vec<std::ops::Range<usize>>,
    imports: Vec<PackageImport>,
    operands_declared: usize,
    runtime_global: RuntimeRoots,
    startup_runtime: RuntimeRoots,
    own_runtime: RuntimeRoots,
    core_cells: Vec<(&'static str, RuntimeName)>,
    targets: Vec<(ChunkId, String)>,
    lazy: bool,
    constant_reads: usize,
    local_constants: Vec<LocalConstant<'a>>,
    local_chunk_constants: Vec<Constant>,
    has_raw_js: bool,
    needs_runtime: bool,
}

/// The chunks of `plan` for `program` with the decisions of `application`,
/// in plan order, with the plan they follow: `plan` and the chunks its
/// constants and runtime units add, which hold no form; see the module
/// documentation. `None` when the forms reach reflection at `Advanced` and
/// `plan` moves a definition out of its namespace's chunk or dropped a chunk
/// that writes no code without reflection: such a program is planned at
/// namespace granularity (docs/code-splitting.md, "Phase 3", the
/// reflection gate). `reflective`: the forms are known to reach reflection,
/// so they are written once, with the namespaces' registration.
///
/// Runs on `slots` CPU work slots (`--jobs N`): parallel per-chunk/sub-slice
/// AST → JS emission with canonical constant merging, parallel `scanned_names`
/// import/export discovery, and parallel [`crate::terse::terse_texts_with_slots`].
/// Keeps `Slots::ONE` inline.
pub(crate) fn emit_chunked_with_slots(
    program: &Program,
    application: Application<'_>,
    plan: &ChunkPlan,
    reflective: bool,
    slots: crate::scheduler::Slots,
) -> Option<(ChunkPlan, Vec<EmittedChunk>)> {
    let direct = application.direct;
    let slice = direct.is_some();
    let terse = slice && application.terse;
    let count = plan.chunks.len();
    let sources = program
        .namespaces
        .iter()
        .enumerate()
        .map(|(source, name)| (name.as_str(), source))
        .collect::<HashMap<_, _>>();
    let mut forms = vec![None; program.exprs.len()];
    for item in &plan.items {
        forms[item.expr.0] = Some(item.chunk);
    }
    let loaded = loaded_chunks(plan);
    // A form outside every namespace's body is the entry's.
    let entry = plan.roots[crate::chunks::STARTUP_ROOT].namespace;

    // Stage 1: Emit all chunk bodies (serial inline on `Slots::ONE`, parallel
    // across chunks and sub-slices of large chunks on `slots > Slots::ONE`).
    let (
        bodies,
        mut merged_runtime,
        mut core_cells,
        mut chunks_constants,
        mut chunks_runtime,
        mut chunks_targets,
        chunks_lazy,
        constants_declarations,
        raw_js,
        needs_runtime,
        register_namespaces,
    ) = if slots == crate::scheduler::Slots::ONE {
        let setup = Setup {
            demand_roots: false,
            application_roots: false,
            module: true,
            mapped: false,
            application: Some(application),
            direct,
            function_names: None,
            js_exposed_locals: None,
            unit_root: None,
            shared_tables: None,
            constant_slice: None,
        };
        // As in the single file, `Advanced` output registers its namespaces only
        // when the forms of all chunks reach reflection.
        let write = |register_namespaces: bool| {
            let mut emitter = Emitter::new(program, setup, register_namespaces);
            emitter.chunks = Some(Chunks {
                plan,
                current: STARTUP,
                loaded: &loaded,
                task_mode: false,
                constants: Vec::new(),
                initializer: None,
                runtime: vec![RuntimeRoots::default(); count],
                targets: Vec::new(),
                lazy: false,
                constant_reads: 0,
            });
            let mut bodies = Vec::with_capacity(count);
            for chunk in 0..count {
                emitter.chunks.as_mut().unwrap().current = ChunkId(chunk);
                emitter.imports = Some(Vec::new());
                emitter.operand_scope(false, |emitter| {
                    emitter.chunk_forms(program.root, ExprContext::Return, entry, &sources, &forms)
                });
                bodies.push((
                    std::mem::take(&mut emitter.output),
                    std::mem::take(&mut emitter.raw_segments),
                    emitter.imports.take().unwrap_or_default(),
                ));
            }
            emitter.record_application_roots(Some(application));
            (emitter, bodies)
        };
        let mut register_namespaces = !slice || reflective;
        let (mut emitter, mut bodies) = write(register_namespaces);
        if slice && !reflective && emitter.runtime.reach_reflection() {
            if plan.moves_definitions() || plan.drops_chunks() {
                return None;
            }
            register_namespaces = true;
            (emitter, bodies) = write(true);
        }
        let chunks = emitter.chunks.take().expect("a chunked build");
        (
            bodies,
            emitter.runtime,
            emitter.core_cells,
            chunks.constants,
            chunks.runtime,
            chunks.targets,
            chunks.lazy,
            emitter.constants.map(|c| c.declarations),
            emitter.has_raw_js,
            emitter.needs_runtime,
            register_namespaces,
        )
    } else {
        // Pre-collect every chunk's forms in one linear walk of `program.root`.
        let mut chunk_items = vec![Vec::new(); count];
        ChunkCollector {
            program,
            plan,
            sources: &sources,
            forms: &forms,
        }
        .collect(program.root, ExprContext::Return, entry, &mut chunk_items);
        // Split large chunks (such as `STARTUP`) into weight-balanced
        // sub-slices so a single large chunk cannot bottleneck the pool.
        let mut memo = vec![0u32; program.exprs.len()];
        let mut stack = Vec::new();
        struct ChunkTaskSpec {
            chunk: ChunkId,
            range: std::ops::Range<usize>,
            weight: u64,
        }
        let mut tasks = Vec::with_capacity(count);
        for (chunk_idx, items) in chunk_items.iter().enumerate() {
            if items.is_empty() {
                continue;
            }
            let weights: Vec<u64> = items
                .iter()
                .map(|&item| match item {
                    ChunkFormItem::NamespaceEntry { .. } => 1,
                    ChunkFormItem::Form { id, .. } => {
                        super::parallel::expr_weight(program, id, &mut memo, &mut stack)
                    }
                })
                .collect();
            let total_w: u64 = weights.iter().sum();
            let sub_slices = if total_w >= 2048 && items.len() >= 16 {
                (slots.get() * 2).min(items.len())
            } else {
                1
            };
            for range in super::parallel::partition_slices(&weights, sub_slices) {
                let weight: u64 = weights[range.clone()].iter().sum();
                tasks.push(ChunkTaskSpec {
                    chunk: ChunkId(chunk_idx),
                    range,
                    weight,
                });
            }
        }
        drop(memo);
        drop(stack);

        let shared_tables = super::parallel::SharedTables::new(program, direct);
        let run_tasks = |register_namespaces: bool| -> Vec<ChunkTaskOutput<'_>> {
            let jobs: Vec<(u64, (usize, &ChunkTaskSpec))> = tasks
                .iter()
                .enumerate()
                .map(|(task_id, spec)| (spec.weight, (task_id, spec)))
                .collect();
            crate::scheduler::map(slots, jobs, |(task_id, spec)| {
                let mut emitter = Emitter::new(
                    program,
                    Setup {
                        demand_roots: false,
                        application_roots: false,
                        module: true,
                        mapped: false,
                        application: Some(application),
                        direct,
                        function_names: None,
                        js_exposed_locals: None,
                        unit_root: None,
                        shared_tables: Some(&shared_tables),
                        constant_slice: slice.then_some(task_id),
                    },
                    register_namespaces,
                );
                emitter.chunks = Some(Chunks {
                    plan,
                    current: spec.chunk,
                    loaded: &loaded,
                    task_mode: true,
                    constants: Vec::new(),
                    initializer: None,
                    runtime: vec![RuntimeRoots::default(); 2],
                    targets: Vec::new(),
                    lazy: false,
                    constant_reads: 0,
                });
                emitter.operands = Some(Operands {
                    hoists: false,
                    ..Operands::default()
                });
                for &item in &chunk_items[spec.chunk.0][spec.range.clone()] {
                    match item {
                        ChunkFormItem::NamespaceEntry { id, empty_return } => {
                            let site = emitter.site.replace(id);
                            emitter.namespace_entry(id);
                            if empty_return {
                                emitter.indentation();
                                emitter.output.push_str("return null;\n");
                            }
                            emitter.site = site;
                        }
                        ChunkFormItem::Form { id, context } => {
                            emitter.emit(id, context);
                        }
                    }
                }
                let operands_declared = emitter.operands.take().map_or(0, |o| o.declared);
                let mut ch = emitter.chunks.take().expect("a chunked build");
                let own_runtime = ch.runtime.pop().unwrap();
                let startup_runtime = ch.runtime.pop().unwrap();
                ChunkTaskOutput {
                    chunk: spec.chunk,
                    output: emitter.output,
                    raw_segments: emitter.raw_segments,
                    imports: emitter.imports.take().unwrap_or_default(),
                    operands_declared,
                    runtime_global: emitter.runtime,
                    startup_runtime,
                    own_runtime,
                    core_cells: emitter.core_cells,
                    targets: ch.targets,
                    lazy: ch.lazy,
                    constant_reads: ch.constant_reads,
                    local_constants: emitter.constants.map(|c| c.entries).unwrap_or_default(),
                    local_chunk_constants: ch.constants,
                    has_raw_js: emitter.has_raw_js,
                    needs_runtime: emitter.needs_runtime,
                }
            })
        };

        let merge_runtime =
            |task_outputs: &[ChunkTaskOutput<'_>]| -> (RuntimeRoots, Vec<RuntimeRoots>) {
                let mut global = RuntimeRoots::default();
                let mut per_chunk = vec![RuntimeRoots::default(); count];
                for task in task_outputs {
                    global.extend(&task.runtime_global);
                    per_chunk[STARTUP.0].extend(&task.startup_runtime);
                    if task.chunk != STARTUP {
                        per_chunk[task.chunk.0].extend(&task.own_runtime);
                    }
                }
                if slice && !global.is_empty() {
                    let mut app_roots = RuntimeRoots::default();
                    for &(unit, site) in application.protocol_declarations {
                        app_roots.record(
                            RuntimeName::direct(unit, None),
                            Some(site),
                            NameUse::Keep,
                        );
                    }
                    for &(unit, site) in application.loadable_members {
                        app_roots.record(
                            RuntimeName::direct(unit, None),
                            Some(site),
                            NameUse::Keep,
                        );
                    }
                    for &(unit, site) in application.member_units {
                        app_roots.record(
                            RuntimeName::direct(unit, None),
                            Some(site),
                            NameUse::Keep,
                        );
                    }
                    if let Some(site) = application.type_static {
                        app_roots.record(
                            RuntimeName::direct(crate::runtime_slice::TYPE_STATICS_UNIT, None),
                            Some(site),
                            NameUse::Keep,
                        );
                    }
                    if let Some(site) = application
                        .inference
                        .and_then(|inference| inference.facts().core_method_write())
                    {
                        app_roots.record(
                            RuntimeName::direct("extendType", None),
                            Some(site),
                            NameUse::Keep,
                        );
                        app_roots.record(
                            RuntimeName::direct("slotWrites", None),
                            Some(site),
                            NameUse::Keep,
                        );
                    }
                    if let Some(site) = application.multi_tables {
                        app_roots.record(
                            RuntimeName::direct(crate::runtime_slice::MULTI_TABLES_UNIT, None),
                            Some(site),
                            NameUse::Keep,
                        );
                    }
                    global.extend(&app_roots);
                    per_chunk[STARTUP.0].extend(&app_roots);
                }
                (global, per_chunk)
            };

        let mut register_namespaces = !slice || reflective;
        let mut task_outputs = run_tasks(register_namespaces);
        let (mut merged_runtime, mut chunks_runtime) = merge_runtime(&task_outputs);
        if slice && !reflective && merged_runtime.reach_reflection() {
            if plan.moves_definitions() || plan.drops_chunks() {
                return None;
            }
            register_namespaces = true;
            task_outputs = run_tasks(true);
            (merged_runtime, chunks_runtime) = merge_runtime(&task_outputs);
        }
        drop(shared_tables);
        drop(chunk_items);

        // Ordered merge of task metadata and canonical constant table.
        let mut core_cells: Vec<(&'static str, RuntimeName)> = Vec::new();
        let mut targets: Vec<(ChunkId, String)> = Vec::new();
        let mut lazy = false;
        let mut has_raw_js = false;
        let mut needs_runtime = false;
        let mut canon_numbers: HashMap<(ConstantKey<'_>, bool), usize> = HashMap::new();
        let mut canon_constants: Vec<Constant> = Vec::new();
        let mut canon_declarations = String::new();
        let mut remaps: Vec<Vec<usize>> = Vec::with_capacity(task_outputs.len());
        let mut base_reads = 0usize;

        for (task_id, task) in task_outputs.iter_mut().enumerate() {
            for &(name, initial) in &task.core_cells {
                if !core_cells.iter().any(|(existing, _)| *existing == name) {
                    core_cells.push((name, initial));
                }
            }
            targets.extend(std::mem::take(&mut task.targets));
            lazy |= task.lazy;
            has_raw_js |= task.has_raw_js;
            needs_runtime |= task.needs_runtime;

            if slice {
                let mut entries = std::mem::take(&mut task.local_constants);
                let mut metas = std::mem::take(&mut task.local_chunk_constants);
                debug_assert_eq!(entries.len(), metas.len());
                let mut remap = Vec::with_capacity(entries.len());
                let mut new_entries: Vec<(usize, usize)> = Vec::new();
                for (local_id, (entry, meta)) in entries.iter().zip(metas.iter_mut()).enumerate() {
                    let canon_key = super::parallel::remap_constant_key(&entry.key, &remap);
                    let dedup_key = (canon_key, entry.lazy);
                    let adjusted_first = meta
                        .first
                        .map(|(seq, chunk, site)| (base_reads + seq, chunk, site));
                    if let Some(&cid) = canon_numbers.get(&dedup_key) {
                        remap.push(cid);
                        canon_constants[cid].readers.union(&meta.readers);
                        if canon_constants[cid].first.is_none() {
                            canon_constants[cid].first = adjusted_first;
                        }
                        canon_constants[cid].runtime.extend(&meta.runtime);
                    } else {
                        let cid = canon_constants.len();
                        canon_numbers.insert(dedup_key, cid);
                        remap.push(cid);
                        let remapped_parts = meta.parts.iter().map(|&p| remap[p]).collect();
                        canon_constants.push(Constant {
                            declaration: 0..0,
                            method: meta.method,
                            parts: remapped_parts,
                            readers: meta.readers.clone(),
                            runtime: std::mem::take(&mut meta.runtime),
                            label: std::mem::take(&mut meta.label),
                            first: adjusted_first,
                        });
                        new_entries.push((local_id, cid));
                    }
                }
                new_entries
                    .sort_unstable_by_key(|&(local_id, _)| entries[local_id].finalized_order);
                let k_prefix = format!("k{task_id}_");
                for (local_id, cid) in new_entries {
                    let entry = &mut entries[local_id];
                    let needs_rewrite = matches!(
                        entry.key,
                        ConstantKey::Composite { .. }
                            | ConstantKey::Shape { .. }
                            | ConstantKey::Method { .. }
                    ) && entry.value.contains(&k_prefix);
                    if needs_rewrite {
                        let (rewritten, _) = super::parallel::rewrite_slice_k_names(
                            &entry.value,
                            &[],
                            &k_prefix,
                            &remap,
                        );
                        entry.value = rewritten;
                    }
                    let decl = if entry.lazy {
                        format!("  let c{cid}; const k{cid} = () => c{cid} ??= ")
                    } else {
                        format!("  const k{cid} = ")
                    };
                    let start = canon_declarations.len();
                    writeln!(canon_declarations, "{decl}{};", entry.value).unwrap();
                    canon_constants[cid].declaration = start..canon_declarations.len();
                }
                remaps.push(remap);
            } else {
                remaps.push(Vec::new());
            }
            base_reads += task.constant_reads;
        }
        drop(canon_numbers);

        // Rewrite slice-local `k{t}_{local}` -> `k{canon}` in parallel across
        // tasks (shifting raw_segments) before joining sub-slices per chunk.
        let rewritten_tasks = if slice {
            let jobs: Vec<(u64, (usize, ChunkTaskOutput<'_>))> = task_outputs
                .into_iter()
                .enumerate()
                .map(|(task_id, task)| (task.output.len() as u64, (task_id, task)))
                .collect();
            crate::scheduler::map(slots, jobs, |(task_id, mut task)| {
                let remap = &remaps[task_id];
                if !remap.is_empty() {
                    let k_prefix = format!("k{task_id}_");
                    let (text, anchors) = super::parallel::rewrite_slice_k_names(
                        &task.output,
                        &task.raw_segments,
                        &k_prefix,
                        remap,
                    );
                    for range in &mut task.raw_segments {
                        let len = range.end - range.start;
                        let start = crate::terse::renamed_offset(&anchors, range.start);
                        range.start = start;
                        range.end = start + len;
                    }
                    task.output = text;
                }
                task
            })
        } else {
            task_outputs
        };

        // Assemble each chunk's `(forms, raw, packages)` from its ordered tasks.
        let mut bodies: Vec<(String, Vec<std::ops::Range<usize>>, Vec<PackageImport>)> = (0..count)
            .map(|_| (String::new(), Vec::new(), Vec::new()))
            .collect();
        let mut max_operands = vec![0usize; count];
        for task in rewritten_tasks {
            let chunk = task.chunk.0;
            max_operands[chunk] = max_operands[chunk].max(task.operands_declared);
            let (out, raw, imports) = &mut bodies[chunk];
            let base = out.len();
            out.push_str(&task.output);
            raw.extend(
                task.raw_segments
                    .into_iter()
                    .map(|r| r.start + base..r.end + base),
            );
            for import in task.imports {
                let index = Emitter::package_import(imports, &import.package);
                imports[index].namespace |= import.namespace;
                for export in import.exports {
                    if !imports[index].exports.contains(&export) {
                        imports[index].exports.push(export);
                    }
                }
            }
        }
        for (chunk, &declared) in max_operands.iter().enumerate() {
            if declared > 0 {
                let mut decl = String::from("  let ");
                for index in 0..declared {
                    if index > 0 {
                        decl.push_str(", ");
                    }
                    write!(decl, "operand{index}").unwrap();
                }
                decl.push_str(";\n");
                let (out, raw, _) = &mut bodies[chunk];
                out.insert_str(0, &decl);
                for range in raw {
                    range.start += decl.len();
                    range.end += decl.len();
                }
            }
        }

        (
            bodies,
            merged_runtime,
            core_cells,
            canon_constants,
            chunks_runtime,
            targets,
            lazy,
            slice.then_some(canon_declarations),
            has_raw_js,
            needs_runtime,
            register_namespaces,
        )
    };

    let embed = if slice {
        !merged_runtime.is_empty()
    } else {
        !program.namespace_entries.is_empty() || needs_runtime
    };
    debug_assert!(
        embed || merged_runtime.is_empty(),
        "emitted code reads the runtime without embedding it"
    );
    // Startup registers the namespaces before any form runs, and declares
    // what every chunk reads of the runtime below `Advanced`.
    let registration = if embed {
        if slice {
            // Startup's runtime registers the build's key as it loads.
            let key_root = RuntimeName::direct(super::RUNTIME_KEY_UNIT, None);
            merged_runtime.record(key_root, None, NameUse::Keep);
            chunks_runtime[STARTUP.0].record(key_root, None, NameUse::Keep);
        }
        if register_namespaces {
            let mut reg_emitter = Emitter::new(
                program,
                Setup {
                    demand_roots: false,
                    application_roots: false,
                    module: false,
                    mapped: false,
                    application: Some(application),
                    direct,
                    function_names: None,
                    js_exposed_locals: None,
                    unit_root: None,
                    shared_tables: None,
                    constant_slice: None,
                },
                true,
            );
            reg_emitter.runtime = std::mem::take(&mut merged_runtime);
            reg_emitter.core_cells = std::mem::take(&mut core_cells);
            reg_emitter.chunks = Some(Chunks {
                plan,
                current: STARTUP,
                loaded: &loaded,
                task_mode: false,
                constants: Vec::new(),
                initializer: None,
                runtime: std::mem::take(&mut chunks_runtime),
                targets: Vec::new(),
                lazy: false,
                constant_reads: 0,
            });
            let retained = |global: usize| !slice || application.globals[global];
            let reg = reg_emitter.registration(None, &[], true, retained, None);
            #[cfg(debug_assertions)]
            {
                merged_runtime = reg_emitter.runtime;
            }
            core_cells = reg_emitter.core_cells;
            chunks_runtime = reg_emitter.chunks.take().unwrap().runtime;
            reg
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let mut startup_head = String::new();
    for (name, initial) in &core_cells {
        writeln!(startup_head, "  let core${name} = {initial};").unwrap();
    }
    // Each constant table entry goes to the chunk of its signature, and at
    // `Advanced` each runtime unit too; either may be a chunk the plan does
    // not have. Each chunk declares its entries in the order the table
    // declares them, the parts of a composite first.
    let signatures = constant_signatures(plan, &chunks_constants);
    let units = if slice && embed {
        unit_signatures(plan, &chunks_runtime, &chunks_constants, &signatures)
    } else {
        Vec::new()
    };
    let constant_count = signatures.len();
    let mut placed_plan = plan.clone();
    let (placed_owners, renumbered) = placed_plan.place(
        program,
        &signatures
            .into_iter()
            .chain(units.iter().map(|(_, signature)| signature.clone()))
            .collect::<Vec<_>>(),
    );
    let (owners, unit_owners) = placed_owners.split_at(constant_count);
    let plan = &placed_plan;
    let count = plan.chunks.len();
    let manifest = RuntimeManifest::embedded();
    // By unit: the chunk whose slice holds it.
    let mut unit_chunks = vec![None; manifest.units.len()];
    let mut slices = (0..count)
        .map(|_| ChunkSlice {
            kept: vec![false; manifest.units.len()],
            parameters: Vec::new(),
            returned: Vec::new(),
        })
        .collect::<Vec<_>>();
    for (&(unit, _), &chunk) in units.iter().zip(unit_owners) {
        unit_chunks[unit.0] = Some(chunk);
        slices[chunk.0].kept[unit.0] = true;
    }
    for slice in &mut slices {
        slice.parameters = crate::runtime_slice::factory_parameters(&slice.kept);
    }
    // What each chunk's code and constants read of the runtime, for the size
    // report.
    let mut chunk_roots = vec![RuntimeRoots::default(); count];
    for (chunk, names) in chunks_runtime.iter().enumerate() {
        chunk_roots[renumbered[chunk].0].extend(names);
    }
    // Each entry's first demand by code, for the size report: its own first
    // read or an earlier one of an entry containing it, transitively (parts
    // precede their composites), in the placed plan's chunks.
    let mut firsts = chunks_constants
        .iter()
        .map(|constant| constant.first)
        .collect::<Vec<_>>();
    for (entry, constant) in chunks_constants.iter().enumerate().rev() {
        if let Some(first) = firsts[entry] {
            for &part in &constant.parts {
                if firsts[part].is_none_or(|demand| first.0 < demand.0) {
                    firsts[part] = Some(first);
                }
            }
        }
    }
    let firsts = firsts
        .into_iter()
        .map(|first| first.map(|(_, chunk, site)| (renumbered[chunk.0], site)))
        .collect::<Vec<_>>();
    let mut tables = vec![String::new(); count];
    let mut chunk_constants = vec![Vec::new(); count];
    if let Some(declarations) = &constants_declarations {
        let mut order = (0..chunks_constants.len()).collect::<Vec<_>>();
        order.sort_unstable_by_key(|&entry| chunks_constants[entry].declaration.start);
        for entry in order {
            let declaration = chunks_constants[entry].declaration.clone();
            tables[owners[entry].0].push_str(&declarations[declaration]);
            chunk_roots[owners[entry].0].extend(&chunks_constants[entry].runtime);
            chunk_constants[owners[entry].0].push(ChunkConstant {
                label: std::mem::take(&mut chunks_constants[entry].label),
                first: firsts[entry],
            });
        }
    }
    #[cfg(debug_assertions)]
    for table in &tables {
        check_runtime_reads(table, &[], &merged_runtime, &core_cells);
    }
    startup_head.push_str(&tables[STARTUP.0]);
    let mut placed = (0..count).map(|_| None).collect::<Vec<_>>();
    for (chunk, body) in bodies.into_iter().enumerate() {
        placed[renumbered[chunk].0] = Some(body);
    }
    for (chunk, _) in &mut chunks_targets {
        *chunk = renumbered[chunk.0];
    }
    let mut written = placed
        .into_iter()
        .enumerate()
        .map(|(chunk, body)| {
            let head = if chunk == STARTUP.0 {
                std::mem::take(&mut startup_head)
            } else {
                std::mem::take(&mut tables[chunk])
            };
            // A chunk the constants or the runtime add holds no form.
            let Some((forms, raw, packages)) = body else {
                return Written {
                    head,
                    runtime: None,
                    run: String::new(),
                    raw: Vec::new(),
                    packages: Vec::new(),
                };
            };
            let open = if chunk == STARTUP.0 {
                format!("{PRELUDE_OPEN}{registration}")
            } else {
                PRELUDE_OPEN.to_owned()
            };
            let raw = raw
                .into_iter()
                .map(|range| range.start + open.len()..range.end + open.len())
                .collect();
            Written {
                head,
                runtime: None,
                run: format!("{open}{forms}}})()"),
                raw,
                packages,
            }
        })
        .collect::<Vec<_>>();

    // The names each chunk reads, and their owners: a runtime binding's is
    // the chunk whose slice declares it at `Advanced`, startup's below.
    let runtime_owner = |binding: &str| -> ChunkId {
        manifest
            .unit_of(binding)
            .and_then(|unit| unit_chunks[unit.0])
            .unwrap_or(STARTUP)
    };
    let owner = |name: &str| -> Option<ChunkId> {
        if let Some(binding) = name.strip_prefix("rt$") {
            Some(runtime_owner(binding))
        } else if name == "rt" || name == "lazy$" || name.starts_with("core$") {
            Some(STARTUP)
        } else if let Some(global) = owned_global(name) {
            Some(plan.global_chunks[global])
        } else {
            constant_entry(name).map(|entry| owners[entry])
        }
    };
    let scan_jobs: Vec<(u64, &Written)> = written
        .iter()
        .map(|text| ((text.head.len() + text.run.len()) as u64, text))
        .collect();
    let mut read: Vec<BTreeSet<String>> = crate::scheduler::map(slots, scan_jobs, |text| {
        #[cfg(debug_assertions)]
        check_runtime_reads(&text.run, &text.raw, &merged_runtime, &core_cells);
        let mut names = BTreeSet::new();
        for (source, skipped) in [(&text.head, &Vec::new()), (&text.run, &text.raw)] {
            for (range, word) in scanned_names(source, skipped) {
                // A member is no binding: `m.g20` reads a module namespace.
                if !source[..range.start].ends_with('.') {
                    names.insert(word.to_owned());
                }
            }
        }
        names
    });
    // Each chunk's runtime: a slice of the units of its signature that
    // returns the bindings its own code and other chunks read
    // (docs/code-splitting.md, "What a chunk file contains"),
    // `const [rt$a, rt$b] = ((c, d) => { ... })(rt$c, rt$d);`, where the
    // parameters are the bindings its units require of other chunks.
    let mut returned = vec![BTreeSet::new(); count];
    for (chunk, names) in read.iter_mut().enumerate() {
        for name in names.iter() {
            if let Some(binding) = name.strip_prefix("rt$") {
                let unit = manifest
                    .unit_of(binding)
                    .unwrap_or_else(|| panic!("{name} is not a runtime binding"));
                let binding = *manifest.units[unit.0]
                    .declares
                    .iter()
                    .find(|declared| **declared == binding)
                    .expect("a declared binding");
                returned[runtime_owner(binding).0].insert((unit, binding));
            }
        }
        for &parameter in &slices[chunk].parameters {
            let unit = manifest.unit_of(parameter).expect("a declared binding");
            returned[runtime_owner(parameter).0].insert((unit, parameter));
            names.insert(format!("rt${parameter}"));
        }
    }
    for (slice, returned) in slices.iter_mut().zip(&returned) {
        slice.returned = returned.iter().map(|&(_, binding)| binding).collect();
    }
    let mut chunk_runtimes = chunk_roots
        .into_iter()
        .map(|roots| ChunkRuntime {
            roots,
            ..ChunkRuntime::default()
        })
        .collect::<Vec<_>>();
    // Each binding a chunk's units take from another chunk is that chunk's,
    // whose signature strictly contains the taker's.
    #[cfg(debug_assertions)]
    for (chunk, slice) in slices.iter().enumerate() {
        for &parameter in &slice.parameters {
            let owner = runtime_owner(parameter);
            let (outer, inner) = (
                &plan.chunks[owner.0].signature,
                &plan.chunks[chunk].signature,
            );
            debug_assert!(
                outer != inner && outer.is_superset(inner),
                "{} takes {parameter} of {}",
                plan.chunks[chunk].name,
                plan.chunks[owner.0].name
            );
        }
    }
    // Terse output writes each chunk's units at module level
    // (light-cljs-303): their bindings are named by the chunks' one table,
    // exported and imported as the program's names are, and the units'
    // text is written once the table is known.
    let module_units = embed && slice && terse && !names_kept_spelling(&written, &slices);
    if module_units {
        for (slice, runtime) in slices.iter().zip(&mut chunk_runtimes) {
            if slice.kept.contains(&true) {
                runtime.kept = slice.kept.clone();
            }
        }
    } else if embed && slice {
        let factories = crate::runtime_slice::chunk_factories(&slices, terse);
        for (chunk, (text, (factory, unit_bytes))) in written.iter_mut().zip(factories).enumerate()
        {
            let slice = &slices[chunk];
            if !slice.kept.contains(&true) {
                continue;
            }
            let factory = factory.trim();
            let arguments = if chunk == STARTUP.0 {
                String::new()
            } else {
                let arguments = slice
                    .parameters
                    .iter()
                    .map(|parameter| format!("rt${parameter}"))
                    .collect::<Vec<_>>();
                format!("({})", arguments.join(", "))
            };
            let open = if slice.returned.is_empty() {
                "  ".to_owned()
            } else {
                let pattern = slice
                    .returned
                    .iter()
                    .map(|binding| format!("rt${binding}"))
                    .collect::<Vec<_>>();
                format!("  const [{}] = ", pattern.join(", "))
            };
            text.head
                .insert_str(0, &format!("{open}{factory}{arguments};\n"));
            text.runtime = Some(open.len()..open.len() + factory.len());
            let runtime = &mut chunk_runtimes[chunk];
            runtime.kept = slice.kept.clone();
            // Counted as the output writer writes them, keyed.
            runtime.bytes = crate::runtime_key::written_len(factory);
            runtime.unit_bytes = unit_bytes.map(crate::runtime_slice::written_unit_bytes);
        }
    } else if embed {
        let statement = format!(
            "  const rt = {};\n",
            crate::runtime_manifest::RUNTIME.trim()
        );
        let factory = statement.find("(() =>").unwrap_or_default();
        written[STARTUP.0].runtime = Some(factory..statement.len() - 2);
        written[STARTUP.0].head.insert_str(0, &statement);
    }
    // The prelude helpers each chunk calls, declared first.
    for (chunk, text) in written.iter_mut().enumerate() {
        let names = &read[chunk];
        let mut helpers_text = String::new();
        if names.contains("truth") {
            helpers_text.push_str(PRELUDE_TRUTH);
        }
        helpers_text.push_str(&helpers(names.contains("host"), names.contains("pkg")));
        if chunk == STARTUP.0 && chunks_lazy {
            helpers_text.push_str(LAZY);
        }
        text.head.insert_str(0, &helpers_text);
        if let Some(runtime) = &mut text.runtime {
            *runtime = runtime.start + helpers_text.len()..runtime.end + helpers_text.len();
        }
    }
    let loaded = loaded_chunks(plan);
    // What each chunk imports from each other chunk, which that chunk
    // exports; and the globals each chunk declares.
    let mut imports: Vec<BTreeMap<ChunkId, BTreeSet<String>>> = vec![BTreeMap::new(); count];
    let mut exports = vec![BTreeSet::new(); count];
    let mut globals = vec![BTreeSet::new(); count];
    for (chunk, names) in read.iter().enumerate() {
        for &import in &plan.chunks[chunk].imports {
            imports[chunk].entry(import).or_default();
        }
        for name in names {
            let Some(owner) = owner(name) else {
                continue;
            };
            if owned_global(name).is_some() && !name.ends_with('$') {
                globals[owner.0].insert(name.clone());
            }
            if owner.0 != chunk {
                // A constant's or a runtime binding's chunk is one more
                // import, of a chunk whose signature holds its readers'.
                debug_assert!(
                    owner == STARTUP
                        || if constant_entry(name).is_some() || name.starts_with("rt$") {
                            let (outer, inner) = (
                                &plan.chunks[owner.0].signature,
                                &plan.chunks[chunk].signature,
                            );
                            outer != inner && outer.is_superset(inner)
                        } else {
                            loaded[chunk][owner.0]
                        },
                    "{} reads {name} of {}, which it does not import",
                    plan.chunks[chunk].name,
                    plan.chunks[owner.0].name
                );
                imports[chunk]
                    .entry(owner)
                    .or_default()
                    .insert(name.clone());
                exports[owner.0].insert(name.clone());
            }
        }
    }
    // A slice holding an `effect:` or `install:` unit, which no chunk reads,
    // has evaluated before any chunk of a smaller signature runs: every root
    // of that chunk needs it, and code loaded with it may build the values it
    // extends or call the operations it extends them for.
    let effects = slices
        .iter()
        .map(|slice| {
            manifest.units.iter().zip(&slice.kept).any(|(unit, kept)| {
                *kept
                    && unit
                        .effect
                        .as_ref()
                        .is_some_and(|effect| !effect.targets.is_empty())
            })
        })
        .collect::<Vec<_>>();
    let mut evaluated = vec![vec![false; count]; count];
    for chunk in 0..count {
        evaluated[chunk][chunk] = true;
        let mut before = imports[chunk].keys().copied().collect::<Vec<_>>();
        let signature = &plan.chunks[chunk].signature;
        for (other, members) in plan.chunks.iter().enumerate().skip(1) {
            if effects[other]
                && &members.signature != signature
                && members.signature.is_superset(signature)
                && !before.iter().any(|import| evaluated[import.0][other])
            {
                debug_assert!(other < chunk, "an import precedes its importer");
                imports[chunk].entry(ChunkId(other)).or_default();
                before.push(ChunkId(other));
            }
        }
        for import in before {
            let (earlier, rest) = evaluated.split_at_mut(chunk);
            for (own, imported) in rest[0].iter_mut().zip(&earlier[import.0]) {
                *own |= *imported;
            }
        }
    }
    for (chunk, name) in &chunks_targets {
        globals[chunk.0].insert(name.clone());
        exports[chunk.0].insert(name.clone());
    }
    // The setters other chunks call: `const g7$ = (v) => g7 = v;`.
    for (chunk, text) in written.iter_mut().enumerate() {
        for setter in &exports[chunk] {
            if let Some(binding) = assigned_binding(setter) {
                writeln!(text.head, "  const {setter} = (v) => {binding} = v;").unwrap();
                if owned_global(binding).is_some() {
                    globals[chunk].insert(binding.to_owned());
                }
            }
        }
    }

    // One renaming of every text.
    let texts = written
        .iter()
        .zip(&chunk_runtimes)
        .flat_map(|(text, runtime)| {
            [
                Text {
                    source: &text.head,
                    raw: Vec::new(),
                    runtime: text.runtime.clone(),
                    units: (module_units && !runtime.kept.is_empty())
                        .then_some(runtime.kept.as_slice()),
                },
                Text {
                    source: &text.run,
                    raw: text.raw.clone(),
                    runtime: None,
                    units: None,
                },
            ]
        })
        .collect::<Vec<_>>();
    // Each compiler name's final spelling: a short name at `Advanced`, the
    // hygiene prefix and the name where raw text could capture it, else the
    // name.
    let (renamed, table, prefix) = if terse {
        let (renamed, names) = crate::terse::terse_texts_with_slots(&texts, slots);
        let names = names
            .into_iter()
            .map(|(word, name)| (word.to_owned(), name))
            .collect::<HashMap<_, _>>();
        (renamed, Some(names), String::new())
    } else if raw_js {
        let prefix = crate::terse::raw_hygiene_prefix(written.iter().flat_map(|text| {
            text.raw
                .iter()
                .map(|range| &text.run[range.clone()])
                .chain(text.runtime.iter().map(|range| &text.head[range.clone()]))
        }));
        let rename_jobs: Vec<(u64, &Text<'_>)> = texts
            .iter()
            .map(|text| (text.source.len() as u64, text))
            .collect();
        let renamed = crate::scheduler::map(slots, rename_jobs, |text| {
            let skipped = text
                .raw
                .iter()
                .cloned()
                .chain(text.runtime.clone())
                .collect::<Vec<_>>();
            crate::terse::rename_compiler_names(text.source, &skipped, |word| {
                format!("{prefix}{word}")
            })
            .0
        });
        (renamed, None, prefix)
    } else {
        let renamed = texts.iter().map(|text| text.source.to_owned()).collect();
        (renamed, None, String::new())
    };
    let name = |word: &str| -> String {
        match &table {
            Some(table) => table
                .get(word)
                .cloned()
                .unwrap_or_else(|| panic!("`{word}` has no short name")),
            None => format!("{prefix}{word}"),
        }
    };
    let space = if terse { "" } else { " " };
    let mut renamed = renamed.into_iter();
    let files = written
        .into_iter()
        .zip(chunk_runtimes)
        .enumerate()
        .map(|(chunk, (text, mut runtime))| {
            let (head, run) = (renamed.next().unwrap(), renamed.next().unwrap());
            let mut source = package_imports(&text.packages);
            // The chunks it imports, in the order their namespaces load in
            // the single file; a chunk of moved definitions alone, which
            // have no effect, by the namespace of its first; and last a
            // chunk of constants alone, which only its readers need.
            let mut imported = imports[chunk].iter().collect::<Vec<_>>();
            imported.sort_by_key(|(import, _)| {
                let chunk = &plan.chunks[import.0];
                chunk
                    .namespaces
                    .first()
                    .copied()
                    .or_else(|| chunk.items.first().map(|&item| plan.items[item].namespace))
                    .map_or((1, 0), |namespace| (0, namespace))
            });
            for (import, names) in &imported {
                let specifier = quoted(&format!("./{}", plan.chunks[import.0].name));
                if names.is_empty() {
                    writeln!(source, "import {specifier};").unwrap();
                } else {
                    let names = names
                        .iter()
                        .map(|word| name(word))
                        .collect::<Vec<_>>()
                        .join(&format!(",{space}"));
                    writeln!(
                        source,
                        "import{space}{{{space}{names}{space}}}{space}from{space}{specifier};"
                    )
                    .unwrap();
                }
            }
            // The chunk's runtime units, at module level in terse output.
            if module_units && !runtime.kept.is_empty() {
                let table = table.as_ref().expect("terse output names every text");
                let (units, unit_bytes) =
                    crate::terse::terse_module_units(&runtime.kept, |word| table[word].as_str());
                runtime.bytes = crate::runtime_key::written_len(&units);
                runtime.unit_bytes = Some(crate::runtime_slice::written_unit_bytes(unit_bytes));
                source.push_str(&units);
                source.push('\n');
            }
            if !head.trim().is_empty() {
                source.push_str(&head);
            }
            // The globals it declares, exported where another chunk reads
            // them, ordered by number.
            let mut declared = globals[chunk].iter().collect::<Vec<_>>();
            declared.sort_by_key(|word| global_order(word));
            let (exported, local): (Vec<_>, Vec<_>) = declared
                .into_iter()
                .partition(|word| exports[chunk].contains(*word));
            for (keyword, words) in [("export let", exported), ("let", local)] {
                if !words.is_empty() {
                    let words = words
                        .iter()
                        .map(|word| name(word))
                        .collect::<Vec<_>>()
                        .join(&format!(",{space}"));
                    writeln!(source, "{keyword} {words};").unwrap();
                }
            }
            // The other names other chunks read: setters, the runtime, core
            // cells and constants.
            let listed = exports[chunk]
                .iter()
                .filter(|word| !globals[chunk].contains(*word))
                .map(|word| name(word))
                .collect::<Vec<_>>();
            if !listed.is_empty() {
                writeln!(
                    source,
                    "export{space}{{{space}{}{space}}};",
                    listed.join(&format!(",{space}"))
                )
                .unwrap();
            }
            if chunk == STARTUP.0 {
                source.push_str("export default ");
            }
            if !run.is_empty() {
                source.push_str(run.trim_end());
                source.push_str(";\n");
            }
            EmittedChunk {
                source,
                imports: imported.iter().map(|(import, _)| **import).collect(),
                runtime,
                constants: std::mem::take(&mut chunk_constants[chunk]),
            }
        })
        .collect();
    Some((placed_plan, files))
}

/// Whether a text of the chunks, or a prelude helper, has a word that is a
/// runtime binding the `slices` declare and that keeps its spelling
/// (`crate::terse::keeps_spelling`): a constructor such as `Atom`, which
/// units at module level declare under that name, so it would capture a
/// global of that name that raw `js*` text reads (`typeof Atom`). The
/// chunks then keep their factories. Any such word counts, in strings too,
/// and so does any raw text with a `\u` escape.
fn names_kept_spelling(written: &[Written], slices: &[ChunkSlice]) -> bool {
    let manifest = RuntimeManifest::embedded();
    let spelled = slices
        .iter()
        .flat_map(|slice| {
            manifest
                .units
                .iter()
                .zip(&slice.kept)
                .filter(|(_, kept)| **kept)
                .flat_map(|(unit, _)| unit.declares.iter().copied())
        })
        .filter(|binding| crate::terse::keeps_spelling(binding))
        .collect::<BTreeSet<_>>();
    if spelled.is_empty() {
        return false;
    }
    // Raw text may spell an identifier with escapes, `Atom` or
    // `\u{41}tom`, which only the emitter's own text never has.
    if written.iter().any(|text| {
        text.raw
            .iter()
            .any(|range| text.run[range.clone()].contains("\\u"))
    }) {
        return true;
    }
    let identifier = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$');
    let helpers = helpers(true, true);
    let fixed = [PRELUDE_TRUTH, LAZY, helpers.as_str()];
    written
        .iter()
        .flat_map(|text| [text.head.as_str(), text.run.as_str()])
        .chain(fixed)
        .any(|text| {
            let bytes = text.as_bytes();
            spelled.iter().any(|word| {
                text.match_indices(word).any(|(at, _)| {
                    !(at > 0 && identifier(bytes[at - 1]))
                        && !bytes
                            .get(at + word.len())
                            .is_some_and(|&byte| identifier(byte))
                })
            })
        })
}

/// The order of declared global names: by global, then method entry.
fn global_order(word: &str) -> (usize, usize) {
    let mut parts = word[1..].split('_');
    let global = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    let entry = parts
        .next()
        .and_then(|part| part.parse::<usize>().ok())
        .map_or(0, |entry| entry + 1);
    (global, entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_to_globals_and_constant_entries() {
        assert_eq!(owned_global("g7"), Some(7));
        assert_eq!(owned_global("g7_1"), Some(7));
        assert_eq!(owned_global("g7$"), Some(7));
        assert_eq!(owned_global("g"), None);
        assert_eq!(owned_global("gx"), None);
        assert_eq!(constant_entry("k3"), Some(3));
        assert_eq!(constant_entry("c3"), Some(3));
        assert_eq!(constant_entry("captures3"), None);
        assert_eq!(constant_entry("caught"), None);
        assert!(crate::terse::compiler_name("g7$") && crate::terse::compiler_name("core$inc$"));
        assert_eq!(assigned_binding("g7$"), Some("g7"));
        assert_eq!(assigned_binding("core$inc$"), Some("core$inc"));
        assert_eq!(assigned_binding("lazy$"), None);
        assert_eq!(assigned_binding("core$inc"), None);
        assert_eq!(global_order("g7_1"), (7, 2));
        assert_eq!(global_order("g7"), (7, 0));
    }

    /// Terse output gives no short name that `lazy$`'s text spells, so its
    /// own names are its properties, the globals it reads, `m`, which a
    /// loadable's getter spells already, and names no short name starts
    /// like (light-cljs-353).
    #[test]
    fn the_loader_spells_no_short_name() {
        let fixed = [
            "const",
            "lazy$",
            "m",
            "Object",
            "defineProperties",
            "importer",
            "getter",
            "load",
            "get",
            "value",
            "configurable",
            "writable",
            "in",
            "null",
            "then",
            "deref",
            "call",
        ];
        let words = LAZY
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
            .filter(|word| !word.is_empty() && !word.bytes().all(|b| b.is_ascii_digit()));
        for word in words {
            assert!(
                fixed.contains(&word) || word.starts_with('_'),
                "`{word}` in lazy$ could be a short name"
            );
        }
        assert!(
            crate::js_syntax::NameGenerator::default()
                .take(100_000)
                .all(|name| !name.starts_with('_'))
        );
    }
}
