use light_cljs::interpreter::*;

fn printed(c: &mut Context, source: &str) -> String {
    let value = c
        .eval_string(source)
        .unwrap_or_else(|e| panic!("{source}: {e}"));
    let ValueView::String(text) = c
        .print_value(&value, PrintOptions::default())
        .unwrap()
        .inspect()
    else {
        panic!("string")
    };
    String::from_utf16(&text).unwrap()
}

#[test]
fn declarations_publish_ordinary_functions_metadata_and_marker_protocols() {
    let mut c = Context::new();
    assert_eq!(
        printed(
            &mut c,
            "(defprotocol Size \"doc\" (size [x] [x y] \"method-doc\"))"
        ),
        "Size"
    );
    assert_eq!(
        printed(
            &mut c,
            "[(ifn? size) (fn? size) (satisfies? Size nil) (satisfies? Size []) (= (:protocol (meta (var size))) (var Size)) (:doc (meta (var Size))) (:doc (meta (var size))) (:arglists (meta (var size)))]"
        ),
        "[true true false false true \"doc\" \"method-doc\" ([x] [x y])]"
    );
    assert_eq!(
        printed(
            &mut c,
            "(defprotocol Marker) [(satisfies? Marker {}) (satisfies? Marker (atom nil))]"
        ),
        "[false false]"
    );
    assert_eq!(
        c.eval_string("Size").unwrap().inspect(),
        ValueView::Protocol(Name::new(Some("user"), "Size"))
    );
    assert_eq!(
        printed(&mut c, "(pr-str Size)"),
        "\"#<Protocol user/Size>\""
    );
    assert_eq!(
        printed(&mut c, "(try (size []) (catch Exception _ :missing))"),
        ":missing"
    );
}

#[test]
fn dispatch_failures_and_fixed_arities_are_explicit_for_all_call_paths() {
    let mut c = Context::new();
    c.eval_string("(defprotocol Size (size [x] [x y])) (def saved size)")
        .unwrap();
    for expression in [
        "(size [])",
        "(saved [])",
        "((var size) [])",
        "(apply size [[]])",
        "((fn [f] (f [])) size)",
    ] {
        let error = c.eval_string(expression).unwrap_err();
        assert!(
            matches!(
                error.kind,
                EvalErrorKind::MissingProtocolImplementation {
                    receiver: "vector",
                    ..
                }
            ),
            "{error}"
        );
        let message = error.to_string();
        assert!(
            message.contains("user/Size") && message.contains("user/size"),
            "{message}"
        );
        assert!(
            error.span.text().contains("size")
                || error.span.text().contains("saved")
                || error.span.text().contains("f")
        );
    }
    for expression in [
        "(size)",
        "(size [] 1 2)",
        "(apply size [])",
        "(apply size (range))",
        "((var size))",
        "(saved [] 1 2)",
    ] {
        let error = c.eval_string(expression).unwrap_err();
        assert!(
            matches!(error.kind, EvalErrorKind::Arity { ref fixed, variadic: None, .. } if fixed.as_ref() == [1, 2]),
            "{expression}: {error}"
        );
    }
    for value in [
        "nil",
        "true",
        "1",
        "1.0",
        "1/2",
        "\\a",
        "\"x\"",
        ":x",
        "'x",
        "[]",
        "{}",
        "#{}",
        "'()",
        "(range)",
        "(fn [] 1)",
        "(atom nil)",
        "(volatile! nil)",
        "(delay 1)",
        "Size",
        "(var size)",
        "*ns*",
    ] {
        assert_eq!(
            printed(&mut c, &format!("(satisfies? Size {value})")),
            "false",
            "{value}"
        );
    }
}

#[test]
fn same_shape_redefinition_preserves_identities_and_refreshes_metadata() {
    let mut c = Context::new();
    c.eval_string("(defprotocol P \"old\" (f [x] [x y] \"old-f\") (g [x])) (def old-p P) (def old-f f) (def old-g g)").unwrap();
    assert_eq!(
        printed(
            &mut c,
            "(defprotocol P \"new\" (g [z]) (f [z y] [z] \"new-f\")) [(identical? P old-p) (identical? f old-f) (identical? g old-g) (:doc (meta (var P))) (:doc (meta (var f))) (:arglists (meta (var f)))]"
        ),
        "[true true true \"new\" \"new-f\" ([z y] [z])]"
    );
    for source in [
        "(defprotocol P (f [x]))",
        "(defprotocol P (f [x] [x y] [x y z]) (g [x]))",
        "(defprotocol P (f [x] [x y]) (h [x]))",
    ] {
        let error = c.eval_string(source).unwrap_err();
        assert!(
            matches!(error.kind, EvalErrorKind::InvalidForm(_)),
            "{error}"
        );
        assert_eq!(
            printed(
                &mut c,
                "[(identical? P old-p) (identical? f old-f) (identical? g old-g)]"
            ),
            "[true true true]"
        );
        assert!(c.resolve_var(&Name::new(None, "h")).unwrap().is_none());
    }
    assert_eq!(
        printed(
            &mut c,
            "(def f (fn [x] 42)) [(f []) (try (old-f []) (catch Exception _ :missing))]"
        ),
        "[42 :missing]"
    );
    assert_eq!(
        printed(
            &mut c,
            "(defprotocol P (f [x] [x y]) (g [x])) (identical? f old-f)"
        ),
        "true"
    );
}

