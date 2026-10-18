//! `--watch` (docs/dev-workflow.md, "The watch loop"): write the dev build of
//! `--reloadable`, keep a `DevSession` that has loaded the same program,
//! then poll the source roots and rewrite the unit of each namespace whose
//! file changed, compiled against the session's retained program.
//!
//! Without `--cache-dir` the startup build runs with the native engine and
//! the session then loads the program again, as a `require` of the entry.
//! With it, the startup build runs once, in the session's host and engine,
//! with the persistent cache, and the session starts from that build
//! (`DevSession::compile_entry`, docs/compiler-cache.md, "Development
//! sessions and engine boundaries"): its manifest, from the records the
//! cache keyed each namespace with, is the watch's. Steps neither read nor
//! write the cache.
//!
//! A step compiles the changed file's forms as fragments in its namespace
//! (`DevSession::load_namespace_file`, in warning mode), writes its unit
//! and source map (`dev_build::namespace_unit`) and the manifest. A form
//! that fails stops the step: the error goes to stderr and the namespace
//! keeps its previous unit, to be compiled again at the next change. A
//! changed `ns` form loads its new dependencies through the source provider,
//! which adds their units to the manifest ahead of it.
//!
//! What else a step recompiles comes from the build's dependency manifest
//! (`light_cljs::dependency_manifest`), recorded by the startup build and
//! revised with what the session observes as it compiles: every namespace
//! that depends on a recompiled one through a macro-provider or helper
//! edge, since its unit holds expansions that may have changed, and so on
//! from those; the consumers the manifest records of a `light.build`
//! resource whose file changed, appeared or went; and on every step, before
//! the retained units are kept, the always-compile set of the policy
//! evaluator (`light_cljs::cache_policy`): the `^:dev/always` namespaces and
//! their compilation consumers. A step that succeeds finds the hook
//! functions again in the retained program for the manifest, and with
//! `--serve` pushes the units of its reload set (`Watch::reload_set`: what
//! it wrote, never a `^:dev/once` one again, and with `--reload-dependents`
//! those downstream).
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::cache_policy::{self, CacheSettings};
use light_cljs::compile_stats::CompileStats;
use light_cljs::compiler_data::Name;
use light_cljs::dependency_manifest::{
    DependencyManifest, EdgeKinds, ExternalInput, Revision, SourceOrigin,
};
use light_cljs::dev_build::{DevFile, DevManifest, DevNamespace, namespace_unit, unit_file};
use light_cljs::dev_server::{Build, DevServer, Diagnostic, LoadUnit, Step};
use light_cljs::dev_session::{Action, DevSession, EntryStart, Unit};
use light_cljs::source_provider::{
    FileSourceProvider, LoadError, LoadRequest, Reload, SourceInput, SourceProvider,
};
use light_cljs::{EvalOptions, LineIndex, MacroHost};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc::RecvTimeoutError,
    time::{Duration, Instant, SystemTime},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub const USAGE: &str = "Watch:
  light-cljs --watch --entry NAMESPACE --source-root DIR [--source-root DIR ...] --out-dir DIR
             [--poll-ms N] [--elide-asserts] [--stats] [--config KEY=EDN ...]
             [--macro-expansions N] [--macro-nodes N] [--macro-timeout-ms N]
             [--macro-memory-limit N] [--macro-poll-budget N]
             [--cache-dir DIR] [--cache-blocker NS ...]
             [--serve PORT [--serve-host HOST] [--reload-dependents]]

Write the dev build of --compile --reloadable into DIR, then poll the .cljs
and .cljc files under the source roots, and every file a light.build
resource was looked for at, every N milliseconds (100), comparing
modification times and sizes. A changed namespace is compiled again against
the program the watch keeps, and its unit, source map and manifest.json are
rewritten; so is every namespace whose macro expansions may have changed:
one that uses a macro of a recompiled namespace, or a macro whose namespace
requires a recompiled one. A changed, new or deleted resource recompiles
the namespaces that read it, a new require adds the units of the namespaces
it loads, and every step also recompiles the namespaces marked ^:dev/always
and those that depend on them.
Each step prints one line to stderr with the namespaces recompiled and the
time taken; a form that fails prints its error with file, line and column,
and the namespace keeps its previous unit. Runs until interrupted.

With --stats, print the --compile --stats report of the startup build before
that build's line, with the time the watch's session took to load the
program as time.session-load-ms.

With --macro-expansions N, --macro-nodes N, --macro-timeout-ms N,
--macro-memory-limit N and --macro-poll-budget N, raise the macro limits as
--compile does (defaults 1024, 100000, 10000, 67108864 and 1000000), for the
startup build and for the watch's session: its load of the program at
startup is one operation under the deadline and poll budget, and in a step
so is each namespace compiled again. As with --compile, macros see stub JavaScript packages, which only the
application has.

With --config KEY=EDN, repeatable, give light.build/config the value EDN for
the keyword KEY in the build and in every step, as --compile does.

With --cache-dir DIR, start from the persistent compiler cache in DIR, as
--compile --cache-dir keeps it: the startup build runs once, in the engine
of the watch's session, installing each namespace whose artifact is current
instead of analyzing it (its macros do not run) and storing the others, and
the session starts from that build, compiling later steps as if it had
loaded every namespace itself. Steps neither read nor write the cache, so a
restart analyzes what changed since the cache was last filled. Deleting DIR
does not change a running watch; a fresh uncached start is a restart
without --cache-dir. With --stats, the report adds the cache.* lines.

With --cache-blocker NS, repeatable, keep NS and every namespace whose
compilation depends on it out of the persistent compiler cache, as --compile
does. A blocker never makes a step compile a namespace whose retained unit
is still valid.

