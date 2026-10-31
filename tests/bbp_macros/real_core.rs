//! `--macro-core real` (light-cljs-648; docs/real-core-design.md, section 5,
//! "Phase 1 results"): the Pants build's macros on the real cljs.core,
//! booted once per compilation and published.

use super::{program_output, temp_dir};
use light_cljs::evaluator::MacroCore;
use light_cljs::{MacroHost, analyze_with_macros, emit_js, parse};

fn host(core: MacroCore) -> MacroHost {
    let mut host = MacroHost::new();
    host.use_native_macros();
    host.evaluator_options.macro_core = core;
    // The real core boots inside the build's deadline: seconds in a debug
    // build on a loaded machine.
    host.evaluator_options.timeout = std::time::Duration::from_secs(600);
    host
}

/// A core function the port never had (`reductions`, a lazy sequence of
/// `reduce`'s steps) answers on the real core, where the port's getter
/// throws: the real core's names resolve from its published hub.
#[test]
fn real_core_resolves_names_the_port_lacks() {
    let source = "(defmacro steps [] (list 'quote [(vec (reductions + [1 2 3])) (count (reductions + 10 [1 2]))])) \
                  (js/console.log (pr-str (steps)))";
    let Err(error) =
        analyze_with_macros(source, &parse(source).unwrap(), &mut host(MacroCore::Port))
    else {
        panic!("the port has no reductions");
    };
    assert!(
        error
            .to_string()
            .contains("unsupported operation: native macro runtime core.reductions"),
        "{error}"
    );
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host(MacroCore::Real))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(program_output(&emit_js(&program)), "[[1 3 6] 3]\n");
}

/// A macro both cores support expands to the same program on each.
#[test]
fn real_core_expands_as_the_port_where_both_answer() {
    let source = "(defmacro pairs [] (list 'quote (mapv (fn [x] [x (inc x)]) (range 3)))) \
                  (js/console.log (pr-str (pairs)))";
    let emit = |core| {
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host(core))
            .unwrap_or_else(|e| panic!("{core:?}: {e}"));
        emit_js(&program)
    };
    let port = emit(MacroCore::Port);
    assert_eq!(program_output(&port), "[[0 1] [1 2] [2 3]]\n");
    assert_eq!(emit(MacroCore::Real), port);
}

/// A demanded value can replace a core Var through a writable task slot,
/// while the published hub retains its original, read-only function.
#[test]
fn real_core_replacements_stay_in_writable_task_slots() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("real-core-replacement");
    std::fs::write(
        root.join("app.cljs"),
        r#"(ns app)
           (def result
             (do (set! cljs.core/inc (fn [_] 42))
                 (inc 2)))
           (defmacro m []
             (list 'quote [result
                  (js* "Object.getPrototypeOf(globalThis.__lightNativeRuntime.core).inc(2)")
                  (js* "Object.getOwnPropertyDescriptor(globalThis.__lightNativeRuntime.core, 'inc').writable")
                  (js* "Object.getOwnPropertyDescriptor(Object.getPrototypeOf(globalThis.__lightNativeRuntime.core), 'inc').writable")]))
           (js/console.log (pr-str (m)))"#,
    )
    .unwrap();
    for threads in [1, 2, 4, 8] {
        let (program, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host(MacroCore::Real),
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(threads).unwrap(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("{threads} threads: {e:?}"));
        assert_eq!(
            program_output(&emit_js(&program)),
            "[42 3 true false]\n",
            "{threads} threads"
        );
    }
}

