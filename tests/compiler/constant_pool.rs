//! Empty collection singletons and the `Advanced` constant pool
//! (light-cljs-218; docs/advanced-optimization.md, "Constant collection
//! literals", tiers 1 and 2).
//!
//! At every level `[]`, `{}`, `#{}` and `()` read the runtime's `EMPTY`
//! singletons. At `Advanced` the constant table of light-cljs-208 also holds
//! constant collection literals, reader metadata on them and `#uuid`
//! literals, hash-consed on their emitted structure: each distinct constant
//! is built once, after the runtime, from earlier entries, and equal literals
//! at different sites are one value. Large constants are lazy cells. The
//! advanced-mode contract allows exactly one difference: equal constant
//! literals may be `identical?`.
use super::optimization::execute;
use light_cljs::{
    ApplicationOptions, ConstantPool, EvalOptions, EvalSession, OptimizationLevel, Program,
    analyze, parse, prepare_application,
};

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn emit(program: &Program, level: OptimizationLevel) -> String {
    prepare_application(program, ApplicationOptions::new(level)).emit_js()
}

fn printed(js: &str) -> String {
    execute(&format!("console.log({js});"))
}

/// The program prints `expected` (its value, through `console.log`) at every
/// level.
fn every_level(source: &str, expected: &str) {
    let program = program(source);
    for level in OptimizationLevel::ALL {
        assert_eq!(
            printed(&emit(&program, level)),
            format!("{expected}\n"),
            "at {level}: {source}"
        );
    }
}

/// The program prints `expected[i]` at level `OptimizationLevel::ALL[i]`.
fn per_level(source: &str, expected: [&str; 3]) {
    let program = program(source);
    for (level, expected) in OptimizationLevel::ALL.into_iter().zip(expected) {
        assert_eq!(
            printed(&emit(&program, level)),
            format!("{expected}\n"),
            "at {level}: {source}"
        );
    }
}

/// The table declarations of `Advanced` output, `<n> = <initializer>;` or
/// `<n> = () => c<n> ??= <initializer>;` for a lazy cell, in order.
fn declarations(js: &str) -> Vec<String> {
    js.lines()
        .filter_map(|line| {
            if let Some(rest) = line.strip_prefix("  const k") {
                rest.starts_with(|c: char| c.is_ascii_digit())
                    .then(|| rest.to_owned())
            } else {
                let rest = line.strip_prefix("  let c")?;
                let (_, entry) = rest.split_once("; const k")?;
                Some(entry.to_owned())
            }
        })
        .collect()
}

