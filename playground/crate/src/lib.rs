//! The browser playground's WebAssembly entry. The page hands over its files
//! and an entry namespace; this compiles them with light-cljs (macros expand in
//! the embedded QuickJS, inside this module) and returns the JavaScript, which
//! the page then runs in the browser's own engine.
use std::collections::HashMap;

use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry_with_provider;
use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput, SourceProvider};
use light_cljs::{ApplicationOptions, MacroHost, OptimizationLevel, prepare_application};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date)]
    fn now() -> f64;
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(message: &str);
}

/// The clock that the engine's wasm shim and the evaluator's session deadline
/// import on wasm32-unknown-unknown: microseconds since the Unix epoch.
/// Defining it here resolves the import at link time, so the module needs no
/// `env` import object.
#[unsafe(no_mangle)]
pub extern "C" fn __rquickjs_host_now_us() -> f64 {
    now() * 1000.0
}

#[wasm_bindgen(start)]
fn start() {
    std::panic::set_hook(Box::new(|info| console_error(&info.to_string())));
}

/// The crate version, for the page header.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// The page's files by path (`demo/main.cljs`), resolved the way source roots
/// are: dots become directories, hyphens underscores, `.cljs` before `.cljc`.
struct MemorySources {
    files: HashMap<String, String>,
}

impl SourceProvider for MemorySources {
    fn load(&self, request: &LoadRequest) -> Result<Option<SourceInput>, LoadError> {
        let name = String::from_utf16_lossy(request.namespace.name());
        let relative = name.replace('.', "/").replace('-', "_");
        for extension in ["cljs", "cljc"] {
            let path = format!("{relative}.{extension}");
            if let Some(source) = self.files.get(&path) {
                return Ok(Some(SourceInput::new(path, source.clone())));
            }
        }
        Ok(None)
    }
}

/// What one compilation produced. Exactly one of `js` and `error` is set.
#[wasm_bindgen(getter_with_clone)]
#[derive(Default)]
pub struct CompileResult {
    /// The emitted program: an expression evaluating to the entry's last form,
    /// or an ES module when `esm` was requested.
    pub js: Option<String>,
    /// The diagnostic, already located (`file:line:column: message`).
    pub error: Option<String>,
    pub error_file: Option<String>,
    /// 1-based, 0 when the error has no location.
    pub error_line: u32,
    pub error_column: u32,
    /// One warning per line, empty without `warnings`.
    pub warnings: String,
    /// The retention report (`--size-report`), empty unless requested.
    pub size_report: String,
    /// Compilation time in milliseconds.
    pub millis: f64,
}

/// Compile `entry` from the files given as parallel `paths` and `sources`.
/// `optimize` is `none`, `prepare` or `advanced`, as for `--optimize`.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn compile(
    paths: Vec<String>,
    sources: Vec<String>,
    entry: &str,
    optimize: &str,
    warnings: bool,
    pseudo_names: bool,
    size_report: bool,
    esm: bool,
) -> CompileResult {
    let started = now();
    let mut result = CompileResult::default();
    let level = match optimize.parse::<OptimizationLevel>() {
        Ok(level) => level,
        Err(error) => {
            result.error = Some(error.to_string());
            return result;
        }
    };
    let files = MemorySources {
        files: paths.into_iter().zip(sources).collect(),
    };
    let options = AnalysisOptions {
        warning_mode: warnings,
        ..AnalysisOptions::default()
    };
    let mut collected = Vec::new();
    let program = analyze_namespace_entry_with_provider(
        entry,
        &files,
        &mut MacroHost::new(),
        options,
        &mut collected,
    );
    result.warnings = collected
        .iter()
        .map(|warning| {
            format!(
                "{}: {} (source {}, bytes {}..{})",
                warning.kind,
                warning.message,
                warning.source_index,
                warning.span.start,
                warning.span.end
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    match program {
        Ok(program) => {
            let mut application = ApplicationOptions::new(level);
            application.pseudo_names = pseudo_names;
            let prepared = prepare_application(&program, application);
            result.js = Some(if esm {
                prepared.emit_esm()
            } else {
                prepared.emit_js()
            });
            if size_report {
                result.size_report = prepared.retention().to_string();
            }
        }
        Err(error) => {
            result.error = Some(error.to_string());
            result.error_file = error.path.map(|path| path.to_string_lossy().into_owned());
            if let Some((line, column)) = error.line_column {
                result.error_line = line as u32;
                result.error_column = column as u32;
            }
        }
    }
    result.millis = now() - started;
    result
}
