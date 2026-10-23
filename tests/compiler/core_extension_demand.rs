//! Core macro expansion preserves bootstrap and user extension demand frontiers.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn both(host: &mut MacroHost, source: &str, expected: &str) {
    let program = analyze_with_macros(source, &parse(source).unwrap(), host)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator.evaluate(&program).unwrap();
    assert_eq!(evaluator.print(value).unwrap(), expected);
    let script = format!(
        "const rt = {}; console.log(rt.core.prStr(({})(rt)));",
        light_cljs::evaluator::RUNTIME_SOURCE,
        emit_js_unit(&program)
    );
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{expected}\n")
    );
}

#[test]
fn core_macros_expand_collection_extensions_in_fresh_and_reused_hosts() {
    let row = include_str!("../fixtures/compiler_core_extension_demand.tsv")
        .lines()
        .find(|line| !line.starts_with('#'))
        .unwrap();
    let fields: Vec<_> = row.split('\t').collect();
    let source = format!("(pr-str {})", fields[1]);
    let expected = format!("{:?}", fields[2]);
    let mut reused = MacroHost::new();
    for _ in 0..2 {
        for host in [&mut reused, &mut MacroHost::new()] {
            both(host, &source, &expected);
            both(
                host,
                &format!("(defmacro contract [] {source}) (contract)"),
                &expected,
            );
        }
    }
}

#[test]
fn core_macro_frontier_restores_ordered_user_equality_extensions() {
    let source = r#"
      (extend-type PersistentVector IEquiv
        (-equiv [_ other] (and (keyword? other) (= other :first))))
      (defmacro observed [] (.equiv [] :first))
      (def earlier (observed))
      (defmacro freeze [] earlier) (freeze)
      (extend-type PersistentVector IEquiv
        (-equiv [_ other] (and (keyword? other) (= other :second))))
      (defmacro current []
        (str (.equiv [] :first) ":" (.equiv [] :second)))
      [earlier (observed) (current) (.equiv [] :first) (.equiv [] :second)]"#;
    let mut host = MacroHost::new();
    for _ in 0..2 {
        both(&mut host, source, "[true false \"false:true\" false true]");
    }
}

#[test]
fn failed_extension_demand_recovers_without_hiding_real_cycles() {
    let mut host = MacroHost::new();
    for (source, diagnostic) in [
        (
            r#"(extend-type PersistentVector IEquiv
              (-equiv [_ other] (and (when-let [x] x) true)))
            (defmacro demand [] (.equiv [] :x)) (demand)"#,
            "conditional binding requires exactly two forms",
        ),
        (
            r#"(declare target)
            (extend-type target IEquiv
              (-equiv [_ other] (and (keyword? other) true)))
            (def target (do (-equiv [] :x) PersistentVector))
            (defmacro demand [] (-equiv [] :x)) (demand)"#,
            "cycle",
        ),
    ] {
        let error = light_cljs::compile_with_macros(source, &mut host)
            .expect_err("invalid demand must fail")
            .to_string();
        assert!(error.contains(diagnostic), "{error}");
        if diagnostic == "cycle" {
            assert!(
                error.contains("bundle initialization cycle")
                    || error.contains("compile dependency cycle"),
                "{error}"
            );
            assert!(
                error.contains("extension") || error.contains("declaration#"),
                "{error}"
            );
        }
        both(
            &mut host,
            r#"
          (extend-type PersistentVector IEquiv
            (-equiv [_ other] (and (keyword? other) (= other :repaired))))
          (defmacro repaired [] (.equiv [] :repaired))
          [(repaired) (.equiv [] :repaired) (.equiv [] :wrong)]"#,
            "[true true false]",
        );
    }
}

/// The compile-time runtime's protocol lookups, which build.rs inlines for an
/// interpreter (`interpreterDispatch`, light-cljs-439): demands only for
/// extended protocols, store tests in another order and none for a
/// primitive. A macro body sees what the parent compiler gave it, for
/// runtime values with and without their own methods, records, types and
/// host values with `extend-type` implementations, and so does the program.
#[test]
fn interpreter_dispatch_keeps_protocol_answers_for_runtime_and_host_values() {
    let source = r#"
      (defrecord R [a])
      (deftype Box [v] ISeqable (-seq [_] (list v v)))
      (extend-type string ICounted (-count [_] 42))
      (extend-protocol ILookup object (-lookup ([_ k] [:object k]) ([_ k nf] [:object k nf])))
      (defmacro probe []
        (let [m {:a 1 :b 2 :c 3} r (->R 5)
              own (specify! [1 2] ISeqable (-seq [_] (list :own)))]
          (pr-str [(get m :c) (get m :z :none) (seq "ab") (count "abc") (count r) (get r :a) (seq (Box. 7))
                   (seq own) (seq (specify! (js-obj) ISeqable (-seq [_] (list 1 2)))) (get (js-obj) :k) (get (js-obj) :k 0)
                   (= [1 2] '(1 2)) (= :a :b) (= {:a 1} r) (satisfies? ISeqable "s") (satisfies? ICounted "s")
                   (satisfies? ILookup (js-obj)) (satisfies? ISeqable (Box. 1)) (hash "x") (count (seq (range 3))) (first (lazy-seq [9]))
                   (meta (with-meta [1] {:k 1})) (vector? own) (map? r) (coll? r) (seq? (list 1))])))
      (probe)"#;
    let expected = format!(
        "{:?}",
        r#"[3 :none ("a" "b") 3 1 5 (7 7) (:own) (1 2) [:object :k] [:object :k 0] true false false false true true true -2120728247 3 9 {:k 1} true true true true]"#
    );
    let mut reused = MacroHost::new();
    for _ in 0..2 {
        for host in [&mut reused, &mut MacroHost::new()] {
            both(host, source, &expected);
        }
    }
}
