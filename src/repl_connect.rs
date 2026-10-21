//! `--repl --connect HOST:PORT`, the connected REPL's terminal client
//! (docs/dev-workflow.md, "The connected REPL"): it reads forms from stdin as
//! the embedded REPL does, posts the text of each to the dev server of
//! `--watch --serve` (`POST /repl`), which compiles it with the watch's
//! session and runs it in a connected runtime, and prints what comes back:
//! the value as the runtime printed it on stdout, warnings and errors on
//! stderr. Reader errors are found here; analysis errors come from the
//! server, runtime errors from the runtime with their stacks mapped to the
//! sources.
//!
//! `light-cljs cljs-repl ID` runs the same client on the port of a running
//! `light-cljs watch`, with its session bound to build ID before the first
//! form is read, as if that form were `(light.dev/repl :ID)`
//! (docs/build-configuration.md, "Commands").
use light_cljs::dev_server::Json;
use light_cljs::eval_session::PrintOptions;
use light_cljs::{ErrorKind, LexError, ParseError, Parser};
use std::{
    error::Error,
    io::{self, BufRead, IsTerminal, Read, Write},
    net::TcpStream,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// The name the forms are read under, as the embedded REPL names stdin.
const FILE: &str = "<stdin>.cljc";

/// The dev server at `address` and the REPL session it gave this client.
pub(crate) struct Connection {
    address: String,
    session: String,
    namespace: String,
    /// The build `(light.dev/repl :ID)` bound the session to, which a
    /// session opened again is bound to as well; `None` for the server's
    /// first build, where `--repl --connect` starts.
    build: Option<String>,
}

/// Why a REPL session could not be opened.
pub(crate) enum OpenError {
    /// No answer: the connection failed.
    Io(io::Error),
    /// The server's answer, which opened no session.
    Answer(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Answer(text) => f.write_str(text),
        }
    }
}

/// What the text read so far holds next.
enum Next {
    /// Complete forms, the text up to this byte: what precedes them
    /// (whitespace, comments, discarded forms) goes with them, and the
    /// server reads them as the embedded REPL does, where a reader prefix
    /// over a conditional that selects nothing takes the next form.
    Forms(usize),
    /// Only whitespace and comments.
    Empty,
    /// A form that more input may complete.
    Incomplete,
    Error(ParseError),
}

fn next(text: &str) -> Next {
    let mut parser = Parser::new(text).retaining_discards();
    let mut end = None;
    loop {
        match parser.read_form() {
            Ok(Some(id)) => end = Some(parser.nodes()[id.0].span.end),
            Ok(None) => return end.map_or(Next::Empty, Next::Forms),
            Err(error) => {
                return match end {
                    Some(end) => Next::Forms(end),
                    None if matches!(
                        error.kind,
                        ErrorKind::UnexpectedEof(_)
                            | ErrorKind::Lexical(
                                LexError::UnterminatedString | LexError::UnterminatedRegex
                            )
                    ) =>
                    {
                        Next::Incomplete
                    }
                    None => Next::Error(error),
                };
            }
        }
    }
}

/// The text read and not yet evaluated, with the line and column of its
/// first byte in the input.
struct Pending {
    text: String,
    line: usize,
    column: usize,
}

impl Pending {
    /// The line and column of byte `at` of the text.
    fn position(&self, at: usize) -> (usize, usize) {
        let before = &self.text[..at];
        match before.rfind('\n') {
            Some(newline) => (self.line + before.matches('\n').count(), at - newline),
            None => (self.line, self.column + at),
        }
    }

    /// Drop the first `end` bytes.
    fn consume(&mut self, end: usize) {
        (self.line, self.column) = self.position(end);
        self.text.drain(..end);
    }
}

pub fn run(address: &str, print: &PrintOptions) -> Result<()> {
    let connection = Connection::open(address)
        .map_err(|error| format!("cannot open a REPL session at {address}: {error}"))?;
    repl(connection, print)
}

