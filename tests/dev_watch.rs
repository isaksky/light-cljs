//! `--watch` (docs/dev-workflow.md, "The watch loop" and "Phase 2: watch"),
//! run as a subprocess on a copy of examples/dev: a saved change to a
//! namespace rewrites its unit and the manifest, which the Phase 0 driver
//! (tests/support/dev_execution.mjs) applies to the running build made before
//! the change, with the hook protocol, and also loads afresh.

#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

#[path = "support/dev_execution.rs"]
#[allow(dead_code)]
mod dev_execution;

#[path = "support/file_watch.rs"]
#[allow(dead_code)]
mod file_watch;

use dev_execution::run_dev;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::Duration,
};
use temp_dir::TempDir;

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/dev");
/// Far above a step's time, so a loaded machine does not fail a test.
const TIMEOUT: Duration = Duration::from_secs(60);
/// The driver's record of the example: its stdout after `start`, and after
/// applying units, with nothing traced and an undefined value.
fn record(stdout: &str) -> String {
    format!(
        r#"{{"id":"program","outcome":{{"status":"returned","value":{{"$value":"undefined"}}}},"trace":[],"stdout":{}}}"#,
        json(stdout)
    )
}
fn json(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// A `--watch` process on a copy of examples/dev, killed when dropped.
struct Watch {
    dir: TempDir,
    child: Child,
    lines: Receiver<String>,
    /// The line that reports the first build.
    built: String,
    /// The stderr lines before it.
    startup: Vec<String>,
}

impl Watch {
    /// Copy examples/dev to `src` in a fresh directory and watch it into
    /// `dev`, polling every 20 ms; return once the build is written and the
    /// polling has begun, with a copy of that first build in `before`.
    fn start(label: &str) -> Self {
        Self::start_with(label, |_| {})
    }

    /// [`Self::start`] after `setup` changed the copy, in the directory it
    /// is given.
    fn start_with(label: &str, setup: impl FnOnce(&Path)) -> Self {
        Self::start_with_args(label, &[], setup)
    }

    /// [`Self::start_with`], also passing `args` to `--watch`.
    fn start_with_args(label: &str, args: &[&str], setup: impl FnOnce(&Path)) -> Self {
        let dir = TempDir::new(label);
        fs::create_dir_all(dir.join("src/app")).unwrap();
        for name in ["state", "macros", "view", "main"] {
            let file = format!("app/{name}.cljs");
            fs::copy(Path::new(EXAMPLE).join(&file), dir.join("src").join(&file)).unwrap();
        }
        setup(&dir.join("src"));
        let (child, lines) = Self::spawn(&dir, args);
        let mut watch = Self {
            dir,
            child,
            lines,
            built: String::new(),
            startup: Vec::new(),
        };
        watch.started();
        watch
    }

    /// Stop the watch and start `--watch` again with `args` on the same
    /// sources and output directory, as a developer restarts it: with
    /// `--cache-dir`, it finds the artifacts the last run published.
    fn restart(&mut self, args: &[&str]) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        fs::remove_dir_all(self.dir.join("before")).unwrap();
        (self.child, self.lines) = Self::spawn(&self.dir, args);
        self.started();
    }

    /// A `--watch` process with `args` on the sources of `dir`, and its
    /// stderr lines.
    fn spawn(dir: &Path, args: &[&str]) -> (Child, Receiver<String>) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(["--watch", "--entry", "app.main", "--source-root"])
            .arg(dir.join("src"))
            .arg("--out-dir")
            .arg(dir.join("dev"))
            .args(["--poll-ms", "20"])
            .args(args)
            .args((!args.contains(&"--cache-dir")).then_some("--no-cache"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let (send, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        (child, lines)
    }

    /// Wait for the line that reports the first build, and copy that build
    /// to `before`.
    fn started(&mut self) {
        let watch = self;
        let (startup, line) = watch.until("light-cljs: built ");
        watch.startup = startup;
        assert!(
            line.starts_with("light-cljs: built app.main (")
                && line.ends_with("; watching every 20 ms"),
            "{line}"
        );
        eprintln!("dev_watch: {line}");
        watch.built = line;
        fs::create_dir(watch.dir.join("before")).unwrap();
        for entry in fs::read_dir(watch.dir.join("dev")).unwrap() {
            let entry = entry.unwrap();
            fs::copy(
                entry.path(),
                watch.dir.join("before").join(entry.file_name()),
            )
            .unwrap();
        }
    }

    /// The stderr lines up to the first one that starts with `prefix`, and
    /// that line.
    fn until(&mut self, prefix: &str) -> (Vec<String>, String) {
        let mut before = Vec::new();
        loop {
            match self.lines.recv_timeout(TIMEOUT) {
                Ok(line) if line.starts_with(prefix) => return (before, line),
                // What `--stats` adds after each step's line, unless asked
                // for: a watch with it prints the lines one without does.
                Ok(line) if line.starts_with("light-cljs: live ") => {}
                Ok(line) => before.push(line),
                Err(RecvTimeoutError::Timeout) => {
                    panic!("no line {prefix:?} in {TIMEOUT:?}; got {before:#?}")
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("--watch exited before a line {prefix:?}; got {before:#?}")
                }
            }
        }
    }

    /// Save `text` as the source file `file` (`app/view.cljs`) at once, as
    /// an editor that renames its new text over the file does, and return
    /// the step's lines and its summary line.
    fn save(&mut self, file: &str, text: &str) -> (Vec<String>, String) {
        let path = self.dir.join("src").join(file);
        let temporary = path.with_file_name(".saving");
        fs::write(&temporary, text).unwrap();
        fs::rename(&temporary, &path).unwrap();
        self.until("light-cljs: recompiled ")
    }

    fn source(&self, file: &str) -> String {
        fs::read_to_string(self.dir.join("src").join(file)).unwrap()
    }

    fn dev(&self, file: &str) -> PathBuf {
        self.dir.join("dev").join(file)
    }

    fn read(&self, file: &str) -> String {
        fs::read_to_string(self.dev(file)).unwrap()
    }

    /// The driver's record of the build made before any change, started and
    /// then given the units `files` of the build as it is now.
    fn reload(&self, files: &[&str]) -> String {
        let units = files.iter().map(|file| self.dev(file)).collect::<Vec<_>>();
        run_dev(&self.dir.join("before"), true, &units).unwrap()
    }

    /// The driver's records of the build as it is now and of a fresh
    /// `--reloadable` build of the sources as they are now, both started.
    fn now_and_fresh(&self) -> [String; 2] {
        let fresh = self.dir.join("fresh");
        let _ = fs::remove_dir_all(&fresh);
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args([
                "--compile",
                "--no-cache",
                "--entry",
                "app.main",
                "--source-root",
            ])
            .arg(self.dir.join("src"))
            .arg("--reloadable")
            .arg("--out-dir")
            .arg(&fresh)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        [
            run_dev(&self.dir.join("dev"), true, &[]).unwrap(),
            run_dev(&fresh, true, &[]).unwrap(),
        ]
    }

    /// The stderr lines until the watch exits, and how it exited.
    fn exit(&mut self) -> (Vec<String>, std::process::ExitStatus) {
        let mut lines = Vec::new();
        loop {
            match self.lines.recv_timeout(TIMEOUT) {
                Ok(line) => lines.push(line),
                Err(RecvTimeoutError::Timeout) => panic!("--watch still runs; got {lines:#?}"),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        (lines, self.child.wait().unwrap())
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The milliseconds of a summary line `light-cljs: recompiled NAMES in X ms`.
fn millis(line: &str) -> f64 {
    line.rsplit_once(" in ")
        .and_then(|(_, rest)| rest.split(' ').next())
        .and_then(|number| number.parse().ok())
        .unwrap_or_else(|| panic!("no time in {line:?}"))
}

/// Where the source map `map` of the unit file `unit` maps the first
/// occurrence of `needle` in the unit, read with Node's `SourceMap`: the
/// source, and its zero-based line and column.
fn original(unit: &Path, map: &Path, needle: &str) -> String {
    const SCRIPT: &str = r#"
const { SourceMap } = require("node:module");
const fs = require("node:fs");
const [unit, map, needle] = process.argv.slice(1);
const text = fs.readFileSync(unit, "utf8");
const lines = text.slice(0, text.indexOf(needle)).split("\n");
const entry = new SourceMap(JSON.parse(fs.readFileSync(map, "utf8")))
  .findEntry(lines.length - 1, lines[lines.length - 1].length);
console.log(`${entry.originalSource}:${entry.originalLine}:${entry.originalColumn}`);
"#;
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("-e")
        .arg(SCRIPT)
        .arg(unit)
        .arg(map)
        .arg(needle)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim_end().into()
}

/// The zero-based line and column of the first `needle` in `text`.
fn position(text: &str, needle: &str) -> String {
    let before = &text[..text.find(needle).unwrap()];
    let line = before.matches('\n').count();
    let column = before.len() - before.rfind('\n').map_or(0, |at| at + 1);
    format!("{line}:{column}")
}

/// A saved change to `app.view` rewrites its unit, and only its unit: the
/// driver applies it to the running first build with the hooks (the next
/// render runs the new code and the `defonce` state keeps its click) and
/// loads the rewritten build afresh. The unit is on disk within 20 ms of the
/// poll noticing the save, the time the step line reports, at best of five
/// saves, with the release CLI the Goal table measures (`cargo nextest run
/// --release --test dev_watch`); the debug build is about twice as slow and
/// runs on slower CI machines, so there the bound is 100 ms, the Goal
/// table's whole save-to-page budget.
#[test]
fn a_saved_change_rewrites_the_unit_the_running_build_reloads() {
    let mut watch = Watch::start("dev-watch-change");
    assert!(
        watch.built.contains(" (4 namespaces) into "),
        "{}",
        watch.built
    );
    let manifest = watch.read("manifest.json");
    let others =
        ["app.state.js", "app.macros.js", "app.main.js", "index.js"].map(|file| watch.read(file));
    let view = watch.source("app/view.cljs");
    let edited = view.replace(
        "(js/console.log (describe card))",
        "(js/console.log \"render:\" (describe card))",
    );
    assert_ne!(edited, view);
    let (lines, step) = watch.save("app/view.cljs", &edited);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    let mut times = vec![millis(&step)];
    assert_eq!(watch.read("manifest.json"), manifest);
    assert_eq!(
        ["app.state.js", "app.macros.js", "app.main.js", "index.js"].map(|file| watch.read(file)),
        others
    );
    let unit = watch.read("app.view.js");
    assert!(
        unit.starts_with(
            "globalThis.__lightDev.define(\"app.view\", [\"app.macros\", \"app.state\"], "
        ) && unit.ends_with("});\n//# sourceMappingURL=app.view.js.map\n"),
        "{unit}"
    );
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\nrender: clicks: [2]\n")
    );
    assert_eq!(
        run_dev(&watch.dir.join("dev"), true, &[]).unwrap(),
        record("start\nrender: clicks: [1]\n")
    );
    // The map maps the unit to the source the session read.
    let map = watch.read("app.view.js.map");
    assert!(
        map.starts_with(
            "{\"version\":3,\"file\":\"app.view.js\",\"sources\":[\"../src/app/view.cljs\"]"
        ),
        "{map}"
    );
    // Each form's text maps to its place in the file, the last form's (on
    // the unit's last lines) as the first one's.
    for needle in ["\"render:\"", "\"app.view/Describe\""] {
        let source = if needle == "\"render:\"" {
            needle
        } else {
            "(defprotocol Describe"
        };
        assert_eq!(
            original(
                &watch.dev("app.view.js"),
                &watch.dev("app.view.js.map"),
                needle
            ),
            format!("../src/app/view.cljs:{}", position(&edited, source))
        );
    }
    for count in 1..5 {
        let text = view.replace(
            "(js/console.log (describe card))",
            &format!(
                "(js/console.log \"render{}\" (describe card))",
                "!".repeat(count)
            ),
        );
        let (lines, step) = watch.save("app/view.cljs", &text);
        assert!(lines.is_empty(), "{lines:#?}");
        assert!(
            step.starts_with("light-cljs: recompiled app.view in "),
            "{step}"
        );
        times.push(millis(&step));
    }
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\nrender!!!! clicks: [2]\n")
    );
    let [rewritten, built] = [
        watch.dev("app.view.js"),
        watch.dir.join("before/app.view.js"),
    ]
    .map(|file| fs::read_to_string(file).unwrap().len());
    eprintln!(
        "dev_watch: save of app.view to its unit on disk, ms: {times:?}; app.view.js {rewritten} bytes (map {}), {built} before",
        watch.read("app.view.js.map").len(),
    );
    // The rewritten unit registers its namespace once, as the build's does,
    // rather than every namespace in each form's unit (light-cljs-301).
    assert_eq!(
        watch
            .read("app.view.js")
            .matches("registerReaderNamespace(")
            .count(),
        1
    );
    assert!(
        rewritten.abs_diff(built) * 10 <= built,
        "app.view.js is {rewritten} bytes, the build's {built}"
    );
    let best = times.iter().copied().fold(f64::INFINITY, f64::min);
    let bound = if cfg!(debug_assertions) { 100.0 } else { 20.0 };
    assert!(best < bound, "steps took {times:?} ms");
}

/// A form that fails stops the step with its error, located in its file, and
/// leaves the previous unit and the manifest; the next save compiles again.
#[test]
fn a_failing_form_leaves_the_previous_unit() {
    let mut watch = Watch::start("dev-watch-error");
    let manifest = watch.read("manifest.json");
    let unit = watch.read("app.view.js");
    let view = watch.source("app/view.cljs");
    let failed = "light-cljs: recompiled nothing in ";
    let kept = "; app.view failed and keeps its previous unit";
    for (broken, error) in [
        // A reader error.
        (
            view.replace("(defn render", "(defn render ("),
            ":23:1: unexpected end of input; expected a closing delimiter",
        ),
        // An analysis error after forms that compiled.
        (
            view.replace(
                "(let [card (->Card \"clicks\" (:clicks @state/state))]",
                "(let [card]",
            ),
            ":20:3: macro expansion: while expanding let: binding vector requires binding/initializer pairs",
        ),
    ] {
        assert_ne!(broken, view);
        let (lines, step) = watch.save("app/view.cljs", &broken);
        assert!(step.starts_with(failed) && step.ends_with(kept), "{step}");
        let path = watch.dir.join("src/app/view.cljs");
        let path = fs::canonicalize(path).unwrap();
        assert!(
            lines.len() == 1
                && lines[0].starts_with(&format!("light-cljs: {}{error}", path.display())),
            "{lines:#?}"
        );
        assert_eq!(watch.read("app.view.js"), unit);
        assert_eq!(watch.read("manifest.json"), manifest);
    }
    let fixed = view.replace(
        "(js/console.log (describe card))",
        "(js/console.log \"fixed:\" (describe card))",
    );
    let (lines, step) = watch.save("app/view.cljs", &fixed);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\nfixed: clicks: [2]\n")
    );
    // Analysis is in warning mode: a use of a Var the file defines later is
    // a warning, located in the file, and the unit runs as upstream's would.
    let later = format!(
        "{}\n(defn later [text] (str \"later \" text))\n",
        view.replace(
            "(js/console.log (describe card))",
            "(js/console.log (later (describe card)))",
        )
    );
    let (lines, step) = watch.save("app/view.cljs", &later);
    let path = fs::canonicalize(watch.dir.join("src/app/view.cljs")).unwrap();
    assert!(
        lines.len() == 1
            && lines[0].starts_with(&format!(
                "WARNING: {}:22:22: Use of undeclared Var app.view/later [undeclared-var]",
                path.display()
            )),
        "{lines:#?}"
    );
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\nlater clicks: [2]\n")
    );
}

/// A change to a namespace that declares macros recompiles the namespaces
/// that require it from their sources, whose units hold the expansions.
#[test]
fn a_macro_change_rewrites_the_units_that_use_it() {
    let mut watch = Watch::start("dev-watch-macro");
    let main = watch.read("app.main.js");
    let macros = watch.source("app/macros.cljs");
    let edited = macros.replace("~label \": \"", "~label \" = \"");
    assert_ne!(edited, macros);
    let (lines, step) = watch.save("app/macros.cljs", &edited);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.macros, app.view in "),
        "{step}"
    );
    assert_eq!(watch.read("app.main.js"), main);
    assert_eq!(
        watch.reload(&["app.macros.js", "app.view.js"]),
        record("start\nclicks: [1]\nreloading\nclicks = [2]\n")
    );
}

/// A new require in a changed `ns` form loads the namespace through the
/// source provider: its unit is written and goes ahead of the requirer in
/// the manifest and index.js. A require of a file that does not exist yet
/// fails the step until the file appears. A `.cljc` file compiles its
/// `:cljs` branches, also when saved again. A deleted file is reported and
/// its namespace keeps its unit.
#[test]
fn a_new_require_adds_a_unit() {
    let mut watch = Watch::start("dev-watch-require");
    let view = watch.source("app/view.cljs");
    let edited = view
        .replace(
            "[app.state :as state]",
            "[app.state :as state]\n            [app.extra :as extra]",
        )
        .replace(
            "(js/console.log (describe card))",
            "(js/console.log (extra/shout (describe card)))",
        );
    let (lines, step) = watch.save("app/view.cljs", &edited);
    assert!(
        lines.len() == 1 && lines[0].contains(":1:1: missing namespace app.extra"),
        "{lines:#?}"
    );
    assert!(
        step.starts_with("light-cljs: recompiled nothing in ")
            && step.ends_with("; app.view failed and keeps its previous unit"),
        "{step}"
    );
    // The new file is a change that compiles the failed namespace again.
    let extra = "(ns app.extra)\n\n(defn shout [text] (str text #?(:cljs \"!\" :clj \"?\")))\n";
    let (lines, step) = watch.save("app/extra.cljc", extra);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.extra, app.view in "),
        "{step}"
    );
    let manifest = watch.read("manifest.json");
    assert!(
        manifest.contains(
            r#"    {"name": "app.macros", "file": "app.macros.js", "requires": [], "macros": true},
    {"name": "app.extra", "file": "app.extra.js", "requires": [], "macros": false},
    {"name": "app.view", "file": "app.view.js", "requires": ["app.macros", "app.state", "app.extra"], "macros": false},
    {"name": "app.main", "#
        ),
        "{manifest}"
    );
    assert!(
        watch.read("index.js").contains(
            r#"["runtime.js", "client.js", "app.state.js", "app.macros.js", "app.extra.js", "app.view.js", "app.main.js"]"#
        ),
        "{}",
        watch.read("index.js")
    );
    assert_eq!(
        watch.reload(&["app.extra.js", "app.view.js"]),
        record("start\nclicks: [1]\nreloading\nclicks: [2]!\n")
    );
    assert_eq!(
        watch.now_and_fresh(),
        [
            record("start\nclicks: [1]!\n"),
            record("start\nclicks: [1]!\n")
        ]
    );
    let (lines, step) = watch.save("app/extra.cljc", &extra.replace("\"!\"", "\"!!\""));
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.extra in "),
        "{step}"
    );
    assert_eq!(
        watch.now_and_fresh(),
        [
            record("start\nclicks: [1]!!\n"),
            record("start\nclicks: [1]!!\n")
        ]
    );
    fs::remove_file(watch.dir.join("src/app/extra.cljc")).unwrap();
    let (_, line) = watch.until("light-cljs: ");
    let path = fs::canonicalize(watch.dir.join("src/app")).unwrap();
    assert_eq!(
        line,
        format!(
            "light-cljs: {} was deleted; app.extra keeps its unit",
            path.join("extra.cljc").display()
        )
    );
    assert!(watch.dev("app.extra.js").exists());
}

