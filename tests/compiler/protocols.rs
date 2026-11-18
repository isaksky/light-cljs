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

/// Slots and markers are keyed names (docs/value-layout.md, section 1):
/// source that names one as a member reaches it, as the analyzer keys the
/// member, and a string naming one sees nothing ("Decisions for the user"
/// 2, which rescoped these cases from `aget` and `aset` of the names).
#[test]
fn protocol_dispatch_uses_public_slots_and_native_tables() {
    assert_eq!(
        run(
            "(defprotocol Slashy (/ [_])) (let [v (reify Slashy (/ [_] 7))] (array (/ v) (fn? (.-cljs$user$Slashy$_SLASH_$arity$1 v)) (undefined? (aget v \"cljs$user$Slashy$_SLASH_$arity$1\"))))"
        ),
        "[7,true,true]\n"
    );
    assert_eq!(
        run(
            "(defprotocol P (f [this])) (let [v #js {} w #js {}] (set! (.-cljs$user$P$f$arity$1 v) (fn [self] (this-as receiver (identical? self receiver)))) (set! (.-cljs$user$P$ v) PROTOCOL_SENTINEL) (aset w \"cljs$user$P$f$arity$1\" (fn [_] true)) (aset w \"cljs$user$P$\" PROTOCOL_SENTINEL) (array (f v) (implements? P v) (satisfies? P v) (implements? P w) (satisfies? P w)))"
        ),
        "[true,true,true,false,false]\n"
    );
    assert_eq!(
        run(
            "(defprotocol P (f [this])) (aset f \"string\" (fn [x] (.toUpperCase x))) (aset P \"string\" true) (array (f \"ok\") (satisfies? P \"ok\") (implements? P \"ok\"))"
        ),
        "[\"OK\",true,false]\n"
    );
}