/// `--stats` names the core, and with the real one how many of its hub's
/// members published, how many tasks booted a private copy for the rest,
/// and the boot's time.
#[test]
fn real_core_stats_report_the_boot_and_publication() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("real-core-stats");
    std::fs::write(
        root.join("tools.cljs"),
        "(ns tools) (defmacro tag [x] (list 'quote [x (count (reductions + [1 2]))]))",
    )
    .unwrap();
    std::fs::write(
        root.join("app.cljs"),
        "(ns app (:require [tools :refer [tag]])) (js/console.log (pr-str [(tag 1) (tag 2)]))",
    )
    .unwrap();
    let report = |core| {
        let mut host = host(core);
        let mut stats = light_cljs::CompileStats::new();
        let (program, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                stats: Some(&mut stats),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("{core:?}: {e:?}"));
        (program_output(&emit_js(&program)), stats.report())
    };
    let value = |report: &str, key: &str| -> String {
        let prefix = format!("\n{key} ");
        let at = report
            .find(&prefix)
            .unwrap_or_else(|| panic!("{key} missing: {report}"));
        report[at + prefix.len()..]
            .lines()
            .next()
            .unwrap()
            .to_owned()
    };
    let (output, real) = report(MacroCore::Real);
    assert_eq!(output, "[[1 2] [2 2]]\n");
    assert_eq!(value(&real, "native.core.real"), "1");
    let number = |key| value(&real, key).parse::<f64>().unwrap();
    let (members, published) = (
        number("native.core.members"),
        number("native.core.published"),
    );
    assert!(published > 0.0 && published <= members, "{real}");
    assert_eq!(number("native.core.per-task"), members - published);
    assert!(number("native.core.boot-ms") > 0.0, "{real}");
    let source = "(ns app) (defmacro m [] 1) (js/console.log (m))";
    std::fs::write(root.join("app.cljs"), source).unwrap();
    let (_, port) = report(MacroCore::Port);
    assert_eq!(value(&port, "native.core.real"), "0");
    assert!(!port.contains("native.core.members"), "{port}");
}

/// The emitted code of a `binding` of a core dynamic Var (and the
/// `with-out-str` built on it) writes the cell the real core's printer
/// reads, and the write is undone when the body throws.
#[test]
fn real_core_dynamic_bindings_reach_runtime_functions() {
    let source = "(defmacro printed [] \
                    (list 'quote \
                      [(binding [*print-readably* false] (pr-str \"x\")) \
                       (binding [*print-length* 2] (pr-str [1 2 3])) \
                       (with-out-str (print \"a\" 1) (prn \"b\")) \
                       (try (binding [*print-readably* false] (throw (ex-info \"boom\" {}))) \
                         (catch :default _ nil)) \
                       (pr-str \"x\") \
                       (pr-str [1 2 3])])) \
                  (js/console.log (pr-str (printed)))";
    let expected = "[\"x\" \"[1 2 ...]\" \"a 1\\\"b\\\"\\n\" nil \"\\\"x\\\"\" \"[1 2 3]\"]\n";
    for core in [MacroCore::Port, MacroCore::Real] {
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host(core))
            .unwrap_or_else(|e| panic!("{core:?}: {e}"));
        assert_eq!(program_output(&emit_js(&program)), expected, "{core:?}");
    }
}

/// The real core's functions read a macro's arguments as the Rust bridge
/// imports them (light-cljs-666, the view of imported forms): a vector,
/// map, list, symbol or keyword through its occurrence (`partition`,
/// `get`, `name`, `str`, `=`, `meta`, printing), and a scalar through a
/// computation boundary (`inc`, `str`, `=`, `nth` with an imported index).
/// The port answers the same.
#[test]
fn real_core_reads_imported_forms_through_their_occurrences() {
    let source = "(defmacro over [v m l k s n] \
                    (list 'quote \
                      [(count (partition 2 v)) (vec (partition 2 v)) \
                       (str k s n) (name k) (namespace k) \
                       (= n 2) (inc n) (nth v n) (= :b/c k) (keyword? k) (symbol? (first l)) \
                       (get m :a) (vec (keys m)) (count m) (contains? m :a) (assoc m :c 3) \
                       (first v) (vec (map inc v)) (conj v 9) (vector? v) (list? l) (count l) \
                       (:x (meta (first l))) (pr-str v m l k s n) (count (hash-set k :a)) \
                       (= v [1 2 3 4]) (= m {:a 1 :b 2}) (= l '(^{:x 1} a b)) \
                       (= (into #{} v) #{1 2 3 4}) (get (frequencies [n n 1]) n)])) \
                  (js/console.log (pr-str (over [1 2 3 4] {:a 1 :b 2} (^{:x 1} a b) :b/c \"s\" 2)))";
    let expected = "[2 [(1 2) (3 4)] \":b/cs2\" \"c\" \"b\" true 3 3 true true true 1 [:a :b] 2 true {:a 1, :b 2, :c 3} 1 [2 3 4 5] [1 2 3 4 9] true true 2 1 \"[1 2 3 4] {:a 1, :b 2} (a b) :b/c \\\"s\\\" 2\" 2 true true true true 2]\n";
    for core in [MacroCore::Port, MacroCore::Real] {
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host(core))
            .unwrap_or_else(|e| panic!("{core:?}: {e}"));
        assert_eq!(program_output(&emit_js(&program)), expected, "{core:?}");
    }
}