With --serve PORT (0 for any free port), also serve the build over HTTP on
127.0.0.1, or on HOST with --serve-host (0.0.0.0 opens it to the network;
a connected page runs what the server sends it): the files of DIR and of
the source roots, a built-in page at / when DIR has no index.html, and at
/events a stream of server-sent events that pushes each step's units, which
the build's client.js loads into the page between the ^:dev/before-load and
^:dev/after-load hooks, and its warnings and errors, which it draws over
the page. A page that connects late catches up with the build. The server
also answers light-cljs --repl --connect 127.0.0.1:PORT, whose forms it
compiles with the program the watch keeps and runs in a connected runtime.

A step loads the namespaces it recompiled, which include those that use a
changed macro and the namespaces marked ^:dev/always, in load order; never
again a namespace marked ^:dev/once. With
--reload-dependents, it also loads every namespace that depends on one it
loads, directly or not, as shadow-cljs's :full reload strategy does.";

/// The options of `--watch`.
struct Options {
    entry: String,
    roots: Vec<PathBuf>,
    out_dir: PathBuf,
    poll: Duration,
    assertions: bool,
    /// `--serve PORT`.
    serve: Option<u16>,
    /// `--serve-host HOST`.
    serve_host: Option<String>,
    /// `--reload-dependents`.
    reload_dependents: bool,
    /// `--stats`: report the startup build (`light_cljs::compile_stats`).
    stats: bool,
    /// `--config KEY=EDN`: what `light.build/config` answers.
    configuration: light_cljs::BuildConfiguration,
    /// `--cache-blocker NS`, for the policy evaluator and the cache.
    blockers: Vec<String>,
    /// `--cache-dir DIR`: start from the persistent compiler cache.
    cache_dir: Option<PathBuf>,
    /// `--macro-expansions N` and the other macro limit flags, for the
    /// startup build and the session.
    macro_limits: crate::MacroLimitFlags,
}

fn options(mut args: impl Iterator<Item = OsString>) -> Result<Option<Options>> {
    let (mut entry, mut roots, mut out_dir) = (None, Vec::new(), None);
    let mut poll = Duration::from_millis(100);
    let mut assertions = true;
    let (mut serve, mut serve_host, mut reload_dependents) = (None, None, false);
    let mut stats = false;
    let mut configuration = light_cljs::BuildConfiguration::new();
    let mut blockers = Vec::new();
    let mut cache_dir = None;
    let mut macro_limits = crate::MacroLimitFlags::default();
    while let Some(arg) = args.next() {
        let flag = arg.to_str().unwrap_or("");
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{flag} requires a value"))
        };
        match flag {
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "--entry" if entry.is_none() => {
                entry = Some(
                    value()?
                        .into_string()
                        .map_err(|_| "entry namespace must be valid UTF-8")?,
                );
            }
            "--entry" => return Err("--entry may only be specified once".into()),
            "--source-root" => roots.push(PathBuf::from(value()?)),
            "--out-dir" => out_dir = Some(PathBuf::from(value()?)),
            "--poll-ms" => {
                let text = value()?;
                let millis = text
                    .to_str()
                    .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|text| text.parse::<u64>().ok())
                    .filter(|&millis| millis > 0)
                    .ok_or("--poll-ms requires a positive integer")?;
                poll = Duration::from_millis(millis);
            }
            "--elide-asserts" => assertions = false,
            "--serve" => {
                let text = value()?;
                serve = Some(
                    text.to_str()
                        .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
                        .and_then(|text| text.parse::<u16>().ok())
                        .ok_or("--serve requires a port number, 0 to 65535")?,
                );
            }
            "--serve-host" => {
                serve_host = Some(
                    value()?
                        .into_string()
                        .map_err(|_| "--serve-host must be valid UTF-8")?,
                );
            }
            "--reload-dependents" => reload_dependents = true,
            "--stats" => stats = true,
            "--config" => {
                let text = value()
                    .map_err(|_| "--config requires KEY=EDN")?
                    .into_string()
                    .map_err(|_| "--config must be valid UTF-8")?;
                configuration
                    .insert_text(&text)
                    .map_err(|error| format!("--config {text}: {error}"))?;
            }
            "--cache-blocker" => blockers.push(
                value()?
                    .into_string()
                    .map_err(|_| "--cache-blocker must be valid UTF-8")?,
            ),
            "--cache-dir" => cache_dir = Some(PathBuf::from(value()?)),
            _ if macro_limits.accept(flag, &mut args)? => {}
            _ => return Err(format!("unexpected watch option: {flag}\n{USAGE}").into()),
        }
    }
    let entry = entry.ok_or("--watch requires --entry NAMESPACE")?;
    if roots.is_empty() {
        return Err("--watch requires --source-root DIR".into());
    }
    let out_dir = out_dir.ok_or("--watch requires --out-dir DIR")?;
    if serve_host.is_some() && serve.is_none() {
        return Err("--serve-host requires --serve PORT".into());
    }
    if reload_dependents && serve.is_none() {
        return Err("--reload-dependents requires --serve PORT".into());
    }
    Ok(Some(Options {
        entry,
        roots,
        out_dir,
        poll,
        assertions,
        serve,
        serve_host,
        reload_dependents,
        stats,
        configuration,
        blockers,
        cache_dir,
        macro_limits,
    }))
}

/// A file's modification time and size, which saving it changes.
type Stamp = (Option<SystemTime>, u64);

/// The `.cljs` and `.cljc` files under `roots`, with their stamps. Hidden
/// files and directories (an editor's lock and swap files) are left out. A
/// symbolic link is followed, as the source provider follows it, but each
/// directory is read once.
fn scan(roots: &[PathBuf]) -> BTreeMap<PathBuf, Stamp> {
    let mut files = BTreeMap::new();
    let mut directories = roots.to_vec();
    let mut read = HashSet::new();
    while let Some(directory) = directories.pop() {
        if !fs::canonicalize(&directory).is_ok_and(|canonical| read.insert(canonical)) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with(['.', '#']) {
                continue;
            }
            let path = entry.path();
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if metadata.is_dir() {
                directories.push(path);
            } else if metadata.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "cljs" || extension == "cljc")
            {
                files.insert(path, (metadata.modified().ok(), metadata.len()));
            }
        }
    }
    files
}

