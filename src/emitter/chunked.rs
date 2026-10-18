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
//! A chunk's slice is a function of the bindings its units take from earlier
//! chunks, which it imports, and returns the bindings its code and later
//! chunks read.
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
    loaded: Vec<Vec<bool>>,
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
    /// `initializer`. Its readers place it.
    pub(super) fn constant(&mut self, number: usize, initializer: Initializer) {
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
        });
    }

    /// Record constant table entry `number`, a hoisted method of a function
    /// of the chunk being written, which declares it: its body's runtime
    /// reads are that chunk's.
    pub(super) fn method(&mut self, number: usize) {
        self.constant(number, Initializer::default());
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
    /// by the code of the chunk being written.
    pub(super) fn read_constant(&mut self, number: usize) {
        match &mut self.initializer {
            Some(initializer) => initializer.parts.push(number),
            None => {
                let signature = &self.plan.chunks[self.current.0].signature;
                self.constants[number].readers.union(signature);
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
                let chunk = if startup { STARTUP } else { self.current };
                self.runtime[chunk.0].record(name, site, used);
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

impl Emitter<'_> {
    /// A loadable of `target`, whose type `class` reads, in a chunk that may
    /// not have evaluated when the loadable's own chunk runs: an importer of
    /// that chunk and a getter that reads the target from its module
    /// namespace object, which startup's `lazy$` ([`LAZY`]) registers under
    /// the chunk, `lazy$("./app.editor.js", new Loadable(() =>
    /// import("./app.editor.js"), (m) => m.g20))`, or `() => null` for a
    /// namespace target.
    pub(super) fn lazy_loadable(&mut self, target: LazyTargetId, class: ExprId) {
        let chunks = self.chunks.as_mut().expect("a chunked build");
        chunks.lazy = true;
        let chunk = chunks
            .importer(self.program, target)
            .expect("a target in another chunk");
        let specifier = quoted(&format!("./{}", chunks.plan.chunks[chunk.0].name));
        write!(self.output, "lazy$({specifier}, new ").unwrap();
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
            self.used.insert(id);
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
/// `lazy$("./app.editor.js", new Loadable(importer, getter))`, keyed by the
/// target chunk (docs/code-splitting.md, "The loading API"). It records the
/// module namespace object of each chunk an importer has imported, and the
/// loadables of each chunk not imported yet. A loadable of an imported chunk
/// is made resident at once: a getter of the module's binding and no
/// importer, so `loaded?` and `@l` read what a single file's loadable has.
/// Any other gets a `load` that calls its importer; when the import
/// resolves, every loadable of that chunk becomes resident, and the loadable
/// then loads as a resident one does. A chunk the host has imported already
/// resolves the import at once, and one that fails to evaluate rejects it.
/// The runtime's `Loadable` and `light.lazy/load` stay as a single file has
/// them, so a program without chunks pays nothing for this. A chunk that
/// another chunk's static import evaluated is not recorded until a loadable
/// of it is loaded: chunks carry no load markers (docs/code-splitting.md,
/// "Open questions").
const LAZY: &str = "  const lazy$ = (() => {
    const modules = new Map(), waiting = new Map();
    const resident = (loadable, module) => {
      const getter = loadable.getter;
      loadable.getter = () => getter(module);
      loadable.importer = null;
    };
    return (chunk, loadable) => {
      if (modules.has(chunk)) {
        resident(loadable, modules.get(chunk));
        return loadable;
      }
      if (!waiting.has(chunk)) waiting.set(chunk, []);
      waiting.get(chunk).push(loadable);
      const importer = loadable.importer, load = loadable.load;
      return Object.defineProperty(loadable, \"load\", {configurable: true, writable: true, value: () => importer().then((module) => {
        if (!modules.has(chunk)) {
          modules.set(chunk, module);
          for (const waiter of waiting.get(chunk)) resident(waiter, module);
          waiting.delete(chunk);
        }
        return load.call(loadable);
      })});
    };
  })();
";

