//! Direct runtime references (docs/advanced-optimization.md, Phase 1 step 3):
//! `Advanced` output reads each core operation from the runtime binding that
//! holds it (`rt$add(1, 2)`) instead of through the `core` hub
//! (`(rt.core.add).call(null, 1, 2)`), so no application slice keeps the hub.
//! The core table in the runtime (`coreCells` and `core` in
//! src/js_runtime.js) maps every runtime name to its binding, a member of one,
//! or a mutable cell. Core operations a program may replace (`set!`, `binding`,
//! `with-redefs`, `js*` holes) read through an output-level cell `core$<name>`; dynamic Vars
//! and the operations the runtime reads through a cell (`rand`, `flush`) keep
//! their `coreCells` cell. Every program here must print what `None` prints.

use super::optimization::execute;
use light_cljs::runtime_manifest::{CORE_CELLS, CORE_HUB, CoreReference, RUNTIME, RuntimeManifest};
use light_cljs::runtime_slice::RuntimeName;
use light_cljs::{
    ApplicationOptions, CoreFnId, OptimizationLevel, PreparedApplication, Program, analyze, parse,
    prepare_application,
};
use std::collections::BTreeSet;

fn manifest() -> &'static RuntimeManifest<'static> {
    RuntimeManifest::embedded()
}

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn prepared(source: &str, level: OptimizationLevel) -> PreparedApplication {
    prepare_application(&program(source), ApplicationOptions::new(level))
}

/// What the program prints, followed by its value, at `level`.
fn printed(source: &str, level: OptimizationLevel) -> String {
    let js = prepared(source, level).emit_js();
    execute(&format!(
        "const value = {js};\nconsole.log(typeof value === \"function\" ? \"#function\" : value);"
    ))
}

/// The `Advanced` output of `source`, after checking that it prints what the
/// `None` output prints.
fn advanced_matches_none(source: &str) -> String {
    let expected = printed(source, OptimizationLevel::None);
    assert_eq!(
        printed(source, OptimizationLevel::Advanced),
        expected,
        "{source}"
    );
    prepared(source, OptimizationLevel::Advanced).emit_js()
}

