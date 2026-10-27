#![cfg(feature = "bbp-macros")]
use light_cljs::{MacroHost, analyze_with_macros, emit_js, parse};

const SEQ_FIXTURES: &[(&str, &str)] = &[
    (
        "(defmacro call-form [x] (seq x)) (js/console.log (call-form [+ 1 2]))",
        "3\n",
    ),
    (
        "(defmacro quoted-seq [x] (list 'quote (seq x))) (js/console.log (pr-str (quoted-seq [1 2])) (pr-str (quoted-seq {:a 1})) (pr-str (quoted-seq \"ab\")))",
        "(1 2) ([:a 1]) (\"a\" \"b\")\n",
    ),
    (
        "(defmacro sequence? [x] (seq? (seq x))) (js/console.log (sequence? [1]) (sequence? {:a 1}) (sequence? \"a\") (sequence? (1)))",
        "true true true true\n",
    ),
    (
        "(defmacro empty-seq? [x] (nil? (seq x))) (js/console.log (empty-seq? nil) (empty-seq? ()) (empty-seq? []) (empty-seq? {}) (empty-seq? \"\"))",
        "true true true true true\n",
    ),
    (
        "(defmacro units [x] (count (seq x))) (js/console.log (units \"😀\"))",
        "2\n",
    ),
];

fn program_output(js: &str) -> String {
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(js)
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
fn native_core_services_match_quickjs() {
    for (body, expected) in [
        (
            r#"[(mapv vec (partition-all 2 [0 1 2 3 4]))
                (mapv vec (partition-all 3 1 '(0 1 2 3)))
                (mapv vec (partition-all 2 3 [0 1 2 3 4]))
                (vec (partition-all 2 nil))
                (mapv vec (take 2 (partition-all 0 [1])))
                (let [p partition-all xf (p 2)] [(into [] xf [1 2 3]) (into [] xf [4 5 6])])
                (into [] (partition-all "2") [1 2 3])
                (into [] (partition-all 0) [1 2 3])]"#,
            "[[[0 1] [2 3] [4]] [[0 1 2] [1 2 3] [2 3] [3]] [[0 1] [3 4]] [] [[] []] [[[1 2] [3]] [[4 5] [6]]] [[1 2 3]] [[1 2 3]]]",
        ),
        (
            r#"[(into [] (remove identity) [nil false 0 1])
                (let [r remove] (into [] (r odd?) [0 1 2 3]))
                (let [seen (js/Array.) s (partition-all 2 (map (fn [x] (.push seen x) x) '(1 2 3)))
                      before (.-length seen) group (first s) after-group (.-length seen)
                      value (first group) after-value (.-length seen) all (mapv vec s)]
                  [before after-group value after-value all (vec seen)])
                (let [seen (js/Array.) result (transduce (partition-all 2)
                    (fn ([] []) ([a] (.push seen "complete") a) ([a x] (.push seen (count x)) (reduced (conj a x))))
                    [] [1 2 3 4 5])] [result (vec seen)])
                (let [xf (partition-all 3) f (xf conj) a (f [] 1) b (f a 2)] [(f b) (f [])])
                (let [p partition-all] (try (p) (catch :default e (.-message e))))]"#,
            "[[nil false] [0 2] [0 1 1 1 [[1 2] [3]] [1 2 3]] [[[1 2]] [2 \"complete\"]] [[[1 2]] []] \"Invalid arity: 0\"]",
        ),
        (
            r#"[(vec (interpose 0 [1 2 3])) (vec (interpose 0 nil)) (vec (interpose 0 [1]))
              (let [xf (interpose 0)] [(into [] xf [1 2]) (into [] xf [3 4])])
              (transduce (interpose :sep) (fn ([a] a) ([a x] (if (= x :sep) (reduced (conj a x)) (conj a x)))) [] [1 2 3])
              (let [a (js/Array.) s (interpose 0 (map (fn [x] (.push a x) x) '(1 2))) x (first s) n (.-length a) y (second s)] [x n y (.-length a)])]"#,
            "[[1 0 2 0 3] [] [1] [[1 0 2] [3 0 4]] [1 :sep] [1 1 0 2]]",
        ),
        (
            r#"[(not= 1 1.0) (not= 1 1 2) (not= nil false) (number? 0) (number? "0")
              (float? 1) (double? 1) (float? nil) (double? false)
              (vec (keep-indexed (fn [i x] (if (even? i) x nil)) [false :skip 2]))
              (into [] (keep-indexed (fn [i x] (if (even? i) [i x] nil))) [false :skip 2])
              (vec (keep-indexed (fn [i x] (if (> i 0) i nil)) '(:a :b :c)))]"#,
            "[false true true true false true true false false [false 2] [[0 false] [2 2]] [1 2]]",
        ),
        (
            // Discard a whole input chunk before producing anything, then
            // demand the second and third chunks repeatedly without replay.
            r#"(let [seen (js/Array.)
                      s (keep-indexed (fn [i x] (.push seen i) (if (>= i 32) [i x] nil)) (vec (range 65)))
                      before (.-length seen) a (first s) after-first (.-length seen)
                      again (first s) after-again (.-length seen)
                      n (count s) after-count (.-length seen)
                      repeated-n (count s) after-repeat (.-length seen)]
                  [before a after-first again after-again n after-count repeated-n after-repeat
                   (vec s) (.-length seen) (= (vec seen) (vec (range 65)))])"#,
            "[0 [32 32] 64 [32 32] 64 33 65 33 65 [[32 32] [33 33] [34 34] [35 35] [36 36] [37 37] [38 38] [39 39] [40 40] [41 41] [42 42] [43 43] [44 44] [45 45] [46 46] [47 47] [48 48] [49 49] [50 50] [51 51] [52 52] [53 53] [54 54] [55 55] [56 56] [57 57] [58 58] [59 59] [60 60] [61 61] [62 62] [63 63] [64 64]] 65 true]",
        ),
        (
            r#"[(replace {:a nil :b false} [:a :b :c]) (vec (replace {1 2} '(1 3)))
              (into [] (replace {1 2}) [1 3]) (meta (replace {1 2} (with-meta [1] {:tag true})))]"#,
            "[[nil false :c] [2 3] [2 3] {:tag true}]",
        ),
        (
            r#"[(seq-to-map-for-destructuring nil) (seq-to-map-for-destructuring [nil])
              (seq-to-map-for-destructuring [{:a 1}]) (seq-to-map-for-destructuring [:a 1 :b 2])
              (seq-to-map-for-destructuring [:a 1 {:b 2}]) (seq-to-map-for-destructuring [:a 1 nil])]"#,
            "[{} nil {:a 1} {:a 1, :b 2} {:a 1, :b 2} {:a 1}]",
        ),
        (
            r#"[(re-find #"a" "cat") (re-find #"z" "cat") (re-matches #"a" "cat")
              (re-matches #"(a)(b)" "ab") (regexp? (re-pattern "(?i)a"))
              (re-find (re-pattern "(?i)a") "A")
              (try (re-pattern 42) (catch :default e (.-message e)))
              (try (re-find #"a" nil) (catch :default e (.-message e)))]"#,
            "[\"a\" nil nil [\"ab\" \"a\" \"b\"] true \"A\" \"regex pattern requires a string\" \"regex matching requires a string\"]",
        ),
        (
            r#"[(/ 1 2) (infinite? (/ 1 0)) (int 4294967297.9) (long 4294967297.9)
             (byte 300) (short 65537) (float 16777217) (double 16777217)
             (quot 2 -5) (rem -0.0 2) (mod 4 -2) (== 1 1.0) (/ 1 (long -0.4))]"#,
            "[0.5 true 1 4294967297 300 65537 16777217 16777217 -0.0 -0.0 -0.0 true ##-Inf]",
        ),
        (
            r#"[(str 'hello 'a/b :x :a/b nil false 42 "😀") (name (gensym :prefix))]"#,
            "[\"helloa/b:x:a/bfalse42😀\" \":prefix1\"]",
        ),
        (r#"(vec (re-seq #"a" "banana"))"#, "[\"a\" \"a\" \"a\"]"),
        (
            r#"(mapv (fn [m] (mapv (fn [x] (if (nil? x) :missing x)) m)) (re-seq #"(a)(b)?" "a ab"))"#,
            "[[\"a\" \"a\" :missing] [\"ab\" \"a\" \"b\"]]",
        ),
        (
            r#"[(re-seq #"z" "abc") (vec (re-seq #"" "😀")) (vec (re-seq (js/RegExp. "a" "g") "a a a"))]"#,
            "[nil [\"\" \"\" \"\"] [\"a\" \"a\"]]",
        ),
        (
            r#"(let [seen (js/Array.) re (js* "({exec(text){ ~{}.push(text); return /a/.exec(text); }})" seen)
                      s (re-seq re "a a") a (first s) n (.-length seen) b (first (next s))]
                   [a n b (vec seen) (first (next s)) (.-length seen)])"#,
            "[\"a\" 1 \"a\" [\"a a\" \" a\"] \"a\" 2]",
        ),
        (
            r#"(let [x (with-meta 'props {:old true}) v vary-meta seen (js/Array.)
                     result (v x (fn [m k value] (.push seen (str k)) (assoc m k value)) :tag 'js)]
                [(= x result) (meta result) (meta x) (= (meta x) (meta (v x identity)))
                 (vary-meta nil (fn [m] (.push seen (nil? m)) {:tag true}))
                 (vec seen) (meta (vary-meta (with-meta 'props {:source true}) assoc :tag 'js))])"#,
            "[true {:old true, :tag js} {:old true} true nil [\":tag\" true] {:source true, :tag js}]",
        ),
        (
            r#"[(mapv true? [true false nil 0 1 "true" :true])
                (let [p true?] (p true))]"#,
            "[[true false false false false false false] true]",
        ),
        (
            r#"[(try (re-seq #"a" 42) (catch :default e (.-message e)))
             (try (let [g gensym] (g "a" "b")) (catch :default e (.-message e)))]"#,
            "[\"regex matching requires a string\" \"Invalid arity: 2\"]",
        ),
    ] {
        let source = format!("(defmacro m [] (list 'quote {body})) (js/console.log (pr-str (m)))");
        assert_native_matches_legacy(&source, &format!("{expected}\n"));
    }
}

#[test]
fn native_reduce_kv_matches_quickjs() {
    // Each row expands a source macro, so the same table compares values,
    // errors, effects, occurrences and callback ABI against the legacy host.
    for (label, declarations, body, argument) in [
        (
            "nil and empty",
            "",
            r#"(let [seed (with-meta [] {:seed true}) f (fn [& _] (throw (js/Error. "called")))]
            [(identical? seed (reduce-kv f seed nil)) (identical? seed (reduce-kv f seed {}))
             (identical? seed (reduce-kv f seed [])) (meta (reduce-kv f seed nil))
             (reduced? (reduce-kv f (reduced :seed) {}))])"#,
            "nil",
        ),
        (
            "vector indices",
            "",
            r#"(let [f reduce-kv v (vec (range 67))]
            [(f (fn [a k v] (conj a [k v])) [] [:a nil false])
             (f (fn [a k v] (+ a (* k v))) 0 v) (count v) (last v)])"#,
            "nil",
        ),
        (
            "array maps and positional duplicates",
            "",
            r#"(mapv (fn [m] (reduce-kv (fn [a k v] (conj a [k v])) [] m))
            [(array-map :z 0 :a 1) '{1 :first 1.0 :second} '{\a 1 "a" 2}
             (array-map \a 1 "a" 2) (assoc '{1 :first 1.0 :second} 2 :last)])"#,
            "nil",
        ),
        (
            "hash paths and nil key",
            "",
            r#"(let [m (apply hash-map (mapcat (fn [i] [(str "k" i) i]) (range 40)))
            m (assoc m nil :nil :keyword :value)]
            [(reduce-kv (fn [a k v] (conj a [k v])) [] m) (count m)])"#,
            "nil",
        ),
        (
            "reader map and metadata",
            "",
            r#"(let [k (with-meta 'k {:key true}) v (with-meta [1] {:value true})
            m (with-meta (array-map k v) {:map true}) seed (with-meta [] {:seed true})
            result (reduce-kv (fn [a key value] (conj a [(identical? k key) (identical? v value) (meta key) (meta value)])) seed m)]
            [result (meta result) (meta m) (reduce-kv (fn [a k v] (conj a [k v])) [] input)])"#,
            "^{:input true} {:z 1 :a 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}",
        ),
        (
            "records and hashed extensions",
            "(defrecord R [z a])",
            r#"(let [r (with-meta (assoc (R. 1 2) :b 3 nil 4 :c 5 :d 6 :e 7 :f 8 :g 9 :h 10 :i 11) {:record true})]
            [(reduce-kv (fn [a k v] (conj a [k v])) [] r) (meta r) (count r)])"#,
            "nil",
        ),
        (
            "records and positional extensions",
            "(defrecord R [a])",
            r#"(let [r (map->R '{:a 0 1 :first 1.0 :second})]
            [(reduce-kv (fn [a k v] (conj a [k v])) [] r) (count r)])"#,
            "nil",
        ),
        (
            "mutable record",
            "(defrecord R [^:mutable a])",
            r#"(let [r (R. 1 nil {:z 2})]
            (reduce-kv (fn [a k v] (conj a [k v])) [] r))"#,
            "nil",
        ),
        (
            "reduced and thrown callbacks",
            "",
            r#"(let [seen (js/Array.)
            f (fn [a k v] (.push seen (str k ":" v)) (reduced (conj a v)))
            result (mapv (fn [c] (reduce-kv f [] c)) [[1 2] (array-map :a 3 :b 4) (apply hash-map [nil 5 :a 6])])
            nested (reduce-kv (fn [a k v] (reduced (reduced v))) nil [7 8])
            error (try (reduce-kv (fn [a k v] (.push seen "throw") (throw (js/Error. "callback"))) nil [:x :y])
                    (catch :default e (.-message e)))]
            [result (vec seen) (reduced? nested) (unreduced nested) error])"#,
            "nil",
        ),
        (
            "callback receiver and getter",
            "",
            r#"(let [seen (js/Array.)
            f (js* "({get call(){const target=this;~{}.push('get'); return function(receiver,a,k,v){~{}.push(this===target,receiver===null,v);return a+v;};}})" seen seen)
            result (reduce-kv f 0 (array-map :a 1 :b 2))]
            [result (vec seen)])"#,
            "nil",
        ),
        (
            "lazy map entries stop before tail",
            "",
            r#"(let [seen (js/Array.)
            entries (map (fn [e] (.push seen (str (key e))) e) (seq (array-map :a 1 :b 2 :c 3)))
            result (reduce-kv (fn [a k v] (reduced [k v])) nil entries)]
            [result (vec seen)])"#,
            "nil",
        ),
        (
            "map entry and invalid entries",
            "",
            r#"(mapv (fn [c] (try (reduce-kv (fn [a k v] (conj a [k v])) [] c)
            (catch :default e (.-message e))))
            [(first (seq {:a 1})) (list [:a 1]) (list nil) (list 1) [] nil])"#,
            "nil",
        ),
        (
            "entry protocol fallback",
            "(deftype E [k v] IMapEntry (-key [_] k) (-val [_] v))",
            r#"(reduce-kv (fn [a k v] (conj a [k v])) [] (list (E. :a 1) (E. :b 2)))"#,
            "nil",
        ),
        (
            "invalid collections and callbacks",
            "",
            r#"[(mapv (fn [c] (try (reduce-kv (fn [a k v] a) [] c) (catch :default e (.-message e))))
                     [42 true :keyword 'symbol (js/Object.)])
                 (mapv (fn [c] (try (reduce-kv nil :seed c) (catch :default e (.-name e)))) [nil [] {} [1] {:a 1}])
                 (reduce-kv (fn [a k v] (conj a [k v])) [] (js/Array.))]"#,
            "nil",
        ),
        (
            "seq protocol fallback",
            "(deftype S [] ISeqable (-seq [_] (seq (array-map :b 2 :a 1))))",
            "(reduce-kv (fn [a k v] (conj a [k v])) [] (S.))",
            "nil",
        ),
        (
            "kv protocol override",
            "(deftype R [] IKVReduce (-kv-reduce [this f init] (f init :custom 42)))",
            r#"[(reduce-kv (fn [a k v] [a k v]) :init (R.))
            (reduced? (reduce-kv (fn [a k v] (reduced v)) nil (R.)))
            (-kv-reduce (R.) (fn [a k v] [a k v]) :init)]"#,
            "nil",
        ),
        (
            "sorted maps retain order through updates",
            "",
            r#"(let [m (with-meta (apply sorted-map (mapcat (fn [i] [i (- i)]) (reverse (range 12)))) {:sorted true})
                     updated (dissoc (assoc m 4 :updated 20 :new) 0)
                     result (reduce-kv (fn [a k v] (conj a [k v])) [] updated)]
                 [result (sorted? m) (sorted? updated) (meta updated) (get updated 4) (count m)
                  (reduce-kv (fn [a k v] (reduced [k v])) nil m)
                  (reduce-kv (fn [a k v] (conj a [k v])) [] (sorted-map :z 1 :a 2 :n))])"#,
            "nil",
        ),
        (
            "sorted native key types",
            "",
            r#"(mapv (fn [m] (reduce-kv (fn [a k v] (conj a [k v])) [] m))
                [(sorted-map :z 1 nil 2 :a/x 3 :a/y 4 :a 5)
                 (sorted-map 'b/x 1 'a/y 2 'a 3)
                 (sorted-map [1 2] :a [0] :b [] :c [1 1] :d)
                 (sorted-map "😀" 1 "\uE000" 2 "a" 3)
                 (sorted-map true 1 false 2 nil 3)])"#,
            "nil",
        ),
        (
            "sorted comparison reuses equal shared vector prefixes",
            "",
            r#"(let [build (fn [] (loop [i 0 v 0] (if (= i 10) v (recur (inc i) [v v]))))
                     a (build) b (build) m (sorted-map a :first b :second)
                     ordered (sorted-map [b 2] :last [a 1] :first)]
              [(count m) (get m b) (identical? a (first (keys m)))
               (get ordered [a 1]) (get ordered [b 2])
               (try (sorted-map [a 1] :a [b "x"] :b)
                    (catch :default e [(.-name e) (.-message e)]))])"#,
            "nil",
        ),
        (
            "sorted comparison errors are catchable and run finally",
            "",
            r#"(let [seen (js/Array.)
                probe (fn [f] (try (f) (catch :default e [(.push seen "catch") (.-name e) (.-message e)])
                                   (finally (.push seen "finally"))))]
              [(mapv probe [(fn [] (sorted-map 1 :a "x" :b))
                            (fn [] (sorted-map :a 1 'b 2))
                            (fn [] (sorted-map [1] :a ["x"] :b))
                            (fn [] (get (sorted-map 1 :a) "x"))
                            (fn [] (assoc (sorted-map "x" :a) 1 :b))
                            (fn [] (dissoc (sorted-map 1 :a) "x"))
                            (fn [] (assoc (sorted-map :k (apply hash-map ["x" :v])) :k (sorted-map 1 :v)))
                            (fn [] (sorted-map :k (apply hash-map ["x" :v]) :k (sorted-map 1 :v)))])
               (vec seen)])"#,
            "nil",
        ),
        (
            "structural key operations propagate nested sorted comparison errors",
            "(defrecord KeyRecord [])",
            r#"(let [stored (sorted-map 1 :v) query (array-map "x" :v)
                     m (array-map stored :stored)
                     probe (fn [f] (try (f) (catch :default e [(.-name e) (.-message e)])))]
              (mapv probe [(fn [] (get m query :missing))
                           (fn [] (contains? m query))
                           (fn [] (assoc m query :new))
                           (fn [] (dissoc m query))
                           (fn [] (array-map stored :stored query :new))
                           (fn [] (contains? #{stored} query))
                           (fn [] (get (assoc (KeyRecord.) stored :stored) query :missing))]))"#,
            "nil",
        ),
        (
            "sorted comparison messages preserve readable collection operands",
            "",
            r#"(let [message (fn [k] (try (get (sorted-map 1 :v) k)
                                         (catch :default e [(.-name e) (.-message e)])))]
              (mapv message [["\b\f\n\r\t\"\\"] (list "\b" "\f")
                             (array-map :a [1 nil] :b #{2 3})
                             (apply hash-map [33 "a" 1 "b" 32 "c" 0 "d" nil "nil"])
                             (hash-set 33 1 32 0 nil 100 101 102 103)
                             (array-map :ns/a 1 :ns/b ["\b\f"])
                             #{:a :b} [##NaN ##Inf ##-Inf -0.0]
                             [(array-map :x #{"\b\f"})]]))"#,
            "nil",
        ),
        (
            "sorted comparison messages honor collection printing bindings",
            "",
            r#"(let [message (fn [k] (try (get (sorted-map 1 :v) k)
                                         (catch :default e [(.-name e) (.-message e)])))
                     values [[1 2 3] (list 1 2 3) #{1 2 3}
                             (array-map :ns/a [1 2] :ns/b [[3]])
                             (apply hash-map [:ns/a [1 2] :ns/b [[3]]])
                             (array-map 'ns/a 1 :ns/b 2)
                             (array-map :ns/a 1 :other/b 2) [] {}]]
              [(binding [*print-length* 1] (mapv message values))
               (binding [*print-length* 0] (mapv message values))
               (binding [*print-level* 1] (mapv message values))
               (binding [*print-level* 0] (mapv message values))
               (binding [*print-namespace-maps* true] (mapv message values))
               (binding [*print-length* 1 *print-level* 2 *print-namespace-maps* true]
                 (mapv message values))
               (binding [*print-readably* false] (message ["quoted" "\n"]))
               (message [1 2 3])])"#,
            "nil",
        ),
        (
            "sorted comparison messages retain exact UTF16 units",
            "",
            r#"(let [units (fn [k] (try (get (sorted-map 1 :v) k)
                                       (catch :default e
                                         (let [m (.-message e)]
                                           (mapv (fn [i] (.charCodeAt m i)) (range (.-length m)))))))
                     hi (js/String.fromCharCode 55296) lo (js/String.fromCharCode 56320)]
              (mapv units [hi lo [hi "\b\f" lo] (array-map :a hi) #{lo}]))"#,
            "nil",
        ),
        (
            "sorted comparison messages retain complete large nested operands",
            "",
            r#"(let [large (apply str (repeat 4096 "x"))
                     nested (loop [i 0 k 0] (if (= i 10) k (recur (inc i) [k k])))
                     probe (fn [k] (try (sorted-map 1 :a k :b)
                                        (catch :default e [(.-name e) (.-message e)])))]
              [(probe large) (probe [large "\b\f"]) (probe nested)
               (probe (array-map :large [large] :nested nested))])"#,
            "nil",
        ),
        (
            "sorted equality uses destination comparison and source value direction",
            "",
            r#"(let [s (sorted-map 1 :v) n (sorted-map ##NaN :v) h (apply hash-map [##NaN :v])
                     p (apply hash-map [1 :v]) left #{1 1.0} right #{1 2}
                     check (fn [a b] (try (= a b) (catch :default e [(.-name e) (.-message e)])))]
              [(check s n) (check n s) (check s h) (check h s) (check s p) (check p s)
               (check (sorted-map 1 left) (array-map 1 right))
               (check (array-map 1 left) (sorted-map 1 right))
               (check (sorted-map 1 :v) (apply hash-map ["x" :v]))
               (check (apply hash-map ["x" :v]) (sorted-map 1 :v))
               (check [(apply hash-map ["x" :v])] [(sorted-map 1 :v)])])"#,
            "nil",
        ),
        (
            "default extensions remain below owned core implementations",
            r#"(deftype R [] IKVReduce (-kv-reduce [_ f init] (f init :own 7)))
               (defrecord D [x])
               (defrecord M [^:mutable x])
               (deftype E [] IMapEntry (-key [_] :own) (-val [_] 8))
               (extend-protocol IKVReduce default (-kv-reduce [_ f init] (f init :default 9)))
               (extend-protocol IMapEntry default (-key [_] :default) (-val [_] 10))"#,
            r#"[(mapv (fn [c] (reduce-kv (fn [a k v] (conj a [k v])) [] c))
                   [[1] (array-map :a 2) (apply hash-map [:b 3]) (sorted-map :c 4) (R.) (D. 5) (M. 6) (js/Object.)])
                (mapv (fn [e] [(key e) (val e)]) [(first (seq {:a 1})) (E.) (js/Object.)])
                (-kv-reduce [2] (fn [a k v] (+ a v)) 0)]"#,
            "nil",
        ),
        (
            "object extensions remain below owned core implementations",
            r#"(extend-protocol IKVReduce object (-kv-reduce [_ f init] (f init :object 9)))
               (extend-protocol IMapEntry object (-key [_] :object) (-val [_] 10))"#,
            r#"[(mapv (fn [c] (reduce-kv (fn [a k v] (conj a [k v])) [] c))
                   [[1] (array-map :a 2) (apply hash-map [:b 3]) (sorted-map :c 4) (js/Object.)])
                (mapv (fn [e] [(key e) (val e)]) [(first (seq {:a 1})) (js/Object.)]) ]"#,
            "nil",
        ),
        (
            "explicit native type extension precedes owned reducer",
            "(extend-type PersistentVector IKVReduce (-kv-reduce [_ f init] (f init :vector 11)))",
            "[(reduce-kv (fn [a k v] [a k v]) :seed [1]) (-kv-reduce [1] (fn [a k v] [a k v]) :seed)]",
            "nil",
        ),
        (
            "self returning custom sequences reduce with rest and next effects",
            r#"(deftype S [entries seen]
                 ISeqable (-seq [this] (.push seen "seq") this)
                 ISeq (-first [_] (.push seen "first") (first entries))
                      (-rest [_] (.push seen "rest") (rest entries)))
               (deftype N [entries seen]
                 ISeqable (-seq [this] (.push seen "seq") this)
                 ISeq (-first [_] (.push seen "first") (first entries))
                      (-rest [_] (throw (js/Error. "rest called")))
                 INext (-next [_] (.push seen "next") (next entries)))
               (deftype Q [source seen] ISeqable (-seq [_] (.push seen "outer") source))"#,
            r#"(let [seen (js/Array.) entries (seq (array-map :a 1 :b 2))
                     s (S. entries seen) n (N. entries seen)
                     result [(reduce-kv (fn [a k v] (.push seen "kv") (conj a [k v])) [] (Q. s seen))
                             (reduce-kv (fn [a k v] (.push seen "stop") (reduced [k v])) nil s)
                             (reduce (fn [a e] (.push seen "reduce") (conj a [(key e) (val e)])) [] n)
                             (reduce (fn [a e] (.push seen "two") [a (val e)]) s)
                             (reduce (fn [a e] (.push seen "two-next") [a (val e)]) n)
                             (reduce-kv (fn [a k v] (conj a [k v])) [] n)]]
               [result (vec seen)])"#,
            "nil",
        ),
        (
            "sequence advancement ignores default next extensions",
            r#"(deftype S [entries seen]
                 ISeqable (-seq [this] (.push seen "seq") this)
                 ISeq (-first [_] (.push seen "first") (first entries))
                      (-rest [_] (.push seen "rest") (rest entries)))
               (extend-protocol INext default (-next [_] (throw (js/Error. "default next called"))))"#,
            r#"(let [seen (js/Array.) s (S. (seq (array-map :a 1)) seen)
                     numbers (S. (seq [1 2 3]) seen)
                     result [(reduce-kv (fn [a k v] (.push seen "kv") (conj a [k v])) [] s)
                             (reduce (fn [a e] (.push seen "reduce") (conj a (val e))) [] s)
                             (reduce (fn [a v] (.push seen "two") (+ a v)) numbers)]]
               [result (vec seen)])"#,
            "nil",
        ),
        (
            "nullish seq protocol results terminate both reduce arities and kv fallback",
            r#"(deftype S [end seen]
                 ISeqable (-seq [_] (.push seen "seq") end))"#,
            r#"(mapv (fn [end]
                (let [seen (js/Array.) s (S. end seen)
                      f (fn ([] (.push seen "empty") :empty)
                            ([a x] (.push seen "step") (conj a x)))
                      result [(reduce f s) (reduce f [] s)
                              (reduce-kv (fn [a k v] (.push seen "kv") (conj a [k v])) [] s)]]
                  [result (vec seen)])) [nil js/undefined])"#,
            "nil",
        ),
        (
            "nullish next protocol results terminate both reduce arities and kv fallback",
            r#"(deftype S [value end seen]
                 ISeqable (-seq [this] (.push seen "seq") this)
                 ISeq (-first [_] (.push seen "first") value)
                      (-rest [_] (throw (js/Error. "rest called")))
                 INext (-next [_] (.push seen "next") end))"#,
            r#"(mapv (fn [end]
                (let [seen (js/Array.) s (S. 1 end seen)
                      f (fn ([] (.push seen "empty") :empty)
                            ([a x] (.push seen "step") (conj a x)))
                      result [(reduce f s) (reduce f [] s)
                              (reduce-kv (fn [a k v] (.push seen "kv") (conj a [k v])) []
                                         (S. (first (seq {:a 1})) end seen))]]
                  [result (vec seen)])) [nil js/undefined])"#,
            "nil",
        ),
        (
            "nullish rest protocol results terminate without INext",
            r#"(deftype S [value end seen]
                 ISeqable (-seq [this] (.push seen "seq") this)
                 ISeq (-first [_] (.push seen "first") value)
                      (-rest [_] (.push seen "rest") end))"#,
            r#"(mapv (fn [end]
                (let [seen (js/Array.) s (S. 1 end seen)
                      result [(reduce + s) (reduce conj [] s)
                              (reduce-kv (fn [a k v] (conj a [k v])) []
                                         (S. (first (seq {:a 1})) end seen))]]
                  [result (vec seen)])) [nil js/undefined])"#,
            "nil",
        ),
        (
            "arrays read replacement elements and current length during reduction",
            "",
            r#"(let [run (fn [mutate]
                          (let [xs (js/Array. 1 2 3)
                                result (reduce (fn [a x] (mutate xs x) (conj a (if (nil? x) :hole x))) [] xs)]
                            [result (.-length xs)]))]
              (mapv run [(fn [xs x] (aset xs 1 9))
                         (fn [xs x] (if (= x 1) (.push xs 4) nil))
                         (fn [xs x] (if (= x 1) (.pop xs) nil))
                         (fn [xs x] (if (= x 1) (set! (.-length xs) 1) nil))
                         (fn [xs x] (if (= x 1) (set! (.-length xs) 5) nil))]))"#,
            "nil",
        ),
        (
            "two argument reduce reads arrays incrementally",
            "",
            r#"(let [run (fn [mutate]
                          (let [xs (js/Array. 1 2 3) seen (js/Array.)
                                result (reduce (fn [a x] (.push seen (if (nil? x) "hole" x)) (mutate xs x)
                                                 (+ a (if (nil? x) 0 x))) xs)]
                            [result (vec seen) (.-length xs)]))]
              (mapv run [(fn [xs x] (aset xs 2 9))
                         (fn [xs x] (if (= x 2) (.push xs 4) nil))
                         (fn [xs x] (if (= x 2) (.pop xs) nil))
                         (fn [xs x] (if (= x 2) (set! (.-length xs) 1) nil))
                         (fn [xs x] (if (= x 2) (set! (.-length xs) 5) nil))]))"#,
            "nil",
        ),
        (
            "kv fallback reads mutated array entries and length",
            "",
            r#"(let [entries (vec (seq (array-map :a 1 :b 2 :c 3 :d 9)))
                     run (fn [mutate]
                           (let [xs (js* "[~{},~{},~{}]" (nth entries 0) (nth entries 1) (nth entries 2))
                                 result (reduce-kv (fn [a k v] (if (= k :a) (mutate xs (nth entries 3)) nil)
                                                    (conj a [k v])) [] xs)]
                             [result (.-length xs)]))]
              (mapv run [(fn [xs entry] (js* "(~{}[1]=~{})" xs entry))
                         (fn [xs entry] (js* "~{}.push(~{})" xs entry))
                         (fn [xs entry] (.pop xs))
                         (fn [xs entry] (set! (.-length xs) 1))]))"#,
            "nil",
        ),
        (
            "array element getters stay incremental and stop at reduced",
            "",
            r#"(let [seen (js/Array.) xs (js* "Object.defineProperty([1,2],1,{get(){~{}.push('read');throw new Error('tail');}})" seen)
                     stopped (reduce (fn [a x] (.push seen "step") (reduced x)) nil xs)
                     error (try (reduce (fn [a x] (.push seen "step") x) nil xs)
                                (catch :default e (.-message e)))]
              [stopped error (vec seen)])"#,
            "nil",
        ),
        (
            "reducer call lookup precedes sequence first effects and exceptions",
            r#"(deftype S [values seen fail]
                 ISeqable (-seq [this] (.push seen "seq") this)
                 ISeq (-first [_] (.push seen "first")
                                 (if fail (throw (js/Error. "first failed")) (first values)))
                      (-rest [_] (throw (js/Error. "rest called")))
                 INext (-next [_] (.push seen "next")
                                 (let [tail (next values)] (if tail (S. tail seen fail) nil))))"#,
            r#"(let [run (fn [mode two]
                          (let [seen (js/Array.)
                                f (js* "({get call(){const target=this;~{}.push('get');if(~{}==='get-throws')throw new Error('get failed');return function(receiver,a,x){~{}.push(this===target,receiver===null,'step');return a+x;};}})" seen mode seen)
                                s (S. '(1 2) seen (= mode "first-throws"))
                                result (try (if two (reduce f s) (reduce f 0 s))
                                            (catch :default e (.-message e)))]
                            [result (vec seen)]))]
              [(run "success" false) (run "success" true)
               (run "get-throws" false) (run "get-throws" true)
               (run "first-throws" false)])"#,
            "nil",
        ),
        (
            "call getter effects are visible to sequence first",
            r#"(deftype S [seen]
                 ISeqable (-seq [this] this)
                 ISeq (-first [_] (.push seen "first") (.-length seen))
                      (-rest [_] ())
                 INext (-next [_] nil))"#,
            r#"(let [seen (js/Array.)
                     f (js* "({get call(){~{}.push('get');return function(_,a,x){return a+x;};}})" seen)
                     result (reduce f 0 (S. seen))]
              [result (vec seen)])"#,
            "nil",
        ),
        (
            "array reads precede reducer call lookup for both arities",
            "",
            r#"(mapv (fn [two]
                (let [seen (js/Array.)
                      xs (js* "Object.defineProperties([1,2],{0:{get(){~{}.push('read0');return 1;}},1:{get(){~{}.push('read1');return 2;}}})" seen seen)
                      f (js* "({get call(){const target=this;~{}.push('get');return function(receiver,a,x){~{}.push(this===target,receiver===null,'step');return a+x;};}})" seen seen)
                      result (if two (reduce f xs) (reduce f 0 xs))]
                  [result (vec seen)])) [false true])"#,
            "nil",
        ),
        (
            "protocol returned arrays retain sequence reduction lookup order",
            r#"(deftype Q [tail] ISeqable (-seq [_] tail))
               (deftype S [head tail]
                 ISeqable (-seq [this] this)
                 ISeq (-first [_] head) (-rest [_] tail))"#,
            r#"(let [run (fn [wrap]
                          (let [seen (js/Array.)
                                tail (js* "Object.defineProperty([2,3],0,{get(){~{}.push('read');return 2;}})" seen)
                                f (js* "({get call(){~{}.push('get');return function(_,a,x){return a+x;};}})" seen)
                                result (reduce f 0 (wrap tail))]
                            [result (vec seen)]))
                     entry (first (seq {:a 1}))]
              [(run (fn [tail] (Q. tail))) (run (fn [tail] (S. 1 tail)))
               (reduce-kv (fn [a k v] (conj a [k v])) [] (Q. (js* "[~{}]" entry)))])"#,
            "nil",
        ),
        (
            "array bounds are rechecked after sequence reducer call getters",
            r#"(deftype Q [tail] ISeqable (-seq [_] tail))
               (deftype S [head tail]
                 ISeqable (-seq [this] this)
                 ISeq (-first [_] head) (-rest [_] tail))"#,
            r#"(let [run (fn [wrap two size fail]
                          (let [seen (js/Array.) xs (js/Array. 1 2)
                                f (js* "({calls:0,get call(){~{}.push('get');if(++this.calls===~{})~{}.length=~{};return function(_,a,x){~{}.push(x===null?'null':x===undefined?'undefined':x);if(~{}&&x===null)throw new Error('nil head');return a;};}})" seen (if two 1 2) xs size seen fail)
                                result (try (if two (reduce f (wrap xs)) (reduce f :seed (wrap xs)))
                                            (catch :default e (.-message e)))]
                            [result (vec seen) (.-length xs)]))]
              (mapv (fn [wrap]
                      (mapv (fn [two]
                              [(run wrap two 0 false) (run wrap two 1 false)
                               (run wrap two 4 false) (run wrap two 0 true)]) [false true]))
                    [identity (fn [tail] (Q. tail)) (fn [tail] (S. 9 tail))]))"#,
            "nil",
        ),
        (
            "array reduction shapes preserve index length and has trap order",
            r#"(deftype Q [tail] ISeqable (-seq [_] tail))
               (deftype S [head tail]
                 ISeqable (-seq [this] this)
                 ISeq (-first [_] head) (-rest [_] tail))"#,
            r#"(let [run (fn [proxy wrap two mode]
                          (let [seen (js/Array.)
                                xs (js* "(()=>{const seen=~{},xs=[1,2,3];if(~{})return new Proxy(xs,{get(t,k,r){if(k==='length'||/^[0-9]+$/.test(String(k)))seen.push('get:'+String(k));return Reflect.get(t,k,r);},has(t,k){seen.push('has:'+String(k));return Reflect.has(t,k);}});for(let i=0;i<3;i++){let value=xs[i];Object.defineProperty(xs,i,{configurable:true,get(){seen.push('get:'+i);return value;},set(x){value=x;}});}return xs;})()" seen proxy)
                                f (js* "(()=>{const seen=~{},xs=~{},mode=~{};let calls=0;const step=function(_,a,x){seen.push('step',x===null?'null':x===undefined?'undefined':x);return a;};if(mode==='plain')return function(a,x){return step(null,a,x);};return {get call(){seen.push('call');if(++calls===1){if(mode==='replace')xs[1]=9;if(mode==='push')xs.push(4);if(mode==='pop')xs.pop();if(mode==='clear')xs.length=0;if(mode==='grow')xs.length=5;if(mode==='delete')delete xs[1];if(mode==='throw')throw new Error('call failed');}return step;}};})()" seen xs mode)
                                result (try (if two (reduce f (wrap xs)) (reduce f :seed (wrap xs)))
                                            (catch :default e (.-message e)))]
                            [result (vec seen)]))]
              (mapv (fn [proxy]
                      (mapv (fn [wrap]
                              (mapv (fn [two]
                                      (mapv (fn [mode] (run proxy wrap two mode))
                                            ["plain" "get" "replace" "push" "pop" "clear" "grow" "delete" "throw"]))
                                    [false true]))
                            [identity (fn [tail] (Q. tail)) (fn [tail] (S. 9 tail))]))
                    [false true]))"#,
            "nil",
        ),
        (
            "array kv fallback preserves index length and has trap order",
            r#"(deftype Q [tail] ISeqable (-seq [_] tail))
               (deftype S [head tail]
                 ISeqable (-seq [this] this)
                 ISeq (-first [_] head) (-rest [_] tail))"#,
            r#"(let [entries (vec (seq (array-map :a 1 :b 2 :c 3 :d 9)))
                     run (fn [proxy wrap mode]
                           (let [seen (js/Array.)
                                 xs (js* "(()=>{const seen=~{},xs=[~{},~{},~{}];if(~{})return new Proxy(xs,{get(t,k,r){if(k==='length'||/^[0-9]+$/.test(String(k)))seen.push('get:'+String(k));return Reflect.get(t,k,r);},has(t,k){seen.push('has:'+String(k));return Reflect.has(t,k);}});for(let i=0;i<3;i++){let value=xs[i];Object.defineProperty(xs,i,{configurable:true,get(){seen.push('get:'+i);return value;},set(x){value=x;}});}return xs;})()" seen (nth entries 0) (nth entries 1) (nth entries 2) proxy)
                                 f (js* "(()=>{const seen=~{},xs=~{},mode=~{},entry=~{};let calls=0;const step=function(_,a,k,v){seen.push('step',v);return a;};if(mode==='plain')return function(a,k,v){return step(null,a,k,v);};return {get call(){seen.push('call');if(++calls===1){if(mode==='replace')xs[1]=entry;if(mode==='push')xs.push(entry);if(mode==='pop')xs.pop();if(mode==='clear')xs.length=0;if(mode==='grow')xs.length=5;if(mode==='delete')delete xs[1];if(mode==='throw')throw new Error('call failed');}return step;}};})()" seen xs mode (nth entries 3))
                                 result (try (reduce-kv f :seed (wrap xs))
                                             (catch :default e (.-message e)))]
                             [result (vec seen)]))]
              (mapv (fn [proxy]
                      (mapv (fn [wrap]
                              (mapv (fn [mode] (run proxy wrap mode))
                                    ["plain" "get" "replace" "push" "pop" "clear" "grow" "delete" "throw"]))
                            [identity (fn [tail] (Q. tail)) (fn [tail] (S. (nth entries 3) tail))]))
                    [false true]))"#,
            "nil",
        ),
        (
            "stored collection seq overrides read heads before reducer call lookup",
            r#"(deftype S [n seen fail]
                 ISeqable (-seq [this] (.push seen "inner seq") this)
                 ISeq (-first [_] (.push seen "first")
                                 (if fail (throw (js/Error. "first failed")) n))
                      (-rest [_] (throw (js/Error. "rest called")))
                 INext (-next [_] (.push seen "next") (if (= n 1) (S. 2 seen fail) nil)))"#,
            r#"(let [run (fn [c two mode]
                          (let [seen (js/Array.) fail (contains? #{"first-throws" "both-throw"} mode)
                                f (js* "({get call(){~{}.push('get');if(~{})throw new Error('get failed');return function(_,a,x){~{}.push('step');return a+x;};}})" seen (contains? #{"get-throws" "both-throw"} mode) seen)]
                            (extend-type PersistentVector ISeqable (-seq [_] (.push seen "seq") (S. 1 seen fail)))
                            (extend-type PersistentArrayMap ISeqable (-seq [_] (.push seen "seq") (S. 1 seen fail)))
                            (extend-type PersistentHashMap ISeqable (-seq [_] (.push seen "seq") (S. 1 seen fail)))
                            (let [result (try (if two (reduce f c) (reduce f 0 c))
                                              (catch :default e (.-message e)))]
                              [result (vec seen)])))]
              (mapv (fn [c]
                      (mapv (fn [[two mode]] (run c two mode))
                            (list [false "success"] [true "success"]
                                  [false "get-throws"] [true "get-throws"]
                                  [false "first-throws"] [true "first-throws"]
                                  [false "both-throw"] [true "both-throw"])))
                    (list [1 2] (vec (range 34))
                          (array-map :a 1 :b 2) (apply hash-map (range 20)))))"#,
            "nil",
        ),
        (
            "list and set seq overrides preserve legacy reducer lookup order",
            r#"(deftype S [n seen fail]
                 ISeqable (-seq [this] this)
                 ISeq (-first [_] (.push seen "first")
                                 (if fail (throw (js/Error. "first failed")) n))
                      (-rest [_] (throw (js/Error. "rest called")))
                 INext (-next [_] (.push seen "next") (if (= n 1) (S. 2 seen fail) nil)))"#,
            r#"(let [run (fn [c two mode]
                          (let [seen (js/Array.) fail (contains? #{"first-throws" "both-throw"} mode)
                                f (js* "({get call(){~{}.push('get');if(~{})throw new Error('get failed');return function(_,a,x){~{}.push('step');return a+x;};}})" seen (contains? #{"get-throws" "both-throw"} mode) seen)]
                            (extend-type List ISeqable (-seq [_] (.push seen "seq") (S. 1 seen fail)))
                            (extend-type PersistentHashSet ISeqable (-seq [_] (.push seen "seq") (S. 1 seen fail)))
                            (let [result (try (if two (reduce f c) (reduce f 0 c))
                                              (catch :default e (.-message e)))]
                              [result (vec seen)])))]
              (mapv (fn [c]
                      (mapv (fn [[two mode]] (run c two mode))
                            [[false "success"] [true "success"]
                             [false "get-throws"] [true "get-throws"]
                             [false "first-throws"] [true "first-throws"]
                             [false "both-throw"] [true "both-throw"]]))
                    [(list 1 2) #{1 2}]))"#,
            "nil",
        ),
        (
            "direct kv protocol rejects non implementations",
            "(defrecord R [x])",
            r#"[(mapv (fn [c] (try (-kv-reduce c (fn [a k v] (conj a [k v])) [])
                                   (catch :default e [(.-name e) (.-message e)])))
                     [nil () (list (first (seq {:a 1}))) (js/Array.) (first (seq {:a 1}))
                      42 "abc" #{1} [2] {:a 1} (R. 3)])
                (reduce-kv (fn [a k v] (conj a [k v])) [] nil)]"#,
            "nil",
        ),
        (
            "initial and source stay persistent",
            "",
            r#"(let [m (with-meta (array-map :a 1 :b 2) {:tag true}) original m
            result (reduce-kv (fn [a k v] (dissoc a k)) m m)]
            [(identical? m original) (count original) result (meta result) (meta original)])"#,
            "nil",
        ),
    ] {
        let source = format!(
            "{declarations} (defmacro probe [input] (list 'quote {body})) (js/console.log (pr-str (probe {argument})))"
        );
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        let native = emit_js(&program);
        let legacy = light_cljs::compile(&source).unwrap_or_else(|e| panic!("{label}: {e}"));
        if label == "reader map and metadata" {
            // As in native_positional_literal_matrix_matches_legacy, imported
            // reader maps retain source positions in the native bridge. The
            // legacy bridge promotes this nine-entry input to a hash map.
            assert_eq!(
                program_output(&native),
                "[[[true true {:key true} {:value true}]] {:seed true} {:map true} [[:z 1] [:a 2] [:b 3] [:c 4] [:d 5] [:e 6] [:f 7] [:g 8] [:h 9]]]\n"
            );
            #[cfg(feature = "quickjs")]
            assert_eq!(
                program_output(&legacy),
                "[[[true true {:key true} {:value true}]] {:seed true} {:map true} [[:e 6] [:z 1] [:g 8] [:c 4] [:h 9] [:b 3] [:d 5] [:f 7] [:a 2]]]\n"
            );
            continue;
        }
        assert_eq!(program_output(&native), program_output(&legacy), "{label}");
        assert_eq!(native, legacy, "{label}: expansion");
    }
}