/// A require that closes a cycle through namespaces the session has loaded
/// is refused, as `--compile` refuses it, so a macro namespace and the
/// namespace that uses it never recompile each other without end; saving
/// the file without the cycle compiles both.
#[test]
fn a_require_cycle_is_refused() {
    let mut watch = Watch::start("dev-watch-cycle");
    let unit = watch.read("app.macros.js");
    let macros = watch.source("app/macros.cljs");
    let cyclic = macros.replace(
        "(ns app.macros",
        "(ns app.macros\n  (:require [app.view]))\n(comment",
    );
    let (lines, step) = watch.save("app/macros.cljs", &cyclic);
    assert!(
        lines.len() == 1
            && lines[0].ends_with(
                "/app/macros.cljs: namespace dependency cycle: app.macros -> app.view -> app.macros"
            ),
        "{lines:#?}"
    );
    assert!(
        step.starts_with("light-cljs: recompiled nothing in ")
            && step.ends_with("; app.macros failed and keeps its previous unit"),
        "{step}"
    );
    assert_eq!(watch.read("app.macros.js"), unit);
    let edited = macros.replace("~label \": \"", "~label \" = \"");
    let (lines, step) = watch.save("app/macros.cljs", &edited);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.macros, app.view in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&["app.macros.js", "app.view.js"]),
        record("start\nclicks: [1]\nreloading\nclicks = [2]\n")
    );
}

/// A namespace whose last form is a directive has the value nil, as the
/// first build gives it, after a save too.
#[test]
fn a_trailing_directive_keeps_the_namespace_value_nil() {
    let mut watch = Watch::start("dev-watch-directive");
    let main = watch.source("app/main.cljs");
    let ended = format!("{main}(require 'app.state)\n");
    let (lines, step) = watch.save("app/main.cljs", &ended);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.main in "),
        "{step}"
    );
    let nil =
        r#"{"id":"program","outcome":{"status":"returned","value":null},"trace":[],"stdout":""}"#;
    assert_eq!(watch.now_and_fresh(), [nil, nil]);
    let (_, step) = watch.save("app/main.cljs", &main);
    assert!(
        step.starts_with("light-cljs: recompiled app.main in "),
        "{step}"
    );
    let started = record("start\nclicks: [1]\n");
    assert_eq!(watch.now_and_fresh(), [started.clone(), started]);
}

/// A rewritten unit's forms share its prelude, which binds the runtime to
/// `rt`, unless raw JavaScript renamed a form's names: then each form's unit
/// runs whole, and raw text that names a global `rt` still reads it.
#[test]
fn a_form_with_raw_javascript_runs_its_whole_unit() {
    let mut watch = Watch::start("dev-watch-raw");
    let view = watch.source("app/view.cljs");
    let (lines, _) = watch.save("app/view.cljs", &format!("{view}\n;; saved\n"));
    assert!(lines.is_empty(), "{lines:#?}");
    let shared = watch.read("app.view.js");
    let raw = format!(
        "{view}\n(js* \"globalThis.rt = \\\"host\\\"\")\n(js/console.log \"rt is\" (js* \"rt\"))\n"
    );
    let (lines, step) = watch.save("app/view.cljs", &raw);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    let unit = watch.read("app.view.js");
    for text in [&shared, &unit] {
        assert_eq!(text.matches("registerReaderNamespace(").count(), 1);
    }
    assert!(
        !shared.contains(")(__lightInteractiveRuntime);\n"),
        "{shared}"
    );
    assert_eq!(unit.matches(")(__lightInteractiveRuntime);\n").count(), 7);
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\nrt is host\nclicks: [2]\n")
    );
    let started = record("rt is host\nstart\nclicks: [1]\n");
    assert_eq!(watch.now_and_fresh(), [started.clone(), started]);
}

/// A rewritten unit registers its namespace once, with the aliases and
/// definitions its forms left, however many the step added; its forms'
/// units register nothing of their own.
#[test]
fn new_definitions_and_aliases_register_the_namespace_once() {
    let mut watch = Watch::start("dev-watch-register");
    let view = watch.source("app/view.cljs");
    let edited = format!(
        "{}\n(def extra-one (read-string \"::nowhere/k\"))\n(def extra-two 2)\n\
         (js/console.log (str extra-one) extra-two)\n",
        view.replace(
            "[app.state :as state]",
            "[app.state :as state]\n            [app.nowhere :as-alias nowhere]"
        )
    );
    let (lines, step) = watch.save("app/view.cljs", &edited);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    assert_eq!(
        watch
            .read("app.view.js")
            .matches("registerReaderNamespace(")
            .count(),
        1
    );
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\n:app.nowhere/k 2\nclicks: [2]\n")
    );
    let started = record(":app.nowhere/k 2\nstart\nclicks: [1]\n");
    assert_eq!(watch.now_and_fresh(), [started.clone(), started]);
}

/// The rewritten unit of the first namespace in load order registers every
/// namespace of the build, as the build's first unit does, so its forms
/// find the namespaces that load after it.
#[test]
fn the_first_namespace_rewritten_registers_every_namespace() {
    let mut watch = Watch::start("dev-watch-first");
    let state = watch.source("app/state.cljs");
    let edited = format!("{state}\n(js/console.log \"main\" (some? (find-ns 'app.main)))\n");
    let (lines, step) = watch.save("app/state.cljs", &edited);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.state in "),
        "{step}"
    );
    let unit = watch.read("app.state.js");
    for namespace in ["app.state", "app.macros", "app.view", "app.main"] {
        assert_eq!(
            unit.matches(&format!("registerReaderNamespace(\"{namespace}\""))
                .count(),
            1,
            "{namespace}"
        );
    }
    let started = record("main true\nstart\nclicks: [1]\n");
    assert_eq!(watch.now_and_fresh(), [started.clone(), started]);
}

/// A changed file is read from its first form in its own namespace, as
/// `--compile` reads it, whichever namespace the step before compiled: a
/// `::` keyword in its `ns` form names it.
#[test]
fn a_file_is_read_in_its_own_namespace() {
    let mut watch = Watch::start("dev-watch-reading");
    let macros = watch.source("app/macros.cljs");
    let (_, step) = watch.save(
        "app/macros.cljs",
        &format!("{macros}\n(defmacro tag [] (str (:tag (meta (:name (:ns &env))))))\n"),
    );
    assert!(
        step.starts_with("light-cljs: recompiled app.macros, app.view in "),
        "{step}"
    );
    let view = watch
        .source("app/view.cljs")
        .replace("(ns app.view", "(ns ^{:tag ::view} app.view")
        .replace(":refer [labeled]", ":refer [labeled tag]")
        .replace(
            "(js/console.log (describe card))",
            "(js/console.log (tag) (describe card))",
        );
    let expected = record("start\nclicks: [1]\nreloading\n:app.view/view clicks: [2]\n");
    let (_, step) = watch.save("app/view.cljs", &view);
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    assert_eq!(watch.reload(&["app.macros.js", "app.view.js"]), expected);
    // After a step in another namespace.
    let state = watch.source("app/state.cljs");
    let (_, step) = watch.save("app/state.cljs", &format!("{state}\n"));
    assert!(
        step.starts_with("light-cljs: recompiled app.state in "),
        "{step}"
    );
    let (_, step) = watch.save("app/view.cljs", &format!("{view}\n"));
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    assert_eq!(watch.reload(&["app.macros.js", "app.view.js"]), expected);
    let [now, fresh] = watch.now_and_fresh();
    assert_eq!(now, fresh);
}

/// A macro that inlines a `light.build` resource by a `./` path compiles
/// in the startup build, in the session the watch loads and in its steps,
/// each resolving the path against the expansion site's namespace, also
/// after a step in another namespace.
#[test]
fn a_macro_inlining_a_resource_compiles_at_startup_and_in_steps() {
    let mut watch = Watch::start_with("dev-watch-resource", |src| {
        fs::write(src.join("app/separator.txt"), ": ").unwrap();
        let macros = fs::read_to_string(src.join("app/macros.cljs")).unwrap();
        let macros = macros
            .replace(
                "\"A macro and the helper function its expansion calls.\")",
                "\"A macro and the helper function its expansion calls.\"\n  (:require [light.build :as b]))",
            )
            .replace("~label \": \"", "~label ~(b/resource \"./separator.txt\")");
        fs::write(src.join("app/macros.cljs"), macros).unwrap();
    });
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    assert_eq!(watch.reload(&[]), record("start\nclicks: [1]\n"));
    let state = watch.source("app/state.cljs");
    let (lines, step) = watch.save("app/state.cljs", &format!("{state}\n"));
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.state in "),
        "{step}"
    );
    let view = watch
        .source("app/view.cljs")
        .replace("(->Card \"clicks\"", "(->Card \"taps\"");
    let (lines, step) = watch.save("app/view.cljs", &view);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\ntaps: [2]\n")
    );
}

/// `--config` reaches `light.build/config` in the startup build and in the
/// steps the watch's session compiles.
#[test]
fn a_macro_reading_the_build_configuration_compiles_at_startup_and_in_steps() {
    let mut watch = Watch::start_with_args(
        "dev-watch-config",
        &["--config", "app/separator=\" = \""],
        |src| {
            let macros = fs::read_to_string(src.join("app/macros.cljs")).unwrap();
            let macros = macros
                .replace(
                    "\"A macro and the helper function its expansion calls.\")",
                    "\"A macro and the helper function its expansion calls.\"\n  (:require [light.build :as b]))",
                )
                .replace("~label \": \"", "~label ~(b/config :app/separator)");
            fs::write(src.join("app/macros.cljs"), macros).unwrap();
        },
    );
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    assert_eq!(watch.reload(&[]), record("start\nclicks = [1]\n"));
    let view = watch
        .source("app/view.cljs")
        .replace("(->Card \"clicks\"", "(->Card \"taps\"");
    let (lines, step) = watch.save("app/view.cljs", &view);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks = [1]\nreloading\ntaps = [2]\n")
    );
}

/// `app.macros` requiring the helper namespace `app.helper`, whose
/// `separator` the macro calls as it expands: `app.view`, which uses the
/// macro, depends on `app.helper` through a helper edge.
fn helper_example(src: &Path) {
    fs::write(
        src.join("app/helper.cljs"),
        "(ns app.helper)\n\n(defn separator [] \": \")\n",
    )
    .unwrap();
    let macros = fs::read_to_string(src.join("app/macros.cljs")).unwrap();
    let macros = macros
        .replace(
            "\"A macro and the helper function its expansion calls.\")",
            "\"A macro and the helper function its expansion calls.\"\n  (:require [app.helper :as helper]))",
        )
        .replace("~label \": \"", "~label ~(helper/separator)");
    fs::write(src.join("app/macros.cljs"), macros).unwrap();
}

/// An edit to a namespace that declares no macros, which a macro's
/// namespace requires, recompiles the namespaces that use the macro, through
/// the dependency manifest's helper edges, and not the macro's namespace,
/// whose unit holds no expansion.
#[test]
fn a_helper_edit_recompiles_the_macro_consumers() {
    let mut watch = Watch::start_with("dev-watch-helper", helper_example);
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    assert_eq!(watch.reload(&[]), record("start\nclicks: [1]\n"));
    let (lines, step) = watch.save(
        "app/helper.cljs",
        "(ns app.helper)\n\n(defn separator [] \" = \")\n",
    );
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.helper, app.view in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&["app.helper.js", "app.view.js"]),
        record("start\nclicks: [1]\nreloading\nclicks = [2]\n")
    );
    // The helper's next edit, after the session compiled the namespaces
    // again and revised the manifest with them, still reaches the consumers.
    let (lines, step) = watch.save(
        "app/helper.cljs",
        "(ns app.helper)\n\n(defn separator [] \" - \")\n",
    );
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.helper, app.view in "),
        "{step}"
    );
    let [now, fresh] = watch.now_and_fresh();
    assert_eq!(now, record("start\nclicks - [1]\n"));
    assert_eq!(now, fresh);
}

