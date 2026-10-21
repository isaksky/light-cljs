//! `--repl --connect` (docs/dev-workflow.md, "The connected REPL" and
//! "Phase 4: connected REPL and nREPL"), run as subprocesses: a `--watch
//! --serve 0` process on a copy of examples/dev bound to 127.0.0.1, Node
//! runtimes (the Phase 0 driver, tests/support/dev_execution.mjs, with
//! `--connect`), and the terminal client reading forms from its stdin. The
//! server compiles each form with the watch's session and runs it in a
//! runtime, which prints the value.

/// `--no-cache` unless `args` name a cache: the default cache directory
/// would carry artifacts from one test to the next.
fn no_cache<'a>(mut args: impl Iterator<Item = &'a str>) -> Option<&'static str> {
    (!args.any(|arg| arg == "--cache-dir")).then_some("--no-cache")
}

#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

#[path = "support/file_watch.rs"]
#[allow(dead_code)]
mod file_watch;

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::Path,
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
/// Far above a round trip's or a step's time, so a loaded machine does not
/// fail a test.
const TIMEOUT: Duration = Duration::from_secs(60);

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

/// A `--watch --serve 0` process on a copy of examples/dev.
struct Watch {
    dir: TempDir,
    process: Process,
    lines: Receiver<String>,
    /// `127.0.0.1:PORT`.
    address: String,
    /// The further `--watch` options it runs with.
    args: Vec<String>,
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
        let (process, lines, address) = Self::spawn(&dir, "0", &[]);
        Self {
            dir,
            process,
            lines,
            address,
            args: Vec::new(),
        }
    }

    /// [`Self::start`] with `--cache-dir`, restarted once, so that its
    /// session starts from the namespaces the cache installs.
    fn start_warm(label: &str) -> Self {
        let mut watch = Self::start(label);
        let cache = watch.dir.join("cache").to_str().unwrap().to_owned();
        watch.args = vec!["--cache-dir".into(), cache];
        watch.restart();
        watch.restart();
        watch
    }

    /// Watch `dir`'s `src` into its `dev` and serve it on `port`: the
    /// process, its stderr lines after the build, and its address.
    fn spawn(dir: &Path, port: &str, args: &[String]) -> (Process, Receiver<String>, String) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(["--watch", "--entry", "app.main", "--source-root"])
            .arg(dir.join("src"))
            .arg("--out-dir")
            .arg(dir.join("dev"))
            .args(["--poll-ms", "20", "--serve", port])
            .args(args)
            .args(no_cache(args.iter().map(String::as_str)))
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
        assert!(address.starts_with("127.0.0.1:"), "{serving}");
        until(&lines, "light-cljs: built ", "--watch");
        (process, lines, address)
    }

    /// Stop the watch and start it again on its port.
    fn restart(&mut self) {
        let _ = self.process.0.kill();
        let _ = self.process.0.wait();
        let port = self.address.rsplit_once(':').unwrap().1.to_owned();
        let (process, lines, address) = Self::spawn(&self.dir, &port, &self.args);
        assert_eq!(address, self.address);
        (self.process, self.lines) = (process, lines);
    }

    /// Save `text` as the source file `file` at once, and return the step's
    /// lines and its summary line.
    fn save(&self, file: &str, text: &str) -> (Vec<String>, String) {
        let path = self.dir.join("src").join(file);
        let temporary = path.with_file_name(".saving");
        fs::write(&temporary, text).unwrap();
        fs::rename(&temporary, &path).unwrap();
        until(&self.lines, "light-cljs: recompiled ", "--watch")
    }

    /// Append `text` to the source file `file` and save it; the line the
    /// text begins on.
    fn append(&self, file: &str, text: &str) -> usize {
        let source = fs::read_to_string(self.dir.join("src").join(file)).unwrap();
        let line = source.lines().count() + 2;
        let (step, summary) = self.save(file, &format!("{source}\n{text}"));
        assert!(
            !summary.contains("failed") && !step.iter().any(|line| line.contains("WARNING")),
            "{step:#?}\n{summary}"
        );
        line
    }

    /// A Node runtime running the first build, connected to the server.
    fn runtime(&self) -> Runtime {
        let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
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
            .expect("the connected REPL tests require Node");
        let lines = lines(child.stderr.take().unwrap());
        let pid = child.id();
        let process = Process(child);
        let (_, hello) = until(&lines, "message hello ", "the runtime");
        Runtime {
            _process: process,
            pid,
            id: hello["message hello ".len()..].to_owned(),
            _lines: lines,
        }
    }

    /// A terminal client connected to the server, with `flags`.
    fn repl(&self, flags: &[&str]) -> Repl {
        connect(&self.address, flags)
    }
}

