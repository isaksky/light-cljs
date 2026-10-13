//! Literal host expectations independent of the shared observation encoder.
use light_cljs::compile;

/// Every optimization level prints what `None` prints.
fn run(source: &str) -> String {
    super::optimization::json_at_every_level(source)
}

#[test]
fn anonymous_types_and_metadata_copies_preserve_receiver_fields() {
    assert_eq!(
        run(
            "(defprotocol P (f [this])) (let [make (fn [captured] (reify P (f [this] (array captured (.-captured this))))) a (make 2) b (make 3) m (with-meta a {})] (set! (.-captured m) 4) (array (identical? (type a) (type b)) (identical? (type a) (type m)) (identical? a m) (object? a) (f a) (f b) (f m)))"
        ),
        "[true,true,false,false,[2,2],[3,3],[4,4]]\n"
    );
}

#[test]
fn raw_protocol_slots_use_the_call_receiver_and_live_capture_fields() {
    assert_eq!(
        run(
            "(defprotocol P (f [this])) (let [make (fn [x] (reify P (f [this] (array x this)))) a (make 1) b (make 2) method (aget a \"cljs$user$P$f$arity$1\") result (.call method b a)] (array (aget result 0) (identical? b (aget result 1))))"
        ),
        "[2,true]\n"
    );
    assert_eq!(
        run("(let [x 1 v (reify IFn (-invoke [this] (set! (.-x this) 2) x))] (array (v) (.-x v)))"),
        "[2,2]\n"
    );
}

#[test]
fn metadata_copies_retain_callable_and_object_overload_behavior() {
    assert_eq!(
        run(
            "(let [x 7 a (reify IFn (-invoke [_ n] (+ x n)) Object (custom [_] x) (custom [_ y] (+ x y))) b (with-meta a {:m 1})] (array (b 2) (apply b [3 4]) (.custom b) (.custom b 4)))"
        ),
        "[9,10,7,11]\n"
    );
}

#[test]
fn nested_reify_captures_each_receiver_independently() {
    assert_eq!(
        run(
            "(defprotocol P (f [this])) (let [make (fn [x] (reify P (f [_] (reify P (f [_] x))))) a (make 1) b (make 2) c (f a) d (f b)] (set! (.-x a) 3) (array (f c) (f d) (f (f a)) (identical? (type c) (type d))))"
        ),
        "[1,2,3,true]\n"
    );
}

#[test]
fn malformed_methods_and_non_tail_recurrence_are_rejected() {
    for source in [
        "(reify Object (f [] 1))",
        "(reify Object (f [this] (inc (recur))))",
        "(reify Object (f [this x] (recur x x x)))",
        "(reify Object (f [this] 1) (f [this] 2))",
        "(reify (f [this] 1))",
        "(reify MissingProtocol)",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
}
