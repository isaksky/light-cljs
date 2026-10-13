//! Recursive JavaScript emission from resolved expressions; no name lookup.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fmt::Write,
};

use crate::analyzer::{
    ExprContext, ExprId, ExprKind, FnMethod, GlobalId, LocalId, MethodDispatch, NamedLiteralId,
    PACKAGE_REGISTRY, Program, RecurTargetId, StringId,
};
use crate::call_shapes::{CallShapes, DirectCall, FunctionShape};
use crate::core_operations::Type;
use crate::inference::{Inference, Known};
use crate::runtime_manifest::{CORE_CELLS, CORE_HUB, CoreReference, RuntimeManifest};
use crate::runtime_slice::{
    ConstantPool, EmittedRuntime, NameUse, RuntimeEmbedding, RuntimeName, RuntimeRoots,
};
use crate::value_hash::ConstantHashes;

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
/// [`crate::runtime_slice`]); see [`Application`] for the other decisions.
pub(crate) fn emit_application(
    program: &Program,
    application: Application<'_>,
    module: bool,
) -> (String, EmittedRuntime) {
    emit_js_mode(program, None, false, false, Some(application), module)
}

/// What application preparation decided for emission.
#[derive(Clone, Copy)]
pub(crate) struct Application<'a> {
    /// Retained globals, by `GlobalId`. At `Advanced` a `defprotocol` defines
    /// only the retained ones of its protocol and methods.
    pub globals: &'a [bool],
    /// Calls specialized to a fixed arity entry.
    pub calls: &'a std::collections::HashMap<ExprId, usize>,
    /// At `Advanced`: read runtime bindings directly from a runtime slice. The
    /// runtime names of the core operations the program may replace, sorted; their
    /// reads and writes go through an output-level cell, `core$<name>`.
    pub direct: Option<&'a [&'static str]>,
    /// At `Advanced`: the program's inferred types, which lower truth tests
    /// and core calls.
    pub inference: Option<&'a Inference>,
    /// At `Advanced`: protocol method calls that read the implementation from
    /// the receiver's slot (`crate::rta`), with the slot each reads.
    pub methods: Option<&'a std::collections::HashMap<ExprId, String>>,
    /// Whether types get their static `getBasis` method, which at `Advanced`
    /// only a program that names it can call (`crate::rta`).
    pub basis: bool,
    /// At `Advanced`: calls that name the method they run, and definitions
    /// that bind each method to an entry of its own (`crate::call_shapes`).
    pub shapes: Option<&'a CallShapes>,
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
    let shapes = application
        .and_then(|application| application.shapes)
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
            retained: application
                .filter(|_| direct.is_some())
                .map(|application| application.globals),
            direct_methods: application
                .and_then(|application| application.methods)
                .filter(|_| direct.is_some()),
            type_basis: direct.is_none() || application.is_none_or(|application| application.basis),
            replaced_core: direct,
            core_cells: Vec::new(),
            constants: direct.map(|_| ConstantTable {
                declarations: String::new(),
                numbers: HashMap::new(),
                named: vec![[None; 2]; program.named_literals.len()],
                lazy: Vec::new(),
                composites: 0,
                classes: vec![ConstantClass::Unknown; program.exprs.len()],
                hashes: inference
                    .is_some_and(|inference| inference.facts().core_protocols_intact())
                    .then(ConstantHashes::default),
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
            shapes,
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
            // A function defined with method entries binds those, and the
            // global only for its dispatcher.
            if let Some(FunctionShape::Entries {
                function,
                dispatcher,
            }) = shapes.map(|shapes| shapes.function(GlobalId(global)))
            {
                if dispatcher {
                    writeln!(emitter.output, "  let g{global};").unwrap();
                }
                let ExprKind::Fn { methods, .. } = &program.exprs[function.0].kind else {
                    unreachable!("method entries of a fn")
                };
                for index in 0..methods.len() {
                    writeln!(emitter.output, "  let g{global}_{index};").unwrap();
                }
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
    let constants = emitter
        .constants
        .map_or_else(ConstantPool::default, |table| ConstantPool {
            entries: table.lazy.len(),
            composites: table.composites,
            lazy: table.lazy.iter().filter(|lazy| **lazy).count(),
            bytes: table.declarations.len(),
        });
    let runtime = EmittedRuntime {
        roots: emitter.runtime,
        embedding,
        bytes: runtime_bytes,
        constants,
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
/// structure: sites with equal keys share one table entry.
#[derive(Clone, PartialEq, Eq, Hash)]
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
    /// A composite constant of the pool (light-cljs-218): the runtime
    /// constructor its sites call below `Advanced` and its parts, whose nested
    /// constants are earlier entries. Keying on these rather than on runtime
    /// equality keeps reader-distinct forms apart (`[1]` and `'(1)` are `=`),
    /// and hash-consing the parts shares equal subterms.
    Composite {
        constructor: Composite,
        parts: Box<[ConstantPart<'a>]>,
    },
    /// The key shape of map literals with these distinct constant keys in
    /// this order (light-cljs-220), `rt$mapShape([keys], [hashes])`, which
    /// sites read as `rt$mapFromShape(k<n>, [values])`; past 8 keys
    /// `rt$hashMapShape` and `rt$hashMapFromShape`, whose shape also holds the
    /// hash trie. With `set`, the shape of set literals whose first members
    /// are these, `rt$setShape([members])`, which sites read as
    /// `rt$setFromShape(k<n>, [other members])`.
    Shape {
        set: bool,
        keys: Box<[ConstantPart<'a>]>,
    },
}

/// How a composite constant is built, which is how its sites build it below
/// `Advanced`, so a pooled value differs from a per-site one only in identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Composite {
    /// `vectorFromArray([items])`.
    Vector,
    /// `listFromArray([items])`, a quoted list.
    List,
    /// `mapFromArray([k0, v0, ...])`, a map literal of at most 8 entries;
    /// `arrayMapFromArray` at `Advanced`, which builds no hash map.
    ArrayMap,
    /// `mapFromArrays([keys], [values])`, a map literal of more entries.
    HashMap,
    /// `setFromArray([items])`, a set literal of scalars, keywords and symbols.
    Set,
    /// `literalSetFromArray([items])`, which throws on members equal at run
    /// time. Never pooled: hoisting would move the throw to load time.
    CheckedSet,
    /// `constantMapFromArray([k0, v0, ...])`, reader-distinct keys.
    LiteralMap,
    /// `constantSetFromArray([items])`, reader-distinct members.
    LiteralSet,
    /// `rt$withMeta(value, metadata)`: reader metadata on a constant
    /// collection or quoted symbol.
    WithMeta,
    /// `rt$uuid(text)`, a `#uuid` literal.
    Uuid,
}

/// A part of a composite constant: an inline scalar, an empty collection
/// singleton or another table entry.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ConstantPart<'a> {
    Nil,
    Bool(bool),
    /// The bits of the number, with one NaN; `-0` and `0` stay distinct.
    Number(u64),
    String(&'a [u16]),
    Empty(EmptyCollection),
    Entry(usize),
}

/// The empty collections every level reads from the runtime's `EMPTY`
/// singletons instead of allocating (light-cljs-218 tier 1), as upstream does.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EmptyCollection {
    Vector,
    Map,
    Set,
    List,
}

impl EmptyCollection {
    /// The empty value of a `[]`, `{}`, `#{}` or `()` literal. A quoted
    /// `'#{}` (a `LiteralSet`) keeps its positional constructor.
    fn of(kind: &ExprKind) -> Option<Self> {
        match kind {
            ExprKind::Vector(items) if items.is_empty() => Some(Self::Vector),
            ExprKind::Map(items) if items.is_empty() => Some(Self::Map),
            ExprKind::Set(items) if items.is_empty() => Some(Self::Set),
            ExprKind::List(items) if items.is_empty() => Some(Self::List),
            _ => None,
        }
    }

    /// The core table name of the type whose `EMPTY` this is.
    fn core_type(self) -> &'static str {
        match self {
            Self::Vector => "PersistentVector",
            Self::Map => "PersistentArrayMap",
            Self::Set => "PersistentHashSet",
            Self::List => "List",
        }
    }
}

/// Constants of at least this many nodes (scalars, keywords, symbols and
/// collections, counted over the literal) are lazy cells, built on first
/// read: `let c<n>; const k<n> = () => c<n> ??= <initializer>;`, read as
/// `k<n>()`. Smaller ones are built while the output loads. A lazy cell saves
/// QuickJS the construction of a constant that startup does not read, and
/// costs V8 a second parse of one that it does; below about a thousand nodes
/// neither engine gains. Measured in benchmarks/compiler.md ("Constant
/// collection pool").
const LAZY_CONSTANT_NODES: u32 = 1024;

/// What the pool knows about an expression (indexed by `ExprId`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConstantClass {
    Unknown,
    /// Not a constant the pool may hoist.
    Variable,
    /// A constant of this many nodes.
    Constant(u32),
}

/// A scalar, keyword or quoted symbol: the members of a set literal that
/// `setFromArray` builds without a duplicate check.
fn scalar_constant(kind: &ExprKind) -> bool {
    matches!(
        kind,
        ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_)
    )
}

