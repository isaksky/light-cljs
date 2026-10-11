//! Public sequence conversion, traversal and deferred contracts in both phases.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::process::Command;

fn both(source: &str, expected: &str) {
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
        .unwrap_or_else(|e| panic!("{source}: {e}"));
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        e.print(e.evaluate(&p).unwrap()).unwrap(),
        expected,
        "QuickJS: {source}"
    );
    let script = format!(
        "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
        emit_js_unit(&p)
    );
    let out = Command::new("node").args(["-e", &script]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        expected,
        "Node: {source}"
    );
}

#[test]
fn public_sequence_conversion_chunk_and_traversal_run_in_both_phases() {
    let source = r#"
      (defn witness []
        (let [events (atom [])
              children (reify IFn (-invoke [_ x] (swap! events conj x) (seq x)))
              tree (tree-seq sequential? children [1 [2 nil] [] (lazy-seq (list 3))])
              b (chunk-buffer 3)]
          (assert (not (realized? tree)))
          (assert (empty? @events))
          (assert (= [[1 [2 nil] [] '(3)] 1 [2 nil] 2 nil [] '(3) 3] (vec tree)))
          (assert (= [1 2 nil 3] (vec (flatten [1 [2 nil] [] (lazy-seq (list 3))]))))
          (assert (empty? (flatten nil)))
          (assert (empty? (flatten 42)))
          (assert (= [{:a 1} #{2} "ab"] (vec (flatten [{:a 1} #{2} "ab"]))))
          (assert (= [0 1 2 3] (vec (take 4 (tree-seq number? #(list (inc %)) 0)))))
          (assert (= 1 (chunk-append b :a)))
          (chunk-append b :b)
          (let [c (chunk b) s (chunk-cons c (list :c))]
            (assert (satisfies? IChunk c))
            (assert (satisfies? IChunkedSeq s))
            (assert (satisfies? IChunkedNext s))
            (assert (chunked-seq? s))
            (assert (= [:a :b :c] (vec s)))
            (assert (= [:a :b] (vec (map #(nth (chunk-first s) %) (range 2)))))
            (assert (= '(:c) (chunk-rest s) (chunk-next s)))
            (assert (= :b (nth (-drop-first c) 0)))
            (assert (= 2 (count (array-chunk #js [0 1]))
                               (count (array-chunk #js [0 1 2] 1))
                               (count (array-chunk #js [0 1 2 3] 1 3))))
            (assert (= :tail (first (chunk-cons (array-chunk #js []) (list :tail))))))
          (let [source (reify ISeqable (-seq [_] (list 7 8)))
                i (iter source) e (es6-iterator source)]
            (assert (satisfies? ISeqable source))
            (assert (satisfies? ISeq (seq source)))
            (assert (.hasNext i)) (assert (.hasNext i))
            (assert (= 7 (.next i))) (assert (= 8 (.next i)))
            (assert (not (.hasNext i)))
            (assert (= 7 (.-value (.next e))))
            (assert (= 8 (.-value (.next e))))
            (assert (true? (.-done (.next e)))))
          (let [n (iter nil) a (iter #js [1 nil false]) s (iter "ab")]
            (assert (not (.hasNext n))) (assert (instance? js/Error (.next n)))
            (assert (= [1 nil false] [(.next a) (.next a) (.next a)]))
            (assert (not (.hasNext a)))
            (assert (= ["a" "b" ""] [(.next s) (.next s) (.next s)]))
            (assert (instance? js/Error (.remove s)))
            (assert (true? (.-done (.next (es6-iterator nil)))))
            (assert (= [1 nil false] (vec (es6-iterator-seq (es6-iterator #js [1 nil false]))))))
          (let [x (reify IIterable (-iterator [_] (iter [9])))]
            (assert (= 9 (.next (iter x)))))
          true))
      (defmacro phase-witness [] (witness))
      [(witness) (phase-witness)]"#;
    both(source, "[true true]");
}

#[test]
fn iterator_demand_receivers_and_lazy_tail_retry_are_observable() {
    both(
        r#"
      (let [calls (atom []) n (atom 0)
            xs (lazy-seq (swap! calls conj :head)
                 (cons 1 (lazy-seq (swap! calls conj :tail) (list 2))))
            i (es6-iterator xs) before @calls a (.next i) after @calls
            js-iter (js-obj "next" (fn [] (swap! n inc) (js-obj "value" @n "done" (> @n 2))))
            s (es6-iterator-seq js-iter)]
        [before (.-value a) after (vec s) (vec s) @n
         (rest (rest s)) @n])"#,
        "[[:head] 1 [:head :tail] [1 2] [1 2] 4 nil 5]",
    );
    both(
        r#"
      (let [calls (atom []) iter (js* "({index:0, next(){ this.index++; return {value:this.index,done:this.index>2}; }})")
            s (seq (js* "({[Symbol.iterator](){ return ~{}; }})" iter))]
        [(first s) (first (rest s)) (first (rest s)) (.-index iter)])"#,
        "[1 2 2 2]",
    );
    both(
        r#"
      (let [n (atom 0) it (js-obj "next" (fn [] (case (swap! n inc)
                          1 (js-obj "value" 1 "done" false)
                          2 (throw :retry)
                          (js-obj "value" 2 "done" false))))
            s (es6-iterator-seq it)]
        [(first s) (try (rest s) (catch :default e e)) (first (rest s)) @n])"#,
        "[1 :retry 2 3]",
    );
}

#[test]
fn deferred_retry_self_realization_apply_and_reduction_preserve_effects() {
    both(
        r#"
      (let [n (atom 0) holder (atom nil)
            d (delay (if (= 1 (swap! n inc)) @(deref holder) 42))]
        (reset! holder d)
        [@d @d @n (realized? d)])"#,
        "[42 42 2 true]",
    );
    both(
        r#"
      (let [n (atom 0) holder (atom nil)
            s (lazy-seq (if (= 1 (swap! n inc)) (first @holder) (list 42)))]
        (reset! holder s)
        [(try (first s) (catch :default e :nonseq)) @n])"#,
        "[:nonseq 2]",
    );
    both(
        r#"
      (let [n (atom 0) s (lazy-seq (if (= 1 (swap! n inc)) (throw :retry) (list 42)))
            before [(try (first s) (catch :default e e)) (realized? s)]]
        [before (first s) (first s) @n (realized? s)])"#,
        "[[:retry false] 42 42 2 true]",
    );
    both(
        r#"
      (let [n (atom 0) f (fn f [i] (lazy-seq (swap! n inc) (cons i (f (inc i)))))
            s (f 0) choose (fn [& args] (first args))]
        [(apply choose s) @n (reduce (fn [a x] (if (= x 2) (reduced (+ a x)) (+ a x))) 0 s) @n])"#,
        "[0 2 3 3]",
    );
}

#[test]
fn public_chunk_constructors_retain_backing_storage_and_custom_protocol_demand() {
    both(
        r#"
      (defn chunk-witness []
        (let [a #js [1 2 3] c (ArrayChunk. a 1 2) b (->ChunkBuffer a 1)
              s (ChunkedCons. c (list 4) {:a 1} nil)]
          (assert (instance? ArrayChunk c)) (assert (instance? ChunkBuffer b))
          (assert (instance? ChunkedCons s))
          (assert (not (satisfies? IReduce s))) (assert (not (iterable? s)))
          (assert (= 9 (nth (->ArrayChunk #js [9] 0 1) 0)))
          (assert (= '(5) (->ChunkedCons (array-chunk #js [5]) nil nil nil)))
          (assert (= 0 (count (ChunkBuffer. #js [] 0))))
          (assert (identical? a (.-arr c))) (assert (= 1 (.-off c)))
          (assert (= 2 (.-end c))) (assert (identical? c (chunk-first s)))
          (aset a 1 9)
          (assert (= 9 (first s))) (assert (= '(9 4) s))
          (assert (= {:a 1} (meta s)))
          (assert (.equiv s '(9 4)))
          (assert (= (hash '(9 4)) (hash s) (.-__hash s)))
          (assert (= 123 (hash (ChunkedCons. c nil nil 123))))
          (assert (= {:b 2} (meta (with-meta s {:b 2}))))
          (assert (= 12 (reduce + c)))
          (assert (= 22 (reduce + 10 c)))
          (assert (= 2 (.add b 8))) (assert (= 2 (count b)))
          (assert (= 8 (nth (.chunk b) 1))) (assert (nil? (.-buf b)))
          true))
      (defmacro chunk-phase [] (chunk-witness))
      [(chunk-witness) (chunk-phase)]"#,
        "[true true]",
    );
    both(
        r#"
      (let [events (atom [])
            c (reify ICounted (-count [_] (swap! events conj :count) 1)
                     IIndexed (-nth [_ i] (swap! events conj :nth) 7)
                     IChunk (-drop-first [_] (throw :unused)))
            s (chunk-cons c nil) before @events]
        [before (identical? c (chunk-first s)) (first s) @events (chunk-next s)])"#,
        "[[:count] true 7 [:count :nth] nil]",
    );
}

#[test]
fn shared_lazy_roots_are_phase_separated_and_failed_compilations_reset() {
    both(
        r#"
      (def effects (atom 0))
      (def s (lazy-seq (swap! effects inc) (list 7)))
      (defn consume [] [(first s) (first s) @effects])
      (defmacro macro-consume [] (consume))
      [(macro-consume) @effects (consume) @effects]"#,
        "[[7 7 1] 0 [7 7 1] 1]",
    );
    let mut host = MacroHost::new();
    let failing = "(def xs (lazy-seq (throw (js/Error. \"sequence-failed\")))) (defmacro fail [] (first xs)) (fail)";
    let err = light_cljs::compile_with_macros(failing, &mut host)
        .unwrap_err()
        .to_string();
    assert!(err.contains("sequence-failed"), "{err}");
    let source = "(def xs (lazy-seq (list 42))) (defmacro pass [] (first xs)) (pass)";
    assert!(light_cljs::compile_with_macros(source, &mut host).is_ok());
    host.evaluator_options.timeout = std::time::Duration::from_millis(100);
    let err = light_cljs::compile_with_macros(
        "(defmacro forever [] (last (tree-seq number? #(list (inc %)) 0))) (forever)",
        &mut host,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("deadline exceeded"), "{err}");
    host.evaluator_options.timeout = std::time::Duration::from_secs(5);
    assert!(light_cljs::compile_with_macros(source, &mut host).is_ok());
}

#[test]
fn native_es6_iteration_has_cljs_construction_and_advance_demand() {
    both(
        r#"
      (let [calls (atom 0) tail (lazy-seq (swap! calls inc) (list 2))
            s (chunk-cons (array-chunk #js [1]) tail) r (chunk-next s)]
        [(identical? tail r) @calls (first r) @calls])"#,
        "[true 0 2 1]",
    );
    both(
        r#"
      (let [events (atom []) s (lazy-seq (swap! events conj :head)
                       (cons 1 (lazy-seq (swap! events conj :tail) (list 2))))
            i (js* "~{}[Symbol.iterator]()" s) before @events v (.next i)]
        [before (.-value v) @events (.-value (.next i)) (.-value (.next i))])"#,
        "[[:head] 1 [:head :tail] 2 nil]",
    );
    both(
        r#"
      (let [collections [[1 2] '(1 2) (range 2) (seq (hash-map :a 1))
                         cljs.core.PersistentQueue.EMPTY (into cljs.core.PersistentQueue.EMPTY [1 2])
                         #{1} {:a 1} (eduction (map inc) [1 2])]]
        (mapv #(vec (seq (js* "Array.from(~{})" %))) collections))"#,
        "[[1 2] [1 2] [0 1] [[:a 1]] [] [1 2] [1] [[:a 1]] [2 3]]",
    );
}
