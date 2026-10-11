#!/usr/bin/env python3
"""Bounded core gate from complete, already mapped upstream adapter programs.

This script generates evidence bindings, never execution passes or catalog writes.
Original EPL-1.0 source notices are retained in the pinned research checkout.
"""
from pathlib import Path
import argparse
import hashlib
import json
import sys

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / 'tests/reference/compiler-language'
sys.path.insert(0, str(HERE))
import extract_inventory as inv
import report

GROUPS = {
    'numeric': ['primitives_test.cljs::test-arithmetic',
                'primitives_test.cljs::test-equiv', 'primitives_test.cljs::test-bit-operations'],
    'collections': ['collections_test.cljs::test-map-operations',
                    'collections_test.cljs::test-sets', 'core_test.cljs::test-in-operations',
                    'core_test.cljs::test-contains?'],
    'state': ['core_test.cljs::test-atoms-and-volatile', 'core_test.cljs::test-delay',
              'core_test.cljs::swap-vals-returns-old-value', 'core_test.cljs::deref-swap-arities',
              'core_test.cljs::deref-reset-returns-old-value'],
    'control': ['primitives_test.cljs::test-case', 'primitives_test.cljs::test-list-comprehensions',
                'core_test.cljs::some->test', 'core_test.cljs::some->>test',
                'core_test.cljs::cond->test', 'core_test.cljs::cond->>test',
                'core_test.cljs::as->test', 'core_test.cljs::threading-loop-recur'],
    'exceptions': ['primitives_test.cljs::test-try-catch',
                   'core_test.cljs::test-data-conveying-exceptions'],
}

# Independent literal expectations supplement upstream direct-call arities.
# Saved functions prevent macro-only source expansion from replacing runtime
# higher-order call checks. Both entire expressions run in both compiler phases.
LITERALS = {
    'numeric': [
        ('saved-arithmetic', '[(+) (+ 1) (+ 1 2) (+ 1 2 3) (*) (* 2) (* 2 3) (* 2 3 4) (- 3) (- 3 2) (- 6 2 1) (/ 2) (/ 6 2) (/ 12 2 3) (let [a + m * s - d /] [(a) (a 1) (a 1 2) (a 1 2 3) (m) (m 2) (m 2 3) (m 2 3 4) (s 3) (s 3 2) (s 6 2 1) (d 2) (d 6 2) (d 12 2 3)])]', '[0 1 3 6 1 2 6 24 -3 1 3 0.5 3 2 [0 1 3 6 1 2 6 24 -3 1 3 0.5 3 2]]'),
        ('numeric-boundaries', '[(inc -1) (dec 1) (quot -7 3) (rem -7 3) (mod -7 3) (max 3) (max 1 3) (max 1 3 2) (min 3) (min 1 3) (min 3 1 2) (zero? 0) (pos? 1) (neg? -1) (even? 4) (odd? -3)]', '[0 0 -2 -1 2 3 3 3 3 1 1 true true true true true]'),
    ],
    'collections': [
        ('saved-collections', '(let [c conj a assoc d dissoc g get] [(c nil) (c [] 1) (c [] 1 2) (a nil :a 1) (a {} :a 1 :b 2) (d {:a 1}) (d {:a 1} :a) (d {:a 1 :b 2} :a :b) (g {:a nil} :a :missing) (g {} :a :missing) (count (list)) (vector) (hash-map) (hash-set)])', '[nil [1] [1 2] {:a 1} {:a 1, :b 2} {:a 1} {} {} nil :missing 0 [] {} #{}]'),
    ],
    'state': [
        ('watch-effects', '(let [a (atom 0) events (atom [])] (add-watch a :w (fn [k r old new] (swap! events conj [k (identical? r a) old new]))) (swap! a + 2) (remove-watch a :w) (reset! a 9) [@a @events (compare-and-set! a 9 10) @a])', '[9 [[:w true 0 2]] true 10]'),
    ],
    'control': [
        ('short-circuit-effects', '(let [events (atom []) f (fn [x] (swap! events conj x) x)] [(and (f false) (f :skipped)) (or (f nil) (f :chosen) (f :skipped)) (if-let [x (f 2)] (inc x) 0) (when-let [x (f 3)] (dec x)) @events])', '[false :chosen 3 2 [false nil :chosen 2 3]]'),
    ],
    'exceptions': [
        ('exception-cause-finally', '(let [events (atom []) cause (ex-info "cause" {:cause true}) error (ex-info "outer" {:n 2} cause)] [(try (throw error) (catch ExceptionInfo e [(ex-message e) (ex-data e) (identical? cause (ex-cause e))]) (finally (swap! events conj :finally))) @events])', '[["outer" {:n 2} true] [:finally]]'),
    ],
}