/// The runtime constructor of a collection literal, `with-meta` of a literal
/// or `#uuid` literal, and its items in constructor order; `None` for any
/// other expression. Empty collections are [`EmptyCollection`]s instead.
fn composite<'p>(program: &Program, kind: &'p ExprKind) -> Option<(Composite, Cow<'p, [ExprId]>)> {
    use crate::compiler_data::CompilerLiteralKind as K;
    let (constructor, items) = match kind {
        ExprKind::Vector(items) => (Composite::Vector, Cow::Borrowed(&items[..])),
        ExprKind::List(items) => (Composite::List, Cow::Borrowed(&items[..])),
        ExprKind::Map(items) if items.len() > 16 => (Composite::HashMap, Cow::Borrowed(&items[..])),
        ExprKind::Map(items) => (Composite::ArrayMap, Cow::Borrowed(&items[..])),
        // Reader-distinct constants retain the runtime's existing
        // Number-coercion policy. Dynamic member results need the pinned
        // checkout's duplicate check after all operands run.
        ExprKind::Set(items)
            if items
                .iter()
                .all(|item| scalar_constant(&program.exprs[item.0].kind)) =>
        {
            (Composite::Set, Cow::Borrowed(&items[..]))
        }
        ExprKind::Set(items) => (Composite::CheckedSet, Cow::Borrowed(&items[..])),
        ExprKind::LiteralMap(items) => (Composite::LiteralMap, Cow::Borrowed(&items[..])),
        ExprKind::LiteralSet(items) => (Composite::LiteralSet, Cow::Borrowed(&items[..])),
        // `#js` data keeps its compiler form `withMeta`.
        ExprKind::WithMeta { value, .. }
            if matches!(
                program.exprs[value.0].kind,
                ExprKind::ReaderLiteral {
                    kind: K::JsArray | K::JsObject,
                    quoted: true,
                    ..
                }
            ) =>
        {
            return None;
        }
        ExprKind::WithMeta { value, metadata } => {
            (Composite::WithMeta, Cow::Owned(vec![*value, *metadata]))
        }
        ExprKind::ReaderLiteral {
            kind: K::Uuid,
            items,
            ..
        } => (Composite::Uuid, Cow::Borrowed(&items[..])),
        _ => return None,
    };
    Some((constructor, items))
}

