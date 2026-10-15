//! The dev server of `--watch --serve` (docs/dev-workflow.md, "The server
//! and the client"): a subset of HTTP/1.1 on `std::net`, one thread per
//! connection, that serves a dev build to the runtimes running it.
//!
//! - `GET` (and `HEAD`) serves the files of the build directory and of the
//!   source roots, which the units' source maps name, with a content type
//!   and an ETag; a source root's files are served at the path its source
//!   map URLs resolve to from the directory. `GET /` serves the directory's
//!   `index.html`, or a built-in page that loads `index.js`, calls the
//!   program's value when it is a function, and shows console output.
//! - `GET /events` is a server-sent-events stream of messages, JSON objects
//!   with an `op` field, to one runtime: `hello` with the runtime ID the
//!   server gave the connection, then the build as it is now (a `load` with
//!   `replay`), so a runtime that connects late catches up, and the
//!   diagnostics of the last step; then each step's `warnings`, `error` and
//!   `load`, and the `invoke`s sent to that runtime.
//! - `POST /reply` takes a JSON object, the reply of a runtime to an
//!   `invoke` with its request ID and its runtime ID, and hands it to the
//!   requester when that runtime was the one asked (`409 Conflict` when
//!   another was, and the request still waits).
//!
//! JSON is written by hand and read by [`Json::parse`]. The dev client,
//! `client.js` of a dev build (src/dev_client.js), interprets the messages.
//! Loopback is the default address, since a connected runtime evaluates what
//! the server sends it; while bound to loopback, the server answers only
//! requests whose `Host` is a loopback name, so that a page of another site
//! cannot reach it through a DNS name that resolves to this machine.

use crate::dev_build::DevHook;
use crate::eval_session::PrintOptions;
use crate::source_map::json_string;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// One namespace's unit in a `load` message: the text of its file in the
/// build directory, which defines the namespace through
/// `globalThis.__lightDev.define`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadUnit {
    pub namespace: String,
    /// The unit's file in the build directory, which the client names as the
    /// evaluated text's `sourceURL`, so its `sourceMappingURL` resolves.
    pub file: String,
    pub js: String,
}

/// A compile diagnostic of an `error` or `warnings` message, located when
/// the compiler knows where.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub file: Option<String>,
    /// One-based.
    pub line: Option<usize>,
    /// One-based.
    pub column: Option<usize>,
}

/// The build as a runtime that connects now catches up with it: every
/// namespace's unit in load order and the hook functions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Build {
    pub units: Vec<LoadUnit>,
    pub before_load: Vec<DevHook>,
    pub after_load: Vec<DevHook>,
}

/// What a watch step produced, for [`DevServer::publish`].
#[derive(Clone, Debug, Default)]
pub struct Step {
    /// The units to load, in load order; none when the step failed.
    pub load: Vec<LoadUnit>,
    pub warnings: Vec<Diagnostic>,
    pub error: Option<Diagnostic>,
    /// The build after the step, when the step changed it.
    pub build: Option<Build>,
}

/// A running dev server. Its threads live as long as the process.
pub struct DevServer {
    shared: Arc<Shared>,
    address: SocketAddr,
}

struct Shared {
    out_dir: PathBuf,
    /// Each source root with the URL path segments its files are served
    /// under.
    roots: Vec<(PathBuf, Vec<String>)>,
    /// Whether to answer only requests for a loopback `Host`.
    loopback: bool,
    state: Mutex<State>,
}

struct State {
    build: Build,
    /// The number of steps published, the `step` of the last one's messages.
    step: u64,
    /// The last step's `warnings` and `error` messages, which a runtime
    /// receives when it connects.
    diagnostics: Vec<String>,
    next_runtime: u64,
    /// The connected runtimes, oldest first, with the senders of their
    /// streams.
    runtimes: Vec<(u64, Sender<String>)>,
    next_request: u64,
    /// The requests awaiting a reply, by request ID: the runtime asked and
    /// the requester's sender.
    pending: HashMap<String, (u64, Sender<Json>)>,
    /// Whether the server was dropped: it accepts no more connections.
    stopped: bool,
}

