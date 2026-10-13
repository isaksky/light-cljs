//! Protocol extension programs use fresh Node processes: native tables and JS
//! prototypes are observable mutable state, including across saved functions.
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
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn native_selectors_default_and_saved_dispatch() {
    assert_eq!(
        run(r#"
      (defprotocol P (f [x])) (def saved f)
      (extend-protocol P nil (f [_] "nil") object (f [_] "object")
        boolean (f [_] "boolean") number (f [n] (+ n 1)) string (f [s] (str s "!"))
        array (f [a] (alength a)) function (f [g] (g)) default (f [_] "default"))
      (array (saved nil) (f js/undefined) (f #js {}) (f false) (f 4) (f "s")
        (f #js [1 2]) (f (fn [] 9)) (f (js/Symbol "x"))
        (satisfies? P nil) (implements? P nil) (satisfies? P 1))
    "#),
        "[\"nil\",\"nil\",\"object\",\"boolean\",5,\"s!\",2,9,\"default\",true,false,true]\n"
    );
}

#[test]
fn old_instances_instance_precedence_and_metadata() {
    assert_eq!(
        run(r#"
      (defprotocol P :extend-via-metadata true (f [x]))
      (deftype T []) (defrecord R [])
      (let [old (T.) record (R.) saved f]
        (extend-type default P (f [_] "default"))
        (extend-type T P (f [_] "type"))
        (extend-type R P (f [_] "record"))
        (specify! old P (f [_] "instance"))
        (array (identical? old (specify! old)) (saved old) (saved (T.)) (saved record)
          (f (with-meta record {`f (fn [_] "metadata")}))
          (satisfies? P old) (implements? P old)))
    "#),
        "[true,\"instance\",\"type\",\"record\",\"metadata\",true,true]\n"
    );
}

#[test]
fn replacement_keeps_current_method_and_changes_later_calls() {
    assert_eq!(
        run(r#"
      (defprotocol P (f [x])) (deftype T [])
      (let [old (T.) saved f]
        (extend-type T P (f [_] (extend-type T P (f [_] 2)) 1))
        (extend-type number P (f [_] (extend-type number P (f [_] 4)) 3))
        (array (saved old) (saved old) (saved 0) (saved 0)))
    "#),
        "[1,2,3,4]\n"
    );
}

#[test]
fn builtin_class_extensions_and_nonmutating_specify() {
    assert_eq!(
        run(r#"
      (defprotocol P (f [x]))
      (let [v [1] m {} h (hash-map) k :x s 'x]
        (extend-type PersistentVector P (f [_] "vector"))
        (extend-type PersistentArrayMap P (f [_] "array-map"))
        (extend-type PersistentHashMap P (f [_] "hash-map"))
        (extend-type Keyword P (f [_] "keyword"))
        (extend-type Symbol P (f [_] "symbol"))
        (let [copy (specify v P (f [_] "instance"))]
          (array (f v) (f m) (f h) (f k) (f s) (f copy)
            (= copy v) (identical? copy v) (instance? PersistentVector v))))
    "#),
        "[\"vector\",\"array-map\",\"hash-map\",\"keyword\",\"symbol\",\"instance\",true,false,true]\n"
    );
}

#[test]
fn native_extension_preserves_core_fast_paths_and_raw_protocol_dispatch() {
    assert_eq!(
        run(r#"
      (extend-type string ICounted (-count [_] 99) IIndexed (-nth ([_ _] 98) ([_ _ _] 97))
        ISeqable (-seq [_] '(96)) IReduce (-reduce ([_ _] 95) ([_ _ _] 94)))
      (array (count "ab") (-count "ab") (nth "ab" 0) (-nth "ab" 0)
        (first (seq "ab")) (first (-seq "ab")) (reduce str "ab") (-reduce "ab" str)
        (counted? "ab") (indexed? "ab") (reduceable? "ab"))
    "#),
        "[2,99,\"a\",98,\"a\",96,\"ab\",95,true,true,true]\n"
    );
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn extension_test_bodies() {
        run(&[
            Case::expression(
                "cljs-2974",
                include_str!("../fixtures/phase-oracles/extensions/cljs-2974.cljs"),
            ),
            Case::expression(
                "test-414",
                include_str!("../fixtures/phase-oracles/extensions/test-414.cljs"),
            ),
            Case::expression(
                "cljs-2812",
                include_str!("../fixtures/phase-oracles/extensions/cljs-2812.cljs"),
            ),
        ]);
    }

    #[test]
    fn instance_extensions() {
        run(&[
            Case::observed(
                "instance-object-ifn",
                include_str!("../fixtures/phase-oracles/extensions/instance-object-ifn.cljs"),
                r#"{"id":"instance-object-ifn","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "instance-exception",
                include_str!("../fixtures/phase-oracles/extensions/instance-exception.cljs"),
                r#"{"id":"instance-exception","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "primitive-instance-noop",
                include_str!("../fixtures/phase-oracles/extensions/primitive-instance-noop.cljs"),
                r#"{"id":"primitive-instance-noop","outcome":{"status":"returned","value":1},"trace":[],"stdout":""}"#,
            ),
            // The value is a JavaScript array, which a macro cannot return as code.
            Case::observed_during_expansion(
                "dispatch-precedence",
                include_str!("../fixtures/phase-oracles/extensions/dispatch-precedence.cljs"),
                r#"{"id":"dispatch-precedence","outcome":{"status":"returned","value":[true,"instance","type","record","metadata",true,true]},"trace":[],"stdout":""}"#,
            ),
        ]);
    }
}