/// A resource a macro reads through `light.build` is watched where it was
/// looked for: it appearing, changing and going each recompile the
/// namespace whose expansion read it, and nothing else.
#[test]
fn a_resource_appearing_changing_or_going_recompiles_its_reader() {
    let mut watch = Watch::start_with("dev-watch-resource-steps", |src| {
        let macros = fs::read_to_string(src.join("app/macros.cljs")).unwrap();
        let macros = macros
            .replace(
                "\"A macro and the helper function its expansion calls.\")",
                "\"A macro and the helper function its expansion calls.\"\n  (:require [light.build :as b]))",
            )
            .replace(
                "~label \": \"",
                "~label ~(or (b/resource? \"./separator.txt\") \": \")",
            );
        fs::write(src.join("app/macros.cljs"), macros).unwrap();
    });
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    assert_eq!(watch.reload(&[]), record("start\nclicks: [1]\n"));
    // The startup build records the read on `app.view`, whose compile-time
    // units made it, and on its compilation consumer `app.main`, which could
    // reuse what they computed (docs/compiler-cache.md, "Conservative keys
    // and invalidation"); the session records it on the namespace whose
    // compile read it.
    for (separator, recompiled, stdout) in [
        (" = ", "app.view, app.main", "clicks = [2]"),
        (" - ", "app.view", "clicks - [2]"),
    ] {
        let (lines, step) = watch.save("app/separator.txt", separator);
        assert!(lines.is_empty(), "{lines:#?}");
        assert!(
            step.starts_with(&format!("light-cljs: recompiled {recompiled} in ")),
            "{step}"
        );
        assert_eq!(
            watch.reload(&["app.view.js", "app.main.js"]),
            record(&format!("start\nclicks: [1]\nreloading\n{stdout}\n"))
        );
    }
    fs::remove_file(watch.dir.join("src/app/separator.txt")).unwrap();
    let (lines, step) = watch.until("light-cljs: recompiled ");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    let [now, fresh] = watch.now_and_fresh();
    assert_eq!(now, record("start\nclicks: [1]\n"));
    assert_eq!(now, fresh);
    // A file no compile looked for starts no step.
    fs::write(watch.dir.join("src/app/unread.txt"), "x").unwrap();
    let state = watch.source("app/state.cljs");
    let (lines, step) = watch.save("app/state.cljs", &format!("{state}\n"));
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.state in "),
        "{step}"
    );
}

/// `app.macros` with a macro that counts its expansions in the session's
/// macro heap, and `app.config`, marked `^:dev/always`, printing the count
/// its unit was compiled with; `app.main` requires `app.config`.
fn always_example(src: &Path) {
    let path = src.join("app/macros.cljs");
    let macros = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        format!("{macros}\n(defonce expansions (atom 0))\n\n(defmacro counted [] (swap! expansions inc))\n"),
    )
    .unwrap();
    fs::write(
        src.join("app/config.cljs"),
        "(ns ^:dev/always app.config\n  (:require [app.macros :refer [counted]]))\n\n(js/console.log \"config\" (counted))\n",
    )
    .unwrap();
    let path = src.join("app/main.cljs");
    let main = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        main.replace(
            "[app.view :as view]))",
            "[app.view :as view]\n            [app.config]))",
        ),
    )
    .unwrap();
}

/// A namespace marked `^:dev/always` is compiled again, with its
/// compilation consumer `app.main`, by every step, whatever changed: its
/// macro expands again, in the session's macro heap, which keeps its state.
#[test]
fn an_always_namespace_recompiles_on_every_step() {
    let mut watch = Watch::start_with("dev-watch-always", always_example);
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    assert_eq!(watch.reload(&[]), record("config 1\nstart\nclicks: [1]\n"));
    for count in [2, 3] {
        let state = watch.source("app/state.cljs");
        let (lines, step) = watch.save("app/state.cljs", &format!("{state}\n"));
        assert!(lines.is_empty(), "{lines:#?}");
        assert!(
            step.starts_with("light-cljs: recompiled app.state, app.config, app.main in "),
            "{step}"
        );
        assert_eq!(
            watch.reload(&["app.state.js", "app.config.js", "app.main.js"]),
            record(&format!(
                "config 1\nstart\nclicks: [1]\nreloading\nconfig {count}\nclicks: [2]\n"
            ))
        );
    }
    // A file of no namespace of the build starts no step, so it compiles
    // nothing, `^:dev/always` included.
    fs::write(watch.dir.join("src/app/notes.txt"), "x").unwrap();
    fs::create_dir_all(watch.dir.join("src/other")).unwrap();
    fs::write(
        watch.dir.join("src/other/unused.cljs"),
        "(ns other.unused)\n",
    )
    .unwrap();
    let view = watch
        .source("app/view.cljs")
        .replace("(->Card \"clicks\"", "(->Card \"taps\"");
    let (lines, step) = watch.save("app/view.cljs", &view);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view, app.config, app.main in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&["app.view.js", "app.config.js", "app.main.js"]),
        record("config 1\nstart\nclicks: [1]\nreloading\nconfig 4\ntaps: [2]\n")
    );
}

/// A macro namespace marked `^:dev/always` is an always-compile provider:
/// every step compiles it again with the namespaces that use its macros and
/// their consumers.
#[test]
fn an_always_provider_recompiles_its_consumers_on_every_step() {
    let mut watch = Watch::start_with("dev-watch-always-provider", |src| {
        let path = src.join("app/macros.cljs");
        let macros = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            macros.replacen("(ns app.macros", "(ns ^:dev/always app.macros", 1),
        )
        .unwrap();
    });
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    let state = watch.source("app/state.cljs");
    let (lines, step) = watch.save("app/state.cljs", &format!("{state}\n"));
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.state, app.macros, app.view, app.main in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&[
            "app.state.js",
            "app.macros.js",
            "app.view.js",
            "app.main.js"
        ]),
        record("start\nclicks: [1]\nreloading\nclicks: [2]\n")
    );
    // The step that removes the mark still compiles the set it began with;
    // the next compiles only what changed.
    let macros = watch.source("app/macros.cljs").replacen(
        "(ns ^:dev/always app.macros",
        "(ns app.macros",
        1,
    );
    let (lines, step) = watch.save("app/macros.cljs", &macros);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.macros, app.view, app.main in "),
        "{step}"
    );
    let (lines, step) = watch.save("app/state.cljs", &format!("{state}\n\n"));
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.state in "),
        "{step}"
    );
}

/// A disk bypass leaves in-memory units valid: a namespace marked
/// `^:light-cljs/no-cache`, or a `--cache-blocker` and its compilation
/// consumers, is not compiled again by a step that changed nothing it
/// depends on, while an edit to it still reaches its macro's users.
#[test]
fn a_cache_bypass_forces_no_recompilation() {
    for (label, args, mark) in [
        ("dev-watch-no-cache", &[][..], true),
        (
            "dev-watch-blocker",
            &["--cache-blocker", "app.macros"][..],
            false,
        ),
    ] {
        let mut watch = Watch::start_with_args(label, args, |src| {
            if mark {
                let path = src.join("app/macros.cljs");
                let macros = fs::read_to_string(&path).unwrap();
                fs::write(
                    &path,
                    macros.replacen("(ns app.macros", "(ns ^:light-cljs/no-cache app.macros", 1),
                )
                .unwrap();
            }
        });
        assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
        let state = watch.source("app/state.cljs");
        let (lines, step) = watch.save("app/state.cljs", &format!("{state}\n"));
        assert!(lines.is_empty(), "{lines:#?}");
        assert!(
            step.starts_with("light-cljs: recompiled app.state in "),
            "{label}: {step}"
        );
        let macros = watch.source("app/macros.cljs");
        let (lines, step) = watch.save(
            "app/macros.cljs",
            &macros.replace("~label \": \"", "~label \" = \""),
        );
        assert!(lines.is_empty(), "{lines:#?}");
        assert!(
            step.starts_with("light-cljs: recompiled app.macros, app.view in "),
            "{label}: {step}"
        );
    }
}

/// A namespace that a save marks `^:dev/always` is compiled again with its
/// compilation consumers in that same step, as is one that a changed `ns`
/// form loads.
#[test]
fn a_new_always_namespace_recompiles_its_consumers_in_the_same_step() {
    let mut watch = Watch::start_with("dev-watch-new-always", |src| {
        fs::write(src.join("app/config.cljs"), "(ns app.config)\n").unwrap();
        let path = src.join("app/main.cljs");
        let main = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            main.replace(
                "[app.view :as view]))",
                "[app.view :as view]\n            [app.config]))",
            ),
        )
        .unwrap();
    });
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    let (lines, step) = watch.save("app/config.cljs", "(ns ^:dev/always app.config)\n");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.config, app.main in "),
        "{step}"
    );
    // A new require of a `^:dev/always` namespace from `app.view`: the
    // namespace and its consumers `app.view` and `app.main` compile, with
    // `app.config` and its consumer, as every step compiles them now.
    fs::write(
        watch.dir.join("src/app/extra.cljs"),
        "(ns ^:dev/always app.extra)\n",
    )
    .unwrap();
    let view = watch.source("app/view.cljs").replace(
        "[app.state :as state]))",
        "[app.state :as state]\n            [app.extra]))",
    );
    let (lines, step) = watch.save("app/view.cljs", &view);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.extra, app.view, app.config, app.main in "),
        "{step}"
    );
}

/// A resource read by a helper's compile-time initializer, whose value a
/// macro reuses: each change compiles the helper again, so its value is read
/// again, and the namespaces that use the macro, also after the session has
/// compiled them once.
#[test]
fn a_retained_helper_resource_is_read_again_after_each_change() {
    let mut watch = Watch::start_with("dev-watch-helper-resource", |src| {
        fs::write(src.join("app/separator.txt"), ": ").unwrap();
        fs::write(
            src.join("app/helper.cljs"),
            "(ns app.helper (:require [light.build :as b]))\n\n;; The application loads this namespace too, where light.build is an error.\n(def separator (when (exists? js/lightCljsBuild) (b/resource \"app/separator.txt\")))\n",
        )
        .unwrap();
        let macros = fs::read_to_string(src.join("app/macros.cljs")).unwrap();
        let macros = macros
            .replace(
                "\"A macro and the helper function its expansion calls.\")",
                "\"A macro and the helper function its expansion calls.\"\n  (:require [app.helper :as helper]))",
            )
            .replace("~label \": \"", "~label ~helper/separator");
        fs::write(src.join("app/macros.cljs"), macros).unwrap();
    });
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    assert_eq!(watch.reload(&[]), record("start\nclicks: [1]\n"));
    for separator in [" = ", " - ", " + "] {
        let (lines, step) = watch.save("app/separator.txt", separator);
        assert!(lines.is_empty(), "{lines:#?}");
        assert!(
            step.contains("app.helper") && step.contains("app.view"),
            "{step}"
        );
        let [now, fresh] = watch.now_and_fresh();
        assert_eq!(
            now,
            record(&format!("start\nclicks{separator}[1]\n")),
            "{step}"
        );
        assert_eq!(now, fresh);
    }
}

/// A source root's own file for a namespace the compiler also bundles wins,
/// in the session as in the first build, and is watched.
#[test]
fn a_source_root_library_overrides_the_bundled_one() {
    let mut watch = Watch::start_with("dev-watch-library", |src| {
        fs::create_dir_all(src.join("clojure")).unwrap();
        fs::write(
            src.join("clojure/string.cljs"),
            "(ns clojure.string)\n\n(defn shout [text] (str text \"!\"))\n",
        )
        .unwrap();
        let view = fs::read_to_string(src.join("app/view.cljs")).unwrap();
        let view = view
            .replace(
                "[app.state :as state]",
                "[app.state :as state]\n            [clojure.string :refer [shout]]",
            )
            .replace(
                "(js/console.log (describe card))",
                "(js/console.log (shout (describe card)))",
            );
        fs::write(src.join("app/view.cljs"), view).unwrap();
    });
    let shouted = record("start\nclicks: [1]!\n");
    assert_eq!(watch.now_and_fresh(), [shouted.clone(), shouted]);
    let (lines, step) = watch.save(
        "clojure/string.cljs",
        "(ns clojure.string)\n\n(defn shout [text] (str text \"!!\"))\n",
    );
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled clojure.string in "),
        "{step}"
    );
    let shouted = record("start\nclicks: [1]!!\n");
    assert_eq!(watch.now_and_fresh(), [shouted.clone(), shouted]);
}

/// A file of the build that cannot be written ends the watch with the error.
#[test]
fn a_unit_that_cannot_be_written_ends_the_watch() {
    let mut watch = Watch::start("dev-watch-write");
    fs::create_dir(watch.dev(".app.view.js.tmp")).unwrap();
    let path = watch.dir.join("src/app/view.cljs");
    let view = watch.source("app/view.cljs");
    fs::write(&path, format!("{view}\n")).unwrap();
    let (lines, status) = watch.exit();
    assert!(!status.success());
    assert!(
        lines.len() == 1
            && lines[0].starts_with("light-cljs: cannot write ")
            && lines[0].contains("app.view.js"),
        "{lines:#?}"
    );
}

/// A source that is a symbolic link is watched through its target, and a
/// save that replaces the link with a file is seen as its namespace's.
#[cfg(unix)]
#[test]
fn a_replaced_symbolic_link_is_still_its_namespace_source() {
    let mut watch = Watch::start_with("dev-watch-link", |src| {
        let target = src.parent().unwrap().join("elsewhere.cljs");
        fs::rename(src.join("app/view.cljs"), &target).unwrap();
        std::os::unix::fs::symlink(&target, src.join("app/view.cljs")).unwrap();
    });
    let view = fs::read_to_string(watch.dir.join("elsewhere.cljs")).unwrap();
    // The link's target changes.
    let linked = view.replace(
        "(js/console.log (describe card))",
        "(js/console.log \"linked:\" (describe card))",
    );
    fs::write(watch.dir.join("elsewhere.cljs"), &linked).unwrap();
    let (_, step) = watch.until("light-cljs: recompiled ");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\nlinked: clicks: [2]\n")
    );
    // A save renames a file over the link.
    let saved = view.replace(
        "(js/console.log (describe card))",
        "(js/console.log \"saved:\" (describe card))",
    );
    let (_, step) = watch.save("app/view.cljs", &saved);
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\nsaved: clicks: [2]\n")
    );
}

/// `--watch` needs an entry, a source root and an out dir, and exits with
/// the error of a first build that fails.
#[test]
fn stats_report_the_startup_build_and_the_session_load() {
    let mut watch = Watch::start_with_args("dev-watch-stats", &["--stats"], |_| {});
    let start = watch
        .startup
        .iter()
        .position(|line| line == "light-cljs-stats 1")
        .unwrap_or_else(|| panic!("no report in {:#?}", watch.startup));
    let report = &watch.startup[start..];
    let value = |key: &str| {
        report
            .iter()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(' '))
            .unwrap_or_else(|| panic!("{key} in {report:#?}"))
    };
    assert!(value("time.session-load-ms").parse::<f64>().unwrap() > 0.0);
    assert!(value("time.analysis-ms").parse::<f64>().unwrap() > 0.0);
    assert_eq!(value("namespaces"), "4");
    // examples/dev's macro is one of the calls.
    assert!(value("macro-calls").parse::<usize>().unwrap() > 0);
    let table = report
        .iter()
        .position(|line| line == "[macro-calls-by-namespace]")
        .unwrap();
    assert_eq!(report.len() - table - 1, 4, "{report:#?}");
    // A step prints no report.
    let view = watch.source("app/view.cljs");
    let (lines, _) = watch.save("app/view.cljs", &format!("{view}\n"));
    assert!(
        !lines
            .iter()
            .any(|line| line.starts_with("light-cljs-stats"))
    );
}

