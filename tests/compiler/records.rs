//! Literal host observations avoid flattening nominal records into encoder maps.
use light_cljs::compile;

/// Every optimization level prints what `None` prints.
fn run(source: &str) -> String {
    super::optimization::json_at_every_level(source)
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

/// light-cljs-266: a record finds a field by its keyword from a table built
/// once per type, by identity, so a keyword equal to a field's but made by
/// `keyword` still names the field, a string or symbol does not, and other
/// keys fall back to the extmap. The atom hides the type from the optimizer.
#[test]
fn record_fields_are_found_by_equal_keywords_and_other_keys_by_the_extmap() {
    assert_eq!(
        run(
            "(defrecord R [a b-c d?])
             (let [r @(atom (assoc (->R 1 2 3) :e 4 \"a\" 5)) fresh (keyword \"a\") bc (keyword \"b-c\")]
               (array (:a r) (get r :b-c) (:d? r) (fresh r) (get r bc) (contains? r (keyword \"d?\"))
                      (get r \"a\") (get r :e) (:zz r 6) (contains? r :zz) (contains? r 'a) (get r nil 7)
                      (:a (assoc r fresh 8)) (count (dissoc r (keyword \"e\"))) (record? (dissoc r bc))
                      (pr-str r) (= r (map->R {fresh 1 bc 2 :d? 3 :e 4 \"a\" 5}))
                      (pr-str (map->R {bc 9 (keyword \"z\") 10})) (= (hash r) (hash (assoc r fresh 1)))))"
        ),
        "[1,2,3,1,2,true,5,4,6,false,false,7,8,4,false,\
         \"#cljs.user.R{:a 1, :b-c 2, :d? 3, :e 4, \\\"a\\\" 5}\",true,\
         \"#cljs.user.R{:a nil, :b-c 9, :d? nil, :z 10}\",true]\n"
    );
}

/// A record's `(-lookup r k)` is `(-lookup r k nil)` through whatever
/// three-argument method the record has, as upstream's is.
#[test]
fn record_two_argument_lookup_goes_through_a_replaced_three_argument_one() {
    assert_eq!(
        run("(defrecord R [a]) (defrecord S [a])
             (extend-type S ILookup (-lookup ([_ k nf] (str \"s\" (name k) nf))))
             (let [r @(atom (->R 1)) s @(atom (->S 1))]
               (array (:a r) (get r :a) (get r :b 2) (:a s) (get s :a) (get s :b 2)))"),
        "[1,1,2,\"sa\",\"sa\",\"sb2\"]\n"
    );
}

/// A record remembers the answer for the first 32 keywords it is asked about
/// by identity: a remembered miss is still found in the extmap once assoc
/// puts it there, and once 32 are remembered a field's own keyword, a keyword
/// equal to it made by `keyword` and a key that only the extmap holds, none
/// of them remembered, are still found, by `get`, `contains?` and `assoc`.
#[test]
fn record_field_misses_are_remembered_without_hiding_the_extmap() {
    assert_eq!(
        run("(defrecord R [a b])
             (let [r @(atom (->R 1 2)) ks (mapv #(keyword (str \"k\" %)) (range 40))
                   misses (count (remove #(contains? r %) (concat ks ks)))
                   r2 (assoc r (nth ks 3) 30 :k39 31 :zz 32)]
               (array misses (get r (nth ks 3) 9) (get r2 (nth ks 3)) (:k39 r2) (contains? r2 :k39)
                      (contains? r2 (nth ks 4)) (:zz r2) (contains? r2 :zz) (get r2 :b) (contains? r2 :a)
                      ((keyword \"a\") r2) (get r2 (keyword \"b\")) (contains? r2 (keyword \"b\"))
                      (:a (assoc r2 (keyword \"a\") 5)) (:b (assoc r2 :b 6)) (count (assoc r2 :a 7))
                      (:yy r2 8) (count r2)))"),
        "[80,9,30,31,true,false,32,true,2,true,1,2,true,5,6,5,8,5]\n"
    );
}

/// `contains?` of a field's keyword answers from the field without reading
/// the extmap, as it did before the nil-extmap answer.
#[test]
fn record_contains_of_a_field_does_not_read_the_extmap() {
    assert_eq!(
        run("(defrecord R [a])
             (let [r @(atom (->R 1))]
               (js/Object.defineProperty r \"__extmap\" #js {:get (fn [] (throw (js/Error. \"read\")))})
               (array (contains? r :a) (contains? r (keyword \"a\"))))"),
        "[true,true]\n"
    );
}

/// A record without an extmap answers `get` and `contains?` of a key that
/// names no field itself, unless `extend-type` gave nil `-contains-key?`,
/// which `contains?` of the missing extmap then calls, at every level.
#[test]
fn record_without_extmap_consults_a_nil_extension_of_contains() {
    assert_eq!(
        run("(defrecord R [a])
             (let [r @(atom (->R 1))] (array (get r :x 5) (:x r 6) (nil? (:x r)) (contains? r :x) (contains? r :a)))"),
        "[5,6,true,false,true]\n"
    );
    assert_eq!(
        run("(extend-type nil IAssociative (-contains-key? [_ k] (= k :x)) (-assoc [_ k v] nil))
             (defrecord R [a])
             (let [r @(atom (->R 1))] (array (contains? r :x) (contains? r :y) (contains? nil :x) (get r :x 5)))"),
        "[true,false,true,5]\n"
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

/// Since light-cljs-258 the constructor assigns each field, under its munged
/// name, as an own data property that `set!` updates in place, instead of an
/// accessor over per-instance storage. (The masks that follow them are the
/// instance's own only below `Advanced`, see the next test.)
#[test]
fn type_instances_hold_fields_as_own_data_properties() {
    assert_eq!(
        run(
            "(defrecord R [a b?]) (deftype T [x ^:mutable y-z] Object (bump [_] (set! y-z (inc y-z)) y-z))
             (let [r (->R 1 2) t (T. 3 4)
                   d (fn [o k] (let [p (js/Object.getOwnPropertyDescriptor o k)] (array (.-value p) (.-writable p) (.-enumerable p) (fn? (.-get p)))))
                   fields (fn [o] (.filter (js/Object.keys o) (fn [k] (not (.startsWith k \"cljs$lang$protocol_mask\")))))]
               (.bump t)
               (array (d r \"a\") (d r \"b_QMARK_\") (d r \"__extmap\") (d t \"x\") (d t \"y_z\") (.-y-z t)
                      (fields r) (fields t) (= r (->R 1 2)) (= (assoc r :a 3) (->R 3 2))))"
        ),
        "[[1,true,true,false],[2,true,true,false],[null,true,true,false],[3,true,true,false],[5,true,true,false],5,\
         [\"a\",\"b_QMARK_\",\"__meta\",\"__extmap\",\"__hash\"],[\"x\",\"y_z\"],true,true]\n"
    );
}

/// light-cljs-264: at `Advanced` the masks of a `deftype` or `defrecord`
/// type are its prototype's, so an instance's own keys are its fields and a
/// record's three slots; lower levels give every instance its masks after
/// them, as upstream's constructor does. Reading a mask gives the same value.
/// The masks are keyed names (docs/value-layout.md, section 1): the keys
/// they list are compared with the build's key normalised, and `own` names
/// the mask in raw JavaScript, which the output writer keys.
#[test]
fn advanced_types_hold_their_masks_on_the_prototype() {
    let source = "(defrecord R [a]) (deftype T [x] ICounted (-count [_] 1))
                  (let [r (->R 1) t (T. 2)
                        own (fn [o] (.hasOwnProperty o (js* \"'cljs$lang$protocol_mask$partition0$$LKEY'\")))]
                    (array (js/Object.keys r) (js/Object.keys t) (.-cljs$lang$protocol_mask$partition0$ t)
                           (own t) (own (js/Object.getPrototypeOf t)) (count t) (count r)))";
    let program = light_cljs::analyze(source, &light_cljs::parse(source).unwrap()).unwrap();
    let json = |level| {
        let js =
            light_cljs::prepare_application(&program, light_cljs::ApplicationOptions::new(level))
                .emit_js();
        let key = light_cljs::runtime_key::RuntimeKey::written(&js).expect("a runtime");
        key.unapply(&super::optimization::execute(&format!(
            "console.log(JSON.stringify({js}));"
        )))
    };
    let masks =
        "\"cljs$lang$protocol_mask$partition0$$LKEY\",\"cljs$lang$protocol_mask$partition1$$LKEY\"";
    for level in [
        light_cljs::OptimizationLevel::None,
        light_cljs::OptimizationLevel::Prepare,
    ] {
        assert_eq!(
            json(level),
            format!(
                "[[\"a\",\"__meta\",\"__extmap\",\"__hash\",{masks}],[\"x\",{masks}],2,true,false,1,1]\n"
            ),
            "at {level}"
        );
    }
    assert_eq!(
        json(light_cljs::OptimizationLevel::Advanced),
        "[[\"a\",\"__meta\",\"__extmap\",\"__hash\"],[\"x\"],2,false,true,1,1]\n"
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

/// Diagnostic only: preserve the original full record-tag source and failures.
/// No semantic or macro execution credit and no replacement tag reader.
mod original_record_tags {
    use light_cljs::compiler_data::Source;
    use light_cljs::source_provider::SourceInput;
    use light_cljs::{EvalOptions, EvalSession, MacroHost, compile, compile_namespace_with_macros};
    use std::io::Write;

    #[test]
    #[ignore = "explicit unchanged record-tag diagnostic; no completion credit"]
    fn unchanged_record_tag_namespace_records_current_read_failures() {
        let source = include_str!(
            "../../research/clojurescript/src/test/cljs/data_readers_test/records.cljc"
        );
        let root = std::path::PathBuf::from(std::env::var("LIGHT_RECORD_TAGS_ARTIFACTS").unwrap());
        let fixture = root.join("source/data_readers_test/records.cljc");
        std::fs::create_dir_all(fixture.parent().unwrap()).unwrap();
        std::fs::write(&fixture, source).unwrap();
        let mut output = std::fs::File::create(root.join("observations.tsv")).unwrap();
        let reader_error = compile(source).expect_err("Original record reader diagnostic changed");
        writeln!(output, "direct-compiler\tfresh\t{reader_error}").unwrap();
        let first_assertion = source
            .lines()
            .find(|line| line.starts_with("(assert (= (Foo. 1 2) #"))
            .unwrap();
        let form_error = MacroHost::new()
            .read_compiler_form(&Source::new("records.cljs", first_assertion))
            .unwrap_err();
        assert!(form_error.message.contains("UnknownTag"), "{form_error}");
        std::fs::write(root.join("original-first-assertion.cljs"), first_assertion).unwrap();
        writeln!(
            output,
            "original-first-assertion-reader\tfresh\t{form_error}"
        )
        .unwrap();
        for reused in [false, true] {
            let lifecycle = if reused { "reused" } else { "fresh" };
            #[cfg(feature = "quickjs")]
            {
                let mut session = EvalSession::new(EvalOptions {
                    namespace: "data-readers-test.records".into(),
                    ..Default::default()
                })
                .unwrap();
                if reused {
                    let warmup = session
                        .eval_string("(defmacro initialized [] :warm) (initialized)")
                        .unwrap();
                    session.release(warmup.value).unwrap();
                }
                let error = match session.eval_source(&SourceInput::new("records.cljc", source)) {
                    Err(error) => error.to_string(),
                    Ok(result) => {
                        session.release(result.value).unwrap();
                        panic!(
                            "Diagnostic expectation changed; independently re-review original execution"
                        );
                    }
                };
                assert!(
                    error.contains(":require-macros is retired"),
                    "unexpected original namespace boundary: {error}"
                );
                writeln!(output, "application\t{lifecycle}\t{error}").unwrap();
            }
            let mut host = MacroHost::new();
            if reused {
                let warm = root.join("warm");
                std::fs::create_dir_all(&warm).unwrap();
                std::fs::write(
                    warm.join("warm.cljs"),
                    "(ns warm) (defmacro initialized [] :warm) (initialized)",
                )
                .unwrap();
                let js = compile_namespace_with_macros("warm", &[warm], &mut host).unwrap();
                std::fs::write(root.join("warmup.js"), js).unwrap();
            }
            let error = compile_namespace_with_macros(
                "data-readers-test.records",
                &[root.join("source")],
                &mut host,
            )
            .expect_err("Diagnostic expectation changed; re-review exact original phase proof");
            writeln!(output, "macro-host-compiler\t{lifecycle}\t{error}").unwrap();
        }
        // The sole documented adapter removes the retired self :require-macros
        // namespace declaration. Every original record declaration/assertion and
        // quoted field stays byte-identical. This diagnostic grants no original
        // source pass; it isolates the reader failure behind the namespace boundary.
        let old_namespace = "(ns data-readers-test.records\n  #?(:cljs (:require-macros [data-readers-test.records])))";
        assert_eq!(source.matches(old_namespace).count(), 1);
        let adapted = source.replacen(old_namespace, "(ns data-readers-test.records)", 1);
        std::fs::write(root.join("namespace-adapted-records.cljs"), &adapted).unwrap();
        for reused in [false, true] {
            let lifecycle = if reused { "reused" } else { "fresh" };
            #[cfg(feature = "quickjs")]
            {
                let mut session = EvalSession::new(EvalOptions {
                    namespace: "data-readers-test.records".into(),
                    ..Default::default()
                })
                .unwrap();
                if reused {
                    let warm = session
                        .eval_string("(defmacro initialized [] :warm) (initialized)")
                        .unwrap();
                    session.release(warm.value).unwrap();
                }
                let error = match session.eval_source(&SourceInput::new(
                    "namespace-adapted-records.cljs",
                    &adapted,
                )) {
                    Err(error) => error.to_string(),
                    Ok(result) => {
                        session.release(result.value).unwrap();
                        panic!("Namespace-adapted diagnostic changed");
                    }
                };
                assert!(error.contains("UnknownTag"), "{error}");
                writeln!(
                    output,
                    "namespace-adapted-application\t{lifecycle}\t{error}"
                )
                .unwrap();
            }
        }
        output.flush().unwrap();
    }
}

/// light-cljs-264: at `Advanced` a type's methods read their fields from
/// `this` and name their receiver only where they use it, a record defined
/// twice keeps copying old instances into the current definition, and a
/// type whose field name holds a space passes its names as an array; every
/// level gives what `None` gives.
#[test]
fn per_type_constructors_keep_receivers_redefinitions_and_field_names() {
    assert_eq!(
        run(
            "(defprotocol Shape (area [s]) (scale [s k]) (parts [s]))
             (defrecord Rect [w h] Shape (area [_] (* w h)) (scale [this k] (assoc this :w (* k w)))
               (parts [_] (mapv (fn [f] (f)) [(fn [] w) (fn [] h)])))
             (def old (->Rect 2 3))
             (defrecord Rect [w h d] Shape (area [_] (* w h d)) (scale [this k] this) (parts [_] [w h d]))
             (deftype Box [^:mutable n] Shape (area [this] (set! n (inc n)) (.-n this)) (scale [_ k] (Box. (* k n)))
               (parts [_] (let [g (fn [] n)] [(g)])))
             (defmacro spaced [] (list 'defrecord 'Sp [(symbol \"a b\")]))
             (spaced)
             (let [b (Box. 1) s (->Sp 5)]
               (array (area old) (area (scale old 2)) (= (type (scale old 2)) Rect) (into-array (parts old))
                      (area (->Rect 1 2 3)) (area b) (area b) (.-n (scale b 3)) (into-array (parts b))
                      (pr-str s) (get s (keyword \"a b\")) (.-fieldNames Sp)))"
        ),
        "[6,0,true,[2,3],6,2,3,9,[3],\"#cljs.user.Sp{:a b 5}\",5,[\"a b\"]]\n"
    );
}

/// light-cljs-264: at `Advanced` a type's definition is statements of the
/// function around it. Its constructor is its own, which assigns the fields
/// and brands nothing (light-cljs-1037: `defineType` gives its prototype the
/// keyed kind that tells its instances, where a `TypeBrand` or, since
/// light-cljs-346, a class's private field branded them), one string holds
/// the type's and the fields' names, a method entry names the protocol
/// method's dispatcher, the method reads its fields from `this`, and a
/// positional factory nothing reads is not defined.
#[test]
fn advanced_types_define_their_own_constructors() {
    let source = "(defprotocol Area (area [s]))
                  (defrecord Rect [w h] Area (area [_] (* w h)))
                  (deftype T [a] Area (area [_] a))
                  (into-array (map area [(->Rect 2 3) (T. 4)]))";
    let program = light_cljs::analyze(source, &light_cljs::parse(source).unwrap()).unwrap();
    let js = light_cljs::prepare_application(
        &program,
        super::optimization::readable(light_cljs::OptimizationLevel::Advanced),
    )
    .emit_js();
    for expected in [
        " = rt$defineType(function Type(a0, a1, a2, a3, a4) { this[\"w\"] = a0; this[\"h\"] = a1; \
         this[\"__meta\"] = a2; this[\"__extmap\"] = a3; this[\"__hash\"] = a4; }, \
         \"cljs.user/Rect w h\", [[g",
        ", 1, function() {\n    return (this[\"w\"] * this[\"h\"]);\n  }]], 1); var g",
        " = rt$defineType(function Type(a0) { this[\"a\"] = a0; }, \"cljs.user/T a\", [[g",
        ", 1, function() {\n    return this[\"a\"];\n  }]]); ",
        " = (a0, a1) => new g",
    ] {
        assert!(js.contains(expected), "{expected}\n{js}");
    }
    let definitions = &js[js.find("rt$defineType(function Type").unwrap()..];
    for unexpected in [
        "(() => { g",
        "const self",
        "(...args) => new",
        "rt$TypeBrand",
        "defineClass(",
    ] {
        assert!(!definitions.contains(unexpected), "{unexpected}\n{js}");
    }
    assert_eq!(run(source), "[6,4]\n");
}

/// light-cljs-1037: at `Advanced` no type's constructor brands its instance,
/// whatever the output reads (light-cljs-262 branded where a unit read
/// brands, and light-cljs-346 wrote a class with a private field of its own
/// where every constructor ran only through `new`): a record and a type are
/// told by their prototypes' keyed kinds, so updating, printing and
/// comparing a record, and reading a type other than with `new`, keep no
/// `TypeBrand`, `ClassBrand` or `defineClass`.
#[test]
fn advanced_constructors_brand_no_instance() {
    let advanced = |source: &str| {
        let program = light_cljs::analyze(source, &light_cljs::parse(source).unwrap()).unwrap();
        light_cljs::prepare_application(
            &program,
            super::optimization::readable(light_cljs::OptimizationLevel::Advanced),
        )
        .emit_js()
    };
    for (source, expected) in [
        (
            "(defprotocol Area (area [s]))
             (defrecord Rect [w h] Area (area [_] (* w h)))
             (deftype T [a] Area (area [_] a))
             (+ (area (->Rect 2 3)) (area (T. 4)))",
            "10\n",
        ),
        (
            "(defrecord R [a]) (deftype T [x])
             (let [r (->R 1)]
               (array (record? (assoc r :a 2)) (pr-str (assoc r :b 2)) (= r (->R 1)) (.-x (T. 3))))",
            "[true,\"#cljs.user.R{:a 1, :b 2}\",true,3]\n",
        ),
        (
            "(defrecord R [a]) (deftype T [x])
             (let [r (->R 1)]
               (array (instance? R r) (pr-str (assoc r :b 2)) (.-x (T. 3))))",
            "[true,\"#cljs.user.R{:a 1, :b 2}\",3]\n",
        ),
    ] {
        let js = advanced(source);
        assert!(
            js.contains(
                "function Type(a0, a1, a2, a3) { this[\"a\"] = a0; this[\"__meta\"] = a1; \
                 this[\"__extmap\"] = a2; this[\"__hash\"] = a3; }, "
            ) || js.contains(
                "function Type(a0, a1, a2, a3, a4) { this[\"w\"] = a0; this[\"h\"] = a1; \
                 this[\"__meta\"] = a2; this[\"__extmap\"] = a3; this[\"__hash\"] = a4; }, "
            ),
            "{js}"
        );
        for unexpected in [
            "rt$TypeBrand",
            "const TypeBrand",
            "const ClassBrand",
            "function defineClass",
            "#b;",
        ] {
            assert!(!js.contains(unexpected), "{unexpected}\n{js}");
        }
        assert_eq!(run(source), expected);
    }
}

/// A type's constructor called as a plain function on an existing instance
/// assigns its fields again, as upstream's does, at every level: no brand
/// rejects a second initialization since light-cljs-1037 (light-cljs-262
/// threw where a brand was written). Calling it on a host object makes no
/// instance: the host object keeps its prototype, which holds no kind.
#[test]
fn constructors_called_without_new_only_assign_fields() {
    assert_eq!(
        run("(deftype T [x]) (defrecord R [a])
             (let [t (T. 1) r (->R 1) host (js-obj)
                   again (fn [f] (try (f) false (catch :default _ true)))]
               (array (again #(.call T t 2)) (again #(.call R r 2 nil nil nil)) (.-x t) (.-a r)
                      (again #(.call T host 5)) (.-x host) (instance? T host) (record? r) (pr-str r)))"),
        "[false,false,2,2,false,5,false,true,\"#cljs.user.R{:a 2}\"]\n"
    );
}

/// A `defrecord` in a function defines a new type at each call, so a copy of
/// an instance of an earlier one (`assoc`, `clone`, `with-meta`) is of the
/// current type at every level: light-cljs-264 omits the record's
/// `currentConstructor` only for a top-level definition.
#[test]
fn nested_record_redefinitions_copy_to_the_current_constructor() {
    assert_eq!(
        run(
            "(defn make [] (defrecord R [x]) [->R R])
             (def old ((first (make)) 1))
             (def current (second (make)))
             (array (identical? (type (assoc old :x 2)) current) (identical? (type (clone old)) current)
                    (identical? (type (with-meta old {:m 1})) current) (identical? (type old) current)
                    (:x (assoc old :x 2)))"
        ),
        "[true,true,true,false,2]\n"
    );
    // A program that reads no `R` as a value keeps its `currentConstructor`
    // at `Advanced` too (light-cljs-346 wrote it as a class there).
    let source = "(defn make [x] (defrecord R [a]) (->R x))
                  (let [[old current] (mapv make [1 2])]
                    (array (identical? (type (assoc old :a 3)) (type current))
                           (identical? (type (clone old)) (type current))
                           (identical? (type (with-meta old {:m 1})) (type current))
                           (identical? (type old) (type current)) (pr-str (assoc old :a 3))))";
    let program = light_cljs::analyze(source, &light_cljs::parse(source).unwrap()).unwrap();
    let js = light_cljs::prepare_application(
        &program,
        super::optimization::readable(light_cljs::OptimizationLevel::Advanced),
    )
    .emit_js();
    assert!(js.contains("rt$defineType(function Type("), "{js}");
    assert_eq!(
        run(source),
        "[true,true,true,false,\"#cljs.user.R{:a 3}\"]\n"
    );
}

/// `get` and `contains?` call the slot of a value whose type declares the
/// protocol by its mask bit themselves (`get.own`, `contains.own`,
/// light-cljs-358): a record's, a type's, one that `extend-type` replaced and
/// one that `specify!` gave an instance. Maps, vectors and nil in the same
/// program take the operations.
#[test]
fn get_and_contains_call_declared_slots_and_their_replacements() {
    assert_eq!(
        run("(defrecord R [a]) (defrecord S [a])
             (deftype T [] ILookup (-lookup [_ k] (str \"t\" (name k))) (-lookup [_ k nf] (str \"t\" (name k) nf)))
             (extend-type S IAssociative (-contains-key? [_ k] (= k :zz)) (-assoc [s _ _] s))
             (let [r (->R 1) t (T.) s (->S 1) u (specify! (->R 2) ILookup (-lookup ([_ _] \"u\") ([_ _ nf] (str \"u\" nf))))]
               (array (get r :a) (get r :b 3) (:a r) (contains? r :a) (contains? r :b) (get t :x) (get t :x 4) (:x t)
                      (contains? s :a) (contains? s :zz) (get u :a) (get u :a 5) (:a u)
                      (get {:a 6} :a) (get [7] 0) (contains? {:a 1} :a) (contains? [1] 0) (get nil :a 8) (contains? nil :a)))"),
        "[1,3,1,true,false,\"tx\",\"tx4\",\"tx\",false,true,\"u\",\"u5\",\"u\",6,7,true,true,8,false]\n"
    );
}

/// light-cljs-267: at `Advanced` a record type whose program compares or
/// hashes values has its own `-equiv` and `-hash`, which read its fields
/// directly and add constant keyword and type-name hashes. They give what
/// the shared methods give at `None`: the same hash values (also of fields
/// with munged names, of no fields and of an extension map), the hash cache
/// a copy keeps, nominal equality, and keys and members found in maps and
/// sets. An `extend-type` given later replaces them. A program that never
/// compares or hashes keeps the shared methods and writes none, though it
/// builds a vector, which keeps `equalPair`.
#[test]
fn advanced_records_compare_and_hash_with_their_own_methods() {
    let advanced = |source: &str| {
        let program = light_cljs::analyze(source, &light_cljs::parse(source).unwrap()).unwrap();
        light_cljs::prepare_application(
            &program,
            super::optimization::readable(light_cljs::OptimizationLevel::Advanced),
        )
        .emit_js()
    };
    let source = "(defrecord P [x y]) (defrecord Q [x y]) (defrecord E []) (defrecord M [foo-bar a? __proto__])
                  (defrecord V [v]) (extend-type V IEquiv (-equiv [_ _] true))
                  (let [p (->P 1 [2 :k]) m (->M \"s\" nil (->P 1 2)) x (assoc p :z {:w 1})
                        keys (into {} (map (fn [r] [r (pr-str r)])) [p (->Q 1 [2 :k]) (->E) m x])]
                    (array (hash p) (hash (->Q 1 [2 :k])) (hash (->E)) (hash m) (hash x) (hash (with-meta x {:m 1}))
                           (= p (->P 1 [2 :k])) (= p (->Q 1 [2 :k])) (= p (->P 1 [2 :j])) (= p nil) (= p 5)
                           (= p {:x 1 :y [2 :k]}) (= x (assoc (->P 1 [2 :k]) :z {:w 1})) (= x p) (= (->E) (->E))
                           (= m (->M \"s\" nil (->P 1 2))) (= (->P js/NaN 1) (->P js/NaN 1))
                           (get keys (->M \"s\" nil (->P 1 2))) (get keys (assoc (->P 1 [2 :k]) :z {:w 1}))
                           (get keys (->P 1 [2 :j]) -1) (count (into #{} [p (->P 1 [2 :k]) (->E) (->E)]))
                           (contains? #{(->E) m} (->M \"s\" nil (->P 1 2))) (= (->V 1) (->V 2))))";
    let js = advanced(source);
    // The kit is `defineType`'s last argument (light-cljs-1069).
    for expected in [
        "rt$defineType(function Type(",
        ", [[\"IEquiv/-equiv\", 2, function(a0, a1) { return a1 != null && \
         this.constructor === a1.constructor && rt$equalPair(this[\"x\"], a1[\"x\"]) && \
         rt$equalPair(this[\"y\"], a1[\"y\"]) && rt$equalPair(this[\"__extmap\"], a1[\"__extmap\"]); }], \
         [\"IHash/-hash\", 1, function() { const h = this[\"__hash\"]; if (h != null) return h; \
         const v0 = this[\"x\"], v1 = this[\"y\"], e = this[\"__extmap\"], x = e == null ? null : rt$toArray(e); \
         let r = (2099068185 ^ rt$hash(v0)) + (-1757859776 ^ rt$hash(v1)) | 0; \
         if (x != null) r = rt$recordEntriesHash(x, r); \
         return this[\"__hash\"] = r ^ 24365320; }], [\"cljs$core$ILookup$_lookup$arity$2$",
        "this[\"foo_bar\"]",
        "{ const h = this[\"__hash\"]; if (h != null) return h; const e = this[\"__extmap\"], \
         x = e == null ? null : rt$toArray(e); let r = 0; if (x",
    ] {
        assert!(js.contains(expected), "{expected}\n{js}");
    }
    assert_eq!(
        run(source),
        "[-1309891250,-1309891249,24365309,-1541209217,1961785658,1961785658,true,false,false,false,false,\
         false,true,false,true,true,false,\"#cljs.user.M{:foo-bar \\\"s\\\", :a? nil, :__proto__ #cljs.user.P{:x 1, :y 2}}\",\
         \"#cljs.user.P{:x 1, :y [2 :k], :z {:w 1}}\",-1,2,true,true]\n"
    );
    let unread = "(defprotocol Area (area [s]))
                  (defrecord Rect [w h] Area (area [_] (* w h)))
                  (into-array (map area [(->Rect 2 3) (->Rect 1 4)]))";
    let js = advanced(unread);
    assert!(
        !js.contains("rt$installMethods(") && !js.contains("function(a0, a1) { return a1 != null"),
        "{js}"
    );
}

/// light-cljs-388: a record field named `constructor` shadows the
/// prototype's, so equality compares the two field values as the shared
/// `-equiv` does, with `===`: `0` and `-0.0` are equal at every level, also
/// where `Advanced` writes the record's own `-equiv`.
#[test]
fn record_constructor_fields_compare_signed_zeros_as_equal() {
    let source = "(defrecord R [constructor])
                  (let [a (atom (->R 0)) b (atom (->R -0.0))]
                    (array (= @a @b) (= @b @a) (= @a (->R 1)) (.-constructor @b)))";
    assert_eq!(run(source), "[true,true,false,0]\n");
    let program = light_cljs::analyze(source, &light_cljs::parse(source).unwrap()).unwrap();
    let js = light_cljs::prepare_application(
        &program,
        super::optimization::readable(light_cljs::OptimizationLevel::Advanced),
    )
    .emit_js();
    assert!(
        js.contains("function(a0, a1) { return a1 != null && this.constructor === a1.constructor"),
        "{js}"
    );
}

/// light-cljs-267: a record's hash adds its fields' keyword hashes, which an
/// extension of `IHash` to keywords replaces at every level: `Advanced` then
/// writes no constant keyword hash.
#[test]
fn record_hash_respects_keyword_hash_extensions() {
    assert_eq!(
        run("(extend-type Keyword IHash (-hash [_] 0)) (defrecord R [x])
             (let [r (->R 1)] (array (hash r) (hash (->R 1)) (= r (->R 1)) (get {r 1} (->R 1))))"),
        "[24365323,24365323,true,1]\n"
    );
}

/// light-cljs-267: a record's hash reads every field and its extension map's
/// entries before it hashes any of them, at every level, so a field's hash
/// that sets another field leaves the record's hash as it was.
#[test]
fn record_hash_preserves_field_snapshot_order() {
    assert_eq!(
        run("(declare target)
             (deftype Touch [] IHash (-hash [_] (set! (.-y target) 2) (set! (.-__extmap target) {:z 5}) 0))
             (defrecord R [x y])
             (def target (assoc (->R (Touch.) 1) :w 3))
             (let [first-hash (hash target) again (hash (assoc (->R (Touch.) 1) :w 3))]
               (array (= first-hash again) (.-y target)))"),
        "[true,2]\n"
    );
}

/// A record type at `Advanced` takes its 15 markers as their masks alone
/// (light-cljs-1069), unless the program names a protocol member or mask:
/// then a marker read by name, and `record?` after the program clears the
/// mask by name, give what they gave, at every level.
#[test]
fn named_record_protocol_markers_remain_visible() {
    assert_eq!(
        run("(defrecord R [a])
             (let [r (->R 1)]
               (array (some? (.-cljs$core$IRecord$ r)) (record? r)
                      (do (set! (.-cljs$lang$protocol_mask$partition0$ r) 0) (record? r))))"),
        "[true,true,true]\n"
    );
}

/// light-cljs-1082: at `Advanced` a record type whose program looks up or
/// associates keys has its own `ILookup/-lookup` (both arities) and
/// `IAssociative/-assoc` methods: interned field keywords match by identity,
/// non-interned keywords by spelling (`keywordFqn`), non-keywords and
/// qualified keywords fall back to `__extmap`, and field `assoc` calls the
/// type's current constructor with the fields written out. `extend-type`,
/// `specify!`, `nil` extensions, 0/1/multi-field records, munged names,
/// `__proto__`, `constructor`, nil/false field values, hash invalidation,
/// and extmap key identity all match `None`.
#[test]
fn advanced_records_lookup_and_assoc_with_their_own_methods() {
    let advanced = |source: &str| {
        let program = light_cljs::analyze(source, &light_cljs::parse(source).unwrap()).unwrap();
        light_cljs::prepare_application(
            &program,
            super::optimization::readable(light_cljs::OptimizationLevel::Advanced),
        )
        .emit_js()
    };
    let source = "(defrecord E [])
                  (defrecord One [a])
                  (defrecord M [w foo-bar a? __proto__ constructor])
                  (defrecord Ext [x])
                  (extend-type Ext
                    ILookup (-lookup ([_ k nf] (str \"ext:\" (name k) \":\" nf)))
                    IAssociative (-assoc [r k v] (str \"assoc:\" (.-x r) \":\" (name k) \":\" v)))
                  (let [e @(atom (->E))
                        o @(atom (with-meta (->One nil) {:m 1}))
                        m @(atom (with-meta (assoc (->M false 2 3 4 5) :extra 6 :ns/w 7 \"w\" 8) {:tag \"m\"}))
                        h-before (hash m)
                        fw (keyword \"w\")
                        fb (keyword \"foo-bar\")
                        fp (keyword \"__proto__\")
                        fk (keyword \"fresh-ext\")
                        m2 (assoc m fw 10)
                        m3 (assoc m fb 20)
                        m4 (assoc m fk 30)
                        m5 (assoc m :ns/w 40)
                        o2 (assoc o (keyword \"a\") 99)
                        e2 (assoc e :z 11)
                        ext @(atom (->Ext 7))
                        spec @(atom (specify! (->One 1)
                                      ILookup (-lookup ([_ k nf] (str \"spec:\" (name k) \":\" nf)))
                                      IAssociative (-assoc [_ k v] (str \"spec-a:\" (name k) \":\" v))))]
                    (array (:w m) (get m :w \"nf\") (get m fw \"nf\") (fw m)
                           (:foo-bar m) (get m fb) (:a? m) (:__proto__ m) (get m fp) (:constructor m)
                           (:ns/w m) (get m \"w\") (get m 'w \"nf\") (get m nil \"nf\") (:extra m) (:missing m \"nf\")
                           (get o :a \"nf\") (:a o) (:a o2) (:m (meta o2))
                           (:z e2) (get e :z \"nf\")
                           (:w m2) (:foo-bar m2) (:tag (meta m2)) (:extra m2) (= (hash m2) h-before)
                           (:foo-bar m3) (:w m3)
                           (get m4 fk) (identical? (first (filter #(= % fk) (keys m4))) fk)
                           (:ns/w m5) (:w m5)
                           (:x ext) (get ext :x \"nf\") (assoc ext :x 8)
                           (:a spec) (get spec :a \"nf\") (assoc spec :a 2)))";
    let js = advanced(source);
    for expected in [
        "cljs$core$ILookup$_lookup$arity$2$",
        "cljs$core$ILookup$_lookup$arity$3$",
        "cljs$core$IAssociative$_assoc$arity$3$",
        "switch (rt$keywordFqn(a1))",
        "case \"foo-bar\": return this[\"foo_bar\"];",
        "case \"__proto__\": return this[\"__proto__\"];",
        "return new g",
    ] {
        assert!(js.contains(expected), "{expected}\n{js}");
    }
    assert_eq!(
        run(source),
        "[false,false,false,false,2,2,3,4,4,5,7,8,\"nf\",\"nf\",6,\"nf\",null,null,99,1,11,\"nf\",10,2,\"m\",6,false,20,false,30,true,40,false,\"ext:x:\",\"ext:x:nf\",\"assoc:7:x:8\",\"spec:a:\",\"spec:a:nf\",\"spec-a:a:2\"]\n"
    );

    // Missing extmap matches `None` (including with nil protocol extensions).
    assert_eq!(
        run("(extend-type nil
               ILookup (-lookup ([_ k] (array \"nil2\" (name k))) ([_ k nf] (array \"nil3\" (name k) nf)))
               IAssociative (-assoc [_ k v] {:from-nil [k v]}))
             (defrecord R [a])
             (let [r @(atom (->R 1))]
               (array (:a r) (:b r) (get r :b \"nf\") (pr-str (assoc r :b 2)) (pr-str (assoc r :a 9))))"),
        "[1,null,\"nf\",\"#cljs.user.R{:a 1, :b 2}\",\"#cljs.user.R{:a 9}\"]\n"
    );

    // A record program that uses seq/reduce without any lookup or assoc writes
    // no per-type `-lookup` or `-assoc` kit.
    let no_lookup = "(defprotocol Area (area [s]))
                     (defrecord Rect [w h] Area (area [_] (* w h)))
                     (reduce (fn [t r] (+ t (area r))) 0 [(->Rect 2 3) (->Rect 1 4)])";
    let js = advanced(no_lookup);
    let definitions = &js[js.find("rt$defineType(function Type").unwrap()..];
    assert!(
        !definitions.contains("ILookup$_lookup") && !definitions.contains("IAssociative$_assoc"),
        "{definitions}"
    );
}

#[test]
fn record_assoc_falls_back_when_constructor_is_rebound_to_nil() {
    assert_eq!(
        run("(defrecord E [])
             (defrecord R [x])
             (defrecord M [x y])
             (let [e @(atom (->E))
                   r @(atom (->R 1))
                   m @(atom (->M 1 2))]
               (with-redefs [E nil R nil M nil]
                 (let [e2 (assoc e :z 9)
                       r2 (assoc r :x 2)
                       r3 (assoc r :ext 3)
                       m2 (assoc m :x 10)
                       m3 (assoc m :ext 20)]
                   (array (:z e2) (.-x r2) (:x r2) (.-x r3) (:ext r3)
                          (.-x m2) (.-y m2) (.-x m3) (.-y m3) (:ext m3)))))"),
        "[9,2,2,1,3,10,2,1,2,20]\n"
    );
}

#[test]
fn record_assoc_preserves_field_read_order() {
    assert_eq!(
        run("(defrecord E [])
             (defrecord One [x])
             (defrecord Pair [x y])
             (let [events (array)
                   install-throwing (fn [r prop]
                                      (js/Object.defineProperty r prop
                                        (js-obj \"get\" (fn [] (throw (js/Error. (str \"read-\" prop)))))))
                   o (install-throwing (->One 1) \"x\")
                   p (install-throwing (->Pair 1 2) \"x\")
                   o-hit (assoc o :x 42)
                   p-hit (assoc p :x 99)
                   mut-ext (fn [r target]
                             (specify! (js-obj)
                               IAssociative
                               (-assoc [_ k v]
                                 (.push events \"ext\")
                                 (when target (aset r target 22))
                                 (aset r \"__meta\" {:after true})
                                 (hash-map k v))))
                   e0 (with-meta (->E) {:before true})
                   _ (aset e0 \"__extmap\" (mut-ext e0 nil))
                   o0 (with-meta (->One 1) {:before true})
                   _ (aset o0 \"__extmap\" (mut-ext o0 \"x\"))
                   p0 (with-meta (->Pair 1 2) {:before true})
                   _ (aset p0 \"__extmap\" (mut-ext p0 \"x\"))
                   e-miss (assoc e0 :extra 7)
                   o-miss (assoc o0 :extra 8)
                   p-miss (assoc p0 :extra 9)
                   ;; On a zero-field record miss, constructor resolution runs after extmap -assoc.
                   ctor-events (array)
                   e-rebind (with-meta (->E) {:rebind true})
                   _ (aset e-rebind \"__extmap\"
                       (specify! (js-obj)
                         IAssociative
                         (-assoc [_ k v]
                           (.push ctor-events \"ext-before-ctor\")
                           (set! E (fn [m ext h]
                                     (.push ctor-events \"rebound-ctor\")
                                     (js-obj \"m\" m \"ext\" ext \"h\" h)))
                           (hash-map k v))))
                   e-rebound (assoc e-rebind :z 1)]
               (array (.-x o-hit) (.-x p-hit) (.-y p-hit)
                      (:extra e-miss) (:before (meta e-miss)) (:after (meta e-miss))
                      (.-x o-miss) (:extra o-miss) (:before (meta o-miss))
                      (.-x p-miss) (.-y p-miss) (:extra p-miss) (:before (meta p-miss))
                      (.-length events)
                      (aget ctor-events 0) (aget ctor-events 1)
                      (:rebind (aget e-rebound \"m\")) (:z (aget e-rebound \"ext\"))))"),
        "[42,99,2,7,true,null,22,8,true,22,2,9,true,3,\"ext-before-ctor\",\"rebound-ctor\",true,1]\n"
    );
}

#[test]
fn advanced_records_emit_lookup_kits_for_named_slots() {
    let source = "(defrecord R [x])
                  (let [r @(atom (->R 1))]
                    (array (.cljs$core$ILookup$_lookup$arity$2 r r :x)
                           (.cljs$core$ILookup$_lookup$arity$3 r r :missing \"nf\")))";
    let program = light_cljs::analyze(source, &light_cljs::parse(source).unwrap()).unwrap();
    let js = light_cljs::prepare_application(
        &program,
        super::optimization::readable(light_cljs::OptimizationLevel::Advanced),
    )
    .emit_js();
    let definitions = &js[js.find("rt$defineType(function Type").unwrap()..];
    assert!(
        definitions.contains("cljs$core$ILookup$_lookup$arity$2$")
            && definitions.contains("cljs$core$ILookup$_lookup$arity$3$"),
        "{definitions}"
    );
    assert_eq!(run(source), "[1,\"nf\"]\n");
}
