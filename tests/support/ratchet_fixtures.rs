//! The optimization fixture set (docs/advanced-optimization.md, Phase 0), shared by
//! the size ratchet in tests/compiler/size_ratchet.rs and the startup benchmark in
//! benches/compiler.rs (`--levels`), so every fixture pinned here is also measured,
//! except the programs of several namespaces in `NAMESPACE_FIXTURES`.
//!
//! Ceilings only move down: a change that shrinks output lowers them to the new size
//! rounded up to the next 100 bytes (next 1,000 from 10,000 bytes up) and records the
//! sizes in benchmarks/compiler.md. Raising a ceiling needs a reason in the commit
//! message. Add fixtures here when a later optimization needs a size or behavior
//! pinned; keep the five upstream rows.
//!
//! 2026-10-16: none/prepare ceilings include 2,000 additional bytes for the
//! compiler bridge's GC-owned provenance and bounded environment cache (1,344
//! bytes of shared runtime growth). Advanced slices discard this machinery;
//! their ceilings are unchanged. See benchmarks/penpot-investigation.md.
//!
//! Native sequence cursors (2026-10-16): eight advanced ceilings below increase
//! to the next 1,000 bytes for first/rest/next paths that avoid redispatching
//! unrelated protocols. This fixes override behavior and cuts the cold Penpot
//! render from 30.66 to 27.32 seconds; the representative QuickJS traversal
//! falls from 3,862 to 1,853 ms. The readable runtime grows by 767 bytes and
//! these slices exceed their previous ceilings by 11–362 bytes. Retain this
//! size/performance rationale in the eventual commit message.
//!
//! Environment descendants (2026-10-17): fifteen none/prepare ceilings move
//! to the next 1,000 bytes for bounded child reuse and canonical-snapshot
//! validation. The full runtime is embedded at these levels; the advanced
//! slices and ceilings are unchanged. The previous ceilings were exceeded by
//! 23–583 bytes. See the phase20 evidence in benchmarks/penpot-investigation.md;
//! retain this size/performance rationale in the eventual commit message.
//!
//! Intrinsic list construction (2026-10-17): eighteen none/prepare ceilings
//! move up 1,000 bytes for a direct list builder (390 readable bytes). Terse
//! slices using it grow by 99 bytes; only features exceeds its ceiling, by
//! 12 bytes, and moves to 71,000. This prevents a public ICounted override
//! from corrupting a new list's stored length and avoids redundant protocol
//! dispatch. The isolated main profile improves 166.78 -> 155.48 seconds.
//! Retain this size/performance rationale in the eventual commit message.

//! Fixed protocol metadata (2026-10-17): the readable runtime grows 1,051
//! bytes to cache private protocol ABI lookups while keeping membership and
//! method reads live. All 34 none/prepare ceilings move to the next 1,000
//! bytes; advanced sizes and ceilings are unchanged. The isolated main
//! profile improves 155.48 -> 148.13 seconds before frontier optimizations.
//! Retain this size/performance rationale in the eventual commit message.

//! Keyword record fields (2026-10-17): the readable runtime grows 493 bytes
//! for lookup/contains/assoc to use keyword identity instead of invoking a
//! key's IEquiv for every field. This fixes false field matches and reduces
//! the prototype Instaparse macro replay from 11.0 to 9.3 seconds. Twenty
//! none/prepare ceilings across eleven fixtures move to the next 1,000;
//! advanced ceilings stay unchanged. Retain this correctness/performance
//! rationale in the eventual commit message; see phase32 evidence.

//! Shared macro inputs (2026-10-17): 1,610 readable runtime bytes keep one
//! decoded call graph through environment import, then restore argument
//! provenance and validate snapshots without rebuilding those values. This
//! fixes &form/argument identity and avoids duplicate literal construction.
//! The prototype main improves 122.11 -> 116.98 seconds. All 34 none/prepare
//! ceiling pairs move to the next 1,000 bytes; advanced ceilings are unchanged.
//! Retain this correctness/performance rationale in the eventual commit message.

