//! The dev build (docs/dev-workflow.md, "The dev build"): a namespace program
//! emitted as a directory whose namespaces a running application can replace
//! one at a time.
//!
//! Every page global of a dev build ends in `$` and the build's key
//! ([`dev_key`], docs/value-layout.md, section 1), written `$KEY` here, so
//! that two dev builds on one page keep their runtimes, clients and
//! definitions apart. The directory holds
//!
//! - `runtime.js`, the whole runtime once:
//!   `globalThis.__lightRuntime$KEY = (factory)();`, with the build's key
//!   ([`runtime_file`]), which each unit written for the page has too;
//! - `client.js`, the dev client ([`CLIENT`], [`client_file`]), which
//!   defines `globalThis.__lightDev$KEY.define`, which records each
//!   namespace's unit and runs it against the runtime at once, keeping its
//!   value, and in a page served by `--serve` reads the server's event
//!   stream; it is also `globalThis.__lightDev`, the client of the dev build
//!   that loaded last;
//! - one `<namespace>.js` per namespace ([`unit_file`]), in dependency
//!   order: the namespace's forms as an interactive unit (the storage model
//!   of the session, [`crate::emitter`]) inside a `define` call, with its
//!   source map, `<namespace>.js.map`;
//! - `manifest.json`: the entry namespace, the namespaces in load order with
//!   their files, requires and whether they declare macros, and the
//!   functions marked `^:dev/before-load` and `^:dev/after-load` (or their
//!   `-async` variants, which take a continuation), in definition order;
//! - `index.js`, which loads the files in that order (script elements in a
//!   page, `require` in Node) and sets `globalThis.__lightDevLoaded$KEY`,
//!   and `globalThis.__lightDevLoaded`, to a promise of the entry
//!   namespace's value, the program's value, which a file that throws while
//!   it runs rejects instead.
//!
//! Units are emitted at `none` semantics with namespace registration on,
//! from the one whole-program [`Program`] by namespace entry, and never go
//! through [`crate::prepare_application`]. Definitions live in
//! `globalThis.__lightApplicationRoots$KEY["ns/name"]`, read at each use, so
//! a unit evaluated again replaces its namespace's definitions for every
//! caller. Each unit registers its namespace's reader context, and the first
//! one registers every namespace of the build, as single-file output does
//! before its first form runs.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::{self, Write};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::analyzer::{ExprId, ExprKind, Program};
use crate::runtime_key::RuntimeKey;
use crate::scheduler::{Config, Job, Next, PoolError, Slots};
use crate::source_map::{SourceFile, json_string};

/// One file of a dev build: its name in the directory and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevFile {
    pub name: String,
    pub text: String,
}

/// Wall-time shares of emitting and writing a dev build, for `--stats`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DevBuildTiming {
    pub emission: Duration,
    pub write: Duration,
}

/// An error while emitting or writing a dev build directly to disk.
#[derive(Debug)]
pub enum DevBuildError {
    CreateDir { path: PathBuf, source: io::Error },
    Write { path: PathBuf, source: io::Error },
    Pool(PoolError),
}

impl fmt::Display for DevBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateDir { path, source } => {
                write!(f, "cannot create {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(f, "cannot write {}: {source}", path.display())
            }
            Self::Pool(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DevBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateDir { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Pool(error) => Some(error),
        }
    }
}

/// The dev client, `client.js` (docs/dev-workflow.md, "The server and the
/// client"): `define`, and the interpreter of a dev server's event stream
/// ([`crate::dev_server`]), which a page served over HTTP connects to.
pub const CLIENT: &str = include_str!("dev_client.js");

