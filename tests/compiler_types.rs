//! Type checks use literal host values independently of the shared encoder.
use light_cljs::compile;
use std::process::Command;

fn run(source: &str) -> String {
    let js = compile(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    let output = Command::new("node")
        .args(["-e", &format!("console.log(JSON.stringify({js}));")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
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
