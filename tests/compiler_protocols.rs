//! Independent host-level expectations for compiled protocol declarations/dispatch.
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