/// The files of the dev build of `program`, a program analyzed from
/// namespace sources ([`crate::compiler::analyze_namespace_entry_with_sources`]),
/// in the order to write them. `sources` name the program's sources by source
/// index for the source maps, as URLs relative to the build directory.
///
/// # Panics
///
/// When `program` was not analyzed from namespace sources.
pub fn emit_dev_build(program: &Program, sources: &[SourceFile<'_>]) -> Vec<DevFile> {
    emit_dev_build_of(program, sources, None)
}

/// [`emit_dev_build`] for `build`, the build of light-cljs.edn it is
/// (`compile ID`, `watch ID`), if any: the build with the entry namespace is
/// the build's identity, from which its key is derived ([`dev_key`]).
pub fn emit_dev_build_of(
    program: &Program,
    sources: &[SourceFile<'_>],
    build: Option<&str>,
) -> Vec<DevFile> {
    emit_dev_build_on(program, sources, build, Slots::ONE)
}

/// [`emit_dev_build_of`] running on `slots` CPU work slots (`--jobs N`),
/// collecting the build's files in manifest order. With [`Slots::ONE`], every
/// unit runs inline on the calling thread with no threads spawned.
pub fn emit_dev_build_on(
    program: &Program,
    sources: &[SourceFile<'_>],
    build: Option<&str>,
    slots: Slots,
) -> Vec<DevFile> {
    let ctx = DevBuildContext::new(program, build);
    let mut files = Vec::with_capacity(ctx.manifest.namespaces.len() * 2 + 4);
    files.push(DevFile {
        name: "runtime.js".into(),
        text: runtime_file(&ctx.page),
    });
    files.push(DevFile {
        name: "client.js".into(),
        text: client_file(&ctx.page),
    });
    if slots.workers() == 0 || ctx.manifest.namespaces.len() <= 1 {
        for index in 0..ctx.manifest.namespaces.len() {
            files.extend(ctx.emit_namespace(sources, index));
        }
    } else {
        let batches = dev_batches(program, &ctx.manifest, slots);
        let config = dev_pool_config(slots, batches.len());
        let pooled = crate::scheduler::run(
            &config,
            |_slot| (),
            |(), batch: DevBatch, worker| {
                let mut out = Vec::with_capacity(batch.indices.len());
                for index in batch.indices {
                    if worker.cancelled() {
                        break;
                    }
                    out.push((index, ctx.emit_namespace(sources, index)));
                }
                out
            },
            |pool| {
                pool.submit_batch(batches);
                let mut emitted: Vec<Option<[DevFile; 2]>> =
                    vec![None; ctx.manifest.namespaces.len()];
                loop {
                    match pool.next_outcome() {
                        Ok(Next::Outcome(batch)) => {
                            for (index, pair) in batch {
                                emitted[index] = Some(pair);
                            }
                        }
                        Ok(Next::Quiescent) => return Ok(emitted),
                        Ok(Next::Stalled { .. }) => {
                            unreachable!("dev build pool holds no demands")
                        }
                        Err(panic) => return Err(panic),
                    }
                }
            },
        );
        match pooled {
            Ok(Ok(emitted)) => {
                for pair in emitted {
                    files.extend(pair.expect("every namespace emitted"));
                }
            }
            Ok(Err(panic)) | Err(PoolError::Panic(panic)) => panic!("{panic}"),
            Err(PoolError::Spawn(_)) => {
                for index in 0..ctx.manifest.namespaces.len() {
                    files.extend(ctx.emit_namespace(sources, index));
                }
            }
        }
    }
    files.push(DevFile {
        name: "manifest.json".into(),
        text: ctx.manifest.to_json(),
    });
    files.push(DevFile {
        name: "index.js".into(),
        text: ctx.manifest.index(&ctx.page),
    });
    files
}

/// Emit and write the dev build of `program` directly into `out_dir` on
/// `slots` CPU work slots (`--jobs N`), streaming each namespace's `<ns>.js`
/// and `<ns>.js.map` straight to disk inside the worker rather than buffering
/// every file in memory. With `timed`, measure emission and write wall times
/// for `--stats`.
pub fn write_dev_build_on(
    program: &Program,
    sources: &[SourceFile<'_>],
    build: Option<&str>,
    out_dir: &Path,
    slots: Slots,
    timed: bool,
) -> Result<DevBuildTiming, DevBuildError> {
    let setup_start = timed.then(Instant::now);
    let ctx = DevBuildContext::new(program, build);
    let fixed = [
        ("runtime.js", runtime_file(&ctx.page)),
        ("client.js", client_file(&ctx.page)),
        ("manifest.json", ctx.manifest.to_json()),
        ("index.js", ctx.manifest.index(&ctx.page)),
    ];
    let batches = (slots.workers() > 0 && ctx.manifest.namespaces.len() > 1)
        .then(|| dev_batches(program, &ctx.manifest, slots));
    let fixed_write_start = timed.then(Instant::now);
    let setup_emit = setup_start
        .zip(fixed_write_start)
        .map_or(Duration::ZERO, |(start, end)| end - start);

    fs::create_dir_all(out_dir).map_err(|source| DevBuildError::CreateDir {
        path: out_dir.to_path_buf(),
        source,
    })?;
    for (name, text) in fixed {
        write_dev_file(out_dir, name, &text)?;
    }
    let fixed_write = fixed_write_start.map_or(Duration::ZERO, |start| start.elapsed());

    let units_start = timed.then(Instant::now);
    let (emit_work, write_work) = match batches {
        None => {
            let mut emit_time = Duration::ZERO;
            let mut write_time = Duration::ZERO;
            for index in 0..ctx.manifest.namespaces.len() {
                let t0 = timed.then(Instant::now);
                let [unit, map] = ctx.emit_namespace(sources, index);
                let t1 = timed.then(Instant::now);
                write_dev_file(out_dir, &unit.name, &unit.text)?;
                write_dev_file(out_dir, &map.name, &map.text)?;
                if let (Some(t0), Some(t1)) = (t0, t1) {
                    let t2 = Instant::now();
                    emit_time += t1 - t0;
                    write_time += t2 - t1;
                }
            }
            (emit_time, write_time)
        }
        Some(batches) => {
            let config = dev_pool_config(slots, batches.len());
            let pooled = crate::scheduler::run(
                &config,
                |_slot| (),
                |(), batch: DevBatch, worker| -> Result<(Duration, Duration), DevBuildError> {
                    let mut emit_time = Duration::ZERO;
                    let mut write_time = Duration::ZERO;
                    for index in batch.indices {
                        if worker.cancelled() {
                            break;
                        }
                        let t0 = timed.then(Instant::now);
                        let [unit, map] = ctx.emit_namespace(sources, index);
                        let t1 = timed.then(Instant::now);
                        write_dev_file(out_dir, &unit.name, &unit.text)?;
                        write_dev_file(out_dir, &map.name, &map.text)?;
                        if let (Some(t0), Some(t1)) = (t0, t1) {
                            let t2 = Instant::now();
                            emit_time += t1 - t0;
                            write_time += t2 - t1;
                        }
                    }
                    Ok((emit_time, write_time))
                },
                |pool| {
                    pool.submit_batch(batches);
                    let mut emit_work = Duration::ZERO;
                    let mut write_work = Duration::ZERO;
                    let mut failure: Option<DevBuildError> = None;
                    loop {
                        match pool.next_outcome() {
                            Ok(Next::Outcome(Ok((emit, write)))) => {
                                emit_work += emit;
                                write_work += write;
                            }
                            Ok(Next::Outcome(Err(error))) => {
                                if failure.is_none() {
                                    failure = Some(error);
                                    pool.cancel();
                                }
                            }
                            Ok(Next::Quiescent) => {
                                return match failure {
                                    Some(error) => Err(error),
                                    None => Ok((emit_work, write_work)),
                                };
                            }
                            Ok(Next::Stalled { .. }) => {
                                unreachable!("dev build pool holds no demands")
                            }
                            Err(panic) => return Err(DevBuildError::Pool(PoolError::Panic(panic))),
                        }
                    }
                },
            );
            pooled.map_err(DevBuildError::Pool)??
        }
    };
    let units_wall = units_start.map_or(Duration::ZERO, |start| start.elapsed());
    let (units_emit, units_write) = split_wall(units_wall, emit_work, write_work);
    Ok(DevBuildTiming {
        emission: setup_emit + units_emit,
        write: fixed_write + units_write,
    })
}

/// Shared read-only state for emitting all namespace units of one dev build.
struct DevBuildContext<'a> {
    entries: HashMap<&'a str, ExprId>,
    manifest: DevManifest,
    page: RuntimeKey,
    key: RuntimeKey,
    units: crate::emitter::NamespaceUnits<'a>,
}

impl<'a> DevBuildContext<'a> {
    fn new(program: &'a Program, build: Option<&str>) -> Self {
        let entries = program
            .namespace_entries
            .iter()
            .map(|(id, namespace)| (namespace.as_str(), *id))
            .collect::<HashMap<_, _>>();
        assert!(
            !program.namespaces.is_empty()
                && program
                    .namespaces
                    .iter()
                    .all(|namespace| entries.contains_key(namespace.as_str())),
            "a dev build needs a program analyzed from namespace sources"
        );
        let manifest = DevManifest::new(program);
        let page = dev_key(build, &manifest.entry);
        let key = page.units();
        let units = crate::emitter::NamespaceUnits::new(program);
        Self {
            entries,
            manifest,
            page,
            key,
            units,
        }
    }

    fn emit_namespace(&self, sources: &[SourceFile<'_>], index: usize) -> [DevFile; 2] {
        let namespace = &self.manifest.namespaces[index];
        let map = format!("{}.map", namespace.file);
        let mut define = String::from(DEFINE);
        json_string(&mut define, &namespace.name);
        define.push_str(", ");
        json_list(&mut define, &namespace.requires);
        define.push_str(", ");
        let (unit, mut mapping) = self.units.emit(
            self.entries[namespace.name.as_str()],
            index == 0,
            [&define, ");\n"],
        );
        let mut unit = self.key.apply_mapped(&unit, &mut mapping);
        writeln!(unit, "//# sourceMappingURL={map}").unwrap();
        let json = mapping.to_json(Some(&namespace.file), sources, false);
        [
            DevFile {
                name: namespace.file.clone(),
                text: unit,
            },
            DevFile {
                name: map,
                text: json,
            },
        ]
    }
}

/// A coarse batch of manifest namespace indices scheduled as one pool job,
/// prioritized longest-first by estimated expression weight.
#[derive(Debug, PartialEq, Eq)]
struct DevBatch {
    indices: Vec<usize>,
    weight: u64,
}

impl Job for DevBatch {
    fn phase(&self) -> usize {
        0
    }

    fn cost(&self) -> u64 {
        self.weight.max(1)
    }

    fn priority(&self) -> u64 {
        self.weight
    }

    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    fn key(&self) -> u64 {
        self.indices[0] as u64
    }
}

const MAX_DEV_BATCH_NAMESPACES: usize = 16;

fn dev_pool_config(slots: Slots, batches: usize) -> Config {
    let mut config = Config::new(slots, 1);
    config.limits.batch_jobs = 1;
    config.limits.admitted_jobs = batches.clamp(1, 4096);
    config
}

/// Partition `0..manifest.namespaces.len()` into coarse batches weighted by
/// expression count (plus whole-program registration cost on namespace 0),
/// ordered heaviest-first so large units start early.
fn dev_batches(program: &Program, manifest: &DevManifest, slots: Slots) -> Vec<DevBatch> {
    let mut weights = vec![0u64; manifest.namespaces.len()];
    for expr in program.exprs.iter() {
        if let Some(weight) = weights.get_mut(expr.source_index) {
            *weight += 1;
        }
    }
    for weight in &mut weights {
        *weight = (*weight).max(1);
    }
    if let Some(first) = weights.first_mut() {
        // Namespace 0 registers every namespace and global of the program.
        *first = first
            .saturating_add(program.globals.len() as u64)
            .saturating_add(program.namespaces.len() as u64);
    }
    pack_dev_batches(&weights, slots)
}

fn pack_dev_batches(weights: &[u64], slots: Slots) -> Vec<DevBatch> {
    let count = weights.len();
    if count == 0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by_key(|&index| (Reverse(weights[index]), index));
    let workers = slots.workers().max(1);
    let max_batch_ns = count
        .div_ceil(workers * 4)
        .clamp(1, MAX_DEV_BATCH_NAMESPACES);
    let total_weight: u64 = weights.iter().copied().fold(0u64, u64::saturating_add);
    let target_weight = (total_weight / (workers as u64 * 8)).max(1);
    let mut batches = Vec::new();
    let mut cursor = 0;
    while cursor < order.len() {
        let first = order[cursor];
        let mut indices = vec![first];
        let mut weight = weights[first];
        cursor += 1;
        while cursor < order.len() && indices.len() < max_batch_ns {
            let next = order[cursor];
            let next_weight = weights[next];
            if weight.saturating_add(next_weight) > target_weight {
                break;
            }
            indices.push(next);
            weight = weight.saturating_add(next_weight);
            cursor += 1;
        }
        batches.push(DevBatch { indices, weight });
    }
    batches
}

fn write_dev_file(out_dir: &Path, name: &str, text: &str) -> Result<(), DevBuildError> {
    let path = out_dir.join(name);
    fs::write(&path, text).map_err(|source| DevBuildError::Write { path, source })
}

fn split_wall(wall: Duration, emit: Duration, write: Duration) -> (Duration, Duration) {
    let total = emit.as_nanos() + write.as_nanos();
    if total == 0 {
        return (wall, Duration::ZERO);
    }
    let write_nanos = wall.as_nanos().saturating_mul(write.as_nanos()) / total;
    let write_wall = Duration::from_nanos(write_nanos.min(u128::from(u64::MAX)) as u64);
    (wall.saturating_sub(write_wall), write_wall)
}

/// The opening of a unit's call of its build's `define`, with the key
/// placeholder the unit's writer replaces.
const DEFINE: &str = "globalThis.__lightDev$LKEY.define(";

/// The key of the dev build `build` (of light-cljs.edn, if any) whose entry
/// namespace is `entry` ([`RuntimeKey::dev`]): the build and the entry are
/// its identity. `compile ID` and `watch ID` of one build give the same key,
/// as do two copies of it; another build, or another entry, another one.
/// The dev session keeps it for the units, steps and REPL forms it writes
/// against a page of the build
/// ([`crate::dev_session::DevSession::page_key`]).
pub fn dev_key(build: Option<&str>, entry: &str) -> RuntimeKey {
    RuntimeKey::dev(&format!("{}\n{entry}", build.unwrap_or("")))
}

/// `runtime.js` of the dev build whose key is `key` ([`dev_key`]): the
/// whole runtime with that key, as `globalThis.__lightRuntime$KEY`, which
/// registers the key with a hash of the build's identity, so that two
/// different dev builds on one page whose keys match stop as the second
/// loads.
pub fn runtime_file(key: &RuntimeKey) -> String {
    key.apply(&format!(
        "globalThis.__lightRuntime{} = {};\n",
        crate::runtime_key::KEY_PLACEHOLDER,
        crate::runtime_manifest::RUNTIME.trim()
    ))
}

/// `client.js` of the dev build whose key is `key` ([`dev_key`]): the dev
/// client ([`CLIENT`]), whose globals end in the key.
pub fn client_file(key: &RuntimeKey) -> String {
    key.units().apply(CLIENT)
}

/// What `manifest.json` of a dev build says: the entry namespace, the
/// namespaces in load order and the hook functions. A watch step
/// (`--watch`) changes it as namespaces change and are added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevManifest {
    /// The namespace whose value is the program's value.
    pub entry: String,
    /// The namespaces in load order: each after those it requires.
    pub namespaces: Vec<DevNamespace>,
    /// The functions marked `^:dev/before-load` or
    /// `^:dev/before-load-async`, in definition order.
    pub before_load: Vec<DevHook>,
    /// The functions marked `^:dev/after-load` or `^:dev/after-load-async`,
    /// in definition order.
    pub after_load: Vec<DevHook>,
}

/// One namespace of a [`DevManifest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevNamespace {
    pub name: String,
    /// Its unit's file in the build directory ([`unit_file`]).
    pub file: String,
    /// The namespaces of the build it requires.
    pub requires: Vec<String>,
    /// Whether it declares macros, which the units of its requirers expand,
    /// so a change to it recompiles them.
    pub macros: bool,
}

/// A hook function of a [`DevManifest`]: `ns/name`, and whether it is an
/// `-async` hook, which takes a continuation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevHook {
    pub name: String,
    pub asynchronous: bool,
}

impl DevManifest {
    /// The manifest of the dev build of `program`, a program analyzed from
    /// namespace sources.
    pub fn new(program: &Program) -> Self {
        let namespaces = program
            .namespaces
            .iter()
            .enumerate()
            .map(|(index, namespace)| DevNamespace {
                name: namespace.clone(),
                file: unit_file(namespace),
                requires: program
                    .namespace_requires
                    .get(index)
                    .map_or(&[][..], Vec::as_slice)
                    .iter()
                    .map(|&required| program.namespaces[required].clone())
                    .collect(),
                macros: program.declares_macros(namespace),
            })
            .collect();
        let [before_load, after_load] = program_hooks(program, &program.namespaces);
        Self {
            entry: entry_namespace(program).to_owned(),
            namespaces,
            before_load,
            after_load,
        }
    }

    /// The position of `namespace` in load order.
    pub fn position(&self, namespace: &str) -> Option<usize> {
        self.namespaces
            .iter()
            .position(|entry| entry.name == namespace)
    }

    /// Put the namespaces in an order where each follows the namespaces of
    /// the build it requires, keeping the present order where it already
    /// does: a namespace that a changed `ns` form added (which a step places
    /// ahead of its requirer) moves after its own requires. A cycle keeps
    /// the order its first namespace reaches.
    pub fn sort(&mut self) {
        fn visit(
            index: usize,
            namespaces: &[DevNamespace],
            positions: &HashMap<&str, usize>,
            seen: &mut [bool],
            order: &mut Vec<usize>,
        ) {
            if std::mem::replace(&mut seen[index], true) {
                return;
            }
            for required in &namespaces[index].requires {
                if let Some(&at) = positions.get(required.as_str()) {
                    visit(at, namespaces, positions, seen, order);
                }
            }
            order.push(index);
        }
        // Each name's first position, as a search from the front finds it:
        // a step sorts the whole build, so a lookup must not scan it.
        let mut positions = HashMap::with_capacity(self.namespaces.len());
        for (at, entry) in self.namespaces.iter().enumerate() {
            positions.entry(entry.name.as_str()).or_insert(at);
        }
        let mut seen = vec![false; self.namespaces.len()];
        let mut order = Vec::with_capacity(self.namespaces.len());
        for index in 0..self.namespaces.len() {
            visit(index, &self.namespaces, &positions, &mut seen, &mut order);
        }
        let mut namespaces = std::mem::take(&mut self.namespaces)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();
        self.namespaces = order
            .into_iter()
            .map(|index| namespaces[index].take().unwrap())
            .collect();
    }

    /// `manifest.json`.
    pub fn to_json(&self) -> String {
        let mut manifest = String::from("{\n  \"entry\": ");
        json_string(&mut manifest, &self.entry);
        manifest.push_str(",\n  \"namespaces\": [");
        for (index, namespace) in self.namespaces.iter().enumerate() {
            manifest.push_str(if index == 0 { "\n" } else { ",\n" });
            manifest.push_str("    {\"name\": ");
            json_string(&mut manifest, &namespace.name);
            manifest.push_str(", \"file\": ");
            json_string(&mut manifest, &namespace.file);
            manifest.push_str(", \"requires\": ");
            json_list(&mut manifest, &namespace.requires);
            write!(manifest, ", \"macros\": {}}}", namespace.macros).unwrap();
        }
        manifest.push_str("\n  ]");
        for (phase, hooks) in [
            ("before-load", &self.before_load),
            ("after-load", &self.after_load),
        ] {
            write!(manifest, ",\n  \"{phase}\": [").unwrap();
            for (index, hook) in hooks.iter().enumerate() {
                manifest.push_str(if index == 0 { "\n    " } else { ",\n    " });
                manifest.push_str("{\"name\": ");
                json_string(&mut manifest, &hook.name);
                write!(manifest, ", \"async\": {}}}", hook.asynchronous).unwrap();
            }
            manifest.push_str(if hooks.is_empty() { "]" } else { "\n  ]" });
        }
        manifest.push_str("\n}\n");
        manifest
    }
}

/// The hook functions of `program` (docs/dev-workflow.md, "The reload set"):
/// the definitions whose metadata marks them `^:dev/before-load` (or
/// `-async`), then those it marks `^:dev/after-load` (or `-async`), each list
/// in definition order: by namespace in the order of `namespaces`, the load
/// order, then in the order of their definitions' metadata maps, which are
/// analyzed with the definitions (a declaration may have allocated the
/// global earlier). A definition of a namespace not in `namespaces` comes
/// last. In a session's retained program, a redefinition that carries
/// metadata replaces the global's, so a step that recompiles a namespace
/// puts its hooks in source order and drops the flag a definition lost; a
/// definition removed from the source keeps its last metadata, as it stays
/// defined. A client runs the before-load hooks in reverse.
pub(crate) fn program_hooks<'a>(
    program: &Program,
    namespaces: impl IntoIterator<Item = &'a String>,
) -> [Vec<DevHook>; 2] {
    let marked = program
        .globals
        .iter()
        .enumerate()
        .filter_map(|(id, global)| hook_definition(program, global).map(|hook| (id, hook)));
    ordered_hooks(program, marked, namespaces)
}

