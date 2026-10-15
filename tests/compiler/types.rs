//! Type checks use literal host values independently of the shared encoder.
use light_cljs::compile;

/// Every optimization level prints what `None` prints.
fn run(source: &str) -> String {
    super::optimization::json_at_every_level(source)
}

#[test]
fn field_reads_and_writes_remain_live_in_escaped_closures() {
    assert_eq!(
        run(
            "(deftype T [^:mutable value] Object (read [_] (fn [] value)) (write [_] (fn [n] (set! value n)))) (let [v (T. 1) a (.read v) b (.write v)] (array (a) (b 7) (a) (.-value v)))"
        ),
        "[1,7,7,7]\n"
    );
}

#[test]
fn field_receiver_is_the_actual_method_call_receiver() {
    assert_eq!(
        run(
            "(defprotocol P (f [this])) (deftype T [x] P (f [this] (array x this))) (let [a (T. 1) b (T. 2) f (aget a \"cljs$user$P$f$arity$1\") r (.call f b a)] (array (aget r 0) (identical? b (aget r 1))))"
        ),
        "[2,true]\n"
    );
}

/// An instance calls through its type's `IFn` table, as upstream's instances
/// call their prototype's methods: an extension affects existing instances,
/// including one that replaces the table. Before light-cljs-258 each
/// instance kept the table its type had when it was constructed.
#[test]
fn ifn_extensions_affect_existing_instances() {
    assert_eq!(
        run(
            "(deftype T [x] IFn (-invoke [_ a] (array \"old\" x a))) (deftype U [x]) (def t (T. 1)) (def u (U. 2))
             (extend-type T IFn (-invoke ([this a] (array \"new\" (.-x this) a))))
             (extend-type U IFn (-invoke ([this a] (array \"u\" (.-x this) a))))
             (array (t 3) ((T. 4) 5) (u 6) (apply t [7]) (ifn? t) (ifn? u))"
        ),
        "[[\"new\",1,3],[\"new\",4,5],[\"u\",2,6],[\"new\",1,7],true,true]\n"
    );
}

/// A field named as a protocol mask is assigned before the masks at lower
/// levels, so the mask wins; at `Advanced` such a type keeps `makeType`'s
/// constructor instead of one of its own, whose masks are its prototype's
/// (light-cljs-264).
#[test]
fn mask_named_fields_read_as_at_none() {
    assert_eq!(
        run(
            "(deftype T [cljs$lang$protocol_mask$partition0$ y]) (deftype U [x cljs$lang$protocol_mask$partition1$])
             (defrecord R [cljs$lang$protocol_mask$partition0$])
             (array (.-cljs$lang$protocol_mask$partition0$ (T. 7 1)) (.-cljs$lang$protocol_mask$partition1$ (U. 1 7))
                    (.-cljs$lang$protocol_mask$partition0$ (->R 7)) (.-y (T. 7 1)) (map? (->R 7)))"
        ),
        "[0,0,-2064251126,1,true]\n"
    );
}

/// Since light-cljs-258 the emitter writes each type's field assignments and
/// `typeMasks` turns a raw type's mask map into assignments once per type.
/// A saved factory still passes every argument to the type's current
/// definition, and a field named `__proto__` stays an own field.
#[test]
fn constructors_keep_raw_masks_redefined_fields_and_proto_fields() {
    assert_eq!(
        run(
            "(deftype* M [x] {0 2 1 16}) (deftype* N [] nil) (deftype* E [y] {}) (defrecord* Q [x] {0 4})
             (deftype T [x]) (def factory ->T) (deftype T [x y]) (defrecord R [a])
             (deftype P [^:mutable __proto__] Object (get [_] __proto__) (put [_ v] (set! __proto__ v)))
             (let [a (new M 1) b (new M 2) q (new Q 5 nil nil nil) t (T. 1 2) r (->R 1) p (P. 7) o (P. #js {:a 1})
                   k (let [__proto__ 9] (reify Object (get [_] __proto__)))]
               (.put p 8)
               (array (.-cljs$lang$protocol_mask$partition0$ a) (.-cljs$lang$protocol_mask$partition1$ b) (js/Object.keys a)
                      (js/Object.keys (new N)) (js/Object.keys (new E 3)) (js/Object.keys q) (.-cljs$lang$protocol_mask$partition0$ q)
                      (.-y (factory 1 2)) (.-y (apply factory [3 4])) (.-y t) (:a r)
                      (.get p) (.get o) (instance? P o) (.hasOwnProperty o \"__proto__\") (.get k)))"
        ),
        "[2,16,[\"x\",\"cljs$lang$protocol_mask$partition0$\",\"cljs$lang$protocol_mask$partition1$\"],\
         [\"cljs$lang$protocol_mask$partition0$\",\"cljs$lang$protocol_mask$partition1$\"],[\"y\"],\
         [\"x\",\"__meta\",\"__extmap\",\"__hash\",\"cljs$lang$protocol_mask$partition0$\"],4,\
         2,4,2,1,8,{\"a\":1},true,true,9]\n"
    );
}

#[test]
fn constructors_and_redefinitions_retain_instance_identity() {
    assert_eq!(
        run(
            "(deftype T [x]) (def old T) (def factory ->T) (def a (T. 1)) (deftype T [x]) (let [b (factory 2)] (array (instance? old a) (instance? T a) (instance? T b) (identical? old (type a)) (.-x b)))"
        ),
        "[true,false,true,true,2]\n"
    );
}

#[test]
fn custom_reduction_uses_protocols_without_requiring_a_sequence() {
    assert_eq!(
        run(
            "(deftype T [x] IReduce (-reduce [_ f] (f x x)) (-reduce [_ f init] (f init x)) IKVReduce (-kv-reduce [_ f init] (f init :x x))) (let [v (T. 3)] (array (reduce + v) (reduce + 2 v) (reduce-kv (fn [sum _ x] (+ sum x)) 4 v) (satisfies? IReduce v)))"
        ),
        "[6,5,7,true]\n"
    );
}

#[test]
fn immutable_fields_shadowed_fields_and_bad_methods_have_located_errors() {
    for source in [
        "(deftype T [x] Object (f [_] (set! x 1)))",
        "(deftype T [^:mutable x] Object (f [_ x] (set! x 1)))",
        "(deftype T [^{:mutable false} x] Object (f [_] (set! x 1)))",
        "(deftype T [x x])",
        "(deftype T [x ^:mutable x])",
        "(deftype T [x] Object (f [] x))",
        "(deftype T [] Missing (f [_] 1))",
        "(deftype T [] IReduce (-reduce [_] 1))",
        "(deftype T [] Object (f [_] (inc (recur))))",
        "(deftype T [] Object (f [_ x] (recur x x x)))",
        "(deftype T [] Object (f [_] 1) (f [_] 2))",
        "(deftype T [] Object (1 [this] 2))",
        "(deftype T [] Object (f ([this] 1) ([this x] x)))",
    ] {
        let error = compile(source).unwrap_err();
        let light_cljs::CompileError::Analysis(error) = error else {
            panic!("{error}");
        };
        assert!(
            error.span.start < source.len() && !error.span.is_empty(),
            "{error}"
        );
    }
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn deftype_test_namespace() {
        run(&[Case::namespace(
            "deftype-core",
            "cljs.core-test",
            "(run nil)",
            include_str!("../fixtures/compiler-namespaces/deftype/cljs/core_test.cljs"),
        )]);
    }
}
