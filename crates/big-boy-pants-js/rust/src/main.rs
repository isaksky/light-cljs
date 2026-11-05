//! Thin binary over the `big_boy_pants_js` library: compile a JavaScript script of
//! the documented subset (`docs/grammar.md`) and run it as one task.
//!
//! * `cargo run` runs the bundled demo, `examples/demo.js` (a documented
//!   synthetic fixture).
//! * `cargo run -- path/to/script.js` runs that file instead, as strict code.
//! * `cargo run -- --sloppy path/to/script.js` runs it as non-strict code,
//!   except where a `"use strict"` directive applies (the conformance harness
//!   runs QuickJS's sloppy-mode test files this way).
//! * `--module` runs the file as a module graph; relative imports are files
//!   beside the importing module, and a `.json` file is a JSON module.
//! * `--host-bindings` installs the QuickJS-style `std.gc`, `os.setTimeout`
//!   and `os.clearTimeout` bindings (`Task::install_sim_host_bindings`), as
//!   the QuickJS-derived Rust tests do.
//!
//! The completion value is printed; a string exactly, as a quoted literal
//! that writes a lone surrogate as `\u{d800}` (`JsString`'s `Debug`). Host
//! effects (`__rt.effect(op, x)`) print `effect op: x` (a string lossily,
//! U+FFFD for a lone surrogate) and return `x`. Compilation errors exit with status 2, run
//! failures with status 1. No qjsc or C engine is involved: the script is
//! compiled by `big_boy_pants_js::frontend` and executed by its VM.

use big_boy_pants_js::frontend::{ScriptOptions, compile_script_with};
use big_boy_pants_js::module::{ModuleHost, ModuleKey, ModuleLoader, ModuleSource};
use big_boy_pants_js::{
    Budget, Host, HostCall, JsString, Primitive, RunOutcome, Runtime, ValueView,
};

/// Prints each host effect and returns its argument.
struct PrintingHost;

impl Host for PrintingHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let shown = match &call.arg {
            Primitive::Number(n) => n.to_string(),
            Primitive::String(s) => s.to_string(),
            other => format!("{other:?}"),
        };
        println!("effect {}: {shown}", call.op);
        Ok(call.arg)
    }
}

/// `--module`: modules are files; a relative specifier resolves against the
/// importing module's directory, and each canonical path is one module.
#[derive(Default)]
struct FileModules {
    keys: std::collections::HashMap<ModuleKey, std::path::PathBuf>,
}

impl ModuleHost for FileModules {
    fn resolve(
        &mut self,
        specifier: &JsString,
        referrer: Option<ModuleKey>,
    ) -> Result<ModuleKey, String> {
        let specifier =
            String::from_utf16(specifier.code_units()).map_err(|_| "invalid UTF-16 specifier")?;
        let base = referrer
            .and_then(|key| self.keys[&key].parent().map(std::path::Path::to_path_buf))
            .unwrap_or_default();
        let path = base
            .join(&specifier)
            .canonicalize()
            .map_err(|e| format!("{specifier}: {e}"))?;
        if let Some((key, _)) = self.keys.iter().find(|(_, p)| **p == path) {
            return Ok(*key);
        }
        let key = ModuleKey::new(self.keys.len() as u64);
        self.keys.insert(key, path);
        Ok(key)
    }

    fn fetch(&mut self, key: ModuleKey) -> Result<ModuleSource, String> {
        let path = &self.keys[&key];
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(ModuleSource::from_name(path.display().to_string(), text))
    }
}

fn main() {
    let mut options = ScriptOptions::STRICT;
    let mut module = false;
    let mut host_bindings = false;
    let mut path = None;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--sloppy" => options = ScriptOptions::SLOPPY,
            "--module" => module = true,
            "--host-bindings" => host_bindings = true,
            _ => path = Some(argument),
        }
    }
    let (name, source) = match path {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(source) => (path, source),
            Err(e) => {
                eprintln!("{path}: {e}");
                std::process::exit(2);
            }
        },
        None => (
            "examples/demo.js".to_string(),
            include_str!("../examples/demo.js").to_string(),
        ),
    };
    let rt = Runtime::new();
    let task = if module {
        let graph = match ModuleLoader::new().load(
            rt.world(),
            &JsString::from(name.as_str()),
            &mut FileModules::default(),
        ) {
            Ok(graph) => graph,
            Err(e) => {
                eprintln!("{name}: {e}");
                std::process::exit(2);
            }
        };
        rt.spawn_module(&graph)
    } else {
        let exe = match compile_script_with(&name, &source, options) {
            Ok(exe) => exe,
            Err(e) => {
                eprintln!("{name}:{e}");
                std::process::exit(2);
            }
        };
        match rt.spawn(&exe, &[]) {
            Ok(task) => task,
            Err(e) => {
                eprintln!("run error: {e}");
                std::process::exit(1);
            }
        }
    };
    if host_bindings && let Err(e) = task.install_sim_host_bindings() {
        eprintln!("run error: host bindings: {e:?}");
        std::process::exit(1);
    }
    let outcome = task.run_with(Budget::UNLIMITED, &mut PrintingHost);
    match outcome {
        Ok(RunOutcome::Complete(v)) => match v.view() {
            ValueView::Number(n) => println!("{name} completed with {n}"),
            ValueView::BigInt => println!("{name} completed with {}n", v.bigint().unwrap()),
            ValueView::String => match v.string() {
                Some(s) => println!("{name} completed with {s:?}"),
                None => println!("{name} completed with a string"),
            },
            other => println!("{name} completed with {other:?}"),
        },
        Ok(RunOutcome::Failed(e)) => {
            // The error's display includes the source position.
            eprintln!("{name}: {e}");
            std::process::exit(1);
        }
        Ok(other) => {
            eprintln!("{name}: {other:?}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("run error: {e}");
            std::process::exit(1);
        }
    }
    let report = task.unhandled_rejections();
    for rejection in &report.entries {
        let reason = &rejection.reason;
        let shown = if let Some(error) = reason.error() {
            error.to_string()
        } else {
            match reason.view() {
                ValueView::Number(n) => n.to_string(),
                ValueView::String => format!("{:?}", reason.string().unwrap()),
                ValueView::BigInt => format!("{}n", reason.bigint().unwrap()),
                other => format!("{other:?}"),
            }
        };
        eprintln!("{name}: Possibly unhandled promise rejection: {shown}");
    }
    if report.dropped > 0 {
        eprintln!(
            "{name}: {} additional unhandled promise rejections omitted",
            report.dropped
        );
    }
    if !report.entries.is_empty() || report.dropped > 0 {
        std::process::exit(1);
    }
}