/// A hook definition's metadata and the phases it marks (before-load,
/// after-load), each `Some(asynchronous)`; `None` for a global that is no
/// hook.
pub(crate) type HookDefinition = (ExprId, [Option<bool>; 2]);

/// The [`HookDefinition`] of `global`, a global of `program`, if it is a
/// hook.
pub(crate) fn hook_definition(
    program: &Program,
    global: &crate::analyzer::GlobalBinding,
) -> Option<HookDefinition> {
    let metadata = global.metadata?;
    let phases = hook_phases(program, metadata);
    phases
        .iter()
        .any(Option::is_some)
        .then_some((metadata, phases))
}

/// [`program_hooks`] from the hook definitions `marked` of `program`, by
/// global ID, ascending, as [`hook_definition`] finds them, which a session
/// keeps between its forms instead of visiting every global again
/// ([`crate::eval_compiler::InteractiveCompiler::dev_hooks`]).
pub(crate) fn ordered_hooks<'a>(
    program: &Program,
    marked: impl Iterator<Item = (usize, HookDefinition)>,
    namespaces: impl IntoIterator<Item = &'a String>,
) -> [Vec<DevHook>; 2] {
    // Each namespace's first position, found once rather than per global:
    // a step runs this over the whole retained program.
    let mut positions = HashMap::new();
    let mut count = 0;
    for (at, namespace) in namespaces.into_iter().enumerate() {
        positions.entry(namespace.as_str()).or_insert(at);
        count = at + 1;
    }
    let position = |namespace: &str| positions.get(namespace).copied().unwrap_or(count);
    // Only definitions that a hook flag marks are ordered; the stable sort
    // keeps their relative order as when every definition was.
    let mut definitions = marked
        .map(|(id, (metadata, phases))| {
            let global = &program.globals[id];
            (position(&global.namespace), metadata, global, phases)
        })
        .collect::<Vec<_>>();
    definitions.sort_by_key(|&(namespace, metadata, _, _)| (namespace, metadata.0));
    [0, 1].map(|phase| {
        definitions
            .iter()
            .filter_map(|&(_, _, global, phases)| {
                let asynchronous = phases[phase]?;
                Some(DevHook {
                    name: format!("{}/{}", global.namespace, global.name),
                    asynchronous,
                })
            })
            .collect()
    })
}

