//! Pre-run static module loading, separate from leaf [`crate::Host`] effects.
//!
//! The embedder calls [`ModuleLoader::load`] before spawning the entry task.
//! Loading only resolves, fetches and parses: it runs no JavaScript, accesses
//! no heap, takes no runtime locks and creates no dependency cells or edges.
//! Linking/evaluation runs separately in a spawned entry task.

mod link;
pub(crate) use link::{Action as LinkAction, Linker};

/// Current phase of a live module entry task, for scheduler diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModulePhase {
    Linking,
    Instantiating,
    Evaluating,
}

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::frontend::{ModuleRecord, SyntaxError, parse_module};
use crate::{JsString, WorldId};

/// Host-assigned identity of a resolved specifier. The private numeric ID is
/// compared without JavaScript equality or user-extensible hashing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModuleKey(u64);

impl ModuleKey {
    /// Assign a key in this loader's host identity domain. Aliases of the same
    /// resolved specifier must return the same key; distinct modules must not.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Root-free source supplied by the host. JSON text is retained unparsed until
/// JSON-module instantiation is implemented; it is never parsed as JavaScript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleSource {
    JavaScript { name: String, text: String },
    Json { name: String, text: String },
}

impl ModuleSource {
    /// Reference host classification: a case-sensitive `.json` suffix selects
    /// JSON. Import attributes are not part of the loading contract yet.
    pub fn from_name(name: String, text: String) -> Self {
        if name.ends_with(".json") {
            Self::Json { name, text }
        } else {
            Self::JavaScript { name, text }
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::JavaScript { name, .. } | Self::Json { name, .. } => name,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::JavaScript { text, .. } | Self::Json { text, .. } => text,
        }
    }
}

/// Embedder-supplied module input, driven outside task runs, never a leaf
/// opcode. Implementations may fetch from memory, disk or a network.
///
/// Resolution receives the exact cooked UTF-16 specifier (including lone
/// surrogates). `None` is the entry request; otherwise the referrer is its
/// resolved key. The engine imposes no path/string comparison policy. The
/// reference in-memory host resolves `./x` relative to the referrer's directory.
/// Resolve/fetch errors become [`ModuleLoadError`]s, not failed dependency cells.
///
/// Within a world/version, sources and resolutions must be immutable. Keys must
/// be stable across calls to a loader, including entry and cyclic requests.
/// Use a separate loader for each runtime and host identity domain, since
/// [`WorldId`] and host keys are only unique within their respective domains.
pub trait ModuleHost {
    fn resolve(
        &mut self,
        specifier: &JsString,
        referrer: Option<ModuleKey>,
    ) -> Result<ModuleKey, String>;

    /// Classify `.json` resolved names as [`ModuleSource::Json`], e.g. with
    /// [`ModuleSource::from_name`]. Names are diagnostic labels, not identity.
    fn fetch(&mut self, key: ModuleKey) -> Result<ModuleSource, String>;
}

/// An immutable cached record and its original source. No environment, binding
/// cell, task state, GC handle or executable instance is shared through it.
#[derive(Debug)]
pub struct LoadedModule {
    key: ModuleKey,
    source: ModuleSource,
    record: Option<ModuleRecord>,
}

impl LoadedModule {
    pub fn key(&self) -> ModuleKey {
        self.key
    }

    pub fn source(&self) -> &ModuleSource {
        &self.source
    }

    /// `None` denotes a JSON source awaiting synthetic module support.
    pub fn record(&self) -> Option<&ModuleRecord> {
        self.record.as_ref()
    }

    fn requests(&self) -> &[JsString] {
        self.record
            .as_ref()
            .map_or(&[], ModuleRecord::requested_modules)
    }
}

/// A graph node: a shared immutable record plus resolved requests in the
/// record's source order. These are static imports, not dependency-cell edges.
#[derive(Debug)]
pub struct ModuleNode {
    module: Arc<LoadedModule>,
    requested_modules: Vec<ModuleKey>,
}

impl ModuleNode {
    pub fn module(&self) -> &LoadedModule {
        &self.module
    }

    /// One resolved key for every `ModuleRecord::requested_modules` entry.
    /// Distinct specifier spellings may resolve to the same key.
    pub fn requested_modules(&self) -> &[ModuleKey] {
        &self.requested_modules
    }
}

/// A complete static graph, including cycles. It contains no module instances;
/// linking allocates mutable environments independently per task.
#[derive(Debug)]
pub struct ModuleGraph {
    world: WorldId,
    entry: ModuleKey,
    nodes: HashMap<ModuleKey, ModuleNode>,
    order: Vec<ModuleKey>,
}

impl ModuleGraph {
    pub(crate) fn entry_code(&self) -> crate::Executable {
        link::entry()
    }

    pub fn world(&self) -> WorldId {
        self.world
    }