/// How long a stream waits before writing a comment line, which finds a
/// closed connection and keeps proxies from closing an idle one.
const HEARTBEAT: Duration = Duration::from_secs(15);
/// How long a connection may take to send its request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

impl DevServer {
    /// Listen on `host` and `port` (0 for any free port) and serve the dev
    /// build in `out_dir`, whose units' source maps name sources under
    /// `roots`, starting from `build`.
    pub fn start(
        host: &str,
        port: u16,
        out_dir: &Path,
        roots: &[PathBuf],
        build: Build,
    ) -> io::Result<Self> {
        let out_dir = std::fs::canonicalize(out_dir)?;
        let roots = roots
            .iter()
            .filter_map(|root| {
                let root = std::fs::canonicalize(root).ok()?;
                let prefix = served_prefix(&root, &out_dir)?;
                Some((root, prefix))
            })
            .collect();
        let addresses = (host, port).to_socket_addrs()?.collect::<Vec<_>>();
        let listener = TcpListener::bind(&addresses[..])?;
        let address = listener.local_addr()?;
        let shared = Arc::new(Shared {
            out_dir,
            roots,
            loopback: address.ip().is_loopback(),
            state: Mutex::new(State {
                build,
                step: 0,
                diagnostics: Vec::new(),
                next_runtime: 1,
                runtimes: Vec::new(),
                next_request: 1,
                pending: HashMap::new(),
                stopped: false,
            }),
        });
        let accepting = shared.clone();
        std::thread::Builder::new()
            .name("light-cljs dev server".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if accepting.state().stopped {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let shared = accepting.clone();
                    let _ = std::thread::Builder::new()
                        .name("light-cljs dev connection".into())
                        .spawn(move || {
                            let _ = connection(stream, &shared);
                        });
                }
            })?;
        Ok(Self { shared, address })
    }

    /// The address the server listens on.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// The IDs of the connected runtimes, oldest first.
    pub fn runtimes(&self) -> Vec<u64> {
        self.shared
            .state()
            .runtimes
            .iter()
            .map(|(id, _)| *id)
            .collect()
    }

    /// Send the messages of a step to every connected runtime, in the order
    /// `warnings`, `error`, `load`, and keep its build and diagnostics for
    /// runtimes that connect later. Returns the step's number.
    pub fn publish(&self, step: Step) -> u64 {
        let mut state = self.shared.state();
        state.step += 1;
        let number = state.step;
        let mut diagnostics = Vec::new();
        if !step.warnings.is_empty() {
            let mut message = format!("{{\"op\":\"warnings\",\"step\":{number},\"warnings\":[");
            for (index, warning) in step.warnings.iter().enumerate() {
                if index > 0 {
                    message.push(',');
                }
                write_diagnostic(&mut message, warning);
            }
            message.push_str("]}");
            diagnostics.push(message);
        }
        if let Some(error) = &step.error {
            let mut message = format!("{{\"op\":\"error\",\"step\":{number},\"error\":");
            write_diagnostic(&mut message, error);
            message.push('}');
            diagnostics.push(message);
        }
        if let Some(build) = step.build {
            state.build = build;
        }
        let mut messages = diagnostics.clone();
        if step.error.is_none() && !step.load.is_empty() {
            messages.push(load_message(
                number,
                false,
                &step.load,
                &state.build.before_load,
                &state.build.after_load,
            ));
        }
        state.diagnostics = diagnostics;
        for message in &messages {
            state.broadcast(message);
        }
        number
    }

    /// Send `unit`, a REPL form's unit, to the runtime `runtime`, or to the
    /// most recently connected one, to evaluate in its namespace and reply
    /// with the value printed by `pr-str` under `print`, or with the error
    /// it threw.
    pub fn invoke(
        &self,
        runtime: Option<u64>,
        unit: &crate::dev_session::Unit,
        print: &PrintOptions,
    ) -> Result<Invocation, String> {
        let mut state = self.shared.state();
        let target = match runtime {
            Some(id) => state.runtimes.iter().find(|(known, _)| *known == id),
            None => state.runtimes.last(),
        };
        let Some((runtime, sender)) = target.cloned() else {
            return Err(match runtime {
                Some(id) => format!("no runtime {id} is connected"),
                None => "no runtime is connected".to_owned(),
            });
        };
        let id = state.next_request.to_string();
        state.next_request += 1;
        let mut message = String::from("{\"op\":\"invoke\",\"id\":");
        json_string(&mut message, &id);
        message.push_str(",\"namespace\":");
        json_string(&mut message, &unit.namespace);
        message.push_str(",\"js\":");
        json_string(&mut message, &unit.js);
        let optional = |value: Option<usize>| value.map_or("null".to_owned(), |n| n.to_string());
        write!(
            message,
            ",\"print\":{{\"readably\":{},\"length\":{},\"level\":{},\"meta\":{},\"namespace-maps\":{},\"dup\":{}}}}}",
            print.readably,
            optional(print.length),
            optional(print.level),
            print.meta,
            print.namespace_maps,
            print.dup
        )
        .unwrap();
        let (reply, receiver) = channel();
        if sender.send(event(&message)).is_err() {
            return Err(format!("runtime {runtime} disconnected"));
        }
        state.pending.insert(id.clone(), (runtime, reply));
        Ok(Invocation {
            runtime,
            id,
            reply: receiver,
        })
    }
}