#[test]
fn invalid_declarations_preserve_tables_and_declaration_locations() {
    for declaration in [
        "(defprotocol 1)",
        "(defprotocol P (1 [x]))",
        "(defprotocol P (f))",
        "(defprotocol P (f []))",
        "(defprotocol P (f [x & xs]))",
        "(defprotocol P (f [x] [y]))",
        "(defprotocol P (f [x]) (f [x y]))",
        "(defprotocol P (P [x]))",
        "(defprotocol P (f [1]))",
        "(defprotocol P (f [x x]))",
        "(defprotocol P (a/f [x]))",
        "(defprotocol P (f [a/x]))",
        "(defprotocol P :extend-via-metadata true)",
        "(defprotocol P :on-interface Foo)",
    ] {
        let mut c = Context::new();
        c.eval_string("(defprotocol Valid (valid [x])) (def before valid)")
            .unwrap();
        let error = c.eval_string(declaration).unwrap_err();
        assert_eq!(
            error.phase,
            EvalPhase::Macroexpansion,
            "{declaration}: {error}"
        );
        assert!(
            matches!(error.kind, EvalErrorKind::InvalidForm(_)),
            "{error}"
        );
        assert_eq!(error.span.text(), declaration);
        assert_eq!(printed(&mut c, "(identical? valid before)"), "true");
        assert!(c.resolve_var(&Name::new(None, "P")).unwrap().is_none());
        assert!(c.resolve_var(&Name::new(None, "f")).unwrap().is_none());
    }
}

#[test]
fn namespaced_methods_aliases_refers_and_conflicts_preserve_distinct_identity() {
    let mut c = Context::new();
    assert_eq!(
        printed(
            &mut c,
            "(ns a) (defprotocol P (f [x])) (ns b) (defprotocol P (f [x])) (ns user (:require [a :as aa] [b :as bb])) [(identical? aa/f bb/f) (= (:protocol (meta (var aa/f))) (var aa/P)) (= (:protocol (meta (var bb/f))) (var bb/P))]"
        ),
        "[false true true]"
    );
    for (source, expected) in [("(aa/f nil)", "a/P"), ("(bb/f nil)", "b/P")] {
        assert!(
            c.eval_string(source)
                .unwrap_err()
                .to_string()
                .contains(expected)
        );
    }
    c.eval_string("(defprotocol Local (same [x]))").unwrap();
    assert!(
        c.eval_string("(defprotocol Other (same [x]))")
            .unwrap_err()
            .to_string()
            .contains("conflicts")
    );
    c.eval_string("(def ordinary 10)").unwrap();
    assert!(c.eval_string("(defprotocol Other (ordinary [x]))").is_err());
    assert!(c.eval_string("(defprotocol same)").is_err());
    assert!(c.eval_string("(defprotocol Other (Local [x]))").is_err());
    c.eval_string("(ns referred (:require [a :refer [f]]))")
        .unwrap();
    assert!(c.eval_string("(defprotocol P (f [x]))").is_err());
    assert_eq!(printed(&mut c, "(= (var f) (var a/f))"), "true");
}

