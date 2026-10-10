use light_cljs::interpreter::*;

fn yes(c: &mut Context, source: &str) {
    let value = c
        .eval_string(source)
        .unwrap_or_else(|e| panic!("{source}: {e}"));
    assert_eq!(value.inspect(), ValueView::Boolean(true), "{source}");
}

#[test]
fn constructors_fill_basis_fields_keep_extras_and_discard_input_metadata() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defrecord Box [items]) (= [[1 2 3] [4 5] true] [(:items (->Box [1 2 3])) (:items (map->Box {:items [4 5]})) (record? (->Box []))])",
    );
    yes(
        &mut c,
        "(defrecord R [x y]) (= [{:x 1 :y nil :z 3} {:x nil :y nil} {:x nil :y nil \"x\" 3} nil {:x 4 :y 5}] [(into {} (map->R {:x 1 :z 3})) (into {} (map->R nil)) (into {} (map->R {\"x\" 3})) (meta (map->R ^{:a 2} {:x 1})) (into {} (map->R (->R 4 5)))])",
    );
    yes(
        &mut c,
        "(defrecord Empty []) (= [true 0 true {} nil false] [(record? (->Empty)) (count (->Empty)) (empty? (->Empty)) (into {} (->Empty)) (seq (->Empty)) (ifn? (->Empty))])",
    );
    yes(
        &mut c,
        "(defrecord Named [Named]) (= 7 (:Named (->Named 7)))",
    );
    for source in [
        "(->R)",
        "(->R 1)",
        "(->R 1 2 3)",
        "(map->R [])",
        "(map->R 1)",
        "(map->R '([:x 1]))",
        "(map->R)",
        "(map->R {} {})",
    ] {
        assert!(c.eval_string(source).is_err(), "{source}");
    }
    assert_eq!(
        c.eval_string("R").unwrap().inspect(),
        ValueView::RecordType(Name::new(Some("user"), "R"))
    );
    assert_eq!(
        c.eval_string("(->R 1 2)").unwrap().inspect(),
        ValueView::Record(2)
    );
}

#[test]
fn nominal_equality_and_hashing_include_all_fields_and_ignore_metadata() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defrecord A [x]) (defrecord B [x]) (let [a (->A 1)] (= [true false false false :ok 3] [(= a (map->A {:x 1})) (= a (->B 1)) (= a {:x 1}) (= {:x 1} a) (get {a :ok} (->A 1)) (count (set [a (->A 1) (->B 1) {:x 1}]))]))",
    );
    let a = c
        .eval_string("(with-meta (assoc (->A [1 1.0 1/2]) :z 2 :y 3) {:a 1})")
        .unwrap();
    let b = c
        .eval_string("(with-meta (map->A {:y 3 :x '(1 1.0 1/2) :z 2}) {:a 2})")
        .unwrap();
    assert!(a.try_value_eq(&b).unwrap());
    assert_eq!(a.value_hash(), b.value_hash());
    yes(
        &mut c,
        "(let [a (->A 1) b (->A 1.0)] (= [false :int :double 2] [(= a b) (get {a :int b :double} (->A 1)) (get {a :int b :double} (->A 1.0)) (count #{a b})]))",
    );
    yes(
        &mut c,
        "(= [false false] [(= (assoc (->A 1) :y 2) (->A 1)) (= (assoc (->A 1) :y 2) (assoc (->A 1) :y 3))])",
    );
    yes(
        &mut c,
        "(let [r (->A ##NaN) a (with-meta r {:m 1}) b (with-meta r {:m 2})] (= [true true :ok] [(= r a) (= a b) (get {a :ok} b)]))",
    );
    let map = c.map(&[(a, c.integer(42))]).unwrap();
    assert_eq!(
        map.get(&b).unwrap().unwrap().inspect(),
        ValueView::Integer(42)
    );
}

