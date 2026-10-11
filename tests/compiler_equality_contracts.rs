//! Cross-type equality, representative retention, public hashes and metadata in both phases.
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit,
    evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE}, parse};
use std::process::Command;

fn both(source: &str) {
    let mut host = MacroHost::new();
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(evaluator.print(evaluator.evaluate(&p).unwrap()).unwrap(), "true");
    let script = format!("const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));", emit_js_unit(&p));
    let out = Command::new("node").args(["-e", &script]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "true");
}

const CONTRACT: &str = r#"
(let [families [[[] () (seq [])]
                [[1 2] '(1 2) (seq [1 2]) (map identity [1 2]) (range 1 3)]
                [{:a 1 :b 2} (hash-map :b 2 :a 1) (sorted-map :a 1 :b 2)]
                [#{1 2} (sorted-set 2 1) (hash-set 2 1)]
                [(symbol "ns" "a") 'ns/a (with-meta 'ns/a {:x 1})]
                [(keyword "ns" "a") :ns/a]
                [(js/Date. 12) (js/Date. 12)]
                [#uuid "00000000-0000-0000-0000-000000000001"
                 (UUID. "00000000-0000-0000-0000-000000000001" nil)]]]
  ;; nil is not an empty sequential collection.
  (assert (not= nil []))
  (doseq [family (rest families), a family, b family]
    (assert (= a b)) (assert (= (hash a) (hash b)))
    (assert (= :found (get {a :found} b)))
    (assert (contains? #{a} b)))
  (doseq [v [[] () [1 2] '(1 2) (seq [1 2]) (map identity [1 2])
             {:a 1} (sorted-map :a 1) #{1 2} (sorted-set 1 2) 'ns/a]]
    (let [m (with-meta v {:origin :meta})]
      (assert (= v m)) (assert (= (hash v) (hash m)))
      (assert (= :v (get {v :v} m)))
      (assert (= {:origin :meta} (meta m) (-meta m)))
      (assert (= v (-with-meta m {})))
      (assert (satisfies? IMeta m)) (assert (satisfies? IWithMeta m))
      (assert (satisfies? IHash m)) (assert (satisfies? IEquiv m))
      (assert (= (hash m) (-hash m))) (assert (-equiv v m))))
  (let [k (with-meta [1 2] {:saved true}) q '(1 2)
        original (hash-map k :old "Aa" :a "BB" :b)
        changed (assoc original q :new)
        s (hash-set k "Aa" "BB")
        t (transient original) ts (transient s)]
    (assert (= (hash "Aa") (hash "BB")))
    (assert (= [:a :b] [(get changed "Aa") (get changed "BB")]))
    (assert (identical? k (key (find changed q))))
    (assert (identical? k (get s q)))
    (assert (= :old (get original q)))
    (assoc! t q :transient) (conj! ts q)
    (assert (= :transient (get (persistent! t) k)))
    (assert (identical? k (get (persistent! ts) q)))
    (assert (= :old (get original k))))
  (let [h hash ho hash-ordered-coll hu hash-unordered-coll]
    (assert (= [0 0 1 -1 0 16451 57407 2146435072 -1048576 2146959360
                74834163 -2123407586 -482876059 -2017569654 -15128758
                1185709346 -1751407317 -2144769406]
               (mapv h [0 -0 1 -1 2147483647 9007199254740992 0.5 ##Inf ##-Inf ##NaN
                         "abc" :a 'a [] {} [1 2] #{1 2} {:a 1}])))
    (assert (= (h [1 2]) (ho [1 2]) (apply ho ['(1 2)])))
    (assert (= (h {:a 1}) (hu {:a 1})))
    (assert (= (h #{1 2}) (apply hu [#{2 1}]))))
  (assert (not= (UUID. "00000000-0000-0000-0000-00000000000A" nil)
                (uuid "00000000-0000-0000-0000-00000000000A")))
  (assert (= 0 -0)) (assert (= (hash 0) (hash -0)))
  (assert (not= ##NaN ##NaN))
  (assert (= :same (get (hash-map 9007199254740992 :same) 9007199254740993)))
  (let [n (hash-map ##NaN :nan)] (assert (= :missing (get n ##NaN :missing))))
  (doseq [v [:ns/a 'ns/a]]
    (assert (satisfies? INamed v))
    (assert (= "a" (name v) (-name v)))
    (assert (= "ns" (namespace v) (-namespace v))))
  (assert (= "a" (name "a")))
  (assert (nil? (namespace :a)))
  (assert (= :error (try (-name "a") (catch js/Error e :error))))
  (let [effects (atom []) xs (map (fn [x] (swap! effects conj x) x) [1 2 3])]
    (assert (empty? @effects))
    (assert (= (hash [1 2 3]) (hash xs)))
    (assert (= [1 2 3] @effects))
    (hash xs) (= xs [1 2 3])
    (assert (= [1 2 3] @effects)))
  (let [effects (atom 0) xs (lazy-seq (swap! effects inc) (throw (js/Error. "hash-realization")))]
    (assert (= "hash-realization" (try (hash xs) (catch js/Error e (.-message e)))))
    (assert (= 1 @effects)))
  (let [a (js-obj) b (js-obj) f (fn [x] x) r (js/RegExp. "x")]
    (aset a "self" a) (aset b "self" b)
    (assert (= a a)) (assert (not= a b))
    (assert (= :host (get {a :host} a)))
    (assert (= (hash a) (hash a)))
    (assert (= f f)) (assert (not= f (fn [x] x)))
    (assert (= 3 ((with-meta f {:callable true}) 3)))
    (assert (= r r)) (assert (not= r (js/RegExp. "x")))))
true
"#;

#[test]
fn canonical_values_hash_metadata_and_effects_in_both_phases() {
    both(CONTRACT);
    both(&format!("(defmacro equality-contract [] {CONTRACT}) (equality-contract)"));
}

#[test]
fn custom_protocols_records_and_vars_in_both_phases() {
    let source = r#"
(deftype NamedKey [id metadata]
  INamed (-name [_] "key") (-namespace [_] "custom")
  IEquiv (-equiv [_ other] (and (instance? NamedKey other) (= id (.-id other))))
  IHash (-hash [_] 42)
  IMeta (-meta [_] metadata)
  IWithMeta (-with-meta [_ m] (NamedKey. id m)))
(defrecord FirstRecord [x])
(defrecord SecondRecord [x])
(def value 12)
(defn check []
  (let [a (NamedKey. 1 nil) b (NamedKey. 1 {:new true}) c (NamedKey. 2 nil)
        m {a :first c :collision} r (FirstRecord. 1)]
    (assert (= a b)) (assert (= (hash a) (hash b)))
    (assert (= :first (get m b))) (assert (= :collision (get m c)))
    (assert (identical? a (key (find (assoc m b :updated) b))))
    (assert (= "key" (name a) (-name a)))
    (assert (= "custom" (namespace a) (-namespace a)))
    (assert (satisfies? INamed a))
    (assert (= {:new true} (meta (with-meta a {:new true}))))
    (assert (= r (FirstRecord. 1) (with-meta r {:record true})))
    (assert (not= r (SecondRecord. 1))) (assert (not= r {:x 1})) (assert (not= {:x 1} r))
    (assert (= :record (get {r :record} (with-meta r {}))))
    (assert (= {} (dissoc r :x))) (assert (not (record? (dissoc r :x))))
    (assert (= r (dissoc (assoc r :extra 2) :extra)))
    (assert (= (hash r) (hash (with-meta r {}))))
    (let [v #'value copy (with-meta v {:var true})]
      (assert (= v copy)) (assert (= (hash v) (hash copy)))
      (assert (= 12 @copy)) (assert (= :var (get {v :var} copy))))
    true))
"#;
    both(&format!("{source} (check)"));
    both(&format!("{source} (defmacro check-macro [] (check)) (check-macro)"));
}