/// An `invoke` sent to a runtime.
pub struct Invocation {
    /// The runtime that evaluates it.
    pub runtime: u64,
    /// The request ID its reply names.
    pub id: String,
    /// The reply, the JSON object the runtime posted; its sender is dropped
    /// if the runtime disconnects or the server stops first.
    pub reply: Receiver<Json>,
}

impl Drop for DevServer {
    /// Stop listening and end every runtime's stream, so that a client
    /// connects again (to a server started on the port later, say).
    fn drop(&mut self) {
        let mut state = self.shared.state();
        state.stopped = true;
        for (_, sender) in state.runtimes.drain(..) {
            let _ = sender.send(String::new());
        }
        state.pending.clear();
        drop(state);
        // Wake the accepting thread, which sees `stopped` and drops the
        // listener.
        let mut wake = self.address;
        if wake.ip().is_unspecified() {
            wake.set_ip(match wake {
                SocketAddr::V4(_) => std::net::Ipv4Addr::LOCALHOST.into(),
                SocketAddr::V6(_) => std::net::Ipv6Addr::LOCALHOST.into(),
            });
        }
        let _ = TcpStream::connect_timeout(&wake, Duration::from_secs(1));
    }
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl State {
    /// Send `message` to every connected runtime, forgetting those whose
    /// streams have ended.
    fn broadcast(&mut self, message: &str) {
        let event = event(message);
        self.runtimes
            .retain(|(_, sender)| sender.send(event.clone()).is_ok());
    }
}

/// `message` as a server-sent event. JSON text holds no line break.
fn event(message: &str) -> String {
    format!("data: {message}\n\n")
}

fn write_diagnostic(out: &mut String, diagnostic: &Diagnostic) {
    out.push_str("{\"message\":");
    json_string(out, &diagnostic.message);
    out.push_str(",\"file\":");
    match &diagnostic.file {
        Some(file) => json_string(out, file),
        None => out.push_str("null"),
    }
    for (key, value) in [("line", diagnostic.line), ("column", diagnostic.column)] {
        match value {
            Some(value) => write!(out, ",\"{key}\":{value}").unwrap(),
            None => write!(out, ",\"{key}\":null").unwrap(),
        }
    }
    out.push('}');
}

/// A `load` message: define `units` in order between the hooks.
fn load_message(
    step: u64,
    replay: bool,
    units: &[LoadUnit],
    before_load: &[DevHook],
    after_load: &[DevHook],
) -> String {
    let mut message = format!("{{\"op\":\"load\",\"step\":{step},\"replay\":{replay},\"units\":[");
    for (index, unit) in units.iter().enumerate() {
        if index > 0 {
            message.push(',');
        }
        message.push_str("{\"namespace\":");
        json_string(&mut message, &unit.namespace);
        message.push_str(",\"file\":");
        json_string(&mut message, &unit.file);
        message.push_str(",\"js\":");
        json_string(&mut message, &unit.js);
        message.push('}');
    }
    message.push(']');
    for (key, hooks) in [("before-load", before_load), ("after-load", after_load)] {
        write!(message, ",\"{key}\":[").unwrap();
        for (index, hook) in hooks.iter().enumerate() {
            if index > 0 {
                message.push(',');
            }
            message.push_str("{\"name\":");
            json_string(&mut message, &hook.name);
            write!(message, ",\"async\":{}}}", hook.asynchronous).unwrap();
        }
        message.push(']');
    }
    message.push('}');
    message
}

/// The URL path segments under which the files of `root` are served: where
/// a source map in `out_dir` names them, by a URL relative to it
/// (`../src/app/view.cljs`), which resolves against the server's root by
/// dropping the leading `..` segments (`/src/app/view.cljs`). None for a
/// root that shares no ancestor with `out_dir` (another drive).
fn served_prefix(root: &Path, out_dir: &Path) -> Option<Vec<String>> {
    let parts = root.components().collect::<Vec<_>>();
    let base = out_dir.components().collect::<Vec<_>>();
    let common = parts.iter().zip(&base).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    parts[common..]
        .iter()
        .map(|part| part.as_os_str().to_str().map(str::to_owned))
        .collect()
}

/// A request: its method, its path (percent-decoded segments), its headers
/// with lowercase names, and its body.
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Read one request from `reader`; None when the connection closed first.
fn read_request(reader: &mut BufReader<TcpStream>) -> io::Result<Option<Request>> {
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let read = reader
            .by_ref()
            .take((MAX_HEADER_BYTES + 1 - head.len().min(MAX_HEADER_BYTES)) as u64)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            if head.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended",
            ));
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request head too long",
            ));
        }
        if line == b"\r\n" || line == b"\n" {
            if head.len() == line.len() {
                // A blank line before the request line is ignored.
                head.clear();
                continue;
            }
            break;
        }
    }
    let head = String::from_utf8(head)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request head is not UTF-8"))?;
    let mut lines = head.lines();
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "malformed request");
    let mut first = lines.next().ok_or_else(invalid)?.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (first.next(), first.next(), first.next(), first.next())
    else {
        return Err(invalid());
    };
    if !version.starts_with("HTTP/1.") {
        return Err(invalid());
    }
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let mut request = Request {
        method: method.to_owned(),
        target: target.to_owned(),
        headers,
        body: Vec::new(),
    };
    if request.header("transfer-encoding").is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "chunked request bodies are not supported",
        ));
    }
    if let Some(length) = request.header("content-length") {
        let length = length.parse::<usize>().map_err(|_| invalid())?;
        if length > MAX_BODY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request body too large",
            ));
        }
        request.body = vec![0; length];
        reader.read_exact(&mut request.body)?;
    }
    Ok(Some(request))
}

