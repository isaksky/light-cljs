//! Independent host-level expectations for compiled protocol declarations/dispatch.
use super::optimization::readable;
use light_cljs::{OptimizationLevel, analyze, compile, parse, prepare_application};

/// Every optimization level prints what `None` prints.
fn run(source: &str) -> String {
    super::optimization::json_at_every_level(source)
}

#[test]
fn direct_saved_metadata_and_receiver_identity_have_literal_expectations() {
    assert_eq!(
        run(
            "(defprotocol P :extend-via-metadata true (f [this] [this x])) (let [v (reify P (f [this] (this-as self (identical? self this))) (f [this x] x)) saved f m (with-meta v {`f (fn [self & xs] (this-as receiver (array (nil? receiver) (identical? self v) (count xs))))})] (array (saved v) (apply saved v [7]) (satisfies? P v) (implements? P v) (f m 1)))"
        ),
        "[true,7,true,true,[false,false,1]]\n"
    );
    assert_eq!(
        run(
            "(defprotocol P (f [this x])) (let [v (reify P (f [_ x] x)) saved f] (array (saved v 1 2) (undefined? (saved v))))"
        ),
        "[1,true]\n"
    );
}

#[test]
fn protocol_dispatch_uses_public_slots_and_native_tables() {
    assert_eq!(
        run(
            "(defprotocol Slashy (/ [_])) (let [v (reify Slashy (/ [_] 7))] (array (/ v) (fn? (aget v \"cljs$user$Slashy$_SLASH_$arity$1\"))))"
        ),
        "[7,true]\n"
    );
    assert_eq!(
        run(
            "(defprotocol P (f [this])) (let [v #js {}] (aset v \"cljs$user$P$f$arity$1\" (fn [self] (this-as receiver (identical? self receiver)))) (aset v \"cljs$user$P$\" PROTOCOL_SENTINEL) (array (f v) (implements? P v) (satisfies? P v)))"
        ),
        "[true,true,true]\n"
    );
    assert_eq!(
        run(
            "(defprotocol P (f [this])) (aset f \"string\" (fn [x] (.toUpperCase x))) (aset P \"string\" true) (array (f \"ok\") (satisfies? P \"ok\") (implements? P \"ok\"))"
        ),
        "[\"OK\",true,false]\n"
    );
}

