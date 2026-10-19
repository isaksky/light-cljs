//! Compile-time runtime machinery and the namespace registration at `Advanced`
//! (light-cljs-206; docs/advanced-optimization.md, Phase 1 step 5).
//!
//! The manifest marks the units of the compiler, the macro host and the REPL
//! `compile-time`: the compiler form bridge, declaration demand and
//! transactions, the namespace registry, the macro reader context, `inspect`
//! and the `eval` stub. Application code reaches them only through reflection,
//! and an `Advanced` program registers its namespaces (and switches `*ns*` at
//! namespace entries) only when it does. The declaration hooks woven into
//! application paths start as plain versions in application slices.
use super::optimization::{Sources, execute, readable};
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry;
use light_cljs::runtime_manifest::{CoreReference, RuntimeManifest, UnitId};
use light_cljs::runtime_slice::REGISTRY_CELLS;
use light_cljs::{
    ApplicationOptions, CoreFnId, MacroHost, OptimizationLevel, Program, analyze, parse,
    prepare_application,
};

fn manifest() -> &'static RuntimeManifest<'static> {
    RuntimeManifest::embedded()
}

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn emit(program: &Program, level: OptimizationLevel) -> String {
    prepare_application(program, ApplicationOptions::new(level)).emit_js()
}

/// The readable output (`pseudo_names`), whose text the tests read.
fn emit_readable(program: &Program, level: OptimizationLevel) -> String {
    prepare_application(program, readable(level)).emit_js()
}

/// What the program's value prints as in Node, or the message it throws.
fn printed(js: &str) -> String {
    execute(&format!(
        "try {{ console.log(String({js})); }} catch (error) {{ console.log(\"threw \" + error.message); }}"
    ))
}

/// The compile-time units that keeping `roots` keeps, by name.
fn compile_time_units(roots: &[UnitId]) -> Vec<&'static str> {
    let manifest = manifest();
    manifest
        .reachable(roots.iter().copied())
        .iter()
        .zip(&manifest.units)
        .filter(|(kept, unit)| **kept && unit.compile_time)
        .map(|(_, unit)| unit.name)
        .collect()
}

/// The core operations whose runtime units reach compile-time machinery, or
/// that read a registry cell: the reflection a program can call.
const REFLECTION: &[&str] = &[
    "*ns*",
    "all-ns",
    "compiler-edn-read",
    "compiler-edn-read-string",
    "compiler-macro-var*",
    "compiler-reader-close",
    "compiler-reader-current-input",
    "compiler-reader-deregister-default-tag-parser",
    "compiler-reader-deregister-tag-parser",
    "compiler-reader-initial-tag-table",
    "compiler-reader-input-provider",
    "compiler-reader-parse-timestamp",
    "compiler-reader-parse-timestamp-fields",
    "compiler-reader-read",
    "compiler-reader-read-line",
    "compiler-reader-read-string",
    "compiler-reader-register-default-tag-parser",
    "compiler-reader-register-tag-parser",
    "compiler-reader-string-reader",
    "compiler-reader-with-input",
    "create-ns",
    "eval",
    "find-ns",
    "read",
    "read-line",
    "read-string",
    "the-ns",
];

#[test]
fn application_roots_reach_compile_time_units_only_through_reflection() {
    let manifest = manifest();
    let unit = |name| manifest.unit_named(name).unwrap();
    // `*ns*` is the registry cell that the namespace registration fills.
    assert_eq!(REGISTRY_CELLS, ["currentNs"]);
    assert_eq!(
        manifest.core_reference("currentNs"),
        Some(CoreReference::Cell("currentNs"))
    );
    let mut reflection = vec!["*ns*"];
    for core in CoreFnId::ALL {
        let operation = core.operation();
        if !compile_time_units(&operation.runtime_units(manifest)).is_empty() {
            reflection.push(operation.name);
        }
    }
    reflection.sort_unstable();
    reflection.dedup();
    assert_eq!(reflection, REFLECTION);

    // The runtime names the emitter writes outside core operations keep none,
    // except the namespace registration, macro functions, quoted `#js`
    // literals and the compile-time emission modes' `declarationReceiver`.
    for name in [
        "keyword",
        "symbol",
        "vector",
        "vectorFromArray",
        "listFromArray",
        "mapFromArray",
        "mapFromArrays",
        "mapShape",
        "mapFromShape",
        "hashMapShape",
        "hashMapFromShape",
        "setShape",
        "setFromShape",
        "queueFromArray",
        "jsObject",
        "protocol",
        "protocolPredicate",
        "protocolMethod",
        "extendType",
        "extendInstance",
        "makeReify",
        "reify",
        "makeType",
        "recordFromMap",
        "fnRest",
        "fnApplyTo",
        "makeVar",
        "definitionVar",
    ] {
        assert_eq!(
            compile_time_units(&[manifest.unit_of(name).unwrap()]),
            [] as [&str; 0],
            "{name}"
        );
    }
    for name in [
        "registerReaderNamespace",
        "theNs",
        "macroReaderFunction",
        "compilerForms",
        "declarationReceiver",
    ] {
        assert!(
            !compile_time_units(&[manifest.unit_of(name).unwrap()]).is_empty(),
            "{name}"
        );
    }
    // The hooks that application paths call are application units; the
    // compile-time units that install the compiler's versions are not reached.
    let protocols = compile_time_units(&[
        unit("directMethod"),
        unit("protocolPredicate"),
        unit("satisfies"),
        unit("protocol"),
        unit("protocolMethod"),
        unit("extendType"),
        unit("installMethods"),
        unit("addMethod"),
        unit("reader"),
    ]);
    for name in [
        "setDeclarationDemand",
        "declarationTransaction",
        "compilerFormBridge",
        "compilerForms",
    ] {
        assert!(!protocols.contains(&name), "{name}");
        assert!(manifest.unit(unit(name)).compile_time, "{name}");
    }
    for name in [
        "demandProtocol",
        "patchDeclaration",
        "setDeclaration",
        "declarationJournal",
    ] {
        assert!(!manifest.unit(unit(name)).compile_time, "{name}");
    }
}

