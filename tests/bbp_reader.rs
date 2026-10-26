#![cfg(feature = "bbp-macros")]

use light_cljs::{MacroHost, analyze_with_macros, emit_js, parse};

fn expand(source: &str, native: bool) -> Result<String, String> {
    let mut host = MacroHost::new();
    if native {
        host.use_native_macros();
    }
    analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .map(|program| emit_js(&program))
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
            "default tag",
            r###"(compiler-reader-read-string {:default (fn [tag x] [tag x])} "#app/tag {:a 1}")"###,
        ),
        (
            "tag metadata",
            r###"(let [x (compiler-reader-read-string {:readers {'x (fn [x] (with-meta x {:from :reader}))}} "#x [1]")] [(meta x) x])"###,
        ),
        (
            "EDN",
            r###"[(compiler-edn-read-string "[1 :a]") (compiler-edn-read-string {:eof :end} "") (compiler-edn-read-string {:readers {'x identity}} "#x 4")]"###,
        ),
        (
            "streams",
            r###"(let [s (compiler-reader-string-reader "1\r\n2" "fixture.edn")] [(compiler-reader-read s) (compiler-reader-read s) (compiler-reader-read {:eof :done} s)])"###,
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
        let source = format!(
            "(defmacro probe [] (list 'quote {body})) (js/console.log (binding [*print-meta* true] (pr-str (probe))))"
        );
        match expand(&source, true) {
            Ok(native) => {
                assert!(!output(&native).is_empty(), "{label}");
                #[cfg(feature = "quickjs")]
                match expand(&source, false) {
                    Ok(legacy) if native == legacy => {}
                    Ok(legacy) => failures.push(format!(
                        "{label}: native {}, QuickJS {}",
                        output(&native),
                        output(&legacy)
                    )),
                    Err(error) => failures.push(format!("{label}: QuickJS {error}")),
                }
            }
            Err(error) => failures.push(format!("{label}: native {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn reader_facade_initializes_and_runs_in_a_source_macro() {
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-reader");
    let build = |native| {
        let mut host = MacroHost::new();
        if native {
            host.use_native_macros();
        }
        light_cljs::compile_namespace_with_macros(
            "reader-use",
            std::slice::from_ref(&root),
            &mut host,
        )
        .unwrap()
    };
    let native = build(true);
    assert_eq!(output(&native), "[42 [1 2]]\n");
    #[cfg(feature = "quickjs")]
    assert_eq!(native, build(false));
}

#[test]
fn reduced_cuerdas_ffmt_reads_placeholder_indices_natively() {
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-reader");
    let build = |native| {
        let mut host = MacroHost::new();
        if native {
            host.use_native_macros();
        }
        light_cljs::compile_namespace_with_macros(
            "format-use",
            std::slice::from_ref(&root),
            &mut host,
        )
        .unwrap()
    };
    let native = build(true);
    assert_eq!(output(&native), "url(A): B/A % B plain j/a\n");
    #[cfg(feature = "quickjs")]
    assert_eq!(native, build(false));
}