/// Read forms from stdin and evaluate each on `connection`'s session until
/// the input ends, which closes the session.
pub(crate) fn repl(mut connection: Connection, print: &PrintOptions) -> Result<()> {
    let prompt = io::stdin().is_terminal();
    let mut input = io::stdin().lock();
    let mut pending = Pending {
        text: String::new(),
        line: 1,
        column: 1,
    };
    let mut eof = false;
    loop {
        if prompt {
            eprint!("{}=> ", connection.namespace);
            io::stderr().flush()?;
        }
        let mut continued = false;
        let form = loop {
            match next(&pending.text) {
                Next::Forms(end) => break Some(end),
                Next::Error(error) => {
                    let (line, column) = pending.position(error.span.start);
                    let mut message = error.to_string();
                    if let Some(open) = &error.related {
                        let (line, column) = pending.position(open.start);
                        message.push_str(&format!("; opened at {FILE}:{line}:{column}"));
                    }
                    eprintln!("light-cljs: {FILE}:{line}:{column}: {message}");
                    // A reader error discards the text read after the form.
                    pending.consume(pending.text.len());
                    break None;
                }
                Next::Incomplete if eof => {
                    let (line, column) = pending.position(pending.text.len());
                    return Err(format!(
                        "{FILE}:{line}:{column}: unexpected end of input in an unfinished form"
                    )
                    .into());
                }
                Next::Empty if eof => {
                    connection.close();
                    return Ok(());
                }
                Next::Incomplete | Next::Empty => {
                    if prompt && continued {
                        eprint!("...=> ");
                        io::stderr().flush()?;
                    }
                    continued = true;
                    if input.read_line(&mut pending.text)? == 0 {
                        eof = true;
                    }
                }
            }
        };
        let Some(end) = form else { continue };
        let (line, column) = (pending.line, pending.column);
        let code = pending.text[..end].to_owned();
        pending.consume(end);
        connection.evaluate(&code, line, column, print)?;
    }
}

impl Connection {
    pub(crate) fn open(address: &str) -> std::result::Result<Self, OpenError> {
        let (status, body) = post(address, r#"{"op":"open"}"#).map_err(OpenError::Io)?;
        let json = response(status, &body).map_err(OpenError::Answer)?;
        let field = |key: &str| {
            json.get(key)
                .and_then(Json::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    OpenError::Answer(format!("the server's answer has no {key}: {body}"))
                })
        };
        Ok(Self {
            address: address.to_owned(),
            session: field("session")?,
            namespace: field("namespace")?,
            build: None,
        })
    }

    /// Bind the session to `build`, as `(light.dev/repl :BUILD)` typed as
    /// its first form does, printing nothing: the session moves to that
    /// build's entry namespace and its most recently connected runtime. An
    /// error is the server's message, such as that of an id the watch does
    /// not run.
    pub(crate) fn bind(
        &mut self,
        build: &str,
        print: &PrintOptions,
    ) -> std::result::Result<(), String> {
        let code = format!("(light.dev/repl :{build})");
        let (status, body) = post(&self.address, &self.request(&code, 1, 1, print))
            .map_err(|error| format!("cannot reach the dev server at {}: {error}", self.address))?;
        let json = response(status, &body)?;
        if let Some(Json::Array(results)) = json.get("results")
            && let Some(error) = results.iter().find_map(|result| result.get("error"))
        {
            let diagnostic = error.get("diagnostic").unwrap_or(&Json::Null);
            return Err(diagnostic
                .get("message")
                .and_then(Json::as_str)
                .unwrap_or_default()
                .to_owned());
        }
        if let Some(namespace) = json.get("namespace").and_then(Json::as_str) {
            self.namespace = namespace.to_owned();
        }
        self.build = Some(build.to_owned());
        Ok(())
    }

    /// The body of a request to evaluate `code`, a form at `line` and
    /// `column` of the input, on the session.
    fn request(&self, code: &str, line: usize, column: usize, print: &PrintOptions) -> String {
        let optional = |value: Option<usize>| value.map_or(Json::Null, |n| Json::Number(n as f64));
        let mut body = String::new();
        Json::Object(vec![
            ("op".into(), Json::String("eval".into())),
            ("session".into(), Json::String(self.session.clone())),
            ("code".into(), Json::String(code.into())),
            ("file".into(), Json::String(FILE.into())),
            ("line".into(), Json::Number(line as f64)),
            ("column".into(), Json::Number(column as f64)),
            (
                "print".into(),
                Json::Object(vec![
                    ("readably".into(), Json::Bool(print.readably)),
                    ("length".into(), optional(print.length)),
                    ("level".into(), optional(print.level)),
                    ("meta".into(), Json::Bool(print.meta)),
                    ("namespace-maps".into(), Json::Bool(print.namespace_maps)),
                    ("dup".into(), Json::Bool(print.dup)),
                ]),
            ),
        ])
        .write(&mut body);
        body
    }

