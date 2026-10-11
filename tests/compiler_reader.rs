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
    assert_eq!(
        run(&compile(source).unwrap_or_else(|e| panic!("{source}: {e}"))),
        format!("{expected}\n")
    );
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
