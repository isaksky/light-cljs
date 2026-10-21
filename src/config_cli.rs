//! The `compile` and `release` commands, which run named builds of
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
    /// `release ID...`: the release build of each, at `:optimizations`.
    Release,
}

impl Command {
    /// The command a first argument names, if any.
    pub(crate) fn named(arg: &str) -> Option<Self> {
        match arg {
            "compile" => Some(Self::Compile),
            "release" => Some(Self::Release),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Compile => "compile",
            Self::Release => "release",
        }
    }

    fn mode(self) -> BuildMode {
        match self {
            Self::Compile => BuildMode::Dev,
            Self::Release => BuildMode::Release,
        }
    }

    /// What the command writes, for refusing an output flag it does not take.
    fn writes(self) -> &'static str {
        match self {
            Self::Compile => {
                "compile writes the dev build into :output-dir, as --reloadable does, with a \
                 source map per namespace"
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
const WATCH_FLAGS: [&str; 5] = [
    "--serve",
    "--serve-host",
    "--nrepl",
    "--poll-ms",
    "--reload-dependents",
];

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
    let environment = |name: &str| std::env::var(name).ok();
    let file = ConfigFile::read(&path, &environment).map_err(|error| failure(words, error))?;
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
        let name = id.strip_prefix(':').unwrap_or(id);
        let Some(build) = file.find_build(name) else {
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
            return Err(failure(words, message));
        };
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
    // A flag that replaces an output directory, the source roots or the cache
    // store moves the output check after it ("Exclusions are checked after
    // merging"): the directories compared are those the build will use.
    let replaced = flags.namespace.out_dir.is_some()
        || !flags.namespace.roots.is_empty()
        || flags.compile.cache_dir.is_some()
        || flags.compile.no_cache;
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
            apply_flags(&file, &mut settings, &merges, command, &flags)
                .map_err(|error| failure(words, error))?;
            if index > 0 {
                out.push('\n');
            }
            out.push_str(&file.print_config(&settings, &merges, false));
        }
        print!("{out}");
        return Ok(());
    }

    eprintln!("light-cljs - config: {}", path.display());
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
            flag if command == Command::Compile && RELEASE_FLAGS.contains(&flag) => {
                return Err(refused(
                    words,
                    flag,
                    &mut args,
                    format!(
                        "{flag} applies to release builds; compile writes the dev build at none \
                         semantics, as --compile --reloadable does"
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
fn apply_flags(
    file: &ConfigFile,
    settings: &mut BuildSettings,
    merges: &[ConfigMerge],
    command: Command,
    flags: &CommandFlags,
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
            (Some(name), Some(parent)) if name == "cache" => {
                let root = match parent.as_os_str().is_empty() {
                    true => ".".to_owned(),
                    false => parent.display().to_string(),
                };
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
    Ok((namespace, options, compile))
}