/// Whether the namespace symbol `symbol`, with the metadata of its `ns`
/// form, marks the namespace `:dev/<name>` with a value other than `nil` or
/// `false`: `^:dev/once` (load once, never reload) or `^:dev/always` (reload
/// with every load).
pub(crate) fn namespace_flag(symbol: &crate::compiler_data::Datum, name: &str) -> bool {
    crate::cache_policy::metadata_flag(symbol, "dev", name)
}

/// The entry namespace: the last one the program loads statically, which
/// namespaces reached only through lazy references follow.
fn entry_namespace(program: &Program) -> &str {
    let entry = program
        .namespace_lazy_only
        .iter()
        .position(|&lazy| lazy)
        .unwrap_or(program.namespaces.len())
        - 1;
    &program.namespaces[entry]
}

/// The unit file of `namespace` written from the session's units of its
/// forms (a watch step), and its source map: the `define` call of an
/// interactive unit, as [`emit_dev_build`] writes, that registers the
/// namespaces `registered` as `session` knows them now, makes `namespace`
/// the current namespace, as a `Namespace` action does, then runs each
/// form's unit in order. With `value` it returns the last one's value, else
/// nil: the value of a namespace whose last form is a directive, or that has
/// no forms. `registered` is `namespace`, or every namespace of the build
/// for the first unit in load order, as the build's units register them;
/// the forms' units of [`crate::dev_session::DevSession::load_namespace_file`]
/// register none, so the file registers its namespace once, with what all
/// its forms defined. The forms' units share the file's prelude: the file's function
/// runs each form's statements in a scope of its own. Where raw JavaScript
/// renamed a unit's names, whose raw text may then name a global `rt`, the
/// file runs each unit whole instead, as a function of the runtime.
/// `requires` are the namespaces of the build it requires; `url` names a
/// unit's source, as the source provider named it, in the map, relative to
/// the directory.
pub fn namespace_unit(
    session: &crate::dev_session::DevSession,
    namespace: &str,
    requires: &[String],
    registered: &[&str],
    units: &[&crate::dev_session::Unit],
    value: bool,
    url: &dyn Fn(&str) -> String,
) -> [DevFile; 2] {
    let file = unit_file(namespace);
    let map = format!("{file}.map");
    let mut text = crate::source_map::MappedText::default();
    let prelude = crate::emitter::interactive_prelude();
    // The lines of the prelude: those of a form's unit before its statements.
    let prelude_lines = prelude.matches('\n').count() as u32;
    // Each unit without its `sourceURL`: the file's `sourceMappingURL` maps
    // it, and the file is what the comment named.
    let texts = units
        .iter()
        .map(|unit| match unit.js.rfind("\n//# sourceURL=") {
            Some(end) => &unit.js[..end],
            None => unit.js.trim_end(),
        })
        .collect::<Vec<_>>();
    let statements = texts
        .iter()
        .map(|js| {
            js.strip_prefix(&prelude)
                .and_then(|rest| rest.strip_suffix(crate::emitter::INTERACTIVE_CLOSE))
        })
        .collect::<Option<Vec<_>>>();
    let mut head = String::from(DEFINE);
    json_string(&mut head, namespace);
    head.push_str(", ");
    json_list(&mut head, requires);
    head.push_str(", ");
    let mut setup = session.registration(registered);
    setup.push_str("  rt.core.currentNs = rt.core.createNs(rt.symbol(null, ");
    json_string(&mut setup, namespace);
    setup.push_str("));\n");
    if statements.is_some() {
        head.push_str(&prelude);
        head.push_str(&setup);
    } else {
        write!(
            head,
            "(__lightInteractiveRuntime) => {{\n  ((rt) => {{\n{setup}}})(__lightInteractiveRuntime);\n"
        )
        .unwrap();
    }
    text.push_str(&head);
    // The sources of the units, by their names.
    let mut sources: Vec<&crate::compiler_data::Source> = Vec::new();
    for (index, (unit, js)) in units.iter().zip(&texts).enumerate() {
        let returns = value && index + 1 == units.len();
        let source = unit.location.source();
        let at = match sources
            .iter()
            .position(|known| known.name() == source.name())
        {
            Some(at) => at,
            None => {
                sources.push(source);
                sources.len() - 1
            }
        };
        text.push_str(if returns { "  return (" } else { "  (" });
        match &statements {
            Some(statements) => {
                text.push_str("() => {\n");
                match &unit.mapping {
                    Some(mapping) => {
                        text.push_mapped_lines(statements[index], mapping, &[at], prelude_lines);
                    }
                    None => text.push_str(statements[index]),
                }
                text.push_str("})();\n");
            }
            None => {
                match &unit.mapping {
                    Some(mapping) => text.push_mapped(js, mapping, &[at]),
                    None => text.push_str(js),
                }
                text.push_str(")(__lightInteractiveRuntime);\n");
            }
        }
    }
    if !value || units.is_empty() {
        text.push_str("  return null;\n");
    }
    if statements.is_some() {
        text.push_str(crate::emitter::INTERACTIVE_CLOSE);
        text.push_str(");\n");
    } else {
        text.push_str("});\n");
    }
    let urls = sources
        .iter()
        .map(|source| url(source.name()))
        .collect::<Vec<_>>();
    let files = urls
        .iter()
        .zip(&sources)
        .map(|(url, source)| SourceFile {
            url,
            text: source.text(),
        })
        .collect::<Vec<_>>();
    // The page has the build's key, which the session keeps
    // (`DevSession::page_key`); the session's own runtime keeps the
    // placeholder.
    let mut mapping = text.mapping;
    let mut unit = session
        .page_key()
        .units()
        .apply_mapped(&text.text, &mut mapping);
    let json = mapping.to_json(Some(&file), &files, false);
    writeln!(unit, "//# sourceMappingURL={map}").unwrap();
    [
        DevFile {
            name: file,
            text: unit,
        },
        DevFile {
            name: map,
            text: json,
        },
    ]
}

