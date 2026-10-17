//! Chunk placement (docs/code-splitting.md, "The model", "Phase 2" and
//! "Phase 3"): which top-level forms of a prepared program each chunk of a
//! chunked build holds, which chunks each imports, and which globals each
//! exports a setter for. [`plan`] is pure data;
//! [`PreparedApplication::emit_chunks`] writes one ES module per chunk of it
//! ([`ChunkedOutput`]).
//!
//! A *root* is a place code can start running: startup (the entry namespace,
//! root 0) and each distinct namespace a `light.lazy/loadable` names (root
//! `1 + i` for [`Program::lazy_namespaces`]`[i]`; a Var target roots its
//! namespace). The *static reach* of a root is the namespaces reachable from
//! its namespace over [`Program::namespace_requires`]. The *signature* of a
//! namespace is the set of roots whose reach contains it, a bitset dense by
//! root index ([`Signature`]). An *item* is a top-level form, an entry of the
//! flattened `program.root`. A *pinned* item has its namespace's signature.
//! At `Advanced` a *movable* item, a definition that removal could drop when
//! unread or a `defprotocol` or `deftype` form, has the signature of the
//! roots whose code reaches it: one walk per root over the item reference
//! graph (the globals each item reads, in function bodies too, to the item
//! defining them) from the pinned items of the root's reach. Below
//! `Advanced`, and for a program that reaches reflection
//! ([`namespace_plan`]), every item is pinned.
//!
//! A chunk is a distinct signature, except that every signature containing
//! startup is the startup chunk: startup loads first, so nothing it reaches
//! is deferred. Reach is monotone, so a chunk's imports, the chunks holding
//! the namespaces it requires and the globals its items read, have strictly
//! larger signatures, and the chunk graph is acyclic by construction (debug
//! builds check every edge). Chunk ids are ordered so that a chunk's imports
//! precede it: startup first, then larger signatures before smaller ones.
//! At `Advanced` a chunk that would write no code, holding no root, no
//! global another chunk reads and only items `Advanced` output writes
//! nothing for, is dropped: its importers import its imports, and startup
//! lists its namespaces ([`ChunkPlan::drops_chunks`]).
//!
//! The constant table's entries are items only emission finds, so the
//! emitter places them once it has written the chunks
//! ([`ChunkPlan::place`]): each in the chunk of the roots whose code reads
//! it, which the plan gains, holding no item, when it has no chunk of that
//! signature.

use crate::analyzer::{ExprId, ExprKind, GlobalId, LazyTargetId, Program};
use crate::application::PreparedApplication;
use crate::program_fragment::children;
use crate::source_map::json_string;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

/// The root index of startup, the entry namespace.
pub const STARTUP_ROOT: usize = 0;

/// The startup chunk, which every other chunk may import and which imports
/// none.
pub const STARTUP: ChunkId = ChunkId(0);

/// The longest file name a shared chunk is named by its roots' namespaces
/// joined with `+`; a longer one is `shared-<hash>.js`.
pub const MAX_NAME_LENGTH: usize = 80;

/// A chunk, an index into [`ChunkPlan::chunks`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ChunkId(pub usize);

/// A set of roots: a bitset of root indices, 64 to a word.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Signature(pub Vec<u64>);

impl Signature {
    /// The empty set over `roots` roots.
    pub fn empty(roots: usize) -> Self {
        Self(vec![0; roots.div_ceil(64)])
    }
    /// Every one of `roots` roots.
    pub fn full(roots: usize) -> Self {
        let mut signature = Self::empty(roots);
        for root in 0..roots {
            signature.insert(root);
        }
        signature
    }
    pub fn insert(&mut self, root: usize) {
        self.0[root / 64] |= 1 << (root % 64);
    }
    pub fn contains(&self, root: usize) -> bool {
        self.0
            .get(root / 64)
            .is_some_and(|word| word & (1 << (root % 64)) != 0)
    }
    /// How many roots the set holds.
    pub fn len(&self) -> usize {
        self.0.iter().map(|word| word.count_ones() as usize).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&word| word == 0)
    }
    /// Add the roots of `other`, a set over as many roots.
    pub fn union(&mut self, other: &Self) {
        debug_assert_eq!(self.0.len(), other.0.len(), "signatures of one plan");
        for (word, other) in self.0.iter_mut().zip(&other.0) {
            *word |= other;
        }
    }
    /// Whether every root of `other` is in this set.
    pub fn is_superset(&self, other: &Self) -> bool {
        other
            .0
            .iter()
            .enumerate()
            .all(|(index, word)| word & !self.0.get(index).copied().unwrap_or(0) == 0)
    }
    /// The root indices of the set, ascending.
    pub fn roots(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(index, &word)| {
            (0..64)
                .filter(move |bit| word & (1 << bit) != 0)
                .map(move |bit| index * 64 + bit)
        })
    }
}

/// A root of the plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Root {
    /// The source index (the position in [`Program::namespaces`]) of the
    /// root's namespace.
    pub namespace: usize,
    /// The chunk holding that namespace: what a loadable of the root loads.
    pub chunk: ChunkId,
}