/// The session's source provider: the source roots, recording the file
/// each namespace was last loaded from.
struct Recording {
    roots: FileSourceProvider,
    files: RefCell<HashMap<String, PathBuf>>,
}

impl SourceProvider for Recording {
    fn load(&self, request: &LoadRequest) -> std::result::Result<Option<SourceInput>, LoadError> {
        self.load_traced(request, &mut Vec::new())
    }
    fn load_traced(
        &self,
        request: &LoadRequest,
        tried: &mut Vec<light_cljs::dependency_manifest::Candidate>,
    ) -> std::result::Result<Option<SourceInput>, LoadError> {
        let source = self.roots.load_traced(request, tried)?;
        if let Some(source) = &source {
            self.files.borrow_mut().insert(
                String::from_utf16_lossy(request.namespace.name()),
                PathBuf::from(&source.file),
            );
        }
        Ok(source)
    }
    fn resources(&self) -> Option<Rc<dyn light_cljs::source_provider::ResourceProvider>> {
        self.roots.resources()
    }
}

/// The state a watch keeps between steps.
struct Watch {
    session: DevSession,
    /// The session's source provider.
    provider: Rc<Recording>,
    /// The source roots, canonical, as the scan names files.
    roots: Vec<PathBuf>,
    manifest: DevManifest,
    out_dir: PathBuf,
    /// `out_dir`, canonical: source maps name sources relative to it.
    base: PathBuf,
    /// The last `manifest.json` and `index.js` written.
    written: [String; 2],
    /// The namespaces a failed step left uncompiled, compiled again at the
    /// next change: the one that failed (perhaps over a file that does not
    /// exist yet) and those after it.
    pending: Vec<String>,
    /// The dev server of `--serve`.
    server: Option<DevServer>,
    /// With a server, the text of each namespace's unit as written.
    units: HashMap<String, String>,
    /// The namespaces whose units failed steps wrote but did not push,
    /// which the next step that succeeds pushes.
    unpushed: Vec<String>,
    /// The warnings of the step under way.
    warnings: Vec<Diagnostic>,
    /// `--reload-dependents`: a load adds the namespaces downstream of
    /// those it reloads.
    reload_dependents: bool,
    /// The namespaces the runtimes have been sent: those of the first build
    /// and of every step that loaded. A `^:dev/once` one is not sent again.
    published: HashSet<String>,
    /// The build's dependency manifest: the startup build's, revised with
    /// what the session observed in each namespace it compiled again.
    graph: DependencyManifest,
    /// The policy evaluator's settings. Only the startup build reads the
    /// persistent cache, and a disk bypass invalidates nothing in memory.
    settings: CacheSettings,
    /// What the step under way compiled again, not yet in `graph`.
    revisions: HashMap<String, Revision>,
    /// Every file a `light.build` resource of `graph` was looked for at,
    /// with its stamp, `None` where there is no file.
    resources: BTreeMap<PathBuf, Option<Stamp>>,
    /// The namespaces `graph` records as reading each file of `resources`.
    resource_consumers: HashMap<PathBuf, Vec<String>>,
    /// Files of `resources` that changed between their read and their first
    /// stamp, which the next poll reports.
    stale_resources: Vec<PathBuf>,
}

/// Why a namespace's compile ended a step.
enum Failure {
    /// Its source did not compile: the namespace keeps its unit and is
    /// compiled again at the next change. The error as printed, and located
    /// for the server.
    Source(String, Diagnostic),
    /// A file of the build could not be written, which ends the watch.
    Output(Box<dyn Error>),
}

