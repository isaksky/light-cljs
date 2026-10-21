//! `--watch` (docs/dev-workflow.md, "The watch loop"): write the dev build of
//! `--reloadable`, keep a `DevSession` that has loaded the same program,
//! then poll the source roots and rewrite the unit of each namespace whose
//! file changed, compiled against the session's retained program.
//!
//! With `--no-cache` the startup build runs with the native engine and
//! the session then loads the program again, as a `require` of the entry.
//! Otherwise (the default cache directory or `--cache-dir`) the startup
//! build runs once, in the session's host and engine,
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
//!
//! Each build runs on a thread of its own, which owns its session, macro
//! engine and retained program (a V8 isolate never crosses threads); the
//! thread that started the watch is the poller, which scans the source roots
//! (and `light-cljs.edn` for `light-cljs watch ID ...`) and sends every
//! build the files that changed, each build's manifest deciding which are
//! its own (docs/build-configuration.md, "Several builds in one process").
//! `--watch` is one such build, at `/` of its server.
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::cache_policy::{self, CacheSettings};
use light_cljs::compile_stats::CompileStats;
use light_cljs::compiler_data::Name;
use light_cljs::dependency_manifest::{
    DependencyManifest, EdgeKinds, ExternalInput, Revision, SourceOrigin,
};
use light_cljs::dev_build::{DevFile, DevManifest, DevNamespace, namespace_unit, unit_file};
use light_cljs::dev_repl::ReplRequest;
use light_cljs::dev_server::{
    Build, DevMount, DevServer, Diagnostic, Listen, LoadUnit, Mount, MountId, Step,
};
use light_cljs::dev_session::{Action, DevSession, EntryStart, Unit};
use light_cljs::source_provider::{
    FileSourceProvider, LoadError, LoadRequest, Reload, SourceInput, SourceProvider,
};
use light_cljs::{EvalOptions, LineIndex, MacroHost};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    error::Error,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        mpsc::{Receiver, RecvTimeoutError, Sender, channel},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub const USAGE: &str = "Watch:
  light-cljs --watch --entry NAMESPACE --source-root DIR [--source-root DIR ...] --out-dir DIR
             [--poll-ms N] [--elide-asserts] [--stats] [--config KEY=EDN ...]
             [--macro-expansions N] [--macro-nodes N] [--macro-timeout-ms N]
             [--macro-memory-limit N] [--macro-poll-budget N]
             [--cache-dir DIR | --no-cache] [--cache-blocker NS ...] [--cache-retention N]
             [--serve PORT [--serve-host HOST] [--reload-dependents] [--nrepl PORT | --no-nrepl]]

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
program as time.session-load-ms, and after each step's line a line
light-cljs: live artifacts=N engines=N engine-handles=N bridge-payloads=N
value-roots=N with what the process still holds once the session's engines
have collected their garbage, and the values the session's macro engine
roots for the compiler.

With --macro-expansions N, --macro-nodes N, --macro-timeout-ms N,
--macro-memory-limit N and --macro-poll-budget N, raise the macro limits as
--compile does (defaults 1024, 100000, 10000, 67108864 and 1000000), for the
startup build and for the watch's session: its load of the program at
startup is one operation under the deadline and poll budget, and in a step
so is each namespace compiled again. As with --compile, macros see stub JavaScript packages, which only the
application has.

With --config KEY=EDN, repeatable, give light.build/config the value EDN for
the keyword KEY in the build and in every step, as --compile does.

Start from the persistent compiler cache that --compile keeps, in
.light-cljs/cache under the current directory or in DIR with --cache-dir
DIR: the startup build runs once, in the engine
of the watch's session, installing each namespace whose artifact is current
instead of analyzing it (its macros do not run) and storing the others, and
the session starts from that build, compiling later steps as if it had
loaded every namespace itself. Steps neither read nor write the cache, so a
restart analyzes what changed since the cache was last filled. Deleting the
directory does not change a running watch. With --no-cache the start is a
fresh uncached one: the startup build runs with the native engine and the
session loads the program again. With --stats, the report adds the cache.*
lines.

With --cache-blocker NS, repeatable, keep NS and every namespace whose
compilation depends on it out of the persistent compiler cache, as --compile
does. A blocker never makes a step compile a namespace whose retained unit
is still valid.

With --cache-retention N, a positive integer, keep N artifacts per namespace
(three by default) when the startup build trims the cache, as --compile
does.

