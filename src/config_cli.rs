//! The `compile`, `watch` and `release` commands, which run named builds of
//! `light-cljs.edn` (docs/build-configuration.md, "Commands", "Precedence"
//! and "Several builds in one process").
//!
//! A command finds the configuration file by walking up from the current
//! directory, reads and validates it, takes the build ids that follow the
//! command and the flags after them, merges each build's layers for the
//! command's mode ([`ConfigFile::settings`], layers 1-5), and translates the
//! result into the flags' own structures, [`NamespaceFlags`] and
//! [`CompileFlags`], before the flags after the command write over it
//! (layer 6). Each build then runs as `--compile --entry` runs: `compile` as
//! `--reloadable --out-dir`, `release` as `--out-dir` at `:optimizations`,
//! one after another in the order named, stopping at the first failure.
//! `watch` translates each build into the watch's `Options` instead
//! (`crate::dev_watch`), which runs them in one process, a thread each,
//! with one server always started, as shadow-cljs's is: `:http`, `:nrepl`, `:fs-watch` and `:devtools`, under the watch's own
//! flags.
//!
//! `cljs-repl ID` ([`cljs_repl`]) runs no build: it reads the file for its
//! builds and `:cache-root`, and connects `--repl --connect`'s client to
//! the running watch whose port is in `<:cache-root>/http.port`, bound to
//! build ID.
//!
//! Every relative path, in the file and in the flags after the command,
//! resolves against the project directory, the file's.

use std::{
    error::Error,
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
    time::Instant,
};

use light_cljs::analyzer::{AnalysisOptions, CheckedArrays};
use light_cljs::config_file::{
    BuildId, BuildMode, BuildSettings, ConfigFile, ConfigMerge, FILE_NAME, Setting, Value,
    ValueKind,
};

use crate::{CompileFlags, NamespaceFlags, build_namespace};

/// A command that runs builds of the configuration file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Command {
    /// `compile ID...`: the dev build of each, once (`--reloadable`).
    Compile,
    /// `watch ID...`: the dev build of each, watched and served (`--watch`).
    Watch,
    /// `release ID...`: the release build of each, at `:optimizations`.
    Release,
}

impl Command {
    /// The command a first argument names, if any.
    pub(crate) fn named(arg: &str) -> Option<Self> {
        match arg {
            "compile" => Some(Self::Compile),
            "watch" => Some(Self::Watch),
            "release" => Some(Self::Release),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Compile => "compile",
            Self::Watch => "watch",
            Self::Release => "release",
        }
    }

    fn mode(self) -> BuildMode {
        match self {
            Self::Compile | Self::Watch => BuildMode::Dev,
            Self::Release => BuildMode::Release,
        }
    }

    /// The flag-only command it runs as: for a dev build's messages.
    fn runs_as(self) -> &'static str {
        match self {
            Self::Compile => "--compile --reloadable",
            Self::Watch => "--watch",
            Self::Release => "--compile",
        }
    }

    /// What the command writes, for refusing an output flag it does not take.
    fn writes(self) -> &'static str {
        match self {
            Self::Compile => {
                "compile writes the dev build into :output-dir, as --reloadable does, with a \
                 source map per namespace"
            }
            Self::Watch => {
                "watch writes the dev build into :output-dir, as --watch does, with a source map \
                 per namespace"
            }
            Self::Release => {
                "release writes a chunked build into :output-dir, as --out-dir does: ES modules \
                 without source maps"
            }
        }
    }
}

/// An error of a command, printed as it is, since it names the command:
/// `light-cljs release app: ...`.
#[derive(Debug)]
pub(crate) struct CommandError(String);

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for CommandError {}

/// `light-cljs COMMAND WORDS...: message`.
fn failure(words: &str, message: impl fmt::Display) -> Box<dyn Error> {
    Box::new(CommandError(format!("light-cljs {words}: {message}")))
}

/// Flags of `--watch` alone, which serve and watch the build.
const WATCH_FLAGS: [&str; 6] = [
    "--serve",
    "--serve-host",
    "--nrepl",
    "--no-nrepl",
    "--poll-ms",
    "--reload-dependents",
];

/// Flags `--compile` takes that `--watch` does not: the watch's session
/// compiles on one slot and reports no trace file.
const NOT_WATCH_FLAGS: [&str; 2] = ["--jobs", "--stats-file"];

/// Flags `--compile` takes that `--reloadable` refuses: they shape release
/// output, or, for the checked-arrays flags, no dev build takes them.
const RELEASE_FLAGS: [&str; 6] = [
    "--optimize",
    "--size-report",
    "--pseudo-names",
    "--define",
    "--checked-arrays-warn",
    "--checked-arrays-error",
];

/// The output flags a build from the file refuses ("Not in the file").
const OUTPUT_FLAGS: [&str; 4] = [
    "--esm",
    "--source-map",
    "--source-map-inline",
    "--source-map-include-sources",
];

/// The refused flags that take a value, which the message quotes with it.
const VALUED: [&str; 9] = [
    "--entry",
    "--out-dir",
    "--source-map",
    "--optimize",
    "--define",
    "--serve",
    "--serve-host",
    "--nrepl",
    "--poll-ms",
];

/// The flags after the build ids: layer 6 of "Precedence".
#[derive(Default)]
struct CommandFlags {
    /// `--entry`, `--source-root` and `--out-dir`.
    namespace: NamespaceFlags,
    /// Every flag `--compile` collects.
    compile: CompileFlags,
    /// Whether `--jobs` was given, which [`CompileFlags`] does not record.
    jobs: bool,
    /// `--warnings`.
    warnings: bool,
    /// `--checked-arrays-warn` or `--checked-arrays-error`.
    checked_arrays: Option<CheckedArrays>,
    /// `--config-merge DATA`, in order.
    merges: Vec<String>,
    /// `--print-config`: print each build's configuration instead of building.
    print: bool,
    /// `--serve PORT`, of `watch`.
    serve: Option<u16>,
    /// `--serve-host HOST`, of `watch`.
    serve_host: Option<String>,
    /// `--nrepl PORT`, of `watch`.
    nrepl: Option<u16>,
    /// `--no-nrepl`, of `watch`.
    no_nrepl: bool,
    /// `--poll-ms N`, of `watch`.
    poll_ms: Option<u64>,
    /// `--reload-dependents`, of `watch`.
    reload_dependents: bool,
}

impl CommandFlags {
    /// Whether a flag replaces an output directory, the source roots or the
    /// cache store, which moves the output check after it ("Exclusions are
    /// checked after merging"): the directories compared are those the
    /// build will use.
    fn replaces_directories(&self) -> bool {
        self.namespace.out_dir.is_some()
            || !self.namespace.roots.is_empty()
            || self.compile.cache_dir.is_some()
            || self.compile.no_cache
    }
}

