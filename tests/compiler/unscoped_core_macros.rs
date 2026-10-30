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

/// Prints, from a `simple-benchmark` `:print-fn`, its line up to the run
/// count, which holds the bindings and expression as the definition printed
/// them.
const PRINTED: &str = r#"(def p (fn [line] (js/console.log (first (.split line " runs")))))"#;

/// Calls whose shape native expansion leaves to the definition expand as
/// the V8 build's engine expands them, outside a namespace and in one.
#[test]
fn valid_declined_core_calls_expand_like_v8() {
    let source = format!(
        r#"{PRINTED}
        (simple-benchmark [s #{{3 1 2 :x "y"}}] (count s) 1 :print-fn p)
        (defmulti area \a :shape)
        (defmethod area :sq [x] 4)
        (js/console.log (area {{:shape :sq}}) (:doc (meta (var area))))
        (js/console.log (pr-str ((selector {{:keys [a] :strs [b] :syms [c] :x/keys [d] :y/keys [e] :z/keys [f] :w/keys [g] :or {{a 0}} :select s}}) {{:a 1 "b" 2 :x/d 3}})))"#
    );
    let expected = "[s #{3 1 2 :x \"y\"}], (count s), 1\n4 a\n{:a 1, :x/d 3, \"b\" 2}\n";
    assert_eq!(output(&source), expected);
    let root = crate::temp_dir::TempDir::new("declined-core-calls");
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/main.cljs"),
        format!("(ns app.main)\n{source}\n"),
    )
    .unwrap();
    let js = light_cljs::compile_namespace("app.main", &[root.path().clone()])
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(execute(&js), expected);
}

/// A `doseq` of nine bindings, past the native expander's name budget,
/// expands as the V8 build's engine expands it where the limits leave room
/// for its expansion, and fails as the engine's conversion of that
/// expansion fails at the default node limit (light-cljs-620).
#[test]
fn declined_doseq_expands_with_raised_node_limit() {
    let source =
        "(doseq [a [0 1] b [0] c [0] d [0] e [0] f [0] g [0] h [0] i [2 3]] (js/console.log a i))";
    let mut host = MacroHost::new();
    host.limits.generated_nodes = 10_000_000;
    host.limits.expansions = 100_000;
    let js = compile_with_macros(source, &mut host).unwrap_or_else(|error| panic!("{error}"));
    // Too long for a command line.
    let root = crate::temp_dir::TempDir::new("declined-doseq");
    let file = root.join("main.js");
    std::fs::write(&file, &js).unwrap();
    let ran = std::process::Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg(&file)
        .output()
        .expect("compiler integration requires Node");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "0 2\n0 3\n1 2\n1 3\n");
    let error = analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
        .unwrap_err()
        .to_string();
    let path = "$[2][3][2][3][2][2][2][2][2][3][2][3][2][2][3][2][3][2][2][2][2][2][3][2][3][2][2][2][2][2][2][3]";
    let expected = format!(
        "macro cljs.core/doseq: Error: Unsupported macro result lazy realization failure at {path}; definition compiler_sequence_macros.cljs:63:1"
    );
    assert!(without_stack(&error).contains(&expected), "{error}");
}