#[test]
fn persistent_updates_and_basis_dissoc_preserve_snapshots_and_metadata() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defrecord R [x]) (let [r (with-meta (->R 1) {:m 2}) a (assoc r :x 2 :y 3) b (dissoc a :y) d (dissoc a :x)] (= [true {:x 2 :y 3} true {:x 2} false {:y 3} 1 false {:m 2} {:m 2} {:m 2} true] [(record? a) (into {} a) (record? b) (into {} b) (record? d) d (:x r) (record? (assoc d :x 9)) (meta a) (meta b) (meta d) (record? (dissoc r :missing))]))",
    );
    yes(
        &mut c,
        "(= [false {}] [(record? (dissoc (->R 1) :x)) (dissoc (->R 1) :x)])",
    );
    yes(
        &mut c,
        "(let [r (->R 1)] (= [true true true true true true false false 1 nil :none true false [:x 1] nil #{:x :y} #{1 2}] [(map? r) (coll? r) (associative? r) (counted? r) (seqable? r) (record? r) (ifn? r) (fn? r) (get r :x) (:z r) (:z r :none) (contains? r :x) (contains? r :z) (find r :x) (find r :z) (set (keys (assoc r :y 2))) (set (vals (assoc r :y 2)))]))",
    );
    for source in ["((->R 1) :x)", "(apply (->R 1) [:x])", "(empty (->R 1))"] {
        assert!(c.eval_string(source).is_err(), "{source}");
    }
    assert!(matches!(
        c.eval_string("(empty (->R 1))").unwrap_err().kind,
        EvalErrorKind::Native {
            error: NativeError::Collection(CollectionError::UnsupportedRecordEmpty),
            ..
        }
    ));
}

#[test]
fn sequences_reduction_nested_updates_and_map_composition_preserve_records() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defrecord R [x y]) (let [r (->R 1 2)] (= [3 3 {:x 1 :y 2} true {:x 4 :y 2 :z 9} true {:x 4 :y 2} {:x 1} 1 {:x {:nested 3} :y 2} {:x 3 :y 2}] [(reduce (fn [a e] (+ a (second e))) 0 r) (reduce-kv (fn [a k v] (+ a v)) 0 r) (into {} (seq r)) (every? map-entry? (seq r)) (into {} (conj r [:x 4] {:z 9})) (record? (into r [[:x 4]])) (into {} (merge r {:x 4})) (select-keys r [:x]) (get-in r [:x]) (into {} (assoc-in (assoc r :x {}) [:x :nested] 3)) (into {} (update r :x + 2))]))",
    );
    yes(
        &mut c,
        "(let [r (->R {:n 1} nil)] (= [true 4] [(record? (update-in r [:x :n] + 3)) (get-in (update-in r [:x :n] + 3) [:x :n])]))",
    );
    yes(
        &mut c,
        "(= [true {:x 3 :y 4}] [(record? (into (->R 1 2) (map identity) [[:x 3] [:y 4]])) (into {} (into (->R 1 2) (map identity) [[:x 3] [:y 4]]))])",
    );
}

#[test]
fn walk_retains_nominal_identity_and_key_conversion_keeps_plain_maps() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(require '[clojure.walk :as walk]) (defrecord R [x]) (let [r (with-meta (assoc (->R [1 2]) :z 3) {:m 7}) same (walk/postwalk identity r) changed (walk/postwalk (fn [x] (if (integer? x) (inc x) x)) r)] (= [true true {:m 7} true {:x [2 3] :z 4} false {\"x\" [1 2] \"z\" 3}] [(record? same) (= r same) (meta same) (record? changed) (into {} changed) (record? (walk/stringify-keys r)) (walk/stringify-keys r)]))",
    );
    yes(
        &mut c,
        "(let [r (map->R {\"z\" 4}) v (walk/keywordize-keys r)] (= [false {:x nil :z 4}] [(record? v) v]))",
    );
    yes(
        &mut c,
        "(let [r (->R 1)] (= [true true] [(= r (walk/prewalk identity r)) (= r (walk/walk identity identity r))]))",
    );
}

