//! `--watch` (docs/dev-workflow.md, "The watch loop"): write the dev build of
//! `--reloadable`, keep a `DevSession` that has loaded the same program,
//! then poll the source roots and rewrite the unit of each namespace whose
//! file changed, compiled against the session's retained program.
//!
//! A step compiles the changed file's forms as fragments in its namespace
//! (`DevSession::load_namespace_file`, in warning mode), writes its unit
//! and source map (`dev_build::namespace_unit`) and the manifest. A form
//! that fails stops the step: the error goes to stderr and the namespace
//! keeps its previous unit, to be compiled again at the next change. A
//! changed `ns` form loads its new dependencies through the source provider,
//! which adds their units to the manifest ahead of it, and a change to a
//! namespace that declares macros recompiles every namespace that requires
//! it from its source, since their units hold the expansions.
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler_data::Name;
use light_cljs::dev_build::{DevFile, DevManifest, DevNamespace, namespace_unit, unit_file};
use light_cljs::dev_session::{Action, DevSession, Unit};
use light_cljs::source_provider::{
    FileSourceProvider, LoadError, LoadRequest, Reload, SourceInput, SourceProvider,
};
use light_cljs::{EvalOptions, MacroHost};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant, SystemTime},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub const USAGE: &str = "Watch:
  light-cljs --watch --entry NAMESPACE --source-root DIR [--source-root DIR ...] --out-dir DIR
             [--poll-ms N] [--elide-asserts]

Write the dev build of --compile --reloadable into DIR, then poll the .cljs
and .cljc files under the source roots every N milliseconds (100), comparing
modification times and sizes. A changed namespace is compiled again against
the program the watch keeps, and its unit, source map and manifest.json are
rewritten; a namespace that declares macros also recompiles the namespaces
that require it, and a new require adds the units of the namespaces it loads.
Each step prints one line to stderr with the namespaces recompiled and the
time taken; a form that fails prints its error with file, line and column,
and the namespace keeps its previous unit. Runs until interrupted.";

/// The options of `--watch`.
struct Options {
    entry: String,
    roots: Vec<PathBuf>,
    out_dir: PathBuf,
    poll: Duration,
    assertions: bool,
}