/// `app.view` expanding 1,201 macro calls, over the default limit of 1,024
/// expansions per compilation.
fn many_expansions(src: &Path) {
    let macros = fs::read_to_string(src.join("app/macros.cljs")).unwrap();
    fs::write(
        src.join("app/macros.cljs"),
        format!(
            "{macros}\n(defmacro one [] 1)\n\n(defmacro many [n] `(+ ~@(repeat n (list 'app.macros/one))))\n"
        ),
    )
    .unwrap();
    let view = fs::read_to_string(src.join("app/view.cljs"))
        .unwrap()
        .replace(":refer [labeled]", ":refer [labeled many]");
    fs::write(
        src.join("app/view.cljs"),
        format!("{view}\n(def total (many 1200))\n"),
    )
    .unwrap();
}

/// The macro limit flags reach the startup build and the session: a
/// program over the default expansion limit stops the watch without them,
/// starts with `--macro-expansions`, cold and from the persistent cache,
/// and its steps compile under the same limit.
#[test]
fn the_macro_limit_flags_raise_the_startup_build_and_the_steps_limits() {
    let dir = TempDir::new("dev-watch-limits");
    fs::create_dir_all(dir.join("src/app")).unwrap();
    for name in ["state", "macros", "view", "main"] {
        let file = format!("app/{name}.cljs");
        fs::copy(Path::new(EXAMPLE).join(&file), dir.join("src").join(&file)).unwrap();
    }
    many_expansions(&dir.join("src"));
    let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .args([
            "--watch",
            "--no-cache",
            "--entry",
            "app.main",
            "--source-root",
        ])
        .arg(dir.join("src"))
        .arg("--out-dir")
        .arg(dir.join("dev"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() && stderr.contains("compiler macro expansion limit exceeded"),
        "{stderr}"
    );
    let limit = ["--macro-expansions", "5000"];
    let mut watch = Watch::start_with_args("dev-watch-limits", &limit, many_expansions);
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    assert_eq!(watch.reload(&[]), record("start\nclicks: [1]\n"));
    let view = watch
        .source("app/view.cljs")
        .replace("(->Card \"clicks\"", "(->Card \"taps\"");
    let (lines, step) = watch.save("app/view.cljs", &view);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    assert_eq!(
        watch.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\ntaps: [2]\n")
    );
    // From the persistent cache: the first start fills it, the second
    // installs app.view, and a step compiles it again.
    let cache = watch.dir.join("cache");
    let cache = cache.to_str().unwrap().to_owned();
    let cached = [limit[0], limit[1], "--cache-dir", &cache];
    watch.restart(&cached);
    watch.restart(&cached);
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    let (lines, step) = watch.save("app/view.cljs", &format!("{view}\n"));
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
}

/// A macro whose expansion demands a helper of a namespace that extends a
/// protocol to a type a JavaScript package exports, as cognitect.transit
/// does in Penpot: the watch's macros see stub packages, as with
/// `--compile`, in the startup build and in the steps, cold and from the
/// persistent cache.
#[test]
fn macros_see_stub_packages_at_startup_and_in_steps() {
    let setup = |src: &Path| {
        fs::write(
            src.join("app/pkg.cljs"),
            "(ns app.pkg (:require [\"left-pad\" :as lp]))\n\n\
             (def Pad (.-constructor (lp/pad 1)))\n\n\
             (extend-type Pad IEquiv (-equiv [_ _] false))\n\n\
             (defn shout [text] (if (= text \"\") \"!\" text))\n",
        )
        .unwrap();
        let macros = fs::read_to_string(src.join("app/macros.cljs"))
            .unwrap()
            .replace(
                "\"A macro and the helper function its expansion calls.\")",
                "\"A macro and the helper function its expansion calls.\"\n  (:require [app.pkg :as pkg]))",
            )
            .replace("~label \": \"", "~(pkg/shout \"\") ~label \": \"");
        fs::write(src.join("app/macros.cljs"), macros).unwrap();
    };
    let mut watch = Watch::start_with("dev-watch-packages", setup);
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    let view = watch
        .source("app/view.cljs")
        .replace("(->Card \"clicks\"", "(->Card \"taps\"");
    let (lines, step) = watch.save("app/view.cljs", &view);
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
    let cache = watch.dir.join("cache");
    let cache = cache.to_str().unwrap().to_owned();
    watch.restart(&["--cache-dir", &cache]);
    watch.restart(&["--cache-dir", &cache]);
    assert!(watch.startup.is_empty(), "{:#?}", watch.startup);
    let (lines, step) = watch.save("app/view.cljs", &format!("{view}\n"));
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("light-cljs: recompiled app.view in "),
        "{step}"
    );
}

/// Each of the other macro limit flags reaches the watch's startup build,
/// in V8 and, with `--cache-dir`, in the session's host: a value too small
/// for examples/dev stops the watch with that limit's error.
#[test]
fn each_macro_limit_flag_bounds_the_startup_build() {
    let dir = TempDir::new("dev-watch-each-limit");
    let out = dir.join("dev");
    let cache = dir.join("cache");
    for (flag, value, message) in [
        (
            "--macro-nodes",
            "10",
            "compiler macro generated-node limit exceeded",
        ),
        (
            "--macro-timeout-ms",
            "1",
            "JavaScript session deadline exceeded",
        ),
        (
            "--macro-memory-limit",
            "100000",
            "JavaScript memory limit must be at least",
        ),
        (
            "--macro-poll-budget",
            "10",
            "JavaScript session poll budget exhausted",
        ),
    ] {
        for cached in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
            command
                .args(["--watch", "--entry", "app.main", "--source-root", EXAMPLE])
                .arg("--out-dir")
                .arg(&out)
                .args([flag, value]);
            if cached {
                command.arg("--cache-dir").arg(&cache);
            } else {
                command.arg("--no-cache");
            }
            let output = command.stdin(Stdio::null()).output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !output.status.success() && stderr.contains(message),
                "{flag} {value} (cache {cached}): {stderr}"
            );
        }
    }
}

#[test]
fn watch_refuses_missing_options_and_a_failing_first_build() {
    let dir = TempDir::new("dev-watch-options");
    let out = dir.join("dev");
    let out = out.to_str().unwrap();
    let cache = dir.join("cache");
    let cache = cache.to_str().unwrap();
    for (args, message) in [
        (
            &["--source-root", EXAMPLE, "--out-dir", out][..],
            "--watch requires --entry NAMESPACE",
        ),
        (
            &["--entry", "app.main", "--out-dir", out][..],
            "--watch requires --source-root DIR",
        ),
        (
            &["--entry", "app.main", "--source-root", EXAMPLE][..],
            "--watch requires --out-dir DIR",
        ),
        (
            &[
                "--entry",
                "app.main",
                "--source-root",
                EXAMPLE,
                "--out-dir",
                out,
                "--poll-ms",
                "0",
            ][..],
            "--poll-ms requires a positive integer",
        ),
        (
            &[
                "--entry",
                "app.main",
                "--source-root",
                EXAMPLE,
                "--out-dir",
                out,
                "--esm",
            ][..],
            "unexpected watch option: --esm",
        ),
        (
            &[
                "--entry",
                "app.main",
                "--source-root",
                EXAMPLE,
                "--out-dir",
                out,
                "--reload-dependents",
            ][..],
            "--reload-dependents requires --serve PORT",
        ),
        (
            &[
                "--entry",
                "app.main",
                "--source-root",
                EXAMPLE,
                "--out-dir",
                out,
                "--no-cache",
                "--cache-dir",
                cache,
            ][..],
            "--no-cache and --cache-dir exclude each other",
        ),
        (
            &[
                "--cache-dir",
                cache,
                "--entry",
                "app.main",
                "--source-root",
                EXAMPLE,
                "--out-dir",
                out,
                "--no-cache",
            ][..],
            "--no-cache and --cache-dir exclude each other",
        ),
        (
            &[
                "--entry",
                "app.missing",
                "--source-root",
                EXAMPLE,
                "--out-dir",
                out,
            ][..],
            "app.missing",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .arg("--watch")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success()
                && stderr.starts_with("light-cljs: ")
                && stderr.contains(message),
            "{args:?}: {stderr}"
        );
    }
    assert!(!dir.join("dev/manifest.json").exists());
    assert!(!dir.join("cache").exists());
}

/// The value of `key` in the `--stats` report among `lines`.
fn stat<'a>(lines: &'a [String], key: &str) -> &'a str {
    lines
        .iter()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(' '))
        .unwrap_or_else(|| panic!("{key} in {lines:#?}"))
}

/// The `[macro-calls-by-namespace]` rows of the `--stats` report among
/// `lines`, as `NAMESPACE COUNT`.
fn macro_calls(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .skip_while(|line| *line != "[macro-calls-by-namespace]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .map(String::as_str)
        .collect()
}

/// The names a step line `light-cljs: recompiled NAMES in X ms` lists.
fn recompiled(line: &str) -> &str {
    line.strip_prefix("light-cljs: recompiled ")
        .and_then(|rest| rest.rsplit_once(" in "))
        .map(|(names, _)| names)
        .unwrap_or_else(|| panic!("no step in {line:?}"))
}

/// The same sources watched twice with `args`: `cold` without a cache, as
/// every watch started before the persistent cache, and `warm` restarted
/// with `--cache-dir` over the artifacts its first run published, so that
/// its session starts from installed namespaces
/// (`DevSession::compile_entry`).
struct Pair {
    cold: Watch,
    warm: Watch,
}

impl Pair {
    fn start(label: &str, args: &[&str], setup: impl Fn(&Path)) -> Self {
        // Labels of one length, so that both builds quote paths alike.
        let cold = Watch::start_with_args(&format!("{label}-c"), args, &setup);
        let mut warm = Watch::start_with_args(&format!("{label}-w"), args, &setup);
        let cache = warm.dir.join("cache");
        let cache = cache.to_str().unwrap().to_owned();
        let mut cached = args.to_vec();
        cached.extend(["--cache-dir", &cache]);
        warm.restart(&cached);
        cached.push("--stats");
        warm.restart(&cached);
        // Both start from byte-identical dev builds.
        assert_eq!(cold.files(), warm.files());
        Self { cold, warm }
    }

    /// Restart both with `args`, `warm` with the cache, and check that their
    /// builds are byte-identical again.
    fn restart(&mut self, args: &[&str]) {
        self.cold.restart(args);
        let cache = self.warm.dir.join("cache");
        let cache = cache.to_str().unwrap().to_owned();
        let mut cached = args.to_vec();
        cached.extend(["--cache-dir", &cache, "--stats"]);
        self.warm.restart(&cached);
        assert_eq!(self.cold.files(), self.warm.files());
    }

    /// Save `text` as `file` in both, and return what the step recompiled,
    /// after checking that both recompiled the same namespaces into the
    /// same files, byte for byte, and printed the same lines.
    fn save(&mut self, file: &str, text: &str) -> String {
        let [cold, warm] = self.save_each(file, text);
        assert_eq!(cold, warm);
        cold
    }

    /// Save `text` as `file` in both, check that both printed the same
    /// lines and, when they recompiled the same namespaces, wrote the same
    /// files, and return what each recompiled.
    fn save_each(&mut self, file: &str, text: &str) -> [String; 2] {
        let (cold_lines, cold) = self.cold.save(file, text);
        let (warm_lines, warm) = self.warm.save(file, text);
        assert_eq!(cold_lines, warm_lines);
        if recompiled(&cold) == recompiled(&warm) {
            assert_eq!(self.cold.files(), self.warm.files());
        }
        [recompiled(&cold).to_owned(), recompiled(&warm).to_owned()]
    }

    /// The driver's record of each first build given the units `files` of
    /// its build now, the same for both.
    fn reload(&self, files: &[&str]) -> String {
        let record = self.cold.reload(files);
        assert_eq!(record, self.warm.reload(files));
        record
    }
}

impl Watch {
    /// The files of the dev build now, by name.
    fn files(&self) -> std::collections::BTreeMap<String, String> {
        fs::read_dir(self.dir.join("dev"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    fs::read_to_string(entry.path()).unwrap(),
                )
            })
            .collect()
    }
}

/// A watch restarted with `--cache-dir` installs every namespace its last
/// run published, running none of their macros, and its session compiles
/// the first edits as a session that loaded the program itself: an edit
/// that expands a cached namespace's macro, whose expansion demands a
/// helper of another cached namespace; a redefinition in a cached
/// namespace; and edits to the cached macro and helper namespaces, whose
/// consumers the manifest of installed namespaces still reaches. Every unit
/// is byte-identical to a cold-started watch's, and the driver runs both
/// builds alike.
#[test]
fn a_warm_started_session_compiles_first_edits_as_a_cold_one() {
    let mut pair = Pair::start("dev-watch-warm", &[], helper_example);
    let report = &pair.warm.startup;
    assert_eq!(stat(report, "cache.hits"), "5", "{report:#?}");
    assert_eq!(stat(report, "cache.misses"), "0");
    assert_eq!(stat(report, "macro-calls"), "0");
    assert_eq!(
        macro_calls(report),
        [
            "app.state 0",
            "app.helper 0",
            "app.macros 0",
            "app.view 0",
            "app.main 0"
        ]
    );
    assert_eq!(pair.reload(&[]), record("start\nclicks: [1]\n"));
    // `labeled` of the installed `app.macros` expands, calling
    // `app.helper/separator` at compile time in the session's engine.
    let view = pair.warm.source("app/view.cljs").replace(
        "(js/console.log (describe card))",
        "(js/console.log (labeled \"render\" (describe card)))",
    );
    assert_eq!(pair.save("app/view.cljs", &view), "app.view");
    assert_eq!(
        pair.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\nrender: [clicks: [2]]\n")
    );
    // Steps publish nothing: a restart analyzes the edited namespace and
    // the one whose key covers its key, and installs the others.
    pair.restart(&[]);
    let report = &pair.warm.startup;
    assert_eq!(stat(report, "cache.hits"), "3", "{report:#?}");
    assert_eq!(stat(report, "cache.miss.source"), "1");
    assert_eq!(stat(report, "cache.miss.dependencies"), "1");
    let calls = macro_calls(report);
    assert!(
        ["app.state 0", "app.helper 0", "app.macros 0"]
            .iter()
            .all(|row| calls.contains(row)),
        "{calls:#?}"
    );
    assert_eq!(pair.reload(&[]), record("start\nrender: [clicks: [1]]\n"));
    // A redefinition of an installed Var replaces it for its callers.
    let state = pair
        .warm
        .source("app/state.cljs")
        .replace("update :clicks inc", "update :clicks + 10");
    assert_eq!(pair.save("app/state.cljs", &state), "app.state");
    assert_eq!(
        pair.reload(&["app.state.js", "app.view.js"]),
        record("start\nrender: [clicks: [1]]\nreloading\nrender: [clicks: [11]]\n")
    );
    // The installed macro namespace itself: its consumer compiles again.
    let macros = pair
        .warm
        .source("app/macros.cljs")
        .replace("(str \"[\" text \"]\")", "(str \"<\" text \">\")");
    assert_eq!(
        pair.save("app/macros.cljs", &macros),
        "app.macros, app.view"
    );
    // And the installed helper the macro calls.
    assert_eq!(
        pair.save(
            "app/helper.cljs",
            "(ns app.helper)\n\n(defn separator [] \" = \")\n"
        ),
        "app.helper, app.view"
    );
    assert_eq!(
        pair.reload(&[
            "app.state.js",
            "app.helper.js",
            "app.macros.js",
            "app.view.js"
        ]),
        record("start\nrender: [clicks: [1]]\nreloading\nrender = <clicks = <11>>\n")
    );
    let [now, fresh] = pair.warm.now_and_fresh();
    assert_eq!(now, fresh);
    assert_eq!(now, pair.cold.now_and_fresh()[0]);
}

