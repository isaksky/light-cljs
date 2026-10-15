use std::{
    env,
    error::Error,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use light_cljs::analyzer::{AnalysisOptions, AnalysisWarning, analyze_named_with_macro_warnings};
use light_cljs::source_map::{SourceFile, data_url};
use light_cljs::{
    ApplicationOptions, Define, LineIndex, MacroHost, OptimizationLevel, PreparedApplication,
    TokenKind, Tokenizer, parse, prepare_application,
};

mod dev_watch;
mod eval_cli;
mod repl_connect;

const USAGE: &str = "Usage: light-cljs [--parse|--analyze|--compile] [FILE|-]\n       light-cljs [--parse|--analyze|--compile] --text SOURCE\n       light-cljs --compile --entry NAMESPACE --source-root DIR [--source-root DIR ...] [--esm]\n       light-cljs --compile --entry NAMESPACE --source-root DIR ... --out-dir DIR\n       light-cljs --compile --entry NAMESPACE --source-root DIR ... --reloadable --out-dir DIR\n       light-cljs --compile --optimize none|prepare|advanced [--size-report] [--pseudo-names] [--define NS/NAME=VALUE ...] [--elide-asserts] [--source-map PATH | --source-map-inline] [--source-map-include-sources] ...\n\nRead UTF-8 from FILE or stdin. By default print raw tokens and byte spans.\nWith --parse, print syntax nodes, child IDs, spans, and source text.\nWith --analyze, print resolved expressions and binding IDs.\nWith --compile, emit a JavaScript expression evaluating to the last form.\nWith --optimize LEVEL after --compile, choose application optimization: none emits every definition as analyzed, prepare (the default) removes unused inert definitions and specializes calls, and advanced is the release mode, which also embeds only the runtime units the program reaches.\nWith --define NS/NAME=VALUE after --compile, replace the default of the goog-define NS/NAME at every level: true or false for a boolean, a number for a number, and the text as written for a string; at advanced, code only a define's other value runs is removed.\nWith --elide-asserts after --compile, omit every assert and fn :pre/:post check at every level, as upstream's :elide-asserts does; it is the only switch, since --define names only goog-define Vars.\nWith --source-map PATH after --compile, also write a Source Map v3 of the output to PATH, at every level, and end the output with a //# sourceMappingURL comment naming the map's file, for JavaScript written next to it; source paths in the map are relative to its directory. With --source-map-inline the comment embeds the map instead, with paths relative to the current directory. With --source-map-include-sources the map also holds the text of every source.\nWith --size-report after --compile, also print to stderr which runtime units the output embeds, with their bytes and the source expression that first demanded each.\nWith --pseudo-names after --compile, keep advanced output readable for debugging: the compiler's own names, the runtime with its comments and the emitter's layout, instead of short names and no whitespace.\nWith --entry, compile the namespace and its dependencies from explicit source roots.\nJavaScript package requires such as [\"react\" :as r] read globalThis.lightCljsPackages[\"react\"], supplied by the host.\nWith --esm, emit one ES module for a bundler instead: packages become static imports and the entry value is the default export.\nWith --out-dir DIR, write a chunked build into DIR (created when missing) instead: ES modules as with --esm, the startup chunk NAMESPACE.js, whose default export is the entry value, and one chunk per group of namespaces only light.lazy/loadable targets reach, which light.lazy/load imports; and chunks.json, which describes them. Other files in DIR are left alone. It works at every --optimize level, --size-report prints the one report, and the source map flags are refused; a program without loadables is one file, its --esm output.\nWith --reloadable --out-dir DIR, write a dev build into DIR instead: runtime.js (the runtime, once), client.js, one NAMESPACE.js unit with its source map per namespace in dependency order, manifest.json and index.js, which loads them in order. Definitions live in a registry, so a unit evaluated again replaces its namespace for every caller; the build runs at none semantics, with warnings for undeclared Vars and arity conflicts.\nErrors cause a nonzero exit status; diagnostics include line/byte-column.";

fn print_warnings(warnings: &[AnalysisWarning]) {
    for warning in warnings {
        eprintln!(
            "WARNING: {} [{}] (source {}, bytes {}..{})",
            warning.message,
            warning.kind,
            warning.source_index,
            warning.span.start,
            warning.span.end
        );
    }
}

fn optimization_level(
    args: &mut impl Iterator<Item = std::ffi::OsString>,
) -> Result<OptimizationLevel, Box<dyn Error>> {
    let level = args
        .next()
        .ok_or("--optimize requires a level: none, prepare or advanced")?;
    Ok(level.to_string_lossy().parse()?)
}

/// The application options `--compile` collects: `--optimize`,
/// `--size-report`, `--pseudo-names`, `--define`, `--elide-asserts` and the
/// source map flags.
#[derive(Default)]
struct CompileFlags {
    /// `--source-map PATH`: where to write the source map.
    source_map: Option<PathBuf>,
    /// `--source-map-inline`: embed the source map in the output.
    source_map_inline: bool,
    /// `--source-map-include-sources`: the map holds the sources' text.
    include_sources: bool,
    level: OptimizationLevel,
    /// Whether `--optimize` was given, which a dev build refuses.
    optimize: bool,
    size_report: bool,
    pseudo_names: bool,
    /// `NS/NAME=VALUE` texts, read against the program once it is analyzed.
    defines: Vec<String>,
    /// Omit assertions, a setting of analysis rather than of the application.
    elide_asserts: bool,
}

impl CompileFlags {
    /// Consume `flag` and its value when it is one of these flags.
    fn accept(
        &mut self,
        flag: &str,
        args: &mut impl Iterator<Item = std::ffi::OsString>,
    ) -> Result<bool, Box<dyn Error>> {
        match flag {
            "--optimize" => {
                self.level = optimization_level(args)?;
                self.optimize = true;
            }
            "--size-report" => self.size_report = true,
            "--pseudo-names" => self.pseudo_names = true,
            "--elide-asserts" => self.elide_asserts = true,
            "--source-map" => {
                self.source_map = Some(PathBuf::from(
                    args.next().ok_or("--source-map requires a path")?,
                ));
            }
            "--source-map-inline" => self.source_map_inline = true,
            "--source-map-include-sources" => self.include_sources = true,
            "--define" => self.defines.push(
                args.next()
                    .ok_or("--define requires NS/NAME=VALUE")?
                    .into_string()
                    .map_err(|_| "--define must be valid UTF-8")?,
            ),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The macro host that analyzes the program.
    fn host(&self) -> MacroHost {
        let mut host = MacroHost::new();
        host.set_assertions(!self.elide_asserts);
        host
    }

    /// The application options for `program`.
    fn options(&self, program: &light_cljs::Program) -> Result<ApplicationOptions, Box<dyn Error>> {
        let mut options = ApplicationOptions::new(self.level);
        options.pseudo_names = self.pseudo_names;
        for text in &self.defines {
            options.defines.push(Define::parse(program, text)?);
        }
        Ok(options)
    }
}

impl CompileFlags {
    /// The output of `prepared`, an ES module with `esm`, and with a source
    /// map flag also its source map, written to its file or embedded.
    /// `sources` are the program's source files by source index: each path
    /// as given (or a name such as `<text>.cljc`) and its text.
    fn output(
        &self,
        prepared: &PreparedApplication,
        esm: bool,
        sources: &[(&Path, &str)],
    ) -> Result<String, Box<dyn Error>> {
        if self.source_map.is_some() && self.source_map_inline {
            return Err("--source-map and --source-map-inline exclude each other".into());
        }
        if self.source_map.is_none() && !self.source_map_inline {
            if self.include_sources {
                return Err(
                    "--source-map-include-sources requires --source-map PATH or --source-map-inline"
                        .into(),
                );
            }
            return Ok(if esm {
                prepared.emit_esm()
            } else {
                prepared.emit_js()
            });
        }
        let (mut code, mapping) = if esm {
            prepared.emit_esm_mapped()
        } else {
            prepared.emit_js_mapped()
        };
        // Sources are named relative to the map's directory, or for an
        // embedded map the current directory.
        let base = match self.source_map.as_deref().and_then(Path::parent) {
            Some(directory) if !directory.as_os_str().is_empty() => directory.to_path_buf(),
            _ => env::current_dir()?,
        };
        let base = fs::canonicalize(&base).unwrap_or(base);
        let urls = sources
            .iter()
            .map(|(path, _)| source_url(path, &base))
            .collect::<Vec<_>>();
        let files = urls
            .iter()
            .zip(sources)
            .map(|(url, (_, text))| SourceFile { url, text })
            .collect::<Vec<_>>();
        let name = self
            .source_map
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy());
        let file = name.as_deref().and_then(|name| name.strip_suffix(".map"));
        let json = mapping.to_json(file, &files, self.include_sources);
        let url = match (&self.source_map, &name) {
            (Some(path), Some(name)) => {
                fs::write(path, &json)
                    .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
                url_segment(name)
            }
            (Some(path), None) => {
                return Err(format!("--source-map needs a file name: {}", path.display()).into());
            }
            _ => data_url(&json),
        };
        code.push_str("//# sourceMappingURL=");
        code.push_str(&url);
        code.push('\n');
        Ok(code)
    }
}

/// The `sources` entry of the source at `path`: relative to the directory
/// `base` when the file exists, else the name as given (`<text>.cljc`).
fn source_url(path: &Path, base: &Path) -> String {
    let Ok(path) = fs::canonicalize(path) else {
        return path
            .to_string_lossy()
            .split(['/', '\\'])
            .map(url_segment)
            .collect::<Vec<_>>()
            .join("/");
    };
    let parts = path.components().collect::<Vec<_>>();
    let base = base.components().collect::<Vec<_>>();
    let common = parts.iter().zip(&base).take_while(|(a, b)| a == b).count();
    if common == 0 {
        // Another drive: an absolute URL.
        return format!(
            "file:///{}",
            path.to_string_lossy()
                .split(['/', '\\'])
                .filter(|part| !part.is_empty())
                .map(|part| if part.ends_with(':') {
                    part.to_owned()
                } else {
                    url_segment(part)
                })
                .collect::<Vec<_>>()
                .join("/")
        );
    }
    std::iter::repeat_n("..".to_owned(), base.len() - common)
        .chain(
            parts[common..]
                .iter()
                .map(|part| url_segment(&part.as_os_str().to_string_lossy())),
        )
        .collect::<Vec<_>>()
        .join("/")
}

/// `text` as one segment of a URL path, percent-encoded, `:` too, so that
/// a first segment never reads as a scheme.
fn url_segment(text: &str) -> String {
    let mut segment = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=@".contains(&byte) {
            segment.push(char::from(byte));
        } else {
            segment.push_str(&format!("%{byte:02X}"));
        }
    }
    segment
}

