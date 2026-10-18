//! The browser playground's WebAssembly entry. The page hands over its files
//! and an entry namespace; this compiles them with light-cljs (macros expand in
//! the embedded QuickJS, inside this module) and returns the JavaScript, which
//! the page then runs in the browser's own engine.
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry_with_provider;
use light_cljs::host_evaluator::ScriptHost;
use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput, SourceProvider};
use light_cljs::{
    ApplicationOptions, MacroHost, OptimizationLevel, PreparedApplication, prepare_application,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date)]
    fn now() -> f64;
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(message: &str);
}

// The page's macro host (playground/macro-host.js), looked up when a call is
// made, so a module that never selects browser macros needs none.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = lightCljsMacroHost, js_name = engine)]
    fn host_engine() -> String;
    #[wasm_bindgen(catch, js_namespace = lightCljsMacroHost, js_name = open)]
    fn host_open(session: f64, services: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_namespace = lightCljsMacroHost, js_name = call)]
    fn host_call(session: f64, name: &str, arguments: &str) -> Result<String, JsValue>;
    #[wasm_bindgen(catch, js_namespace = lightCljsMacroHost, js_name = install)]
    fn host_install(session: f64, name: &str, index: u32, strings: bool) -> Result<(), JsValue>;
    #[wasm_bindgen(js_namespace = lightCljsMacroHost, js_name = close)]
    fn host_close(session: f64);
}

fn host_error(error: JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| "the macro host failed".to_owned())
}

/// Runs compiler macros in the JavaScript engine running this module,
/// through `globalThis.lightCljsMacroHost`.
struct BrowserHost;

impl ScriptHost for BrowserHost {
    fn engine(&self) -> String {
        host_engine()
    }
    fn open(&self, session: u64, services: &str) -> Result<(), String> {
        host_open(session as f64, services).map_err(host_error)
    }
    fn call(&self, session: u64, name: &str, arguments: &str) -> Result<String, String> {
        host_call(session as f64, name, arguments).map_err(host_error)
    }
    fn install(&self, session: u64, name: &str, index: usize, strings: bool) -> Result<(), String> {
        host_install(session as f64, name, index as u32, strings).map_err(host_error)
    }
    fn close(&self, session: u64) {
        host_close(session as f64)
    }
}

thread_local! {
    static BROWSER_MACROS: Cell<bool> = const { Cell::new(false) };
}

/// Run the macros of later compilations in this JavaScript engine (true),
/// through `globalThis.lightCljsMacroHost`, or in the embedded QuickJS
/// (false, the default). light-cljs-307's prototype; see
/// docs/browser-macros.md.
#[wasm_bindgen]
pub fn set_browser_macros(enabled: bool) {
    BROWSER_MACROS.with(|cell| cell.set(enabled));
}

/// The macro host's way back in: a compiler service its session called.
#[wasm_bindgen]
pub fn serve_macro_host(
    session: f64,
    index: u32,
    strings: bool,
    request: &str,
) -> Result<String, String> {
    light_cljs::host_evaluator::serve(session as u64, index as usize, strings, request)
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

/// What one compilation produced. Exactly one of `error` and the output is
/// set: `js` from [`compile`], or `manifest` with `files` and `chunks` from
/// [`compile_chunks`].
#[wasm_bindgen(getter_with_clone)]
#[derive(Default)]
pub struct CompileResult {
    /// The emitted program: an expression evaluating to the entry's last form,
    /// or an ES module when `esm` was requested.
    pub js: Option<String>,
    /// A chunked build's file names, startup first (`app.main.js`), and,
    /// parallel to them, each file's ES module, as `--out-dir` writes them.
    pub files: Vec<String>,
    pub chunks: Vec<String>,
    /// A chunked build's manifest, `chunks.json`.
    pub manifest: Option<String>,
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
    let options = Options {
        entry,
        optimize,
        warnings,
        pseudo_names,
        size_report,
    };
    prepare(paths, sources, options, |prepared, result| {
        result.js = Some(if esm {
            prepared.emit_esm()
        } else {
            prepared.emit_js()
        });
    })
}

/// Compile `entry` as [`compile`] does, into a chunked build (`--out-dir`):
/// one ES module per chunk, which load each other as `./NAME`, and the
/// manifest. The files are values; the page decides where they go.
#[wasm_bindgen]
pub fn compile_chunks(
    paths: Vec<String>,
    sources: Vec<String>,
    entry: &str,
    optimize: &str,
    warnings: bool,
    pseudo_names: bool,
    size_report: bool,
) -> CompileResult {
    let options = Options {
        entry,
        optimize,
        warnings,
        pseudo_names,
        size_report,
    };
    prepare(paths, sources, options, |prepared, result| {
        let output = prepared.emit_chunks();
        result.manifest = Some(output.manifest());
        for file in output.files {
            result.files.push(file.name);
            result.chunks.push(file.source);
        }
    })
}

/// The options [`compile`] and [`compile_chunks`] share.
struct Options<'a> {
    entry: &'a str,
    optimize: &'a str,
    warnings: bool,
    pseudo_names: bool,
    size_report: bool,
}

/// Analyze and prepare `options.entry` from the files given as parallel
/// `paths` and `sources`, and let `emit` write the output into the result.
fn prepare(
    paths: Vec<String>,
    sources: Vec<String>,
    options: Options<'_>,
    emit: impl FnOnce(&PreparedApplication, &mut CompileResult),
) -> CompileResult {
    let started = now();
    let mut result = CompileResult::default();
    let level = match options.optimize.parse::<OptimizationLevel>() {
        Ok(level) => level,
        Err(error) => {
            result.error = Some(error.to_string());
            return result;
        }
    };
    let files = MemorySources {
        files: paths.into_iter().zip(sources).collect(),
    };
    let analysis = AnalysisOptions {
        warning_mode: options.warnings,
        ..AnalysisOptions::default()
    };
    let mut collected = Vec::new();
    let mut host = MacroHost::new();
    if BROWSER_MACROS.with(Cell::get) {
        host.set_script_host(Rc::new(BrowserHost));
    }
    let program = analyze_namespace_entry_with_provider(
        options.entry,
        &files,
        &mut host,
        analysis,
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
            application.pseudo_names = options.pseudo_names;
            let prepared = prepare_application(&program, application);
            emit(&prepared, &mut result);
            if options.size_report {
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