With --serve PORT (0 for any free port), also serve the build over HTTP on
127.0.0.1, or on HOST with --serve-host (0.0.0.0 opens it to the network;
a connected page runs what the server sends it): the files of DIR and of
the source roots, a built-in page at / when DIR has no index.html, and at
/events a stream of server-sent events that pushes each step's units, which
the build's client.js loads into the page between the ^:dev/before-load and
^:dev/after-load hooks, and its warnings and errors, which it draws over
the page. A page that connects late catches up with the build. The server
also answers light-cljs --repl --connect 127.0.0.1:PORT, whose forms it
compiles with the program the watch keeps and runs in a connected runtime,
and nREPL clients (editors), whose sessions are such REPLs: the operations
clone, close, describe, eval, load-file, interrupt, completions, lookup and
ls-sessions, completions and lookup answered from the program's definitions.

With --nrepl PORT (0 for any free port), also answer nREPL on PORT, on the
host of the server (PORT may be the --serve PORT), and write the port into
.nrepl-port in the working directory, where editors look for it, removing
the file when the watch ends. With --no-nrepl, answer no nREPL client on
the server's port either.

A step loads the namespaces it recompiled, which include those that use a
changed macro and the namespaces marked ^:dev/always, in load order; never
again a namespace marked ^:dev/once. With
--reload-dependents, it also loads every namespace that depends on one it
loads, directly or not, as shadow-cljs's :full reload strategy does.";

/// How the server binds its port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PortRule {
    /// That port or fail: `--serve PORT`.
    Exact,
    /// When the port is in use, the next free one upward: `:http {:port N}`
    /// of `light-cljs watch ID`, as shadow-cljs's server does.
    NextFree,
    /// That port or fail, as `:http {:strict true}` says.
    Strict,
}

/// The options of `--watch`, and of `light-cljs watch ID`, which
/// translates a build of `light-cljs.edn` into them (`crate::config_cli`).
pub(crate) struct Options {
    pub(crate) entry: String,
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) out_dir: PathBuf,
    pub(crate) poll: Duration,
    pub(crate) assertions: bool,
    /// `--serve PORT`.
    pub(crate) serve: Option<u16>,
    /// How the server binds `serve`.
    pub(crate) port_rule: PortRule,
    /// `--serve-host HOST`.
    pub(crate) serve_host: Option<String>,
    /// `--reload-dependents`.
    pub(crate) reload_dependents: bool,
    /// `--nrepl PORT`.
    pub(crate) nrepl: Option<u16>,
    /// `--no-nrepl`: the server's port answers no nREPL client.
    pub(crate) no_nrepl: bool,
    /// Where `.nrepl-port` is written: the working directory, or the
    /// project directory of `light-cljs watch ID`.
    pub(crate) nrepl_port_dir: PathBuf,
    /// `light-cljs watch ID`'s `<:cache-root>/http.port`, which holds the
    /// server's port while the watch runs; none for `--watch`.
    pub(crate) http_port_file: Option<PathBuf>,
    /// The build id of `light-cljs watch ID`, the server's mount name, which
    /// serves the build under `/ID/` and at `/`; none for `--watch`, whose
    /// one mount is at `/` alone.
    pub(crate) mount: Option<String>,
    /// `--stats`: report the startup build (`light_cljs::compile_stats`).
    pub(crate) stats: bool,
    /// `--config KEY=EDN`: what `light.build/config` answers.
    pub(crate) configuration: light_cljs::BuildConfiguration,
    /// `--cache-blocker NS`, for the policy evaluator and the cache.
    pub(crate) blockers: Vec<String>,
    /// Where the persistent compiler cache the watch starts from is: the
    /// default directory or `--cache-dir DIR`, and `None` with `--no-cache`.
    pub(crate) cache_dir: Option<PathBuf>,
    /// `--cache-retention N`: the artifacts the startup build's trim keeps
    /// per namespace; three when `None`.
    pub(crate) cache_retention: Option<usize>,
    /// `--macro-expansions N` and the other macro limit flags, for the
    /// startup build and the session.
    pub(crate) macro_limits: crate::MacroLimitFlags,
}

/// The configuration file of `light-cljs watch ID ...` in the watch's poll
/// (docs/build-configuration.md, "Watching the configuration file").
pub(crate) struct ConfigWatch {
    /// `light-cljs.edn`, whose stamp the poll compares.
    pub(crate) path: PathBuf,
    /// Read the file again, once the poll finds its stamp changed, and
    /// compare each build's settings with those it runs with
    /// (`crate::config_cli`), reporting on stderr what a restart does not
    /// apply.
    pub(crate) reread: Box<dyn FnMut() -> Reread>,
}

/// What reading the configuration file again found.
pub(crate) enum Reread {
    /// Its text, or the builds' settings in it, did not change: a comment,
    /// a section of a build the watch does not run, or only a setting the
    /// running server holds.
    Unchanged,
    /// It is invalid: the error as printed, and located for the overlays.
    /// Every build keeps running with its settings.
    Invalid(String, Diagnostic),
    /// The settings of these builds, by their index in the watch's builds,
    /// changed: each starts again with its options, of which those of the
    /// server and the poll are not used.
    Restart(Vec<(usize, Box<Options>)>),
}

