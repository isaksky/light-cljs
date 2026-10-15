//! `--watch --serve` (docs/dev-workflow.md, "The server and the client" and
//! "Phase 3: server, client and reload policy"), run as a subprocess on a
//! copy of examples/dev and bound to 127.0.0.1. The runtime is the Phase 0
//! driver (tests/support/dev_execution.mjs) with `--connect`: it loads a dev
//! build and starts it, then the build's own client.js reads the server's
//! event stream with `fetch` and applies its loads with the hook protocol.

#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

use light_cljs::EvalOptions;
use light_cljs::dev_server::{Build, DevServer, Json, LoadUnit};
use light_cljs::dev_session::{Action, DevSession, Unit};
use light_cljs::eval_session::PrintOptions;
use light_cljs::source_provider::{FileSourceProvider, SourceInput};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    rc::Rc,
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::Duration,
};
use temp_dir::TempDir;

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/dev");
const DRIVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/dev_execution.mjs"
);
/// Far above a step's or a load's time, so a loaded machine does not fail a
/// test.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The driver's record of the example: its stdout, with nothing traced and
/// an undefined value.
fn record(stdout: &str) -> String {
    let mut json = String::from("\"");
    for ch in stdout.chars() {
        match ch {
            '\n' => json.push_str("\\n"),
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            ch => json.push(ch),
        }
    }
    json.push('"');
    format!(
        r#"{{"id":"program","outcome":{{"status":"returned","value":{{"$value":"undefined"}}}},"trace":[],"stdout":{json}}}"#
    )
}

/// The lines a child process writes to stderr, as they come.
fn lines(stderr: impl Read + Send + 'static) -> Receiver<String> {
    let (send, lines) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if send.send(line).is_err() {
                break;
            }
        }
    });
    lines
}

/// The lines from `lines` up to the first that starts with `prefix`, and
/// that line.
fn until(lines: &Receiver<String>, prefix: &str, what: &str) -> (Vec<String>, String) {
    let mut before = Vec::new();
    loop {
        match lines.recv_timeout(TIMEOUT) {
            Ok(line) if line.starts_with(prefix) => return (before, line),
            Ok(line) => before.push(line),
            Err(RecvTimeoutError::Timeout) => {
                panic!("no line {prefix:?} from {what} in {TIMEOUT:?}; got {before:#?}")
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("{what} exited before a line {prefix:?}; got {before:#?}")
            }
        }
    }
}

/// A `--watch --serve 0` process on a copy of examples/dev, killed when
/// dropped.
struct Watch {
    dir: TempDir,
    child: Child,
    lines: Receiver<String>,
    /// The server's address, `127.0.0.1:PORT`.
    address: String,
}

impl Watch {
    /// Copy examples/dev to `src` in a fresh directory and watch it into
    /// `dev`, serving it, polling every 20 ms; return once the build is
    /// written and the polling has begun, with a copy of that first build in
    /// `before`.
    fn start(label: &str) -> Self {
        Self::start_with(label, |_| {})
    }