/// Whether the output registers namespaces or switches `*ns*`
/// (`rt$coreCells.currentNs = rt$theNs(...)`), outside its runtime.
fn registers(js: &str) -> bool {
    js.contains("registerReaderNamespace(\"") || js.contains(".currentNs = rt")
}

/// `text` as the emitter spells a namespace or binding name in the namespace
/// registration: a string of `\u` escapes.
fn escaped(text: &str) -> String {
    text.encode_utf16()
        .map(|unit| format!("\\u{unit:04x}"))
        .collect()
}

#[test]
fn advanced_programs_register_namespaces_only_when_they_reach_reflection() {
    // The add unit alone: no namespace registration, so no symbol, namespace
    // or collection machinery.
    let sum = emit(
        &program("(js/console.log (+ 1 2))"),
        OptimizationLevel::Advanced,
    );
    let readable_sum = emit_readable(
        &program("(js/console.log (+ 1 2))"),
        OptimizationLevel::Advanced,
    );
    assert!(!registers(&readable_sum), "{readable_sum}");
    assert!(sum.len() < 1_000, "{} bytes:\n{sum}", sum.len());
    assert_eq!(execute(&sum), "3\n");
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        assert!(registers(&emit(&program("(+ 1 2)"), level)), "{level}");
    }
    // Reflection registers, and the program prints what `None` prints. The
    // reader is one unit, so EDN reading counts as reflection too; `eval` keeps
    // its stub; a quoted `#js` literal keeps the compiler form bridge.
    for (source, expected) in [
        ("(str *ns*)", "cljs.user"),
        ("(binding [*ns* (create-ns 'x)] (str *ns*))", "x"),
        ("(do (set! *ns* (create-ns 'y)) (str *ns*))", "y"),
        ("(some? (find-ns 'cljs.user))", "true"),
        ("(str (the-ns 'cljs.core))", "cljs.core"),
        ("(count (all-ns))", "2"),
        ("(str (create-ns 'z))", "z"),
        ("(pr-str (read-string \"`foo\"))", "(quote cljs.user/foo)"),
        (
            "(pr-str (read-string \"[{:a 1} #js [2]]\"))",
            "[{:a 1} #js [2]]",
        ),
        (
            "(eval '(+ 1 2))",
            "threw compiler eval service is not installed",
        ),
        ("(pr-str '#js [1 2])", "#js [1 2]"),
    ] {
        let program = program(source);
        let advanced = emit(&program, OptimizationLevel::Advanced);
        assert!(
            registers(&emit_readable(&program, OptimizationLevel::Advanced)),
            "{source}"
        );
        assert_eq!(printed(&advanced), format!("{expected}\n"), "{source}");
        assert_eq!(
            printed(&emit(&program, OptimizationLevel::None)),
            format!("{expected}\n"),
            "{source}"
        );
    }
    // `resolve`, `ns-publics` and `ns-interns` expand to vars at compile time,
    // and `ns-name` of a symbol reads the symbol: none of them is reflection.
    for (source, expected) in [
        ("(pr-str (resolve 'inc))", "#'cljs.core/inc"),
        (
            "(def a 1) (pr-str (ns-interns 'cljs.user))",
            "{a #'cljs.user/a}",
        ),
        ("(str (ns-name 'x))", "x"),
    ] {
        let program = program(source);
        let advanced = emit(&program, OptimizationLevel::Advanced);
        assert!(
            !registers(&emit_readable(&program, OptimizationLevel::Advanced)),
            "{source}"
        );
        assert_eq!(printed(&advanced), format!("{expected}\n"), "{source}");
        assert_eq!(
            printed(&emit(&program, OptimizationLevel::None)),
            format!("{expected}\n"),
            "{source}"
        );
    }
}