/// The size in nodes of the constant `id` evaluates to, or `None` when the
/// pool may not hoist it (docs/advanced-optimization.md, "Constant
/// collection literals"). Constants are scalars, keywords, quoted symbols,
/// `#uuid` literals, collection literals of constants and reader metadata of
/// a constant on a constant collection or symbol. Locals, globals, calls,
/// `js*`, `#js`, regexes (`lastIndex` is mutable), `#inst` (a mutable `Date`)
/// and `#queue` are not, and neither is a set literal whose members are not
/// all scalars, keywords or symbols, because its construction may throw.
/// Neither is a literal whose construction reads a core operation in
/// `replaced` (sorted runtime names), which a `set!` may change before an
/// evaluation: `with-meta`, `uuid`, or the type of an empty collection.
/// Memoized in `classes`, so classifying every literal of a program is linear.
fn constant_nodes(
    program: &Program,
    replaced: &[&str],
    classes: &mut [ConstantClass],
    id: ExprId,
) -> Option<u32> {
    match classes[id.0] {
        ConstantClass::Constant(nodes) => return Some(nodes),
        ConstantClass::Variable => return None,
        ConstantClass::Unknown => {}
    }
    let fixed = |name: &str| replaced.binary_search(&name).is_err();
    let kind = &program.exprs[id.0].kind;
    let nodes = if scalar_constant(kind) {
        Some(1)
    } else if let Some(empty) = EmptyCollection::of(kind) {
        fixed(empty.core_type()).then_some(1)
    } else {
        match composite(program, kind) {
            Some((Composite::CheckedSet, _)) | None => None,
            Some((Composite::WithMeta, _)) if !fixed("withMeta") => None,
            Some((Composite::Uuid, _)) if !fixed("uuid") => None,
            Some((Composite::WithMeta, items))
                if !matches!(
                    program.exprs[items[0].0].kind,
                    ExprKind::Vector(_)
                        | ExprKind::List(_)
                        | ExprKind::Map(_)
                        | ExprKind::Set(_)
                        | ExprKind::LiteralMap(_)
                        | ExprKind::LiteralSet(_)
                        | ExprKind::Symbol(_)
                        | ExprKind::WithMeta { .. }
                ) =>
            {
                None
            }
            Some((_, items)) => items.iter().try_fold(1u32, |nodes, &item| {
                Some(nodes.saturating_add(constant_nodes(program, replaced, classes, item)?))
            }),
        }
    };
    classes[id.0] = nodes.map_or(ConstantClass::Variable, ConstantClass::Constant);
    nodes
}