/// A terminal client connected to the server at `address`, with `flags`.
fn connect(address: &str, flags: &[&str]) -> Repl {
    // One pipe for stdout and stderr keeps the client's lines in order.
    let mut child = Command::new("sh")
        .args(["-c", "exec \"$0\" \"$@\" 2>&1"])
        .arg(env!("CARGO_BIN_EXE_light-cljs"))
        .args(["--repl", "--connect", address])
        .args(flags)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let lines = lines(child.stdout.take().unwrap());
    Repl {
        _process: Process(child),
        stdin,
        lines,
        line: 1,
    }
}

/// A Node runtime connected to the server.
struct Runtime {
    _process: Process,
    /// Its process ID, which `(.-pid js/process)` gives.
    pid: u32,
    /// The ID the server gave it.
    id: String,
    _lines: Receiver<String>,
}

/// A `--repl --connect` client.
struct Repl {
    _process: Process,
    stdin: ChildStdin,
    lines: Receiver<String>,
    /// The input line the next input begins on.
    line: usize,
}

impl Repl {
    /// End the input, which closes the session, and wait for the client to
    /// exit.
    fn close(mut self) {
        drop(self.stdin);
        let status = self._process.0.wait().unwrap();
        assert!(status.success(), "{status}");
    }

    /// Type `input`, and return what the client printed for it: the lines
    /// up to the error of a line typed after it that does not read, which
    /// the client reports without the server and leaves `*1` as it is.
    fn eval(&mut self, input: &str) -> Vec<String> {
        writeln!(self.stdin, "{input}\n)").unwrap();
        self.stdin.flush().unwrap();
        self.line += input.lines().count() + 1;
        let marker = format!(
            "light-cljs: <stdin>.cljc:{}:1: unexpected closing delimiter )",
            self.line - 1
        );
        until(&self.lines, &marker, "the REPL").0
    }

    /// The value line of `input`, which prints nothing else.
    fn value(&mut self, input: &str) -> String {
        match &self.eval(input)[..] {
            [value] => value.clone(),
            other => panic!("{input} printed {other:#?}"),
        }
    }
}