/// A top-level form of the prepared program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub expr: ExprId,
    /// The source index of the namespace whose body holds the form. A form
    /// outside every body (the read of the entry's value that ends a program
    /// with lazily loaded sources) belongs to the entry namespace.
    pub namespace: usize,
    /// The chunk of the item's signature: its namespace's chunk when the
    /// item is pinned.
    pub chunk: ChunkId,
    /// Whether the item is a definition placed by its readers rather than
    /// pinned to its namespace (docs/code-splitting.md, "Items and
    /// signatures"); only at `Advanced`. It may still land in its
    /// namespace's chunk.
    pub movable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chunk {
    /// The file name: `<entry>.js` for startup, `<ns>.js` for one lazy root,
    /// the roots' namespaces sorted and joined with `+` for several, and
    /// `shared-<hash>.js` when that would exceed [`MAX_NAME_LENGTH`]; a
    /// shared name another chunk holds takes a `-<n>` suffix.
    pub name: String,
    /// The roots whose code reaches the chunk's items. Startup's is every
    /// root, since every root loads after it.
    pub signature: Signature,
    /// The source indices of the namespaces the chunk holds, those of its
    /// signature, whose pinned items it holds, ascending. A chunk of moved
    /// definitions alone holds none. At `Advanced` startup also holds the
    /// namespaces of the chunks the plan dropped since they would write no
    /// code ([`ChunkPlan::drops_chunks`]).
    pub namespaces: Vec<usize>,
    /// The roots whose namespace the chunk holds, ascending.
    pub roots: Vec<usize>,
    /// The chunk's items, indices into [`ChunkPlan::items`], ascending: the
    /// order of `program.root`, so namespaces initialize in source order and
    /// a definition precedes the items that read it while the chunk loads.
    pub items: Vec<usize>,
    /// The chunks this chunk imports, ascending: each has a smaller id and a
    /// signature strictly containing this chunk's.
    pub imports: Vec<ChunkId>,
    /// The globals this chunk owns that another chunk assigns (`set!`,
    /// `binding`), ascending: the owner exports a setter for each.
    pub setters: Vec<GlobalId>,
    /// The loadable sites among the items whose target the chunk holds, in
    /// program order, each target once per namespace holding it.
    pub sites: Vec<Site>,
}

/// A `light.lazy/loadable` site in an item of the plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Site {
    pub target: LazyTargetId,
    /// The source index of the namespace of the item holding the site.
    pub namespace: usize,
}

/// Where each top-level form of a chunked build goes; see the module
/// documentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkPlan {
    /// By root index: root 0 is startup, root `1 + i` is
    /// [`Program::lazy_namespaces`]`[i]`.
    pub roots: Vec<Root>,
    /// By source index: the roots whose static reach contains the namespace.
    pub namespace_signatures: Vec<Signature>,
    /// By source index: the chunk holding the namespace.
    pub namespace_chunks: Vec<ChunkId>,
    /// The entries of the flattened `program.root` of the prepared program,
    /// in order.
    pub items: Vec<Item>,
    /// By [`GlobalId`]: the chunk owning the global, which declares it: the
    /// chunk of its definition when that is movable, else its namespace's.
    pub global_chunks: Vec<ChunkId>,
    /// By [`ChunkId`]; startup is [`STARTUP`].
    pub chunks: Vec<Chunk>,
}

/// The files of a chunked build, one ES module per chunk in plan order
/// ([`PreparedApplication::emit_chunks`]): startup first, whose default
/// export is the program's value. The library writes no file; the CLI's
/// `--out-dir` writes each file and [`ChunkedOutput::manifest`] as
/// `chunks.json` into one directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkedOutput {
    pub files: Vec<ChunkFile>,
}

/// One chunk's ES module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkFile {
    /// The file name ([`Chunk::name`]), which importers and loaders name as
    /// `./<name>`, so the files go in one directory.
    pub name: String,
    pub source: String,
    /// The names of the files it imports statically, in the order of its
    /// import lines: every chunk holding a namespace its namespaces require
    /// or a binding its code reads.
    pub imports: Vec<String>,
    /// The namespaces whose top-level forms it holds, in source order.
    pub namespaces: Vec<String>,
    /// The definitions it holds of namespaces another chunk holds, moved
    /// toward their readers at `Advanced`, in program order: each by the
    /// first global it defines, `app.shared/fmt`.
    pub definitions: Vec<String>,
    /// The namespaces of the roots it holds: the entry for startup, and the
    /// namespaces whose loadables load it.
    pub roots: Vec<String>,
    /// The `light.lazy/loadable` sites of the output whose target it holds,
    /// in output order, each distinct pair of site namespace and target
    /// once.
    pub loadables: Vec<LoadableSite>,
}

/// A `light.lazy/loadable` site of a chunked build's output: the namespace
/// whose code holds it, and its target as written, `app.editor/start` for a
/// Var or `app.editor` for a namespace.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LoadableSite {
    pub namespace: String,
    pub target: String,
}

impl ChunkedOutput {
    pub(crate) fn new(
        program: &Program,
        plan: &ChunkPlan,
        emitted: Vec<crate::emitter::EmittedChunk>,
    ) -> Self {
        let namespace = |source: usize| program.namespaces[source].clone();
        let target = |target: LazyTargetId| {
            let target = &program.lazy_targets[target.0];
            match target.global {
                Some(global) => {
                    let global = &program.globals[global.0];
                    format!("{}/{}", global.namespace, global.name)
                }
                None => program.lazy_namespaces[target.namespace].clone(),
            }
        };
        let definition = |item: &Item| {
            let kind = &program.exprs[item.expr.0].kind;
            let global = crate::application::globals(kind)[0];
            let global = &program.globals[global.0];
            format!("{}/{}", global.namespace, global.name)
        };
        let files = plan
            .chunks
            .iter()
            .enumerate()
            .zip(emitted)
            .map(|((id, chunk), emitted)| ChunkFile {
                name: chunk.name.clone(),
                source: emitted.source,
                imports: emitted
                    .imports
                    .iter()
                    .map(|import| plan.chunks[import.0].name.clone())
                    .collect(),
                namespaces: chunk
                    .namespaces
                    .iter()
                    .map(|&source| namespace(source))
                    .collect(),
                definitions: plan.moved_items(ChunkId(id)).map(definition).collect(),
                roots: chunk
                    .roots
                    .iter()
                    .map(|&root| namespace(plan.roots[root].namespace))
                    .collect(),
                loadables: {
                    let mut listed = HashSet::new();
                    chunk
                        .sites
                        .iter()
                        .map(|site| LoadableSite {
                            namespace: namespace(site.namespace),
                            target: target(site.target),
                        })
                        .filter(|site| listed.insert(site.clone()))
                        .collect()
                },
            })
            .collect();
        Self { files }
    }

