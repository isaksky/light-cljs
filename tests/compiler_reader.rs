use light_cljs::{MacroHost, compile, compile_with_macros};
use std::io::Write;
use std::process::{Command, Stdio};
fn run(js: &str) -> String {
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
        .write_all(format!("console.log({js});").as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn check(source: &str, expected: &str) {
    let js = compile(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    let quickjs = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
    let value = quickjs.evaluate_js(&format!("String({js})")).unwrap();
    assert_eq!(
        quickjs.string(value).unwrap(),
        expected,
        "QuickJS: {source}"
    );
    assert_eq!(run(&js), format!("{expected}\n"));
}

#[test]
fn complete_pinned_syntax_quote_and_tagged_bodies_execute_in_both_engines() {
    use light_cljs::compiler_data::Name;
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reader-coverage");
    for (namespace, assertions) in [
        ("cljs.syntax-quote-test", "4"),
        ("cljs.tagged-literals-test", "5"),
    ] {
        let mut host = MacroHost::new();
        host.register_compiler_data_reader(Name::new(Some("cljs"), "tag"), "identity")
            .unwrap();
        let js = light_cljs::compile_namespace_with_macros(
            namespace,
            std::slice::from_ref(&root),
            &mut host,
        )
        .unwrap();
        assert_eq!(run(&js), format!("{assertions}\n"));
        let e = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
        assert_eq!(
            e.number(e.evaluate_js(&js).unwrap()).unwrap().to_string(),
            assertions
        );
    }
}

#[test]
fn reader_api_arities_and_rest_execute_in_application_and_macro_phases() {
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reader-coverage");
    let js = light_cljs::compile_namespace("reader.api", &[root]).unwrap();
    assert_eq!(run(&js), "true\n");
    let e = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
    assert_eq!(
        e.string(e.evaluate_js(&format!("String({js})")).unwrap())
            .unwrap(),
        "true"
    );
}

#[test]
fn spelling_metadata_gensyms_and_public_tagged_data_execute_in_both_phases() {
    check(
        r#"(let [s "\ud800\udc00\udfff" a `[x# x#] b `[x# x#]]
        (and (= [(.-length s) (.charCodeAt s 0) (.charCodeAt s 1) (.charCodeAt s 2)] [3 55296 56320 57343])
             (= (first a) (second a)) (not= (first a) (first b))
             (= (meta ^:outer ^{:inner 2} [1]) {:outer true :inner 2})
             (= (meta '^"Type" café) {:tag "Type"})
             (= (meta '^:quoted café) {:quoted true})
             (= (#(vector % #_ %3 %&) 1 2 3 4) [1 '(4)])
             (= '#::{:a 1 :_/b 2} {:cljs.user/a 1 :b 2})
             (tagged-literal? (tagged-literal 't [1]))
             (= (:tag (tagged-literal 't [1])) 't)
             (= (:form (tagged-literal 't [1])) [1])
             (not (tagged-literal? [1]))))"#,
        "true",
    );
    let mut host = MacroHost::new();
    let js = compile_with_macros(
        r#"(defmacro reader-check []
        (let [a `[x# x#] b `[x# x#] tagged (tagged-literal 't [1])]
          (and (= (first a) (second a)) (not= (first a) (first b))
               (tagged-literal? tagged) (= (:tag tagged) 't) (= (:form tagged) [1])
               (= (meta ^:phase [1]) {:phase true}))))
        (reader-check)"#,
        &mut host,
    )
    .unwrap();
    assert_eq!(run(&js), "true\n");
    let e = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
    assert_eq!(
        e.string(e.evaluate_js(&format!("String({js})")).unwrap())
            .unwrap(),
        "true"
    );
}

#[test]
fn parser_acceptance_does_not_hide_contextual_reader_errors_or_numeric_duplicates() {
    use light_cljs::compiler_data::Source;
    for (text, diagnostic, offending, related) in [
        ("::absent/x", "MissingAlias", "::absent/x", None),
        ("#::absent{:x 1}", "MissingAlias", "absent", None),
        ("#unknown/tag 1", "UnknownTag", "#unknown/tag 1", None),
        ("^:x 1", "InvalidMetadataTarget", "1", Some("^:x 1")),
        ("#(%3/2)", "InvalidAnonymousArgument", "%3/2", None),
        ("{1 :a 1N :b}", "DuplicateMapKey", "1N", Some("1")),
        ("{1 :a 2/2 :b}", "DuplicateMapKey", "2/2", Some("1")),
        ("{1.0 :a 1e0 :b}", "DuplicateMapKey", "1e0", Some("1.0")),
        ("{1M :a 1.00M :b}", "DuplicateMapKey", "1.00M", Some("1M")),
    ] {
        assert!(light_cljs::parse(text).is_ok(), "syntax only: {text}");
        let source = Source::new("reader.cljs", text);
        let error = MacroHost::new().read_compiler_form(&source).unwrap_err();
        assert!(error.message.contains(diagnostic), "{text}: {error}");
        assert_eq!(error.span.text(), offending, "{text}");
        assert_eq!(error.related.as_ref().map(|s| s.text()), related, "{text}");
    }
    // Wide distinct integers remain distinct in reader data, even when their
    // Number representations collide in the application runtime.
    for text in [
        "{9007199254740992 :a 9007199254740993 :b}",
        "#{9007199254740992 9007199254740993}",
        "{1 :a 1.0 :b}",
        "{0 :a -0.0 :b}",
        "{1 :a 1M :b}",
        "{[1] :a [1.0] :b}",
    ] {
        let source = Source::new("reader.cljs", text);
        assert!(MacroHost::new().read_compiler_form(&source).is_ok());
    }
    for (text, duplicate) in [
        ("#{1 1N}", "1N"),
        ("#{1 2/2}", "2/2"),
        ("#{1M 1.00M}", "1.00M"),
    ] {
        assert!(light_cljs::parse(text).is_ok());
        let light_cljs::CompileError::Analysis(error) = compile(text).unwrap_err() else {
            panic!()
        };
        assert_eq!(
            &text[error.span], duplicate,
            "duplicate readable data: {text}"
        );
    }
    for text in ["#{1 1.0}", "#{0 -0.0}", "#{1 1M}"] {
        assert!(compile(text).is_ok(), "distinct reader categories: {text}");
    }
}

#[test]
fn cljc_selection_metadata_and_inactive_forms_have_distinct_phase_evidence() {
    use light_cljs::compiler_data::Name;
    use light_cljs::{EvalOptions, EvalSession, source_provider::SourceInput};
    let mut session = EvalSession::new(EvalOptions {
        data_readers: vec![(
            Name::new(None, "counter"),
            "(let [n (atom 0)] (fn [x] [x (swap! n inc)]))".into(),
        )],
        ..Default::default()
    })
    .unwrap();
    let input = SourceInput::new(
        "selected.cljc",
        r#"(let [f #(vector % #?(:clj %3 :cljs %2))]
        [(f 1 2) #?(:default 3 :cljs 4) #?(:other 0 :cljs 7)
         ^:flag [#?@(:cljs [1 2])]
         #?(:clj #unknown/tag 1 :cljs 8)
         #?(:clj {1 :a 1N :b} :cljs 9)
         #?(:clj #counter 10 :cljs #counter 11)])"#,
    );
    assert!(light_cljs::parse(&input.source).is_ok());
    let value = session.eval_source(&input).unwrap().value;
    assert_eq!(
        session.print(value).unwrap(),
        "[[1 2] 3 7 [1 2] 8 9 [11 1]]"
    );
    session.release(value).unwrap();
    for (text, offending) in [
        ("#?(:clj 1 :cljs 2)", "#?(:clj 1 :cljs 2)"),
        ("[1 #?@(:cljs 2)]", "#?@(:cljs 2)"),
        ("{:a #?(:clj 1)}", "{:a #?(:clj 1)}"),
    ] {
        let filename = if text.starts_with("#?") {
            "rejected.cljs"
        } else {
            "rejected.cljc"
        };
        assert!(light_cljs::parse(text).is_ok());
        let error = session
            .eval_source(&SourceInput::new(filename, text))
            .unwrap_err();
        assert_eq!(error.location.unwrap().text(), offending);
    }
    for text in [
        r#"#?(:cljs 1 :clj "\q")"#,
        "#?(:cljs 1 :clj {:a})",
        "#?(:cljs 1 :clj #=(+ 1 2))",
    ] {
        let error = light_cljs::parse(text).unwrap_err();
        assert!(text.get(error.span).is_some());
        assert!(
            session
                .eval_source(&SourceInput::new("malformed.cljc", text))
                .is_err()
        );
    }
    let value = session.eval_string("42").unwrap().value;
    assert_eq!(session.print(value).unwrap(), "42");
    session.release(value).unwrap();
    let input = SourceInput::new("owned-diagnostic.cljc", "\n[λ #?@(:cljs 2)]");
    let error = session.read_forms(&input).unwrap_err();
    drop(session);
    assert_eq!(error.location.as_ref().unwrap().text(), "#?@(:cljs 2)");
    assert!(error.to_string().starts_with("owned-diagnostic.cljc:2:5:"));
}
#[test]
fn reader_values() {
    for (source, expected) in [
        (
            r#"(= [\a \tab 9007199254740993 2/2 1N 1M] ["a" "\t" 9007199254740992 1 1 1])"#,
            "true",
        ),
        (r#"(js/Number.isNaN ##NaN)"#, "true"),
        (r#"(= (re-find #"(?i)a(b)" "AB") ["AB" "B"])"#, "true"),
        (r#"(= (re-matches #"a+" "aa") "aa")"#, "true"),
        (
            r#"(= #inst "2010-11-12T13:14:15.666-05:00" #inst "2010-11-12T18:14:15.666Z")"#,
            "true",
        ),
        (r#"(identical? #inst "2000" #inst "2000")"#, "false"),
        (
            r#"(= #uuid "550E8400-E29B-41D4-A716-446655440000" #uuid "550e8400-e29b-41d4-a716-446655440000")"#,
            "true",
        ),
        (
            r#"(identical? #uuid "1-1-1-1-1" #uuid "1-1-1-1-1")"#,
            "false",
        ),
        (
            r#"(pr-str #inst "1582-10-04T23:59:59.999")"#,
            "#inst \"1582-10-04T23:59:59.999-00:00\"",
        ),
        (r#"(pr-str #queue [1 2 3])"#, "#queue [1 2 3]"),
        (
            r#"(= '#queue [x] '(cljs.core/into cljs.core.PersistentQueue.EMPTY [x]))"#,
            "true",
        ),
        (r#"(= (.-x #js {:x 3}) 3)"#, "true"),
        (r#"(= (name (.-x '#js {:x foo})) "foo")"#, "true"),
        (r#"(= ((fn* [] #js [1 2])) #js [1 2])"#, "false"),
        (r#"(#(+ % %3) 1 2 3)"#, "4"),
        (r#"(= (#(vector % %&) 1 2 3) [1 '(2 3)])"#, "true"),
        (r#"(= (#(vector %&) ) [nil])"#, "true"),
        (r#"(= `foo 'cljs.user/foo)"#, "true"),
        (r#"(= `+ 'cljs.core/+)"#, "true"),
        (
            r#"(let* [x 3] (= `[a ~x ~@[4 5]] '[cljs.user/a 3 4 5]))"#,
            "true",
        ),
        (r#"(= (meta ^:x [1]) {:x true})"#, "true"),
        (r#"(= (meta '^String foo) {:tag 'String})"#, "true"),
        (r#"(let* [^String x 1] x)"#, "1"),
        (r#"(def x 2) (def v #'x) (def x 3) @v"#, "3"),
        (r#"(def x (fn* [] 4)) (#'x)"#, "4"),
        (
            r#"(= #:x{:a 1 :_/b 2 :other/c 3} {:x/a 1 :b 2 :other/c 3})"#,
            "true",
        ),
        (r#"(= '#:x{a 1 _/b 2} '{x/a 1 b 2})"#, "true"),
    ] {
        check(source, expected);
    }
}
#[test]
fn macro_values_keep_semantic_types() {
    let mut h = MacroHost::new();
    let source = r#"(defmacro same [x] x) (defmacro value [] [#inst "2000" #uuid "1-1-1-1-1" #"a"])
        (= [(inst? (nth (value) 0)) (uuid? (nth (value) 1)) (regexp? (nth (value) 2)) (same (#(+ % %2) 1 2))] [true true true 3])"#;
    assert_eq!(run(&compile_with_macros(source, &mut h).unwrap()), "true\n");
}
#[test]
fn invalid_reader_syntax_stays_located() {
    for source in [
        r#"#inst "2023-02-29""#,
        r#"#uuid "bad""#,
        "#js {:a/x 1}",
        "#js 1",
        "#queue 1",
        "#unknown 1",
        "#(#(%))",
        "#(%1.5)",
        "`~@x",
        "#=(js/console.log 1)",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
}

#[test]
fn configured_readers_run_once_and_keep_effect_order_including_discards() {
    use light_cljs::compiler_data::Name;
    let mut host = MacroHost::new();
    // Reader functions own counters in the fresh canonical JS compilation heap.
    // Discarded and untaken forms are still read in source order; host reuse
    // preserves reader configuration and creates fresh mutable reader roots.
    let reader = "(let [seen (atom 0)] (fn [x] [x (swap! seen inc)]))";
    host.register_compiler_data_reader(Name::new(Some("test"), "tag"), reader)
        .unwrap();
    let source = "#_ #test/tag 1 (assert (= [2 2] (let [x #test/tag 2] (if false #test/tag 3 x)))) #_ #test/tag 4 (= [5 5] #test/tag 5)";
    assert_eq!(
        run(&compile_with_macros(source, &mut host).unwrap()),
        "true\n"
    );
    assert_eq!(
        run(&compile_with_macros("(= [5 1] #test/tag 5)", &mut host).unwrap()),
        "true\n"
    );
    host.register_compiler_data_reader(Name::new(None, "inst"), "(fn [x] x)")
        .unwrap();
    assert_eq!(
        run(&compile_with_macros("#inst 5", &mut host).unwrap()),
        "5\n"
    );
    assert_eq!(
        run(&compile_with_macros("(= ::x :cljs.user/x)", &mut host).unwrap()),
        "true\n"
    );
    for source in ["@missing", "#'missing", "#js {:x missing}"] {
        let light_cljs::CompileError::Analysis(error) = compile(source).unwrap_err() else {
            panic!()
        };
        assert_eq!(&source[error.span], "missing");
    }
}

#[test]
fn host_types_and_lazy_regex_tails_have_direct_runtime_observations() {
    check(
        r#"(= (first ^:x [10000000000000000000000000000000000000000/2]) 5000000000000000000000000000000000000000)"#,
        "true",
    );
    check(
        r#"(count ^:x {1000000000000000000000 1 1000000000000000000001 2})"#,
        "1",
    );
    check(
        r#"(count ^:x #{1000000000000000000000 1000000000000000000001})"#,
        "1",
    );
    check(r#"(= (with-meta nil {:x true}) nil)"#, "true");
    check(r#"(def x 1) (let* [x 2] @#'x)"#, "1");
    check(r#"(def ^:flag x 1) (:flag (meta #'x))"#, "true");
    check(r#"(:flag (meta ^:flag (fn* [] 1)))"#, "true");
    check(r#"(@#'count [1 2])"#, "2");
    check(r#"(#(+ %1/1 %2) 2 3)"#, "5");
    check(r#"(def map 1) (= `map 'cljs.user/map)"#, "true");
    check(r#"(js/Array.isArray #js [1 2])"#, "true");
    check(r#"(.toISOString #inst "0001")"#, "0001-01-01T00:00:00.000Z");
    check(
        r#"(= (re-seq #"f(.)o" "foo foo") '(["foo" "o"] ["foo" "o"]))"#,
        "true",
    );
    check(r#"(= (re-seq #"" "a") '("" ""))"#, "true");
    check(
        r#"(= (meta (with-meta (list 1 2) {:x true})) {:x true})"#,
        "true",
    );
    check(r#"(= (first (with-meta () {})) nil)"#, "true");
}

#[test]
fn compiler_forms_and_ir_survive_host_teardown_without_native_roots() {
    use light_cljs::compiler_data::{DatumView, FormKind, Source};
    let input = String::from(r#"^:kept {:z [#js [1] "\ud800"] :a #uuid "1-1-1-1-1"}"#);
    let source = Source::new("owned.cljs", input.clone());
    let mut host = MacroHost::new();
    let form = host.read_compiler_form(&source).unwrap().unwrap();
    assert_eq!(form.span().source().id(), source.id());
    // Owned compiler forms outlive the entire host without native roots.
    let program_source = format!("(count {input})");
    let program = light_cljs::analyze_with_macros(
        &program_source,
        &light_cljs::parse(&program_source).unwrap(),
        &mut host,
    )
    .unwrap();
    let error_source = Source::new("bad.cljs", "\n#uuid \"bad\"");
    let error = host.read_compiler_form(&error_source).unwrap_err();
    drop(host);
    drop(source);
    drop(error_source);
    drop(input);
    drop(program_source);
    assert!(matches!(
        form.node().kind,
        FormKind::Collection {
            kind: light_cljs::CollectionKind::Map,
            ..
        }
    ));
    let children = form.children();
    assert_eq!(
        children
            .iter()
            .map(|c| c.span().text().to_owned())
            .collect::<Vec<_>>(),
        [":z", r#"[#js [1] "\ud800"]"#, ":a", r#"#uuid "1-1-1-1-1""#]
    );
    let DatumView::String(text) = children[1].children()[1].data().inspect() else {
        panic!()
    };
    assert_eq!(&*text, &[0xd800]);
    assert!(form.metadata().is_some());
    assert!(error.to_string().starts_with("bad.cljs:2:"));
    let js = light_cljs::emit_js(&program);
    assert_eq!(run(&js), "2\n");
}

#[test]
fn compiler_forms_can_be_constructed_without_any_evaluator() {
    use light_cljs::compiler_data::{Datum, DatumKind, DatumView, Form, Name, Origin, Source};
    let source = Source::new("supplied", "");
    let meta = Datum::new(DatumKind::Collection {
        kind: light_cljs::CollectionKind::Map,
        children: vec![
            Datum::new(DatumKind::Keyword(Name::new(None, "flag"))),
            Datum::new(DatumKind::Boolean(true)),
        ]
        .into(),
    });
    let data = Datum::new(DatumKind::Collection {
        kind: light_cljs::CollectionKind::Vector,
        children: vec![
            Datum::new(DatumKind::Symbol(Name::from_utf16(
                Some(&[0xdc00]),
                &[0xd800],
            ))),
            Datum::new(DatumKind::Double(-0.0)),
        ]
        .into(),
    })
    .with_metadata(meta);
    let form = Form::from_data(data, source.span(0..0).unwrap(), Origin::SuppliedData);
    drop(source);
    assert_eq!(form.node().origin, Origin::SuppliedData);
    assert!(form.metadata().is_some());
    let DatumView::Symbol(name) = form.children()[0].data().inspect() else {
        panic!()
    };
    assert_eq!(name.namespace(), Some([0xdc00].as_slice()));
    assert_eq!(name.name(), &[0xd800]);
    let DatumView::Double(number) = form.children()[1].data().inspect() else {
        panic!()
    };
    assert_eq!(number.to_bits(), (-0.0_f64).to_bits());
}


#[test]
fn runtime_reader_uses_dynamic_namespace_and_restores_nested_reads() {
    check(r##"(pr-str (let [events (atom [])]
      [(read-string "::x")
       (binding [*ns* (create-ns 'other)]
         (swap! events conj (read-string "::x"))
         (binding [*ns* (create-ns 'inner)] (swap! events conj (read-string "::x")))
         (swap! events conj (read-string "::x"))
         (try (binding [*ns* (create-ns 'throwing)] (read-string "::missing/x"))
              (catch :default _ (swap! events conj (read-string "::x"))))
         (read-string "`local"))
       (read-string "::x") @events (second (read-string "`nil?"))]))"##,
      "[:cljs.user/x (quote other/local) :cljs.user/x [:other/x :inner/x :other/x :other/x] cljs.core/nil?]");
}

#[test]
fn runtime_reader_registers_aliases_refers_and_source_namespace_entries() {
    let root = std::env::temp_dir().join(format!("light-cljs-reader-context-{}", std::process::id()));
    std::fs::create_dir_all(root.join("reader")).unwrap();
    std::fs::write(root.join("reader/dep.cljs"), r##"(ns reader.dep) (def value 9) (def observed (read-string "::value"))"##).unwrap();
    std::fs::write(root.join("reader/app.cljs"), r##"
      (ns reader.app (:require [reader.dep :as d :refer [value]]) (:refer-clojure :exclude [map]))
      (def local 1)
      (defmacro inc [] 1)
      (defmacro reader-probe []
        (and (= (ns-name *ns*) 'reader.app)
             (= (read-string "::d/x") :reader.dep/x)
             (= (second (read-string "`value")) 'reader.dep/value)
             (= (second (read-string "`local")) 'reader.app/local)
             (= (second (read-string "`map")) 'reader.app/map)
             (= (second (read-string "`inc")) 'reader.app/inc)))
      (def source-agrees (= [::local ::d/value '#::d{:x 1 :_/y 2} `d/value `local `value `map]
         [(read-string "::local") (read-string "::d/value") (read-string "#::d{:x 1 :_/y 2}")
          (second (read-string "`d/value")) (second (read-string "`local"))
          (second (read-string "`value")) (second (read-string "`map"))]))
      (and (reader-probe) source-agrees (= reader.dep/observed :reader.dep/value)
           (= (ns-name *ns*) 'reader.app)
           (= (read-string "#::{:x 1}") {:reader.app/x 1})
           (= (read-string "::d/value") :reader.dep/value)
           (= (second (read-string "`value")) 'reader.dep/value)
           (= (second (read-string "`local")) 'reader.app/local)
           (= (second (read-string "`map")) 'reader.app/map)
           (= (second (read-string "`inc")) `inc))"##).unwrap();
    let js = light_cljs::compile_namespace("reader.app", &[root.clone()]).unwrap();
    let e = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
    assert_eq!(e.string(e.evaluate_js(&format!("String({js})")).unwrap()).unwrap(), "true");
    assert_eq!(run(&js), "true\n");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn runtime_reader_macro_and_reused_session_contexts_are_explicit() {
    use light_cljs::{EvalSession, EvalOptions};
    let mut session = EvalSession::new(EvalOptions {namespace: "reader.session".into(),
      aliases: vec![("d".into(), "reader.dep".into())], ..Default::default()}).unwrap();
    for text in [r##"[(read-string "::x") (read-string "::d/x") (second (read-string "`d/x"))]"##,
                 r##"(defmacro read-context [] (list 'quote [(read-string "::x") (read-string "::d/x") (second (read-string "`d/x"))])) (read-context)"##] {
      let value = session.eval_string(text).unwrap().value;
      assert_eq!(session.print(value).unwrap(), "[:reader.session/x :reader.dep/x reader.dep/x]");
      session.release(value).unwrap();
    }
    session.eval_string("(in-ns 'reader.other)").unwrap();
    let value = session.eval_string(r##"[(read-string "::x") (binding [*ns* (the-ns 'reader.session)] (read-string "::d/x"))]"##).unwrap().value;
    assert_eq!(session.print(value).unwrap(), "[:reader.other/x :reader.dep/x]");
    session.release(value).unwrap();
    session.eval_string("(in-ns 'reader.session)").unwrap();
    let value = session.eval_string(r##"(read-string "::d/x")"##).unwrap().value;
    assert_eq!(session.print(value).unwrap(), ":reader.dep/x");
    session.release(value).unwrap();
}

#[test]
fn runtime_reader_profiles_missing_alias_locations_and_nested_tag_reads() {
    let checks = r##"
      const c = rt.core;
      c.currentNs = c.registerReaderNamespace('reader.outer', {aliases: [['d','reader.dep']]});
      for (const text of ['::absent/x', '#::absent{:x 1}']) {
        assert.throws(() => c.readString(text), e => e.category === 'reader' &&
          e.source === '<read-string>' && e.line === 1 && e.column > 1 && /Missing namespace alias: absent/.test(e.message));
      }
      for (const read of [c.ednReadString,c.readerReadString]) {
        for (const text of ['::x','#::{:x 1}','`x']) assert.throws(() => read(text));
      }
      c.readerRegisterTagParser(rt.symbol(null,'nested'), value => {
        const saved = c.currentNs; c.currentNs = c.createNs(rt.symbol(null,'reader.inner'));
        try { return c.readString('::x'); } finally { c.currentNs = saved; }
      });
      assert.equal(c.prStr(c.readString('[::x #nested 0 ::x]')), '[:reader.outer/x :reader.inner/x :reader.outer/x]');
      c.readerRegisterTagParser(rt.symbol(null,'bad'), value => {
        const saved = c.currentNs; c.currentNs = c.createNs(rt.symbol(null,'reader.inner'));
        try { return c.readString('::absent/x'); } finally { c.currentNs = saved; }
      });
      assert.throws(() => c.readString('#bad 0'), /Missing namespace alias/);
      assert.equal(c.prStr(c.readString('::x')), ':reader.outer/x');
      assert.throws(() => c.readString('#=(1)'), /Unsupported dispatch syntax/);
      const env = c.hashMap(rt.keyword(null,'ns'), c.hashMap(rt.keyword(null,'name'), rt.symbol(null,'reader.caller'),
        rt.keyword(null,'requires'), c.hashMap(rt.symbol(null,'d'), rt.symbol(null,'reader.dep'))));
      const macro = c.macroReaderFunction(() => c.prStr(c.vector(c.readString('::x'),c.readString('::d/x'))));
      assert.equal(macro(null,env), '[:reader.caller/x :reader.dep/x]');
      const badMacro = c.macroReaderFunction(() => c.readString('::absent/x'));
      assert.throws(() => badMacro(null,env), /Missing namespace alias/);
      assert.equal(c.prStr(c.readString('::d/x')), ':reader.dep/x');
      assert.equal(c.prStr(c.readString('::x')), ':reader.outer/x');
    "##;
    let script = format!("const assert = require('node:assert/strict'); const rt = {}; {}", include_str!("../src/js_runtime.js"),checks);
    let output = Command::new("node").args(["-e", &script]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let script = format!("(() => {{ const rt = {}; const assert = {{equal: (a,b) => {{if (a !== b) throw Error(a+' != '+b);}}, throws: (f,test) => {{try {{f();}} catch(e) {{ if (test === undefined || (typeof test === 'function' ? test(e) : test.test(e.message))) return; throw e;}} throw Error('Expected failure');}} }}; {}; return true; }})()", include_str!("../src/js_runtime.js"), checks);
    let e = light_cljs::evaluator::Evaluator::new(Default::default()).unwrap();
    e.evaluate_js(&script).unwrap();
}

#[test]
fn runtime_reader_core_name_inventory_matches_the_source_reader_tables() {
    let runtime = include_str!("../src/js_runtime.js");
    let line = runtime.lines().find(|line| line.contains("const readerCoreNames = new Set(")).unwrap();
    let names = line.split('"').nth(1).unwrap().split(' ').collect::<std::collections::BTreeSet<_>>();
    let expected = [include_str!("../src/core_public_bindings.tsv"), include_str!("../src/core_var_metadata.tsv")].into_iter()
        .flat_map(str::lines).filter_map(|line| line.split_once('\t').map(|(name, _)| name)).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(names, expected, "Regenerate runtime reader core names when pinned source tables change");
}
