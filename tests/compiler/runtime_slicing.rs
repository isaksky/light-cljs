//! Advanced-mode runtime slicing (docs/advanced-optimization.md, Phase 1
//! steps 2 to 4): `Advanced` output reads runtime bindings directly and
//! embeds only the runtime units those reads reach over the unit manifest, and
//! `PreparedApplication::retention()` reports each embedded unit with the root
//! and source span that first demanded it. Runtime effects are demand-driven:
//! a core function the output only calls leaves out its variadic arity ABI, a
//! builtin type's members travel with its constructor, and an `extend-type` of
//! a core protocol keeps the dispatcher the runtime patches by key.
//!
//! The emitter records every runtime read it writes; in debug builds it also
//! checks that no `rt.`, `rt$` or `core$` read in the output escaped that
//! record, so every test that emits code at any level checks the roots.

use super::optimization::{execute, readable};
use light_cljs::runtime_manifest::{RUNTIME, RootUse, RuntimeManifest, UnitId};
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

/// The prepared program with readable `Advanced` output (`pseudo_names`),
/// whose slice the tests compare with the manifest's text.
fn prepared(source: &str, level: OptimizationLevel) -> PreparedApplication {
    prepare_application(&program(source), readable(level))
}

/// The output at `level`, terse at `Advanced`, which the programs run as.
fn terse(source: &str, level: OptimizationLevel) -> String {
    prepare_application(&program(source), ApplicationOptions::new(level)).emit_js()
}

/// The units these roots keep, indexed by `UnitId`.
fn kept(roots: &[(UnitId, RootUse)]) -> Vec<bool> {
    manifest()
        .reached_by_uses(roots)
        .iter()
        .map(Option::is_some)
        .collect()
}

