#[test]
fn record_field_lookup_uses_keyword_identity_without_equivalence_dispatch() {
    // Upstream defrecord uses keyword case for lookup/contains and
    // keyword-identical? for assoc. An IEquiv override cannot name a field.
    let body = r#"(let [calls (atom 0)
                       key (specify! (keyword "y")
                             IEquiv (-equiv [_ _] (swap! calls inc) true))
                       unknown (specify! (keyword "missing")
                                 IEquiv (-equiv [_ _] (swap! calls inc) true))
                       impostor (reify IEquiv (-equiv [_ _] (swap! calls inc) true)
                                       IHash (-hash [_] 0))
                       record (->R 10 20)
                       associated (assoc record key 30)
                       extra (assoc record unknown 40)]
                   [(get record key :missing) (contains? record key)
                    (get record unknown :missing) (contains? record unknown)
                    (get record impostor :missing) (contains? record impostor)
                    [(.-x associated) (.-y associated)]
                    [(.-x extra) (.-y extra) (count extra)] @calls])"#;
    let expected = "[20 true :missing false :missing false [10 30] [10 20 3] 0]";
    every_level(&format!("(defrecord R [x y]) (pr-str {body})"), expected);
    every_level(
        &format!("(defrecord R [x y]) (defmacro result [] (list 'quote {body})) (pr-str (result))"),
        expected,
    );
}