pub fn run(args: impl Iterator<Item = OsString>) -> Result<()> {
    let Some(options) = options(args)? else {
        return Ok(());
    };
    let begun = Instant::now();
    // Canonical roots name files as the source provider does. The stamps
    // come first, so a file saved while the build is made is a change.
    let roots = options
        .roots
        .iter()
        .map(|root| {
            fs::canonicalize(root)
                .map_err(|error| format!("cannot read source root {}: {error}", root.display()))
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut stamps = scan(&roots);
    let mut host = MacroHost::new();
    host.set_assertions(options.assertions);
    host.set_build_configuration(options.configuration.clone());
    // Macros see stub packages, as with --compile: only the application
    // has the host's packages.
    host.opaque_packages = true;
    options
        .macro_limits
        .apply(&mut host.limits, &mut host.evaluator_options);
    // Dev compilation reports undeclared Vars and arity conflicts as
    // warnings (docs/dev-workflow.md, "Dev-mode contract").
    let analysis = AnalysisOptions {
        warning_mode: true,
        ..AnalysisOptions::default()
    };
    let mut warnings = Vec::new();
    let mut stats = options.stats.then(CompileStats::new);
    // The startup build's dependency manifest is the watch's, which each
    // step revises with what the session observes.
    let mut graph = DependencyManifest::default();
    // The session's source provider, recording the file of each namespace.
    let provider = Rc::new(Recording {
        roots: FileSourceProvider::new(roots.clone()),
        files: RefCell::default(),
    });
    let mut session_options = EvalOptions {
        source_provider: Some(provider.clone()),
        assertions: options.assertions,
        configuration: options.configuration.clone(),
        opaque_packages: true,
        ..EvalOptions::default()
    };
    // The limits bound the session's startup load, its `require` of the
    // entry, as one operation, and each step's.
    options
        .macro_limits
        .apply(&mut session_options.compiler, &mut session_options.engine);
    let cache = options
        .cache_dir
        .clone()
        .map(|dir| light_cljs::compile_cache::CacheOptions {
            dir,
            blockers: options.blockers.clone(),
        });
    // With --cache-dir, the startup build runs once, in the session's own
    // host and engine, with the persistent cache, and the session starts
    // from it (`DevSession::compile_entry`). Without it, the build runs
    // with V8 and the session loads the program again below.
    let (program, seeded) = match &cache {
        Some(cache) => match DevSession::compile_entry(
            &session_options,
            EntryStart {
                entry: &options.entry,
                roots: &options.roots,
                cache: Some(cache),
                stats: stats.as_mut(),
                manifest: Some(&mut graph),
            },
            &mut warnings,
        ) {
            Ok((session, program, sources)) => (Ok((program, sources)), Some(session)),
            Err(error) => (Err(error), None),
        },
        None => (
            crate::analyze_entry(
                &options.entry,
                &options.roots,
                &mut host,
                analysis,
                &mut warnings,
                stats.as_mut(),
                Some(&mut graph),
                &|_| {
                    Some(crate::EmissionSettings {
                        application: light_cljs::ApplicationOptions::new(
                            light_cljs::OptimizationLevel::None,
                        ),
                        layout: light_cljs::cache_key::OutputLayout::Reloadable,
                        source_map: light_cljs::cache_key::SourceMapOutput::None,
                        include_sources: false,
                    })
                },
            ),
            None,
        ),
    };
    crate::print_warnings(&warnings);
    let (program, sources) = program?;
    // The build's warnings, located for the server, which shows them to the
    // runtimes that connect before the first step.
    let startup = warnings
        .iter()
        .map(|warning| {
            let (path, text) = &sources[warning.source_index];
            let (line, column) = LineIndex::new(text)
                .line_column(warning.span.start)
                .unwrap_or((1, 1));
            Diagnostic {
                message: format!("{} [{}]", warning.message, warning.kind),
                file: Some(
                    fs::canonicalize(path)
                        .unwrap_or_else(|_| path.clone())
                        .display()
                        .to_string(),
                ),
                line: Some(line),
                column: Some(column),
            }
        })
        .collect::<Vec<_>>();
    crate::write_dev_build(
        &program,
        &sources,
        &options.out_dir,
        stats.as_mut().map(|stats| &mut stats.phases),
    )?;
    let manifest = DevManifest::new(&program);
    drop(program);
    let session_started = stats.is_some().then(Instant::now);
    let session = match seeded {
        // The session has the build's program; the provider records the
        // file each namespace of the source roots was loaded from, as a
        // load through it would.
        Some(session) => {
            // Every namespace a source root provided, at the file the
            // loader resolved: a symbolic link's target may lie outside
            // the roots.
            let mut files = provider.files.borrow_mut();
            for record in &graph.namespaces {
                if record.source.origin == SourceOrigin::Provider {
                    files.insert(record.name.clone(), PathBuf::from(&record.source.file));
                }
            }
            drop(files);
            session
        }
        // The session loads the program again, which is the program its
        // steps compile against; the build printed the warnings.
        None => DevSession::new(&session_options)?,
    };
    // A `require` of the entry, which a seeded session has loaded already.
    let actions = session.source(&SourceInput::new(
        "<watch>",
        format!("(require '{})", options.entry),
    ));
    if let Some(Action::Error(error)) = actions.last() {
        return Err(crate::eval_cli::diagnostic(error).into());
    }
    if let (Some(stats), Some(started)) = (&mut stats, session_started)
        && stats.session_load.is_none()
    {
        stats.session_load = Some(started.elapsed());
    }
    crate::print_stats(stats, begun)?;
    let base = fs::canonicalize(&options.out_dir)?;
    let mut watch = Watch {
        written: [manifest.to_json(), manifest.index()],
        session,
        provider,
        roots: roots.clone(),
        manifest,
        out_dir: options.out_dir.clone(),
        base,
        pending: Vec::new(),
        server: None,
        units: HashMap::new(),
        unpushed: Vec::new(),
        warnings: Vec::new(),
        reload_dependents: options.reload_dependents,
        published: HashSet::new(),
        graph,
        settings: CacheSettings {
            enabled: options.cache_dir.is_some(),
            blockers: options.blockers.clone(),
        },
        revisions: HashMap::new(),
        resources: BTreeMap::new(),
        resource_consumers: HashMap::new(),
        stale_resources: Vec::new(),
    };
    watch.watch_resources();
    watch.published = watch.namespaces().map(str::to_owned).collect();
    // The hooks as the session's program has them, which the manifest of
    // the build lists already; each step that succeeds finds them again.
    watch.refresh_hooks();
    watch.write_manifest()?;
    let mut repl = None;
    if let Some(port) = options.serve {
        for namespace in &watch.manifest.namespaces {
            let path = options.out_dir.join(&namespace.file);
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            watch.units.insert(namespace.name.clone(), text);
        }
        let host = options.serve_host.as_deref().unwrap_or("127.0.0.1");
        let server = DevServer::start(host, port, &options.out_dir, &roots, watch.build())
            .map_err(|error| format!("cannot serve on {host}:{port}: {error}"))?;
        eprintln!(
            "light-cljs: serving {} at http://{}/",
            options.out_dir.display(),
            server.address()
        );
        if !startup.is_empty() {
            server.publish(Step {
                warnings: startup,
                ..Step::default()
            });
        }
        // The connected REPL compiles with the watch's session, in the entry
        // namespace to begin with, and calls the runtime's `light.dev`
        // functions.
        light_cljs::dev_repl::declare(&watch.session)?;
        repl = Some(server.serve_repl(&options.entry));
        watch.server = Some(server);
    }
    eprintln!(
        "light-cljs: built {} ({} namespaces) into {} in {} ms; watching every {} ms",
        options.entry,
        watch.manifest.namespaces.len(),
        options.out_dir.display(),
        begun.elapsed().as_millis(),
        options.poll.as_millis()
    );
    loop {
        // Between polls, compile the forms of connected REPLs as they come.
        let poll = Instant::now() + options.poll;
        while let Some(wait) = poll.checked_duration_since(Instant::now()) {
            match &repl {
                Some(requests) => match requests.recv_timeout(wait) {
                    Ok(request) => request.compile(&watch.session),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => repl = None,
                },
                None => std::thread::sleep(wait),
            }
        }
        let now = scan(&roots);
        let resources = watch.poll_resources();
        if now == stamps && resources.is_empty() {
            continue;
        }
        let noticed = Instant::now();
        let changed = now
            .iter()
            .filter(|&(path, stamp)| stamps.get(path) != Some(stamp))
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        let deleted = stamps
            .keys()
            .filter(|path| !now.contains_key(*path))
            .cloned()
            .collect::<Vec<_>>();
        stamps = now;
        watch.step(&changed, &deleted, &resources, noticed)?;
    }
}

/// The stamp of the file at `path`, `None` when there is no file there.
fn stamp(path: &Path) -> Option<Stamp> {
    fs::metadata(path)
        .ok()
        .filter(fs::Metadata::is_file)
        .map(|metadata| (metadata.modified().ok(), metadata.len()))
}

impl Watch {
    /// The namespace of the build whose source `path` is, a file the scan
    /// found under a source root: by its place under the root, as the source
    /// provider looks for a namespace, so a file replaced by another (a
    /// symbolic link by a file, say) is still its namespace's. Only a
    /// namespace loaded from the source roots is watched.
    fn namespace_of(&self, path: &Path) -> Option<String> {
        let relative = self
            .roots
            .iter()
            .find_map(|root| path.strip_prefix(root).ok())?
            .with_extension("");
        let relative = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let files = self.provider.files.borrow();
        self.manifest
            .namespaces
            .iter()
            .find(|entry| {
                files.contains_key(&entry.name)
                    && entry.name.replace('.', "/").replace('-', "_") == relative
            })
            .map(|entry| entry.name.clone())
    }

    /// One step: the source files `changed` (or added) and `deleted` since
    /// the last poll, and the files of `resources` that changed, appeared or
    /// went, which the poll noticed at `noticed`.
    fn step(
        &mut self,
        changed: &[PathBuf],
        deleted: &[PathBuf],
        resources: &[PathBuf],
        noticed: Instant,
    ) -> Result<()> {
        for path in deleted {
            if let Some(namespace) = self.namespace_of(path) {
                eprintln!(
                    "light-cljs: {} was deleted; {namespace} keeps its unit",
                    path.display()
                );
            }
        }
        if changed.is_empty() && resources.is_empty() {
            return Ok(());
        }
        let mut queue = std::mem::take(&mut self.pending);
        let enqueue = |queue: &mut Vec<String>, namespace: String| {
            if !queue.contains(&namespace) {
                queue.push(namespace);
            }
        };
        for path in changed {
            if let Some(namespace) = self.namespace_of(path) {
                enqueue(&mut queue, namespace);
            }
        }
        // A resource invalidates the namespaces the manifest records as
        // reading it, which a namespace's next compile records again.
        for path in resources {
            for namespace in self.resource_consumers.get(path).into_iter().flatten() {
                enqueue(&mut queue, namespace.clone());
            }
        }
        if queue.is_empty() {
            return Ok(());
        }
        // A triggered step recompiles the always-compile set before it keeps
        // any retained unit (docs/compiler-cache.md, "Opt-outs and their
        // propagation"): the `^:dev/always` namespaces and their compilation
        // consumers, as the policy evaluator finds them for the cache too.
        for namespace in self.always_compile() {
            enqueue(&mut queue, namespace);
        }
        let mut recompiled = Vec::new();
        let mut failed = None;
        while !queue.is_empty() {
            // The first in load order, so a macro namespace comes before
            // the namespaces that use it.
            let next = (0..queue.len())
                .min_by_key(|&index| self.manifest.position(&queue[index]))
                .unwrap();
            let namespace = queue.remove(next);
            match self.compile(&namespace, &mut recompiled, &mut queue) {
                Ok(()) => {}
                Err(Failure::Source(error, diagnostic)) => {
                    eprintln!("light-cljs: {error}");
                    queue.insert(0, namespace.clone());
                    self.pending = queue;
                    failed = Some((namespace, diagnostic));
                    break;
                }
                Err(Failure::Output(error)) => return Err(error),
            }
        }
        // What the step compiled joins the manifest, and its resources the
        // files the poll watches.
        self.revise();
        self.watch_resources();
        // A step that succeeds finds the hooks again in the retained
        // program; a failed one leaves them, as it leaves its namespace's
        // unit.
        if failed.is_none() {
            self.refresh_hooks();
        }
        self.write_manifest()?;
        recompiled.sort_by_key(|namespace: &String| self.manifest.position(namespace));
        recompiled.dedup();
        let names = if recompiled.is_empty() {
            "nothing".to_owned()
        } else {
            recompiled.join(", ")
        };
        let warnings = std::mem::take(&mut self.warnings);
        if let Some(server) = &self.server {
            // A failed step pushes nothing; the next one that succeeds
            // pushes what it wrote too.
            let mut load = Vec::new();
            if failed.is_none() {
                let mut written = std::mem::take(&mut self.unpushed);
                written.extend(recompiled.iter().cloned());
                load = self
                    .reload_set(&written)
                    .iter()
                    .filter_map(|namespace| self.load_unit(namespace))
                    .collect();
                if !load.is_empty() {
                    self.published = self.namespaces().map(str::to_owned).collect();
                }
            } else {
                self.unpushed.extend(recompiled.iter().cloned());
            }
            server.publish(Step {
                load,
                warnings,
                error: failed.as_ref().map(|(_, diagnostic)| diagnostic.clone()),
                // A runtime that connects after a failed step catches up
                // with the last step that succeeded, then gets the error.
                build: (failed.is_none() && !recompiled.is_empty()).then(|| self.build()),
            });
        }
        let millis = noticed.elapsed().as_secs_f64() * 1000.0;
        match failed {
            None => eprintln!("light-cljs: recompiled {names} in {millis:.1} ms"),
            Some((namespace, _)) => eprintln!(
                "light-cljs: recompiled {names} in {millis:.1} ms; {namespace} failed and keeps its previous unit"
            ),
        }
        Ok(())
    }

    /// Compile the source of `namespace` again, as the source provider finds
    /// it now, and write the units it makes, adding them to `recompiled`; a
    /// namespace that declares macros adds the namespaces that require it,
    /// unless this step compiled them already, to `queue`.
    fn compile(
        &mut self,
        namespace: &str,
        recompiled: &mut Vec<String>,
        queue: &mut Vec<String>,
    ) -> std::result::Result<(), Failure> {
        let request = LoadRequest {
            namespace: Name::new(None, namespace),
            requesting_namespace: Name::new(None, namespace),
            requesting_file: None,
            reload: Reload::Namespace,
        };
        let source = match self.provider.load(&request) {
            Ok(Some(source)) => source,
            Ok(None) => {
                eprintln!(
                    "light-cljs: no source of {namespace} under the source roots; it keeps its unit"
                );
                return Ok(());
            }
            Err(LoadError::Provider(message)) => {
                let diagnostic = Diagnostic {
                    message: message.clone(),
                    ..Diagnostic::default()
                };
                return Err(Failure::Source(message, diagnostic));
            }
            Err(error) => {
                let message = format!("{namespace}: {error}");
                let diagnostic = Diagnostic {
                    message: message.clone(),
                    ..Diagnostic::default()
                };
                return Err(Failure::Source(message, diagnostic));
            }
        };
        let macros = self
            .manifest
            .position(namespace)
            .is_some_and(|index| self.manifest.namespaces[index].macros);
        let always = self.always(namespace);
        let (actions, observations) = self
            .session
            .observing(|session| session.load_namespace_file(namespace, &source));
        // The namespaces the file's forms ran in or loaded, in the order the
        // step entered them, with their units, and the positions of the
        // action of each one's last unit and of the last action entering it.
        let mut entered: Vec<&str> = Vec::new();
        let mut units: HashMap<&str, Vec<&Unit>> = HashMap::new();
        let mut last_unit = HashMap::new();
        let mut last_entry = HashMap::new();
        let mut error = None;
        for (index, action) in actions.iter().enumerate() {
            match action {
                Action::Namespace(name) => {
                    entered.push(name);
                    last_entry.insert(name.as_str(), index);
                }
                Action::Load(loaded) => {
                    for unit in loaded {
                        entered.push(&unit.namespace);
                        units.entry(&unit.namespace).or_default().push(unit);
                        last_unit.insert(unit.namespace.as_str(), index);
                    }
                }
                Action::Invoke(unit) => {
                    entered.push(&unit.namespace);
                    units.entry(&unit.namespace).or_default().push(unit);
                    last_unit.insert(unit.namespace.as_str(), index);
                }
                Action::Warnings(warnings) => {
                    for warning in warnings {
                        eprintln!("WARNING: {}", crate::eval_cli::diagnostic(warning));
                        self.warnings.push(Diagnostic::located(warning));
                    }
                }
                Action::Error(diagnostic) => {
                    error = Some((
                        crate::eval_cli::diagnostic(diagnostic),
                        Diagnostic::located(diagnostic),
                    ));
                }
            }
        }
        let mut seen = HashSet::new();
        entered.retain(|name| seen.insert(*name));
        // A require the session does not refuse, of a namespace it has
        // loaded, can close a cycle, which `--compile` refuses.
        if error.is_none()
            && let Some(cycle) = self.cycle(namespace)
        {
            let message = format!("namespace dependency cycle: {}", cycle.join(" -> "));
            error = Some((
                format!("{}: {message}", source.file),
                Diagnostic {
                    message,
                    file: Some(source.file.clone()),
                    ..Diagnostic::default()
                },
            ));
        }
        // A namespace the file's forms loaded is new to the build; it goes
        // ahead of the namespace that loaded it, which `DevManifest::sort`
        // then puts in order. One that failed to load is not written: the
        // session loads it again with the next compile of its requirer.
        // Without an error, the namespace itself and any other one whose
        // forms ran are written too.
        let write = entered
            .into_iter()
            .filter(|&name| {
                let new = self.manifest.position(name).is_none();
                if error.is_some() {
                    new && self.session.loaded(name)
                } else {
                    name == namespace
                        || units.contains_key(name)
                        || new && self.session.loaded(name)
                }
            })
            .collect::<Vec<_>>();
        for &name in &write {
            if self.manifest.position(name).is_none() {
                let at = self.manifest.position(namespace).unwrap_or(0);
                self.manifest.namespaces.insert(
                    at,
                    DevNamespace {
                        name: name.to_owned(),
                        file: unit_file(name),
                        requires: Vec::new(),
                        macros: false,
                    },
                );
            }
        }
        for &name in &write {
            let requires = self
                .session
                .requires(name)
                .into_iter()
                .filter(|required| self.manifest.position(required).is_some())
                .collect::<Vec<_>>();
            // The namespace's value is its last form's: nil when that is a
            // directive, which enters the namespace after its last unit.
            let value = last_unit
                .get(name)
                .is_some_and(|unit| last_entry.get(name).is_none_or(|entry| entry < unit));
            // The file registers its namespace, and the first in load order
            // every namespace of the build, as the build's units do.
            let registered = if self.manifest.position(name) == Some(0) {
                self.manifest
                    .namespaces
                    .iter()
                    .map(|entry| entry.name.as_str())
                    .collect()
            } else {
                vec![name]
            };
            let base = self.base.clone();
            let url = move |source: &str| crate::source_url(Path::new(source), &base);
            let files = namespace_unit(
                &self.session,
                name,
                &requires,
                &registered,
                units.get(name).map_or(&[][..], Vec::as_slice),
                value,
                &url,
            );
            for file in &files {
                self.write(file).map_err(Failure::Output)?;
            }
            if self.server.is_some() {
                self.units.insert(name.to_owned(), files[0].text.clone());
            }
            let index = self.manifest.position(name).unwrap();
            let entry = &mut self.manifest.namespaces[index];
            entry.requires = requires;
            entry.macros = self.session.declares_macros(name);
            recompiled.push(name.to_owned());
        }
        // What the file's forms observed, and the requires they left, revise
        // the manifest: for each namespace written, and for the namespace
        // itself when its compile failed, which compiles again at the next
        // change, after a resource it looked for appears, say. The session
        // compiles every form as one source, so each gets all of it.
        let mut revised = write
            .iter()
            .map(|&name| name.to_owned())
            .collect::<Vec<_>>();
        if error.is_some() && self.manifest.position(namespace).is_some() {
            revised.push(namespace.to_owned());
        }
        let files = self.provider.files.borrow();
        for name in &revised {
            self.revisions.insert(
                name.clone(),
                Revision {
                    file: files
                        .get(name)
                        .map(|file| file.display().to_string())
                        .unwrap_or_default(),
                    requires: self.session.requires(name),
                    observations: observations.clone(),
                },
            );
        }
        drop(files);
        if let Some((error, diagnostic)) = error {
            return Err(Failure::Source(error, diagnostic));
        }
        // The units of the namespaces that depend on a recompiled one through
        // a macro-provider or helper edge hold expansions that may have
        // changed: compile them again from their sources. A namespace that
        // begins or stops declaring macros, or that loaded new ones, changes
        // those edges at once; the consumers are those before and after.
        let mut consumers = self.consumers(&write);
        let added = write.iter().any(|&name| self.graph.id(name).is_none());
        let marked = always != self.always(namespace);
        if added || marked || macros != self.session.declares_macros(namespace) {
            self.revise();
            consumers.extend(self.consumers(&write));
        }
        // A namespace newly marked `^:dev/always`, or one new to the build,
        // can add to the always-compile set, which this step recompiles too.
        if added || marked {
            consumers.extend(self.always_compile());
        }
        for consumer in consumers {
            if !write.contains(&consumer.as_str())
                && !queue.contains(&consumer)
                && !recompiled.contains(&consumer)
            {
                queue.push(consumer);
            }
        }
        Ok(())
    }

    /// The namespaces that depend on one of `namespaces` through a
    /// macro-provider or helper edge of the manifest, in no order.
    fn consumers(&self, namespaces: &[&str]) -> Vec<String> {
        let mut consumers = Vec::new();
        for id in namespaces.iter().filter_map(|name| self.graph.id(name)) {
            for (consumer, kinds) in self.graph.dependents(id) {
                if kinds.intersects(EdgeKinds::MACRO_PROVIDER | EdgeKinds::HELPER) {
                    consumers.push(self.graph.record(consumer).name.clone());
                }
            }
        }
        consumers
    }

    /// The manifest after the step's revisions: the build's namespaces in
    /// load order, each one the step compiled again as the session knows it.
    fn revise(&mut self) {
        self.manifest.sort();
        if self.revisions.is_empty() {
            return;
        }
        let revisions = std::mem::take(&mut self.revisions);
        let macros = self.session.macro_namespaces();
        let order = self.namespaces().collect::<Vec<_>>();
        let graph = self
            .graph
            .revise(&order, &revisions, |name| macros.contains(name));
        self.graph = graph;
    }

    /// The always-compile set of the policy evaluator, by name: the
    /// namespaces whose `ns` forms the session last compiled mark
    /// `^:dev/always`, and their compilation consumers. A disk bypass
    /// (`^:light-cljs/no-cache`, a cache blocker) adds nothing.
    fn always_compile(&self) -> Vec<String> {
        let policy = cache_policy::evaluate(
            &self.graph,
            &self.settings,
            &self.session.source_metadata(&self.graph),
            &cache_policy::non_cacheable_inputs(&self.graph),
        );
        policy
            .always_compile()
            .iter()
            .map(|&id| self.graph.record(id).name.clone())
            .collect()
    }

    /// Watch the files the manifest's resources were looked for at: keep
    /// the stamps of those watched already, and stamp the others now. One
    /// that the read found otherwise than it is now (it appeared, went, or
    /// its text changed since) is stale, and the next poll reports it.
    fn watch_resources(&mut self) {
        let mut consumers: HashMap<PathBuf, Vec<String>> = HashMap::new();
        let mut recorded: HashMap<PathBuf, (bool, Option<[u8; 32]>)> = HashMap::new();
        for (id, record) in self.graph.namespaces.iter().enumerate() {
            let id = light_cljs::dependency_manifest::NamespaceId(id as u32);
            for input in self.graph.external_inputs(id) {
                let ExternalInput::Resource(resource) = input else {
                    continue;
                };
                for candidate in &resource.candidates {
                    let readers = consumers.entry(candidate.path.clone()).or_default();
                    if !readers.contains(&record.name) {
                        readers.push(record.name.clone());
                    }
                    let found = resource
                        .found
                        .as_ref()
                        .filter(|_| candidate.present)
                        .map(|found| found.sha256);
                    recorded
                        .entry(candidate.path.clone())
                        .or_insert((candidate.present, found));
                }
            }
        }
        let mut resources = BTreeMap::new();
        for (path, (present, sha256)) in recorded {
            let stamp = match self.resources.remove(&path) {
                Some(stamp) => stamp,
                None => {
                    let stamp = stamp(&path);
                    let stale = match (present, stamp.is_some()) {
                        (true, true) => sha256.is_some_and(|sha256| {
                            fs::read(&path)
                                .map_or(true, |bytes| light_cljs::sha256::digest(&bytes) != sha256)
                        }),
                        (false, false) => false,
                        _ => true,
                    };
                    if stale {
                        self.stale_resources.push(path.clone());
                    }
                    stamp
                }
            };
            resources.insert(path, stamp);
        }
        self.resources = resources;
        self.resource_consumers = consumers;
    }

    /// Stamp the watched resource files again: those whose stamps changed,
    /// with the stale ones.
    fn poll_resources(&mut self) -> Vec<PathBuf> {
        let mut changed = std::mem::take(&mut self.stale_resources);
        for (path, before) in &mut self.resources {
            let now = stamp(path);
            if now != *before {
                *before = now;
                if !changed.contains(path) {
                    changed.push(path.clone());
                }
            }
        }
        changed
    }

    /// A chain of requires from `namespace` back to itself, as the session
    /// knows them now, if there is one.
    fn cycle(&self, namespace: &str) -> Option<Vec<String>> {
        fn visit(
            session: &DevSession,
            from: &str,
            target: &str,
            path: &mut Vec<String>,
            seen: &mut HashSet<String>,
        ) -> bool {
            for required in session.requires(from) {
                if required == target {
                    path.push(required);
                    return true;
                }
                if seen.insert(required.clone()) {
                    path.push(required.clone());
                    if visit(session, &required, target, path, seen) {
                        return true;
                    }
                    path.pop();
                }
            }
            false
        }
        let mut path = vec![namespace.to_owned()];
        visit(
            &self.session,
            namespace,
            namespace,
            &mut path,
            &mut HashSet::new(),
        )
        .then_some(path)
    }

    /// The namespaces of the build, in load order.
    fn namespaces(&self) -> impl Iterator<Item = &str> {
        self.manifest
            .namespaces
            .iter()
            .map(|namespace| namespace.name.as_str())
    }

    /// Find the hook functions in the session's retained program again, for
    /// the manifest and the server's loads.
    fn refresh_hooks(&mut self) {
        let names = self.namespaces().map(str::to_owned).collect::<Vec<_>>();
        [self.manifest.before_load, self.manifest.after_load] = self.session.hooks(&names);
    }

    /// Write `manifest.json` and `index.js` where they changed.
    fn write_manifest(&mut self) -> Result<()> {
        for (at, name, text) in [
            (0, "manifest.json", self.manifest.to_json()),
            (1, "index.js", self.manifest.index()),
        ] {
            if text != self.written[at] {
                self.write(&DevFile {
                    name: name.into(),
                    text: text.clone(),
                })?;
                self.written[at] = text;
            }
        }
        Ok(())
    }

    /// Whether the `ns` form of `namespace` marks it `^:dev/once`.
    fn once(&self, namespace: &str) -> bool {
        self.session.namespace_flag(namespace, "once")
    }

    /// Whether the `ns` form of `namespace` marks it `^:dev/always`.
    fn always(&self, namespace: &str) -> bool {
        self.session.namespace_flag(namespace, "always")
    }

    /// The namespaces a step that succeeded and wrote the units of `written`
    /// loads into the runtimes, in load order (docs/dev-workflow.md, "The
    /// reload set"), none when it wrote none: each one written, which with a
    /// changed namespace are the requirers of a changed macro namespace and
    /// the namespaces a changed `ns` form loaded, and every namespace marked
    /// `^:dev/always`, as it is; with `--reload-dependents`, also every
    /// namespace that requires one of those, directly or through others the
    /// set reloads, as shadow-cljs's `:full` strategy loads. Never a
    /// namespace marked `^:dev/once` that the runtimes have been sent, which
    /// passes no change on downstream; one that a step adds to the build
    /// loads with that step.
    fn reload_set(&self, written: &[String]) -> Vec<String> {
        if written.is_empty() {
            return Vec::new();
        }
        let sent_once =
            |namespace: &str| self.published.contains(namespace) && self.once(namespace);
        let mut reload = written
            .iter()
            .map(String::as_str)
            .chain(self.namespaces().filter(|namespace| self.always(namespace)))
            .filter(|namespace| !sent_once(namespace))
            .collect::<HashSet<_>>();
        if self.reload_dependents {
            // Every namespace that requires one in the set joins it, until
            // none does; a `^:dev/once` one neither joins nor passes the
            // change on, as shadow-cljs's never-load list leaves it out of
            // the walk.
            let mut grown = true;
            while grown {
                grown = false;
                for entry in &self.manifest.namespaces {
                    if !reload.contains(entry.name.as_str())
                        && !sent_once(&entry.name)
                        && entry
                            .requires
                            .iter()
                            .any(|required| reload.contains(required.as_str()))
                    {
                        reload.insert(&entry.name);
                        grown = true;
                    }
                }
            }
        }
        self.namespaces()
            .filter(|namespace| reload.contains(namespace))
            .map(str::to_owned)
            .collect()
    }

    /// The unit of `namespace` as a `load` message carries it.
    fn load_unit(&self, namespace: &str) -> Option<LoadUnit> {
        Some(LoadUnit {
            namespace: namespace.to_owned(),
            file: unit_file(namespace),
            js: self.units.get(namespace)?.clone(),
            requires: self
                .manifest
                .position(namespace)
                .map(|at| self.manifest.namespaces[at].requires.clone())
                .unwrap_or_default(),
        })
    }

    /// The build as the server keeps it for runtimes that connect later.
    fn build(&self) -> Build {
        Build {
            units: self
                .manifest
                .namespaces
                .iter()
                .filter_map(|namespace| self.load_unit(&namespace.name))
                .collect(),
            before_load: self.manifest.before_load.clone(),
            after_load: self.manifest.after_load.clone(),
            once: self
                .namespaces()
                .filter(|namespace| self.once(namespace))
                .map(str::to_owned)
                .collect(),
            always: self
                .namespaces()
                .filter(|namespace| self.always(namespace))
                .map(str::to_owned)
                .collect(),
            dependents: self.reload_dependents,
        }
    }

    /// Write `file` into the build directory, replacing the previous one at
    /// once: a reader sees the old text or the new, never a part.
    fn write(&self, file: &DevFile) -> Result<()> {
        let path = self.out_dir.join(&file.name);
        let temporary = self.out_dir.join(format!(".{}.tmp", file.name));
        fs::write(&temporary, &file.text)
            .and_then(|()| fs::rename(&temporary, &path))
            .map_err(|error| format!("cannot write {}: {error}", path.display()).into())
    }
}