/// The hook that counts runtime calls: `js` with a counter at the top of each
/// named runtime function it embeds (`globalThis.lightCalls.<name>`). A
/// function the output does not embed counts zero.
fn counted(js: &str, names: &[&str]) -> String {
    let mut js = js.to_owned();
    for name in names {
        let header = format!("  function {name}(");
        let found = js.matches(&header).count();
        assert!(found <= 1, "{name} is declared {found} times");
        if let Some(start) = js.find(&header) {
            let body = start + js[start..].find("{\n").unwrap() + 1;
            js.insert_str(body, &format!(" globalThis.lightCalls.{name}++;"));
        }
    }
    let counters = names
        .iter()
        .map(|name| format!("{name}: 0"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "globalThis.lightCalls = {{{counters}}};\nconst value = {js};\n\
         console.log(value);\nconsole.log(JSON.stringify(globalThis.lightCalls));"
    )
}

/// The value and the call counts of `names` of a run of `js`.
pub(crate) fn calls(js: &str, names: &[&str]) -> (String, String) {
    let output = execute(&counted(js, names));
    let (value, counts) = output.trim_end().rsplit_once('\n').unwrap();
    (value.to_owned(), counts.to_owned())
}

fn pool(program: &Program, level: OptimizationLevel) -> ConstantPool {
    prepare_application(program, ApplicationOptions::new(level))
        .retention()
        .constants
}

/// Bytes of the table declarations in `js`, newlines included.
fn table_bytes(js: &str) -> usize {
    js.lines()
        .filter(|line| {
            ["  const k", "  let c"].iter().any(|prefix| {
                line.strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
            })
        })
        .map(|line| line.len() + 1)
        .sum()
}

#[test]
fn empty_literals_read_the_runtime_singletons_at_every_level() {
    // Tier 1, as upstream: every evaluation of an empty literal is the one
    // `EMPTY` value of its type, at every level and in the REPL.
    let source = r#"
      (defn v [] []) (defn m [] {}) (defn s [] #{}) (defn l [] ())
      (pr-str [(identical? (v) (v)) (identical? (v) []) (identical? (m) {}) (identical? (s) #{})
               (identical? (l) ()) (identical? '[] []) (identical? '{} {}) (identical? '() ())
               (identical? (v) (vector)) (= (v) (vector)) (conj (v) 1) (v) (assoc (m) :a 1) (m)
               (conj (s) 1) (s) (conj (l) 1) (l) (meta (with-meta (v) {:a 1})) (meta (v))
               (let [t (transient (m))] (assoc! t :b 2) (persistent! t)) (m)
               (= (hash (v)) (hash (vector))) (= (hash (m)) (hash (hash-map)))])"#;
    every_level(
        source,
        "[true true true true true true true true false true [1] [] {:a 1} {} #{1} #{} (1) () {:a 1} nil {:b 2} {} true true]",
    );
    let program = program(source);
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        let js = emit(&program, level);
        for empty in [
            "rt.core.PersistentVector.EMPTY",
            "rt.core.PersistentArrayMap.EMPTY",
            "rt.core.PersistentHashSet.EMPTY",
            "rt.core.List.EMPTY",
        ] {
            assert!(js.contains(empty), "{level}: {empty}");
        }
        for constructor in ["vector", "map", "set", "list"] {
            let allocation = format!("rt.{constructor}FromArray([])");
            assert!(!js.contains(&allocation), "{level}: {allocation}");
        }
    }
    let advanced = emit(&program, OptimizationLevel::Advanced);
    for empty in [
        "rt$PersistentVectorValue.EMPTY",
        "rt$PersistentArrayMapValue.EMPTY",
        "rt$PersistentHashSetValue.EMPTY",
        "rt$ListValue.EMPTY",
    ] {
        assert!(advanced.contains(empty), "{empty}");
    }
    for constructor in ["vector", "map", "set", "list"] {
        assert!(!advanced.contains(&format!("rt${constructor}FromArray([])")));
    }
    // Empty literals are inline reads, not table entries.
    assert!(
        declarations(&advanced)
            .iter()
            .all(|line| !line.contains("EMPTY"))
    );

    let mut session = EvalSession::new(EvalOptions::default()).unwrap();
    let value = session
        .eval_string("(defn v [] []) [(identical? (v) []) (identical? {} {}) (identical? #{} #{})]")
        .unwrap()
        .value;
    assert_eq!(session.print(value).unwrap(), "[true true true]");
}

#[test]
fn advanced_hash_conses_constants_on_their_emitted_structure() {
    // Equal subterms are one entry, declared before the entries that contain
    // them. Equality at run time does not merge entries: `[1 2]` and `'(1 2)`
    // are `=` but stay two values.
    let source = r#"
      (defn a [] [1 2]) (defn b [] [[1 2] {:k [1 2]}]) (defn c [] '(1 2))
      (pr-str [(a) (b) (c) (identical? (a) (first (b))) (identical? (a) (:k (second (b))))
               (= (a) (c)) (identical? (a) (c))])"#;
    per_level(
        source,
        [
            "[[1 2] [[1 2] {:k [1 2]}] (1 2) false false true false]",
            "[[1 2] [[1 2] {:k [1 2]}] (1 2) false false true false]",
            "[[1 2] [[1 2] {:k [1 2]}] (1 2) true true true false]",
        ],
    );
    let program = program(source);
    let advanced = emit(&program, OptimizationLevel::Advanced);
    assert_eq!(
        declarations(&advanced),
        [
            "0 = rt$hashed(rt$vectorFromArray([1, 2]), 1185709346);",
            "1 = rt$keyword(null, \"k\");",
            "2 = rt$hashed(rt$arrayMapFromArray([k1, k0]), 1918603281);",
            "3 = rt$hashed(rt$vectorFromArray([k0, k2]), 1067525070);",
            "4 = rt$hashed(rt$listFromArray([1, 2]), 1185709346);",
        ]
    );
    // The sites read the entries; no site builds a literal. The functions,
    // each a read of an entry, are inlined at their calls (light-cljs-214).
    assert_eq!(advanced.matches("rt$vectorFromArray(").count(), 3);
    assert!(
        advanced.contains("rt$vectorFromArray([k0, k3, k4, (k0 === rt$first(k3))"),
        "{advanced}"
    );
    // The table precedes the program in bundler output too.
    let esm = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
    .emit_esm();
    assert!(esm.contains("  const k3 = rt$hashed(rt$vectorFromArray([k0, k2]), 1067525070);\n"));
    // Below `Advanced` each evaluation builds its literal, as before.
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        let js = emit(&program, level);
        assert!(declarations(&js).is_empty(), "{level}");
        assert_eq!(
            js.matches("rt.vectorFromArray([1, 2])").count(),
            3,
            "{level}"
        );
    }
}

#[test]
fn reader_distinct_constants_are_distinct_entries() {
    // `1` and `1.0` read as distinct members of a literal set or map although
    // they lower to one JavaScript number; the pool keys on the constructor,
    // so the reader-distinct literal is not merged with `#{1}`.
    let source = r#"
      (defn a [] #{1 1.0}) (defn b [] #{1}) (defn c [] {1 :a 1.0 :b}) (defn d [] {1 :a})
      (defn e [] '#{x}) (defn f [] #{'x})
      (pr-str [(a) (b) (count (a)) (count (b)) (c) (d) (count (c)) (count (d)) (identical? (a) (b))
               (identical? (c) (d)) (e) (f) (identical? (e) (f)) (= (e) (f))])"#;
    every_level(
        source,
        "[#{1 1} #{1} 2 1 {1 :a, 1 :b} {1 :a} 2 1 false false #{x} #{x} false true]",
    );
    let advanced = emit(&program(source), OptimizationLevel::Advanced);
    let declarations = declarations(&advanced);
    for expected in [
        "rt$hashed(rt$constantSetFromArray([1, 1]), ",
        "rt$hashed(rt$setFromArray([1]), ",
        "rt$hashed(rt$arrayMapFromArray([1, k",
        "rt$hashed(rt$constantMapFromArray([1, k",
        "rt$hashed(rt$constantSetFromArray([k",
        "rt$hashed(rt$setFromArray([k",
    ] {
        assert!(
            declarations.iter().any(|line| line.contains(expected)),
            "{expected}: {declarations:?}"
        );
    }
}

#[test]
fn pooled_constants_keep_metadata_transients_equality_and_hashes() {
    // Sharing is invisible apart from identity: `with-meta` returns a new
    // value, transients copy, and equality and hashes do not depend on it.
    every_level(
        r#"
      (defn v [] [1 2 3]) (defn m [] {:a 1 :b [2]}) (defn s [] #{:x :y}) (defn mv [] ^{:k 1} [4 5])
      (defn sym [] '^:private x) (defn u [] #uuid "00000000-0000-0000-0000-000000000001")
      (pr-str [(meta (with-meta (v) {:m 1})) (meta (v)) (meta (mv)) (meta (vary-meta (mv) assoc :x 2))
               (meta (mv)) (meta (sym)) (meta (with-meta (sym) nil)) (meta (sym))
               (let [t (transient (v))] (conj! t 4) (persistent! t)) (v)
               (let [t (transient (m))] (assoc! t :c 3) (dissoc! t :a) (persistent! t)) (m)
               (let [t (transient (s))] (conj! t :z) (disj! t :x) (count (persistent! t))) (count (s))
               (conj (v) 9) (assoc (m) :a 2) (disj (s) :x) (v) (m) (count (s))
               (= (hash (v)) (hash (vector 1 2 3))) (= (hash (m)) (hash (hash-map :a 1 :b [2])))
               (= (hash (s)) (hash (hash-set :x :y))) (= (v) (vector 1 2 3)) (= (m) (hash-map :a 1 :b [2]))
               (= (u) (uuid "00000000-0000-0000-0000-000000000001")) (str (u))])"#,
        r#"[{:m 1} nil {:k 1} {:k 1, :x 2} {:k 1} {:private true} nil {:private true} [1 2 3 4] [1 2 3] {:c 3, :b [2]} {:a 1, :b [2]} 2 2 [1 2 3 9] {:a 2, :b [2]} #{:y} [1 2 3] {:a 1, :b [2]} 2 true true true true true true "00000000-0000-0000-0000-000000000001"]"#,
    );
}

#[test]
fn equal_literals_at_two_sites_are_identical_only_at_advanced() {
    // The advanced-mode contract: equal constant literals may be `identical?`.
    // Below `Advanced` each evaluation builds a new value; empty literals are
    // the runtime's singletons everywhere.
    per_level(
        r#"
      (defn a [] [1 {:b #{2}}]) (defn b [] [1 {:b #{2}}]) (defn u [] #uuid "00000000-0000-0000-0000-000000000002")
      (pr-str [(identical? (a) (b)) (identical? (a) (a)) (identical? (second (a)) (second (b)))
               (identical? (u) (u)) (identical? [] []) (= (a) (b))])"#,
        [
            "[false false false false true true]",
            "[false false false false true true]",
            "[true true true true true true]",
        ],
    );
}

#[test]
fn mutable_dynamic_and_possibly_throwing_literals_are_built_at_their_site() {
    // `#js` data, `js-obj`, regexes (`lastIndex`) and `#inst` (a mutable
    // `Date`) are fresh per evaluation at every level. A set literal whose
    // members are not all scalars may throw on equal members, so it is built
    // at its site, and a program that never evaluates it loads; its constant
    // members are still pooled. Partially constant literals pool their
    // constant parts.
    let source = r#"
      (defn arr [] #js [1 2]) (defn obj [] #js {:a 1}) (defn jo [] (js-obj "a" 1))
      (defn re [] #"a+") (defn inst [] #inst "2020-01-01T00:00:00.000-00:00")
      (defn dup [] #{[1] '(1)}) (defn sets [] #{[1] [2]}) (defn part [x] [x [1 2] {:k x}])
      (pr-str [(identical? (arr) (arr)) (identical? (obj) (obj)) (identical? (jo) (jo))
               (identical? (re) (re)) (identical? (inst) (inst))
               (let [d (inst)] (.setTime d 0) (= d (inst)))
               (let [r (re)] (.exec r "aa") (.-lastIndex r))
               (try (dup) (catch :default e (.-message e))) (sets) (part 3)
               (= (part 1) (part 1)) (= (second (part 1)) (second (part 2)))])"#;
    every_level(
        source,
        r#"[false false false false false false 0 "Duplicate key: [1]" #{[2] [1]} [3 [1 2] {:k 3}] true true]"#,
    );
    // `js-obj` expands to `js*`, whose hygiene renames the table, so the
    // output is checked without it.
    let advanced = emit(
        &program(&source.replace("(identical? (jo) (jo))", "")),
        OptimizationLevel::Advanced,
    );
    assert!(!advanced.contains("__light"));
    let table = declarations(&advanced).join("\n");
    assert!(!table.is_empty());
    for site in [
        "rt$rePattern(",
        "new Date(",
        "rt$literalSetFromArray([k",
        "rt$jsObject(",
    ] {
        assert!(advanced.contains(site), "{site}");
        assert!(!table.contains(site), "{site} is pooled:\n{table}");
    }
    // The constant part of `part` is pooled and shared across evaluations.
    per_level(
        "(defn part [x] [x [1 2] {:k x}]) (identical? (second (part 1)) (second (part 2)))",
        ["false", "false", "true"],
    );
    every_level("(defn dup [] #{[1] '(1)}) (count [dup])", "1");
}

#[test]
fn literals_built_by_a_replaced_core_operation_stay_at_their_site() {
    // `with-meta`, `uuid` and the type of an empty literal are core
    // operations a program may `set!`; every evaluation then sees the
    // replacement, so their literals are not built once ahead of time.
    every_level(
        r#"
      (defn f [] ^{:m 1} [1]) (defn u [] #uuid "00000000-0000-0000-0000-000000000003") (defn e [] [])
      (def before [(meta (f)) (str (u)) (e)])
      (set! cljs.core/with-meta (fn [x m] :replaced))
      (set! cljs.core/uuid (fn [s] :uuid-replaced))
      (set! cljs.core/PersistentVector #js {:EMPTY :empty})
      (pr-str [before (f) (u) (e)])"#,
        r#"[[{:m 1} "00000000-0000-0000-0000-000000000003" []] :replaced :uuid-replaced :empty]"#,
    );
}

#[test]
fn large_constants_are_lazy_cells_built_on_first_read() {
    // A constant of at least 1,024 nodes is declared as a lazy cell, read as
    // `k<n>()`; the constants built only for it are lazy too. Retained code
    // that never reads it does not build it.
    let numbers = (0..1100)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let definitions =
        format!("(defn big [] [[7 8 9] {numbers}]) (defn small [] [1 2]) (defn nested [] [7 8 9])");
    let unread = program(&format!("{definitions} (count (list big small nested))"));
    let advanced = emit(&unread, OptimizationLevel::Advanced);
    let declarations = declarations(&advanced);
    assert_eq!(declarations.len(), 3, "{declarations:?}");
    assert_eq!(
        declarations[0],
        "0 = () => c0 ??= rt$hashed(rt$vectorFromArray([7, 8, 9]), 298402038);"
    );
    assert!(
        declarations[1]
            .starts_with("1 = () => c1 ??= rt$hashed(rt$vectorFromArray([k0(), 0, 1, 2, ")
    );
    assert_eq!(
        declarations[2],
        "2 = rt$hashed(rt$vectorFromArray([1, 2]), 1185709346);"
    );
    assert!(advanced.contains("  let c1; const k1 = () => c1 ??= "));
    assert!(advanced.contains("return k1();") && advanced.contains("return k0();"));
    assert_eq!(
        pool(&unread, OptimizationLevel::Advanced),
        ConstantPool {
            entries: 3,
            composites: 3,
            lazy: 2,
            bytes: table_bytes(&advanced),
        }
    );
    // Loading builds only the eager entry.
    assert_eq!(
        calls(&advanced, &["vectorFromArray"]),
        ("3".into(), r#"{"vectorFromArray":1}"#.into())
    );
    // Reading builds a lazy cell once, with the lazy entries it reads; the
    // unread `small` is removed at `Advanced`.
    let read = program(&format!(
        "{definitions} (+ (count (big)) (count (big)) (count (nested)) (first (first (big))))"
    ));
    let expected = (1101 + 1101 + 3 + 7).to_string();
    for (level, count) in OptimizationLevel::ALL.into_iter().zip([7, 7, 2]) {
        assert_eq!(
            calls(&emit(&read, level), &["vectorFromArray"]),
            (
                expected.clone(),
                format!(r#"{{"vectorFromArray":{count}}}"#)
            ),
            "{level}"
        );
    }
}

#[test]
fn hot_loop_allocations_and_pool_size_are_pinned() {
    // The fixed allocation fixture: per-evaluation calls of the literal
    // constructors in a loop of 1,000 iterations, and the pool it declares.
    // Below `Advanced` each iteration builds every literal (keywords are
    // interned but still looked up per site); at `Advanced` the constant
    // parts are built once, and only the map with a dynamic value is built
    // per iteration, on its key shape (light-cljs-220), and the constant map
    // by `arrayMapFromArray` (light-cljs-230).
    let source = r#"
      (defn row [i] {:id i :tags #{:a :b} :pos [1 2] :empty [] :meta {:kind :row :dims [1 2]}})
      (defn total [n]
        (loop [i 0 acc 0]
          (if (< i n)
            (let [r (row i)] (recur (inc i) (+ acc (count (:pos r)) (count (:tags r)) (count (:empty r)))))
            acc)))
      (total 1000)"#;
    let names = [
        "vectorFromArray",
        "mapFromArray",
        "arrayMapFromArray",
        "mapFromShape",
        "setFromArray",
        "keyword",
        "makeVector",
    ];
    let program = program(source);
    let expected = [
        r#"{"vectorFromArray":2000,"mapFromArray":2000,"arrayMapFromArray":0,"mapFromShape":0,"setFromArray":1000,"keyword":13000,"makeVector":2001}"#,
        r#"{"vectorFromArray":2000,"mapFromArray":2000,"arrayMapFromArray":0,"mapFromShape":0,"setFromArray":1000,"keyword":13000,"makeVector":2001}"#,
        r#"{"vectorFromArray":1,"mapFromArray":0,"arrayMapFromArray":1,"mapFromShape":1000,"setFromArray":1,"keyword":10,"makeVector":2}"#,
    ];
    for (level, expected) in OptimizationLevel::ALL.into_iter().zip(expected) {
        assert_eq!(
            calls(&emit(&program, level), &names),
            ("4000".into(), expected.into()),
            "{level}"
        );
    }
    let advanced = emit(&program, OptimizationLevel::Advanced);
    assert_eq!(
        declarations(&advanced),
        [
            "0 = rt$keyword(null, \"id\");",
            "1 = rt$keyword(null, \"tags\");",
            "2 = rt$keyword(null, \"pos\");",
            "3 = rt$keyword(null, \"empty\");",
            "4 = rt$keyword(null, \"meta\");",
            "5 = rt$mapShape([k0, k1, k2, k3, k4], \
             [-1388402092, 1771418977, -864607220, 767870958, 1499536964]);",
            "6 = rt$keyword(null, \"b\");",
            "7 = rt$keyword(null, \"a\");",
            "8 = rt$hashed(rt$setFromArray([k6, k7]), 551401264);",
            "9 = rt$hashed(rt$vectorFromArray([1, 2]), 1185709346);",
            "10 = rt$keyword(null, \"kind\");",
            "11 = rt$keyword(null, \"row\");",
            "12 = rt$keyword(null, \"dims\");",
            "13 = rt$hashed(rt$arrayMapFromArray([k10, k11, k12, k9]), 1014014460);",
        ]
    );
    let table = pool(&program, OptimizationLevel::Advanced);
    assert_eq!(
        table,
        ConstantPool {
            entries: 14,
            composites: 3,
            lazy: 0,
            bytes: 700
        }
    );
    assert_eq!(
        prepare_application(
            &program,
            ApplicationOptions::new(OptimizationLevel::Advanced)
        )
        .retention()
        .to_string()
        .lines()
        .nth(1),
        Some("constant pool: 14 entries (3 composite, 0 lazy), 700 bytes")
    );
    assert_eq!(table.bytes, table_bytes(&advanced));
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        assert_eq!(pool(&program, level), ConstantPool::default(), "{level}");
    }
}

#[test]
fn discarded_constants_add_no_entries() {
    let source = "(do [1 2] :k 'sym {:a [3]} ^:m [4] #{:s} 5)";
    every_level(source, "5");
    let advanced = emit(&program(source), OptimizationLevel::Advanced);
    assert!(declarations(&advanced).is_empty(), "{advanced}");
}
