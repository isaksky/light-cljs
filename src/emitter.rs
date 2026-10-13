//! Recursive JavaScript emission from resolved expressions; no name lookup.

use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
};

use crate::analyzer::{
    ExprContext, ExprId, ExprKind, FnMethod, GlobalId, LocalId, MethodDispatch, NamedLiteralId,
    PACKAGE_REGISTRY, Program, RecurTargetId, StringId,
};
use crate::application::OptimizationLevel;
use crate::core_operations::Type;
use crate::inference::{Inference, Known};
use crate::runtime_manifest::{CORE_CELLS, CORE_HUB, CoreReference, RuntimeManifest};
use crate::runtime_slice::{EmittedRuntime, NameUse, RuntimeEmbedding, RuntimeName, RuntimeRoots};

/// What `rt.core.<name>` holds in the embedded runtime's core table.
fn core_reference(name: &str) -> CoreReference<'static> {
    RuntimeManifest::embedded()
        .core_reference(name)
        .unwrap_or_else(|| {
            panic!("core operation `{name}` is missing from the runtime's core table")
        })
}

/// Emit one JavaScript expression whose value is the last source form (or nil).
/// The result is self-contained. The default Rust application preparation pass
/// runs first; use `prepare_application` to select options or reuse preparation.
pub fn emit_js(program: &Program) -> String {
    crate::application::prepare_application(program, Default::default()).emit_js()
}

/// Emit a prepared application. At `Advanced` the output reads runtime
/// bindings directly and embeds only the units those reads reach (see
/// [`crate::runtime_slice`]); `replaced_core` lists the runtime names of the
/// core operations the program may replace, which it reads through cells.
pub(crate) fn emit_application(
    program: &Program,
    globals: &[bool],
    calls: &std::collections::HashMap<ExprId, usize>,
    level: OptimizationLevel,
    replaced_core: &[&'static str],
    inference: Option<&Inference>,
    module: bool,
) -> (String, EmittedRuntime) {
    emit_js_mode(
        program,
        None,
        false,
        false,
        Some(Application {
            globals,
            calls,
            direct: level.advanced().then_some(replaced_core),
            inference: inference.filter(|_| level.advanced()),
        }),
        module,
    )
}

/// What application preparation decided for emission.
#[derive(Clone, Copy)]
struct Application<'a> {
    /// Retained globals, by `GlobalId`.
    globals: &'a [bool],
    /// Calls specialized to a fixed arity entry.
    calls: &'a std::collections::HashMap<ExprId, usize>,
    /// At `Advanced`: read runtime bindings directly from a runtime slice. The
    /// runtime names of the core operations the program may replace, sorted; their
    /// reads and writes go through an output-level cell, `core$<name>`.
    direct: Option<&'a [&'static str]>,
    /// At `Advanced`: the program's inferred types, which lower truth tests
    /// and core calls.
    inference: Option<&'a Inference>,
}

/// Emit one ES module for a bundler instead of a bare expression. Each package
/// registry reference (see [`crate::analyzer::PACKAGE_REGISTRY`]) becomes a
/// module-local binding of a static import: `r/createElement` is
/// `import { createElement as $react$createElement } from "react"`, and an alias
/// used as a value imports the namespace object. Only references that survive
/// application preparation are imported. The program value is the default export.
pub fn emit_esm(program: &Program) -> String {
    crate::application::prepare_application(program, Default::default()).emit_esm()
}

/// Emit a function accepting the session canonical runtime as its sole argument.
/// This shares runtime brands and bypasses optional application optimization.
pub fn emit_js_unit(program: &Program) -> String {
    emit_unit_mode(program, false)
}

/// Compile-time globals use the session demand registry; all semantic lowering is shared.
pub(crate) fn emit_deferred_unit(program: &Program) -> String {
    emit_unit_mode(program, true)
}

fn emit_unit_mode(program: &Program, demand_roots: bool) -> String {
    let raw = program
        .strings
        .iter()
        .map(|s| String::from_utf16_lossy(s))
        .collect::<String>();
    let mut index = 0;
    let parameter = loop {
        let candidate = format!("__lightSessionRuntime{index}");
        if !raw.contains(&candidate) {
            break candidate;
        }
        index += 1;
    };
    format!(
        "({parameter}) => {}",
        emit_js_mode(program, Some(&parameter), demand_roots, false, None, false).0
    )
}