# Independent higher-order results exercise each factory at its public arity,
# then call the returned function. Saved factory calls supplement direct calls;
# literal results and ordered effects remain adjacent, independent expectations.
LITERALS['control'].extend([
    ('higher-order-factories', '(let [saved comp p partial] [(identity :value) ((constantly :fixed) 1 2) ((complement nil?) :value) ((comp) 7) ((comp inc) 7) ((comp inc inc) 7) ((comp inc inc inc) 7) ((comp inc inc inc inc) 7) ((saved inc inc inc inc) 7) ((partial +) 1 2) ((partial + 1) 2 3) ((partial + 1 2) 3 4) ((partial + 1 2 3) 4) ((partial + 1 2 3 4) 5) ((p + 1 2 3 4) 5) ((fnil vector :a) nil 2) ((fnil vector :a :b) nil nil 3) ((fnil vector :a :b :c) nil nil nil 4) ((every-pred pos?) 1 2) ((every-pred pos? even?) 2 4) ((every-pred pos? even? number?) 2 4) ((every-pred pos? even? number? integer?) 2 4) ((every-pred pos?) 1 -1) ((some-fn nil?) nil) ((some-fn nil? even?) 3 4) ((some-fn nil? even? pos?) -3 3) ((some-fn nil? even? pos? number?) -3 3) ((some-fn nil?) 1 2) ((juxt inc) 3) ((juxt inc dec) 3) ((juxt inc dec identity) 3) ((juxt inc dec identity str) 3)])', '[:value :fixed true 7 8 9 10 11 11 3 6 10 10 15 15 [:a 2] [:a :b 3] [:a :b :c 4] true true true true false true true true true false [4] [4 2] [4 2 3] [4 2 3 \"3\"]]'),
    ('higher-order-effects', '(let [events (atom []) cached (memoize (fn [x] (swap! events conj x) (* x 2))) step (fn step [n] (if (zero? n) :done (fn [] (step (dec n)))))] [(cached 3) (cached 3) (cached 4) @events (trampoline (fn [] (step 3))) (trampoline step 4)])', '[6 6 8 [3 4] :done :done]'),
])
LITERALS['collections'].append(
    ('collection-function-arities', '[(not-empty []) (not-empty [1]) (every? pos? [1 2]) (not-every? pos? [1 -1]) (some #{2} [1 2 3]) (not-any? neg? [1 2]) (distinct? 1) (distinct? 1 2) (distinct? 1 2 3) (distinct? 1 2 1) (into) (into [1]) (into [] [1 2]) (into [] (map inc) [1 2]) (mapv inc [1 2]) (mapv + [1 2] [3 4]) (mapv + [1] [2] [3]) (mapv + [1] [2] [3] [4]) (filterv even? [1 2 3 4]) (zipmap [:a :b] [1 2]) (frequencies [:a :b :a]) (group-by even? [1 2 3 4]) (vec (map-indexed vector [:a :b])) (into [] (keep identity) [nil false 2]) (vec (keep identity [nil false 2])) (into [] (keep-indexed (fn [i x] (when x i))) [true nil true]) (vec (keep-indexed (fn [i x] (when x i)) [true nil true]))]', '[nil [1] true true 2 true true true true false [] [1] [1 2] [2 3] [2 3] [4 6] [6] [10] [2 4] {:a 1, :b 2} {:a 2, :b 1} {false [1 3], true [2 4]} [[0 :a] [1 :b]] [false 2] [false 2] [0 2] [0 2]]'))

# Numeric inputs stay in the adopted JavaScript Number domain; the complete
# upstream Long/Integer table remains a separate source fixture obligation.
LITERALS['numeric'].append(
    ('number-predicate-domain', '[(number? 0) (number? "0") (number? ##NaN) (number? ##Inf) (integer? 0) (integer? 1.5) (integer? ##NaN) (integer? ##Inf) (integer? -0.0) (integer? 1000000000000) (integer? 1e21) (int? 0) (int? 1.5) (int? 1e21) (pos-int? 1) (pos-int? 0) (pos-int? -1) (pos-int? 1.5) (neg-int? -1) (neg-int? 0) (neg-int? 1) (nat-int? 0) (nat-int? -0.0) (nat-int? 1) (nat-int? -1) (float? 0) (float? ##NaN) (float? "0") (double? 1.5) (double? ##Inf) (double? nil) (infinite? ##Inf) (infinite? ##-Inf) (infinite? ##NaN) (infinite? 0) (NaN? ##NaN) (NaN? 0)]', '[true false true true true false false false true true false true false false true false false false true false false true true true false true true false true true false true true false false true false]'))
