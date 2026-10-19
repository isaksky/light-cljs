//! Generated names are allocated per unit (docs/compiler-concurrency.md,
//! "Generated names"; src/generated_names.rs): the same program compiles to
//! the same bytes whatever the process compiled before and on whichever
//! thread it compiles, while the names stay hygienic.
use crate::temp_dir::TempDir;
use light_cljs::{
    ApplicationOptions, MacroHost, OptimizationLevel, analyzer::AnalysisOptions,
    compiler::analyze_namespace_entry, prepare_application, source_map::SourceMapping,
};
use std::{fs, path::PathBuf, process::Command};

/// Every allocator: syntax-quote gensyms read in a macro namespace and in
/// the entry, the compile-time runtime's `gensym` in a macro, a core macro
/// (`or`) and as data, `#()` parameters, `binding` and destructuring
/// locals, in top-level forms and in deferred units.
const PROGRAM: [(&str, &str); 2] = [
    (
        "lib/macros.cljs",
        r#"(ns lib.macros)
           (defmacro swap-let [a b] `(let [t# ~a u# ~b] [u# t#]))
           (defmacro with-gensym [x] (let [g (gensym "g")] `(let [~g ~x] (+ ~g 1))))
           (defmacro quoted-gensym [] (list 'quote (gensym "data")))"#,
    ),
    (
        "app.cljs",
        r#"(ns app (:require [lib.macros :refer [swap-let with-gensym quoted-gensym]]))
           (def ^:dynamic *scale* 1)
           (defmacro twice [x] `(let [v# ~x] (+ v# v#)))
           (defn f [x] (swap-let x (twice (inc x))))
           (defn g [{:keys [a b]} [c & d]]
             (binding [*scale* 3] (mapv #(+ % a b c *scale*) d)))
           (def h (with-gensym (twice 20)))
           (def sym (quoted-gensym))
           (defn k [x y] (or x (and y (swap-let x y))))
           (js/console.log (pr-str (f 1)) h (symbol? sym) (pr-str (g {:a 1 :b 2} [3 4 5]))
                           (pr-str (k nil 2)))"#,
    ),
];

const OTHER: [(&str, &str); 1] = [(
    "other.cljs",
    r#"(ns other)
       (defmacro many [& xs] `(let [a# 1 b# 2] (+ a# b# ~@xs)))
       (defn p [{:keys [x y]} & more] (or x y (many 1 2 3) #(apply + % more)))
       (js/console.log (str (p {:y 1})) (pr-str [(gensym) (gensym "x")]))"#,
)];

fn write(sources: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new("generated-names");
    for (path, text) in sources {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

/// `entry` from `root` at `none`, with its source map, whose `names` list
/// the generated names.
fn compile(entry: &str, root: &TempDir) -> (String, SourceMapping) {
    let program = analyze_namespace_entry(
        entry,
        &[PathBuf::from(root.path())],
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    prepare_application(&program, ApplicationOptions::new(OptimizationLevel::None)).emit_js_mapped()
}

fn execute(js: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--input-type=commonjs", "-e", js])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_program_compiles_alike_after_another_compilation() {
    let (program, other) = (write(&PROGRAM), write(&OTHER));
    let first = compile("app", &program);
    assert_eq!(execute(&first.0), "[4 1] 41 true [13 14] [2 nil]\n");
    // Another compilation in between advances nothing the program's names
    // are drawn from.
    let between = compile("other", &other);
    assert_eq!(execute(&between.0).lines().count(), 1);
    assert_eq!(compile("app", &program), first);
    assert_eq!(compile("other", &other), between);
    // The names are their scopes': a syntax-quote gensym read in the
    // entry's top-level forms, `v__D_N__auto__` with the namespace's
    // discriminator D, and `gensym`s of a deferred unit's expansions,
    // `gD_U_N`.
    let names = format!("{:?}", first.1);
    let spelled = |prefix: &str, parts: usize| {
        names.split('"').any(|name| {
            name.strip_prefix(prefix).is_some_and(|rest| {
                let number = rest.strip_suffix("__auto__").unwrap_or(rest);
                number.split('_').count() == parts
                    && number.split('_').all(|part| part.parse::<u64>().is_ok())
            })
        })
    };
    assert!(spelled("v__", 2), "{names}");
    assert!(spelled("g", 3), "{names}");
}

/// What `[a (b) (quoted-gensym)]` prints, each a quoted `gensym`.
fn printed_data(sources: &[(&str, &str)]) -> String {
    let program = write(sources);
    let first = compile("app", &program);
    assert_eq!(compile("app", &program), first);
    execute(&first.0)
}

const DATA: &str = r#"(ns app (:require [lib.macros :refer [quoted-gensym]]))
                      (def a (quoted-gensym))
                      (defn b [] (quoted-gensym))
                      (js/console.log (pr-str [a (b) (quoted-gensym)]))"#;

/// A `gensym` escaping as data prints its unit's scope and count: units `a`
/// and `b`, registered first and second, then the namespace's own first.
/// The namespace's discriminator is its own: loaded after another namespace,
/// at another source index, it prints the same.
#[test]
fn a_gensym_escaping_as_data_prints_its_unit_scope() {
    let mut sources = PROGRAM.to_vec();
    sources[1].1 = DATA;
    let printed = printed_data(&sources);
    let [a, b, top] = printed
        .trim_end()
        .trim_matches(['[', ']'])
        .split(' ')
        .map(|name| name.strip_prefix("data").unwrap().to_owned())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let discriminator = top.strip_suffix("_1").unwrap();
    assert!(discriminator.parse::<u64>().is_ok(), "{printed}");
    assert_eq!(a, format!("{discriminator}_1_1"));
    assert_eq!(b, format!("{discriminator}_2_1"));
    sources.push(("extra.cljs", "(ns extra) (def x 1)"));
    // `extra` loads before the macros, so `app` moves to source index 2.
    let later = PROGRAM[0]
        .1
        .replacen("(ns lib.macros)", "(ns lib.macros (:require [extra]))", 1);
    sources[0].1 = &later;
    assert_eq!(printed_data(&sources), printed);
}

#[test]
fn compilations_on_two_threads_compile_as_serially() {
    let (program, other) = (write(&PROGRAM), write(&OTHER));
    let serial = (compile("app", &program), compile("other", &other));
    let barrier = std::sync::Barrier::new(2);
    let parallel = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            (0..3).map(|_| compile("app", &program)).collect::<Vec<_>>()
        });
        let b = scope.spawn(|| {
            barrier.wait();
            (0..3).map(|_| compile("other", &other)).collect::<Vec<_>>()
        });
        (a.join().unwrap(), b.join().unwrap())
    });
    for app in parallel.0 {
        assert_eq!(app, serial.0);
    }
    for other in parallel.1 {
        assert_eq!(other, serial.1);
    }
}

/// A macro that calls a helper of its namespace, whose body expands a macro
/// of its own, numbers in the scope of the unit it expands in (`v`'s) after
/// the call, and the helper's macro in the helper's unit's.
/// (`RestoreGensymScope` puts a runtime back in the expanding unit's scope
/// after a unit analyzed on demand from a running macro; this program has
/// the helper analyzed before `outer` runs.)
#[test]
fn a_macro_numbers_in_its_scope_after_a_helper_analyzed_on_demand() {
    let mut sources = PROGRAM.to_vec();
    sources[1].1 = r#"(ns app)
       (defmacro inner [] (list 'quote (gensym "in")))
       (defn helper [] (inner))
       (defmacro outer [] (let [h (helper) g (gensym "out")] (list 'quote [h g])))
       (def v (outer))
       (js/console.log (pr-str v))"#;
    let program = write(&sources);
    let printed = execute(&compile("app", &program).0);
    let [inside, outside]: [String; 2] = printed
        .trim_end()
        .trim_matches(['[', ']'])
        .split(' ')
        .map(str::to_owned)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let scope = |name: &str, prefix: &str| {
        let number = name.strip_prefix(prefix).unwrap().to_owned();
        number.rsplit_once('_').unwrap().0.to_owned()
    };
    let (helper, unit) = (scope(&inside, "in"), scope(&outside, "out"));
    assert_ne!(helper, unit, "{printed}");
    assert_eq!(
        helper.split('_').next(),
        unit.split('_').next(),
        "{printed}"
    );
    assert!(outside.ends_with("_1"), "{printed}");
}