    pub fn entry(&self) -> ModuleKey {
        self.entry
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn get(&self, key: ModuleKey) -> Option<&ModuleNode> {
        self.nodes.get(&key)
    }

    /// Nodes in depth-first discovery order, with each key present once.
    pub fn modules(&self) -> impl ExactSizeIterator<Item = &ModuleNode> {
        self.order.iter().map(|key| &self.nodes[key])
    }

    fn insert(&mut self, module: Arc<LoadedModule>) {
        let key = module.key;
        self.nodes.insert(
            key,
            ModuleNode {
                module,
                requested_modules: Vec::new(),
            },
        );
        self.order.push(key);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleLoadErrorKind {
    Resolve(String),
    Fetch(String),
    Syntax { name: String, error: SyntaxError },
}

/// A pre-run failure. Specifier/referrer describe the failing request; syntax
/// errors also retain the fetched module name and the frontend's line/column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleLoadError {
    pub specifier: JsString,
    pub referrer: Option<String>,
    pub kind: ModuleLoadErrorKind,
}

impl fmt::Display for ModuleLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ModuleLoadErrorKind::Resolve(reason) | ModuleLoadErrorKind::Fetch(reason) => {
                write!(
                    f,
                    "ReferenceError: could not load module '{}': {reason}",
                    self.specifier
                )?;
            }
            ModuleLoadErrorKind::Syntax { name, error } => {
                write!(f, "{name}:{error} (module '{}')", self.specifier)?;
            }
        }
        if let Some(referrer) = &self.referrer {
            write!(f, " imported from '{referrer}'")?;
        }
        Ok(())
    }
}

impl std::error::Error for ModuleLoadError {}

/// Embedder-owned pre-run loader and immutable record cache. Keep one per
/// runtime/host identity domain. Cache identity is `(world/version, resolved
/// specifier key)`; even unchanged sources are fetched anew in a new world.
///
/// Successful records survive a later graph failure; errors and partial graphs
/// are not cached. A retry resolves requests again and reuses successful records.
/// Loading is exclusive through `&mut self`, with no locks around host calls.
#[derive(Debug, Default)]
pub struct ModuleLoader {
    records: HashMap<(WorldId, ModuleKey), Arc<LoadedModule>>,
}

impl ModuleLoader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve the entry, then fetch/parse its entire static graph depth-first
    /// in source order. Insert nodes before walking requests so cycles resolve
    /// to the already loaded node. The traversal uses a heap stack, not Rust
    /// recursion, and never starts a task or executes module bodies.
    /// The supplied host must expose the immutable source/resolution snapshot
    /// for `world`; the world selects the record cache, not a host-side world.
    pub fn load(
        &mut self,
        world: WorldId,
        entry: &JsString,
        host: &mut (impl ModuleHost + ?Sized),
    ) -> Result<ModuleGraph, ModuleLoadError> {
        let entry_key = host
            .resolve(entry, None)
            .map_err(|reason| ModuleLoadError {
                specifier: entry.clone(),
                referrer: None,
                kind: ModuleLoadErrorKind::Resolve(reason),
            })?;
        let module = self.fetch(world, entry_key, entry, None, host)?;
        let mut graph = ModuleGraph {
            world,
            entry: entry_key,
            nodes: HashMap::new(),
            order: Vec::new(),
        };
        graph.insert(module);
        let mut pending = vec![(entry_key, 0)];
        while let Some((key, index)) = pending.last_mut() {
            let node = &graph.nodes[key];
            let Some(specifier) = node.module.requests().get(*index).cloned() else {
                pending.pop();
                continue;
            };
            *index += 1;
            let referrer = node.module.source.name().to_owned();
            let resolved =
                host.resolve(&specifier, Some(*key))
                    .map_err(|reason| ModuleLoadError {
                        specifier: specifier.clone(),
                        referrer: Some(referrer.clone()),
                        kind: ModuleLoadErrorKind::Resolve(reason),
                    })?;
            graph
                .nodes
                .get_mut(key)
                .unwrap()
                .requested_modules
                .push(resolved);
            if !graph.nodes.contains_key(&resolved) {
                let module = self.fetch(world, resolved, &specifier, Some(&referrer), host)?;
                graph.insert(module);
                pending.push((resolved, 0));
            }
        }
        Ok(graph)
    }

    fn fetch(
        &mut self,
        world: WorldId,
        key: ModuleKey,
        specifier: &JsString,
        referrer: Option<&str>,
        host: &mut (impl ModuleHost + ?Sized),
    ) -> Result<Arc<LoadedModule>, ModuleLoadError> {
        if let Some(module) = self.records.get(&(world, key)) {
            return Ok(Arc::clone(module));
        }
        let fail = |kind| ModuleLoadError {
            specifier: specifier.clone(),
            referrer: referrer.map(str::to_owned),
            kind,
        };
        let source = host
            .fetch(key)
            .map_err(|reason| fail(ModuleLoadErrorKind::Fetch(reason)))?;
        let record = match &source {
            ModuleSource::JavaScript { name, text } => {
                Some(parse_module(text).map_err(|error| {
                    fail(ModuleLoadErrorKind::Syntax {
                        name: name.clone(),
                        error,
                    })
                })?)
            }
            ModuleSource::Json { .. } => None,
        };
        let module = Arc::new(LoadedModule {
            key,
            source,
            record,
        });
        self.records.insert((world, key), Arc::clone(&module));
        Ok(module)
    }
}