LITERALS['collections'].append(
    ('type-predicate-domain', '(let [cloned (reify ICloneable (-clone [_] :copy)) f (fn [x] x) m (with-meta f {:tag :m}) effects (atom [])] [(any? nil) (any? false) (any? 0) (object? #js {}) (object? #js []) (object? nil) (object? {}) (string? "a") (string? :a) (char? "a") (char? "ab") (char? "😀") (fn? f) (fn? m) (fn? :a) (ifn? f) (ifn? :a) (ifn? []) (ifn? 1) (js-fn? f) (js-fn? m) (js-fn? nil) (js-fn? {}) (js-fn? (do (swap! effects conj :once) f)) @effects (seqable? nil) (seqable? "ab") (seqable? #js [1]) (seqable? 1) (iterable? []) (iterable? nil) (js-iterable? "ab") (js-iterable? nil) (cloneable? cloned) (cloneable? nil) (clone cloned) (ifind? {}) (ifind? #js {}) (reduceable? []) (reduceable? #js {})])', '[true true true true false false false true false true false false true true false true true true false true false false false true [:once] true true true false true false true false true false :copy true false true false]'))

# Independently specified sequence and nested-update workflows retain whole
# factory/realization/effect paths. Infinite factories are only finitely consumed.
LITERALS['collections'].extend([
    ('sequence-consumer-arities', '(let [xs [1 2 3 4] nested [[1 2] [3 4]]] [(next nil) (vec (next xs)) (second xs) (ffirst nested) (vec (nfirst nested)) (fnext xs) (vec (nnext xs)) (last xs) (vec (nthrest xs 2)) (vec (nthnext xs 2)) (vec (butlast xs)) (vec (reverse xs)) (vec (rseq xs)) (peek xs) (pop xs) (empty xs) (empty? []) (coll? xs) (set? #{1}) (associative? xs) (sequential? xs) (sorted? (sorted-map)) (map? {}) (record? {}) (vector? xs) (seq? (seq xs)) (list? (list 1)) (reversible? xs) (counted? xs) (indexed? xs) (reduced? (reduced 3)) (unreduced (ensure-reduced 3)) (vec (prim-seq #js [4 5])) (vec (prim-seq #js [4 5] 1)) (vec (array-seq #js [4 5])) (vec (array-seq #js [4 5] 1))])', '[nil [2 3 4] 2 1 [2] 2 [3 4] 4 [3 4] [3 4] [1 2 3] [4 3 2 1] [4 3 2 1] 4 [1 2 3] [] true true true true true true true false true true true true true true true 3 [4 5] [5] [4 5] [5]]'),
    ('sequence-producer-arities', '(let [n (atom 0) f (fn [] (swap! n inc))] [@n (vec (take 3 (repeatedly f))) @n (vec (repeatedly 2 f)) @n (vec (take 4 (range))) (vec (range 4)) (vec (range 1 5)) (vec (range 1 6 2)) (vec (take 3 (repeat :a))) (vec (repeat 2 :a)) (vec (replicate 2 :b)) (vec (take 3 (iterate inc 1))) (vec (take 5 (cycle [1 2]))) (vec (concat)) (vec (concat [1])) (vec (concat [1] [2])) (vec (concat [1] [2] [3])) (vec (list* [1 2])) (vec (list* 1 [2 3])) (vec (list* 1 2 [3 4])) (vec (list* 1 2 3 [4 5])) (vec (list* 1 2 3 4 [5 6])) (vec (interleave)) (vec (interleave [:a :b])) (vec (interleave [:a :b] [1 2])) (vec (interleave [:a] [1] [:x])) (vec (interpose :x [1 2 3])) (mapv vec (partition 2 [1 2 3 4])) (mapv vec (partition 3 2 (range 7))) (mapv vec (partition 3 2 [:p] (range 6))) (mapv vec (partition-all 2 [1 2 3])) (mapv vec (partition-all 2 1 [1 2 3])) (mapv vec (partition-by even? [1 3 2 4 5])) (mapv vec (split-at 2 [1 2 3])) (mapv vec (split-with odd? [1 3 2 5])) (vec (drop-last [1 2 3])) (vec (drop-last 2 [1 2 3])) (vec (take-last 2 [1 2 3])) (vec (map + [1 2] [3 4])) (vec (map + [1] [2] [3])) (vec (map + [1] [2] [3] [4])) (vec (mapcat vector [1 2])) (vec (filter even? [1 2 3 4])) (vec (remove odd? [1 2 3 4])) (vec (take-nth 2 [1 2 3 4 5])) (vec (take-while pos? [1 2 0 3])) (vec (drop-while pos? [1 2 0 3]))])', '[0 [1 2 3] 3 [4 5] 5 [0 1 2 3] [0 1 2 3] [1 2 3 4] [1 3 5] [:a :a :a] [:a :a] [:b :b] [1 2 3] [1 2 1 2 1] [] [1] [1 2] [1 2 3] [1 2] [1 2 3] [1 2 3 4] [1 2 3 4 5] [1 2 3 4 5 6] [] [:a :b] [:a 1 :b 2] [:a 1 :x] [1 :x 2 :x 3] [[1 2] [3 4]] [[0 1 2] [2 3 4] [4 5 6]] [[0 1 2] [2 3 4] [4 5 :p]] [[1 2] [3]] [[1 2] [2 3] [3]] [[1 3] [2 4] [5]] [[1 2] [3]] [[1 3] [2 5]] [1 2] [1] [2 3] [4 6] [6] [10] [1 2] [2 4] [2 4] [1 3 5] [1 2] [0 3]]'),
    ('nested-update-name-arities', '(let [events (atom []) r (reduce-kv (fn [a k v] (swap! events conj [k v]) (conj a [k v])) [] (sorted-map :b 2 :a 1))] [(reduce + 0 [1 2 3]) r @events (vec (sort [3 1 2])) (vec (sort > [3 1 2])) (vec (sort-by count ["bb" "a" "ccc"])) (vec (sort-by count > ["bb" "a" "ccc"])) (vec (keys (sorted-map :b 2 :a 1))) (vec (vals (sorted-map :b 2 :a 1))) (key (find {:a 1} :a)) (val (find {:a 1} :a)) (select-keys {:a 1 :b 2} [:b :missing]) (merge) (merge-with +) (get-in {:a {:b false}} [:a :b] :missing) (get-in {} [:a :b] :missing) (update {:n 1} :n + 2) (update {:n 1} :n + 2 3) (update {:n 1} :n + 2 3 4) (update {:n 1} :n + 2 3 4 5) (update {:n 1} :n + 2 3 4 5 6) (update-in {:a {:n 1}} [:a :n] + 2) (update-in {:a {:n 1}} [:a :n] + 2 3) (update-in {:a {:n 1}} [:a :n] + 2 3 4) (update-in {:a {:n 1}} [:a :n] + 2 3 4 5) (update-in {:a {:n 1}} [:a :n] + 2 3 4 5 6) (max-key count "a") (max-key count "a" "bb") (max-key count "a" "bb" "ccc") (min-key count "a") (min-key count "a" "bb") (min-key count "a" "bb" "ccc") (symbol? (symbol "x")) (namespace (symbol "n" "x")) (name (keyword "x")) (namespace (keyword "n" "x")) (keyword? :x) (ident? :x) (simple-ident? :x) (qualified-ident? :n/x) (simple-symbol? (symbol "x")) (qualified-symbol? (symbol "n" "x")) (simple-keyword? :x) (qualified-keyword? :n/x) (keyword-identical? :x (keyword "x")) (symbol-identical? (symbol "x") (symbol "x"))])', '[6 [[:a 1] [:b 2]] [[:a 1] [:b 2]] [1 2 3] [3 2 1] ["a" "bb" "ccc"] ["ccc" "bb" "a"] [:a :b] [1 2] :a 1 {:b 2} nil nil false :missing {:n 3} {:n 6} {:n 10} {:n 15} {:n 21} {:a {:n 3}} {:a {:n 6}} {:a {:n 10}} {:a {:n 15}} {:a {:n 21}} "a" "bb" "ccc" "a" "a" "a" true "n" "x" "n" true true true true true true true true true true]'),
])