/// One chunk of a chunked build: its ES module, and the chunks it imports,
/// in the order of its import lines.
pub(crate) struct EmittedChunk {
    pub(crate) source: String,
    pub(crate) imports: Vec<ChunkId>,
    /// Its runtime at `Advanced`, for the size report.
    pub(crate) runtime: ChunkRuntime,
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

/// The chunks of `plan` for `program` with the decisions of `application`,
/// in plan order, with the plan they follow: `plan` and the chunks its
/// constants and runtime units add, which hold no form; see the module
/// documentation. `None` when the forms reach reflection at `Advanced` and
/// `plan` moves a definition out of its namespace's chunk or dropped a chunk
/// that writes no code without reflection: such a program is planned at
/// namespace granularity (docs/code-splitting.md, "Phase 3", the
/// reflection gate). `reflective`: the forms are known to reach reflection,
/// so they are written once, with the namespaces' registration.
pub(crate) fn emit_chunked(
    program: &Program,
    application: Application<'_>,
    plan: &ChunkPlan,
    reflective: bool,
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
    let setup = Setup {
        demand_roots: false,
        application_roots: false,
        module: true,
        mapped: false,
        application: Some(application),
        direct,
        function_names: &[],
    };
    // As in the single file, `Advanced` output registers its namespaces only
    // when the forms of all chunks reach reflection.
    let write = |register_namespaces: bool, brand_types: bool| {
        let mut emitter = Emitter::new(program, setup, register_namespaces);
        emitter.brand_types = brand_types;
        emitter.chunks = Some(Chunks {
            plan,
            current: STARTUP,
            loaded: loaded.clone(),
            constants: Vec::new(),
            initializer: None,
            runtime: vec![RuntimeRoots::default(); count],
            targets: Vec::new(),
            lazy: false,
        });
        let mut bodies = Vec::with_capacity(count);
        for chunk in 0..count {
            emitter.chunks.as_mut().unwrap().current = ChunkId(chunk);
            emitter.imports = Some(Vec::new());
            emitter.operand_scope(|emitter| {
                emitter.chunk_forms(program.root, ExprContext::Return, entry, &sources, &forms)
            });
            emitter.allow_js_local_assignment();
            bodies.push((
                std::mem::take(&mut emitter.output),
                std::mem::take(&mut emitter.raw_segments),
                emitter.imports.take().unwrap_or_default(),
            ));
        }
        emitter.record_application_roots(Some(application));
        (emitter, bodies)
    };
    let (mut emitter, mut bodies) = write(!slice || reflective, true);
    if slice && !reflective && emitter.runtime.reach_reflection() {
        if plan.moves_definitions() || plan.drops_chunks() {
            return None;
        }
        (emitter, bodies) = write(true, true);
    }
    // As in the single file, constructors brand their instances only where
    // the forms of all chunks may observe a brand.
    if slice && emitter.unobserved_brands() {
        (emitter, bodies) = write(emitter.register_namespaces, false);
    }
    let embed = if slice {
        !emitter.runtime.is_empty()
    } else {
        reads_runtime(program, &emitter.used)
    };
    debug_assert!(
        embed || emitter.runtime.is_empty(),
        "emitted code reads the runtime without embedding it"
    );
    // Startup registers the namespaces before any form runs, and declares
    // what every chunk reads of the runtime below `Advanced`.
    let registration = if embed {
        if let Some(chunks) = &mut emitter.chunks {
            chunks.write_startup();
        }
        let retained = |global: usize| !slice || application.globals[global];
        emitter.registration(None, true, retained)
    } else {
        String::new()
    };
    let mut startup_head = String::new();
    for (name, initial) in &emitter.core_cells {
        writeln!(startup_head, "  let core${name} = {initial};").unwrap();
    }
    let mut chunks = emitter.chunks.take().expect("a chunked build");
    // Each constant table entry goes to the chunk of its signature, and at
    // `Advanced` each runtime unit too; either may be a chunk the plan does
    // not have. Each chunk declares its entries in the order the table
    // declares them, the parts of a composite first.
    let signatures = constant_signatures(plan, &chunks.constants);
    let units = if slice && embed {
        unit_signatures(plan, &chunks.runtime, &chunks.constants, &signatures)
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
    for (chunk, names) in chunks.runtime.iter().enumerate() {
        chunk_roots[renumbered[chunk].0].extend(names);
    }
    let mut tables = vec![String::new(); count];
    if let Some(constants) = &emitter.constants {
        let mut order = (0..chunks.constants.len()).collect::<Vec<_>>();
        order.sort_unstable_by_key(|&entry| chunks.constants[entry].declaration.start);
        for entry in order {
            let declaration = chunks.constants[entry].declaration.clone();
            tables[owners[entry].0].push_str(&constants.declarations[declaration]);
            chunk_roots[owners[entry].0].extend(&chunks.constants[entry].runtime);
        }
    }
    #[cfg(debug_assertions)]
    for table in &tables {
        check_runtime_reads(table, &[], &emitter.runtime, &emitter.core_cells);
    }
    startup_head.push_str(&tables[STARTUP.0]);
    let mut placed = (0..count).map(|_| None).collect::<Vec<_>>();
    for (chunk, body) in bodies.into_iter().enumerate() {
        placed[renumbered[chunk].0] = Some(body);
    }
    for (chunk, _) in &mut chunks.targets {
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
    let mut read = vec![BTreeSet::new(); count];
    for (chunk, text) in written.iter().enumerate() {
        #[cfg(debug_assertions)]
        check_runtime_reads(&text.run, &text.raw, &emitter.runtime, &emitter.core_cells);
        let mut names = BTreeSet::new();
        for (source, skipped) in [(&text.head, &Vec::new()), (&text.run, &text.raw)] {
            for (range, word) in scanned_names(source, skipped) {
                // A member is no binding: `m.g20` reads a module namespace.
                if !source[..range.start].ends_with('.') {
                    names.insert(word.to_owned());
                }
            }
        }
        read[chunk] = names;
    }
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
    if embed && slice {
        let factories = crate::runtime_slice::chunk_factories(&slices, terse);
        for (chunk, (text, (factory, unit_bytes))) in written.iter_mut().zip(factories).enumerate()
        {
            let slice = &slices[chunk];
            if !slice.kept.contains(&true) {
                continue;
            }
            #[cfg(debug_assertions)]
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
            runtime.bytes = factory.len();
            runtime.unit_bytes = unit_bytes;
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
        if chunk == STARTUP.0 && chunks.lazy {
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
    for (chunk, name) in &chunks.targets {
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
        .flat_map(|text| {
            [
                Text {
                    source: &text.head,
                    raw: Vec::new(),
                    runtime: text.runtime.clone(),
                },
                Text {
                    source: &text.run,
                    raw: text.raw.clone(),
                    runtime: None,
                },
            ]
        })
        .collect::<Vec<_>>();
    let raw_js = emitter
        .used
        .iter()
        .any(|id| matches!(program.exprs[id.0].kind, ExprKind::JsTemplate { .. }));
    // Each compiler name's final spelling: a short name at `Advanced`, the
    // hygiene prefix and the name where raw text could capture it, else the
    // name.
    let (renamed, table, prefix) = if terse {
        let (renamed, names) = crate::terse::terse_texts(&texts);
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
        let renamed = texts
            .iter()
            .map(|text| {
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
            })
            .collect();
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
        .map(|(chunk, (text, runtime))| {
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
            }
        })
        .collect();
    Some((placed_plan, files))
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
}
