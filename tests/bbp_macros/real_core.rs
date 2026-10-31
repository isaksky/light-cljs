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