/// `(+ 1 2)` goes from the terminal to the server, which compiles it, to the
/// runtime, which runs and prints it, and back; the runtime keeps `*1`,
/// `*2`, `*3` and the application's state; a value that is not finite data
/// also comes back as a reference; reader errors are the client's own.
#[test]
fn a_form_round_trips_through_a_node_runtime() {
    let watch = Watch::start("dev-repl-round-trip");
    let _runtime = watch.runtime();
    let mut repl = watch.repl(&[]);
    assert_eq!(repl.value("(+ 1 2)"), "3");
    // The round trip, measured at the terminal: from the line typed to the
    // value printed.
    let mut times = (0..20)
        .map(|_| {
            let begun = Instant::now();
            assert_eq!(repl.value("(+ 1 2)"), "3");
            begun.elapsed()
        })
        .collect::<Vec<_>>();
    times.sort();
    eprintln!(
        "(+ 1 2) round trip: median {:.1} ms, {:.1}-{:.1} ms over {} (with a marker form each)",
        times[times.len() / 2].as_secs_f64() * 1000.0,
        times[0].as_secs_f64() * 1000.0,
        times[times.len() - 1].as_secs_f64() * 1000.0,
        times.len()
    );
    assert!(times[times.len() / 2] < Duration::from_secs(2), "{times:?}");
    assert_eq!(repl.value("(* *1 2)"), "6");
    assert_eq!(repl.value("[*1 *2 *3]"), "[6 3 3]");
    assert_eq!(
        repl.value("@app.state/state"),
        "{:clicks 1, :card #app.view.Card{:label \"clicks\", :clicks 1}}"
    );
    assert_eq!(repl.value("(str *ns*)"), "\"app.main\"");
    assert_eq!(repl.eval("(def x 5) (inc x)"), ["5", "6"]);
    // Data comes back as text alone; anything else with a reference.
    assert_eq!(
        repl.value("[1 :a \"b\" {:c #{2}} nil]"),
        "[1 :a \"b\" {:c #{2}} nil]"
    );
    assert_eq!(
        repl.eval("(atom {:a 1})"),
        ["#object[cljs.core.Atom {:val {:a 1}}]", ";; ref 1"]
    );
    assert_eq!(repl.value("@(light.dev/ref 1)"), "{:a 1}");
    assert_eq!(
        repl.value("(light.dev/inspect 1)"),
        "{:ref 1, :type \"Atom\", :deref {:a 1}}"
    );
    assert_eq!(repl.eval("(range)").last().unwrap(), ";; ref 2");
    let missing = repl.eval("(light.dev/ref 9)");
    assert!(
        missing[0].starts_with("light-cljs: <stdin>.cljc:")
            && missing[0].ends_with(": Error: light.dev: no reference 9 in this runtime"),
        "{missing:#?}"
    );
    // A reader error is found locally and discards the rest of its line.
    let line = repl.line;
    assert_eq!(
        repl.eval(") (+ 1 2)"),
        [format!(
            "light-cljs: <stdin>.cljc:{line}:1: unexpected closing delimiter )"
        )]
    );
    assert_eq!(repl.value("(+ 1\n2)"), "3");
    // The server reads what the client read as the embedded REPL does: a
    // prefix over a conditional that selects nothing takes the next form,
    // and discarded forms go with the form after them.
    assert_eq!(repl.value("'#?(:clj 1) (+ 1 2)"), "(+ 1 2)");
    assert_eq!(repl.value("#_(no-such-form) (+ 1 2)"), "3");
    // A form that fails leaves the forms after it to run.
    let both = repl.eval("(throw (js/Error. \"first\")) (+ 1 2)");
    assert!(both[0].ends_with(": Error: first"), "{both:#?}");
    assert_eq!(both.last().unwrap(), "3");

    let mut short = watch.repl(&["--print-length", "3"]);
    assert_eq!(short.eval("(range 10)"), ["(0 1 2 ...)", ";; ref 3"]);
    // Each session keeps its own history.
    assert_eq!(short.value("*2"), "nil");
}

