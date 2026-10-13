//! Advanced-mode runtime slicing (docs/advanced-optimization.md, Phase 1
//! steps 2 and 3): `Advanced` output reads runtime bindings directly and
//! embeds only the runtime units those reads reach over the unit manifest, and
//! `PreparedApplication::retention()` reports each embedded unit with the root
//! and source span that first demanded it.
//!
//! The emitter records every runtime read it writes; in debug builds it also
//! checks that no `rt.`, `rt$` or `core$` read in the output escaped that
//! record, so every test that emits code at any level checks the roots.

use super::optimization::execute;
use light_cljs::runtime_manifest::{RUNTIME, RuntimeManifest, UnitId};
use light_cljs::runtime_slice::{Demand, RuntimeEmbedding, RuntimeName};
use light_cljs::{
    ApplicationOptions, OptimizationLevel, PreparedApplication, Program, analyze, parse,
    prepare_application,
};

fn manifest() -> &'static RuntimeManifest<'static> {
    RuntimeManifest::embedded()
}

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn prepared(source: &str, level: OptimizationLevel) -> PreparedApplication {
    prepare_application(&program(source), ApplicationOptions::new(level))
}

/// The `Advanced` runtime declaration for these root units and bindings: their
/// closure, returning the bindings in the given order, which the output
/// destructures into `rt$<binding>` constants.
fn slice(units: &[UnitId], bindings: &[&str]) -> (String, String) {
    let manifest = manifest();
    let kept = manifest.reachable(units.iter().copied());
    let factory = manifest.sliced_bindings_text(&kept, bindings);
    let pattern = bindings
        .iter()
        .map(|binding| format!("rt${binding}"))
        .collect::<Vec<_>>()
        .join(", ");
    (
        format!("  const [{pattern}] = {};\n", factory.trim()),
        factory,
    )
}

/// Programs whose emission reads every kind of runtime name the emitter
/// writes: core operations, value constructors, reader literals, quoted
/// compiler forms, Vars, metadata, protocols, types, records, reify,
/// variadic functions, `case`, `assert` and `with-out-str`.
const PROGRAMS: &[&str] = &[
    "(+ 1 2)",
    "(defn kind [k] (case k :a 1 :b 2 3)) (pr-str [(kind :a) (kind :c) (case 2 1 :x 2 :y)])",
    r#"(pr-str {:a 1} #{:b} '(c) [1 2] (with-meta [1] {:m 1}) (var inc))"#,
    r#"(with-out-str (print #queue [1 2] #uuid "00000000-0000-0000-0000-000000000000"
                       (re-find #"a+" "baa") '#js [1] '#js {:a 1}))"#,
    r#"(.-a #js {"a" 7})"#,
    "(defprotocol Area (area [shape])) (defrecord Rect [w h] Area (area [_] (* w h)))
     (extend-type number Area (area [n] n))
     (pr-str [(area (->Rect 3 4)) (area 5) (satisfies? Area 5) (map->Rect {:w 1 :h 2})])",
    "(deftype Box [v]) (let [x 2 r (reify IFn (-invoke [_ y] (+ x y)))]
       (pr-str [(.-v (Box. 1)) (r 3) ((fn [& xs] (count xs)) 1 2)]))",
    r#"(try (assert (= 1 2) "no") (catch :default e (ex-message e)))"#,
];