# Array identity/ordered effects and direct public protocol dispatch contracts.
LITERALS['collections'].extend([
    ('array-public', '(let [events (atom []) original #js [1 2] copied (aclone original) matrix (make-array nil 2 3) cube (make-array nil 2 1 2) simple (make-array 2) saved-get aget saved-set aset saved-into into-array obj #js {:base 10 :sum (fn [x y] (this-as self (+ (.-base self) x y)))}] (aset simple 0 (do (swap! events conj :first) 4)) (saved-set simple 1 (do (swap! events conj :second) 5)) (aset original 0 7) (aset matrix 1 2 9) [(alength (make-array 0)) (alength (make-array :ignored 2)) (mapv alength (array-seq matrix)) (mapv (fn [layer] (mapv alength (array-seq layer))) (array-seq cube)) (identical? (aget matrix 0) (aget matrix 1)) (aget matrix 1 2) (saved-get matrix 1 2) (vec original) (vec copied) (vec simple) @events (vec (array)) (vec (array 1)) (vec (array 1 2 3)) (vec (into-array [1 2])) (vec (into-array :ignored (list 3 4))) (vec (saved-into [5 6])) (vec (to-array (list 7 8))) (js-invoke obj "sum" 2 3)])', '[0 2 [3 3] [[2] [2]] false 9 9 [7 2] [1 2] [4 5] [:first :second] [] [1] [1 2 3] [1 2] [3 4] [5 6] [7 8] 15]'),
    ('protocol-public', '(let [xs [1 2] pair (find {:a 1} :a) a (atom 3) tagged (with-meta [1] {:tag :v})] [(-count xs) (-nth xs 1) (-nth xs 8 :missing) (-first (list 1 2)) (vec (-rest (list 1 2))) (vec (-next (list 1 2))) (vec (-seq xs)) (vec (-rseq xs)) (-lookup {:a 1} :a) (-lookup {} :a :missing) (-contains-key? {:a 1} :a) (-assoc {} :a 1) (-assoc-n [1] 0 2) (-conj [1] 2) (-dissoc {:a 1 :b 2} :a) (-disjoin #{1 2} 1) (-empty xs) (-peek xs) (-pop xs) (-reduce xs +) (-reduce xs + 10) (-kv-reduce (sorted-map :b 2 :a 1) (fn [acc k v] (conj acc [k v])) []) (-key pair) (-val pair) (-compare [1] [2]) (-deref a) (-meta tagged) (let [result (-with-meta [1] {:tag :x})] [result (-meta result)])])', '[2 2 :missing 1 [2] [2] [1 2] [2 1] 1 :missing true {:a 1} [2] [1 2] {:b 2} #{2} [] 2 [1] 3 13 [[:a 1] [:b 2]] :a 1 -1 3 {:tag :v} [[1] {:tag :x}]]'),
    ('protocol-custom-state', '(let [state (atom 1) events (atom []) r (reify IDeref (-deref [_] @state) IReset (-reset! [_ value] (swap! events conj [:reset value]) (reset! state value)) ISwap (-swap! [_ f] (swap! events conj [:swap 0]) (swap! state f)) (-swap! [_ f a] (swap! events conj [:swap 1]) (swap! state f a)) (-swap! [_ f a b] (swap! events conj [:swap 2]) (swap! state f a b)) (-swap! [_ f a b xs] (swap! events conj [:swap 3 (vec xs)]) (apply swap! state f a b xs))) v (volatile! 2)] [(-swap! r inc) (-swap! r + 3) (-swap! r + 1 2) (-swap! r + 1 2 (list 3 4)) (-reset! r 20) (-deref r) @events (-vreset! v 4) (-deref v) (-realized? (delay 3)) (let [d (delay 3)] [(-deref d) (-realized? d)])])', '[2 5 8 18 20 20 [[:swap 0] [:swap 1] [:swap 2] [:swap 3 [3 4]] [:reset 20]] 4 4 false [3 true]]'),
])