#[test]
fn namespace_constructors_and_saved_functions_resolve_current_generations() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(ns a) (defrecord Box [x]) (ns b) (defrecord Box [x]) (ns user (:require [a :as aa] [b :as bb])) (= [2 3 false] [(:x (aa/->Box 2)) (:x (bb/map->Box {:x 3})) (= (aa/->Box 1) (bb/->Box 1))])",
    );
    yes(
        &mut c,
        "(defrecord R [x]) (def old (->R 1)) (def saved ->R) (def saved-map map->R) (defrecord R [x]) (= [false true true] [(= old (->R 1)) (= (saved 1) (->R 1)) (= (saved-map {:x 1}) (->R 1))])",
    );
    yes(
        &mut c,
        "(defrecord R [x y]) (= [{:x 1} {:x 3 :y 4} {:x 2 :y nil}] [(into {} old) (into {} (->R 3 4)) (into {} (saved-map {:x 2}))])",
    );
    assert!(c.eval_string("(saved 3)").is_err());
    assert!(c.eval_string("(saved 3 4)").is_err());
    yes(&mut c, "(= '[x y] (first (:arglists (meta (var ->R)))))");
    yes(&mut c, "(= '[m] (first (:arglists (meta (var map->R)))))");
    yes(&mut c, "(= 'user (ns-name (:ns (meta (var ->R)))))");
}

#[test]
fn declaration_validation_reports_locations_and_rejects_unknown_protocols() {
    let mut c = Context::new();
    for source in [
        "(defrecord 1 [])",
        "(defrecord a/R [])",
        "(defrecord R ())",
        "(defrecord R [a a])",
        "(defrecord R [a/x])",
        "(defrecord R [1])",
        "(defrecord R [& x])",
        "(defrecord R [^:volatile-mutable x])",
        "(defrecord R [^:unsynchronized-mutable x])",
        "(defrecord R [x] P (p [_] x))",
    ] {
        let error = c.eval_string(source).unwrap_err();
        assert_eq!(error.phase, EvalPhase::Macroexpansion, "{source}: {error}");
        assert_eq!(error.span.text(), source);
        assert!(c.resolve_var(&Name::new(None, "R")).unwrap().is_none());
    }
    c.eval_string("(defprotocol P (p [x]))").unwrap();
    assert!(c.eval_string("(defrecord p [])").is_err());
    yes(
        &mut c,
        "(defrecord R [x]) (= [true true] [(ifn? ->R) (ifn? map->R)])",
    );
}

#[test]
fn printing_obeys_record_name_length_depth_and_metadata_controls() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defrecord R [x]) (= [\"#user.R{:x [1 2]}\" \"#user.R{:x [1 ...], ...}\" \"#user.R{:x #}\" \"#\"] [(pr-str (->R [1 2])) (binding [*print-length* 1] (pr-str (assoc (->R [1 2]) :z 3))) (binding [*print-level* 1] (pr-str (->R [1 2]))) (binding [*print-level* 0] (pr-str (->R [1 2])))])",
    );
    yes(
        &mut c,
        "(= \"^{:m 2} #user.R{:x 1}\" (binding [*print-meta* true] (pr-str (with-meta (->R 1) {:m 2}))))",
    );
}