    /// The startup file, whose default export is the program's value.
    pub fn startup(&self) -> &ChunkFile {
        &self.files[0]
    }

    /// `chunks.json`, the manifest of the files (docs/compiler.md, "Chunked
    /// builds"): the startup file's name and, per file in plan order, one
    /// line with its chunk's name (the file name without `.js`), the file,
    /// the files it imports, its namespaces, its roots' namespaces, the
    /// loadable sites that resolve to it and its size in bytes.
    pub fn manifest(&self) -> String {
        let list = |out: &mut String, items: &[String]| {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                json_string(out, item);
            }
            out.push(']');
        };
        let mut manifest = String::from("{\n  \"startup\": ");
        json_string(&mut manifest, &self.startup().name);
        manifest.push_str(",\n  \"chunks\": [");
        for (index, file) in self.files.iter().enumerate() {
            manifest.push_str(if index == 0 { "\n" } else { ",\n" });
            manifest.push_str("    {\"name\": ");
            let name = file.name.strip_suffix(".js").unwrap_or(&file.name);
            json_string(&mut manifest, name);
            manifest.push_str(", \"file\": ");
            json_string(&mut manifest, &file.name);
            manifest.push_str(", \"imports\": ");
            list(&mut manifest, &file.imports);
            manifest.push_str(", \"namespaces\": ");
            list(&mut manifest, &file.namespaces);
            manifest.push_str(", \"definitions\": ");
            list(&mut manifest, &file.definitions);
            manifest.push_str(", \"roots\": ");
            list(&mut manifest, &file.roots);
            manifest.push_str(", \"loadables\": [");
            for (index, site) in file.loadables.iter().enumerate() {
                if index > 0 {
                    manifest.push_str(", ");
                }
                manifest.push_str("{\"namespace\": ");
                json_string(&mut manifest, &site.namespace);
                manifest.push_str(", \"target\": ");
                json_string(&mut manifest, &site.target);
                manifest.push('}');
            }
            write!(manifest, "], \"bytes\": {}}}", file.source.len()).unwrap();
        }
        manifest.push_str("\n  ]\n}\n");
        manifest
    }
}

impl ChunkPlan {
    /// The chunk a loadable of `target` loads.
    pub fn target_chunk(&self, program: &Program, target: LazyTargetId) -> ChunkId {
        self.roots[1 + program.lazy_targets[target.0].namespace].chunk
    }

    /// Whether an item lies outside its namespace's chunk.
    pub fn moves_definitions(&self) -> bool {
        self.items
            .iter()
            .any(|item| item.chunk != self.namespace_chunks[item.namespace])
    }

    /// Whether the plan dropped a chunk that would write no code at
    /// `Advanced`: startup then holds a namespace that only lazy roots
    /// reach.
    pub fn drops_chunks(&self) -> bool {
        self.chunks[STARTUP.0].namespaces.iter().any(|&namespace| {
            let signature = &self.namespace_signatures[namespace];
            !signature.is_empty() && !signature.contains(STARTUP_ROOT)
        })
    }

    /// The items of chunk `chunk` from namespaces another chunk holds, in
    /// program order.
    pub fn moved_items(&self, chunk: ChunkId) -> impl Iterator<Item = &Item> + '_ {
        self.chunks[chunk.0]
            .items
            .iter()
            .map(|&item| &self.items[item])
            .filter(move |item| self.namespace_chunks[item.namespace] != chunk)
    }

    /// Place what only the emitter finds, the constant table's entries
    /// (docs/code-splitting.md, "Phase 3"), by their signatures, each the
    /// roots whose code reads one: a signature holding startup's root, or
    /// none, is startup's; any other is its chunk's, which is added, holding
    /// no item, when the plan has no chunk of that signature. Added chunks
    /// are named as the plan names chunks, and ordered as it orders them,
    /// after the chunks of as many roots, so chunk ids change. Returns the
    /// chunk of each of `signatures`, and by chunk id before the call, its id
    /// after it.
    pub fn place(
        &mut self,
        program: &Program,
        signatures: &[Signature],
    ) -> (Vec<ChunkId>, Vec<ChunkId>) {
        let startup =
            |signature: &Signature| signature.contains(STARTUP_ROOT) || signature.is_empty();
        let before = self.chunks.len();
        let mut chunk_of_signature = self
            .chunks
            .iter()
            .enumerate()
            .skip(1)
            .map(|(chunk, members)| (members.signature.clone(), chunk))
            .collect::<HashMap<_, _>>();
        let mut taken = self
            .chunks
            .iter()
            .map(|chunk| chunk.name.clone())
            .collect::<HashSet<_>>();
        for signature in signatures {
            if startup(signature) || chunk_of_signature.contains_key(signature) {
                continue;
            }
            chunk_of_signature.insert(signature.clone(), self.chunks.len());
            let roots = signature
                .roots()
                .map(|root| program.namespaces[self.roots[root].namespace].as_str())
                .collect();
            let name = chunk_name(roots);
            let stem = name.strip_suffix(".js").unwrap_or_default().to_owned();
            let (mut name, mut suffix) = (name, 1);
            while !taken.insert(name.clone()) {
                suffix += 1;
                name = format!("{stem}-{suffix}.js");
            }
            self.chunks.push(Chunk {
                name,
                signature: signature.clone(),
                namespaces: Vec::new(),
                roots: Vec::new(),
                items: Vec::new(),
                imports: Vec::new(),
                setters: Vec::new(),
                sites: Vec::new(),
            });
        }
        let mut renumbered = (0..self.chunks.len()).map(ChunkId).collect::<Vec<_>>();
        if self.chunks.len() > before {
            // Larger signatures first, so every import still precedes its
            // importer; the sort is stable, so the plan's chunks keep their
            // order and an added chunk follows those of its size.
            let mut order = (1..self.chunks.len()).collect::<Vec<_>>();
            order.sort_by_key(|&chunk| std::cmp::Reverse(self.chunks[chunk].signature.len()));
            for (position, &chunk) in order.iter().enumerate() {
                renumbered[chunk] = ChunkId(1 + position);
            }
            let mut chunks = std::mem::take(&mut self.chunks)
                .into_iter()
                .map(Some)
                .collect::<Vec<_>>();
            self.chunks = std::iter::once(STARTUP.0)
                .chain(order)
                .map(|chunk| chunks[chunk].take().expect("each chunk once"))
                .collect();
            let renumber = |chunk: &mut ChunkId| *chunk = renumbered[chunk.0];
            for chunk in &mut self.chunks {
                chunk.imports.iter_mut().for_each(renumber);
            }
            for root in &mut self.roots {
                renumber(&mut root.chunk);
            }
            self.namespace_chunks.iter_mut().for_each(renumber);
            for item in &mut self.items {
                renumber(&mut item.chunk);
            }
            self.global_chunks.iter_mut().for_each(renumber);
        }
        let placed = signatures
            .iter()
            .map(|signature| {
                if startup(signature) {
                    STARTUP
                } else {
                    renumbered[chunk_of_signature[signature]]
                }
            })
            .collect();
        renumbered.truncate(before);
        (placed, renumbered)
    }
}