# Chunk workflows keep buffer mutation, finite sequence consumption, metadata,
# protocol dispatch and error observations together. The public factory calls
# are followed by observations of their results, including saved runtime calls.
LITERALS['collections'].extend([
    ('chunk-public-pipeline', '(let [events (atom []) b (chunk-buffer 3) append chunk-append finish chunk saved-first chunk-first saved-rest chunk-rest saved-next chunk-next] [(count b) (chunk-append b (do (swap! events conj :a) 2)) (append b (do (swap! events conj :b) 4)) (chunk-append b (do (swap! events conj :c) 6)) (count b) (let [c (finish b) s (chunk-cons c (list 8 10)) tagged (with-meta s {:tag :chunk})] [(count c) (nth c 0) (nth c 9 :missing) (reduce + c) (reduce + 1 c) (chunked-seq? s) (vec s) (count (chunk-first s)) (nth (saved-first s) 1) (vec (chunk-rest s)) (vec (saved-rest s)) (vec (chunk-next s)) (vec (saved-next s)) (meta tagged) (meta (next tagged)) (vec tagged) (count (-drop-first c)) (nth (-drop-first c) 0) (identical? (chunk-cons (array-chunk #js []) s) s) (empty? (chunk-rest (chunk-cons c nil))) (chunk-next (chunk-cons c nil)) (try (-drop-first (array-chunk #js [])) (catch js/Error e (.-message e)))]) @events])', '[0 1 2 3 3 [3 2 :missing 12 13 true [2 4 6 8 10] 3 4 [8 10] [8 10] [8 10] [8 10] {:tag :chunk} nil [2 4 6 8 10] 2 4 true true nil "-drop-first of empty chunk"] [:a :b :c]]'),
    ('chunk-public-factories', '(let [a #js [1 2 3] full (array-chunk a) tail (array-chunk a 1) middle (array-chunk a 1 2) direct (->ArrayChunk a 0 3) make ->ArrayChunk saved (make a 1 3) b (->ChunkBuffer #js [7 8 nil] 2) make-buffer ->ChunkBuffer other (make-buffer #js [9 nil] 1) appended (chunk-append other 11) c (chunk b) d (chunk other) s (->ChunkedCons c (list 10) {:from :factory} nil) make-cons ->ChunkedCons t (make-cons d nil {:from :saved} nil)] [(count full) (nth full 0) (count tail) (nth tail 0) (count middle) (nth middle 0) (nth middle 1 :missing) (count direct) (count saved) (nth saved 0) (instance? ArrayChunk direct) (instance? ChunkBuffer b) appended (count other) (vec s) (meta s) (instance? ChunkedCons s) (vec t) (meta t) (count (-chunked-first s)) (vec (-chunked-rest s)) (vec (-chunked-next s)) (-chunked-next t)])', '[3 1 2 2 1 2 :missing 3 2 2 true true 2 2 [7 8 10] {:from :factory} true [9 11] {:from :saved} 2 [10] [10] nil]'),
    ('chunk-custom-dispatch', '(let [events (atom []) c (array-chunk #js [3 5]) r (reify IChunkedSeq (-chunked-first [_] (swap! events conj :first) c) (-chunked-rest [_] (swap! events conj :rest) (list 7 9))) n (reify IChunkedSeq (-chunked-first [_] c) (-chunked-rest [_] (swap! events conj :unexpected) (list 11)) IChunkedNext (-chunked-next [_] (swap! events conj :next) (list 13))) f chunk-next] [(satisfies? IChunkedSeq r) (satisfies? IChunkedNext r) (chunked-seq? r) (count (chunk-first r)) (vec (chunk-rest r)) (vec (chunk-next r)) (vec (f r)) (satisfies? IChunkedNext n) (vec (chunk-next n)) (vec (-chunked-next n)) @events])', '[true false true 2 [7 9] [7 9] [7 9] true [13] [13] [:first :rest :rest :rest :next :next]]'),
])
LITERALS['state'].append(
    ('reference-metadata-state', '(let [events (atom []) a (atom 1 :meta {:tag :initial}) alter alter-meta! reset reset-meta! change (fn [m k v] (swap! events conj [:alter m k v]) (assoc m k v))] [(meta a) (alter-meta! a assoc :direct 2) (alter a change :saved 3) (meta a) (reset-meta! a {:tag :reset}) (reset a nil) (meta a) @a @events (let [v (volatile! 1)] [(vswap! v inc) (vswap! v + 2 3 4) @v])])', '[{:tag :initial} {:tag :initial, :direct 2} {:tag :initial, :direct 2, :saved 3} {:tag :initial, :direct 2, :saved 3} {:tag :reset} nil nil 1 [[:alter {:tag :initial, :direct 2} :saved 3]] [2 11 11]]'))