#[test]
fn a_registered_namespace_lists_only_retained_definitions() {
    // `inc` is read, so it is retained and still shadows `cljs.core/inc` in the
    // reader's syntax quote. `map` and `unused` are removed: at `Advanced` the
    // registration does not list them, so `map` resolves to `cljs.core/map`
    // (the advanced-mode contract), while lower levels list every definition.
    let source = r#"(def map 3) (def unused 5) (def inc 4)
                    (pr-str [inc (read-string "`inc") (read-string "`map")])"#;
    let program = program(source);
    let advanced = emit(&program, OptimizationLevel::Advanced);
    let readable_advanced = emit_readable(&program, OptimizationLevel::Advanced);
    let bindings = readable_advanced
        .lines()
        .find(|line| line.trim_start().starts_with("bindings: ["))
        .unwrap();
    assert!(bindings.contains(&escaped("inc")), "{bindings}");
    for removed in ["map", "unused"] {
        assert!(
            !bindings.contains(&escaped(removed)),
            "{removed}: {bindings}"
        );
    }
    assert_eq!(
        printed(&advanced),
        "[4 (quote cljs.user/inc) (quote cljs.core/map)]\n"
    );
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        let js = emit(&program, level);
        assert!(js.contains(&escaped("unused")), "{level}");
        assert_eq!(
            printed(&js),
            "[4 (quote cljs.user/inc) (quote cljs.user/map)]\n",
            "{level}"
        );
    }
}

#[test]
fn namespace_entries_switch_ns_only_when_the_program_reaches_reflection() {
    let entry = |current: bool| {
        let sources = Sources::new();
        sources.write(
            "app/util.cljs",
            &format!(
                "(ns app.util)\n(defn f [x] {})\n",
                if current {
                    "(str *ns* \"/\" x)"
                } else {
                    "(+ x 1)"
                }
            ),
        );
        sources.write(
            "app/main.cljs",
            "(ns app.main (:require [app.util :as u]))\n(u/f 1)\n",
        );
        crate::cache_contract::check_mixed(sources.0.path(), "app.main");
        analyze_namespace_entry(
            "app.main",
            std::slice::from_ref(sources.0.path()),
            &mut MacroHost::new(),
            AnalysisOptions::default(),
            &mut Vec::new(),
        )
        .unwrap()
    };
    let plain = entry(false);
    let advanced = emit(&plain, OptimizationLevel::Advanced);
    let readable_advanced = emit_readable(&plain, OptimizationLevel::Advanced);
    assert!(!registers(&readable_advanced), "{readable_advanced}");
    assert_eq!(printed(&advanced), "2\n");
    assert!(registers(&emit(&plain, OptimizationLevel::Prepare)));
    // `*ns*` is the namespace being loaded when the call runs, at every level.
    let current = entry(true);
    for level in OptimizationLevel::ALL {
        let js = emit(&current, level);
        for namespace in ["app.util", "app.main"] {
            assert!(js.contains(&escaped(namespace)), "{level}: {namespace}");
        }
        assert_eq!(printed(&js), "app.main/1\n", "{level}");
    }
}

#[test]
fn application_slices_keep_plain_declaration_hooks() {
    // Protocols, types, `extend-type` and multimethods reach the declaration
    // hooks; their slices keep the plain versions and no compile-time unit.
    let source = r#"(defprotocol Shape (area [s]))
                    (defrecord Rect [w h] Shape (area [_] (* w h)))
                    (extend-type number Shape (area [n] (* n n)))
                    (defmulti kind (fn [x] (if (number? x) :n :o)))
                    (defmethod kind :n [_] "number")
                    (defmethod kind :default [_] "other")
                    (pr-str [(area (->Rect 3 4)) (area 5) (satisfies? Shape 1)
                             (kind 1) (kind "s")])"#;
    let program = program(source);
    let prepared = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    );
    let report = prepared.retention();
    let manifest = manifest();
    for unit in &report.units {
        assert!(!manifest.unit(unit.unit).compile_time, "{}", unit.name);
    }
    let kept = report
        .units
        .iter()
        .map(|unit| unit.name)
        .collect::<Vec<_>>();
    for hook in [
        "demandProtocol",
        "patchDeclaration",
        "setDeclaration",
        "declarationJournal",
    ] {
        assert!(kept.contains(&hook), "{hook}");
    }
    let advanced = prepared.emit_js();
    let readable_advanced = emit_readable(&program, OptimizationLevel::Advanced);
    assert!(!registers(&readable_advanced));
    assert!(readable_advanced.contains("let demandProtocol = () => {}, demandMethod = () => {}"));
    let expected = "[12 25 true \"number\" \"other\"]\n";
    assert_eq!(printed(&advanced), expected);
    assert_eq!(printed(&emit(&program, OptimizationLevel::None)), expected);
}