/// A watch started warm from `--cache-dir` compiles REPL forms in an
/// installed namespace as a cold one does: with the aliases, refers and
/// records the namespace's `ns` form and definitions gave it, the macro it
/// refers expanding in the session's engine, and a redefinition seen by the
/// installed namespace's callers.
#[test]
fn repl_forms_compile_in_installed_namespaces() {
    let inputs = [
        "(in-ns 'app.view)",
        "(:clicks @state/state)",
        "(labeled \"x\" 2)",
        "(describe (->Card \"c\" 3))",
        "(defn render [] (labeled \"r\" (:clicks @state/state)))",
        "(in-ns 'app.main)",
        "(view/render)",
        "(str *ns*)",
    ];
    let mut printed = Vec::new();
    for watch in [
        Watch::start("dev-repl-cold"),
        Watch::start_warm("dev-repl-warm"),
    ] {
        let _runtime = watch.runtime();
        let mut repl = watch.repl(&[]);
        printed.push(
            inputs
                .iter()
                .map(|input| repl.eval(input))
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(
        printed[0]
            .iter()
            .map(|lines| lines.join("\n"))
            .collect::<Vec<_>>(),
        [
            "nil",
            "1",
            "\"x: [2]\"",
            "\"c: [3]\"",
            "#object[Function]\n;; ref 1",
            "nil",
            "\"r: [1]\"",
            "\"app.main\""
        ]
    );
    assert_eq!(printed[0], printed[1]);
}

/// A cached namespace whose file adds requires after its `ns` form, with
/// `ns*` and a top-level `require`, keeps their aliases in a warm-started
/// watch's session, as a cold one has them.
#[test]
fn repl_forms_keep_the_later_directives_of_installed_namespaces() {
    let mut printed = Vec::new();
    for warm in [false, true] {
        let mut watch = if warm {
            Watch::start_warm("dev-repl-later-w")
        } else {
            Watch::start("dev-repl-later-c")
        };
        let main = fs::read_to_string(watch.dir.join("src/app/main.cljs")).unwrap();
        let main = main.replacen(
            "\n(defn ^:dev/before-load",
            "\n(ns* (:require '[clojure.string :as text]))\n\n(require '[clojure.set :as sets])\n\n(defn ^:dev/before-load",
            1,
        );
        watch.save("app/main.cljs", &main);
        // The edited `app.main` is published by a start, and installed by
        // the next.
        watch.restart();
        if warm {
            watch.restart();
        }
        let _runtime = watch.runtime();
        let mut repl = watch.repl(&[]);
        printed.push(
            [
                "(str *ns*)",
                "(text/upper-case \"a\")",
                "(sets/union #{1} #{2})",
            ]
            .map(|input| repl.eval(input).join("\n")),
        );
    }
    assert_eq!(printed[0], ["\"app.main\"", "\"A\"", "#{1 2}"]);
    assert_eq!(printed[0], printed[1]);
}

/// A definition made at the REPL is seen by a namespace a save reloads, and
/// a definition the reload makes is seen at the REPL: they share the
/// watch's program and the runtime's registry.
#[test]
fn definitions_cross_between_the_repl_and_reloads() {
    let watch = Watch::start("dev-repl-definitions");
    let _runtime = watch.runtime();
    let mut repl = watch.repl(&[]);
    assert_eq!(repl.value("(in-ns 'app.state)"), "nil");
    assert_eq!(repl.value("(def bonus 10)"), "10");
    assert_eq!(repl.value("(str *ns*)"), "\"app.state\"");
    watch.append(
        "app/view.cljs",
        "(defn bonus-label [] (str \"bonus \" state/bonus))\n",
    );
    assert_eq!(repl.value("(app.view/bonus-label)"), "\"bonus 10\"");
    assert_eq!(repl.value("(set! bonus 11)"), "11");
    assert_eq!(repl.value("(app.view/bonus-label)"), "\"bonus 11\"");
    // A REPL redefinition of a reloaded function is what its callers run.
    assert_eq!(repl.value("(in-ns 'app.view)"), "nil");
    assert_eq!(
        repl.eval("(defn bonus-label [] \"replaced\")"),
        ["#object[Function]", ";; ref 1"]
    );
    assert_eq!(
        repl.value("((fn [] (app.view/bonus-label)))"),
        "\"replaced\""
    );
}

/// An error thrown in the runtime comes back with its stack mapped to the
/// sources, of the REPL form and of a reloaded namespace, and is `*e` there;
/// an analysis error comes from the server, located, and runs nothing.
#[test]
fn errors_come_back_located() {
    let watch = Watch::start("dev-repl-errors");
    let _runtime = watch.runtime();
    let mut repl = watch.repl(&[]);
    let line = watch.append(
        "app/view.cljs",
        "(defn explode []\n  (throw (js/Error. \"kaboom\")))\n",
    );
    let error = repl.eval("(app.view/explode)");
    assert_eq!(
        error[0], "light-cljs: <stdin>.cljc:1:1: Error: kaboom",
        "{error:#?}"
    );
    let source = watch.dir.join("src/app/view.cljs");
    let frame = format!(
        "{}:{}:",
        fs::canonicalize(&source).unwrap().display(),
        line + 1
    );
    assert!(error[1].contains(&frame), "{frame} in {error:#?}");
    assert!(
        error.last().unwrap().contains("(<stdin>.cljc:1:1)"),
        "{error:#?}"
    );
    assert_eq!(repl.value("(ex-message *e)"), "\"kaboom\"");
    let thrown = repl.eval("(throw (ex-info \"bad\" {:a 1}))");
    assert_eq!(
        thrown[0], "light-cljs: <stdin>.cljc:5:1: Error: bad",
        "{thrown:#?}"
    );
    assert!(
        thrown.iter().any(|line| line.contains("(<stdin>.cljc:5:")),
        "{thrown:#?}"
    );
    assert_eq!(thrown.last().unwrap(), "  data: {:a 1}");
    assert_eq!(
        repl.eval("(let [x] x)"),
        [
            "light-cljs: <stdin>.cljc:7:1: macro expansion: while expanding let: binding vector requires binding/initializer pairs"
        ]
    );
    // An analysis error does not change *e.
    assert_eq!(repl.value("(ex-message *e)"), "\"bad\"");
    let warned = repl.eval("(no-such-function 1)");
    assert_eq!(
        warned[0],
        "WARNING: <stdin>.cljc:11:2: Use of undeclared Var app.main/no-such-function [undeclared-var]"
    );
    assert!(
        warned[1].starts_with("light-cljs: <stdin>.cljc:11:1: TypeError"),
        "{warned:#?}"
    );
}

/// Forms go to the most recently connected runtime unless
/// `light.dev/select!` chose another; a save's load and what a REPL form
/// loads reach every runtime; each REPL connection has its own namespace.
#[test]
fn two_runtimes_and_selection() {
    let watch = Watch::start("dev-repl-runtimes");
    let first = watch.runtime();
    let second = watch.runtime();
    let mut repl = watch.repl(&[]);
    assert_eq!(
        repl.value("(light.dev/runtimes)"),
        format!(
            "[{{:id {}, :agent \"node\", :selected false}} {{:id {}, :agent \"node\", :selected true}}]",
            first.id, second.id
        )
    );
    assert_eq!(repl.value("(.-pid js/process)"), second.pid.to_string());
    assert_eq!(
        repl.value(&format!("(light.dev/select! {})", first.id)),
        first.id
    );
    assert_eq!(repl.value("(.-pid js/process)"), first.pid.to_string());
    // What a form loads goes to every runtime.
    assert_eq!(
        repl.eval("(require '[clojure.set :as set]) (set/union #{1} #{2})"),
        ["nil", "#{1 2}"]
    );
    // What a form loaded before it failed reaches the runtimes too: the
    // session counts it as loaded.
    let failed = repl.eval("(require 'clojure.walk 'no.such.namespace)");
    assert!(
        failed[0].starts_with("light-cljs: <stdin>.cljc:")
            && failed[0].contains("no.such.namespace"),
        "{failed:#?}"
    );
    assert_eq!(
        repl.value("(clojure.walk/keywordize-keys {\"a\" 1})"),
        "{:a 1}"
    );
    watch.append("app/view.cljs", "(defn shout [] \"HEY\")\n");
    assert_eq!(
        repl.value("[(app.view/shout) (set/union #{1} #{2})]"),
        "[\"HEY\" #{1 2}]"
    );
    assert_eq!(repl.value("(light.dev/select! nil)"), "nil");
    assert_eq!(repl.value("(.-pid js/process)"), second.pid.to_string());
    assert_eq!(
        repl.value(
            "[(app.view/shout) (set/union #{1} #{2}) (clojure.walk/keywordize-keys {\"a\" 1})]"
        ),
        "[\"HEY\" #{1 2} {:a 1}]"
    );
    let line = repl.line;
    assert_eq!(
        repl.eval("(light.dev/select! 7)"),
        [format!(
            "light-cljs: <stdin>.cljc:{line}:1: no runtime 7 is connected; (light.dev/runtimes) lists those that are"
        )]
    );
    // A namespace per connection.
    assert_eq!(repl.value("(in-ns 'app.state)"), "nil");
    let mut other = watch.repl(&[]);
    assert_eq!(other.value("(str *ns*)"), "\"app.main\"");
    assert_eq!(repl.value("(str *ns*)"), "\"app.state\"");
    // A chosen runtime that went away is not replaced by another.
    assert_eq!(
        repl.value(&format!("(light.dev/select! {})", first.id)),
        first.id
    );
    let first_id = first.id.clone();
    drop(first);
    let begun = Instant::now();
    loop {
        let listed = repl.value("(light.dev/runtimes)");
        if !listed.contains(&format!(":id {},", first_id)) {
            break;
        }
        assert!(begun.elapsed() < TIMEOUT, "{listed}");
        std::thread::sleep(Duration::from_millis(20));
    }
    let gone = repl.eval("(+ 1 2)");
    assert!(
        gone[0].contains(&format!(
            "runtime {first_id}, which (light.dev/select! {first_id}) chose, is not connected"
        )),
        "{gone:#?}"
    );
}

/// A session that closes takes its history out of the runtimes, and a
/// session belongs to one server: after a restart, a client of the old
/// server opens a new session rather than take another's, and a new session
/// does not get the history a surviving runtime kept for an old one.
#[test]
fn sessions_belong_to_one_server() {
    let mut watch = Watch::start("dev-repl-sessions");
    let _runtime = watch.runtime();
    let sessions = "(.-size (.. js/globalThis -__lightDev -repl -sessions))";
    let mut first = watch.repl(&[]);
    assert_eq!(first.value("42"), "42");
    let mut second = watch.repl(&[]);
    assert_eq!(second.value(sessions), "2");
    second.close();
    let begun = Instant::now();
    while first.value(sessions) != "1" {
        assert!(
            begun.elapsed() < TIMEOUT,
            "the closed session's history stayed"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    watch.restart();
    let mut third = watch.repl(&[]);
    let begun = Instant::now();
    while third.value("(light.dev/runtimes)") == "[]" {
        assert!(begun.elapsed() < TIMEOUT, "the runtime did not reconnect");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(third.value("[*1 *2 *3]"), "[nil nil nil]");
    assert_eq!(
        first.eval("*1"),
        [
            "light-cljs: the dev server forgot this REPL's session; a new one starts in app.main",
            "nil"
        ]
    );
}

/// A page cannot use the REPL endpoint: a request with an `Origin`, or
/// without a JSON content type, which a page could send to another site, is
/// refused.
#[test]
fn pages_cannot_evaluate() {
    let watch = Watch::start("dev-repl-pages");
    let post = |headers: &str| {
        let body = r#"{"op":"open"}"#;
        let mut stream = TcpStream::connect(&watch.address).unwrap();
        write!(
            stream,
            "POST /repl HTTP/1.1\r\nHost: {}\r\n{headers}Content-Length: {}\r\n\r\n{body}",
            watch.address,
            body.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response.lines().next().unwrap().to_owned()
    };
    assert_eq!(
        post("Content-Type: application/json\r\nOrigin: http://example.com\r\n"),
        "HTTP/1.1 403 Forbidden"
    );
    assert_eq!(
        post("Content-Type: text/plain\r\n"),
        "HTTP/1.1 415 Unsupported Media Type"
    );
    assert_eq!(
        post("Content-Type: application/json\r\n"),
        "HTTP/1.1 200 OK"
    );
}

/// `light-cljs watch features` on a temporary copy of examples/features
/// (docs/build-configuration.md, "Several builds in one process"): a
/// `--repl --connect` session starts bound to `:features`, in its entry,
/// `(light.dev/repl :features)` binds it there again, and an id the watch
/// does not run, or an argument that is not one keyword, is an exact error
/// that leaves the session as it was. A runtime of the build is listed with
/// it.
#[test]
fn watch_features_binds_its_sessions_with_light_dev_repl() {
    let (_dir, project) = file_watch::features("dev-repl-features");
    let mut watch =
        file_watch::Light::spawn(&project, &["watch", "features"], Some("{:http {:port 0}}"));
    let (address, _, _) = watch.serving();
    watch.until("[:features] built ");
    let mut runtime = file_watch::node_runtime(
        &project.join("out/dev"),
        &format!("http://{address}/events"),
        1_000_000,
    );
    let (_, hello) = runtime.until("message hello ");
    let id = &hello["message hello ".len()..];
    let mut repl = connect(&address, &[]);
    assert_eq!(
        repl.value("(light.dev/runtimes)"),
        format!("[{{:id {id}, :build :features, :agent \"node\", :selected true}}]")
    );
    assert_eq!(repl.value("(str *ns*)"), "\"app.main\"");
    assert_eq!(repl.value("(in-ns 'app.shared)"), "nil");
    assert_eq!(
        repl.value("(light.dev/repl :features)"),
        "[:selected :features]"
    );
    assert_eq!(repl.value("(str *ns*)"), "\"app.main\"");
    for (form, message) in [
        (
            "(light.dev/repl :nope)",
            "the watch runs no build :nope; its builds are :features",
        ),
        (
            "(light.dev/repl \"features\")",
            "light.dev/repl takes one build id, a keyword such as :app",
        ),
        (
            "(light.dev/repl)",
            "light.dev/repl takes one build id, a keyword such as :app",
        ),
        (
            "(light.dev/repl :features :features)",
            "light.dev/repl takes one build id, a keyword such as :app",
        ),
        (
            "(light.dev/repl :app/features)",
            "light.dev/repl takes one build id, a keyword such as :app",
        ),
    ] {
        let line = repl.line;
        assert_eq!(
            repl.eval(form),
            [format!("light-cljs: <stdin>.cljc:{line}:1: {message}")],
            "{form}"
        );
    }
    assert_eq!(repl.value("(str *ns*)"), "\"app.main\"");
}

/// A flag-only `--watch` runs one unnamed build: `(light.dev/repl ...)` is
/// an exact error saying so, and `light.dev/runtimes` names no build.
#[test]
fn a_flag_only_watch_runs_no_named_builds() {
    let watch = Watch::start("dev-repl-unnamed");
    let runtime = watch.runtime();
    let mut repl = watch.repl(&[]);
    let line = repl.line;
    assert_eq!(
        repl.eval("(light.dev/repl :app)"),
        [format!(
            "light-cljs: <stdin>.cljc:{line}:1: the watch runs no named builds: light.dev/repl switches between the builds of light-cljs watch ID ..."
        )]
    );
    assert_eq!(
        repl.value("(light.dev/runtimes)"),
        format!("[{{:id {}, :agent \"node\", :selected true}}]", runtime.id)
    );
    // Called anywhere but as a form of its own, it throws in the runtime.
    let thrown = repl.eval("(do (light.dev/repl :app))");
    assert!(
        thrown[0].contains(
            "light.dev/repl is a command of the connected REPL: evaluate it as a form of its own"
        ),
        "{thrown:#?}"
    );
}

/// The Phase 3 test project, tests/fixtures/build-configuration/multi.
const MULTI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/build-configuration/multi"
);

/// `light-cljs watch app worker` on a temporary copy of the multi fixture:
/// a `--repl --connect` session evaluates in the app's runtime, then after
/// `(light.dev/repl :worker)` in the worker's, in its entry namespace, then
/// back in the app's, with a form after the command in the same input,
/// each reading a Var only its build defines and its runtime's process ID.
#[test]
fn connected_repl_switches_between_app_and_worker() {
    let (_dir, project) = file_watch::project("dev-repl-multi", Path::new(MULTI));
    let mut watch = file_watch::Light::spawn(
        &project,
        &["watch", "app", "worker"],
        Some("{:http {:port 0} :nrepl false}"),
    );
    let (address, _, _) = watch.serving();
    watch.until("[:app] built ");
    watch.until("[:worker] built ");
    let runtime = |build: &str| {
        let mut runtime = file_watch::node_runtime(
            &project.join("out").join(build),
            &format!("http://{address}/{build}/events"),
            1_000_000,
        );
        runtime.until("message load replay ");
        runtime
    };
    let (app, worker) = (runtime("app"), runtime("worker"));
    let (app_pid, worker_pid) = (app.child.id(), worker.child.id());
    let mut repl = connect(&address, &[]);
    assert_eq!(
        repl.value("[app.build-info/built-by (.-pid js/process)]"),
        format!("[\"light-cljs\" {app_pid}]")
    );
    assert_eq!(
        repl.value("(light.dev/repl :worker)"),
        "[:selected :worker]"
    );
    assert_eq!(repl.value("(str *ns*)"), "\"app.worker\"");
    assert_eq!(
        repl.value("[(jobs/run 2) (.-pid js/process)]"),
        format!("[42 {worker_pid}]")
    );
    assert_eq!(
        repl.eval("(light.dev/repl :app) [app.build-info/built-by (.-pid js/process)]"),
        [
            "[:selected :app]".to_owned(),
            format!("[\"light-cljs\" {app_pid}]")
        ]
    );
    assert_eq!(repl.value("(str *ns*)"), "\"app.main\"");
}