/// Imported scalar provenance must coexist with ordinary user computations,
/// including applied arithmetic, primitive predicates, string operations and
/// collection indexes. This deliberately passes scalars as macro arguments.
#[test]
fn real_core_computes_with_imported_scalar_occurrences() {
    let source = r#"
      (defmacro bump [x] (inc x))
      (defmacro compute [n d z s c yes no nothing]
        (list 'quote
          [(+ n d) (- n d) (* n d) (/ n d) (inc n) (dec n)
           (mod n d) (quot n d) (rem n d)
           (apply + [n d n]) (apply * [n d d]) (apply < [d n (+ n d)])
           (bit-and n d) (bit-shift-left d d) (even? n) (integer? n)
           (pos-int? n) (zero? z) (number? n) (boolean? no) (false? no)
           (true? yes) (nil? nothing) (not nothing) (not no) (boolean no)
           (= n 6) (= no false) (= nothing nil) (= s "abcd") (= c \a)
           (identical? s "abcd") (= (hash n) (hash 6))
           (str n ":" s ":" no ":" nothing) (subs s d) (count s)
           (first s) (nth s d) (nth [c s] z) (get [s c] z)
           (contains? [s c] z) (name s) (name (keyword s))
           (vec (range z n d)) (vec (take d [c s n]))
           (vec (partition d [n d z n]))]))
      (js/console.log (bump 2))
      (js/console.log (pr-str (compute 6 2 0 "abcd" \a true false nil)))
    "#;
    let expected = "3\n[8 4 12 3 7 5 0 3 0 14 24 true 2 8 true true true true true true true true true true true false true true true true true true true \"6:abcd:false:\" \"cd\" 4 \"a\" \"c\" \"a\" \"abcd\" true \"abcd\" \"abcd\" [0 2 4] [\"a\" \"abcd\"] [(6 2) (0 6)]]\n";
    for core in [MacroCore::Port, MacroCore::Real] {
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host(core))
            .unwrap_or_else(|e| panic!("{core:?}: {e}"));
        assert_eq!(program_output(&emit_js(&program)), expected, "{core:?}");
    }
}

#[test]
fn real_core_engine_atoms_read_imported_nil_options() {
    let source = r#"
      (defmacro options [nothing state]
        (let [a (atom state :validator nothing)
              first-state (reset! a (inc state))
              selected (set-validator! a nothing)
              second-state (swap! a inc)
              metadata (reset-meta! a nothing)]
          (list 'quote [first-state (nil? (get-validator a))
                        (js* "~{} === ~{}" nothing selected)
                        second-state (nil? metadata) (nil? (meta a))])))
      (js/console.log (pr-str (options nil 2)))
    "#;
    for core in [MacroCore::Port, MacroCore::Real] {
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host(core))
            .unwrap_or_else(|e| panic!("{core:?}: {e}"));
        assert_eq!(
            program_output(&emit_js(&program)),
            "[3 true true 4 true true]\n",
            "{core:?}"
        );
    }
}