#[test]
fn private_descriptor_helpers_are_not_namespace_apis_and_macros_expand_owned_forms() {
    let mut c = Context::new();
    assert_eq!(
        printed(&mut c, "(first (macroexpand-1 '(defprotocol P (f [x]))))"),
        "let*"
    );
    assert!(c.resolve_var(&Name::new(None, "P")).unwrap().is_none());
    for name in ["protocol-install*", "protocol-method*", "defprotocol*"] {
        assert!(
            c.resolve_var(&Name::new(Some("clojure.core"), name))
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        printed(&mut c, "(eval '(defprotocol P (f [x]))) (ifn? f)"),
        "true"
    );
    assert_eq!(printed(&mut c, "(f (reify P (f [x] 1)))"), "1");
}

#[test]
fn protocol_and_method_handles_have_owner_scoped_identity_and_reclaimed_generations() {
    let mut c = Context::new();
    c.eval_string("(defprotocol P (f [x]))").unwrap();
    let protocol = c.eval_string("P").unwrap();
    let method = c.eval_string("f").unwrap();
    let protocol_id = protocol.heap_id().unwrap();
    let method_id = method.heap_id().unwrap();
    assert_eq!(
        protocol.value_hash(),
        c.eval_string("P").unwrap().value_hash()
    );
    assert_eq!(
        method.value_hash(),
        c.eval_string("f").unwrap().value_hash()
    );
    assert_eq!(
        printed(
            &mut c,
            "[(get {P :protocol f :method} P) (get {P :protocol f :method} f)]"
        ),
        "[:protocol :method]"
    );
    let mut other = Context::new();
    other.eval_string("(defprotocol P (f [x]))").unwrap();
    assert!(!protocol.value_eq(&other.eval_string("P").unwrap()));
    assert!(!method.value_eq(&other.eval_string("f").unwrap()));
    assert!(matches!(
        other.heap_value(protocol_id),
        Err(HandleError::WrongOwner)
    ));
    assert!(matches!(
        other.heap_value(method_id),
        Err(HandleError::WrongOwner)
    ));
    let ns = c.current_namespace().namespace_id().unwrap();
    c.unmap_var(ns, &Name::new(None, "P")).unwrap();
    c.unmap_var(ns, &Name::new(None, "f")).unwrap();
    drop((protocol, method));
    c.collect_garbage();
    assert!(matches!(
        c.heap_value(protocol_id),
        Err(HandleError::StaleId)
    ));
    assert!(matches!(c.heap_value(method_id), Err(HandleError::StaleId)));
    c.eval_string("(defprotocol P (f [x]))").unwrap();
    assert_ne!(c.eval_string("P").unwrap().heap_id().unwrap(), protocol_id);
}

#[test]
fn reify_captures_multi_arity_partial_and_marker_implementations() {
    let mut c = Context::new();
    c.eval_string("(defprotocol P (p [x] [x n]) (q [x])) (defprotocol Marker)")
        .unwrap();
    assert_eq!(
        printed(
            &mut c,
            "(let [n 4 o (reify P (p [_] n) (p [_ x] (* n x)) Marker) f p] [(f o) (apply f [o 3]) ((var p) o) (satisfies? P o) (satisfies? Marker o) (try (q o) (catch Exception _ :missing))])"
        ),
        "[4 12 4 true true :missing]"
    );
    assert_eq!(
        printed(
            &mut c,
            "(def make (fn [n] (reify P (p [_] n)))) (let [a (make 1) b (make 2)] [(p a) (p b) (= a b) (= a a) (count (set [a b a]))])"
        ),
        "[1 2 false true 2]"
    );
    assert_eq!(
        printed(&mut c, "(satisfies? Marker (reify Marker))"),
        "true"
    );
    assert_eq!(
        printed(
            &mut c,
            "(let [o (reify P)] [(satisfies? P o) (try (p o) (catch Exception _ :missing))])"
        ),
        "[true :missing]"
    );
    assert_eq!(
        printed(&mut c, "(let [x 100 o (reify P (p [_ x] x))] (p o 9))"),
        "9"
    );
    assert_eq!(
        printed(&mut c, "(p (reify P (p [_ {:keys [n]}] n)) {:n 9})"),
        "9"
    );
    assert!(matches!(
        c.eval_string("(p (reify P (p [_] 1)) 2)").unwrap_err().kind,
        EvalErrorKind::Arity { actual: 2, .. }
    ));
}

#[test]
fn reify_protocol_keys_are_qualified_and_redeclarations_keep_existing_objects() {
    let mut c = Context::new();
    assert_eq!(
        printed(
            &mut c,
            "(ns a) (defprotocol P (f [x])) (ns b) (defprotocol P (f [x])) (ns user (:require [a :as aa] [b :as bb])) (def o (reify aa/P (f [_] :a) bb/P (f [_] :b))) [(aa/f o) (bb/f o)]"
        ),
        "[:a :b]"
    );
    assert_eq!(
        printed(
            &mut c,
            "(defprotocol P (p [x])) (def saved p) (def o (let [n 7] (reify P (p [_] n)))) (defprotocol P (p [x])) [(saved o) (p o) (satisfies? P o)]"
        ),
        "[7 7 true]"
    );
    assert_eq!(
        printed(&mut c, "(def p (fn [_] 9)) [(p o) (saved o)]"),
        "[9 7]"
    );
}

#[test]
fn reify_metadata_replacement_keeps_captures_but_creates_a_fresh_identity() {
    let mut c = Context::new();
    c.eval_string("(defprotocol P (p [x])) (def o (let [n [42]] (reify P (p [_] (first n)))))")
        .unwrap();
    assert_eq!(
        printed(
            &mut c,
            "(let [m (with-meta (with-meta o {:a 1 :b 3}) {:a 2}) z (with-meta m nil)] [(p m) (meta m) (= o m) (= m z) (meta z) (p z) (count (set [o m z]))])"
        ),
        "[42 {:a 2} false false nil 42 3]"
    );
    assert_eq!(
        printed(
            &mut c,
            "[(map? o) (seqable? o) (ifn? o) (coll? o) (pr-str o) (:a (meta ^{:a 1} (reify P)))]"
        ),
        "[false false false false \"#<Reify>\" 1]"
    );
    for source in ["(o)", "(seq o)", "(count o)"] {
        assert!(c.eval_string(source).is_err(), "{source}");
    }
    let o = c.eval_string("o").unwrap();
    let m = c.with_meta(&o, &c.nil()).unwrap();
    assert_eq!(o.inspect(), ValueView::Reify);
    assert!(!o.value_eq(&m));
    assert_eq!(o.value_hash(), o.clone().value_hash());
    assert_eq!(
        printed(&mut c, "[(get {o :found} o) (= o {}) (= {} o)]"),
        "[:found false false]"
    );
}

#[test]
fn reify_invalid_methods_have_qualified_diagnostics_and_source_provenance() {
    let mut c = Context::new();
    c.eval_string("(defprotocol P (p [x] [x y]))").unwrap();
    for (source, message) in [
        ("(reify P (unknown [_] 1))", "unknown protocol method"),
        (
            "(reify P (p [_] 1) (p [x] 2))",
            "duplicate protocol method arity",
        ),
        ("(reify P (p [] 1))", "undeclared protocol method arity"),
        (
            "(reify P (p [x y z] 1))",
            "undeclared protocol method arity",
        ),
        (
            "(reify P (p [x & xs] 1))",
            "variadic protocol implementations",
        ),
        ("(reify P (p x 1))", "method parameters"),
        ("(reify P (user/p [_] 1))", "unknown protocol method"),
        ("(reify P P)", "duplicate protocol implementation"),
    ] {
        let error = c.eval_string(source).unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains(message) && text.contains("user/P"),
            "{source}: {error}"
        );
        assert_eq!(error.span.text(), source);
        assert_eq!(error.phase, EvalPhase::Macroexpansion);
    }
    for source in [
        "(reify 42)",
        "(reify Object)",
        "(reify P ())",
        "(reify P [])",
    ] {
        assert!(c.eval_string(source).is_err(), "{source}");
    }
    let error = c.eval_string("(p (reify P (p [_] (/ 1 0))))").unwrap_err();
    assert!(error.to_string().contains("DivideByZero"), "{error}");
    assert_eq!(error.span.text(), "(/ 1 0)");
}