fn options(mut args: impl Iterator<Item = OsString>) -> Result<Option<Options>> {
    let (mut entry, mut roots, mut out_dir) = (None, Vec::new(), None);
    let mut poll = Duration::from_millis(100);
    let mut assertions = true;
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
            _ => return Err(format!("unexpected watch option: {flag}\n{USAGE}").into()),
        }
    }
    let entry = entry.ok_or("--watch requires --entry NAMESPACE")?;
    if roots.is_empty() {
        return Err("--watch requires --source-root DIR".into());
    }
    let out_dir = out_dir.ok_or("--watch requires --out-dir DIR")?;
    Ok(Some(Options {
        entry,
        roots,
        out_dir,
        poll,
        assertions,
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
        let source = self.roots.load(request)?;
        if let Some(source) = &source {
            self.files.borrow_mut().insert(
                String::from_utf16_lossy(request.namespace.name()),
                PathBuf::from(&source.file),
            );
        }
        Ok(source)
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
}

/// Why a namespace's compile ended a step.
enum Failure {
    /// Its source did not compile: the namespace keeps its unit and is
    /// compiled again at the next change.
    Source(String),
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
    // Dev compilation reports undeclared Vars and arity conflicts as
    // warnings (docs/dev-workflow.md, "Dev-mode contract").
    let analysis = AnalysisOptions {
        warning_mode: true,
        ..AnalysisOptions::default()
    };
    let mut warnings = Vec::new();
    let program = light_cljs::compiler::analyze_namespace_entry_with_sources(
        &options.entry,
        &options.roots,
        &mut host,
        analysis,
        &mut warnings,
    );
    crate::print_warnings(&warnings);
    let (program, sources) = program?;
    crate::write_dev_build(&program, &sources, &options.out_dir)?;
    let manifest = DevManifest::new(&program);
    drop(program);
    // The session loads the program again, as a `require` of the entry,
    // which is the program its steps compile against; the build printed the
    // warnings.
    let provider = Rc::new(Recording {
        roots: FileSourceProvider::new(roots.clone()),
        files: RefCell::default(),
    });
    let session = DevSession::new(&EvalOptions {
        source_provider: Some(provider.clone()),
        assertions: options.assertions,
        ..EvalOptions::default()
    })?;
    let actions = session.source(&SourceInput::new(
        "<watch>",
        format!("(require '{})", options.entry),
    ));
    if let Some(Action::Error(error)) = actions.last() {
        return Err(crate::eval_cli::diagnostic(error).into());
    }
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
    };
    eprintln!(
        "light-cljs: built {} ({} namespaces) into {} in {} ms; watching every {} ms",
        options.entry,
        watch.manifest.namespaces.len(),
        options.out_dir.display(),
        begun.elapsed().as_millis(),
        options.poll.as_millis()
    );
    loop {
        std::thread::sleep(options.poll);
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
        watch.step(&changed, &deleted, noticed)?;
    }
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

    /// One step: the files `changed` (or added) and `deleted` since the last
    /// poll, which noticed them at `noticed`.
    fn step(&mut self, changed: &[PathBuf], deleted: &[PathBuf], noticed: Instant) -> Result<()> {
        for path in deleted {
            if let Some(namespace) = self.namespace_of(path) {
                eprintln!(
                    "light-cljs: {} was deleted; {namespace} keeps its unit",
                    path.display()
                );
            }
        }
        if changed.is_empty() {
            return Ok(());
        }
        let mut queue = std::mem::take(&mut self.pending);
        for path in changed {
            if let Some(namespace) = self.namespace_of(path)
                && !queue.contains(&namespace)
            {
                queue.push(namespace);
            }
        }
        if queue.is_empty() {
            return Ok(());
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
                Err(Failure::Source(error)) => {
                    eprintln!("light-cljs: {error}");
                    queue.insert(0, namespace.clone());
                    self.pending = queue;
                    failed = Some(namespace);
                    break;
                }
                Err(Failure::Output(error)) => return Err(error),
            }
        }
        self.manifest.sort();
        let manifest = self.manifest.to_json();
        if manifest != self.written[0] {
            self.write(&DevFile {
                name: "manifest.json".into(),
                text: manifest.clone(),
            })?;
            self.written[0] = manifest;
        }
        let index = self.manifest.index();
        if index != self.written[1] {
            self.write(&DevFile {
                name: "index.js".into(),
                text: index.clone(),
            })?;
            self.written[1] = index;
        }
        recompiled.sort_by_key(|namespace: &String| self.manifest.position(namespace));
        recompiled.dedup();
        let names = if recompiled.is_empty() {
            "nothing".to_owned()
        } else {
            recompiled.join(", ")
        };
        let millis = noticed.elapsed().as_secs_f64() * 1000.0;
        match failed {
            None => eprintln!("light-cljs: recompiled {names} in {millis:.1} ms"),
            Some(namespace) => eprintln!(
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
            Err(LoadError::Provider(message)) => return Err(Failure::Source(message)),
            Err(error) => return Err(Failure::Source(format!("{namespace}: {error}"))),
        };
        let macros = self
            .manifest
            .position(namespace)
            .is_some_and(|index| self.manifest.namespaces[index].macros);
        let actions = self.session.load_namespace_file(namespace, &source);
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
                    }
                }
                Action::Error(diagnostic) => error = Some(crate::eval_cli::diagnostic(diagnostic)),
            }
        }
        let mut seen = HashSet::new();
        entered.retain(|name| seen.insert(*name));
        // A require the session does not refuse, of a namespace it has
        // loaded, can close a cycle, which `--compile` refuses.
        if error.is_none()
            && let Some(cycle) = self.cycle(namespace)
        {
            error = Some(format!(
                "{}: namespace dependency cycle: {}",
                source.file,
                cycle.join(" -> ")
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
            let base = self.base.clone();
            let url = move |source: &str| crate::source_url(Path::new(source), &base);
            let files = namespace_unit(
                name,
                &requires,
                units.get(name).map_or(&[][..], Vec::as_slice),
                value,
                &url,
            );
            for file in &files {
                self.write(file).map_err(Failure::Output)?;
            }
            let index = self.manifest.position(name).unwrap();
            let entry = &mut self.manifest.namespaces[index];
            entry.requires = requires;
            entry.macros = self.session.declares_macros(name);
            recompiled.push(name.to_owned());
        }
        if let Some(error) = error {
            return Err(Failure::Source(error));
        }
        // The units of the namespaces that require a macro namespace hold
        // their expansions: compile them again from their sources.
        if macros || self.session.declares_macros(namespace) {
            for entry in &self.manifest.namespaces {
                if entry.name != namespace
                    && entry.requires.iter().any(|required| required == namespace)
                    && !queue.contains(&entry.name)
                    && !recompiled.contains(&entry.name)
                {
                    queue.push(entry.name.clone());
                }
            }
        }
        Ok(())
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