/// A regular expression of such a `doseq` that does not compile fails as
/// the engine fails constructing it while it decodes the call, before any
/// conversion limit and whatever the limits.
#[test]
fn declined_doseq_rejects_invalid_regex_before_conversion_limits() {
    let bindings = "a [0] b [0] c [0] d [0] e [0] f [0] g [0] h [0] i [0]";
    for (regex, error) in [
        (
            "[",
            "Invalid regular expression: /[/: Unterminated character class",
        ),
        ("(?i)(?", "Invalid regular expression: /(?/i: Invalid group"),
        (")", "Invalid regular expression: /)/: Unmatched ')'"),
        ("(?x)a", "Invalid flags supplied to RegExp constructor 'x'"),
        // Reasons that depend on where the pattern stops.
        ("^*", "Invalid regular expression: /^*/: Nothing to repeat"),
        (
            "(?<=a)*",
            "Invalid regular expression: /(?<=a)*/: Invalid quantifier",
        ),
        (
            "(?u)a{",
            "Invalid regular expression: /a{/u: Incomplete quantifier",
        ),
        (
            "(?u)^{",
            "Invalid regular expression: /^{/u: Lone quantifier brackets",
        ),
        (
            "(?i-i:a)",
            "Invalid regular expression: /(?i-i:a)/: Repeated flag in flag group",
        ),
        (
            "a(?i)b",
            "Invalid regular expression: /a(?i)b/: Invalid group",
        ),
        (
            "(?u)[\\p{Foo}]",
            "Invalid regular expression: /[\\p{Foo}]/u: Invalid property name in character class",
        ),
    ] {
        let source = format!("(doseq [{bindings}] #\"{regex}\")");
        for raised in [false, true] {
            let mut host = MacroHost::new();
            if raised {
                host.limits.generated_nodes = 10_000_000;
                host.limits.expansions = 100_000;
            }
            let failure = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
                .unwrap_err()
                .to_string();
            let expected = format!("macro cljs.core/doseq: SyntaxError: {error}; definition");
            // The QuickJS reference words the reason in its own way, in
            // its `RegExp` constructor all the same.
            let found = if cfg!(all(light_quickjs, not(light_v8))) {
                failure.contains("macro cljs.core/doseq: ") && failure.contains("at RegExp")
            } else {
                without_stack(&failure).contains(&expected)
            };
            assert!(found, "{source} raised {raised}: {failure}");
        }
    }
}

/// Regular expressions are checked as their UTF-16 code units: a range
/// from a lone surrogate that a macro built (`\uD800`, which no UTF-8 text
/// holds) to `\uE000` compiles, and astral characters are their two units
/// each, as the engine reads them without the u flag and as one character
/// with it.
#[test]
fn declined_doseq_preserves_surrogate_regex_ranges() {
    let bindings = "[a [0] b [0] c [0] d [0] e [0] f [0] g [0] h [0] i [2 3]]";
    let source = format!(
        r#"(defmacro wide [] (list 'doseq '{bindings} (list 'js/console.log (list '.test (js/RegExp. "^[\uD800-\uE000]$") "\uDBFF") (list '.test #"(?u)^[😀-😂]$" "😁") 'i)))
        (wide)"#
    );
    // The QuickJS reference's own decoding refuses the lone surrogate's
    // range ("invalid class range").
    if cfg!(all(light_quickjs, not(light_v8))) {
        return;
    }
    let mut host = MacroHost::new();
    host.limits.generated_nodes = 10_000_000;
    host.limits.expansions = 100_000;
    let js = compile_with_macros(&source, &mut host).unwrap_or_else(|error| panic!("{error}"));
    let root = crate::temp_dir::TempDir::new("surrogate-regex");
    let file = root.join("main.js");
    std::fs::write(&file, &js).unwrap();
    let ran = std::process::Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg(&file)
        .output()
        .expect("compiler integration requires Node");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        "true true 2\ntrue true 3\n"
    );
    // Without the u flag an astral range is out of order between its units.
    let source = format!("(doseq {bindings} #\"[😀-😂]\")");
    let error = analyze_with_macros(&source, &parse(&source).unwrap(), &mut MacroHost::new())
        .unwrap_err()
        .to_string();
    assert!(
        without_stack(&error).contains(
            "macro cljs.core/doseq: SyntaxError: Invalid regular expression: /[😀-😂]/: Range out of order in character class"
        ),
        "{error}"
    );
}

/// The engine's snapshot of a call charges a map's keys it finds equal
/// (`0` and `-0.0`) as one entry, and the Pants build's emulation of it
/// refuses the call where the engine does (light-cljs-620).
#[test]
fn snapshots_of_calls_merge_keys_the_engine_finds_equal() {
    let source = "(if-not 1 2 3 4 {-0.0 1 0 2})";
    // From a limit of 16 the V8 build also converts the environment, which
    // it then refuses.
    for (limit, expected) in [
        (10, "compiler input conversion limit"),
        (11, "Error: Unsupported macro result sequence limit at $"),
        (12, "Error: Unsupported macro result node limit at $[5]"),
        (
            13,
            "Error: Unsupported macro result node limit at $[5].key[0]",
        ),
        (
            14,
            "Error: Unsupported macro result node limit at $[5].value[0]",
        ),
        (15, "Error: Unsupported macro result sequence limit at $"),
    ] {
        let mut host = MacroHost::new();
        host.limits.generated_nodes = limit;
        let error = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_err()
            .to_string();
        // The QuickJS reference names its conversion between the two.
        let error = without_stack(&error);
        assert!(
            error.contains("macro cljs.core/if-not: ")
                && error.contains(&format!("{expected}; definition")),
            "{limit}: {error}"
        );
    }
}

