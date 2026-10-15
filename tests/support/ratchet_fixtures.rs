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
        ceilings: [427_000, 427_000, 200],
        removed: [0, 0, 0],
        printed: "3\nundefined\n",
    },
    Fixture {
        name: "println-hi",
        source: r#"(enable-console-print!) (println "hi")"#,
        ceilings: [427_000, 427_000, 4_800],
        removed: [0, 0, 0],
        printed: "hi\nnull\n",
    },
    Fixture {
        name: "vector-printed",
        source: "(enable-console-print!) (println [1 2 3])",
        ceilings: [427_000, 427_000, 11_000],
        removed: [0, 0, 0],
        printed: "[1 2 3]\nnull\n",
    },
    Fixture {
        name: "map-printed",
        source: "(enable-console-print!) (println {})",
        ceilings: [427_000, 427_000, 15_000],
        removed: [0, 0, 0],
        printed: "{}\nnull\n",
    },
    Fixture {
        name: "seq-pipeline",
        source: "(js/console.log (->> (map inc (range 10)) (filter even?) (partition 2) \
                 (drop 1) (mapcat identity) into-array))",
        ceilings: [427_000, 427_000, 26_000],
        removed: [0, 0, 0],
        printed: "[ 6, 8 ]\nundefined\n",
    },
    Fixture {
        name: "add",
        source: "(+ 1 2)",
        ceilings: [427_000, 427_000, 100],
        removed: [0, 0, 0],
        printed: "3\n",
    },
    Fixture {
        name: "rest-fn",
        source: "(fn [& xs] xs)",
        ceilings: [427_000, 427_000, 9_800],
        removed: [0, 0, 0],
        printed: "#function\n",
    },
    Fixture {
        name: "keyword-case",
        source: "(defn kind [k] (case k :a 1 :b 2 3))
                 (+ (kind :a) (* 10 (kind :b)) (* 100 (kind :c)))",
        ceilings: [427_000, 427_000, 3_600],
        removed: [0, 0, 0],
        printed: "321\n",
    },
    Fixture {
        name: "record-protocol",
        source: "(defprotocol Area (area [shape]))
                 (defrecord Rect [w h] Area (area [_] (* w h)))
                 (area (->Rect 3 4))",
        ceilings: [428_000, 428_000, 5_500],
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
        ceilings: [428_000, 428_000, 5_400],
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
        ceilings: [428_000, 428_000, 31_000],
        removed: [0, 0, 0],
        printed: "17\n",
    },
    // Constant collection literals (light-cljs-218): the empty vector reads
    // the runtime's singleton at every level, and at `Advanced` the constant
    // parts of `row` and the `get-in` path are built once, from the pool.
    // `row`, called once, is inlined into its caller (light-cljs-214), where
    // the lookups of its constant values and their counts fold, so no row is
    // built (light-cljs-221).
    Fixture {
        name: "constant-literals",
        source: "(defn row [i] {:id i :tags #{:a :b} :pos [1 2] :empty []
                                :meta {:kind :row :dims [1 2 3]}})
                 (reduce + (map (fn [i] (let [r (row i)]
                                          (+ (count (:pos r)) (count (get-in r [:meta :dims])))))
                                (range 10)))",
        ceilings: [428_000, 428_000, 22_000],
        removed: [0, 0, 1],
        printed: "50\n",
    },
    // Lookups, counts and destructuring of constants (light-cljs-221): at
    // `Advanced` a `^:const` map's destructured keys, a constant vector's
    // items and the counts are their values, and the membership test is a
    // `switch` on the keyword's spelling, so no collection is left.
    Fixture {
        name: "constant-lookups",
        source: "(def ^:const CONFIG {:width 640 :height 480 :title \"demo\" :modes #{:fast :safe}})
                 (defn allowed? [mode] (if (#{:fast :safe} mode) 1 0))
                 (let [{:keys [width height title]} CONFIG
                       [x y] [3 4]]
                   (+ (* width height) (count title) x y (count (:modes CONFIG))
                      (allowed? (keyword (.toLowerCase \"FAST\")))))",
        ceilings: [428_000, 428_000, 4_000],
        removed: [0, 0, 1],
        printed: "307214\n",
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
        ceilings: [430_000, 430_000, 5_500],
        removed: [0, 0, 0],
        printed: "9\n",
    },
    // Constant collections as keys of a hash map (light-cljs-219): the
    // runtime caches each key's hash, and at `Advanced` the pooled keys carry
    // their hashes from the compiler.
    Fixture {
        name: "constant-keys",
        source: "(def table (zipmap (concat (map (fn [i] {:id i :kind :row}) (range 12))
                                        (map (fn [i] [:pos i (inc i)]) (range 12))
                                        (map (fn [i] #{i :tag}) (range 12)))
                                (range 36)))
                 (+ (get table {:id 3 :kind :row}) (get table [:pos 5 6]) (get table #{7 :tag})
                    (get table {:id 11 :kind :row} 0))",
        ceilings: [428_000, 428_000, 43_000],
        removed: [0, 0, 0],
        printed: "62\n",
    },
    // Event maps, a map literal with constant keys and dynamic values
    // (light-cljs-220): at `Advanced` its keys and their hashes are a pooled
    // shape, and each evaluation passes only the values. `event`, called
    // once, is inlined into its caller (light-cljs-214).
    Fixture {
        name: "event-maps",
        source: "(defn event [i] {:type :click :x i :y (- 100 i) :button 0 :target \"submit\"})
                 (reduce + (map (fn [i] (let [e (event i)] (+ (:x e) (:y e) (count e)))) (range 10)))",
        ceilings: [427_000, 427_000, 31_000],
        removed: [0, 0, 1],
        printed: "1050\n",
    },
    // A program that only builds a vector or a keyword keeps that type, what
    // its host-facing methods (`equiv`, `indexOf`, iteration, `call`) need
    // for the kinds it builds, and no map, set or protocol machinery
    // (light-cljs-227).
    Fixture {
        name: "vector-only",
        source: r#"(js/console.log (.join (js/Array.from [1 2 3]) " "))"#,
        ceilings: [427_000, 427_000, 5_000],
        removed: [0, 0, 0],
        printed: "1 2 3\nundefined\n",
    },
    Fixture {
        name: "keyword-only",
        source: "(js/console.log (.toString :a))",
        ceilings: [427_000, 427_000, 3_600],
        removed: [0, 0, 0],
        printed: ":a\nundefined\n",
    },
    // Since light-cljs-230 a vector or map answers its host-facing methods
    // from its own items, so neither keeps a seq; a map literal keeps no hash
    // map, `str` of scalars no printer and `atom` without options no option
    // parsing.
    Fixture {
        name: "map-only",
        source: "(js/console.log (.get {:a 1} :a))",
        ceilings: [427_000, 427_000, 10_000],
        removed: [0, 0, 0],
        printed: "1\nundefined\n",
    },
    Fixture {
        name: "atom-str",
        source: "(js/console.log (str @(atom 1) 2))",
        ceilings: [427_000, 427_000, 2_500],
        removed: [0, 0, 0],
        printed: "12\nundefined\n",
    },
    // Since light-cljs-231 a set answers its host-facing methods from its
    // members and a set literal of at most 8 constants builds an array set,
    // so a set keeps no seq, hash map or hashing.
    Fixture {
        name: "set-only",
        source: r#"(js/console.log (.has #{1 2 3} 2) (.join (js/Array.from #{1 2 3}) " "))"#,
        ceilings: [427_000, 427_000, 5_300],
        removed: [0, 0, 0],
        printed: "true 1 3 2\nundefined\n",
    },
    // Since light-cljs-238 the generic seq operations call protocol
    // implementations only where the program has some, and keep a branch for
    // vectors only where it builds them.
    Fixture {
        name: "mapv-count",
        source: "(js/console.log (count (mapv inc [1 2 3])))",
        ceilings: [427_000, 427_000, 32_000],
        removed: [0, 0, 0],
        printed: "3\nundefined\n",
    },
    // Retained raw JavaScript is an escape below `Advanced` only. There,
    // `add` is also inlined and folded (light-cljs-214).
    Fixture {
        name: "raw-js",
        source: r#"(def unused "unused")
                 (defn add ([x] x) ([x y] (+ x y)))
                 (js* "void 0")
                 (add 1 2)"#,
        ceilings: [428_000, 428_000, 100],
        removed: [0, 0, 2],
        printed: "3\n",
    },
    // Functions with several methods that are only called: at `Advanced` each
    // call names the method it runs and no dispatcher is defined
    // (light-cljs-212). Since light-cljs-214 `clamp` and `area` are inlined
    // at their calls and folded, and only `fib`, a `recur` target, is left.
    Fixture {
        name: "multi-arity",
        source: "(defn clamp ([x] (clamp x 0 10)) ([x lo hi] (max lo (min hi x))))
                 (defn area ([w] (area w w)) ([w h] (* w h)))
                 (defn fib ([n] (fib n 0 1)) ([n a b] (if (zero? n) a (recur (dec n) b (+ a b)))))
                 (+ (clamp 15) (area 3) (area 2 5) (fib 10))",
        ceilings: [429_000, 429_000, 400],
        removed: [0, 0, 2],
        printed: "84\n",
    },
    // Closed functions whose parameters take the types of their calls'
    // arguments (light-cljs-229): at `Advanced` record field reads, `nth` on a
    // vector, `=` on strings and keywords and `count` of a string lower inside
    // them. Since light-cljs-214 `at`, `width` and `weight` are inlined into
    // `score`, where the arguments' own types apply.
    Fixture {
        name: "closed-params",
        source: "(defrecord Pt [x y])
                 (defn dist2 [p q] (let [dx (- (:x p) (:x q)) dy (- (:y p) (:y q))] (+ (* dx dx) (* dy dy))))
                 (defn at [v i] (nth v i))
                 (defn width [s] (if (= s \"\") 0 (count s)))
                 (defn weight [k] (if (= k :heavy) 10 1))
                 (defn score [acc v i s k]
                   (+ acc (dist2 (->Pt i 0) (->Pt 0 2)) (at v (rem i 3)) (width s) (weight k)))
                 (loop [i 0 acc 0]
                   (if (< i 10)
                     (recur (inc i) (score acc [1 2 3] i (if (even? i) \"\" \"word\") (if (odd? i) :heavy :light)))
                     acc))",
        ceilings: [429_000, 429_000, 14_000],
        removed: [0, 0, 3],
        printed: "419\n",
    },
    // A debug-only logging path behind a `goog-define` (light-cljs-214): at
    // `Advanced` the define's value folds every `when DEBUG` away, the logging
    // function becomes empty and is inlined at its calls, and nothing of the
    // path (its strings, `pr-str` and the printer) is left, so the output is
    // the multiplication alone.
    Fixture {
        name: "debug-logging",
        source: r#"(goog-define DEBUG false)
                 (defn debug-log [& parts] (when DEBUG (js/console.log "[debug]" (pr-str (vec parts)))))
                 (defn area [w h] (debug-log "area" w h {:w w :h h}) (* w h))
                 (when DEBUG (debug-log "startup" {:debug true}))
                 (area (.-length "abc") 4)"#,
        ceilings: [428_000, 428_000, 100],
        removed: [0, 0, 3],
        printed: "12\n",
    },
    // Local and named functions with several methods that are only called: at
    // `Advanced` their methods are entries with no dispatcher, those that
    // capture nothing in the constant table (light-cljs-228).
    Fixture {
        name: "local-arities",
        source: "(defn score [n lo hi]
                   (let [clamp (fn clamp ([x] (clamp x lo hi)) ([x l h] (if (< x l) l (if (> x h) h x))))
                         area (fn area ([w] (area w w)) ([w h] (* w h)))]
                     (letfn [(walk ([i] (walk i 0))
                                   ([i acc] (if (< i n) (recur (inc i) (+ acc (clamp i) (area i 2))) acc)))]
                       (+ (walk -3) (area 3)))))
                 (score 12 0 10)",
        ceilings: [430_000, 430_000, 500],
        removed: [0, 0, 0],
        printed: "194\n",
    },
    // Functions passed to `filter`, `reduce`, `map` and `swap!`, whose
    // parameters take what those operations pass them, as their callback
    // shapes in src/core_operations.tsv say (light-cljs-243): at `Advanced`
    // `heavy?`, `add-cost` (with `cost` inlined) and the `map` callback read
    // the record's fields, so no `:qty`, `:price` or `:label` keyword is
    // built, and the `swap!` callback compares its keyword argument directly.
    Fixture {
        name: "callbacks",
        source: "(defrecord Item [label qty price])
                 (defn cost [item] (* (:qty item) (:price item)))
                 (defn heavy? [item] (> (:qty item) 2))
                 (defn add-cost [total item] (+ total (cost item)))
                 (def counter (atom 0))
                 (let [items [(->Item \"a\" 1 2) (->Item \"bb\" 3 4) (->Item \"ccc\" 5 1)]]
                   (swap! counter (fn [n k] (if (= k :all) (+ n (count items)) n)) :all)
                   (+ (reduce add-cost 0 (filter heavy? items))
                      (reduce + (map (fn [item] (count (:label item))) items))
                      @counter))",
        ceilings: [429_000, 429_000, 38_000],
        removed: [0, 0, 1],
        printed: "26\n",
    },
    // Protocol methods whose parameters take what the protocol's calls pass
    // them (light-cljs-253): at `Advanced` both types' methods read the
    // record's fields, so no `:qty`, `:price` or `:label` keyword is built,
    // and `tag` tests its boolean flag directly.
    Fixture {
        name: "protocol-params",
        source: "(defrecord Item [label qty price])
                 (defprotocol Pricing (cost [p item]) (tag [p item flag]))
                 (deftype Plain [] Pricing
                   (cost [_ item] (* (:qty item) (:price item)))
                   (tag [_ item flag] (if flag (:label item) \"-\")))
                 (deftype Sale [pct] Pricing
                   (cost [_ item] (* (:qty item) (:price item) (- 1 pct)))
                   (tag [_ item flag] (if flag (str (:label item) \"%\") \"-\")))
                 (let [items [(->Item \"a\" 1 2) (->Item \"bb\" 3 4) (->Item \"ccc\" 5 1)]
                       plain (Plain.)
                       sale (Sale. 0.5)]
                   (str (reduce + (map (fn [item] (+ (cost plain item) (cost sale item))) items))
                        (apply str (map (fn [item]
                                          (str (tag plain item true)
                                               (tag sale item (> (:qty item) 2))))
                                        items))))",
        ceilings: [430_000, 430_000, 43_000],
        removed: [0, 0, 0],
        printed: "28.5a-bbbb%cccccc%\n",
    },
];