/// A warm-started watch keeps the always-compile and blocker rules: a
/// `^:dev/always` namespace and its consumers bypass the cache at startup
/// and every step compiles them again, with the macro state of an installed
/// macro namespace, which its compile-time units build in the session's
/// engine on demand, counting from the startup build as a cold session's
/// does; a `--cache-blocker` and its consumers bypass the cache at startup,
/// and a step recompiles them only when what they depend on changed.
#[test]
fn a_warm_started_session_keeps_the_always_and_blocker_rules() {
    let mut pair = Pair::start("dev-watch-warm-always", &[], always_example);
    let report = &pair.warm.startup;
    assert_eq!(stat(report, "cache.hits"), "3", "{report:#?}");
    assert_eq!(stat(report, "cache.bypass.always"), "2");
    assert_eq!(
        macro_calls(report),
        [
            "app.main 6",
            "app.config 1",
            "app.state 0",
            "app.macros 0",
            "app.view 0"
        ]
    );
    assert_eq!(pair.reload(&[]), record("config 1\nstart\nclicks: [1]\n"));
    for count in [2, 3] {
        let state = pair.warm.source("app/state.cljs");
        assert_eq!(
            pair.save("app/state.cljs", &format!("{state}\n")),
            "app.state, app.config, app.main"
        );
        assert_eq!(
            pair.reload(&["app.state.js", "app.config.js", "app.main.js"]),
            record(&format!(
                "config 1\nstart\nclicks: [1]\nreloading\nconfig {count}\nclicks: [2]\n"
            ))
        );
    }

    let mut pair = Pair::start(
        "dev-watch-warm-block",
        &["--cache-blocker", "app.macros"],
        |_| {},
    );
    let report = &pair.warm.startup;
    assert_eq!(stat(report, "cache.hits"), "1", "{report:#?}");
    assert_eq!(stat(report, "cache.bypass.blocked"), "3");
    let state = pair.warm.source("app/state.cljs");
    assert_eq!(
        pair.save("app/state.cljs", &format!("{state}\n")),
        "app.state"
    );
    let macros = pair
        .warm
        .source("app/macros.cljs")
        .replace("~label \": \"", "~label \" = \"");
    assert_eq!(
        pair.save("app/macros.cljs", &macros),
        "app.macros, app.view"
    );
    assert_eq!(
        pair.reload(&["app.state.js", "app.macros.js", "app.view.js"]),
        record("start\nclicks: [1]\nreloading\nclicks = [2]\n")
    );
}

/// A program whose entry reaches a namespace only through a lazy reference,
/// which the build loads after the entry: a warm-started session retains
/// none of the build's bookkeeping for it, so an edit of the entry writes
/// the unit a cold session writes.
#[test]
fn a_warm_started_session_with_a_lazy_target_writes_cold_units() {
    let mut pair = Pair::start("dev-watch-warm-lazy", &[], |src| {
        fs::write(
            src.join("app/extra.cljs"),
            "(ns app.extra)\n\n(defn start [] :extra)\n",
        )
        .unwrap();
        let path = src.join("app/main.cljs");
        let main = fs::read_to_string(&path).unwrap().replace(
            "[app.view :as view]))",
            "[app.view :as view]\n            [light.lazy :as lazy]))\n\n(def extra (lazy/loadable app.extra/start))",
        );
        fs::write(&path, main).unwrap();
    });
    // `light.lazy` is one of them.
    assert_eq!(stat(&pair.warm.startup, "cache.hits"), "6");
    let main = pair.warm.source("app/main.cljs");
    assert_eq!(
        pair.save(
            "app/main.cljs",
            &main.replacen(
                "(state/click!)",
                "(js/console.log \"again\")\n  (state/click!)",
                1
            )
        ),
        "app.main"
    );
    assert_eq!(
        pair.reload(&["app.main.js"]),
        record("start\nclicks: [1]\nreloading\nagain\nclicks: [2]\n")
    );
}

/// The helper-resource fixture of
/// `a_retained_helper_resource_is_read_again_after_each_change`, started
/// warm, and warm with `app.view` blocked, so that the helper whose
/// initializer read the resource is installed while its consumer is
/// analyzed. A helper value the session computed holds the resource, so a
/// change compiles the helper again with its consumers, as in a cold
/// session; after a warm start with every namespace installed the session
/// has computed none, and the first change compiles the namespaces whose
/// keys cover the resource, whose expansion computes the value afresh.
#[test]
fn a_warm_started_session_reads_a_helper_resource_again() {
    let setup = |src: &Path| {
        fs::write(src.join("app/separator.txt"), ": ").unwrap();
        fs::write(
            src.join("app/helper.cljs"),
            "(ns app.helper (:require [light.build :as b]))\n\n(def separator (when (exists? js/lightCljsBuild) (b/resource \"app/separator.txt\")))\n",
        )
        .unwrap();
        let macros = fs::read_to_string(src.join("app/macros.cljs")).unwrap();
        let macros = macros
            .replace(
                "\"A macro and the helper function its expansion calls.\")",
                "\"A macro and the helper function its expansion calls.\"\n  (:require [app.helper :as helper]))",
            )
            .replace("~label \": \"", "~label ~helper/separator");
        fs::write(src.join("app/macros.cljs"), macros).unwrap();
    };
    for (label, args) in [
        ("dev-watch-warm-resource", &[][..]),
        (
            "dev-watch-warm-resblock",
            &["--cache-blocker", "app.view"][..],
        ),
    ] {
        let mut pair = Pair::start(label, args, setup);
        for (index, separator) in [" = ", " - ", " + "].into_iter().enumerate() {
            let [cold, warm] = pair.save_each("app/separator.txt", separator);
            assert_eq!(cold, "app.helper, app.macros, app.view, app.main");
            if index == 0 && args.is_empty() {
                assert_eq!(warm, "app.view, app.main");
            } else {
                assert_eq!(warm, cold, "{label}");
            }
            let [now, fresh] = pair.warm.now_and_fresh();
            assert_eq!(now, record(&format!("start\nclicks{separator}[1]\n")));
            assert_eq!(now, fresh);
        }
    }
}

/// A namespace whose source is a symbolic link to a file outside the
/// source roots is watched after a warm start too: editing the target, and
/// saving a file over the link, each recompile it as in a cold session.
#[test]
fn a_warm_started_session_watches_a_linked_source() {
    let mut pair = Pair::start("dev-watch-warm-link", &[], |src| {
        let target = src.parent().unwrap().join("elsewhere.cljs");
        fs::rename(src.join("app/view.cljs"), &target).unwrap();
        std::os::unix::fs::symlink(&target, src.join("app/view.cljs")).unwrap();
    });
    let view = fs::read_to_string(pair.warm.dir.join("elsewhere.cljs")).unwrap();
    let linked = view.replace(
        "(js/console.log (describe card))",
        "(js/console.log \"linked:\" (describe card))",
    );
    for watch in [&mut pair.cold, &mut pair.warm] {
        fs::write(watch.dir.join("elsewhere.cljs"), &linked).unwrap();
        let (_, step) = watch.until("light-cljs: recompiled ");
        assert_eq!(recompiled(&step), "app.view");
    }
    assert_eq!(pair.cold.files(), pair.warm.files());
    assert_eq!(
        pair.reload(&["app.view.js"]),
        record("start\nclicks: [1]\nreloading\nlinked: clicks: [2]\n")
    );
    let saved = view.replace(
        "(js/console.log (describe card))",
        "(js/console.log \"saved:\" (describe card))",
    );
    assert_eq!(pair.save("app/view.cljs", &saved), "app.view");
}

/// `app.text`, which `app.view` requires, requiring Closure Library
/// namespaces that no source root provides, as upstream code does:
/// `goog.string` with an alias and a renamed refer, `goog.string.format`
/// bare (as `instaparse.abnf` requires it), and `goog.object` in a
/// top-level `require`.
fn goog_example(src: &Path) {
    fs::write(
        src.join("app/text.cljs"),
        "(ns app.text\n  (:require [goog.string :as gstr :refer [trim] :rename {trim strim}]\n            [goog.string.format]))\n\n(require '[goog.object :as gobj :refer [get]])\n\n(defn pad [n] (gstr/format \"%05d\" n))\n\n(defn clean [s] (strim (goog.string/trim s)))\n\n(defn field [o k] (if (get o k) (gobj/get o k) k))\n",
    )
    .unwrap();
    let view = fs::read_to_string(src.join("app/view.cljs")).unwrap();
    let view = view.replace(
        "[app.state :as state]",
        "[app.state :as state]\n            [app.text :as text]",
    );
    fs::write(src.join("app/view.cljs"), view).unwrap();
}

/// A session that loaded the program itself and one that installed it from
/// the cache write the same unit and map, byte for byte, for a step whose
/// forms generate names: `or`'s `gensym`, and a syntax-quote gensym and a
/// `gensym` of a macro. The startup build allocates each namespace's names
/// in its own scope (light-cljs-332), so it leaves the session's
/// process-wide counters and its runtime's `gensym` counter where a session
/// that ran none of its macros has them.
#[test]
fn a_step_generates_the_same_names_cold_and_warm() {
    let mut pair = Pair::start("dev-watch-names", &[], |src| {
        fs::write(
            src.join("app/logic.cljs"),
            "(ns app.logic)\n\n(defmacro both [a b] `(let [x# ~a] (and x# ~b)))\n\n(defmacro tagged [] (list 'quote (gensym \"tag\")))\n\n(defn pick [a b] (or a (both b a)))\n\n(def tag (tagged))\n",
        )
        .unwrap();
        let view = fs::read_to_string(src.join("app/view.cljs")).unwrap();
        let view = view.replace(
            "[app.state :as state]",
            "[app.state :as state]\n            [app.logic :as logic]",
        );
        fs::write(src.join("app/view.cljs"), view).unwrap();
    });
    let text = pair.cold.source("app/logic.cljs");
    let text = format!("{text}\n(defn more [a b] (or a (both a b) (tagged)))\n");
    assert_eq!(pair.save("app/logic.cljs", &text), "app.logic, app.view");
    let unit = pair.cold.read("app.logic.js");
    assert!(unit.contains("tag"), "{unit}");
}

/// A `goog.*` namespace that no source root provides is a JavaScript host
/// namespace in the session as in the startup build: neither loads it, and
/// its alias and refers name host paths. A session that loaded the program
/// itself and one that installed it from the cache, replaying the `ns`
/// forms, both start, and a step that compiles the namespace again writes
/// the startup build's unit and map, byte for byte.
#[test]
fn an_unprovided_goog_require_is_a_host_namespace_in_the_session() {
    let mut pair = Pair::start("dev-watch-goog", &[], goog_example);
    let report = &pair.warm.startup;
    assert_eq!(stat(report, "cache.hits"), "5", "{report:#?}");
    // The host paths a unit reads, in order.
    let hosts = |unit: &str| {
        unit.split("host(\"goog\")")
            .skip(1)
            .map(|rest| {
                let end = rest
                    .find(|c: char| !(c.is_alphanumeric() || "[]\"".contains(c)))
                    .unwrap_or(rest.len());
                format!("goog{}", &rest[..end])
            })
            .collect::<Vec<_>>()
    };
    let built = hosts(&pair.cold.read("app.text.js"));
    assert_eq!(
        built,
        [
            "goog[\"string\"][\"format\"]",
            "goog[\"string\"][\"trim\"]",
            "goog[\"string\"][\"trim\"]",
            "goog[\"object\"][\"get\"]",
            "goog[\"object\"][\"get\"]",
        ]
    );
    // Both sessions compile the namespace again into the same unit, which
    // reads the host paths the startup build's reads.
    let text = pair.cold.source("app/text.cljs");
    assert_eq!(pair.save("app/text.cljs", &text), "app.text");
    assert_eq!(hosts(&pair.cold.read("app.text.js")), built);
    assert_eq!(
        pair.reload(&["app.text.js"]),
        record("start\nclicks: [1]\nreloading\nclicks: [2]\n")
    );
}

/// The cache is on by default (light-cljs-328): a watch that names no cache
/// starts from `.light-cljs/cache` under the current directory, filling it
/// on its first start and installing every namespace on the next, while
/// `--no-cache` starts uncached and writes nothing there.
#[test]
fn a_watch_starts_from_the_default_cache_directory() {
    let dir = TempDir::new("dev-watch-default-cache");
    fs::create_dir_all(dir.join("src/app")).unwrap();
    for name in ["state", "macros", "view", "main"] {
        let file = format!("app/{name}.cljs");
        fs::copy(Path::new(EXAMPLE).join(&file), dir.join("src").join(&file)).unwrap();
    }
    let start = |extra: &[&str]| -> Vec<String> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .current_dir(&*dir)
            .args(["--watch", "--entry", "app.main", "--source-root", "src"])
            .args(["--out-dir", "dev", "--poll-ms", "20", "--stats"])
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = Vec::new();
        for line in BufReader::new(child.stderr.take().unwrap()).lines() {
            let line = line.unwrap();
            let built = line.starts_with("light-cljs: built ");
            lines.push(line);
            if built {
                break;
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            lines
                .last()
                .is_some_and(|line| line.starts_with("light-cljs: built ")),
            "{lines:#?}"
        );
        lines
    };
    let cache = dir.join(".light-cljs/cache");
    let uncached = start(&["--no-cache"]);
    assert!(!uncached.iter().any(|line| line.starts_with("cache.")));
    assert!(!cache.exists());
    let filling = start(&[]);
    assert_eq!(stat(&filling, "cache.published"), "4", "{filling:#?}");
    assert!(cache.join("v1/objects").is_dir());
    let warm = start(&[]);
    assert_eq!(stat(&warm, "cache.hits"), "4", "{warm:#?}");
    assert_eq!(stat(&warm, "macro-calls"), "0");
}