//! Seq kind tables (light-cljs-237, 2026-10-18): `seq`, `first`, `rest`,
//! `next` and `count` read a lazy seq's, a cell's or a view's branch from a
//! private table by the kind number its prototype holds. A program that
//! builds a lazy seq or a cell grows about 2.9 KB at advanced (the user
//! accepted about 2.8 KB for seq-pipeline going from 4.1x to 1.9x upstream;
//! it is 3.7x to 1.8x on this master); others move by at most 66 bytes.
//! Eleven advanced ceilings move to the next 1,000 bytes (closed-params by
//! 3 bytes) and every none/prepare ceiling by the 10,452 readable bytes.

//! Map entries, the array map's constructor and the types' statics
//! (light-cljs-247, 2026-10-18): the runtime that none and prepare embed
//! grows by the lean map entries, the public constructors' hooks, the statics'
//! condition and the wrappers of `lookup` and structural equality, with their
//! comments (5,425 bytes over both commits); those ceilings move to the next
//! 1,000. No advanced size grows; those that shrink by a step lower their
//! ceilings.

//! Private payload fields for the remaining per-operation kinds
//! (light-cljs-291, 2026-10-18): reduced markers, transients, queues and
//! iterator seqs use `privateStore`, and the maps and sets outside the hash
//! tries a `kindedStore` that holds payload and kind in two private fields of
//! one construction. An advanced program that tests values against these
//! stores keeps `privateStore` and `kindedStore` and grows 484-535 bytes
//! (`keyword-only` 2,875 to 3,396), for `event-maps` 6.2x and
//! `keyword-lookups` 2.2x as fast in Node; eighteen advanced ceilings move
//! to the new size rounded up (to 100 bytes below 10,000, else 1,000) and
//! every none/prepare ceiling by the 2,016 readable bytes.

//! WeakMap stores where the host states them (light-cljs-292, 2026-10-18):
//! `privateStore` and `kindedStore` return WeakMaps in the evaluator's
//! QuickJS, which makes its seq and per-operation workloads 10-35% faster.
//! The readable runtime grows 387 bytes; the nineteen none/prepare ceiling
//! pairs it passed move to the next 1,000. Advanced sizes shrink by up to
//! 61 bytes, as the stores share one test for a value that can hold a field;
//! the ceilings of atom-str, vector-printed, map-printed and defmulti move down.

//! Collection kinds (light-cljs-358, second part, 2026-10-19): vectors,
//! maps, sets and queues are kinds of the kind tables, which `seq`, `count`,
//! `nth`, `get`, `conj` and `assoc` read once per call. In Node at advanced,
//! `count` of the four collections is 3.3x as fast, `get` of a vector 5x, of
//! a hash map 1.7x, `nth` 2.9x, and with protocol implementations `conj`
//! onto a vector 4.7x. A program that keeps one of these operations and
//! builds a collection keeps its wrapper, the collections' branches and the
//! tables: 30 to 2,770 bytes at advanced over the first part (`map-only`
//! 7,313 to 7,879, `features` 72,736 to 75,501). The advanced ceilings
//! passed move to the new size rounded up (to 100 bytes below 10,000, else
//! 1,000), and every none/prepare ceiling, for the 10.1 KB of readable
//! runtime (branches, wrappers and their comments), to the next 1,000.

//! Per-type record equality and hash (light-cljs-267, 2026-10-19): at
//! advanced a record type of a program that compares or hashes values gets
//! its own `-equiv` and `-hash` (`record-keys` 7.7x to 2.07x upstream); no
//! fixture's advanced size moves. The readable runtime grows 283 bytes for
//! `recordEntriesHash`, which the shared record hash and the per-type one
//! share; the five none/prepare ceiling pairs it passed move to the next
//! 1,000.