#[test]
fn rumext_property_filters_resume_reduce_kv_callbacks() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("native-reduce-kv");
    std::fs::write(
        root.join("filters.cljs"),
        // Property filters reduced from rumext.v2. Each callback demands a
        // cold helper after recording its input, so VM suspension cannot
        // replay effects or lose the reduction's persistent accumulator.
        r#"(ns filters)
        (defn qualified [k] (qualified-keyword? k))
        (defn empty-value [v] (nil? v))
        (defn without-qualified [data seen]
          (reduce-kv (fn [data k _]
                       (.push seen (str "qualified:" k))
                       (if (qualified k) (dissoc data k) data)) data data))
        (defn without-nils [data seen]
          (reduce-kv (fn [data k v]
                       (.push seen (str "nil:" k))
                       (if (empty-value v) (dissoc data k) data)) data data))
        (defn stop [a k v] (reduced (conj a [k v])))
        (defmacro probe []
          (let [seen (js/Array.) data (with-meta (array-map :ns/private 0 :nil nil :false false :kept 42) {:tag :kept})
                filtered (without-nils (without-qualified data seen) seen)
                early (reduce-kv (fn [a k v] (.push seen (str "stop:" k)) (stop a k v)) [] data)]
            (list 'quote [filtered (meta filtered) (vec seen) early
                          (mapv qualified-keyword? [nil false 1 "a/b" 'a/b :a :a/b])])) )"#,
    )
    .unwrap();
    std::fs::write(
        root.join("app.cljs"),
        "(ns app (:require [filters :refer [probe]])) (js/console.log (pr-str [(probe) (probe)]))",
    )
    .unwrap();
    let expected = "[{:false false, :kept 42} {:tag :kept} [\"qualified::ns/private\" \"qualified::nil\" \"qualified::false\" \"qualified::kept\" \"nil::nil\" \"nil::false\" \"nil::kept\" \"stop::ns/private\"] [[:ns/private 0]] [false false false false false false true]]";
    let expected = format!("[{expected} {expected}]\n");
    let mut reference = None;
    for jobs in [1, 2, 4] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let (program, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                stats: Some(&mut stats),
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("jobs {jobs}: {e}"));
        let native = emit_js(&program);
        assert_eq!(program_output(&native), expected, "jobs {jobs}");
        assert!(stats.native_helpers.compiled >= 5, "{stats:?}");
        assert!(stats.native_helpers.published > 0, "{stats:?}");
        assert!(stats.native_helpers.waits >= 2, "{stats:?}");
        if let Some(reference) = &reference {
            assert_eq!(&native, reference, "jobs {jobs}");
        } else {
            reference = Some(native);
        }
        #[cfg(feature = "quickjs")]
        {
            let (legacy, _) = analyze_namespace_entry_recorded(
                "app",
                &[root.path().to_path_buf()],
                &mut MacroHost::new(),
                Default::default(),
                &mut Vec::new(),
                EntryRecorders::default(),
            )
            .unwrap();
            assert_eq!(program_output(&emit_js(&legacy)), expected, "legacy");
        }
    }
}

#[test]
fn partition_all_helpers_publish_and_resume_with_buffered_values() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("native-partition-all");
    std::fs::write(
        root.join("parts.cljs"),
        r#"(ns parts)
        (def xf (comp (partition-all 2) (remove (fn [group] (= (first group) :skip)))))
        (defn step [a x] (conj a x))
        (defmacro probe [] (list 'quote (transduce xf (completing step) [] [1 2 :skip 4 5])))"#,
    )
    .unwrap();
    std::fs::write(
        root.join("app.cljs"),
        r#"(ns app (:require [parts :refer [probe]]))
        (js/console.log (pr-str [(probe) (probe)]))"#,
    )
    .unwrap();
    for jobs in [1, 2, 4] {
        let build = |native| {
            let mut host = MacroHost::new();
            if native {
                host.use_native_macros();
            }
            let mut stats = light_cljs::CompileStats::default();
            let (program, _) = analyze_namespace_entry_recorded(
                "app",
                &[root.path().to_path_buf()],
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
                assert_eq!(stats.native_helpers.published, 2);
                assert_eq!(stats.native_helpers.per_task, 0);
                assert!(stats.native_helpers.waits >= 2);
            }
            emit_js(&program)
        };
        let native = build(true);
        assert_eq!(program_output(&native), "[[[1 2] [5]] [[1 2] [5]]]\n");
        #[cfg(feature = "quickjs")]
        assert_eq!(native, build(false));
    }
}

#[test]
fn native_printing_matches_quickjs_for_every_native_value_kind() {
    // One differential table covers constructed values and imported reader
    // occurrences. Compare emitted strings too: Node's stdout replaces lone
    // surrogates, whereas the emitted program preserves every UTF-16 unit.
    let mut failures = Vec::new();
    for (label, declarations, body, argument) in [
        ("scalars", "", "(pr-str nil false true 0 -0.0 ##NaN ##Inf ##-Inf (/ 1 3) 9007199254740993)", "nil"),
        ("strings", "", r#"(pr-str "\"\\\b\f\n\r\t\u0000😀" (js/String.fromCharCode 55296 56320 55296 56320 56320))"#, "nil"),
        ("reader chars", "", "(pr-str x)", r#"[\a \newline \space \tab \backspace \formfeed \return \u0000 \u263a]"#),
        ("named", "", "(pr-str 'a 'a/b :a :a/b (symbol \"\") (keyword \"\"))", "nil"),
        ("empty", "", "(pr-str '() [] {} #{})", "nil"),
        ("reduced and atoms", "", "(pr-str (reduced [1]) (pr-str* (reduced 2)) (str (reduced 3)) (atom [1 2]) (pr-str* (atom false)) (str (atom nil)))", "nil"),
        ("collections", "", "(pr-str '(1 [2] {:a 3} #{4}) [1 nil false] {:a 1 :b 2} #{:a :b})", "nil"),
        ("map ordering", "", "(pr-str (into {} (map (fn [i] [i (- i)]) (range 40))))", "nil"),
        ("set ordering", "", "(pr-str (into #{} (range 40)))", "nil"),
        ("small constructors", "", "(let [f hash-map] (pr-str (array-map :a 1 :b 2) (f :a 1 :b 2) (hash-set :a :b) (set [:a :b]) (into {} [[:a 1] [:b 2]]) (assoc nil :a 1 :b 2)))", "nil"),
        ("retained promotion", "", "(let [m (into {} (map (fn [i] [i i]) (range 40))) s (into #{} (range 40))] (pr-str (apply dissoc m (range 35)) (apply disj s (range 35))))", "nil"),
        ("hash collisions", "", "(let [f hash-map] (pr-str (f \"Aa\" 1 \"BB\" 2 0 3 2147483647 4 nil 5) (hash-set \"Aa\" \"BB\" 0 2147483647 nil)))", "nil"),
        ("positional literals", "", r#"(pr-str '{\a 1 "a" 2 1 :int 1.0 :float} '#{\a "a" 1 1.0} {\a 1 "a" 2} #{\a "a"})"#, "nil"),
        // The legacy input bridge collapses lowered duplicates (496's
        // documented boundary). Construct those inside the macro above;
        // compare imported occurrences whose stored values are the same here.
        ("positional reader inputs", "", "(pr-str x)", r#"[{\a 1 "b" 2} #{\a "b"} {1 :int 2.0 :float} #{1 2.0} {{\a 1} :first {"b" 1} :last} #{[\a] ["b"]}]"#),
        ("positional updates", "", r#"(let [m '{\a 1 "a" 2 :z 3} s '#{1 1.0 2}] (pr-str (assoc m "a" 9) (dissoc m "a") (conj s 3) (disj s 1)))"#, "nil"),
        ("positional promotion", "", r#"(let [m '{\a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8} s '#{1 1.0 2 3 4 5 6 7}] (pr-str m (assoc m :h 9) s (conj s 8)))"#, "nil"),
        ("array maps above threshold", "", "(let [a (array-map 9 :a 8 :b 7 :c 6 :d 5 :e 4 :f 3 :g 2 :h 1 :i 9 :last) f array-map] (pr-str a (apply f [9 :a 8 :b 7 :c 6 :d 5 :e 4 :f 3 :g 2 :h 1 :i 9 :last]) (assoc a 0 :new)))", "nil"),
        ("sequence maps", "", "(pr-str (seq-to-map-for-destructuring [:z 1 :c 2 :a 3]) (assoc (seq-to-map-for-destructuring []) :z 1 :a 2))", "nil"),
        ("positional metadata", "", r#"(binding [*print-meta* true] (pr-str (with-meta '#{1 1.0} '{\a 1 "a" 2}) (with-meta '{\a 1 "a" 2} {:z 0 :a 1})))"#, "nil"),
        ("lazy", "", "(pr-str (map inc '(1 2 3)) (cons 0 (map inc [1 2])) (range 3) (take 3 (repeat :x)))", "nil"),
        ("lazy bound", "", "(binding [*print-length* 3] (pr-str (range) (repeat :x)))", "nil"),
        ("lazy effects", "", "(let [a (js/Array.) s (map (fn [x] (.push a x) x) '(1 2 3 4)) p (binding [*print-length* 2] (pr-str s)) n (.-length a)] (pr-str p n (pr-str s) (vec a)))", "nil"),
        ("metadata", "", "(binding [*print-meta* true] (pr-str (with-meta [1] {:tag 'T}) (with-meta 's {:x true}) (with-meta (map inc '(1)) {:m 1}) (with-meta {} {:m 2})))", "nil"),
        ("no metadata", "", "(pr-str (with-meta [1] {:tag true}))", "nil"),
        ("length", "", "(binding [*print-length* 0] (pr-str [1] {} {:x 1} #{1} '(1)))", "nil"),
        ("depth", "", "(binding [*print-level* 1] (pr-str [[1] {:a [2]}] '((1)) {:a #{2}}))", "nil"),
        ("depth zero", "", "(binding [*print-level* 0] (pr-str [] {} #{} '() :x 1))", "nil"),
        ("namespace maps", "", "(binding [*print-namespace-maps* true] (pr-str {:a/x 1 :a/y 2} {'a/x 1 :a/y 2} {:a/x 1 :b/y 2} {:x 1}))", "nil"),
        ("readability", "", "(binding [*print-readably* false *print-dup* true *print-fn-bodies* true] (pr-str [\"a\\n\" \\newline] :x 's))", "nil"),
        ("string functions", "", "(pr-str (pr-str) (prn-str 1 \"x\") (print-str [\"x\"] \"y\") (println-str :a) (str [\"x\"] {:a 1} #{:x} '(1)))", "nil"),
        ("direct printer", "", "(pr-str (pr-str* [1 \"x\"]) (try (pr-str* nil) (catch :default e (.-message e))) (try (pr-str* 1) (catch :default e (.-message e))))", "nil"),
        ("explicit options", "", "(pr-str (pr-str-with-opts [\"x\" [1 2 3]] {:readably true :print-length 1 :more-marker \"MORE\"}) (prn-str-with-opts [] {}))", "nil"),
        ("options metadata", "", "(pr-str-with-opts [(with-meta [\"x\"] {:m 1})] {:meta true :readably true})", "nil"),
        ("options implementation", "", "(pr-str-with-opts [[1 2] :x nil] {:readably true :alt-impl (fn [x w o] (-write w \"!\") ((:fallback-impl o) x w (dissoc o :alt-impl)))})", "nil"),
        ("printer restoration on error", "", "(pr-str (try (binding [*print-level* 1] (pr-str (lazy-seq (throw (js/Error. \"print failure\"))))) (catch :default e (.-message e))) (pr-str [[1]]))", "nil"),
        ("restoration", "", "(let [a (binding [*print-length* 1 *print-level* 1] (pr-str [[1] 2]))] (pr-str a [[1] 2]))", "nil"),
        ("record", "(defrecord R [a b])", "(pr-str (->R 1 [2]) (assoc (->R 1 2) :c 3) R)", "nil"),
        ("record extensions", "(defrecord R [a b])", "(pr-str (assoc (->R 1 2) :z 3 :c 4 :d 5))", "nil"),
        ("record hashed extensions", "(defrecord R [a])", "(pr-str (reduce (fn [r i] (assoc r i (- i))) (->R 1) (range 12)))", "nil"),
        ("record positional extensions", "(defrecord R [a])", r#"(pr-str (R. 0 nil '{\a 1 "a" 2 :z 3}) (map->R '{\a 1 :a 0 "a" 2 :z 3}))"#, "nil"),
        ("record options", "(defrecord R [a b])", "(binding [*print-meta* true *print-length* 1 *print-level* 2] (pr-str (with-meta (->R 1 [2]) {:m 1})))", "nil"),
        ("record string functions", "(defrecord R [a])", "(let [r (->R \"x\")] (pr-str (str r) (pr-str r) (print-str r)))", "nil"),
        ("record custom string functions", "(defrecord R [a] Object (toString [_] (str \"value=\" a)))", "(let [r (->R \"x\")] (pr-str (str r) (pr-str r) (print-str r)))", "nil"),
        ("mutable record string functions", "(defrecord R [^:mutable a])", "(let [r (->R \"x\")] (set! (.-a r) \"y\") (pr-str (str r) (pr-str r) (print-str r) (pr-str* r)))", "nil"),
        ("mutable record custom string functions", "(defrecord R [^:mutable a] Object (toString [_] (str \"value=\" a)))", "(let [r (->R \"x\")] (pr-str (str r) (pr-str r) (print-str r)))", "nil"),
        ("mutable record options", "(defrecord R [^:mutable a-b ready?])", "(let [r (R. \"x\" [2 3] {:m true} {:z 3 :c 4})] (pr-str (binding [*print-meta* true *print-length* 1 *print-level* 2] (pr-str r)) (binding [*print-level* 0] (pr-str r)) (print-str r) (print-meta? {:meta true} r) (print-meta? {:meta false} r)))", "nil"),
        ("mutable record colliding members", "(defrecord R [^:mutable a-b a_b tail])", "(let [r (->R 1 2 3)] (set! (.-a-b r) 4) (pr-str r (print-str r) (pr-str* r)))", "nil"),
        ("volatile record reserved members", "(defrecord R [^:volatile-mutable class class$ constructor constructor$ __proto__ ready? ready_QMARK_ tail])", "(let [r (->R 1 2 3 4 5 6 7 8)] (set! (.-class r) 9) (pr-str r (print-str r) (pr-str* r)))", "nil"),
        ("unsynchronized record colliding members", "(defrecord R [^:unsynchronized-mutable a-b a_b tail])", "(pr-str (->R 1 2 3))", "nil"),
        ("mutable record internal members", "(defrecord R [^:mutable __meta __extmap __hash tail])", "(pr-str (R. 1 2 3 4 {:m true} {:x 5} 6))", "nil"),
        ("type", "(deftype T [a])", "(pr-str (T. 1) T)", "nil"),
        ("mutable type", "(deftype T [^:mutable a])", "(pr-str (T. 1) T)", "nil"),
        ("type string", "(deftype T [a] Object (toString [_] (str \"value=\" a)))", "(pr-str (T. 1) (str (T. 2)))", "nil"),
        ("type string functions", "(deftype T [a])", "(let [t (T. \"x\")] (pr-str (str t) (pr-str t) (print-str t)))", "nil"),
        ("type custom string functions", "(deftype T [a] Object (toString [_] (str \"value=\" a)))", "(let [t (T. \"x\")] (pr-str (str t) (pr-str t) (print-str t)))", "nil"),
        ("mutable type string functions", "(deftype T [^:mutable a])", "(let [t (T. \"x\")] (pr-str (str t) (pr-str t) (print-str t)))", "nil"),
        ("mutable type custom string functions", "(deftype T [^:mutable a] Object (toString [_] (str \"value=\" a)))", "(let [t (T. \"x\")] (pr-str (str t) (pr-str t) (print-str t)))", "nil"),
        ("printer protocol", "(deftype T [a] IPrintWithWriter (-pr-writer [this writer opts] (-write writer \"<\") (-pr-writer a writer opts) (-write writer \">\")))", "(pr-str (T. [1 2]))", "nil"),
        ("printer writer identity", "(deftype T [] IPrintWithWriter (-pr-writer [_ w o] (-write w (if (instance? StringBufferWriter w) \"writer\" \"other\"))))", "(pr-str (pr-str (T.)) (pr-str* (T.)) (print-str (T.)) (str [(T.)]))", "nil"),
        ("printer write result", "(deftype T [] IPrintWithWriter (-pr-writer [_ w o] (let [b (-write w \"x\")] (-write w (if (nil? b) \"nil\" \"buffer\")) (.append b \"!\"))))", "(pr-str (pr-str (T.)) (pr-str* (T.)) (print-str (T.)) (str [(T.)]))", "nil"),
        ("printer discards writer buffer", "(deftype T [] IPrintWithWriter (-pr-writer [_ w o] (-write w \"before\") (set! (.-sb w) nil)))", "(pr-str (pr-str (T.)) (pr-str* (T.)) (print-str (T.)) (pr-str-with-opts [(T.)] {:readably true}) (prn-str-with-opts [(T.)] {}))", "nil"),
        ("printer replaces writer buffer", r#"(deftype T [] IPrintWithWriter (-pr-writer [_ w o] (-write w (str "before:" (.-text (.-sb w)))) (js* "(~{}.sb = {text: 'other', append(s) { this.text += s; return this; }})" w) (-write w "after")))"#, "(pr-str (pr-str (T.)) (pr-str* (T.)) (print-str (T.)) (str [(T.)]) (pr-str-with-opts [(T.)] {:readably true}) (prn-str-with-opts [(T.)] {}))", "nil"),
        ("printer unreadable writer buffer", r#"(deftype T [] IPrintWithWriter (-pr-writer [_ w o] (-write w "before") (js* "Object.defineProperty(~{}, 'sb', {get() { throw new Error('read sb'); }})" w)))"#, "(pr-str (pr-str (T.)) (pr-str* (T.)) (print-str (T.)) (pr-str-with-opts [(T.)] {:readably true}))", "nil"),
        ("writer protocol", "(deftype W [^:mutable s] IWriter (-write [_ text] (set! s (str s text))) (-flush [_] nil))", "(let [w (W. \"\")] (-pr-writer [1 \"x\"] w {:readably true}) (-flush w) (.-s w))", "nil"),
        ("nil printer protocol", "(extend-protocol IPrintWithWriter nil (-pr-writer [_ w o] (-write w \"NULL\")))", "(pr-str (pr-str* nil) (pr-str nil))", "nil"),
        ("writer errors", "", "(pr-str (try (-write (js/Object.) \"x\") (catch :default e (.-message e))) (try (-flush (js/Object.)) (catch :default e (.-message e))))", "nil"),
        ("regex", "", "(pr-str #\"a\\\\b\")", "nil"),
        ("namespace", "", "(pr-str (str *ns*) (ns-name *ns*) *ns*)", "nil"),
        ("namespace constructor", "", "(let [s (with-meta 'printing.namespace {:doc \"fresh\"}) n (Namespace. nil s)] (pr-str (str n) (meta (ns-name n)) (identical? s (ns-name n)) (ns-name 'a/b) (Namespace. nil \"text\")))", "nil"),
        ("namespace registry", "", "(let [s (with-meta 'printing.namespace {:doc \"first\"}) n (create-ns s) again (create-ns (with-meta 'printing.namespace {:doc \"second\"}))] (pr-str n (str n) (meta (ns-name n)) (identical? n again) (identical? n (find-ns s)) (identical? n (find-ns n)) (find-ns 'absent)))", "nil"),
        ("namespace explicit initializer", "(def captured [(str *ns*) (meta (ns-name *ns*)) (identical? *ns* *ns*)])", "(binding [*ns* (Namespace. nil (with-meta 'cljs.user {:tag true}))] (pr-str captured))", "nil"),
        ("namespace explicit identity", "(def captured-ns *ns*)", "(let [n (Namespace. nil (with-meta 'cljs.user {:tag true}))] (binding [*ns* n] (pr-str (identical? n captured-ns) (identical? captured-ns captured-ns) (meta (ns-name captured-ns)))))", "nil"),
        ("namespace distinct explicit snapshots", "(def captured-a *ns*) (def captured-b *ns*)", "(let [a (Namespace. nil (with-meta 'cljs.user {:tag :a})) b (Namespace. nil (with-meta 'cljs.user {:tag :b})) x (binding [*ns* a] captured-a) y (binding [*ns* b] captured-b)] (pr-str (identical? x a) (identical? y b) (identical? x y) (meta (ns-name x)) (meta (ns-name y))))", "nil"),
        ("namespace text initializer", "(def captured [(str *ns*) (ns-name *ns*) (identical? *ns* *ns*)])", "(binding [*ns* (Namespace. nil \"text\")] (pr-str captured))", "nil"),
    ].into_iter().chain(include_str!("fixtures/compiler-native-printing.tsv").lines().filter(|line|!line.starts_with('#')).map(|line| {
        let mut columns=line.split('\t');
        (columns.next().unwrap(),"(defn observe [x] x)",columns.next().unwrap(),"nil")
    })) {
        let source = format!("{declarations} (defmacro m [x] {body}) (js/console.log (m {argument}))");
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = match analyze_with_macros(&source, &parse(&source).unwrap_or_else(|e| panic!("{label}: {e}")), &mut host) {
            Ok(program)=>program,
            Err(e)=>{failures.push(format!("{label}: {e}"));continue;}
        };
        let native = emit_js(&program);
        assert!(!program_output(&native).is_empty(), "{label}");
        #[cfg(feature = "quickjs")]
        {
            let legacy = light_cljs::compile(&source).unwrap_or_else(|e| panic!("{label}: {e}"));
            if native!=legacy {failures.push(format!("{label}: native {:?}, legacy {:?}",program_output(&native),program_output(&legacy)));}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn native_printer_rejects_unimplemented_host_values_explicitly() {
    use light_cljs::evaluator::MacroEvaluator;
    let evaluator = light_cljs::bbp_evaluator::BbpEvaluator::new(Default::default()).unwrap();
    let result=evaluator.evaluate_unit(r#"rt=>{
        for(const [value,kind] of [[{},'object'],[[1,2],'object'],[()=>1,'function']]){
            let message;
            try{rt.core.prStr(value);}catch(e){message=e.message;}
            if(message!=='unsupported operation: native macro runtime pr-str of '+kind)throw new Error('printer boundary: '+message);
        }
        return null;
    }"#).unwrap();
    evaluator.release(result).unwrap();
}

#[test]
fn native_namespace_context_matches_quickjs_across_helpers_and_jobs() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("native-namespace-context");
    for (file, source) in [
        (
            "helpers.cljs",
            r#"(ns helpers)
            (def initial [(str *ns*) (str (ns-name *ns*))])
            (def nested-initial [initial (str *ns*)])
            (def bound-value [(str *ns*) (meta (ns-name *ns*))])
            (def nil-context (str *ns*))
            (def symbol-context [(str *ns*) (ns-name *ns*) (meta *ns*)])
            (def explicit-context [(str *ns*) (meta (ns-name *ns*)) (identical? *ns* *ns*)])
            (def text-context [(str *ns*) (ns-name *ns*) (identical? *ns* *ns*)])
            (def binding-inner [(str *ns*) (meta (ns-name *ns*))])
            (def binding-outer (binding [*ns* (create-ns (with-meta 'inside-initializer {:bound true}))] binding-inner))
            (defmacro helper-site [] (str *ns*))
            (defn nested [] [(helper-site) (str *ns*) (str (ns-name *ns*))])
            (defn context [] [binding-outer nested-initial (nested)])"#,
        ),
        (
            "macros.cljs",
            r#"(ns macros (:require [helpers :as h]))
            (defmacro capture []
              (let [before (str *ns*) result (h/context)
                    bound (binding [*ns* (create-ns (with-meta 'temporary {:tag true}))]
                            [(str *ns*) (meta (ns-name *ns*)) h/bound-value
                             (binding [*ns* nil] h/nil-context)
                             (binding [*ns* (with-meta 'bound-symbol {:bound :symbol})] h/symbol-context)
                             (binding [*ns* (Namespace. nil (with-meta (ns-name *ns*) {:explicit true}))] h/explicit-context)
                             (binding [*ns* (Namespace. nil "text")] h/text-context)])
                    thrown (try (binding [*ns* (create-ns 'throwing)]
                                  (throw (js/Error. (str *ns*))))
                                (catch :default e (.-message e)))]
                (list 'quote [before result bound thrown (str *ns*) (pr-str *ns*)
                              (identical? *ns* (find-ns (ns-name *ns*)))])))
            (defmacro diagnostic [x]
              (throw (js/Error. (str (str *ns*) ": " (pr-str x)))))"#,
        ),
        (
            "a.cljs",
            "(ns a (:require [macros :refer [capture]])) (def value [(capture) (capture)])",
        ),
        (
            "b.cljs",
            "(ns b (:require [macros :refer [capture]])) (def value [(capture) (capture)])",
        ),
        (
            "app.cljs",
            "(ns app (:require [a] [b] [macros :refer [capture]])) (js/console.log (pr-str [a/value b/value (capture)]))",
        ),
    ] {
        std::fs::write(root.join(file), source).unwrap();
    }
    let mut reference: Option<String> = None;
    for (jobs, seed, held) in [
        (1, 1, false),
        (2, 1, false),
        (4, 1, false),
        (4, 2, false),
        (4, 3, true),
    ] {
        let build = |native| {
            let mut host = MacroHost::new();
            if native {
                host.use_native_macros();
            }
            let mut stats = light_cljs::CompileStats::default();
            let (program, _) = analyze_namespace_entry_recorded(
                "app",
                &[root.path().to_path_buf()],
                &mut host,
                Default::default(),
                &mut Vec::new(),
                EntryRecorders {
                    stats: Some(&mut stats),
                    slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                    #[cfg(debug_assertions)]
                    perturbation: native.then_some(light_cljs::scheduler::Perturbation {
                        seed,
                        lane_hold: if held {
                            vec!["a/value".into()]
                        } else {
                            Vec::new()
                        },
                        limits: Some(light_cljs::scheduler::Limits {
                            batch_jobs: 1,
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|e| panic!("jobs {jobs}, native {native}: {e}"));
            if native {
                assert!(stats.native_helpers.units_evaluated > 0);
                if held {
                    assert!(stats.lanes.inherited > 0, "no inherited namespace demand");
                }
            }
            emit_js(&program)
        };
        let native = build(true);
        let output = program_output(&native);
        assert!(output.contains("temporary") && output.contains("throwing"));
        if let Some(previous) = &reference {
            assert_eq!(output, program_output(previous), "jobs {jobs}");
            assert!(&native == previous, "emitted program differs, jobs {jobs}");
        }
        reference = Some(native.clone());
        #[cfg(feature = "quickjs")]
        {
            let legacy = build(false);
            assert_eq!(output, program_output(&legacy), "jobs {jobs}");
            assert!(native == legacy, "emitted program differs, jobs {jobs}");
        }
    }
    std::fs::write(
        root.join("error.cljs"),
        "(ns error (:require [macros :refer [diagnostic]])) (diagnostic [1 :x])",
    )
    .unwrap();
    for jobs in [1, 2, 4] {
        let build = |native| {
            let mut host = MacroHost::new();
            if native {
                host.use_native_macros();
            }
            analyze_namespace_entry_recorded(
                "error",
                &[root.path().to_path_buf()],
                &mut host,
                Default::default(),
                &mut Vec::new(),
                EntryRecorders {
                    slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                    ..Default::default()
                },
            )
            .unwrap_err()
            .to_string()
        };
        let native = build(true);
        assert!(native.contains("error: [1 :x]"), "{native}");
        #[cfg(feature = "quickjs")]
        {
            let legacy = build(false);
            // Engine stack frames and the native engine's prefix differ;
            // the diagnostic message, call and definition provenance do not.
            let marker = "macro macros/diagnostic: ";
            let (native_call, native_message) = native.split_once(marker).unwrap();
            let (legacy_call, legacy_message) = legacy.split_once(marker).unwrap();
            assert_eq!(native_call, legacy_call, "jobs {jobs}");
            assert_eq!(
                native_message
                    .strip_prefix("big-boy-pants-js: ")
                    .unwrap()
                    .split_once("; definition ")
                    .unwrap()
                    .0,
                legacy_message.lines().next().unwrap(),
                "jobs {jobs}"
            );
            assert_eq!(
                native_message.split_once("; definition ").unwrap().1,
                legacy_message.split_once("; definition ").unwrap().1,
                "jobs {jobs}"
            );
        }
    }
}

#[test]
fn native_compiler_macro_fixtures_keep_their_legacy_results() {
    let fixtures = include_str!("compiler/macros.rs");
    let root = temp_dir::TempDir::new("native-compiler-macro-fixtures");
    for (name, expected) in [
        ("SOURCE_ORDER_GENERATIONS", "true\n"),
        ("GENERATED_DECLARATIONS", "true\n"),
        ("NESTED_DEMANDS", "true\n"),
        ("SNAPSHOT_GENERATIONS", "true\n"),
        ("MUTUAL_RECURSION", "true\n"),
        ("FAILED_GENERATION", "Error: sticky"),
        ("SAVED_FUNCTION_CODE", "true\n"),
        ("FRONTIER_REPLAY", "[[1 10] [2 10] 2 10]\n"),
    ] {
        let start = format!("pub(crate) const {name}: &str = r#\"");
        let source = fixtures
            .split_once(&start)
            .unwrap()
            .1
            .split_once("\"#;")
            .unwrap()
            .0;
        let source = format!("(ns fixture) {source}");
        std::fs::write(root.join("fixture.cljs"), source).unwrap();
        let mut host = MacroHost::new();
        host.use_native_macros();
        let native = light_cljs::compile_namespace_with_macros(
            "fixture",
            &[root.path().to_path_buf()],
            &mut host,
        )
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        if name == "FAILED_GENERATION" {
            // The macro catches this initializer and repairs it later; the
            // emitted application's first initializer deliberately throws.
            let output = std::process::Command::new("node")
                .arg("-e")
                .arg(&native)
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
        } else {
            assert_eq!(
                program_output(&format!("console.log({native});")),
                expected,
                "{name}"
            );
        }
        #[cfg(feature = "quickjs")]
        assert_eq!(
            native,
            light_cljs::compile_namespace("fixture", &[root.path().to_path_buf()]).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn native_unscoped_gensym_reserves_imported_symbols_and_cache_names() {
    use light_cljs::compiler_data::{Datum, DatumKind, Name};
    use light_cljs::evaluator::MacroEvaluator;
    let evaluator = light_cljs::bbp_evaluator::BbpEvaluator::new(Default::default()).unwrap();
    let reserved = Datum::new(DatumKind::Symbol(Name::new(None, "G__1")));
    let value = evaluator
        .import_data(&reserved, Default::default())
        .unwrap();
    evaluator.release(value).unwrap();
    let result = evaluator.evaluate_unit("rt => { const a=rt.core.gensym(); rt.compilerForms.raiseGensyms(40); const names=[a,rt.core.gensym(),rt.core.gensym(null),rt.core.gensym(false),rt.core.gensym('x\\ud800')].map(rt.core.name); const expected=['G__2','G__41','42','false43','x\\ud80044']; for(let i=0;i<names.length;i++) if(names[i]!==expected[i]) throw new Error('gensym name '+i+': '+names[i]); return null; }").unwrap();
    evaluator.release(result).unwrap();
}

#[test]
fn native_gensym_bindings_share_the_rust_core_macro_counter() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("native-gensym-bindings");
    std::fs::write(
        root.join("app.cljs"),
        r#"(ns app)
           (defmacro hygienic []
             (let [g (gensym "logic")]
               (list 'let [g 7] [g (list 'or false g)])))
           (defn values [] [(hygienic) (or false 11) (hygienic)])
           (js/console.log (pr-str (values)))"#,
    )
    .unwrap();
    let mut reference = None;
    for jobs in [1, 2, 4] {
        let build = |native| {
            let mut host = MacroHost::new();
            let mut stats = light_cljs::CompileStats::default();
            if native {
                host.use_native_macros();
            }
            let (program, _) = analyze_namespace_entry_recorded(
                "app",
                &[root.path().to_path_buf()],
                &mut host,
                Default::default(),
                &mut Vec::new(),
                EntryRecorders {
                    stats: Some(&mut stats),
                    slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|error| panic!("jobs {jobs}, native {native}: {error}"));
            if native {
                assert!(stats.native_helpers.units_evaluated > 0);
            }
            emit_js(&program)
        };
        let native = build(true);
        assert_eq!(program_output(&native), "[[7 7] 11 [7 7]]\n");
        if let Some(reference) = &reference {
            assert_eq!(&native, reference, "jobs {jobs}");
        } else {
            reference = Some(native.clone());
        }
        #[cfg(feature = "quickjs")]
        {
            let legacy = build(false);
            // Exact emitted names must agree as well as binding behavior.
            assert_eq!(native, legacy, "jobs {jobs}");
            assert_eq!(program_output(&legacy), "[[7 7] 11 [7 7]]\n");
        }
    }
}

#[test]
fn native_gensym_keeps_scopes_helpers_and_cache_names_at_one_two_and_four_slots() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    use light_cljs::namespace_artifact::NamespaceCapture;
    let root = temp_dir::TempDir::new("native-gensym");
    std::fs::create_dir(root.join("names")).unwrap();
    std::fs::write(
        root.join("names/macros.cljs"),
        r#"
      (ns names.macros)
      (declare helper)
      (defmacro fresh []
        (let [before (gensym) middle (helper) after (gensym "explicit")]
          (list 'quote [before middle after (gensym nil) (gensym false) (gensym 7)
                       (gensym :keyword) (gensym 'prefix/qualified) (gensym "escaped\ud800😀")])))
      (defmacro compiled [] (list 'quote (gensym "compiled")))
      (defn helper [] [(compiled) (gensym "helper")])
      (def retained (fresh))"#,
    )
    .unwrap();
    for ns in ["a", "b", "c"] {
        std::fs::write(
            root.join(format!("names/{ns}.cljs")),
            format!(
                r#"
          (ns names.{ns} (:require [names.macros :refer [fresh]]))
          (def first-names (fresh))
          (defn second-names [] (fresh))"#
            ),
        )
        .unwrap();
    }
    std::fs::write(
        root.join("app.cljs"),
        r#"
      (ns app (:require [names.macros :as m] [names.a :as a] [names.b :as b] [names.c :as c]))
      (def values [m/retained a/first-names (a/second-names)
                  b/first-names (b/second-names) c/first-names (c/second-names)])
      (let [symbols (mapcat (fn [xs] (concat [(first xs) (second (second xs))] (drop 2 xs))) values)]
        (assert (= (count symbols) (count (set symbols)))))
      (js/console.log (pr-str values))"#,
    )
    .unwrap();
    type CapturedNames = std::collections::BTreeMap<String, std::collections::BTreeSet<Vec<u16>>>;
    fn assert_names(actual: &CapturedNames, expected: &CapturedNames) {
        assert_eq!(
            actual.keys().collect::<Vec<_>>(),
            expected.keys().collect::<Vec<_>>()
        );
        for (ns, names) in actual {
            let expected = &expected[ns];
            let extra = names
                .difference(expected)
                .map(|name| String::from_utf16_lossy(name))
                .collect::<Vec<_>>();
            let missing = expected
                .difference(names)
                .map(|name| String::from_utf16_lossy(name))
                .collect::<Vec<_>>();
            assert!(
                names == expected,
                "{ns}: extra {extra:?}, missing {missing:?}"
            );
        }
    }
    let mut reference = None;
    let mut reference_names = None;
    for (jobs, fixed) in [
        (1, false),
        (2, false),
        (4, false),
        (1, true),
        (2, true),
        (4, true),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut artifacts = std::collections::BTreeMap::new();
        let mut capture = |captured: NamespaceCapture| {
            if let Ok(artifact) = captured.artifact {
                artifacts.insert(captured.namespace, artifact);
            }
        };
        let (cold, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                capture: Some(&mut capture),
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                #[cfg(debug_assertions)]
                perturbation: Some(light_cljs::scheduler::Perturbation {
                    seed: 3,
                    fixed,
                    limits: Some(light_cljs::scheduler::Limits {
                        batch_jobs: 1,
                        admitted_jobs: if fixed {
                            1
                        } else {
                            light_cljs::scheduler::Limits::default().admitted_jobs
                        },
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("jobs {jobs}, fixed {fixed}: {error}"));
        let cold = emit_js(&cold);
        let output = program_output(&cold);
        assert!(
            output.contains("compiled") && output.contains("helper") && output.contains("explicit"),
            "{output}"
        );
        assert!(!artifacts["names.macros"].names.gensyms.is_empty());
        let names: CapturedNames = artifacts
            .iter()
            .map(|(ns, artifact)| (ns.clone(), artifact.names.gensyms.iter().cloned().collect()))
            .collect();
        if let Some(reference) = &reference_names {
            assert_names(&names, reference);
        } else {
            reference_names = Some(names);
        }
        if let Some(reference) = &reference {
            assert_eq!(&cold, reference);
        } else {
            reference = Some(cold.clone());
        }
        drop(host);
        for installed in [&["names.macros"][..], &["names.macros", "names.a"][..]] {
            let lookup = |name: &str| {
                installed
                    .contains(&name)
                    .then(|| artifacts.get(name))
                    .flatten()
            };
            let reservation = light_cljs::generated_names::Reservation::of(
                installed.iter().map(|name| &artifacts[*name].names),
            );
            let mut host = MacroHost::new();
            host.use_native_macros();
            let mut stats = light_cljs::CompileStats::new();
            let (warm, _) = analyze_namespace_entry_recorded(
                "app",
                &[root.path().to_path_buf()],
                &mut host,
                Default::default(),
                &mut Vec::new(),
                EntryRecorders {
                    install: Some(&lookup),
                    reserve: Some(&reservation),
                    stats: Some(&mut stats),
                    slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|error| panic!("jobs {jobs}, install {installed:?}: {error}"));
            assert_eq!(stats.install.unwrap().installed, installed);
            let warm = emit_js(&warm);
            let difference = warm.lines().zip(cold.lines()).find(|(a, b)| a != b);
            assert!(
                warm == cold,
                "jobs {jobs}, install {installed:?}, first difference: {difference:?}"
            );
        }
    }
    #[cfg(feature = "quickjs")]
    {
        let mut names = CapturedNames::new();
        let mut capture = |captured: NamespaceCapture| {
            if let Ok(artifact) = captured.artifact {
                names.insert(
                    captured.namespace,
                    artifact.names.gensyms.into_iter().collect(),
                );
            }
        };
        let (program, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut MacroHost::new(),
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                capture: Some(&mut capture),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(Some(emit_js(&program)), reference);
        assert_names(&names, reference_names.as_ref().unwrap());
    }
    // Initializer effects run only on misses. As with QuickJS, a provider
    // whose macros require those allocations to repeat must opt out of cache.
    // Check this separate, explicitly uncached case across all worker counts.
    let path = root.join("names/macros.cljs");
    let provider = std::fs::read_to_string(&path).unwrap();
    let provider = provider.replace("(ns names.macros)", "(ns ^:light-cljs/no-cache names.macros)")
        .replace("(defn helper [] [(compiled) (gensym \"helper\")])",
            "(def seed (gensym \"initializer\")) (def nested-seed [seed]) (defn helper [] [(compiled) (gensym \"helper\") nested-seed])")
        .replace("(def retained (fresh))", "(def retained nil)");
    assert!(provider.contains("(def seed"));
    std::fs::write(path, provider).unwrap();
    let app = root.join("app.cljs");
    std::fs::write(
        &app,
        std::fs::read_to_string(&app)
            .unwrap()
            .replace("m/retained ", ""),
    )
    .unwrap();
    let mut initializer_reference: Option<String> = None;
    let mut initializer_names = None;
    for (jobs, seed, held) in [
        (1, 1, false),
        (2, 1, false),
        (4, 1, false),
        (2, 2, false),
        (4, 2, false),
        (2, 3, false),
        (4, 3, false),
        (4, 4, true),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut names = CapturedNames::new();
        let mut capture = |captured: NamespaceCapture| {
            if let Ok(artifact) = captured.artifact {
                names.insert(
                    captured.namespace,
                    artifact.names.gensyms.into_iter().collect(),
                );
            }
        };
        let (program, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                capture: Some(&mut capture),
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                #[cfg(debug_assertions)]
                perturbation: Some(light_cljs::scheduler::Perturbation {
                    seed,
                    lane_hold: if held {
                        vec!["names.a/first-names".into()]
                    } else {
                        Vec::new()
                    },
                    limits: Some(light_cljs::scheduler::Limits {
                        batch_jobs: 1,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();
        let js = emit_js(&program);
        assert!(program_output(&js).contains("initializer"));
        if let Some(reference) = &initializer_names {
            assert_names(&names, reference);
        } else {
            initializer_names = Some(names);
        }
        if let Some(reference) = &initializer_reference {
            let difference = js.lines().zip(reference.lines()).find(|(a, b)| a != b);
            assert!(
                js == *reference,
                "initializer jobs {jobs}, first difference: {difference:?}"
            );
        } else {
            initializer_reference = Some(js);
        }
    }
    #[cfg(feature = "quickjs")]
    {
        let mut names = CapturedNames::new();
        let mut capture = |captured: NamespaceCapture| {
            if let Ok(artifact) = captured.artifact {
                names.insert(
                    captured.namespace,
                    artifact.names.gensyms.into_iter().collect(),
                );
            }
        };
        let (program, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut MacroHost::new(),
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                capture: Some(&mut capture),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(Some(emit_js(&program)), initializer_reference);
        assert_names(&names, initializer_names.as_ref().unwrap());
    }
}

#[test]
fn penpot_macro_declarations_fit_the_native_frontend() {
    // Verbatim defrecord and with-open declarations, with unrelated source
    // removed. The former emitted >16 KiB; both exceeded weighted nesting256.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/frontend");
    let build = |native| {
        let mut host = MacroHost::new();
        if native {
            host.use_native_macros();
        }
        let mut stats = light_cljs::CompileStats::default();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_with_stats(
            "admission.app",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut Vec::new(),
            &mut stats,
        )
        .unwrap();
        if native {
            assert!(stats.native_helpers.units_compiled >= 2);
        }
        emit_js(&program)
    };
    let native = build(true);
    assert_eq!(program_output(&native), "42\n");
    #[cfg(feature = "quickjs")]
    assert_eq!(native, build(false));
}

#[test]
fn deep_native_constructor_trees_preserve_effects_and_provenance() {
    // Siblings run left to right. Branches, exceptions and closures stay at
    // their original evaluation site, and input occurrence identity survives.
    let tree = |leaf: &str| format!("{}{}{}", "(list ".repeat(70), leaf, ")".repeat(70));
    for (leaf, expected) in [
        (
            "(do (.push seen \"value\") x)",
            "[[\"before\" \"value\" \"after\"] true]\n",
        ),
        (
            "(if false (throw (js/Error. \"unselected\")) (do (.push seen \"branch\") x))",
            "[[\"before\" \"branch\" \"after\"] true]\n",
        ),
        (
            "(try (throw (js/Error. \"caught\")) (catch :default e (.push seen \"caught\") x) (finally (.push seen \"finally\")))",
            "[[\"before\" \"caught\" \"finally\" \"after\"] true]\n",
        ),
        (
            "((fn [n] (.push seen \"closure\") n) x)",
            "[[\"before\" \"closure\" \"after\"] true]\n",
        ),
        // Raw JS must not capture the emitter's new temporary locals.
        (
            "(do (.push seen (js* \"typeof temporary0_0\")) x)",
            "[[\"before\" \"undefined\" \"after\"] true]\n",
        ),
    ] {
        let source = format!(
            "(defmacro m [x] (let [seen (js/Array.)\n\
               result (list (do (.push seen \"before\") 0)\n\
                            {} (do (.push seen \"after\") 1))]\n\
               [(list 'quote (vec seen))\n\
                (identical? x (loop [value (second result) n 70]\n\
                  (if (= n 0) value (recur (first value) (dec n)))))]))\n\
             (js/console.log (pr-str (m 42)))",
            tree(leaf),
        );
        let build = |native| {
            let mut host = MacroHost::new();
            if native {
                host.use_native_macros();
            }
            let program = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
                .unwrap_or_else(|e| panic!("{leaf}: {e}"));
            emit_js(&program)
        };
        let native = build(true);
        assert_eq!(program_output(&native), expected, "{leaf}");
        #[cfg(feature = "quickjs")]
        assert_eq!(native, build(false), "{leaf}");
    }
}

#[test]
fn wide_and_deep_native_constructor_trees_reuse_operand_slots() {
    for (width, depth) in [(4096, 16), (1, 120)] {
        // The wide tree has 69,633 constructor/leaf occurrences, more than
        // the engine's 65,535-local boundary. The deep tree approaches the
        // source reader's 128-level bound. Both must expand, retaining every
        // leaf's left-to-right effect and its input occurrence identity.
        let branches = (0..width)
            .map(|index| {
                format!(
                    "{}(do (.push seen {index}) x){}",
                    "(list ".repeat(depth),
                    ")".repeat(depth),
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!(
            "(defmacro m [x]\n\
               (let [seen (js/Array.) result (list {branches})]\n\
                 [(count result)\n\
                  (loop [i 0]\n\
                    (if (= i {width}) (= (.-length seen) {width})\n\
                      (if (= (aget seen i) i) (recur (inc i)) false)))\n\
                  (identical? x (loop [value (first result) n {depth}]\n\
                    (if (= n 0) value (recur (first value) (dec n)))))]))\n\
             (js/console.log (pr-str (m 42)))"
        );
        let build = |native| {
            let mut host = MacroHost::new();
            // A finite large unit needs room for its frontend/bytecode and
            // execution. Keep these test-only budgets explicit and bounded.
            host.limits.generated_nodes = 2_000_000;
            host.evaluator_options.memory_limit = 512 * 1024 * 1024;
            host.evaluator_options.poll_budget = 100_000_000;
            host.evaluator_options.timeout = std::time::Duration::from_secs(60);
            if native {
                host.use_native_macros();
            }
            let program = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
                .unwrap_or_else(|error| panic!("{width} branches, depth {depth}: {error}"));
            emit_js(&program)
        };
        let native = build(true);
        assert_eq!(program_output(&native), format!("[{width} true true]\n"));
        #[cfg(feature = "quickjs")]
        assert_eq!(native, build(false), "{width} branches, depth {depth}");
    }
}

#[test]
fn bootstrap_namespace_macros_match_quickjs() {
    let root = temp_dir::TempDir::new("native-bootstrap");
    std::fs::create_dir(root.join("bootstrap")).unwrap();
    for (label, source, expected) in [
        (
            "as-> bindings and effects",
            include_str!("fixtures/native-helper-demand/bootstrap/bootstrap/app.cljs"),
            "[[24 24] 99 7 12 5 [\"start\" \"double\" \"shadow\" \"nested\" \"zero\"]]\n",
        ),
        (
            "conditional threading",
            r#"(ns bootstrap.app)
               (let [seen (js/Array.)
                     step (fn [x] (.push seen x) (inc x))
                     stop (fn [_] nil)]
                 (js/console.log (pr-str
                   [(cond-> 1 true step false step true (+ 3))
                    (cond->> [1] true (concat [0]) false (concat [9]) true vec)
                    (some-> nil step) (some-> false nil?)
                    (some-> 1 step stop step)
                    (some->> [1] (concat [0]) vec)
                    (cond-> 7) (some-> 8) (vec seen)])))"#,
            "[5 [0 1] nil false nil [0 1] 7 8 [1 1]]\n",
        ),
        (
            "doto target and step order",
            r#"(ns bootstrap.app)
               (let [seen (js/Array.)
                     target (doto (do (.push seen "target") (js/Array.))
                              (.push (do (.push seen "first") 1))
                              (.push 2))
                     identity-step (fn [x] (.push seen "symbol") x)]
                 (js/console.log (pr-str
                   [(vec (doto target identity-step))
                    (identical? target (doto target)) (vec seen)])))"#,
            "[[1 2] true [\"target\" \"first\" \"symbol\"]]\n",
        ),
        (
            "iteration limit and array effects",
            r#"(ns bootstrap.app)
               (let [seen (js/Array.) a (array 1 2 3) v (volatile! 0)]
                 (dotimes [i (do (.push seen "limit") 3)] (.push seen i))
                 (dotimes [i -1] (.push seen "bad"))
                 (while (< @v 2) (vswap! v inc))
                 (js/console.log (pr-str
                   [(vec (amap (do (.push seen "map") a) i ret
                           (+ (aget a i) i)))
                    (areduce (do (.push seen "reduce") a) i ret 0
                      (+ ret (aget a i)))
                    (vec a) @v (vec seen)])))"#,
            "[[1 3 5] 6 [1 2 3] 2 [\"limit\" 0 1 2 \"map\" \"reduce\"]]\n",
        ),
        (
            "interop and unchecked extrema",
            r#"(ns bootstrap.app)
               (let [upper (memfn toUpperCase) seen (js/Array.)]
                 (js/console.log (pr-str
                   [(upper "abc") (.. "abc" (toUpperCase) (charAt 1))
                    (js-fn? upper) (js-fn? 42)
                    (unchecked-max 4) (unchecked-min 4 4)
                    (unchecked-max (do (.push seen 1) 2)
                                   (do (.push seen 2) 7))
                    (unchecked-min 8 3 5 1) (vec seen)])))"#,
            "[\"ABC\" \"B\" true false 4 4 7 1 [1 2]]\n",
        ),
        (
            "discarded forms and lazy concatenation",
            r#"(ns bootstrap.app)
               (defn timed [] (time 42))
               (defn input [] (with-in-str "" nil))
               (let [seen (js/Array.)
                     xs (lazy-cat (do (.push seen 1) [1])
                                  (do (.push seen 2) [2 3]))]
                 (js/console.log (pr-str
                   [(vec seen) (vec xs) (vec seen)
                    (locking (throw (js/Error. "discarded")) 42)
                    (comment (unresolved)) (js-debugger)
                    (js-comment "first\n   second\n")
                    (js-fn? timed) (js-fn? input)
                    (= {:a 1 :b 2} (hash-map :a 1 :b 2))])))"#,
            "[[] [1 2 3] [1 2] 42 nil nil nil true true true]\n",
        ),
    ] {
        std::fs::write(root.join("bootstrap/app.cljs"), source).unwrap();
        let build = |native| {
            let mut host = MacroHost::new();
            if native {
                host.use_native_macros();
            }
            let program = light_cljs::compiler::analyze_namespace_entry(
                "bootstrap.app",
                &[root.path().to_path_buf()],
                &mut host,
                Default::default(),
                &mut Vec::new(),
            )
            .unwrap_or_else(|error| panic!("{label}: {error}"));
            emit_js(&program)
        };
        let native = build(true);
        assert_eq!(program_output(&native), expected, "{label}");
        #[cfg(feature = "quickjs")]
        {
            let legacy = build(false);
            assert_eq!(native, legacy, "{label}");
            assert_eq!(program_output(&legacy), expected, "{label}");
        }
    }
}

#[test]
fn as_thread_namespace_errors_keep_the_original_step_location() {
    let root = temp_dir::TempDir::new("native-bootstrap-error");
    let source = "(ns app)\n(as-> 1 value\n  (let [x] value))\n";
    std::fs::write(root.join("app.cljs"), source).unwrap();
    let build = |native| {
        let mut host = MacroHost::new();
        if native {
            host.use_native_macros();
        }
        light_cljs::compiler::analyze_namespace_entry(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
        )
        .unwrap_err()
    };
    let native = build(true);
    assert_eq!(
        native.path,
        Some(root.join("app.cljs").canonicalize().unwrap())
    );
    let span = native.span.clone().unwrap();
    assert_eq!(&source[span], "(let [x] value)");
    assert_eq!(native.line_column, Some((3, 3)));
    assert!(native.message.contains("binding"), "{native}");
    #[cfg(feature = "quickjs")]
    assert_eq!(native, build(false));
}

#[test]
fn seq_converts_supported_finite_inputs_to_lists() {
    for (source, expected) in SEQ_FIXTURES {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        assert_eq!(program_output(&emit_js(&program)), *expected, "{source}");
        assert!(!host.native_macro_calls().is_empty());
    }
}

#[test]
fn seq_preserves_list_identity_and_existing_element_occurrences() {
    use light_cljs::{
        CollectionKind,
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, ExpansionOrigin, Name, Source},
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let source = "(fn [form env x] (seq x))";
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let function = evaluator
        .evaluate_unit(&light_cljs::emitter::emit_js_unit(&program))
        .unwrap();
    let environment = Datum::new(DatumKind::Collection {
        kind: CollectionKind::Map,
        children: Box::new([]),
    });
    for input in [
        "(sequence (1 1))",
        "(sequence [1 1])",
        "(sequence {:a [1 1]})",
    ] {
        let call = host
            .read_compiler_form(&Source::new("sequence.cljs", input))
            .unwrap()
            .unwrap();
        let original = &call.children()[1];
        for iteration in 0..2 {
            let result = host
                .expand_javascript(
                    &evaluator,
                    function,
                    &call,
                    &environment,
                    ExpansionOrigin {
                        macro_name: Name::new(None, "sequence"),
                        call: call.span(),
                        definition: None,
                        helpers: vec![],
                    },
                )
                .unwrap();
            assert!(matches!(
                result.data().kind(),
                DatumKind::Collection {
                    kind: CollectionKind::List,
                    ..
                }
            ));
            if input.contains("(1 1)") {
                assert!(std::ptr::eq(result.data().kind(), original.data().kind()));
                assert_eq!(result.span().bytes(), original.span().bytes());
            }
            let elements = if input.contains('{') {
                result.children()[0].children()
            } else {
                result.children()
            };
            assert_eq!(elements.len(), original.children().len());
            for (element, original) in elements.iter().zip(original.children()) {
                assert!(std::ptr::eq(element.data().kind(), original.data().kind()));
                assert_eq!(element.span().bytes(), original.span().bytes());
                assert_eq!(element.span().source().name(), "sequence.cljs");
            }
            if iteration == 1 {
                assert_eq!(host.native_macro_calls().last().unwrap().imported, 0);
            }
        }
    }
    evaluator.release(function).unwrap();
}

#[cfg(any(feature = "quickjs", feature = "v8-macros"))]
#[test]
fn seq_expansions_and_program_results_match_legacy() {
    for (source, expected) in SEQ_FIXTURES {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        let native = emit_js(&program);
        let legacy = light_cljs::compile(source).unwrap();
        assert_eq!(native, legacy, "{source}");
        assert_eq!(program_output(&native), *expected, "{source}");
        assert_eq!(program_output(&legacy), *expected, "{source}");
    }
}

#[test]
fn constructs_real_macro_form() {
    let source = "(defmacro make-add [x] (list '+ x 2)) (js/console.log (make-add 40))";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let js = emit_js(&program);
    let result = std::process::Command::new("node")
        .arg("-e")
        .arg(js)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
}

#[test]
fn variadic_and_multi_arity_macros_forward_the_receiver() {
    for (source, expected) in [
        (
            "(defmacro maximum [& xs] `(js/Math.max ~@xs)) (js/console.log (maximum 1 2) (maximum 40 2 42))",
            "2 42\n",
        ),
        (
            "(defmacro choose ([] 1) ([x] x) ([x y & more] `(js/Math.max ~x ~y ~@more))) (js/console.log (choose) (choose 2) (choose 1 2) (choose 1 2 3))",
            "1 2 2 3\n",
        ),
        (
            "(defmacro choose ([] 1) ([x] x)) (js/console.log (choose) (choose 2))",
            "1 2\n",
        ),
        (
            "(defmacro signatures [pname & sigs] `(defprotocol ~pname ~@sigs)) (signatures Sample (value [this])) (deftype Example [] Sample (value [this] 42)) (js/console.log (satisfies? Sample (Example.)) (value (Example.)))",
            "true 42\n",
        ),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        assert_eq!(program_output(&emit_js(&program)), expected, "{source}");
    }
}

#[test]
fn metadata_environment_form_and_syntax_quote() {
    for (source, want) in [
        (
            "(defmacro wrap [x] `(+ ~x 2)) (js/console.log (wrap 40))",
            "42\n",
        ),
        (
            "(defmacro place [] (count &form)) (js/console.log (place))",
            "1\n",
        ),
        (
            "(defmacro ns-name [] (list 'quote (:name (:ns &env)))) (js/console.log (str (ns-name)))",
            "cljs.user\n",
        ),
        (
            "(defmacro annotated [x] (with-meta x {:native true})) (js/console.log (annotated (+ 1 41)))",
            "42\n",
        ),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(emit_js(&p))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), want, "{source}");
    }
}

#[test]
fn preloaded_native_helper_expands_bootstrap_macros_in_namespaces() {
    let root = temp_dir::TempDir::new("native-preloaded-bootstrap");
    std::fs::write(
        root.join("app.cljs"),
        "(ns app) (declare helper) (defmacro m [x] (helper x)) (js/console.log (m 41))",
    )
    .unwrap();
    let mut host = MacroHost::new();
    host.preload_native_helper("app/helper", "(fn* [x] (as-> x n (list '+ n 1)))")
        .unwrap();
    let program = light_cljs::compiler::analyze_namespace_entry(
        "app",
        &[root.path().to_path_buf()],
        &mut host,
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(program_output(&emit_js(&program)), "42\n");
    assert_eq!(host.native_macro_calls().len(), 1);
}

#[test]
fn preloaded_real_helper_and_ordinary_js_interop() {
    let mut host = MacroHost::new();
    host.preload_native_helper(
        "cljs.user/helper",
        "(fn [x] (list '+ x (js* \"Math.max(1, 2)\")))",
    )
    .unwrap();
    let source =
        "(declare helper) (defmacro via-helper [x] (helper x)) (js/console.log (via-helper 40))";
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(emit_js(&p))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
    assert_eq!(host.native_macro_calls().len(), 1);
}

#[test]
fn javascript_interop_unwraps_input_scalars_and_preserves_javascript_objects() {
    for (source, expected) in [
        (
            "(defmacro maximum [x] (js/Math.max x 2)) (js/console.log (maximum 40))",
            "40\n",
        ),
        (
            "(defmacro part [s n] (.substring s n)) (js/console.log (part \"hello\" 2))",
            "llo\n",
        ),
        (
            "(defmacro size [s] (.-length s)) (js/console.log (size \"hello\"))",
            "5\n",
        ),
        (
            "(defmacro stamp [n] (.getTime (js/Date. n))) (js/console.log (stamp 42))",
            "42\n",
        ),
        (
            "(defmacro stored [x] (let [o (js* \"({value: 0})\")] (set! (.-value o) x) (js/Math.max (.-value o) 2))) (js/console.log (stored 40))",
            "40\n",
        ),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(emit_js(&program))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{source}"
        );
        assert_eq!(host.native_macro_calls().len(), 1);
    }
}

#[test]
fn false_and_nil_input_occurrences_keep_language_truth() {
    let source =
        "(defmacro truthy [x] (if x 1 0)) (js/console.log (truthy nil) (truthy false) (truthy 0))";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(emit_js(&p))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "0 0 1\n");
}

#[test]
fn thrown_macro_error_names_call_and_definition() {
    let source = "(defmacro explode [] (throw (js/Error. \"macro boom\")))\n(explode)";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let message = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .err()
        .unwrap()
        .to_string();
    assert!(message.contains("macro boom"), "{message}");
    assert!(message.contains("cljs.user:2:1"), "{message}");
    assert!(message.contains("definition cljs.user:1:1"), "{message}");
}

#[test]
fn ordinary_javascript_macro_state_persists_in_source_order() {
    let source = "(defmacro bump [] (js* \"(globalThis.n = (globalThis.n || 0) + 1)\")) (js/console.log (bump) (bump))";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(emit_js(&program))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1 2\n");
    assert_eq!(host.native_macro_calls().len(), 2);
}

#[test]
fn native_occurrence_identity_survives_real_emitted_macro_and_second_import() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, ExpansionOrigin, Name, Source},
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let source = "(fn [form env x] x)";
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let function = evaluator
        .evaluate_unit(&light_cljs::emitter::emit_js_unit(&program))
        .unwrap();
    let call = host
        .read_compiler_form(&Source::new(
            "identity.cljs",
            "(same ^{:m 1} [9007199254740993 1.0 \\a])",
        ))
        .unwrap()
        .unwrap();
    let original = call.children()[1].clone();
    let environment = Datum::new(DatumKind::Collection {
        kind: light_cljs::CollectionKind::Map,
        children: Box::new([]),
    });
    for i in 0..2 {
        let result = host
            .expand_javascript(
                &evaluator,
                function,
                &call,
                &environment,
                ExpansionOrigin {
                    macro_name: Name::new(None, "same"),
                    call: call.span(),
                    definition: None,
                    helpers: vec![],
                },
            )
            .unwrap();
        assert!(
            std::ptr::eq(result.data().kind(), original.data().kind()),
            "original Datum allocation survives"
        );
        assert_eq!(result.span().source().name(), "identity.cljs");
        assert_eq!(result.span().bytes(), original.span().bytes());
        let stats = host.native_macro_calls();
        assert_eq!(stats[i].exported, 0);
        assert_eq!(stats[i].reused, 1);
        if i == 1 {
            assert_eq!(stats[i].imported, 0);
        }
    }
    evaluator.release(function).unwrap();
    // The same identity behavior also traverses normal defmacro registration.
    let source = "(defmacro same [x] x) (same (+ 40 2))";
    analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    assert_eq!(host.native_macro_calls()[0].reused, 1);
    assert_eq!(host.native_macro_calls()[0].exported, 0);
}

#[test]
fn native_environment_updates_changed_slots_without_importing_the_namespace_again() {
    use light_cljs::{
        CollectionKind,
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, Name},
        evaluator::{EvaluatorOptions, MacroEvaluator},
        form_bridge::ConversionLimits,
    };
    fn keyword(s: &str) -> Datum {
        Datum::new(DatumKind::Keyword(Name::new(None, s)))
    }
    fn number(n: i64) -> Datum {
        Datum::new(DatumKind::Integer(n))
    }
    fn map(xs: Vec<Datum>) -> Datum {
        Datum::new(DatumKind::Collection {
            kind: CollectionKind::Map,
            children: xs.into(),
        })
    }
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let ns = map(vec![
        keyword("name"),
        Datum::new(DatumKind::Symbol(Name::new(None, "large.ns"))),
        keyword("bindings"),
        Datum::new(DatumKind::Collection {
            kind: CollectionKind::Vector,
            children: (0..500).map(number).collect(),
        }),
    ]);
    let a = map(vec![keyword("ns"), ns.clone(), keyword("line"), number(1)]);
    let b = map(vec![keyword("ns"), ns, keyword("line"), number(2)]);
    let first = evaluator
        .import_environment(&a, ConversionLimits::default())
        .unwrap();
    let before = evaluator.bridge_stats();
    let second = evaluator
        .import_environment(&b, ConversionLimits::default())
        .unwrap();
    let after = evaluator.bridge_stats();
    assert_eq!(
        after.imported - before.imported,
        3,
        "only the envelope and changed key/value are imported"
    );
    evaluator.release(first).unwrap();
    evaluator.release(second).unwrap();
}

#[test]
fn declaration_bundle_registration_preserves_preloaded_roots_and_defers_its_code() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let result = evaluator.evaluate_unit(r#"rt => {
        const roots = globalThis.__lightCompilerRoots;
        roots['fixture/ready'] = 42;
        let ran = false;
        const code = () => { ran = true; throw new Error('must stay deferred'); };
        __lightRegisterBundle(['fixture/ready', 'fixture/P', 'fixture/method'], code, [], 'fixture:1:1', false, false, 7);
        __lightRegisterBundle(['fixture/P'], code, [], 'fixture:2:1', true, false, 8);
        __lightRegisterBundle([], code, [['fixture/P', null, 'fixture/Type']], 'fixture:3:1', false, false, 9);
        if (ran || roots['fixture/ready'] !== 42) throw new Error('registration changed preloaded state');
        // A bundle without a unit keeps the explicit unsupported demand.
        __lightRegisterBundle(['fixture/orphan'], code, [], 'fixture:4:1', false, false, -1);
        let message = '';
        try { roots['fixture/orphan']; } catch (error) { message = String(error); }
        if (!message.includes('unsupported operation: helper not preloaded: fixture/orphan')) throw new Error('missing demand diagnostic');
        if (ran) throw new Error('registration ran bundle code');
        return null;
    }"#).unwrap();
    evaluator.release(result).unwrap();
}

#[test]
fn unsupported_operations_are_explicit_and_do_not_fall_back() {
    assert_native_matches_legacy(
        "(defmacro sorted [] (sorted-map 1 2)) (js/console.log (pr-str (sorted)))",
        "{1 2}\n",
    );
    for (source, expected) in [
        (
            "(defprotocol Sample (value [this])) (defmacro demand [] Sample) (demand)",
            "unsupported operation: macro returned a non-form value",
        ),
        (
            "(defmacro lazy-result [] (take 3 (cycle [1 2]))) (lazy-result)",
            "unsupported operation: native macro runtime core.cycle",
        ),
        // Custom sorted comparators and transients remain unsupported.
        (
            "(defmacro sorted [] (sorted-map-by > 1 2)) (sorted)",
            "unsupported operation: native macro runtime core.sortedMapBy",
        ),
        (
            "(defmacro edit [] (transient [])) (edit)",
            "unsupported operation: native macro runtime core.transient",
        ),
        (
            "(defmacro js-collection [x] (js/Object.keys x)) (js-collection [1 2])",
            "unsupported operation: JavaScript interop with native vector form",
        ),
        (
            "(defmacro js-instance [x] (instance? js/Object x)) (js-instance [1 2])",
            "unsupported operation: JavaScript interop with native vector form",
        ),
        (
            "(defmacro sequence [x] (seq x)) (sequence 1)",
            "unsupported operation: native macro runtime seq on number",
        ),
        (
            "(defmacro sequence [x] (seq x)) (sequence :a)",
            "unsupported operation: native macro runtime seq on keyword",
        ),
        (
            "(defmacro sequence [x] (seq x)) (sequence a)",
            "unsupported operation: native macro runtime seq on symbol",
        ),
        (
            "(defmacro sequence [] (seq (js* \"({})\"))) (sequence)",
            "unsupported operation: native macro runtime seq on object",
        ),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let message = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .err()
            .unwrap()
            .to_string();
        assert!(message.contains(expected), "{source}: {message}");
    }
}

#[test]
fn demands_a_real_helper_without_replaying_the_macro() {
    let source = include_str!("fixtures/native-helper-demand/macro.cljs");
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    assert_eq!(program_output(&emit_js(&program)), "42\n");
}

#[test]
fn demanded_helper_bindings_preserve_overlays_and_restore_after_errors() {
    for (definitions, body, expected) in [
        (
            "(def ^:dynamic *x* :root) (def ^:dynamic *y* :other)",
            "[(binding [*x* nil] [*x* (binding [*x* false] *x*) *x*])
              (binding [*x* :outer] (binding [*x* *y* *y* *x*] [*x* *y*]))
              *x* *y*]",
            "[[nil false nil] [:other :outer] :root :other]\n",
        ),
        (
            "(def ^:dynamic *x* :root) (defn value [] *x*)",
            r#"[(try (binding [*x* false] (throw (js/Error. (str (value)))))
                    (catch :default e (.-message e)))
                (with-redefs [value (fn [] nil)] (value)) (value) *x*]"#,
            "[\"false\" nil :root :root]\n",
        ),
        (
            "(def effects (atom []))
             (def ^:dynamic *x* (do (swap! effects conj :root) :root))
             (defn value [] (swap! effects conj :value) *x*)",
            "(let [a (binding [*x* false] (value))
                   before @effects root *x*]
               [a before root (value) @effects])",
            "[false [:value] :root :root [:value :root :value]]\n",
        ),
        (
            "(declare ^:dynamic *x*)",
            "[(binding [*x* nil] [(exists? *x*) *x*])
              (binding [*x* false] [(exists? *x*) *x*])
              (binding [*x* js/undefined] [(exists? *x*) (nil? *x*)]) (exists? *x*)]",
            "[[true nil] [true false] [false true] false]\n",
        ),
        (
            "(def ^:dynamic *x* 1) (def ^:dynamic *y* 2)",
            r#"[(binding [*x* 3] (set! *x* 4) *x*)
                (try (binding [*x* 10 *y* (throw (js/Error. "entry"))] nil)
                     (catch :default e (.-message e))) *x* *y*]"#,
            "[4 \"entry\" 1 2]\n",
        ),
        (
            // The root fails in the macro host; Node can execute the emitted
            // program to check the captured compile-time result too.
            "(def effects (atom []))
             (def ^:dynamic *x* (if (exists? js/process) :application
                 (do (swap! effects conj :failed) (throw (js/Error. \"root failure\")))))",
            // Legacy root errors add source context. Compare the failure,
            // cached error identity and once-only effects within each host.
            r#"(let [first (try *x* (catch :default e e))
                     bound (binding [*x* nil] *x*)
                     second (try *x* (catch :default e e))]
                 [(.endsWith (.-message first) "root failure") bound
                  (.endsWith (.-message second) "root failure")
                  (identical? first second) @effects])"#,
            "[true nil true true [:failed]]\n",
        ),
    ] {
        let source = format!(
            "{definitions} (defn helper [] {body})
             (defmacro probe [] (list 'quote (helper)))
             (js/console.log (pr-str (probe)))"
        );
        assert_native_matches_legacy(&source, expected);
    }
}

#[test]
fn native_binding_snapshots_preserve_root_slots_and_belong_to_their_task() {
    use light_cljs::{
        bbp_evaluator::BbpRuntime,
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let runtime = BbpRuntime::new(&EvaluatorOptions::default()).unwrap();
    let first = runtime.evaluator().unwrap();
    let second = runtime.evaluator().unwrap();
    let run = |evaluator: &light_cljs::bbp_evaluator::BbpEvaluator, source| {
        let value = evaluator.evaluate_unit(source).unwrap();
        evaluator.release(value).unwrap();
    };
    run(
        &first,
        r#"rt => {
        const roots = globalThis.__lightCompilerRoots;
        __lightRegisterUnit('fixture/pending', 520);
        const pending = Object.getOwnPropertyDescriptor(roots, 'fixture/pending').get;
        const saved = __lightSaveCompilerBinding('fixture/pending');
        __lightBindCompilerRoot('fixture/pending', false);
        if (roots['fixture/pending'] !== false) throw new Error('missing overlay');
        __lightRestoreCompilerBinding('fixture/pending', saved);
        if (Object.getOwnPropertyDescriptor(roots, 'fixture/pending').get !== pending)
          throw new Error('pending slot replaced');
        let reads = 0;
        Object.defineProperty(roots, 'fixture/failed', {get: () => { reads++; throw new Error('sticky'); }, configurable: true});
        const failed = __lightSaveCompilerBinding('fixture/failed');
        __lightBindCompilerRoot('fixture/failed', null);
        if (roots['fixture/failed'] !== null || reads !== 0) throw new Error('save forced root');
        __lightRestoreCompilerBinding('fixture/failed', failed);
        try { roots['fixture/failed']; } catch (e) { if (e.message !== 'sticky') throw e; }
        if (reads !== 1) throw new Error('failed slot replaced');
        roots['fixture/ready'] = 42;
        globalThis.fixtureSaved = __lightSaveCompilerBinding('fixture/ready');
        __lightBindCompilerRoot('fixture/ready', null);
        return null;
    }"#,
    );
    run(
        &second,
        r#"rt => {
        if (__lightSaveCompilerBinding('fixture/ready')[0]) throw new Error('overlay crossed tasks');
        __lightRegisterUnit('fixture/ready', null);
        __lightBindCompilerRoot('fixture/ready', false);
        if (globalThis.__lightCompilerRoots['fixture/ready'] !== false) throw new Error('missing task overlay');
        return null;
    }"#,
    );
    run(
        &first,
        r#"rt => {
        const roots = globalThis.__lightCompilerRoots;
        if (roots['fixture/ready'] !== null) throw new Error('task overlay replaced');
        __lightRestoreCompilerBinding('fixture/ready', globalThis.fixtureSaved);
        if (roots['fixture/ready'] !== 42) throw new Error('initialized slot replaced');
        return null;
    }"#,
    );
}

#[test]
fn demanded_multimethod_dispatch_shapes_match_quickjs_at_every_pool_size() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    // Every row takes the demanded namespace path, including construction,
    // declarations and dispatch. Inline --text uses a different bootstrap.
    for (label, declarations, body, expected) in [
        (
            "bundle product replacement without a declaration",
            r#"
          (defmulti replacement identity)
          (defmethod replacement :x [_] :old)
          (def replacement (fn [_] :replacement))"#,
            "(replacement :x)",
            ":replacement",
        ),
        (
            "named dispatch functions",
            r#"
          (defn key-of [x] (name x))
          (defn many [& xs] (name (first xs)))
          (defmulti m key-of)
          (defmethod m "x" [_] :fixed)
          (defmulti n many)
          (defmethod n "x" [& xs] (count xs))"#,
            "[(m :x) (n :x) (n :x :y :z)]",
            "[:fixed 1 3]",
        ),
        (
            "dispatch functions from another namespace",
            r#"
          (def alias dispatch/key-of)
          (defmulti m alias)
          (defmethod m "x" [_] :fixed)
          (defmulti n dispatch/arities)
          (defmethod n "x" [& xs] (count xs))"#,
            "[(m :x) (n :x) (n :x :y :z)]",
            "[:fixed 1 3]",
        ),
        (
            "seeded methods, preferences and retained public tables",
            r#"
          (def h (atom (-> (make-hierarchy) (derive :dog :pet) (derive :dog :animal))))
          (def method-state (atom {:pet (fn [_] :pet) :animal (fn [_] :animal) 1 (fn [_] :number)}))
          (def prefer-state (atom {:pet #{:animal}}))
          (def method-cache (atom {}))
          (def cached-hierarchy (atom nil))
          (def m (MultiFn. 'seeded identity :fallback h method-state prefer-state method-cache cached-hierarchy))"#,
            r#"(let [before [(m :dog) (m 1) ((get-method m :pet) :pet)
                              (= #{:pet :animal 1} (set (keys (methods m))))
                              (= @method-state (methods m)) (= @prefer-state (prefers m))]
                     removed (identical? m (remove-method m :pet))
                     after-remove [(m :dog) (not (contains? @method-state :pet))]
                     preferred (= @h (prefer-method m :a :b))
                     after-prefer [(= {:pet #{:animal} :a #{:b}} @prefer-state)
                                   (= @prefer-state (prefers m))]
                     added (identical? m (-add-method m :fallback (fn [_] :fallback)))
                     after-add [((get @method-state :fallback) nil) (m :missing)]
                     cleared (identical? m (remove-all-methods m))]
                 [before removed after-remove preferred after-prefer added after-add cleared
                  @method-state @prefer-state @method-cache @cached-hierarchy (get-method m :missing)])"#,
            "[[:pet :number :pet true true true] true [:animal true] true [true true] true [:fallback :fallback] true {} {} {} nil nil]",
        ),
        (
            "multifns sharing constructor tables observe each others mutations",
            r#"
          (def h (atom (-> (make-hierarchy) (derive :dog :pet) (derive :dog :animal))))
          (def method-state (atom {}))
          (def prefer-state (atom {}))
          (def a (MultiFn. 'a identity :default h method-state prefer-state (atom {}) (atom nil)))
          (def b (MultiFn. 'b identity :default h method-state prefer-state (atom {}) (atom nil)))"#,
            r#"(do
                 (defmethod a :pet [_] :pet)
                 (defmethod b :animal [_] :animal)
                 (let [added [(b :pet) (a :animal) (= (methods a) (methods b))
                              (= @method-state (methods b))]
                       preferred (do (prefer-method a :pet :animal)
                                     [(b :dog) (= (prefers b) @prefer-state)])
                       removed (do (remove-method b :pet)
                                   [(contains? (methods a) :pet) (= @method-state (methods a))])
                       preferred-back (do (prefer-method b :x :y)
                                          [(= (prefers a) (prefers b)) (= #{:y} (get (prefers a) :x))])
                       cleared (do (remove-all-methods a)
                                   [(methods b) (prefers b) @method-state @prefer-state])]
                   (defmethod b :x [_] :restored)
                   [added preferred removed preferred-back cleared (a :x)]))"#,
            "[[:pet :animal true true] [:pet true] [false true] [true true] [{} {} {} {}] :restored]",
        ),
        (
            "independently shared method and preference tables",
            r#"
          (def h (atom (make-hierarchy)))
          (def method-state (atom {}))
          (def prefer-state (atom {}))
          (def a (MultiFn. 'a identity :default h method-state (atom {}) (atom {}) (atom nil)))
          (def b (MultiFn. 'b identity :default h method-state prefer-state (atom {}) (atom nil)))
          (def c (MultiFn. 'c identity :default h (atom {}) prefer-state (atom {}) (atom nil)))"#,
            r#"(do
                 (defmethod a :x [_] :x)
                 (defmethod c :y [_] :y)
                 (prefer-method c :p :q)
                 (let [before [(b :x) (nil? (get-method b :y)) (nil? (get-method c :x))
                               (= {:p #{:q}} (prefers b)) (empty? (prefers a))]]
                   (remove-all-methods b)
                   [before (methods a) (prefers a) (c :y) (prefers c)]))"#,
            "[[:x true true true true] {} {} :y {}]",
        ),
        (
            "native type hierarchy tags",
            r#"
          (deftype T [])
          (defrecord R [])
          (def h (atom (-> (make-hierarchy) (derive T ::pet) (derive R ::pet) (derive ::pet ::animal))))
          (defmulti m identity :hierarchy h)
          (defmethod m ::animal [_] :animal)
          (defmethod m :default [_] :fallback)"#,
            r#"(let [local [(m T) (m R) (isa? @h T ::animal) (isa? @h R ::animal)
                            (isa? @h T js/Function) (isa? @h R js/Object)
                            (contains? (parents @h T) ::pet) (contains? (ancestors @h R) ::animal)
                            (try (descendants @h T) (catch :default e (= "Can't get descendants of constructors" (.-message e))))]]
                 (derive T ::global) (derive R ::global)
                 (let [global [(isa? T ::global) (isa? R ::global)]]
                   (underive T ::global) (underive R ::global)
                   (swap! h underive T ::pet)
                   [local global (isa? T ::global) (isa? R ::global) (m T) (m R)]))"#,
            "[[:animal :animal true true true true true true true] [true true] false false :fallback :animal]",
        ),
        (
            "multimethod instances and callable predicates",
            r#"
          (defmulti m identity)
          (defmethod m :x [_] :x)
          (def C MultiFn)
          (def check instance?)
          (def function? fn?)
          (def callable? ifn?)"#,
            "[(instance? MultiFn m) (instance? C m) (check MultiFn m) (check C m)
              (fn? m) (ifn? m) (function? m) (callable? m) (instance? MultiFn {})
              (fn? identity) (ifn? identity) (fn? :x) (ifn? :x)]",
            "[true true true true false true false true false true true false true]",
        ),
        (
            "all native callable predicates",
            r#"
          (deftype T [])
          (defrecord R [])
          (deftype Mutable [^:mutable x])
          (deftype Callable [] IFn (-invoke [_ x] x))
          (deftype FunctionLike [] Fn IFn (-invoke [_ x] x))
          (defmulti m identity)
          (def function? fn?)
          (def callable? ifn?)"#,
            r#"[(fn? T) (ifn? T) (function? T) (callable? T)
                (fn? R) (ifn? R) (function? R) (callable? R)
                (mapv (fn [x] [(fn? x) (ifn? x) (function? x) (callable? x)])
                  [Mutable Callable FunctionLike MultiFn identity (fn [x] x)
                   (FunctionLike.)
                   m :x 'x [] {} #{} (Callable.)
                   (T.) (R.) (Mutable. 0) nil false 1 "x" (list) (js/Array.)])]"#,
            "[true true true true true true true true [[true true true true] [true true true true] [true true true true] [true true true true] [true true true true] [true true true true] [true true true true] [false true false true] [false true false true] [false true false true] [false true false true] [false true false true] [false true false true] [false true false true] [false false false false] [false false false false] [false false false false] [false false false false] [false false false false] [false false false false] [false false false false] [false false false false] [false false false false]]]",
        ),
        (
            "scalar and structural keys",
            r#"
          (defmulti m identity)
          (defmethod m :x [_] :keyword)
          (defmethod m 'x [_] :symbol)
          (defmethod m "x" [_] :string)
          (defmethod m nil [_] :nil)
          (defmethod m false [_] :false)
          (defmethod m 3 [_] :number)
          (defmethod m [1 :x] [_] :vector)
          (defmethod m {:x 1} [_] :map)
          (defmethod m #{:x :y} [_] :set)
          (defmethod m :default [x] [:default x])"#,
            r#"(mapv m [:x 'x "x" nil false 3 [1 :x] {:x 1} #{:y :x} :other])"#,
            "[:keyword :symbol :string :nil :false :number :vector :map :set [:default :other]]",
        ),
        (
            "constructor and function dispatch",
            r#"
          (defmulti m identity)
          (defmethod m js/Function [_] :function)
          (defmethod m js/Array [_] :array)
          (defmethod m :default [_] :fallback)"#,
            "[(m js/Array) (m (fn [])) (m js/Object) (m :other)]",
            "[:array :function :function :fallback]",
        ),
        (
            "keyword dispatch and custom default",
            r#"
          (defmulti m :kind :default :fallback)
          (defmethod m :x [x] (:value x))
          (defmethod m :fallback [x] [:fallback (:kind x)])"#,
            "[(m {:kind :x :value false}) (m {:kind :other}) (m {})]",
            "[false [:fallback :other] [:fallback nil]]",
        ),
        (
            "arguments and apply",
            r#"
          (defmulti m (fn [& xs] (count xs)))
          (defmethod m 0 [] :zero)
          (defmethod m 2 ([a b] (+ a b)) ([a b c] (+ a b c)))
          (defmethod m :default [a & xs] (into [a] xs))"#,
            "[(m) (m 1 2) (apply m [1 2 3]) ((get-method m 2) 4 5)]",
            "[:zero 3 [1 2 3] 9]",
        ),
        (
            "hierarchy and nested vector dispatch",
            r#"
          (def h (atom (-> (make-hierarchy) (derive :dog :pet) (derive :pet :animal))))
          (defmulti m identity :hierarchy h)
          (defmethod m :animal [x] [:animal x])
          (defmethod m [:animal [:animal :animal]] [_] :nested)
          (defmethod m :default [_] :fallback)"#,
            "[(m :dog) (m [:dog [:pet :dog]]) (m [:dog]) (isa? @h :dog :animal)
            (= #{:pet} (parents @h :dog)) (= #{:pet :animal} (ancestors @h :dog))
            (= #{:dog :pet} (descendants @h :animal))
            (isa? (underive @h :dog :pet) :dog :animal)]",
            "[[:animal :dog] :nested :fallback true true true true false]",
        ),
        (
            "global hierarchy",
            r#"
          (defmulti m identity)
          (defmethod m ::animal [_] :animal)
          (defmethod m :default [_] :fallback)"#,
            "(do (derive ::dog ::animal) (let [a (m ::dog)] (underive ::dog ::animal) [a (m ::dog)]))",
            "[:animal :fallback]",
        ),
        (
            "preferences, mutation and failure messages",
            r#"
          (def h (atom (-> (make-hierarchy) (derive :dog :animal) (derive :dog :pet))))
          (defmulti m identity :hierarchy h :default :fallback)
          (defmethod m :animal [_] :animal)
          (defmethod m :pet [_] :pet)
          (defmethod m :fallback [_] :fallback)"#,
            r#"(let [ambiguous (try (m :dog) (catch :default e (.includes (.-message e) "Multiple methods")))
                    p (prefer-method m :pet :animal)
                    chosen (m :dog)
                    preferences (= {:pet #{:animal}} (prefers m))
                    conflict (try (prefer-method m :animal :pet) (catch :default e (.includes (.-message e) "Preference conflict")))
                    removed (do (remove-method m :pet) (m :dog))
                    cut (do (swap! h underive :dog :animal) (m :dog))
                    restored (do (swap! h derive :dog :animal) (m :dog))]
                (remove-all-methods m)
                [ambiguous chosen preferences conflict removed cut restored
                 (empty? (methods m)) (empty? (prefers m)) (nil? (get-method m :dog))
                 (try (m :dog) (catch :default e (.includes (.-message e) "No method in multimethod 'helpers/m'")))])"#,
            "[true :pet true true :animal :fallback :animal true true true true]",
        ),
        (
            "exact method precedes preferred ancestor",
            r#"
          (def h (atom (derive (make-hierarchy) :dog :animal)))
          (defmulti m identity :hierarchy h)
          (defmethod m :dog [_] :dog)
          (defmethod m :animal [_] :animal)"#,
            "(do (prefer-method m :animal :dog) [(m :dog) ((get-method m :dog) :dog)])",
            "[:dog :dog]",
        ),
        (
            "explicit construction and protocol operations",
            "",
            r#"(let [m (MultiFn. 'custom identity :fallback (atom (make-hierarchy))
                                 (atom {}) (atom {}) (atom {}) (atom nil))]
                 (-add-method m :x (fn [x] [:x x]))
                 (-add-method m :fallback (fn [_] :fallback))
                 (let [a (m :x) b ((-get-method m :x) :x) c (apply m [:other])
                       d (= (methods m) (-methods m)) e ((-dispatch-fn m) :dispatch)
                       f (-default-dispatch-val m)]
                   (-remove-method m :x)
                   (-prefer-method m :a :b)
                   [a b c d e f (-prefers m) (identical? m (-reset m)) (empty? (-methods m))]))"#,
            "[[:x :x] [:x :x] :fallback true :dispatch :fallback {:a #{:b}} true true]",
        ),
        (
            "declaration frontier and replacement",
            r#"
          (defmulti m identity)
          (defmethod m :x [_] :first)
          (defmacro snapshot [] (list 'quote (m :x)))
          (defn early [] (snapshot))
          (defmethod m :x [_] :second)"#,
            "[(m :x) (early) (m :x)]",
            "[:second :first :second]",
        ),
        (
            "declaration is not an initialized defonce",
            r#"
          (declare m)
          (defmulti m identity)
          (defmulti m (throw (js/Error. "declared defonce replay")))
          (defmethod m :x [_] :declared)
          (defmethod m :default [_] :fallback)
          (declare replacement)
          (defmulti replacement identity)
          (defmethod replacement :x [_] :old)
          (def replacement (fn [_] :replacement))"#,
            "[(m :x) (m :other) (replacement :x)]",
            "[:declared :fallback :replacement]",
        ),
        (
            "defonce and binding restoration",
            r#"
          (def ^:dynamic *x* :root)
          (defmulti m identity)
          (defmulti m (throw (js/Error. "defonce replay")))
          (defmethod m :x [_] *x*)
          (defmethod m :throw [_] (throw (js/Error. "bound")))
          (defmethod m :default [_] *x*)"#,
            r#"[(binding [*x* nil] (m :x)) (binding [*x* false] (m :other))
              (try (binding [*x* :bound] (m :throw)) (catch :default e (.-message e))) (m :x)]"#,
            "[nil false \"bound\" :root]",
        ),
    ] {
        let root = temp_dir::TempDir::new("native-multimethod-shapes");
        std::fs::write(
            root.join("dispatch.cljs"),
            "(ns dispatch) (defn key-of [x] (name x)) (defn arities ([x] (name x)) ([x & _] (name x)))",
        )
        .unwrap();
        std::fs::write(
            root.join("helpers.cljs"),
            format!("(ns helpers (:require [dispatch :as dispatch])) {declarations} (defmacro probe [] (list 'quote {body}))"),
        )
        .unwrap();
        std::fs::write(
            root.join("app.cljs"),
            "(ns app (:require [helpers :refer [probe]])) (js/console.log (pr-str (probe)))",
        )
        .unwrap();
        let build = |native, jobs| {
            let mut host = MacroHost::new();
            if native {
                host.use_native_macros();
            }
            let (program, _) = analyze_namespace_entry_recorded(
                "app",
                &[root.path().to_path_buf()],
                &mut host,
                Default::default(),
                &mut Vec::new(),
                EntryRecorders {
                    slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|e| panic!("{label}, native {native}, jobs {jobs}: {e}"));
            emit_js(&program)
        };
        let mut reference = None;
        for jobs in [1, 2, 4, 8] {
            let native = build(true, jobs);
            assert_eq!(
                program_output(&native),
                format!("{expected}\n"),
                "{label}, jobs {jobs}"
            );
            if let Some(previous) = &reference {
                assert_eq!(&native, previous, "{label}, jobs {jobs}");
            }
            reference = Some(native.clone());
            #[cfg(feature = "quickjs")]
            assert_eq!(native, build(false, jobs), "{label}, jobs {jobs}");
        }
    }
}

#[test]
fn multifns_sharing_constructor_tables_observe_each_others_mutations() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("native-shared-multimethod-tables");
    for (file, source) in [
        (
            "helpers.cljs",
            r#"(ns helpers)
          (def h (atom (-> (make-hierarchy) (derive :dog :pet) (derive :dog :animal))))
          (def method-state (atom {}))
          (def prefer-state (atom {}))
          (defn new-multi [name]
            (MultiFn. name identity :default h method-state prefer-state (atom {}) (atom nil)))
          (def a (new-multi 'a))
          (def b (new-multi 'b))
          (defmacro install []
            (-add-method a :x (fn [_] :x))
            (-add-method a :pet (fn [_] :pet))
            (-add-method a :animal (fn [_] :animal))
            (prefer-method a :pet :animal)
            (list 'quote :installed))
          (defmacro probe []
            (let [x (b :x) dog (b :dog)]
              (remove-method b :x)
              (prefer-method b :p :q)
              (list 'quote [x dog (contains? (methods a) :x) (= (prefers a) (prefers b))])))
          (defmacro finish []
            (let [c (new-multi 'c)
                  before [(contains? (methods c) :x) (= (methods a) (methods c))
                          (= #{:q} (get (prefers c) :p))]]
              (remove-all-methods c)
              (-add-method a :y (fn [_] :y))
              (list 'quote (into before [(c :y) (= (methods b) @method-state) (empty? (prefers b))]))))"#,
        ),
        (
            "a.cljs",
            "(ns a (:require [helpers :refer [install]])) (def value (install))",
        ),
        (
            "b.cljs",
            "(ns b (:require [helpers :refer [probe]])) (def value (probe))",
        ),
        (
            "app.cljs",
            "(ns app (:require [a] [b] [helpers :refer [finish]]))
          (js/console.log (pr-str [a/value b/value (finish)]))",
        ),
    ] {
        std::fs::write(root.join(file), source).unwrap();
    }
    let build = |native, jobs, seed| {
        let mut host = MacroHost::new();
        if native {
            host.use_native_macros();
        }
        let (program, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                #[cfg(debug_assertions)]
                perturbation: native.then_some(light_cljs::scheduler::Perturbation {
                    seed,
                    limits: Some(light_cljs::scheduler::Limits {
                        batch_jobs: 1,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("native {native}, jobs {jobs}, seed {seed}: {e}"));
        emit_js(&program)
    };
    let reference = build(true, 1, 0);
    assert_eq!(
        program_output(&reference),
        "[:installed [:x :pet false true] [false true true :y true true]]\n"
    );
    let seeds = std::env::var("LIGHT_CLJS_STRESS_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    for jobs in [1, 2, 4, 8] {
        #[cfg(feature = "quickjs")]
        assert_eq!(reference, build(false, jobs, 0), "legacy, jobs {jobs}");
        for seed in 1..=seeds {
            assert_eq!(
                reference,
                build(true, jobs, seed),
                "jobs {jobs}, seed {seed}"
            );
        }
    }
}

#[test]
fn demanded_multimethods_reject_task_owned_dispatch_without_replaying_effects() {
    let root = temp_dir::TempDir::new("native-multimethod-task-owned");
    std::fs::write(root.join("helpers.cljs"), r#"(ns helpers)
      (def effects (atom 0))
      (def dispatch (let [state (js/Array.)] (fn [x] (.push state x) x)))
      (defmulti m (do (swap! effects inc) dispatch))
      (defmethod m :x [_] :x)
      (defmacro probe [] (list 'quote
        [(try (m :x) (catch :default e (.includes (.-message e) "native multimethod captures task-owned state"))) @effects]))"#).unwrap();
    for (file, source) in [
        (
            "a.cljs",
            "(ns a (:require [helpers :refer [probe]])) (def value (probe))",
        ),
        (
            "b.cljs",
            "(ns b (:require [helpers :refer [probe]])) (def value (probe))",
        ),
        (
            "app.cljs",
            "(ns app (:require [a] [b] [helpers :refer [probe]])) (js/console.log (pr-str [a/value b/value (probe)]))",
        ),
    ] {
        std::fs::write(root.join(file), source).unwrap();
    }
    let mut reference = None;
    for jobs in [1, 2, 4, 8] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("jobs {jobs}: {e}"));
        let js = emit_js(&program);
        assert_eq!(
            program_output(&js),
            "[[true 1] [true 1] [true 1]]\n",
            "jobs {jobs}"
        );
        if let Some(previous) = &reference {
            assert_eq!(&js, previous, "jobs {jobs}");
        }
        reference = Some(js);
    }
}

#[test]
fn demanded_multimethod_effects_run_once_across_suspension_and_lending() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("native-multimethod-effects");
    for (file, source) in [
        (
            "helpers.cljs",
            r#"(ns helpers)
          (def events (atom []))
          (def ^:dynamic *x* :root)
          (def captured (do (swap! events conj [:capture *x*]) *x*))
          (defn leaf [] (swap! events conj [:leaf *x*]) captured)
          (defmulti m (do (swap! events conj [:construct *x*])
                       (fn [tag] (swap! events conj [:dispatch tag *x*]) tag)))
          (defmulti m (throw (js/Error. "repeated defonce")))
          (defmethod m (do (swap! events conj [:register *x*]) :x) [_]
            (let [a (leaf)] [a *x*]))
          (defmethod m :throw [_] (throw (js/Error. "bound")))
          (defmethod m :default [_] *x*)
          (defn helper [tag]
            (binding [*x* tag]
              [(m :x) (binding [*x* false] (apply m [:missing]))
               (try (binding [*x* nil] (m :throw)) (catch :default e (.-message e))) *x*]))
          (defmacro probe [tag] (list 'quote [(helper tag) *x*]))
          (defmacro log [] (list 'quote @events))"#,
        ),
        (
            "a.cljs",
            "(ns a (:require [helpers :refer [probe]])) (def value (probe :a))",
        ),
        (
            "b.cljs",
            "(ns b (:require [helpers :refer [probe]])) (def value (probe :b))",
        ),
        (
            "app.cljs",
            "(ns app (:require [a] [b] [helpers :refer [probe log]]))
          (js/console.log (pr-str [a/value b/value (probe :app) (log)]))",
        ),
    ] {
        std::fs::write(root.join(file), source).unwrap();
    }
    let build = |native, jobs, seed, held| {
        let mut host = MacroHost::new();
        if native {
            host.use_native_macros();
        }
        let mut stats = light_cljs::CompileStats::default();
        let (program, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                stats: Some(&mut stats),
                #[cfg(debug_assertions)]
                perturbation: native.then_some(light_cljs::scheduler::Perturbation {
                    seed,
                    lane_hold: if held {
                        vec!["a/value".into()]
                    } else {
                        Vec::new()
                    },
                    limits: Some(light_cljs::scheduler::Limits {
                        batch_jobs: 1,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("native {native}, jobs {jobs}, seed {seed}, held {held}: {e}"));
        if native {
            assert!(stats.lanes.entered > 0);
            if held {
                assert!(stats.lanes.inherited > 0, "no inherited multimethod lane");
            }
        }
        emit_js(&program)
    };
    let reference = build(true, 1, 0, false);
    assert_eq!(
        program_output(&reference),
        "[[[[:a :a] false \"bound\" :a] :root] [[[:a :b] false \"bound\" :b] :root] \
       [[[:a :app] false \"bound\" :app] :root] \
       [[:construct :a] [:dispatch :x :a] [:register :a] [:leaf :a] [:capture :a] \
        [:dispatch :missing false] [:dispatch :throw nil] \
        [:dispatch :x :b] [:leaf :b] [:dispatch :missing false] [:dispatch :throw nil] \
        [:dispatch :x :app] [:leaf :app] [:dispatch :missing false] [:dispatch :throw nil]]]\n"
    );
    let seeds = std::env::var("LIGHT_CLJS_STRESS_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    // With two later callers (b and app), holding a requires at least one
    // more worker to start a. The hold deliberately disables starvation
    // rescue; exercise that barrier at four/eight slots, as in the binding
    // regressions, and the ordinary scheduler at all four pool sizes.
    for (jobs, held) in [
        (1, false),
        (2, false),
        (4, false),
        (8, false),
        (4, true),
        (8, true),
    ] {
        #[cfg(feature = "quickjs")]
        assert_eq!(
            reference,
            build(false, jobs, 0, false),
            "legacy, jobs {jobs}"
        );
        for seed in 1..=seeds {
            assert_eq!(
                reference,
                build(true, jobs, seed, held),
                "jobs {jobs}, seed {seed}, held {held}"
            );
        }
    }
}

#[test]
fn demanded_helper_bindings_keep_once_only_effects_at_one_two_four_and_eight_slots() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("native-helper-bindings");
    for (file, source) in [
        (
            "helpers.cljs",
            r#"(ns helpers)
            (def events (atom []))
            (def ^:dynamic *x* :root)
            (def captured *x*)
            (def prepared (binding [*x* :setup] :ready))
            (defn leaf [tag] (swap! events conj [tag *x*]) *x*)
            (defn helper [tag]
              (let [ready prepared]
               (binding [*x* tag]
                (let [first captured
                      outer (leaf :outer)
                      inner (with-redefs [leaf (fn [_] *x*)]
                              (binding [*x* false] (leaf :inner)))
                      thrown (try (binding [*x* nil]
                                    (leaf :throw)
                                    (throw (js/Error. "bound")))
                                  (catch :default e (.-message e)))]
                  [first outer inner thrown (leaf :after) *x* ready]))))
            (defn fail [] (binding [*x* :failed] (throw (js/Error. "binding failure"))))
            (defmacro probe [tag] (list 'quote [(helper tag) *x*]))
            (defmacro log [] (list 'quote @events))"#,
        ),
        (
            "a.cljs",
            "(ns a (:require [helpers :refer [probe]])) (def value (probe :a))",
        ),
        (
            "b.cljs",
            "(ns b (:require [helpers :refer [probe]])) (def value (probe :b))",
        ),
        (
            "app.cljs",
            "(ns app (:require [a] [b] [helpers :refer [probe log]]))
             (js/console.log (pr-str [a/value b/value (probe :app) (log)]))",
        ),
        (
            "error.cljs",
            "(ns error (:require [helpers :as h]))
             (defmacro probe [] (h/fail)) (probe)",
        ),
    ] {
        std::fs::write(root.join(file), source).unwrap();
    }
    let mut reference = None;
    for (jobs, held) in [(1, false), (2, false), (4, false), (8, false), (4, true)] {
        let build = |entry, native| {
            let mut host = MacroHost::new();
            if native {
                host.use_native_macros();
            }
            let mut stats = light_cljs::CompileStats::default();
            let result = analyze_namespace_entry_recorded(
                entry,
                &[root.path().to_path_buf()],
                &mut host,
                Default::default(),
                &mut Vec::new(),
                EntryRecorders {
                    stats: Some(&mut stats),
                    slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                    #[cfg(debug_assertions)]
                    perturbation: native.then_some(light_cljs::scheduler::Perturbation {
                        seed: 520,
                        lane_hold: if held {
                            vec!["a/value".into()]
                        } else {
                            Vec::new()
                        },
                        limits: Some(light_cljs::scheduler::Limits {
                            batch_jobs: 1,
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            );
            if native && result.is_ok() {
                assert!(stats.native_helpers.units_evaluated > 0);
                assert!(stats.lanes.entered > 0);
                #[cfg(debug_assertions)]
                if held {
                    assert!(stats.lanes.inherited > 0, "no inherited binding lane");
                }
            }
            result.map(|(program, _)| emit_js(&program))
        };
        let native = build("app", true).unwrap_or_else(|e| panic!("jobs {jobs}, held {held}: {e}"));
        assert_eq!(
            program_output(&native),
            "[[[:a :a false \"bound\" :a :a :ready] :root] [[:a :b false \"bound\" :b :b :ready] :root] \
             [[:a :app false \"bound\" :app :app :ready] :root] \
             [[:outer :a] [:throw nil] [:after :a] [:outer :b] [:throw nil] [:after :b] \
             [:outer :app] [:throw nil] [:after :app]]]\n",
            "jobs {jobs}"
        );
        if let Some(previous) = &reference {
            assert_eq!(&native, previous, "jobs {jobs}");
        }
        reference = Some(native.clone());
        let error = build("error", true).unwrap_err().to_string();
        assert!(
            error.contains("binding failure") && error.contains("macro error/probe"),
            "{error}"
        );
        #[cfg(feature = "quickjs")]
        {
            assert_eq!(native, build("app", false).unwrap(), "jobs {jobs}");
            let legacy = build("error", false).unwrap_err().to_string();
            assert!(
                legacy.contains("binding failure") && legacy.contains("macro error/probe"),
                "{legacy}"
            );
        }
    }
}

#[test]
fn inherited_initializers_use_the_earlier_callers_bindings_under_seeded_scheduling() {
    inherited_initializer_bindings(false);
}

#[test]
fn indirect_initializers_use_the_earlier_callers_bindings_under_seeded_scheduling() {
    inherited_initializer_bindings(true);
}

fn inherited_initializer_bindings(indirect: bool) {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let root = temp_dir::TempDir::new("native-inherited-bindings");
    for (file, source) in [
        (
            "helpers.cljs",
            r#"(ns helpers)
            (def events (atom []))
            (def ^:dynamic *x* "root")
            (def ^:dynamic *y* :root)
            (def scalar (binding [*y* :inside]
                          (swap! events conj :scalar) [*x* *y*]))
            (def object (binding [*y* false]
                          (swap! events conj :object) [(aget *x* 0) *y*]))
            (def via-scalar (binding [*x* :helper] scalar))
            (def via-object (binding [*x* (js/Array. "helper-object")] object))
            (defmacro earlier []
              (let [a (binding [*x* :earlier *y* :outside] SCALAR)
                    value (js/Array. "earlier-object")
                    b (binding [*x* value *y* nil] [OBJECT (identical? *x* value)])]
                (list 'quote [a b *x* *y*])))
            (defmacro later []
              (let [a scalar b object]
                (list 'quote [a b (binding [*x* :later *y* :later] [scalar object *x* *y*]) *x* *y*])))
            (defmacro log [] (list 'quote @events))"#,
        ),
        (
            "a.cljs",
            "(ns a (:require [helpers :refer [earlier]])) (def value (earlier))",
        ),
        (
            "b.cljs",
            "(ns b (:require [helpers :refer [later]])) (def value (later))",
        ),
        (
            "app.cljs",
            "(ns app (:require [a] [b] [helpers :refer [log]]))
             (js/console.log (pr-str [a/value b/value (log)]))",
        ),
    ] {
        let source = source
            .replace("SCALAR", if indirect { "via-scalar" } else { "scalar" })
            .replace("OBJECT", if indirect { "via-object" } else { "object" });
        std::fs::write(root.join(file), source).unwrap();
    }
    let build = |native, jobs, seed, held| {
        let mut host = MacroHost::new();
        if native {
            host.use_native_macros();
        }
        let mut stats = light_cljs::CompileStats::default();
        let (program, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                stats: Some(&mut stats),
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                #[cfg(debug_assertions)]
                perturbation: native.then_some(light_cljs::scheduler::Perturbation {
                    seed,
                    lane_hold: if held {
                        vec!["a/value".into()]
                    } else {
                        Vec::new()
                    },
                    limits: Some(light_cljs::scheduler::Limits {
                        batch_jobs: 1,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("native {native}, jobs {jobs}, seed {seed}, held {held}: {e}"));
        #[cfg(debug_assertions)]
        if native && held {
            assert!(
                stats.lanes.inherited > 0,
                "no inherited initializer: {:?}",
                stats.lanes
            );
        }
        emit_js(&program)
    };
    let reference = build(true, 1, 0, false);
    let expected = "[[[:earlier :inside] [[\"earlier-object\" false] true] \"root\" :root] \
         [[:earlier :inside] [\"earlier-object\" false] \
         [[:earlier :inside] [\"earlier-object\" false] :later :later] \"root\" :root] [:scalar :object]]\n";
    let expected = if indirect {
        expected
            .replace(":earlier", ":helper")
            .replace("earlier-object", "helper-object")
    } else {
        expected.into()
    };
    assert_eq!(program_output(&reference), expected);
    #[cfg(feature = "quickjs")]
    for jobs in [1, 2, 4, 8] {
        assert!(
            build(false, jobs, 0, false) == reference,
            "QuickJS jobs {jobs}"
        );
    }
    let seeds = std::env::var("LIGHT_CLJS_STRESS_SEEDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1u64);
    for seed in 1..=seeds {
        for jobs in [2, 4, 8] {
            for held in [false, true] {
                // Eight slots leave workers for the later caller while the
                // hook holds the earlier unit and its helper productions.
                if held && jobs < 8 {
                    continue;
                }
                let built = build(true, jobs, seed, held);
                assert_eq!(
                    program_output(&built),
                    program_output(&reference),
                    "jobs {jobs}, seed {seed}, held {held}"
                );
                assert!(built == reference, "jobs {jobs}, seed {seed}, held {held}");
            }
        }
    }
}

#[test]
fn source_macros_expand_inside_on_demand_helpers() {
    for source in [
        "(defmacro one [] 1) (def v (one)) (defmacro read-v [] v) (js/console.log (read-v))",
        "(defmacro one [] 1) (defn f [] (one)) (defmacro call-f [] (f)) (js/console.log (call-f))",
    ] {
        assert_native_matches_legacy(source, "1\n");
    }
}

#[test]
fn native_installed_macros_expand_in_demanded_helpers() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    use light_cljs::namespace_artifact::NamespaceCapture;

    let root = temp_dir::TempDir::new("native-installed-helper-macros");
    std::fs::create_dir(root.join("lib")).unwrap();
    std::fs::write(
        root.join("lib/macros.cljs"),
        "(ns lib.macros) \
         (def effects (atom [])) \
         (defmacro one [tag] (swap! effects conj tag) 1) \
         (defmacro seen [] (list 'quote @effects))",
    )
    .unwrap();
    std::fs::write(
        root.join("app.cljs"),
        "(ns app (:require [lib.macros :refer [one seen]])) \
         (def v (+ (one :v1) (one :v2))) \
         (defn f [] (+ (one :f1) (one :f2))) \
         (defmacro read-v [] v) (defmacro call-f [] (f)) \
         (js/console.log (one :direct) (read-v) (call-f) (pr-str (seen)))",
    )
    .unwrap();
    for jobs in [1, 2] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut artifact = None;
        let mut capture = |captured: NamespaceCapture| {
            if captured.namespace == "lib.macros" {
                artifact = Some(captured.artifact.expect("native macro-provider artifact"));
            }
        };
        let (cold, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                capture: Some(&mut capture),
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
        let artifact = artifact.expect("captured macro provider");
        // The warm build has a new native runtime. Only the provider is
        // installed; its consumer and both demanded helpers analyze fresh.
        drop(host);
        let lookup = |name: &str| (name == "lib.macros").then_some(&artifact);
        let reservation = light_cljs::generated_names::Reservation::of([&artifact.names]);
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let (warm, _) = analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                install: Some(&lookup),
                reserve: Some(&reservation),
                stats: Some(&mut stats),
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("jobs {jobs}: {error}"));
        let installed = stats.install.unwrap();
        assert_eq!(installed.installed, ["lib.macros"], "jobs {jobs}");
        assert!(installed.refused.is_empty(), "{installed:?}");
        let cold = emit_js(&cold);
        let warm = emit_js(&warm);
        let expected = "1 2 2 [:direct :v1 :v2 :f1 :f2]\n";
        assert_eq!(program_output(&cold), expected, "cold, jobs {jobs}");
        assert_eq!(program_output(&warm), expected, "warm, jobs {jobs}");
        assert_eq!(warm, cold, "jobs {jobs}");
    }
}

#[test]
fn nested_helper_macros_keep_generations_and_execute_effects_once() {
    assert_native_matches_legacy(
        "(def effects (atom [])) \
         (defmacro one [] (swap! effects conj :one) 1) \
         (def a (one)) (defn f [] (one)) \
         (defmacro old [] [(f) a]) (def b (old)) \
         (defmacro one [] (swap! effects conj :new) 9) \
         (def c (one)) \
         (defmacro read-all [] (list 'quote [a b c @effects])) \
         (js/console.log (pr-str (read-all)))",
        "[1 [1 1] 9 [:one :one :new]]\n",
    );
}

// FRONTIER_REPLAY from the compiler macro suite, with its result printed.
const HELPER_FRONTIER: &str = "(defprotocol P (p [x])) (defprotocol Q (q [x])) (deftype T []) (extend-type T P (p [_] 1)) \
     (extend-type T Q (q [_] 10)) (defmacro snapshot [] [(p (T.)) (q (T.))]) (def earlier (snapshot)) \
     (extend-type T P (p [_] 2)) (defmacro warm [] (q (T.)) (p (T.)) nil) (warm) \
     (defmacro read-earlier [] earlier) \
     (js/console.log (pr-str [(read-earlier) (snapshot) (p (T.)) (q (T.))]))";

#[test]
fn on_demand_helper_macros_match_legacy_at_one_and_two_slots() {
    let root = temp_dir::TempDir::new("native-nested-macros");
    for (source, expected) in [
        (
            "(defmacro one [] 1) \
             (def v (one)) (defn f [] (one)) \
             (defmacro read-v [] v) (defmacro call-f [] (f)) \
             (def a (read-v)) (def b (call-f)) \
             (js/console.log a b)",
            "1 1\n",
        ),
        (HELPER_FRONTIER, "[[1 10] [2 10] 2 10]\n"),
    ] {
        std::fs::write(root.join("app.cljs"), format!("(ns app) {source}")).unwrap();
        let mut outputs = Vec::new();
        for jobs in [1, 2] {
            for native in [true, false] {
                let mut host = MacroHost::new();
                if native {
                    host.use_native_macros();
                }
                let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
                    "app",
                    &[root.path().to_path_buf()],
                    &mut host,
                    Default::default(),
                    &mut Vec::new(),
                    light_cljs::compiler::EntryRecorders {
                        slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                        ..Default::default()
                    },
                )
                .unwrap_or_else(|error| panic!("jobs {jobs}, native {native}: {error}"));
                let js = emit_js(&program);
                assert_eq!(
                    program_output(&js),
                    expected,
                    "jobs {jobs}, native {native}"
                );
                outputs.push(js);
            }
        }
        for output in &outputs[1..] {
            assert_eq!(output, &outputs[0]);
        }
    }
}

#[test]
fn cached_forms_still_obey_conversion_limits() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        compiler_data::Source,
        evaluator::{EvaluatorOptions, MacroEvaluator},
        form_bridge::ConversionLimits,
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    let form = host
        .read_compiler_form(&Source::new("limits.cljs", "[1 [2 3]]"))
        .unwrap()
        .unwrap();
    let value = evaluator
        .import_form(&form, ConversionLimits::default())
        .unwrap();
    evaluator.release(value).unwrap();
    for limits in [
        ConversionLimits {
            nodes: 2,
            depth: 128,
        },
        ConversionLimits {
            nodes: 100,
            depth: 1,
        },
    ] {
        let error = evaluator.import_form(&form, limits).unwrap_err();
        assert!(error.0.contains("form conversion limit"), "{error}");
    }
}

#[test]
fn native_execution_observes_shared_cancellation_and_fuel() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions {
        poll_budget: 50,
        ..Default::default()
    })
    .unwrap();
    let function = evaluator
        .evaluate_unit("rt => () => {while(true){}} ")
        .unwrap();
    let error = evaluator.call(function, &[]).unwrap_err();
    assert!(error.0.contains("poll budget"), "{error}");
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    evaluator.control().cancel();
    let error = evaluator.evaluate_unit("rt => 1").unwrap_err();
    assert!(error.0.contains("cancel"), "{error}");
}

#[test]
fn extracted_native_metadata_reuses_the_original_datum_and_occurrence() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, ExpansionOrigin, Name, Source},
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let source = "(fn [form env x] (meta x))";
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let function = evaluator
        .evaluate_unit(&light_cljs::emitter::emit_js_unit(&program))
        .unwrap();
    let call = host
        .read_compiler_form(&Source::new("meta.cljs", "(metadata ^{:m 1} [2])"))
        .unwrap()
        .unwrap();
    let original = call.children()[1].metadata().unwrap();
    let environment = Datum::new(DatumKind::Collection {
        kind: light_cljs::CollectionKind::Map,
        children: Box::new([]),
    });
    let result = host
        .expand_javascript(
            &evaluator,
            function,
            &call,
            &environment,
            ExpansionOrigin {
                macro_name: Name::new(None, "metadata"),
                call: call.span(),
                definition: None,
                helpers: vec![],
            },
        )
        .unwrap();
    assert!(std::ptr::eq(result.data().kind(), original.data().kind()));
    assert_eq!(result.span().bytes(), original.span().bytes());
    assert_eq!(host.native_macro_calls()[0].exported, 0);
    assert_eq!(host.native_macro_calls()[0].reused, 1);
}

#[test]
fn equal_scalar_occurrences_keep_their_distinct_source_identity() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, ExpansionOrigin, Name, Source},
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let source = "(fn [form env a b] b)";
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let function = evaluator
        .evaluate_unit(&light_cljs::emitter::emit_js_unit(&program))
        .unwrap();
    let call = host
        .read_compiler_form(&Source::new(
            "scalar.cljs",
            "(second 9007199254740993 9007199254740993)",
        ))
        .unwrap()
        .unwrap();
    let original = call.children()[2].clone();
    let environment = Datum::new(DatumKind::Collection {
        kind: light_cljs::CollectionKind::Map,
        children: Box::new([]),
    });
    let result = host
        .expand_javascript(
            &evaluator,
            function,
            &call,
            &environment,
            ExpansionOrigin {
                macro_name: Name::new(None, "second"),
                call: call.span(),
                definition: None,
                helpers: vec![],
            },
        )
        .unwrap();
    assert!(std::ptr::eq(result.data().kind(), original.data().kind()));
    assert_eq!(result.span().bytes(), original.span().bytes());
    assert_ne!(result.span().bytes(), call.children()[1].span().bytes());
    assert_eq!(host.native_macro_calls()[0].exported, 0);
}

#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

#[test]
fn many_namespace_reader_registrations_do_not_limit_native_macro_builds() {
    let root = temp_dir::TempDir::new("native-many-reader-namespaces");
    std::fs::create_dir(root.join("many")).unwrap();
    let mut app = String::from("(ns app (:require");
    for i in 0..256 {
        let namespace = format!("many.namespace_{i}");
        app.push_str(&format!(" [{namespace}]"));
        std::fs::write(
            root.join(format!("many/namespace_{i}.cljs")),
            format!("(ns {namespace}) (def value {i})"),
        )
        .unwrap();
    }
    app.push_str(")) (defmacro same [x] x) (js/console.log (same 42))");
    std::fs::write(root.join("app.cljs"), app).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = light_cljs::compiler::analyze_namespace_entry(
        "app",
        &[root.path().to_path_buf()],
        &mut host,
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(program_output(&emit_js(&program)), "42\n");
    assert_eq!(host.native_macro_calls().len(), 1);
}

#[test]
fn reduced_project_namespaces_expand_variadic_macros() {
    use light_cljs::analyzer::ExprKind;
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("benchmarks/project-results/native-2026-10-25");
    for (entry, directory) in [
        ("app.common.math", "reduced-penpot"),
        ("workload.fonam", "reduced-generated"),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = light_cljs::compiler::analyze_namespace_entry(
            entry,
            &[fixtures.join(directory)],
            &mut host,
            Default::default(),
            &mut Vec::new(),
        )
        .unwrap();
        let js = emit_js(&program);
        if entry == "app.common.math" {
            assert_eq!(program_output(&format!("console.log({js});")), "2\n");
        } else {
            let protocols = program
                .exprs
                .iter()
                .filter_map(|expr| match &expr.kind {
                    ExprKind::DefProtocol {
                        global, methods, ..
                    } => Some((global, methods)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(protocols.len(), 1);
            let (global, methods) = protocols[0];
            assert_eq!(program.globals[global.0].namespace, "workload.fonam");
            assert_eq!(program.globals[global.0].name, "Bagrir");
            assert_eq!(methods.len(), 1);
            assert_eq!(methods[0].key, "workload.fonam/Bagrir/mamjalsko");
            assert_eq!(program.globals[methods[0].global.0].name, "mamjalsko");
            assert_eq!(methods[0].arities, [1]);
            assert_eq!(methods[0].variadic, None);
            assert_eq!(program_output(&js), "");
        }
    }
}

#[test]
fn namespace_stats_report_bridge_work_for_each_native_call() {
    let root = temp_dir::TempDir::new("native-macro-stats");
    std::fs::write(
        root.join("app.cljs"),
        "(ns app) (defmacro twice [x] (list '+ x x)) (twice 21)",
    )
    .unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let mut stats = light_cljs::CompileStats::default();
    light_cljs::compiler::analyze_namespace_entry_with_stats(
        "app",
        &[root.path().to_path_buf()],
        &mut host,
        Default::default(),
        &mut Vec::new(),
        &mut stats,
    )
    .unwrap();
    assert_eq!(stats.native_macro_calls.len(), 1);
    let call = stats.native_macro_calls[0];
    assert!(call.imported > 0);
    assert!(call.exported > 0);
    assert_eq!(call.reused, 2);
    assert!(call.nanos > 0);
    // The default --macro-memory-limit is the runtime's accounted-byte cap.
    assert_eq!(call.gc_byte_limit, 64 << 20);
    assert!(call.gc_admitted_bytes > 0);
    assert!(call.gc_managed_bytes > 0 && call.gc_managed_bytes <= call.gc_peak_managed_bytes);
    assert!(call.gc_peak_managed_bytes <= call.gc_byte_limit);
    let report = stats.report();
    for key in [
        "macro.bridge.0.imported",
        "macro.bridge.0.exported",
        "macro.bridge.0.reused",
        "macro.bridge.0.ns",
        "macro.gc.0.managed-bytes",
        "macro.gc.0.peak-managed-bytes",
        "macro.gc.0.live-bytes",
        "macro.gc.0.admitted-bytes",
        "macro.gc.0.byte-limit 67108864",
    ] {
        assert!(report.contains(key), "{report}");
    }
}

#[cfg(any(feature = "quickjs", feature = "v8-macros"))]
#[test]
fn native_expansions_match_the_legacy_compiler_output() {
    for source in [
        "(defmacro wrap [x] `(+ ~x 2)) (wrap 40)",
        "(defmacro same [x] x) (same (+ 40 2))",
        "(defmacro ns-name [] (list 'quote (:name (:ns &env)))) (ns-name)",
        "(defmacro annotated [x] (with-meta x {:native true})) (annotated (+ 40 2))",
        "(defmacro maximum [x] (js/Math.max x 2)) (maximum 40)",
        "(defmacro part [s n] (.substring s n)) (part \"hello\" 2)",
        "(defmacro stamp [n] (.getTime (js/Date. n))) (stamp 42)",
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        assert_eq!(
            emit_js(&program),
            light_cljs::compile(source).unwrap(),
            "{source}"
        );
    }
}

/// Macro code over native collections, keywords and symbols, which the
/// native unit lowers to engine intrinsics (light-cljs-477), expands as the
/// legacy compiler does and runs.
const COLLECTION_FIXTURES: &[(&str, &str)] = &[
    (
        "(defmacro members [s] (count (conj (disj s 1) 3 2))) (js/console.log (members #{1 2}))",
        "2\n",
    ),
    (
        "(defmacro has? [m k] (contains? m k)) (js/console.log (has? {:a 1} :a) (has? #{:b} :a) (has? [5] 0))",
        "true false true\n",
    ),
    (
        "(defmacro drop-key [m] (list 'quote (dissoc m :a))) (js/console.log (pr-str (drop-key {:a 1 :b 2})))",
        "{:b 2}\n",
    ),
    (
        "(defmacro same-hash? [a b] (if (= a b) (= (hash a) (hash b)) false)) (js/console.log (same-hash? [1 #{2}] (1 #{2})))",
        "true\n",
    ),
    (
        "(defmacro input-set [s] (list 'quote s)) (js/console.log (pr-str (input-set #{:a})) (count (input-set #{1 2 3})))",
        "#{:a} 3\n",
    ),
    (
        "(defmacro picks [v m] [(nth v 1) (nth v 5 :none) (get m :k 0) (:k m) (:z m 9) (first v) (second v)]) (js/console.log (pr-str (picks [10 20] {:k 5})))",
        "[20 :none 5 5 9 10 20]\n",
    ),
    (
        "(defmacro sum-rest [v] `(+ ~@(rest v) ~@(next (next v)))) (js/console.log (sum-rest [0 1 2]))",
        "5\n",
    ),
    (
        "(defmacro grow [v] (list 'quote (assoc (conj v :c) 0 :z))) (js/console.log (pr-str (grow [:a :b])))",
        "[:z :b :c]\n",
    ),
    (
        "(defmacro kinds [x] [(keyword? x) (symbol? x) (vector? x) (map? x) (set? x) (seq? x) (nil? x) (empty? x)]) (js/console.log (pr-str (kinds #{})))",
        "[false false false false true false false true]\n",
    ),
    (
        "(defmacro tag-of [x] (list 'quote (:tag (meta (with-meta x {:tag 'my-tag}))))) (js/console.log (str (tag-of [1])))",
        "my-tag\n",
    ),
    (
        "(defmacro walk [xs] (loop [s (seq xs) n 0] (if s (recur (next s) (+ n (first s))) n))) (js/console.log (walk [1 2 3 4]))",
        "10\n",
    ),
    (
        // A tail past the first trie leaf (light-cljs-502): returned,
        // compared, counted, given metadata and updated as the list it is.
        "(defmacro tail-of [v] (loop [s (seq v) i 0] (if (= i 33) (list 'quote [s (rest s) (= s (seq (vec s))) (count s) (seq? s) (conj s :x) (:k (meta (with-meta s {:k 1})))]) (recur (next s) (+ i 1))))) \
         (js/console.log (pr-str (tail-of [0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35])))",
        "[(33 34 35) (34 35) true 3 true (:x 33 34 35) 1]\n",
    ),
    (
        "(defmacro built [] (list 'quote (list (vector 1 2) (hash-set 3) (cons 0 [5])))) (js/console.log (pr-str (built)))",
        "([1 2] #{3} (0 5))\n",
    ),
    (
        "(defmacro as-vec [l] (list 'quote (vec l))) (js/console.log (pr-str (as-vec (1 2))))",
        "[1 2]\n",
    ),
];

#[test]
fn collection_core_lowering_matches_the_legacy_compiler_and_runs() {
    for (source, expected) in COLLECTION_FIXTURES {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        let native = emit_js(&program);
        assert_eq!(program_output(&native), *expected, "{source}");
        // In a build with a legacy engine (`quickjs`, or the default V8),
        // `compile` expands with it: the comparison is then cross-backend.
        let legacy = light_cljs::compile(source).unwrap();
        assert_eq!(native, legacy, "{source}");
    }
}

/// Expand `source` natively, check that the program prints `expected`, and
/// that the expansion is the legacy compiler's (cross-backend in a build
/// with a legacy engine).
fn assert_native_matches_legacy(source: &str, expected: &str) {
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .unwrap_or_else(|e| panic!("{source}: {e}"));
    let native = emit_js(&program);
    assert_eq!(program_output(&native), expected, "{source}");
    let legacy = light_cljs::compile(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    assert_eq!(native, legacy, "{source}");
}

/// A macro-local JavaScript array `a` and `t`, which records its operand
/// in `a` and returns it: operand effects in evaluation order.
const TRACE: &str = "a (js/Array.) t (fn [x] (.push a x) x)";

#[test]
fn native_hash_map_literals_evaluate_keys_before_values() {
    // Nine entries: a hash-map literal, whose keys all run before its
    // values (`mapFromArrays`).
    let entries: String = (0..9).map(|i| format!("(t {i}) (t {}) ", i + 10)).collect();
    let source = format!(
        "(defmacro order [] (let [{TRACE} m {{{entries}}}] [(.join a \" \") (get m 1) (count m)])) \
         (js/console.log (pr-str (order)))"
    );
    assert_native_matches_legacy(
        &source,
        "[\"2 6 0 5 1 4 8 7 3 12 16 10 15 11 14 18 17 13\" 11 9]\n",
    );
}

#[test]
fn nth_of_nil_honors_the_default_directly_and_through_apply() {
    assert_native_matches_legacy(
        "(defmacro fallback [] [(nth nil 0 :missing) (apply nth [nil 0 :missing]) (nth nil 0)]) \
         (js/console.log (pr-str (fallback)))",
        "[:missing :missing nil]\n",
    );
}

#[test]
fn native_wide_concat_compiles_and_preserves_operand_effects() {
    // 128 operands: a nested lowering would exceed the engine's parser
    // nesting budget.
    // `w` records its (number) operand and returns it in a vector.
    let operands: String = (0..128).map(|i| format!("(w {i}) ")).collect();
    let source = format!(
        "(defmacro wide [] (let [a (js/Array.) w (fn [x] (.push a x) [x]) c (concat {operands})] \
         [(count c) (first c) (nth c 127) (.-length a) (aget a 0) (aget a 127)])) \
         (js/console.log (pr-str (wide)))"
    );
    assert_native_matches_legacy(&source, "[128 0 127 128 0 127]\n");
}

#[test]
fn native_dynamic_constructors_match_legacy() {
    // Runtime string operands (macro inputs), direct calls and the
    // constructors as values through `apply`.
    assert_native_matches_legacy(
        "(defmacro names [s n] (list 'quote [(keyword s) (keyword nil n) (keyword s n) (keyword 'a/b) \
         (keyword \"x/y\") (keyword :k) (keyword 1) (symbol n) (symbol s n) (symbol :c/d) (symbol \"p/q\") \
         (symbol 'r) (apply keyword [s n]) (apply symbol [n]) (apply keyword [:kw]) \
         (get (array-map :k 1 :j 2) :j) (count (array-map :k 1 :k 2)) (get (apply array-map [:z 3]) :z) \
         (map? (array-map))])) \
         (js/console.log (pr-str (names \"ns\" \"nm\")))",
        "[:ns :nm :ns/nm :a/b :x/y :k nil nm ns/nm c/d p/q r :ns/nm nm :kw 2 1 3 true]\n",
    );
}

#[test]
fn native_constructor_errors_match_legacy() {
    // Invalid operands throw a plain Error, as keywordCore and symbolCore
    // do, directly and through apply; a TypeError catch does not take it.
    assert_native_matches_legacy(
        "(defmacro errors [] [(try (symbol 1) (catch :default e (.-name e))) \
         (try (apply symbol [1]) (catch :default e (.-message e))) \
         (try (keyword nil 1) (catch :default e (.-name e))) \
         (try (symbol nil :k) (catch js/TypeError e :type) (catch :default e (.-message e)))]) \
         (js/console.log (pr-str (errors)))",
        "[\"Error\" \"no conversion to symbol\" \"Error\" \"symbol expects strings and an optional nil namespace\"]\n",
    );
    // A keyword converts to a symbol through its spelling.
    assert_native_matches_legacy(
        "(defmacro parts [] (let [s (symbol :a/b) t (symbol (keyword \"x\"))] \
         [(namespace s) (name s) (namespace t) (name t)])) (js/console.log (pr-str (parts)))",
        "[\"a\" \"b\" nil \"x\"]\n",
    );
    // `(keyword "" "x")` spells `/x`, whose symbol is the unqualified name
    // `/x` in the legacy runtimes; it has no native symbol, so it fails
    // instead of becoming namespace "" and name "x".
    let source = "(defmacro odd [] (name (symbol (keyword \"\" \"x\")))) (odd)";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let Err(error) = analyze_with_macros(source, &parse(source).unwrap(), &mut host) else {
        panic!("expanded");
    };
    let message = error.to_string();
    assert!(message.contains("must split back"), "{message}");
}

#[test]
fn native_dynamic_set_literals_reject_duplicates_after_operand_effects() {
    // A small and a 1,025-member literal (number constants keep it short
    // enough for one macro unit); three members are calls of `w`, which
    // records and returns its operand, and the last of them equals another
    // member. Every operand runs once before the duplicate is rejected.
    // Literals past the 4,096-operand counted-constructor limit do not fit
    // one macro unit; `bbp_evaluator`'s tests cover their constructors.
    for size in [3, 1025] {
        let constants: String = (1..=size - 3).map(|k| format!("{k} ")).collect();
        let (big, dup) = (size + 1, if size > 3 { 1 } else { 0 });
        let set = |last: usize| format!("#{{{constants}(w 0) (w {big}) (w (+ 0 {last}))}}");
        let source = format!(
            "(defmacro dup [] (let [a (js/Array.) w (fn [x] (.push a x) x)] (try {} \
             (catch :default e [(.startsWith (.-message e) \"Duplicate key\") \
             (.join (.sort a (fn [x y] (- x y))) \",\")])))) \
             (js/console.log (pr-str (dup)))",
            set(dup),
        );
        assert_native_matches_legacy(&source, &format!("[true \"{},{dup},{big}\"]\n", 0));
        // Distinct members of the same sizes are accepted.
        let source = format!(
            "(defmacro ok [] (let [a (js/Array.) w (fn [x] (.push a x) x)] [(count {}) (.-length a)])) \
             (js/console.log (pr-str (ok)))",
            set(big + 1),
        );
        assert_native_matches_legacy(&source, &format!("[{size} 3]\n"));
    }
}

#[test]
fn native_positional_literal_matrix_matches_legacy() {
    // Every row executes inside a macro. With bbp-macros,quickjs this is a
    // table-driven differential against QuickJS, including emitted constants,
    // reader inputs, mutation semantics, metadata, and exported duplicate keys.
    let cases = [
        (
            "empty",
            r#"(defmacro probe [] [(count '#{}) (nil? (seq '#{})) (count '{}) (nil? (seq '{})) (= '#{} (set [])) (= '{} (apply hash-map []))]) (probe)"#,
        ),
        (
            "small-constructors",
            r#"(defmacro probe [] (let [m (array-map \a 1 "a" 2) s (hash-set \a "a")] (list 'quote [(count m) (get m "a") (vec (seq m)) (count s) (vec (seq s))]))) (probe)"#,
        ),
        (
            "char-string",
            r#"(defmacro probe [] [(count {\a 1 "a" 2}) (get {\a 1 "a" 2} \a) (count #{\a "a"}) (contains? #{\a "a"} "a")]) (probe)"#,
        ),
        (
            "map-order",
            r#"(defmacro probe [] (let [m {\a 1 "a" 2}] (list 'quote [(count m) (get m \a) (get m "a") (contains? m \a) (contains? m "a") (vec (seq m)) (vec (keys m)) (vec (vals m))]))) (probe)"#,
        ),
        (
            "reverse",
            r#"(defmacro probe [] (let [m {"a" 2 \a 1} s #{"a" \a}] (list 'quote [(get m \a) (vec (seq m)) (count s) (vec (seq s)) (get s "a")])) ) (probe)"#,
        ),
        (
            "set-lookup",
            r#"(defmacro probe [] (let [s #{\a "a"}] (list 'quote [(count s) (contains? s \a) (contains? s "a") (get s \a) (get s "a") (vec (seq s))]))) (probe)"#,
        ),
        (
            "quoted",
            r#"(defmacro probe [] (let [m '{\a 1 "a" 2} s '#{\a "a"}] (list 'quote [(count m) (get m \a) (vec (seq m)) (count s) (vec (seq s))]))) (probe)"#,
        ),
        (
            "numbers",
            r#"(defmacro probe [] (list 'quote [(count {1 :int 1.0 :float}) (get {1 :int 1.0 :float} 1) (count #{1 1.0}) (vec (seq '#{1 1.0}))])) (probe)"#,
        ),
        (
            "nested",
            r#"(defmacro probe [] (let [x [{:m {\a 1 "a" 2} :s #{\a "a"}}] m (:m (first x)) s (:s (first x))] (list 'quote [(count m) (get m \a) (vec (seq m)) (count s) (vec (seq s))]))) (probe)"#,
        ),
        (
            "quoted-nested",
            r#"(defmacro probe [] (let [x '({:m {\a 1 "a" 2} :s #{\a "a"}}) m (:m (first x)) s (:s (first x))] (list 'quote [(count m) (get m \a) (count s) (vec (seq s))]))) (probe)"#,
        ),
        (
            "unicode",
            r#"(defmacro probe [] (list 'quote [(get {\u0061 1 "a" 2} \a) (vec (seq #{\u0061 "a"})) (get {\newline 1 "\n" 2} \u000a) (vec (seq #{\space " "}))])) (probe)"#,
        ),
        (
            "operand-effects",
            r#"(defmacro probe [] (let [events (atom []) m {\a (do (swap! events conj :first) 11) "a" (do (swap! events conj :second) 22)}] (list 'quote [(count m) (get m \a) (vec (seq m)) @events]))) (probe)"#,
        ),
        (
            "eight-pairs",
            r#"(defmacro probe [] (let [m {\a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8}] (list 'quote [(count m) (get m "a") (vec (vals m))]))) (probe)"#,
        ),
        (
            "nine-pairs",
            r#"(defmacro probe [] (let [m {\a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8 :h 9}] [(count m) (get m \a) (get m "a")])) (probe)"#,
        ),
        (
            "ten-members",
            r#"(defmacro probe [] (let [s #{\a "a" :b :c :d :e :f :g :h :i}] (list 'quote [(count s) (vec (seq s)) (contains? s :i)]))) (probe)"#,
        ),
        (
            "composite-keys",
            r#"(defmacro probe [] (let [m '{[\a] 1 ["a"] 2} n '{{\a 1} 11 {"a" 1} 22} t '{#{\a} 33 #{"a"} 44}] (list 'quote [(count m) (get m [\a]) (vec (seq m)) (count n) (get n {"a" 1}) (count t) (get t #{"a"})]))) (probe)"#,
        ),
        (
            "composite-members",
            r#"(defmacro probe [] (let [s '#{[\a] ["a"]} t '#{{\a 1} {"a" 1}} u '#{#{\a} #{"a"}}] (list 'quote [(count s) (vec (seq s)) (contains? s ["a"]) (count t) (contains? t {"a" 1}) (count u) (contains? u #{"a"})]))) (probe)"#,
        ),
        (
            "mixed",
            r#"(defmacro probe [] (let [s '#{\a "a" 1 1.0 :a a 'b} m '{\a 1 "a" 2 1 :int 1.0 :float :a :keyword a :symbol}] (list 'quote [(count s) (vec (seq s)) (count m) (vec (seq m)) (get m 'a) (contains? s 'a)]))) (probe)"#,
        ),
        (
            "inner-quote",
            r#"(defmacro probe [] (let [s '#{\a "a" '\b \b} m '{\a 1 "a" 2 '\b 3 \b 4}] (list 'quote [(count s) (contains? s \b) (contains? s '(quote \b)) (vec (seq s)) (count m) (get m \b) (get m '(quote \b)) (vec (vals m))]))) (probe)"#,
        ),
        (
            "metadata",
            r#"(defmacro probe [] (let [m ^{:tag :map} {\a 1 "a" 2} s ^{:tag :set} #{1 1.0}] (list 'quote [(meta m) (meta s) (vec (seq m)) (vec (seq s)) (meta (assoc m "a" 3)) (meta (disj s 1))]))) (probe)"#,
        ),
        (
            "assoc-conj",
            r#"(defmacro probe [] (let [m '{\a 1 "a" 2} s '#{1 1.0}] (list 'quote [(vec (seq (assoc m "a" 9))) (vec (seq (conj m ["b" 3]))) (vec (seq (conj s 1))) (vec (seq (conj s 2)))]))) (probe)"#,
        ),
        (
            "dissoc-disj",
            r#"(defmacro probe [] (let [m '{\a 1 "a" 2 :b 3} s '#{1 1.0 2}] (list 'quote [(mapv (fn [e] [(nil? (first e)) (nil? (second e))]) (seq (dissoc m "a"))) (mapv nil? (seq (disj s 1))) (count (dissoc m "a")) (count (disj s 1)) (contains? (dissoc m "a") nil) (contains? (disj s 1) nil)]))) (probe)"#,
        ),
        (
            "promotion",
            r#"(defmacro probe [] (let [m '{\a 1 "a" 2 :b 3 :c 4 :d 5 :e 6 :f 7 :g 8} s '#{1 1.0 2 3 4 5 6 7}] [(count (assoc m :h 9)) (get (assoc m :h 9) "a") (count (conj s 8)) (count (assoc m "a" 9)) (get (assoc m "a" 9) "a")])) (probe)"#,
        ),
        (
            "promotion-member-metadata",
            r#"(defmacro probe [] (let [s '#{^{:tag :first} [\a] ^{:tag :last} ["a"] 2 3 4 5 6 7} t (conj s 8)] (list 'quote [(count t) (meta (get s ["a"])) (meta (get t ["a"])) (meta (first (filter #(= % ["a"]) t)))]))) (probe)"#,
        ),
        (
            "equality-hash",
            r#"(defmacro probe [] (let [m '{:b 2 :a 1} h (apply hash-map [:a 1 :b 2]) s '#{2 1} t (set [1 2])] [(= m h) (= h m) (= (hash m) (hash h)) (= s t) (= t s) (= (hash s) (hash t)) (= '{\a 1 "a" 2} '{\a 1 "a" 2}) (= '#{1 1.0} '#{1 2})])) (probe)"#,
        ),
        (
            "nested-directional-equality",
            r#"(defmacro probe [m] (let [a '#{1 1.0} b '#{1 2} p (assoc m :k a) q (apply hash-map [:k b])] [(= p q) (= q p) (= [a] [b]) (= [b] [a])])) (probe {:k nil})"#,
        ),
        (
            "positional-metadata",
            r#"(defmacro probe [] (let [v ^{\a 1 "a" 2} [] m (meta v)] (list 'quote [(count m) (get m "a") (vec (seq m))]))) (probe)"#,
        ),
        (
            "directional-hashed-keys",
            r#"(defmacro probe [] (let [a '#{0 0.0} b '#{0 2147483647} m (apply hash-map [a :old]) n (assoc m b :new)] [(= (hash a) (hash b)) (= a b) (= b a) (get m b :missing) (count n) (get n a) (get n b) (count (dissoc m b))])) (probe)"#,
        ),
        (
            "map-to-record",
            r#"(defrecord R [x]) (defmacro probe [] (let [r (map->R '{\a 1 :x 0 "a" 2 :z 3})] (list 'quote [(count r) (get r "a") (:x r) (vec (seq r))]))) (probe)"#,
        ),
        (
            "record-extension-identity",
            r#"(defrecord R [x]) (defmacro probe [] (let [m '{\a 1 "a" 2 :z 3} r (R. 0 nil m) n (with-meta r {:tag true})] [(= r n) (= n r) (= (hash r) (hash n)) (= r (R. 0 nil m)) (= r (R. 0 nil (with-meta m {:tag true}))) (= m (with-meta m {:tag true})) (= r (assoc (assoc r :x 9) :x 0)) (= (map->R m) (map->R m))])) (probe)"#,
        ),
        (
            "record-extension-updates",
            r#"(defrecord R [x]) (defmacro probe [] (let [r (map->R '{\a 1 :x 0 "a" 2 :z 3}) a (assoc r "a" 9) d (dissoc r "a")] [(= r (with-meta r {:tag true})) (= r (assoc r "a" 1)) (= r a) (= r (assoc a "a" 1)) (= r (dissoc (assoc r :new 4) :new)) (= r (dissoc r :missing)) (= a (with-meta a {:tag true})) (= d (with-meta d {:tag true})) (count d) (get a "a") (get d :z)])) (probe)"#,
        ),
        (
            "full-hash-collision-first-match",
            r#"(defmacro probe [] (let [a '#{0 0.0} b '#{0 2147483647} h (apply hash-map [a :first b :second]) p (array-map a :second b :second) q (apply hash-map [a :second b :second]) match (array-map a :first b :second)] [(= (hash a) (hash b)) (= a b) (= b a) (count h) (get h a) (get h b) (= p h) (= h p) (= q h) (= h q) (= match h) (= h match)])) (probe)"#,
        ),
        (
            "lazy-key-equality",
            r#"(defmacro probe [] (let [k (lazy-seq (throw (js/Error. "hashed lazy key"))) m (assoc '{\a 0 "a" 0} k 7) n (with-meta m {:tag true})] [(= m n) (= n m)])) (probe)"#,
        ),
        (
            "entry-source-order",
            r#"(defmacro probe [] (let [late (lazy-seq (throw (js/Error. "late entry"))) m (assoc '{\a 0 "a" 0 :late nil} :late late) n '{\a 1 "a" 1 :late [1]}] [(= m n) (= n m) (= (array-map :missing late) (apply hash-map [:other [1]])) (= (apply hash-map [:other [1]]) (array-map :missing late))])) (probe)"#,
        ),
        (
            "mixed-representation-equality",
            r#"(defmacro probe [] (let [a '#{0 0.0} b (set [0 2147483647]) p (array-map a :value) h (apply hash-map [b :value]) q (array-map b :value) j (apply hash-map [a :value])] [(= a b) (= b a) (= p h) (= h p) (= q j) (= j q) (= '#{1 1.0} (set [1 2])) (= (set [1 2]) '#{1 1.0}) (= '{:b 2 :a 1} (apply hash-map [:a 1 :b 2])) (= (apply hash-map [:a 1 :b 2]) '{:b 2 :a 1})])) (probe)"#,
        ),
        (
            "identity-user-keys",
            r#"(def events (atom [])) (deftype K [] IHash (-hash [_] (swap! events conj :hash) 17) IEquiv (-equiv [this other] (swap! events conj :equal) (identical? this other))) (defmacro probe [] (let [k (K.) m (assoc '{} k 7) s (conj '#{} k)] (reset! events []) [(get m k) (contains? m k) (identical? k (get s k :missing)) (contains? s k) (= m (with-meta m {:tag true})) (count @events)])) (probe)"#,
        ),
        (
            "bridge",
            r#"(defmacro probe [m s] (list 'quote [(count m) (get m "a") (vec (seq m)) (count s) (vec (seq s)) (get (assoc m "a" 9) "a") (vec (seq (disj s 1)))])) (probe {\a 1 "a" 2} #{1 1.0})"#,
        ),
        (
            "bridge-nested-metadata",
            r#"(defmacro probe [x] (let [m (:m (first x)) s (:s (first x))] (list 'quote [(count m) (vec (seq m)) (meta m) (count s) (vec (seq s)) (meta s)]))) (probe [{:m ^{:tag :map} {\a 1 "a" 2} :s ^{:tag :set} #{1 1.0}}])"#,
        ),
        (
            "export-changed",
            r#"(defmacro probe [m s] (list 'quote [(assoc m "a" 9) (conj s 2)])) (probe {\a 1 "a" 2} #{1 1.0})"#,
        ),
        (
            "realize-rebuild",
            r#"(defmacro probe [] (let [m (assoc '{\a nil "a" 2} "a" (map inc [1 2])) s (conj '#{1 1.0} (map inc [1 2]))] (list 'quote [(vec (seq m)) (vec (seq s))]))) (probe)"#,
        ),
    ];
    for (name, expression) in cases {
        let (declaration, call) = expression.split_at(expression.rfind("(probe").unwrap());
        let source = format!("{declaration}(js/console.log (pr-str {call}))");
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
            .unwrap_or_else(|e| panic!("{name}, native: {e}"));
        let native = emit_js(&program);
        let legacy = light_cljs::compile(&source).unwrap_or_else(|e| panic!("{name}, legacy: {e}"));
        let native_output = program_output(&native);
        let legacy_output = program_output(&legacy);
        // Reader inputs previously collapsed in both bridges. Native inputs
        // now retain their original positions, an intentional extension; the
        // legacy bridge stays unchanged. Pin both results in this same matrix.
        let bridge_outputs = match name {
            "bridge" => Some((
                "[2 1 [[\"a\" 1] [\"a\" 2]] 2 [1 1] 9 [nil]]\n",
                "[1 2 [[\"a\" 2]] 1 [1] 9 []]\n",
            )),
            "bridge-nested-metadata" => Some((
                "[2 [[\"a\" 1] [\"a\" 2]] {:tag :map} 2 [1 1] {:tag :set}]\n",
                "[1 [[\"a\" 2]] {:tag :map} 1 [1] {:tag :set}]\n",
            )),
            "export-changed" => Some(("[{\"a\" 9, \"a\" 2} #{1 1 2}]\n", "[{\"a\" 9} #{1 2}]\n")),
            _ => None,
        };
        if let Some((expected_native, expected_legacy)) = bridge_outputs {
            assert_eq!(native_output, expected_native, "{name}: native inputs");
            if cfg!(any(feature = "quickjs", feature = "v8-macros")) {
                assert_eq!(legacy_output, expected_legacy, "{name}: legacy inputs");
            } else {
                assert_eq!(native_output, legacy_output, "{name}: native-only host");
            }
        } else {
            assert_eq!(native_output, legacy_output, "{name}");
            assert_eq!(native, legacy, "{name}: expanded form");
        }
    }
}

#[test]
fn collection_helpers_publish_once_at_one_and_two_slots() {
    // A set constant and two functions built only from lowered core
    // operations and literals: each value is initialized once and shared,
    // not initialized again in each consuming task.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/collections");
    for jobs in [1, 2] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let mut warnings = Vec::new();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
            "coll.app",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut warnings,
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                stats: Some(&mut stats),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("jobs {jobs}: {error}"));
        assert_eq!(
            program_output(&emit_js(&program)),
            "[{:op :get} 2] [{} 1]\n",
            "jobs {jobs}"
        );
        assert_eq!(stats.native_helpers.compiled, 3, "jobs {jobs}");
        assert_eq!(stats.native_helpers.published, 3, "jobs {jobs}");
        assert_eq!(stats.native_helpers.per_task, 0, "jobs {jobs}");
    }
}

#[test]
fn helper_namespace_output_is_deterministic_with_one_two_and_four_slots() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/pool");
    let mut reference = None;
    for jobs in [1, 2, 4] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let mut warnings = Vec::new();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
            "demand.app",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut warnings,
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                stats: Some(&mut stats),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("jobs {jobs}: {error}"));
        let js = emit_js(&program);
        assert_eq!(program_output(&js), "42 42 3\n");
        assert_eq!(stats.native_helpers.compiled, 1);
        // The helper builds its expansion with native core intrinsics, so its
        // value publishes once instead of initializing in each task.
        assert_eq!(stats.native_helpers.published, 1);
        assert_eq!(stats.native_helpers.per_task, 0);
        assert!(warnings.is_empty());
        if let Some(reference) = &reference {
            assert_eq!(&js, reference);
        } else {
            reference = Some(js);
        }
        if jobs > 1 {
            assert!(stats.deferred_units.pool_engine_workers > 0);
            assert!(stats.deferred_units.pool.helped > 0);
        }
    }
}

#[test]
fn helper_failures_have_the_same_source_diagnostic_at_one_two_and_four_slots() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/pool");
    let mut reference = None;
    for jobs in [1, 2, 4] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let error = light_cljs::compiler::analyze_namespace_entry_recorded(
            "demand.failure",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut Vec::new(),
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("fixture helper failed"), "{error}");
        assert!(
            error.contains("failure.cljs:2:15") && error.contains("demand.macros/demand-failure"),
            "{error}"
        );
        if let Some(reference) = &reference {
            assert_eq!(&error, reference, "jobs {jobs}");
        } else {
            reference = Some(error);
        }
    }
}

/// `--macro-memory-limit` is the native runtime's aggregate accounted-byte
/// budget: a macro that retains more than it fails with the engine's
/// catchable resource error, a macro may catch that error, and the
/// compilation continues in the same runtime afterwards.
#[test]
fn the_macro_memory_limit_denies_allocation_catchably_and_the_build_recovers() {
    let hog = "(defmacro hog [] (js* \"(() => { const xs = []; try { for (;;) xs.push({v: xs.length}); } catch (e) { xs.length = 0; return e === `heap byte limit exceeded`; } })()\"))";
    let source =
        format!("{hog} (defmacro twice [x] (list '+ x x)) (js/console.log (hog) (twice 21) (hog))");
    let mut host = MacroHost::new();
    host.use_native_macros();
    host.evaluator_options.memory_limit = 16 << 20;
    let program = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host).unwrap();
    assert_eq!(program_output(&emit_js(&program)), "true 42 true\n");

    let source =
        "(defmacro hog! [] (js* \"(() => { const xs = []; for (;;) xs.push({}); })()\"))\n(hog!)";
    let mut host = MacroHost::new();
    host.use_native_macros();
    host.evaluator_options.memory_limit = 16 << 20;
    let message = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .err()
        .unwrap()
        .to_string();
    assert!(message.contains("heap byte limit exceeded"), "{message}");
    assert!(message.contains("cljs.user:2:1"), "{message}");
}

#[test]
fn a_macro_memory_limit_below_the_native_realm_fails_at_startup_with_a_diagnostic() {
    let source = "(defmacro twice [x] (list '+ x x)) (twice 21)";
    let mut host = MacroHost::new();
    host.use_native_macros();
    host.evaluator_options.memory_limit = 256 << 10;
    let message = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .err()
        .unwrap()
        .to_string();
    assert!(
        message.contains(
            "macro memory limit of 262144 bytes cannot hold the native macro runtime's realm"
        ),
        "{message}"
    );
}

/// Macro code over compile-time protocols, types and records, which the
/// native unit runs on engine dispatch tables and native structs
/// (light-cljs-478; docs/native-macros.md, "Protocols and types"). Each
/// expands as the legacy compiler does and runs.
const PROTOCOL_FIXTURES: &[(&str, &str)] = &[
    // Records: protocol methods, field and extension lookup, map->R,
    // assoc, equality and the record hash.
    (
        "(defprotocol P (p [x]) (q [x y])) (defrecord R [a b] P (p [_] a) (q [this y] (+ b y))) \
         (defmacro m [] [(p (->R 1 2)) (q (R. 3 4) 10) (:a (->R 5 6)) (get (map->R {:a 7 :b 8 :c 9}) :c) \
         (count (assoc (->R 1 2) :z 3)) (= (->R 1 2) (->R 1 2)) (= (->R 1 2) (->R 1 3)) \
         (hash (->R 1 2)) (hash (assoc (->R 1 2) :c 3))]) (js/console.log (pr-str (m)))",
        "[1 14 5 9 3 true false -658793607 1908665455]\n",
    ),
    // Host-type extension, `nil`, `default` and `satisfies?`.
    (
        "(defprotocol P (p [x])) (extend-protocol P string (p [x] (str \"s\" x)) number (p [x] (inc x)) \
         nil (p [_] :nil) default (p [_] :default)) \
         (defmacro m [] [(p \"a\") (p 1) (p nil) (p :k) (satisfies? P 1) (satisfies? P :k)]) \
         (js/console.log (pr-str (m)))",
        "[\"sa\" 2 :nil :default true true]\n",
    ),
    // A deftype's fields, read in its methods and through interop.
    (
        "(defprotocol P (p [x])) (deftype T [a] P (p [this] (.-a this))) \
         (defmacro m [] [(p (T. 5)) (.-a (T. 6)) (= (T. 1) (T. 1))]) (js/console.log (pr-str (m)))",
        "[5 6 false]\n",
    ),
    // A mutable field keeps a task-owned JavaScript instance.
    (
        "(defprotocol P (p [x])) (deftype T [^:mutable a] P (p [_] (set! a (inc a)) a)) \
         (defmacro m [] (let [t (T. 1)] (p t) (p t))) (js/console.log (pr-str (m)))",
        "3\n",
    ),
    // Record map operations.
    (
        "(defrecord R [a b]) (defmacro m [] (let [r (->R 1 2)] (list 'quote [(seq r) (dissoc r :a) \
         (= r (dissoc (assoc r :c 3) :c)) (meta (with-meta r {:x 1})) (:d (conj r [:d 4])) \
         (contains? r :a) (keys r) (vals r) (record? r) (map? r) (map? (->R 1 2))]))) \
         (js/console.log (pr-str (m)))",
        "[([:a 1] [:b 2]) {:b 2} true {:x 1} 4 true (:a :b) (1 2) true true true]\n",
    ),
    // The protocol-extension frontier: an expansion before a later
    // extension sees the earlier one even when a later macro demands a
    // helper whose analysis expands `snapshot` (`FRONTIER_REPLAY`).
    (HELPER_FRONTIER, "[[1 10] [2 10] 2 10]\n"),
    // A macro that generates protocol, record and method declarations
    // (`generated_destructured_methods...` in the compiler macro suite).
    (
        "(defmacro bundle [] '(do (defprotocol P (run [this input])) \
         (defrecord R [a] P (run [_ {:keys [b]}] (+ a b))) (run (->R 2) {:b 3}))) \
         (js/console.log (bundle))",
        "5\n",
    ),
    // A record helper value read by a macro.
    (
        "(defprotocol P (p [x])) (defrecord R [a] P (p [_] a)) (def r (->R 41)) \
         (defmacro m [] (inc (p r))) (js/console.log (m))",
        "42\n",
    ),
    // A methodless deftype with a mutable field keeps a JavaScript
    // instance that interop assigns.
    (
        "(deftype T [^:mutable a]) (defmacro m [] (let [t (T. 1)] (set! (.-a t) 2) (.-a t))) \
         (js/console.log (m))",
        "2\n",
    ),
    // A record constructor's metadata and extension map.
    (
        "(defrecord R [a]) (defmacro m [] (let [r (R. 1 {:tag true} {:extra 2})] [(meta r) (:extra r) (count r)])) \
         (js/console.log (pr-str (m)))",
        "[{:tag true} 2 2]\n",
    ),
    // Object methods on struct and mutable instances.
    (
        "(deftype T [x] Object (value [_] x)) (deftype U [^:mutable y] Object (value [_] (set! y (inc y)) y)) \
         (defmacro m [] [(.value (T. 7)) (.value (U. 1))]) (js/console.log (pr-str (m)))",
        "[7 2]\n",
    ),
    // `instance?` of struct types: records, a deftype with methods and a
    // methodless one, directly, through an expression-valued class and
    // through a saved `instance?`.
    (
        "(defprotocol P (p [x])) (defrecord R []) (deftype T [a] P (p [_] a)) (deftype U [b]) \
         (defmacro m [] (let [i instance? c (if (= 1 (count [1])) T U)] [(instance? T (T. 1)) (instance? U (T. 1)) \
         (instance? U (U. 1)) (instance? R (->R)) (instance? T (->R)) (instance? c (T. 1)) (instance? c (U. 1)) \
         (i T (T. 1)) (i R (T. 1))])) (js/console.log (pr-str (m)))",
        "[true false true true false true false true false]\n",
    ),
    // Object methods follow later extensions, inside a macro too, for
    // struct and mutable instances.
    (
        "(deftype T [x] Object (f [_] 1)) (deftype U [^:mutable y] Object (f [_] 1)) \
         (extend-type T Object (g [_] :g)) \
         (defn install [] (extend-type T Object (f [_] 2)) (extend-type U Object (f [_] 2) (g [_] :u))) \
         (defmacro m [] (let [a [(.f (T. 0)) (.f (U. 0)) (.g (T. 0))]] (install) [a (.f (T. 0)) (.f (U. 0)) (.g (U. 0))])) \
         (js/console.log (pr-str (m)))",
        "[[1 1 :g] 2 2 :u]\n",
    ),
    // Object method overloads by arity, and punctuated member names.
    (
        "(deftype T [a] Object (f [_] 1) (f [_ x] [a x]) (foo-bar [_] 7)) \
         (deftype U [^:mutable a] Object (f [_] 1) (f [_ x] [a x]) (foo-bar [_] 8)) \
         (defmacro m [] [(.f (T. 0)) (.f (T. 0) 5) (.f (U. 0)) (.f (U. 0) 5) (.foo-bar (T. 0)) (.foo-bar (U. 0))]) \
         (js/console.log (pr-str (m)))",
        "[1 [0 5] 1 [0 5] 7 8]\n",
    ),
    // An Object method taken from one instance runs its type's method on
    // the receiver it is called with.
    (
        "(deftype T [^:mutable a] Object (f [_] [:t a])) (deftype U [^:mutable a] Object (f [_] [:u a])) \
         (deftype V [a] Object (f [_] [:v a])) (deftype W [a] Object (f [_] [:w a])) \
         (defmacro m [] [(.call (.-f (T. 0)) (U. 1)) (.call (.-f (V. 2)) (V. 3)) (.call (.-f (V. 4)) (W. 5))]) \
         (js/console.log (pr-str (m)))",
        "[[:t 1] [:v 3] [:v 5]]\n",
    ),
    // Object methods whose names munge alike share the member the method
    // first installed last holds.
    (
        "(deftype T [^:mutable a] Object (foo-bar [_] 1) (foo_bar [_ x] 2)) \
         (deftype U [a] Object (foo-bar [_] 3) (foo_bar [_ x] 4)) \
         (defmacro m [] [(.foo-bar (T. 0)) (.foo_bar (T. 0) 1) (.foo-bar (U. 0)) (.foo_bar (U. 0) 1)]) \
         (js/console.log (pr-str (m)))",
        "[2 2 4 4]\n",
    ),
    // A colliding Object method that a later extension installs holds the
    // member from the first call after it, and not at an earlier frontier.
    (
        "(deftype T [a] Object (foo-bar [_] 1)) (deftype U [^:mutable a] Object (foo-bar [_] 3)) \
         (defmacro early [] [(.foo-bar (T. 0)) (.foo-bar (U. 0))]) (def e (early)) \
         (extend-type T Object (foo_bar [_] 2)) (extend-type U Object (foo_bar [_] 4)) \
         (defmacro m [] [(.foo-bar (T. 0)) (.foo-bar (T. 0)) (.foo-bar (U. 0)) (.foo-bar (U. 0))]) \
         (js/console.log (pr-str [e (m) (early)]))",
        "[[1 3] [2 2 4 4] [2 4]]\n",
    ),
    // An object inheriting a struct's or a mutable instance's Object
    // method reads the fields it inherits.
    (
        "(deftype T [x] Object (f [_] x)) (deftype U [^:mutable x] Object (f [_] x)) \
         (defmacro m [] [(.f (js/Object.create (T. 7))) (.f (js/Object.create (U. 8)))]) \
         (js/console.log (pr-str (m)))",
        "[7 8]\n",
    ),
    // Variadic Object methods, alone and beside a fixed overload, with too
    // few, exact and extra arguments.
    (
        "(deftype T [a] Object (f [_] :fixed) (f [_ x & xs] [x (count xs)]) (g [_ & xs] (count xs))) \
         (deftype U [^:mutable a] Object (f [_] :fixed) (f [_ x & xs] [x (count xs)]) (g [_ & xs] (count xs))) \
         (defmacro m [] (let [t (T. 0) u (U. 0)] [(.f t) (.f t 1) (.f t 1 2 3) (.g t) (.g t 1 2) \
         (.f u) (.f u 1) (.f u 1 2 3) (.g u) (.g u 1 2)])) \
         (js/console.log (pr-str (m)))",
        "[:fixed [1 0] [1 2] 0 2 :fixed [1 0] [1 2] 0 2]\n",
    ),
    // `implements?` of extended core collection types.
    (
        "(defprotocol P (p [x])) (extend-type PersistentVector P (p [_] 1)) (extend-type EmptyList P (p [_] 2)) \
         (extend-type Keyword P (p [_] 3)) (extend-protocol P string (p [_] 4)) \
         (defmacro m [] [(implements? P []) (satisfies? P []) (implements? P '()) (satisfies? P '()) \
         (implements? P '(1)) (satisfies? P '(1)) (implements? P :k) (implements? P \"s\") (satisfies? P \"s\")]) \
         (js/console.log (pr-str (m)))",
        "[true true true true false false true false true]\n",
    ),
    // An extension that macro code runs overrides the type's own.
    (
        "(defprotocol P (p [x])) (deftype T [] P (p [_] 1)) (defn install [] (extend-type T P (p [_] 2))) \
         (defmacro m [] (let [a (p (T.))] (install) [a (p (T.))])) (js/console.log (pr-str (m)))",
        "[1 2]\n",
    ),
    // `:extend-via-metadata`.
    (
        "(defprotocol P :extend-via-metadata true (p [x])) (extend-protocol P default (p [_] :default)) \
         (defmacro m [] [(p (with-meta [1] {`p (fn [_] :meta)})) (p [1])]) (js/console.log (pr-str (m)))",
        "[:meta :default]\n",
    ),
    // `implements?` sees the type's own implementations only.
    (
        "(defprotocol P (p [x])) (extend-protocol P string (p [x] 1)) (deftype T [] P (p [_] 2)) \
         (defmacro m [] [(implements? P \"x\") (satisfies? P \"x\") (implements? P (T.))]) \
         (js/console.log (pr-str (m)))",
        "[false true true]\n",
    ),
    // Variadic comparisons and identity through input occurrences.
    (
        "(defmacro m [x] [(< 1 3 2) (< 1) (<= 1 1 2) (> 3 2 1) (>= 1 2) (identical? x 1)]) \
         (js/console.log (pr-str (m 1)))",
        "[false true true true false true]\n",
    ),
    // `EmptyList` and `List` are distinct core types.
    (
        "(defprotocol P (p [x])) (extend-protocol P EmptyList (p [_] :empty) List (p [_] :list)) \
         (defmacro m [] [(p '()) (p '(1))]) (js/console.log (pr-str (m)))",
        "[:empty :list]\n",
    ),
    // One signature above arity three pads its arguments.
    (
        "(defprotocol P (p [x a b c])) (deftype T [] P (p [_ a b c] [a (nil? c)])) \
         (defmacro m [] (p (T.) 9)) (js/console.log (pr-str (m)))",
        "[9 true]\n",
    ),
];

#[test]
fn protocol_type_and_record_macros_match_the_legacy_compiler_and_run() {
    for (source, expected) in PROTOCOL_FIXTURES {
        assert_native_matches_legacy(source, expected);
    }
}

#[test]
fn fixed_native_arithmetic_preserves_coercion_occurrences_and_evaluation_order() {
    for (source, expected) in [
        (
            r#"(defmacro m [x] [(+ x) (* x) (+) (*) (- x) (/ x)
                 (+ x 2) (* x 2) (- 10 x 1) (/ 100 x 2)
                 (< x 4 9) (<= x 3 4) (> 5 x 1) (>= 4 x 3)])
               (js/console.log (pr-str (m 2)))"#,
            "[2 2 0 1 -2 0.5 4 4 7 25 true true true false]\n",
        ),
        (
            r#"(defmacro m [x] [(+ x) (* x) (+ x nil) (* x 2)
                 (+ "a") (* "2") (+ "a" "b") (+ "a" "b" "c")
                 (/ 1 (+ -0.0)) (/ 1 (* -0.0)) (< x) (< x 1) (>= x 0)])
               (js/console.log (pr-str (m nil)))"#,
            "[nil nil 0 0 \"a\" \"2\" \"ab\" \"abc\" ##-Inf ##-Inf true true true]\n",
        ),
        (
            r#"(defmacro m []
                (let [seen (js/Array.)
                      x (js/Object.)
                      _ (set! (.-valueOf x) (fn [] (.push seen 10) 2))
                      arg (fn [n] (.push seen n) n)
                      sum (+ x (arg 3) (arg 4))
                      comparison (< (arg 3) (arg 2) (arg 9))]
                  [sum comparison (vec seen)]))
               (js/console.log (pr-str (m)))"#,
            "[9 false [3 4 10 3 2 9]]\n",
        ),
    ] {
        assert_native_matches_legacy(source, expected);
    }
}

#[test]
fn direct_native_protocol_entries_keep_receivers_recur_and_scope_sensitive_methods() {
    for (source, expected) in [
        (
            r#"(defprotocol P :extend-via-metadata true (p [x]))
               (defmacro m []
                 (let [f (js* "(function(x) { 'use strict'; return this === null; })")]
                   (p (with-meta [1] {`p f}))))
               (js/console.log (m))"#,
            "true\n",
        ),
        (
            r#"(defprotocol P (p [x n]) (q [x] [x a] [x a b] [x a b c]))
               (deftype T [a] P
                 (p [this n] (if (= n 0) [a (.-a this)] (recur nil (dec n))))
                 (q [_] a) (q [_ b] (+ a b)) (q [_ b c] (+ a b c)) (q [_ b c d] (+ a b c d)))
               (defmacro m [] (let [t (T. 7)] [(p t 3) (q t) (q t 1) (q t 1 2) (q t 1 2 3)]))
               (js/console.log (pr-str (m)))"#,
            "[[7 7] 7 8 10 13]\n",
        ),
        (
            r#"(defprotocol P (p [x]) (q [x]))
               (deftype T [^:mutable a] P
                 (p [_] (set! a (inc a)) (fn [] a))
                 (q [_] (this-as receiver (.-a receiver))))
               (defmacro m [] (let [t (T. 1) f (p t) x (q t) g (p t)] [(f) x (g)]))
               (js/console.log (pr-str (m)))"#,
            "[3 2 3]\n",
        ),
        (
            r#"(defprotocol P (p [x]) (q [x]))
               (deftype T [a] P (p [_] (js* "this")) (q [_] (this-as receiver (.-a receiver))))
               (defmacro m [] (let [t (T. 8)] [(identical? t (p t)) (q t)]))
               (js/console.log (pr-str (m)))"#,
            "[true 8]\n",
        ),
    ] {
        assert_native_matches_legacy(source, expected);
    }
}

#[test]
fn native_core_bindings_preserve_with_redefs_and_restoration() {
    for (source, expected) in [
        (
            r#"(defmacro m [] [(even? 2)
                   (with-redefs [even? (fn [_] false)] (even? 2))
                   (even? 2)])
               (js/console.log (pr-str (m)))"#,
            "[true false true]\n",
        ),
        (
            r#"(defmacro m [] [(binding [even? (fn [_] false)] (even? 2))
                   (try (with-redefs [even? (fn [_] false)]
                          (throw (js/Error. (str (even? 2)))))
                     (catch :default e (.-message e)))
                   (even? 2)])
               (js/console.log (pr-str (m)))"#,
            "[false \"false\" true]\n",
        ),
        (
            r#"(defmacro m [] (let [original even?]
                   (try (set! cljs.core/even? (fn [_] false)) (even? 2)
                     (finally (set! cljs.core/even? original)))))
               (js/console.log (m))"#,
            "false\n",
        ),
    ] {
        assert_native_matches_legacy(source, expected);
    }
}

#[test]
fn native_core_property_writes_preserve_call_overrides() {
    for source in [
        r#"(defmacro m [] (let [saved (.-call cljs.core/+)]
             (try (set! (.-call cljs.core/+) (fn [_ a b] 42)) (+ 1 2)
               (finally (set! (.-call cljs.core/+) saved)))))
           (js/console.log (m))"#,
        r#"(declare install)
           (defmacro m [] (install) (+ 1 2))
           (defn install [] (set! (.-call cljs.core/+) (fn [_ a b] 42)))
           (js/console.log (m))"#,
    ] {
        assert_native_matches_legacy(source, "42\n");
    }
}

#[test]
fn native_core_writes_through_javascript_holes_observe_replacements() {
    for (source, expected) in [
        (
            r#"(defmacro m [] (let [before (even? 2)]
                 (js* "(~{} = function (_) { return false; })" cljs.core/even?)
                 [before (even? 2)]))
               (js/console.log (pr-str (m)))"#,
            "[true false]\n",
        ),
        (
            r#"(declare install)
               (defmacro m [] (let [before (+ 1 2)] (install) [before (+ 1 2)]))
               (defn install []
                 (js* "(~{}.call = function (_, a, b) { return 42; })" cljs.core/+))
               (js/console.log (pr-str (m)))"#,
            "[3 42]\n",
        ),
    ] {
        assert_native_matches_legacy(source, expected);
    }
}

#[test]
fn native_core_reads_observe_replacements_in_demanded_helpers() {
    for (source, expected) in [
        (
            r#"(declare install)
               (defmacro m [] (let [before (even? 2)] (install) [before (even? 2)]))
               (defn install [] (set! cljs.core/even? (fn [_] false)))
               (js/console.log (pr-str (m)))"#,
            "[true false]\n",
        ),
        (
            r#"(declare helper)
               (defmacro m [] [(with-redefs [even? (fn [_] false)] (helper)) (helper)])
               (defn helper [] (even? 2))
               (js/console.log (pr-str (m)))"#,
            "[false true]\n",
        ),
        (
            r#"(declare install)
               (defmacro m [] (let [before (inc 2)] (install) [before (inc 2)]))
               (defn install [] (set! cljs.core/inc (fn [_] 42)))
               (js/console.log (pr-str (m)))"#,
            "[3 42]\n",
        ),
        (
            r#"(declare install)
               (defmacro m [] (let [before (/ 8 2)] (install) [before (/ 8 2)]))
               (defn install [] (set! cljs.core// (fn [_ _] 42)))
               (js/console.log (pr-str (m)))"#,
            "[4 42]\n",
        ),
        (
            r#"(declare install)
               (defmacro m [] (let [before (even? 2)] (install) [before (even? 2)]))
               (defmacro installer [] '(defn install [] (set! cljs.core/even? (fn [_] false))))
               (installer)
               (js/console.log (pr-str (m)))"#,
            "[true false]\n",
        ),
        (
            r#"(declare install)
               (defmacro m [] (let [before (even? 2)] (install) [before (even? 2)]))
               (defmacro installer []
                 (list 'defn 'install [] (list 'set! 'cljs.core/even? '(fn [_] false))))
               (installer)
               (js/console.log (pr-str (m)))"#,
            "[true false]\n",
        ),
        (
            r#"(declare install)
               (defmacro m [] (let [before (even? 2)] (install) [before (even? 2)]))
               (defmacro installer [op]
                 (list 'defn 'install [] (list op 'cljs.core/even? '(fn [_] false))))
               (installer set!)
               (js/console.log (pr-str (m)))"#,
            "[true false]\n",
        ),
        (
            r#"(declare install)
               (defmacro m [] (let [before (even? 2)] (install) [before (even? 2)]))
               (defmacro installer [target] `(defn ~'install [] (set! ~target (fn [~'_] false))))
               (installer cljs.core/even?)
               (js/console.log (pr-str (m)))"#,
            "[true false]\n",
        ),
    ] {
        assert_native_matches_legacy(source, expected);
    }
    for (helper, app, expected) in [
        (
            r#"(ns helper (:refer-clojure :exclude [even?]))
               (defn even? [_] false)
               (defn probe [] [(even? 2) (cljs.core/even? 2)])"#,
            r#"(ns app (:require [helper :as h]))
               (defmacro m [] (h/probe))
               (js/console.log (pr-str (m)))"#,
            "[false true]\n",
        ),
        (
            r#"(ns helper) (defn install [] (set! cljs.core/even? (fn [_] false)))"#,
            r#"(ns app (:require [helper :as h]))
               (defmacro m [] (let [before (even? 2)] (h/install) [before (even? 2)]))
               (js/console.log (pr-str (m)))"#,
            "[true false]\n",
        ),
        (
            r#"(ns helper (:require [cljs.core :as c]))
               (defn install [] (set! c/even? (fn [_] false)))"#,
            r#"(ns app (:require [helper :as h]))
               (defmacro m [] (let [before (even? 2)] (h/install) [before (even? 2)]))
               (js/console.log (pr-str (m)))"#,
            "[true false]\n",
        ),
        (
            r#"(ns helper (:refer-clojure :rename {even? original-even?}))
               (defn install [] (set! original-even? (fn [_] false)))"#,
            r#"(ns app (:require [helper :as h]))
               (defmacro m [] (let [before (even? 2)] (h/install) [before (even? 2)]))
               (js/console.log (pr-str (m)))"#,
            "[true false]\n",
        ),
    ] {
        let root = temp_dir::TempDir::new("native-core-replacement");
        std::fs::write(root.join("helper.cljs"), helper).unwrap();
        std::fs::write(root.join("app.cljs"), app).unwrap();
        for jobs in [1, 2] {
            let compile = |native| {
                let mut host = MacroHost::new();
                if native {
                    host.use_native_macros();
                }
                let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
                    "app",
                    &[root.path().to_path_buf()],
                    &mut host,
                    Default::default(),
                    &mut Vec::new(),
                    light_cljs::compiler::EntryRecorders {
                        slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                        ..Default::default()
                    },
                )
                .unwrap();
                emit_js(&program)
            };
            let native = compile(true);
            assert_eq!(program_output(&native), expected, "{helper}: jobs {jobs}");
            assert_eq!(native, compile(false), "{helper}: jobs {jobs}");
        }
    }
    let source = r#"(declare install)
        (defmacro m [] (let [before (even? 2)] (install) [before (even? 2)]))
        (js/console.log (pr-str (m)))"#;
    let helper = "(fn [] (set! cljs.core/even? (fn [_] false)))";
    let mut host = MacroHost::new();
    host.preload_native_helper("cljs.user/install", helper)
        .unwrap();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let native = program_output(&emit_js(&program));
    assert_eq!(native, "[true false]\n");
    let legacy = light_cljs::compile(&format!("(def install {helper}) {source}")).unwrap();
    assert_eq!(native, program_output(&legacy));
}

#[test]
fn native_core_replacements_reanalyze_helpers_from_earlier_artifacts() {
    use light_cljs::{compiler::EntryRecorders, namespace_artifact::NamespaceCapture};
    let root = temp_dir::TempDir::new("native-core-artifact");
    std::fs::write(
        root.join("helper.cljs"),
        "(ns helper) (defn probe [] (even? 2))",
    )
    .unwrap();
    let cold = r#"(ns app (:require [helper :as h]))
        (defmacro m [] (h/probe)) (js/console.log (m))"#;
    let changed = r#"(ns app (:require [helper :as h]))
        (defmacro m [] (let [before (h/probe)]
          (set! cljs.core/even? (fn [_] false)) [before (h/probe)]))
        (js/console.log (pr-str (m)))"#;
    for jobs in [1, 2] {
        std::fs::write(root.join("app.cljs"), cold).unwrap();
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut artifact = None;
        let mut capture = |captured: NamespaceCapture| {
            if captured.namespace == "helper" {
                artifact = Some(captured.artifact.unwrap());
            }
        };
        light_cljs::compiler::analyze_namespace_entry_recorded(
            "app",
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                capture: Some(&mut capture),
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
        let artifact = artifact.unwrap();
        let lookup = |name: &str| (name == "helper").then_some(&artifact);
        let reservation = light_cljs::generated_names::Reservation::of([&artifact.names]);
        std::fs::write(root.join("app.cljs"), changed).unwrap();
        let compile = |native: bool, installed: bool| {
            let mut host = MacroHost::new();
            if native {
                host.use_native_macros();
            }
            let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
                "app",
                &[root.path().to_path_buf()],
                &mut host,
                Default::default(),
                &mut Vec::new(),
                EntryRecorders {
                    install: installed.then_some(&lookup),
                    reserve: installed.then_some(&reservation),
                    slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                    ..Default::default()
                },
            )
            .unwrap();
            emit_js(&program)
        };
        let native = compile(true, true);
        assert_eq!(program_output(&native), "[true false]\n", "jobs {jobs}");
        assert_eq!(native, compile(true, false), "cold, jobs {jobs}");
        assert_eq!(native, compile(false, false), "legacy, jobs {jobs}");
    }
}

#[test]
fn native_generated_names_do_not_capture_raw_javascript() {
    for (source, expected) in [
        (
            r#"(defmacro m [] [(even? 2) (js* "typeof nativeCore$isEven")
                   (js* "typeof nativeCoreGetter$isEven")])
               (js/console.log (pr-str (m)))"#,
            "[true \"undefined\" \"undefined\"]\n",
        ),
        (
            r#"(defmacro m [] [(if false (cycle [1]) :unused)
                   (js* "typeof nativeCoreGetter$cycle")])
               (js/console.log (pr-str (m)))"#,
            "[:unused \"undefined\"]\n",
        ),
        (
            r#"(defprotocol P (p [x]))
               (deftype T [] P (p [_] ((fn [] (js* "typeof receiver2")))))
               (defmacro m [] (p (T.)))
               (js/console.log (m))"#,
            "undefined\n",
        ),
    ] {
        assert_native_matches_legacy(source, expected);
    }
}

#[test]
fn native_core_bindings_keep_unsupported_getters_at_the_original_read() {
    let source = r#"(defmacro m [x]
             (let [seen (js/Array.)
                   unused (if x (cycle [1]) :unreached)
                   caught (try (cycle (do (.push seen 1) [1]))
                            (catch :default e :caught))]
               [unused caught (vec seen)]))
           (js/console.log (pr-str (m false)))"#;
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    assert_eq!(
        program_output(&emit_js(&program)),
        "[:unreached :caught []]\n"
    );
}

#[test]
fn record_helpers_publish_once_at_one_and_two_slots() {
    // A record built by a helper initializer and a function over records:
    // each value is initialized once and shared by the consuming tasks.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/records");
    for jobs in [1, 2] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let mut warnings = Vec::new();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
            "rec.app",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut warnings,
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                stats: Some(&mut stats),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("jobs {jobs}: {error}"));
        assert_eq!(
            program_output(&emit_js(&program)),
            "[8080 \"localhost\"] [9090 :tls]\n",
            "jobs {jobs}"
        );
        assert_eq!(stats.native_helpers.per_task, 0, "jobs {jobs}");
        assert!(stats.native_helpers.published >= 2, "jobs {jobs}");
    }
}

/// What a native namespace build at some slots produced: its JavaScript or
/// error, its source mappings at two optimization levels, its warnings and
/// the actual helper factories it compiled.
#[cfg(debug_assertions)]
#[derive(Debug, PartialEq)]
struct NativeBuild {
    output: Result<String, String>,
    mapped: Vec<(String, light_cljs::source_map::SourceMapping)>,
    warnings: Vec<(String, String, usize, std::ops::Range<usize>)>,
    compiled: usize,
}

#[cfg(debug_assertions)]
fn native_build(
    entry: &str,
    slots: usize,
    perturbation: Option<light_cljs::scheduler::Perturbation>,
) -> (NativeBuild, light_cljs::compile_stats::NativeHelperCounts) {
    let (build, stats) =
        native_build_in("native-helper-demand/pool", entry, slots, perturbation, &[]);
    (build, stats.native_helpers)
}

/// [`native_build`] of `entry` under `tests/fixtures/{fixtures}`, with
/// `--cache-blocker` namespaces, and all of its stats.
#[cfg(debug_assertions)]
fn native_build_in(
    fixtures: &str,
    entry: &str,
    slots: usize,
    perturbation: Option<light_cljs::scheduler::Perturbation>,
    blockers: &[String],
) -> (NativeBuild, light_cljs::CompileStats) {
    use light_cljs::application::{ApplicationOptions, OptimizationLevel};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixtures);
    let mut host = MacroHost::new();
    host.use_native_macros();
    let mut stats = light_cljs::CompileStats::new();
    let mut warnings = Vec::new();
    let analyzed = light_cljs::compiler::analyze_namespace_entry_recorded(
        entry,
        std::slice::from_ref(&root),
        &mut host,
        Default::default(),
        &mut warnings,
        light_cljs::compiler::EntryRecorders {
            slots: light_cljs::scheduler::Slots::new(slots).unwrap(),
            stats: Some(&mut stats),
            perturbation,
            blockers,
            ..Default::default()
        },
    );
    let mapped = analyzed.as_ref().map_or_else(
        |_| Vec::new(),
        |(program, _)| {
            [OptimizationLevel::None, OptimizationLevel::Prepare]
                .into_iter()
                .map(|optimize| {
                    light_cljs::prepare_application(
                        program,
                        ApplicationOptions {
                            optimize,
                            ..Default::default()
                        },
                    )
                    .emit_js_mapped()
                })
                .collect()
        },
    );
    let build = NativeBuild {
        output: analyzed
            .map(|(program, _)| emit_js(&program))
            .map_err(|error| error.to_string()),
        mapped,
        warnings: warnings
            .into_iter()
            .map(|w| (w.kind.to_owned(), w.message, w.source_index, w.span))
            .collect(),
        compiled: stats.native_helpers.compiled,
    };
    (build, stats)
}

/// Native namespace builds at one, two, four and eight slots under the
/// scheduler's seeded perturbation (randomized job-boundary delays, batches
/// and steal victims, fixed assignments, one-job admission) match the
/// unperturbed one-slot build: JavaScript or error, source maps, warnings
/// and compiled helper factories. Seed 1, or 1 to `LIGHT_CLJS_STRESS_SEEDS`
/// (12 for the acceptance stress, about five minutes in a debug build).
#[cfg(debug_assertions)]
#[test]
fn native_pool_builds_match_one_slot_under_seeded_scheduling_stress() {
    use light_cljs::scheduler::{Limits, Perturbation};
    let seeds = std::env::var("LIGHT_CLJS_STRESS_SEEDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1u64);
    for entry in ["many.app", "stress.app", "demand.app", "demand.failure"] {
        let (reference, serial) = native_build(entry, 1, None);
        if entry != "demand.failure" {
            let js = reference.output.as_ref().unwrap();
            assert!(!program_output(js).is_empty(), "{entry}");
        }
        for seed in 1..=seeds {
            for slots in [1, 2, 4, 8] {
                for mode in 0..3 {
                    let perturbation = Perturbation {
                        seed,
                        fixed: mode == 1,
                        limits: (mode == 2).then(|| Limits {
                            batch_jobs: 1,
                            admitted_jobs: 1,
                            ..Default::default()
                        }),
                        ..Default::default()
                    };
                    let (build, counts) = native_build(entry, slots, Some(perturbation));
                    assert_eq!(
                        build, reference,
                        "{entry}: seed {seed}, {slots} slots, mode {mode}"
                    );
                    // Each helper factory is compiled once per build, and
                    // each helper, whose value publishes, initializes as
                    // often as at one slot.
                    assert_eq!(counts.compiled, serial.compiled);
                    assert_eq!(
                        (counts.published, counts.per_task),
                        (serial.published, serial.per_task),
                        "{entry}: seed {seed}, {slots} slots, mode {mode}"
                    );
                }
            }
        }
    }
}

/// The effect-lane fixtures (`tests/fixtures/native-lanes`,
/// docs/compiler-concurrency.md, "Effect lanes (light-cljs-480)"): each
/// entry, whether it needs `--cache-blocker counter.macros`, and what its
/// one-slot program prints, or `Err` with a part of its error.
#[cfg(debug_assertions)]
const LANE_FIXTURES: &[(&str, bool, Result<&str, &str>)] = &[
    // Shared counter: `dep`'s unit and `main`'s top-level call.
    ("counter.main", false, Ok("1 2\n")),
    ("counter.main", true, Ok("1 2\n")),
    // Sibling namespaces whose units all take the next value.
    (
        "siblings.main",
        false,
        Ok("[1 2 [3 4] 5 6 7 8 18 10 11 24]\n"),
    ),
    // Source-order macro generations appending to one log.
    (
        "generations.main",
        false,
        Ok("[:early [:v2 :late] [:v2 :a1] [:v2 :a2] [:v2 :b1] [:b :b2] [:v2 :main]]\n"),
    ),
    // `defonce` keeps an atom and a definition, and fills a declaration.
    (
        "once.main",
        false,
        Ok("[[:a1 0 42 7 1] [:a2 1 42 7 1] [:b1 2 42 7 1] [:b2 3 42 7 1] [:main 4 42 7 1]]\n"),
    ),
    // A failed initializer is cached: its effect counts once.
    ("failure.main", false, Ok("11 21 31 41\n")),
    // The fourth unit in source order fails, whichever runs first.
    ("tickets.main", false, Err("tickets/b.cljs:3:8")),
    // An earlier unit demands a helper whose initializer touches the
    // shared atom.
    ("nested.main", false, Ok("[1 101 101 102 103]\n")),
    // The same within one namespace: a later unit may take the helper
    // first.
    ("inherit.main", false, Ok("[1 101 101 102 103]\n")),
    // Every atom operation, with the legacy runtimes' values (checked
    // against the V8 build at one slot), and a swap whose function resets
    // the same atom.
    (
        "ops.main",
        false,
        Ok(
            "[[1 2 true false 4 true 5 7] [1 1 [20 6]] [7 8 true false 10 true 11 13] \
            [13 14 true false 16 true 17 19]]\n",
        ),
    ),
    // A unit fails without touching an atom; the units after it must not
    // touch the counter before the earlier unit has read it.
    ("ordfail.main", false, Err("ordfail/a.cljs:3:8")),
];

/// Compile-time atoms keep their `--jobs 1` order at one, two, four and
/// eight slots under the scheduler's seeded perturbation (light-cljs-480):
/// each lane fixture's JavaScript or error, source maps, warnings and
/// helper counts match the unperturbed one-slot build, without
/// `--cache-blocker` and with it. Seed 1, or 1 to `LIGHT_CLJS_STRESS_SEEDS`.
#[cfg(debug_assertions)]
#[test]
fn effect_lanes_keep_one_slot_atom_effects_under_seeded_scheduling_stress() {
    use light_cljs::scheduler::{Limits, Perturbation};
    let seeds = std::env::var("LIGHT_CLJS_STRESS_SEEDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1u64);
    for &(entry, blocked, expected) in LANE_FIXTURES {
        let blockers = if blocked {
            vec!["counter.macros".to_owned()]
        } else {
            Vec::new()
        };
        let (reference, serial) = native_build_in("native-lanes", entry, 1, None, &blockers);
        match (&reference.output, expected) {
            (Ok(js), Ok(printed)) => assert_eq!(program_output(js), printed, "{entry}"),
            (Err(error), Err(part)) => assert!(error.contains(part), "{entry}: {error}"),
            (output, _) => panic!("{entry}: {output:?}"),
        }
        // One slot runs everything in lane order: atoms were checked, and
        // nothing waited.
        if reference.output.is_ok() {
            assert!(serial.lanes.entered > 0, "{entry}");
            assert_eq!(serial.lanes.waits, 0, "{entry}");
        }
        for seed in 1..=seeds {
            for slots in [1, 2, 4, 8] {
                for mode in 0..3 {
                    let perturbation = Perturbation {
                        seed,
                        fixed: mode == 1,
                        limits: (mode == 2).then(|| Limits {
                            batch_jobs: 1,
                            admitted_jobs: 1,
                            ..Default::default()
                        }),
                        ..Default::default()
                    };
                    let (build, stats) = native_build_in(
                        "native-lanes",
                        entry,
                        slots,
                        Some(perturbation),
                        &blockers,
                    );
                    let at = format!(
                        "{entry} (blocked {blocked}): seed {seed}, {slots} slots, mode {mode}"
                    );
                    assert_eq!(build, reference, "{at}");
                    let (counts, serial) = (stats.native_helpers, serial.native_helpers);
                    assert_eq!(counts.compiled, serial.compiled, "{at}");
                    assert_eq!(
                        (counts.published, counts.per_task),
                        (serial.published, serial.per_task),
                        "{at}"
                    );
                }
            }
        }
    }
}

/// A later unit, `inherit.app/t`, takes the helper `snapshot` first: its
/// initializer parks for its lane on the shared counter, behind `x` and
/// `s`, which the hook holds until it has (with one-job batches, eight
/// slots give `t` a worker of its own). `s` then demands `snapshot`, so the
/// initializer inherits `s`'s position and runs, instead of each waiting
/// for the other; the output is the one-slot build's.
#[cfg(debug_assertions)]
#[test]
fn an_earlier_unit_demanding_a_parked_initializer_lends_it_its_lane() {
    let (reference, _) = native_build_in("native-lanes", "inherit.main", 1, None, &[]);
    assert_eq!(
        program_output(reference.output.as_ref().unwrap()),
        "[1 101 101 102 103]\n"
    );
    for seed in 1..=3 {
        let slots = 8;
        let perturbation = light_cljs::scheduler::Perturbation {
            seed,
            lane_hold: vec!["inherit.app/x".into(), "inherit.app/s".into()],
            limits: Some(light_cljs::scheduler::Limits {
                batch_jobs: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (build, stats) = native_build_in(
            "native-lanes",
            "inherit.main",
            slots,
            Some(perturbation),
            &[],
        );
        assert_eq!(build, reference, "seed {seed}");
        assert!(stats.lanes.inherited >= 1, "seed {seed}: {:?}", stats.lanes);
    }
}

/// Function combinators at compile time, including constructors passed as
/// values, callable collections, input occurrences and variadic invocation.
const COMBINATOR_FIXTURES: &[(&str, &str)] = &[
    (
        "(defmacro m [x] [((comp) x) ((comp inc) x) ((comp inc inc) x) \
         ((comp inc inc inc inc) x) ((comp inc +) 1 2 3 4) ((comp vector +)) \
         (identical? identity (comp))]) \
         (js/console.log (pr-str (m 0)))",
        "[0 1 2 4 11 [0] true]\n",
    ),
    (
        "(defmacro m [] (let [f (fn [x] x)] [(identical? f (comp f)) (identical? f (partial f)) \
         ((partial + 10)) ((partial + 10 20 30 40) 1 2 3 4) ((partial :x) {:x 42})])) \
         (js/console.log (pr-str (m)))",
        "[true true 10 110 42]\n",
    ),
    (
        "(defmacro m [x] [(identity x) ((constantly x)) ((constantly x) 1 2 3 4) \
         ((complement identity) x) ((complement (constantly 0))) ((complement +) 1 2 3 4)]) \
         (js/console.log (pr-str (m false)) (pr-str (m nil)))",
        "[false false false true false false] [nil nil nil true false false]\n",
    ),
    (
        "(defmacro m [] [((juxt + vector)) ((juxt + vector) 1 2 3 4) \
         ((juxt :x {:x :mapped} #{:x}) :x) ((juxt [:zero :one] {1 :mapped} #{1}) 1) \
         ((juxt inc dec identity) 2)]) \
         (js/console.log (pr-str (m)))",
        "[[0 []] [10 [1 2 3 4]] [nil :mapped :x] [:one :mapped 1] [3 1 2]]\n",
    ),
    (
        "(defmacro m [n f] [((fnil vector :a) n) ((fnil vector :a :b) n f) \
         ((fnil vector :a :b :c) n n) ((fnil vector :a :b :c) n f n 4 5) \
         ((fnil vector :a) 0 \"\" false nil)]) (js/console.log (pr-str (m nil false)))",
        "[[:a] [:a false] [:a :b] [:a false :c 4 5] [0 \"\" false nil]]\n",
    ),
    (
        "(defmacro m [] (let [c comp p partial j juxt n fnil k constantly o complement i identity] \
         [((c inc +) 1 2) ((p vector :a :b) :c) ((j + vector) 1 2) ((n vector :x) nil) \
         ((k false) 1) ((o i) nil)])) (js/console.log (pr-str (m)))",
        "[4 [:a :b :c] [3 [1 2]] [:x] false true]\n",
    ),
    (
        "(defmacro m [] (let [a (js/Array.) f (fn [x] (.push a 1) (inc x)) \
         g (fn [& xs] (.push a 2) (apply + xs)) c (comp f g) j (juxt f f)] \
         [(c 1 2) (j 3) (vec a)])) (js/console.log (pr-str (m)))",
        "[4 [4 4] [2 1 1 1]]\n",
    ),
    (
        "(defmacro m [] (let [f (js* \"(function(){'use strict'; return this === null;})\") \
         c comp p partial j juxt o complement n fnil] \
         [(f) ((comp f)) ((comp identity f)) ((partial f)) ((partial f 1)) \
          ((juxt f)) ((complement f)) ((fnil f :x) nil) \
          ((c identity f)) ((p f 1)) ((j f)) ((o f)) ((n f :x) nil)])) \
         (js/console.log (pr-str (m)))",
        "[true true true true true [true] false true true true [true] false true]\n",
    ),
    (
        "(defmacro m [] [((juxt inc dec identity (constantly :four)) 2) \
         ((comp 'x identity) {'x 42}) ((partial 'x) {'x 43}) \
         ((fnil vector :a :b :c) js/undefined nil false 4)]) \
         (js/console.log (pr-str (m)))",
        "[[3 1 2 :four] 42 43 [:a :b false 4]]\n",
    ),
    (
        "(defmacro m [] (let [message (fn [e] (js* \"(typeof ~{} === 'string' ? ~{} : ~{}.message)\" e e e))] \
         [(try (fnil vector) (catch :default e (message e))) \
         (mapv nil? ((fnil vector :a))) \
         (mapv nil? ((fnil vector :a :b) nil)) \
         (try ((comp identity [:a]) :x) (catch :default e (message e)))])) \
         (js/console.log (pr-str (m)))",
        "[\"Invalid arity: 1\" [false true true] [false false true] \"Key must be integer\"]\n",
    ),
    (
        "(defmacro m [x] (let [c comp v [:first :second]] \
         [(v x) ((comp identity v) x) ((c identity v) x) \
         ((comp identity v) 1.5) ((comp identity v) -0) \
         (try ((comp identity v) -0.5) (catch :default e (.-message e))) \
         (try ((comp identity v) 2) (catch :default e (.-message e))) \
         (try ((comp identity v) js/NaN) (catch :default e (.-message e)))])) \
         (js/console.log (pr-str (m 0.5)))",
        "[:first :first :first :second :first \"Index out of bounds\" \"Index out of bounds\" \"Index out of bounds\"]\n",
    ),
];

#[test]
fn function_combinators_match_the_legacy_compiler_and_run() {
    for (source, expected) in COMBINATOR_FIXTURES {
        assert_native_matches_legacy(source, expected);
    }
}

#[test]
fn native_vector_and_map_entry_call_matrix_matches_legacy() {
    let bounds = "[:error \"Index out of bounds\"]";
    let key_type = "[:error \"Key must be integer\"]";
    // name, index expression, vector result, entry result (without a fallback).
    // Keep each numeric and non-numeric category explicit: vector calls truncate
    // positive fractions, while entries accept only exact numeric zero or one.
    let indices = [
        ("zero", "0", "[:value :a]", "[:value :a]"),
        ("one", "1", "[:value 1]", "[:value 1]"),
        ("negative-zero", "-0.0", "[:value :a]", "[:value :a]"),
        ("fraction-zero", "0.5", "[:value :a]", bounds),
        ("fraction-one", "1.5", "[:value 1]", bounds),
        ("negative-integer", "-1", bounds, bounds),
        ("negative-fraction", "-0.5", bounds, bounds),
        ("past-end", "2", bounds, bounds),
        ("large-integer", "9007199254740991", bounds, bounds),
        ("nan", "js/NaN", bounds, bounds),
        ("infinity", "js/Infinity", bounds, bounds),
        ("negative-infinity", "(js* \"-Infinity\")", bounds, bounds),
        ("string-zero", "\"0\"", key_type, bounds),
        ("string-one", "\"1\"", key_type, bounds),
        ("empty-string", "\"\"", key_type, bounds),
        ("nil", "nil", key_type, bounds),
        ("undefined", "js/undefined", key_type, bounds),
        ("false", "false", key_type, bounds),
        ("true", "true", key_type, bounds),
        ("keyword", ":zero", key_type, bounds),
        ("symbol", "'zero", key_type, bounds),
        ("vector", "[]", key_type, bounds),
        ("map", "{}", key_type, bounds),
        (
            "coercible-object",
            "(js* \"({valueOf() { return 0; }})\")",
            key_type,
            bounds,
        ),
    ];
    let table: String = indices
        .iter()
        .map(|(name, index, _, _)| format!("[:{name} {index}] "))
        .collect();
    let source = format!(
        r#"(defmacro m [fallback]
            (let [e (first {{:a 1}}) v [:a 1] c comp p partial
                  inspect (fn [f index]
                            [(try [:value (f)] (catch :default ex [:error (.-message ex)]))
                             (try [:value (f index)] (catch :default ex [:error (.-message ex)]))
                             (try [:value (f index fallback)] (catch :default ex [:error (.-message ex)]))
                             (try [:value (f index fallback :extra)] (catch :default ex [:error (.-message ex)]))])
                  callables [[:vector/direct v] [:entry/direct e]
                             [:vector/comp (comp identity v)] [:entry/comp (comp identity e)]
                             [:vector/saved-comp (c identity v)] [:entry/saved-comp (c identity e)]
                             [:vector/partial (partial (fn [_ & xs] (apply v xs)) nil)]
                             [:entry/partial (partial (fn [_ & xs] (apply e xs)) nil)]
                             [:vector/saved-partial (p (fn [_ & xs] (apply v xs)) nil)]
                             [:entry/saved-partial (p (fn [_ & xs] (apply e xs)) nil)]
                             [:vector/apply (fn [& xs] (apply v xs))]
                             [:entry/apply (fn [& xs] (apply e xs))]]]
              [(mapv (fn [[label index]]
                       [label (mapv (fn [[kind f]] [kind (inspect f index)]) callables)])
                     [{table}])
               (mapv (fn [d]
                       (mapv (fn [index]
                               (try (identical? d (e index d)) (catch :default ex false)))
                             [0.5 1.5 -1 -0.5 2 js/NaN js/Infinity "0" nil false []]))
                     [nil false 0 "" js/undefined {{:a 1}} [:fallback] fallback])]))
           (js/console.log (pr-str (m :default)))"#
    );
    let mut rows = Vec::new();
    for (name, _, vector, entry) in indices {
        let mut calls = Vec::new();
        for path in [
            "direct",
            "comp",
            "saved-comp",
            "partial",
            "saved-partial",
            "apply",
        ] {
            for (kind, value) in [("vector", vector), ("entry", entry)] {
                let zero = if kind == "entry" {
                    "[:error \"Invalid arity: 0\"]"
                } else {
                    key_type
                };
                let two = if kind == "entry" && value == bounds {
                    "[:value :default]"
                } else {
                    value
                };
                let three = if kind == "entry" {
                    "[:error \"Invalid arity: 3\"]"
                } else {
                    value
                };
                let results = format!("[{zero} {value} {two} {three}]");
                calls.push(format!("[:{kind}/{path} {results}]"));
            }
        }
        rows.push(format!("[:{name} [{}]]", calls.join(" ")));
    }
    let fallbacks = format!("[{}]", ["true"; 11].join(" "));
    let expected = format!(
        "[[{}] [{}]]\n",
        rows.join(" "),
        vec![fallbacks; 8].join(" ")
    );
    // Establish every row against QuickJS before comparing the native backend.
    #[cfg(feature = "quickjs")]
    assert_eq!(
        program_output(&light_cljs::compile(&source).unwrap()),
        expected
    );
    assert_native_matches_legacy(&source, &expected);
}

#[test]
fn native_map_entry_calls_match_legacy() {
    assert_native_matches_legacy(
        r#"(defmacro m []
            (let [e (first {:a 1}) v [:a 1] c comp
                  message (fn [f x]
                            (try (f x) (catch :default e (.-message e))))]
              [(e 0) (e 1) ((comp identity e) 0) ((c identity e) 1)
               (message e 0.5) (message e 1.5)
               (message (comp identity e) 0.5) (message (comp identity e) 1.5)
               (message (c identity e) 0.5) (message (c identity e) 1.5)
               (v 0.5) (v 1.5) ((comp identity v) 0.5) ((c identity v) 1.5)
               ((vec e) 0.5) ((with-meta e nil) 0.5)
               ((with-meta e {:tag true}) 1.5)
               ((assoc e 0 :a) 0.5) ((assoc e 1 2) 1.5) ((conj e 2) 2.5)]))
           (js/console.log (pr-str (m)))"#,
        "[:a 1 :a 1 \"Index out of bounds\" \"Index out of bounds\" \
         \"Index out of bounds\" \"Index out of bounds\" \"Index out of bounds\" \
         \"Index out of bounds\" :a 1 :a 1 :a :a 1 :a 2 2]\n",
    );
}

#[test]
fn native_callable_objects_keep_the_legacy_call_abi() {
    assert_native_matches_legacy(
        r#"(defmacro m []
            (let [f (js* "({offset: 1, call: function(_, x) { return this.offset + x; }})")
                  c comp p partial]
              [(f 41) ((comp identity f) 41) ((partial f 41))
               ((c identity f) 41) ((p f 41))]))
           (js/console.log (pr-str (m)))"#,
        "[42 42 42 42 42]\n",
    );
}

#[test]
fn native_failed_calls_do_not_evaluate_arguments() {
    assert_native_matches_legacy(
        r#"(defmacro m [n]
            (let [a (js/Array.) b (js/Array.) c (js/Array.) f nil
                  g (js* "({get call() { throw new Error('getter'); }})")]
              (try (f (.push a 1)) (catch :default e nil))
              (try (g (.push b 1)) (catch :default e nil))
              (try (n (.push c 1)) (catch :default e nil))
              [(vec a) (vec b) (vec c)]))
           (js/console.log (pr-str (m nil)))"#,
        "[[] [] []]\n",
    );
    // Read the method once before the argument replaces it, retaining its
    // receiver and the callee's own evaluation effects.
    assert_native_matches_legacy(
        r#"(defmacro m []
            (let [a (js/Array.)
                  f (js* "({get call() { ~{}.push(0); return this.fn; },
                            fn: function(_, x) { ~{}.push(2); return x; }})" a a)]
              [((do (.push a -1) f)
                (do (.push a 1) (set! (.-fn f) (fn [_ x] 99)) 7))
               (vec a)]))
           (js/console.log (pr-str (m)))"#,
        "[7 [-1 0 1 2]]\n",
    );
}

#[test]
fn native_fixed_calls_retain_custom_function_calls_and_null_receivers() {
    assert_native_matches_legacy(
        r#"(defmacro m []
            (let [seen (js/Array.)
                  f (js* "(function(x) { 'use strict'; return this === null ? x : -1; })")
                  _ (js/Object.defineProperty f "call"
                      (js* "({configurable: true, get() { ~{}.push(0); return Function.prototype.call; }})" seen))
                  first (f (do (.push seen 1)
                               (js/Object.defineProperty f "call"
                                 (js* "({value: function(_, x) { return x + 10; }})"))
                               7))
                  second (f 8)
                  nested ((fn [a b] [a b]) ((fn [x] x) 2) ((fn [x] x) 3))]
              [first second nested (vec seen)]))
           (js/console.log (pr-str (m)))"#,
        "[7 18 [2 3] [0 1]]\n",
    );
}

#[test]
fn partial_zero_arity_matches_legacy() {
    assert_native_matches_legacy(
        r#"(defmacro m []
            (let [p partial f (partial) g (p)]
              [(nil? f) (nil? g)
               (js* "(typeof ~{} === 'function')" f)
               (js* "(typeof ~{} === 'function')" g)
               (try (f) (catch :default e true))
               (try (g) (catch :default e true))]))
           (js/console.log (pr-str (m)))"#,
        "[false false true true true true]\n",
    );
}

#[test]
fn combinator_helpers_publish_once_at_one_two_and_four_slots() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/combinators");
    let mut expected_js = None;
    for jobs in [1, 2, 4] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
            "combinators.app",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut Vec::new(),
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                stats: Some(&mut stats),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("jobs {jobs}: {error}"));
        let js = emit_js(&program);
        assert_eq!(
            program_output(&js),
            "[3 [:p 1] [2 0 1] [:a 1] false :v] [4 [:p 2] [3 1 2] [:a 2] false :v]\n"
        );
        assert_eq!(stats.native_helpers.compiled, 6, "jobs {jobs}");
        assert_eq!(stats.native_helpers.published, 6, "jobs {jobs}");
        assert_eq!(stats.native_helpers.per_task, 0, "jobs {jobs}");
        if let Some(expected) = &expected_js {
            assert_eq!(&js, expected, "jobs {jobs}");
        } else {
            expected_js = Some(js);
        }
        #[cfg(any(feature = "quickjs", feature = "v8-macros"))]
        {
            let mut legacy = MacroHost::new();
            let program = light_cljs::compiler::analyze_namespace_entry(
                "combinators.app",
                std::slice::from_ref(&root),
                &mut legacy,
                Default::default(),
                &mut Vec::new(),
            )
            .unwrap();
            assert_eq!(emit_js(&program), *expected_js.as_ref().unwrap());
        }
    }
}

#[test]
fn native_scalar_predicates_and_binding_probes_match_legacy() {
    for (source, expected) in [
        (
            "(defmacro m [n f z s] (let [some some?] \
             [(some? n) (some n) (some? f) (some f) (some? z) (some z) \
              (some? s) (some s) (some? js/undefined) (some js/undefined)])) \
             (js/console.log (pr-str (m nil false 0 \"\")))",
            "[false false true true true true true true false false]\n",
        ),
        (
            "(defmacro m [z f n] (let [zero zero? no false?] \
          [(zero? z) (zero z) (zero? \"0\") (zero? n) (false? f) (no f) (false? z) (false? n)])) \
          (js/console.log (pr-str (m 0 false nil)))",
            "[true true false false true true false false]\n",
        ),
        (
            "(declare absent) (def n nil) (def f false) (def u js/undefined) \
          (defmacro m [] [(exists? absent) (exists? n) (exists? f) (exists? u) \
          (exists? unknown) (exists? js/noSuchGlobal.child) (exists? js/Math.max) \
          (exists? (identity false)) (exists? (identity js/undefined))]) \
          (js/console.log (pr-str (m)))",
            "[false true true false false false true true false]\n",
        ),
        (
            "(defmacro m [] (nil? (resolve 'absent))) (js/console.log (m))",
            "true\n",
        ),
        (
            "(def hits (atom 0)) (def value (do (swap! hits inc) 42)) \
          (defmacro m [] [(exists? value) @hits (exists? value) @hits]) \
          (js/console.log (pr-str (m)))",
            "[true 1 true 1]\n",
        ),
    ] {
        assert_native_matches_legacy(source, expected);
    }
}

#[test]
fn native_exists_does_not_suppress_an_initializer_error() {
    // The definition throws in the emitted application too, so compare its
    // compilation without running it. The macro assertion checks the value.
    let source = r#"
        (def broken (throw (js/Error. "unavailable definition: cljs.user/broken")))
        (defmacro m []
          (assert (= :caught
                     (try (exists? broken)
                          (catch :default e
                            (if (.includes (.-message e) "unavailable definition: cljs.user/broken")
                              :caught :wrong-error)))))
          true)
        (m)"#;
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    assert_eq!(emit_js(&program), light_cljs::compile(source).unwrap());
}

#[test]
fn native_resolve_reports_the_missing_var_runtime() {
    let source = "(declare absent) (defmacro m [] (resolve 'absent)) (m)";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let error = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("unsupported operation: native macro runtime Var reflection"),
        "{error}"
    );
}

#[test]
fn native_helpers_use_zero_in_the_verbatim_mutual_recursion_fixture() {
    // MUTUAL_RECURSION from tests/compiler/macros.rs, followed by an
    // observable application of the same macro.
    let source = r#"
          (declare even-step odd-step)
          (defn even-step [n] (if (zero? n) true (odd-step (dec n))))
          (defn odd-step [n] (if (zero? n) false (even-step (dec n))))
          (defmacro m [] (even-step 20))
          (m)
          (js/console.log (m))"#;
    assert_native_matches_legacy(source, "true\n");
}

/// Stateful cuerdas pipeline and each new call shape, compared in one table
/// against QuickJS when built with `bbp-macros,quickjs`. The common operands
/// are imported forms, so scalar and element occurrences are exercised too.
#[test]
fn native_transducer_call_shapes_match_legacy() {
    for (body, expected) in [
        (
            r#"[(into [] (partition-by odd?) xs)
                (into [] (comp (partition-by odd?) (filter some?) (mapcat identity)) xs)
                (into [] (comp (filter odd?) (map inc)) xs)]"#,
            "[[[1 1] [2] [3 3]] [1 1 2 3 3] [2 2 4 4]]",
        ),
        (
            r#"(let [p partition-by f filter m map mc mapcat i into]
                 [(i [] (p odd?) xs)
                  (i [] (comp (p odd?) (f some?) (mc identity)) xs)
                  (i [] (comp (f odd?) (m inc)) xs)])"#,
            "[[[1 1] [2] [3 3]] [1 1 2 3 3] [2 2 4 4]]",
        ),
        (
            r#"[(into) (into xs) (into [] nil) (into [] [])
                (into [] (partition-by odd?) nil) (into [] (partition-by odd?) [])
                (into [] (filter some?) nil) (into [] (map inc) [])
                (into [] (mapcat identity) nil) (into [] (mapcat identity) []) (vec (map))
                (into [] (mapcat (fn [x] nil)) xs)
                (into [] (filter some?) [nil false 0 ""])]"#,
            "[[] [1 1 2 3 3] [] [] [] [] [] [] [] [] [] [] [false 0 \"\"]]",
        ),
        (
            r#"[(into '(9) (filter odd?) xs) (into nil (map inc) [1 2])
                (into {:a 1} (map identity) [[:b 2]])
                (meta (into (with-meta [] {:tag :kept}) (map inc) xs))
                (into #{1} (filter odd?) xs)]"#,
            "[(3 3 1 1 9) (3 2) {:a 1, :b 2} {:tag :kept} #{1 3}]",
        ),
        (
            r#"(let [s string? r reduced p reduced? e ensure-reduced u unreduced
                       v (r text) nested (reduced v)]
                 [(string? text) (s text) (string? nil) (s false) (s 0)
                  (p v) (p text) (= text @v) (= text (u v))
                  (identical? v (e v)) (reduced? (e nil))
                  (identical? v (unreduced nested)) (= text (unreduced text))
                  (= (r 1) (r 1)) (= v v)])"#,
            "[true true false false false true false true true true true true true false true]",
        ),
        (
            r#"[(into [] (partition-by (fn [x] [(odd? x)])) xs)
                (into [] (partition-by identity) [:cljs.core/none 1 2])
                (vec (map vec (partition-by odd? xs)))
                (vec (partition-by odd? nil))
                (vec (mapcat vector [1 2] [3 4]))
                (vec (filter some? [nil false 0 ""]))]"#,
            "[[[1 1] [2] [3 3]] [[:cljs.core/none 1] [2]] [[1 1] [2] [3 3]] [] [1 3 2 4] [false 0 \"\"]]",
        ),
        (
            r#"(let [log (js/Array.)
                       xf (comp (partition-by (fn [x] (.push log (str "k" x)) (odd? x)))
                                (filter (fn [p] (.push log (str "f" (count p))) true))
                                (mapcat (fn [p] (.push log (str "m" (count p))) p)))
                       result (into [] xf xs)]
                 [result (.join log ",")])"#,
            "[[1 1 2 3 3] \"k1,k1,k2,f2,m2,k3,f1,m1,k3,f2,m2\"]",
        ),
        (
            r#"(let [xf (comp (partition-by odd?) (filter some?) (mapcat identity))]
                 [(into [] xf xs) (into [] xf []) (into [] xf [2 2]) (into [] xf xs)])"#,
            "[[1 1 2 3 3] [] [2 2] [1 1 2 3 3]]",
        ),
        (
            r#"[(transduce (map inc) + xs) (transduce (map inc) + 100 xs)
                (transduce (filter odd?) + nil) (transduce (filter odd?) + 10 [])
                (transduce (map inc) (completing + (fn [x] (* 10 x))) xs)
                (transduce (map inc) (completing +) 100 xs)
                (let [c completing t transduce] (t (map inc) (c + (fn [x] (* 10 x))) 100 xs))]"#,
            "[15 115 0 10 150 115 1150]",
        ),
        (
            r#"[(reduce (fn [r x] (reduced (+ r x))) [1 2 100])
                (reduce (fn [r x] (if (= x 2) (reduced (conj r x)) (conj r x))) []
                        (cons 1 (lazy-seq (cons 2 (lazy-seq (throw (js/Error. "tail")))))))
                (reduce + []) (reduce + nil) (reduce + 10 nil)
                (reduced? (reduce (fn [r x] (reduced (reduced x))) 0 [1 2]))]"#,
            "[3 [1 2] 0 0 10 true]",
        ),
        (
            r#"(let [log (js/Array.)
                       xf (mapcat (fn [x] (.push log (str "map" x)) [x x]))
                       rf (fn ([] []) ([r] (.push log "done") r)
                            ([r x] (.push log (str "step" x)) (reduced (conj r x))))
                       result (transduce xf rf [1 2 3])]
                 [result (.join log ",")])"#,
            "[[1] \"map1,step1,done\"]",
        ),
        (
            r#"(let [log (js/Array.)
                       xf (comp (partition-by (fn [x] (.push log x) (odd? x))) (mapcat identity))
                       rf (fn ([r] (.push log "done") r) ([r x] (reduced (conj r x))))
                       result (transduce xf rf [] xs)]
                 [result (.join log ",")])"#,
            "[[1] \"1,1,2,done\"]",
        ),
        (
            r#"(let [log (js/Array.)
                       rf (fn ([r] (.push log "done") r)
                            ([r p] (.push log "step") (reduced (conj r p))))]
                 [(transduce (partition-by odd?) rf [] [1 1])
                  (transduce (partition-by odd?) rf [] [1 1 2 3])
                  (.join log ",")])"#,
            "[[[1 1]] [[1 1]] \"step,done,step,done\"]",
        ),
        (
            r#"(let [xf (partition-by odd?) rf (completing conj)
                       a (xf rf) b (xf rf)
                       a0 (a) b0 (b)
                       a1 (a a0 1) b1 (b b0 2)
                       a2 (a a1 2) b2 (b b1 3)]
                 [(a a2) (b b2) (a []) (b [])])"#,
            "[[[1] [2]] [[2] [3]] [] []]",
        ),
        (
            r#"[(into [] (fn [rf] (fn ([] (rf)) ([r] (conj (rf r) :done)) ([r x] (rf r x)))) [])
                (let [f ((map vector) (completing conj))] [(f) (f []) (f [] 1 2)])
                (let [f ((mapcat vector) (completing conj))] [(f) (f []) (f [] 1 2)])
                (let [f ((filter some?) (completing conj))] [(f) (f []) (f [] false)])]"#,
            "[[:done] [[] [] [[1 2]]] [[] [] [1 2]] [[] [] [false]]]",
        ),
        (
            r#"(let [log (js/Array.)
                       rf (js* "({get call() { ~{}.push('get'); return function(_, result, input) { ~{}.push('run'); return result; }; }})" log log)
                       f (fn [x] (.push log "map") x)
                       m map a ((map f) rf) b ((m f) rf)]
                 [(a [] 1) (b [] 2) (.join log ",")])"#,
            "[[] [] \"get,map,run,get,map,run\"]",
        ),
        (
            r#"(let [log (js/Array.)
                       xf (fn [rf] (js* "({get call() { ~{}.push('get'); return function(_, result, input) { if (arguments.length === 2) ~{}.push('done'); else { ~{}.push('step'); result++; } return result; }; }})" log log log))
                       result (transduce xf + 0 [1 2])]
                 [result (.join log ",")])"#,
            "[2 \"get,get,step,get,step,done\"]",
        ),
        (
            r#"(let [error (fn [f a] (try (apply f a) :ok (catch :default e (.-message e))))]
                 [(error map []) (error mapcat []) (error filter []) (error filter [identity nil nil])
                  (error partition-by []) (error partition-by [identity nil nil])
                  (error into [nil nil nil nil]) (error transduce []) (error transduce [identity +])
                  (error transduce [identity + nil nil nil])
                  (error completing []) (error completing [+ identity identity])
                  (error ((partition-by identity) (completing conj)) [[] 1 2])
                  (error ((filter identity) (completing conj)) [[] 1 2])])"#,
            "[:ok \"Invalid arity: 0\" \"Invalid arity: 0\" \"Invalid arity: 3\" \"Invalid arity: 0\" \"Invalid arity: 3\" \"Invalid arity: 4\" \"Invalid arity: 0\" \"Invalid arity: 2\" \"Invalid arity: 5\" \"Invalid arity: 0\" \"Invalid arity: 3\" \"Invalid arity: 3\" \"Invalid arity: 3\"]",
        ),
    ] {
        let source = format!(
            "(defmacro m [xs text] (list 'quote {body})) \
             (js/console.log (pr-str (m [1 1 2 3 3] \"s\")))"
        );
        assert_native_matches_legacy(&source, &format!("{expected}\n"));
    }

    // Editable targets supply an adapter with step/completion arities only;
    // noneditable targets supply conj, including its initialization and
    // variadic arities. A transducer may inspect those arities itself.
    for (target, empty, input, result, variadic, editable) in [
        ("[]", "[]", "[1 2]", "[1 2]", "", true),
        (
            "(first {:a 1})",
            "[:a 1]",
            "[1 2]",
            "[:a 1 1 2]",
            "[:a 1 1 1 2 2]",
            false,
        ),
        ("'(9)", "(9)", "[1 2]", "(2 1 9)", "(2 2 1 1 9)", false),
        ("'()", "()", "[1 2]", "(2 1)", "(2 2 1 1)", false),
        ("nil", "nil", "[1 2]", "(2 1)", "(2 2 1 1)", false),
        ("{:a 1}", "{:a 1}", "[[:b 2]]", "{:a 1, :b 2}", "", true),
        (
            "(array-map :a 1)",
            "{:a 1}",
            "[[:b 2]]",
            "{:a 1, :b 2}",
            "",
            true,
        ),
        ("#{1}", "#{1}", "[1 2]", "#{1 2}", "", true),
    ] {
        let metadata = if target == "nil" {
            "nil".to_owned()
        } else {
            format!("(meta (into (with-meta {target} {{:tag :kept}}) (map identity) {input}))")
        };
        let source = format!(
            r#"(defmacro m []
                 (let [log (js/Array.)
                       init (fn [rf]
                              (.push log (try (empty? (rf)) (catch :default e (.-message e))))
                              (fn ([r] (.push log (try (empty? (rf)) (catch :default e (.-message e)))) (rf r))
                                  ([r x] (rf r x))))
                       run (fn [i c] (i {target} init c))
                       result [(run into {input}) (run into []) (run into nil)]
                       initialized (try (into {target} (fn [rf] (rf) rf) {input})
                                        (catch :default e (.-message e)))
                       variadic (try (into {target}
                                           (fn [rf] (fn ([r] (rf r)) ([r x] (rf r x x))))
                                           {input})
                                     (catch :default e (.-message e)))
                       metadata {metadata}]
                   (list 'quote [result (vec log) initialized variadic metadata])))
               (js/console.log (pr-str (m)))"#
        );
        let init = if editable {
            "\"Invalid arity: 0\""
        } else {
            "true"
        };
        let initialized = if editable { init } else { result };
        let variadic = if editable {
            "\"Invalid arity: 3\""
        } else {
            variadic
        };
        let metadata = if target == "nil" {
            "nil"
        } else {
            "{:tag :kept}"
        };
        let expected = format!(
            "[[{result} {empty} {empty}] [{}] {initialized} {variadic} {metadata}]\n",
            [init; 6].join(" ")
        );
        // Check the table against the oracle before exercising native code.
        #[cfg(feature = "quickjs")]
        assert_eq!(
            program_output(&light_cljs::compile(&source).unwrap()),
            expected
        );
        assert_native_matches_legacy(&source, &expected);
    }

    // Default sorted maps now preserve the noneditable reducer contract.
    // Sorted sets remain an explicit unsupported boundary.
    for (target, input, expected, operation) in [
        (
            "(sorted-map :a 1)",
            "[[:b 2]]",
            "[{:a 1, :b 2} {:a 1} {:a 1}]\n",
            "sortedMap",
        ),
        (
            "(sorted-set 1)",
            "[2 1]",
            "[#{1 2} #{1} #{1}]\n",
            "sortedSet",
        ),
    ] {
        let source = format!(
            "(defmacro m [] (list 'quote \
             [(into {target} (fn [rf] (rf) rf) {input}) \
              (into {target} (fn [rf] (rf) rf) []) \
              (into {target} (fn [rf] (rf) rf) nil)])) \
             (js/console.log (pr-str (m)))"
        );
        #[cfg(feature = "quickjs")]
        assert_eq!(
            program_output(&light_cljs::compile(&source).unwrap()),
            expected
        );
        if operation == "sortedMap" {
            assert_native_matches_legacy(&source, expected);
            continue;
        }
        let mut host = MacroHost::new();
        host.use_native_macros();
        let error = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!(
                "unsupported operation: native macro runtime core.{operation}"
            )),
            "{error}"
        );
    }
}

#[test]
fn native_transducer_helpers_publish_and_resume_without_replay() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/transducers");
    let expected = concat!(
        "[[1 1 2 3 :done] [2 2 1 :done] [:done] [[1 2] [2 1]] ",
        "\"input1,input1,input2,before-step,3,after-step,out1,out1,input3,out2,out3,before-flush,3,after-flush,before-done,after-done,",
        "input2,before-step,3,after-step,input2,before-step,3,after-step,input1,out2,out2,out1,before-done,after-done,before-done,after-done\"] ",
        "[[1 2 2 3 3 :done] [2 2 1 :done] [:done] [[1 2] [2 1]] ",
        "\"input1,input2,before-step,3,after-step,out1,input2,before-step,3,after-step,input3,out2,out2,input3,",
        "out3,before-flush,3,after-flush,out3,before-flush,3,after-flush,before-done,after-done,",
        "input2,before-step,3,after-step,input2,before-step,3,after-step,input1,out2,out2,out1,before-done,after-done,before-done,after-done\"]\n"
    );
    let mut reference = None;
    // Exercise both the seeded parallel schedule and fixed assignments with
    // one-job admission. Cold helper reads force VM waits in either mode;
    // no timing assumption controls when the callbacks reach those reads.
    for (jobs, single_admission) in [
        (1, false),
        (2, false),
        (4, false),
        (1, true),
        (2, true),
        (4, true),
    ] {
        let at = format!("jobs {jobs}, single admission {single_admission}");
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
            "transducers.app",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut Vec::new(),
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                stats: Some(&mut stats),
                #[cfg(debug_assertions)]
                perturbation: Some(light_cljs::scheduler::Perturbation {
                    seed: 1,
                    fixed: single_admission,
                    limits: Some(light_cljs::scheduler::Limits {
                        batch_jobs: 1,
                        admitted_jobs: if single_admission {
                            1
                        } else {
                            light_cljs::scheduler::Limits::default().admitted_jobs
                        },
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("{at}: {error}"));
        let js = emit_js(&program);
        assert_eq!(program_output(&js), expected, "{at}");
        assert_eq!(stats.native_helpers.compiled, 4, "{at}");
        assert_eq!(stats.native_helpers.published, 4, "{at}");
        assert_eq!(stats.native_helpers.per_task, 0, "{at}");
        // These are actual saved-VM dependency waits, even at one slot.
        assert!(
            stats.native_helpers.waits >= 4,
            "{at}: {:?}",
            stats.native_helpers
        );
        if let Some(reference) = &reference {
            assert_eq!(&js, reference, "{at}");
        } else {
            reference = Some(js);
        }
    }
    #[cfg(any(feature = "quickjs", feature = "v8-macros"))]
    {
        let program = light_cljs::compiler::analyze_namespace_entry(
            "transducers.app",
            std::slice::from_ref(&root),
            &mut MacroHost::new(),
            Default::default(),
            &mut Vec::new(),
        )
        .unwrap();
        let js = emit_js(&program);
        assert_eq!(program_output(&js), expected);
        assert_eq!(Some(js), reference);
    }
}

#[test]
fn native_verbatim_cuerdas_unsafe_concat_pipeline() {
    // Reduced from vendor/cuerdas/core.cljc in the read-only Penpot port.
    // The helper and unsafe-concat bodies are verbatim; c aliases cljs.core.
    let source = include_str!("fixtures/native-helper-demand/cuerdas/cuerdas/repro.cljc");
    let root = temp_dir::TempDir::new("native-cuerdas-transducers");
    std::fs::create_dir(root.join("cuerdas")).unwrap();
    std::fs::write(root.join("cuerdas/repro.cljc"), source).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let native = light_cljs::compiler::analyze_namespace_entry(
        "cuerdas.repro",
        &[root.path().to_path_buf()],
        &mut host,
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap();
    let native = emit_js(&native);
    assert_eq!(program_output(&native), "abXcd  ab\n");
    #[cfg(any(feature = "quickjs", feature = "v8-macros"))]
    {
        let legacy = light_cljs::compiler::analyze_namespace_entry(
            "cuerdas.repro",
            &[root.path().to_path_buf()],
            &mut MacroHost::new(),
            Default::default(),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(native, emit_js(&legacy));
    }
}

/// Macro code over lazy sequences, which the native unit runs on the
/// engine's once-realized cells (light-cljs-479; docs/native-macros.md,
/// "Lazy sequences"). Each expands as the legacy compiler does and runs.
const LAZY_FIXTURES: &[(&str, &str)] = &[
    (
        "(defmacro m [& xs] `(+ ~@(map (fn [x] `(* 30 ~x)) xs))) (js/console.log (m 1 2))",
        "90\n",
    ),
    (
        "(defmacro m [xs] (list 'quote [(map inc xs) (filter odd? xs) (remove odd? xs) \
         (keep (fn [x] (if (odd? x) (* 10 x) nil)) xs)])) (js/console.log (pr-str (m [1 2 3])))",
        "[(2 3 4) (1 3) (2) (10 30)]\n",
    ),
    (
        "(defmacro m [xs] (list 'quote [(take 2 xs) (drop 1 xs) (take-while odd? xs) (drop-while odd? xs) \
         (map-indexed vector xs)])) (js/console.log (pr-str (m (1 3 4 5))))",
        "[(1 3) (3 4 5) (1 3) (4 5) ([0 1] [1 3] [2 4] [3 5])]\n",
    ),
    (
        "(defmacro m [] (list 'quote [(take 5 (iterate inc 0)) (take 3 (repeat :x)) (repeat 2 :y) (range 4) \
         (range 2 5) (range 10 0 -3) (take 3 (range))])) (js/console.log (pr-str (m)))",
        "[(0 1 2 3 4) (:x :x :x) (:y :y) (0 1 2 3) (2 3 4) (10 7 4 1) (0 1 2)]\n",
    ),
    (
        "(defmacro m [xs] (list 'quote [(mapcat (fn [x] [x x]) xs) (interleave xs [:a :b :c]) \
         (partition 2 (range 5)) (partition 2 1 [1 2 3])])) (js/console.log (pr-str (m [1 2])))",
        "[(1 1 2 2) (1 :a 2 :b) ((0 1) (2 3)) ((1 2) (2 3))]\n",
    ),
    // A lazy sequence compares, hashes, counts and indexes as the list of
    // its elements.
    (
        "(defmacro m [xs] (let [s (map inc xs)] [(count s) (first s) (second s) (nth s 2) (vec (rest s)) \
         (= s [2 3 4]) (= s (list 2 3 4)) (= (hash s) (hash [2 3 4])) (seq? s) (realized? s) \
         (empty? (filter neg? xs))])) (js/console.log (pr-str (m [1 2 3])))",
        "[3 2 3 4 [3 4] true true true true true true]\n",
    ),
    (
        "(defmacro m [xs] [(reduce + (map inc xs)) (reduce + 10 xs) (into [] (map inc xs)) (into #{} xs) \
         (every? odd? xs) (some even? xs) (last xs) (vec (butlast xs)) (vec (reverse xs)) (mapv inc xs) \
         (filterv odd? xs)]) (js/console.log (pr-str (m [1 2 3])))",
        "[9 16 [2 3 4] #{1 2 3} false true 3 [1 2] [3 2 1] [2 3 4] [1 3]]\n",
    ),
    (
        "(defmacro m [xs] (list 'quote (lazy-seq (cons 1 (lazy-seq (concat xs (lazy-seq nil))))))) \
         (js/console.log (pr-str (m [2 3])))",
        "(1 2 3)\n",
    ),
    (
        "(defmacro m [xs] (list 'quote (map vector xs (range)))) (js/console.log (pr-str (m [:a :b])))",
        "([:a 0] [:b 1])\n",
    ),
    // A thunk's exception reaches the realizing expression.
    (
        "(defmacro m [xs] (let [s (map (fn [x] (if (= x 2) (throw (js/Error. \"two\")) x)) xs)] \
         (try (doall s) :done (catch :default e (.-message e))))) (js/console.log (m [1 2 3]))",
        "two\n",
    ),
    (
        "(defmacro m [xs] (cons 'list (map (fn [x] (list 'inc x)) xs))) (js/console.log (pr-str (m [1 2])))",
        "(2 3)\n",
    ),
    (
        "(defmacro m [xs] (list 'quote (with-meta (map inc xs) {:tag :t}))) (js/console.log (pr-str (m [1])))",
        "(2)\n",
    ),
    // Sequences of each kind a macro returns as quoted data: their kinds'
    // metadata, which the bridge exports, as the legacy runtime's
    // `LazySeq`, `ChunkedCons`, `ChunkedSeq`, `IntegerRange` and `Cons` have it.
    (
        "(defmacro m [v l] (list 'quote [(seq v) (seq l) (seq (map inc v)) (seq (map inc l)) (rest (map inc v)) \
         (next (map inc l)) (cons 0 (map inc v)) (concat v (map inc l)) (seq (concat v (map inc l))) \
         (seq (concat l (map inc v))) `(a ~@(map inc v)) (range 3) (rest (range 3))])) \
         (js/console.log (pr-str (m [1 2] (3 4))))",
        "[(1 2) (3 4) (2 3) (4 5) (3) (5) (0 2 3) (1 2 4 5) (1 2 4 5) (3 4 2 3) (cljs.user/a 2 3) (0 1 2) (1 2)]\n",
    ),
    // Over more than one chunk.
    (
        "(defmacro m [] (let [v (vec (range 70))] (list 'quote [(count (map inc v)) (nth (filter odd? v) 30) \
         (vec (take 3 (drop 33 (seq v))))]))) (js/console.log (pr-str (m)))",
        "[70 61 [33 34 35]]\n",
    ),
    // Realization boundaries: a vector of at most 32 elements is read one
    // element at a time, a longer one 32 at a time, `concat` one collection
    // at a time, and `doall n` realizes only a prefix.
    (
        "(defmacro m [] (let [boom (fn [x] (if (= x 2) (throw (js/Error. \"two\")) x)) a (js/Array.) b (js/Array.)] \
         [(first (map boom [1 2])) (first (concat (cons 1 (lazy-seq (throw (js/Error. \"tail\")))) [])) \
         (do (first (map-indexed (fn [i x] (.push a i) x) (vec (range 40)))) (.-length a)) \
         (let [s (map (fn [x] (.push b x) x) (list 1 2 3 4))] (doall 1 s) (.-length b))])) \
         (js/console.log (pr-str (m)))",
        "[1 1 32 2]\n",
    ),
    // `keep` of imported `nil` and lazy values in a symbol's metadata.
    (
        "(defmacro m [xs] (list 'quote [(keep identity xs) (with-meta 'x {:foo (map inc [1])})])) \
         (let [[k x] (m [nil false 1])] (js/console.log (pr-str k) (pr-str (meta x))))",
        "(false 1) {:foo (2)}\n",
    ),
    // An explicit `LazySeq` construction keeps its metadata.
    (
        "(defmacro m [] (let [s (cljs.core/LazySeq. {:tag 1} (fn [] (list 1)) nil nil)] [(count s) (meta s)])) \
         (js/console.log (pr-str (m)))",
        "[1 {:tag 1}]\n",
    ),
];

#[test]
fn lazy_sequence_macros_match_the_legacy_compiler_and_run() {
    for (source, expected) in LAZY_FIXTURES {
        assert_native_matches_legacy(source, expected);
    }
}

/// `for` and `doseq` (expanded in Rust in a namespace build) over lazy and
/// chunked sequences, and lazy helper values: the same JavaScript at one,
/// two and four slots, the legacy compiler's in a build with a legacy
/// engine, and helper sequences built from lowered operations publish once.
#[test]
fn for_and_doseq_over_lazy_helpers_match_the_legacy_compiler_at_one_two_and_four_slots() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/lazy");
    let build = |host: &mut MacroHost, jobs: usize, stats: &mut light_cljs::CompileStats| {
        let mut warnings = Vec::new();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
            "seqs.app",
            std::slice::from_ref(&root),
            host,
            Default::default(),
            &mut warnings,
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                stats: Some(stats),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("jobs {jobs}: {error}"));
        assert!(warnings.is_empty(), "jobs {jobs}");
        emit_js(&program)
    };
    let legacy = build(
        &mut MacroHost::new(),
        1,
        &mut light_cljs::CompileStats::new(),
    );
    for jobs in [1, 2, 4] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let js = build(&mut host, jobs, &mut stats);
        assert_eq!(
            program_output(&js),
            "30 (0 2 [:item :p] [:item :q]) 35 [2 5 4 7] [[3 3] 14] [[3 3] 14]\n",
            "jobs {jobs}"
        );
        assert_eq!(js, legacy, "jobs {jobs}");
        // Arithmetic lowering lets `squares` publish, and binding the pure
        // core predicate once lets `evens` publish as well.
        assert_eq!(stats.native_helpers.compiled, 5, "jobs {jobs}");
        assert_eq!(stats.native_helpers.published, 5, "jobs {jobs}");
        assert_eq!(stats.native_helpers.per_task, 0, "jobs {jobs}");
    }
}