/// `application` carries the decisions of application preparation, including
/// whether to read a runtime slice directly (`Advanced`). The returned runtime
/// report lists every runtime name the output reads.
fn emit_js_mode(
    program: &Program,
    shared: Option<&str>,
    demand_roots: bool,
    application_roots: bool,
    application: Option<Application<'_>>,
    module: bool,
) -> (String, EmittedRuntime) {
    let direct = application
        .and_then(|application| application.direct)
        .filter(|_| shared.is_none());
    let inference = application
        .and_then(|application| application.inference)
        .filter(|_| direct.is_some());
    // The prelude, the `let g<n>;` declarations and the program body.
    let body = |register_namespaces: bool| {
        let mut emitter = Emitter {
            program,
            demand_roots,
            application_roots,
            register_namespaces,
            needs_host: false,
            needs_package: false,
            imports: module.then(Vec::new),
            specialized_calls: application.map(|application| application.calls),
            replaced_core: direct,
            core_cells: Vec::new(),
            constants: direct.map(|_| ConstantTable {
                declarations: String::new(),
                numbers: HashMap::new(),
                named: vec![[None; 2]; program.named_literals.len()],
            }),
            output: String::new(),
            indent: 1,
            captures: Vec::new(),
            reify_captures: Vec::new(),
            reify_receivers: Vec::new(),
            capture_resets: Vec::new(),
            field_resets: Vec::new(),
            used: HashSet::new(),
            returned_definition: None,
            raw_segments: Vec::new(),
            runtime: RuntimeRoots::default(),
            site: None,
            inference,
            truth_used: false,
        };
        emitter.output.push_str(PRELUDE_OPEN);
        emitter.output.push_str(PRELUDE_TRUTH);
        let mut prelude_end = emitter.output.len();
        for global in 0..program.globals.len() {
            if application_roots {
                break;
            }
            if application.is_some_and(|application| !application.globals[global]) {
                continue;
            }
            writeln!(emitter.output, "  let g{global};").unwrap();
        }
        emitter.emit(program.root, ExprContext::Return);
        // `Advanced` output that tests no value with `truth` does not declare it.
        if emitter.inference.is_some() && !emitter.truth_used {
            let start = PRELUDE_OPEN.len();
            emitter
                .output
                .replace_range(start..start + PRELUDE_TRUTH.len(), "");
            prelude_end -= PRELUDE_TRUTH.len();
            for range in &mut emitter.raw_segments {
                range.start -= PRELUDE_TRUTH.len();
                range.end -= PRELUDE_TRUTH.len();
            }
        }
        (emitter, prelude_end)
    };
    // Advanced output registers its namespaces only when the program reaches
    // reflection, which the roots of its body decide
    // (`RuntimeRoots::reach_reflection`): the body is emitted without the
    // registration, and again with it when those roots reach reflection.
    let slice = direct.is_some();
    let (mut emitter, mut prelude_end) = body(!slice);
    if slice && emitter.runtime.reach_reflection() {
        (emitter, prelude_end) = body(true);
    }
    #[cfg(debug_assertions)]
    check_runtime_reads(
        &emitter.output,
        &emitter.raw_segments,
        &emitter.runtime,
        &emitter.core_cells,
    );
    emitter.output.push_str("})()\n");
    if emitter.needs_host || emitter.needs_package {
        let mut host = String::new();
        if emitter.needs_host {
            host.push_str("  const host = (name) => { if (!(name in globalThis)) throw new ReferenceError(name + \" is not defined\"); return globalThis[name]; };\n");
        }
        if emitter.needs_package {
            writeln!(host, "  const pkg = (name) => {{ const registry = globalThis.{PACKAGE_REGISTRY}; if (registry == null || !(name in Object(registry))) throw new ReferenceError(\"JavaScript package \\\"\" + name + \"\\\" is not provided by globalThis.{PACKAGE_REGISTRY}\"); return registry[name]; }};").unwrap();
        }
        let host = host.as_str();
        for range in &mut emitter.raw_segments {
            if range.start >= prelude_end {
                range.start += host.len();
                range.end += host.len();
            }
        }
        emitter.output.insert_str(prelude_end, host);
    }
    // Advanced application output embeds the units its runtime reads reach;
    // everything else keeps the whole runtime under the expression-kind test.
    let embed = if slice {
        !emitter.runtime.is_empty()
    } else {
        demand_roots || application_roots || reads_runtime(program, &emitter.used)
    };
    debug_assert!(
        embed || emitter.runtime.is_empty(),
        "emitted code reads the runtime without embedding it"
    );
    let mut embedding = RuntimeEmbedding::None;
    let mut runtime_bytes = 0;
    if embed {
        let mut registration = String::new();
        let register_namespaces = emitter.register_namespaces;
        // At `Advanced` a namespace lists only the definitions the output keeps.
        let retained = |global: usize| {
            !slice || application.is_none_or(|application| application.globals[global])
        };
        for namespace in program.namespaces.iter().filter(|_| register_namespaces) {
            let previous = std::mem::take(&mut emitter.output);
            emitter.string_units(&namespace.encode_utf16().collect::<Vec<_>>());
            let literal = std::mem::replace(&mut emitter.output, previous);
            let register = emitter.runtime_read(
                RuntimeName::core("registerReaderNamespace"),
                None,
                NameUse::Value,
            );
            writeln!(registration, "  {register}({literal}, {{aliases: [").unwrap();
            if let Some(context) = program.reader_contexts.get(namespace) {
                for (alias, target) in &context.aliases {
                    writeln!(
                        registration,
                        "    [{}, {}],",
                        js_string(alias),
                        js_string(target)
                    )
                    .unwrap();
                }
                registration.push_str("  ], refers: [\n");
                for (local, ns, original) in &context.refers {
                    writeln!(
                        registration,
                        "    [{}, {}, {}],",
                        js_string(local),
                        js_string(ns),
                        js_string(original)
                    )
                    .unwrap();
                }
                writeln!(
                    registration,
                    "  ], excludes: [{}],",
                    context
                        .excludes
                        .iter()
                        .map(|s| js_string(s))
                        .collect::<Vec<_>>()
                        .join(",")
                )
                .unwrap();
            } else {
                registration.push_str("  ],\n");
            }
            writeln!(
                registration,
                "  bindings: [{}]}});",
                program
                    .globals
                    .iter()
                    .enumerate()
                    .filter(|(id, g)| g.namespace == *namespace && retained(*id))
                    .map(|(_, g)| js_string(&g.name))
                    .chain(
                        program
                            .reader_contexts
                            .get(namespace)
                            .into_iter()
                            .flat_map(|c| &c.macros)
                            .map(|name| js_string(name))
                    )
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .unwrap();
        }
        if shared.is_none()
            && register_namespaces
            && let Some(namespace) = program.namespaces.last()
        {
            let [current, the_ns, symbol] = [
                RuntimeName::core("currentNs"),
                RuntimeName::core("theNs"),
                RuntimeName::export("symbol"),
            ]
            .map(|name| emitter.runtime_read(name, None, NameUse::Value));
            writeln!(
                registration,
                "  {current} = {the_ns}({symbol}(null, {}));",
                js_string(namespace)
            )
            .unwrap();
        }
        let mut runtime = if let Some(parameter) = shared {
            format!("  const rt = {parameter};\n")
        } else if slice {
            // `const [rt$add, rt$symbol] = (() => { ...; return [add, symbol]; })();`
            embedding = RuntimeEmbedding::Slice;
            let (factory, bindings) = crate::runtime_slice::sliced_runtime(&emitter.runtime);
            runtime_bytes = factory.trim().len();
            let pattern = bindings
                .iter()
                .map(|binding| format!("rt${binding}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("  const [{pattern}] = {};\n", factory.trim())
        } else {
            embedding = RuntimeEmbedding::Whole;
            runtime_bytes = crate::runtime_manifest::RUNTIME.trim().len();
            format!(
                "  const rt = {};\n",
                crate::runtime_manifest::RUNTIME.trim()
            )
        };
        let source_length = runtime.len();
        // The cells of the core operations the program may replace start as the
        // operations: `let core$inc = rt$inc;`.
        for (name, initial) in &emitter.core_cells {
            writeln!(runtime, "  let core${name} = {initial};").unwrap();
        }
        // The constant table follows the runtime it reads.
        if let Some(constants) = &emitter.constants {
            #[cfg(debug_assertions)]
            check_runtime_reads(
                &constants.declarations,
                &[],
                &emitter.runtime,
                &emitter.core_cells,
            );
            runtime.push_str(&constants.declarations);
        }
        runtime.push_str(&registration);
        for range in &mut emitter.raw_segments {
            if range.start >= prelude_end {
                range.start += runtime.len();
                range.end += runtime.len();
            }
        }
        if shared.is_none() {
            let start = prelude_end + runtime.find("(() =>").unwrap();
            emitter
                .raw_segments
                .push(start..prelude_end + source_length - 2);
        }
        emitter.output.insert_str(prelude_end, &runtime);
    }

    // Raw JavaScript is an escape hatch, but its user-authored identifiers must
    // not accidentally capture compiler locals or be captured by our helpers.
    let output = if emitter
        .used
        .iter()
        .any(|id| matches!(program.exprs[id.0].kind, ExprKind::JsTemplate { .. }))
    {
        hygienic_js(emitter.output, emitter.raw_segments)
    } else {
        emitter.output
    };
    let output = match emitter.imports {
        Some(imports) => module_source(&imports, &output),
        None => output,
    };
    let runtime = EmittedRuntime {
        roots: emitter.runtime,
        embedding,
        bytes: runtime_bytes,
    };
    (output, runtime)
}

/// The opening of every emitted expression, followed by [`PRELUDE_TRUTH`].
const PRELUDE_OPEN: &str = "(() => {\n";
/// Clojure truthiness, which `if` tests use unless the test's type makes
/// JavaScript truthiness agree.
const PRELUDE_TRUTH: &str = "  const truth = (value) => value != null && value !== false;\n";

/// Whether some emitted expression is of a kind whose emission reads the
/// runtime. Below `Advanced` this decides whether the whole runtime is embedded.
fn reads_runtime(program: &Program, used: &HashSet<ExprId>) -> bool {
    !program.namespace_entries.is_empty() || program.exprs.iter().enumerate().any(|(id, expr)| {
        used.contains(&ExprId(id)) && (
        matches!(
            expr.kind,
            ExprKind::Keyword(_)
                | ExprKind::Symbol(_)
                | ExprKind::List(_)
                | ExprKind::Vector(_)
                | ExprKind::Map(_)
                | ExprKind::Set(_)
                | ExprKind::LiteralMap(_)
                | ExprKind::LiteralSet(_)
                | ExprKind::CoreFn(_)
                | ExprKind::ReaderLiteral { .. }
                | ExprKind::WithMeta { .. }
                | ExprKind::Var { .. }
                | ExprKind::WithOutStr(_)
                | ExprKind::DefProtocol { .. }
                | ExprKind::ExtendType { .. }
                | ExprKind::ProtocolPredicate { .. }
                | ExprKind::Reify { .. }
                | ExprKind::DefType { .. }
                | ExprKind::Assert { message: Some(_), .. }
                | ExprKind::Case { strict: false, .. }
                | ExprKind::Case { keyword_tests: true, .. }
                | ExprKind::Case { default: None, .. }

        ) || matches!(&expr.kind, ExprKind::Fn { methods, .. } if methods.iter().any(|m| m.rest.is_some()))
        )
    })
}

/// Debug check that every runtime read in emitted code outside raw JavaScript
/// was recorded through [`Emitter::rt`]: each `rt.<binding>[.<member>]`, each
/// `rt$<binding>` (`Advanced`) and each `core$<name>` cell. A read the roots
/// miss would be absent from an `Advanced` runtime slice.
#[cfg(debug_assertions)]
fn check_runtime_reads(
    output: &str,
    raw: &[std::ops::Range<usize>],
    roots: &RuntimeRoots,
    core_cells: &[(&str, RuntimeName)],
) {
    let bytes = output.as_bytes();
    let identifier = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$');
    let mut raw = raw.to_vec();
    raw.sort_by_key(|range| range.start);
    let mut raw = raw.into_iter().peekable();
    let mut missing = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if let Some(range) = raw.next_if(|range| range.start <= at) {
            at = at.max(range.end);
            continue;
        }
        let start = at;
        if bytes[at] == b'"' {
            at += 1;
            while at < bytes.len() {
                let byte = bytes[at];
                at += if byte == b'\\' { 2 } else { 1 };
                if byte == b'"' {
                    break;
                }
            }
        } else if identifier(bytes[at]) {
            let end = raw.peek().map_or(bytes.len(), |range| range.start);
            while at < end && identifier(bytes[at]) {
                at += 1;
            }
            let token = &output[start..at];
            if start > 0 && bytes[start - 1] == b'.' {
                continue;
            }
            if let Some(binding) = token.strip_prefix("rt$") {
                if !roots.contains_direct(binding) {
                    missing.push(token.to_owned());
                }
                continue;
            }
            if let Some(name) = token.strip_prefix("core$") {
                if !core_cells.iter().any(|(cell, _)| *cell == name) {
                    missing.push(token.to_owned());
                }
                continue;
            }
            if token != "rt" {
                continue;
            }
            let mut parts = Vec::new();
            while parts.len() < 2 && bytes.get(at) == Some(&b'.') {
                let part = at + 1;
                let mut part_end = part;
                while part_end < end && identifier(bytes[part_end]) {
                    part_end += 1;
                }
                if part_end == part {
                    break;
                }
                parts.push(&output[part..part_end]);
                at = part_end;
            }
            let recorded = match parts[..] {
                [export] => roots.contains(export, None),
                [export, member] => roots.contains(export, Some(member)),
                _ => false,
            };
            if !recorded {
                missing.push(format!("rt.{}", parts.join(".")));
            }
        } else {
            at += 1;
        }
    }
    assert!(
        missing.is_empty(),
        "emitted runtime reads not recorded as runtime roots: {missing:?}"
    );
}

/// A package imported by bundler output, in first-reference order.
struct PackageImport {
    package: String,
    namespace: bool,
    exports: Vec<String>,
}

/// ASCII letters and digits are kept; every other UTF-8 byte becomes `_xx`.
/// The mapping is injective and never produces `$`, which separates the parts
/// of an import binding. No emitter local begins with `$`.
fn import_identifier_part(text: &str) -> String {
    let mut result = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() {
            result.push(byte.into());
        } else {
            write!(result, "_{byte:02x}").unwrap();
        }
    }
    result
}

fn namespace_import_binding(package: &str) -> String {
    format!("${}", import_identifier_part(package))
}

fn named_import_binding(package: &str, export: &str) -> String {
    format!(
        "${}${}",
        import_identifier_part(package),
        import_identifier_part(export)
    )
}

fn module_source(imports: &[PackageImport], expression: &str) -> String {
    let mut source = String::new();
    for import in imports {
        let specifier = quoted(&import.package);
        if import.namespace {
            writeln!(
                source,
                "import * as {} from {specifier};",
                namespace_import_binding(&import.package)
            )
            .unwrap();
        }
        if !import.exports.is_empty() {
            let names = import
                .exports
                .iter()
                .map(|export| {
                    format!(
                        "{export} as {}",
                        named_import_binding(&import.package, export)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(source, "import {{ {names} }} from {specifier};").unwrap();
        }
    }
    source.push_str("export default ");
    source.push_str(expression.trim_end());
    source.push_str(";\n");
    source
}

/// A constant of the `Advanced` constant table, compared by emitted
/// structure: sites with equal keys share one table entry. Collection
/// constants (light-cljs-218) add variants keyed by their kind, metadata and
/// the table numbers of their parts.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ConstantKey<'a> {
    /// `rt$keyword(namespace, name)`; equal parts give the interned keyword.
    Keyword {
        namespace: Option<&'a [u16]>,
        name: &'a [u16],
    },
    /// `rt$symbol(namespace, name)`, a quoted symbol without metadata.
    Symbol {
        namespace: Option<&'a [u16]>,
        name: &'a [u16],
    },
}

/// The constant table of `Advanced` application output
/// (docs/advanced-optimization.md, "Phase 1 constant table extension
/// points"). Every distinct constant the emitted code reads is declared
/// once, `const k<n> = <initializer>;`, at the top of the output closure after
/// the runtime slice, and each site reads `k<n>`. Entries are numbered and
/// declared in first-use order, and an entry's initializer is written before
/// the entry is numbered, so the constants it reads come first. Only emitted
/// sites add entries, so constants are inert for reachability.
struct ConstantTable<'a> {
    /// The declarations, one line each.
    declarations: String,
    /// Entry numbers by key.
    numbers: HashMap<ConstantKey<'a>, usize>,
    /// Entry numbers by `NamedLiteralId` and kind (keyword, symbol), so a
    /// repeated site reads its number without hashing its spelling.
    named: Vec<[Option<usize>; 2]>,
}

struct Emitter<'a> {
    program: &'a Program,
    demand_roots: bool,
    application_roots: bool,
    /// Whether the output registers its namespaces with the runtime and
    /// switches `*ns*` at each namespace entry. Always below `Advanced`; at
    /// `Advanced` only for a program that reaches reflection.
    register_namespaces: bool,
    needs_host: bool,
    needs_package: bool,
    /// Bundler output binds packages with static imports instead of the registry.
    imports: Option<Vec<PackageImport>>,
    specialized_calls: Option<&'a std::collections::HashMap<ExprId, usize>>,
    /// `Some` at `Advanced`: runtime reads are direct (`rt$add`), and these
    /// replaced core operations read through output-level cells.
    replaced_core: Option<&'a [&'static str]>,
    /// The output-level cells used so far: the core runtime name and the
    /// direct read that initializes `core$<name>`.
    core_cells: Vec<(&'static str, RuntimeName)>,
    /// `Some` at `Advanced`: literal keywords and symbols read table constants.
    constants: Option<ConstantTable<'a>>,
    output: String,
    indent: usize,
    captures: Vec<(LocalId, RecurTargetId)>,
    reify_captures: Vec<(LocalId, RecurTargetId)>,
    reify_receivers: Vec<RecurTargetId>,
    capture_resets: Vec<RecurTargetId>,
    field_resets: Vec<RecurTargetId>,
    used: HashSet<ExprId>,
    // Upstream return-context def exits before its following test-property write.
    returned_definition: Option<ExprId>,
    raw_segments: Vec<std::ops::Range<usize>>,
    /// Every runtime name written so far, the roots of an `Advanced` slice.
    runtime: RuntimeRoots,
    /// The innermost expression being emitted, to which runtime reads are attributed.
    site: Option<ExprId>,
    /// At `Advanced`: the program's inferred types (`crate::inference`).
    inference: Option<&'a Inference>,
    /// Whether the output calls the prelude's `truth`.
    truth_used: bool,
}

impl<'a> Emitter<'a> {
    /// Write a runtime read (`rt.keyword`, `rt.core.add`; `rt$keyword`,
    /// `rt$add` at `Advanced`) and record it as a root of the runtime slice.
    /// All emitted runtime reads go through here.
    fn rt(&mut self, name: RuntimeName) {
        let text = self.runtime_read(name, self.site, NameUse::Value);
        self.output.push_str(&text);
    }

    /// The spelling of a runtime read attributed to `site`, recorded as a root
    /// with its use. At `Advanced` a core operation resolves through the core
    /// table: to a cell of `coreCells`, to the output-level cell `core$<name>`
    /// when the program may replace the operation, or else to the binding (or
    /// member of one) holding it.
    fn runtime_read(&mut self, name: RuntimeName, site: Option<ExprId>, used: NameUse) -> String {
        if self.replaced_core.is_none() {
            self.runtime.record(name, site, used);
            return name.to_string();
        }
        let resolved = match (name.binding, name.member) {
            (CORE_HUB, Some(member)) => match core_reference(member) {
                CoreReference::Cell(cell) => RuntimeName::direct(CORE_CELLS, Some(cell)),
                reference => {
                    let target = match reference {
                        CoreReference::Binding(binding) => RuntimeName::direct(binding, None),
                        CoreReference::Member(binding, member) => {
                            RuntimeName::direct(binding, Some(member))
                        }
                        CoreReference::Cell(_) => unreachable!(),
                    };
                    if self.replaced(member) {
                        self.runtime.demand(target, site);
                        if !self.core_cells.iter().any(|(name, _)| *name == member) {
                            self.core_cells.push((member, target));
                        }
                        return format!("core${member}");
                    }
                    target
                }
            },
            (binding, member) => RuntimeName::direct(binding, member),
        };
        self.runtime.record(resolved, site, used);
        resolved.to_string()
    }

    /// Whether the program may replace the core operation with this runtime name,
    /// so that `Advanced` output reads it through an output-level cell.
    fn replaced(&self, name: &str) -> bool {
        self.replaced_core
            .is_some_and(|replaced| replaced.binary_search(&name).is_ok())
    }

    /// Whether a call of this core operation may be a plain call of its unit
    /// binding, `rt$add(1, 2)`: at `Advanced`, for an operation held by a
    /// binding of its own that the program never replaces and whose value is
    /// a function (the `value` column of src/core_operations.tsv; `INIT` is
    /// not). Members and cells keep `.call(null, ...)`.
    fn known_unit(&self, core: crate::CoreFnId) -> bool {
        self.replaced_core.is_some()
            && core.operation().value == crate::core_operations::Type::FN
            && matches!(
                core_reference(core.runtime_name()),
                CoreReference::Binding(_)
            )
            && !self.replaced(core.runtime_name())
    }

    /// The spelling a keyword `case` switches on, or `otherwise` for a value
    /// that is not a keyword: `rt.core.isKeyword(caseValue) ?
    /// rt.core.str(caseValue).slice(1) : otherwise`, and at `Advanced`
    /// `rt$keywordFqn(caseValue)`, which is null for anything else.
    fn keyword_case_name(&mut self, otherwise: &str) {
        if self.constants.is_none() {
            self.rt(RuntimeName::core("isKeyword"));
            self.output.push_str("(caseValue) ? ");
            self.rt(RuntimeName::core("str"));
            write!(self.output, "(caseValue).slice(1) : {otherwise}").unwrap();
        } else if otherwise == "null" {
            self.rt(RuntimeName::export("keywordFqn"));
            self.output.push_str("(caseValue)");
        } else {
            self.output.push('(');
            self.rt(RuntimeName::export("keywordFqn"));
            write!(self.output, "(caseValue) ?? {otherwise})").unwrap();
        }
    }

    /// Write a reference to the table constant for `key`, `k<n>`, declaring
    /// it on first use: `initializer` writes the value expression, reading
    /// the runtime through [`Emitter::rt`] as usual (the first site is the
    /// one the retention report names). `Advanced` only.
    fn constant(&mut self, key: ConstantKey<'a>, initializer: impl FnOnce(&mut Self)) -> usize {
        let constants = self
            .constants
            .as_ref()
            .expect("constants are hoisted at Advanced");
        let number = match constants.numbers.get(&key) {
            Some(&number) => number,
            None => {
                let site = std::mem::take(&mut self.output);
                let raw = self.raw_segments.len();
                initializer(self);
                debug_assert_eq!(
                    self.raw_segments.len(),
                    raw,
                    "a constant holds raw JavaScript"
                );
                let value = std::mem::replace(&mut self.output, site);
                let constants = self.constants.as_mut().unwrap();
                let number = constants.numbers.len();
                writeln!(constants.declarations, "  const k{number} = {value};").unwrap();
                constants.numbers.insert(key, number);
                number
            }
        };
        write!(self.output, "k{number}").unwrap();
        number
    }

    /// A keyword or quoted symbol literal: a table constant at `Advanced`,
    /// else `rt.keyword(namespace, name)` or `rt.symbol(namespace, name)`.
    fn named_literal(&mut self, literal: NamedLiteralId, symbol: bool) {
        let program = self.program;
        let construct = |emitter: &mut Self| {
            let name = &program.named_literals[literal.0];
            let constructor = if symbol { "symbol" } else { "keyword" };
            emitter.rt(RuntimeName::export(constructor));
            emitter.output.push('(');
            if let Some(namespace) = name.namespace {
                emitter.string(namespace);
            } else {
                emitter.output.push_str("null");
            }
            emitter.output.push_str(", ");
            emitter.string(name.name);
            emitter.output.push(')');
        };
        let Some(constants) = &self.constants else {
            return construct(self);
        };
        if let Some(number) = constants.named[literal.0][usize::from(symbol)] {
            write!(self.output, "k{number}").unwrap();
            return;
        }
        let name = &program.named_literals[literal.0];
        let namespace = name.namespace.map(|id| &program.strings[id.0][..]);
        let name = &program.strings[name.name.0][..];
        let key = if symbol {
            ConstantKey::Symbol { namespace, name }
        } else {
            ConstantKey::Keyword { namespace, name }
        };
        let number = self.constant(key, construct);
        self.constants.as_mut().unwrap().named[literal.0][usize::from(symbol)] = Some(number);
    }

    fn indentation(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("  ");
        }
    }

    fn local(&mut self, id: LocalId) {
        write!(self.output, "l{}", id.0).unwrap();
    }

    fn local_value(&mut self, id: LocalId) {
        if let Some((_, target)) = self.captures.iter().rev().find(|(local, _)| *local == id) {
            write!(self.output, "captures{}.l{}", target.0, id.0).unwrap();
        } else if let Some(&(_, target)) = self
            .reify_captures
            .iter()
            .rev()
            .find(|(local, _)| *local == id)
        {
            self.rt(RuntimeName::export("field"));
            write!(self.output, "(self{}, ", target.0).unwrap();
            self.string_units(
                &self.program.locals[id.0]
                    .name
                    .encode_utf16()
                    .collect::<Vec<_>>(),
            );
            self.output.push(')');
        } else if let Some((target, _)) = self.program.field_bindings.get(&id) {
            write!(self.output, "self{}", target.0).unwrap();
            self.member(&crate::analyzer::munge_js_member(
                &self.program.locals[id.0].name,
            ));
        } else {
            self.local(id);
        }
    }

    fn global(&mut self, id: GlobalId) {
        let name = &self.program.globals[id.0].name;
        if name.contains('.') {
            self.needs_host = true;
            let path = crate::analyzer::munge_js_path(name);
            self.output.push_str("host(");
            self.string_units(&path[0].encode_utf16().collect::<Vec<_>>());
            self.output.push(')');
            for member in &path[1..] {
                self.member(member);
            }
        } else if self.application_roots {
            self.output.push_str("globalThis.__lightApplicationRoots[");
            let global = &self.program.globals[id.0];
            self.string_units(
                &format!("{}/{}", global.namespace, global.name)
                    .encode_utf16()
                    .collect::<Vec<_>>(),
            );
            self.output.push(']');
        } else if self.demand_roots {
            self.output
                .push_str("globalThis[\"__lightCompilerRoots\"][");
            let global = &self.program.globals[id.0];
            self.string_units(
                &format!("{}/{}", global.namespace, global.name)
                    .encode_utf16()
                    .collect::<Vec<_>>(),
            );
            self.output.push(']');
        } else {
            write!(self.output, "g{}", id.0).unwrap();
        }
    }

    // Declaration products may be linked while the bundle is initializing.
    // Ordinary value references (including method bodies) still use demand roots.
    fn product(&mut self, id: GlobalId) {
        if self.demand_roots {
            self.output
                .push_str("globalThis.__lightCompilerBundleProducts[");
            let binding = &self.program.globals[id.0];
            self.string_units(
                &format!("{}/{}", binding.namespace, binding.name)
                    .encode_utf16()
                    .collect::<Vec<_>>(),
            );
            self.output.push(']');
        } else {
            self.global(id);
        }
    }

    fn compiler_binding(&self, id: ExprId) -> bool {
        self.demand_roots
            && matches!(self.program.exprs[id.0].kind,
            ExprKind::Global(g) if !self.program.globals[g.0].name.contains('.'))
    }

    fn binding_name(&mut self, id: ExprId) {
        let ExprKind::Global(g) = self.program.exprs[id.0].kind else {
            unreachable!()
        };
        let global = &self.program.globals[g.0];
        self.string_units(
            &format!("{}/{}", global.namespace, global.name)
                .encode_utf16()
                .collect::<Vec<_>>(),
        );
    }

    fn assignable(&mut self, id: ExprId) {
        if let ExprKind::JsGlobal(global) = self.program.exprs[id.0].kind {
            let program = self.program;
            if let Some((package, members)) = program.js_globals[global.0].package()
                && self.imports.is_some()
            {
                self.namespace_import(package);
                for member in members {
                    self.member(member);
                }
                return;
            }
            self.output.push_str("globalThis");
            for part in &self.program.js_globals[global.0].path {
                self.member(part);
            }
        } else {
            if let ExprKind::CoreFn(core) = self.program.exprs[id.0].kind {
                debug_assert!(
                    self.replaced_core.is_none()
                        || self.replaced(core.runtime_name())
                        || matches!(core_reference(core.runtime_name()), CoreReference::Cell(_)),
                    "Advanced output assigns `{}` without a cell",
                    core.runtime_name()
                );
            }
            self.value(id);
        }
    }

    /// Bundler output: reference the namespace object of `package`.
    fn namespace_import(&mut self, package: &str) {
        let imports = self.imports.as_mut().expect("bundler output");
        let index = Self::package_import(imports, package);
        imports[index].namespace = true;
        self.output.push_str(&namespace_import_binding(package));
    }

    fn package_import(imports: &mut Vec<PackageImport>, package: &str) -> usize {
        imports
            .iter()
            .position(|import| import.package == package)
            .unwrap_or_else(|| {
                imports.push(PackageImport {
                    package: package.to_owned(),
                    namespace: false,
                    exports: Vec::new(),
                });
                imports.len() - 1
            })
    }

    /// A package registry lookup. Unbundled output checks the registry at each
    /// use; bundler output names the export's import binding directly.
    fn package_reference(&mut self, package: &str, members: &[String]) {
        let Some(imports) = self.imports.as_mut() else {
            self.needs_package = true;
            self.output.push_str("pkg(");
            self.string_units(&package.encode_utf16().collect::<Vec<_>>());
            self.output.push(')');
            for member in members {
                self.member(member);
            }
            return;
        };
        let Some((export, rest)) = members.split_first() else {
            self.namespace_import(package);
            return;
        };
        let index = Self::package_import(imports, package);
        if !imports[index].exports.contains(export) {
            imports[index].exports.push(export.clone());
        }
        self.output.push_str(&named_import_binding(package, export));
        for member in rest {
            self.member(member);
        }
    }

    fn member(&mut self, name: &str) {
        self.output.push('[');
        self.string_units(&name.encode_utf16().collect::<Vec<_>>());
        self.output.push(']');
    }

    fn arguments(&mut self, args: &[ExprId]) {
        for (index, &arg) in args.iter().enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            self.value(arg);
        }
    }

    /// An `if` test: `truth(test)`, or the test itself when it is a
    /// `BooleanHint` or, at `Advanced`, when its inferred type makes JavaScript
    /// truthiness agree with Clojure's (no number, string or host value).
    fn condition(&mut self, id: ExprId) {
        if let ExprKind::BooleanHint(value) = self.program.exprs[id.0].kind {
            self.value(value);
        } else if self
            .inference
            .is_some_and(|inference| inference.value(id).truth_is_js())
        {
            self.value(id);
        } else {
            self.truth_used = true;
            self.output.push_str("truth(");
            self.value(id);
            self.output.push(')');
        }
    }

    /// At `Advanced`, the operands of `(and a b)` or `(or a b)` that emit as
    /// `(a && b)` or `(a || b)`, and whether it is `and`: the expansion
    /// `(let [t a] (if t b t))` returns the same value when `a`'s type makes
    /// JavaScript truthiness agree with Clojure's and `t` is read only there
    /// (upstream's `and`/`or` pass). In value position the expansion's function
    /// scope goes away, so neither operand may read the enclosing `arguments`
    /// or contain raw JavaScript.
    fn short_circuit(&self, id: ExprId, value: bool) -> Option<(ExprId, ExprId, bool)> {
        let inference = self.inference?;
        let ExprKind::Let { bindings, body } = &self.program.exprs[id.0].kind else {
            return None;
        };
        let (temporary, init, other, and) =
            crate::inference::short_circuit(self.program, bindings, *body)?;
        if inference.uses(temporary) != 2 || !inference.value(init).truth_is_js() {
            return None;
        }
        if value
            && [init, other]
                .into_iter()
                .any(|id| reads_function_scope(self.program, id))
        {
            return None;
        }
        Some((init, other, and))
    }

    fn short_circuit_value(&mut self, (init, other, and): (ExprId, ExprId, bool)) {
        self.output.push('(');
        self.value(init);
        self.output.push_str(if and { " && " } else { " || " });
        self.value(other);
        self.output.push(')');
    }

    /// `(a op b op ...)`.
    fn infix(&mut self, operator: &str, args: &[ExprId]) {
        self.output.push('(');
        for (index, &arg) in args.iter().enumerate() {
            if index > 0 {
                self.output.push_str(operator);
            }
            self.value(arg);
        }
        self.output.push(')');
    }

    /// `(prefix value suffix)`.
    fn wrapped(&mut self, prefix: &str, value: ExprId, suffix: &str) {
        self.output.push('(');
        self.output.push_str(prefix);
        self.value(value);
        self.output.push_str(suffix);
        self.output.push(')');
    }

    /// Whether reading `id` twice is reading it once: a literal or a local
    /// other than a mutable type field.
    fn repeatable(&self, id: ExprId) -> bool {
        match self.program.exprs[id.0].kind {
            ExprKind::Local(local) => !self.program.field_bindings.contains_key(&local),
            ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_) => true,
            _ => false,
        }
    }

    /// At `Advanced`, a call that emission writes as JavaScript operators or a
    /// field read instead of calling the runtime (docs/advanced-optimization.md,
    /// "Phase 2 type inference extension points"); whether it wrote one.
    fn lowered_call(&mut self, callee: ExprId, args: &[ExprId]) -> bool {
        let Some(inference) = self.inference else {
            return false;
        };
        match self.program.exprs[callee.0].kind {
            ExprKind::Keyword(literal) => match args {
                [receiver] => self.field_read(inference, *receiver, literal),
                _ => false,
            },
            ExprKind::CoreFn(core) if !self.replaced(core.runtime_name()) => {
                self.lowered_core_call(inference, core, args)
            }
            _ => false,
        }
    }

    fn lowered_core_call(
        &mut self,
        inference: &Inference,
        core: crate::CoreFnId,
        args: &[ExprId],
    ) -> bool {
        use crate::CoreFnId as C;
        // Operations whose runtime definition is a JavaScript operator on its
        // arguments (src/js_runtime.js): a call with these many arguments is
        // that operator, whatever the argument types.
        let comparison = match core {
            C::Less => Some(" < "),
            C::LessEqual => Some(" <= "),
            C::Greater => Some(" > "),
            C::GreaterEqual => Some(" >= "),
            _ => None,
        };
        match (core, args) {
            (C::Add, []) => self.output.push('0'),
            (C::Multiply, []) => self.output.push('1'),
            (C::Add | C::Multiply, [value]) => self.value(*value),
            (C::Add, _) => self.infix(" + ", args),
            (C::Multiply, _) => self.infix(" * ", args),
            (C::Subtract, [value]) => self.wrapped("- ", *value, ""),
            (C::Subtract, [_, _, ..]) => self.infix(" - ", args),
            (C::Divide, [value]) => self.wrapped("1 / ", *value, ""),
            (C::Divide, [_, _, ..]) => self.infix(" / ", args),
            (C::Inc, [value]) => self.wrapped("", *value, " + 1"),
            (C::Dec, [value]) => self.wrapped("", *value, " - 1"),
            (_, [value]) if comparison.is_some() => self.wrapped("", *value, ", true"),
            (_, [_, _]) if comparison.is_some() => self.infix(comparison.unwrap(), args),
            // A chain compares neighbors, short-circuiting like the runtime's
            // loop, which reads each argument once.
            (_, [_, _, _, ..])
                if comparison.is_some() && args.iter().all(|arg| self.repeatable(*arg)) =>
            {
                self.output.push('(');
                for (index, pair) in args.windows(2).enumerate() {
                    if index > 0 {
                        self.output.push_str(" && ");
                    }
                    self.value(pair[0]);
                    self.output.push_str(comparison.unwrap());
                    self.value(pair[1]);
                }
                self.output.push(')');
            }
            (C::Identical, [_, _]) => self.infix(" === ", args),
            (C::IsNil, [value]) => self.wrapped("", *value, " == null"),
            (C::IsSome, [value]) => self.wrapped("", *value, " != null"),
            (C::IsZero, [value]) => self.wrapped("", *value, " === 0"),
            (C::IsPositive, [value]) => self.wrapped("", *value, " > 0"),
            (C::IsNegative, [value]) => self.wrapped("", *value, " < 0"),
            // Typed lowerings. `not` is `x == null || x === false`.
            (C::Not, [value]) if inference.value(*value).truth_is_js() => {
                self.wrapped("!", *value, "")
            }
            (C::Equal | C::NotEqual, [left, right]) => {
                return self.equality(inference, *left, *right, core == C::NotEqual);
            }
            (C::NumericEqual, [_, _])
                if inference.facts().core_protocols_intact()
                    && args
                        .iter()
                        .all(|arg| Type::NUMBER.contains(inference.ty(*arg))) =>
            {
                self.infix(" === ", args)
            }
            (C::Count, [value]) => return self.count(inference, *value),
            (C::Nth, [vector, index] | [vector, index, _]) => {
                return self.nth(inference, *vector, *index, args.get(2).copied());
            }
            (C::Get, [receiver, key]) => match self.program.exprs[key.0].kind {
                ExprKind::Keyword(literal) => {
                    return self.field_read(inference, *receiver, literal);
                }
                _ => return false,
            },
            _ => return false,
        }
        true
    }

    /// `(= a b)` (or `not=`): `===` when a primitive (boolean, number or
    /// string) decides it, and for a keyword literal the lowering that 208's
    /// constant table documents, `x === k<n> || rt$keywordFqn(x) === "k"`
    /// (or only the spelling test when `x` cannot be read twice). Both need the
    /// core protocols intact, since `=` dispatches `IEquiv` on its first
    /// operand; with the primitive or literal second, the first must not be an
    /// instance of a program type when the program implements `IEquiv`.
    fn equality(&mut self, inference: &Inference, left: ExprId, right: ExprId, not: bool) -> bool {
        let facts = inference.facts();
        if !facts.core_protocols_intact() {
            return false;
        }
        let primitive = Type::BOOLEAN.union(Type::NUMBER).union(Type::STRING);
        let program_types = Type::RECORD.union(Type::OBJECT);
        let (left_type, right_type) = (inference.ty(left), inference.ty(right));
        let plain_left =
            !facts.implements_equiv() || left_type.intersection(program_types).is_never();
        if primitive.contains(left_type) || primitive.contains(right_type) && plain_left {
            self.infix(if not { " !== " } else { " === " }, &[left, right]);
            return true;
        }
        let literal = |id: ExprId| match self.program.exprs[id.0].kind {
            ExprKind::Keyword(literal) => Some(literal),
            _ => None,
        };
        let (value, literal) = match (literal(left), literal(right)) {
            (_, Some(literal)) if plain_left => (left, literal),
            (Some(literal), _) => (right, literal),
            _ => return false,
        };
        let name = &self.program.named_literals[literal.0];
        let mut spelling = Vec::new();
        if let Some(namespace) = name.namespace {
            spelling.extend_from_slice(&self.program.strings[namespace.0]);
            spelling.push('/' as u16);
        }
        spelling.extend_from_slice(&self.program.strings[name.name.0]);
        if not {
            self.output.push('!');
        }
        self.output.push('(');
        if self.repeatable(value) {
            self.value(value);
            self.output.push_str(" === ");
            self.named_literal(literal, false);
            self.output.push_str(" || ");
        }
        self.rt(RuntimeName::export("keywordFqn"));
        self.output.push('(');
        self.value(value);
        self.output.push_str(") === ");
        self.string_units(&spelling);
        self.output.push(')');
        true
    }

    /// `(count x)`: `.length` of a string or JavaScript array, and of a flat
    /// vector's payload.
    fn count(&mut self, inference: &Inference, value: ExprId) -> bool {
        if !inference.facts().core_protocols_intact() {
            return false;
        }
        let inferred = inference.value(value);
        if Type::STRING.contains(inferred.ty)
            || inferred.known == Known::Array && Type::HOST.contains(inferred.ty)
        {
            self.output.push('(');
            self.value(value);
            self.output.push_str(").length");
            return true;
        }
        if inferred.known == Known::Vector && Type::VECTOR.contains(inferred.ty) {
            self.rt(RuntimeName::export("vectors"));
            self.output.push_str(".get(");
            self.value(value);
            self.output.push_str(").length");
            return true;
        }
        false
    }

    /// `(nth v i)` and `(nth v i not-found)` on a flat vector with a number
    /// index: the runtime's direct index path, `vectorNth` on the payload.
    fn nth(
        &mut self,
        inference: &Inference,
        vector: ExprId,
        index: ExprId,
        not_found: Option<ExprId>,
    ) -> bool {
        let inferred = inference.value(vector);
        if !inference.facts().core_protocols_intact()
            || inferred.known != Known::Vector
            || !Type::VECTOR.contains(inferred.ty)
            || !Type::NUMBER.contains(inference.ty(index))
        {
            return false;
        }
        self.rt(RuntimeName::export("vectorNth"));
        self.output.push('(');
        self.rt(RuntimeName::export("vectors"));
        self.output.push_str(".get(");
        self.value(vector);
        self.output.push_str("), ");
        self.value(index);
        self.output.push_str(", ");
        match not_found {
            Some(not_found) => self.value(not_found),
            None => self.rt(RuntimeName::export("missing")),
        }
        self.output.push(')');
        true
    }

    /// `(:k rec)` or `(get rec :k)` on a record of a known program type with a
    /// field `k`: the field's property, which the record's `ILookup` reads.
    fn field_read(
        &mut self,
        inference: &Inference,
        receiver: ExprId,
        literal: NamedLiteralId,
    ) -> bool {
        let inferred = inference.value(receiver);
        let facts = inference.facts();
        let Known::Instance(global) = inferred.known else {
            return false;
        };
        if !Type::RECORD.contains(inferred.ty)
            || !facts.core_protocols_intact()
            || facts.implements_lookup(global)
        {
            return false;
        }
        let Some((definition, true)) = facts.program_type(global) else {
            return false;
        };
        let program = self.program;
        let ExprKind::DefType { fields, .. } = &program.exprs[definition.0].kind else {
            return false;
        };
        let name = &program.named_literals[literal.0];
        let Ok(spelling) = String::from_utf16(&program.strings[name.name.0]) else {
            return false;
        };
        if name.namespace.is_some() || !fields.contains(&spelling) {
            return false;
        }
        self.output.push('(');
        self.value(receiver);
        self.output.push(')');
        self.member(&crate::analyzer::munge_js_member(&spelling));
        true
    }

    fn string(&mut self, id: StringId) {
        self.string_units(&self.program.strings[id.0]);
    }

    fn string_units(&mut self, units: &[u16]) {
        self.output.push('"');
        for &unit in units {
            match unit {
                0x22 => self.output.push_str("\\\""),
                0x5c => self.output.push_str("\\\\"),
                0x20..=0x7e => self.output.push(char::from_u32(unit.into()).unwrap()),
                _ => write!(self.output, "\\u{unit:04x}").unwrap(),
            }
        }
        self.output.push('"');
    }

    fn emit(&mut self, id: ExprId, context: ExprContext) {
        let site = self.site.replace(id);
        self.emit_expr(id, context);
        self.site = site;
    }

    fn emit_expr(&mut self, id: ExprId, context: ExprContext) {
        self.used.insert(id);
        if context == ExprContext::Value {
            self.value(id);
            return;
        }
        if self.register_namespaces
            && let Some(namespace) = self.program.namespace_entries.get(&id)
        {
            self.indentation();
            self.rt(RuntimeName::core("currentNs"));
            self.output.push_str(" = ");
            self.rt(RuntimeName::core("theNs"));
            self.output.push('(');
            self.rt(RuntimeName::export("symbol"));
            writeln!(self.output, "(null, {}));", js_string(namespace)).unwrap();
        }
        // A discarded `nil`, such as a definition application preparation
        // removed, is no statement at `Advanced`.
        if context == ExprContext::Discard
            && self.replaced_core.is_some()
            && matches!(self.program.exprs[id.0].kind, ExprKind::Nil)
        {
            return;
        }
        if let Some(parts) = self.short_circuit(id, false) {
            self.indentation();
            if context == ExprContext::Return {
                self.output.push_str("return ");
            }
            self.short_circuit_value(parts);
            self.output.push_str(";\n");
            return;
        }
        match &self.program.exprs[id.0].kind {
            ExprKind::JsTemplate { segments, args } => {
                self.indentation();
                if context == ExprContext::Return {
                    self.output.push_str("return ");
                }
                self.js_template(segments, args);
                self.output.push_str(";\n");
            }
            ExprKind::WithOutStr(body) => {
                self.indentation();
                let (print_fn, print_newline) = (
                    RuntimeName::core("printFn"),
                    RuntimeName::core("printNewline"),
                );
                self.output
                    .push_str("{ let text = \"\"; const savedPrint = ");
                self.rt(print_fn);
                self.output.push_str(", savedNewline = ");
                self.rt(print_newline);
                self.output.push_str(";\n");
                self.indent += 1;
                self.indentation();
                self.rt(print_fn);
                self.output
                    .push_str(" = value => { text += value == null ? \"\" : value; }; ");
                self.rt(print_newline);
                self.output.push_str(" = true;\n");
                self.indentation();
                self.output.push_str("try {\n");
                self.indent += 1;
                self.emit(*body, ExprContext::Discard);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("} finally { ");
                self.rt(print_fn);
                self.output.push_str(" = savedPrint; ");
                self.rt(print_newline);
                self.output.push_str(" = savedNewline; }\n");
                if context == ExprContext::Return {
                    self.indentation();
                    self.output.push_str("return text;\n");
                }
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Binding { bindings, body } => {
                self.indentation();
                self.output.push_str("{\n");
                self.indent += 1;
                // Compiler scope saves overlays without demanding the root.
                // Every initializer still sees the outer environment.
                for (i, (target, _)) in bindings.iter().enumerate() {
                    self.indentation();
                    write!(self.output, "const saved{i} = ").unwrap();
                    if self.compiler_binding(*target) {
                        self.output
                            .push_str("globalThis.__lightSaveCompilerBinding(");
                        self.binding_name(*target);
                        self.output.push(')');
                    } else {
                        self.value(*target);
                    }
                    writeln!(self.output, "; let installed{i} = false;").unwrap();
                }
                for (i, (_, init)) in bindings.iter().enumerate() {
                    self.indentation();
                    write!(self.output, "const temporary{i} = ").unwrap();
                    self.value(*init);
                    self.output.push_str(";\n");
                }
                self.indentation();
                self.output.push_str("try {\n");
                self.indent += 1;
                for (i, (target, _)) in bindings.iter().enumerate() {
                    self.indentation();
                    if self.compiler_binding(*target) {
                        self.output.push_str("globalThis.__lightBindCompilerRoot(");
                        self.binding_name(*target);
                        writeln!(self.output, ", temporary{i});").unwrap();
                    } else {
                        self.assignable(*target);
                        writeln!(self.output, " = temporary{i};").unwrap();
                    }
                    self.indentation();
                    writeln!(self.output, "installed{i} = true;").unwrap();
                }
                self.emit(*body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("} finally {\n");
                self.indent += 1;
                for (i, (target, _)) in bindings.iter().enumerate().rev() {
                    self.indentation();
                    write!(self.output, "if (installed{i}) ").unwrap();
                    if self.compiler_binding(*target) {
                        self.output
                            .push_str("globalThis.__lightRestoreCompilerBinding(");
                        self.binding_name(*target);
                        writeln!(self.output, ", saved{i});").unwrap();
                    } else {
                        self.assignable(*target);
                        writeln!(self.output, " = saved{i};").unwrap();
                    }
                }
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Loop {
                target,
                bindings,
                body,
            } => {
                self.indentation();
                self.output.push_str("{\n");
                self.indent += 1;
                // Initializers see preceding lexical bindings, before the fresh
                // per-iteration copies come into scope.
                for &(local, init) in bindings {
                    self.indentation();
                    self.output.push_str("const ");
                    self.local(local);
                    self.output.push_str(" = ");
                    self.value(init);
                    self.output.push_str(";\n");
                }
                self.iteration(*target, *body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Recur { target, args } => {
                self.indentation();
                self.output.push_str("{\n");
                self.indent += 1;
                for (index, &arg) in args.iter().enumerate() {
                    self.indentation();
                    write!(self.output, "const next{index} = ").unwrap();
                    self.value(arg);
                    self.output.push_str(";\n");
                }
                for index in 0..args.len() {
                    self.indentation();
                    writeln!(self.output, "r{}_{index} = next{index};", target.0).unwrap();
                }
                // Upstream emits non-strict formal parameters: its arguments
                // object aliases supplied parameter slots. Preserve this even
                // though loop iteration locals are copied for closure capture.
                for (index, slot) in self.program.recur_targets[target.0]
                    .argument_slots
                    .iter()
                    .enumerate()
                {
                    if let Some(slot) = slot {
                        self.indentation();
                        writeln!(
                            self.output,
                            "if (arguments.length > {slot}) arguments[{slot}] = next{index};"
                        )
                        .unwrap();
                    }
                }
                if self.field_resets.contains(target) {
                    self.indentation();
                    writeln!(self.output, "self{} = null;", target.0).unwrap();
                }
                if self.capture_resets.contains(target) {
                    self.indentation();
                    writeln!(self.output, "captures{} = null;", target.0).unwrap();
                }
                self.indentation();
                writeln!(self.output, "continue recur{};", target.0).unwrap();
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Throw(value) => {
                self.indentation();
                self.output.push_str("throw ");
                self.value(*value);
                self.output.push_str(";\n");
            }
            ExprKind::Try {
                body,
                catches,
                finally,
            } => {
                if catches.is_empty() && finally.is_none() {
                    self.emit(*body, context);
                    return;
                }
                self.indentation();
                self.output.push_str("try {\n");
                self.indent += 1;
                self.emit(*body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push('}');
                if !catches.is_empty() {
                    self.output.push_str(" catch (caught) {\n");
                    self.indent += 1;
                    for (index, catch) in catches.iter().enumerate() {
                        self.indentation();
                        self.output
                            .push_str(if index == 0 { "if (" } else { "else if (" });
                        if let Some(class) = catch.class {
                            self.output.push_str("caught instanceof ");
                            self.value(class);
                        } else {
                            self.output.push_str("true");
                        }
                        self.output.push_str(") {\n");
                        self.indent += 1;
                        self.indentation();
                        self.output.push_str("const ");
                        self.local(catch.binding);
                        self.output.push_str(" = caught;\n");
                        self.emit(catch.body, context);
                        self.indent -= 1;
                        self.indentation();
                        self.output.push_str("}\n");
                    }
                    self.indentation();
                    self.output.push_str("else { throw caught; }\n");
                    self.indent -= 1;
                    self.indentation();
                    self.output.push('}');
                }
                if let Some(finally) = finally {
                    self.output.push_str(" finally {\n");
                    self.indent += 1;
                    self.emit(*finally, ExprContext::Discard);
                    self.indent -= 1;
                    self.indentation();
                    self.output.push('}');
                }
                self.output.push('\n');
            }
            ExprKind::Case {
                strict,
                keyword_tests,
                test,
                branches,
                default,
            } => {
                self.indentation();
                self.output.push_str("{ const caseValue = ");
                self.value(*test);
                self.output.push_str(";\n");
                self.indent += 1;
                let switch = *strict || *keyword_tests;
                if switch {
                    self.indentation();
                    if *keyword_tests {
                        self.output.push_str("switch (");
                        self.keyword_case_name("null");
                        self.output.push_str(") {\n");
                    } else {
                        self.output.push_str("switch (caseValue) {\n");
                    }
                    self.indent += 1;
                }
                for (index, &(constant, branch)) in branches.iter().enumerate() {
                    self.indentation();
                    if switch {
                        self.output.push_str("case ");
                        if *keyword_tests {
                            let ExprKind::Keyword(name) = self.program.exprs[constant.0].kind
                            else {
                                unreachable!()
                            };
                            let name = &self.program.named_literals[name.0];
                            let mut spelling = Vec::new();
                            if let Some(namespace) = name.namespace {
                                spelling.extend_from_slice(&self.program.strings[namespace.0]);
                                spelling.push('/' as u16);
                            }
                            spelling.extend_from_slice(&self.program.strings[name.name.0]);
                            self.string_units(&spelling);
                        } else {
                            self.value(constant);
                        }
                        self.output.push_str(": {\n");
                    } else {
                        self.output
                            .push_str(if index == 0 { "if (" } else { "else if (" });
                        self.rt(RuntimeName::core("equal"));
                        self.output.push_str("(caseValue, ");
                        self.value(constant);
                        self.output.push_str(")) {\n");
                    }
                    self.indent += 1;
                    self.emit(branch, context);
                    if switch {
                        self.indentation();
                        self.output.push_str("break;\n");
                    }
                    self.indent -= 1;
                    self.indentation();
                    self.output.push_str("}\n");
                }
                self.indentation();
                self.output.push_str(if switch {
                    "default: {\n"
                } else if branches.is_empty() {
                    "{\n"
                } else {
                    "else {\n"
                });
                self.indent += 1;
                if let Some(default) = default {
                    self.emit(*default, context);
                } else {
                    self.indentation();
                    self.output
                        .push_str("throw new Error(\"No matching clause: \" + ");
                    if *keyword_tests {
                        self.output.push('(');
                        self.keyword_case_name("\"\"");
                        self.output.push_str("));\n");
                    } else {
                        self.rt(RuntimeName::export("displayStr"));
                        self.output.push_str("(caseValue));\n");
                    }
                }
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
                if switch {
                    self.indent -= 1;
                    self.indentation();
                    self.output.push_str("}\n");
                }
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::If { test, yes, no } => {
                self.indentation();
                self.output.push_str("if (");
                self.condition(*test);
                self.output.push_str(") {\n");
                self.indent += 1;
                self.emit(*yes, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("} else {\n");
                self.indent += 1;
                self.emit(*no, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Do(exprs) => {
                if exprs.is_empty() && context == ExprContext::Return {
                    self.indentation();
                    self.output.push_str("return null;\n");
                }
                for (index, &expr) in exprs.iter().enumerate() {
                    self.emit(
                        expr,
                        if index + 1 == exprs.len() {
                            context
                        } else {
                            ExprContext::Discard
                        },
                    );
                }
            }
            ExprKind::ThisAs { binding, body } => {
                self.indentation();
                self.output.push_str("{ const ");
                self.local(*binding);
                self.output
                    .push_str(" = (this == null ? globalThis : Object(this));\n");
                self.indent += 1;
                self.emit(*body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Let { bindings, body } => {
                self.indentation();
                self.output.push_str("{\n");
                self.indent += 1;
                for &(local, init) in bindings {
                    self.indentation();
                    self.output.push_str("const ");
                    self.local(local);
                    self.output.push_str(" = ");
                    self.value(init);
                    self.output.push_str(";\n");
                }
                self.emit(*body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::LetFn { bindings, body } => {
                self.indentation();
                self.output.push_str("{\n");
                self.indent += 1;
                // Each entry into the block creates fresh slots, including
                // within recur iterations. Closures capture these slots, so
                // every peer is visible regardless of declaration order.
                for &(local, _) in bindings {
                    self.indentation();
                    self.output.push_str("let ");
                    self.local(local);
                    self.output.push_str(";\n");
                }
                for &(local, init) in bindings {
                    self.indentation();
                    self.local(local);
                    self.output.push_str(" = ");
                    self.value(init);
                    self.output.push_str(";\n");
                }
                self.emit(*body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            _ => {
                self.indentation();
                if context == ExprContext::Return {
                    self.output.push_str("return ");
                }
                let previous = self.returned_definition;
                self.returned_definition = (context == ExprContext::Return).then_some(id);
                self.value(id);
                self.returned_definition = previous;
                self.output.push_str(";\n");
            }
        }
    }

    fn iteration(&mut self, target: RecurTargetId, body: ExprId, context: ExprContext) {
        let frame = &self.program.recur_targets[target.0];
        for (index, &param) in frame.params.iter().enumerate() {
            self.indentation();
            write!(self.output, "let r{}_{index} = ", target.0).unwrap();
            self.local(param);
            self.output.push_str(";\n");
        }
        self.indentation();
        writeln!(self.output, "recur{}: while (true) {{", target.0).unwrap();
        self.indent += 1;
        for (index, &param) in frame.params.iter().enumerate() {
            self.indentation();
            self.output.push_str("const ");
            self.local(param);
            if frame.receiver_from_this && index == 0 {
                self.output.push_str(" = this;\n");
            } else {
                writeln!(self.output, " = r{}_{index};", target.0).unwrap();
            }
        }
        self.emit(body, context);
        if context == ExprContext::Discard {
            self.indentation();
            self.output.push_str("break;\n");
        }
        self.indent -= 1;
        self.indentation();
        self.output.push_str("}\n");
    }

    fn method_with_ifn_captures(
        &mut self,
        name: Option<LocalId>,
        method: &FnMethod,
        ifn_call: bool,
    ) {
        self.output.push_str("(function");
        if let Some(name) = name {
            self.output.push(' ');
            self.local(name);
        }
        self.output.push('(');
        for (index, param) in method.params.iter().chain(method.rest.iter()).enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            self.local(*param);
        }
        self.output.push_str(") {\n");
        self.indent += 1;
        let field_reset = ifn_call
            && self.program.type_methods.contains(&method.target)
            && self.program.recur_targets[method.target.0].used;
        if field_reset {
            self.field_resets.push(method.target);
        }
        if self.reify_receivers.contains(&method.target)
            || self.program.type_methods.contains(&method.target)
        {
            self.indentation();
            writeln!(
                self.output,
                "{} self{} = this;",
                if field_reset { "let" } else { "const" },
                method.target.0
            )
            .unwrap();
            if let Some(receiver) = method.params.first() {
                self.indentation();
                self.local(*receiver);
                self.output.push_str(" = this;\n");
            }
        }
        if self.program.recur_targets[method.target.0].receiver_from_this
            && !self.reify_receivers.contains(&method.target)
            && !self.program.type_methods.contains(&method.target)
            && let Some(receiver) = method.params.first()
        {
            // Ordinary extensions rebind the explicit receiver without gaining
            // the hidden field receiver reserved for annotated type methods.
            self.indentation();
            self.local(*receiver);
            self.output.push_str(" = this;\n");
        }
        let previous_captures = self.captures.len();
        // In the pinned CLJS IFn .call adapter, the implicit receiver parameter
        // aliases the hidden field-access receiver and recur sets it to nil.
        // Captured outer-local reads then throw; the direct invoke entry keeps
        // its receiver. Preserve this observable distinction, including reads
        // delayed inside a returned closure.
        let capture_reset = ifn_call
            && self.program.recur_targets[method.target.0].used
            && !method.captures.is_empty();
        if capture_reset {
            self.indentation();
            write!(self.output, "let captures{} = {{", method.target.0).unwrap();
            for (index, &capture) in method.captures.iter().enumerate() {
                if index > 0 {
                    self.output.push_str(", ");
                }
                let reify = self.reify_receivers.contains(&method.target);
                if reify {
                    self.output.push_str("get ");
                }
                self.local(capture);
                self.output
                    .push_str(if reify { "() { return " } else { ": " });
                self.local_value(capture);
                if reify {
                    self.output.push_str("; }");
                }
            }
            self.output.push_str("};\n");
            self.captures
                .extend(method.captures.iter().map(|&id| (id, method.target)));
            self.capture_resets.push(method.target);
        }
        if self.program.recur_targets[method.target.0].used {
            self.iteration(method.target, method.body, ExprContext::Return);
        } else {
            self.emit(method.body, ExprContext::Return);
        }
        self.captures.truncate(previous_captures);
        if field_reset {
            self.field_resets.pop();
        }
        if capture_reset {
            self.capture_resets.pop();
        }
        self.indent -= 1;
        self.indentation();
        self.output.push_str("})");
    }

    fn function(&mut self, name: Option<LocalId>, methods: &[FnMethod], macro_function: bool) {
        self.function_with_ifn_captures(name, methods, macro_function, false);
    }

    fn function_with_ifn_captures(
        &mut self,
        name: Option<LocalId>,
        methods: &[FnMethod],
        macro_function: bool,
        ifn_call: bool,
    ) {
        if methods.len() == 1 && methods[0].rest.is_none() {
            // A single fixed method deliberately retains ordinary JS missing/
            // extra argument behavior, with no dispatcher or runtime prelude.
            self.method_with_ifn_captures(name, &methods[0], ifn_call);
            return;
        }
        self.output.push_str("(() => {\n");
        self.indent += 1;
        for (index, method) in methods.iter().enumerate() {
            self.indentation();
            write!(self.output, "const m{index} = ").unwrap();
            self.method_with_ifn_captures(None, method, ifn_call);
            self.output.push_str(";\n");
        }
        let function = name.map_or_else(|| "f".into(), |name| format!("l{}", name.0));
        self.indentation();
        write!(self.output, "const {function} = function(").unwrap();
        let count = if macro_function {
            1
        } else {
            methods
                .iter()
                .map(|m| m.params.len() + usize::from(m.rest.is_some()))
                .max()
                .unwrap()
        };
        for index in 0..count {
            if index > 0 {
                self.output.push_str(", ");
            }
            write!(self.output, "a{index}").unwrap();
        }
        self.output.push_str(") {\n");
        self.indent += 1;
        self.indentation();
        self.output.push_str("switch (arguments.length) {\n");
        self.indent += 1;
        for (index, method) in methods.iter().enumerate().filter(|(_, m)| m.rest.is_none()) {
            self.indentation();
            writeln!(
                self.output,
                "case {}: return m{index}.apply(this, arguments);",
                method.params.len()
            )
            .unwrap();
        }
        self.indentation();
        if let Some((index, method)) = methods.iter().enumerate().find(|(_, m)| m.rest.is_some()) {
            // Like CLJS, the variadic default also receives underfilled calls.
            write!(self.output, "default: return m{index}.call(this, ").unwrap();
            for i in 0..method.params.len() {
                write!(self.output, "arguments[{i}], ").unwrap();
            }
            self.rt(RuntimeName::export("fnRest"));
            writeln!(self.output, "(arguments, {}));", method.params.len()).unwrap();
        } else {
            self.output.push_str(if macro_function {
                "default: throw new Error(\"Invalid arity: \" + (arguments.length - 2));\n"
            } else {
                "default: throw new Error(\"Invalid arity: \" + arguments.length);\n"
            });
        }
        self.indent -= 1;
        self.indentation();
        self.output.push_str("}\n");
        self.indent -= 1;
        self.indentation();
        self.output.push_str("};\n");
        for (index, method) in methods.iter().enumerate() {
            self.indentation();
            if method.rest.is_some() {
                writeln!(
                    self.output,
                    "{function}.cljs$core$IFn$_invoke$arity$variadic = m{index};"
                )
                .unwrap();
                self.indentation();
                writeln!(
                    self.output,
                    "{function}.cljs$lang$maxFixedArity = {};",
                    method.params.len()
                )
                .unwrap();
                self.indentation();
                write!(self.output, "{function}.cljs$lang$applyTo = (args) => ").unwrap();
                self.rt(RuntimeName::export("fnApplyTo"));
                writeln!(self.output, "(m{index}, {}, args);", method.params.len()).unwrap();
            } else {
                writeln!(
                    self.output,
                    "{function}.cljs$core$IFn$_invoke$arity${} = m{index};",
                    method.params.len()
                )
                .unwrap();
            }
        }
        if macro_function && methods.iter().all(|method| method.rest.is_none()) {
            self.indentation();
            writeln!(
                self.output,
                "{function}.cljs$lang$maxFixedArity = {};",
                methods
                    .iter()
                    .map(|method| method.params.len())
                    .max()
                    .unwrap()
            )
            .unwrap();
        }
        self.indentation();
        writeln!(self.output, "return {function};").unwrap();
        self.indent -= 1;
        self.indentation();
        self.output.push_str("})()");
    }

    fn object_methods(&mut self, methods: &[(MethodDispatch, FnMethod)]) {
        self.output.push('[');
        for (index, (dispatch, method)) in methods.iter().enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            self.output.push('[');
            let ifn = match dispatch {
                MethodDispatch::Protocol(key) => {
                    self.string_units(&key.encode_utf16().collect::<Vec<_>>());
                    false
                }
                MethodDispatch::Builtin(name) => {
                    self.string_units(&name.encode_utf16().collect::<Vec<_>>());
                    name == "IFn/-invoke"
                }
            };
            write!(
                self.output,
                ", {}, ",
                method.params.len() + if method.rest.is_some() { 2 } else { 0 }
            )
            .unwrap();
            if ifn {
                // Callable adapters supply the receiver as the first argument;
                // their rest binding still needs the ordinary fn dispatcher.
                self.function_with_ifn_captures(None, std::slice::from_ref(method), false, true);
            } else {
                self.function(None, std::slice::from_ref(method), false);
            }
            if ifn {
                self.output.push_str(", ");
                self.function(None, std::slice::from_ref(method), false);
            }
            if method.rest.is_some() {
                if ifn {
                    write!(self.output, ", {}", method.params.len()).unwrap();
                } else if matches!(dispatch, MethodDispatch::Builtin(name) if name.starts_with("Object/"))
                {
                    write!(self.output, ", null, {}", method.params.len()).unwrap();
                }
            }
            self.output.push(']');
        }
        self.output.push(']');
    }

    fn js_template(&mut self, segments: &[String], args: &[ExprId]) {
        for (index, segment) in segments.iter().enumerate() {
            let start = self.output.len();
            self.output.push_str(segment);
            self.raw_segments.push(start..self.output.len());
            if let Some(&arg) = args.get(index) {
                self.value(arg);
            }
        }
    }

    fn value(&mut self, id: ExprId) {
        let site = self.site.replace(id);
        self.value_expr(id);
        self.site = site;
    }

    fn value_expr(&mut self, id: ExprId) {
        self.used.insert(id);
        if let Some(parts) = self.short_circuit(id, true) {
            self.short_circuit_value(parts);
            return;
        }
        match &self.program.exprs[id.0].kind {
            ExprKind::Nil => self.output.push_str("null"),
            ExprKind::Construct { callee, args } => {
                self.output.push_str("new ");
                // Host resolution is an implementation call, not a source call.
                // Preserve upstream new/call precedence for actual expressions.
                let host = matches!(self.program.exprs[callee.0].kind, ExprKind::JsGlobal(_));
                if host {
                    self.output.push('(');
                }
                self.value(*callee);
                if host {
                    self.output.push(')');
                }
                self.output.push('(');
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.value(*arg);
                }
                self.output.push(')');
            }
            ExprKind::Assert {
                test,
                message,
                form,
            } => {
                if self.inference.is_some() {
                    self.output.push_str("(() => { if (!(");
                    self.condition(*test);
                } else {
                    self.output.push_str("(() => { if (!truth(");
                    self.value(*test);
                }
                self.output
                    .push_str(")) throw new Error(\"Assert failed: \" + ");
                if let Some(message) = message {
                    self.rt(RuntimeName::export("displayStr"));
                    self.output.push('(');
                    self.value(*message);
                    self.output.push_str(") + \"\\n\" + ");
                }
                self.string(*form);
                self.output.push_str("); return null; })()");
            }
            ExprKind::Reify {
                metadata,
                captures,
                methods,
            } => {
                self.rt(RuntimeName::export("reify"));
                write!(self.output, "({}, [", id.0).unwrap();
                for (index, local) in captures.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.string_units(
                        &self.program.locals[local.0]
                            .name
                            .encode_utf16()
                            .collect::<Vec<_>>(),
                    );
                }
                self.output.push_str("], [");
                for (index, local) in captures.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.local_value(*local);
                }
                self.output.push_str("], () => ");
                let old_captures = self.reify_captures.len();
                let old_receivers = self.reify_receivers.len();
                // Each method closes over its own receiver. Nested functions
                // keep that receiver even after the method returns.
                self.output.push('[');
                for (index, method) in methods.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ...");
                    } else {
                        self.output.push_str("...");
                    }
                    self.reify_captures
                        .extend(captures.iter().map(|&local| (local, method.1.target)));
                    self.reify_receivers.push(method.1.target);
                    let enclosing_resets = self.captures.clone();
                    self.captures.retain(|(local, _)| !captures.contains(local));
                    self.object_methods(std::slice::from_ref(method));
                    self.captures = enclosing_resets;
                    self.reify_captures.truncate(old_captures);
                    self.reify_receivers.truncate(old_receivers);
                }
                self.output.push_str("], ");
                if let Some(metadata) = metadata {
                    self.value(*metadata);
                } else {
                    self.output.push_str("null");
                }
                self.output.push(')');
            }
            ExprKind::DefType {
                record,
                global,
                constructor,
                body,
                masks,
                map_constructor,
                fields,
                methods,
            } => {
                self.output.push_str("(() => { ");
                self.product(*global);
                self.output.push_str(" = ");
                self.rt(RuntimeName::export("makeType"));
                self.output.push('(');
                self.output.push('[');
                for (index, field) in fields.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.string_units(&field.encode_utf16().collect::<Vec<_>>());
                }
                self.output.push_str("], ");
                self.object_methods(methods);
                self.output.push_str(if *record {
                    if constructor.is_some() {
                        ", true"
                    } else {
                        ", \"raw\""
                    }
                } else {
                    ", false"
                });
                if let Some(masks) = masks {
                    self.output.push_str(", ");
                    self.value(*masks);
                } else {
                    self.output.push_str(", null");
                }
                self.output.push_str(", ");
                let binding = &self.program.globals[global.0];
                self.string_units(
                    &format!("{}/{}", binding.namespace, binding.name)
                        .encode_utf16()
                        .collect::<Vec<_>>(),
                );
                if *record && constructor.is_some() {
                    self.output.push_str(", () => ");
                    self.global(*global);
                }
                self.output.push_str("); ");
                if let Some(constructor) = constructor {
                    self.product(*constructor);
                    self.output.push_str(" = (...args) => new ");
                    self.global(*global);
                    if *record {
                        self.output.push_str("(...Array.from({length: ");
                        self.output.push_str(&fields.len().to_string());
                        self.output
                            .push_str("}, (_, i) => args[i]), null, null, null); ");
                    } else {
                        self.output.push_str("(...args); ");
                    }
                    self.product(*global);
                    self.output.push_str(".getBasis = () => ");
                    self.rt(RuntimeName::core("vector"));
                    self.output.push('(');
                    for (index, field) in fields.iter().enumerate() {
                        if index > 0 {
                            self.output.push_str(", ");
                        }
                        self.rt(RuntimeName::export("symbol"));
                        self.output.push_str("(null, ");
                        self.string_units(&field.encode_utf16().collect::<Vec<_>>());
                        self.output.push(')');
                    }
                    self.output.push_str("); ");
                    self.product(*global);
                    self.output.push_str(".cljs$lang$type = true; ");
                    if !*record {
                        self.product(*global);
                        self.output.push_str(".cljs$lang$ctorStr = ");
                        let binding = &self.program.globals[global.0];
                        self.string_units(
                            &format!("{}/{}", binding.namespace, binding.name)
                                .encode_utf16()
                                .collect::<Vec<_>>(),
                        );
                        self.output.push_str("; ");
                    }
                }
                if let Some(map_constructor) = map_constructor {
                    self.product(*map_constructor);
                    self.output.push_str(" = value => ");
                    self.rt(RuntimeName::export("recordFromMap"));
                    self.output.push('(');
                    self.global(*global);
                    self.output.push_str(", value); ");
                }
                self.output.push_str("return ");
                if let Some(body) = body {
                    self.value(*body);
                } else {
                    self.product(*global);
                }
                self.output.push_str("; })()");
            }

            ExprKind::JsArguments => self.output.push_str("arguments"),
            ExprKind::JsTemplate { segments, args } => {
                self.output.push('(');
                self.js_template(segments, args);
                self.output.push(')');
            }
            ExprKind::JsIndex { receiver, indices } => {
                self.output.push('(');
                self.value(*receiver);
                self.output.push(')');
                for index in indices {
                    self.output.push('[');
                    self.value(*index);
                    self.output.push(']');
                }
            }
            ExprKind::JsDelete { receiver, key } => {
                self.output.push_str("(delete (");
                self.value(*receiver);
                self.output.push_str(")[");
                self.value(*key);
                self.output.push_str("])");
            }
            ExprKind::QualifiedMethod { class, method } => {
                if let Some(method) = method {
                    self.output
                        .push_str("(function (x, ...args) { return Reflect.apply(");
                    self.value(*class);
                    self.output.push_str(".prototype");
                    self.member(method);
                    self.output.push_str(", x, args); })");
                } else {
                    self.output
                        .push_str("(function (...args) { return Reflect.construct(");
                    self.value(*class);
                    self.output.push_str(", args); })");
                }
            }
            ExprKind::CopyArguments(dest) => {
                self.output.push_str("((dest) => { for (let i = 0; i < arguments.length; i++) dest.push(arguments[i]); return null; })(");
                self.value(*dest);
                self.output.push(')');
            }
            ExprKind::DefProtocol {
                global,
                methods,
                via_metadata,
            } => {
                self.output.push_str("(() => { ");
                self.product(*global);
                self.output.push_str(" = ");
                self.rt(RuntimeName::export("protocol"));
                self.output.push('(');
                let binding = &self.program.globals[global.0];
                self.string_units(
                    &format!("{}/{}", binding.namespace, binding.name)
                        .encode_utf16()
                        .collect::<Vec<_>>(),
                );
                self.output.push_str("); ");
                for method in methods {
                    self.product(method.global);
                    self.output.push_str(" = ");
                    self.rt(RuntimeName::export("protocolMethod"));
                    self.output.push('(');
                    self.string_units(&method.key.encode_utf16().collect::<Vec<_>>());
                    self.output.push_str(", ");
                    let b = &self.program.globals[method.global.0];
                    self.string_units(
                        &format!("{}/{}", b.namespace, b.name)
                            .encode_utf16()
                            .collect::<Vec<_>>(),
                    );
                    write!(
                        self.output,
                        ", {:?}, {}, {}, () => ",
                        method.arities,
                        via_metadata,
                        method
                            .variadic
                            .map_or_else(|| "null".into(), |n| n.to_string())
                    )
                    .unwrap();
                    self.global(method.global);
                    self.output.push_str("); ");
                }
                self.output.push_str("return undefined; })()");
            }
            ExprKind::ProtocolPredicate {
                protocol,
                value,
                key,
                direct,
            } => {
                self.rt(RuntimeName::export("protocolPredicate"));
                self.output.push('(');
                if *direct {
                    self.output.push_str("null");
                } else {
                    self.value(*protocol);
                }
                self.output.push_str(", ");
                self.string_units(&key.encode_utf16().collect::<Vec<_>>());
                self.output.push_str(", ");
                self.value(*value);
                write!(self.output, ", {})", direct).unwrap();
            }
            ExprKind::ExtendType {
                instance,
                return_target,
                target,
                methods,
            } => {
                self.rt(RuntimeName::export(if *instance {
                    "extendInstance"
                } else {
                    "extendType"
                }));
                if !*instance && self.replaced_core.is_some() {
                    // `extendType` patches the dispatcher it finds in
                    // `coreDispatchers` by key, so a sliced runtime keeps the
                    // dispatcher of every core protocol method extended here.
                    for (dispatch, _) in methods {
                        if let MethodDispatch::Builtin(key) = dispatch
                            && let Some(binding) = RuntimeManifest::embedded().core_dispatcher(key)
                        {
                            self.runtime.record(
                                RuntimeName::direct(binding, None),
                                self.site,
                                NameUse::Keep,
                            );
                        }
                    }
                }
                self.output.push('(');
                self.value(*target);
                self.output.push_str(", ");
                self.object_methods(methods);
                if *instance {
                    write!(self.output, ", {return_target}").unwrap();
                }
                self.output.push(')');
            }
            ExprKind::Bool(value) => self.output.push_str(if *value { "true" } else { "false" }),
            ExprKind::Number(value) => {
                if value.is_nan() {
                    self.output.push_str("NaN");
                } else if *value == f64::INFINITY {
                    self.output.push_str("Infinity");
                } else if *value == f64::NEG_INFINITY {
                    self.output.push_str("-Infinity");
                } else {
                    write!(self.output, "{value}").unwrap();
                }
            }
            ExprKind::String(id) => self.string(*id),
            ExprKind::ReaderLiteral {
                kind,
                items,
                quoted,
            } => {
                use crate::compiler_data::CompilerLiteralKind as K;
                let (constructor, open) = match (kind, quoted) {
                    (K::JsArray, true) => (Some(RuntimeName::compiler_forms("jsArray")), "(["),
                    (K::JsObject, true) => (Some(RuntimeName::compiler_forms("jsObject")), "(["),
                    _ => match kind {
                        K::BigInteger => unreachable!("integers lower to Number"),
                        K::Regex => (Some(RuntimeName::core("rePattern")), "("),
                        K::Instant => (None, "new Date("),
                        K::Uuid => (Some(RuntimeName::core("uuid")), "("),
                        K::JsArray => (None, "["),
                        K::JsObject => (Some(RuntimeName::export("jsObject")), "(["),
                        K::Queue => (Some(RuntimeName::export("queueFromArray")), "(["),
                    },
                };
                if let Some(constructor) = constructor {
                    self.rt(constructor);
                }
                self.output.push_str(open);
                self.arguments(items);
                self.output.push_str(match (kind, quoted) {
                    (K::JsArray, true) => "])",
                    _ => match kind {
                        K::JsArray => "]",
                        K::JsObject | K::Queue => "])",
                        _ => ")",
                    },
                });
            }
            ExprKind::DefinitionTest { value, unit } => {
                if self.demand_roots {
                    write!(self.output, "globalThis.__lightCompilerTest({unit})").unwrap();
                } else {
                    self.value(*value);
                }
            }
            ExprKind::WithMeta { value, metadata } => {
                use crate::compiler_data::CompilerLiteralKind as K;
                self.rt(
                    if matches!(
                        self.program.exprs[value.0].kind,
                        ExprKind::ReaderLiteral {
                            kind: K::JsArray | K::JsObject,
                            quoted: true,
                            ..
                        }
                    ) {
                        RuntimeName::compiler_forms("withMeta")
                    } else {
                        RuntimeName::core("withMeta")
                    },
                );
                self.output.push('(');
                self.value(*value);
                self.output.push_str(", ");
                self.value(*metadata);
                self.output.push(')');
            }
            ExprKind::Var {
                definition,
                value,
                symbol,
                metadata,
            } => {
                self.rt(RuntimeName::export(if *definition {
                    "definitionVar"
                } else {
                    "makeVar"
                }));
                self.output.push_str("(() => ");
                self.value(*value);
                self.output.push_str(", ");
                self.rt(RuntimeName::export("symbol"));
                self.output.push('(');
                let literal = &self.program.named_literals[symbol.0];
                if let Some(namespace) = literal.namespace {
                    self.string(namespace);
                } else {
                    self.output.push_str("null");
                }
                self.output.push_str(", ");
                self.string(literal.name);
                self.output.push(')');
                if let Some(metadata) = metadata {
                    self.output.push_str(", ");
                    self.value(*metadata);
                }
                if self.demand_roots && *definition {
                    if metadata.is_none() {
                        self.output.push_str(", null");
                    }
                    self.output
                        .push_str(", globalThis.__lightCompilerVarMetadata(");
                    let namespace = literal
                        .namespace
                        .map(|id| String::from_utf16_lossy(&self.program.strings[id.0]))
                        .unwrap_or_default();
                    let name = String::from_utf16_lossy(&self.program.strings[literal.name.0]);
                    self.string_units(
                        &format!("{namespace}/{name}")
                            .encode_utf16()
                            .collect::<Vec<_>>(),
                    );
                    self.output.push(')');
                }
                self.output.push(')');
            }
            ExprKind::Keyword(literal) => self.named_literal(*literal, false),
            ExprKind::Symbol(literal) => self.named_literal(*literal, true),
            ExprKind::Vector(items) => {
                self.rt(RuntimeName::export("vectorFromArray"));
                self.output.push_str("([");
                self.arguments(items);
                self.output.push_str("])");
            }
            ExprKind::Map(items) => {
                if items.len() > 16 {
                    self.rt(RuntimeName::export("mapFromArrays"));
                    self.output.push_str("([");
                    self.arguments(&items.iter().step_by(2).copied().collect::<Vec<_>>());
                    self.output.push_str("], [");
                    self.arguments(&items.iter().skip(1).step_by(2).copied().collect::<Vec<_>>());
                    self.output.push_str("])");
                } else {
                    self.rt(RuntimeName::export("mapFromArray"));
                    self.output.push_str("([");
                    self.arguments(items);
                    self.output.push_str("])");
                }
            }
            ExprKind::LiteralMap(items) | ExprKind::LiteralSet(items) => {
                self.rt(RuntimeName::export(
                    if matches!(self.program.exprs[id.0].kind, ExprKind::LiteralMap(_)) {
                        "constantMapFromArray"
                    } else {
                        "constantSetFromArray"
                    },
                ));
                self.output.push_str("([");
                self.arguments(items);
                self.output.push_str("])");
            }
            ExprKind::Set(items) => {
                // Reader-distinct constants retain the runtime's existing
                // Number-coercion policy. Dynamic member results need the
                // pinned checkout's duplicate check after all operands run.
                let constant = items.iter().all(|item| {
                    matches!(
                        self.program.exprs[item.0].kind,
                        ExprKind::Nil
                            | ExprKind::Bool(_)
                            | ExprKind::Number(_)
                            | ExprKind::String(_)
                            | ExprKind::Keyword(_)
                            | ExprKind::Symbol(_)
                    )
                });
                self.rt(RuntimeName::export(if constant {
                    "setFromArray"
                } else {
                    "literalSetFromArray"
                }));
                self.output.push_str("([");
                self.arguments(items);
                self.output.push_str("])");
            }
            ExprKind::List(items) => {
                self.rt(RuntimeName::export("listFromArray"));
                self.output.push_str("([");
                self.arguments(items);
                self.output.push_str("])");
            }
            ExprKind::CoreFn(core) => {
                self.rt(RuntimeName::core(core.runtime_name()));
            }
            ExprKind::Local(local) => self.local_value(*local),
            ExprKind::Global(global) => self.global(*global),
            ExprKind::JsGlobal(global) => {
                let program = self.program;
                if let Some((package, members)) = program.js_globals[global.0].package() {
                    self.package_reference(package, members);
                    return;
                }
                self.needs_host = true;
                let path = &self.program.js_globals[global.0].path;
                write!(self.output, "host(\"{}\")", path[0]).unwrap();
                for part in &path[1..] {
                    self.member(part);
                }
            }
            ExprKind::BooleanHint(value) => self.value(*value),
            ExprKind::Exists(value) | ExprKind::BindingExists(value) => {
                let value = if let ExprKind::BooleanHint(value) = self.program.exprs[value.0].kind {
                    value
                } else {
                    *value
                };
                if self.compiler_binding(value) {
                    self.output.push_str(
                        if matches!(self.program.exprs[id.0].kind, ExprKind::BindingExists(_)) {
                            "globalThis.__lightHasCompilerRoot("
                        } else {
                            "globalThis.__lightCompilerRootExists("
                        },
                    );
                    self.binding_name(value);
                    self.output.push(')');
                    return;
                }
                self.output.push('(');
                let program = self.program;
                if let ExprKind::JsGlobal(global) = program.exprs[value.0].kind
                    && let Some((package, members)) = program.js_globals[global.0].package()
                    && self.imports.is_some()
                {
                    // A named import of a missing export fails at link time;
                    // the namespace object can be probed.
                    self.output.push_str("typeof ");
                    self.namespace_import(package);
                    self.output.push_str(" !== \"undefined\"");
                    for i in 0..members.len() {
                        self.output.push_str(" && typeof ");
                        self.namespace_import(package);
                        for member in &members[..=i] {
                            self.member(member);
                        }
                        self.output.push_str(" !== \"undefined\"");
                    }
                } else if let ExprKind::JsGlobal(global) = self.program.exprs[value.0].kind {
                    let path = &self.program.js_globals[global.0].path;
                    for i in 0..path.len() {
                        if i > 0 {
                            self.output.push_str(" && ");
                        }
                        self.output.push_str("typeof globalThis");
                        for part in &path[..=i] {
                            self.member(part);
                        }
                        self.output.push_str(" !== \"undefined\"");
                    }
                } else {
                    self.output.push_str("typeof ");
                    self.value(value);
                    self.output.push_str(" !== \"undefined\"");
                }
                self.output.push(')');
            }
            ExprKind::SetGlobal { target, value } => {
                self.output.push('(');
                self.assignable(*target);
                self.output.push_str(" = ");
                self.value(*value);
                self.output.push(')');
            }
            ExprKind::Def {
                global,
                init,
                test,
                once,
            } => {
                if self.demand_roots && !self.program.globals[global.0].name.contains('.') {
                    self.output
                        .push_str("globalThis.__lightDefineCompilerRoot(");
                    let binding = &self.program.globals[global.0];
                    self.string_units(
                        &format!("{}/{}", binding.namespace, binding.name)
                            .encode_utf16()
                            .collect::<Vec<_>>(),
                    );
                    self.output.push_str(", ");
                    if let Some(init) = init {
                        self.output.push_str("() => ");
                        if let Some(test) = test {
                            self.output.push_str("{ const value = ");
                            self.value(*init);
                            self.output.push_str("; value.cljs$lang$test = ");
                            self.value(*test);
                            self.output.push_str("; return value; }");
                        } else {
                            self.output.push('(');
                            self.value(*init);
                            self.output.push(')');
                        }
                    } else {
                        self.output.push_str("null");
                    }
                    write!(self.output, ", {once})").unwrap();
                    return;
                }
                self.output.push('(');
                if *once {
                    self.output.push_str("typeof ");
                    self.global(*global);
                    self.output.push_str(" === \"undefined\" ? ");
                }
                if let Some(init) = init {
                    self.output.push('(');
                    self.global(*global);
                    self.output.push_str(" = ");
                    self.value(*init);
                    if let Some(test) = test
                        && self.returned_definition != Some(id)
                    {
                        self.output.push_str(", ");
                        self.global(*global);
                        self.output.push_str("[\"cljs$lang$test\"] = ");
                        self.value(*test);
                        self.output.push_str(", ");
                        self.global(*global);
                    }
                    self.output.push(')');
                } else {
                    self.output.push_str("void 0");
                }
                if *once {
                    self.output.push_str(" : null");
                }
                self.output.push(')');
            }
            ExprKind::InstanceOf { class, value } => {
                self.output.push('(');
                self.value(*value);
                self.output.push_str(" instanceof ");
                self.value(*class);
                self.output.push(')');
            }
            ExprKind::Property { receiver, property } => {
                if self.demand_roots {
                    self.rt(RuntimeName::export("declarationReceiver"));
                    self.output.push('(');
                } else {
                    self.output.push('(');
                }
                self.value(*receiver);
                self.output.push(')');
                self.member(property);
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                if self.demand_roots {
                    self.rt(RuntimeName::export("declarationReceiver"));
                    self.output.push('(');
                } else {
                    self.output.push('(');
                }
                self.value(*receiver);
                self.output.push(')');
                self.member(method);
                self.output.push('(');
                self.arguments(args);
                self.output.push(')');
            }
            ExprKind::If { test, yes, no } => {
                self.output.push('(');
                self.condition(*test);
                self.output.push_str(" ? ");
                self.value(*yes);
                self.output.push_str(" : ");
                self.value(*no);
                self.output.push(')');
            }
            ExprKind::ThisAs { .. } => {
                // CLJS value-context this-as has an ordinary function scope;
                // its implicit receiver is the global object, not lexical this.
                self.output.push_str("(function () {\n");
                self.indent += 1;
                self.emit(id, ExprContext::Return);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("})()");
            }
            ExprKind::Try {
                body,
                catches,
                finally,
            } if catches.is_empty() && finally.is_none() => {
                // A clause-free try is its do body. In particular a single
                // value must not introduce a new JavaScript arguments scope.
                if let ExprKind::Do(forms) = &self.program.exprs[body.0].kind
                    && forms.len() <= 1
                {
                    if let Some(&form) = forms.first() {
                        self.value(form);
                    } else {
                        self.output.push_str("null");
                    }
                } else {
                    self.value(*body);
                }
            }
            ExprKind::WithOutStr(_)
            | ExprKind::Binding { .. }
            | ExprKind::Do(_)
            | ExprKind::Let { .. }
            | ExprKind::LetFn { .. }
            | ExprKind::Loop { .. }
            | ExprKind::Try { .. }
            | ExprKind::Throw(_)
            | ExprKind::Case { .. } => {
                self.output.push_str("(function () {\n");
                self.indent += 1;
                self.emit(id, ExprContext::Return);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("})()");
            }
            ExprKind::Fn { name, methods } => {
                let macro_function = self.program.macro_functions.contains(&id);
                if macro_function {
                    self.rt(RuntimeName::core("macroReaderFunction"));
                    self.output.push('(');
                }
                self.function(*name, methods, macro_function);
                if macro_function {
                    self.output.push(')');
                }
            }
            ExprKind::Recur { .. } => unreachable!("analysis forbids recur in value position"),
            ExprKind::Call { callee, args } => {
                if self.lowered_call(*callee, args) {
                    return;
                }
                if let ExprKind::CoreFn(core) = self.program.exprs[callee.0].kind
                    && self.known_unit(core)
                {
                    // `rt$add(1, 2)`: a core operation in a unit binding of its
                    // own. Only called, its value does not escape, so the slice
                    // leaves out its variadic arity ABI (`install:` units).
                    let site = self.site.replace(*callee);
                    self.used.insert(*callee);
                    let text = self.runtime_read(
                        RuntimeName::core(core.runtime_name()),
                        self.site,
                        NameUse::Call,
                    );
                    self.output.push_str(&text);
                    self.site = site;
                    self.output.push('(');
                    self.arguments(args);
                    self.output.push(')');
                    return;
                }
                self.output.push('(');
                self.value(*callee);
                if let Some(arity) = self.specialized_calls.and_then(|calls| calls.get(&id)) {
                    write!(self.output, ".cljs$core$IFn$_invoke$arity${arity}").unwrap();
                }
                if matches!(self.program.exprs[callee.0].kind, ExprKind::JsGlobal(_)) {
                    // A JS path is a host invocation and retains its final receiver.
                    self.output.push_str(")(");
                } else {
                    self.output.push_str(").call(null");
                    if !args.is_empty() {
                        self.output.push_str(", ");
                    }
                }
                self.arguments(args);
                self.output.push(')');
            }
        }
    }
}

/// Whether the value of `id` may read the `arguments` of the function it is
/// emitted in, or holds raw JavaScript (which may read `this`), outside
/// nested functions.
fn reads_function_scope(program: &Program, id: ExprId) -> bool {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        match program.exprs[id.0].kind {
            ExprKind::JsArguments | ExprKind::CopyArguments(_) | ExprKind::JsTemplate { .. } => {
                return true;
            }
            ExprKind::Fn { .. } => (),
            _ => pending.extend(crate::program_fragment::children(program, id)),
        }
    }
    false
}

// The compiler emits only quoted strings and ordinary identifier tokens outside
// explicitly recorded raw segments; the runtime body is also excluded here.
// Every name the emitted code binds must be renamed here, so raw text can name
// only its holes and host globals: advanced preparation relies on that to treat
// `js*` as an opaque leaf. `optimization::raw_js_names_resolve_only_to_holes_and_host_globals`
// in tests/compiler checks it from raw text in each scope the emitter creates.
fn hygienic_js(source: String, mut raw: Vec<std::ops::Range<usize>>) -> String {
    raw.retain(|range| !range.is_empty());
    raw.sort_by_key(|range| range.start);
    let mut index = 0;
    let prefix = loop {
        let candidate = format!("__light{index}_");
        if !raw
            .iter()
            .any(|range| source[range.clone()].contains(&candidate))
        {
            break candidate;
        }
        index += 1;
    };
    let mut result = String::new();
    let bytes = source.as_bytes();
    let mut at = 0;
    let mut raw_at = 0;
    while at < bytes.len() {
        if raw.get(raw_at).is_some_and(|range| range.start == at) {
            let range = &raw[raw_at];
            result.push_str(&source[range.clone()]);
            at = range.end;
            raw_at += 1;
            continue;
        }
        let start = at;
        if bytes[at] == b'"' {
            at += 1;
            while at < bytes.len() {
                let byte = bytes[at];
                at += 1;
                if byte == b'\\' {
                    at += 1;
                } else if byte == b'"' {
                    break;
                }
            }
        } else if bytes[at].is_ascii_alphabetic() || matches!(bytes[at], b'_' | b'$') {
            at += 1;
            while at < bytes.len()
                && raw.get(raw_at).is_none_or(|range| at < range.start)
                && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'_' | b'$'))
            {
                at += 1;
            }
            let token = &source[start..at];
            let numbered = [
                "l",
                "g",
                "k",
                "m",
                "a",
                "r",
                "next",
                "saved",
                "temporary",
                "captures",
                "self",
                "recur",
                "installed",
            ]
            .iter()
            .any(|head| {
                token.strip_prefix(head).is_some_and(|tail| {
                    !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit() || b == b'_')
                })
            });
            // `rt$<binding>` and `core$<name>` are the runtime reads and the
            // core operation cells of `Advanced` output.
            if numbered
                || matches!(
                    token,
                    "rt" | "truth"
                        | "host"
                        | "pkg"
                        | "f"
                        | "text"
                        | "savedPrint"
                        | "savedNewline"
                        | "caught"
                        | "caseValue"
                )
                || token.starts_with("rt$")
                || token.starts_with("core$")
            {
                result.push_str(&prefix);
            }
        } else {
            at += 1;
        }
        // All non-ASCII source outside raw JS occurs inside escaped strings.
        result.push_str(&source[start..at]);
    }
    result
}

/// Interactive application roots live in the application heap, never the macro heap.
pub(crate) fn emit_interactive_unit(program: &Program) -> String {
    let (body, _) = emit_js_mode(
        program,
        Some("__lightInteractiveRuntime"),
        false,
        true,
        None,
        false,
    );
    format!(
        "(__lightInteractiveRuntime) => {{ globalThis.__lightApplicationRoots ??= Object.create(null); return {body}; }}"
    )
}

/// A readable JavaScript string literal; non-ASCII and controls are escaped.
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for unit in text.encode_utf16() {
        match unit {
            0x22 => out.push_str("\\\""),
            0x5c => out.push_str("\\\\"),
            0x20..=0x7e => out.push(char::from_u32(unit.into()).unwrap()),
            _ => write!(out, "\\u{unit:04x}").unwrap(),
        }
    }
    out.push('"');
    out
}

fn js_string(text: &str) -> String {
    let mut out = String::from("\"");
    for unit in text.encode_utf16() {
        write!(out, "\\u{unit:04x}").unwrap();
    }
    out.push('"');
    out
}