#[test]
fn nested_lazy_fields_realize_for_equality_and_keys_without_borrowed_callbacks() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defrecord R [x]) (let [seen (atom []) r (->R (map (fn [x] (swap! seen conj x) x) [1 2])) eager (->R [1 2])] (= [true [1 2] :found true] [(= r eager) @seen (get {r :found} eager) (contains? #{[r]} [eager])]))",
    );
    yes(
        &mut c,
        "(let [r (map->R {:x (map inc [1 2]) :extra {:nested (map inc [3])}}) eager (map->R {:extra {:nested [4]} :x [2 3]})] (= [true :ok] [(= r eager) (get {r :ok} eager)]))",
    );
    let lazy = c.eval_string("(->R (map inc [1 2]))").unwrap();
    assert_eq!(
        c.map(&[(lazy, c.nil())]).unwrap_err(),
        CollectionError::RequiresRealization
    );
}

#[test]
fn generated_names_preserve_host_utf16_and_macroexpansion_does_not_install_types() {
    let mut c = Context::new();
    yes(&mut c, "(= 'do (first (macroexpand-1 '(defrecord R [x]))))");
    assert!(c.resolve_var(&Name::new(None, "R")).unwrap().is_none());
    let type_name = [u16::from(b'R'), 0xd800];
    let field_name = [u16::from(b'x'), 0xdc00];
    let declaration = c
        .list(&[
            c.symbol(Name::new(None, "defrecord")),
            c.symbol(Name::from_utf16(None, &type_name)),
            c.vector(&[c.symbol(Name::from_utf16(None, &field_name))])
                .unwrap(),
        ])
        .unwrap();
    let eval = c.eval_string("eval").unwrap();
    c.invoke(&eval, &[declaration]).unwrap();
    let mut constructor_name: Vec<_> = "->".encode_utf16().collect();
    constructor_name.extend(type_name);
    let constructor = c
        .resolve_var(&Name::from_utf16(None, &constructor_name))
        .unwrap()
        .unwrap();
    let record = c.invoke(&constructor, &[c.integer(7)]).unwrap();
    assert_eq!(
        record
            .get(&c.keyword(Name::from_utf16(None, &field_name)))
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::Integer(7)
    );
    let ValueView::String(printed) = c
        .print_value(&record, PrintOptions::default())
        .unwrap()
        .inspect()
    else {
        panic!("string")
    };
    assert!(printed.contains(&0xd800));
    assert!(printed.contains(&0xdc00));
}

#[test]
fn protocol_acceptance_and_overloads_use_ordinary_function_dispatch() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defprotocol Size (size [x])) (defrecord Box [items] Size (size [_] (count items))) (= [3 [4 5] 7] [(size (->Box [1 2 3])) (:items (map->Box {:items [4 5]})) (let [n 7] (size (reify Size (size [_] n))))])",
    );
    yes(
        &mut c,
        "(defprotocol P (p [x] [x y])) (defrecord R [v] P (p [_] v) (p [_ n] (+ v n))) (let [f p r (->R 2) o (reify P (p [_] 4) (p [_ n] (* 4 n)))] (= [2 5 4 12 5 2 [2 4]] [(f r) (f r 3) (f o) (f o 3) (apply f [r 3]) ((var p) r) (vec (map f [r o]))]))",
    );
}

#[test]
fn fields_capture_scope_shadowing_destructuring_and_recur() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defprotocol P (p [x] [x y])) (let [x 100 outer 7] (defrecord R [x] P (p [_] [x outer]) (p [_ x] x))) (let [r (->R 3) updated (assoc r :x 9)] (= [[3 7] [9 7] [nil 7] 5 [3 7]] [(p r) (p updated) (p (map->R {:extra 8})) (p r 5) (p r)]))",
    );
    yes(
        &mut c,
        "(defrecord R [x] P (p [_] x) (p [this n] (if (zero? n) x (recur (assoc this :x (inc x)) (dec n))))) (= [3 8] [(p (->R 3)) (p (->R 3) 5)])",
    );
    yes(
        &mut c,
        "(defrecord Destructure [x y] P (p [{:keys [x]}] [x y]) (p [_ {:keys [x] :or {x y}}] [x y])) (= [[3 7] [5 7] [7 7]] [(p (->Destructure 3 7)) (p (->Destructure 3 7) {:x 5}) (p (->Destructure 3 7) {})])",
    );
    yes(
        &mut c,
        "(defrecord Conditions [x] P (p [_] {:pre [(pos? x)] :post [(= % x)]} x)) (= 3 (p (->Conditions 3)))",
    );
    assert!(c.eval_string("(p (->Conditions 0))").is_err());
    yes(
        &mut c,
        "(defrecord Receiver [x] P (p [x] (:x x))) (= 3 (p (->Receiver 3)))",
    );
    assert!(
        c.eval_string("(defrecord NoExtra [x] P (p [_] extra))")
            .is_err()
    );
}

