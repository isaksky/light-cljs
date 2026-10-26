#![cfg(feature = "bbp-macros")]

use light_cljs::{MacroHost, analyze_with_macros, emit_js, parse};

fn expand(source: &str, native: bool) -> Result<String, String> {
    analyzed(source, native).map(|program| emit_js(&program))
}

fn analyzed(source: &str, native: bool) -> Result<light_cljs::Program, String> {
    let mut host = MacroHost::new();
    if native {
        host.use_native_macros();
    }
    analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .map_err(|error| error.to_string())
}

fn output(js: &str) -> String {
    let result = std::process::Command::new("node")
        .args(["-e", js])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

fn namespace_js(entry: &str, native: bool, jobs: usize) -> String {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-reader");
    let mut host = MacroHost::new();
    if native {
        host.use_native_macros();
    }
    let mut stats = light_cljs::CompileStats::default();
    let (program, _) = analyze_namespace_entry_recorded(
        entry,
        &[root],
        &mut host,
        Default::default(),
        &mut Vec::new(),
        EntryRecorders {
            stats: Some(&mut stats),
            slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
    if native {
        assert!(stats.native_helpers.waits > 0);
    }
    emit_js(&program)
}

#[test]
fn native_reader_forms_metadata_positions_and_errors_match_quickjs() {
    // A single differential table exercises the actual source macro path.
    // Error properties retain UTF-16 cursors; emitted JS compares exact string
    // units too, including lone surrogates that stdout would replace.
    let cases = [
        (
            "numbers",
            r###"(mapv compiler-reader-read-string ["0" "-0" "-12" "1.25" "1e3" "2/3" "0xff" "017" "2r101" "##Inf" "##-Inf" "##NaN"])"###,
        ),
        (
            "strings",
            r###"(mapv compiler-reader-read-string ["\"a\\t\\r\\n\\b\\f\\\"\\\\z\"" "\"\\141\\u263a\\ud800\\udfff\"" "\"😀\""])"###,
        ),
        (
            "characters",
            r###"(mapv compiler-reader-read-string ["\\a" "\\newline" "\\space" "\\uD800" "\\o141"])"###,
        ),
        (
            "names",
            r###"(mapv compiler-reader-read-string ["nil" "true" "false" ":key" ":ns/key" "sym" "ns/sym" "/" "😀"] )"###,
        ),
        (
            "collections",
            r###"(compiler-reader-read-string "[() (1 :a) [] {:a 1 :b [2]} #{1 2} #_ ignored :last] trailing")"###,
        ),
        (
            "metadata",
            r###"(let [x (compiler-reader-read-string "^:outer ^{:inner 2} [^String sym ^{:x [1]} (a)]")] [x (meta x) (meta (first x)) (meta (second x))])"###,
        ),
        (
            "collection thresholds",
            r###"(compiler-reader-read-string "[{:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9 nil 10} #{:a :b :c :d :e :f :g :h :i nil}]")"###,
        ),
        (
            "reader collection traversal and updates",
            r###"(let [m (compiler-reader-read-string "{:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9 nil 10}") s (compiler-reader-read-string "#{:a :b :c :d :e :f :g :h :i nil}")] [(vec m) (vec s) (assoc m :new 11) (disj s :a) (conj s :new)])"###,
        ),
        (
            "hash collections keep traversal after shrinking",
            r###"(let [m (assoc (dissoc (compiler-reader-read-string "{1 1 2 2 3 3 4 4 5 5 6 6 7 7 8 8 9 9}") 7 8 9) 0 0)
                      s (conj (disj (compiler-reader-read-string "#{1 2 3 4 5 6 7 8 9}") 7 8 9) 0)]
                  [(pr-str m) (vec m) (vec (keys m)) (pr-str s) (vec s)
                   (assoc (with-meta m {:updated true}) 10 10)
                   (assoc (apply dissoc m (keys m)) 0 0 1 1)] )"###,
        ),
        (
            "nil map merge arguments",
            r###"[(merge {:a 1} nil) (merge nil {:a 1} nil) (merge nil nil)
                  (merge {:a 1} nil {:b 2})]"###,
        ),
        (
            "tag result traversal",
            r###"[(compiler-reader-read-string {:readers {'x set}} "#x [:a :b :c :d :e :f :g :h :i nil]") (compiler-reader-read-string {:readers {'x (fn [x] (into {} x))}} "#x [[:a 1] [:b 2] [:c 3] [:d 4] [:e 5] [:f 6] [:g 7] [:h 8] [:i 9] [nil 10]]")]"###,
        ),
        (
            "namespaced maps",
            r###"(compiler-reader-read-string "#:foo{:a 1 :_/b 2 c 3 other/d 4}")"###,
        ),
        (
            "eof",
            r###"[(compiler-reader-read-string "") (compiler-reader-read-string " ;comment\r\n") (compiler-reader-read-string {:eof :done} "#_ 1") (compiler-reader-read-string nil) (compiler-reader-read-string false)]"###,
        ),
        (
            "custom tags",
            r###"(compiler-reader-read-string {:readers {'pair (fn [x] (with-meta [x x] {:tagged true}))}} "#pair [1 2]")"###,
        ),
        (
            "record callback results",
            r###"(let [supplied (with-meta [8] {:supplied :yes})
                      record (with-meta (->R supplied) {:record true})
                      result (compiler-reader-read-string {:readers {'r (fn [_] record)}} "#r 1")]
                  [(:a (compiler-reader-read-string {:readers {'r ->R}} "#r 1"))
                   (:a result) (meta result) (identical? record result)
                   (identical? (:a result) supplied)])"###,
        ),
        (
            "default tag",
            r###"(compiler-reader-read-string {:default (fn [tag x] [tag x])} "#app/tag {:a 1}")"###,
        ),
        (
            "tag metadata",
            r###"(let [x (compiler-reader-read-string {:readers {'x (fn [x] (with-meta x {:from :reader}))}} "#x [1]")] [(meta x) x])"###,
        ),
        (
            "callable tags",
            r###"[(compiler-reader-read-string {:readers {'x :a}} "#x {:a 7}") (compiler-reader-read-string {:readers {'x [10 20]}} "#x 1") (compiler-reader-read-string {:readers {'inst identity}} "#inst [1]")]"###,
        ),
        (
            "reader options errors",
            r###"(mapv (fn [opts] (try (compiler-reader-read-string opts "1") (catch :default e (.-message e)))) [nil [] {:other true} {"eof" nil} {:readers []} {:readers {:x identity}} {:readers {'x 1}} {:default 1}])"###,
        ),
        (
            "undefined default registration",
            r###"(let [old (compiler-reader-register-default-tag-parser js/undefined) previous (compiler-reader-deregister-default-tag-parser)] (js* "(~{} === undefined)" previous))"###,
        ),
        (
            "empty explicit options",
            r###"(try (compiler-reader-read-string {} "") (catch :default e [(.-message e) (.-source e) (.-line e) (.-column e) (.-offset e) (.-category e)]))"###,
        ),
        (
            "reader result validation",
            r###"[(try (compiler-reader-read-string {:readers {'x (fn [_] (js/Object.))}} "#x 1") (catch :default e (.-message e))) (try (compiler-reader-read-string {:readers {'x (fn [_] (fn [] nil))}} "#x 1") (catch :default e (.-message e))) (compiler-reader-read-string {:readers {'x (fn [_] '(1 2))}} "#x 1")]"###,
        ),
        (
            "depth limit",
            r###"(try (compiler-reader-read-string (str (.repeat "[" 33) "0" (.repeat "]" 33))) (catch :default e [(.-message e) (.-offset e) (.-category e)]))"###,
        ),
        (
            "input limit",
            r###"(try (compiler-reader-read-string (.repeat " " 1048577)) (catch :default e [(.-message e) (.-offset e) (.-category e)]))"###,
        ),
        (
            "EDN",
            r###"[(compiler-edn-read-string "[1 :a]") (compiler-edn-read-string {:eof :end} "") (compiler-edn-read-string {:readers {'x identity}} "#x 4")]"###,
        ),
        (
            "streams",
            r###"(let [s (compiler-reader-string-reader "1\r\n2" "fixture.edn")] [(compiler-reader-read s) (compiler-reader-read s) (compiler-reader-read {:eof :done} s)])"###,
        ),
        (
            "four argument EOF flags retain occurrences",
            r###"(mapv (fn [flag]
                    (mapv (fn [read]
                      (try (let [result (read (compiler-reader-string-reader "") flag supplied {})]
                             [(if (identical? result supplied) :same :different) (meta result)])
                           (catch :default e [(.-message e) (.-category e)])))
                      [compiler-reader-read compiler-edn-read]))
                    [false-flag nil-flag true-flag])"###,
        ),
        (
            "read-line close and read-after-close",
            r###"(let [chunks (atom ["a\r" "\nb\n" "c\r" "d" nil])
                      s (compiler-reader-input-provider "lines.edn"
                          (fn [] (let [c (first @chunks)] (swap! chunks rest) c)))
                      lines (mapv (fn [_] (compiler-reader-read-line s)) (range 6))
                      closed (compiler-reader-close s)]
                  [lines closed (try (compiler-reader-close s) (catch :default e (.-message e)))
                   (try (compiler-reader-read s) (catch :default e (.-message e)))
                   (try (compiler-reader-read-line s) (catch :default e (.-message e)))])"###,
        ),
        (
            "timestamp services",
            r###"(mapv (fn [text]
                    [(vec (compiler-reader-parse-timestamp-fields text))
                     (.getTime (compiler-reader-parse-timestamp text))])
                    ["2020" "2020-02-29T12:34:56.123456+02:30" "1970-01-01T00:00:00Z"
                     "1969-12-31T23:59:59.9-00:30"])"###,
        ),
        (
            "tag registration results and effects",
            r###"(let [a (fn [x] [:a x]) b (fn [x] [:b x])
                      old (compiler-reader-register-tag-parser 'app/tag a)
                      first-read (compiler-reader-read-string "#app/tag 1")
                      previous (compiler-reader-register-tag-parser 'app/tag b)
                      second-read (compiler-reader-read-string "#app/tag 2")
                      removed (compiler-reader-deregister-tag-parser 'app/tag)
                      missing (try (compiler-reader-read-string "#app/tag 3") (catch :default e (.-message e)))
                      default-a (fn [tag x] [tag x]) default-b (fn [tag x] [:default x])
                      old-default (compiler-reader-register-default-tag-parser default-a)
                      default-read (compiler-reader-read-string "#unknown 4")
                      previous-default (compiler-reader-register-default-tag-parser default-b)
                      replaced-default (compiler-reader-read-string "#unknown 5")
                      removed-default (compiler-reader-deregister-default-tag-parser)]
                  [(nil? old) first-read (identical? previous a) second-read
                   (identical? removed b) (compiler-reader-deregister-tag-parser 'app/tag) missing
                   (nil? old-default) default-read (identical? previous-default default-a)
                   replaced-default (identical? removed-default default-b)
                   (compiler-reader-deregister-default-tag-parser)
                   (try (compiler-reader-read-string "#unknown 6") (catch :default e (.-message e)))])"###,
        ),
        (
            "input restoration",
            r###"(let [before (compiler-reader-current-input) during (compiler-reader-with-input "1 2" (fn [] [(compiler-reader-read (compiler-reader-current-input)) (compiler-reader-read (compiler-reader-current-input))])) after (compiler-reader-current-input)] [(js* "(~{} === null)" before) during (js* "(~{} === null)" after)])"###,
        ),
    ];
    let errors = [
        ("EOF collection", "[1"),
        ("EOF string", "\"x"),
        ("unicode", "\"\\u12xz\""),
        ("octal", "\"\\400\""),
        ("number", "1e"),
        ("overflow", "1e999"),
        ("ratio", "1/0"),
        ("octal number", "09"),
        ("radix", "2r2"),
        ("numeric extension", "1N"),
        ("name", ":a/b/c"),
        ("metadata target", "^:a 1"),
        ("metadata value", "^1 []"),
        ("odd map", "{:a 1 :b}"),
        ("duplicate map", "{1 1 1.0 2}"),
        ("duplicate set", "#{1 1}"),
        ("conditional", "#?(:cljs 1 :clj 2)"),
        ("splicing conditional", "[#?@(:cljs [1])]"),
        ("unknown tag", "#unknown [1]"),
        ("auto keyword", "::a"),
        ("auto map", "#::{:a 1}"),
        ("quote", "'x"),
        ("position", " ;😀\r\n [1\r\n \"\\q\"]"),
    ];
    let error_cases: Vec<_> = errors.iter().map(|(label, text)| {
        // Debug escaping is compatible for these ASCII/control-character inputs.
        (*label, format!("(try (compiler-reader-read-string {text:?}) (catch :default e [(.-message e) (.-source e) (.-line e) (.-column e) (.-offset e) (.-category e)]))"))
    }).collect();
    let mut failures = Vec::new();
    for (label, body) in cases.into_iter().chain(
        error_cases
            .iter()
            .map(|(label, body)| (*label, body.as_str())),
    ) {
        let (prelude, params, args) = match label {
            "record callback results" => ("(defrecord R [a])", "", ""),
            "four argument EOF flags retain occurrences" => (
                "",
                "supplied false-flag nil-flag true-flag",
                "^:supplied [1] false nil true",
            ),
            _ => ("", "", ""),
        };
        let source = format!(
            "{prelude} (defmacro probe [{params}] (list 'quote {body})) (js/console.log (binding [*print-meta* true] (pr-str (probe {args}))))"
        );
        match analyzed(&source, true) {
            Ok(native_program) => {
                let native = emit_js(&native_program);
                assert!(!output(&native).is_empty(), "{label}");
                if label == "undefined default registration" {
                    assert_eq!(output(&native), "true\n", "{label}");
                }
                #[cfg(feature = "quickjs")]
                match analyzed(&source, false) {
                    Ok(legacy_program) => {
                        let legacy = emit_js(&legacy_program);
                        let positions = |program: &light_cljs::Program| {
                            program
                                .exprs
                                .iter()
                                .map(|expr| (expr.span.clone(), expr.source_index))
                                .collect::<Vec<_>>()
                        };
                        assert_eq!(
                            positions(&native_program),
                            positions(&legacy_program),
                            "{label}: compiler source positions"
                        );
                        if native != legacy {
                            failures.push(format!(
                                "{label}: native {}, QuickJS {}",
                                output(&native),
                                output(&legacy)
                            ));
                        }
                    }
                    Err(error) => failures.push(format!("{label}: QuickJS {error}")),
                }
            }
            Err(error) => failures.push(format!("{label}: native {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn native_reader_keeps_unrepresented_literals_explicitly_unsupported() {
    for (tag, text) in [
        ("inst", "#inst \"2020\""),
        ("UUID", "#uuid \"550e8400-e29b-41d4-a716-446655440000\""),
        ("queue", "#queue []"),
        ("JS", "#js []"),
    ] {
        let source = format!("(defmacro probe [] (compiler-reader-read-string {text:?})) (probe)");
        let error = expand(&source, true).unwrap_err();
        assert!(
            error.contains(&format!(
                "unsupported operation: native macro runtime reader {tag} literal"
            )),
            "{error}"
        );
    }
}

#[test]
fn native_reader_unwraps_input_and_retains_eof_and_callback_occurrences() {
    let source = r##"
      (defmacro probe [text sentinel supplied]
        (let [read compiler-reader-read-string
              a (read {:eof sentinel} "")
              b (read {:readers {'x (fn [_] supplied)}} "#x 0")]
          (list 'quote [(read text) (identical? a sentinel) (meta a)
                        (identical? b supplied) (meta b) b])))
      (js/console.log (pr-str (probe "[1 2]" ^:sentinel [3] ^:supplied [4])))"##;
    let native = expand(source, true).unwrap();
    assert_eq!(
        output(&native),
        "[[1 2] true {:sentinel true} true {:supplied true} [4]]\n"
    );
    #[cfg(feature = "quickjs")]
    assert_eq!(native, expand(source, false).unwrap());
}

#[test]
fn native_reader_options_and_streams_accept_supplied_scalar_occurrences() {
    let source = r###"
      (defmacro probe [opts missing-opts text chunk]
        (let [read compiler-reader-read-string
              s (compiler-reader-input-provider "chunks.edn" (let [chunks (atom [chunk nil])]
                  (fn [] (let [c (first @chunks)] (swap! chunks rest) c))))]
          (list 'quote [(read opts text)
                        (try (read missing-opts "#x 1") (catch :default e (.-message e)))
                        (compiler-reader-read s)])))
      (js/console.log (pr-str (probe {:default nil :readers nil} {:default nil :readers {x nil}} "[1 2]" "3")))"###;
    let native = expand(source, true).unwrap();
    assert_eq!(
        output(&native),
        "[[1 2] \"No reader function for tag x at <read-string>:1:5\" 3]\n"
    );
    #[cfg(feature = "quickjs")]
    assert_eq!(native, expand(source, false).unwrap());
}

#[test]
fn reader_facade_initializes_and_runs_in_a_source_macro() {
    for jobs in [1, 2, 4] {
        let native = namespace_js("reader-use", true, jobs);
        assert_eq!(
            output(&native),
            "[42 [1 2]]\n[[7] {:tagged true} [\"before\" \"after\"]]\n"
        );
        #[cfg(feature = "quickjs")]
        assert_eq!(native, namespace_js("reader-use", false, jobs));
    }
}

#[test]
fn native_reader_facade_accepts_missing_readers_options() {
    let native = namespace_js("options-use", true, 1);
    assert_eq!(output(&native), "[1 2 :done 3 4 :done]\n");
    #[cfg(feature = "quickjs")]
    assert_eq!(native, namespace_js("options-use", false, 1));
}

#[test]
fn reduced_cuerdas_ffmt_reads_placeholder_indices_natively() {
    let native = namespace_js("format-use", true, 1);
    assert_eq!(output(&native), "url(A): B/A % B plain j/a\n");
    #[cfg(feature = "quickjs")]
    assert_eq!(native, namespace_js("format-use", false, 1));
}

#[test]
fn native_promise_object_extension_remains_explicit() {
    let source = r##"
        (defmacro probe []
          (extend-type js/Promise Object (value [_] 7))
          (.value (js/Promise.resolve 0)))
        (js/console.log (probe))"##;
    let error = expand(source, true).unwrap_err();
    assert!(
        error.contains("unsupported operation: native Promise Object extension"),
        "{error}"
    );
    #[cfg(feature = "quickjs")]
    assert_eq!(output(&expand(source, false).unwrap()), "7\n");
}

#[test]
fn native_promise_printer_extension_matches_quickjs() {
    // The promesa printer that app.main demands while initializing unrelated
    // Transit declarations. Installing it must also change actual dispatch.
    let source = r##"
        (defprotocol P (show [x]))
        (extend-protocol P default (show [x] :fallback))
        (extend-type js/Promise
          P (show [x] :promise)
          IPrintWithWriter (-pr-writer [p writer opts] (-write writer "#<js/Promise[~]>")))
        (defmacro probe []
          (let [p (js/Promise.resolve 42)]
            (list 'quote [(pr-str p) (show p) (show (js/Object.)) (satisfies? P p) (implements? P p)])))
        (js/console.log (pr-str (probe)))"##;
    let native = expand(source, true).unwrap();
    assert_eq!(
        output(&native),
        "[\"#<js/Promise[~]>\" :promise :fallback true true]\n"
    );
    #[cfg(feature = "quickjs")]
    assert_eq!(native, expand(source, false).unwrap());
}