/// A dispatcher tests its receiver as upstream's `x != null` (light-cljs-1022):
/// nil and `undefined` take the `nil` table, and the other falsy values
/// (false, 0, the empty string, NaN) read their slot and take their type's
/// table, at every level, called directly and through the method's value, of
/// one arity and of several.
#[test]
fn dispatchers_send_nil_receivers_to_the_nil_table_and_falsy_ones_to_their_type() {
    assert_eq!(
        run("(defprotocol P (f [x]) (g [x] [x y]))
             (extend-protocol P
               nil (f [_] :nil) (g ([_] :nil) ([_ y] [:nil y]))
               boolean (f [_] :boolean) (g ([_] :boolean) ([_ y] [:boolean y]))
               number (f [_] :number) (g ([_] :number) ([_ y] [:number y]))
               string (f [_] :string) (g ([_] :string) ([_ y] [:string y])))
             (defn each [x] [(f x) (g x) (g x 1)])
             (def fs [f g])
             (pr-str (mapv (fn [x] [(each x) ((fs 0) x) ((fs 1) x 2)])
                           [nil js/undefined false 0 \"\" ##NaN]))"),
        "\"[[[:nil :nil [:nil 1]] :nil [:nil 2]] [[:nil :nil [:nil 1]] :nil [:nil 2]] \
         [[:boolean :boolean [:boolean 1]] :boolean [:boolean 2]] \
         [[:number :number [:number 1]] :number [:number 2]] \
         [[:string :string [:string 1]] :string [:string 2]] \
         [[:number :number [:number 1]] :number [:number 2]]]\"\n"
    );
}

/// Each arity of a method that `extend-type` gives in one form, `(m ([x] ...)
/// ([x y] ...))`, is dispatched on every host type, a host constructor, a
/// program's types, a runtime type and `specify!`, called directly and
/// through the method's value, at every level (light-cljs-1063). A method's
/// arities in forms of their own are a shape upstream warns of
/// (`warnings::extend_type_warns_of_ungrouped_method_arities`), and the
/// answers stay upstream's, as shadow-cljs 3.5.3 printed them: a host type's
/// table keeps the last form, so a one-argument call runs the two-argument
/// method (`##NaN`), and a prototype each form's arity.
#[test]
fn extend_type_dispatches_each_grouped_arity_on_host_types() {
    let types = "(deftype T []) (defrecord R [])";
    let targets = [
        ("number", "4"),
        ("string", "\"s\""),
        ("boolean", "true"),
        ("nil", "nil"),
        ("array", "(array)"),
        ("function", "inc"),
        ("object", "#js {}"),
        ("default", "(js/Symbol \"q\")"),
        ("js/Date", "(js/Date. 0)"),
        ("T", "(T.)"),
        ("R", "(R.)"),
        ("PersistentVector", "[1]"),
    ];
    let extensions = |grouped: bool| {
        targets
            .iter()
            .map(|(target, _)| {
                let tag = target.trim_start_matches("js/");
                if grouped {
                    format!("(extend-type {target} P (m ([x] [:{tag} 1]) ([x y] [:{tag} 2])))")
                } else {
                    format!("(extend-type {target} P (m [x] [:{tag} 1]) (m [x y] [:{tag} 2]))")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let values = targets
        .iter()
        .map(|(_, value)| *value)
        .collect::<Vec<_>>()
        .join(" ");
    // Each value's `[(m x) (m x 0) (f x) (f x 0)]` when `(m x)` answers
    // `[:tag one]`.
    let answers = |tags: &[&str], one: u8| {
        tags.iter()
            .map(|tag| format!("[[:{tag} {one}] [:{tag} 2] [:{tag} {one}] [:{tag} 2]]"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let hosts = [
        "number", "string", "boolean", "nil", "array", "function", "object", "default",
    ];
    let prototypes = ["Date", "T", "R", "PersistentVector"];
    let grouped = format!(
        "(defprotocol P (m [x] [x y])) {types}
         {}
         (extend-type number ILookup (-lookup ([x k] [:lookup k]) ([x k nf] [:lookup k nf])))
         (def xs [{values} {{}} (specify! #js {{}} P (m ([x] [:specify 1]) ([x y] [:specify 2])))])
         (def f m)
         (pr-str [(mapv (fn [x] [(m x) (m x 0) (f x) (f x 0)]) xs) (get 4 :a) (get 4 :a :nf)])",
        extensions(true)
    );
    // A map takes the `object` table (docs/value-layout.md, section 3).
    assert_eq!(
        run(&grouped),
        format!(
            "\"[[{} {} {}] [:lookup :a] [:lookup :a :nf]]\"\n",
            answers(&hosts, 1),
            answers(&prototypes, 1),
            answers(&["object", "specify"], 1)
        )
    );
    let ungrouped = format!(
        "(defprotocol P (m [x] [x y])) {types}
         (deftype V [] P (m [x] [:V 1]) (m [x y] [:V 2]))
         {}
         (defprotocol Q (q [x] [x y]))
         (extend-type number Q (q [x] (inc x)) (q [x y] (+ x y 100)))
         (def xs [{values} (V.) (specify! #js {{}} P (m [x] [:specify 1]) (m [x y] [:specify 2]))])
         (def f m)
         (pr-str [(mapv (fn [x] [(m x) (m x 0) (f x) (f x 0)]) xs) [(q 4) (q 4 1)]])",
        extensions(false)
    );
    assert_eq!(
        run(&ungrouped),
        format!(
            "\"[[{} {} {}] [##NaN 105]]\"\n",
            answers(&hosts, 2),
            answers(&prototypes, 1),
            answers(&["V", "specify"], 1)
        )
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
    // one extendable through metadata keep the shared one. Each names its
    // slots, as the text spells them (light-cljs-1058). Nothing assigns
    // these Vars, so the dispatcher is the function itself, which no factory
    // makes at load, and the rest of its dispatch its `protocolMiss`, and no
    // getter reads the Var (light-cljs-1069).
    for expected in [
        "\"cljs.user/f\", [1], false, null, null, function (r0) { return r0 != null && r0[\"cljs$user$P$f$arity$1$",
        "\"](r0) : g1[\"protocolMiss$",
        "\"cljs.user/g\", [1, 2], false, null, null, function (r0, a1 = undefined) { if (arguments.length === 1) { if (r0 != null && r0[\"cljs$user$P$g$arity$1$",
        "\"](r0, a1); } return g2[\"protocolMiss$",
        "\"].apply(null, arguments); }, [\"cljs$user$P$g$arity$1$",
        "\"cljs.user/h\", [3], false, 1, null, null, [\"cljs$user$P$h$arity$3$",
        "\"cljs.user/m\", [1], true, null, null, null, [\"cljs$user$M$m$arity$1$",
    ] {
        assert!(js.contains(expected), "{expected}\n{js}");
    }
    assert_eq!(js.matches("=> function (r0").count(), 0, "{js}");
    assert_eq!(js.matches("\"], true)").count(), 2, "{js}");
}

/// A dispatcher calls the slot of the call's arity that a host getter gives,
/// reads a slot that holds nothing once before the `object` implementation,
/// and keeps the shared dispatcher's `length`. How often it reads a slot that
/// holds a method is not part of the contract (docs/advanced-optimization.md).
/// A slot is a keyed name (docs/value-layout.md, section 1), so the getter's
/// name is the one a member write gives (`slot`): a string can no longer
/// name it ("Decisions for the user" 2).
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
             (defn slot [write] (let [o #js {}] (write o) (aget (js-keys o) 0)))
             (defn call [slot value call]
               (reset! reads 0)
               (let [result (str (call (host slot value)))] (if value result (array result @reads))))
             (let [f1 (slot #(set! (.-cljs$user$P$f$arity$1 %) nil))
                   g1 (slot #(set! (.-cljs$user$P$g$arity$1 %) nil))
                   g2 (slot #(set! (.-cljs$user$P$g$arity$2 %) nil))
                   h3 (slot #(set! (.-cljs$user$P$h$arity$3 %) nil))]
             (array (call f1 (fn [_] 42) #((fs 0) %))
                    (call f1 nil #((fs 0) %))
                    (call g1 (fn [_] 1) #((fs 1) %))
                    (call g2 (fn [_ y] (* 2 y)) #((fs 1) % 3))
                    (call g2 nil #((fs 1) % 3))
                    (call h3 (fn [_ y z] (+ y z)) #((fs 2) % 3 4))
                    (call h3 nil #((fs 2) % 3 4))
                    (.-length f) (.-length g) (.-length h)))"
        ),
        "[\"42\",[\":default\",1],\"1\",\"6\",[\"3\",1],\"7\",[\"4\",1],1,1,1]\n"
    );
}

/// A program protocol's `object` and `default` tables cover the runtime's
/// values as they did before the class layout (docs/value-layout.md,
/// section 3): a vector, map entry, subvector or transient vector, now a
/// class value, reaches `object` (its type to `satisfies?`), then `default`,
/// as maps, seqs and keywords do, while strings, numbers, booleans, nil,
/// arrays and functions reach only `default`; `satisfies?` sees the tables
/// and `implements?` does not, unless the class itself implements the
/// protocol. The expected text is what master `f533efb1` prints at every
/// level.
#[test]
fn user_protocol_tables_cover_runtime_values() {
    let values = "[[1 2] (first {:a 1}) (subvec [1 2 3] 1) (transient [1]) {:a 1} (sorted-map 1 2) #{1} (list 1) () (seq [1 2]) (lazy-seq [1]) (range 3) :k 'sym \"s\" 1 true nil (array 1) (js-obj) inc]";
    let row = |answer: &str| format!("[{answer} true false]");
    let rows = |runtime: &str, host: &str, vector: Option<&str>| {
        let mut rows = Vec::new();
        for i in 0..14 {
            rows.push(match (i, vector) {
                (0, Some(vector)) => vector.to_owned(),
                _ => row(runtime),
            });
        }
        for i in 14..21 {
            rows.push(match i {
                19 => row(runtime),
                _ if host == ":error" => "[:error false false]".to_owned(),
                _ => row(host),
            });
        }
        format!("\"[{}]\"\n", rows.join(" "))
    };
    for (extension, expected) in [
        (
            "(extend-type object P (f [_] :object))",
            rows(":object", ":error", None),
        ),
        (
            "(extend-type default P (f [_] :default))",
            rows(":default", ":default", None),
        ),
        (
            "(extend-type object P (f [_] :object)) (extend-type default P (f [_] :default))",
            rows(":object", ":default", None),
        ),
        (
            "(extend-protocol P object (f [_] :object) PersistentVector (f [_] :vector) default (f [_] :default))",
            rows(":object", ":default", Some("[:vector true true]")),
        ),
    ] {
        assert_eq!(
            run(&format!(
                "(defprotocol P (f [this])) {extension}
                 (pr-str (mapv (fn [v] [(try (f v) (catch :default _ :error)) (satisfies? P v) (implements? P v)]) {values}))"
            )),
            expected,
            "{extension}"
        );
    }
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

/// Dereferencing a value without `IDeref` names the protocol method, as
/// upstream's `missing-protocol` does, so code matching the message (re-frame's
/// own tests expect `#"No protocol method IDeref"`) sees it.
#[test]
fn deref_of_a_value_without_ideref_names_the_protocol_method() {
    let missing = "\"No protocol method IDeref.-deref defined for this value\"";
    assert_eq!(
        run(
            "(into-array (map (fn [x] (try @x (catch :default e (.-message e)))) [nil 1 [1] (js-obj)]))"
        ),
        format!("[{missing},{missing},{missing},{missing}]\n")
    );
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