# Reviewed names used as direct calls in these exact workflows. No unbound names,
# private storage/host helpers, quoted data or textual occurrences become arities.
NAMES = set('''+ - * / = not= < <= > >= inc dec max min quot rem mod zero? pos? neg?
even? odd? number? integer? int? pos-int? neg-int? nat-int? float? double? infinite? NaN? int bit-xor bit-and bit-or bit-and-not bit-clear bit-set
bit-flip bit-test bit-not bit-shift-left bit-shift-right unsigned-bit-shift-right
apply not boolean true? false? nil? some? identical? compare
get assoc dissoc contains? find conj disj count list vector hash-map array-map hash-set
set vec nth first rest next empty empty? peek pop subvec keys vals merge merge-with
select-keys get-in assoc-in update-in update reduce reduce-kv reduced deref
atom swap! reset! compare-and-set! get-validator set-validator! volatile! vswap! vreset!
swap-vals! reset-vals! add-watch remove-watch meta with-meta realized? force delay
ex-info ex-data ex-message ex-cause instance? str pr-str
identity constantly complement comp partial fnil every-pred some-fn juxt memoize trampoline
not-empty every? not-every? some not-any? distinct? into mapv filterv zipmap frequencies
group-by map-indexed keep keep-indexed
next second ffirst nfirst fnext nnext last nthrest nthnext butlast reverse rseq peek pop empty empty? coll? set? associative? sequential? sorted? map? record? vector? seq? list? reversible? counted? indexed? reduced? unreduced ensure-reduced reduced prim-seq array-seq vec seq sorted-map range repeat replicate repeatedly iterate cycle concat list* interleave interpose partition partition-all partition-by split-at split-with drop-last take-last map mapcat filter remove take-nth take-while drop-while take reduce reduce-kv sort sort-by keys vals key val select-keys merge merge-with get-in update update-in max-key min-key symbol? symbol namespace name keyword keyword? ident? simple-ident? qualified-ident? simple-symbol? qualified-symbol? simple-keyword? qualified-keyword? keyword-identical? symbol-identical?
make-array alength aclone array aget aset into-array to-array js-invoke this-as
-count -nth -first -rest -next -seq -rseq -lookup -contains-key? -assoc -assoc-n -conj -dissoc -disjoin -empty -peek -pop -reduce -kv-reduce -key -val -compare -deref -meta -with-meta -swap! -reset! -vreset! -realized?
any? object? string? char? fn? ifn? js-fn? seqable? iterable? js-iterable? cloneable? clone ifind? reduceable?
chunk-buffer chunk-append chunk array-chunk chunk-cons chunk-first chunk-rest chunk-next chunked-seq? ->ArrayChunk ->ChunkBuffer ->ChunkedCons -drop-first -chunked-first -chunked-rest -chunked-next satisfies? alter-meta! reset-meta!
and or if-let when-let when when-not cond case for doseq dotimes some-> some->>
cond-> cond->> as-> -> ->>'''.split())
METHOD_CONTAINERS = {'reify', 'deftype', 'defrecord', 'extend-type',
                     'extend-protocol', 'specify', 'specify!'}