/// Write a complete response and close the connection.
fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        write!(head, "{name}: {value}\r\n").unwrap();
    }
    if !status.starts_with("204") && !status.starts_with("304") {
        write!(head, "Content-Length: {}\r\n", body.len()).unwrap();
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    if !head_only && !status.starts_with("204") && !status.starts_with("304") {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn respond_text(stream: &mut TcpStream, status: &str, text: &str) -> io::Result<()> {
    respond(
        stream,
        status,
        &[("Content-Type", "text/plain; charset=utf-8")],
        format!("{text}\n").as_bytes(),
        false,
    )
}

/// Serve one connection: one request, then close, except that an event
/// stream stays open until the runtime goes away.
fn connection(stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    // A peer that stops reading (a frozen tab) ends its connection rather
    // than holding the thread that writes to it.
    stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let request = match read_request(&mut reader) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error) => return respond_text(&mut writer, "400 Bad Request", &error.to_string()),
    };
    if shared.loopback && !request.header("host").is_some_and(loopback_host) {
        return respond_text(
            &mut writer,
            "403 Forbidden",
            "the dev server answers only requests for a loopback host",
        );
    }
    let path = request
        .target
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_owned();
    let head_only = request.method == "HEAD";
    match (request.method.as_str(), path.as_str()) {
        ("GET", "/events") => events(writer, shared),
        ("POST", "/reply") => reply(&mut writer, shared, &request.body),
        ("GET" | "HEAD", _) => file(&mut writer, shared, &request, &path, head_only),
        (_, "/reply") => respond_text(&mut writer, "405 Method Not Allowed", "POST a reply"),
        _ => respond_text(
            &mut writer,
            "405 Method Not Allowed",
            "the dev server answers GET, HEAD and POST /reply",
        ),
    }
}