/// An error that ends the watch, with the index of the build whose it is;
/// none for one of the server or the poll.
pub(crate) type WatchError = (Option<usize>, Box<dyn Error>);

/// The port of `--serve` or `--nrepl`, 0 to 65535.
pub(crate) fn port_value(flag: &str, text: &std::ffi::OsStr) -> Result<u16> {
    text.to_str()
        .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|text| text.parse::<u16>().ok())
        .ok_or_else(|| format!("{flag} requires a port number, 0 to 65535").into())
}

/// The milliseconds of `--poll-ms`, a positive integer.
pub(crate) fn poll_value(text: &std::ffi::OsStr) -> Result<u64> {
    text.to_str()
        .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|&millis| millis > 0)
        .ok_or_else(|| "--poll-ms requires a positive integer".into())
}

fn options(mut args: impl Iterator<Item = OsString>) -> Result<Option<Options>> {
    let (mut entry, mut roots, mut out_dir) = (None, Vec::new(), None);
    let mut poll = Duration::from_millis(100);
    let mut assertions = true;
    let (mut serve, mut serve_host, mut reload_dependents) = (None, None, false);
    let (mut nrepl, mut no_nrepl) = (None, false);
    let mut stats = false;
    let mut configuration = light_cljs::BuildConfiguration::new();
    let mut blockers = Vec::new();
    let mut cache_dir = None;
    let mut no_cache = false;
    let mut cache_retention = None;
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
            "--poll-ms" => poll = Duration::from_millis(poll_value(&value()?)?),
            "--elide-asserts" => assertions = false,
            "--serve" => serve = Some(port_value(flag, &value()?)?),
            "--serve-host" => {
                serve_host = Some(
                    value()?
                        .into_string()
                        .map_err(|_| "--serve-host must be valid UTF-8")?,
                );
            }
            "--reload-dependents" => reload_dependents = true,
            "--nrepl" => nrepl = Some(port_value(flag, &value()?)?),
            "--no-nrepl" => no_nrepl = true,
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
            "--no-cache" => no_cache = true,
            "--cache-retention" => cache_retention = Some(crate::cache_retention(&mut args)?),
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
    if nrepl.is_some() && serve.is_none() {
        return Err("--nrepl requires --serve PORT".into());
    }
    if no_nrepl && serve.is_none() {
        return Err("--no-nrepl requires --serve PORT".into());
    }
    if no_nrepl && nrepl.is_some() {
        return Err("--nrepl and --no-nrepl exclude each other".into());
    }
    if no_cache && cache_dir.is_some() {
        return Err("--no-cache and --cache-dir exclude each other".into());
    }
    let cache_dir = (!no_cache).then(|| {
        cache_dir.unwrap_or_else(|| PathBuf::from(light_cljs::compile_cache::DEFAULT_DIR))
    });
    Ok(Some(Options {
        entry,
        roots,
        out_dir,
        poll,
        assertions,
        serve,
        port_rule: PortRule::Exact,
        serve_host,
        reload_dependents,
        nrepl,
        no_nrepl,
        nrepl_port_dir: PathBuf::from("."),
        http_port_file: None,
        mount: None,
        stats,
        configuration,
        blockers,
        cache_dir,
        cache_retention,
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
    /// The build's mount of the dev server of `--serve`.
    server: Option<DevMount>,
    /// `[:ID] ` for a build of `light-cljs.edn`, which begins its lines on
    /// stderr and its messages to the overlay; empty for `--watch`.
    tag: String,
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
    /// `--stats`: each step also prints what the process still holds
    /// (`light_cljs::compile_stats::live_counts`).
    stats: bool,
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
    run_builds(vec![options], None).map_err(|(_, error)| error)
}

/// `[:ID] ` for a build of `light-cljs.edn`, which begins its lines on stderr
/// and its messages to the overlay, as shadow-cljs's `[:app] Build
/// completed.` names the build; empty for `--watch`.
fn tag(options: &Options) -> String {
    options
        .mount
        .as_ref()
        .map_or_else(String::new, |name| format!("[:{name}] "))
}

/// How a build's status lines begin: its tag, or `light-cljs: ` for
/// `--watch`.
fn lead(tag: &str) -> &str {
    if tag.is_empty() { "light-cljs: " } else { tag }
}

/// `diagnostic` with its message after `tag`, for the overlay.
fn tagged(tag: &str, mut diagnostic: Diagnostic) -> Diagnostic {
    if !tag.is_empty() {
        diagnostic.message = format!("{tag}{}", diagnostic.message);
    }
    diagnostic
}

/// The source roots of `options`, canonical: they name files as the source
/// provider does.
fn canonical_roots(options: &Options) -> Result<Vec<PathBuf>> {
    let roots = options
        .roots
        .iter()
        .map(|root| {
            fs::canonicalize(root)
                .map_err(|error| format!("cannot read source root {}: {error}", root.display()))
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(roots)
}

/// Make the startup build `options` say, from the canonical source roots
/// `roots`, and the session that starts from it: the watch's state, with the
/// text of each unit when the watch serves, and the build's warnings located
/// for the server.
fn start(
    options: &Options,
    roots: Vec<PathBuf>,
    begun: Instant,
) -> Result<(Watch, Vec<Diagnostic>)> {
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
            retention: crate::retention(options.cache_retention),
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
    let tag = tag(options);
    crate::print_tagged_warnings(&tag, &warnings);
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
        tag,
        units: HashMap::new(),
        unpushed: Vec::new(),
        warnings: Vec::new(),
        reload_dependents: options.reload_dependents,
        stats: options.stats,
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
    if options.serve.is_some() {
        for namespace in &watch.manifest.namespaces {
            let path = options.out_dir.join(&namespace.file);
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            watch.units.insert(namespace.name.clone(), text);
        }
    }
    Ok((watch, startup))
}

/// What the poller sends a build's thread.
enum Work {
    /// The source files that changed (or appeared) and went since the last
    /// poll, which noticed them at `noticed`. Each build's manifest decides
    /// which are its own.
    Changes {
        changed: Vec<PathBuf>,
        deleted: Vec<PathBuf>,
        noticed: Instant,
    },
    /// The configuration file was saved and the save did not restart the
    /// build: a build that a failed restart left waiting starts again.
    Touched,
    /// The build's settings changed: it starts again with these options.
    Restart(Box<Options>),
    /// The server started: serve the build on mount `MountId` of it.
    Attach(Arc<DevServer>, MountId),
    /// A form of a connected REPL, for the session of the build's start
    /// numbered `u64`: one for a session that ended is dropped unanswered,
    /// as a session's requests were when it ended.
    Repl(u64, ReplRequest),
}

/// What a build's thread tells the poller while the watch starts.
enum Report {
    /// The startup build is made: the build a runtime catches up with.
    Started(Build),
    /// The build of this index is served, when the watch serves: its
    /// `built` line.
    Ready(usize, String),
}

/// A build's thread, as the poller keeps it.
struct Worker {
    work: Sender<Work>,
    /// Taken once the thread has ended.
    thread: Option<JoinHandle<std::result::Result<(), String>>>,
}

/// How deep a build's thread analyzes: as deep as `--compile` on the main
/// thread, which has 8 MiB.
const BUILD_STACK: usize = 8 << 20;

/// How often the poller looks for a build's thread that ended while the
/// watch starts.
const STARTUP_WAKE: Duration = Duration::from_millis(20);

/// Build, serve and watch each of `builds` (one for `--watch`; those named
/// for `light-cljs watch ID ...`, docs/build-configuration.md, "Several
/// builds in one process"), until interrupted, or a startup build fails, or
/// a step cannot write its build.
///
/// Each build runs on a thread of its own, which owns its session, macro
/// engine and retained program. The startup builds run one after another,
/// in the order given, so the peak is one analysis beside the programs
/// already made; then one server serves every build, the first at `/` (and
/// under `/ID/`, as each build of the file is), and every build steps on
/// its own. The calling thread is the poller: it scans the source roots,
/// which every build shares, every `poll` of the first build, and sends
/// every build the files that changed. With `config`, each poll also reads
/// `light-cljs.edn` again when it was saved, and a build whose settings
/// changed starts again in this process, on its own thread, with the same
/// server, port files and poll interval (docs/build-configuration.md,
/// "Watching the configuration file").
pub(crate) fn run_builds(
    builds: Vec<Options>,
    mut config: Option<ConfigWatch>,
) -> std::result::Result<(), WatchError> {
    let begun = Instant::now();
    let general = |error: Box<dyn Error>| (None, error);
    let first = builds
        .first()
        .ok_or_else(|| general("a watch runs at least one build".into()))?;
    // The server's and the poll's settings are the first build's: the
    // file's top level and the flags, the same for every build.
    let poll = first.poll;
    let serve = first.serve;
    let port_rule = first.port_rule;
    let host = first
        .serve_host
        .clone()
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    let (nrepl, no_nrepl) = (first.nrepl, first.no_nrepl);
    let nrepl_port_dir = first.nrepl_port_dir.clone();
    let http_port_file = first.http_port_file.clone();
    let named = first.mount.is_some();
    // The stamps come first, so a file saved while a build is made is a
    // change.
    let mut roots = canonical_roots(first).map_err(general)?;
    let mut stamps = scan(&roots);
    // Restarts, too, make one analysis at a time, in the order named.
    let starts = Arc::new(Starts::default());
    let (report, reports) = channel();
    let mut workers = Vec::with_capacity(builds.len());
    // Each build is ready once it serves, or at once without a server.
    let mut lines = vec![None; builds.len()];
    let mut mounts = Vec::with_capacity(builds.len());
    for (index, options) in builds.into_iter().enumerate() {
        let (work, received) = channel();
        let out_dir = options.out_dir.clone();
        let name = options.mount.clone().unwrap_or_default();
        let thread = {
            let (roots, report, work, starts) =
                (roots.clone(), report.clone(), work.clone(), starts.clone());
            // The first build's time counts from the process's start, as a
            // single build's always has.
            let begun = if index == 0 { begun } else { Instant::now() };
            std::thread::Builder::new()
                .name(format!("light-cljs-build-{}", index + 1))
                .stack_size(BUILD_STACK)
                .spawn(move || {
                    let thread = BuildThread {
                        index,
                        options,
                        serving: serve.is_some(),
                        poll,
                        work,
                        report,
                        starts,
                    };
                    thread.run(roots, begun, received)
                })
                .map_err(|error| general(error.into()))?
        };
        workers.push(Worker {
            work,
            thread: Some(thread),
        });
        // The next build starts once this one is made.
        let build = loop {
            match reports.recv_timeout(STARTUP_WAKE) {
                Ok(Report::Started(build)) => break build,
                Ok(Report::Ready(at, line)) => lines[at] = Some(line),
                Err(_) => ended(&mut workers)?,
            }
        };
        mounts.push(Mount {
            name,
            out_dir,
            build,
        });
    }
    // Removes .nrepl-port when the watch ends.
    let mut _port_file = None;
    // Removes `<:cache-root>/http.port` when the watch ends.
    let mut _http_port_file = None;
    let mut server = None;
    if let Some(port) = serve {
        let listen = Listen {
            next_free: port_rule == PortRule::NextFree,
            nrepl: !no_nrepl,
            ..Listen::new(&host, port)
        };
        // As shadow-cljs's server says of each port in use it passes over.
        let mut taken = |port: u16| {
            eprintln!(
                "light-cljs: TCP port {port} in use, trying {}",
                port as u32 + 1
            )
        };
        let directories = mounts
            .iter()
            .map(|mount| mount.out_dir.clone())
            .collect::<Vec<_>>();
        let served = if named {
            DevServer::start_mounts_with(listen, mounts, &roots, &mut taken)
        } else {
            let mount = mounts.pop().expect("one build");
            DevServer::start_with(listen, &mount.out_dir, &roots, mount.build, &mut taken)
        };
        let served = Arc::new(served.map_err(|error| {
            general(
                match port_rule {
                    PortRule::Strict => format!(
                        "cannot serve on {host}:{port}: {error}; :http {{:strict true}} tries no \
                         other port"
                    ),
                    PortRule::Exact | PortRule::NextFree => {
                        format!("cannot serve on {host}:{port}: {error}")
                    }
                }
                .into(),
            )
        })?);
        if let Some(path) = &http_port_file {
            // As shadow-cljs writes `.shadow-cljs/http.port`, creating the
            // cache root when it is missing.
            let file = path
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| {
                    light_cljs::dev_nrepl::PortFile::write(path, served.address().port())
                })
                .map_err(|error| {
                    general(format!("cannot write {}: {error}", path.display()).into())
                })?;
            _http_port_file = Some(file);
        }
        // The first build at `/`, and each other one under its name.
        for (index, (directory, mount)) in directories.iter().zip(served.mounts()).enumerate() {
            let under = match mount.name().filter(|_| index > 0) {
                Some(name) => format!("{name}/"),
                None => String::new(),
            };
            eprintln!(
                "light-cljs: serving {} at http://{}/{under}",
                directory.display(),
                served.address()
            );
        }
        for index in 0..workers.len() {
            send(
                &mut workers,
                index,
                Work::Attach(served.clone(), MountId(index as u32)),
            )?;
        }
        server = Some(served);
    }
    // The nREPL listener comes after the builds' REPLs, and the builds'
    // lines after both.
    while lines.iter().any(Option::is_none) {
        match reports.recv_timeout(STARTUP_WAKE) {
            Ok(Report::Ready(at, line)) => lines[at] = Some(line),
            Ok(Report::Started(_)) => unreachable!("every build has started"),
            Err(_) => ended(&mut workers)?,
        }
    }
    if let (Some(server), Some(nrepl)) = (&server, nrepl) {
        let address = if nrepl != 0 && nrepl == server.address().port() {
            server.address()
        } else {
            server.serve_nrepl(&host, nrepl).map_err(|error| {
                general(format!("cannot serve nREPL on {host}:{nrepl}: {error}").into())
            })?
        };
        let file = light_cljs::dev_nrepl::PortFile::create(&nrepl_port_dir, address.port())
            .map_err(|error| general(format!("cannot write .nrepl-port: {error}").into()))?;
        eprintln!(
            "light-cljs: nREPL at {address}, its port in {}",
            file.path().display()
        );
        _port_file = Some(file);
    }
    for line in lines.into_iter().flatten() {
        eprintln!("{line}");
    }
    // The configuration file's stamp at the last poll; none before the first
    // poll, which reads the file in case it was saved since it was read.
    let mut config_stamp = None;
    loop {
        std::thread::sleep(poll);
        ended(&mut workers)?;
        if let Some(config) = &mut config {
            let now = stamp(&config.path);
            if config_stamp != Some(now) {
                config_stamp = Some(now);
                // The builds this save restarts, which need no touch: a
                // touch that came after a restart failed would start it
                // again for the same save.
                let mut restarted = vec![false; workers.len()];
                match (config.reread)() {
                    Reread::Unchanged => {}
                    Reread::Invalid(printed, diagnostic) => {
                        eprintln!("light-cljs: {printed}");
                        for mount in server.iter().flat_map(|server| server.mounts()) {
                            mount.publish(Step {
                                error: Some(diagnostic.clone()),
                                ..Step::default()
                            });
                        }
                    }
                    Reread::Restart(restarts) => {
                        // A restart that moves the source roots, which every
                        // build shares, moves the poll, stamped before the
                        // build reads them.
                        if let Some(next) = restarts
                            .first()
                            .and_then(|(_, options)| canonical_roots(options).ok())
                            .filter(|next| *next != roots)
                        {
                            roots = next;
                            stamps = scan(&roots);
                        }
                        for &(index, _) in &restarts {
                            starts.enqueue(index);
                            restarted[index] = true;
                        }
                        for (index, options) in restarts {
                            send(&mut workers, index, Work::Restart(options))?;
                        }
                    }
                }
                for (index, restarted) in restarted.into_iter().enumerate() {
                    if !restarted {
                        send(&mut workers, index, Work::Touched)?;
                    }
                }
            }
        }
        let now = scan(&roots);
        if now == stamps {
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
        for index in 0..workers.len() {
            let changes = Work::Changes {
                changed: changed.clone(),
                deleted: deleted.clone(),
                noticed,
            };
            send(&mut workers, index, changes)?;
        }
    }
}

/// The error of the first build whose thread ended, which ends the watch: a
/// startup build that failed or a step that could not write its build. A
/// thread that panicked panics the poller with its payload.
fn ended(workers: &mut [Worker]) -> std::result::Result<(), WatchError> {
    for (index, worker) in workers.iter_mut().enumerate() {
        if worker.thread.as_ref().is_some_and(JoinHandle::is_finished) {
            let thread = worker.thread.take().unwrap();
            match thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err((Some(index), error.into())),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
    }
    Ok(())
}

/// Send build `index` `work`; when its thread has ended, its error.
fn send(workers: &mut [Worker], index: usize, work: Work) -> std::result::Result<(), WatchError> {
    if workers[index].work.send(work).is_ok() {
        return Ok(());
    }
    if let Some(thread) = workers[index].thread.take() {
        match thread.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err((Some(index), error.into())),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    Err((Some(index), "the build's thread ended".into()))
}

/// The builds waiting to start, in the order they start: one at a time, so
/// that the peak is one analysis beside the programs of the others, and in
/// the order the poller asked for them, the order named.
#[derive(Default)]
struct Starts {
    queue: Mutex<VecDeque<usize>>,
    turn: Condvar,
}

impl Starts {
    /// Put build `index` in the queue, unless it is in it.
    fn enqueue(&self, index: usize) {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if !queue.contains(&index) {
            queue.push_back(index);
        }
    }

    /// Start build `index` with `start` once the builds ahead of it in the
    /// queue have started.
    fn run<T>(&self, index: usize, start: impl FnOnce() -> T) -> T {
        self.enqueue(index);
        let queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        let queue = self
            .turn
            .wait_while(queue, |queue| queue.front() != Some(&index))
            .unwrap_or_else(PoisonError::into_inner);
        drop(queue);
        let started = start();
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        self.turn.notify_all();
        started
    }
}

/// A build's thread: what it keeps besides its watch.
struct BuildThread {
    /// Its index in the watch's builds.
    index: usize,
    options: Options,
    /// Whether the watch serves: the thread waits for its mount once its
    /// startup build is made.
    serving: bool,
    /// The poll interval the watch started with, at which the thread stamps
    /// its build's resource files.
    poll: Duration,
    /// Its own work, for the REPL's forms.
    work: Sender<Work>,
    report: Sender<Report>,
    starts: Arc<Starts>,
}

impl BuildThread {
    /// Start the build from the canonical source roots `roots`, then step it
    /// as `received` says until the poller is gone: the error that ends the
    /// watch, if any.
    fn run(
        mut self,
        roots: Vec<PathBuf>,
        begun: Instant,
        received: Receiver<Work>,
    ) -> std::result::Result<(), String> {
        let text = |error: Box<dyn Error>| error.to_string();
        let tag = tag(&self.options);
        let (watch, startup) = self
            .starts
            .run(self.index, || start(&self.options, roots, begun))
            .map_err(text)?;
        let _ = self.report.send(Report::Started(watch.build()));
        let mut current = Some(watch);
        // The server and the build's mount on it.
        let mut mount: Option<(Arc<DevServer>, MountId)> = None;
        // Which start of the build the REPL's forms are for.
        let mut generation = 0;
        if self.serving {
            let Ok(Work::Attach(server, id)) = received.recv() else {
                return Ok(());
            };
            let watch = current.as_mut().unwrap();
            let served = server.mount(id);
            if !startup.is_empty() {
                served.publish(Step {
                    warnings: startup
                        .into_iter()
                        .map(|warning| tagged(&tag, warning))
                        .collect(),
                    ..Step::default()
                });
            }
            self.serve_repl(watch, &served, generation).map_err(text)?;
            watch.server = Some(served);
            mount = Some((server, id));
        }
        let line = built(&self.options, current.as_ref().unwrap(), begun, self.poll);
        let _ = self.report.send(Report::Ready(self.index, line));
        // When the build's resource files are stamped next.
        let mut resources_due = Instant::now() + self.poll;
        loop {
            let mut messages = Vec::new();
            if current.is_some() {
                // Until the resource files are due, or work comes.
                let wait = resources_due.saturating_duration_since(Instant::now());
                match received.recv_timeout(wait) {
                    Ok(message) => messages.push(message),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return Ok(()),
                }
            } else {
                // A build that a failed restart left waiting has no
                // resources to stamp: it waits for the next change.
                match received.recv() {
                    Ok(message) => messages.push(message),
                    Err(_) => return Ok(()),
                }
            }
            messages.extend(received.try_iter());
            // What came since the thread last looked, the changes merged.
            let mut changes: Option<(Vec<PathBuf>, Vec<PathBuf>, Instant)> = None;
            let mut touched = false;
            let mut restart = false;
            for message in messages {
                match message {
                    Work::Repl(of, request) => {
                        if let Some(watch) = current.as_ref().filter(|_| of == generation) {
                            request.answer(&watch.session);
                        }
                    }
                    Work::Changes {
                        changed,
                        deleted,
                        noticed,
                    } => merge(&mut changes, changed, deleted, noticed),
                    Work::Touched => touched = true,
                    Work::Restart(options) => {
                        self.options = *options;
                        restart = true;
                    }
                    Work::Attach(..) => {}
                }
            }
            // A restart is due: the build's settings changed, or a restart
            // failed and a source or the file changed since.
            if restart || current.is_none() && (touched || changes.is_some()) {
                // The session ends before the next one starts, as when the
                // process started; the server stays.
                drop(current.take());
                generation += 1;
                current = self.restart(mount.as_ref(), &tag, generation)?;
                resources_due = Instant::now() + self.poll;
                continue;
            }
            let Some(watch) = &mut current else {
                continue;
            };
            let now = Instant::now();
            let resources = if changes.is_some() || now >= resources_due {
                if now >= resources_due {
                    resources_due = now + self.poll;
                }
                watch.poll_resources()
            } else {
                Vec::new()
            };
            let (changed, deleted, noticed) =
                changes.unwrap_or_else(|| (Vec::new(), Vec::new(), now));
            if changed.is_empty() && deleted.is_empty() && resources.is_empty() {
                continue;
            }
            watch
                .step(&changed, &deleted, &resources, noticed)
                .map_err(text)?;
        }
    }

    /// Start the build again with its options, after its session ended: the
    /// watch, or none when its startup build failed, which the next change
    /// starts again while the server keeps the build that ran last.
    fn restart(
        &self,
        mount: Option<&(Arc<DevServer>, MountId)>,
        tag: &str,
        generation: u64,
    ) -> std::result::Result<Option<Watch>, String> {
        let options = &self.options;
        let started = self.starts.run(self.index, || {
            let begun = Instant::now();
            let started = canonical_roots(options).and_then(|roots| start(options, roots, begun));
            started.map(|started| (started, begun))
        });
        let started = started.map(|((watch, startup), begun)| (watch, startup, begun));
        match started {
            Ok((mut watch, startup, begun)) => {
                if let Some((server, id)) = mount {
                    let startup = startup
                        .into_iter()
                        .map(|warning| tagged(tag, warning))
                        .collect();
                    server
                        .restart(
                            *id,
                            &options.out_dir,
                            &options.roots,
                            watch.build(),
                            startup,
                        )
                        .map_err(|error| {
                            format!("cannot serve {}: {error}", options.out_dir.display())
                        })?;
                    let served = server.mount(*id);
                    self.serve_repl(&watch, &served, generation)
                        .map_err(|error| error.to_string())?;
                    watch.server = Some(served);
                }
                eprintln!("{}", built(options, &watch, begun, self.poll));
                Ok(Some(watch))
            }
            Err(error) => {
                let message = error.to_string();
                eprintln!(
                    "{}{message}; the build starts again at the next change",
                    lead(tag)
                );
                if let Some((server, id)) = mount {
                    server.mount(*id).publish(Step {
                        error: Some(tagged(
                            tag,
                            Diagnostic {
                                message,
                                file: None,
                                line: None,
                                column: None,
                            },
                        )),
                        ..Step::default()
                    });
                }
                Ok(None)
            }
        }
    }

    /// Answer the connected REPL's forms on `mount` with the session of
    /// `watch`, the build's start numbered `generation`: the REPL compiles
    /// in the entry namespace to begin with, and calls the runtime's
    /// `light.dev` functions. Its forms come to the build's thread with the
    /// poller's work.
    fn serve_repl(&self, watch: &Watch, mount: &DevMount, generation: u64) -> Result<()> {
        light_cljs::dev_repl::declare(&watch.session)?;
        let requests = mount.serve_repl(&self.options.entry);
        let work = self.work.clone();
        std::thread::Builder::new()
            .name("light-cljs-repl-forms".to_owned())
            .spawn(move || {
                for request in requests {
                    if work.send(Work::Repl(generation, request)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(())
    }
}

/// Merge the changes of a later poll into those not yet stepped: a file that
/// changed after it went, or went after it changed, is as the later poll
/// found it.
fn merge(
    into: &mut Option<(Vec<PathBuf>, Vec<PathBuf>, Instant)>,
    changed: Vec<PathBuf>,
    deleted: Vec<PathBuf>,
    noticed: Instant,
) {
    let Some((earlier, went, _)) = into else {
        *into = Some((changed, deleted, noticed));
        return;
    };
    for path in changed {
        went.retain(|other| *other != path);
        if !earlier.contains(&path) {
            earlier.push(path);
        }
    }
    for path in deleted {
        earlier.retain(|other| *other != path);
        if !went.contains(&path) {
            went.push(path);
        }
    }
}

/// `light-cljs: built ENTRY (N namespaces) into DIR in N ms; watching every
/// N ms`, once a build has started, with `[:ID] ` for `light-cljs: ` for a
/// build of `light-cljs.edn`.
fn built(options: &Options, watch: &Watch, begun: Instant, poll: Duration) -> String {
    format!(
        "{}built {} ({} namespaces) into {} in {} ms; watching every {} ms",
        lead(&watch.tag),
        options.entry,
        watch.manifest.namespaces.len(),
        options.out_dir.display(),
        begun.elapsed().as_millis(),
        poll.as_millis()
    )
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
                    "{}{} was deleted; {namespace} keeps its unit",
                    lead(&self.tag),
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
                    eprintln!("{}{error}", lead(&self.tag));
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
                warnings: warnings
                    .into_iter()
                    .map(|warning| tagged(&self.tag, warning))
                    .collect(),
                error: failed
                    .as_ref()
                    .map(|(_, diagnostic)| tagged(&self.tag, diagnostic.clone())),
                // A runtime that connects after a failed step catches up
                // with the last step that succeeded, then gets the error.
                build: (failed.is_none() && !recompiled.is_empty()).then(|| self.build()),
            });
        }
        let millis = noticed.elapsed().as_secs_f64() * 1000.0;
        let lead = lead(&self.tag);
        match failed {
            None => eprintln!("{lead}recompiled {names} in {millis:.1} ms"),
            Some((namespace, _)) => eprintln!(
                "{lead}recompiled {names} in {millis:.1} ms; {namespace} failed and keeps its previous unit"
            ),
        }
        if self.stats {
            // What the session's engines can still reach, not what they
            // have yet to collect.
            self.session.collect_garbage();
            let live = light_cljs::compile_stats::live_counts();
            let roots = match self.session.value_roots() {
                Ok(roots) => roots.to_string(),
                Err(error) => crate::eval_cli::diagnostic(&error),
            };
            eprintln!("{lead}live {} value-roots={roots}", live.summary());
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
                    "{}no source of {namespace} under the source roots; it keeps its unit",
                    lead(&self.tag)
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
                        eprintln!(
                            "{}WARNING: {}",
                            self.tag,
                            crate::eval_cli::diagnostic(warning)
                        );
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
