//! Compiler-backed scripts and incremental REPL with explicit standard streams.
use light_cljs::compiler_data::{Datum, DatumKind, Name};
use light_cljs::eval_session::PrintOptions;
use light_cljs::evaluator::{EvalError, ValueId};
use light_cljs::source_provider::{FileSourceProvider, SourceInput};
use light_cljs::{EvalDiagnostic, EvalOptions, EvalSession, UnhandledRejections};
use std::{
    cell::{Cell, RefCell},
    error::Error,
    ffi::OsString,
    fs,
    io::{self, BufRead, IsTerminal, Read, Write},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub const USAGE: &str = "Compiled JavaScript evaluation:
  light-cljs --eval [OPTIONS] [FILE|-]
  light-cljs --eval [OPTIONS] --text SOURCE
  light-cljs --repl [OPTIONS]

  --source-root DIR  Explicit .cljs/.cljc namespace roots (.cljs first)
  --poll-budget N    Cooperative engine/compiler polls per operation (1000000)
  --memory-limit N   JavaScript heap bytes (67108864)
  --stack-limit N    JavaScript stack bytes (1048576)
  --timeout-ms N     Operation deadline in milliseconds (10000)
  --source-limit N   Source bytes per input (4194304)
  --output-limit N   UTF-8 emitted/rendered bytes per operation (1048576)
  --print-length N   Collection result length (100)
  --print-level N    Collection result depth (20)
  --elide-asserts    Compile without assert and fn :pre/:post checks
  --warnings         Warning mode: undeclared Vars and conflicting arities are
                     warnings, printed to stderr, as with --compile --warnings

--eval prints the final value only after success; program output is immediate.
--repl retains namespaces and *1/*2/*3/*e and recovers language/reader errors.
Terminal prompts go to stderr. Terminal engine limits/cancellation exit nonzero.
Legacy --fuel/--allocation-limit/--call-depth/--evaluation-depth are rejected.
.clj inputs are retired: rename/adapt to .cljs or .cljc with :cljs conditionals.
Numbers use JavaScript semantics; ECMAScript intrinsics work. No Node/JVM process.
Package requires such as [\"react\" :as r] read globalThis.lightCljsPackages[\"react\"]
at each use; no package is loaded, so supply the registry first, for example with
(set! js/lightCljsPackages (js-obj \"react\" ...)).";
#[derive(Default)]
enum Input {
    #[default]
    Stdin,
    File(PathBuf),
    Text(String),
}
struct Options {
    input: Input,
    roots: Vec<PathBuf>,
    eval: EvalOptions,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            input: Input::Stdin,
            roots: Vec::new(),
            eval: EvalOptions {
                print: PrintOptions {
                    length: Some(100),
                    level: Some(20),
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }
}
fn argument(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<OsString> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value").into())
}
fn number(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<u64> {
    let value = argument(args, flag)?;
    let text = value.to_str().unwrap_or("");
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("{flag} requires a nonnegative integer").into());
    }
    text.parse()
        .map_err(|_| format!("{flag} value is too large").into())
}
fn size(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<usize> {
    usize::try_from(number(args, flag)?).map_err(|_| format!("{flag} value is too large").into())
}
fn options(repl: bool, mut args: impl Iterator<Item = OsString>) -> Result<Option<Options>> {
    let mut options = Options::default();
    let mut has_input = false;
    let mut positional = false;
    while let Some(arg) = args.next() {
        let flag = arg.to_str().unwrap_or("");
        if !positional {
            match flag {
                "--help" | "-h" => {
                    println!("{USAGE}");
                    return Ok(None);
                }
                "--" => {
                    positional = true;
                    continue;
                }
                "--source-root" => {
                    options.roots.push(argument(&mut args, flag)?.into());
                    continue;
                }
                "--poll-budget" => {
                    options.eval.engine.poll_budget = number(&mut args, flag)?;
                    continue;
                }
                "--memory-limit" => {
                    options.eval.engine.memory_limit = size(&mut args, flag)?;
                    continue;
                }
                "--stack-limit" => {
                    options.eval.engine.stack_limit = size(&mut args, flag)?;
                    continue;
                }
                "--timeout-ms" => {
                    options.eval.engine.timeout = Duration::from_millis(number(&mut args, flag)?);
                    continue;
                }
                "--source-limit" => {
                    options.eval.max_source_bytes = size(&mut args, flag)?;
                    continue;
                }
                "--output-limit" => {
                    options.eval.max_output_bytes = size(&mut args, flag)?;
                    continue;
                }
                "--fuel" | "--allocation-limit" | "--call-depth" | "--evaluation-depth" => {
                    let replacement = match flag {
                        "--fuel" => "--poll-budget",
                        "--allocation-limit" => "--memory-limit (JS heap bytes)",
                        _ => "--stack-limit (JS stack bytes)",
                    };
                    return Err(format!("{flag} is retired; use {replacement}; native resource units are not equivalent").into());
                }
                "--print-length" => {
                    options.eval.print.length = Some(size(&mut args, flag)?);
                    continue;
                }
                "--print-level" => {
                    options.eval.print.level = Some(size(&mut args, flag)?);
                    continue;
                }
                "--elide-asserts" => {
                    options.eval.assertions = false;
                    continue;
                }
                "--warnings" => {
                    options.eval.warning_mode = true;
                    options.eval.warnings = Some(Rc::new(|warning| {
                        let mut err = io::stderr().lock();
                        writeln!(err, "WARNING: {}", diagnostic(warning))
                            .and_then(|_| err.flush())
                            .map_err(|e| EvalError(e.to_string()))
                    }));
                    continue;
                }
                "--text" if !repl && !has_input => {
                    options.input = Input::Text(
                        argument(&mut args, flag)?
                            .into_string()
                            .map_err(|_| "source must be valid UTF-8")?,
                    );
                    has_input = true;
                    continue;
                }
                _ if flag.starts_with('-') && flag != "-" => {
                    return Err(format!("unexpected evaluation option: {flag}\n{USAGE}").into());
                }
                _ => (),
            }
        }
        if repl || has_input {
            return Err(format!(
                "unexpected input argument: {}\n{USAGE}",
                arg.to_string_lossy()
            )
            .into());
        }
        options.input = if arg == "-" {
            Input::Stdin
        } else {
            Input::File(arg.into())
        };
        has_input = true;
    }
    Ok(Some(options))
}

pub fn run(repl: bool, args: impl Iterator<Item = OsString>) -> Result<()> {
    let Some(mut options) = options(repl, args)? else {
        return Ok(());
    };
    let source = if repl {
        None
    } else {
        let (name, text) = match options.input {
            Input::Text(text) => ("<text>.cljc".to_owned(), text),
            Input::File(path) => {
                if path.extension().is_some_and(|s| s == "clj") {
                    return Err(".clj scripts are retired; rename/adapt to .cljs or .cljc using :cljs reader conditionals".into());
                }
                let text =
                    fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                (path.to_string_lossy().into_owned(), text)
            }
            Input::Stdin => {
                let mut text = String::new();
                io::stdin().read_to_string(&mut text)?;
                ("<stdin>.cljc".into(), text)
            }
        };
        Some(SourceInput::new(name, text))
    };
    let continuation = Rc::new(Cell::new(false));
    let reader = Rc::new(RefCell::new(io::BufReader::new(io::stdin())));
    let prompt = repl && io::stdin().is_terminal();
    let continued = continuation.clone();
    options.eval.input = Some(Rc::new(move |control| {
        control.poll()?;
        if prompt && continued.replace(true) {
            eprint!("...=> ");
            io::stderr().flush().map_err(|e| EvalError(e.to_string()))?;
        }
        let mut text = String::new();
        let n = reader
            .borrow_mut()
            .read_line(&mut text)
            .map_err(|e| EvalError(e.to_string()))?;
        Ok((n != 0).then_some(text))
    }));
    options.eval.output = Some(Rc::new(|text| {
        let mut out = io::stdout().lock();
        out.write_all(text.as_bytes())
            .and_then(|_| out.flush())
            .map_err(|e| EvalError(e.to_string()))
    }));
    options.eval.error_output = Some(Rc::new(|text| {
        let mut err = io::stderr().lock();
        err.write_all(text.as_bytes())
            .and_then(|_| err.flush())
            .map_err(|e| EvalError(e.to_string()))
    }));
    if !options.roots.is_empty() {
        options.eval.source_provider = Some(Rc::new(FileSourceProvider::new(options.roots)));
    }
    // A rejection no handler catches fails a script, as in Node, and is a
    // warning after the form's result in the REPL, as in Node's REPL.
    if !repl {
        options.eval.unhandled_rejections = UnhandledRejections::Fail;
    }
    let mut session = EvalSession::new(options.eval)?;
    if repl {
        return repl_loop(&mut session, continuation, prompt);
    }
    let source = source.unwrap();
    bind_file(&mut session, &source.file)?;
    let value = session
        .eval_source(&source)
        .map_err(|e| diagnostic(&e))?
        .value;
    let rendered = session.print(value).map_err(|e| diagnostic(&e))?;
    session.release(value)?;
    write_result(&rendered)
}
fn bind_file(session: &mut EvalSession, file: &str) -> Result<()> {
    let value = session.value(&Datum::new(DatumKind::String(
        file.encode_utf16().collect(),
    )))?;
    session.bind("cljs.core/*file*", value)?;
    session.release(value)?;
    Ok(())
}
fn write_result(text: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{text}")?;
    stdout.flush()?;
    Ok(())
}
pub(crate) fn diagnostic(error: &EvalDiagnostic) -> String {
    let mut text = error.to_string();
    if let Some(span) = &error.location {
        let bytes = span.bytes();
        text.push_str(&format!(" (bytes {}..{})", bytes.start, bytes.end));
    }
    text
}
fn remember_error(session: &mut EvalSession, error: &EvalDiagnostic, runtime: bool) -> Result<()> {
    let retained = if runtime {
        let value = session.last_exception()?;
        if matches!(session.data(value), Ok(data) if matches!(data.kind(), DatumKind::Nil)) {
            session.release(value)?;
            None
        } else {
            Some(value)
        }
    } else {
        None
    };
    let value = if let Some(value) = retained {
        value
    } else {
        let message = session.value(&Datum::new(DatumKind::String(
            diagnostic(error).encode_utf16().collect(),
        )))?;
        let phase = if runtime { "analysis" } else { "read" };
        let mut entries = vec![
            Datum::new(DatumKind::Keyword(Name::new(None, "phase"))),
            Datum::new(DatumKind::Keyword(Name::new(None, phase))),
        ];
        if let Some(span) = &error.location {
            let (line, column) = span.source().line_column(span.bytes().start).unwrap();
            for (key, value) in [
                (
                    "file",
                    DatumKind::String(span.source().name().encode_utf16().collect()),
                ),
                ("line", DatumKind::Double(line as f64)),
                ("column", DatumKind::Double(column as f64)),
            ] {
                entries.push(Datum::new(DatumKind::Keyword(Name::new(None, key))));
                entries.push(Datum::new(value));
            }
        }
        let data = session.value(&Datum::new(DatumKind::Collection {
            kind: light_cljs::CollectionKind::Map,
            children: entries.into(),
        }))?;
        let function = session.lookup("cljs.core/ex-info")?;
        let value = session.invoke(function, &[message, data])?;
        for id in [message, data, function] {
            session.release(id)?;
        }
        value
    };
    session.bind("cljs.core/*e", value)?;
    session.release(value)?;
    Ok(())
}
fn remember_result(session: &mut EvalSession, value: ValueId) -> Result<()> {
    let one = session.lookup("cljs.core/*1")?;
    let two = session.lookup("cljs.core/*2")?;
    session.bind("cljs.core/*3", two)?;
    session.bind("cljs.core/*2", one)?;
    session.bind("cljs.core/*1", value)?;
    for id in [one, two, value] {
        session.release(id)?;
    }
    Ok(())
}
fn repl_loop(session: &mut EvalSession, continuation: Rc<Cell<bool>>, prompt: bool) -> Result<()> {
    bind_file(session, "<stdin>.cljc")?;
    loop {
        continuation.set(false);
        if prompt {
            eprint!("{}=> ", session.namespace());
            io::stderr().flush()?;
        }
        let mut runtime = false;
        let result = match session.read_input_form() {
            Ok(None) => break,
            Ok(Some(form)) => {
                runtime = true;
                session
                    .eval_form(&form)
                    .and_then(|result| match session.print(result.value) {
                        Ok(text) => Ok((result.value, text)),
                        Err(error) => {
                            let _ = session.release(result.value);
                            Err(error)
                        }
                    })
            }
            Err(error) if error.location.is_none() => return Err(diagnostic(&error).into()),
            Err(error) => Err(error),
        };
        match result {
            Ok((value, text)) => {
                write_result(&text)?;
                remember_result(session, value)?;
            }
            Err(error) => {
                if error.message.contains("Maximum call stack size exceeded") {
                    return Err(
                        format!("{}; start a fresh --repl command", diagnostic(&error)).into(),
                    );
                }
                // Sticky engine failure cannot be repaired by catching a language exception.
                session
                    .control()
                    .poll()
                    .map_err(|e| format!("{e}; start a fresh --repl command"))?;
                eprintln!("light-cljs: {}", diagnostic(&error));
                remember_error(session, &error, runtime)?;
            }
        }
        for report in session.take_unhandled_rejections() {
            eprintln!("light-cljs: WARNING: {report}");
        }
        session.recover_input()?;
    }
    Ok(())
}