/// Whether the `Host` header names this machine by a loopback name.
fn loopback_host(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        // [::1]:port
        rest.split(']').next().unwrap_or_default()
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    let name = name.to_ascii_lowercase();
    name == "localhost"
        || name.ends_with(".localhost")
        || name
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// `GET /events`: the runtime's stream.
fn events(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    let (sender, receiver) = channel();
    let (runtime, first) = {
        let mut state = shared.state();
        if state.stopped {
            drop(state);
            return respond_text(&mut stream, "503 Service Unavailable", "the server stopped");
        }
        let runtime = state.next_runtime;
        state.next_runtime += 1;
        let mut first = event(&format!("{{\"op\":\"hello\",\"runtime\":\"{runtime}\"}}"));
        if !state.build.units.is_empty() {
            first.push_str(&event(&load_message(
                state.step,
                true,
                &state.build.units,
                &state.build.before_load,
                &state.build.after_load,
            )));
        }
        for message in &state.diagnostics {
            first.push_str(&event(message));
        }
        state.runtimes.push((runtime, sender.clone()));
        (runtime, first)
    };
    // The runtime sends nothing on the stream: a read that ends finds that
    // it went away, and wakes the writer with an empty message.
    let mut closed = stream.try_clone()?;
    closed.set_read_timeout(None)?;
    std::thread::Builder::new()
        .name("light-cljs dev stream".into())
        .spawn(move || {
            let mut buffer = [0; 256];
            while matches!(closed.read(&mut buffer), Ok(read) if read > 0) {}
            let _ = sender.send(String::new());
        })?;
    let result = (|| -> io::Result<()> {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        )?;
        stream.write_all(first.as_bytes())?;
        stream.flush()?;
        loop {
            match receiver.recv_timeout(HEARTBEAT) {
                Ok(message) if message.is_empty() => return Ok(()),
                Ok(message) => stream.write_all(message.as_bytes())?,
                Err(RecvTimeoutError::Timeout) => stream.write_all(b": heartbeat\n\n")?,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
            stream.flush()?;
        }
    })();
    let mut state = shared.state();
    state.runtimes.retain(|(id, _)| *id != runtime);
    state.pending.retain(|_, (asked, _)| *asked != runtime);
    drop(state);
    let _ = stream.shutdown(std::net::Shutdown::Both);
    result
}

/// `POST /reply`: hand a runtime's reply to its requester.
fn reply(stream: &mut TcpStream, shared: &Shared, body: &[u8]) -> io::Result<()> {
    let json = std::str::from_utf8(body)
        .map_err(|_| "the reply is not UTF-8".to_owned())
        .and_then(Json::parse);
    let json = match json {
        Ok(json @ Json::Object(_)) => json,
        Ok(_) => return respond_text(stream, "400 Bad Request", "a reply is a JSON object"),
        Err(error) => return respond_text(stream, "400 Bad Request", &error),
    };
    let Some(id) = json.get("id").and_then(Json::as_str).map(str::to_owned) else {
        return respond_text(
            stream,
            "400 Bad Request",
            "a reply needs its request's \"id\"",
        );
    };
    let Some(runtime) = json
        .get("runtime")
        .and_then(Json::as_str)
        .map(str::to_owned)
    else {
        return respond_text(
            stream,
            "400 Bad Request",
            "a reply needs the \"runtime\" that sends it",
        );
    };
    // Only the runtime asked consumes the request.
    let mut state = shared.state();
    let pending = match state.pending.get(&id) {
        Some((asked, _)) if asked.to_string() == runtime => state.pending.remove(&id),
        Some((asked, _)) => {
            let message = format!("request {id} was sent to runtime {asked}");
            drop(state);
            return respond_text(stream, "409 Conflict", &message);
        }
        None => None,
    };
    drop(state);
    match pending {
        Some((_, sender)) => {
            let _ = sender.send(json);
            respond(stream, "204 No Content", &[], &[], false)
        }
        None => respond_text(
            stream,
            "404 Not Found",
            &format!("no request {id} awaits a reply"),
        ),
    }
}

/// `GET` of a file of the build directory or a source root, or of `/`.
fn file(
    stream: &mut TcpStream,
    shared: &Shared,
    request: &Request,
    path: &str,
    head_only: bool,
) -> io::Result<()> {
    let found = if path == "/" || path == "/index.html" {
        let page = shared.out_dir.join("index.html");
        if page.is_file() {
            Some(page)
        } else {
            return send(
                stream,
                request,
                "text/html; charset=utf-8",
                INDEX_PAGE.as_bytes(),
                head_only,
            );
        }
    } else {
        resolve(shared, path)
    };
    let Some(found) = found else {
        return respond_text(stream, "404 Not Found", &format!("no file {path}"));
    };
    match std::fs::read(&found) {
        Ok(bytes) => send(stream, request, content_type(&found), &bytes, head_only),
        Err(_) => respond_text(stream, "404 Not Found", &format!("no file {path}")),
    }
}

/// Send `bytes`, or `304 Not Modified` when the request's `If-None-Match`
/// names their ETag.
fn send(
    stream: &mut TcpStream,
    request: &Request,
    content_type: &str,
    bytes: &[u8],
    head_only: bool,
) -> io::Result<()> {
    let etag = format!("\"{:016x}\"", fnv1a(bytes));
    let matches = request.header("if-none-match").is_some_and(|tags| {
        tags.split(',').any(|tag| {
            let tag = tag.trim();
            tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag
        })
    });
    let headers = [
        ("Content-Type", content_type),
        ("ETag", etag.as_str()),
        ("Cache-Control", "no-cache"),
    ];
    if matches {
        respond(stream, "304 Not Modified", &headers, &[], head_only)
    } else {
        respond(stream, "200 OK", &headers, bytes, head_only)
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The file a request path names: under the build directory, or else under
/// a source root at its served prefix. A path with an empty, `.` or `..`
/// segment, or a segment that is not a plain file name, names nothing.
fn resolve(shared: &Shared, path: &str) -> Option<PathBuf> {
    let segments = path
        .strip_prefix('/')?
        .split('/')
        .map(percent_decode)
        .collect::<Option<Vec<_>>>()?;
    if segments.iter().any(|segment| {
        segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains(['/', '\\', '\0', ':'])
    }) {
        return None;
    }
    let join = |base: &Path, segments: &[String]| {
        let mut path = base.to_path_buf();
        path.extend(segments);
        path.is_file().then_some(path)
    };
    join(&shared.out_dir, &segments).or_else(|| {
        shared.roots.iter().find_map(|(root, prefix)| {
            segments
                .strip_prefix(prefix.as_slice())
                .filter(|rest| !rest.is_empty())
                .and_then(|rest| join(root, rest))
        })
    })
}

fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = segment.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("js" | "mjs" | "cjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("cljs" | "cljc" | "clj" | "edn" | "txt" | "md") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// The page `GET /` serves when the build directory has no `index.html`:
/// it shows console output, loads `index.js`, and calls the program's value
/// when it is a function, as a host calls an application's start function.
/// The dev client draws its overlay over it.
const INDEX_PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>light-cljs dev</title>
<style>
  body { margin: 0; font: 14px/1.4 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; background: #fafafa; color: #222; }
  header { padding: 8px 16px; background: #222; color: #eee; }
  #light-cljs-console { margin: 0; padding: 8px 16px; white-space: pre-wrap; }
  #light-cljs-console .warn { color: #8a6100; }
  #light-cljs-console .error { color: #b00020; }
</style>
</head>
<body>
<header>light-cljs dev build: console</header>
<pre id="light-cljs-console"></pre>
<script>
  (() => {
    const out = document.getElementById("light-cljs-console");
    const text = value => {
      if (typeof value === "string") return value;
      if (value instanceof Error) return value.stack || String(value);
      try { return String(value); } catch { return Object.prototype.toString.call(value); }
    };
    for (const level of ["log", "info", "warn", "error", "debug"]) {
      const original = console[level].bind(console);
      console[level] = (...values) => {
        original(...values);
        const line = document.createElement("div");
        line.className = level;
        line.textContent = values.map(text).join(" ");
        out.append(line);
      };
    }
  })();
</script>
<script src="index.js"></script>
<script>
  globalThis.__lightDevLoaded.then(
    value => { if (typeof value === "function") value(); },
    error => console.error(error));
</script>
</body>
</html>
"#;

/// A JSON value, as [`Json::parse`] reads it.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    /// Members in their order in the text.
    Object(Vec<(String, Json)>),
}

/// How deeply arrays and objects may nest, so that a hostile body cannot
/// exhaust the stack.
const MAX_JSON_DEPTH: usize = 512;

impl Json {
    /// Read `text`, one JSON value with only whitespace around it.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut parser = JsonParser {
            text: text.as_bytes(),
            at: 0,
        };
        let value = parser.value(0)?;
        parser.whitespace();
        if parser.at != parser.text.len() {
            return Err(parser.error("unexpected text after the value"));
        }
        Ok(value)
    }

    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    /// Append the value as JSON text.
    pub fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(value) => write!(out, "{value}").unwrap(),
            Json::Number(value) if !value.is_finite() => out.push_str("null"),
            Json::Number(value) => write!(out, "{value}").unwrap(),
            Json::String(text) => json_string(out, text),
            Json::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Object(members) => {
                out.push('{');
                for (index, (key, value)) in members.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    json_string(out, key);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

struct JsonParser<'a> {
    text: &'a [u8],
    at: usize,
}

impl JsonParser<'_> {
    fn error(&self, message: &str) -> String {
        format!("invalid JSON at byte {}: {message}", self.at)
    }

    fn whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.text.get(self.at) {
            self.at += 1;
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.text[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("unexpected character"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_JSON_DEPTH {
            return Err(self.error("nested too deeply"));
        }
        self.whitespace();
        match self.text.get(self.at) {
            None => Err(self.error("unexpected end")),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.whitespace();
                if self.text.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.whitespace();
                    match self.text.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return Err(self.error("expected , or ]")),
                    }
                }
            }
            Some(b'{') => {
                self.at += 1;
                let mut members = Vec::new();
                self.whitespace();
                if self.text.get(self.at) == Some(&b'}') {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                loop {
                    self.whitespace();
                    if self.text.get(self.at) != Some(&b'"') {
                        return Err(self.error("expected a member name"));
                    }
                    let key = self.string()?;
                    self.whitespace();
                    if self.text.get(self.at) != Some(&b':') {
                        return Err(self.error("expected :"));
                    }
                    self.at += 1;
                    members.push((key, self.value(depth + 1)?));
                    self.whitespace();
                    match self.text.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Json::Object(members));
                        }
                        _ => return Err(self.error("expected , or }")),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        let digits = |parser: &mut Self| {
            let begun = parser.at;
            while parser.text.get(parser.at).is_some_and(u8::is_ascii_digit) {
                parser.at += 1;
            }
            parser.at > begun
        };
        if self.text.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        if self.text.get(self.at) == Some(&b'0') {
            self.at += 1;
        } else if !digits(self) {
            return Err(self.error("expected a digit"));
        }
        if self.text.get(self.at) == Some(&b'.') {
            self.at += 1;
            if !digits(self) {
                return Err(self.error("expected a digit"));
            }
        }
        if let Some(b'e' | b'E') = self.text.get(self.at) {
            self.at += 1;
            if let Some(b'+' | b'-') = self.text.get(self.at) {
                self.at += 1;
            }
            if !digits(self) {
                return Err(self.error("expected a digit"));
            }
        }
        let text = std::str::from_utf8(&self.text[start..self.at]).unwrap();
        text.parse()
            .map(Json::Number)
            .map_err(|_| self.error("invalid number"))
    }

    fn hex4(&mut self) -> Result<u16, String> {
        let hex = self
            .text
            .get(self.at..self.at + 4)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u16::from_str_radix(hex, 16).ok())
            .ok_or_else(|| self.error("expected four hex digits"))?;
        self.at += 4;
        Ok(hex)
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let start = self.at;
            while let Some(&byte) = self.text.get(self.at) {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.at += 1;
            }
            // The text is a `str`, and the run stops at ASCII bytes only.
            out.push_str(std::str::from_utf8(&self.text[start..self.at]).unwrap());
            match self.text.get(self.at) {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = *self
                        .text
                        .get(self.at)
                        .ok_or_else(|| self.error("unterminated string"))?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let unit = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&unit)
                                && self.text[self.at..].starts_with(b"\\u")
                            {
                                let at = self.at;
                                self.at += 2;
                                let low = self.hex4()?;
                                if (0xdc00..0xe000).contains(&low) {
                                    0x10000
                                        + ((u32::from(unit) - 0xd800) << 10)
                                        + (u32::from(low) - 0xdc00)
                                } else {
                                    // A lone high surrogate; the next escape
                                    // is read on its own.
                                    self.at = at;
                                    0xfffd
                                }
                            } else {
                                u32::from(unit)
                            };
                            // A lone surrogate, which a JavaScript string
                            // may hold, becomes U+FFFD.
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(self.error("invalid escape")),
                    }
                }
                Some(_) => return Err(self.error("control character in a string")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_reads_what_javascript_writes() {
        let text = r#" {"op":"reply","id":"7","value":"[1 \"two\" \\ \/]","n":-1.5e3,
            "list":[true,false,null,[],{}],"text":"\u00e9\ud83d\ude00\ud800x\u2028"} "#;
        let json = Json::parse(text).unwrap();
        assert_eq!(json.get("id").and_then(Json::as_str), Some("7"));
        assert_eq!(
            json.get("value").and_then(Json::as_str),
            Some(r#"[1 "two" \ /]"#)
        );
        assert_eq!(json.get("n"), Some(&Json::Number(-1500.0)));
        assert_eq!(
            json.get("text").and_then(Json::as_str),
            Some("\u{e9}\u{1f600}\u{fffd}x\u{2028}")
        );
        let mut written = String::new();
        json.write(&mut written);
        assert_eq!(Json::parse(&written).unwrap(), json);
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "01",
            "1.",
            "\"\u{1}\"",
            "nul",
            "1 2",
            "\"\\x\"",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad:?}");
        }
        assert!(Json::parse(&"[".repeat(10_000)).is_err());
    }

    #[test]
    fn source_roots_are_served_where_source_maps_name_them() {
        let prefix = |root: &str, out: &str| served_prefix(Path::new(root), Path::new(out));
        assert_eq!(
            prefix("/work/app/src", "/work/app/dev"),
            Some(vec!["src".to_owned()])
        );
        assert_eq!(prefix("/work", "/work/dev"), Some(Vec::new()));
        assert_eq!(
            prefix("/other/lib", "/work/dev"),
            Some(vec!["other".to_owned(), "lib".to_owned()])
        );
        assert!(loopback_host("localhost:9630"));
        assert!(loopback_host("127.0.0.1:9630"));
        assert!(loopback_host("[::1]:9630"));
        assert!(loopback_host("app.localhost"));
        assert!(!loopback_host("example.com:9630"));
        assert!(!loopback_host("192.168.1.2:9630"));
        assert_eq!(percent_decode("a%20b"), Some("a b".to_owned()));
        assert_eq!(percent_decode("a%2"), None);
    }
}