#[test]
fn reify_method_recur_and_language_exceptions_use_existing_execution() {
    let mut c = Context::new();
    c.eval_string("(defprotocol P (p [x n]))").unwrap();
    assert_eq!(
        printed(
            &mut c,
            "(let [o (reify P (p [this n] (if (zero? n) 7 (recur this (dec n)))))] (p o 1000))"
        ),
        "7"
    );
    assert_eq!(
        printed(
            &mut c,
            "(let [trace (atom []) o (reify P (p [_ n] (try (throw (ex-info \"method\" {:n n})) (finally (swap! trace conj :finally)))))] [(try (p o 3) (catch Exception e (:n (ex-data e)))) @trace])"
        ),
        "[3 [:finally]]"
    );
}

#[test]
fn reify_private_constructor_revalidates_tables_from_retained_expansions() {
    let mut c = Context::new();
    c.eval_string("(defprotocol P (p [x])) (def make-object (first (macroexpand-1 '(reify P))))")
        .unwrap();
    for source in [
        "(make-object nil [P 0 inc])",
        "(make-object nil [P 0 (fn [x y] y)])",
        "(make-object nil [P 0 (fn [x & ys] x)])",
        "(make-object nil [P 99 (fn [x] x)])",
        "(make-object nil [P 0 (fn [x] x) 0 (fn [x] x)])",
        "(make-object nil [P] [P])",
        "(make-object nil [P 0])",
        "(make-object nil [1])",
        "(make-object 1 [P])",
    ] {
        assert!(c.eval_string(source).is_err(), "{source}");
    }
    assert_eq!(
        printed(
            &mut c,
            "(p (make-object {:a 7} [P 0 (fn [this] (:a (meta this)))]))"
        ),
        "7"
    );
}