/// The file of `namespace`'s unit: `<namespace>.js`, or `<namespace>$.js`
/// for a namespace named like a file of its own (`runtime`, `client`,
/// `index`), since no namespace name contains a `$`.
pub fn unit_file(namespace: &str) -> String {
    if matches!(namespace, "runtime" | "client" | "index") {
        format!("{namespace}$.js")
    } else {
        format!("{namespace}.js")
    }
}

impl DevManifest {
    /// `index.js` of the build whose key is `key` ([`dev_key`]): loads the
    /// files of the build in order and sets `globalThis.__lightDevLoaded$KEY`
    /// and `globalThis.__lightDevLoaded` to a promise of the entry's value.
    pub fn index(&self, key: &RuntimeKey) -> String {
        let files = ["runtime.js", "client.js"]
            .into_iter()
            .chain(
                self.namespaces
                    .iter()
                    .map(|namespace| namespace.file.as_str()),
            )
            .collect::<Vec<_>>();
        let mut list = String::new();
        json_list(&mut list, &files);
        let mut entry = String::new();
        json_string(&mut entry, &self.entry);
        key.units().apply(&index_text(&list, &entry))
    }
}

/// The text of `index.js` for the JSON array `list` of its files and the
/// JSON string `entry`, with the key placeholder for the build's key.
///
/// In a page every file is requested at once and the files run in order:
/// script elements with `async = false`, which a browser fetches in parallel
/// and runs in insertion order. Waiting for each file's load event before
/// requesting the next cost the 3,077 files of a generated 200k-line app
/// about 3 s of round trips (benchmarks/watch-vs-shadow.md). The first error
/// thrown while the files run, or a file that cannot load, is the failure:
/// from then until every file has settled, `globalThis.__lightDevHalted$KEY`
/// makes the build's `define` skip each later unit, so, as when the files
/// loaded one at a time, no unit after the failure runs. An error is the
/// build's when one of its own script elements is the document's
/// `currentScript` as the page reports it, as a browser has it while the
/// script runs, so another dev build loading alongside never halts this one
/// (the Sol review of light-cljs-1045). The promise rejects once every file
/// has settled and the flag is gone, so a reload the client applies
/// afterwards defines its units.
fn index_text(list: &str, entry: &str) -> String {
    format!(
        r#"// Loads a light-cljs dev build: the runtime, the dev client and one unit per
// namespace, in dependency order. globalThis.__lightDevLoaded$LKEY is a
// promise of the entry namespace's value, the program's value, and so is
// globalThis.__lightDevLoaded, until another dev build loads. In a page each
// file is a script element after this one, all requested at once and run in
// order (`async = false`). The first error thrown while they run, or a file
// that cannot load, rejects the promise once every file has settled, and no
// unit after it runs: the build's `define` skips units while
// globalThis.__lightDevHalted$LKEY is set. An error the page reports while
// another script runs, another dev build's say, is not this build's. In
// Node (CommonJS) each file is required, which `node --enable-source-maps`
// maps to the sources.
globalThis.__lightDevLoaded = globalThis.__lightDevLoaded$LKEY = (async () => {{
  const files = {list};
  if (typeof document === "object" && document !== null) {{
    const base = document.currentScript?.src ?? document.baseURI;
    let failure = null;
    const fail = error => {{
      if (failure !== null) return;
      failure = error;
      globalThis.__lightDevHalted$LKEY = true;
    }};
    const scripts = new Set();
    const failed = event => {{
      if (scripts.has(document.currentScript)) fail(event.error ?? new Error(event.message));
    }};
    globalThis.addEventListener("error", failed);
    try {{
      await Promise.all(files.map(file => new Promise(resolve => {{
        const script = document.createElement("script");
        scripts.add(script);
        script.src = new URL(file, base).href;
        script.async = false;
        script.onload = resolve;
        script.onerror = () => {{ fail(new Error("cannot load " + script.src)); resolve(); }};
        document.head.append(script);
      }})));
    }} finally {{
      globalThis.removeEventListener("error", failed);
      delete globalThis.__lightDevHalted$LKEY;
    }}
    if (failure !== null) throw failure;
  }} else {{
    for (const file of files) require("./" + file);
  }}
  return globalThis.__lightDev$LKEY.values[{entry}];
}})();
"#
    )
}