/// `simple-benchmark` prints its bindings and expression as the engine
/// decodes them (keys equal there merge, larger maps and sets take their
/// hashes' order, negative zero stays `-0.0`) and as the compile-time
/// printer settings a source macro set say.
#[test]
fn benchmark_printing_matches_the_engine() {
    let source = format!(
        r#"{PRINTED}
        (simple-benchmark [m {{\a 1 "a" 2}} n {{1 :i 1.0 :d}} z -0.0] -0.0 1 :print-fn p)
        (simple-benchmark [m {{:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}} s #{{3 1 2 :x "y"}}] m 1 :print-fn p)
        (defmacro shorten [] (set! *print-length* 1) nil)
        (shorten)
        (simple-benchmark [x 1 y 2] [x y] 1 :print-fn p)"#
    );
    assert_eq!(
        output(&source),
        "[m {\"a\" 2} n {1 :d} z -0.0], -0.0, 1\n[m {:e 5, :g 7, :c 3, :h 8, :b 2, :d 4, :f 6, :i 9, :a 1} s #{3 1 2 :x \"y\"}], m, 1\n[x ...], [x ...], 1\n"
    );
    // Nine reader entries that the engine merges to eight still take
    // hash order, and metadata decodes too when it prints.
    let source = format!(
        r#"{PRINTED}
        (simple-benchmark [m {{\a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}}] 0 1 :print-fn p)
        (defmacro with-meta-printed [] (set! *print-meta* true) nil)
        (with-meta-printed)
        (simple-benchmark [] ^{{\a 1 "a" 2}} [1] 1 :print-fn p)"#
    );
    assert_eq!(
        output(&source),
        "[m {:e 6, :g 8, :c 4, \"a\" 2, :h 9, :b 3, :d 5, :f 7}], 0, 1\n[], ^{\"a\" 2} [1], 1\n"
    );
}

/// Loading a core macro's definition leaves the program's own definitions
/// of the same names alone.
#[test]
fn core_definition_fallback_preserves_user_helpers() {
    let source = format!(
        r#"{PRINTED}
        (defn macro-decoded [x] 42)
        (defn macro-simple-benchmark [& _] 43)
        (defmacro use-helpers [] [(macro-decoded 1) (macro-simple-benchmark 1)])
        (js/console.log (pr-str (use-helpers)))
        (simple-benchmark [] 0 1 :print-fn p)
        (js/console.log (pr-str (use-helpers)))"#
    );
    assert_eq!(output(&source), "[42 43]\n[], 0, 1\n[42 43]\n");
}

/// A call its definition rejects fails as the V8 build reports it, also
/// after an earlier call of the macro ran the definition.
#[test]
fn cached_core_definitions_keep_rejection_checks() {
    let expected = "macro cljs.core/simple-benchmark: Error: Unsupported macro result undefined at $[2][1][3][1][1]; definition compiler_core_macros.cljs:238:1";
    for source in [
        "(simple-benchmark [] 1)",
        "(simple-benchmark [] 0 1 :print-fn identity) (simple-benchmark [] 1)",
    ] {
        let error = analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
            .unwrap_err()
            .to_string();
        assert!(
            without_stack(&error).contains(expected),
            "{source}\n{error}"
        );
    }
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
        let error = without_stack(&error);
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
    // The QuickJS reference names its conversion between the two.
    assert!(
        error.contains("macro cljs.core/when: ")
            && error.contains(
                "compiler input conversion limit; definition compiler_core_macros.cljs:29:1"
            ),
        "{error}"
    );
}

/// `error` without the stack the QuickJS reference adds to an engine error
/// (`    at call (...)` lines before `; definition`), which the V8 and Pants
/// builds do not give.
fn without_stack(error: &str) -> String {
    error
        .split('\n')
        .filter(|line| !line.starts_with("    at "))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("\n; definition", "; definition")
}
