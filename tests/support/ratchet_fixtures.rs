//! The optimization fixture set (docs/advanced-optimization.md, Phase 0), shared by
//! the size ratchet in tests/compiler/size_ratchet.rs and the startup benchmark in
//! benches/compiler.rs (`--levels`), so every fixture pinned here is also measured.
//!
//! Ceilings only move down: a change that shrinks output lowers them to the new size
//! rounded up to the next 100 bytes (next 1,000 from 10,000 bytes up) and records the
//! sizes in benchmarks/compiler.md. Raising a ceiling needs a reason in the commit
//! message. Add fixtures here when a later optimization needs a size or behavior
//! pinned; keep the five upstream rows.

pub struct Fixture {
    pub name: &'static str,
    pub source: &'static str,
    /// Byte ceilings in `OptimizationLevel::ALL` order: none, prepare, advanced.
    pub ceilings: [usize; 3],
    /// `ApplicationStats::removed_definitions` in the same order.
    pub removed: [usize; 3],
    /// Program output followed by the final value, as Node's `console.log` shows
    /// them, except that a function value prints as `#function`.
    pub printed: &'static str,
}

/// The first five rows are the Targets table of the design document, mirroring
/// upstream's `build_api_tests.clj` size ratchets (`trivial/core*.cljs`).
pub const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "console-log-sum",
        source: "(js/console.log (+ 1 2))",
        ceilings: [363_000, 363_000, 200],
        removed: [0, 0, 0],
        printed: "3\nundefined\n",
    },
    Fixture {
        name: "println-hi",
        source: r#"(enable-console-print!) (println "hi")"#,
        ceilings: [363_000, 363_000, 135_000],
        removed: [0, 0, 0],
        printed: "hi\nnull\n",
    },
    Fixture {
        name: "vector-printed",
        source: "(enable-console-print!) (println [1 2 3])",
        ceilings: [363_000, 363_000, 135_000],
        removed: [0, 0, 0],
        printed: "[1 2 3]\nnull\n",
    },
    Fixture {
        name: "map-printed",
        source: "(enable-console-print!) (println {})",
        ceilings: [363_000, 363_000, 135_000],
        removed: [0, 0, 0],
        printed: "{}\nnull\n",
    },
    Fixture {
        name: "seq-pipeline",
        source: "(js/console.log (->> (map inc (range 10)) (filter even?) (partition 2) \
                 (drop 1) (mapcat identity) into-array))",
        ceilings: [363_000, 363_000, 139_000],
        removed: [0, 0, 0],
        printed: "[ 6, 8 ]\nundefined\n",
    },
    Fixture {
        name: "add",
        source: "(+ 1 2)",
        ceilings: [363_000, 363_000, 100],
        removed: [0, 0, 0],
        printed: "3\n",
    },
    Fixture {
        name: "rest-fn",
        source: "(fn [& xs] xs)",
        ceilings: [363_000, 363_000, 120_000],
        removed: [0, 0, 0],
        printed: "#function\n",
    },
    Fixture {
        name: "keyword-case",
        source: "(defn kind [k] (case k :a 1 :b 2 3))
                 (+ (kind :a) (* 10 (kind :b)) (* 100 (kind :c)))",
        ceilings: [364_000, 364_000, 121_000],
        removed: [0, 0, 0],
        printed: "321\n",
    },
    Fixture {
        name: "record-protocol",
        source: "(defprotocol Area (area [shape]))
                 (defrecord Rect [w h] Area (area [_] (* w h)))
                 (area (->Rect 3 4))",
        ceilings: [364_000, 364_000, 12_000],
        removed: [0, 0, 0],
        printed: "12\n",
    },
    // Unused definitions with pure initializers: `Prepare` removes the literal,
    // `Advanced` also the core calls, `let` and record construction.
    Fixture {
        name: "pure-defs",
        source: r#"(defrecord Point [x y])
                 (def label (str "n" (+ 1 2)))
                 (def table (hash-map :a 1 :b (* 2 3)))
                 (def origin (->Point 0 0))
                 (def size (count [1 2 3]))
                 (def lookup (let [k :a] (get {:a 1} k)))
                 (def literal [1 2 3])
                 (:x (->Point 1 2))"#,
        ceilings: [365_000, 365_000, 12_000],
        removed: [0, 1, 6],
        printed: "1\n",
    },
    // Keyword literals at every site: interned at every level, and one table
    // constant each at `Advanced` (light-cljs-208).
    Fixture {
        name: "keyword-lookups",
        source: "(defn score [m] (+ (get m :a 0) (get m :b 0) (:c m 0) (get m :ns/d 0)))
                 (defn kind [k] (case k :x 1 :y 2 0))
                 (reduce + (map score [{:a 1 :b 2} {:c 3 :ns/d 4} {:a 5 :c (kind :y)}]))",
        ceilings: [364_000, 364_000, 132_000],
        removed: [0, 0, 0],
        printed: "17\n",
    },
    // Constant collection literals (light-cljs-218): the empty vector reads
    // the runtime's singleton at every level, and at `Advanced` the constant
    // parts of `row` and the `get-in` path are built once, from the pool.
    Fixture {
        name: "constant-literals",
        source: "(defn row [i] {:id i :tags #{:a :b} :pos [1 2] :empty []
                                :meta {:kind :row :dims [1 2 3]}})
                 (reduce + (map (fn [i] (let [r (row i)]
                                          (+ (count (:pos r)) (count (get-in r [:meta :dims])))))
                                (range 10)))",
        ceilings: [364_000, 364_000, 133_000],
        removed: [0, 0, 0],
        printed: "50\n",
    },
    // Three records implement one protocol and only one is constructed: at
    // `Advanced` rapid type analysis (light-cljs-211) keeps that type and the
    // one method called, which the call reads from the receiver, and no
    // dispatcher, protocol or record method.
    Fixture {
        name: "three-records",
        source: "(defprotocol Shape (area [s]) (perimeter [s]))
                 (defrecord Square [side] Shape (area [_] (* side side)) (perimeter [_] (* 4 side)))
                 (defrecord Circle [r] Shape (area [_] (* 3 r r)) (perimeter [_] (* 6 r)))
                 (defrecord Tri [a b c] Shape (area [_] (* a b)) (perimeter [_] (+ a b c)))
                 (area (->Square 3))",
        ceilings: [366_000, 366_000, 12_000],
        removed: [0, 0, 0],
        printed: "9\n",
    },
    // Retained raw JavaScript is an escape below `Advanced` only.
    Fixture {
        name: "raw-js",
        source: r#"(def unused "unused")
                 (defn add ([x] x) ([x y] (+ x y)))
                 (js* "void 0")
                 (add 1 2)"#,
        ceilings: [364_000, 364_000, 900],
        removed: [0, 0, 1],
        printed: "3\n",
    },
];