/// The hook phases the metadata map `metadata` marks, before-load then
/// after-load: for each, `true` if it has `:dev/<phase>-async` with a value
/// other than `nil` or `false`, else `false` if it has `:dev/<phase>` with
/// one, else `None`. One pass over the map, its keys compared as UTF-16
/// without decoding: a watch step checks every definition's metadata.
fn hook_phases(program: &Program, metadata: ExprId) -> [Option<bool>; 2] {
    const NAMES: [&str; 4] = [
        "before-load",
        "before-load-async",
        "after-load",
        "after-load-async",
    ];
    let ExprKind::Map(entries) = &program.exprs[metadata.0].kind else {
        return [None; 2];
    };
    let is = |id: crate::analyzer::StringId, text: &str| {
        program.strings[id.0]
            .iter()
            .copied()
            .eq(text.encode_utf16())
    };
    let mut marked = [false; 4];
    for [key, value] in entries.as_chunks::<2>().0 {
        let ExprKind::Keyword(literal) = program.exprs[key.0].kind else {
            continue;
        };
        let literal = &program.named_literals[literal.0];
        if !literal
            .namespace
            .is_some_and(|namespace| is(namespace, "dev"))
            || matches!(
                program.exprs[value.0].kind,
                ExprKind::Nil | ExprKind::Bool(false)
            )
        {
            continue;
        }
        if let Some(at) = NAMES.iter().position(|name| is(literal.name, name)) {
            marked[at] = true;
        }
    }
    [0, 1].map(|phase| {
        if marked[2 * phase + 1] {
            Some(true)
        } else {
            marked[2 * phase].then_some(false)
        }
    })
}