    pub(crate) fn close(&self) {
        let body = format!(r#"{{"op":"close","session":"{}"}}"#, self.session);
        let _ = post(&self.address, &body);
    }

    /// Evaluate `code`, a form at `line` and `column` of the input, and
    /// print its results. A server that has forgotten the session, after a
    /// restart, gets a new one and the form again.
    fn evaluate(
        &mut self,
        code: &str,
        line: usize,
        column: usize,
        print: &PrintOptions,
    ) -> Result<()> {
        let mut answer = post(&self.address, &self.request(code, line, column, print));
        if matches!(answer, Ok((404, _))) {
            match Self::open(&self.address) {
                Ok(mut connection) => {
                    if let Some(build) = &self.build
                        && let Err(error) = connection.bind(build, print)
                    {
                        connection.close();
                        return Err(format!(
                            "the dev server forgot this REPL's session, and a new one cannot be bound to :{build}: {error}"
                        )
                        .into());
                    }
                    eprintln!(
                        "light-cljs: the dev server forgot this REPL's session; a new one starts in {}",
                        connection.namespace
                    );
                    *self = connection;
                    answer = post(&self.address, &self.request(code, line, column, print));
                }
                Err(error) => {
                    eprintln!("light-cljs: {error}");
                    return Ok(());
                }
            }
        }
        let json = match answer
            .map_err(|error| format!("cannot reach the dev server at {}: {error}", self.address))
            .and_then(|(status, body)| response(status, &body))
        {
            Ok(json) => json,
            Err(error) => {
                eprintln!("light-cljs: {error}");
                return Ok(());
            }
        };
        if let Some(namespace) = json.get("namespace").and_then(Json::as_str) {
            self.namespace = namespace.to_owned();
        }
        let Some(Json::Array(results)) = json.get("results") else {
            return Ok(());
        };
        let mut out = io::stdout().lock();
        for result in results {
            if let Some(Json::Array(warnings)) = result.get("warnings") {
                for warning in warnings {
                    eprintln!("WARNING: {}", located(warning));
                }
            }
            if let Some(value) = result.get("value").and_then(Json::as_str) {
                writeln!(out, "{value}")?;
                out.flush()?;
                if let Some(Json::Number(reference)) = result.get("ref") {
                    eprintln!(";; ref {reference}");
                }
            }
            if let Some(error) = result.get("error") {
                let diagnostic = error.get("diagnostic").unwrap_or(&Json::Null);
                let mut text = located(diagnostic);
                // A stack names the error, as the embedded REPL prints it,
                // and its frames.
                if let Some(stack) = error.get("stack").and_then(Json::as_str)
                    && let Some(message) = diagnostic.get("message").and_then(Json::as_str)
                    && stack.contains(message)
                {
                    text.truncate(text.len() - message.len());
                    text.push_str(stack);
                }
                if let Some(data) = error.get("data").and_then(Json::as_str) {
                    text.push_str("\n  data: ");
                    text.push_str(data);
                }
                eprintln!("light-cljs: {text}");
            }
        }
        Ok(())
    }
}

/// A diagnostic of the server as `FILE:LINE:COLUMN: MESSAGE`.
fn located(diagnostic: &Json) -> String {
    let message = diagnostic
        .get("message")
        .and_then(Json::as_str)
        .unwrap_or_default();
    let mut parts = Vec::new();
    if let Some(file) = diagnostic.get("file").and_then(Json::as_str) {
        parts.push(file.to_owned());
    }
    for key in ["line", "column"] {
        if let Some(Json::Number(n)) = diagnostic.get(key) {
            parts.push(n.to_string());
        }
    }
    if parts.is_empty() {
        message.to_owned()
    } else {
        format!("{}: {message}", parts.join(":"))
    }
}

/// The JSON of a `200 OK` answer, or what the server said instead.
fn response(status: u16, body: &str) -> std::result::Result<Json, String> {
    if status != 200 {
        return Err(format!(
            "the dev server answered {status}: {}",
            body.trim_end()
        ));
    }
    Json::parse(body).map_err(|error| format!("the dev server's answer is not JSON: {error}"))
}

/// `POST /repl` of `body` to `address`: the status and the body of the
/// answer, which the server ends by closing the connection.
fn post(address: &str, body: &str) -> io::Result<(u16, String)> {
    let mut stream = TcpStream::connect(address)?;
    stream.set_nodelay(true)?;
    stream.write_all(
        format!(
            "POST /repl HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer)?;
    let answer = String::from_utf8_lossy(&answer);
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "a malformed HTTP answer");
    let (head, body) = answer.split_once("\r\n\r\n").ok_or_else(invalid)?;
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|status| status.parse().ok())
        .ok_or_else(invalid)?;
    Ok((status, body.to_owned()))
}
