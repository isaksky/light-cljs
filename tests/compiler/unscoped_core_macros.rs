//! Core macro calls analyzed outside every scope of generated names (a single
//! source, `light-cljs --compile FILE`) expand on the Pants build as they do in
//! the V8 build, and calls their definitions reject fail with the definitions'
//! diagnostics (light-cljs-611). The expectations are the V8 build's: there
//! the definitions run in the engine, on the Pants build natively.
use super::optimization::execute;
use light_cljs::{MacroHost, analyze_with_macros, compile_with_macros, parse};

/// The program's Node output, compiled by a fresh host outside every scope.
fn output(source: &str) -> String {
    let js = compile_with_macros(source, &mut MacroHost::new())
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    execute(&js)
}

/// Core macros number their names with the compile-time runtime's `gensym`
/// counter, which a source macro's `gensym` shares, skipping the names the
/// call spells (`start2`, `start3`), as the engine does.
#[test]
fn unscoped_core_macros_share_the_runtime_gensym_count_and_skip_call_names() {
    let source = r#"
        (set! *print-fn* (fn [& _] nil))
        (defmacro g [] (list 'quote (gensym "start")))
        (js/console.log (str (g)))
        (let [start2 5 start3 6] (js/console.log (time start2) (time start3) (str (g))))
        (js/console.log (str (g)))
        (js/console.log (case 2 1 "a" 2 "b") (and 1 2) (when-let [x 3] x))"#;
    assert_eq!(output(source), "start1\n5 6 start7\nstart8\nb 2 3\n");
}

/// `selector` and `simple-benchmark`, which only bootstrap definitions
/// expanded before, run as they do there.
#[test]
fn unscoped_selector_and_simple_benchmark_expand() {
    let source = r#"
        (let [f (selector {:keys [a] :select picked})
              g (selector {:keys [b] :select s :missing m})]
          (js/console.log (pr-str (f {:a 1 :b 2})) (pr-str (g {:a 1}))))
        (let [lines (atom [])]
          (simple-benchmark [x 1] (+ x 1) 2 :print-fn (fn [line] (swap! lines conj line)))
          (js/console.log (count @lines)
                          (subs (first @lines) 0 22)))"#;
    assert_eq!(
        output(source),
        "{:a 1} {:select {}}\n1 [x 1], (+ x 1), 2 runs\n"
    );
}

/// `light-cljs --compile FILE` with core macros and no namespace.
#[test]
fn compile_file_expands_core_macros_outside_a_namespace() {
    let root = crate::temp_dir::TempDir::new("unscoped-core-macros");
    let file = root.join("main.cljs");
    std::fs::write(
        &file,
        "(js/console.log (case 2 1 \"a\" 2 \"b\") (when true 3))\n",
    )
    .unwrap();
    let compiled = std::process::Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .arg("--compile")
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    assert_eq!(
        execute(&String::from_utf8(compiled.stdout).unwrap()),
        "b 3\n"
    );
}

/// Each call fails with what the core macro's definition reports, the
/// engine's conversion of its expansion included.
#[test]
fn rejected_core_macro_calls_report_the_definitions_errors() {
    let depth = format!("${}[0]", "[1]".repeat(128));
    let thread = format!("(-> 1 {})", "inc ".repeat(130));
    let cases: Vec<(&str, String)> = vec![
        ("(js-fn? 1 2)", "js-fn?: Error: js-fn? requires one argument; definition compiler_core_macros.cljs:17:1".into()),
        ("(js-comment :x)", "js-comment: Error: js-comment requires a string; definition compiler_core_macros.cljs:212:1".into()),
        ("(if-some [a] 1)", "if-some: Error: conditional binding requires exactly two forms; definition compiler_core_macros.cljs:51:1".into()),
        ("(when-first)", "when-first: Error: conditional binding requires a vector; definition compiler_core_macros.cljs:57:1".into()),
        ("(if-let [a 1])", "if-let: Error: Unsupported macro result undefined at $[2][2][2]; definition compiler_core_macros.cljs:49:1".into()),
        ("(if-not 1 2 3 4)", "if-not: Error: Invalid arity: 6; definition compiler_core_macros.cljs:24:1".into()),
        ("(memfn)", "memfn: Error: Unsupported macro result undefined at $[2][2][0]; definition compiler_core_macros.cljs:207:1".into()),
        ("(unchecked-max)", "unchecked-max: Error: Unsupported macro result undefined at $[1][1]; definition compiler_core_macros.cljs:258:1".into()),
        ("(dotimes [a] 1)", "dotimes: Error: dotimes requires exactly two binding forms; definition compiler_core_macros.cljs:160:1".into()),
        ("(cond-> 1 2)", "cond->: Error: conditional threading requires test/step pairs; definition compiler_core_macros.cljs:120:1".into()),
        ("(selector {:select nil})", "selector: Error: form must contain at least one of :select :excess :missing :all; definition compiler_core_macros.cljs:282:1".into()),
        ("(simple-benchmark [] 1 2 \"a\\nb\")", "simple-benchmark: Error: No value supplied for key: \"a\\nb\"; definition compiler_core_macros.cljs:238:1".into()),
        ("(simple-benchmark [] 1)", "simple-benchmark: Error: Unsupported macro result undefined at $[2][1][3][1][1]; definition compiler_core_macros.cljs:238:1".into()),
        ("(defmulti f g :bad 1)", "defmulti: Error: Unsupported defmulti option; definition compiler_core_macros.cljs:294:1".into()),
        ("(for [:let [a 1] x xs] x)", "for: Error: Can't pop empty vector; definition compiler_sequence_macros.cljs:11:1".into()),
        ("(for [x xs :bad 1 :worse 2 y ys] x)", "for: Error: Invalid 'for' keyword :worse; definition compiler_sequence_macros.cljs:11:1".into()),
        ("(for [x xs])", "for: Error: Unsupported macro result undefined at $[1][1][3][1][2][2][2][2][1][1][2][2][2][1][2]; definition compiler_sequence_macros.cljs:11:1".into()),
        ("(doseq [x xs :bad 1 y ys :worse 2] x)", "doseq: Error: Invalid 'doseq' keyword:worse; definition compiler_sequence_macros.cljs:63:1".into()),
        ("(doseq [x] x)", "doseq: Error: Assert failed: doseq requires an even binding vector\n(and (vector? seq-exprs) (even? (count seq-exprs))); definition compiler_sequence_macros.cljs:63:1".into()),
        (&thread, format!("->: Error: Unsupported macro result depth limit at {depth}; definition compiler_core_macros.cljs:96:1")),
    ];
    for (source, expected) in cases {
        let error = analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("macro cljs.core/{expected}")),
            "{source}\nexpected {expected:?}\nin {error}"
        );
    }
    // Expansions the analysis of the result rejects.
    for (source, expected) in [
        (
            "(defmulti user/f identity)",
            "binding must be an unqualified symbol",
        ),
        ("(for [] 1)", "unsupported binding form"),
    ] {
        let error = analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
    // The engine's conversion of a call over the node limit.
    let mut host = MacroHost::new();
    host.limits.generated_nodes = 100;
    let source = format!("(when x [{}])", "0 ".repeat(200));
    let error = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("macro cljs.core/when: compiler input conversion limit; definition compiler_core_macros.cljs:29:1"),
        "{error}"
    );
}