def executing_children(node):
    """Visit method bodies without treating protocol definitions as calls."""
    if inv.head(node).removeprefix('clojure.core/') in METHOD_CONTAINERS:
        for child in node.children:
            if (child.kind == 'list' and len(child.children) >= 2
                    and child.children[1].kind == 'vector'):
                yield from child.children[2:]
            else:
                yield child
    else:
        yield from node.children


def qualify_literal_core_calls(source):
    """Retain a whole literal workflow while executing exact compatibility aliases.

    Replace only actual unquoted call heads, never data, lexical names or
    saved-function bindings. Ordinary and macro executions check the result.
    """
    edits = []
    def walk(node):
        if node.kind in {'quote', 'syntax-quote', 'discard', 'var'} or inv.head(node) == 'quote':
            return
        if node.kind == 'list' and node.children and inv.head(node) in NAMES:
            head = node.children[0]
            edits.append((head.start, head.end, 'clojure.core/' + inv.head(node)))
        for child in executing_children(node):
            walk(child)
    for node in inv.Reader(source).all():
        walk(node)
    for start, end, replacement in sorted(edits, reverse=True):
        source = source[:start] + replacement + source[end:]
    return source

for group, literals in LITERALS.items():
    literals.extend([('qualified-' + name, qualify_literal_core_calls(source), expected)
                     for name, source, expected in list(literals)])
NAMES |= {'clojure.core/' + name for name in NAMES}

THREADS = {'->', '->>', 'some->', 'some->>', 'cond->', 'cond->>', 'as->'}
TESTS = {group: 'core_' + group + '_applications_and_macros_execute' for group in GROUPS}
FIXTURE = ROOT / 'tests/fixtures/compiler-core-gate.tsv'
TEST_FILE = 'tests/compiler_core_gate.rs'


def digest(value):
    return hashlib.sha256(value.encode() if isinstance(value, str) else value).hexdigest()


def read(name):
    return [json.loads(line) for line in (HERE / name).read_text().splitlines() if line]


def fixture_rows():
    inventory, witnesses, cases = report.load_contract('intended-language')
    bodies = {row['id']: row for row in read('full-bodies.jsonl')}
    rows = []
    for group, names in GROUPS.items():
        for name in names:
            body = bodies[name]
            ids = [identity for identity in body['assertions']
                   if inventory[identity]['disposition'] == 'in-scope']
            assert ids and all(identity in witnesses for identity in ids), name
            case_ids = list(dict.fromkeys(witnesses[identity]['fixture_id'] for identity in ids))
            selected = []
            for identity in case_ids:
                case = cases[identity]
                assert case['expected'] == {'id': identity,
                    'outcome': {'status': 'returned', 'value': True}, 'trace': [], 'stdout': ''}, identity
                assert 'source_root' not in case and 'compile_difference' not in case, identity
                # Complete adapter sources are nested without rewriting their
                # helper/setup/effect/expectation expressions or case ordering.
                assert '\n' not in case['source'] and '\t' not in case['source']
                selected.append(case)
            source = '(do ' + ' '.join('(assert (do ' + c['source'] + '))' for c in selected) + ' true)'
            provenance = {'body_id': name, 'body_sha256': body['source_span']['sha256'],
                'assertion_ids': ids,
                'cases': [{'id': c['id'], 'source_sha256': digest(c['source']),
                           'expected': c['expected']} for c in selected]}
            rows.append([group, name, source, 'true', json.dumps(provenance, separators=(',', ':'))])
        for name, source, expected in LITERALS[group]:
            rows.append([group, 'literal::' + name, source, expected,
                         json.dumps({'literal': name, 'source_sha256': digest(source)}, separators=(',', ':'))])
    return rows


def calls(source):
    """Exact direct arities; skip data, discarded forms and threaded call shapes."""
    result = {}
    def walk(node):
        if node.kind in {'quote', 'syntax-quote', 'discard', 'var'}:
            return
        head = inv.head(node)
        if head.startswith('cljs.core/'):
            head = head[len('cljs.core/'):]
        if head == 'quote':
            return
        if head in NAMES and node.kind == 'list':
            result.setdefault(head, set()).add(len(node.children) - 1)
        if head.removeprefix('clojure.core/') in THREADS:
            # Their public macro arity is exact, but nested calls receive
            # implicit arguments and cannot be assigned their raw list length.
            return
        for child in executing_children(node):
            walk(child)
    for node in inv.Reader(source).all():
        walk(node)
    return result