//! Sets' `get` and map entry kinds (light-cljs-373, 2026-10-19): `get` of a
//! set and `seq`, `count`, `nth` and `get` of a map entry read the kind
//! tables (in Node at advanced `get` of a set 1.2x as fast, `count` of an
//! entry 2.6x, `get` of an entry 4.4x, destructuring an entry 1.3x). A
//! program building a map keeps the entries' kind (18 bytes) and the
//! branches of the operations it keeps: `map-only` 7,879 to 8,053,
//! `keyword-lookups` 34,714 to 35,170, `eight-records` 34,883 to 35,315,
//! `constant-keys` 47,532 to 48,297 and `features` 75,499 to 76,265 (which
//! also keep `get.sets`), and `closed-params` 14,970 to 15,027 (`get`'s
//! token for a method answering `missing`). Those advanced ceilings move to
//! the new size rounded up as before, and every none/prepare ceiling, for
//! the 3.1 KB of readable runtime, to the next 1,000.

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
        ceilings: [491_000, 491_000, 200],
        removed: [0, 0, 0],
        printed: "3\nundefined\n",
    },
    Fixture {
        name: "println-hi",
        source: r#"(enable-console-print!) (println "hi")"#,
        ceilings: [491_000, 491_000, 4_800],
        removed: [0, 0, 0],
        printed: "hi\nnull\n",
    },
    Fixture {
        name: "vector-printed",
        source: "(enable-console-print!) (println [1 2 3])",
        ceilings: [491_000, 491_000, 11_000],
        removed: [0, 0, 0],
        printed: "[1 2 3]\nnull\n",
    },
    Fixture {
        name: "map-printed",
        source: "(enable-console-print!) (println {})",
        ceilings: [491_000, 491_000, 14_000],
        removed: [0, 0, 0],
        printed: "{}\nnull\n",
    },
    Fixture {
        name: "seq-pipeline",
        source: "(js/console.log (->> (map inc (range 10)) (filter even?) (partition 2) \
                 (drop 1) (mapcat identity) into-array))",
        ceilings: [491_000, 491_000, 30_000],
        removed: [0, 0, 0],
        printed: "[ 6, 8 ]\nundefined\n",
    },
    Fixture {
        name: "add",
        source: "(+ 1 2)",
        ceilings: [491_000, 491_000, 100],
        removed: [0, 0, 0],
        printed: "3\n",
    },
    Fixture {
        name: "rest-fn",
        source: "(fn [& xs] xs)",
        ceilings: [491_000, 491_000, 11_000],
        removed: [0, 0, 0],
        printed: "#function\n",
    },
    Fixture {
        name: "keyword-case",
        source: "(defn kind [k] (case k :a 1 :b 2 3))
                 (+ (kind :a) (* 10 (kind :b)) (* 100 (kind :c)))",
        ceilings: [491_000, 491_000, 3_500],
        removed: [0, 0, 0],
        printed: "321\n",
    },
    Fixture {
        name: "record-protocol",
        source: "(defprotocol Area (area [shape]))
                 (defrecord Rect [w h] Area (area [_] (* w h)))
                 (area (->Rect 3 4))",
        ceilings: [492_000, 492_000, 5_300],
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
        ceilings: [492_000, 492_000, 5_200],
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
        ceilings: [492_000, 492_000, 36_000],
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
        ceilings: [491_000, 491_000, 26_000],
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
        ceilings: [492_000, 492_000, 3_900],
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
        ceilings: [494_000, 494_000, 5_300],
        removed: [0, 0, 0],
        printed: "9\n",
    },
    // Eight records and eight deftypes (light-cljs-264), each constructed,
    // its method called through the protocol's dispatcher and its field read
    // by a function that gets all sixteen, so that every type survives. What
    // each type adds is tested in `size_ratchet`, which builds the same
    // source with fewer types.
    Fixture {
        name: "eight-records",
        source: "(defprotocol Shape (area [s]))\n\
                 (defrecord R1 [a b] Shape (area [_] (+ a b)))\n\
                 (defrecord R2 [a b] Shape (area [_] (- a b)))\n\
                 (defrecord R3 [a b] Shape (area [_] (* a b)))\n\
                 (defrecord R4 [a b] Shape (area [_] (+ b a)))\n\
                 (defrecord R5 [a b] Shape (area [_] (- b a)))\n\
                 (defrecord R6 [a b] Shape (area [_] (* b a)))\n\
                 (defrecord R7 [a b] Shape (area [_] (+ a a)))\n\
                 (defrecord R8 [a b] Shape (area [_] (* b b)))\n\
                 (deftype T1 [a b] Shape (area [_] (* b b)))\n\
                 (deftype T2 [a b] Shape (area [_] (+ a a)))\n\
                 (deftype T3 [a b] Shape (area [_] (* b a)))\n\
                 (deftype T4 [a b] Shape (area [_] (- b a)))\n\
                 (deftype T5 [a b] Shape (area [_] (+ b a)))\n\
                 (deftype T6 [a b] Shape (area [_] (* a b)))\n\
                 (deftype T7 [a b] Shape (area [_] (- a b)))\n\
                 (deftype T8 [a b] Shape (area [_] (+ a b)))\n\
                 (reduce (fn [t s] (+ t (area s) (.-a s))) 0 \
                 [(->R1 1 2) (->R2 2 3) (->R3 3 4) (->R4 4 5) (->R5 5 6) (->R6 6 7) (->R7 7 8) \
                 (->R8 8 9) (T1. 9 10) (T2. 10 11) (T3. 11 12) (T4. 12 13) (T5. 13 14) \
                 (T6. 14 15) (T7. 15 16) (T8. 16 17)])",
        ceilings: [502_000, 502_000, 36_000],
        removed: [0, 0, 0],
        printed: "819\n",
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
        ceilings: [492_000, 492_000, 49_000],
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
        ceilings: [491_000, 491_000, 34_000],
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
        ceilings: [491_000, 491_000, 5_100],
        removed: [0, 0, 0],
        printed: "1 2 3\nundefined\n",
    },
    Fixture {
        name: "keyword-only",
        source: "(js/console.log (.toString :a))",
        ceilings: [491_000, 491_000, 3_400],
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
        ceilings: [491_000, 491_000, 8_100],
        removed: [0, 0, 0],
        printed: "1\nundefined\n",
    },
    Fixture {
        name: "atom-str",
        source: "(js/console.log (str @(atom 1) 2))",
        ceilings: [491_000, 491_000, 3_000],
        removed: [0, 0, 0],
        printed: "12\nundefined\n",
    },
    // Since light-cljs-231 a set answers its host-facing methods from its
    // members and a set literal of at most 8 constants builds an array set,
    // so a set keeps no seq, hash map or hashing.
    Fixture {
        name: "set-only",
        source: r#"(js/console.log (.has #{1 2 3} 2) (.join (js/Array.from #{1 2 3}) " "))"#,
        ceilings: [491_000, 491_000, 4_800],
        removed: [0, 0, 0],
        printed: "true 1 3 2\nundefined\n",
    },
    // Since light-cljs-238 the generic seq operations call protocol
    // implementations only where the program has some, and keep a branch for
    // vectors only where it builds them.
    Fixture {
        name: "mapv-count",
        source: "(js/console.log (count (mapv inc [1 2 3])))",
        ceilings: [491_000, 491_000, 37_000],
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
        ceilings: [492_000, 492_000, 100],
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
        ceilings: [493_000, 493_000, 400],
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
        ceilings: [493_000, 493_000, 16_000],
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
        ceilings: [492_000, 492_000, 100],
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
        ceilings: [494_000, 494_000, 500],
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
        ceilings: [493_000, 493_000, 44_000],
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
        ceilings: [494_000, 494_000, 49_000],
        removed: [0, 0, 0],
        printed: "28.5a-bbbb%cccccc%\n",
    },
    // light-cljs-239: a multimethod with keyword dispatch values and no
    // hierarchy keeps its tables as arrays, walks no hierarchy and calls no
    // `IMultiFn` dispatcher; its map argument keeps what a map literal keeps.
    Fixture {
        name: "defmulti",
        source: "(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (m {:k :a}))",
        ceilings: [491_000, 491_000, 11_000],
        removed: [0, 0, 0],
        printed: "1\nundefined\n",
    },
];