/// Print the retention report of `prepared` to stderr (`--size-report`).
fn print_size_report(prepared: &light_cljs::PreparedApplication) -> io::Result<()> {
    write!(io::stderr().lock(), "{}", prepared.retention())
}

/// Write the dev build of `program` (`--reloadable`) into `out_dir`.
/// `sources` are the program's source files by source index.
fn write_dev_build(
    program: &light_cljs::Program,
    sources: &[(PathBuf, String)],
    out_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(out_dir)
        .map_err(|error| format!("cannot create {}: {error}", out_dir.display()))?;
    // Source maps name the sources relative to the build directory.
    let base = fs::canonicalize(out_dir)?;
    let urls = sources
        .iter()
        .map(|(path, _)| source_url(path, &base))
        .collect::<Vec<_>>();
    let files = urls
        .iter()
        .zip(sources)
        .map(|(url, (_, text))| SourceFile { url, text })
        .collect::<Vec<_>>();
    for file in light_cljs::dev_build::emit_dev_build(program, &files) {
        let path = out_dir.join(&file.name);
        fs::write(&path, file.text)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    Ok(())
}

/// Write the chunked build `output` (`--out-dir` without `--reloadable`)
/// into `out_dir`, created when missing: each chunk file and `chunks.json`,
/// its manifest. Other files in `out_dir` stay as they are.
fn write_chunks(output: &light_cljs::ChunkedOutput, out_dir: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(out_dir)
        .map_err(|error| format!("cannot create {}: {error}", out_dir.display()))?;
    let manifest = output.manifest();
    let files = output
        .files
        .iter()
        .map(|file| (file.name.as_str(), file.source.as_str()))
        .chain([("chunks.json", manifest.as_str())]);
    for (name, text) in files {
        // A chunk is named after namespaces, which never hold a path
        // separator; refuse to write anywhere but `out_dir` regardless.
        if Path::new(name).file_name() != Some(name.as_ref()) {
            return Err(format!("chunk file name {name:?} is not a plain file name").into());
        }
        let path = out_dir.join(name);
        fs::write(&path, text)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    Ok(())
}

fn run_namespace(
    mut args: impl Iterator<Item = std::ffi::OsString>,
    mut options: AnalysisOptions,
    mut flags: CompileFlags,
) -> Result<(), Box<dyn Error>> {
    let mut entry = None;
    let mut roots = Vec::new();
    let mut esm = false;
    let mut reloadable = false;
    let mut out_dir = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--esm") => esm = true,
            Some("--reloadable") => reloadable = true,
            Some("--out-dir") => {
                out_dir = Some(PathBuf::from(
                    args.next().ok_or("--out-dir requires a directory")?,
                ));
            }
            Some(flag) if flags.accept(flag, &mut args)? => {}
            Some("--entry") => {
                if entry.is_some() {
                    return Err("--entry may only be specified once".into());
                }
                entry = Some(
                    args.next()
                        .ok_or("--entry requires a namespace")?
                        .into_string()
                        .map_err(|_| "entry namespace must be valid UTF-8")?,
                );
            }
            Some("--source-root") => roots.push(PathBuf::from(
                args.next().ok_or("--source-root requires a directory")?,
            )),
            _ => return Err(USAGE.into()),
        }
    }
    let entry = entry.ok_or("namespace compilation requires --entry NAMESPACE")?;
    if reloadable && out_dir.is_none() {
        return Err("--reloadable requires --out-dir DIR".into());
    }
    // Without --reloadable, --out-dir writes a chunked build, ES modules
    // without source maps.
    if !reloadable && out_dir.is_some() {
        if flags.source_map.is_some() || flags.source_map_inline || flags.include_sources {
            return Err("--out-dir writes one ES module per chunk without source maps; it excludes --source-map, --source-map-inline and --source-map-include-sources".into());
        }
        esm = true;
    }
    if reloadable {
        if esm
            || flags.optimize
            || flags.size_report
            || flags.pseudo_names
            || !flags.defines.is_empty()
            || flags.source_map.is_some()
            || flags.source_map_inline
            || flags.include_sources
        {
            return Err("--reloadable writes the dev build at none semantics with source maps; it excludes --esm, --optimize, --size-report, --pseudo-names, --define and the --source-map flags".into());
        }
        // Dev compilation reports undeclared Vars and arity conflicts as
        // warnings (docs/dev-workflow.md, "Dev-mode contract").
        options.warning_mode = true;
    }
    let mut warnings = Vec::new();
    let program = light_cljs::compiler::analyze_namespace_entry_with_sources(
        &entry,
        &roots,
        &mut flags.host(),
        options,
        &mut warnings,
    );
    if options.warning_mode {
        print_warnings(&warnings);
    }
    let (program, files) = program?;
    if let (true, Some(out_dir)) = (reloadable, &out_dir) {
        return write_dev_build(&program, &files, out_dir);
    }
    let prepared = prepare_application(&program, flags.options(&program)?);
    if let Some(out_dir) = out_dir {
        write_chunks(&prepared.emit_chunks(), &out_dir)?;
    } else {
        let sources = files
            .iter()
            .map(|(path, text)| (path.as_path(), text.as_str()))
            .collect::<Vec<_>>();
        let output = flags.output(&prepared, esm, &sources)?;
        io::stdout().lock().write_all(output.as_bytes())?;
    }
    if flags.size_report {
        print_size_report(&prepared)?;
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let mut first = args.next();
    let mut options = AnalysisOptions::default();
    loop {
        match first.as_deref().and_then(|arg| arg.to_str()) {
            Some("--warnings") => options.warning_mode = true,
            Some("--checked-arrays-warn") => {
                options.warning_mode = true;
                options.checked_arrays = true;
                options.array_checks = light_cljs::analyzer::CheckedArrays::Warn;
            }
            Some("--checked-arrays-error") => {
                options.warning_mode = true;
                options.checked_arrays = true;
                options.array_checks = light_cljs::analyzer::CheckedArrays::Error;
            }
            _ => break,
        }
        first = args.next();
    }
    if let Some(mode @ ("--eval" | "--repl")) = first.as_deref().and_then(|arg| arg.to_str()) {
        return eval_cli::run(mode == "--repl", args);
    }
    if first.as_deref().and_then(|arg| arg.to_str()) == Some("--watch") {
        return dev_watch::run(args);
    }
    let mode = match first.as_deref().and_then(|arg| arg.to_str()) {
        Some("--parse") => "parse",
        Some("--analyze") => "analyze",
        Some("--compile") => "compile",
        _ => "tokens",
    };
    if mode != "tokens" {
        first = args.next();
    }
    let mut flags = CompileFlags::default();
    while let Some(
        flag @ ("--optimize"
        | "--size-report"
        | "--pseudo-names"
        | "--define"
        | "--elide-asserts"
        | "--source-map"
        | "--source-map-inline"
        | "--source-map-include-sources"),
    ) = first.as_deref().and_then(|arg| arg.to_str())
    {
        if mode != "compile" {
            return Err(format!("{flag} requires --compile").into());
        }
        flags.accept(flag, &mut args)?;
        first = args.next();
    }
    if matches!(
        first.as_deref().and_then(|arg| arg.to_str()),
        Some("--entry" | "--source-root" | "--esm" | "--reloadable" | "--out-dir")
    ) {
        if mode != "compile" {
            return Err("namespace options require --compile".into());
        }
        return run_namespace(first.into_iter().chain(args), options, flags);
    }
    let source_name = match first.as_deref() {
        Some(arg) if arg == "--text" => "<text>.cljc".to_owned(),
        Some(arg) if arg != "-" => arg.to_string_lossy().into_owned(),
        _ => "<stdin>.cljc".to_owned(),
    };
    let source = match first.as_deref() {
        Some(arg) if arg == "--help" || arg == "-h" => {
            println!(
                "{USAGE}\nPrepend --warnings for ClojureScript warning-compatible analysis (undeclared Vars and conflicting arities are warnings).\nPrepend --checked-arrays-warn or --checked-arrays-error for array operand diagnostics and runtime warning/error checks.\n\n{}\n\n{}",
                eval_cli::USAGE,
                dev_watch::USAGE
            );
            return Ok(());
        }
        Some(arg) if arg == "--text" => args
            .next()
            .ok_or("--text requires a source string")?
            .into_string()
            .map_err(|_| "source must be valid UTF-8")?,
        Some(arg) if arg != "-" => fs::read_to_string(arg)?,
        _ => {
            let mut source = String::new();
            io::stdin().read_to_string(&mut source)?;
            source
        }
    };
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some(flag) if mode == "compile" && flags.accept(flag, &mut args)? => {}
            _ => return Err(USAGE.into()),
        }
    }

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    if mode != "tokens" {
        let tree = parse(&source).map_err(|error| {
            let lines = LineIndex::new(&source);
            let (line, column) = lines.line_column(error.span.start).unwrap();
            let mut message = format!(
                "{line}:{column}: {error} (bytes {}..{})",
                error.span.start, error.span.end
            );
            if let Some(span) = error.related {
                let (line, column) = lines.line_column(span.start).unwrap();
                message.push_str(&format!(
                    "; opened at {line}:{column} (bytes {}..{})",
                    span.start, span.end
                ));
            }
            message
        })?;
        if mode != "parse" {
            let mut warnings = Vec::new();
            let program = analyze_named_with_macro_warnings(
                &source_name,
                &source,
                &tree,
                &mut flags.host(),
                options,
                &mut warnings,
            );
            print_warnings(&warnings);
            let program = program.map_err(|error| {
                let (line, column) = LineIndex::new(&source)
                    .line_column(error.span.start)
                    .unwrap();
                format!(
                    "{line}:{column}: {error} (bytes {}..{})",
                    error.span.start, error.span.end
                )
            })?;
            if mode == "compile" {
                let prepared = prepare_application(&program, flags.options(&program)?);
                let sources = [(Path::new(&source_name), source.as_str())];
                write!(out, "{}", flags.output(&prepared, false, &sources)?)?;
                if flags.size_report {
                    out.flush()?;
                    print_size_report(&prepared)?;
                }
            } else {
                writeln!(out, "{program:#?}")?;
            }
            out.flush()?;
            return Ok(());
        }
        writeln!(out, "roots: {:?}", tree.roots)?;
        for (id, node) in tree.nodes.iter().enumerate() {
            writeln!(
                out,
                "{id}\t{:?}\t{}..{}\t{:?}",
                node.kind,
                node.span.start,
                node.span.end,
                &source[node.span.clone()]
            )?;
        }
        out.flush()?;
        return Ok(());
    }
    let mut has_errors = false;
    for token in Tokenizer::new(&source) {
        has_errors |= matches!(token.kind, TokenKind::Error(_));
        writeln!(
            out,
            "{:?}\t{}..{}\t{:?}",
            token.kind,
            token.span.start,
            token.span.end,
            &source[token.span.clone()]
        )?;
    }
    out.flush()?;
    if has_errors {
        return Err("lexical errors encountered; see token spans above".into());
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("light-cljs: {error}");
            ExitCode::FAILURE
        }
    }
}