/// Plan the chunks of `prepared`, the application of `program`. At
/// `Advanced` the definitions that removal could drop when unread
/// ([`PreparedApplication::removable`]), and `defprotocol` and `deftype`
/// forms, move toward their readers (docs/code-splitting.md, "Phase 3");
/// below it every top-level form stays in its namespace's chunk, as in
/// [`namespace_plan`]. At `Advanced` it also drops the chunks that would
/// write no code ([`ChunkPlan::drops_chunks`]). Whether a program reaches
/// reflection is known only once its forms are written, so this plan does
/// not ask: [`PreparedApplication::emit_chunks`] falls back to
/// [`namespace_plan`] for a program that does. A program that was not
/// analyzed from namespace sources has one namespace and one chunk.
pub fn plan(program: &Program, prepared: &PreparedApplication) -> ChunkPlan {
    plan_with(program, prepared, prepared.level().advanced())
}

/// The plan of `prepared` with every top-level form pinned to its
/// namespace's chunk, in source order (docs/code-splitting.md, "Phase 2"):
/// the plan below `Advanced`, and at `Advanced` the plan of a program that
/// reaches reflection, so that a namespace it can look up is never split
/// across chunks.
pub fn namespace_plan(program: &Program, prepared: &PreparedApplication) -> ChunkPlan {
    plan_with(program, prepared, false)
}

/// What one walk over each item's expressions finds: the item reference
/// graph of the plan.
struct ItemGraph {
    /// By item: the globals its expressions read, in function bodies too:
    /// each global reference, and the dispatcher of each program protocol
    /// method that a method of a type, `reify` or extension names by key,
    /// which the emitter may write as the dispatcher's binding.
    reads: Vec<Vec<GlobalId>>,
    /// By item: the globals its expressions define with a value.
    defines: Vec<Vec<GlobalId>>,
    /// By global: the items defining it with a value, ascending.
    definers: Vec<Vec<usize>>,
    /// By item: the globals its expressions assign (`set!`, `binding`).
    assigns: Vec<Vec<GlobalId>>,
    /// By item: its `light.lazy/loadable` sites, in walk order.
    sites: Vec<Vec<(ExprId, LazyTargetId)>>,
}

impl ItemGraph {
    /// Walk `items`, the top-level forms of `program`, once each.
    fn new(program: &Program, items: &[(ExprId, usize)]) -> Self {
        use crate::analyzer::MethodDispatch;
        let dispatchers = program
            .protocols
            .values()
            .flatten()
            .map(|method| (method.key.as_str(), method.global))
            .collect::<HashMap<_, _>>();
        let count = items.len();
        let mut graph = Self {
            reads: vec![Vec::new(); count],
            defines: vec![Vec::new(); count],
            definers: vec![Vec::new(); program.globals.len()],
            assigns: vec![Vec::new(); count],
            sites: vec![Vec::new(); count],
        };
        let mut walked = vec![usize::MAX; program.exprs.len()];
        let mut pending = Vec::new();
        for (position, &(expr, _)) in items.iter().enumerate() {
            let (reads, defines) = (&mut graph.reads[position], &mut graph.defines[position]);
            pending.push(expr);
            while let Some(id) = pending.pop() {
                if std::mem::replace(&mut walked[id.0], position) == position {
                    continue;
                }
                let kind = &program.exprs[id.0].kind;
                let methods = match kind {
                    ExprKind::Global(global) => {
                        reads.push(*global);
                        &[][..]
                    }
                    ExprKind::Def { init: None, .. } => &[],
                    ExprKind::Def { .. } => {
                        defines.extend(crate::application::globals(kind));
                        &[]
                    }
                    ExprKind::DefProtocol { .. } => {
                        defines.extend(crate::application::globals(kind));
                        &[]
                    }
                    ExprKind::DefType { methods, .. } => {
                        defines.extend(crate::application::globals(kind));
                        methods.as_slice()
                    }
                    ExprKind::ExtendType { methods, .. } | ExprKind::Reify { methods, .. } => {
                        methods.as_slice()
                    }
                    ExprKind::SetGlobal { target, .. } => {
                        graph.assigns[position].extend(global_of(program, *target));
                        &[]
                    }
                    ExprKind::Binding { bindings, .. } => {
                        for (target, _) in bindings {
                            graph.assigns[position].extend(global_of(program, *target));
                        }
                        &[]
                    }
                    ExprKind::Loadable { target, .. } => {
                        graph.sites[position].push((id, *target));
                        &[]
                    }
                    _ => &[],
                };
                for (dispatch, _) in methods {
                    if let MethodDispatch::Protocol(key) = dispatch
                        && let Some(&dispatcher) = dispatchers.get(key.as_str())
                    {
                        reads.push(dispatcher);
                    }
                }
                pending.extend(children(program, id));
            }
            reads.sort_unstable_by_key(|global| global.0);
            reads.dedup();
            defines.sort_unstable_by_key(|global| global.0);
            defines.dedup();
            for global in defines.iter() {
                graph.definers[global.0].push(position);
            }
        }
        graph
    }
}