#[test]
fn real_core_reads_imported_nil_in_function_combinators() {
    let source = r#"
      (defmacro f [x] ((fnil identity 9) x))
      (defmacro defaults [n f a b]
        (list 'quote
          [((fnil vector a b a) n n n n)
           (apply (fnil vector a b) [n n f a])
           ((fnil identity a) f)
           ((complement identity) n) ((complement identity) f)
           ((complement identity) a)]))
      (js/console.log (f nil))
      (js/console.log (pr-str (defaults nil false 7 8)))
    "#;
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host(MacroCore::Real))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        program_output(&emit_js(&program)),
        "9\n[[7 8 7 nil] [7 8 false 7] false true true false]\n"
    );
}

#[test]
fn real_core_keep_reads_imported_nil_in_lazy_and_transducer_paths() {
    let large = ["nil", "false", "1", "2"].repeat(10).join(" ");
    let source = format!(
        r#"
      (defmacro keep-values [v l large]
        (list 'quote
          [(vec (keep identity v)) (vec (keep identity l))
           (vec (keep-indexed (fn [_ x] x) v))
           (into [] (keep identity) v)
           (into [] (keep-indexed (fn [_ x] x)) l)
           (count (keep identity large))
           (count (keep-indexed (fn [_ x] x) large))
           (count (into [] (keep identity) large))]))
      (js/console.log (pr-str (keep-values [nil false \a "a"] (nil false \a "a") [{large}])))
    "#
    );
    let program = analyze_with_macros(
        &source,
        &parse(&source).unwrap(),
        &mut host(MacroCore::Real),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        program_output(&emit_js(&program)),
        "[[false \"a\" \"a\"] [false \"a\" \"a\"] [false \"a\" \"a\"] [false \"a\" \"a\"] [false \"a\" \"a\"] 30 30 30]\n"
    );
}

#[test]
fn real_core_reads_imported_partition_and_stride_sizes() {
    let source = r#"
      (defmacro sized [n step]
        (list 'quote
          [(vec (partitionv n [1 2 3 4]))
           (vec (partitionv n step [1 2 3 4]))
           (vec (partitionv n n [9] [1 2 3]))
           (vec (partitionv-all n [1 2 3]))
           (into [] (partitionv-all n) [1 2 3])
           (into [] (partition-all n) [1 2 3])
           (vec (take-nth n [0 1 2 3 4]))
           (into [] (take-nth n) [0 1 2 3 4])
           (vec (nthrest [0 1 2 3] n)) (vec (nthnext [0 1 2 3] n))]))
      (js/console.log (pr-str (sized 2 1)))
    "#;
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host(MacroCore::Real))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        program_output(&emit_js(&program)),
        "[[[1 2] [3 4]] [[1 2] [2 3] [3 4]] [[1 2] [3 9]] [[1 2] [3]] [[1 2] [3]] [[1 2] [3]] [0 2 4] [0 2 4] [2 3] [2 3]]\n"
    );
}

#[test]
fn real_core_iteration_reads_imported_nil_without_erasing_values() {
    let source = r#"
      (defmacro iterated [n a]
        (list 'quote
          [(vec (take 3 (iteration (constantly n))))
           (into [] (take 3) (iteration (constantly n)))
           (vec (take 3 (iteration (constantly a) :kf (constantly n))))
           (into [] (take 3) (iteration (constantly a) :kf (constantly n)))]))
      (js/console.log (pr-str (iterated nil 7)))
    "#;
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host(MacroCore::Real))
        .unwrap_or_else(|e| panic!("macro expansion: {e}"));
    assert_eq!(program_output(&emit_js(&program)), "[[] [] [7] [7]]\n");
}