/// The constant table of `Advanced` application output
/// (docs/advanced-optimization.md, "Phase 1 constant table extension
/// points" and "Constant collection literals"). Every distinct constant the
/// emitted code reads is declared once, `const k<n> = <initializer>;`, at the
/// top of the output closure after the runtime slice, and each site reads
/// `k<n>`. Large composite constants are lazy cells instead,
/// `let c<n>; const k<n> = () => c<n> ??= <initializer>;`, read as `k<n>()`.
/// Entries are numbered and declared in first-use order, and an entry's
/// initializer is written before the entry is numbered, so the constants it
/// reads come first. Only emitted sites add entries, so constants are inert
/// for reachability.
struct ConstantTable<'a> {
    /// The declarations, one line each.
    declarations: String,
    /// Entry numbers by key.
    numbers: HashMap<ConstantKey<'a>, usize>,
    /// Entry numbers by `NamedLiteralId` and kind (keyword, symbol), so a
    /// repeated site reads its number without hashing its spelling.
    named: Vec<[Option<usize>; 2]>,
    /// Whether each entry, by number, is a lazy cell.
    lazy: Vec<bool>,
    /// The number of composite entries.
    composites: usize,
    /// What the pool knows about each expression, by `ExprId`.
    classes: Vec<ConstantClass>,
    /// The entries' hashes, which composite initializers embed
    /// (light-cljs-219), when the program extends no core protocol beyond its
    /// own types: then the runtime's `hash` of a constant is a function of
    /// its structure, which `crate::value_hash` computes.
    hashes: Option<ConstantHashes<'a>>,
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
    /// At `Advanced`: the retained globals, by `GlobalId`.
    retained: Option<&'a [bool]>,
    /// At `Advanced`: devirtualized protocol method calls and their slots.
    direct_methods: Option<&'a std::collections::HashMap<ExprId, String>>,
    /// Whether types get their static `getBasis` method: below `Advanced`
    /// always, at `Advanced` when the program names it.
    type_basis: bool,
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
    /// At `Advanced`: direct calls and method entries (`crate::call_shapes`).
    shapes: Option<&'a CallShapes>,
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

    /// At `Advanced`, keep the unit declaring `binding` in the slice although
    /// the output does not read it: the runtime reaches it through a hook that
    /// its install effect fills in (`typeHooks`).
    fn keep_runtime(&mut self, binding: &'static str) {
        if self.replaced_core.is_some() {
            self.runtime
                .record(RuntimeName::direct(binding, None), self.site, NameUse::Keep);
        }
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

    /// The number of the table entry for `key`, declaring it on first use:
    /// `initializer` writes the value expression, reading the runtime through
    /// [`Emitter::rt`] as usual (the first site is the one the retention
    /// report names). The entry is eager, or a lazy cell when `lazy` is set;
    /// an existing entry keeps how it was declared. Sites write the entry with
    /// [`Emitter::constant_reference`]. `Advanced` only.
    fn constant_entry(
        &mut self,
        key: ConstantKey<'a>,
        lazy: bool,
        initializer: impl FnOnce(&mut Self),
    ) -> usize {
        let constants = self
            .constants
            .as_ref()
            .expect("constants are hoisted at Advanced");
        if let Some(&number) = constants.numbers.get(&key) {
            return number;
        }
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
        let number = constants.lazy.len();
        if lazy {
            writeln!(
                constants.declarations,
                "  let c{number}; const k{number} = () => c{number} ??= {value};"
            )
            .unwrap();
        } else {
            writeln!(constants.declarations, "  const k{number} = {value};").unwrap();
        }
        constants.composites += usize::from(matches!(key, ConstantKey::Composite { .. }));
        constants.lazy.push(lazy);
        constants.numbers.insert(key, number);
        number
    }

    /// A read of table entry `number`: `k<n>`, or `k<n>()` for a lazy cell.
    fn constant_reference(&mut self, number: usize) {
        let lazy = self.constants.as_ref().unwrap().lazy[number];
        write!(self.output, "k{number}{}", if lazy { "()" } else { "" }).unwrap();
    }

    /// `rt.keyword(namespace, name)` or `rt.symbol(namespace, name)`.
    fn construct_named(&mut self, literal: NamedLiteralId, symbol: bool) {
        let name = &self.program.named_literals[literal.0];
        let constructor = if symbol { "symbol" } else { "keyword" };
        self.rt(RuntimeName::export(constructor));
        self.output.push('(');
        if let Some(namespace) = name.namespace {
            self.string(namespace);
        } else {
            self.output.push_str("null");
        }
        self.output.push_str(", ");
        self.string(name.name);
        self.output.push(')');
    }

    /// A keyword or quoted symbol literal: a table constant at `Advanced`,
    /// else `rt.keyword(namespace, name)` or `rt.symbol(namespace, name)`.
    fn named_literal(&mut self, literal: NamedLiteralId, symbol: bool) {
        if self.constants.is_none() {
            return self.construct_named(literal, symbol);
        }
        let number = self.named_entry(literal, symbol);
        self.constant_reference(number);
    }

    /// The table entry of a keyword or quoted symbol literal (`Advanced`).
    fn named_entry(&mut self, literal: NamedLiteralId, symbol: bool) -> usize {
        let constants = self.constants.as_ref().unwrap();
        if let Some(number) = constants.named[literal.0][usize::from(symbol)] {
            return number;
        }
        let program = self.program;
        let name = &program.named_literals[literal.0];
        let namespace = name.namespace.map(|id| &program.strings[id.0][..]);
        let name = &program.strings[name.name.0][..];
        let key = if symbol {
            ConstantKey::Symbol { namespace, name }
        } else {
            ConstantKey::Keyword { namespace, name }
        };
        let number = self.constant_entry(key, false, |emitter| {
            emitter.construct_named(literal, symbol)
        });
        let constants = self.constants.as_mut().unwrap();
        constants.named[literal.0][usize::from(symbol)] = Some(number);
        if let Some(hashes) = &mut constants.hashes {
            hashes.named(number, symbol, namespace, name);
        }
        number
    }

    /// At `Advanced`, write a constant collection literal, `with-meta` of
    /// one or `#uuid` literal `id` as a read of its pool entry and return
    /// true; an empty collection reads its singleton. Return false for
    /// anything else, which its site builds.
    fn pooled(&mut self, id: ExprId) -> bool {
        let Some(nodes) = self.constant_nodes(id) else {
            return false;
        };
        let part = self.constant_part(id, nodes >= LAZY_CONSTANT_NODES);
        self.part(part);
        true
    }

    /// The size of `id` when it is a constant the pool hoists (`Advanced`).
    fn constant_nodes(&mut self, id: ExprId) -> Option<u32> {
        let constants = self.constants.as_mut()?;
        let replaced = self.replaced_core.unwrap_or_default();
        constant_nodes(self.program, replaced, &mut constants.classes, id)
    }

    /// The part a constant contributes to its parent, declaring the pool
    /// entries of it and its nested constants bottom-up (hash-consing): equal
    /// subterms are one entry. New entries are lazy cells when `lazy` is set,
    /// so the parts of a lazy constant are built with it.
    fn constant_part(&mut self, id: ExprId, lazy: bool) -> ConstantPart<'a> {
        let program = self.program;
        let kind = &program.exprs[id.0].kind;
        if let Some(empty) = EmptyCollection::of(kind) {
            return ConstantPart::Empty(empty);
        }
        match kind {
            ExprKind::Nil => ConstantPart::Nil,
            ExprKind::Bool(value) => ConstantPart::Bool(*value),
            ExprKind::Number(value) => ConstantPart::Number(if value.is_nan() {
                f64::NAN.to_bits()
            } else {
                value.to_bits()
            }),
            ExprKind::String(string) => ConstantPart::String(&program.strings[string.0]),
            ExprKind::Keyword(literal) => ConstantPart::Entry(self.named_entry(*literal, false)),
            ExprKind::Symbol(literal) => ConstantPart::Entry(self.named_entry(*literal, true)),
            _ => {
                let (constructor, items) =
                    composite(program, kind).expect("a classified constant is a composite");
                let parts = items
                    .iter()
                    .map(|&item| self.constant_part(item, lazy))
                    .collect::<Box<[_]>>();
                let key = ConstantKey::Composite {
                    constructor,
                    parts: parts.clone(),
                };
                let (mut created, mut hash) = (false, None);
                let number = self.constant_entry(key, lazy, |emitter| {
                    created = true;
                    hash = emitter
                        .constants
                        .as_ref()
                        .unwrap()
                        .hashes
                        .as_ref()
                        .and_then(|hashes| hashes.composite_hash(constructor, &parts));
                    // `withMeta` copies its value's hash, and a `#uuid`
                    // caches its own.
                    let seeded = hash
                        .filter(|_| !matches!(constructor, Composite::WithMeta | Composite::Uuid));
                    if seeded.is_some() {
                        emitter.rt(RuntimeName::export("hashed"));
                        emitter.output.push('(');
                    }
                    emitter.construct(constructor, parts.len(), |emitter, index| {
                        emitter.part(parts[index]);
                    });
                    if let Some(hash) = seeded {
                        write!(emitter.output, ", {hash})").unwrap();
                    }
                });
                if let Some(hashes) = &mut self.constants.as_mut().unwrap().hashes
                    && created
                {
                    hashes.composite(number, constructor, parts, hash);
                }
                ConstantPart::Entry(number)
            }
        }
    }

    /// At `Advanced`, build a map or set literal on its key shape, a pooled
    /// [`ConstantKey::Shape`], and return true (light-cljs-220;
    /// docs/advanced-optimization.md, "Constant pool extension points"):
    ///
    /// - a map literal whose keys are distinct constants and whose values are
    ///   not all constants, `rt$mapFromShape(k<n>, [values])`, or past 8
    ///   entries, where sites call `mapFromArrays`, `rt$hashMapFromShape`;
    /// - a set literal whose first members are distinct scalar, keyword or
    ///   symbol constants and whose other members are not all such constants,
    ///   `rt$setFromShape(k<n>, [other members])`, which adds and checks the
    ///   other members in order as `literalSetFromArray` does.
    ///
    /// Values and members are evaluated in order, once each, as before; the
    /// keys need no lookups. Return false for any other literal, which its
    /// site builds. Telling keys apart needs the program's core protocols
    /// intact, as the pool's hashes do (`ConstantHashes`).
    fn shaped(&mut self, id: ExprId) -> bool {
        if self
            .constants
            .as_ref()
            .is_none_or(|constants| constants.hashes.is_none())
        {
            return false;
        }
        let program = self.program;
        match &program.exprs[id.0].kind {
            ExprKind::Map(items) => self.shaped_map(items),
            ExprKind::Set(items) => self.shaped_set(items),
            _ => false,
        }
    }

    /// [`Emitter::shaped`] for the items of a map literal.
    fn shaped_map(&mut self, items: &'a [ExprId]) -> bool {
        let mut lazy = Vec::with_capacity(items.len() / 2);
        for &key in items.iter().step_by(2) {
            let Some(nodes) = self.constant_nodes(key) else {
                return false;
            };
            lazy.push(nodes >= LAZY_CONSTANT_NODES);
        }
        let keys = items
            .iter()
            .step_by(2)
            .zip(&lazy)
            .map(|(&key, &lazy)| self.constant_part(key, lazy))
            .collect::<Box<[_]>>();
        let hashes = self.constants.as_ref().unwrap().hashes.as_ref().unwrap();
        let Some(key_hashes) = keys
            .iter()
            .map(|&key| hashes.part(key))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        if !hashes.distinct(&keys) {
            return false;
        }
        // Past 8 entries the shape also holds the hash trie.
        let hash_map = keys.len() > 8;
        let shape = ConstantKey::Shape {
            set: false,
            keys: keys.clone(),
        };
        let number = self.constant_entry(shape, lazy.contains(&true), |emitter| {
            emitter.rt(RuntimeName::export(if hash_map {
                "hashMapShape"
            } else {
                "mapShape"
            }));
            emitter.output.push('(');
            emitter.parts(&keys);
            emitter.output.push_str(", [");
            for (index, hash) in key_hashes.iter().enumerate() {
                if index > 0 {
                    emitter.output.push_str(", ");
                }
                write!(emitter.output, "{hash}").unwrap();
            }
            emitter.output.push_str("])");
        });
        let constructor = if hash_map {
            "hashMapFromShape"
        } else {
            "mapFromShape"
        };
        let values = items.iter().skip(1).step_by(2).copied();
        self.shaped_site(constructor, number, values);
        true
    }

    /// [`Emitter::shaped`] for the members of a set literal.
    fn shaped_set(&mut self, items: &'a [ExprId]) -> bool {
        let program = self.program;
        let first = items
            .iter()
            .take_while(|item| scalar_constant(&program.exprs[item.0].kind))
            .count();
        // A literal of such constants only is pooled instead.
        if first == 0 || first == items.len() {
            return false;
        }
        let members = items[..first]
            .iter()
            .map(|&member| self.constant_part(member, false))
            .collect::<Box<[_]>>();
        let hashes = self.constants.as_ref().unwrap().hashes.as_ref().unwrap();
        if !hashes.distinct(&members) {
            return false;
        }
        let shape = ConstantKey::Shape {
            set: true,
            keys: members.clone(),
        };
        let number = self.constant_entry(shape, false, |emitter| {
            emitter.rt(RuntimeName::export("setShape"));
            emitter.output.push('(');
            emitter.parts(&members);
            emitter.output.push(')');
        });
        self.shaped_site("setFromShape", number, items[first..].iter().copied());
        true
    }

    /// `[part, ...]`.
    fn parts(&mut self, parts: &[ConstantPart<'a>]) {
        self.output.push('[');
        for (index, &part) in parts.iter().enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            self.part(part);
        }
        self.output.push(']');
    }

    /// A site built on shape entry `number`: `rt$<constructor>(k<n>, [items])`.
    fn shaped_site(
        &mut self,
        constructor: &'static str,
        number: usize,
        items: impl Iterator<Item = ExprId>,
    ) {
        self.rt(RuntimeName::export(constructor));
        self.output.push('(');
        self.constant_reference(number);
        self.output.push_str(", [");
        for (index, item) in items.enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            self.value(item);
        }
        self.output.push_str("])");
    }

    /// Write a constant part as an initializer or a site reads it.
    fn part(&mut self, part: ConstantPart<'a>) {
        match part {
            ConstantPart::Nil => self.output.push_str("null"),
            ConstantPart::Bool(value) => self.output.push_str(if value { "true" } else { "false" }),
            ConstantPart::Number(bits) => self.number(f64::from_bits(bits)),
            ConstantPart::String(units) => self.string_units(units),
            ConstantPart::Empty(empty) => self.empty_collection(empty),
            ConstantPart::Entry(number) => self.constant_reference(number),
        }
    }

    /// A number literal.
    fn number(&mut self, value: f64) {
        if value.is_nan() {
            self.output.push_str("NaN");
        } else if value == f64::INFINITY {
            self.output.push_str("Infinity");
        } else if value == f64::NEG_INFINITY {
            self.output.push_str("-Infinity");
        } else {
            write!(self.output, "{value}").unwrap();
        }
    }

    /// The runtime's empty singleton, `rt.core.PersistentVector.EMPTY`
    /// (`rt$PersistentVectorValue.EMPTY` at `Advanced`), which an empty
    /// literal reads at every level instead of allocating.
    fn empty_collection(&mut self, empty: EmptyCollection) {
        self.rt(RuntimeName::core(empty.core_type()));
        self.output.push_str(".EMPTY");
    }

    /// A call of a composite's runtime constructor with `count` items, which
    /// `item` writes by index: the one construction of both a pooled
    /// constant's initializer and a site that builds its value.
    fn construct(
        &mut self,
        constructor: Composite,
        count: usize,
        mut item: impl FnMut(&mut Self, usize),
    ) {
        let mut items = |emitter: &mut Self, indexes: &mut dyn Iterator<Item = usize>| {
            for (position, index) in indexes.enumerate() {
                if position > 0 {
                    emitter.output.push_str(", ");
                }
                item(emitter, index);
            }
        };
        self.rt(match constructor {
            Composite::Vector => RuntimeName::export("vectorFromArray"),
            Composite::List => RuntimeName::export("listFromArray"),
            // At `Advanced` an array-map literal keeps no hash map.
            Composite::ArrayMap if self.replaced_core.is_some() => {
                RuntimeName::direct("arrayMapFromArray", None)
            }
            Composite::ArrayMap => RuntimeName::export("mapFromArray"),
            Composite::HashMap => RuntimeName::export("mapFromArrays"),
            Composite::Set => RuntimeName::export("setFromArray"),
            Composite::CheckedSet => RuntimeName::export("literalSetFromArray"),
            Composite::LiteralMap => RuntimeName::export("constantMapFromArray"),
            Composite::LiteralSet => RuntimeName::export("constantSetFromArray"),
            Composite::WithMeta => RuntimeName::core("withMeta"),
            Composite::Uuid => RuntimeName::core("uuid"),
        });
        match constructor {
            Composite::HashMap => {
                self.output.push_str("([");
                items(self, &mut (0..count).step_by(2));
                self.output.push_str("], [");
                items(self, &mut (1..count).step_by(2));
                self.output.push_str("])");
            }
            Composite::WithMeta | Composite::Uuid => {
                self.output.push('(');
                items(self, &mut (0..count));
                self.output.push(')');
            }
            _ => {
                self.output.push_str("([");
                items(self, &mut (0..count));
                self.output.push_str("])");
            }
        }
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

    /// A devirtualized protocol method call (`crate::rta`): the receiver's
    /// `slot` called with the receiver as `this` and first argument, as the
    /// dispatcher calls it, `(r)["ns$P$m$arity$2"](r, x)`. Unless reading
    /// every argument twice is reading it once, the arguments are evaluated
    /// first, in order, as for the dispatcher call:
    /// `((r0, a1) => r0["ns$P$m$arity$2"](r0, a1))(receiver, x)`.
    fn direct_method_call(&mut self, slot: &str, args: &[ExprId]) {
        let (receiver, rest) = args
            .split_first()
            .expect("a protocol method takes its receiver");
        if args.iter().all(|arg| self.repeatable(*arg)) {
            self.output.push('(');
            self.value(*receiver);
            self.output.push(')');
            self.member(slot);
            self.output.push('(');
            self.arguments(args);
            self.output.push(')');
            return;
        }
        let names = (0..args.len())
            .map(|index| {
                if index == 0 {
                    "r0".to_owned()
                } else {
                    format!("a{index}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        write!(self.output, "(({names}) => r0").unwrap();
        self.member(slot);
        write!(self.output, "({names}))(").unwrap();
        self.value(*receiver);
        for arg in rest {
            self.output.push_str(", ");
            self.value(*arg);
        }
        self.output.push(')');
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
            // An atom without options has neither metadata nor a validator,
            // so it keeps none of the option parsing (`seqToMap`, `into`).
            (C::Atom, [value]) if !self.replaced(C::AtomType.runtime_name()) => {
                self.output.push_str("new ");
                self.rt(RuntimeName::core(C::AtomType.runtime_name()));
                self.output.push('(');
                self.value(*value);
                self.output.push_str(", null, null, null)");
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
        // removed, is no statement at `Advanced`, and neither is a discarded
        // definition without a value (`declare`), which assigns nothing.
        if context == ExprContext::Discard
            && self.replaced_core.is_some()
            && matches!(
                self.program.exprs[id.0].kind,
                ExprKind::Nil
                    | ExprKind::Def {
                        init: None,
                        test: None,
                        once: false,
                        ..
                    }
            )
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
            // A discarded collection, keyword or symbol constant has no effect
            // at `Advanced`, so it adds no table entry.
            kind if context == ExprContext::Discard
                && !matches!(
                    kind,
                    ExprKind::Nil | ExprKind::Bool(_) | ExprKind::Number(_) | ExprKind::String(_)
                )
                && self.constant_nodes(id).is_some() => {}
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
        let entries: Vec<String> = (0..methods.len())
            .map(|index| format!("m{index}"))
            .collect();
        for (entry, method) in entries.iter().zip(methods) {
            self.indentation();
            write!(self.output, "const {entry} = ").unwrap();
            self.method_with_ifn_captures(None, method, ifn_call);
            self.output.push_str(";\n");
        }
        let function = name.map_or_else(|| "f".into(), |name| format!("l{}", name.0));
        self.indentation();
        write!(self.output, "const {function} = ").unwrap();
        self.dispatcher(&entries, methods, macro_function);
        self.output.push_str(";\n");
        self.arity_properties(&function, &entries, methods, macro_function, true);
        self.indentation();
        writeln!(self.output, "return {function};").unwrap();
        self.indent -= 1;
        self.indentation();
        self.output.push_str("})()");
    }

    /// The `arguments.length` dispatcher over methods bound to `entries`:
    /// `function(a0, a1) { switch (arguments.length) { ... } }`.
    fn dispatcher(&mut self, entries: &[String], methods: &[FnMethod], macro_function: bool) {
        self.output.push_str("function(");
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
        for (entry, method) in entries
            .iter()
            .zip(methods)
            .filter(|(_, m)| m.rest.is_none())
        {
            self.indentation();
            writeln!(
                self.output,
                "case {}: return {entry}.apply(this, arguments);",
                method.params.len()
            )
            .unwrap();
        }
        self.indentation();
        if let Some((entry, method)) = entries.iter().zip(methods).find(|(_, m)| m.rest.is_some()) {
            // Like CLJS, the variadic default also receives underfilled calls.
            write!(self.output, "default: return {entry}.call(this, ").unwrap();
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
        self.output.push('}');
    }

    /// The arity properties of the dispatcher `function` over methods bound to
    /// `entries`: one statement each, or with `statements` false, each after
    /// `, ` as the items of a comma expression.
    fn arity_properties(
        &mut self,
        function: &str,
        entries: &[String],
        methods: &[FnMethod],
        macro_function: bool,
        statements: bool,
    ) {
        let end = if statements { ";\n" } else { "" };
        let start = |emitter: &mut Self| {
            if statements {
                emitter.indentation();
            } else {
                emitter.output.push_str(", ");
            }
        };
        for (entry, method) in entries.iter().zip(methods) {
            start(self);
            if method.rest.is_some() {
                write!(
                    self.output,
                    "{function}.cljs$core$IFn$_invoke$arity$variadic = {entry}{end}"
                )
                .unwrap();
                start(self);
                write!(
                    self.output,
                    "{function}.cljs$lang$maxFixedArity = {}{end}",
                    method.params.len()
                )
                .unwrap();
                start(self);
                write!(self.output, "{function}.cljs$lang$applyTo = (args) => ").unwrap();
                self.rt(RuntimeName::export("fnApplyTo"));
                write!(self.output, "({entry}, {}, args){end}", method.params.len()).unwrap();
            } else {
                write!(
                    self.output,
                    "{function}.cljs$core$IFn$_invoke$arity${} = {entry}{end}",
                    method.params.len()
                )
                .unwrap();
            }
        }
        if macro_function && methods.iter().all(|method| method.rest.is_none()) {
            start(self);
            write!(
                self.output,
                "{function}.cljs$lang$maxFixedArity = {}{end}",
                methods
                    .iter()
                    .map(|method| method.params.len())
                    .max()
                    .unwrap()
            )
            .unwrap();
        }
    }

    /// The definition of `global`, whose initializer `init` is the `fn`
    /// expression `function` in one-form `do`s, with each method bound to an
    /// entry of its own (`FunctionShape::Entries`, `crate::call_shapes`):
    /// `(g3_0 = function() {...}, g3_1 = function(l2) {...})`. With its
    /// `dispatcher` the global is the dispatcher over the same entries, with
    /// its arity properties, and the value of the definition.
    fn method_entries(
        &mut self,
        global: GlobalId,
        init: ExprId,
        function: ExprId,
        dispatcher: bool,
    ) {
        let program = self.program;
        let mut wrapper = init;
        while wrapper != function {
            self.used.insert(wrapper);
            let ExprKind::Do(items) = &program.exprs[wrapper.0].kind else {
                unreachable!("a fn in one-form do forms")
            };
            wrapper = items[0];
        }
        self.used.insert(function);
        let ExprKind::Fn { methods, .. } = &program.exprs[function.0].kind else {
            unreachable!("method entries of a fn")
        };
        let site = self.site.replace(function);
        let entries: Vec<String> = (0..methods.len())
            .map(|index| format!("g{}_{index}", global.0))
            .collect();
        self.output.push('(');
        for (index, (entry, method)) in entries.iter().zip(methods).enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            write!(self.output, "{entry} = ").unwrap();
            self.method_with_ifn_captures(None, method, false);
        }
        if dispatcher {
            let name = format!("g{}", global.0);
            write!(self.output, ", {name} = ").unwrap();
            self.dispatcher(&entries, methods, false);
            self.arity_properties(&name, &entries, methods, false, false);
            write!(self.output, ", {name}").unwrap();
        }
        self.output.push(')');
        self.site = site;
    }

    /// A call whose method is chosen at compile time (`crate::call_shapes`):
    /// its entry, `g3_1(a)`, or the callee itself when it is the method,
    /// `g3(a, b)`. A variadic method receives the rest sequence its dispatcher
    /// would build, `g3_2(a, rt$fnRest([b, c], 0))`, or `null`. A method that
    /// may read `this` keeps `.call(null, ...)`.
    fn direct_call(&mut self, callee: ExprId, args: &[ExprId], direct: DirectCall) {
        match direct.entry {
            Some((global, method)) => {
                self.used.insert(callee);
                write!(self.output, "g{}_{method}", global.0).unwrap();
            }
            // A member read, `captures3.l5`, would pass its object as `this`.
            None => match self.program.exprs[callee.0].kind {
                ExprKind::Local(local) if !self.plain_local(local) => {
                    self.output.push_str("(0, ");
                    self.value(callee);
                    self.output.push(')');
                }
                _ => self.value(callee),
            },
        }
        let mut first = !direct.receiver;
        self.output
            .push_str(if direct.receiver { ".call(null" } else { "(" });
        let fixed = direct.rest.unwrap_or(args.len());
        for &arg in &args[..fixed] {
            if !std::mem::take(&mut first) {
                self.output.push_str(", ");
            }
            self.value(arg);
        }
        if direct.rest.is_some() {
            if !first {
                self.output.push_str(", ");
            }
            if args.len() > fixed {
                self.rt(RuntimeName::export("fnRest"));
                self.output.push_str("([");
                self.arguments(&args[fixed..]);
                self.output.push_str("], 0)");
            } else {
                self.output.push_str("null");
            }
        }
        self.output.push(')');
    }

    /// Whether [`Emitter::local_value`] writes `id` as an identifier or a
    /// call, rather than a member read.
    fn plain_local(&self, id: LocalId) -> bool {
        !self.captures.iter().any(|(local, _)| *local == id)
            && (self.reify_captures.iter().any(|(local, _)| *local == id)
                || !self.program.field_bindings.contains_key(&id))
    }

    /// Whether the `do` form `id` may be a comma expression in value
    /// position: none of its forms reads the function scope (`arguments`, or
    /// `this` from raw JavaScript), which a function around them would
    /// replace, or holds a definition with `:test` metadata, which skips its
    /// test when it is the value a function returns, or enters a namespace.
    fn sequence_keeps_scope(&self, id: ExprId) -> bool {
        let program = self.program;
        let ExprKind::Do(items) = &program.exprs[id.0].kind else {
            return false;
        };
        if items
            .iter()
            .any(|item| program.namespace_entries.contains_key(item))
        {
            return false;
        }
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            match program.exprs[id.0].kind {
                ExprKind::JsArguments
                | ExprKind::CopyArguments(_)
                | ExprKind::JsTemplate { .. }
                | ExprKind::Def { test: Some(_), .. } => return false,
                ExprKind::Fn { .. } => (),
                _ => pending.extend(crate::program_fragment::children(program, id)),
            }
        }
        true
    }

    /// A `do` in value position as a comma expression, `(a(), b)`, without
    /// the forms before the last whose evaluation has no effect, or as its
    /// last form alone.
    fn sequence(&mut self, items: &[ExprId]) {
        let Some((&last, before)) = items.split_last() else {
            self.output.push_str("null");
            return;
        };
        let mut effects = Vec::new();
        for &item in before {
            self.used.insert(item);
            let kind = &self.program.exprs[item.0].kind;
            if !(matches!(
                kind,
                ExprKind::Nil | ExprKind::Bool(_) | ExprKind::Number(_) | ExprKind::String(_)
            ) || self.constant_nodes(item).is_some())
            {
                effects.push(item);
            }
        }
        if effects.is_empty() {
            self.value(last);
            return;
        }
        self.output.push('(');
        for item in effects {
            self.value(item);
            self.output.push_str(", ");
        }
        self.value(last);
        self.output.push(')');
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
                if *record && constructor.is_some() {
                    // `makeType` reads the record methods from `typeHooks`.
                    self.keep_runtime("recordMethods");
                }
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
                    // At `Advanced` only a program that names `getBasis` has it
                    // (`crate::rta`), as Closure removes unread properties.
                    if self.type_basis {
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
                    }
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
                // At `Advanced` the protocol value exists only when something
                // reads it (`crate::rta`), and so do the dispatchers, which
                // rapid type analysis leaves in `methods`.
                if self.retained.is_none_or(|retained| retained[global.0]) {
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
                }
                for method in methods {
                    self.product(method.global);
                    self.output.push_str(" = ");
                    self.rt(RuntimeName::export("protocolMethod"));
                    // The dispatcher reads these from `typeHooks`.
                    if *via_metadata {
                        self.keep_runtime("metadataMethod");
                    }
                    if method.variadic.is_some() {
                        self.keep_runtime("fnRest");
                    }
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
            ExprKind::Number(value) => self.number(*value),
            ExprKind::String(id) => self.string(*id),
            // At `Advanced`, constant literals read the pool.
            ExprKind::Vector(_)
            | ExprKind::List(_)
            | ExprKind::Map(_)
            | ExprKind::Set(_)
            | ExprKind::LiteralMap(_)
            | ExprKind::LiteralSet(_)
            | ExprKind::WithMeta { .. }
            | ExprKind::ReaderLiteral { .. }
                if self.pooled(id) => {}
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
                // `#js` data keeps its compiler form `withMeta`.
                let core = composite(self.program, &self.program.exprs[id.0].kind).is_some();
                self.rt(if core {
                    RuntimeName::core("withMeta")
                } else {
                    RuntimeName::compiler_forms("withMeta")
                });
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
            // Below `Advanced`, and for literals that are not constants, each
            // evaluation builds the value; empty literals read the runtime's
            // singletons at every level. At `Advanced` a map or set literal
            // with distinct constant keys or first members builds on its key
            // shape.
            kind @ (ExprKind::Vector(_)
            | ExprKind::List(_)
            | ExprKind::Map(_)
            | ExprKind::Set(_)
            | ExprKind::LiteralMap(_)
            | ExprKind::LiteralSet(_)) => {
                let program = self.program;
                if let Some(empty) = EmptyCollection::of(kind) {
                    self.empty_collection(empty);
                } else if self.shaped(id) {
                } else {
                    let (constructor, items) = composite(program, kind).unwrap();
                    self.construct(constructor, items.len(), |emitter, index| {
                        emitter.value(items[index]);
                    });
                }
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
                if let Some(init) = init
                    && let Some(FunctionShape::Entries {
                        function,
                        dispatcher,
                    }) = self.shapes.map(|shapes| shapes.function(*global))
                {
                    self.method_entries(*global, *init, function, dispatcher);
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
            // At `Advanced` a `do` in value position is a comma expression,
            // where that keeps the function scope its forms see.
            ExprKind::Do(items) if self.shapes.is_some() && self.sequence_keeps_scope(id) => {
                self.sequence(items);
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
                if let Some(slot) = self.direct_methods.and_then(|methods| methods.get(&id)) {
                    self.direct_method_call(slot, args);
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
                if let Some(direct) = self.shapes.and_then(|shapes| shapes.call(id)) {
                    self.direct_call(*callee, args, direct);
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
                "c",
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
