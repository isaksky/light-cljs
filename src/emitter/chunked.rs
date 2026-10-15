//! Chunked output (docs/code-splitting.md, "What a chunk file contains",
//! "Assignment across chunks" and "Phase 2"): one ES module per chunk of a
//! [`ChunkPlan`], at every level.
//!
//! One [`Emitter`] writes every chunk's forms in plan order, so the constant
//! table, the runtime roots and the `used` set are the program's, as in the
//! single file. Each chunk's forms run in a function of their own, as the
//! single file's do, and what other chunks may read is declared at module
//! level: startup's runtime (the whole runtime below `Advanced`, the
//! whole-program slice at `Advanced`), output-level core cells and constant
//! table, and each chunk's `let` declarations of the globals it owns. Hoisted
//! methods, which only their function's code reads, are declared in that
//! function's chunk.
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
use crate::chunks::{ChunkId, ChunkPlan, STARTUP};
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
    /// By constant table entry: the chunk declaring it.
    constants: Vec<ChunkId>,
    /// By chunk other than startup: the declarations of the hoisted methods
    /// of its functions ([`ConstantKey::Method`]).
    methods: Vec<String>,
    /// The globals the getter of a loadable reads from its target chunk's
    /// module namespace object, each with that chunk, which exports it.
    targets: Vec<(ChunkId, String)>,
    /// Whether a chunk has a loadable whose target may not have evaluated
    /// when the chunk runs, for which startup declares `lazy$`.
    lazy: bool,
}

impl Chunks<'_> {
    /// Record a new constant table entry: a hoisted method, declared in the
    /// chunk being written, or a constant, which stays in startup.
    pub(super) fn constant(&mut self, method: bool) {
        self.constants
            .push(if method { self.current } else { STARTUP });
    }

    /// Where the declaration of a hoisted method of the chunk being written
    /// goes, unless that chunk is startup, whose methods are entries of the
    /// constant table as in the single file.
    pub(super) fn method_declarations(&mut self) -> Option<&mut String> {
        (self.current != STARTUP).then(|| &mut self.methods[self.current.0])
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
/// ascending.
pub(crate) struct EmittedChunk {
    pub(crate) source: String,
    pub(crate) imports: Vec<ChunkId>,
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

/// The chunks of `plan` for `program` with the decisions of `application`,
/// in plan order; see the module documentation.
pub(crate) fn emit_chunked(
    program: &Program,
    application: Application<'_>,
    plan: &ChunkPlan,
) -> Vec<EmittedChunk> {
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
    let write = |register_namespaces: bool| {
        let mut emitter = Emitter::new(program, setup, register_namespaces);
        emitter.chunks = Some(Chunks {
            plan,
            current: STARTUP,
            loaded: loaded.clone(),
            constants: Vec::new(),
            methods: vec![String::new(); count],
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
            bodies.push((
                std::mem::take(&mut emitter.output),
                std::mem::take(&mut emitter.raw_segments),
                emitter.imports.take().unwrap_or_default(),
            ));
        }
        emitter.record_application_roots(Some(application));
        (emitter, bodies)
    };
    let (mut emitter, mut bodies) = write(!slice);
    if slice && emitter.runtime.reach_reflection() {
        (emitter, bodies) = write(true);
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
    // what every chunk reads of the runtime.
    let registration = if embed {
        let retained = |global: usize| !slice || application.globals[global];
        emitter.registration(None, true, retained)
    } else {
        String::new()
    };
    let mut startup_head = String::new();
    let mut runtime = None;
    if embed {
        let statement = if slice {
            let (factory, bindings, _) =
                crate::runtime_slice::sliced_runtime(&emitter.runtime, terse);
            let pattern = bindings
                .iter()
                .map(|binding| format!("rt${binding}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("  const [{pattern}] = {};\n", factory.trim())
        } else {
            format!(
                "  const rt = {};\n",
                crate::runtime_manifest::RUNTIME.trim()
            )
        };
        let factory = statement
            .find("(() =>")
            .or_else(|| statement.find("(()=>"))
            .unwrap_or_default();
        runtime = Some(factory..statement.len() - 2);
        startup_head.push_str(&statement);
        for (name, initial) in &emitter.core_cells {
            writeln!(startup_head, "  let core${name} = {initial};").unwrap();
        }
    }
    if let Some(constants) = &emitter.constants {
        startup_head.push_str(&constants.declarations);
    }
    let chunks = emitter.chunks.take().expect("a chunked build");
    let mut written = bodies
        .into_iter()
        .enumerate()
        .map(|(chunk, (forms, raw, packages))| {
            let (head, runtime) = if chunk == STARTUP.0 {
                (std::mem::take(&mut startup_head), runtime.clone())
            } else {
                (chunks.methods[chunk].clone(), None)
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
                runtime,
                run: format!("{open}{forms}}})()"),
                raw,
                packages,
            }
        })
        .collect::<Vec<_>>();

    // The names each chunk reads, and their owners.
    let owner = |name: &str| -> Option<ChunkId> {
        if name == "rt" || name == "lazy$" || name.starts_with("rt$") || name.starts_with("core$") {
            Some(STARTUP)
        } else if let Some(global) = owned_global(name) {
            Some(plan.global_chunks[global])
        } else {
            constant_entry(name).map(|entry| chunks.constants[entry])
        }
    };
    let mut read = vec![BTreeSet::new(); count];
    for (chunk, text) in written.iter_mut().enumerate() {
        #[cfg(debug_assertions)]
        {
            let declarations = if chunk == STARTUP.0 {
                emitter
                    .constants
                    .as_ref()
                    .map_or("", |constants| constants.declarations.as_str())
            } else {
                &chunks.methods[chunk]
            };
            check_runtime_reads(declarations, &[], &emitter.runtime, &emitter.core_cells);
            check_runtime_reads(&text.run, &text.raw, &emitter.runtime, &emitter.core_cells);
        }
        let mut names = BTreeSet::new();
        let skipped = text.runtime.iter().cloned().collect::<Vec<_>>();
        for (source, skipped) in [(&text.head, &skipped), (&text.run, &text.raw)] {
            for (range, word) in scanned_names(source, skipped) {
                // A member is no binding: `m.g20` reads a module namespace.
                if !source[..range.start].ends_with('.') {
                    names.insert(word.to_owned());
                }
            }
        }
        // The prelude helpers this chunk calls, declared first.
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
        read[chunk] = names;
    }
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
                debug_assert!(
                    owner == STARTUP || chunks.loaded[chunk][owner.0],
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
    written
        .into_iter()
        .enumerate()
        .map(|(chunk, text)| {
            let (head, run) = (renamed.next().unwrap(), renamed.next().unwrap());
            let mut source = package_imports(&text.packages);
            // The chunks it imports, in the order their namespaces load in
            // the single file.
            let mut imported = imports[chunk].iter().collect::<Vec<_>>();
            imported.sort_by_key(|(import, _)| plan.chunks[import.0].namespaces.first().copied());
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
            source.push_str(run.trim_end());
            source.push_str(";\n");
            EmittedChunk {
                source,
                imports: imports[chunk].keys().copied().collect(),
            }
        })
        .collect()
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