    /// [`Self::start`] after `setup` changed the copy, in the directory it
    /// is given.
    fn start_with(label: &str, setup: impl FnOnce(&Path)) -> Self {
        let dir = TempDir::new(label);
        fs::create_dir_all(dir.join("src/app")).unwrap();
        for name in ["state", "macros", "view", "main"] {
            let file = format!("app/{name}.cljs");
            fs::copy(Path::new(EXAMPLE).join(&file), dir.join("src").join(&file)).unwrap();
        }
        setup(&dir.join("src"));
        let (child, lines, address) = Self::spawn(&dir, "0");
        fs::create_dir(dir.join("before")).unwrap();
        for entry in fs::read_dir(dir.join("dev")).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), dir.join("before").join(entry.file_name())).unwrap();
        }
        Self {
            dir,
            child,
            lines,
            address,
        }
    }

    /// Watch `dir`'s `src` into its `dev` and serve it on `port`; the
    /// process, its stderr lines after the first build, and its address.
    fn spawn(dir: &Path, port: &str) -> (Child, Receiver<String>, String) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(["--watch", "--entry", "app.main", "--source-root"])
            .arg(dir.join("src"))
            .arg("--out-dir")
            .arg(dir.join("dev"))
            .args(["--poll-ms", "20", "--serve", port])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let lines = lines(child.stderr.take().unwrap());
        let (_, serving) = until(&lines, "light-cljs: serving ", "--watch");
        let address = serving
            .rsplit_once(" at http://")
            .and_then(|(_, url)| url.strip_suffix('/'))
            .unwrap_or_else(|| panic!("no address in {serving:?}"))
            .to_owned();
        assert!(address.starts_with("127.0.0.1:"), "{serving}");
        until(&lines, "light-cljs: built ", "--watch");
        (child, lines, address)
    }

    /// End the watch process.
    fn stop(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }

    /// Start the watch again, on the port it had.
    fn restart(&mut self) {
        let port = self.address.rsplit_once(':').unwrap().1.to_owned();
        let (child, lines, address) = Self::spawn(&self.dir, &port);
        assert_eq!(address, self.address);
        (self.child, self.lines) = (child, lines);
    }

    /// The port the server listens on.
    fn port(&self) -> &str {
        self.address.rsplit_once(':').unwrap().1
    }

    /// Save `text` as the source file `file` at once, as an editor that
    /// renames its new text over the file does, and return the step's lines
    /// and its summary line.
    fn save(&mut self, file: &str, text: &str) -> (Vec<String>, String) {
        let path = self.dir.join("src").join(file);
        let temporary = path.with_file_name(".saving");
        fs::write(&temporary, text).unwrap();
        fs::rename(&temporary, &path).unwrap();
        until(&self.lines, "light-cljs: recompiled ", "--watch")
    }

    fn source(&self, file: &str) -> String {
        fs::read_to_string(self.dir.join("src").join(file)).unwrap()
    }

    /// A runtime: the driver running `build` (the build directory `before`
    /// or `dev`), connected to the server until it has handled `messages`
    /// messages after `hello`.
    fn runtime(&self, build: &str, messages: usize) -> Runtime {
        Runtime::connect(&self.dir.join(build), &self.address, messages)
    }

    /// `GET path` (raw, unnormalized) with the extra request headers
    /// `headers`.
    fn get(&self, path: &str, headers: &str) -> Response {
        request(
            &self.address,
            &format!(
                "GET {path} HTTP/1.1\r\nHost: {}\r\n{headers}\r\n",
                self.address
            ),
        )
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The driver connected to a dev server.
struct Runtime {
    child: Child,
    stdout: ChildStdout,
    lines: Receiver<String>,
}

impl Runtime {
    fn connect(build: &Path, address: &str, messages: usize) -> Self {
        let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
            .arg(DRIVER)
            .arg(build)
            .arg("--start")
            .arg("--connect")
            .arg(format!("http://{address}/events"))
            .arg("--messages")
            .arg(messages.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the dev server tests require Node");
        let lines = lines(child.stderr.take().unwrap());
        let stdout = child.stdout.take().unwrap();
        Self {
            child,
            stdout,
            lines,
        }
    }

    /// The runtime's lines up to the first that starts with `prefix`.
    fn until(&self, prefix: &str) -> (Vec<String>, String) {
        until(&self.lines, prefix, "the runtime")
    }

    /// Wait for the run to end; its record and its remaining stderr lines.
    fn finish(mut self) -> (String, Vec<String>) {
        let mut stdout = String::new();
        self.stdout.read_to_string(&mut stdout).unwrap();
        let status = self.child.wait().unwrap();
        let mut rest = Vec::new();
        while let Ok(line) = self.lines.recv_timeout(TIMEOUT) {
            // The outcome protocol marks the record's case on stderr.
            if !line.starts_with("CASE ") {
                rest.push(line);
            }
        }
        assert!(
            status.success(),
            "{status}\nstdout:\n{stdout}\nstderr:\n{rest:#?}"
        );
        let record = stdout
            .lines()
            .find(|line| line.starts_with("{\"id\":\"program\""))
            .unwrap_or_else(|| panic!("no record in {stdout}"))
            .to_owned();
        (record, rest)
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A response: its status line, its headers with lowercase names, its body.
struct Response {
    status: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Send the raw request `text` to `address` and read the response, which
/// the server ends by closing the connection.
fn request(address: &str, text: &str) -> Response {
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.write_all(text.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap().to_owned();
    let headers = lines
        .map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.trim().to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    Response {
        status,
        headers,
        body: body.to_owned(),
    }
}

/// A runtime running the build made before any change receives the build
/// when it connects, which it already runs, then the unit of a saved change
/// to `app.view`, which it loads between the hooks: the next render runs the
/// new code and the `defonce` state keeps its click.
#[test]
fn a_connected_runtime_loads_a_saved_change() {
    let mut watch = Watch::start("dev-server-change");
    let runtime = watch.runtime("before", 2);
    let (before, replay) = runtime.until("message load replay ");
    assert_eq!(
        replay,
        "message load replay app.state app.macros app.view app.main"
    );
    assert!(
        before.len() == 2
            && before[0].starts_with("light-cljs: connected to http://127.0.0.1:")
            && before[0].ends_with(" as runtime 1")
            && before[1] == "message hello 1",
        "{before:#?}"
    );
    let edited = watch.source("app/view.cljs").replace(
        "(js/console.log (describe card))",
        "(js/console.log \"render:\" (describe card))",
    );
    let (lines, step) = watch.save("app/view.cljs", &edited);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    let (record_line, rest) = runtime.finish();
    assert_eq!(
        record_line,
        record("start\nclicks: [1]\nreloading\nrender: clicks: [2]\n")
    );
    assert!(
        rest.len() == 2
            && rest[0].starts_with("light-cljs: reloaded app.view in ")
            && rest[1] == "message load app.view",
        "{rest:#?}"
    );
}

/// A step that fails pushes its error, located, and no load; a step with a
/// warning pushes the warning and its load; and a runtime that connects
/// after a failed step receives the error after the build.
#[test]
fn a_compile_error_is_pushed_without_a_load() {
    let mut watch = Watch::start("dev-server-error");
    let view = watch.source("app/view.cljs");
    let path = fs::canonicalize(watch.dir.join("src/app/view.cljs")).unwrap();
    let path = path.display();
    let runtime = watch.runtime("before", 4);
    runtime.until("message load replay ");
    let (_, step) = watch.save(
        "app/view.cljs",
        &view.replace("(defn render", "(defn render ("),
    );
    assert!(
        step.ends_with("; app.view failed and keeps its previous unit"),
        "{step}"
    );
    let (lines, error) = runtime.until("message ");
    assert!(
        error.starts_with(&format!(
            "message error {path}:23:1: unexpected end of input; expected a closing delimiter"
        )) && error.ends_with(&format!("; opened at {path}:17:1")),
        "{error}"
    );
    assert!(
        lines.len() == 1
            && lines[0].starts_with(&format!("light-cljs: {path}:23:1: unexpected end of input")),
        "{lines:#?}"
    );
    // A runtime that connects now catches up with the error.
    let late = watch.runtime("dev", 2);
    late.until("message load replay ");
    let (_, late_error) = late.until("message ");
    assert_eq!(late_error, error);
    let (late_record, _) = late.finish();
    assert_eq!(late_record, record("start\nclicks: [1]\n"));
    // Analysis is in warning mode: a use of a Var the file defines later is
    // a warning, located in the file, and the unit loads.
    let later = format!(
        "{}\n(defn later [text] (str \"later \" text))\n",
        view.replace(
            "(js/console.log (describe card))",
            "(js/console.log (later (describe card)))",
        )
    );
    let (_, step) = watch.save("app/view.cljs", &later);
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    let (lines, warnings) = runtime.until("message ");
    assert_eq!(
        warnings,
        format!(
            "message warnings {path}:22:22: Use of undeclared Var app.view/later [undeclared-var]"
        )
    );
    assert!(
        lines.len() == 1 && lines[0].starts_with("light-cljs: WARNING: "),
        "{lines:#?}"
    );
    let (record_line, rest) = runtime.finish();
    assert_eq!(
        record_line,
        record("start\nclicks: [1]\nreloading\nlater clicks: [2]\n")
    );
    assert!(
        rest.len() == 2
            && rest[0].starts_with("light-cljs: reloaded app.view in ")
            && rest[1] == "message load app.view",
        "{rest:#?}"
    );
}

/// A runtime that connects after a step receives the build as it is now: a
/// runtime running the build made before the step loads the changed unit
/// (between the hooks), and one running the rewritten build loads nothing.
#[test]
fn a_late_runtime_catches_up() {
    let mut watch = Watch::start("dev-server-late");
    let edited = watch.source("app/view.cljs").replace(
        "(js/console.log (describe card))",
        "(js/console.log \"render:\" (describe card))",
    );
    watch.save("app/view.cljs", &edited);
    let stale = watch.runtime("before", 1);
    let current = watch.runtime("dev", 1);
    let (record_line, rest) = stale.finish();
    assert_eq!(
        record_line,
        record("start\nclicks: [1]\nreloading\nrender: clicks: [2]\n")
    );
    assert!(
        rest.iter()
            .any(|line| line.starts_with("light-cljs: reloaded app.view in ")),
        "{rest:#?}"
    );
    let (record_line, rest) = current.finish();
    assert_eq!(record_line, record("start\nrender: clicks: [1]\n"));
    assert!(
        rest.iter().all(|line| !line.contains("reloaded")),
        "{rest:#?}"
    );
}

/// When the server goes away, the client connects again with backoff, and
/// catches up with the build of the server it reaches: here a new watch on
/// the same port, started after a change to the sources.
#[test]
fn a_runtime_reconnects_and_catches_up() {
    let mut watch = Watch::start("dev-server-reconnect");
    let runtime = watch.runtime("before", 2);
    runtime.until("message load replay ");
    watch.stop();
    runtime.until("light-cljs: disconnected from the dev server");
    let view = watch.source("app/view.cljs");
    fs::write(
        watch.dir.join("src/app/view.cljs"),
        view.replace(
            "(js/console.log (describe card))",
            "(js/console.log \"again:\" (describe card))",
        ),
    )
    .unwrap();
    watch.restart();
    let (_, hello) = runtime.until("message hello ");
    assert_eq!(hello, "message hello 1");
    let (record_line, _) = runtime.finish();
    assert_eq!(
        record_line,
        record("start\nclicks: [1]\nreloading\nagain: clicks: [2]\n")
    );
}

/// `GET` serves the build directory's files and the source roots' (at the
/// paths the source maps name them by) with content types and ETags, the
/// built-in page at `/` until the directory has an `index.html`, and
/// nothing outside them; a request for another host is refused, and `POST
/// /reply` refuses what is not a reply to a pending request.
#[test]
fn get_serves_the_build_the_sources_and_a_page() {
    let watch = Watch::start("dev-server-get");
    let page = watch.get("/", "");
    assert_eq!(page.status, "HTTP/1.1 200 OK");
    assert_eq!(
        page.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert!(
        page.body.contains("<script src=\"index.js\"></script>")
            && page.body.contains("id=\"light-cljs-console\""),
        "{}",
        page.body
    );
    let unit = watch.get("/app.view.js", "");
    assert_eq!(unit.status, "HTTP/1.1 200 OK");
    assert_eq!(
        unit.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(
        unit.body,
        fs::read_to_string(watch.dir.join("dev/app.view.js")).unwrap()
    );
    let etag = unit.header("etag").unwrap();
    assert!(etag.starts_with('"') && etag.ends_with('"'), "{etag}");
    let again = watch.get("/app.view.js", &format!("If-None-Match: {etag}\r\n"));
    assert_eq!(again.status, "HTTP/1.1 304 Not Modified");
    assert_eq!(again.body, "");
    let map = watch.get("/app.view.js.map", "");
    assert_eq!(
        map.header("content-type"),
        Some("application/json; charset=utf-8")
    );
    // The map names the source relative to the build directory, which
    // resolves against the server's root to /src/app/view.cljs.
    assert!(
        map.body.contains("\"../src/app/view.cljs\""),
        "{}",
        map.body
    );
    let source = watch.get("/src/app/view.cljs", "");
    assert_eq!(source.status, "HTTP/1.1 200 OK");
    assert_eq!(source.body, watch.source("app/view.cljs"));
    for path in [
        "/missing.js",
        "/../src/app/view.cljs",
        "/src/app/../app/view.cljs",
        "/src/%2e%2e/src/app/view.cljs",
        "/src/app%2Fview.cljs",
        "/src/",
    ] {
        assert_eq!(
            watch.get(path, "").status,
            "HTTP/1.1 404 Not Found",
            "{path}"
        );
    }
    let other = request(
        &watch.address,
        "GET / HTTP/1.1\r\nHost: example.com\r\n\r\n",
    );
    assert_eq!(other.status, "HTTP/1.1 403 Forbidden");
    let head = request(
        &watch.address,
        &format!(
            "HEAD /index.js HTTP/1.1\r\nHost: localhost:{}\r\n\r\n",
            watch.port()
        ),
    );
    assert_eq!(head.status, "HTTP/1.1 200 OK");
    assert_eq!(head.body, "");
    let post = |body: &str| {
        request(
            &watch.address,
            &format!(
                "POST /reply HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                watch.address,
                body.len()
            ),
        )
        .status
    };
    assert_eq!(
        post("{\"runtime\":\"1\",\"id\":\"1\",\"value\":\"3\"}"),
        "HTTP/1.1 404 Not Found"
    );
    assert_eq!(
        post("{\"id\":\"1\",\"value\":\"3\"}"),
        "HTTP/1.1 400 Bad Request"
    );
    assert_eq!(post("{\"id\":1"), "HTTP/1.1 400 Bad Request");
    assert_eq!(post("[]"), "HTTP/1.1 400 Bad Request");
    // A page in the build directory wins over the built-in one.
    fs::write(watch.dir.join("dev/index.html"), "<p>mine</p>\n").unwrap();
    assert_eq!(watch.get("/", "").body, "<p>mine</p>\n");
}

/// The dev build of examples/dev in `dir`'s `dev`, written by the CLI.
fn example_build(dir: &Path) -> PathBuf {
    let out = dir.join("dev");
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--compile", "--entry", "app.main", "--source-root", EXAMPLE])
        .arg("--reloadable")
        .arg("--out-dir")
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    out
}

/// An `invoke` runs a REPL form's unit in the runtime, in the application's
/// state, and the runtime posts the value printed with the print options
/// sent, or the error it threw, to `/reply` with the request ID; a reply
/// that names another runtime is refused and leaves the request waiting.
/// The server here is the library's, given the units of a `DevSession`.
#[test]
fn an_invoke_replies_with_the_printed_value() {
    let dir = TempDir::new("dev-server-invoke");
    let build = example_build(&dir);
    let server = DevServer::start(
        "127.0.0.1",
        0,
        &build,
        &[PathBuf::from(EXAMPLE)],
        Build::default(),
    )
    .unwrap();
    let address = server.address().to_string();
    let runtime = Runtime::connect(&build, &address, 5);
    runtime.until("message hello 1");
    assert_eq!(server.runtimes(), [1]);
    let session = DevSession::new(&EvalOptions {
        source_provider: Some(Rc::new(FileSourceProvider::new(vec![PathBuf::from(
            EXAMPLE,
        )]))),
        ..EvalOptions::default()
    })
    .unwrap();
    // The session loads app.state to compile against it; the runtime
    // already runs it.
    session.source(&SourceInput::new("<repl>", "(require 'app.state)"));
    let unit = |text: &str| -> Unit {
        let actions = session.source(&SourceInput::new("<repl>", text));
        match actions.last() {
            Some(Action::Invoke(unit)) => unit.clone(),
            other => panic!("no unit for {text}: {other:?}"),
        }
    };
    let reply = |text: &str, print: &PrintOptions| -> Json {
        let invocation = server.invoke(None, &unit(text), print).unwrap();
        assert_eq!(invocation.runtime, 1);
        invocation.reply.recv_timeout(TIMEOUT).unwrap()
    };
    let field = |json: &Json, key: &str| json.get(key).and_then(Json::as_str).map(str::to_owned);
    let print = PrintOptions::default();
    let value = reply("(+ 1 2)", &print);
    assert_eq!(field(&value, "value").as_deref(), Some("3"));
    assert_eq!(field(&value, "runtime").as_deref(), Some("1"));
    assert_eq!(
        field(&reply("@app.state/state", &print), "value").as_deref(),
        Some("{:clicks 1, :card #app.view.Card{:label \"clicks\", :clicks 1}}")
    );
    let short = PrintOptions {
        length: Some(3),
        ..PrintOptions::default()
    };
    assert_eq!(
        field(&reply("(range 10)", &short), "value").as_deref(),
        Some("(0 1 2 ...)")
    );
    let thrown = reply("(throw (js/Error. \"boom\"))", &print);
    assert_eq!(field(&thrown, "value"), None);
    assert_eq!(
        thrown
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Json::as_str),
        Some("boom")
    );
    // While the runtime is busy for a second, a reply to its request from
    // another runtime, or from none, is refused, and its own still counts.
    let slow = server
        .invoke(
            None,
            &unit("(let [end (+ (js/Date.now) 1000)] (while (< (js/Date.now) end)) 42)"),
            &print,
        )
        .unwrap();
    let post = |body: String| {
        request(
            &address,
            &format!(
                "POST /reply HTTP/1.1\r\nHost: {address}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
        .status
    };
    let id = &slow.id;
    assert_eq!(
        post(format!(
            "{{\"runtime\":\"2\",\"id\":\"{id}\",\"value\":\"forged\"}}"
        )),
        "HTTP/1.1 409 Conflict"
    );
    assert_eq!(
        post(format!("{{\"id\":\"{id}\",\"value\":\"forged\"}}")),
        "HTTP/1.1 400 Bad Request"
    );
    let value = slow.reply.recv_timeout(TIMEOUT).unwrap();
    assert_eq!(field(&value, "value").as_deref(), Some("42"));
    let (record_line, _) = runtime.finish();
    assert_eq!(record_line, record("start\nclicks: [1]\n"));
    // The server forgets a runtime whose stream closed.
    let begun = std::time::Instant::now();
    while !server.runtimes().is_empty() {
        assert!(begun.elapsed() < TIMEOUT, "runtime 1 is still connected");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        server.invoke(None, &unit("1"), &print).err().as_deref(),
        Some("no runtime is connected")
    );
}

/// A replay runs again a unit that threw, though the runtime has its text,
/// before the units after it: here a unit that throws on its first run only,
/// replayed by a second server on the port after the first stopped, which
/// ends the stream it serves.
#[test]
fn a_replay_runs_again_a_unit_that_threw() {
    let dir = TempDir::new("dev-server-retry");
    let build = example_build(&dir);
    let unit = |namespace: &str, js: &str| LoadUnit {
        namespace: namespace.into(),
        file: format!("{namespace}.js"),
        js: js.into(),
    };
    let replay = Build {
        units: vec![
            unit(
                "app.flaky",
                r#"globalThis.__lightDev.define("app.flaky", [], (rt) => {
  globalThis.flakyRuns = (globalThis.flakyRuns ?? 0) + 1;
  if (globalThis.flakyRuns === 1) throw new Error("the first run fails");
  console.log("flaky ran " + globalThis.flakyRuns);
  return null;
});
"#,
            ),
            unit(
                "app.after",
                r#"globalThis.__lightDev.define("app.after", ["app.flaky"], (rt) => {
  console.log("after ran with " + globalThis.flakyRuns);
  return null;
});
"#,
            ),
        ],
        ..Build::default()
    };
    let first = DevServer::start("127.0.0.1", 0, &build, &[], replay.clone()).unwrap();
    let address = first.address();
    let runtime = Runtime::connect(&build, &address.to_string(), 2);
    let (lines, failed) = runtime.until("message load replay ");
    assert_eq!(
        failed,
        "message load replay app.flaky app.after failed: the first run fails"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("light-cljs: app.flaky threw while it loaded")),
        "{lines:#?}"
    );
    drop(first);
    runtime.until("light-cljs: disconnected from the dev server");
    let _second = DevServer::start("127.0.0.1", address.port(), &build, &[], replay).unwrap();
    let (_, replayed) = runtime.until("message load replay ");
    assert_eq!(replayed, "message load replay app.flaky app.after");
    let (record_line, _) = runtime.finish();
    assert_eq!(
        record_line,
        record("start\nclicks: [1]\nflaky ran 2\nafter ran with 2\n")
    );
}

/// A failed step that loaded a new namespace before its error pushes the
/// error alone, and a runtime that connects after it catches up with the
/// build of the last step that succeeded; the next step that succeeds loads
/// the new namespace with its requirer.
#[test]
fn a_failed_step_is_not_replayed() {
    let mut watch = Watch::start("dev-server-partial");
    let view = watch.source("app/view.cljs");
    fs::write(
        watch.dir.join("src/app/extra.cljs"),
        "(ns app.extra)\n\n(js/console.log \"extra runs\")\n",
    )
    .unwrap();
    let runtime = watch.runtime("before", 3);
    runtime.until("message load replay ");
    let requiring = view.replace(
        "[app.state :as state]",
        "[app.state :as state]\n            [app.extra]",
    );
    assert_ne!(requiring, view);
    let (_, step) = watch.save(
        "app/view.cljs",
        &requiring.replace(
            "(let [card (->Card \"clicks\" (:clicks @state/state))]",
            "(let [card]",
        ),
    );
    assert!(
        step.starts_with("light-cljs: recompiled app.extra in ")
            && step.ends_with("; app.view failed and keeps its previous unit"),
        "{step}"
    );
    let (_, error) = runtime.until("message ");
    assert!(error.starts_with("message error "), "{error}");
    let late = watch.runtime("before", 2);
    let (_, replay) = late.until("message load replay ");
    assert_eq!(
        replay,
        "message load replay app.state app.macros app.view app.main"
    );
    let (_, late_error) = late.until("message ");
    assert_eq!(late_error, error);
    let (late_record, _) = late.finish();
    assert_eq!(late_record, record("start\nclicks: [1]\n"));
    watch.save(
        "app/view.cljs",
        &requiring.replace(
            "(js/console.log (describe card))",
            "(js/console.log \"render:\" (describe card))",
        ),
    );
    let (_, load) = runtime.until("message ");
    assert_eq!(load, "message load app.extra app.view");
    let (record_line, _) = runtime.finish();
    assert_eq!(
        record_line,
        record("start\nclicks: [1]\nreloading\nextra runs\nrender: clicks: [2]\n")
    );
}

/// The build's own warnings reach a runtime that connects before any step,
/// located as a step's are.
#[test]
fn startup_warnings_reach_a_runtime() {
    let watch = Watch::start_with("dev-server-startup-warning", |src| {
        let path = src.join("app/view.cljs");
        let view = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            format!(
                "{}\n(defn later [text] (str \"later \" text))\n",
                view.replace(
                    "(js/console.log (describe card))",
                    "(js/console.log (later (describe card)))",
                )
            ),
        )
        .unwrap();
    });
    let path = fs::canonicalize(watch.dir.join("src/app/view.cljs")).unwrap();
    let runtime = watch.runtime("dev", 2);
    runtime.until("message load replay ");
    let (_, warnings) = runtime.until("message ");
    assert_eq!(
        warnings,
        format!(
            "message warnings {}:22:22: Use of undeclared Var app.view/later [undeclared-var]",
            path.display()
        )
    );
    let (record_line, _) = runtime.finish();
    assert_eq!(record_line, record("start\nlater clicks: [1]\n"));
}

/// The connected client runs the hooks around a load: the before-load ones
/// in reverse definition order, waiting for an async one's continuation
/// before the next, a throwing one logged and skipped; then the units; then
/// the after-load ones in order, an async one awaited too.
#[test]
fn the_client_runs_hooks_in_order() {
    let mut watch = Watch::start_with("dev-server-hooks", |src| {
        fs::write(
            src.join("app/main.cljs"),
            r#"(ns app.main
  (:require [app.state :as state]
            [app.view :as view]))

(defn ^:dev/before-load first-before [] (js/console.log "before 1"))

(defn ^:dev/before-load-async second-before [done]
  (js/console.log "before 2 begins")
  (js/setTimeout (fn [] (js/console.log "before 2 done") (done)) 50))

(defn ^:dev/before-load broken [] (throw (js/Error. "the hook failed")))

(defn ^:dev/after-load first-after [] (state/click!) (view/render))

(defn ^:dev/after-load-async second-after [done]
  (js/setTimeout (fn [] (js/console.log "after 2") (done)) 50))

(defn ^:dev/after-load third-after [] (js/console.log "after 3"))

(defn start [] (js/console.log "start") (state/click!) (view/render))

start
"#,
        )
        .unwrap();
    });
    let runtime = watch.runtime("before", 2);
    runtime.until("message load replay ");
    let view = watch.source("app/view.cljs");
    watch.save(
        "app/view.cljs",
        &format!(
            "{}\n(js/console.log \"view loads\")\n",
            view.replace(
                "(js/console.log (describe card))",
                "(js/console.log \"render:\" (describe card))",
            )
        ),
    );
    let (lines, load) = runtime.until("message load app.view");
    assert_eq!(load, "message load app.view");
    assert!(
        lines.iter().any(|line| line
            .starts_with("light-cljs: hook app.main/broken threw: Error: the hook failed")),
        "{lines:#?}"
    );
    let (record_line, _) = runtime.finish();
    assert_eq!(
        record_line,
        record(
            "start\nclicks: [1]\nbefore 2 begins\nbefore 2 done\nbefore 1\nview loads\nrender: clicks: [2]\nafter 2\nafter 3\n"
        )
    );
}