#[test]
fn advanced_output_embeds_exactly_the_units_its_runtime_reads_reach() {
    let manifest = manifest();
    for source in PROGRAMS {
        let advanced = prepared(source, OptimizationLevel::Advanced);
        let report = advanced.retention();
        assert_eq!(report.level, OptimizationLevel::Advanced);
        assert_eq!(report.embedding, RuntimeEmbedding::Slice, "{source}");
        let js = advanced.emit_js();
        let units = report
            .roots
            .iter()
            .map(|root| root.unit)
            .collect::<Vec<_>>();
        for root in &report.roots {
            assert!(root.name.direct, "{source}: {}", root.name);
            assert_eq!(manifest.unit_of(root.name.binding), Some(root.unit));
            assert!(
                js.contains(&root.name.to_string()),
                "{source}: {}",
                root.name
            );
        }
        // No slice keeps the `core` hub.
        assert!(
            !report
                .units
                .iter()
                .any(|unit| unit.name == "core" || unit.name == "core.seal"),
            "{source}"
        );
        let kept = manifest.reachable(units.iter().copied());
        assert_eq!(
            report
                .units
                .iter()
                .map(|unit| unit.unit)
                .collect::<Vec<_>>(),
            (0..kept.len())
                .filter(|&index| kept[index])
                .map(UnitId)
                .collect::<Vec<_>>(),
            "{source}"
        );
        let mut bindings = Vec::new();
        for root in &report.roots {
            if !bindings.contains(&root.name.binding) {
                bindings.push(root.name.binding);
            }
        }
        let (declaration, factory) = slice(&units, &bindings);
        assert!(
            js.contains(&declaration),
            "{source}: the output embeds the slice"
        );
        // Outside the runtime, Advanced output reads it only directly.
        assert!(
            !js.replacen(&declaration, "", 1).contains("rt."),
            "{source}"
        );
        assert_eq!(report.runtime_bytes, factory.trim().len());
        let whole = prepared(source, OptimizationLevel::Prepare).emit_js();
        assert!(whole.contains(RUNTIME.trim()), "{source}");
        assert!(js.len() < whole.len(), "{source}");
        let none = prepared(source, OptimizationLevel::None).emit_js();
        assert_eq!(
            execute(&format!("console.log({js});")),
            execute(&format!("console.log({none});")),
            "{source}: advanced prints what none prints"
        );
    }
}

#[test]
fn bundler_modules_embed_the_same_slice() {
    for source in PROGRAMS {
        let advanced = prepared(source, OptimizationLevel::Advanced);
        let js = advanced.emit_js();
        let module = advanced.emit_esm();
        assert_eq!(
            module,
            format!("export default {};\n", js.trim_end()),
            "{source}"
        );
    }
}

#[test]
fn levels_below_advanced_embed_the_whole_runtime_and_programs_without_reads_none() {
    let manifest = manifest();
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        let application = prepared("(+ 1 2)", level);
        let report = application.retention();
        assert_eq!(report.embedding, RuntimeEmbedding::Whole);
        assert_eq!(report.units.len(), manifest.units.len());
        assert_eq!(report.runtime_bytes, RUNTIME.trim().len());
        assert!(application.emit_js().contains(RUNTIME.trim()));
        // Units no root reaches are embedded only because the runtime is whole.
        let kept = manifest.reachable(report.roots.iter().map(|root| root.unit));
        for unit in &report.units {
            assert_eq!(unit.root.is_some(), kept[unit.unit.0], "{}", unit.name);
        }
        assert!(report.units.iter().any(|unit| unit.root.is_none()));
        assert!(report.to_string().contains("whole runtime"));
    }
    for level in OptimizationLevel::ALL {
        let application = prepared("(js/console.log (if (js/Math.random) 1 2))", level);
        let report = application.retention();
        assert_eq!(report.embedding, RuntimeEmbedding::None, "{level}");
        assert!(report.roots.is_empty() && report.units.is_empty());
        assert_eq!(report.runtime_bytes, 0);
        assert!(!application.emit_js().contains("const rt ="));
        assert_eq!(
            report.to_string(),
            format!("{level}: no runtime embedded\n")
        );
    }
}