/// `*ns*` in a macro is the evaluator's namespace context on the real core
/// too, as on the port: the calling namespace while a macro runs and while
/// the helper it reads is initialized on demand inside it, and the caller's
/// again after a `binding` of `*ns*` returns. (The names stay symbols: the
/// real `str` of a namespace name the compiler supplied, an imported form,
/// waits for light-cljs-666's view of imported forms.)
#[test]
fn real_core_preserves_macro_namespace_context() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("real-core-namespace");
    std::fs::write(
        root.join("tools.cljs"),
        "(ns tools) (def defined-in (ns-name *ns*)) \
         (defmacro where [] \
           (list 'quote [(ns-name *ns*) (str *ns*) defined-in \
                         (binding [*ns* (create-ns 'elsewhere)] (ns-name *ns*)) \
                         (ns-name *ns*)]))",
    )
    .unwrap();
    std::fs::write(
        root.join("app.cljs"),
        "(ns app (:require [tools :refer [where]])) (js/console.log (pr-str (where)))",
    )
    .unwrap();
    let output = |core| {
        let (program, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host(core),
            Default::default(),
            &mut Vec::new(),
            EntryRecorders::default(),
        )
        .unwrap_or_else(|e| panic!("{core:?}: {e:?}"));
        program_output(&emit_js(&program))
    };
    let expected = "[app \"app\" app elsewhere app]\n";
    assert_eq!(output(MacroCore::Port), expected);
    assert_eq!(output(MacroCore::Real), expected);
}

/// Selecting either of two scalars must preserve its reader representation,
/// even when both have the same JavaScript primitive value.
#[test]
fn real_core_preserves_selected_scalar_occurrences() {
    use light_cljs::{
        CollectionKind,
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, ExpansionOrigin, Name, Source},
        evaluator::MacroEvaluator,
    };
    for core in [MacroCore::Port, MacroCore::Real] {
        let mut host = host(core);
        let evaluator = BbpEvaluator::new(host.evaluator_options.clone()).unwrap();
        let environment = Datum::new(DatumKind::Collection {
            kind: CollectionKind::Map,
            children: Box::new([]),
        });
        for (selected, body) in [
            (1, "a"),
            (2, "b"),
            (1, "(+ a)"),
            (1, "(byte a)"),
            (2, "(double b)"),
            (1, "(min a)"),
            (2, "(max b)"),
            (1, "((fnil identity b) a)"),
            (2, "((fnil identity b) nil)"),
            (1, "(first (keep identity [nil a b]))"),
            (1, "(first (into [] (keep identity) [nil a b]))"),
        ] {
            let source = format!("(fn [form env a b] {body})");
            let program =
                analyze_with_macros(&source, &parse(&source).unwrap(), &mut host).unwrap();
            let function = evaluator
                .evaluate_unit(&light_cljs::emitter::emit_js_unit(&program))
                .unwrap();
            for input in [
                "(pick \\a \"a\")",
                "(pick \"a\" \\a)",
                "(pick 1 1.0)",
                "(pick 1.0 1)",
                "(pick 9007199254740992 9007199254740993)",
                "(pick 2.5 2.5)",
            ] {
                let call = host
                    .read_compiler_form(&Source::new("scalars.cljs", input))
                    .unwrap()
                    .unwrap();
                let result = host
                    .expand_javascript(
                        &evaluator,
                        function,
                        &call,
                        &environment,
                        ExpansionOrigin {
                            macro_name: Name::new(None, "pick"),
                            call: call.span(),
                            definition: None,
                            helpers: vec![],
                        },
                    )
                    .unwrap_or_else(|e| panic!("{core:?} {input}, select {selected}: {e}"));
                assert_eq!(
                    result.data().inspect(),
                    call.children()[selected].data().inspect(),
                    "{core:?} {input}, select {selected}"
                );
                let original = &call.children()[selected];
                assert!(std::ptr::eq(result.data().kind(), original.data().kind()));
                assert_eq!(result.span().bytes(), original.span().bytes());
            }
            evaluator.release(function).unwrap();
        }
    }
}