/// The `Advanced` runtime declaration for these root units and bindings: their
/// closure, returning the bindings in the given order, which the output
/// destructures into `rt$<binding>` constants.
fn slice(units: &[(UnitId, RootUse)], bindings: &[&str]) -> (String, String) {
    let manifest = manifest();
    let kept = kept(units);
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
/// variadic functions, `case`, `assert` and `with-out-str`. Operands that
/// would be constants read a string's length, so that constant folding
/// (light-cljs-214) leaves the call.
const PROGRAMS: &[&str] = &[
    r#"(max 1 (.-length "ab"))"#,
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
            .map(|root| (root.unit, root.root_use()))
            .collect::<Vec<_>>();
        for root in &report.roots {
            assert!(root.name.direct, "{source}: {}", root.name);
            assert_eq!(manifest.unit_of(root.name.binding), Some(root.unit));
            assert_eq!(
                js.contains(&root.name.to_string()),
                root.read(),
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
        let kept = kept(&units);
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
        for root in report.roots.iter().filter(|root| root.read()) {
            if !bindings.contains(&root.name.binding) {
                bindings.push(root.name.binding);
            }
        }
        let (declaration, factory) = slice(&units, &bindings);
        // The runtime text holds the key's placeholder, which the output
        // writer replaces (docs/value-layout.md, section 1).
        let unkeyed = light_cljs::runtime_key::unkeyed(&js);
        assert!(
            unkeyed.contains(&declaration),
            "{source}: the output embeds the slice"
        );
        // Outside the runtime, Advanced output reads it only directly.
        assert!(
            !unkeyed.replacen(&declaration, "", 1).contains("rt."),
            "{source}"
        );
        // Counted as written, with the key (docs/value-layout.md, section 1).
        assert_eq!(
            report.runtime_bytes,
            light_cljs::runtime_key::written_len(factory.trim())
        );
        let whole = prepared(source, OptimizationLevel::Prepare).emit_js();
        assert!(
            light_cljs::runtime_key::unkeyed(&whole).contains(RUNTIME.trim()),
            "{source}"
        );
        assert!(js.len() < whole.len(), "{source}");
        let none = execute(&format!(
            "console.log({});",
            prepared(source, OptimizationLevel::None).emit_js()
        ));
        assert_eq!(
            execute(&format!("console.log({js});")),
            none,
            "{source}: readable advanced prints what none prints"
        );
        assert_eq!(
            execute(&format!(
                "console.log({});",
                terse(source, OptimizationLevel::Advanced)
            )),
            none,
            "{source}: advanced prints what none prints"
        );
    }
}

#[test]
fn bundler_modules_embed_the_same_slice() {
    for source in PROGRAMS {
        for options in [
            ApplicationOptions::new(OptimizationLevel::Advanced),
            readable(OptimizationLevel::Advanced),
        ] {
            let advanced = prepare_application(&program(source), options);
            // Each output's key is its own text's: compared as written
            // with the placeholder.
            let js = light_cljs::runtime_key::unkeyed(&advanced.emit_js());
            let module = light_cljs::runtime_key::unkeyed(&advanced.emit_esm());
            assert_eq!(
                module,
                format!("export default {};\n", js.trim_end()),
                "{source}"
            );
        }
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
        assert_eq!(
            report.runtime_bytes,
            light_cljs::runtime_key::written_len(RUNTIME.trim())
        );
        assert!(light_cljs::runtime_key::unkeyed(&application.emit_js()).contains(RUNTIME.trim()));
        // Units no root reaches are embedded only because the runtime is whole.
        let roots = report
            .roots
            .iter()
            .map(|root| (root.unit, root.root_use()))
            .collect::<Vec<_>>();
        let kept = kept(&roots);
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
    // `*ns*` reaches reflection, so the output registers its namespaces. The
    // length is no constant, so `mod` is called (light-cljs-214).
    let source = r#"#js {"a" 1} (mod 7 (.-length "ab")) *ns*"#;
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
            (RuntimeName::direct("mod", None), Some(expression("mod"))),
            (
                RuntimeName::direct("coreCells", Some("currentNs")),
                Some(expression("*ns*"))
            ),
            // Every slice registers the build's key.
            (RuntimeName::direct("runtimeKey", None), None),
            (RuntimeName::direct("registerReaderNamespace", None), None),
            (RuntimeName::direct("theNs", None), None),
            (RuntimeName::direct("symbol", None), None),
        ]
    );
    // `jsObject` alone keeps its own unit; every later root adds the units of
    // its closure that earlier roots do not reach: `*ns*` its cell object, the
    // namespace registration the registry and the symbol machinery.
    let attributed = |root: usize| {
        report
            .units
            .iter()
            .filter(|unit| unit.root == Some(root))
            .map(|unit| unit.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(attributed(0), ["jsObject"]);
    // `mod` is a unit of its own.
    assert_eq!(attributed(1), ["mod"]);
    let units = report
        .roots
        .iter()
        .map(|root| (root.unit, root.root_use()))
        .collect::<Vec<_>>();
    for root in 1..report.roots.len() {
        let before = kept(&units[..root]);
        let after = kept(&units[..=root]);
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
    assert!(!attributed(2).is_empty() && !attributed(3).is_empty());
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
    let (start, end) = expression("mod");
    let mod_row = format!(" rt$mod(...) source 0, bytes {start}..{end}");
    assert!(
        rows.iter()
            .any(|row| row.starts_with("rt$theNs ") && row.ends_with(" namespace registration")),
        "{table}"
    );
    assert_eq!(
        rows.iter().filter(|row| row.ends_with(&mod_row)).count(),
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

/// The units an `Advanced` report embeds, by name.
fn embedded(application: &PreparedApplication) -> Vec<&'static str> {
    application
        .retention()
        .units
        .iter()
        .map(|unit| unit.name)
        .collect()
}

/// Runs a program at every level and returns what it prints, which must be
/// the same at each.
fn same_at_every_level(source: &str) -> String {
    let printed = OptimizationLevel::ALL.map(|level| {
        let js = terse(source, level);
        execute(&format!("console.log({js});"))
    });
    assert_eq!(printed[0], printed[1], "{source}: prepare");
    assert_eq!(printed[0], printed[2], "{source}: advanced");
    printed[2].clone()
}

/// A core function's variadic arity ABI (`cljs$lang$applyTo` and friends, read
/// by `apply`) is an `install:` unit: kept where the function's value escapes,
/// left out where the program only calls it.
#[test]
fn plain_calls_leave_out_the_variadic_arity_that_escaping_values_keep() {
    // A bit operation chain whose later operand runs code stays a call of
    // the runtime (light-cljs-1020), which evaluates every operand first.
    for (called, applied, install) in [
        (
            "(bit-xor 1 2 (js/Math.abs 4))",
            "(apply bit-xor [1 2 (js/Math.abs 4)])",
            "bitXor.variadic",
        ),
        (
            "(bit-and 1 2 (js/Math.abs 4))",
            "(apply bit-and [1 2 (js/Math.abs 4)])",
            "bitAnd.variadic",
        ),
        (
            "(bit-or 1 2 (js/Math.abs 4))",
            "(apply bit-or [1 2 (js/Math.abs 4)])",
            "bitOr.variadic",
        ),
    ] {
        let call = prepared(called, OptimizationLevel::Advanced);
        let report = call.retention();
        let root = &report.roots[0];
        assert_eq!(root.root_use(), RootUse::Call, "{called}");
        assert!(report.to_string().contains(&format!("{}(...)", root.name)));
        assert!(!embedded(&call).contains(&install), "{called}");
        // The call keeps its own unit and helpers (`bitOperation`,
        // `numericFold`) and the key's registration (`runtimeKey`), and
        // nothing else: without reflection there is no namespace
        // registration.
        assert!(report.units.len() <= 4, "{called}:\n{report}");
        let apply = prepared(applied, OptimizationLevel::Advanced);
        assert!(embedded(&apply).contains(&install), "{applied}");
        assert_eq!(
            same_at_every_level(called),
            same_at_every_level(applied),
            "{called}"
        );
    }
    // Calls of `+` and `<` (light-cljs-210) and of the bit operations
    // (light-cljs-1020) are JavaScript operators at `Advanced` and read no
    // runtime; their values keep the install.
    for (called, applied, install) in [
        ("(+ 1 2 3)", "(apply + [1 2 3])", "add.variadic"),
        ("(< 1 2 3)", "(apply < [1 2 3])", "less.variadic"),
        (
            "(bit-xor 1 2 4)",
            "(apply bit-xor [1 2 4])",
            "bitXor.variadic",
        ),
        (
            "(bit-and 1 2 4)",
            "(apply bit-and [1 2 4])",
            "bitAnd.variadic",
        ),
        ("(bit-or 1 2 4)", "(apply bit-or [1 2 4])", "bitOr.variadic"),
    ] {
        let call = prepared(called, OptimizationLevel::Advanced);
        assert_eq!(
            call.retention().embedding,
            RuntimeEmbedding::None,
            "{called}"
        );
        let apply = prepared(applied, OptimizationLevel::Advanced);
        assert!(embedded(&apply).contains(&install), "{applied}");
        assert_eq!(
            same_at_every_level(called),
            same_at_every_level(applied),
            "{called}"
        );
    }
    // A value read keeps the install; here the host reads the ABI.
    let source = "(let [f +] (.-cljs$lang$maxFixedArity f))";
    assert!(embedded(&prepared(source, OptimizationLevel::Advanced)).contains(&"add.variadic"));
    assert_eq!(same_at_every_level(source), "2\n");
}

/// `extendType` patches the core protocol dispatcher it finds by key, so an
/// `extend-type` of a core protocol method keeps that dispatcher even where
/// nothing calls it: the runtime's generic dispatch reads the patch. Without
/// it, `reduce` with an init would find no 3-arity method and throw.
#[test]
fn extended_core_protocols_keep_their_dispatchers() {
    let source = "(extend-type number IReduce (-reduce [n f] [:two n]))
                  (pr-str [(reduce + 0 5) (reduce + 5)])";
    let advanced = prepared(source, OptimizationLevel::Advanced);
    let report = advanced.retention();
    let kept = report
        .roots
        .iter()
        .find(|root| root.name.binding == "reduceProtocol")
        .unwrap_or_else(|| panic!("{report}"));
    assert!(!kept.read());
    assert!(report.to_string().contains("(rt$reduceProtocol)"));
    let js = advanced.emit_js();
    assert!(js.contains("coreMethod(\"IReduce/-reduce\""));
    assert!(!js.contains("rt$reduceProtocol"));
    assert_eq!(same_at_every_level(source), "[[:two 5] [:two 5]]\n");
}

/// light-cljs-1052: an `extend-type` or `extend-protocol` of a runtime class
/// makes no value of it. Where nothing else keeps the class whole, the slice
/// keeps only its prototype, which `extendType` writes onto and whose kind it
/// reads (its type units), and none of its slots or the helpers only they
/// reach. A program that can make a value of the class, by a core function,
/// a constant, a macro's constant or the reader, keeps the slots, and the
/// extension's method applies to the value.
#[test]
fn a_runtime_class_only_extended_keeps_its_prototype_alone() {
    let extension = "(defprotocol P (p [x]))
                     (defmacro built [] (list 'sorted-set 2 1))
                     (defmacro literal [] (sorted-set 2 1))
                     (extend-protocol P
                       cljs.core/PersistentTreeSet (p [s] [:tree (count s) (first s)])
                       default (p [_] :other))";
    // The operations that dispatch to the slots below.
    let uses = "[(p #{2 1}) (p [1]) (count #{1 2}) (first #{3}) (contains? #{1} 1)
                 (= #{1} #{1}) (hash #{1}) (str #{4}) (satisfies? P #{1}) (satisfies? P [1])]";
    let slots = [
        "TreeSetCell.members",
        "TreeSetCell.seq",
        "TreeSetCell.count",
        "TreeSetCell.lookup",
        "treeSetConstructor.equivSlot",
        "treeSetConstructor.hashSlot",
    ];
    let prototype = [
        "treeSetConstructor",
        "treeSetConstructor.type",
        "TreeSetCell.type",
    ];
    // A sorted set a macro returns is a set literal, as upstream reads it:
    // a hash set.
    let source = format!("{extension} (pr-str [(p (literal)) {uses}])");
    let extended = prepared(&source, OptimizationLevel::Advanced);
    let report = extended.retention();
    let root = report
        .roots
        .iter()
        .find(|root| root.name.binding == "treeSetConstructor")
        .unwrap_or_else(|| panic!("{report}"));
    assert_eq!(root.root_use(), RootUse::Extend);
    assert!(
        report
            .to_string()
            .contains("rt$treeSetConstructor (extended)")
    );
    let units = embedded(&extended);
    for kept in prototype {
        assert!(units.contains(&kept), "{kept}:\n{report}");
    }
    for left_out in slots {
        assert!(!units.contains(&left_out), "{left_out}:\n{report}");
    }
    assert_eq!(
        same_at_every_level(&source),
        "[:other [:other :other 2 3 true true 1978132887 \"#{4}\" true true]]\n"
    );
    // A value of the class, however the program makes it, keeps them all.
    for (built, answer) in [
        ("(sorted-set 2 1)", "[:tree 2 1]"),
        ("(into (sorted-set) [3 1 2])", "[:tree 3 1]"),
        ("(disj (sorted-set-by > 1 2 3) 2)", "[:tree 2 3]"),
        ("(built)", "[:tree 2 1]"),
        (
            "(cljs.core/PersistentTreeSet. nil (sorted-map 5 nil) nil)",
            "[:tree 1 5]",
        ),
    ] {
        let source = format!("{extension} (pr-str [(p {built}) {uses}])");
        let units = embedded(&prepared(&source, OptimizationLevel::Advanced));
        for kept in slots.iter().chain(&prototype) {
            assert!(units.contains(kept), "{kept}: {built}");
        }
        assert_eq!(
            same_at_every_level(&source),
            format!("[{answer} [:other :other 2 3 true true 1978132887 \"#{{4}}\" true true]]\n"),
            "{built}"
        );
    }
    // So does a read of the class as a value other than to extend it.
    let read =
        format!("{extension} (pr-str [(some? (.-prototype cljs.core/PersistentTreeSet)) {uses}])");
    let units = embedded(&prepared(&read, OptimizationLevel::Advanced));
    for kept in slots {
        assert!(units.contains(&kept), "{kept}");
    }
}

/// light-cljs-1052, for every runtime class: extending one that the program
/// never builds leaves the answers for the values it does build as they
/// were, among them values of classes the class shares a prototype or a
/// kind with, and the extension applies wherever one of its values comes
/// from a constant, a macro or the reader.
#[test]
fn extending_runtime_classes_keeps_the_answers_of_every_value() {
    let values = "(into [nil 1 :k 'k [1] {:a 1} #{1} (list 1) (seq [1 2]) (range 3) (map inc [1])
                         (subvec [1 2] 1) (first {:a 1}) (atom 1) (sorted-map 1 2)
                         (seq (sorted-map 1 2)) (transient []) (volatile! 1) (delay 1) (reduced 1)
                         (ex-info \"x\" {}) (zipmap (range 20) (range 20)) (read-string \"#{3}\")
                         (read-string \"[4]\")]
                        (constants))";
    for class in [
        "PersistentTreeSet",
        "PersistentTreeMap",
        "PersistentHashSet",
        "PersistentHashMap",
        "PersistentQueue",
        "MapEntry",
        "Subvec",
        "MultiFn",
        "UUID",
        "Keyword",
        "Symbol",
        "PersistentVector",
        "ExceptionInfo",
        "TransientVector",
        "Delay",
        "Atom",
    ] {
        let source = format!(
            "(defprotocol P (p [x]))
             (defmacro constants [] [:mk (list 'quote 'ms) #{{5}} (list 'quote [8])])
             (extend-protocol P cljs.core/{class} (p [_] :extended) default (p [_] :other))
             (let [values {values}]
               (pr-str (map p values) (map #(satisfies? P %) values)))"
        );
        let printed = same_at_every_level(&source);
        let extended = printed.matches(":extended").count();
        let expected = match class {
            "PersistentHashSet" | "PersistentVector" => 3,
            "Keyword" | "Symbol" => 2,
            "PersistentTreeMap" | "PersistentHashMap" | "MapEntry" | "Subvec" | "ExceptionInfo"
            | "TransientVector" | "Delay" | "Atom" => 1,
            _ => 0,
        };
        assert_eq!(extended, expected, "{class}: {printed}");
    }
}

/// light-cljs-1052, from the Sol review: a class kept for its prototype
/// keeps the effect that defines it, `MapEntryType.vector`, which waits for
/// `makeVector` and assigns the class (without it the type unit read the
/// prototype of an undefined class as the output loaded), and drops its
/// members, `Eduction.members` (marked `values`), which a value brings back.
#[test]
fn a_class_kept_for_its_prototype_keeps_its_definition_and_drops_its_members() {
    let source = "(defprotocol P (p [x]))
                  (extend-protocol P cljs.core/MapEntry (p [_] 1) default (p [_] 2))
                  (pr-str [(p [3]) (p 4)])";
    let units = embedded(&prepared(source, OptimizationLevel::Advanced));
    assert!(units.contains(&"MapEntryType.vector") && units.contains(&"MapEntryType.type"));
    assert!(!units.contains(&"MapEntryType.nth"), "a slot");
    assert_eq!(same_at_every_level(source), "[2 2]\n");
    let extension = "(defprotocol P (p [x]))
                     (extend-protocol P cljs.core/Eduction (p [_] :eduction) default (p [_] :other))";
    let only = format!("{extension} (pr-str [(p 1) (p [1])])");
    let units = embedded(&prepared(&only, OptimizationLevel::Advanced));
    assert!(units.contains(&"Eduction.kind") && !units.contains(&"Eduction.members"));
    assert_eq!(same_at_every_level(&only), "[:other :other]\n");
    let built = format!(
        "{extension} (pr-str [(p (eduction (map inc) [1])) (vec (eduction (map inc) [1]))])"
    );
    assert!(embedded(&prepared(&built, OptimizationLevel::Advanced)).contains(&"Eduction.members"));
    assert_eq!(same_at_every_level(&built), "[:eduction [2]]\n");
}

/// light-cljs-1052: every runtime class `cljs.core` names, extended alone by
/// a program that builds other values, loads and answers as below
/// `:advanced`; a name that is no type (a protocol) fails to extend alike.
#[test]
fn every_runtime_class_extended_alone_answers_at_every_level() {
    let mut names = light_cljs::CoreFnId::ALL
        .iter()
        .map(|core| core.operation().name)
        .filter(|name| name.starts_with(|c: char| c.is_ascii_uppercase()))
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    assert!(names.len() > 40, "{names:?}");
    for name in names {
        let source = format!(
            "(defprotocol P (p [x]))
             (def extended
               (try (extend-protocol P cljs.core/{name} (p [_] :extended)) :extended
                    (catch :default _ :no-type)))
             (extend-protocol P default (p [_] :other))
             (pr-str [extended (p 1) (p [3]) (p {{:a 1}}) (p :k) (p (seq [1 2])) (= [1] (list 1))
                      (count #{{1 2}}) (str (range 3)) (first {{:b 2}})])"
        );
        same_at_every_level(&source);
    }
}

/// A builtin type's prototype setup, members and static fields are effects of
/// its constructor: a program keeps those of the types it can construct.
#[test]
fn builtin_type_members_travel_with_their_constructors() {
    let vector = prepared("[1 2 3]", OptimizationLevel::Advanced);
    let units = embedded(&vector);
    for kept in ["PersistentVectorValue", "PersistentVectorValue.type"] {
        assert!(units.contains(&kept), "{kept}");
    }
    // The upstream fields only where the program names one (light-cljs-238),
    // and the statics only where it names one or reads an empty literal
    // (light-cljs-247).
    for left_out in [
        "PersistentVectorValue.fields",
        "typeStatics",
        "PersistentVectorValue.statics",
        "Eduction.members",
        "UUIDType",
        "TransientVectorType",
        "Delay",
        "ChunkBuffer.fields",
    ] {
        assert!(!units.contains(&left_out), "{left_out}");
    }
    let source = "(pr-str [(.-EMPTY cljs.core/PersistentVector) (.-cnt [1 2]) \
                  (.-arr (array-map :a 1)) (.-val (reduced 3)) (.-ns 'a/b) @(delay 4) \
                  (vec (eduction (map inc) [1 2])) (persistent! (conj! (transient []) 1)) \
                  (.indexOf (list 1 2 3) 3) (vec (js/Array.from (range 3))) \
                  (cljs.core/PersistentHashMap.fromArrays #js [:k] #js [1])])";
    assert_eq!(
        same_at_every_level(source),
        "[[] 2 #js [:a 1] 3 \"a\" 4 [2 3] [1] 2 [0 1 2] {:k 1}]\n"
    );
    let named = embedded(&prepared(source, OptimizationLevel::Advanced));
    assert!(named.contains(&"upstreamMembers") && named.contains(&"PersistentVectorValue.fields"));
    for statics in [
        "typeStatics",
        "PersistentVectorValue.statics",
        "PersistentHashMapValue.statics",
    ] {
        assert!(named.contains(&statics), "{statics}");
    }
    let empty = embedded(&prepared(
        "(js/console.log [] {})",
        OptimizationLevel::Advanced,
    ));
    for statics in [
        "typeStatics",
        "PersistentVectorValue.statics",
        "PersistentArrayMapValue.statics",
    ] {
        assert!(empty.contains(&statics), "{statics}");
    }
}