/// Protocol methods called as values, whose receivers no level can type: at
/// `Advanced` each method's dispatcher is a function of its own that calls the
/// receiver's slot and leaves the rest to the shared path (light-cljs-265).
#[test]
fn protocol_functions_as_values_keep_dispatch_at_every_level() {
    // Four types, `extend-type` of a host type, nil and `object` after the
    // values were saved, and a value no implementation covers.
    assert_eq!(
        run(
            "(defprotocol Shape (area [s]) (scale [s k]))
             (defrecord Sq [a] Shape (area [_] (* a a)) (scale [_ k] (* a k)))
             (defrecord Re [w h] Shape (area [_] (* w h)) (scale [_ k] (* w h k)))
             (deftype Tr [b h] Shape (area [_] (/ (* b h) 2)) (scale [_ k] k))
             (def shapes [(->Sq 2) (->Re 2 3) (Tr. 4 1) (reify Shape (area [_] 1) (scale [_ k] (- k)))])
             (defn each [f xs & args] (into-array (map #(apply f % args) xs)))
             (def saved [area scale])
             (extend-type string Shape (area [s] (count s)) (scale [s k] (str s k)))
             (extend-type nil Shape (area [_] -1) (scale [_ k] (array k)))
             (array (each area shapes) (each scale shapes 10) ((saved 0) \"abc\") ((saved 1) \"ab\" 1)
                    ((saved 0) nil) ((saved 1) nil 2) (area (Sq. 3 nil nil))
                    (try ((saved 0) 5) (catch :default e (.-message e)))
                    (do (extend-type object Shape (area [_] \"object\") (scale [_ k] k))
                        (array ((saved 0) (js/Date. 0)) ((saved 0) #js {}) ((saved 1) (js/Date. 0) 7))))"
        ),
        "[[4,6,2,1],[20,60,10,-10],3,\"ab1\",-1,[2],9,\"No protocol method cljs.user/area defined for this value\",[\"object\",\"object\",7]]\n"
    );
    // One arity pads or cuts the arguments to it; with several, the count
    // selects the slot and any other count is an arity error.
    assert_eq!(
        run("(defprotocol P (one [x y]) (two [x] [x y z]))
             (def v (reify P (one [_ y] (array y)) (two [_] \"one\") (two [_ y z] (+ y z))))
             (def fs [one two])
             (array ((fs 0) v) ((fs 0) v 1 2) ((fs 1) v) ((fs 1) v 1 2) (apply (fs 1) [v 3 4])
                    (try ((fs 1) v 1) (catch :default e (.-message e))))"),
        "[[null],[1],\"one\",3,7,\"Invalid arity: 2\"]\n"
    );
    let source = "(defprotocol P (f [x]) (g [x] [x y]) (h [x & ys])) (defprotocol M :extend-via-metadata true (m [x]))
                  (def v (reify P (f [_] 1) (g [_] 2) (g [_ y] y) (h [_ a b] (count b)) M (m [_] 4)))
                  (def fs [f g h m]) (array ((fs 0) v) ((fs 1) v 3) ((fs 2) v 1 2) ((fs 3) v))";
    assert_eq!(run(source), "[1,3,2,4]\n");
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let js = prepare_application(&program, readable(OptimizationLevel::Advanced)).emit_js();
    // A method of fixed signatures gets its own dispatcher; a variadic one and
    // one extendable through metadata keep the shared one.
    for expected in [
        "(m0, m1) => function (r0) { return r0?.[m1] != null ? r0[m1](r0) : m0(r0); })",
        "(m0, m1, m2) => function (r0, a1 = undefined) { if (arguments.length === 1) { if (r0?.[m1] != null) return r0[m1](r0); } else if (arguments.length === 2) { if (r0?.[m2] != null) return r0[m2](r0, a1); } return m0.apply(null, arguments); })",
        "\"cljs.user/h\", [3], false, 1, () => ",
        "\"cljs.user/m\", [1], true, null, () => ",
    ] {
        assert!(js.contains(expected), "{expected}\n{js}");
    }
    assert_eq!(js.matches("=> function (r0").count(), 2, "{js}");
}

/// A dispatcher calls the slot of the call's arity that a host getter gives,
/// reads a slot that holds nothing once before the `object` implementation,
/// and keeps the shared dispatcher's `length`. How often it reads a slot that
/// holds a method is not part of the contract (docs/advanced-optimization.md).
#[test]
fn protocol_dispatchers_call_host_slots_and_keep_their_length() {
    assert_eq!(
        run(
            "(defprotocol P (f [x]) (g [x] [x y]) (h [x y z]))
             (extend-type object P (f [_] :default) (g ([_] :default) ([_ y] y)) (h [_ y z] z))
             (def reads (atom 0))
             (defn host [slot value]
               (let [o #js {}]
                 (js/Object.defineProperty o slot #js {:get (fn [] (swap! reads inc) value)})
                 o))
             (def fs [f g h])
             (defn call [slot value call]
               (reset! reads 0)
               (let [result (str (call (host slot value)))] (if value result (array result @reads))))
             (array (call \"cljs$user$P$f$arity$1\" (fn [_] 42) #((fs 0) %))
                    (call \"cljs$user$P$f$arity$1\" nil #((fs 0) %))
                    (call \"cljs$user$P$g$arity$1\" (fn [_] 1) #((fs 1) %))
                    (call \"cljs$user$P$g$arity$2\" (fn [_ y] (* 2 y)) #((fs 1) % 3))
                    (call \"cljs$user$P$g$arity$2\" nil #((fs 1) % 3))
                    (call \"cljs$user$P$h$arity$3\" (fn [_ y z] (+ y z)) #((fs 2) % 3 4))
                    (call \"cljs$user$P$h$arity$3\" nil #((fs 2) % 3 4))
                    (.-length f) (.-length g) (.-length h))"
        ),
        "[\"42\",[\":default\",1],\"1\",\"6\",[\"3\",1],\"7\",[\"4\",1],1,1,1]\n"
    );
}

#[test]
fn declared_fixed_protocol_arities_include_large_signatures() {
    for n in [1, 2, 20, 21, 24] {
        let params = (0..n)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let args = (1..n).map(|i| format!(" {i}")).collect::<String>();
        let source = format!(
            "(defprotocol P (f [{params}])) (let [v (reify P (f [{params}] {}))] (f v{args}))",
            if n == 1 {
                "42".into()
            } else {
                format!("a{}", n - 1)
            }
        );
        assert_eq!(
            run(&source),
            format!("{}\n", if n == 1 { 42 } else { n - 1 })
        );
    }
}

#[test]
fn malformed_protocol_declarations_and_implementations_keep_locations() {
    for source in [
        "(defprotocol)",
        "(defprotocol 3)",
        "(defprotocol P (f []))",
        "(defprotocol P (f [x] [y]))",
        "(defprotocol P (f))",
        "(defprotocol P 42)",
        "(defprotocol P :extend-via-metadata)",
        "(defprotocol P (f [x])) (reify P (f [x y] 1))",
        "(defprotocol P (f [x])) (reify P (g [x] 1))",
        "(reify missing)",
        "(extend-type)",
    ] {
        let error = compile(source).expect_err(source);
        assert!(!error.to_string().is_empty(), "{source}");
    }
}

#[test]
fn primitive_programs_still_omit_the_runtime() {
    let js = compile("42").unwrap();
    assert!(!js.contains("protocolMethod"));
    assert!(js.len() < 200);
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn protocol_bodies() {
        run(&[
            Case::observed(
                "membership",
                include_str!("../fixtures/phase-oracles/protocols/membership.cljs"),
                r#"{"id":"membership","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "metadata-precedence",
                include_str!("../fixtures/phase-oracles/protocols/metadata-precedence.cljs"),
                r#"{"id":"metadata-precedence","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "native-and-default",
                include_str!("../fixtures/phase-oracles/protocols/native-and-default.cljs"),
                r#"{"id":"native-and-default","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "external-named",
                include_str!("../fixtures/phase-oracles/protocols/external-named.cljs"),
                r#"{"id":"external-named","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::observed(
                "external-host",
                include_str!("../fixtures/phase-oracles/protocols/external-host.cljs"),
                r#"{"id":"external-host","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
            Case::expression(
                "extend-to-object",
                include_str!("../fixtures/phase-oracles/protocols/extend-to-object.cljs"),
            ),
        ]);
    }
}