def execution_mappings(*, include_existing=False):
    """Return new ordinary, phase bindings and exact reviewed call receipts.

    The caller integrates these only after executing the named cohort tests.
    An ordinary assertion mapping includes EVERY in-scope site of its complete
    original body; literal-only additions bind exact local execution instead.
    """
    rows = fixture_rows()
    bodies = {r['id']: r for r in read('full-bodies.jsonl')}
    obligations = read('obligations.jsonl')
    already = set() if include_existing else {r['obligation_id'] for r in read('obligation-witnesses.jsonl')}
    phases = set() if include_existing else {r['obligation_id'] for r in read('phase-witnesses.jsonl')}
    test_hash = digest((ROOT / TEST_FILE).read_bytes())
    support = {str(FIXTURE.relative_to(ROOT)): digest(FIXTURE.read_bytes())}
    candidates = []
    for group, identity, source, expected, provenance in rows:
        original = bodies[identity]['source_span']['text'] if identity in bodies else source
        direct = calls(original)
        adapter_calls = calls(source)
        # A name/arity must remain an actual call in both original and adapter.
        direct = {name: arities & adapter_calls.get(name, set()) for name, arities in direct.items()}
        if identity == 'literal::saved-collections':
            # These reviewed bindings use saved runtime functions. They witness
            # their public runtime declarations, not source macro declarations.
            direct.update({'conj': {1, 2, 3}, 'assoc': {3, 5}, 'dissoc': {1, 2, 3}, 'get': {3}})
        candidates.append((group, identity, direct, json.loads(provenance)))
    direct_rows, phase_rows, receipts = [], [], []
    for obligation in obligations:
        if obligation['disposition'] != 'intended' or obligation['name'] not in NAMES:
            continue
        if obligation['obligation_kind'] not in {'public-api', 'local-api', 'local-metadata'}:
            continue
        if obligation['obligation_kind'] == 'public-api' and not obligation['source'].endswith(('cljs/core.cljs', 'cljs/core.cljc')):
            continue
        signature = obligation.get('signature')
        if signature and signature['kind'] not in {'fixed', 'variadic'}:
            continue
        found = None
        for group, identity, names, provenance in candidates:
            arities = names.get(obligation['name'], set())
            if signature:
                arities = {n for n in arities if n == signature['minimum']} if signature['kind'] == 'fixed' else {
                    n for n in arities if n > signature['minimum']}
            if arities:
                found = group, identity, arities, provenance
                break
        if not found:
            continue
        group, identity, arities, provenance = found
        rationale = ('The complete ' + identity + ' workflow executes ' + obligation['name']
            + ' at reviewed direct call arities ' + str(sorted(arities))
            + '. The whole original adapter suite and literal contract run independently in '
            'QuickJS and Node applications and as a compiler-executed macro, preserving '
            'all setup, branch/loop/effect paths, original source hashes and literal expectations. '
            'Quoted/data/discarded/thread-injected calls are not inferred as arity evidence.')
        if identity == 'literal::saved-collections':
            rationale += (' Reviewed let bindings c=conj, a=assoc, d=dissoc and g=get '
                          'are invoked as saved runtime functions at those exact argument counts.')
        if obligation['obligation_kind'] in {'local-api', 'local-metadata'}:
            rationale += (' This local catalog row is an advertised operation cross-check only. '
                          'Operation execution establishes no complete metadata-value coverage.')
        common = {'obligation_id': obligation['id'], 'body_sha256': obligation['body_sha256'],
                  'rationale': rationale}
        if signature:
            common['signature_sha256'] = digest(json.dumps(signature, sort_keys=True))
        execution = {'test_file': TEST_FILE, 'test_name': TESTS[group], 'test_source_sha256': test_hash}
        phase = {**common, **execution, 'support_source_sha256': support}
        if obligation['id'] not in phases:
            phase_rows.append(phase)
        if obligation['id'] not in already:
            if identity in bodies:
                direct_rows.append({**common, 'assertion_ids': provenance['assertion_ids']})
            elif obligation['id'] not in phases:
                direct_rows.append({**common, 'assertion_ids': [], 'local_execution': execution})
        receipts.append({'obligation_id': obligation['id'], 'cohort': group,
                         'source_body': identity, 'direct_arities': sorted(arities),
                         'complete_body_provenance': provenance})
    return direct_rows, phase_rows, receipts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true', help='write only the owned TSV fixture')
    parser.add_argument('--mappings', type=Path, help='save unexecuted catalog candidates and call receipts')
    args = parser.parse_args()
    rows = fixture_rows()
    expected = ('# Complete existing adapter sources; upstream excerpts are EPL-1.0.\n'
                '# cohort\tbody/literal ID\tcomplete source\tliteral pr-str expectation\tprovenance JSON\n'
                + ''.join('\t'.join(row) + '\n' for row in rows))
    if args.write:
        FIXTURE.write_text(expected)
    else:
        assert FIXTURE.read_text() == expected, 'Core gate source/body/hash/expectation drift'
    if args.mappings:
        ordinary, phase, receipts = execution_mappings()
        args.mappings.write_text(json.dumps({'ordinary': ordinary, 'phase': phase,
                                             'reviewed_calls': receipts}, indent=2))
    print(f'{len(GROUPS)} bounded cohorts; {sum(len(v) for v in GROUPS.values())} complete upstream bodies; {len(rows)} workflows reproduce')


if __name__ == '__main__':
    main()