/// Run `command` with the arguments after it.
pub(crate) fn run(
    command: Command,
    args: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    let mut args = args.peekable();
    let mut typed = Vec::new();
    while let Some(arg) = args.next_if(|arg| !arg.to_string_lossy().starts_with('-')) {
        typed.push(arg.to_string_lossy().into_owned());
    }
    let mut words = command.name().to_owned();
    for id in &typed {
        words.push(' ');
        words.push_str(id);
    }
    let words = words.as_str();

    let environment = |name: &str| std::env::var(name).ok();
    let (path, file) = discover(words, &environment)?;
    let names = defined(&file);
    if typed.is_empty() {
        return Err(failure(
            words,
            format!(
                "requires one or more build ids; {} defines {names}",
                path.display()
            ),
        ));
    }
    let mut builds: Vec<BuildId> = Vec::new();
    for id in &typed {
        let build = find_build(&file, &path, id, words)?;
        if !builds.contains(&build) {
            builds.push(build);
        }
    }
    let flags = command_flags(command, builds.len(), args, words)?;
    let merges = flags
        .merges
        .iter()
        .enumerate()
        .map(|(index, data)| {
            ConfigMerge::parse(index + 1, data, file.project_dir(), &environment)
                .map_err(|error| failure(words, error))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let replaced = flags.replaces_directories();
    let settings = builds
        .iter()
        .map(|&build| {
            if replaced {
                file.settings_before_flags(build, command.mode(), &merges)
            } else {
                file.settings(build, command.mode(), &merges)
            }
            .map_err(|error| failure(words, error))
        })
        .collect::<Result<Vec<_>, _>>()?;
    file.check_outputs(&settings, &merges)
        .map_err(|error| failure(words, error))?;
    let translated = settings
        .iter()
        .map(|settings| {
            if replaced {
                check_directories(&file, settings, &merges, &flags)
                    .map_err(|error| failure(words, error))?;
            }
            translate(&file, settings, command, &flags).map_err(|error| failure(words, error))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // `--print-config`: every check above has passed, and nothing compiles,
    // writes or opens the cache.
    if flags.print {
        let mut out = String::new();
        for (index, settings) in settings.into_iter().enumerate() {
            let mut settings = settings;
            apply_flags(&file, &mut settings, &merges, command, &flags, true)
                .map_err(|error| failure(words, error))?;
            if index > 0 {
                out.push('\n');
            }
            out.push_str(&file.print_config(&settings, &merges, command == Command::Watch));
        }
        print!("{out}");
        return Ok(());
    }

    eprintln!("light-cljs - config: {}", path.display());
    if command == Command::Watch {
        // Every build named, in that order, in one process: one thread
        // each, one poller, one server (docs/build-configuration.md,
        // "Several builds in one process").
        let mut builds = Vec::with_capacity(settings.len());
        let mut names = Vec::with_capacity(settings.len());
        let mut running = Vec::with_capacity(settings.len());
        for (settings, (namespace, _, compile)) in settings.iter().zip(translated) {
            builds.push(
                watch_options(&file, settings, namespace, compile, &flags)
                    .map_err(|error| failure(words, error))?,
            );
            names.push(settings.name().to_owned());
            running.push(effective(&file, settings, &merges, &flags));
        }
        let text = file.source().to_owned();
        let config = config_watch(path, names.clone(), running, text, merges, flags);
        // An error names its build, or every build for the server's.
        return crate::dev_watch::run_builds(builds, Some(config)).map_err(|(build, error)| {
            let named = match build {
                Some(build) => names[build].clone(),
                None => names.join(" "),
            };
            failure(&format!("watch {named}"), error)
        });
    }
    for (settings, (namespace, options, compile)) in settings.iter().zip(translated) {
        let name = settings.name();
        let out_dir = namespace.out_dir.clone().unwrap_or_default();
        eprintln!("[:{name}] Compiling ...");
        let begun = Instant::now();
        build_namespace(namespace, options, compile)
            .map_err(|error| failure(&format!("{} {name}", command.name()), error))?;
        eprintln!(
            "[:{name}] Build completed in {} ms: {}",
            begun.elapsed().as_millis(),
            out_dir.display()
        );
    }
    Ok(())
}

/// The configuration file, found by walking up from the current directory,
/// read and validated: its path and contents.
fn discover(
    words: &str,
    environment: &dyn Fn(&str) -> Option<String>,
) -> Result<(PathBuf, ConfigFile), Box<dyn Error>> {
    let start = std::env::current_dir().map_err(|error| failure(words, error))?;
    let path = ConfigFile::discover(&start).ok_or_else(|| {
        failure(
            words,
            format!(
                "no {FILE_NAME} in {} or any parent directory; create one \
                 (docs/build-configuration.md)",
                start.display()
            ),
        )
    })?;
    let file = ConfigFile::read(&path, environment).map_err(|error| failure(words, error))?;
    Ok((path, file))
}

/// The build `id` names, written with or without its colon, or the
/// unknown-id error ("Errors").
fn find_build(
    file: &ConfigFile,
    path: &Path,
    id: &str,
    words: &str,
) -> Result<BuildId, Box<dyn Error>> {
    let name = id.strip_prefix(':').unwrap_or(id);
    file.find_build(name).ok_or_else(|| {
        let names = defined(file);
        let message = match file.suggest_build(name) {
            Some(suggestion) => format!(
                "{} defines no build :{name}; did you mean :{suggestion}? Its builds are \
                 {names}",
                path.display()
            ),
            None => format!(
                "{} defines no build :{name}; its builds are {names}",
                path.display()
            ),
        };
        failure(words, message)
    })
}

/// `light-cljs cljs-repl ID [--print-length N] [--print-level N]`: the
/// `--repl --connect 127.0.0.1:PORT` client on the port the running watch
/// wrote to `<:cache-root>/http.port`, its session bound to build ID before
/// the first form is read, as `(light.dev/repl :ID)` binds it
/// (docs/build-configuration.md, "Commands"). It never starts a watch.
pub(crate) fn cljs_repl(args: impl Iterator<Item = OsString>) -> Result<(), Box<dyn Error>> {
    let mut args = args.peekable();
    let mut typed = Vec::new();
    while let Some(arg) = args.next_if(|arg| !arg.to_string_lossy().starts_with('-')) {
        typed.push(arg.to_string_lossy().into_owned());
    }
    let mut words = "cljs-repl".to_owned();
    for id in &typed {
        words.push(' ');
        words.push_str(id);
    }
    let words = words.as_str();

    let environment = |name: &str| std::env::var(name).ok();
    let (path, file) = discover(words, &environment)?;
    let id = match &typed[..] {
        [id] => id,
        [] => {
            return Err(failure(
                words,
                format!(
                    "requires one build id; {} defines {}",
                    path.display(),
                    defined(&file)
                ),
            ));
        }
        _ => {
            return Err(failure(
                words,
                "takes one build id, the build the session is bound to; (light.dev/repl :ID) \
                 binds it to another",
            ));
        }
    };
    let build = find_build(&file, &path, id, words)?;
    let mut print = crate::eval_cli::print_defaults();
    while let Some(arg) = args.next() {
        let flag = arg.to_string_lossy().into_owned();
        if !crate::eval_cli::print_flag(&flag, &mut args, &mut print)
            .map_err(|error| failure(words, error))?
        {
            return Err(failure(
                words,
                format!(
                    "{flag} is not a flag of cljs-repl, which connects to a running watch; it \
                     takes --print-length and --print-level"
                ),
            ));
        }
    }

    // The watch writes `http.port` under the file's `:cache-root` of its
    // dev settings (layers 1-5), which no flag moves; the build's output
    // directories do not matter here, so they are not compared.
    let settings = file
        .settings_before_flags(build, BuildMode::Dev, &[])
        .map_err(|error| failure(words, error))?;
    let name = settings.name().to_owned();
    let cache_root = string(&settings, Setting::CacheRoot).unwrap_or(".light-cljs");
    let port_file = file.resolve(Path::new(cache_root)).join("http.port");
    let not_running = || {
        failure(
            words,
            format!(
                "no watch is running for {}; start one with light-cljs watch {name}",
                path.display()
            ),
        )
    };
    // A file that does not hold a port is one no watch wrote.
    let port = std::fs::read_to_string(&port_file)
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .ok_or_else(not_running)?;
    let address = format!("127.0.0.1:{port}");
    let mut connection = match crate::repl_connect::Connection::open(&address) {
        Ok(connection) => connection,
        Err(crate::repl_connect::OpenError::Io(error))
            if error.kind() == std::io::ErrorKind::ConnectionRefused =>
        {
            return Err(not_running());
        }
        Err(error) => {
            return Err(failure(
                words,
                format!("cannot open a REPL session at {address}: {error}"),
            ));
        }
    };
    if let Err(error) = connection.bind(&name, &print) {
        connection.close();
        return Err(failure(words, error));
    }
    crate::repl_connect::repl(connection, &print)
}

/// The file's build ids, each with its colon: `:app, :test, :worker`.
fn defined(file: &ConfigFile) -> String {
    file.builds()
        .iter()
        .map(|build| format!(":{}", build.name()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Read the flags after the build ids with today's parsers, refusing what
/// the command does not take.
fn command_flags(
    command: Command,
    builds: usize,
    args: impl Iterator<Item = OsString>,
    words: &str,
) -> Result<CommandFlags, Box<dyn Error>> {
    let mut args = args.peekable();
    let mut flags = CommandFlags::default();
    while let Some(arg) = args.next() {
        let Some(flag) = arg.to_str() else {
            return Err(failure(
                words,
                format!(
                    "{} is not a flag of {}",
                    arg.to_string_lossy(),
                    command.name()
                ),
            ));
        };
        match flag {
            "--config-merge" => flags.merges.push(
                args.next()
                    .ok_or_else(|| {
                        failure(
                            words,
                            "--config-merge requires EDN text starting with { or a file path",
                        )
                    })?
                    .into_string()
                    .map_err(|_| failure(words, "--config-merge must be valid UTF-8"))?,
            ),
            "--entry" | "--out-dir" if builds > 1 => {
                return Err(refused(
                    words,
                    flag,
                    &mut args,
                    "--entry and --out-dir name one build's entry and directory; give one \
                     build id",
                ));
            }
            flag if OUTPUT_FLAGS.contains(&flag) => {
                return Err(refused(
                    words,
                    flag,
                    &mut args,
                    format!("{}; it takes no {flag}", command.writes()),
                ));
            }
            "--serve" | "--nrepl" if command == Command::Watch => {
                let text = args.next().ok_or_else(|| {
                    failure(words, format!("{flag} requires a port number, 0 to 65535"))
                })?;
                let port = crate::dev_watch::port_value(flag, &text)
                    .map_err(|error| failure(words, error))?;
                match flag {
                    "--serve" => flags.serve = Some(port),
                    _ => flags.nrepl = Some(port),
                }
            }
            "--serve-host" if command == Command::Watch => {
                flags.serve_host = Some(
                    args.next()
                        .ok_or_else(|| failure(words, "--serve-host requires a value"))?
                        .into_string()
                        .map_err(|_| failure(words, "--serve-host must be valid UTF-8"))?,
                );
            }
            "--poll-ms" if command == Command::Watch => {
                let text = args
                    .next()
                    .ok_or_else(|| failure(words, "--poll-ms requires a value"))?;
                flags.poll_ms = Some(
                    crate::dev_watch::poll_value(&text).map_err(|error| failure(words, error))?,
                );
            }
            "--no-nrepl" if command == Command::Watch => flags.no_nrepl = true,
            "--reload-dependents" if command == Command::Watch => flags.reload_dependents = true,
            flag if command == Command::Watch && NOT_WATCH_FLAGS.contains(&flag) => {
                return Err(failure(
                    words,
                    format!("{flag} is not a flag of watch; see light-cljs --help"),
                ));
            }
            flag if WATCH_FLAGS.contains(&flag) => {
                return Err(refused(
                    words,
                    flag,
                    &mut args,
                    format!(
                        "{flag} is a flag of light-cljs watch, which serves and watches the \
                         build; {} builds once",
                        command.name()
                    ),
                ));
            }
            "--reloadable" => {
                return Err(refused(
                    words,
                    flag,
                    &mut args,
                    "the command chooses the build: compile writes the dev build, as \
                     --reloadable does, and release the optimized chunks; give no --reloadable",
                ));
            }
            flag if command != Command::Release && RELEASE_FLAGS.contains(&flag) => {
                return Err(refused(
                    words,
                    flag,
                    &mut args,
                    format!(
                        "{flag} applies to release builds; {} writes the dev build at none \
                         semantics, as {} does",
                        command.name(),
                        command.runs_as()
                    ),
                ));
            }
            "--warnings" => flags.warnings = true,
            "--print-config" => flags.print = true,
            "--checked-arrays-warn" => flags.checked_arrays = Some(CheckedArrays::Warn),
            "--checked-arrays-error" => flags.checked_arrays = Some(CheckedArrays::Error),
            flag => {
                flags.jobs |= flag == "--jobs";
                let accepted = flags
                    .namespace
                    .accept(flag, &mut args)
                    .and_then(|accepted| Ok(accepted || flags.compile.accept(flag, &mut args)?))
                    .map_err(|error| failure(words, error))?;
                if !accepted {
                    return Err(failure(
                        words,
                        format!(
                            "{flag} is not a flag of {}; see light-cljs --help",
                            command.name()
                        ),
                    ));
                }
            }
        }
    }
    // A flag naming one define twice is an error, as one naming a `--config`
    // key twice is; each replaces the file's define of its name.
    let defines = &flags.compile.defines;
    for (index, text) in defines.iter().enumerate() {
        let name = define_name(text);
        if defines[..index]
            .iter()
            .any(|seen| define_name(seen) == name)
        {
            return Err(failure(
                words,
                format!("--define {text}: the define {name} is given twice"),
            ));
        }
    }
    if flags.compile.no_cache && flags.compile.cache_dir.is_some() {
        return Err(failure(
            words,
            "--no-cache and --cache-dir exclude each other",
        ));
    }
    if flags.no_nrepl && flags.nrepl.is_some() {
        return Err(failure(words, "--nrepl and --no-nrepl exclude each other"));
    }
    Ok(flags)
}

/// The refusal of `flag`, quoted after `words` with its value when it
/// takes one: `light-cljs watch app worker --entry x: ...`.
fn refused(
    words: &str,
    flag: &str,
    args: &mut impl Iterator<Item = OsString>,
    message: impl fmt::Display,
) -> Box<dyn Error> {
    let mut quoted = format!("{words} {flag}");
    if VALUED.contains(&flag)
        && let Some(value) = args.next()
    {
        quoted.push(' ');
        quoted.push_str(&value.to_string_lossy());
    }
    failure(&quoted, message)
}

/// A setting's string, such as `:output-dir`'s.
fn string(settings: &BuildSettings, setting: Setting) -> Option<&str> {
    settings.get(setting).and_then(Value::as_str)
}

/// A setting's decimal integer, such as a macro limit's, unless it is the
/// built-in default, which the flag structures already hold.
fn count(settings: &BuildSettings, setting: Setting) -> Option<u64> {
    let slot = settings.slot(setting)?;
    if slot.value.source == light_cljs::config_file::ConfigSource::Default {
        return None;
    }
    match &slot.value.kind {
        ValueKind::Number { text, .. } => text.parse().ok(),
        _ => None,
    }
}

fn boolean(settings: &BuildSettings, setting: Setting) -> bool {
    matches!(
        settings.get(setting).map(|value| &value.kind),
        Some(ValueKind::Boolean(true))
    )
}

fn keyword(settings: &BuildSettings, setting: Setting) -> Option<&str> {
    settings.get(setting).and_then(Value::as_keyword)
}

/// The elements of a vector or set setting.
fn elements(settings: &BuildSettings, setting: Setting) -> &[Value] {
    match settings.get(setting).map(|value| &value.kind) {
        Some(ValueKind::Vector(values) | ValueKind::Set(values)) => values,
        _ => &[],
    }
}

/// The entries of a map setting.
fn entries(settings: &BuildSettings, setting: Setting) -> &[(Value, Value)] {
    match settings.get(setting).map(|value| &value.kind) {
        Some(ValueKind::Map(entries)) => entries,
        _ => &[],
    }
}

/// The namespace of the build's one module's one entry.
fn entry(settings: &BuildSettings) -> Option<String> {
    let (_, module) = entries(settings, Setting::Modules).first()?;
    match &module.get("entries")?.kind {
        ValueKind::Vector(namespaces) => match &namespaces.first()?.kind {
            ValueKind::Symbol(namespace) => Some(namespace.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The `NS/NAME` of a `--define NS/NAME=VALUE` text: everything before its
/// first `=`, as [`light_cljs::Define::parse`] reads it.
fn define_name(text: &str) -> &str {
    text.split_once('=').map_or(text, |(name, _)| name)
}

/// A directory a build will use, as written and where it was written.
struct Placed {
    path: PathBuf,
    text: String,
    origin: String,
}

/// With a flag replacing the output directory, the source roots or the
/// cache store, compare the directories the build will use as
/// [`ConfigFile::settings`] compares the file's: its output directory must
/// not be another build's in its mode, as the file defines that build, nor
/// the cache store (when the cache is on), nor a source root. The message
/// gives both origins.
fn check_directories(
    file: &ConfigFile,
    settings: &BuildSettings,
    merges: &[ConfigMerge],
    flags: &CommandFlags,
) -> Result<(), String> {
    let mode = settings.mode();
    let mode_name = mode.name();
    let name = settings.name();
    let user = &flags.compile;
    let given = |path: &Path, flag: &str| Placed {
        path: file.resolve(path),
        text: path.display().to_string(),
        origin: flag.to_owned(),
    };
    let written = |value: &Value| {
        value.as_str().map(|text| Placed {
            path: file.resolve(Path::new(text)),
            text: text.to_owned(),
            origin: file.origin(value, merges).to_string(),
        })
    };
    let out = match &flags.namespace.out_dir {
        Some(dir) => given(dir, "--out-dir"),
        None => match settings.get(Setting::OutputDir).and_then(written) {
            Some(out) => out,
            None => return Ok(()),
        },
    };
    for (index, other) in file.builds().iter().enumerate() {
        if index == settings.build().0 as usize {
            continue;
        }
        let Some(theirs) = other.output_dir_value(mode) else {
            continue;
        };
        let Some(theirs) = written(theirs).map(|placed| Placed {
            origin: file.origin(theirs, &[]).to_string(),
            ..placed
        }) else {
            continue;
        };
        if theirs.path == out.path {
            let (a, b) = if name <= other.name() {
                (name, other.name())
            } else {
                (other.name(), name)
            };
            return Err(format!(
                "builds :{a} and :{b} both write {} in {mode_name} mode (:{name}'s from {}, \
                 :{}'s from {})",
                out.text,
                out.origin,
                other.name(),
                theirs.origin
            ));
        }
    }
    let store = if user.no_cache {
        None
    } else if let Some(dir) = &user.cache_dir {
        Some(given(dir, "--cache-dir"))
    } else if keyword(settings, Setting::CacheLevel) == Some("off") {
        None
    } else {
        settings
            .get(Setting::CacheRoot)
            .and_then(written)
            .map(|root| Placed {
                path: root.path.join("cache"),
                text: format!(":cache-root {:?}", root.text),
                origin: root.origin,
            })
    };
    if let Some(store) = store.filter(|store| store.path == out.path) {
        return Err(format!(
            "build :{name} writes {} in {mode_name} mode ({}), the cache store of {} ({}); give \
             it a directory of its own",
            out.text, out.origin, store.text, store.origin
        ));
    }
    let roots: Vec<Placed> = if flags.namespace.roots.is_empty() {
        elements(settings, Setting::SourcePaths)
            .iter()
            .filter_map(written)
            .collect()
    } else {
        flags
            .namespace
            .roots
            .iter()
            .map(|root| given(root, "--source-root"))
            .collect()
    };
    if let Some(root) = roots.iter().find(|root| root.path == out.path) {
        return Err(format!(
            "build :{name} writes {} in {mode_name} mode ({}), the source path {:?} ({}); give \
             it a directory of its own",
            out.text, out.origin, root.text, root.origin
        ));
    }
    Ok(())
}

/// A define's value as `--define NS/NAME=TEXT` takes it: a string's
/// characters, a boolean's or number's text.
fn define_text(value: &Value) -> String {
    match &value.kind {
        ValueKind::String(text) => text.clone(),
        _ => value.to_edn(),
    }
}

/// Write the flags after the command over `settings` (layer 6), as
/// [`translate`] applies them, for `--print-config`: each flag the file can
/// express replaces its setting, or its entry of `:config` and
/// `:closure-defines`, with the flag as its origin; `--cache-blocker` adds to
/// the build's blockers. `--stats`, `--stats-file` and `--size-report`
/// report on one run and are not in the file ("Not in the file").
///
/// With `printing` false, for a watch's reread to compare with, a
/// `--cache-dir` the file cannot name is no error: it gives `:cache-root` its
/// own text, which the file's `:cache-root` then cannot change.
fn apply_flags(
    file: &ConfigFile,
    settings: &mut BuildSettings,
    merges: &[ConfigMerge],
    command: Command,
    flags: &CommandFlags,
    printing: bool,
) -> Result<(), String> {
    let user = &flags.compile;
    let string = |text: String| Value::from(ValueKind::String(text));
    let integer = |number: u128| ValueKind::Number {
        kind: light_cljs::parser::NumberKind::Integer,
        text: number.to_string(),
    };
    let named = |name: &str| ValueKind::Keyword(name.to_owned());

    let roots = &flags.namespace.roots;
    if !roots.is_empty() {
        let typed = roots
            .iter()
            .map(|root| format!("--source-root {}", root.display()))
            .collect::<Vec<_>>()
            .join(" ");
        let paths = roots
            .iter()
            .map(|root| string(root.display().to_string()))
            .collect();
        settings.set_by_flag(Setting::SourcePaths, ValueKind::Vector(paths), &typed);
    }
    if let Some(namespace) = &flags.namespace.entry {
        // The module keeps the file's id; only its entry is the flag's.
        let module = entries(settings, Setting::Modules)
            .first()
            .and_then(|(id, _)| id.as_keyword())
            .unwrap_or("main")
            .to_owned();
        let entries = ValueKind::Vector(vec![Value::from(ValueKind::Symbol(namespace.clone()))]);
        let module = ValueKind::Map(vec![(
            Value::from(named(&module)),
            Value::from(ValueKind::Map(vec![(
                Value::from(named("entries")),
                Value::from(entries),
            )])),
        )]);
        settings.set_by_flag(Setting::Modules, module, &format!("--entry {namespace}"));
    }
    if let Some(dir) = &flags.namespace.out_dir {
        settings.set_by_flag(
            Setting::OutputDir,
            ValueKind::String(dir.display().to_string()),
            &format!("--out-dir {}", dir.display()),
        );
    }
    if command == Command::Release {
        if user.optimize {
            settings.set_by_flag(
                Setting::Optimizations,
                named(user.level.name()),
                &format!("--optimize {}", user.level),
            );
        }
        if user.pseudo_names {
            settings.set_by_flag(
                Setting::PseudoNames,
                ValueKind::Boolean(true),
                "--pseudo-names",
            );
        }
        for text in &user.defines {
            let (name, value) = text.split_once('=').unwrap_or((text, ""));
            settings.set_entry_by_flag(
                file,
                merges,
                Setting::ClosureDefines,
                Value::from(ValueKind::Symbol(name.to_owned())),
                define_value(value),
                &format!("--define {text}"),
            );
        }
        if flags.warnings {
            settings.set_by_flag(
                Setting::CljsWarnings,
                ValueKind::Boolean(true),
                "--warnings",
            );
        }
        let checks = match flags.checked_arrays {
            Some(CheckedArrays::Warn) => Some(("warn", "--checked-arrays-warn")),
            Some(CheckedArrays::Error) => Some(("error", "--checked-arrays-error")),
            Some(CheckedArrays::Off) | None => None,
        };
        if let Some((name, flag)) = checks {
            settings.set_by_flag(Setting::CheckedArrays, named(name), flag);
        }
    }
    if user.elide_asserts {
        settings.set_by_flag(
            Setting::ElideAsserts,
            ValueKind::Boolean(true),
            "--elide-asserts",
        );
    }
    let limits = &user.macro_limits;
    for (setting, flag, value) in [
        (
            Setting::MacroExpansions,
            "--macro-expansions",
            limits.expansions.map(|n| n as u128),
        ),
        (
            Setting::MacroNodes,
            "--macro-nodes",
            limits.nodes.map(|n| n as u128),
        ),
        (
            Setting::MacroTimeoutMs,
            "--macro-timeout-ms",
            limits.timeout_ms.map(u128::from),
        ),
        (
            Setting::MacroMemoryLimit,
            "--macro-memory-limit",
            limits.memory_limit.map(|n| n as u128),
        ),
        (
            Setting::MacroPollBudget,
            "--macro-poll-budget",
            limits.poll_budget.map(u128::from),
        ),
    ] {
        if let Some(value) = value {
            settings.set_by_flag(setting, integer(value), &format!("{flag} {value}"));
        }
    }
    if flags.jobs {
        let jobs = user.jobs.get();
        settings.set_by_flag(
            Setting::CompileThreads,
            integer(jobs as u128),
            &format!("--jobs {jobs}"),
        );
    }
    if let Some(retention) = user.cache_retention {
        settings.set_by_flag(
            Setting::CacheRetention,
            integer(retention as u128),
            &format!("--cache-retention {retention}"),
        );
    }
    for (key, text) in user.configuration.entries() {
        let value =
            Value::parse(text).map_err(|error| format!("--config {key}={text}: {error}"))?;
        settings.set_entry_by_flag(
            file,
            merges,
            Setting::Config,
            Value::from(named(key)),
            value,
            &format!("--config {key}={text}"),
        );
    }
    if user.no_cache {
        settings.set_by_flag(Setting::CacheLevel, named("off"), "--no-cache");
    } else if let Some(dir) = &user.cache_dir {
        let flag = format!("--cache-dir {}", dir.display());
        // A store named `cache` is `:cache-root`'s, its parent; any other
        // store the file cannot name, so it cannot print.
        match (dir.file_name(), dir.parent()) {
            _ if !printing => settings.set_by_flag(
                Setting::CacheRoot,
                ValueKind::String(dir.display().to_string()),
                &flag,
            ),
            (Some(name), Some(parent)) if name == "cache" => {
                let root = match parent.as_os_str().is_empty() {
                    true => ".".to_owned(),
                    false => parent.display().to_string(),
                };
                // The watch keeps `http.port` in the file's `:cache-root`,
                // which `--cache-dir` does not move: a printed `:cache-root`
                // elsewhere would move it.
                if command == Command::Watch
                    && let Some(kept) = settings.get(Setting::CacheRoot).and_then(Value::as_str)
                    && file.resolve(Path::new(kept)) != file.resolve(Path::new(&root))
                {
                    return Err(format!(
                        "{flag}: watch keeps http.port in :cache-root {kept:?}, which --cache-dir \
                         does not move, and a light-cljs.edn names its store as :cache-root's \
                         cache directory, so --print-config cannot print both; give --cache-dir \
                         {} or no --cache-dir",
                        Path::new(kept).join("cache").display()
                    ));
                }
                settings.set_by_flag(Setting::CacheRoot, ValueKind::String(root), &flag);
            }
            _ => {
                return Err(format!(
                    "{flag}: a light-cljs.edn names its store as :cache-root's cache \
                     directory, which {} is not, so --print-config cannot print it; give \
                     --cache-dir a directory named cache",
                    dir.display()
                ));
            }
        }
        if keyword(settings, Setting::CacheLevel) == Some("off") {
            settings.set_by_flag(Setting::CacheLevel, named("all"), &flag);
        }
    }
    if !user.cache_blockers.is_empty() {
        let blockers = user
            .cache_blockers
            .iter()
            .map(|blocker| Value::from(ValueKind::Symbol(blocker.clone())))
            .collect();
        settings.add_by_flag(Setting::BuildCacheBlockers, "--cache-blocker", blockers);
    }
    // The watch's own flags. `--serve PORT` binds that port or fails, which
    // the file says as `:strict true`.
    if let Some(port) = flags.serve {
        let flag = format!("--serve {port}");
        settings.set_by_flag(Setting::HttpPort, integer(port as u128), &flag);
        settings.set_by_flag(Setting::HttpStrict, ValueKind::Boolean(true), &flag);
    }
    if let Some(host) = &flags.serve_host {
        settings.set_by_flag(
            Setting::HttpHost,
            ValueKind::String(host.clone()),
            &format!("--serve-host {host}"),
        );
    }
    if let Some(port) = flags.nrepl {
        let map = ValueKind::Map(vec![(
            Value::from(named("port")),
            Value::from(integer(port as u128)),
        )]);
        settings.set_by_flag(Setting::Nrepl, map, &format!("--nrepl {port}"));
    }
    if flags.no_nrepl {
        settings.set_by_flag(Setting::Nrepl, ValueKind::Boolean(false), "--no-nrepl");
    }
    if let Some(millis) = flags.poll_ms {
        settings.set_by_flag(
            Setting::FsWatchLoopWait,
            integer(millis as u128),
            &format!("--poll-ms {millis}"),
        );
    }
    if flags.reload_dependents {
        settings.set_by_flag(
            Setting::ReloadStrategy,
            named("full"),
            "--reload-dependents",
        );
    }
    Ok(())
}

/// A `--define NS/NAME=VALUE`'s VALUE as the file would write it: a boolean
/// or a decimal number as itself, anything else as a string. Each gives
/// [`define_text`] the flag's text back, so the goog-define's own type still
/// decides.
fn define_value(text: &str) -> Value {
    match Value::parse(text) {
        Ok(value)
            if matches!(
                value.kind,
                ValueKind::Boolean(_)
                    | ValueKind::Number {
                        kind: light_cljs::parser::NumberKind::Integer
                            | light_cljs::parser::NumberKind::Float,
                        ..
                    }
            ) && value.to_edn() == text =>
        {
            value
        }
        _ => Value::from(ValueKind::String(text.to_owned())),
    }
}

/// The watch's `Options` for the dev build of `settings`, from what
/// [`translate`] made of it for `--compile --reloadable` and the watch's own
/// settings, each under its flag: `:http` as `--serve` and `--serve-host`
/// (the server always starts; a port in use gives way to the next unless
/// `:strict true` or `--serve`), `:nrepl {:port N}` as `--nrepl N` with
/// `.nrepl-port` in the project directory, `:nrepl false` as `--no-nrepl`,
/// `:fs-watch {:loop-wait N}` as `--poll-ms N` and `:devtools
/// {:reload-strategy :full}` as `--reload-dependents`. The server's port is
/// written to `<:cache-root>/http.port`, the file's cache root whatever
/// `--cache-dir` names, since `cljs-repl` reads it from the file.
fn watch_options(
    file: &ConfigFile,
    settings: &BuildSettings,
    namespace: NamespaceFlags,
    compile: CompileFlags,
    flags: &CommandFlags,
) -> Result<crate::dev_watch::Options, String> {
    use crate::dev_watch::PortRule;
    let integer = |setting: Setting| match settings.get(setting).map(|value| &value.kind) {
        Some(ValueKind::Number { text, .. }) => text.parse::<u64>().ok(),
        _ => None,
    };
    let (serve, port_rule) = match flags.serve {
        Some(port) => (port, PortRule::Exact),
        None => {
            let port = integer(Setting::HttpPort)
                .and_then(|port| u16::try_from(port).ok())
                .unwrap_or(9630);
            let rule = if boolean(settings, Setting::HttpStrict) {
                PortRule::Strict
            } else {
                PortRule::NextFree
            };
            (port, rule)
        }
    };
    let nrepl_setting = settings.get(Setting::Nrepl);
    // `--no-nrepl` turns off the file's listener too.
    let nrepl = flags.nrepl.or_else(|| {
        nrepl_setting
            .filter(|_| !flags.no_nrepl)
            .and_then(|value| value.get("port"))
            .and_then(|port| match &port.kind {
                ValueKind::Number { text, .. } => text.parse::<u16>().ok(),
                _ => None,
            })
    });
    let no_nrepl = flags.no_nrepl
        || (flags.nrepl.is_none()
            && matches!(
                nrepl_setting.map(|value| &value.kind),
                Some(ValueKind::Boolean(false))
            ));
    let poll = flags
        .poll_ms
        .or_else(|| integer(Setting::FsWatchLoopWait))
        .unwrap_or(100);
    let cache_root = string(settings, Setting::CacheRoot).unwrap_or(".light-cljs");
    Ok(crate::dev_watch::Options {
        entry: namespace.entry.ok_or("the build has no entry")?,
        roots: namespace.roots,
        out_dir: namespace.out_dir.ok_or("the build has no :output-dir")?,
        poll: std::time::Duration::from_millis(poll),
        assertions: !compile.elide_asserts,
        serve: Some(serve),
        port_rule,
        serve_host: Some(
            flags
                .serve_host
                .clone()
                .or_else(|| string(settings, Setting::HttpHost).map(str::to_owned))
                .unwrap_or_else(|| "127.0.0.1".to_owned()),
        ),
        reload_dependents: flags.reload_dependents
            || keyword(settings, Setting::ReloadStrategy) == Some("full"),
        nrepl,
        no_nrepl,
        nrepl_port_dir: file.project_dir().to_path_buf(),
        http_port_file: Some(file.resolve(Path::new(cache_root)).join("http.port")),
        mount: Some(settings.name().to_owned()),
        stats: compile.stats,
        cache_dir: (!compile.no_cache)
            .then(|| compile.cache_dir.clone())
            .flatten(),
        cache_retention: compile.cache_retention,
        blockers: compile.cache_blockers,
        configuration: compile.configuration,
        macro_limits: compile.macro_limits,
    })
}

/// The settings of the top level the running server and poll hold, by the
/// key a change of them names: a reread reports them, and applies none.
const HELD: [(&str, &[Setting]); 3] = [
    (
        "http",
        &[Setting::HttpPort, Setting::HttpHost, Setting::HttpStrict],
    ),
    ("nrepl", &[Setting::Nrepl]),
    (
        "fs-watch",
        &[Setting::FsWatchImpl, Setting::FsWatchLoopWait],
    ),
];

/// `settings` with the flags after the command over them, as the build runs
/// with them, so that a reread compares what the flags leave to the file:
/// a setting a flag gives does not change when the file's does. A
/// `--cache-dir` the file cannot name is written too, as `:cache-root`'s
/// text; only a `--config` value the flag's own parser refused, which never
/// reaches a watch, would leave the file's settings alone.
fn effective(
    file: &ConfigFile,
    settings: &BuildSettings,
    merges: &[ConfigMerge],
    flags: &CommandFlags,
) -> BuildSettings {
    let mut over = settings.clone();
    match apply_flags(file, &mut over, merges, Command::Watch, flags, false) {
        Ok(()) => over,
        Err(_) => settings.clone(),
    }
}

/// The watch's reread of `path` for the builds `names`, which run with the
/// settings `running`, in the same order, read from `text`
/// (docs/build-configuration.md, "Watching the configuration file"). Once
/// the file's text changed, it is read and validated again, against the same
/// environment, `merges` and flags, as at startup, and each build's settings
/// compared slot by slot, by value alone:
///
/// - an invalid file, or one that no longer defines one of the builds, is an
///   error, and every build keeps running with its settings;
/// - a change of `:http`, `:nrepl` or `:fs-watch`, which the running server
///   and poll hold, is reported once, `light-cljs: :http changed in
///   light-cljs.edn; restart the watch to apply it`, and applies to nothing;
/// - any other change of a build's settings restarts that build with the new
///   settings, under the same flags: a top-level setting, every build;
/// - otherwise (a comment, the section of a build not watched) nothing
///   happens.
fn config_watch(
    path: PathBuf,
    names: Vec<String>,
    mut running: Vec<BuildSettings>,
    text: String,
    merges: Vec<ConfigMerge>,
    flags: CommandFlags,
) -> crate::dev_watch::ConfigWatch {
    use crate::dev_watch::Reread;
    use light_cljs::dev_server::Diagnostic;
    let watched = path.clone();
    // The text last read, valid or not, so that a save that leaves it as it
    // was reads nothing again.
    let mut last = Some(text);
    let located = |error: light_cljs::config_file::ConfigFileError| {
        let diagnostic = Diagnostic {
            message: error.message.clone(),
            file: Some(error.path.display().to_string()),
            line: error.location.map(|(line, _)| line),
            column: error.location.map(|(_, column)| column),
        };
        Reread::Invalid(error.to_string(), diagnostic)
    };
    let plain = |message: String| {
        let diagnostic = Diagnostic {
            message: message.clone(),
            file: None,
            line: None,
            column: None,
        };
        Reread::Invalid(message, diagnostic)
    };
    let reread = move || {
        let environment = |name: &str| std::env::var(name).ok();
        let read = std::fs::read_to_string(&path);
        let text = read.as_ref().ok();
        if text == last.as_ref() {
            return Reread::Unchanged;
        }
        last = text.cloned();
        let file = match read {
            Ok(text) => ConfigFile::from_source(&path, text, &environment),
            Err(_) => ConfigFile::read(&path, &environment),
        };
        let file = match file {
            Ok(file) => file,
            Err(error) => return located(error),
        };
        let mut settings = Vec::with_capacity(names.len());
        for name in &names {
            let Some(build) = file.find_build(name) else {
                return plain(format!(
                    "{} defines no build :{name}; the build keeps its settings until it does, \
                     and its builds are {}",
                    path.display(),
                    defined(&file)
                ));
            };
            let merged = if flags.replaces_directories() {
                file.settings_before_flags(build, BuildMode::Dev, &merges)
            } else {
                file.settings(build, BuildMode::Dev, &merges)
            };
            match merged {
                Ok(merged) => settings.push(merged),
                Err(error) => return located(error),
            }
        }
        if let Err(error) = file.check_outputs(&settings, &merges) {
            return located(error);
        }
        let mut options = Vec::with_capacity(settings.len());
        for settings in &settings {
            let translated = (|| {
                if flags.replaces_directories() {
                    check_directories(&file, settings, &merges, &flags)?;
                }
                let (namespace, _, compile) = translate(&file, settings, Command::Watch, &flags)?;
                watch_options(&file, settings, namespace, compile, &flags)
            })();
            match translated {
                Ok(translated) => options.push(translated),
                Err(message) => return plain(message),
            }
        }
        let next = settings
            .iter()
            .map(|settings| effective(&file, settings, &merges, &flags))
            .collect::<Vec<_>>();
        let changed = running
            .iter()
            .zip(&next)
            .map(|(running, next)| running.changed(next))
            .collect::<Vec<_>>();
        running = next;
        let held = |setting: &Setting| HELD.iter().any(|(_, slots)| slots.contains(setting));
        // The held settings are the top level's, the same for every build:
        // each is reported once.
        for (key, slots) in HELD {
            if changed
                .iter()
                .flatten()
                .any(|setting| slots.contains(setting))
            {
                eprintln!(
                    "light-cljs: :{key} changed in {FILE_NAME}; restart the watch to apply it"
                );
            }
        }
        let mut restarts = Vec::new();
        for (index, (changed, options)) in changed.iter().zip(options).enumerate() {
            if changed.iter().all(held) {
                continue;
            }
            eprintln!(
                "light-cljs: {FILE_NAME} changed the settings of :{}; restarting the build",
                names[index]
            );
            restarts.push((index, Box::new(options)));
        }
        if restarts.is_empty() {
            Reread::Unchanged
        } else {
            Reread::Restart(restarts)
        }
    };
    crate::dev_watch::ConfigWatch {
        path: watched,
        reread: Box::new(reread),
    }
}

/// Translate `settings` into the flags' structures, as the equivalent
/// flags would set them, then write the flags after the command over them.
fn translate(
    file: &ConfigFile,
    settings: &BuildSettings,
    command: Command,
    flags: &CommandFlags,
) -> Result<(NamespaceFlags, AnalysisOptions, CompileFlags), String> {
    let release = command == Command::Release;
    let user = &flags.compile;
    let resolve = |path: &Path| file.resolve(path);

    let namespace = NamespaceFlags {
        entry: flags.namespace.entry.clone().or_else(|| entry(settings)),
        roots: if flags.namespace.roots.is_empty() {
            elements(settings, Setting::SourcePaths)
                .iter()
                .filter_map(Value::as_str)
                .map(|path| resolve(Path::new(path)))
                .collect()
        } else {
            flags
                .namespace
                .roots
                .iter()
                .map(|path| resolve(path))
                .collect()
        },
        esm: false,
        reloadable: !release,
        out_dir: flags
            .namespace
            .out_dir
            .as_deref()
            .or_else(|| string(settings, Setting::OutputDir).map(Path::new))
            .map(resolve),
    };

    let mut options = AnalysisOptions::default();
    let mut compile = CompileFlags::default();
    if release {
        compile.optimize = true;
        compile.level = if user.optimize {
            user.level
        } else {
            keyword(settings, Setting::Optimizations)
                .unwrap_or("advanced")
                .parse()
                .map_err(|error| format!("{error}"))?
        };
        compile.pseudo_names = user.pseudo_names || boolean(settings, Setting::PseudoNames);
        compile.size_report = user.size_report;
        for (name, value) in entries(settings, Setting::ClosureDefines) {
            let ValueKind::Symbol(name) = &name.kind else {
                continue;
            };
            if !user.defines.iter().any(|text| define_name(text) == name) {
                compile
                    .defines
                    .push(format!("{name}={}", define_text(value)));
            }
        }
        compile.defines.extend(user.defines.iter().cloned());
        options.warning_mode = flags.warnings || boolean(settings, Setting::CljsWarnings);
        let checked = flags
            .checked_arrays
            .or(match keyword(settings, Setting::CheckedArrays) {
                Some("warn") => Some(CheckedArrays::Warn),
                Some("error") => Some(CheckedArrays::Error),
                _ => None,
            });
        if let Some(checks) = checked {
            options.warning_mode = true;
            options.checked_arrays = true;
            options.array_checks = checks;
        }
    }
    compile.elide_asserts = user.elide_asserts || boolean(settings, Setting::ElideAsserts);

    let limits = &user.macro_limits;
    let usize_count = |setting| count(settings, setting).map(|n| n as usize);
    compile.macro_limits = crate::MacroLimitFlags {
        expansions: limits
            .expansions
            .or_else(|| usize_count(Setting::MacroExpansions)),
        nodes: limits.nodes.or_else(|| usize_count(Setting::MacroNodes)),
        timeout_ms: limits
            .timeout_ms
            .or_else(|| count(settings, Setting::MacroTimeoutMs)),
        memory_limit: limits
            .memory_limit
            .or_else(|| usize_count(Setting::MacroMemoryLimit)),
        poll_budget: limits
            .poll_budget
            .or_else(|| count(settings, Setting::MacroPollBudget)),
    };

    compile.stats = user.stats;
    compile.stats_file = user.stats_file.as_deref().map(resolve);

    for (key, value) in entries(settings, Setting::Config) {
        let Some(key) = key.as_keyword() else {
            continue;
        };
        compile
            .configuration
            .insert(key, &value.to_edn())
            .map_err(|error| format!(":config :{key}: {error}"))?;
    }
    for (key, value) in user.configuration.entries() {
        compile
            .configuration
            .set(key, value)
            .map_err(|error| format!("--config {key}={value}: {error}"))?;
    }

    // `--cache-dir` names the store and turns the cache on over
    // `:cache-level :off`; `--no-cache` turns it off.
    if user.no_cache {
        compile.no_cache = true;
    } else if let Some(dir) = &user.cache_dir {
        compile.cache_dir = Some(resolve(dir));
    } else if keyword(settings, Setting::CacheLevel) == Some("off") {
        compile.no_cache = true;
    } else {
        let root = string(settings, Setting::CacheRoot).unwrap_or(".light-cljs");
        compile.cache_dir = Some(resolve(Path::new(root)).join("cache"));
    }
    // Cache blockers accumulate: the file's (the top level's already
    // unioned into the build's), then the flags'.
    for blocker in elements(settings, Setting::BuildCacheBlockers)
        .iter()
        .filter_map(|value| match &value.kind {
            ValueKind::Symbol(name) => Some(name),
            _ => None,
        })
        .chain(&user.cache_blockers)
    {
        if !compile.cache_blockers.contains(blocker) {
            compile.cache_blockers.push(blocker.clone());
        }
    }

    compile.jobs = if flags.jobs {
        user.jobs
    } else {
        count(settings, Setting::CompileThreads)
            .and_then(|n| usize::try_from(n).ok())
            .and_then(light_cljs::scheduler::Slots::new)
            .unwrap_or(user.jobs)
    };
    // `--cache-retention N` over the top-level `:cache-retention N`; neither
    // leaves the store's three.
    compile.cache_retention = user
        .cache_retention
        .or_else(|| count(settings, Setting::CacheRetention).and_then(|n| usize::try_from(n).ok()));
    Ok((namespace, options, compile))
}