#[test]
fn retention_attributes_each_unit_to_the_first_root_and_its_source_span() {
    let manifest = manifest();
    let source = r#"#js {"a" 1} (+ 1 2)"#;
    let report = prepared(source, OptimizationLevel::Advanced).retention();
    let expression = |text: &str| {
        let start = source.find(text).unwrap();
        (start, start + text.len())
    };
    let roots = report
        .roots
        .iter()
        .map(|root| {
            let site = match &root.demand {
                Demand::Expression {
                    source_index, span, ..
                } => {
                    assert_eq!(*source_index, 0);
                    Some((span.start, span.end))
                }
                Demand::NamespaceRegistration => None,
            };
            (root.name, site)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        roots,
        [
            (
                RuntimeName::direct("jsObject", None),
                Some(expression(r#"#js {"a" 1}"#))
            ),
            (RuntimeName::direct("add", None), Some(expression("+"))),
            (RuntimeName::direct("registerReaderNamespace", None), None),
            (RuntimeName::direct("coreCells", Some("currentNs")), None),
            (RuntimeName::direct("theNs", None), None),
            (RuntimeName::direct("symbol", None), None),
        ]
    );
    // `jsObject` alone keeps its own unit; every later root adds the units of
    // its closure that earlier roots do not reach. Without the core hub the
    // namespace registration adds units of its own.
    let attributed = |root: usize| {
        report
            .units
            .iter()
            .filter(|unit| unit.root == Some(root))
            .map(|unit| unit.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(attributed(0), ["jsObject"]);
    let units = report
        .roots
        .iter()
        .map(|root| root.unit)
        .collect::<Vec<_>>();
    for root in 1..report.roots.len() {
        let before = manifest.reachable(units[..root].iter().copied());
        let after = manifest.reachable(units[..=root].iter().copied());
        assert_eq!(
            attributed(root),
            manifest
                .units
                .iter()
                .enumerate()
                .filter(|&(index, _)| after[index] && !before[index])
                .map(|(_, unit)| unit.name)
                .collect::<Vec<_>>(),
            "{}",
            report.roots[root].name
        );
    }
    assert!(!attributed(2).is_empty());
    assert_eq!(
        report.unit_bytes(),
        report.units.iter().map(|unit| unit.bytes).sum::<usize>()
    );

    let table = report.to_string();
    let mut lines = table.lines();
    assert_eq!(
        lines.next().unwrap(),
        format!(
            "advanced: runtime slice, {} of {} units, {} unit bytes, {} runtime bytes",
            report.units.len(),
            manifest.units.len(),
            thousands(report.unit_bytes()),
            thousands(report.runtime_bytes)
        )
    );
    let row = |line: &str| line.split_whitespace().collect::<Vec<_>>().join(" ");
    let rows = table.lines().map(row).collect::<Vec<_>>();
    let unit_bytes = manifest
        .unit(manifest.unit_of("jsObject").unwrap())
        .body_range
        .len();
    for expected in [
        "root units bytes first demanded by".to_owned(),
        format!("rt$jsObject 1 {unit_bytes} source 0, bytes 0..11"),
        "unit bytes root first demanded by".to_owned(),
        format!("jsObject {unit_bytes} rt$jsObject source 0, bytes 0..11"),
    ] {
        assert!(
            rows.contains(&expected),
            "missing row {expected:?}:\n{table}"
        );
    }
    let (start, end) = expression("+");
    let add_row = format!(" rt$add source 0, bytes {start}..{end}");
    assert!(
        rows.iter()
            .any(|row| row.starts_with("rt$theNs ") && row.ends_with(" namespace registration")),
        "{table}"
    );
    assert_eq!(
        rows.iter().filter(|row| row.ends_with(&add_row)).count(),
        attributed(1).len()
    );
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Every export the emitter can read, sliced alone, is a runtime factory that
/// evaluates and freezes exactly the requested exports. Exports whose closures
/// are equal share one run.
#[test]
fn the_slice_for_each_export_evaluates_on_its_own() {
    let manifest = manifest();
    let mut groups: Vec<(Vec<bool>, Vec<&str>)> = Vec::new();
    for &export in &manifest.exports.names {
        let kept = manifest.reachable([manifest.unit_of(export).unwrap()]);
        match groups.iter_mut().find(|(group, _)| *group == kept) {
            Some((_, exports)) => exports.push(export),
            None => groups.push((kept, vec![export])),
        }
    }
    assert!(groups.len() > 1);
    for (kept, exports) in &groups {
        let factory = manifest.sliced_text(kept, exports);
        let printed = execute(&format!(
            "const rt = {};\nconsole.log(JSON.stringify([Object.isFrozen(rt), Object.keys(rt), \
             Object.values(rt).every(value => value !== undefined)]));",
            factory.trim()
        ));
        let expected = format!(
            "[true,[{}],true]\n",
            exports
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(printed, expected, "{exports:?}");
    }
}