/// Whether `js` reads an output-level core operation cell, a `core$<name>`
/// token (not the tail of an ABI name such as `cljs$core$IFn$_invoke$arity$1`).
fn cell_reads(js: &str) -> bool {
    js.match_indices("core$").any(|(at, _)| {
        !js[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    })
}

/// The runtime names of every `CoreFnId`.
fn core_runtime_names() -> BTreeSet<String> {
    CoreFnId::ALL
        .into_iter()
        .map(|id| id.runtime_key().to_owned())
        .collect()
}

#[test]
fn console_log_max_calls_the_max_unit_directly() {
    // `(+ 1 2)` no longer reads the runtime at `Advanced`: its operation is
    // the JavaScript operator (light-cljs-210), so the output embeds no runtime.
    let sum = advanced_matches_none("(js/console.log (+ 1 2))");
    assert!(
        sum.contains("(host(\"console\")[\"log\"])((1 + 2))"),
        "{sum}"
    );
    assert!(!sum.contains("rt$"), "{sum}");
    let source = "(js/console.log (max 1 2))";
    let js = advanced_matches_none(source);
    assert!(
        js.contains("(host(\"console\")[\"log\"])(rt$max(1, 2))"),
        "{js}"
    );
    let advanced = prepared(source, OptimizationLevel::Advanced);
    let report = advanced.retention();
    assert_eq!(report.roots[0].name, RuntimeName::direct("max", None));
    assert!(report.units.iter().all(|unit| unit.name != CORE_HUB));
    assert!(report.units.iter().any(|unit| unit.name == "max"));
    let prepare = prepared(source, OptimizationLevel::Prepare).emit_js();
    assert!(prepare.contains("(rt.core.max).call(null, 1, 2)"));
    // The slice keeps less than half of the runtime; the rest of the way to a
    // few kilobytes is demand-driven effects and namespace registration
    // (Phase 1 steps 4 and 5).
    assert!(
        js.len() * 2 < prepare.len(),
        "{} of {}",
        js.len(),
        prepare.len()
    );
}

#[test]
fn every_core_operation_resolves_through_the_core_table() {
    let manifest = manifest();
    let names = core_runtime_names();
    assert!(names.len() > 600, "{}", names.len());
    for name in &names {
        assert!(
            manifest.core_reference(name).is_some(),
            "`{name}` is missing from the core table"
        );
    }
    let mut cells = Vec::new();
    let mut table = BTreeSet::new();
    for (name, reference) in manifest.core_references() {
        table.insert(name.to_owned());
        match reference {
            CoreReference::Binding(binding) | CoreReference::Member(binding, _) => {
                assert!(manifest.unit_of(binding).is_some(), "{name}");
                assert_ne!(binding, CORE_HUB, "{name}");
            }
            CoreReference::Cell(cell) => {
                assert_eq!(cell, name);
                cells.push(name);
            }
        }
    }
    for dynamic in [
        "printFn",
        "printLength",
        "currentNs",
        "assertions",
        "rand",
        "flush",
    ] {
        assert!(cells.contains(&dynamic), "{dynamic}");
    }
    // The shared runtime's `core` map has exactly the table's keys.
    let keys = execute(&format!(
        "const rt = {};\nconsole.log(Object.keys(rt.core).sort().join(\"\\n\"));",
        RUNTIME.trim()
    ));
    assert_eq!(
        keys.lines().map(str::to_owned).collect::<BTreeSet<_>>(),
        table
    );
    assert!(names.is_subset(&table));
    assert!(manifest.unit_of(CORE_CELLS).is_some());
}

/// Every binding the core table names, sliced alone, is a runtime factory that
/// evaluates and returns a value with the members the table reads from it.
/// Bindings whose closures are equal share one factory; factories run in
/// batches to bound the size of each Node script.
#[test]
fn every_core_reference_slices_and_evaluates_on_its_own() {
    let manifest = manifest();
    let mut members: Vec<(&str, Vec<&str>)> = Vec::new();
    for (_, reference) in manifest.core_references() {
        let (binding, member) = match reference {
            CoreReference::Binding(binding) => (binding, None),
            CoreReference::Member(binding, member) => (binding, Some(member)),
            CoreReference::Cell(cell) => (CORE_CELLS, Some(cell)),
        };
        let index = match members.iter().position(|(name, _)| *name == binding) {
            Some(index) => index,
            None => {
                members.push((binding, Vec::new()));
                members.len() - 1
            }
        };
        members[index].1.extend(member);
    }
    members.sort();
    let mut groups: Vec<(Vec<bool>, Vec<&str>)> = Vec::new();
    for (binding, _) in &members {
        let kept = manifest.reachable([manifest.unit_of(binding).unwrap()]);
        match groups.iter_mut().find(|(group, _)| *group == kept) {
            Some((_, bindings)) => bindings.push(binding),
            None => groups.push((kept, vec![binding])),
        }
    }
    let mut script = String::new();
    let mut checked = 0;
    let path = std::env::temp_dir().join(format!(
        "light-cljs-runtime-references-{}.js",
        std::process::id()
    ));
    let run = |script: &mut String, checked: &mut usize| {
        if script.is_empty() {
            return;
        }
        std::fs::write(&path, &script).unwrap();
        let output =
            std::process::Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
                .arg(&path)
                .output()
                .expect("runtime reference tests require Node");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let printed = String::from_utf8(output.stdout).unwrap();
        assert_eq!(printed.lines().count(), *checked, "{printed}");
        for line in printed.lines() {
            assert!(line.ends_with(" ok"), "{line}");
        }
        script.clear();
        *checked = 0;
    };
    for (kept, bindings) in &groups {
        let factory = manifest.sliced_bindings_text(kept, bindings);
        let checks = bindings
            .iter()
            .enumerate()
            .map(|(index, binding)| {
                let members = &members.iter().find(|(name, _)| name == binding).unwrap().1;
                let members = members
                    .iter()
                    .map(|member| format!("{member:?}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "  {{ const value = values[{index}]; const missing = [{members}].filter(member => !(member in Object(value))); \
                     console.log({binding:?} + (value === undefined || missing.length ? \" missing \" + missing : \" ok\")); }}\n"
                )
            })
            .collect::<String>();
        script.push_str(&format!(
            "{{\n  const values = {};\n{checks}}}\n",
            factory.trim()
        ));
        checked += bindings.len();
        if script.len() > 16_000_000 {
            run(&mut script, &mut checked);
        }
    }
    run(&mut script, &mut checked);
    let _ = std::fs::remove_file(&path);
    assert!(groups.len() > 100, "{}", groups.len());
}

#[test]
fn replaced_core_operations_read_through_output_cells() {
    let source = "(set! cljs.core/inc dec) [(inc 5) (mapv inc [1 2])]";
    let js = advanced_matches_none(&format!("(pr-str (do {source}))"));
    assert!(js.contains("  let core$inc = rt$inc;\n"), "{js}");
    assert!(js.contains("(core$inc = rt$dec)"), "{js}");
    assert!(js.contains("(core$inc).call(null, 5)"), "{js}");
    assert!(!js.contains("rt$inc("), "{js}");
    assert_eq!(
        prepared(source, OptimizationLevel::Advanced).replaced_core(),
        ["inc"]
    );
    assert!(
        prepared(source, OptimizationLevel::Prepare)
            .replaced_core()
            .is_empty()
    );

    // `with-redefs` and `binding` assign too, and restore the operation.
    let js = advanced_matches_none(
        "(pr-str [(with-redefs [inc dec] (mapv inc [1 2])) (inc 1)
                  (binding [cljs.core/str (fn [& xs] \"s\")] (str 1)) (str 2)])",
    );
    assert!(js.contains("let core$inc = rt$inc;"), "{js}");
    assert!(js.contains("let core$str = rt$str;"), "{js}");

    // A `js*` hole may be assigned by its raw text, so the operations passed to
    // holes read through cells too, and the assignment reaches every read.
    let js =
        advanced_matches_none(r#"(js* "~{} = ~{}" inc dec) (pr-str [(inc 5) (mapv inc [1 2])])"#);
    // Raw text makes hygiene prefix the compiler's names.
    assert!(
        js.contains("let __light0_core$inc = __light0_rt$inc;"),
        "{js}"
    );
    assert!(js.contains("__light0_core$inc = __light0_core$dec"), "{js}");
    assert_eq!(
        prepared(r#"(js* "~{}" inc)"#, OptimizationLevel::Advanced).replaced_core(),
        ["inc"]
    );
    // So does assigning a property of a core value.
    advanced_matches_none(
        "(set! (.-probe cljs.core/PersistentHashMap) 7)
         (pr-str [(.-probe cljs.core/PersistentHashMap) (count (hash-map 1 2))])",
    );

    // A program that assigns nothing reads every operation directly; a call
    // of `inc` is the operator (light-cljs-210).
    let js = advanced_matches_none("(pr-str [(inc 5) (mapv inc [1 2])])");
    assert!(!cell_reads(&js), "{js}");
    assert!(js.contains("(5 + 1)"), "{js}");
    assert!(js.contains("rt$mapv(rt$inc, "), "{js}");
}

#[test]
fn dynamic_vars_and_redefinition_hooks_keep_their_runtime_cells() {
    // Dynamic Vars are cells the printer reads.
    let js = advanced_matches_none(
        "(pr-str [(binding [*print-length* 2] (pr-str [1 2 3])) (pr-str [1 2 3])])",
    );
    assert!(js.contains("rt$coreCells.printLength = temporary0"), "{js}");
    assert!(!cell_reads(&js), "{js}");
    let js = advanced_matches_none("(with-out-str (print \"a\" 1))");
    assert!(js.contains("rt$coreCells.printFn"), "{js}");
    // The runtime reads `rand` and `flush` through their cells, so redefining
    // them reaches `random-sample` and `newline`.
    let js = advanced_matches_none(
        "(pr-str [(with-redefs [rand (constantly 0.25)] (vec (random-sample 0.5 (range 4))))
                  (with-redefs [rand (constantly 0.75)] (vec (random-sample 0.5 (range 4))))])",
    );
    assert!(js.contains("rt$coreCells.rand = temporary0"), "{js}");
    advanced_matches_none(
        "(def events (atom []))
         (with-redefs [flush #(swap! events conj :flush)] (with-out-str (newline)))
         (pr-str @events)",
    );
    advanced_matches_none("(set! *print-length* 1) (pr-str (range 3))");
    advanced_matches_none("(pr-str (str *ns*))");
}

#[test]
fn raw_javascript_cannot_capture_direct_reads_or_cells() {
    let js = advanced_matches_none(
        r#"(js* "globalThis.rt$max = 'user max'; globalThis.core$inc = 'user cell'")
           (set! cljs.core/inc dec)
           (pr-str [(max 1 2) (inc 5) (js* "rt$max") (js* "core$inc")])"#,
    );
    assert!(js.contains("__light0_rt$max(1, 2)"), "{js}");
    assert!(
        js.contains("let __light0_core$inc = __light0_rt$inc;"),
        "{js}"
    );
}

#[test]
fn shared_and_lower_level_output_keep_reading_the_core_hub() {
    let source = "(set! cljs.core/inc dec) (+ (inc 1) 2)";
    let unit = light_cljs::emitter::emit_js_unit(&program(source));
    assert!(unit.contains("rt.core.inc = rt.core.dec"), "{unit}");
    assert!(unit.contains("(rt.core.add).call(null, "), "{unit}");
    assert!(!unit.contains("rt$") && !cell_reads(&unit), "{unit}");
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        let js = prepared(source, level).emit_js();
        assert!(js.contains("rt.core.inc = rt.core.dec"), "{level}");
        assert!(js.contains(RUNTIME.trim()), "{level}");
        assert!(!js.contains("rt$") && !cell_reads(&js), "{level}");
    }
}