/// Steps that each run a changed macro again in the watch's session, after a
/// start from the persistent cache that installed every namespace, hold no
/// artifact and the same engines and engine handles after each step, and
/// bridge payloads that stop growing (`compile_stats::live_counts`, the line
/// `--stats` adds to each step after a full collection, which also gives
/// `DevSession::value_roots`), and the same values rooted in the session
/// engine's registry after each step, since a redefinition releases the
/// replaced macro's function (light-cljs-381). The payloads grow
/// by those of the macro environments each step imports until the bridge's
/// bounded cache of environments (64 slots and 128 child slots) is full,
/// after about 33 steps here, so the last ten of 45 steps are flat. The
/// startup report, made once the start has dropped the artifacts it
/// installed, counts nothing live.
#[test]
fn repeated_steps_hold_bounded_engine_state_and_no_artifact() {
    let mut watch = Watch::start("dev-watch-live");
    let cache = watch.dir.join("cache");
    let cache = cache.to_str().unwrap().to_owned();
    let args = ["--stats", "--cache-dir", &cache];
    watch.restart(&args);
    watch.restart(&args);
    let report = watch.startup.join("\n");
    assert!(report.contains("\ncache.hits 4\n"), "{report}");
    // No macro ran, so the session has not started its macro engine.
    assert!(
        report.contains(
            "\nmemory.live.artifacts 0\n\
             memory.live.engines 0\n\
             memory.live.engine-handles 0\n\
             memory.live.bridge-payloads 0\n"
        ),
        "{report}"
    );
    let macros = watch.source("app/macros.cljs");
    // Per step: artifacts, engines, engine handles, bridge payloads and the
    // values the session engine's registry roots.
    let mut counts = Vec::new();
    for step in 1..=45 {
        let edited = macros.replace("~label \": \"", &format!("~label \" {step}: \""));
        assert_ne!(edited, macros);
        let (lines, summary) = watch.save("app/macros.cljs", &edited);
        assert!(lines.is_empty(), "{lines:#?}");
        assert!(
            summary.starts_with("light-cljs: recompiled app.macros, app.view in "),
            "{summary}"
        );
        let (lines, live) = watch.until("light-cljs: live ");
        assert!(lines.is_empty(), "{lines:#?}");
        let values = [
            "artifacts",
            "engines",
            "engine-handles",
            "bridge-payloads",
            "value-roots",
        ]
        .map(|key| {
            live.split(' ')
                .find_map(|field| field.strip_prefix(key)?.strip_prefix('='))
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or_else(|| panic!("no {key} in {live}"))
        });
        counts.push(values);
    }
    let [_, engines, handles, _, _] = counts[0];
    assert!(engines > 0, "{counts:?}");
    for (step, &[artifacts, ..]) in counts.iter().enumerate() {
        assert_eq!(artifacts, 0, "step {}: {counts:?}", step + 1);
    }
    assert!(
        counts.iter().all(|c| c[1] == engines && c[2] == handles),
        "{counts:?}"
    );
    // A redefinition releases the function of the macro it replaces, so
    // the session engine roots as many values after every step.
    assert!(counts.iter().all(|c| c[4] == counts[0][4]), "{counts:?}");
    let last = &counts[counts.len() - 10..];
    assert!(last.iter().all(|c| c[3] == last[0][3]), "{counts:?}");
}

/// `light-cljs watch features` (docs/build-configuration.md, "Commands"),
/// from the example's `app/` subdirectory, with `:http {:port 0}` and
/// `:fs-watch {:loop-wait 50}` from `LIGHT_CLJS`: it builds `out/dev` in the
/// project directory, serves it on the port it bound under `/features/` and
/// at `/`, holds that port in `.light-cljs/http.port`, steps on a save, and
/// removes the port file when SIGINT ends it. Nothing is written below the
/// working directory.
#[cfg(unix)]
#[test]
fn watch_features_builds_serves_and_steps_from_light_cljs_edn() {
    let (_dir, project) = file_watch::features("dev-watch-file");
    let app = project.join("app");
    let sources = fs::read_dir(&app).unwrap().count();
    let mut watch = file_watch::Light::spawn(
        &app,
        &["watch", "features"],
        Some("{:http {:port 0} :fs-watch {:loop-wait 50}}"),
    );
    let (address, before, serving) = watch.serving();
    let out = project.join("out/dev");
    assert_eq!(
        before,
        [format!(
            "light-cljs - config: {}",
            project.join("light-cljs.edn").display()
        )]
    );
    assert_eq!(
        serving,
        format!("light-cljs: serving {} at http://{address}/", out.display())
    );
    let port = address.strip_prefix("127.0.0.1:").unwrap();
    assert_ne!(port, "0");
    let port_file = project.join(".light-cljs/http.port");
    assert_eq!(fs::read_to_string(&port_file).unwrap(), port);
    let (lines, built) = watch.until("[:features] built ");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        built.starts_with("[:features] built app.main (")
            && built.contains(&format!(" namespaces) into {} in ", out.display()))
            && built.ends_with(" ms; watching every 50 ms"),
        "{built}"
    );

    let index = fs::read_to_string(out.join("index.js")).unwrap();
    let unit = fs::read_to_string(out.join("app.shared.js")).unwrap();
    for (path, body) in [
        ("/index.js", &index),
        ("/features/index.js", &index),
        ("/app.shared.js", &unit),
        ("/features/app.shared.js", &unit),
    ] {
        assert_eq!(
            file_watch::get(&address, path),
            ("HTTP/1.1 200 OK".to_owned(), body.clone()),
            "{path}"
        );
    }
    assert!(file_watch::replay(&address, "/features/events").ends_with(",\"dependents\":false}"));

    let shared = app.join("shared.cljs");
    let text = fs::read_to_string(&shared).unwrap();
    let saving = app.join(".saving");
    fs::write(
        &saving,
        format!("{text}\n(def saved-by-test \"saved by the test\")\n"),
    )
    .unwrap();
    fs::rename(&saving, &shared).unwrap();
    let (lines, step) = watch.until("[:features] recompiled ");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("[:features] recompiled app.shared in "),
        "{step}"
    );
    assert!(
        fs::read_to_string(out.join("app.shared.js"))
            .unwrap()
            .contains("saved by the test")
    );

    assert_eq!(watch.interrupt().code(), Some(130));
    assert!(!port_file.exists());
    assert!(project.join(".light-cljs/cache").is_dir());
    assert_eq!(fs::read_dir(&app).unwrap().count(), sources);
}

/// The watch's flags after the id write over the file's `:http`, `:fs-watch`
/// and `:devtools`, as `compile`'s write over its settings: `--serve 0` over
/// a port the test holds with `:strict true`, and `--poll-ms 30` over
/// `:loop-wait 50`, while `:devtools {:reload-strategy :full}` reaches the
/// watch as `--reload-dependents` (the build a runtime catches up with
/// reloads dependents).
#[test]
fn watch_flags_write_over_the_files_watch_settings() {
    let (_dir, project) = file_watch::features("dev-watch-file-flags");
    let (_held, held) = file_watch::held_port();
    let merged = format!(
        "{{:http {{:port {held} :strict true}} :fs-watch {{:loop-wait 50}} \
         :builds {{:features {{:devtools {{:reload-strategy :full}}}}}}}}"
    );
    let mut watch = file_watch::Light::spawn(
        &project,
        &["watch", "features", "--serve", "0", "--poll-ms", "30"],
        Some(&merged),
    );
    let (address, _, _) = watch.serving();
    let port = address.strip_prefix("127.0.0.1:").unwrap();
    assert_ne!(port, held.to_string());
    assert_eq!(
        fs::read_to_string(project.join(".light-cljs/http.port")).unwrap(),
        port
    );
    let (_, built) = watch.until("[:features] built ");
    assert!(built.ends_with(" ms; watching every 30 ms"), "{built}");
    assert!(file_watch::replay(&address, "/events").ends_with(",\"dependents\":true}"));
}

/// `watch app` compiles what its equivalent `--watch` flags compile, at
/// startup and after a save: the commands fixture's `:config` and top-level
/// `:cache-blockers`, and from `LIGHT_CLJS` `:elide-asserts`, a macro limit
/// and a build blocker, with `--config app/size=2` after the id over the
/// file's value. The flag-only watch, run second against the same store,
/// writes the same files before and after the save, bypasses the same
/// namespaces and misses no artifact.
#[test]
fn watch_id_compiles_what_its_equivalent_watch_flags_compile() {
    let (_dir, project) = file_watch::project(
        "dev-watch-file-equivalent",
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/build-configuration/commands"),
    );
    let main = project.join("src/app/main.cljs");
    let text = fs::read_to_string(&main).unwrap();
    let out = project.join("out/app");
    // The file's build names its lines `[:app] `, and the flags' says
    // `light-cljs: `.
    let run = |args: &[&str], merged: Option<&str>, lead: &str| {
        let _ = fs::remove_dir_all(project.join("out"));
        fs::write(&main, &text).unwrap();
        let mut watch = file_watch::Light::spawn(&project, args, merged);
        let (startup, _) = watch.until(&format!("{lead}built "));
        let started = file_watch::tree(&out);
        let saving = project.join("src/app/.saving");
        fs::write(&saving, format!("{text}\n(def saved \"saved\")\n")).unwrap();
        fs::rename(&saving, &main).unwrap();
        let (_, step) = watch.until(&format!("{lead}recompiled "));
        let step = step.split(" in ").next().unwrap()[lead.len()..].to_owned();
        let saved = file_watch::tree(&out);
        let bypasses = startup
            .iter()
            .skip_while(|line| *line != "[cache-misses-and-bypasses]")
            .filter(|line| line.contains(" bypass "))
            .cloned()
            .collect::<Vec<_>>();
        let misses = startup
            .iter()
            .find(|line| line.starts_with("cache.misses "))
            .cloned();
        (started, step, saved, bypasses, misses, startup)
    };
    let file = run(
        &["watch", "app", "--stats", "--config", "app/size=2"],
        Some(
            "{:http {:port 0} :builds {:app {:compiler-options {:elide-asserts true \
             :macro-timeout-ms 20000} :build-options {:cache-blockers #{app.util}}}}}",
        ),
        "[:app] ",
    );
    let src = project.join("src");
    let store = project.join(".light-cljs/cache");
    let flags = run(
        &[
            "--watch",
            "--entry",
            "app.main",
            "--source-root",
            src.to_str().unwrap(),
            "--out-dir",
            out.to_str().unwrap(),
            "--cache-dir",
            store.to_str().unwrap(),
            "--config",
            "app/flavor=:file",
            "--config",
            "app/size=2",
            "--elide-asserts",
            "--macro-timeout-ms",
            "20000",
            "--cache-blocker",
            "app.macros",
            "--cache-blocker",
            "app.util",
            "--stats",
            "--serve",
            "0",
        ],
        None,
        "light-cljs: ",
    );
    assert!(
        file.0
            .iter()
            .any(|(path, bytes)| path == Path::new("app.main.js")
                && String::from_utf8_lossy(bytes).contains("\":file\"")
                && String::from_utf8_lossy(bytes).contains("\"2\"")),
        "{:#?}",
        file.5
    );
    assert_eq!(file.0, flags.0, "the startup builds differ");
    assert_eq!(file.1, flags.1);
    assert_eq!(file.2, flags.2, "the builds after the save differ");
    assert_ne!(file.0, file.2);
    assert_eq!(file.3, flags.3);
    assert_eq!(file.3.len(), 3, "{:#?}", file.5);
    assert_eq!(flags.4.as_deref(), Some("cache.misses 0"), "{:#?}", flags.5);
}

/// A `watch app` of a temporary copy of the commands fixture, whose file
/// also gives `:http {:port 0}` and `:fs-watch {:loop-wait 20}`, once it has
/// built: the directory, the project, the watch and the server's address.
fn rereading(label: &str, flags: &[&str]) -> (TempDir, PathBuf, file_watch::Light, String) {
    let (dir, project) = file_watch::project(
        label,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/build-configuration/commands"),
    );
    let config = project.join("light-cljs.edn");
    let text = fs::read_to_string(&config).unwrap().replace(
        "{:source-paths [\"src\"]",
        "{:source-paths [\"src\"]\n :http {:port 0}\n :fs-watch {:loop-wait 20}",
    );
    fs::write(&config, text).unwrap();
    let args = [&["watch", "app"][..], flags].concat();
    let mut watch = file_watch::Light::spawn(&project, &args, None);
    let (address, _, _) = watch.serving();
    let (lines, _) = watch.until("[:app] built ");
    assert!(lines.is_empty(), "{lines:#?}");
    (dir, project, watch, address)
}

/// Edit light-cljs.edn of `project`, replacing `from` with `to`.
fn edit_config(project: &Path, from: &str, to: &str) {
    let path = project.join("light-cljs.edn");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{from:?} is not in {text}");
    file_watch::save(&path, &text.replacen(from, to, 1));
}

/// Wait for many polls of the watch, then save a definition into
/// app/main.cljs: the step's line, and every line the watch printed since the
/// last one read.
fn step_after_polls(
    project: &Path,
    watch: &mut file_watch::Light,
    def: &str,
) -> (Vec<String>, String) {
    std::thread::sleep(Duration::from_millis(400));
    let main = project.join("src/app/main.cljs");
    let text = fs::read_to_string(&main).unwrap();
    file_watch::save(&main, &format!("{text}\n(def {def} \"{def}\")\n"));
    watch.until("[:app] recompiled ")
}

/// An edit of another build's section, or of a comment alone, changes
/// nothing of the running build's settings: the watch does nothing, prints
/// nothing, and its next step compiles as before (docs/build-configuration.md,
/// "Watching the configuration file").
#[test]
fn a_reread_that_changes_nothing_of_the_build_does_nothing() {
    let (_dir, project, mut watch, _) = rereading("dev-watch-reread-other", &[]);
    edit_config(&project, "\"out/other\"", "\"out/other-edited\"");
    edit_config(
        &project,
        ";; release commands run, with flags over them.",
        ";; release commands run, with flags over them, edited.",
    );
    let (lines, step) = step_after_polls(&project, &mut watch, "first");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(step.starts_with("[:app] recompiled app.main in "), "{step}");
    let unit = fs::read_to_string(project.join("out/app/app.main.js")).unwrap();
    assert!(
        unit.contains("\":file\"") && unit.contains("first"),
        "{unit}"
    );
}

/// An invalid edit is reported, located, on stderr and to every connected
/// runtime as an error event, and the build keeps running with its settings:
/// the next save steps with the `:config` it started with.
#[test]
fn an_invalid_reread_is_reported_and_the_build_keeps_its_settings() {
    let (_dir, project, mut watch, address) = rereading("dev-watch-reread-invalid", &[]);
    let mut events = file_watch::Events::connect(&address, "/events");
    assert!(events.next().unwrap().starts_with("{\"op\":\"hello\","));
    assert!(events.next().unwrap().starts_with("{\"op\":\"load\","));
    edit_config(
        &project,
        ":config {:app/flavor :file :app/size 1}}",
        ":config {:app/flavor :edited :app/size 1}\n        :bogus 1}",
    );
    let config = project.join("light-cljs.edn");
    let (lines, error) = watch.until("light-cljs: ");
    assert!(lines.is_empty(), "{lines:#?}");
    let message = "unknown key :bogus in build :app; the keys here are :build-options, \
                   :closure-defines, :compiler-options, :config, :dev, :devtools, :modules, \
                   :output-dir, :release, :target";
    assert_eq!(
        error,
        format!("light-cljs: {}:12:9: {message}", config.display())
    );
    assert_eq!(
        events.next().unwrap(),
        format!(
            "{{\"op\":\"error\",\"step\":1,\"error\":{{\"message\":\"{message}\",\"file\":\"{}\",\"line\":12,\"column\":9}}}}",
            config.display()
        )
    );
    let (lines, step) = step_after_polls(&project, &mut watch, "after-invalid");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(step.starts_with("[:app] recompiled app.main in "), "{step}");
    let unit = fs::read_to_string(project.join("out/app/app.main.js")).unwrap();
    assert!(
        unit.contains("\":file\"") && !unit.contains("edited") && unit.contains("after-invalid"),
        "{unit}"
    );
    let load = events.next().unwrap();
    assert!(
        load.starts_with("{\"op\":\"load\",\"step\":2,\"replay\":false,"),
        "{load}"
    );
}