/// The global `target`, the assigned place of a `set!` or `binding`, names,
/// if it is one.
fn global_of(program: &Program, target: ExprId) -> Option<GlobalId> {
    match program.exprs[target.0].kind {
        ExprKind::Global(global) => Some(global),
        _ => None,
    }
}

fn plan_with(
    program: &Program,
    prepared: &PreparedApplication,
    move_definitions: bool,
) -> ChunkPlan {
    let emitted = prepared.program();
    debug_assert_eq!(emitted.namespaces, program.namespaces);
    let names = &program.namespaces;
    let sources = names.len();
    assert!(sources > 0, "an analyzed program has a namespace");
    let index = names
        .iter()
        .enumerate()
        .map(|(source, name)| (name.as_str(), source))
        .collect::<HashMap<_, _>>();
    let requires = |source: usize| {
        program
            .namespace_requires
            .get(source)
            .map_or(&[][..], Vec::as_slice)
    };
    // The entry is the last source the entry's static closure loads; the
    // sources reached only through lazy references follow it.
    let entry = program
        .namespace_lazy_only
        .iter()
        .rposition(|lazy| !lazy)
        .unwrap_or(sources - 1);

    // Roots and their reach.
    let mut root_namespaces = vec![entry];
    root_namespaces.extend(program.lazy_namespaces.iter().map(|name| {
        *index
            .get(name.as_str())
            .expect("a lazy target namespace is a source")
    }));
    let root_count = root_namespaces.len();
    let mut namespace_signatures = vec![Signature::empty(root_count); sources];
    let mut visited = vec![usize::MAX; sources];
    let mut pending = Vec::new();
    for (root, &namespace) in root_namespaces.iter().enumerate() {
        visited[namespace] = root;
        pending.push(namespace);
        while let Some(source) = pending.pop() {
            namespace_signatures[source].insert(root);
            for &required in requires(source) {
                if visited[required] != root {
                    visited[required] = root;
                    pending.push(required);
                }
            }
        }
    }

    // The items, each with its namespace, in program order.
    let mut forms = Vec::new();
    let mut pending = vec![(emitted.root, entry)];
    while let Some((id, namespace)) = pending.pop() {
        let namespace = emitted
            .namespace_entries
            .get(&id)
            .map_or(namespace, |name| index[name.as_str()]);
        if let ExprKind::Do(body) = &emitted.exprs[id.0].kind {
            pending.extend(body.iter().rev().map(|&form| (form, namespace)));
        } else {
            forms.push((id, namespace));
        }
    }
    let graph = ItemGraph::new(emitted, &forms);

    // Which items move: at `Advanced`, the definitions removal could drop
    // when unread, and protocol and type definitions, each the only
    // definition of the globals it defines. A raw `deftype*` or `defrecord*`
    // runs its body when it defines the type, so it stays. A lazy target's
    // definition stays in its namespace, which is what its loadable loads,
    // and so does a form holding a loadable site, which the manifest gives to
    // its namespace.
    let mut targets = vec![false; emitted.globals.len()];
    for target in &emitted.lazy_targets {
        if let Some(global) = target.global {
            targets[global.0] = true;
        }
    }
    let mut movable = forms
        .iter()
        .enumerate()
        .map(|(position, &(id, _))| {
            move_definitions
                && (prepared.removable(id)
                    || matches!(emitted.exprs[id.0].kind, ExprKind::DefProtocol { .. })
                    || matches!(
                        &emitted.exprs[id.0].kind,
                        ExprKind::DefType { layout, .. } if layout.body.is_none()
                    ))
                && graph.sites[position].is_empty()
                && !graph.defines[position].is_empty()
                && graph.defines[position]
                    .iter()
                    .all(|global| graph.definers[global.0] == [position] && !targets[global.0])
        })
        .collect::<Vec<_>>();

    // Item signatures: a pinned item has its namespace's; a movable one the
    // roots whose walk reaches it, one walk per root from the pinned items
    // of the root's reach (startup's walk also from those of namespaces no
    // root reaches, which startup holds) over reads of globals to their
    // definitions. A movable item that no walk reaches, one only an explicit
    // export keeps, is pinned, and the walks run again.
    let mut signatures;
    let mut walk = Vec::new();
    loop {
        signatures = forms
            .iter()
            .zip(&movable)
            .map(|(&(_, namespace), &movable)| {
                if movable {
                    Signature::empty(root_count)
                } else {
                    namespace_signatures[namespace].clone()
                }
            })
            .collect::<Vec<_>>();
        for root in 0..root_count {
            seeds(&forms, &movable, &namespace_signatures, root, &mut walk);
            while let Some(item) = walk.pop() {
                for global in &graph.reads[item] {
                    if let [definer] = graph.definers[global.0][..]
                        && movable[definer]
                        && !signatures[definer].contains(root)
                    {
                        signatures[definer].insert(root);
                        walk.push(definer);
                    }
                }
            }
        }
        let mut unreached = false;
        for (movable, signature) in movable.iter_mut().zip(&signatures) {
            if *movable && signature.is_empty() {
                *movable = false;
                unreached = true;
            }
        }
        if !unreached {
            break;
        }
    }

    // One chunk per distinct signature without startup, of a namespace or of
    // a movable item; a namespace no root reaches (none of a program the
    // loader built) and its items stay in startup, where the single file
    // runs them.
    let startup = |signature: &Signature| signature.contains(STARTUP_ROOT) || signature.is_empty();
    let mut groups: Vec<(Signature, Vec<usize>)> = Vec::new();
    let mut group_of = HashMap::new();
    let mut startup_namespaces = Vec::new();
    let mut group = |signature: &Signature, groups: &mut Vec<(Signature, Vec<usize>)>| {
        *group_of.entry(signature.clone()).or_insert_with(|| {
            groups.push((signature.clone(), Vec::new()));
            groups.len() - 1
        })
    };
    for (source, signature) in namespace_signatures.iter().enumerate() {
        if startup(signature) {
            startup_namespaces.push(source);
        } else {
            let group = group(signature, &mut groups);
            groups[group].1.push(source);
        }
    }
    for (signature, _) in signatures.iter().zip(&movable).filter(|(_, m)| **m) {
        if !startup(signature) {
            group(signature, &mut groups);
        }
    }
    // Larger signatures first, so that every import precedes its importer;
    // equal sizes keep the order of their first namespace, then of their
    // first item.
    groups.sort_by_key(|(signature, _)| std::cmp::Reverse(signature.len()));
    let chunk_of_signature = groups
        .iter()
        .enumerate()
        .map(|(group, (signature, _))| (signature.clone(), ChunkId(1 + group)))
        .collect::<HashMap<_, _>>();
    let chunk_of = |signature: &Signature| {
        if startup(signature) {
            STARTUP
        } else {
            chunk_of_signature[signature]
        }
    };
    let mut chunks = vec![Chunk {
        name: format!("{}.js", names[entry]),
        signature: Signature::full(root_count),
        namespaces: startup_namespaces,
        roots: Vec::new(),
        items: Vec::new(),
        imports: Vec::new(),
        setters: Vec::new(),
        sites: Vec::new(),
    }];
    chunks.extend(groups.into_iter().map(|(signature, namespaces)| {
        let roots = signature
            .roots()
            .map(|root| names[root_namespaces[root]].as_str())
            .collect::<Vec<_>>();
        Chunk {
            name: chunk_name(roots),
            signature,
            namespaces,
            roots: Vec::new(),
            items: Vec::new(),
            imports: Vec::new(),
            setters: Vec::new(),
            sites: Vec::new(),
        }
    }));
    // Startup and each lazy root are named after their namespace, so their
    // names are distinct. A namespace may be spelled `shared-<hash>`,
    // though, and two hashes may agree: a shared chunk whose name is taken
    // gets the first free `-<n>` suffix.
    let mut taken = chunks
        .iter()
        .enumerate()
        .filter(|(id, chunk)| *id == STARTUP.0 || chunk.signature.len() == 1)
        .map(|(_, chunk)| chunk.name.clone())
        .collect::<HashSet<_>>();
    for chunk in chunks.iter_mut().skip(1) {
        if chunk.signature.len() > 1 {
            let stem = chunk
                .name
                .strip_suffix(".js")
                .unwrap_or_default()
                .to_owned();
            let mut suffix = 1;
            while !taken.insert(chunk.name.clone()) {
                suffix += 1;
                chunk.name = format!("{stem}-{suffix}.js");
            }
        }
    }
    let mut namespace_chunks = vec![STARTUP; sources];
    for (chunk, members) in chunks.iter().enumerate() {
        for &source in &members.namespaces {
            namespace_chunks[source] = ChunkId(chunk);
        }
    }
    let roots = root_namespaces
        .iter()
        .enumerate()
        .map(|(root, &namespace)| {
            let chunk = namespace_chunks[namespace];
            chunks[chunk.0].roots.push(root);
            Root { namespace, chunk }
        })
        .collect::<Vec<_>>();

    // Each item goes to the chunk of its signature, in program order: a
    // definition precedes every item that reads it while the program loads,
    // so the single file's order restricted to a chunk is one in which the
    // chunk's items can run.
    let items = forms
        .iter()
        .zip(&movable)
        .zip(&signatures)
        .enumerate()
        .map(|(position, ((&(expr, namespace), &movable), signature))| {
            let chunk = chunk_of(signature);
            debug_assert!(movable || chunk == namespace_chunks[namespace]);
            chunks[chunk.0].items.push(position);
            Item {
                expr,
                namespace,
                chunk,
                movable,
            }
        })
        .collect::<Vec<_>>();

    // A global belongs to the chunk of its definition when that moves, else
    // to its namespace's chunk; a global of a namespace without a source
    // stays with the runtime in startup.
    let mut global_chunks = emitted
        .globals
        .iter()
        .map(|global| {
            index
                .get(global.namespace.as_str())
                .map_or(STARTUP, |&source| namespace_chunks[source])
        })
        .collect::<Vec<_>>();
    for (position, item) in items.iter().enumerate() {
        if item.movable {
            for global in &graph.defines[position] {
                global_chunks[global.0] = item.chunk;
            }
        }
    }

    // Edges: every require, and every read of a global across chunks, in
    // function bodies too, since a function may run as soon as its chunk has
    // loaded. A loadable's target is no child of its site and no edge.
    let mut imports = vec![Vec::new(); chunks.len()];
    let mut setters = vec![Vec::new(); chunks.len()];
    let mut edge = |from: ChunkId, to: ChunkId| {
        if from != to {
            debug_assert!(from != STARTUP, "startup imports {}", chunks[to.0].name);
            debug_assert!(
                to < from
                    && chunks[to.0].signature != chunks[from.0].signature
                    && chunks[to.0]
                        .signature
                        .is_superset(&chunks[from.0].signature),
                "{} imports {}, whose signature does not strictly contain its own",
                chunks[from.0].name,
                chunks[to.0].name
            );
            imports[from.0].push(to);
        }
    };
    for source in 0..sources {
        for &required in requires(source) {
            edge(namespace_chunks[source], namespace_chunks[required]);
        }
    }
    // By chunk: the loadable sites resolving to it, keyed for program order;
    // each target once per namespace (a site copied by inlining is one).
    let mut sites = vec![Vec::new(); chunks.len()];
    let mut listed = HashSet::new();
    for (position, item) in items.iter().enumerate() {
        for global in &graph.reads[position] {
            let binding = &emitted.globals[global.0];
            // Every root reaching the reader reaches what it reads: its
            // namespace, and the item defining it.
            debug_assert!(
                index.get(binding.namespace.as_str()).is_none_or(|&source| {
                    namespace_signatures[source].is_superset(&namespace_signatures[item.namespace])
                }),
                "{} reads {}/{} outside its reach",
                names[item.namespace],
                binding.namespace,
                binding.name
            );
            debug_assert!(
                match graph.definers[global.0][..] {
                    [definer] if items[definer].movable => {
                        signatures[definer].is_superset(&signatures[position])
                    }
                    _ => true,
                },
                "{} reads {}/{}, which fewer roots reach",
                names[item.namespace],
                binding.namespace,
                binding.name
            );
            edge(item.chunk, global_chunks[global.0]);
        }
        // An assignment of another chunk's global goes through the owner's
        // setter; the owner's own assignments stay direct.
        for global in &graph.assigns[position] {
            let owner = global_chunks[global.0];
            if owner != item.chunk {
                setters[owner.0].push(*global);
            }
        }
        // A site resolves to the chunk of its target's root.
        for &(id, target) in &graph.sites[position] {
            if listed.insert((target.0, item.namespace)) {
                let root = 1 + emitted.lazy_targets[target.0].namespace;
                sites[roots[root].chunk.0].push((
                    (position, id.0),
                    Site {
                        target,
                        namespace: item.namespace,
                    },
                ));
            }
        }
    }
    let chunk_data = imports.into_iter().zip(setters).zip(sites);
    for (chunk, ((mut imports, mut setters), mut sites)) in chunks.iter_mut().zip(chunk_data) {
        imports.sort_unstable();
        imports.dedup();
        setters.sort_unstable_by_key(|global: &GlobalId| global.0);
        setters.dedup();
        // Program order: by item, then by expression within it.
        sites.sort_unstable_by_key(|&(order, _)| order);
        chunk.imports = imports;
        chunk.setters = setters;
        chunk.sites = sites.into_iter().map(|(_, site)| site).collect();
    }
    let mut plan = ChunkPlan {
        roots,
        namespace_signatures,
        namespace_chunks,
        items,
        global_chunks,
        chunks,
    };
    if move_definitions {
        plan.drop_silent_chunks(emitted, &graph);
    }
    plan
}

