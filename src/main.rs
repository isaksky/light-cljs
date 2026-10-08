use std::{
    env,
    error::Error,
    fs,
    io::{self, Read, Write},
    process::ExitCode,
};

use light_cljs::{TokenKind, Tokenizer};

const USAGE: &str = "Usage: light-cljs [FILE|-]\n       light-cljs --text SOURCE\n\nRead UTF-8 from FILE or stdin and print token kinds, byte spans, and escaped text.\nPrints raw ClojureScript lexical tokens, including comments and reader prefixes.\nLexical errors are printed with spans and cause a nonzero exit status.";

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let first = args.next();
    let source = match first.as_deref() {
        Some(arg) if arg == "--help" || arg == "-h" => {
            println!("{USAGE}");
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
    if args.next().is_some() {
        return Err(USAGE.into());
    }

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
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