/// A changed `:config` restarts the build in the same process and on the
/// same server: the startup build and the next step see the new value
/// through `light.build/config`, and a connected runtime's stream ends, so
/// that it connects again and catches up with the new build.
#[test]
fn a_changed_config_restarts_the_build() {
    let (_dir, project, mut watch, address) = rereading("dev-watch-reread-config", &[]);
    let port_file = project.join(".light-cljs/http.port");
    let port = fs::read_to_string(&port_file).unwrap();
    let mut events = file_watch::Events::connect(&address, "/events");
    assert!(events.next().unwrap().starts_with("{\"op\":\"hello\","));
    assert!(events.next().unwrap().contains("\\\":file\\\""));
    edit_config(&project, ":app/flavor :file", ":app/flavor :edited");
    let (lines, built) = watch.until("[:app] built ");
    assert_eq!(
        lines,
        ["light-cljs: light-cljs.edn changed the settings of :app; restarting the build"]
    );
    assert!(
        built.starts_with("[:app] built app.main (")
            && built.ends_with(" ms; watching every 20 ms"),
        "{built}"
    );
    let unit = fs::read_to_string(project.join("out/app/app.main.js")).unwrap();
    assert!(unit.contains("\":edited\""), "{unit}");
    // The runtime's stream ended; connecting again catches up with the
    // restarted build, on the same port.
    assert_eq!(events.next(), None);
    let replay = file_watch::replay(&address, "/events");
    assert!(replay.contains("\\\":edited\\\""), "{replay}");
    assert_eq!(fs::read_to_string(&port_file).unwrap(), port);

    let (lines, step) = step_after_polls(&project, &mut watch, "after-restart");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(step.starts_with("[:app] recompiled app.main in "), "{step}");
    let unit = fs::read_to_string(project.join("out/app/app.main.js")).unwrap();
    assert!(
        unit.contains("\":edited\"") && unit.contains("after-restart"),
        "{unit}"
    );
    assert!(watch.child.try_wait().unwrap().is_none());
}

/// The flags after the id still win after a reread: an edit of the
/// `:config` entry `--config` gives changes nothing the build runs with, and
/// restarts nothing; an edit of another entry restarts the build, with the
/// flag's value over the file's still.
#[test]
fn flags_still_win_after_a_reread() {
    // A store the file cannot name, `--config` over one `:config` entry and
    // `--serve` over `:http`.
    let (_dir, project, mut watch, _) = rereading(
        "dev-watch-reread-flags",
        &[
            "--cache-dir",
            "state/store",
            "--config",
            "app/flavor=:flag",
            "--serve",
            "0",
        ],
    );
    edit_config(&project, ":app/flavor :file", ":app/flavor :edited");
    edit_config(&project, ":http {:port 0}", ":http {:port 9}");
    std::thread::sleep(Duration::from_millis(400));
    edit_config(&project, ":app/size 1", ":app/size 3");
    let (lines, _) = watch.until("[:app] built ");
    assert_eq!(
        lines,
        ["light-cljs: light-cljs.edn changed the settings of :app; restarting the build"]
    );
    let unit = fs::read_to_string(project.join("out/app/app.main.js")).unwrap();
    assert!(
        unit.contains("\":flag\"") && unit.contains("\"3\"") && !unit.contains("edited"),
        "{unit}"
    );
    assert!(project.join("state/store").is_dir());
}

/// A reread that moves `:output-dir` and the top-level `:source-paths`
/// restarts the build into the new directory from the new roots, and the
/// server, on its port, serves the new directory and the new roots' sources
/// (where the units' source maps name them), while a save under the new
/// roots steps.
#[test]
fn a_reread_moves_the_output_and_the_source_roots() {
    let (_dir, project, mut watch, address) = rereading("dev-watch-reread-moved", &[]);
    fs::create_dir_all(project.join("lib/app")).unwrap();
    for entry in fs::read_dir(project.join("src/app")).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(
            &path,
            project.join("lib/app").join(path.file_name().unwrap()),
        )
        .unwrap();
    }
    let config = project.join("light-cljs.edn");
    let text = fs::read_to_string(&config).unwrap();
    file_watch::save(
        &config,
        &text
            .replacen("[\"src\"]", "[\"lib\"]", 1)
            .replacen("\"out/app\"", "\"out/moved\"", 1),
    );
    let (lines, built) = watch.until("[:app] built ");
    assert_eq!(
        lines,
        ["light-cljs: light-cljs.edn changed the settings of :app; restarting the build"]
    );
    let moved = project.join("out/moved");
    assert!(
        built.contains(&format!(" into {} in ", moved.display())),
        "{built}"
    );
    let unit = fs::read_to_string(moved.join("app.main.js")).unwrap();
    for path in ["/app.main.js", "/app/app.main.js"] {
        assert_eq!(
            file_watch::get(&address, path),
            ("HTTP/1.1 200 OK".to_owned(), unit.clone()),
            "{path}"
        );
    }
    let map = fs::read_to_string(moved.join("app.main.js.map")).unwrap();
    assert!(map.contains("\"../../lib/app/main.cljs\""), "{map}");
    // The old directory still holds the old map, which names ../src.
    assert_eq!(
        file_watch::get(&address, "/app.main.js.map"),
        ("HTTP/1.1 200 OK".to_owned(), map.clone())
    );
    let main = project.join("lib/app/main.cljs");
    let source = fs::read_to_string(&main).unwrap();
    assert_eq!(
        file_watch::get(&address, "/lib/app/main.cljs"),
        ("HTTP/1.1 200 OK".to_owned(), source.clone())
    );
    file_watch::save(&main, &format!("{source}\n(def moved \"moved\")\n"));
    let (lines, step) = watch.until("[:app] recompiled ");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(step.starts_with("[:app] recompiled app.main in "), "{step}");
    assert!(
        fs::read_to_string(moved.join("app.main.js"))
            .unwrap()
            .contains("\"moved\"")
    );
}

/// A reread that changes a top-level `:cache-blockers`, `:compile-threads` or
/// `:cache-root` restarts the build; the new `:cache-root` holds the store
/// the restarted build writes, and `http.port` stays where the watch wrote it.
#[test]
fn top_level_settings_restart_the_build() {
    let (_dir, project, mut watch, _) = rereading("dev-watch-reread-top", &[]);
    for (from, to) in [
        (
            ":cache-blockers #{app.macros}",
            ":cache-blockers #{app.macros app.util}",
        ),
        (
            ":fs-watch {:loop-wait 20}",
            ":fs-watch {:loop-wait 20}\n :compile-threads 2",
        ),
        (
            ":fs-watch {:loop-wait 20}",
            ":fs-watch {:loop-wait 20}\n :cache-root \".other\"",
        ),
    ] {
        edit_config(&project, from, to);
        let (lines, _) = watch.until("[:app] built ");
        assert_eq!(
            lines,
            ["light-cljs: light-cljs.edn changed the settings of :app; restarting the build"],
            "{to}"
        );
    }
    assert!(project.join(".other/cache").is_dir());
    assert!(project.join(".light-cljs/http.port").is_file());
}

/// `:http`, `:nrepl` and `:fs-watch` edits are each reported once, and a
/// later restart of the build applies none of them: the server keeps its
/// port and answers nREPL, and the poll interval stays the one the watch
/// started with.
#[test]
fn held_settings_stay_across_a_restart() {
    let (_dir, project, mut watch, address) = rereading("dev-watch-reread-held", &[]);
    let port_file = project.join(".light-cljs/http.port");
    let port = fs::read_to_string(&port_file).unwrap();
    for (from, to, key) in [
        (
            ":fs-watch {:loop-wait 20}",
            ":fs-watch {:loop-wait 20}\n :nrepl false",
            "nrepl",
        ),
        (":loop-wait 20", ":loop-wait 50", "fs-watch"),
        (":http {:port 0}", ":http {:port 9}", "http"),
    ] {
        edit_config(&project, from, to);
        let (lines, line) = watch.until("light-cljs: ");
        assert!(lines.is_empty(), "{lines:#?}");
        assert_eq!(
            line,
            format!("light-cljs: :{key} changed in light-cljs.edn; restart the watch to apply it")
        );
    }
    edit_config(&project, ":app/flavor :file", ":app/flavor :edited");
    let (lines, built) = watch.until("[:app] built ");
    assert_eq!(
        lines,
        ["light-cljs: light-cljs.edn changed the settings of :app; restarting the build"]
    );
    assert!(built.ends_with(" ms; watching every 20 ms"), "{built}");
    assert_eq!(fs::read_to_string(&port_file).unwrap(), port);
    assert!(!file_watch::nrepl_describe(&address).is_empty());
    let (status, _) = file_watch::get(&address, "/app.main.js");
    assert_eq!(status, "HTTP/1.1 200 OK");
}

/// A restart whose build fails keeps the watch, and the server, running: the
/// error is reported, and the next change starts the build again.
#[test]
fn a_restart_that_fails_starts_again_at_the_next_change() {
    let (_dir, project, mut watch, address) = rereading("dev-watch-reread-failed", &[]);
    let mut events = file_watch::Events::connect(&address, "/events");
    assert!(events.next().unwrap().starts_with("{\"op\":\"hello\","));
    assert!(events.next().unwrap().starts_with("{\"op\":\"load\","));
    edit_config(&project, "[app.main]", "[app.missing]");
    let (lines, failed) = watch.until("light-cljs: ");
    assert!(lines.is_empty(), "{lines:#?}");
    assert_eq!(
        failed,
        "light-cljs: light-cljs.edn changed the settings of :app; restarting the build"
    );
    let (lines, failed) = watch.until("[:app] ");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        failed.contains("app.missing")
            && failed.ends_with("; the build starts again at the next change"),
        "{failed}"
    );
    let error = events.next().unwrap();
    assert!(
        error.starts_with("{\"op\":\"error\",\"step\":1,"),
        "{error}"
    );
    assert!(watch.child.try_wait().unwrap().is_none());
    edit_config(&project, "[app.missing]", "[app.main]");
    let (lines, built) = watch.until("[:app] built ");
    assert_eq!(
        lines,
        ["light-cljs: light-cljs.edn changed the settings of :app; restarting the build"]
    );
    assert!(built.starts_with("[:app] built app.main ("), "{built}");
    assert_eq!(events.next(), None);
    let (lines, step) = step_after_polls(&project, &mut watch, "after-failure");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(step.starts_with("[:app] recompiled app.main in "), "{step}");
}

/// An edit of `:http` is reported once, with the exact message, and changes
/// nothing: the server keeps its port, a comment edit after it reports
/// nothing again, and the next step compiles as before.
#[test]
fn an_http_edit_is_reported_once_and_keeps_the_server() {
    let (_dir, project, mut watch, address) = rereading("dev-watch-reread-http", &[]);
    let port_file = project.join(".light-cljs/http.port");
    let port = fs::read_to_string(&port_file).unwrap();
    edit_config(&project, ":http {:port 0}", ":http {:port 9}");
    let (lines, line) = watch.until("light-cljs: ");
    assert!(lines.is_empty(), "{lines:#?}");
    assert_eq!(
        line,
        "light-cljs: :http changed in light-cljs.edn; restart the watch to apply it"
    );
    edit_config(
        &project,
        ";; release commands run, with flags over them.",
        ";; release commands run, with flags over them, edited.",
    );
    let (lines, step) = step_after_polls(&project, &mut watch, "after-http");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(step.starts_with("[:app] recompiled app.main in "), "{step}");
    assert_eq!(fs::read_to_string(&port_file).unwrap(), port);
    let (status, body) = file_watch::get(&address, "/app.main.js");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(body.contains("after-http"), "{body}");
}

/// The Phase 2 exit criterion: in a copy of examples/features with a
/// `:config` key and a namespace that reads it, `light-cljs watch features`
/// serves the example, and an edit of the `:config` value in light-cljs.edn
/// reaches a connected runtime without restarting the process: the same
/// process and port, the runtime connected again and running the new value.
#[test]
fn watch_features_carries_a_config_edit_to_a_connected_runtime() {
    let (_dir, project) = file_watch::features("dev-watch-reread-features");
    let config = project.join("light-cljs.edn");
    let text = fs::read_to_string(&config).unwrap();
    let text = text
        .replace(
            "{:source-paths [\".\"]",
            "{:source-paths [\".\"]\n :http {:port 0}\n :fs-watch {:loop-wait 20}",
        )
        .replace(
            "   :dev {:output-dir \"out/dev\"}}}}",
            "   :dev {:output-dir \"out/dev\"}\n   :config {:app/greeting \"hello\"}}}}",
        );
    fs::write(&config, &text).unwrap();
    fs::write(
        project.join("app/greeting.cljs"),
        "(ns app.greeting (:require [light.build :as b]))\n\n\
         (defmacro greeting [] (b/config :app/greeting))\n",
    )
    .unwrap();
    let main = project.join("app/main.cljs");
    let source = fs::read_to_string(&main).unwrap();
    fs::write(
        &main,
        source
            .replace(
                "[light.lazy :as lazy]))",
                "[light.lazy :as lazy]\n            [app.greeting :refer [greeting]]))",
            )
            .replace(
                "(js/console.log \"menu:\"",
                "(js/console.log \"greeting:\" (greeting))\n\n(js/console.log \"menu:\"",
            ),
    )
    .unwrap();

    let mut watch = file_watch::Light::spawn(&project, &["watch", "features"], None);
    let pid = watch.child.id();
    let (address, _, _) = watch.serving();
    let port_file = project.join(".light-cljs/http.port");
    let port = fs::read_to_string(&port_file).unwrap();
    let (lines, _) = watch.until("[:features] built ");
    assert!(lines.is_empty(), "{lines:#?}");
    let out = project.join("out/dev");
    let mut runtime =
        file_watch::node_runtime(&out, &format!("http://{address}/features/events"), 2);
    // The runtime ran the build it loaded, which the replay leaves as it is.
    let (lines, replay) = runtime.until("message load replay ");
    assert_eq!(lines.last().map(String::as_str), Some("message hello 1"));
    assert!(
        replay.ends_with(" app.greeting app.main app.editor clojure.string app.reports"),
        "{replay}"
    );

    file_watch::save(
        &config,
        &text.replace(":app/greeting \"hello\"", ":app/greeting \"bonjour\""),
    );
    let (lines, _) = watch.until("[:features] built ");
    assert_eq!(
        lines,
        ["light-cljs: light-cljs.edn changed the settings of :features; restarting the build"]
    );
    // Its stream ended; it connected again, and the replay of the restarted
    // build reloaded the namespace whose unit the new value changed.
    let (lines, reloaded) = runtime.until("light-cljs: reloaded ");
    assert_eq!(
        lines[0],
        "light-cljs: disconnected from the dev server (ended); reconnecting"
    );
    assert_eq!(lines.last().map(String::as_str), Some("message hello 2"));
    assert!(
        reloaded.starts_with("light-cljs: reloaded app.main in "),
        "{reloaded}"
    );
    let mut stdout = String::new();
    std::io::Read::read_to_string(runtime.child.stdout.as_mut().unwrap(), &mut stdout).unwrap();
    assert!(runtime.child.wait().unwrap().success(), "{stdout}");
    let record = stdout
        .lines()
        .find(|line| line.starts_with("{\"id\":\"program\""))
        .unwrap_or_else(|| panic!("no record in {stdout}"));
    assert!(
        record.contains("greeting: hello\\n") && record.contains("greeting: bonjour\\n"),
        "{record}"
    );
    assert_eq!(watch.child.id(), pid);
    assert!(watch.child.try_wait().unwrap().is_none());
    assert_eq!(fs::read_to_string(&port_file).unwrap(), port);
    let (status, body) = file_watch::get(&address, "/features/app.main.js");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(body.contains("bonjour"), "{body}");
}

