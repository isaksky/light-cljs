//! Demand-driven printing at `Advanced` (light-cljs-207;
//! docs/advanced-optimization.md, Phase 1 step 6).
//!
//! The printer (`printWriterImpl` and what it requires) installs collection
//! and `ExceptionInfo` `toString` through its `printWriterImpl.install` effect,
//! so a program keeps the printer only when it can print. Error paths format
//! values without it (`displayStr`, `printHooks.text`), and records and
//! eductions reach it through `printHooks` from their `-pr-writer` methods.
//! Where the printer is kept, every message is the same as at `none`.
use super::optimization::execute;
use light_cljs::runtime_manifest::{RuntimeManifest, UnitId};
use light_cljs::{
    ApplicationOptions, CoreFnId, OptimizationLevel, PreparedApplication, Program, analyze, parse,
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

/// Whether keeping `roots` keeps the printer.
fn reaches_printer(roots: &[UnitId]) -> bool {
    let manifest = manifest();
    manifest.reachable(roots.iter().copied())[manifest.unit_named("printWriterImpl").unwrap().0]
}

/// Whether the `Advanced` output of `source` embeds the printer, and with it
/// its install.
fn prints(source: &str) -> bool {
    let units = prepared(source, OptimizationLevel::Advanced)
        .retention()
        .units
        .iter()
        .map(|unit| unit.name)
        .collect::<Vec<_>>();
    let printer = units.contains(&"printWriterImpl");
    assert_eq!(
        printer,
        units.contains(&"printWriterImpl.install"),
        "{source}"
    );
    printer
}

/// What `source` logs at each level, in `OptimizationLevel::ALL` order.
fn logged(source: &str) -> [String; 3] {
    OptimizationLevel::ALL.map(|level| execute(&prepared(source, level).emit_js()))
}

/// The core operations whose runtime units reach the printer: the print
/// functions and `str`, `-pr-writer`, `clj->js` (map keys that are not
/// scalars print), `gensym` and `munge` (they `str` their argument), tagged
/// literals (`TaggedLiteral` prints through `pr-str`) and the reflection that
/// keeps the reader or the namespace registry. A runtime change that adds a
/// printer edge to another operation fails until the operation is listed
/// here, or the edge goes through `printHooks` or `displayStr` instead.
const PRINTING: &[&str] = &[
    "->Namespace",
    "->TaggedLiteral",
    "-pr-writer",
    "Namespace",
    "TaggedLiteral",
    "all-ns",
    "clj->js",
    "compiler-edn-read",
    "compiler-edn-read-string",
    "compiler-macro-var*",
    "compiler-reader-close",
    "compiler-reader-current-input",
    "compiler-reader-deregister-default-tag-parser",
    "compiler-reader-deregister-tag-parser",
    "compiler-reader-initial-tag-table",
    "compiler-reader-input-provider",
    "compiler-reader-parse-timestamp",
    "compiler-reader-parse-timestamp-fields",
    "compiler-reader-read",
    "compiler-reader-read-line",
    "compiler-reader-read-string",
    "compiler-reader-register-default-tag-parser",
    "compiler-reader-register-tag-parser",
    "compiler-reader-string-reader",
    "compiler-reader-with-input",
    "create-ns",
    "demunge",
    "find-ns",
    "gensym",
    "key->js",
    "munge",
    "pr",
    "pr-seq-writer",
    "pr-str",
    "pr-str*",
    "pr-str-with-opts",
    "print",
    "print-str",
    "println",
    "println-str",
    "prn",
    "prn-str",
    "prn-str-with-opts",
    "read",
    "read-line",
    "read-string",
    "str",
    "tagged-literal",
    "tagged-literal?",
    "the-ns",
];

#[test]
fn only_printing_operations_reach_the_printer() {
    let manifest = manifest();
    let mut printing = CoreFnId::ALL
        .iter()
        .map(|core| core.operation())
        .filter(|operation| reaches_printer(&operation.runtime_units(manifest)))
        .map(|operation| operation.name)
        .collect::<Vec<_>>();
    printing.sort_unstable();
    printing.dedup();
    assert_eq!(printing, PRINTING);

    // Value constructors, types, records and the error formatters keep none.
    for name in [
        "vectorFromArray",
        "listFromArray",
        "mapFromArray",
        "mapFromArrays",
        "mapShape",
        "mapFromShape",
        "hashMapShape",
        "hashMapFromShape",
        "setFromArray",
        "literalSetFromArray",
        "setShape",
        "setFromShape",
        "queueFromArray",
        "makeType",
        "recordFromMap",
        "makeReify",
        "protocol",
        "protocolMethod",
        "extendType",
        "displayStr",
        "printHooks",
    ] {
        assert!(
            !reaches_printer(&[manifest.unit_of(name).unwrap()]),
            "{name}"
        );
    }
}

#[test]
fn advanced_programs_keep_the_printer_only_when_they_print() {
    for source in [
        "[1 2 3]",
        "(js/console.log (count {:a [1 #{2}]}))",
        "(defrecord R [a]) (:a (->R 1))",
        "(defmulti m :t) (defmethod m :a [_] 1) (m {:t :a})",
        "(try (throw (ex-info \"x\" {:a 1})) (catch :default e (ex-message e)))",
        "(defn f [x] (case x 1 :one)) (f 1)",
        "(assert (= 1 1) \"never\")",
        "(into [] (eduction (map inc) [1 2]))",
    ] {
        assert!(!prints(source), "{source}");
    }
    for source in [
        "(str [1 2 3])",
        "(pr-str [1 2 3])",
        "(enable-console-print!) (println [1 2 3])",
        "(with-out-str (prn {:a 1}))",
        "(defrecord R [a]) (pr-str (->R 1))",
        "(clj->js {[1] 2})",
    ] {
        assert!(prints(source), "{source}");
    }
}

/// Without the printer, collection and `ExceptionInfo` `toString` and the
/// collection values in error messages fall back to the host's `toString`;
/// everything else an error message shows is the same as at `none`.
#[test]
fn collection_to_string_exists_only_where_printing_is_reachable() {
    let messages = r#"(defn msg [f] (try (f) (catch :default e (.replace (.-message e) "\n" "/"))))
        (defn sel [x] (case x 1 :one))
        (defmulti m identity)
        (defmethod m :a [_] 1)
        (def values [[1 "a"] nil "s" ##NaN -0.0 ##-Inf :k/w 'x/y true])
        (js/console.log (.toString [1 2]) (.toString {:a 1}) (.toString #{1})
          (.toString (list 1)) (.toString (range 2)) (.toString (transient []))
          (.toString (ex-info "x" {:a 1})))
        (doseq [v values]
          (js/console.log (msg #(hash-map v)) "|" (msg #(sel v)) "|" (msg #(m v))
            "|" (msg #(assert false v))))
        (prefer-method m :a :b)
        (derive ::x ::y)
        (js/console.log (msg #(prefer-method m :b :a)) "|" (msg #(derive ::y ::x)))"#;
    let [none, prepare, advanced] = logged(messages);
    assert_eq!(none, prepare);
    let lines = none.lines().collect::<Vec<_>>();
    assert_eq!(
        lines[0],
        r#"[1 2] {:a 1} #{1} (1) (0 1) #object[cljs.core.TransientVector] #error {:message "x", :data {:a 1}}"#
    );
    assert_eq!(
        lines[1],
        r#"No value supplied for key: [1 "a"] | No matching clause: [1 "a"] | No method in multimethod 'cljs.user/m' for dispatch value: [1 a] | Assert failed: [1 "a"]/false"#
    );
    assert_eq!(
        lines[2],
        "No value supplied for key:  | No matching clause:  | No method in multimethod 'cljs.user/m' for dispatch value: nil | Assert failed: /false"
    );
    assert_eq!(lines.len(), 11);
    assert!(!prints(messages));
    let mut expected = none
        .replace(r#"[1 "a"]"#, "[object Object]")
        .replace("[1 a]", "[object Object]");
    for printed in [
        "[1 2]",
        "{:a 1}",
        "#{1}",
        "(1)",
        "(0 1)",
        "#object[cljs.core.TransientVector]",
    ] {
        expected = expected.replacen(&format!("{printed} "), "[object Object] ", 1);
    }
    expected = expected.replace(r#"#error {:message "x", :data {:a 1}}"#, "Error: x");
    assert_eq!(advanced, expected);

    // The same program printing one value has the printer and its install, so
    // every level logs the same.
    let printing = format!("{messages} (pr-str 1)");
    assert!(prints(&printing));
    let [none, prepare, advanced] = logged(&printing);
    assert_eq!(prepare, none);
    assert_eq!(advanced, none);
}

/// Records and eductions print through `printHooks`, which only the printer
/// fills in: the same output at every level.
#[test]
fn records_and_eductions_print_through_the_installed_printer() {
    let source = r#"(defrecord R [a b])
        (js/console.log (pr-str (->R 1 [2 "x"]) (eduction (map inc) [1 2]))
          (str (->R 1 2)) (satisfies? IPrintWithWriter (->R 1 2))
          (binding [*print-length* 1] (pr-str (eduction (map inc) [1 2]))))"#;
    let [none, prepare, advanced] = logged(source);
    assert_eq!(
        none,
        "#cljs.user.R{:a 1, :b [2 \"x\"]} (2 3) [object Object] true (2 ...)\n"
    );
    assert_eq!(prepare, none);
    assert_eq!(advanced, none);
}
