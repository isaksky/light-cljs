//! Chunk placement at namespace granularity (docs/code-splitting.md, "The
//! model" and "Phase 2"): which top-level forms of a prepared program each
//! chunk of a chunked build holds, which chunks each imports, and which
//! globals each exports a setter for. [`plan`] is pure data;
//! [`PreparedApplication::emit_chunks`] writes one ES module per chunk of it
//! ([`ChunkedOutput`]).
//!
//! A *root* is a place code can start running: startup (the entry namespace,
//! root 0) and each distinct namespace a `light.lazy/loadable` names (root
//! `1 + i` for [`Program::lazy_namespaces`]`[i]`; a Var target roots its
//! namespace at this phase). The *static reach* of a root is the namespaces
//! reachable from its namespace over [`Program::namespace_requires`]. The
//! *signature* of a namespace is the set of roots whose reach contains it, a
//! bitset dense by root index ([`Signature`]), and every top-level form of
//! the namespace (an *item*, an entry of the flattened `program.root`) is
//! pinned to it.
//!
//! A chunk is a distinct signature, except that every signature containing
//! startup is the startup chunk: startup loads first, so nothing it reaches
//! is deferred. Reach is monotone, so a chunk's imports, the chunks holding
//! the namespaces it requires and the globals its items read, have strictly
//! larger signatures, and the chunk graph is acyclic by construction (debug
//! builds check every edge). Chunk ids are ordered so that a chunk's imports
//! precede it: startup first, then larger signatures before smaller ones.

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
    pub chunk: ChunkId,
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
    /// The source indices of the namespaces the chunk holds, ascending.
    pub namespaces: Vec<usize>,
    /// The roots whose namespace the chunk holds, ascending.
    pub roots: Vec<usize>,
    /// The chunk's items, indices into [`ChunkPlan::items`], ascending: the
    /// order of `program.root`, so namespaces initialize in source order.
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
    /// By [`GlobalId`]: the chunk owning the global, which declares it.
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
    /// The names of the files it imports statically: every chunk holding a
    /// namespace its namespaces require or a binding its code reads.
    pub imports: Vec<String>,
    /// The namespaces whose top-level forms it holds, in source order.
    pub namespaces: Vec<String>,
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
        let files = plan
            .chunks
            .iter()
            .zip(emitted)
            .map(|(chunk, emitted)| ChunkFile {
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
}

/// Plan the chunks of `prepared`, the application of `program`, at namespace
/// granularity. A program that was not analyzed from namespace sources has
/// one namespace and one chunk.
pub fn plan(program: &Program, prepared: &PreparedApplication) -> ChunkPlan {
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

    // One chunk per distinct signature without startup; a namespace no root
    // reaches (none of a program the loader built) stays in startup, where
    // the single file runs it.
    let mut groups: Vec<(Signature, Vec<usize>)> = Vec::new();
    let mut group_of = HashMap::new();
    let mut startup_namespaces = Vec::new();
    for (source, signature) in namespace_signatures.iter().enumerate() {
        if signature.contains(STARTUP_ROOT) || signature.is_empty() {
            startup_namespaces.push(source);
            continue;
        }
        let group = *group_of.entry(signature).or_insert_with(|| {
            groups.push((signature.clone(), Vec::new()));
            groups.len() - 1
        });
        groups[group].1.push(source);
    }
    // Larger signatures first, so that every import precedes its importer;
    // equal sizes keep the order of their first namespace.
    groups.sort_by_key(|(signature, _)| std::cmp::Reverse(signature.len()));
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

    // Items, each pinned to its namespace, in program order.
    let mut items = Vec::new();
    let mut forms = vec![(emitted.root, entry)];
    while let Some((id, namespace)) = forms.pop() {
        let namespace = emitted
            .namespace_entries
            .get(&id)
            .map_or(namespace, |name| index[name.as_str()]);
        if let ExprKind::Do(body) = &emitted.exprs[id.0].kind {
            forms.extend(body.iter().rev().map(|&form| (form, namespace)));
        } else {
            let chunk = namespace_chunks[namespace];
            chunks[chunk.0].items.push(items.len());
            items.push(Item {
                expr: id,
                namespace,
                chunk,
            });
        }
    }

    // A global belongs to its namespace's chunk; a global of a namespace
    // without a source stays with the runtime in startup.
    let global_chunks = emitted
        .globals
        .iter()
        .map(|global| {
            index
                .get(global.namespace.as_str())
                .map_or(STARTUP, |&source| namespace_chunks[source])
        })
        .collect::<Vec<_>>();

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
    let mut walked = vec![usize::MAX; emitted.exprs.len()];
    let mut pending = Vec::new();
    // By chunk: the loadable sites resolving to it, keyed for program order;
    // each target once per namespace (a site copied by inlining is one).
    let mut sites = vec![Vec::new(); chunks.len()];
    let mut listed = HashSet::new();
    for (position, item) in items.iter().enumerate() {
        pending.push(item.expr);
        while let Some(id) = pending.pop() {
            if std::mem::replace(&mut walked[id.0], position) == position {
                continue;
            }
            let kind = &emitted.exprs[id.0].kind;
            if let ExprKind::Global(global) = *kind {
                let binding = &emitted.globals[global.0];
                // Every root reaching the reader reaches what it reads.
                debug_assert!(
                    index.get(binding.namespace.as_str()).is_none_or(|&source| {
                        namespace_signatures[source]
                            .is_superset(&namespace_signatures[item.namespace])
                    }),
                    "{} reads {}/{} outside its reach",
                    names[item.namespace],
                    binding.namespace,
                    binding.name
                );
                edge(item.chunk, global_chunks[global.0]);
            }
            // An assignment of another chunk's global goes through the
            // owner's setter; the owner's own assignments stay direct.
            match kind {
                ExprKind::SetGlobal { target, .. } => {
                    assign(emitted, &global_chunks, item.chunk, *target, &mut setters);
                }
                ExprKind::Binding { bindings, .. } => {
                    for (target, _) in bindings {
                        assign(emitted, &global_chunks, item.chunk, *target, &mut setters);
                    }
                }
                // A site resolves to the chunk of its target's root.
                ExprKind::Loadable { target, .. } if listed.insert((target.0, item.namespace)) => {
                    let root = 1 + emitted.lazy_targets[target.0].namespace;
                    sites[roots[root].chunk.0].push((
                        (position, id.0),
                        Site {
                            target: *target,
                            namespace: item.namespace,
                        },
                    ));
                }
                _ => {}
            }
            pending.extend(children(emitted, id));
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
    ChunkPlan {
        roots,
        namespace_signatures,
        namespace_chunks,
        items,
        global_chunks,
        chunks,
    }
}

/// Record that an item of chunk `from` assigns `target` when it is a global
/// another chunk owns.
fn assign(
    program: &Program,
    global_chunks: &[ChunkId],
    from: ChunkId,
    target: ExprId,
    setters: &mut [Vec<GlobalId>],
) {
    if let ExprKind::Global(global) = program.exprs[target.0].kind {
        let owner = global_chunks[global.0];
        if owner != from {
            setters[owner.0].push(global);
        }
    }
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