/// Whether `Advanced` output writes nothing for `item`, a top-level form
/// that is not the program's value, in output that registers no namespace:
/// a `nil`, such as a definition removal dropped, or a definition without a
/// value (`declare`), as `Emitter::emit_expr` skips them.
fn silent(program: &Program, item: &Item) -> bool {
    matches!(
        program.exprs[item.expr.0].kind,
        ExprKind::Nil
            | ExprKind::Def {
                init: None,
                test: None,
                once: false,
                ..
            }
    )
}

impl ChunkPlan {
    /// Drop the chunks that would write no code at `Advanced`
    /// (docs/code-splitting.md, "Phase 3", light-cljs-299): a chunk other
    /// than startup that holds no root, owns no global another chunk's code
    /// reads or assigns, and whose items are all [`silent`], because removal
    /// dropped or inlining consumed its namespaces' definitions or they
    /// moved toward their readers. Such a chunk would be `(()=>{})();`
    /// with import lines, a request and a module for nothing. Its importers
    /// import its imports instead, each of a signature strictly containing
    /// its own and so theirs, and startup holds its namespaces and their
    /// items: startup is the chunk every root reaching them loads, and
    /// first, so a namespace's chunk still has a signature containing the
    /// namespace's, from which its moved definitions moved down. The
    /// emitter writes this plan only for forms that reach no reflection,
    /// which would register the namespaces ([`ChunkPlan::drops_chunks`]).
    fn drop_silent_chunks(&mut self, program: &Program, graph: &ItemGraph) {
        let count = self.chunks.len();
        let mut read = vec![false; count];
        for (position, item) in self.items.iter().enumerate() {
            for global in graph.reads[position].iter().chain(&graph.assigns[position]) {
                let owner = self.global_chunks[global.0];
                if owner != item.chunk {
                    read[owner.0] = true;
                }
            }
        }
        let dropped = self
            .chunks
            .iter()
            .enumerate()
            .map(|(id, chunk)| {
                id != STARTUP.0
                    && chunk.roots.is_empty()
                    && !read[id]
                    && chunk
                        .items
                        .iter()
                        .all(|&item| silent(program, &self.items[item]))
            })
            .collect::<Vec<_>>();
        if !dropped.contains(&true) {
            return;
        }
        // Imports precede their importers, so a dropped chunk's imports no
        // longer name a dropped chunk when its importers are reached.
        for id in 0..count {
            if self.chunks[id]
                .imports
                .iter()
                .any(|import| dropped[import.0])
            {
                let mut imports = Vec::new();
                for import in &self.chunks[id].imports {
                    if dropped[import.0] {
                        imports.extend_from_slice(&self.chunks[import.0].imports);
                    } else {
                        imports.push(*import);
                    }
                }
                imports.sort_unstable();
                imports.dedup();
                self.chunks[id].imports = imports;
            }
        }
        // Startup takes the dropped chunks' namespaces and items; the other
        // chunks keep their order.
        let mut renumbered = vec![STARTUP; count];
        let mut chunks = Vec::with_capacity(count);
        for (id, chunk) in std::mem::take(&mut self.chunks).into_iter().enumerate() {
            if dropped[id] {
                debug_assert!(chunk.setters.is_empty() && chunk.sites.is_empty());
                let startup: &mut Chunk = &mut chunks[STARTUP.0];
                startup.namespaces.extend(chunk.namespaces);
                startup.items.extend(chunk.items);
            } else {
                renumbered[id] = ChunkId(chunks.len());
                chunks.push(chunk);
            }
        }
        chunks[STARTUP.0].namespaces.sort_unstable();
        chunks[STARTUP.0].items.sort_unstable();
        let renumber = |chunk: &mut ChunkId| *chunk = renumbered[chunk.0];
        for chunk in &mut chunks {
            chunk.imports.iter_mut().for_each(renumber);
            debug_assert!(chunk.imports.is_sorted());
        }
        self.chunks = chunks;
        for root in &mut self.roots {
            renumber(&mut root.chunk);
        }
        self.namespace_chunks.iter_mut().for_each(renumber);
        for item in &mut self.items {
            renumber(&mut item.chunk);
        }
        self.global_chunks.iter_mut().for_each(renumber);
        #[cfg(debug_assertions)]
        for (id, chunk) in self.chunks.iter().enumerate() {
            for import in &chunk.imports {
                let outer = &self.chunks[import.0].signature;
                debug_assert!(
                    import.0 < id
                        && *outer != chunk.signature
                        && outer.is_superset(&chunk.signature),
                    "{} imports {}, whose signature does not strictly contain its own",
                    chunk.name,
                    self.chunks[import.0].name
                );
            }
        }
    }
}

