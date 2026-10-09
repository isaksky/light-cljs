//! Standalone native modes. The compiler CLI deliberately keeps its old parser.
use std::{
    cell::Cell,
    error::Error,
    ffi::OsString,
    fs,
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
    rc::Rc,
};

use light_cljs::interpreter::{
    Context, ContextOptions, ErrorClass, EvalError, EvalErrorKind, EvalPhase, ExceptionCategory,
    FileSourceProvider, HostInput, InvokeError, Name, NativeError, Origin, PrintOptions,
    ResourceLimits, RootedValue, SourceSpan, Utf8Input, Utf8Output, ValueView, VarId,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub const USAGE: &str = "Native interpreter:\n  light-cljs --eval [OPTIONS] [FILE|-]\n  light-cljs --eval [OPTIONS] --text SOURCE\n  light-cljs --repl [OPTIONS]\n\n  --source-root DIR      Search explicit roots for required .clj/.cljc namespaces\n  --fuel N              Work units per operation (default 1000000)\n  --allocation-limit N  Semantic allocation units per operation (default 16000000)\n  --output-limit N      UTF-16 output bytes per operation (default 1000000)\n  --call-depth N        Call depth (default/cap 32)\n  --evaluation-depth N  Expression depth (default/cap 128)\n  --print-length N      Result items per collection (default 100)\n  --print-level N       Result collection depth (default 20)\n\n--eval prints the last value only after successful execution; program output is immediate.\n--repl reads stdin, retains namespaces and *1/*2/*3/*e, and continues after errors.\nPrompts appear on stderr only for terminal stdin. No Java or Node is used.\nEvaluation, streaming reads and result printing each have a fresh resource budget.";

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
    limits: ResourceLimits,
    print: PrintOptions,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            input: Input::Stdin,
            roots: Vec::new(),
            limits: ResourceLimits {
                fuel: Some(1_000_000),
                allocation: Some(16_000_000),
                output: Some(1_000_000),
                ..ResourceLimits::default()
            },
            print: PrintOptions {
                length: Some(100),
                level: Some(20),
                ..PrintOptions::default()
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
                "--fuel" => {
                    options.limits.fuel = Some(number(&mut args, flag)?);
                    continue;
                }
                "--allocation-limit" => {
                    options.limits.allocation = Some(number(&mut args, flag)?);
                    continue;
                }
                "--output-limit" => {
                    options.limits.output = Some(number(&mut args, flag)?);
                    continue;
                }
                "--call-depth" => {
                    options.limits.call_depth = size(&mut args, flag)?;
                    continue;
                }
                "--evaluation-depth" => {
                    options.limits.evaluation_depth = size(&mut args, flag)?;
                    continue;
                }
                "--print-length" => {
                    options.print.length = Some(size(&mut args, flag)?);
                    continue;
                }
                "--print-level" => {
                    options.print.level = Some(size(&mut args, flag)?);
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
                    return Err(format!("unexpected native option: {flag}\n{USAGE}").into());
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
    let Some(options) = options(repl, args)? else {
        return Ok(());
    };
    let mut context = Context::init(ContextOptions {
        limits: Some(options.limits),
        source_provider: (!options.roots.is_empty())
            .then(|| Some(Rc::new(FileSourceProvider::new(options.roots)) as Rc<_>)),
        ..ContextOptions::default()
    })?;
    let out = context.output_stream(Utf8Output(io::stdout()));
    let err = context.output_stream(Utf8Output(io::stderr()));
    // One owned buffering layer serves the REPL and interpreted input operations.
    let continuation = Rc::new(Cell::new(false));
    let input = context.input_stream(
        "<stdin>",
        TerminalInput {
            reader: Utf8Input(io::BufReader::new(io::stdin())),
            prompt: repl && io::stdin().is_terminal(),
            continuation: continuation.clone(),
        },
    );
    let _io = context.bind_io(Some(&input), Some(&out), Some(&err))?;
    if repl {
        return repl_loop(&mut context, &input, options.print, continuation);
    }
    let (name, text) = match options.input {
        Input::Text(text) => ("<text>".to_owned(), text),
        Input::File(path) => {
            let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            (path.to_string_lossy().into_owned(), text)
        }
        Input::Stdin => {
            let mut text = String::new();
            io::stdin().read_to_string(&mut text)?;
            ("<stdin>".to_owned(), text)
        }
    };
    let source = context.source(name, text);
    let value = context.eval_source(&source).map_err(|e| diagnostic(&e))?;
    let span = source.span(0..source.text().len())?;
    let text = render(&mut context, &value, options.print, &span).map_err(|e| diagnostic(&e))?;
    write_result(&text)
}

struct TerminalInput {
    reader: Utf8Input<io::BufReader<io::Stdin>>,
    prompt: bool,
    continuation: Rc<Cell<bool>>,
}
impl HostInput for TerminalInput {
    fn read_line(&mut self) -> io::Result<Option<Vec<u16>>> {
        if self.prompt && self.continuation.replace(true) {
            eprint!("...=> ");
            io::stderr().flush()?;
        }
        self.reader.read_line()
    }
}
fn render(
    context: &mut Context,
    value: &RootedValue,
    options: PrintOptions,
    span: &SourceSpan,
) -> std::result::Result<String, EvalError> {
    let rendered = context
        .print_value(value, options)
        .map_err(|error| print_error(error, span))?;
    let ValueView::String(text) = rendered.inspect() else {
        unreachable!("printer returns string")
    };
    String::from_utf16(&text)
        .map_err(|error| print_error(NativeError::Host(error.to_string()), span))
}
fn print_error(error: NativeError, span: &SourceSpan) -> EvalError {
    if let NativeError::Invocation(forwarded) = &error
        && let InvokeError::Execution(error) = forwarded.error()
    {
        return *error.clone();
    }
    EvalError {
        phase: EvalPhase::Execution,
        kind: EvalErrorKind::Native {
            name: Box::new(Name::new(None, "result printing")),
            error,
        },
        span: Box::new(span.clone()),
        related: None,
        origin: Origin::Source,
        frames: Vec::new(),
        exception: None,
    }
}
fn write_result(text: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{text}")?;
    stdout.flush()?;
    Ok(())
}
fn diagnostic(error: &EvalError) -> String {
    let bytes = error.span.bytes();
    let mut text = format!("{error} (bytes {}..{})", bytes.start, bytes.end);
    if let Some(span) = &error.related {
        let (line, column) = span.source().line_column(span.bytes().start).unwrap();
        text.push_str(&format!(
            "; opened at {}:{line}:{column}",
            span.source().name()
        ));
    }
    // A source excerpt also makes structured names and reader errors readable.
    let excerpt = error.span.text();
    if !excerpt.is_empty() {
        let excerpt: String = excerpt.chars().take(160).collect();
        text.push_str(&format!("\n  source: {excerpt:?}"));
    }
    text
}
fn history(context: &Context) -> Result<[VarId; 4]> {
    let mut vars = Vec::new();
    for name in ["*1", "*2", "*3", "*e"] {
        vars.push(
            context
                .resolve_var(&Name::new(Some("clojure.core"), name))?
                .expect("core history Var")
                .var_id()
                .unwrap(),
        );
    }
    Ok(vars.try_into().unwrap())
}
fn remember_error(context: &mut Context, var: VarId, error: &EvalError) -> Result<()> {
    let value = if let Some(value) = &error.exception {
        *value.clone()
    } else {
        let (line, column) = error
            .span
            .source()
            .line_column(error.span.bytes().start)
            .unwrap();
        let message = context.string(diagnostic(error).encode_utf16().collect::<Vec<_>>());
        let phase = match error.phase {
            EvalPhase::Read => "read",
            EvalPhase::Analysis => "analysis",
            EvalPhase::Macroexpansion => "macroexpansion",
            EvalPhase::Execution => "execution",
        };
        let data = context.map(&[
            (
                context.keyword(Name::new(None, "phase")),
                context.keyword(Name::new(None, phase)),
            ),
            (
                context.keyword(Name::new(None, "file")),
                context.string(
                    error
                        .span
                        .source()
                        .name()
                        .encode_utf16()
                        .collect::<Vec<_>>(),
                ),
            ),
            (
                context.keyword(Name::new(None, "line")),
                context.integer(line as i64),
            ),
            (
                context.keyword(Name::new(None, "column")),
                context.integer(column as i64),
            ),
        ])?;
        context.exception(
            ExceptionCategory::ExceptionInfo,
            &message,
            &data,
            &context.nil(),
        )?
    };
    context.set_var_binding(var, &value)?;
    Ok(())
}
fn repl_loop(
    context: &mut Context,
    input: &RootedValue,
    print: PrintOptions,
    continuation: Rc<Cell<bool>>,
) -> Result<()> {
    let vars = history(context)?;
    let bindings = vars.map(|id| (id, context.nil()));
    let _history = context.bind_vars(&bindings)?;
    loop {
        continuation.set(false);
        if io::stdin().is_terminal() {
            let ValueView::Namespace(name) = context.current_namespace().inspect() else {
                unreachable!()
            };
            eprint!("{}=> ", String::from_utf16_lossy(name.name()));
            io::stderr().flush()?;
        }
        let result = match context.read_input_form(input) {
            Ok(None) => break,
            Ok(Some(form)) => context.eval_form(&form).and_then(|value| {
                render(context, &value, print, &form.span()).map(|text| (value, text))
            }),
            Err(error) => {
                if matches!(error.kind.class(), ErrorClass::Control | ErrorClass::Host) {
                    return Err(diagnostic(&error).into());
                }
                Err(error)
            }
        };
        match result {
            Ok((value, text)) => {
                write_result(&text)?;
                context.set_var_binding(vars[2], &context.var_get(vars[1])?)?;
                context.set_var_binding(vars[1], &context.var_get(vars[0])?)?;
                context.set_var_binding(vars[0], &value)?;
            }
            Err(error) => {
                eprintln!("light-cljs: {}", diagnostic(&error));
                remember_error(context, vars[3], &error)?;
            }
        }
        // An interpreted catch may have handled a failed EDN read. Recover the
        // shared stream at every interaction boundary, including successful ones.
        context.recover_input(input)?;
        context.compact_input(input)?;
        context.collect_garbage();
    }
    Ok(())
}
