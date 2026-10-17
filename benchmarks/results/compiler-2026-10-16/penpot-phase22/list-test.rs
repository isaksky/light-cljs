#[test]
fn list_construction_keeps_intrinsic_count_while_public_count_remains_live() {
    // Pinned List.-conj increments its stored count; list construction never
    // asks ICounted for the size of its newly allocated tail. Public count
    // still observes a method installed on List.prototype.
    let body = r#"(do (extend-type List ICounted (-count [xs] (.-count xs)))
                     (let [prototype (.-prototype List)
                       slot "cljs$core$ICounted$_count$arity$1"
                       previous (js/Object.getOwnPropertyDescriptor prototype slot)
                       calls (atom 0)
                       value (atom nil)
                       observed (atom nil)]
                   (try
                     (js/Object.defineProperty prototype slot
                       #js {:configurable true :writable true
                            :value (fn [_] (swap! calls inc) 999)})
                     (reset! value (list 10 20 30))
                     (reset! observed (count @value))
                     (finally
                       (if previous
                         (js/Object.defineProperty prototype slot previous)
                         (js-delete prototype slot))))
                   [@calls @observed (count @value) (into [] @value)]))"#;
    let expected = "[1 999 3 [10 20 30]]";
    every_level(&format!("(pr-str {body})"), expected);
    every_level(
        &format!("(defmacro result [] (list 'quote {body})) (pr-str (result))"),
        expected,
    );
}
