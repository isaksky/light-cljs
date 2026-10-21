//! nREPL from the dev server (docs/dev-workflow.md, "The connected REPL"
//! and "Phase 4: connected REPL and nREPL"), run as subprocesses: a
//! `--watch --serve 0 --nrepl 0` process on a copy of examples/dev bound to
//! 127.0.0.1, a Node runtime (the Phase 0 driver,
//! tests/support/dev_execution.mjs, with `--connect`), and a Node nREPL
//! client speaking bencode over TCP (tests/support/nrepl_client.mjs), as an
//! editor does.

#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

#[path = "support/file_watch.rs"]
#[allow(dead_code)]
mod file_watch;

use light_cljs::dev_server::Json;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::{Duration, Instant},
};
use temp_dir::TempDir;

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/dev");
const DRIVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/dev_execution.mjs"
);
const CLIENT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/nrepl_client.mjs"
);
/// Far above a round trip's or a step's time, so a loaded machine does not
/// fail a test.
const TIMEOUT: Duration = Duration::from_secs(60);

fn node() -> std::ffi::OsString {
    std::env::var_os("NODE").unwrap_or_else(|| "node".into())
}

/// The lines `stream` writes, as they come.
fn lines(stream: impl Read + Send + 'static) -> Receiver<String> {
    let (send, lines) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
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

/// A child process, killed when dropped.
struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A `--watch --serve 0 --nrepl 0` process on a copy of examples/dev, run
/// in its directory.
struct Watch {
    dir: TempDir,
    process: Process,
    lines: Receiver<String>,
    /// `127.0.0.1:PORT` of the server.
    address: String,
    /// `127.0.0.1:PORT` of `--nrepl`.
    nrepl: String,
}

impl Watch {
    fn start(label: &str) -> Self {
        let dir = TempDir::new(label);
        let src = dir.join("src");
        fs::create_dir_all(src.join("app")).unwrap();
        for name in ["state", "macros", "view", "main"] {
            let file = format!("app/{name}.cljs");
            fs::copy(Path::new(EXAMPLE).join(&file), src.join(&file)).unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args([
                "--watch",
                "--no-cache",
                "--entry",
                "app.main",
                "--source-root",
                "src",
            ])
            .args(["--out-dir", "dev", "--poll-ms", "20", "--serve", "0"])
            .args(["--nrepl", "0"])
            .current_dir(&*dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let lines = lines(child.stderr.take().unwrap());
        let process = Process(child);
        let (_, serving) = until(&lines, "light-cljs: serving ", "--watch");
        let address = serving
            .rsplit_once(" at http://")
            .and_then(|(_, url)| url.strip_suffix('/'))
            .unwrap_or_else(|| panic!("no address in {serving:?}"))
            .to_owned();
        let (_, nrepl) = until(&lines, "light-cljs: nREPL at ", "--watch");
        let nrepl = nrepl["light-cljs: nREPL at ".len()..]
            .split_once(',')
            .unwrap_or_else(|| panic!("no address in {nrepl:?}"))
            .0
            .to_owned();
        assert!(
            nrepl.starts_with("127.0.0.1:") && nrepl != address,
            "{nrepl}"
        );
        until(&lines, "light-cljs: built ", "--watch");
        Self {
            dir,
            process,
            lines,
            address,
            nrepl,
        }
    }

    /// Save `text` as the source file `file` at once, and return the step's
    /// summary line, asserting that it succeeded without warnings.
    fn save(&self, file: &str, text: &str) -> String {
        let path = self.dir.join("src").join(file);
        let temporary = path.with_file_name(".saving");
        fs::write(&temporary, text).unwrap();
        fs::rename(&temporary, &path).unwrap();
        let (step, summary) = until(&self.lines, "light-cljs: recompiled ", "--watch");
        assert!(
            !summary.contains("failed") && !step.iter().any(|line| line.contains("WARNING")),
            "{step:#?}\n{summary}"
        );
        summary
    }

    /// Append `text` to the source file `file` and save it.
    fn append(&self, file: &str, text: &str) -> String {
        let source = fs::read_to_string(self.dir.join("src").join(file)).unwrap();
        self.save(file, &format!("{source}\n{text}\n"))
    }

    /// A Node runtime running the first build, connected to the server.
    fn runtime(&self) -> (Process, Receiver<String>) {
        let mut child = Command::new(node())
            .arg(DRIVER)
            .arg(self.dir.join("dev"))
            .arg("--start")
            .arg("--connect")
            .arg(format!("http://{}/events", self.address))
            .arg("--messages")
            .arg("1000000")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the nREPL tests require Node");
        let lines = lines(child.stderr.take().unwrap());
        let process = Process(child);
        until(&lines, "message hello ", "the runtime");
        (process, lines)
    }

    /// An nREPL client connected to `address`.
    fn client(&self, address: &str) -> Client {
        client(address)
    }
}

/// An nREPL client connected to `address`.
fn client(address: &str) -> Client {
    let (host, port) = address.rsplit_once(':').unwrap();
    let mut child = Command::new(node())
        .arg(CLIENT)
        .args([host, port])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the nREPL tests require Node");
    let stdin = child.stdin.take().unwrap();
    let lines = lines(child.stdout.take().unwrap());
    Client {
        _process: Process(child),
        stdin: Some(stdin),
        lines,
        next: 1,
    }
}

/// The Node nREPL client.
struct Client {
    _process: Process,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next: u64,
}

impl Client {
    /// Send `message`, a JSON object's members without the braces, with a
    /// fresh `id`, and return its ID.
    fn send(&mut self, message: &str) -> String {
        let id = format!("m{}", self.next);
        self.next += 1;
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{{\"id\":\"{id}\",{message}}}").unwrap();
        stdin.flush().unwrap();
        id
    }

    /// The responses to `id` up to the one whose status has `done`.
    fn responses(&mut self, id: &str) -> Vec<Json> {
        let mut responses = Vec::new();
        loop {
            let line = match self.lines.recv_timeout(TIMEOUT) {
                Ok(line) => line,
                Err(error) => panic!("no done for {id}: {error}; got {responses:#?}"),
            };
            let response = Json::parse(&line).unwrap();
            assert_eq!(
                response.get("id").and_then(Json::as_str),
                Some(id),
                "{line}"
            );
            let done = statuses(&response).contains(&"done".to_owned());
            responses.push(response);
            if done {
                return responses;
            }
        }
    }

    /// Send `message` and return its responses.
    fn call(&mut self, message: &str) -> Vec<Json> {
        let id = self.send(message);
        self.responses(&id)
    }

    /// The single response to `message`, whose status is `done`.
    fn one(&mut self, message: &str) -> Json {
        match &self.call(message)[..] {
            [response] => response.clone(),
            other => panic!("{message} gave {other:#?}"),
        }
    }

    /// Open a session.
    fn clone_session(&mut self) -> String {
        let response = self.one("\"op\":\"clone\"");
        text(&response, "new-session").to_owned()
    }

    /// Evaluate `code` in `session` with the further `members`: the values,
    /// namespaces and errors of its responses as `VALUE @ NS`, `err: TEXT`
    /// and `ex: CLASS`, and its statuses.
    fn eval(&mut self, session: &str, code: &str, members: &str) -> Vec<String> {
        let message = format!(
            "\"op\":\"eval\",\"session\":\"{session}\",\"code\":{}{members}",
            json(code)
        );
        summary(&self.call(&message))
    }

    /// The value of evaluating `code` in `session`, which printed nothing
    /// else.
    fn value(&mut self, session: &str, code: &str) -> String {
        match &self.eval(session, code, "")[..] {
            [value, done] if done == "status: done" => {
                value.rsplit_once(" @ ").unwrap().0.to_owned()
            }
            other => panic!("{code} gave {other:#?}"),
        }
    }

    /// End the input, which closes the connection.
    fn close(&mut self) {
        self.stdin = None;
    }
}

/// `text` as a JSON string.
fn json(text: &str) -> String {
    let mut out = String::new();
    Json::String(text.to_owned()).write(&mut out);
    out
}

fn text<'a>(response: &'a Json, key: &str) -> &'a str {
    response
        .get(key)
        .and_then(Json::as_str)
        .unwrap_or_else(|| panic!("no {key} in {response:?}"))
}

fn statuses(response: &Json) -> Vec<String> {
    match response.get("status") {
        Some(Json::Array(items)) => items
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// An eval's responses as lines: `VALUE @ NS`, `err: TEXT`, `ex: CLASS` and
/// `status: S1 S2`.
fn summary(responses: &[Json]) -> Vec<String> {
    let mut lines = Vec::new();
    for response in responses {
        if let Some(value) = response.get("value").and_then(Json::as_str) {
            lines.push(format!("{value} @ {}", text(response, "ns")));
        }
        if let Some(err) = response.get("err").and_then(Json::as_str) {
            lines.push(format!("err: {}", err.trim_end()));
        }
        if let Some(ex) = response.get("ex").and_then(Json::as_str) {
            lines.push(format!("ex: {ex}"));
        }
        let statuses = statuses(response);
        if !statuses.is_empty() && response.get("ex").is_none() {
            lines.push(format!("status: {}", statuses.join(" ")));
        }
    }
    lines
}

/// The candidates of a completions response, as `CANDIDATE TYPE NS`.
fn candidates(response: &Json) -> Vec<String> {
    let Some(Json::Array(items)) = response.get("completions") else {
        panic!("no completions in {response:?}");
    };
    items
        .iter()
        .map(|item| {
            format!(
                "{} {} {}",
                text(item, "candidate"),
                text(item, "type"),
                item.get("ns").and_then(Json::as_str).unwrap_or("-")
            )
        })
        .collect()
}

/// The responses to each of `ids` up to the one whose status has `done`,
/// as they interleave on the connection.
fn collect(client: &mut Client, ids: &[&str]) -> Vec<Vec<Json>> {
    let mut responses = vec![Vec::new(); ids.len()];
    let mut done = vec![false; ids.len()];
    while done.contains(&false) {
        let line = match client.lines.recv_timeout(TIMEOUT) {
            Ok(line) => line,
            Err(error) => panic!("no done for {ids:?}: {error}; got {responses:#?}"),
        };
        let response = Json::parse(&line).unwrap();
        let id = response.get("id").and_then(Json::as_str);
        let index = ids
            .iter()
            .position(|known| Some(*known) == id)
            .unwrap_or_else(|| panic!("{line}"));
        done[index] |= statuses(&response).contains(&"done".to_owned());
        responses[index].push(response);
    }
    responses
}

/// A lookup's `info` as `KEY=VALUE` lines, then its statuses.
fn info(response: &Json) -> Vec<String> {
    let Some(Json::Object(info)) = response.get("info") else {
        panic!("{response:?}");
    };
    let mut lines = info
        .iter()
        .map(|(key, value)| match value {
            Json::String(text) => format!("{key}={text}"),
            other => {
                let mut out = format!("{key}=");
                other.write(&mut out);
                out
            }
        })
        .collect::<Vec<_>>();
    lines.extend(
        statuses(response)
            .iter()
            .map(|status| format!("status={status}")),
    );
    lines
}

/// Every operation, through an editor's protocol: `describe` lists them;
/// `clone` opens sessions that evaluate in a Node runtime with `eval` (with
/// `ns`, errors and warnings) and `load-file`; `completions` and `lookup`
/// answer from the retained program; `interrupt` ends a wait for the
/// runtime; `close` closes a session; a watch reload keeps the Vars the REPL
/// defined; the server's own port speaks nREPL too; `.nrepl-port` names the
/// port while the watch runs and goes when it ends.
#[test]
fn nrepl_operations_round_trip_through_a_node_runtime() {
    let mut watch = Watch::start("dev-nrepl");
    let port_file = watch.dir.join(".nrepl-port");
    assert_eq!(
        fs::read_to_string(&port_file).unwrap(),
        watch.nrepl.rsplit_once(':').unwrap().1
    );
    let mut client = watch.client(&watch.nrepl);
    // Completions and lookups need no runtime.
    let early = client.one("\"op\":\"lookup\",\"ns\":\"app.view\",\"sym\":\"state/click!\"");
    assert_eq!(
        info(&early)[..3],
        ["ns=app.state", "name=click!", "type=function"]
    );
    let early = client.one("\"op\":\"completions\",\"ns\":\"app.view\",\"prefix\":\"ren\"");
    assert_eq!(candidates(&early), ["render function app.view"]);
    let (_runtime, runtime_lines) = watch.runtime();

    let described = client.one("\"op\":\"describe\"");
    let Some(Json::Object(ops)) = described.get("ops") else {
        panic!("{described:?}");
    };
    let ops = ops.iter().map(|(op, _)| op.as_str()).collect::<Vec<_>>();
    assert_eq!(
        ops,
        [
            "clone",
            "close",
            "completions",
            "describe",
            "eval",
            "interrupt",
            "load-file",
            "lookup",
            "ls-sessions"
        ]
    );
    assert!(
        described
            .get("versions")
            .and_then(|versions| versions.get("nrepl"))
            .is_some(),
        "{described:?}"
    );

    // eval, in the session's namespace and with `ns`; the runtime keeps the
    // session's history.
    let session = client.clone_session();
    assert_eq!(client.value(&session, "(+ 1 2)"), "3");
    // The round trip, measured at the client: from the message sent to its
    // `done`.
    let mut times = (0..20)
        .map(|_| {
            let begun = Instant::now();
            assert_eq!(client.value(&session, "(+ 1 2)"), "3");
            begun.elapsed()
        })
        .collect::<Vec<_>>();
    times.sort();
    eprintln!(
        "nREPL (+ 1 2) round trip: median {:.1} ms, {:.1}-{:.1} ms over {}",
        times[times.len() / 2].as_secs_f64() * 1000.0,
        times[0].as_secs_f64() * 1000.0,
        times[times.len() - 1].as_secs_f64() * 1000.0,
        times.len()
    );
    assert!(times[times.len() / 2] < Duration::from_secs(2), "{times:?}");
    assert_eq!(client.value(&session, "(* *1 2)"), "6");
    assert_eq!(
        client.eval(&session, "(def x 5) (inc x)", ""),
        ["5 @ app.main", "6 @ app.main", "status: done"]
    );
    assert_eq!(
        client.eval(&session, "(:clicks @state/state)", ",\"ns\":\"app.view\""),
        ["1 @ app.view", "status: done"]
    );
    assert_eq!(client.value(&session, "(str *ns*)"), "\"app.view\"");
    assert_eq!(
        client.eval(&session, "(in-ns 'app.main) (str *ns*)", ""),
        ["nil @ app.main", "\"app.main\" @ app.main", "status: done"]
    );
    // Errors as --repl --connect prints them, each with an `ex`; a form that
    // fails leaves the next to run; warnings go to `err`.
    let thrown = client.eval(
        &session,
        "(throw (js/Error. \"boom\")) (+ 1 1)",
        ",\"file\":\"src/app/main.cljs\",\"line\":30,\"column\":3",
    );
    assert!(
        thrown[0].starts_with("err: src/app/main.cljs:30:3: Error: boom\n    at "),
        "{thrown:#?}"
    );
    assert_eq!(
        thrown[1..],
        [
            "ex: light-cljs/runtime-error",
            "2 @ app.main",
            "status: done"
        ]
    );
    let undeclared = client.eval(&session, "(undeclared-thing 1)", "");
    assert_eq!(
        undeclared[0],
        "err: WARNING: <nrepl>:1:2: Use of undeclared Var app.main/undeclared-thing [undeclared-var]"
    );
    assert_eq!(
        client.eval(&session, "(+ 1", ""),
        [
            "err: <nrepl>:1:5: unexpected end of input; expected a closing delimiter (bytes 4..4); opened at <nrepl>:1:1",
            "ex: light-cljs/compile-error",
            "status: done"
        ]
    );
    // A message without a session evaluates in the connection's own.
    let response = client.call("\"op\":\"eval\",\"code\":\"(+ 2 2)\"");
    assert_eq!(summary(&response), ["4 @ app.main", "status: done"]);

    // completions and lookup, from the retained program.
    let complete = |client: &mut Client, ns: &str, prefix: &str| {
        candidates(&client.one(&format!(
            "\"op\":\"completions\",\"session\":\"{session}\",\"ns\":\"{ns}\",\"prefix\":\"{prefix}\""
        )))
    };
    assert_eq!(
        complete(&mut client, "app.view", "state/"),
        [
            "state/click! function app.state",
            "state/state var app.state"
        ]
    );
    assert_eq!(
        complete(&mut client, "app.view", "app.state/cl"),
        ["app.state/click! function app.state"]
    );
    assert_eq!(
        complete(&mut client, "app.view", "lab"),
        ["labeled macro app.macros"]
    );
    assert_eq!(
        complete(&mut client, "app.view", "ren"),
        ["render function app.view"]
    );
    let st = complete(&mut client, "app.view", "st");
    for expected in ["state namespace -", "string? macro cljs.core"] {
        assert!(st.contains(&expected.to_owned()), "{expected} in {st:#?}");
    }
    assert!(
        complete(&mut client, "app.view", "map").contains(&"mapv function cljs.core".to_owned())
    );
    assert_eq!(
        complete(&mut client, "app.main", "app.v"),
        ["app.view namespace -"]
    );
    let lookup = |client: &mut Client, ns: &str, sym: &str| {
        info(&client.one(&format!(
            "\"op\":\"lookup\",\"session\":\"{session}\",\"ns\":\"{ns}\",\"sym\":\"{sym}\""
        )))
    };
    let click = lookup(&mut client, "app.view", "state/click!");
    let file = format!(
        "file={}",
        watch
            .dir
            .join("src/app/state.cljs")
            .canonicalize()
            .unwrap()
            .display()
    );
    assert_eq!(
        click,
        [
            "ns=app.state",
            "name=click!",
            "type=function",
            "arglists-str=([])",
            "doc=Counts one click.",
            &file,
            "line=7",
            "column=1",
            "status=done"
        ]
    );
    assert_eq!(lookup(&mut client, "app.state", "click!"), click);
    assert_eq!(
        lookup(&mut client, "app.view", "labeled"),
        [
            "ns=app.macros",
            "name=labeled",
            "type=macro",
            "macro=true",
            "arglists-str=([label & body])",
            "doc=The string of `label`, a colon and the bracketed `body`.",
            "status=done"
        ]
    );
    let map = lookup(&mut client, "app.view", "map");
    assert_eq!(
        map[..4],
        [
            "ns=cljs.core",
            "name=map",
            "type=function",
            "arglists-str=([f] [f coll] [f c1 c2] [f c1 c2 c3] [f c1 c2 c3 & colls])"
        ]
    );
    assert!(
        map[4].starts_with("doc=Returns a lazy sequence"),
        "{map:#?}"
    );
    assert_eq!(
        lookup(&mut client, "app.view", "*print-length*")[..3],
        ["ns=cljs.core", "name=*print-length*", "type=var"]
    );
    assert_eq!(
        complete(&mut client, "app.view", "*print-len"),
        ["*print-length* var cljs.core"]
    );
    assert_eq!(
        lookup(&mut client, "app.view", "click!"),
        ["status=done", "status=no-info"]
    );

    // A watch reload keeps the Vars the REPL defined: a save reads one, and
    // a save of its namespace leaves it to the REPL and to that reader.
    assert_eq!(
        client.eval(&session, "(def from-repl 42)", ",\"ns\":\"app.state\""),
        ["42 @ app.state", "status: done"]
    );
    watch.append("app/view.cljs", "(defn read-repl [] state/from-repl)");
    assert_eq!(client.value(&session, "(app.view/read-repl)"), "42");
    watch.append("app/state.cljs", "(defn twice [n] (* 2 n))");
    assert_eq!(client.value(&session, "from-repl"), "42");
    assert_eq!(client.value(&session, "(app.view/read-repl)"), "42");
    assert_eq!(client.value(&session, "(twice 4)"), "8");
    assert_eq!(
        lookup(&mut client, "app.view", "state/from-repl")[..3],
        ["ns=app.state", "name=from-repl", "type=var"]
    );

    // load-file evaluates a file's forms in its namespace, answers with the
    // last value and leaves the session's namespace as it was.
    let path = watch.dir.join("src/app/extra.cljs");
    let loaded = client.call(&format!(
        "\"op\":\"load-file\",\"session\":\"{session}\",\"file\":{},\"file-path\":{},\"file-name\":\"extra.cljs\"",
        json("(ns app.extra)\n\n(defn twice\n  \"Twice x.\"\n  [x]\n  (* 2 x))\n\n(twice 21)\n"),
        json(path.to_str().unwrap())
    ));
    assert_eq!(summary(&loaded), ["42 @ app.state", "status: done"]);
    assert_eq!(client.value(&session, "(str *ns*)"), "\"app.state\"");
    assert_eq!(client.value(&session, "(app.extra/twice 4)"), "8");
    assert_eq!(
        lookup(&mut client, "app.state", "app.extra/twice"),
        [
            "ns=app.extra",
            "name=twice",
            "type=function",
            "arglists-str=([x])",
            "doc=Twice x.",
            &format!("file={}", path.display()),
            "line=3",
            "column=1",
            "status=done"
        ]
    );

    // interrupt ends the wait for a runtime busy with a form; the runtime
    // finishes it, and the session's next form runs after it.
    for _ in runtime_lines.try_iter() {}
    let busy = client.send(&format!(
        "\"op\":\"eval\",\"session\":\"{session}\",\"code\":{}",
        json(
            "(do (.writeSync (.getBuiltinModule js/process \"fs\") 2 \"busy\\n\")
                 (let [end (+ (js/Date.now) 3000)]
                   (loop [] (when (< (js/Date.now) end) (recur)))))"
        )
    ));
    until(&runtime_lines, "busy", "the runtime");
    // From another connection, too.
    let mut second = watch.client(&watch.nrepl);
    let mismatch = second.one(&format!(
        "\"op\":\"interrupt\",\"session\":\"{session}\",\"interrupt-id\":\"other\""
    ));
    assert_eq!(
        statuses(&mismatch),
        ["error", "interrupt-id-mismatch", "done"]
    );
    let interrupt = second.one(&format!(
        "\"op\":\"interrupt\",\"session\":\"{session}\",\"interrupt-id\":\"{busy}\""
    ));
    assert_eq!(summary(&[interrupt]), ["status: done"]);
    second.close();
    let ended = collect(&mut client, &[&busy]).pop().unwrap();
    assert_eq!(summary(&ended), ["status: interrupted", "status: done"]);
    assert!(
        !runtime_lines
            .try_iter()
            .any(|line| line.starts_with("message invoke")),
        "the runtime finished the form before the interrupt ended its wait"
    );
    assert_eq!(client.value(&session, "(+ 1 1)"), "2");
    assert!(
        runtime_lines
            .try_iter()
            .any(|line| line.starts_with("message invoke")),
        "the session's next form ran before the runtime finished the interrupted one"
    );
    let idle = client.one(&format!("\"op\":\"interrupt\",\"session\":\"{session}\""));
    assert_eq!(statuses(&idle), ["session-idle", "done"]);

    // Sessions: clone copies the namespace, not the history; ls-sessions
    // lists them; close closes one, which then evaluates nothing.
    let other = client.one(&format!("\"op\":\"clone\",\"session\":\"{session}\""));
    let other = text(&other, "new-session").to_owned();
    assert_eq!(client.value(&other, "(str *ns*)"), "\"app.state\"");
    assert_eq!(client.value(&other, "*2"), "nil");
    let listed = client.one("\"op\":\"ls-sessions\"");
    let Some(Json::Array(sessions)) = listed.get("sessions") else {
        panic!("{listed:?}");
    };
    for id in [&session, &other] {
        assert!(sessions.contains(&Json::String(id.clone())), "{listed:?}");
    }
    let closed = client.one(&format!("\"op\":\"close\",\"session\":\"{other}\""));
    assert_eq!(statuses(&closed), ["session-closed", "done"]);
    assert_eq!(
        client.eval(&other, "(+ 1 1)", ""),
        ["status: error unknown-session done"]
    );
    let unknown = client.one("\"op\":\"no-such-op\"");
    assert_eq!(statuses(&unknown), ["error", "unknown-op", "done"]);

    // The server's own port answers nREPL too.
    let mut same = watch.client(&watch.address);
    let session = same.clone_session();
    assert_eq!(same.value(&session, "(+ 40 2)"), "42");
    same.close();
    client.close();

    // SIGINT ends the watch, and .nrepl-port goes with it.
    #[cfg(unix)]
    {
        assert_eq!(interrupt_watch(&mut watch).code(), Some(130));
        assert!(!port_file.exists());
    }
}

/// Send SIGINT to `watch` and wait for it to exit.
#[cfg(unix)]
fn interrupt_watch(watch: &mut Watch) -> std::process::ExitStatus {
    let pid = watch.process.0.id().to_string();
    let status = Command::new("kill").args(["-INT", &pid]).status().unwrap();
    assert!(status.success());
    watch.process.0.wait().unwrap()
}

/// A signal leaves `.nrepl-port` alone when another server has written its
/// port there since.
#[cfg(unix)]
#[test]
fn a_signal_leaves_another_servers_port_file() {
    let mut watch = Watch::start("dev-nrepl-port");
    let port_file = watch.dir.join(".nrepl-port");
    fs::write(&port_file, "1").unwrap();
    assert_eq!(interrupt_watch(&mut watch).code(), Some(130));
    assert_eq!(fs::read_to_string(&port_file).unwrap(), "1");
}

/// A shared session runs evaluations in the order the server received them,
/// whichever connection sent them: an update queued on one connection
/// behind a blocked evaluation runs before one another connection sends
/// after it.
#[test]
fn shared_session_requests_preserve_cross_connection_arrival_order() {
    let watch = Watch::start("dev-nrepl-order");
    let (_runtime, runtime_lines) = watch.runtime();
    let mut first = watch.client(&watch.nrepl);
    let mut second = watch.client(&watch.nrepl);
    let session = first.clone_session();
    assert_eq!(
        first.value(&session, "(do (def order (atom [])) nil)"),
        "nil"
    );
    // The first evaluation blocks the runtime until the test releases it.
    let release = watch.dir.join("release");
    let blocked = first.send(&format!(
        "\"op\":\"eval\",\"session\":\"{session}\",\"code\":{}",
        json(&format!(
            "(let [fs (.getBuiltinModule js/process \"fs\")]
               (.writeSync fs 2 \"blocked\\n\")
               (loop [] (when-not (.existsSync fs {}) (recur)))
               :released)",
            json(release.to_str().unwrap())
        ))
    ));
    until(&runtime_lines, "blocked", "the runtime");
    // A connection answers its messages in order, so the answer to a
    // `describe` shows that the server has read the `eval` sent before it.
    let swap = |client: &mut Client, key: &str| {
        let id = client.send(&format!(
            "\"op\":\"eval\",\"session\":\"{session}\",\"code\":\"(swap! order conj {key})\""
        ));
        client.one("\"op\":\"describe\"");
        id
    };
    let a = swap(&mut first, ":a");
    let b = swap(&mut second, ":b");
    fs::write(&release, "").unwrap();
    let answers = collect(&mut first, &[&blocked, &a]);
    assert_eq!(
        summary(&answers[0]),
        [":released @ app.main", "status: done"]
    );
    assert_eq!(summary(&answers[1]), ["[:a] @ app.main", "status: done"]);
    let answer = collect(&mut second, &[&b]).pop().unwrap();
    assert_eq!(summary(&answer), ["[:a :b] @ app.main", "status: done"]);
    assert_eq!(first.value(&session, "@order"), "[:a :b]");
}

/// Closing a shared session ends its running evaluation as interrupted and
/// answers each evaluation still queued on it, on the connection that sent
/// it, with an error and `done`, without running it.
#[test]
fn closing_shared_session_finishes_queued_requests_without_running_them() {
    let watch = Watch::start("dev-nrepl-close-queued");
    let (_runtime, runtime_lines) = watch.runtime();
    let mut first = watch.client(&watch.nrepl);
    let mut second = watch.client(&watch.nrepl);
    let session = first.clone_session();
    assert_eq!(
        first.value(&session, "(do (def order (atom [])) nil)"),
        "nil"
    );
    let release = watch.dir.join("release");
    let blocked = first.send(&format!(
        "\"op\":\"eval\",\"session\":\"{session}\",\"code\":{}",
        json(&format!(
            "(let [fs (.getBuiltinModule js/process \"fs\")]
               (.writeSync fs 2 \"blocked\\n\")
               (loop [] (when-not (.existsSync fs {}) (recur)))
               :released)",
            json(release.to_str().unwrap())
        ))
    ));
    until(&runtime_lines, "blocked", "the runtime");
    let swap = |client: &mut Client, key: &str| {
        let id = client.send(&format!(
            "\"op\":\"eval\",\"session\":\"{session}\",\"code\":\"(swap! order conj {key})\""
        ));
        client.one("\"op\":\"describe\"");
        id
    };
    let a = swap(&mut first, ":a");
    let b = swap(&mut second, ":b");
    let close = second.send(&format!("\"op\":\"close\",\"session\":\"{session}\""));
    let answers = collect(&mut second, &[&b, &close]);
    assert_eq!(summary(&answers[1]), ["status: session-closed done"]);
    let closed = |answer: &[Json]| match &summary(answer)[..] {
        [err, done] if err.starts_with("err: REPL session ") && err.ends_with(" was closed") => {
            done.clone()
        }
        other => panic!("{other:#?}"),
    };
    assert_eq!(closed(&answers[0]), "status: error done");
    let answers = collect(&mut first, &[&blocked, &a]);
    assert_eq!(
        summary(&answers[0]),
        ["status: interrupted", "status: done"]
    );
    assert_eq!(closed(&answers[1]), "status: error done");
    fs::write(&release, "").unwrap();
    let other = first.clone_session();
    assert_eq!(first.value(&other, "@order"), "[]");
}

/// `light-cljs watch features` with `:nrepl {:port 0}` (docs/build-configuration.md,
/// "Top-level keys"), from the example's `app/` subdirectory: nREPL answers
/// on its own port, written to `.nrepl-port` in the project directory, and
/// on the `:http` port; SIGINT removes `.nrepl-port` and `.light-cljs/http.port`
/// both. With `:nrepl false` the `:http` port answers no nREPL client and
/// serves the build still, and `--nrepl 0` after the id turns it on again,
/// since flags win.
#[cfg(unix)]
#[test]
fn watch_id_answers_nrepl_as_its_nrepl_setting_says() {
    let (_dir, project) = file_watch::features("dev-nrepl-file");
    let nrepl_file = project.join(".nrepl-port");
    let http_file = project.join(".light-cljs/http.port");
    let mut watch = file_watch::Light::spawn(
        &project.join("app"),
        &["watch", "features"],
        Some("{:http {:port 0} :nrepl {:port 0}}"),
    );
    let (address, _, _) = watch.serving();
    let (_, line) = watch.until("light-cljs: nREPL at ");
    let nrepl = line
        .strip_prefix("light-cljs: nREPL at ")
        .and_then(|rest| rest.strip_suffix(&format!(", its port in {}", nrepl_file.display())))
        .unwrap_or_else(|| panic!("{line}"))
        .to_owned();
    assert_ne!(nrepl, address);
    assert_eq!(
        fs::read_to_string(&nrepl_file).unwrap(),
        nrepl.rsplit_once(':').unwrap().1
    );
    watch.until("[:features] built ");
    for answering in [&nrepl, &address] {
        let described = String::from_utf8(file_watch::nrepl_describe(answering)).unwrap();
        assert!(
            described.starts_with('d') && described.contains("3:ops"),
            "{answering}: {described}"
        );
    }
    assert!(http_file.exists());
    assert_eq!(watch.interrupt().code(), Some(130));
    assert!(!nrepl_file.exists() && !http_file.exists());

    let mut refusing = file_watch::Light::spawn(
        &project,
        &["watch", "features"],
        Some("{:http {:port 0} :nrepl false}"),
    );
    let (address, _, _) = refusing.serving();
    let (lines, _) = refusing.until("[:features] built ");
    assert!(lines.is_empty(), "{lines:#?}");
    assert_eq!(file_watch::nrepl_describe(&address), b"");
    assert_eq!(file_watch::get(&address, "/index.js").0, "HTTP/1.1 200 OK");
    assert!(!nrepl_file.exists());
    drop(refusing);

    let mut flagged = file_watch::Light::spawn(
        &project,
        &["watch", "features", "--nrepl", "0"],
        Some("{:http {:port 0} :nrepl false}"),
    );
    let (address, _, _) = flagged.serving();
    flagged.until("light-cljs: nREPL at ");
    flagged.until("[:features] built ");
    let described = String::from_utf8(file_watch::nrepl_describe(&address)).unwrap();
    assert!(described.contains("3:ops"), "{described}");
}

/// With `:nrepl {:port N}` on a port the test holds, `watch features` fails
/// to listen once `.light-cljs/http.port` is written, and the error removes
/// the file again; `--no-nrepl` after the id turns that listener off too, so
/// the watch starts, writes no `.nrepl-port` and answers no nREPL client.
#[test]
fn watch_id_removes_its_port_file_on_an_error_and_no_nrepl_drops_the_listener() {
    let (_dir, project) = file_watch::features("dev-nrepl-file-held");
    let (_held, held) = file_watch::held_port();
    let merged = format!("{{:http {{:port 0}} :nrepl {{:port {held}}}}}");
    let http_file = project.join(".light-cljs/http.port");
    let mut failing = file_watch::Light::spawn(&project, &["watch", "features"], Some(&merged));
    let (lines, status) = failing.exit();
    assert_eq!(status.code(), Some(1), "{lines:#?}");
    assert_eq!(
        lines.last().unwrap(),
        &format!(
            "light-cljs watch features: cannot serve nREPL on 127.0.0.1:{held}: {}",
            file_watch::bind_error(held)
        )
    );
    assert!(project.join(".light-cljs").is_dir() && !http_file.exists());
    assert!(!project.join(".nrepl-port").exists());

    let mut quiet = file_watch::Light::spawn(
        &project,
        &["watch", "features", "--no-nrepl"],
        Some(&merged),
    );
    let (address, _, _) = quiet.serving();
    let (lines, _) = quiet.until("[:features] built ");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(http_file.exists() && !project.join(".nrepl-port").exists());
    assert_eq!(file_watch::nrepl_describe(&address), b"");
}

/// The Phase 3 test project, tests/fixtures/build-configuration/multi: a
/// page (`:app`) and its web worker (`:worker`), which both require
/// `app.shared`; only the app requires `app.build-info`, only the worker
/// `app.jobs`.
const MULTI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/build-configuration/multi"
);

/// A Node runtime of the dev build in `build`, connected to the event
/// stream at `url`: the process, its process ID, the ID the server gave it
/// and its stderr lines.
fn runtime_of(build: &Path, url: &str) -> (Process, u32, String, Receiver<String>) {
    let mut child = Command::new(node())
        .arg(DRIVER)
        .arg(build)
        .arg("--connect")
        .arg(url)
        .arg("--messages")
        .arg("1000000")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the nREPL tests require Node");
    let lines = lines(child.stderr.take().unwrap());
    let pid = child.id();
    let process = Process(child);
    let (_, hello) = until(&lines, "message hello ", "the runtime");
    let id = hello["message hello ".len()..].to_owned();
    until(&lines, "message load replay ", "the runtime");
    (process, pid, id, lines)
}

/// The Phase 3 exit criterion (docs/build-configuration.md, "Phase 3" and
/// "Several builds in one process"), on a temporary copy of the multi
/// fixture: `light-cljs watch app worker` serves the page and its worker
/// from one origin; a save of `app.shared` reloads both; and one nREPL
/// connection evaluates in the app's runtime, then, after `(light.dev/repl
/// :worker)`, in the worker's, each evaluation reading a Var only its build
/// defines and the process ID of the runtime that ran it. Its session's
/// completions come from the build it is bound to, `light.dev/runtimes`
/// lists both builds' runtimes, `light.dev/select!` chooses among the bound
/// build's, and an unknown build id is an exact error.
#[test]
fn one_nrepl_connection_evaluates_in_the_app_and_after_light_dev_repl_in_the_worker() {
    let (_dir, project) = file_watch::project("dev-nrepl-multi", Path::new(MULTI));
    let mut watch = file_watch::Light::spawn(
        &project,
        &["watch", "app", "worker"],
        Some("{:http {:port 0} :fs-watch {:loop-wait 20}}"),
    );
    let (address, _, _) = watch.serving();
    let (_, line) = watch.until("light-cljs: nREPL at ");
    let nrepl = line["light-cljs: nREPL at ".len()..]
        .split_once(',')
        .unwrap_or_else(|| panic!("no address in {line:?}"))
        .0
        .to_owned();
    watch.until("[:app] built ");
    watch.until("[:worker] built ");

    // One origin: the page's `new Worker("/worker/index.js", ...)` names a
    // path of the server that serves the page, at `/` and `/app/`.
    let (app_out, worker_out) = (project.join("out/app"), project.join("out/worker"));
    for (path, file) in [
        ("/app/index.js", app_out.join("index.js")),
        ("/worker/index.js", worker_out.join("index.js")),
    ] {
        assert_eq!(
            file_watch::get(&address, path),
            (
                "HTTP/1.1 200 OK".to_owned(),
                fs::read_to_string(&file).unwrap()
            ),
            "{path}"
        );
    }
    let page = fs::read_to_string(app_out.join("app.main.js")).unwrap();
    assert!(
        page.contains(
            "new (host(\"Worker\"))(\"/worker/index.js\", rt.jsObject([\"type\", \"module\"]))"
        ),
        "{page}"
    );

    let (_app, app_pid, app_id, app_lines) =
        runtime_of(&app_out, &format!("http://{address}/app/events"));
    let (_worker, worker_pid, worker_id, worker_lines) =
        runtime_of(&worker_out, &format!("http://{address}/worker/events"));

    // A save of the shared namespace reloads both.
    let shared = project.join("src/app/shared.cljs");
    let text = fs::read_to_string(&shared).unwrap();
    file_watch::save(&shared, &text.replace("\"hello, \"", "\"hi, \""));
    let (_, step) = until(&app_lines, "message load ", "the app's runtime");
    assert_eq!(step, "message load app.shared app.main");
    let (_, step) = until(&worker_lines, "message load ", "the worker's runtime");
    assert_eq!(step, "message load app.shared");

    // The session starts bound to the first build named, in its entry.
    let mut client = client(&nrepl);
    let session = client.clone_session();
    assert_eq!(
        client.eval(
            &session,
            "[app.build-info/built-by (shared/greeting \"app\") (.-pid js/process)]",
            ""
        ),
        [
            format!("[\"light-cljs\" \"hi, app\" {app_pid}] @ app.main"),
            "status: done".to_owned()
        ]
    );
    let complete = |client: &mut Client, session: &str| {
        candidates(&client.one(&format!(
            "\"op\":\"completions\",\"session\":\"{session}\",\"prefix\":\"jobs/r\""
        )))
    };
    assert_eq!(complete(&mut client, &session), Vec::<String>::new());
    assert_eq!(
        client.value(&session, "(light.dev/runtimes)"),
        format!(
            "[{{:id {app_id}, :build :app, :agent \"node\", :selected true}} {{:id {worker_id}, :build :worker, :agent \"node\", :selected false}}]"
        )
    );

    // Bound to the worker: its entry namespace, compiler and runtime.
    assert_eq!(
        client.eval(&session, "(light.dev/repl :worker)", ""),
        ["[:selected :worker] @ app.worker", "status: done"]
    );
    assert_eq!(
        client.eval(
            &session,
            "[(jobs/run 2) (shared/greeting \"worker\") (.-pid js/process)]",
            ""
        ),
        [
            format!("[42 \"hi, worker\" {worker_pid}] @ app.worker"),
            "status: done".to_owned()
        ]
    );
    assert_eq!(
        complete(&mut client, &session),
        ["jobs/run function app.jobs"]
    );
    assert_eq!(
        client.value(&session, "(light.dev/runtimes)"),
        format!(
            "[{{:id {app_id}, :build :app, :agent \"node\", :selected false}} {{:id {worker_id}, :build :worker, :agent \"node\", :selected true}}]"
        )
    );
    assert_eq!(
        client.value(&session, &format!("(light.dev/select! {worker_id})")),
        worker_id
    );
    assert_eq!(
        client.eval(&session, &format!("(light.dev/select! {app_id})"), ""),
        [
            format!(
                "err: <nrepl>:1:1: runtime {app_id} runs :app, and the session is bound to :worker; (light.dev/repl :app) binds it to :app"
            ),
            "ex: light-cljs/command-error".to_owned(),
            "status: done".to_owned()
        ]
    );
    assert_eq!(
        client.eval(&session, "(light.dev/repl :nope)", ""),
        [
            "err: <nrepl>:1:1: the watch runs no build :nope; its builds are :app, :worker",
            "ex: light-cljs/command-error",
            "status: done"
        ]
    );
    // An interrupt ends the wait for the worker's runtime.
    let busy = client.send(&format!(
        "\"op\":\"eval\",\"session\":\"{session}\",\"code\":{}",
        json(
            "(do (.writeSync (.getBuiltinModule js/process \"fs\") 2 \"busy\\n\")
                 (let [end (+ (js/Date.now) 2000)]
                   (loop [] (when (< (js/Date.now) end) (recur)))))"
        )
    ));
    until(&worker_lines, "busy", "the worker's runtime");
    let interrupt = client.send(&format!(
        "\"op\":\"interrupt\",\"session\":\"{session}\",\"interrupt-id\":\"{busy}\""
    ));
    let mut answers = collect(&mut client, &[&busy, &interrupt]);
    assert_eq!(summary(&answers.pop().unwrap()), ["status: done"]);
    assert_eq!(
        summary(&answers.pop().unwrap()),
        ["status: interrupted", "status: done"]
    );

    // Another session of the connection is still the app's.
    let other = client.clone_session();
    assert_eq!(
        client.value(&other, "(.-pid js/process)"),
        app_pid.to_string()
    );

    // Back to the app, and the forms after it in the same evaluation run
    // there.
    assert_eq!(
        client.eval(
            &session,
            "(light.dev/repl :app) [app.build-info/built-by (.-pid js/process)]",
            ""
        ),
        [
            "[:selected :app] @ app.main".to_owned(),
            format!("[\"light-cljs\" {app_pid}] @ app.main"),
            "status: done".to_owned()
        ]
    );
}

/// `light-cljs watch app worker` on a temporary copy of the multi fixture,
/// both builds made: the directory, the project, the watch and its nREPL
/// address.
fn watch_multi(label: &str) -> (temp_dir::TempDir, PathBuf, file_watch::Light, String) {
    let (dir, project) = file_watch::project(label, Path::new(MULTI));
    let mut watch = file_watch::Light::spawn(
        &project,
        &["watch", "app", "worker"],
        Some("{:http {:port 0} :fs-watch {:loop-wait 20}}"),
    );
    let (_, line) = watch.until("light-cljs: nREPL at ");
    let nrepl = line["light-cljs: nREPL at ".len()..]
        .split_once(',')
        .unwrap_or_else(|| panic!("no address in {line:?}"))
        .0
        .to_owned();
    watch.until("[:app] built ");
    watch.until("[:worker] built ");
    (dir, project, watch, nrepl)
}

/// The address of the server of `watch_multi`'s watch, from the port file.
fn http_address(project: &Path) -> String {
    let port = fs::read_to_string(project.join(".light-cljs/http.port")).unwrap();
    format!("127.0.0.1:{}", port.trim())
}

/// A session cloned from one bound to the worker is bound to the worker
/// too, in its namespace and with its runtime choice; a session that ran in
/// both builds, when it closes, leaves the history of each build's runtime,
/// and its clone goes on.
#[test]
fn switched_sessions_clone_and_close_across_builds() {
    let (_dir, project, _watch, nrepl) = watch_multi("dev-nrepl-multi-clone");
    let address = http_address(&project);
    let (_app, app_pid, _, app_lines) = runtime_of(
        &project.join("out/app"),
        &format!("http://{address}/app/events"),
    );
    let (_worker, worker_pid, worker_id, worker_lines) = runtime_of(
        &project.join("out/worker"),
        &format!("http://{address}/worker/events"),
    );
    let mut client = client(&nrepl);
    let session = client.clone_session();
    assert_eq!(
        client.value(&session, "(.-pid js/process)"),
        app_pid.to_string()
    );
    assert_eq!(
        client.value(&session, "(light.dev/repl :worker)"),
        "[:selected :worker]"
    );
    assert_eq!(
        client.value(&session, &format!("(light.dev/select! {worker_id})")),
        worker_id
    );
    let clone = client.one(&format!("\"op\":\"clone\",\"session\":\"{session}\""));
    let clone = text(&clone, "new-session").to_owned();
    assert_eq!(
        client.eval(&clone, "[(jobs/run 1) (.-pid js/process)]", ""),
        [
            format!("[21 {worker_pid}] @ app.worker"),
            "status: done".to_owned()
        ]
    );
    assert_eq!(
        client.value(&clone, "(light.dev/runtimes)"),
        format!(
            "[{{:id {}, :build :app, :agent \"node\", :selected false}} {{:id {worker_id}, :build :worker, :agent \"node\", :selected true}}]",
            worker_id.parse::<u64>().unwrap() - 1
        )
    );
    for _ in app_lines.try_iter() {}
    for _ in worker_lines.try_iter() {}
    let closed = client.one(&format!("\"op\":\"close\",\"session\":\"{session}\""));
    assert_eq!(statuses(&closed), ["session-closed", "done"]);
    until(&app_lines, "message forget", "the app's runtime");
    until(&worker_lines, "message forget", "the worker's runtime");
    assert_eq!(client.value(&clone, "(jobs/run 2)"), "42");
}

/// A build whose restart failed answers no form, but its sessions still
/// switch to another build, and a session opened after the failure too.
#[test]
fn can_switch_away_from_a_build_whose_restart_failed() {
    let (_dir, project, mut watch, nrepl) = watch_multi("dev-nrepl-multi-failed");
    let address = http_address(&project);
    let (_worker, worker_pid, _, _worker_lines) = runtime_of(
        &project.join("out/worker"),
        &format!("http://{address}/worker/events"),
    );
    let mut client = client(&nrepl);
    let before = client.clone_session();
    let path = project.join("light-cljs.edn");
    let config = fs::read_to_string(&path).unwrap();
    file_watch::save(&path, &config.replacen("[app.main]", "[app.missing]", 1));
    let (_, failed) = watch.until("[:app] ");
    assert!(
        failed.ends_with("; the build starts again at the next change"),
        "{failed}"
    );
    assert_eq!(
        client.eval(&before, "(+ 1 2)", ""),
        ["err: the compiler stopped", "status: error done"]
    );
    let after = client.clone_session();
    for session in [&before, &after] {
        assert_eq!(
            client.eval(
                session,
                "(light.dev/repl :worker) [(jobs/run 2) (.-pid js/process)]",
                ""
            ),
            [
                "[:selected :worker] @ app.worker".to_owned(),
                format!("[42 {worker_pid}] @ app.worker"),
                "status: done".to_owned()
            ]
        );
    }
}
