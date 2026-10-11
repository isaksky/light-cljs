//! Sorted membership, order, saved callbacks, protocols, and macro/runtime parity.
use light_cljs::{
    EvalSession, MacroHost, analyze_with_macros,
    emitter::emit_js_unit,
    evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE},
    parse,
};
use std::process::Command;

const CONTRACT: &str = r#"
(let [events (atom [])
      cmp (fn [a b] (swap! events conj [a b]) (compare (count a) (count b)))
      k (with-meta [1 2] {:key true}) q [3 4]
      m (with-meta (sorted-map-by cmp k :old [1] :short [1 2 3] :long) {:source true})
      updated (assoc m q :new [8 9 10 11] :last)
      s (with-meta (sorted-set-by cmp k [1] [1 2 3]) {:source true})
      su (conj s q [8 9 10 11])]
  (assert (= :old (get m q)))
  (assert (identical? m (assoc m q :old)))
  (assert (identical? m (dissoc m [])))
  (assert (identical? s (conj s q)))
  (assert (= :new (updated q)))
  (assert (= :missing (updated [] :missing)))
  (assert (identical? k (key (find updated q))))
  (assert (identical? k (get su q)))
  (assert (= [[1] k [1 2 3] [8 9 10 11]] (vec (keys updated))))
  (assert (= [[8 9 10 11] [1 2 3] k [1]] (vec (rseq su))))
  (assert (= {:source true} (meta updated) (meta su) (meta (empty updated)) (meta (clone su))))
  (assert (= :old (get m k)))
  (assert (= 3 (count s)))
  (assert (= [[1] [1 2 3] [8 9 10 11]] (vec (keys (dissoc updated q)))))
  (assert (= [[1] [1 2 3] [8 9 10 11]] (vec (disj su q))))
  (assert (not (empty? @events)))
  (assert (identical? (-comparator m) (-comparator updated)))
  (assert (identical? (-comparator m) (-comparator (empty m))))
  (assert (identical? (-comparator s) (-comparator (with-meta s {}))))
  (assert (identical? (-comparator s) (-comparator (clone s))))
  (assert (satisfies? ISorted m))
  (assert (satisfies? ISorted s))
  (assert (satisfies? IReversible s))
  (assert (not (satisfies? IEditableCollection s)))
  (assert (= [k :new] (-find updated q)))
  (assert (= 4 (count (conj m {[8 9 10 11] :last}))))
  (assert (= [[:short :new :long :last] [[1] k [1 2 3] [8 9 10 11]]]
             [(reduce (fn [xs e] (conj xs (val e))) [] updated)
              (reduce-kv (fn [xs key value] (conj xs key)) [] updated)]))
  (assert (= [:short :new] (reduce (fn [xs e] (if (= 2 (count xs)) (reduced xs) (conj xs (val e)))) [] updated)))
  (assert (= [k [1]] (vec (-sorted-seq-from s q false))))
  (assert (= [k [1 2 3]] (vec (-sorted-seq-from s q true))))
  (assert (= [[1] k [1 2 3]] (vec (-sorted-seq-from s [] true))))
  (assert (nil? (seq (-sorted-seq-from s [] false))))
  (assert (nil? (seq (-sorted-seq-from s [0 0 0 0] true))))
  (assert (= [[1] k [1 2 3]] (vec (-sorted-seq s true))))
  (assert (= [[1 2 3] k [1]] (vec (-sorted-seq s false))))
  (assert (= q (-entry-key s q)))
  (assert (= q (-entry-key m (MapEntry. q :x nil))))
  (let [m (sorted-map 3 :c 1 :a 2 :b) s (sorted-set 3 1 2)
        hm {1 :a 2 :b 3 :c} hs #{1 2 3}]
    (assert (= m hm)) (assert (= hm m))
    (assert (= s hs)) (assert (= hs s))
    (assert (= (hash m) (hash hm))) (assert (= (hash s) (hash hs)))
    (assert (identical? compare (-comparator m)))
    (assert (= [3 2 1] (vec (keys (rseq m)))))
    (assert (instance? PersistentTreeMapSeq (seq m)))
    (assert (instance? PersistentTreeMapSeq (rseq m)))
    (assert (satisfies? ICounted (seq m)))
    (assert (= () (-sorted-seq (sorted-set) true)))
    (assert (not (nil? (-sorted-seq-from (sorted-set) 1 true))))
    (assert (instance? PersistentTreeMap m)) (assert (instance? PersistentTreeSet s))
    (assert (= [1 2 3] (vec (reduce conj [] s))))
    (assert (= [] (reduce conj [] (sorted-set))))
    (assert (nil? (rseq (sorted-map)))) (assert (nil? (rseq (sorted-set)))))
  (let [ctors [sorted-map sorted-map-by sorted-set sorted-set-by]
        m (apply (nth ctors 1) > [1 :a 3 :c 2 :b])
        s (apply (nth ctors 3) > [1 3 2])
        sub subseq rsub rsubseq]
    (assert (= [3 2 1] (vec (keys m)) (vec s)))
    (assert (= -1 ((comparator >) 3 2)))
    (assert (= 1 ((comparator >) 1 2)))
    (assert (= 0 ((comparator >) 2 2)))
    (assert (= [1 2 3] (vec (rseq s))))
    (assert (= #{1 2} (apply (nth ctors 2) [2 1 2])))
    (assert (= {} (apply (nth ctors 0) [])))
    (doseq [coll [(sorted-set 1 2 3) (sorted-map 1 :a 2 :b 3 :c)]
            [test forward backward] [[< [1] [1]] [<= [1 2] [2 1]] [> [3] [3]] [>= [2 3] [3 2]]]]
      (let [entry-key (fn [e] (-entry-key coll e))]
        (assert (= forward (mapv entry-key (sub coll test 2))))
        (assert (= backward (mapv entry-key (rsub coll test 2))))
        (assert (= [2] (mapv entry-key (sub coll > 1 < 3))))
        (assert (= [3 2 1] (mapv entry-key (rsub coll >= 1 <= 3))))))
    (assert (= [3 2] (vec (sub s <= 2))))
    (assert (= [1 2] (vec (rsub s >= 2))))
    (assert (= [3 2] (vec (sub s >= 3 <= 2))))
    (assert (= [2 3] (vec (rsub s >= 3 <= 2)))))
  (let [n (sorted-map 9007199254740992 :first 9007199254740993 :last -0 :zero 0 :zero2)]
    (assert (= 2 (count n)))
    (assert (= :last (get n 9007199254740992)))
    (assert (= :zero2 (get n 0))))
  (assert (= {1 nil} (sorted-map 1) (sorted-map-by compare 1)))
  (let [m (PersistentTreeMap. compare nil 0 {:source true} 0)
        m2 (assoc m 2 :b 1 :a)
        s (PersistentTreeSet. {:source true} m2 nil)
        entries (PersistentTreeMapSeq. {:source true} (list (first m2)) true 1 nil)]
    (assert (= {1 :a 2 :b} m2))
    (assert (= [1 2] (vec s)))
    (assert (= [[1 :a]] (vec entries)))
    (assert (= {:source true} (meta m) (meta s) (meta entries)))
    (assert (= [[1 :a]] (reduce conj [] entries))))
  (assert (= {} (apply sorted-map-by [])))
  (assert (= #{} (apply sorted-set-by [])))
  (doseq [f [(fn [] (assoc (apply sorted-map-by []) 1 :a 2 :b))
            (fn [] (conj (apply sorted-set-by []) 1 2))
            (fn [] (subseq (sorted-set) <)) (fn [] (rsubseq (sorted-set) < 2 >))
            (fn [] (subseq #{} < 2)) (fn [] (rseq #{}))
            (fn [] (-sorted-seq {} true)) (fn [] (-comparator #{}))
            (fn [] (transient (sorted-set)))]]
    (assert (= :error (try (f) :unexpected (catch js/Error e :error)))))
  true)
"#;

fn both(source: &str) {
    let mut host = MacroHost::new();
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator.print(evaluator.evaluate(&p).unwrap()).unwrap(),
        "true"
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
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "true");
}
#[test]
fn sorted_contract_executes_in_application_and_macro_phases() {
    both(CONTRACT);
    both(&format!(
        "(defmacro sorted-contract [] {CONTRACT}) (sorted-contract)"
    ));
}
#[test]
fn sorted_callbacks_survive_reused_sessions_and_custom_protocol_dispatch() {
    let mut session = EvalSession::new(Default::default()).unwrap();
    for source in [
        "(def m (sorted-map-by > 1 :a 2 :b))",
        "(def s (sorted-set-by > 1 2))",
        "(def sub subseq)",
    ] {
        let v = session.eval_string(source).unwrap().value;
        session.release(v).unwrap();
    }
    let v = session
        .eval_string("[(vec (keys (assoc m 3 :c))) (vec (sub (conj s 3) >= 2))]")
        .unwrap()
        .value;
    assert_eq!(session.print(v).unwrap(), "[[3 2 1] [2 1]]");
    session.release(v).unwrap();
    both(
        r#"
      (let [events (atom [])
            sc (reify ISorted
                 (-sorted-seq [_ ascending] (swap! events conj [:seq ascending]) (if ascending [1 2 3] [3 2 1]))
                 (-sorted-seq-from [_ k ascending] (swap! events conj [:from k ascending]) (if ascending [2 3] [2 1]))
                 (-entry-key [_ e] (swap! events conj [:key e]) e)
                 (-comparator [_] (swap! events conj :comparator) compare))]
        (assert (= [2 3] (vec (subseq sc >= 2))))
        (assert (= [2 1] (vec (rsubseq sc <= 2))))
        (assert (= [[:from 2 true] :comparator [:key 2] [:from 2 false] :comparator [:key 2]] @events))
        true)"#,
    );
}

#[test]
fn sorted_membership_does_not_hash_keys() {
    both(
        r#"
      (let [events (atom [])
            k (reify IHash (-hash [_] (swap! events conj :hash) 7))
            cmp (fn [a b] 0)
            m (sorted-map-by cmp k :value)
            s (sorted-set-by cmp k)]
        (assert (= :value (get m :equivalent)))
        (assert (identical? k (get s :equivalent)))
        (assert (identical? m (assoc m :equivalent :value)))
        (assert (= 0 (count (dissoc m :equivalent)) (count (disj s :equivalent))))
        (assert (= 1 (count (-sorted-seq-from m :equivalent true))))
        (assert (empty? @events))
        (hash m) (hash s)
        (assert (= [:hash :hash] @events))
        true)"#,
    );
}
