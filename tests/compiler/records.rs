//! Literal host observations avoid flattening nominal records into encoder maps.
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
fn record_updates_preserve_fields_and_drop_nominal_type_only_on_field_dissociation() {
    assert_eq!(
        run(
            "(defrecord R [a b]) (let [v (with-meta (assoc (->R 1 2) :c 3) {:m 4}) a (assoc v :a 5) b (dissoc a :a :c)] (array (.-a v) (.-a a) (record? a) (record? b) (:b b) (:m (meta b)) (count a) (nil? (empty a))))"
        ),
        "[1,5,true,false,2,4,3,true]\n"
    );
}

#[test]
fn records_have_nominal_equality_and_structural_key_lookup() {
    assert_eq!(
        run(
            "(defrecord R [x]) (defrecord S [x]) (let [a (->R [1 2]) b (map->R {:x [1 2]}) c (->S [1 2])] (array (= a b) (= (hash a) (hash b)) (= a c) (= a {:x [1 2]}) (= {:x [1 2]} a) (get {a 7} b) (count #{a c}) (contains? #{a} b)))"
        ),
        "[true,true,false,false,false,7,2,true]\n"
    );
}

#[test]
fn records_expose_generated_protocols_without_gaining_map_callability() {
    assert_eq!(
        run(
            "(defrecord R [x]) (let [r (->R 1)] (array (ifn? r) (ifind? r) (reduceable? r) (satisfies? IRecord r) (satisfies? IHash r) (satisfies? IEmptyableCollection r) (cloneable? r) (map-entry? (first (seq r))) (map-entry? (find r :x))))"
        ),
        "[false,false,false,true,true,false,true,true,true]\n"
    );
}

#[test]
fn record_printing_metadata_and_constructor_internals_are_observable() {
    assert_eq!(
        run(
            "(defrecord R [x]) (let [r (R. 1 {:m 2} {:z 3} 17) c (clone r)] (array (pr-str r) (str r) (hash c) (:m (meta c)) (:z c) (identical? r c) (.-__hash c)))"
        ),
        "[\"#cljs.user.R{:x 1, :z 3}\",\"[object Object]\",17,2,3,false,17]\n"
    );
}

#[test]
fn external_record_methods_affect_existing_instances_and_copies() {
    assert_eq!(
        run(
            "(defprotocol P (f [this])) (defrecord R [x] P (f [_] x)) (def r (->R 1)) (extend-type R P (f [this] (+ 10 (:x this))) ICounted (-count [_] 30)) (array (f r) (f (assoc r :x 2)) (f (with-meta r {})) (count r))"
        ),
        "[11,12,11,30]\n"
    );
}

#[test]
fn invalid_record_fields_and_methods_have_located_diagnostics() {
    for source in [
        "(defrecord R [x x])",
        "(defrecord R [x] Object (f [_] (set! x 2)))",
        "(defrecord R [x] Unknown (f [_] x))",
        "(defrecord R [x] ICounted (-count [_ x] x))",
        "(defrecord R [] Object (f [] 1))",
        "(defrecord* R [x])",
        "(defrecord* R [x x] {})",
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
