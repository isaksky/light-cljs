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
    // The Pants build embeds no QuickJS to run this half of the check.
    #[cfg(feature = "quickjs")]
    {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert_eq!(
            evaluator.print(evaluator.evaluate(&p).unwrap()).unwrap(),
            "true"
        );
    }
    let script = format!(
        "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
        emit_js_unit(&p)
    );
    use std::io::Write as _;
    use std::process::Stdio;
    let mut child = Command::new("node")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
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
// Interactive sessions (EvalSession, --eval, --repl) run in QuickJS.
#[cfg(feature = "quickjs")]
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

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn sorted_receivers() {
        run(&[Case::expression(
            "sorted-receivers",
            include_str!("../fixtures/phase-oracles/sorted-contracts/sorted-receivers.cljs"),
        )]);
    }
}

/// light-cljs-1086: `compare` compares numbers, strings, keywords and symbols
/// without probing `IComparable` on every call, while preserving custom
/// `deftype`/`defrecord` implementations, `extend-type`, `specify!`, escaped
/// `-compare` dispatchers, forged prototypes, and edge-case `Keyword.` fields.
#[test]
fn compare_fast_paths_preserve_protocol_extensions_and_edge_cases() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    // Core ordering across numbers, strings, keywords, symbols, un-interned
    // keywords, undefined fqn, forged prototypes, and type mismatches.
    assert_eq!(
        run(r#"
        (let [err (fn [f] (try (f) :ok (catch js/Error _ :err)))
              k-undef (Keyword. nil "b" js/undefined nil)
              k-dup (keyword "a")
              forged-kw (js/Object.create (.-prototype Keyword))
              forged-sym (js/Object.create (.-prototype Symbol))]
          (pr-str
            [(compare 1 2) (compare 2 1) (compare 1 1) (compare -0 0) (compare ##NaN ##NaN)
             (compare "a" "b") (compare "b" "a") (compare "a" "a")
             (compare :a :b) (compare :b :a) (compare :a :a) (compare :a k-dup)
             (compare :a :ns/a) (compare :ns/a :a) (compare :a/z :b/a) (compare :a/a :a/b)
             (compare :a k-undef) (compare k-undef :a) (compare k-undef :c)
             (compare 'a 'b) (compare 'b 'a) (compare 'a 'a)
             (compare 'a 'ns/a) (compare 'ns/a 'a) (compare 'a/z 'b/a) (compare 'a/a 'a/b)
             (err #(compare 1 "1")) (err #(compare "1" 1))
             (err #(compare :a 'a)) (err #(compare 'a :a))
             (err #(compare :a "a")) (err #(compare :a 1))
             (err #(compare :a forged-kw)) (err #(compare forged-kw :a))
             (err #(compare 'a forged-sym)) (err #(compare forged-sym 'a))
             (vec (keys (into (sorted-map) {:g 7 :b 2 :e 5 :a 1 :f 6 :c 3 :d 4})))]))"#),
        "\"[-1 1 0 0 0 -1 1 0 -1 1 0 0 -1 1 -1 -1 -1 1 -1 -1 1 0 -1 1 -1 -1 :err :err :err :err :err :err :err :err :err :err [:a :b :c :d :e :f :g]]\"\n"
    );

    // Custom deftype / defrecord implementing IComparable, plus sort-by with
    // default and custom comparators.
    assert_eq!(
        run(r#"
        (deftype Box [v] IComparable (-compare [_ o] (compare (.-v o) v)))
        (defrecord RBox [v] IComparable (-compare [_ o] (compare v (.-v o))))
        (pr-str
          [(compare (Box. 1) (Box. 2))
           (mapv #(.-v %) (keys (sorted-map (Box. 1) :a (Box. 3) :c (Box. 2) :b)))
           (mapv :v (keys (sorted-map (->RBox 3) :c (->RBox 1) :a (->RBox 2) :b)))
           (vec (sort-by - [3 1 2]))
           (vec (sort-by :k > [{:k 1} {:k 3} {:k 2}]))])"#),
        "\"[1 [3 2 1] [1 2 3] [3 2 1] [{:k 3} {:k 2} {:k 1}]]\"\n"
    );

    // extend-type Keyword and Symbol IComparable override compare and sorted-map.
    assert_eq!(
        run(r#"
        (extend-type Keyword IComparable (-compare [a b] (compare (name b) (name a))))
        (extend-type Symbol IComparable (-compare [a b] (compare (name b) (name a))))
        (pr-str
          [(compare :a :b)
           (vec (keys (into (sorted-map) {:a 1 :b 2 :c 3})))
           (compare 'a 'b)
           (vec (keys (sorted-map 'a 1 'b 2 'c 3)))])"#),
        "\"[1 [:c :b :a] 1 [c b a]]\"\n"
    );

    // specify! on a keyword instance overrides compare for that instance.
    assert_eq!(
        run(r#"
        (let [k (specify! (keyword "a") IComparable (-compare [_ _] 42))]
          (pr-str [(compare k :z) (compare :a :z)]))"#),
        "\"[42 -1]\"\n"
    );

    // extend-type string IComparable overrides compare on strings.
    assert_eq!(
        run(r#"
        (extend-type string IComparable (-compare [a b] (compare (count a) (count b))))
        (pr-str
          [(compare "zzz" "aa")
           (vec (keys (sorted-map "bbb" 1 "a" 2 "cc" 3)))])"#),
        "\"[1 [\\\"a\\\" \\\"cc\\\" \\\"bbb\\\"]]\"\n"
    );

    // Escaped -compare dispatcher patched on "string" disables string fast path.
    assert_eq!(
        run(r#"
        (aset -compare "string" (fn [a b] (compare (count b) (count a))))
        (pr-str [(compare "a" "bb") (compare "ccc" "bb")])"#),
        "\"[1 -1]\"\n"
    );

    // extend-type default IComparable applies to strings/host objects (and via
    // recursive compareKeys on keyword/symbol names, matching None/Prepare/Advanced).
    assert_eq!(
        run(r#"
        (extend-type default IComparable (-compare [_ _] 9))
        (pr-str [(compare :a :b) (compare 'a 'b) (compare "a" "b") (compare #js {} #js {})])"#),
        "\"[9 9 9 9]\"\n"
    );

    // Mixed string-left comparisons keep the existing error path without
    // stringifying named operands (including a Keyword. whose spelling is a JS Symbol).
    assert_eq!(
        run(r#"
        (let [err-msg (fn [f] (try (f) :ok (catch :default e (.-message e))))
              sym-kw (Keyword. nil "y" js/Symbol.iterator nil)]
          (pr-str
            [(err-msg #(compare "x" :y))
             (err-msg #(compare "x" sym-kw))
             (err-msg #(compare "x" 'y))
             (err-msg #(compare "x" 1))]))"#),
        "\"[\\\"Cannot compare different types\\\" \\\"Cannot compare different types\\\" \\\"Cannot compare different types\\\" \\\"Cannot compare x to 1\\\"]\"\n"
    );

    // Re-entrant namespace comparison that installs a string IComparable
    // extension must be observed by the subsequent name comparison.
    assert_eq!(
        run(r#"
        (deftype NsBox [tag]
          IComparable
          (-compare [_ _]
            (extend-type string IComparable (-compare [_ _] 42))
            0))
        (let [k1 (Keyword. (NsBox. 1) "a" "x/a" nil)
              k2 (Keyword. (NsBox. 2) "b" "y/b" nil)]
          (pr-str [(compare k1 k2)]))"#),
        "\"[42]\"\n"
    );

    // Getter-backed -compare slot on a named value uses the first read.
    assert_eq!(
        run(r#"
        (extend-type Atom ICounted (-count [_] 1))
        (let [k (keyword "a")
              reads (atom 0)
              slot (js* "'cljs$core$IComparable$_compare$arity$2$LKEY'")]
          (js/Object.defineProperty k slot
            #js {:configurable true
                 :get (fn [] (when (= 1 (swap! reads inc)) (fn [_ _] 42)))})
          (pr-str [(compare k :b) @reads]))"#),
        "\"[42 1]\"\n"
    );
}

/// light-cljs-1086: `into` onto a sorted map and `conj` of map entries onto a
/// sorted map preserve metadata and respect `extend-type`/`specify!` overrides
/// on `MapEntry` (`IIndexed`), `PersistentTreeMap` (`IAssociative`,
/// `IEditableCollection`, `IMeta`), getter-backed slots, and constructor values
/// whose native editability depends on instance fields.
#[test]
fn sorted_map_into_and_conj_respect_protocol_overrides() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    // Metadata preservation, duplicate keys, empty sorted-map, and re-entrant comparator.
    assert_eq!(
        run(r#"
        (let [m0 (with-meta (sorted-map) {:tag :meta})
              m1 (into m0 {:c 3 :a 1 :b 2})
              m2 (into m1 [[:b 20] [:d 4]])
              side (atom (sorted-map))
              re-cmp (fn [a b] (swap! side assoc a b) (compare a b))
              m3 (into (sorted-map-by re-cmp) {:b 2 :a 1 :c 3})]
          (pr-str
            [(vec (keys m1)) (vec (vals m1)) (meta m1)
             (vec (keys m2)) (get m2 :b) (meta m2)
             (vec (keys m3)) (count @side)]))"#),
        "\"[[:a :b :c] [1 2 3] {:tag :meta} [:a :b :c :d] 20 {:tag :meta} [:a :b :c] 2]\"\n"
    );

    // specify! IMeta on a sorted map preserves mapConjSlot's outer preserveMeta call.
    assert_eq!(
        run(r#"
        (let [calls (atom 0)
              target (specify! (sorted-map)
                       IMeta
                       (-meta [_] {:calls (swap! calls inc)}))
              res (into target {:a 1})]
          (pr-str [@calls (meta res) (get res :a)]))"#),
        "\"[2 {:calls 2} 1]\"\n"
    );

    // Getter-backed -nth on a real MapEntry and -assoc on a sorted map are each
    // read once in dispatch order (-nth 0, -nth 1, then -assoc).
    assert_eq!(
        run(r#"
        (extend-type Atom ICounted (-count [_] 1))
        (let [order (atom [])
              nth-slot (js* "'cljs$core$IIndexed$_nth$arity$2$LKEY'")
              assoc-slot (js* "'cljs$core$IAssociative$_assoc$arity$3$LKEY'")
              orig-nth (aget (.-prototype MapEntry) nth-slot)
              orig-assoc (aget (.-prototype PersistentTreeMap) assoc-slot)
              entry (first {:a 1})
              nth-reads (atom 0)
              m (sorted-map)
              assoc-reads (atom 0)]
          (js/Object.defineProperty entry nth-slot
            #js {:configurable true
                 :get (fn []
                        (let [n (swap! nth-reads inc)]
                          (swap! order conj [:nth n])
                          (if (= n 1) (fn [_ _] :rewritten) orig-nth)))})
          (js/Object.defineProperty m assoc-slot
            #js {:configurable true
                 :get (fn []
                        (let [n (swap! assoc-reads inc)]
                          (swap! order conj [:assoc n])
                          orig-assoc))})
          (let [res (conj m entry)]
            (pr-str [(vec (keys res)) (get res :rewritten) @nth-reads @assoc-reads @order])))"#),
        "\"[[:rewritten] 1 2 1 [[:nth 1] [:nth 2] [:assoc 1]]]\"\n"
    );

    // Constructor values whose native editability depends on instance fields
    // (source-less Subvec and comparator-less PersistentTreeMap) stay editable.
    assert_eq!(
        run(r#"
        (let [s (Subvec. nil js/undefined 0 0 nil)
              r (into s [])]
          (pr-str [(identical? s r) (instance? PersistentVector r) (vec r)]))"#),
        "\"[false true []]\"\n"
    );

    // extend-type MapEntry IIndexed is respected by (into (sorted-map) m).
    assert_eq!(
        run(r#"
        (let [orig (.-cljs$core$IIndexed$_nth$arity$2 (.-prototype MapEntry))
              seen (atom 0)]
          (extend-type MapEntry
            IIndexed
            (-nth ([e i] (swap! seen inc) (if (zero? i) (.call orig e e 0) (+ 100 (.call orig e e 1))))
                  ([e i nf] (if (or (= i 0) (= i 1)) (-nth e i) nf))))
          (let [m (into (sorted-map) {:a 1 :b 2})]
            (pr-str [@seen (get m :a) (get m :b)])))"#),
        "\"[4 101 102]\"\n"
    );

    // extend-type PersistentTreeMap IAssociative is respected by (into (sorted-map) m).
    assert_eq!(
        run(r#"
        (let [orig (.-cljs$core$IAssociative$_assoc$arity$3 (.-prototype PersistentTreeMap))
              calls (atom 0)]
          (extend-type PersistentTreeMap
            IAssociative
            (-assoc [m k v] (swap! calls inc) (.call orig m m k (+ v 10))))
          (let [m (into (sorted-map) {:a 1 :b 2})]
            (pr-str [@calls (get m :a) (get m :b)])))"#),
        "\"[2 11 12]\"\n"
    );

    // specify! on a sorted-map instance's IAssociative is respected by conj.
    assert_eq!(
        run(r#"
        (let [calls (atom 0)
              m (specify! (sorted-map)
                  IAssociative
                  (-assoc [self k v] (swap! calls inc) [:custom k v]))
              res (conj m (first {:a 1}))]
          (pr-str [@calls res]))"#),
        "\"[1 [:custom :a 1]]\"\n"
    );

    // extend-type PersistentTreeMap IEditableCollection is respected by into.
    assert_eq!(
        run(r#"
        (let [used (atom false)]
          (extend-type PersistentTreeMap
            IEditableCollection
            (-as-transient [m]
              (reset! used true)
              (let [state (atom m)]
                (reify
                  ITransientCollection
                  (-conj! [this entry] (swap! state conj entry) this)
                  (-persistent! [_] @state)))))
          (let [m (into (sorted-map) {:b 2 :a 1})]
            (pr-str [@used (vec (keys m))])))"#),
        "\"[true [:a :b]]\"\n"
    );
}

/// light-cljs-1086: in `:advanced` output with protocol machinery retained,
/// ordinary number, string, keyword, symbol, and sorted-map comparisons skip
/// `protocolHooks.builtin` `IComparable` lookups, while custom `IComparable`
/// receivers still dispatch through `builtin`.
#[test]
fn compare_of_builtin_keys_skips_protocol_lookup() {
    use light_cljs::{ApplicationOptions, OptimizationLevel, analyze, prepare_application};

    let source = r#"
    (extend-type Atom ICounted (-count [_] 1))
    (deftype CustomCmp [v] IComparable (-compare [_ o] (compare v (.-v o))))
    (let [probes #(aget js/globalThis "__cmp_probes")
          measure (fn [f] (let [before (probes) r (f)] [(- (probes) before) r]))]
      (pr-str
        [(measure #(compare 1 2))
         (measure #(compare "alpha" "beta"))
         (measure #(compare :a/x :a/y))
         (measure #(compare :a :b))
         (measure #(compare 'a/x 'b/y))
         (measure #(count (into (sorted-map) {:g 7 :b 2 :e 5 :a 1 :f 6 :c 3 :d 4})))
         (measure #(compare (CustomCmp. 1) (CustomCmp. 2)))]))"#;

    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let parsed = parse(source).expect("parse program");
    let program = analyze(source, &parsed).expect("analyze program");
    let mut js = prepare_application(&program, options).emit_js();
    let head = "function builtin(value, key, arity) {";
    assert!(
        js.contains(head),
        "expected retained builtin in pseudo-names output"
    );
    js = js.replace(
        head,
        &format!("{head} if (key && key.name === \"IComparable\") globalThis.__cmp_probes++;"),
    );
    let output = Command::new("node")
        .args([
            "-e",
            &format!("globalThis.__cmp_probes = 0; console.log({js});"),
        ])
        .output()
        .expect("run node");
    assert!(
        output.status.success(),
        "node failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "[[0 -1] [0 -1] [0 -1] [0 -1] [0 -1] [0 7] [1 -1]]\n"
    );
}