/// The Phase 3 test project, tests/fixtures/build-configuration/multi: the
/// illustration's `:app`, `:worker` and `:test` builds, which all require
/// `app.shared`.
const MULTI: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/build-configuration/multi"
);

/// `light-cljs watch app worker` of a temporary copy of the multi fixture,
/// on any free port and polling every 20 ms, with `edit` made to its
/// light-cljs.edn first, once both builds are served: the directory, the
/// project, the watch and the server's address. Before both builds' `built`
/// lines, in the order named, come the file's line, each build's `serving`
/// line, in the order named, and the nREPL listener's.
fn watching_app_and_worker(
    label: &str,
    edit: (&str, &str),
) -> (TempDir, PathBuf, file_watch::Light, String) {
    let (dir, project) = file_watch::project(label, Path::new(MULTI));
    edit_config(&project, edit.0, edit.1);
    let mut watch = file_watch::Light::spawn(
        &project,
        &["watch", "app", "worker"],
        Some("{:http {:port 0} :fs-watch {:loop-wait 20}}"),
    );
    let (address, before, serving) = watch.serving();
    assert_eq!(
        before,
        [format!(
            "light-cljs - config: {}",
            project.join("light-cljs.edn").display()
        )]
    );
    assert_eq!(
        serving,
        format!(
            "light-cljs: serving {} at http://{address}/",
            project.join("out/app").display()
        )
    );
    let (lines, built) = watch.until("[:app] built ");
    assert_eq!(lines.len(), 2, "{lines:#?}");
    assert_eq!(
        lines[0],
        format!(
            "light-cljs: serving {} at http://{address}/worker/",
            project.join("out/worker").display()
        )
    );
    assert!(
        lines[1].starts_with("light-cljs: nREPL at 127.0.0.1:"),
        "{lines:#?}"
    );
    assert!(
        built.starts_with("[:app] built app.main (")
            && built.contains(&format!(
                " namespaces) into {} in ",
                project.join("out/app").display()
            ))
            && built.ends_with(" ms; watching every 20 ms"),
        "{built}"
    );
    let (lines, built) = watch.until("[:worker] built ");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        built.starts_with("[:worker] built app.worker (")
            && built.ends_with(" ms; watching every 20 ms"),
        "{built}"
    );
    (dir, project, watch, address)
}

/// The latest and the earliest modification time of the files in `dir`.
fn written_between(dir: &Path) -> (std::time::SystemTime, std::time::SystemTime) {
    let times = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().modified().unwrap())
        .collect::<Vec<_>>();
    (*times.iter().max().unwrap(), *times.iter().min().unwrap())
}

/// The watch's lines up to a line of each build that starts with its one of
/// `prefixes`, which the builds print in either order, since each steps on
/// its own thread: every line read, in order.
fn both_until(watch: &mut file_watch::Light, prefixes: [&str; 2]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut seen = [false; 2];
    while !seen.iter().all(|&seen| seen) {
        let (before, line) = watch.until("[:");
        lines.extend(before);
        for (index, prefix) in prefixes.iter().enumerate() {
            seen[index] |= line.starts_with(prefix);
        }
        lines.push(line);
    }
    lines
}

/// `light-cljs watch app worker` runs both builds in one process
/// (docs/build-configuration.md, "Several builds in one process"): the
/// startup builds one after another in the order named, then one server,
/// the app at `/` and `/app/` and the worker at `/worker/`, each build's
/// lines on stderr named after it. A save of `app.shared`, which both
/// require, steps both, and a Node runtime connected to each build's event
/// stream receives its build's step; a save of `app.jobs`, which only the
/// worker requires, steps only the worker; a compile error in the worker is
/// on the worker's stderr lines and in its runtime's overlay events, named
/// after it, while a later save of the app's entry still steps the app.
#[test]
fn watch_app_worker_steps_each_build_on_its_own() {
    let (_dir, project, mut watch, address) =
        watching_app_and_worker("dev-watch-multi", ("\"out/test\"", "\"out/test\""));
    let (app_out, worker_out) = (project.join("out/app"), project.join("out/worker"));
    // The worker's startup build ran once the app's had written its build.
    let (app_last, _) = written_between(&app_out);
    let (_, worker_first) = written_between(&worker_out);
    assert!(app_last <= worker_first);
    for (path, file) in [
        ("/index.js", app_out.join("index.js")),
        ("/app/index.js", app_out.join("index.js")),
        ("/app/app.shared.js", app_out.join("app.shared.js")),
        ("/worker/index.js", worker_out.join("index.js")),
        ("/worker/app.jobs.js", worker_out.join("app.jobs.js")),
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
    let (status, _) = file_watch::get(&address, "/app.jobs.js");
    assert_eq!(status, "HTTP/1.1 404 Not Found");

    // The app's runtime receives the replay, the shared step and the app's
    // last step; the worker's the replay, the shared step, the worker's own
    // step and the error.
    let mut app = file_watch::node_runtime(&app_out, &format!("http://{address}/app/events"), 3);
    let mut worker =
        file_watch::node_runtime(&worker_out, &format!("http://{address}/worker/events"), 4);
    for runtime in [&mut app, &mut worker] {
        let (lines, replay) = runtime.until("message load replay ");
        assert!(
            lines.last().unwrap().starts_with("message hello "),
            "{lines:#?}"
        );
        assert!(replay.contains(" app.shared "), "{replay}");
    }

    let shared = project.join("src/app/shared.cljs");
    let text = fs::read_to_string(&shared).unwrap();
    file_watch::save(&shared, &text.replace("\"hello, \"", "\"hi, \""));
    let lines = both_until(&mut watch, ["[:app] recompiled ", "[:worker] recompiled "]);
    assert_eq!(lines.len(), 2, "{lines:#?}");
    for build in ["app", "worker"] {
        let step = format!("[:{build}] recompiled app.shared in ");
        assert!(
            lines.iter().any(|line| line.starts_with(&step)),
            "{lines:#?}"
        );
    }
    for out in [&app_out, &worker_out] {
        let unit = fs::read_to_string(out.join("app.shared.js")).unwrap();
        assert!(unit.contains("hi, "), "{unit}");
    }
    // The app reloads dependents (`:devtools {:reload-strategy :full}`).
    let (_, step) = app.until("message load ");
    assert_eq!(step, "message load app.shared app.main");
    let (_, step) = worker.until("message load ");
    assert_eq!(step, "message load app.shared");

    let jobs = project.join("src/app/jobs.cljs");
    let text = fs::read_to_string(&jobs).unwrap();
    file_watch::save(&jobs, &text.replace("(* n 21)", "(* n 3)"));
    let (lines, step) = watch.until("[:");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("[:worker] recompiled app.jobs in "),
        "{step}"
    );
    let (_, step) = worker.until("message load ");
    assert_eq!(step, "message load app.jobs");

    // A form that does not read is the worker's error alone.
    file_watch::save(&jobs, &text.replace("(* n 21))", "(* n 21)"));
    let (lines, failed) = watch.until("[:worker] recompiled ");
    assert_eq!(lines.len(), 1, "{lines:#?}");
    let error = lines[0]
        .strip_prefix("[:worker] ")
        .unwrap_or_else(|| panic!("{lines:#?}"));
    assert!(
        error.starts_with(&format!("{}:", jobs.display())),
        "{error}"
    );
    assert!(
        failed.starts_with("[:worker] recompiled nothing in ")
            && failed.ends_with(" ms; app.jobs failed and keeps its previous unit"),
        "{failed}"
    );
    let (_, event) = worker.until("message error ");
    assert!(
        event.starts_with(&format!("message error {}:", jobs.display()))
            && event.contains(": [:worker] "),
        "{event}"
    );

    let main = project.join("src/app/main.cljs");
    let text = fs::read_to_string(&main).unwrap();
    file_watch::save(&main, &format!("{text}\n(def saved \"saved\")\n"));
    // The worker compiles its failed namespace again at every change, as
    // one build does, and fails again, on its own lines.
    let (lines, step) = watch.until("[:app] ");
    assert!(
        lines.iter().all(|line| line.starts_with("[:worker] ")),
        "{lines:#?}"
    );
    assert!(step.starts_with("[:app] recompiled app.main in "), "{step}");
    let (_, step) = app.until("message load ");
    assert_eq!(step, "message load app.main");
    for mut runtime in [app, worker] {
        assert!(runtime.child.wait().unwrap().success());
    }
    assert!(watch.child.try_wait().unwrap().is_none());
}

/// A reread of light-cljs.edn restarts each build whose settings changed,
/// and only those (docs/build-configuration.md, "Watching the configuration
/// file"): an edit of `:worker`'s `:config` restarts the worker, whose event
/// stream ends while the app's goes on and steps, and a top-level change
/// restarts both, in the order named.
#[test]
fn a_reread_restarts_only_the_builds_whose_settings_changed() {
    let (_dir, project, mut watch, address) = watching_app_and_worker(
        "dev-watch-multi-reread",
        (
            ":modules {:main {:entries [app.worker]}}}",
            ":modules {:main {:entries [app.worker]}}\n   :config {:app/role \"worker\"}}",
        ),
    );
    let mut app = file_watch::Events::connect(&address, "/app/events");
    let mut worker = file_watch::Events::connect(&address, "/worker/events");
    for events in [&mut app, &mut worker] {
        assert!(events.next().unwrap().starts_with("{\"op\":\"hello\","));
        assert!(events.next().unwrap().starts_with("{\"op\":\"load\","));
    }
    edit_config(&project, ":app/role \"worker\"", ":app/role \"background\"");
    let (lines, built) = watch.until("[:");
    assert_eq!(
        lines,
        ["light-cljs: light-cljs.edn changed the settings of :worker; restarting the build"]
    );
    assert!(built.starts_with("[:worker] built app.worker ("), "{built}");
    assert_eq!(worker.next(), None);
    let shared = project.join("src/app/shared.cljs");
    let text = fs::read_to_string(&shared).unwrap();
    file_watch::save(&shared, &format!("{text}\n(def edited \"edited\")\n"));
    let lines = both_until(&mut watch, ["[:app] recompiled ", "[:worker] recompiled "]);
    assert_eq!(lines.len(), 2, "{lines:#?}");
    let load = app.next().unwrap();
    assert!(
        load.starts_with("{\"op\":\"load\",\"step\":1,\"replay\":false,"),
        "{load}"
    );

    edit_config(
        &project,
        ":cache-blockers #{app.build-info}",
        ":cache-blockers #{app.build-info app.jobs}",
    );
    let (lines, built) = watch.until("[:");
    assert_eq!(
        lines,
        [
            "light-cljs: light-cljs.edn changed the settings of :app; restarting the build",
            "light-cljs: light-cljs.edn changed the settings of :worker; restarting the build",
        ]
    );
    assert!(built.starts_with("[:app] built app.main ("), "{built}");
    let (lines, built) = watch.until("[:");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(built.starts_with("[:worker] built app.worker ("), "{built}");
    assert_eq!(app.next(), None);
    assert!(watch.child.try_wait().unwrap().is_none());
}

/// While one build's step is busy (a macro of the worker's `app.jobs` that
/// spins for three seconds), another build steps on its own: a save of
/// `app.main` during the worker's step reaches the app first, and a second
/// save of `app.jobs` during that step is stepped after it, so the worker's
/// unit ends with the latest text.
#[test]
fn a_busy_build_does_not_hold_up_the_other() {
    let (_dir, project) = file_watch::project("dev-watch-multi-busy", Path::new(MULTI));
    fs::write(
        project.join("src/app/slow.cljs"),
        "(ns app.slow)\n\n(defmacro spin [ms]\n  (let [end (+ (.now js/Date) ms)]\n    \
         (loop [] (when (< (.now js/Date) end) (recur)))\n    ms))\n",
    )
    .unwrap();
    let jobs = project.join("src/app/jobs.cljs");
    let source = |ms: u32, label: &str| {
        format!(
            "(ns app.jobs (:require [app.slow :refer [spin]]))\n\n(def spun (spin {ms}))\n\n\
             (def label \"{label}\")\n\n(defn run [n] (* n 21))\n"
        )
    };
    fs::write(&jobs, source(0, "started")).unwrap();
    let mut watch = file_watch::Light::spawn(
        &project,
        &["watch", "app", "worker"],
        Some("{:http {:port 0} :fs-watch {:loop-wait 20}}"),
    );
    watch.until("[:worker] built ");
    file_watch::save(&jobs, &source(3000, "busy"));
    std::thread::sleep(Duration::from_millis(500));
    let main = project.join("src/app/main.cljs");
    let text = fs::read_to_string(&main).unwrap();
    file_watch::save(&main, &format!("{text}\n(def saved \"saved\")\n"));
    std::thread::sleep(Duration::from_millis(100));
    file_watch::save(&jobs, &source(0, "latest"));
    let (lines, step) = watch.until("[:");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(step.starts_with("[:app] recompiled app.main in "), "{step}");
    let (lines, step) = watch.until("[:");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("[:worker] recompiled app.jobs in "),
        "{step}"
    );
    let millis = step
        .rsplit_once(" in ")
        .and_then(|(_, rest)| rest.strip_suffix(" ms"))
        .and_then(|millis| millis.parse::<f64>().ok())
        .unwrap_or_else(|| panic!("{step}"));
    assert!(millis >= 3000.0, "{step}");
    let (lines, step) = watch.until("[:");
    assert!(lines.is_empty(), "{lines:#?}");
    assert!(
        step.starts_with("[:worker] recompiled app.jobs in "),
        "{step}"
    );
    let unit = fs::read_to_string(project.join("out/worker/app.jobs.js")).unwrap();
    assert!(unit.contains("latest") && !unit.contains("busy"), "{unit}");
}

/// The CPU time the process `pid` has used, in seconds, as `ps` reports it.
fn cpu_seconds(pid: u32) -> f64 {
    let output = std::process::Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let mut seconds = 0.0;
    for part in text.trim().split(':') {
        seconds = seconds * 60.0 + part.parse::<f64>().unwrap_or_else(|_| panic!("{text:?}"));
    }
    seconds
}

/// A build whose restart failed waits for the next change without using
/// the CPU, while the other build steps; the next change of the file
/// starts it again.
#[test]
fn a_failed_restart_waits_for_work() {
    let (_dir, project, mut watch, _) =
        watching_app_and_worker("dev-watch-multi-failed", ("\"out/test\"", "\"out/test\""));
    edit_config(&project, "[app.worker]", "[app.missing]");
    let (lines, failed) = watch.until("[:");
    assert_eq!(
        lines,
        ["light-cljs: light-cljs.edn changed the settings of :worker; restarting the build"]
    );
    assert!(
        failed.starts_with("[:worker] ")
            && failed.contains("app.missing")
            && failed.ends_with("; the build starts again at the next change"),
        "{failed}"
    );
    let pid = watch.child.id();
    let before = cpu_seconds(pid);
    std::thread::sleep(Duration::from_millis(1500));
    let used = cpu_seconds(pid) - before;
    assert!(used < 0.5, "{used} s of CPU in 1.5 s");
    let main = project.join("src/app/main.cljs");
    let text = fs::read_to_string(&main).unwrap();
    file_watch::save(&main, &format!("{text}\n(def saved \"saved\")\n"));
    let lines = both_until(&mut watch, ["[:app] recompiled app.main in ", "[:worker] "]);
    assert_eq!(lines.len(), 2, "{lines:#?}");
    edit_config(&project, "[app.missing]", "[app.worker]");
    let (lines, built) = watch.until("[:");
    assert_eq!(
        lines,
        ["light-cljs: light-cljs.edn changed the settings of :worker; restarting the build"]
    );
    assert!(built.starts_with("[:worker] built app.worker ("), "{built}");
}