#[test]
fn record_generations_keep_old_methods_and_saved_methods_keep_protocol_identity() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defprotocol P (p [x])) (defrecord R [x] P (p [_] x)) (def r (->R 2)) (def saved-ctor ->R) (def f p) (defrecord R [x] P (p [_] (+ 10 x))) (= [false 2 12 12] [(= r (->R 2)) (p r) (p (saved-ctor 2)) (p (->R 2))])",
    );
    yes(
        &mut c,
        "(defprotocol P (p [x])) (= [true true true 2 12] [(= f p) (satisfies? P r) (satisfies? P (->R 1)) (f r) (f (->R 2))])",
    );
    yes(
        &mut c,
        "(def p (fn [_] 99)) (= [99 2 12] [(p r) (f r) (f (->R 2))])",
    );
    assert!(c.eval_string("(defprotocol P (p [x] [x y]))").is_err());
    yes(&mut c, "(= 2 (f r))");
}

#[test]
fn record_protocol_aliases_colliding_short_names_markers_and_partial_methods() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(ns a) (defprotocol P (f [x])) (ns b) (defprotocol P (f [x])) (ns user (:require [a :as aa] [b :as bb])) (defrecord R [x] aa/P (f [_] :a) bb/P (f [_] :b)) (let [r (->R 1)] (= [:a :b true true] [(aa/f r) (bb/f r) (satisfies? aa/P r) (satisfies? bb/P r)]))",
    );
    yes(
        &mut c,
        "(defprotocol Marker) (defprotocol P (p [x]) (q [x])) (defrecord Partial [x] Marker P (p [_] x)) (= [true true 7] [(satisfies? Marker (->Partial 7)) (satisfies? P (->Partial 7)) (p (->Partial 7))])",
    );
    let error = c.eval_string("(q (->Partial 7))").unwrap_err();
    assert!(matches!(
        error.kind,
        EvalErrorKind::MissingProtocolImplementation { .. }
    ));
    assert!(error.to_string().contains("user/q"));
}

#[test]
fn invalid_methods_do_not_replace_existing_types_or_constructor_roots() {
    let mut c = Context::new();
    c.eval_string("(defprotocol P (p [x] [x n])) (defrecord R [x] P (p [_] x))")
        .unwrap();
    let original = c.eval_string("R").unwrap();
    let constructor = c.eval_string("->R").unwrap();
    for source in [
        "(defrecord R [x] P (no [_] x))",
        "(defrecord R [x] P (p [] x))",
        "(defrecord R [x] P (p [_ n z] x))",
        "(defrecord R [x] P (p [_ & n] x))",
        "(defrecord R [x] P (p [_] x) (p [_] 2))",
        "(defrecord R [x] P P)",
        "(defrecord R [x] P (p [_] x) P)",
        "(defrecord R [x] P (p [_] missing))",
    ] {
        let error = c.eval_string(source).expect_err(source);
        assert!(!error.span.text().is_empty(), "{source}: {error}");
        assert!(
            original.same_identity(&c.eval_string("R").unwrap()),
            "{source}"
        );
        assert!(
            constructor.same_identity(&c.eval_string("->R").unwrap()),
            "{source}"
        );
        yes(&mut c, "(= 3 (p (->R 3)))");
    }
}

#[test]
fn extensions_update_existing_records_saved_methods_and_inline_implementations() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defprotocol P (p [x] [x y])) (defrecord R [n]) (def r (->R 2)) (def saved p) (extend-type R P (p ([x] (:n x)) ([x y] (+ (:n x) y)))) (= [true 2 5 5 2] [(satisfies? P r) (p r) (p r 3) (apply saved [r 3]) ((var p) r)])",
    );
    yes(
        &mut c,
        "(let [captured 10] (extend-type R P (p [x] (+ captured (:n x))))) (= [12 12 12] [(p r) (saved r) (p (->R 2))])",
    );
    assert!(matches!(
        c.eval_string("(p r 3)").unwrap_err().kind,
        EvalErrorKind::Arity { .. }
    ));
    yes(
        &mut c,
        "(defrecord Inline [n] P (p [_] n)) (def inline (->Inline 2)) (extend-type Inline P (p [_] 99)) (= [99 99] [(p inline) (saved (->Inline 3))])",
    );
    yes(
        &mut c,
        "(defrecord A [n]) (defrecord B [n]) (extend-protocol P A (p ([x] (:n x)) ([x y] (+ (:n x) y))) B (p [x] (* 2 (:n x)))) (= [2 4 5] [(p (->A 2)) (saved (->B 2)) (apply p [(->A 2) 3])])",
    );
}