/// A program of several namespaces, analyzed from `entry` over the in-memory
/// source root `files` (light-cljs-272). The startup benchmark measures only
/// [`FIXTURES`], whose programs are one source text.
pub struct NamespaceFixture {
    pub name: &'static str,
    pub entry: &'static str,
    /// Each namespace's source by its path under the source root.
    pub files: &'static [(&'static str, &'static str)],
    /// As [`Fixture::ceilings`].
    pub ceilings: [usize; 3],
    /// As [`Fixture::removed`].
    pub removed: [usize; 3],
    /// As [`Fixture::printed`], followed by what the program logs once its
    /// promise reactions have run.
    pub printed: &'static str,
}

/// The source of the loadable target of `loadable-site` and the function
/// `direct-site` calls directly.
const GREET: (&str, &str) = (
    "app/greet.cljs",
    "(ns app.greet) (defn greet [who] (.concat \"hello \" who))",
);

/// The namespaces of examples/features its entry `app.main` loads.
const FEATURES_SHARED: (&str, &str) = (
    "app/shared.cljs",
    include_str!("../../examples/features/app/shared.cljs"),
);
const FEATURES_EDITOR: (&str, &str) = (
    "app/editor.cljs",
    include_str!("../../examples/features/app/editor.cljs"),
);
const FEATURES_REPORTS: (&str, &str) = (
    "app/reports.cljs",
    include_str!("../../examples/features/app/reports.cljs"),
);

