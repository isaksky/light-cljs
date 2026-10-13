use std::{
    env,
    error::Error,
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use light_cljs::analyzer::{AnalysisOptions, AnalysisWarning, analyze_named_with_warnings};
use light_cljs::{
    ApplicationOptions, LineIndex, MacroHost, OptimizationLevel, TokenKind, Tokenizer, parse,
    prepare_application,
};

mod eval_cli;

const USAGE: &str = "Usage: light-cljs [--parse|--analyze|--compile] [FILE|-]\n       light-cljs [--parse|--analyze|--compile] --text SOURCE\n       light-cljs --compile --entry NAMESPACE --source-root DIR [--source-root DIR ...] [--esm]\n       light-cljs --compile --optimize none|prepare|advanced ...\n\nRead UTF-8 from FILE or stdin. By default print raw tokens and byte spans.\nWith --parse, print syntax nodes, child IDs, spans, and source text.\nWith --analyze, print resolved expressions and binding IDs.\nWith --compile, emit a JavaScript expression evaluating to the last form.\nWith --optimize LEVEL after --compile, choose application optimization: none emits every definition as analyzed, prepare (the default) removes unused inert definitions and specializes calls, and advanced is the release mode (currently the same as prepare).\nWith --entry, compile the namespace and its dependencies from explicit source roots.\nJavaScript package requires such as [\"react\" :as r] read globalThis.lightCljsPackages[\"react\"], supplied by the host.\nWith --esm, emit one ES module for a bundler instead: packages become static imports and the entry value is the default export.\nErrors cause a nonzero exit status; diagnostics include line/byte-column.";

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

fn run_namespace(
    mut args: impl Iterator<Item = std::ffi::OsString>,
    options: AnalysisOptions,
    mut level: OptimizationLevel,
) -> Result<(), Box<dyn Error>> {
    let mut entry = None;
    let mut roots = Vec::new();
    let mut esm = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--esm") => esm = true,
            Some("--optimize") => level = optimization_level(&mut args)?,
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
    let mut warnings = Vec::new();
    let program = light_cljs::compiler::analyze_namespace_entry(
        &entry,
        &roots,
        &mut MacroHost::new(),
        options,
        &mut warnings,
    );
    if options.warning_mode {
        print_warnings(&warnings);
    }
    let prepared = prepare_application(&program?, ApplicationOptions::new(level));
    let output = if esm {
        prepared.emit_esm()
    } else {
        prepared.emit_js()
    };
    io::stdout().lock().write_all(output.as_bytes())?;
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
    let mode = match first.as_deref().and_then(|arg| arg.to_str()) {
        Some("--parse") => "parse",
        Some("--analyze") => "analyze",
        Some("--compile") => "compile",
        _ => "tokens",
    };
    if mode != "tokens" {
        first = args.next();
    }
    let mut level = OptimizationLevel::default();
    while first.as_deref().and_then(|arg| arg.to_str()) == Some("--optimize") {
        if mode != "compile" {
            return Err("--optimize requires --compile".into());
        }
        level = optimization_level(&mut args)?;
        first = args.next();
    }
    if matches!(
        first.as_deref().and_then(|arg| arg.to_str()),
        Some("--entry" | "--source-root" | "--esm")
    ) {
        if mode != "compile" {
            return Err("namespace options require --compile".into());
        }
        return run_namespace(first.into_iter().chain(args), options, level);
    }
    let source_name = match first.as_deref() {
        Some(arg) if arg == "--text" => "<text>.cljc".to_owned(),
        Some(arg) if arg != "-" => arg.to_string_lossy().into_owned(),
        _ => "<stdin>.cljc".to_owned(),
    };
    let source = match first.as_deref() {
        Some(arg) if arg == "--help" || arg == "-h" => {
            println!(
                "{USAGE}\nPrepend --warnings for ClojureScript warning-compatible analysis (undeclared Vars and conflicting arities are warnings).\nPrepend --checked-arrays-warn or --checked-arrays-error for array operand diagnostics and runtime warning/error checks.\n\n{}",
                eval_cli::USAGE
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
        if mode != "compile" || arg != "--optimize" {
            return Err(USAGE.into());
        }
        level = optimization_level(&mut args)?;
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
            let program =
                analyze_named_with_warnings(&source_name, &source, &tree, options, &mut warnings);
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
                let prepared = prepare_application(&program, ApplicationOptions::new(level));
                write!(out, "{}", prepared.emit_js())?;
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