#[test]
fn partial_extensions_preserve_omitted_methods_protocols_and_marker_membership() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defprotocol P (p [x]) (q [x]) (missing [x])) (defprotocol Q (other [x])) (defprotocol Marker) (defrecord R [n] Q (other [_] :other)) (def r (->R 2)) (extend-type R P (p [_] 1) (q [_] 2) Marker) (extend-type R P (p [_] 3)) (= [3 2 :other true true] [(p r) (q r) (other r) (satisfies? P r) (satisfies? Marker r)])",
    );
    assert!(matches!(
        c.eval_string("(missing r)").unwrap_err().kind,
        EvalErrorKind::MissingProtocolImplementation { .. }
    ));
    yes(
        &mut c,
        "(extend-protocol P R (missing [_] 4)) (= [3 2 4 :other] [(p r) (q r) (missing r) (other r)])",
    );
    yes(
        &mut c,
        "(extend-type R P) (extend-protocol Marker R) (= [3 2 4 true] [(p r) (q r) (missing r) (satisfies? Marker r)])",
    );
}

#[test]
fn external_methods_capture_lexicals_without_implicit_fields_and_keep_type_generations() {
    let mut c = Context::new();
    yes(
        &mut c,
        "(defprotocol P (p [x])) (defrecord R [n]) (let [n 7] (extend-type R P (p [x] [n (:n x)]))) (= [7 2] (p (->R 2)))",
    );
    assert!(c.eval_string("(extend-type R P (p [_] n))").is_err());
    yes(
        &mut c,
        "(extend-type R P (p [_] 1)) (def old (->R 2)) (defrecord R [n]) (extend-type R P (p [_] 10)) (= [1 10] [(p old) (p (->R 2))])",
    );
    yes(
        &mut c,
        "(defprotocol P (p [x])) (= [1 10] [(p old) (p (->R 2))])",
    );
}

#[test]
fn invalid_extensions_leave_all_previous_target_methods_callable() {
    let mut c = Context::new();
    c.eval_string("(defprotocol P (p [x] [x y]) (q [x])) (defrecord R [n] P (p [_] 1) (q [_] 2)) (def r (->R 3)) (def instance r) (def anonymous (reify P))").unwrap();
    for source in [
        "(extend-type R P (p [_] 9) (no [_] 3))",
        "(extend-type R P (p ([_] 9) ([] 3)))",
        "(extend-type R P (p ([_] 9) ([_ x y] 3)))",
        "(extend-type R P (p [_ & xs] 3))",
        "(extend-type R P (p ([_] 9) ([_] 3)))",
        "(extend-type R P (p [_] 9) P (q [_] 3))",
        "(extend-type R P (p [_] 9) (q [_] unresolved))",
        "(extend-type R P (p))",
        "(extend-type R P (p 3))",
        "(extend-type R P [])",
        "(extend-type R Missing (p [_] 9))",
        "(extend-type nil P (p [_] 9))",
        "(extend-type Number P (p [_] 9))",
        "(extend-type java.lang.String P (p [_] 9))",
        "(extend-type instance P (p [_] 9))",
        "(extend-type anonymous P (p [_] 9))",
        "(extend-type (reify P) P (p [_] 9))",
        "(extend-protocol P R (p [_] 9) nil (p [_] 9))",
    ] {
        let error = c.eval_string(source).expect_err(source);
        assert!(!error.span.text().is_empty(), "{source}: {error}");
        yes(&mut c, "(= [1 2] [(p r) (q r)])");
    }
}