/// Fill `walk` with the seeds of `root`'s walk: the pinned items whose
/// namespace `root` reaches, and for startup those of namespaces no root
/// reaches.
fn seeds(
    forms: &[(ExprId, usize)],
    movable: &[bool],
    namespace_signatures: &[Signature],
    root: usize,
    walk: &mut Vec<usize>,
) {
    walk.clear();
    walk.extend(
        forms
            .iter()
            .zip(movable)
            .enumerate()
            .filter(|(_, ((_, namespace), movable))| {
                let signature = &namespace_signatures[*namespace];
                !**movable
                    && (signature.contains(root) || (root == STARTUP_ROOT && signature.is_empty()))
            })
            .map(|(item, _)| item),
    );
}

/// The file name of a chunk other than startup whose signature holds the
/// roots of namespaces `roots`.
fn chunk_name(mut roots: Vec<&str>) -> String {
    roots.sort_unstable();
    let joined = roots.join("+");
    let name = format!("{joined}.js");
    if roots.len() > 1 && name.len() > MAX_NAME_LENGTH {
        format!("shared-{:016x}.js", fnv1a(joined.as_bytes()))
    } else {
        name
    }
}

/// The 64-bit FNV-1a hash: stable across builds and platforms, so a shared
/// chunk's name changes only with its roots.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_bitsets_by_root_index() {
        let mut a = Signature::empty(70);
        assert_eq!(a.0.len(), 2);
        assert!(a.is_empty());
        a.insert(1);
        a.insert(65);
        assert!(a.contains(65) && !a.contains(64) && !a.contains(200));
        assert_eq!(a.len(), 2);
        assert_eq!(a.roots().collect::<Vec<_>>(), [1, 65]);
        let full = Signature::full(70);
        assert_eq!(full.len(), 70);
        assert!(full.is_superset(&a) && !a.is_superset(&full));
        assert!(a.is_superset(&a.clone()));
        let mut b = Signature::empty(70);
        b.insert(1);
        assert!(a.is_superset(&b) && !b.is_superset(&a));
    }

    #[test]
    fn chunks_are_named_by_their_roots_and_long_names_by_a_hash() {
        assert_eq!(chunk_name(vec!["app.editor"]), "app.editor.js");
        assert_eq!(
            chunk_name(vec!["app.reports", "app.editor"]),
            "app.editor+app.reports.js"
        );
        // A single root keeps its name however long it is.
        let long = "app.".to_owned() + &"x".repeat(90);
        assert_eq!(chunk_name(vec![&long]), format!("{long}.js"));
        // 80 characters is the longest joined name.
        let a = format!("a.{}", "x".repeat(36));
        let b = format!("b.{}", "y".repeat(36));
        assert_eq!(chunk_name(vec![&b, &a]).len(), MAX_NAME_LENGTH);
        let longer = format!("{b}z");
        let name = chunk_name(vec![&longer, &a]);
        assert_eq!(name, chunk_name(vec![&a, &longer]));
        assert!(
            name.starts_with("shared-") && name.ends_with(".js"),
            "{name}"
        );
        assert_eq!(name.len(), "shared-".len() + 16 + ".js".len());
        assert_ne!(name, chunk_name(vec![&longer, &a, "c"]));
        // FNV-1a's published test vectors.
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