/// Append `items` as a JSON array of strings.
fn json_list(out: &mut String, items: &[impl AsRef<str>]) {
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        json_string(out, item.as_ref());
    }
    out.push(']');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_dev_batches_covers_every_namespace_heaviest_first() {
        // One heavy namespace 0 plus 500 small namespaces on 8 slots (7 workers).
        let mut weights = vec![100u64; 501];
        weights[0] = 25_000;
        weights[17] = 8_000;
        let slots = Slots::new(8).unwrap();
        let batches = pack_dev_batches(&weights, slots);
        assert_eq!(batches[0].indices, vec![0]);
        assert_eq!(batches[0].priority(), 25_000);
        assert_eq!(batches[1].indices, vec![17]);
        assert_eq!(batches[1].priority(), 8_000);
        assert!(
            batches.iter().all(|batch| !batch.indices.is_empty()
                && batch.indices.len() <= MAX_DEV_BATCH_NAMESPACES)
        );
        // Every namespace appears exactly once.
        let mut seen = vec![false; weights.len()];
        for batch in &batches {
            let sum: u64 = batch.indices.iter().map(|&i| weights[i]).sum();
            assert_eq!(batch.weight, sum);
            for &index in &batch.indices {
                assert!(
                    !std::mem::replace(&mut seen[index], true),
                    "duplicate {index}"
                );
            }
        }
        assert!(seen.into_iter().all(|v| v));
        // Small project gets 1 namespace per batch so workers stay busy.
        let small = pack_dev_batches(&[50, 40, 30, 20], slots);
        assert_eq!(small.len(), 4);
        assert_eq!(small[0].indices, vec![0]);
    }

    #[test]
    fn split_wall_preserves_wall_total() {
        let wall = Duration::from_millis(100);
        let (emit, write) =
            split_wall(wall, Duration::from_millis(300), Duration::from_millis(100));
        assert_eq!(emit + write, wall);
        assert_eq!(emit, Duration::from_millis(75));
        assert_eq!(write, Duration::from_millis(25));
        assert_eq!(
            split_wall(wall, Duration::ZERO, Duration::ZERO),
            (wall, Duration::ZERO)
        );
    }
}