/// The code-splitting example and the same example calling its features
/// directly (`features-direct`), and a loadable at its smallest beside the same
/// program reading its target directly: the difference between the last two
/// is the cost of one `light.lazy/loadable` site with the `Loadable` type and
/// `light.lazy/load` (docs/code-splitting.md, "Phase 1").
pub const NAMESPACE_FIXTURES: &[NamespaceFixture] = &[
    NamespaceFixture {
        name: "features",
        entry: "app.main",
        files: &[
            (
                "app/main.cljs",
                include_str!("../../examples/features/app/main.cljs"),
            ),
            FEATURES_SHARED,
            FEATURES_EDITOR,
            FEATURES_REPORTS,
        ],
        ceilings: [512_000, 512_000, 77_000],
        removed: [0, 0, 21],
        printed: "menu: Editor | Reports\n#function\n",
    },
    NamespaceFixture {
        name: "features-direct",
        entry: "app.main",
        files: &[
            (
                "app/main.cljs",
                "(ns app.main (:require [app.shared :as shared] [app.editor] [app.reports]))
                 (js/console.log \"menu:\" (shared/label :editor) \"|\" (shared/label :reports))
                 (def editor app.editor/start)
                 (def reports app.reports/start)
                 (defn start []
                   (-> (js/Promise.resolve editor)
                       (.then (fn [start-editor] (start-editor) (js/Promise.resolve reports)))
                       (.then (fn [start-reports] (start-reports) nil))))
                 start",
            ),
            FEATURES_SHARED,
            FEATURES_EDITOR,
            FEATURES_REPORTS,
        ],
        ceilings: [511_000, 511_000, 76_000],
        removed: [0, 0, 20],
        printed: "menu: Editor | Reports\n#function\n",
    },
    NamespaceFixture {
        name: "loadable-site",
        entry: "app.main",
        files: &[
            (
                "app/main.cljs",
                "(ns app.main (:require [light.lazy :as lazy]))
                 (def greeting (lazy/loadable app.greet/greet))
                 (.then (lazy/load greeting) (fn [greet] (js/console.log (greet \"lazy\"))))
                 nil",
            ),
            GREET,
        ],
        ceilings: [492_000, 492_000, 600],
        removed: [0, 1, 2],
        printed: "null\nhello lazy\n",
    },
    NamespaceFixture {
        name: "direct-site",
        entry: "app.main",
        files: &[
            (
                "app/main.cljs",
                "(ns app.main (:require [app.greet :as greet]))
                 (def greeting greet/greet)
                 (.then (js/Promise.resolve greeting) (fn [greet] (js/console.log (greet \"lazy\"))))
                 nil",
            ),
            GREET,
        ],
        ceilings: [491_000, 491_000, 300],
        removed: [0, 0, 0],
        printed: "null\nhello lazy\n",
    },
];
