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
        let dir = TempDir::new(label);
        fs::create_dir_all(dir.join("src/app")).unwrap();
        for name in ["state", "macros", "view", "main"] {
            let file = format!("app/{name}.cljs");
            fs::copy(Path::new(EXAMPLE).join(&file), dir.join("src").join(&file)).unwrap();
        }
        setup(&dir.join("src"));
        let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
            .args(["--watch", "--entry", "app.main", "--source-root"])
            .arg(dir.join("src"))
            .arg("--out-dir")
            .arg(dir.join("dev"))
            .args(["--poll-ms", "20"])
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
        let mut watch = Self {
            dir,
            child,
            lines,
            built: String::new(),
        };
        let (_, line) = watch.until("light-cljs: built ");
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
        watch
    }

    /// The stderr lines up to the first one that starts with `prefix`, and
    /// that line.
    fn until(&mut self, prefix: &str) -> (Vec<String>, String) {
        let mut before = Vec::new();
        loop {
            match self.lines.recv_timeout(TIMEOUT) {
                Ok(line) if line.starts_with(prefix) => return (before, line),
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
            .args(["--compile", "--entry", "app.main", "--source-root"])
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
    eprintln!(
        "dev_watch: save of app.view to its unit on disk, ms: {times:?}; app.view.js {} bytes (map {}), {} before",
        watch.read("app.view.js").len(),
        watch.read("app.view.js.map").len(),
        fs::read_to_string(watch.dir.join("before/app.view.js"))
            .unwrap()
            .len()
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
fn watch_refuses_missing_options_and_a_failing_first_build() {
    let dir = TempDir::new("dev-watch-options");
    let out = dir.join("dev");
    let out = out.to_str().unwrap();
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
}
