#![cfg(feature = "bbp-macros")]
use light_cljs::{MacroHost, analyze_with_macros, emit_js, parse};

const SEQ_FIXTURES: &[(&str, &str)] = &[
    (
        "(defmacro call-form [x] (seq x)) (js/console.log (call-form [+ 1 2]))",
        "3\n",
    ),
    (
        "(defmacro quoted-seq [x] (list 'quote (seq x))) (js/console.log (pr-str (quoted-seq [1 2])) (pr-str (quoted-seq {:a 1})) (pr-str (quoted-seq \"ab\")))",
        "(1 2) ([:a 1]) (\"a\" \"b\")\n",
    ),
    (
        "(defmacro sequence? [x] (seq? (seq x))) (js/console.log (sequence? [1]) (sequence? {:a 1}) (sequence? \"a\") (sequence? (1)))",
        "true true true true\n",
    ),
    (
        "(defmacro empty-seq? [x] (nil? (seq x))) (js/console.log (empty-seq? nil) (empty-seq? ()) (empty-seq? []) (empty-seq? {}) (empty-seq? \"\"))",
        "true true true true true\n",
    ),
    (
        "(defmacro units [x] (count (seq x))) (js/console.log (units \"😀\"))",
        "2\n",
    ),
];

fn program_output(js: &str) -> String {
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(js)
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
fn seq_converts_supported_finite_inputs_to_lists() {
    for (source, expected) in SEQ_FIXTURES {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        assert_eq!(program_output(&emit_js(&program)), *expected, "{source}");
        assert!(!host.native_macro_calls().is_empty());
    }
}

#[test]
fn seq_preserves_list_identity_and_existing_element_occurrences() {
    use light_cljs::{
        CollectionKind,
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, ExpansionOrigin, Name, Source},
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let source = "(fn [form env x] (seq x))";
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let function = evaluator
        .evaluate_unit(&light_cljs::emitter::emit_js_unit(&program))
        .unwrap();
    let environment = Datum::new(DatumKind::Collection {
        kind: CollectionKind::Map,
        children: Box::new([]),
    });
    for input in [
        "(sequence (1 1))",
        "(sequence [1 1])",
        "(sequence {:a [1 1]})",
    ] {
        let call = host
            .read_compiler_form(&Source::new("sequence.cljs", input))
            .unwrap()
            .unwrap();
        let original = &call.children()[1];
        for iteration in 0..2 {
            let result = host
                .expand_javascript(
                    &evaluator,
                    function,
                    &call,
                    &environment,
                    ExpansionOrigin {
                        macro_name: Name::new(None, "sequence"),
                        call: call.span(),
                        definition: None,
                        helpers: vec![],
                    },
                )
                .unwrap();
            assert!(matches!(
                result.data().kind(),
                DatumKind::Collection {
                    kind: CollectionKind::List,
                    ..
                }
            ));
            if input.contains("(1 1)") {
                assert!(std::ptr::eq(result.data().kind(), original.data().kind()));
                assert_eq!(result.span().bytes(), original.span().bytes());
            }
            let elements = if input.contains('{') {
                result.children()[0].children()
            } else {
                result.children()
            };
            assert_eq!(elements.len(), original.children().len());
            for (element, original) in elements.iter().zip(original.children()) {
                assert!(std::ptr::eq(element.data().kind(), original.data().kind()));
                assert_eq!(element.span().bytes(), original.span().bytes());
                assert_eq!(element.span().source().name(), "sequence.cljs");
            }
            if iteration == 1 {
                assert_eq!(host.native_macro_calls().last().unwrap().imported, 0);
            }
        }
    }
    evaluator.release(function).unwrap();
}

#[cfg(any(feature = "quickjs", feature = "v8-macros"))]
#[test]
fn seq_expansions_and_program_results_match_legacy() {
    for (source, expected) in SEQ_FIXTURES {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        let native = emit_js(&program);
        let legacy = light_cljs::compile(source).unwrap();
        assert_eq!(native, legacy, "{source}");
        assert_eq!(program_output(&native), *expected, "{source}");
        assert_eq!(program_output(&legacy), *expected, "{source}");
    }
}

#[test]
fn constructs_real_macro_form() {
    let source = "(defmacro make-add [x] (list '+ x 2)) (js/console.log (make-add 40))";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let js = emit_js(&program);
    let result = std::process::Command::new("node")
        .arg("-e")
        .arg(js)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), "42\n");
}

#[test]
fn variadic_and_multi_arity_macros_forward_the_receiver() {
    for (source, expected) in [
        (
            "(defmacro maximum [& xs] `(js/Math.max ~@xs)) (js/console.log (maximum 1 2) (maximum 40 2 42))",
            "2 42\n",
        ),
        (
            "(defmacro choose ([] 1) ([x] x) ([x y & more] `(js/Math.max ~x ~y ~@more))) (js/console.log (choose) (choose 2) (choose 1 2) (choose 1 2 3))",
            "1 2 2 3\n",
        ),
        (
            "(defmacro choose ([] 1) ([x] x)) (js/console.log (choose) (choose 2))",
            "1 2\n",
        ),
        (
            "(defmacro signatures [pname & sigs] `(defprotocol ~pname ~@sigs)) (signatures Sample (value [this])) (deftype Example [] Sample (value [this] 42)) (js/console.log (satisfies? Sample (Example.)) (value (Example.)))",
            "true 42\n",
        ),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        assert_eq!(program_output(&emit_js(&program)), expected, "{source}");
    }
}

#[test]
fn metadata_environment_form_and_syntax_quote() {
    for (source, want) in [
        (
            "(defmacro wrap [x] `(+ ~x 2)) (js/console.log (wrap 40))",
            "42\n",
        ),
        (
            "(defmacro place [] (count &form)) (js/console.log (place))",
            "1\n",
        ),
        (
            "(defmacro ns-name [] (list 'quote (:name (:ns &env)))) (js/console.log (str (ns-name)))",
            "cljs.user\n",
        ),
        (
            "(defmacro annotated [x] (with-meta x {:native true})) (js/console.log (annotated (+ 1 41)))",
            "42\n",
        ),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(emit_js(&p))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), want, "{source}");
    }
}

#[test]
fn preloaded_real_helper_and_ordinary_js_interop() {
    let mut host = MacroHost::new();
    host.preload_native_helper(
        "cljs.user/helper",
        "(fn [x] (list '+ x (js* \"Math.max(1, 2)\")))",
    )
    .unwrap();
    let source =
        "(declare helper) (defmacro via-helper [x] (helper x)) (js/console.log (via-helper 40))";
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(emit_js(&p))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
    assert_eq!(host.native_macro_calls().len(), 1);
}

#[test]
fn javascript_interop_unwraps_input_scalars_and_preserves_javascript_objects() {
    for (source, expected) in [
        (
            "(defmacro maximum [x] (js/Math.max x 2)) (js/console.log (maximum 40))",
            "40\n",
        ),
        (
            "(defmacro part [s n] (.substring s n)) (js/console.log (part \"hello\" 2))",
            "llo\n",
        ),
        (
            "(defmacro size [s] (.-length s)) (js/console.log (size \"hello\"))",
            "5\n",
        ),
        (
            "(defmacro stamp [n] (.getTime (js/Date. n))) (js/console.log (stamp 42))",
            "42\n",
        ),
        (
            "(defmacro stored [x] (let [o (js* \"({value: 0})\")] (set! (.-value o) x) (js/Math.max (.-value o) 2))) (js/console.log (stored 40))",
            "40\n",
        ),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(emit_js(&program))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{source}"
        );
        assert_eq!(host.native_macro_calls().len(), 1);
    }
}

#[test]
fn false_and_nil_input_occurrences_keep_language_truth() {
    let source =
        "(defmacro truthy [x] (if x 1 0)) (js/console.log (truthy nil) (truthy false) (truthy 0))";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(emit_js(&p))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), "0 0 1\n");
}

#[test]
fn thrown_macro_error_names_call_and_definition() {
    let source = "(defmacro explode [] (throw (js/Error. \"macro boom\")))\n(explode)";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let message = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .err()
        .unwrap()
        .to_string();
    assert!(message.contains("macro boom"), "{message}");
    assert!(message.contains("cljs.user:2:1"), "{message}");
    assert!(message.contains("definition cljs.user:1:1"), "{message}");
}

#[test]
fn ordinary_javascript_macro_state_persists_in_source_order() {
    let source = "(defmacro bump [] (js* \"(globalThis.n = (globalThis.n || 0) + 1)\")) (js/console.log (bump) (bump))";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(emit_js(&program))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1 2\n");
    assert_eq!(host.native_macro_calls().len(), 2);
}

#[test]
fn native_occurrence_identity_survives_real_emitted_macro_and_second_import() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, ExpansionOrigin, Name, Source},
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let source = "(fn [form env x] x)";
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let function = evaluator
        .evaluate_unit(&light_cljs::emitter::emit_js_unit(&program))
        .unwrap();
    let call = host
        .read_compiler_form(&Source::new(
            "identity.cljs",
            "(same ^{:m 1} [9007199254740993 1.0 \\a])",
        ))
        .unwrap()
        .unwrap();
    let original = call.children()[1].clone();
    let environment = Datum::new(DatumKind::Collection {
        kind: light_cljs::CollectionKind::Map,
        children: Box::new([]),
    });
    for i in 0..2 {
        let result = host
            .expand_javascript(
                &evaluator,
                function,
                &call,
                &environment,
                ExpansionOrigin {
                    macro_name: Name::new(None, "same"),
                    call: call.span(),
                    definition: None,
                    helpers: vec![],
                },
            )
            .unwrap();
        assert!(
            std::ptr::eq(result.data().kind(), original.data().kind()),
            "original Datum allocation survives"
        );
        assert_eq!(result.span().source().name(), "identity.cljs");
        assert_eq!(result.span().bytes(), original.span().bytes());
        let stats = host.native_macro_calls();
        assert_eq!(stats[i].exported, 0);
        assert_eq!(stats[i].reused, 1);
        if i == 1 {
            assert_eq!(stats[i].imported, 0);
        }
    }
    evaluator.release(function).unwrap();
    // The same identity behavior also traverses normal defmacro registration.
    let source = "(defmacro same [x] x) (same (+ 40 2))";
    analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    assert_eq!(host.native_macro_calls()[0].reused, 1);
    assert_eq!(host.native_macro_calls()[0].exported, 0);
}

#[test]
fn native_environment_updates_changed_slots_without_importing_the_namespace_again() {
    use light_cljs::{
        CollectionKind,
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, Name},
        evaluator::{EvaluatorOptions, MacroEvaluator},
        form_bridge::ConversionLimits,
    };
    fn keyword(s: &str) -> Datum {
        Datum::new(DatumKind::Keyword(Name::new(None, s)))
    }
    fn number(n: i64) -> Datum {
        Datum::new(DatumKind::Integer(n))
    }
    fn map(xs: Vec<Datum>) -> Datum {
        Datum::new(DatumKind::Collection {
            kind: CollectionKind::Map,
            children: xs.into(),
        })
    }
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let ns = map(vec![
        keyword("name"),
        Datum::new(DatumKind::Symbol(Name::new(None, "large.ns"))),
        keyword("bindings"),
        Datum::new(DatumKind::Collection {
            kind: CollectionKind::Vector,
            children: (0..500).map(number).collect(),
        }),
    ]);
    let a = map(vec![keyword("ns"), ns.clone(), keyword("line"), number(1)]);
    let b = map(vec![keyword("ns"), ns, keyword("line"), number(2)]);
    let first = evaluator
        .import_environment(&a, ConversionLimits::default())
        .unwrap();
    let before = evaluator.bridge_stats();
    let second = evaluator
        .import_environment(&b, ConversionLimits::default())
        .unwrap();
    let after = evaluator.bridge_stats();
    assert_eq!(
        after.imported - before.imported,
        3,
        "only the envelope and changed key/value are imported"
    );
    evaluator.release(first).unwrap();
    evaluator.release(second).unwrap();
}

#[test]
fn declaration_bundle_registration_preserves_preloaded_roots_and_defers_its_code() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let result = evaluator.evaluate_unit(r#"rt => {
        const roots = globalThis.__lightCompilerRoots;
        roots['fixture/ready'] = 42;
        let ran = false;
        const code = () => { ran = true; throw new Error('must stay deferred'); };
        __lightRegisterBundle(['fixture/ready', 'fixture/P', 'fixture/method'], code, [], 'fixture:1:1', false, false, 7);
        __lightRegisterBundle(['fixture/P'], code, [], 'fixture:2:1', true, false, 8);
        __lightRegisterBundle([], code, [['fixture/P', null, 'fixture/Type']], 'fixture:3:1', false, false, 9);
        if (ran || roots['fixture/ready'] !== 42) throw new Error('registration changed preloaded state');
        // A bundle without a unit keeps the explicit unsupported demand.
        __lightRegisterBundle(['fixture/orphan'], code, [], 'fixture:4:1', false, false, -1);
        let message = '';
        try { roots['fixture/orphan']; } catch (error) { message = String(error); }
        if (!message.includes('unsupported operation: helper not preloaded: fixture/orphan')) throw new Error('missing demand diagnostic');
        if (ran) throw new Error('registration ran bundle code');
        return null;
    }"#).unwrap();
    evaluator.release(result).unwrap();
}

#[test]
fn unsupported_operations_are_explicit_and_do_not_fall_back() {
    for (source, expected) in [
        (
            "(defprotocol Sample (value [this])) (defmacro demand [] Sample) (demand)",
            "unsupported operation: macro returned a non-form value",
        ),
        (
            "(defmacro lazy-result [] (range 3)) (lazy-result)",
            "unsupported operation: native macro runtime core.range",
        ),
        // Sorted collections and transients remain JavaScript-only types.
        (
            "(defmacro sorted [] (sorted-map 1 2)) (sorted)",
            "unsupported operation: native macro runtime core.sortedMap",
        ),
        (
            "(defmacro edit [] (transient [])) (edit)",
            "unsupported operation: native macro runtime core.transient",
        ),
        (
            "(defmacro js-collection [x] (js/Object.keys x)) (js-collection [1 2])",
            "unsupported operation: JavaScript interop with native vector form",
        ),
        (
            "(defmacro js-instance [x] (instance? js/Object x)) (js-instance [1 2])",
            "unsupported operation: JavaScript interop with native vector form",
        ),
        (
            "(defmacro sequence [x] (seq x)) (sequence 1)",
            "unsupported operation: native macro runtime seq on number",
        ),
        (
            "(defmacro sequence [x] (seq x)) (sequence :a)",
            "unsupported operation: native macro runtime seq on keyword",
        ),
        (
            "(defmacro sequence [x] (seq x)) (sequence a)",
            "unsupported operation: native macro runtime seq on symbol",
        ),
        (
            "(defmacro sequence [] (seq (js* \"({})\"))) (sequence)",
            "unsupported operation: native macro runtime seq on object",
        ),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let message = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .err()
            .unwrap()
            .to_string();
        assert!(message.contains(expected), "{source}: {message}");
    }
}

#[test]
fn demands_a_real_helper_without_replaying_the_macro() {
    let source = include_str!("fixtures/native-helper-demand/macro.cljs");
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    assert_eq!(program_output(&emit_js(&program)), "42\n");
}

#[test]
fn cached_forms_still_obey_conversion_limits() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        compiler_data::Source,
        evaluator::{EvaluatorOptions, MacroEvaluator},
        form_bridge::ConversionLimits,
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    let form = host
        .read_compiler_form(&Source::new("limits.cljs", "[1 [2 3]]"))
        .unwrap()
        .unwrap();
    let value = evaluator
        .import_form(&form, ConversionLimits::default())
        .unwrap();
    evaluator.release(value).unwrap();
    for limits in [
        ConversionLimits {
            nodes: 2,
            depth: 128,
        },
        ConversionLimits {
            nodes: 100,
            depth: 1,
        },
    ] {
        let error = evaluator.import_form(&form, limits).unwrap_err();
        assert!(error.0.contains("form conversion limit"), "{error}");
    }
}

#[test]
fn native_execution_observes_shared_cancellation_and_fuel() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions {
        poll_budget: 50,
        ..Default::default()
    })
    .unwrap();
    let function = evaluator
        .evaluate_unit("rt => () => {while(true){}} ")
        .unwrap();
    let error = evaluator.call(function, &[]).unwrap_err();
    assert!(error.0.contains("poll budget"), "{error}");
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    evaluator.control().cancel();
    let error = evaluator.evaluate_unit("rt => 1").unwrap_err();
    assert!(error.0.contains("cancel"), "{error}");
}

#[test]
fn extracted_native_metadata_reuses_the_original_datum_and_occurrence() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, ExpansionOrigin, Name, Source},
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let source = "(fn [form env x] (meta x))";
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let function = evaluator
        .evaluate_unit(&light_cljs::emitter::emit_js_unit(&program))
        .unwrap();
    let call = host
        .read_compiler_form(&Source::new("meta.cljs", "(metadata ^{:m 1} [2])"))
        .unwrap()
        .unwrap();
    let original = call.children()[1].metadata().unwrap();
    let environment = Datum::new(DatumKind::Collection {
        kind: light_cljs::CollectionKind::Map,
        children: Box::new([]),
    });
    let result = host
        .expand_javascript(
            &evaluator,
            function,
            &call,
            &environment,
            ExpansionOrigin {
                macro_name: Name::new(None, "metadata"),
                call: call.span(),
                definition: None,
                helpers: vec![],
            },
        )
        .unwrap();
    assert!(std::ptr::eq(result.data().kind(), original.data().kind()));
    assert_eq!(result.span().bytes(), original.span().bytes());
    assert_eq!(host.native_macro_calls()[0].exported, 0);
    assert_eq!(host.native_macro_calls()[0].reused, 1);
}

#[test]
fn equal_scalar_occurrences_keep_their_distinct_source_identity() {
    use light_cljs::{
        bbp_evaluator::BbpEvaluator,
        compiler_data::{Datum, DatumKind, ExpansionOrigin, Name, Source},
        evaluator::{EvaluatorOptions, MacroEvaluator},
    };
    let evaluator = BbpEvaluator::new(EvaluatorOptions::default()).unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let source = "(fn [form env a b] b)";
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let function = evaluator
        .evaluate_unit(&light_cljs::emitter::emit_js_unit(&program))
        .unwrap();
    let call = host
        .read_compiler_form(&Source::new(
            "scalar.cljs",
            "(second 9007199254740993 9007199254740993)",
        ))
        .unwrap()
        .unwrap();
    let original = call.children()[2].clone();
    let environment = Datum::new(DatumKind::Collection {
        kind: light_cljs::CollectionKind::Map,
        children: Box::new([]),
    });
    let result = host
        .expand_javascript(
            &evaluator,
            function,
            &call,
            &environment,
            ExpansionOrigin {
                macro_name: Name::new(None, "second"),
                call: call.span(),
                definition: None,
                helpers: vec![],
            },
        )
        .unwrap();
    assert!(std::ptr::eq(result.data().kind(), original.data().kind()));
    assert_eq!(result.span().bytes(), original.span().bytes());
    assert_ne!(result.span().bytes(), call.children()[1].span().bytes());
    assert_eq!(host.native_macro_calls()[0].exported, 0);
}

#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

#[test]
fn reduced_project_namespaces_expand_variadic_macros() {
    use light_cljs::analyzer::ExprKind;
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("benchmarks/project-results/native-2026-10-25");
    for (entry, directory) in [
        ("app.common.math", "reduced-penpot"),
        ("workload.fonam", "reduced-generated"),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = light_cljs::compiler::analyze_namespace_entry(
            entry,
            &[fixtures.join(directory)],
            &mut host,
            Default::default(),
            &mut Vec::new(),
        )
        .unwrap();
        let js = emit_js(&program);
        if entry == "app.common.math" {
            assert_eq!(program_output(&format!("console.log({js});")), "2\n");
        } else {
            let protocols = program
                .exprs
                .iter()
                .filter_map(|expr| match &expr.kind {
                    ExprKind::DefProtocol {
                        global, methods, ..
                    } => Some((global, methods)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(protocols.len(), 1);
            let (global, methods) = protocols[0];
            assert_eq!(program.globals[global.0].namespace, "workload.fonam");
            assert_eq!(program.globals[global.0].name, "Bagrir");
            assert_eq!(methods.len(), 1);
            assert_eq!(methods[0].key, "workload.fonam/Bagrir/mamjalsko");
            assert_eq!(program.globals[methods[0].global.0].name, "mamjalsko");
            assert_eq!(methods[0].arities, [1]);
            assert_eq!(methods[0].variadic, None);
            assert_eq!(program_output(&js), "");
        }
    }
}

#[test]
fn namespace_stats_report_bridge_work_for_each_native_call() {
    let root = temp_dir::TempDir::new("native-macro-stats");
    std::fs::write(
        root.join("app.cljs"),
        "(ns app) (defmacro twice [x] (list '+ x x)) (twice 21)",
    )
    .unwrap();
    let mut host = MacroHost::new();
    host.use_native_macros();
    let mut stats = light_cljs::CompileStats::default();
    light_cljs::compiler::analyze_namespace_entry_with_stats(
        "app",
        &[root.path().to_path_buf()],
        &mut host,
        Default::default(),
        &mut Vec::new(),
        &mut stats,
    )
    .unwrap();
    assert_eq!(stats.native_macro_calls.len(), 1);
    let call = stats.native_macro_calls[0];
    assert!(call.imported > 0);
    assert!(call.exported > 0);
    assert_eq!(call.reused, 2);
    assert!(call.nanos > 0);
    // The default --macro-memory-limit is the runtime's accounted-byte cap.
    assert_eq!(call.gc_byte_limit, 64 << 20);
    assert!(call.gc_admitted_bytes > 0);
    assert!(call.gc_managed_bytes > 0 && call.gc_managed_bytes <= call.gc_peak_managed_bytes);
    assert!(call.gc_peak_managed_bytes <= call.gc_byte_limit);
    let report = stats.report();
    for key in [
        "macro.bridge.0.imported",
        "macro.bridge.0.exported",
        "macro.bridge.0.reused",
        "macro.bridge.0.ns",
        "macro.gc.0.managed-bytes",
        "macro.gc.0.peak-managed-bytes",
        "macro.gc.0.live-bytes",
        "macro.gc.0.admitted-bytes",
        "macro.gc.0.byte-limit 67108864",
    ] {
        assert!(report.contains(key), "{report}");
    }
}

#[cfg(any(feature = "quickjs", feature = "v8-macros"))]
#[test]
fn native_expansions_match_the_legacy_compiler_output() {
    for source in [
        "(defmacro wrap [x] `(+ ~x 2)) (wrap 40)",
        "(defmacro same [x] x) (same (+ 40 2))",
        "(defmacro ns-name [] (list 'quote (:name (:ns &env)))) (ns-name)",
        "(defmacro annotated [x] (with-meta x {:native true})) (annotated (+ 40 2))",
        "(defmacro maximum [x] (js/Math.max x 2)) (maximum 40)",
        "(defmacro part [s n] (.substring s n)) (part \"hello\" 2)",
        "(defmacro stamp [n] (.getTime (js/Date. n))) (stamp 42)",
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        assert_eq!(
            emit_js(&program),
            light_cljs::compile(source).unwrap(),
            "{source}"
        );
    }
}

/// Macro code over native collections, keywords and symbols, which the
/// native unit lowers to engine intrinsics (light-cljs-477), expands as the
/// legacy compiler does and runs.
const COLLECTION_FIXTURES: &[(&str, &str)] = &[
    (
        "(defmacro members [s] (count (conj (disj s 1) 3 2))) (js/console.log (members #{1 2}))",
        "2\n",
    ),
    (
        "(defmacro has? [m k] (contains? m k)) (js/console.log (has? {:a 1} :a) (has? #{:b} :a) (has? [5] 0))",
        "true false true\n",
    ),
    (
        "(defmacro drop-key [m] (list 'quote (dissoc m :a))) (js/console.log (pr-str (drop-key {:a 1 :b 2})))",
        "{:b 2}\n",
    ),
    (
        "(defmacro same-hash? [a b] (if (= a b) (= (hash a) (hash b)) false)) (js/console.log (same-hash? [1 #{2}] (1 #{2})))",
        "true\n",
    ),
    (
        "(defmacro input-set [s] (list 'quote s)) (js/console.log (pr-str (input-set #{:a})) (count (input-set #{1 2 3})))",
        "#{:a} 3\n",
    ),
    (
        "(defmacro picks [v m] [(nth v 1) (nth v 5 :none) (get m :k 0) (:k m) (:z m 9) (first v) (second v)]) (js/console.log (pr-str (picks [10 20] {:k 5})))",
        "[20 :none 5 5 9 10 20]\n",
    ),
    (
        "(defmacro sum-rest [v] `(+ ~@(rest v) ~@(next (next v)))) (js/console.log (sum-rest [0 1 2]))",
        "5\n",
    ),
    (
        "(defmacro grow [v] (list 'quote (assoc (conj v :c) 0 :z))) (js/console.log (pr-str (grow [:a :b])))",
        "[:z :b :c]\n",
    ),
    (
        "(defmacro kinds [x] [(keyword? x) (symbol? x) (vector? x) (map? x) (set? x) (seq? x) (nil? x) (empty? x)]) (js/console.log (pr-str (kinds #{})))",
        "[false false false false true false false true]\n",
    ),
    (
        "(defmacro tag-of [x] (list 'quote (:tag (meta (with-meta x {:tag 'my-tag}))))) (js/console.log (str (tag-of [1])))",
        "my-tag\n",
    ),
    (
        "(defmacro walk [xs] (loop [s (seq xs) n 0] (if s (recur (next s) (+ n (first s))) n))) (js/console.log (walk [1 2 3 4]))",
        "10\n",
    ),
    (
        "(defmacro built [] (list 'quote (list (vector 1 2) (hash-set 3) (cons 0 [5])))) (js/console.log (pr-str (built)))",
        "([1 2] #{3} (0 5))\n",
    ),
    (
        "(defmacro as-vec [l] (list 'quote (vec l))) (js/console.log (pr-str (as-vec (1 2))))",
        "[1 2]\n",
    ),
];

#[test]
fn collection_core_lowering_matches_the_legacy_compiler_and_runs() {
    for (source, expected) in COLLECTION_FIXTURES {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        let native = emit_js(&program);
        assert_eq!(program_output(&native), *expected, "{source}");
        // In a build with a legacy engine (`quickjs`, or the default V8),
        // `compile` expands with it: the comparison is then cross-backend.
        let legacy = light_cljs::compile(source).unwrap();
        assert_eq!(native, legacy, "{source}");
    }
}

/// Expand `source` natively, check that the program prints `expected`, and
/// that the expansion is the legacy compiler's (cross-backend in a build
/// with a legacy engine).
fn assert_native_matches_legacy(source: &str, expected: &str) {
    let mut host = MacroHost::new();
    host.use_native_macros();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .unwrap_or_else(|e| panic!("{source}: {e}"));
    let native = emit_js(&program);
    assert_eq!(program_output(&native), expected, "{source}");
    assert_eq!(native, light_cljs::compile(source).unwrap(), "{source}");
}

/// A macro-local JavaScript array `a` and `t`, which records its operand
/// in `a` and returns it: operand effects in evaluation order.
const TRACE: &str = "a (js/Array.) t (fn [x] (.push a x) x)";

#[test]
fn native_hash_map_literals_evaluate_keys_before_values() {
    // Nine entries: a hash-map literal, whose keys all run before its
    // values (`mapFromArrays`).
    let entries: String = (0..9).map(|i| format!("(t {i}) (t {}) ", i + 10)).collect();
    let source = format!(
        "(defmacro order [] (let [{TRACE} m {{{entries}}}] [(.join a \" \") (get m 1) (count m)])) \
         (js/console.log (pr-str (order)))"
    );
    assert_native_matches_legacy(
        &source,
        "[\"2 6 0 5 1 4 8 7 3 12 16 10 15 11 14 18 17 13\" 11 9]\n",
    );
}

#[test]
fn nth_of_nil_honors_the_default_directly_and_through_apply() {
    assert_native_matches_legacy(
        "(defmacro fallback [] [(nth nil 0 :missing) (apply nth [nil 0 :missing]) (nth nil 0)]) \
         (js/console.log (pr-str (fallback)))",
        "[:missing :missing nil]\n",
    );
}

#[test]
fn native_wide_concat_compiles_and_preserves_operand_effects() {
    // 128 operands: a nested lowering would exceed the engine's parser
    // nesting budget.
    // `w` records its (number) operand and returns it in a vector.
    let operands: String = (0..128).map(|i| format!("(w {i}) ")).collect();
    let source = format!(
        "(defmacro wide [] (let [a (js/Array.) w (fn [x] (.push a x) [x]) c (concat {operands})] \
         [(count c) (first c) (nth c 127) (.-length a) (aget a 0) (aget a 127)])) \
         (js/console.log (pr-str (wide)))"
    );
    assert_native_matches_legacy(&source, "[128 0 127 128 0 127]\n");
}

#[test]
fn native_dynamic_constructors_match_legacy() {
    // Runtime string operands (macro inputs), direct calls and the
    // constructors as values through `apply`.
    assert_native_matches_legacy(
        "(defmacro names [s n] (list 'quote [(keyword s) (keyword nil n) (keyword s n) (keyword 'a/b) \
         (keyword \"x/y\") (keyword :k) (keyword 1) (symbol n) (symbol s n) (symbol :c/d) (symbol \"p/q\") \
         (symbol 'r) (apply keyword [s n]) (apply symbol [n]) (apply keyword [:kw]) \
         (get (array-map :k 1 :j 2) :j) (count (array-map :k 1 :k 2)) (get (apply array-map [:z 3]) :z) \
         (map? (array-map))])) \
         (js/console.log (pr-str (names \"ns\" \"nm\")))",
        "[:ns :nm :ns/nm :a/b :x/y :k nil nm ns/nm c/d p/q r :ns/nm nm :kw 2 1 3 true]\n",
    );
}

#[test]
fn native_constructor_errors_match_legacy() {
    // Invalid operands throw a plain Error, as keywordCore and symbolCore
    // do, directly and through apply; a TypeError catch does not take it.
    assert_native_matches_legacy(
        "(defmacro errors [] [(try (symbol 1) (catch :default e (.-name e))) \
         (try (apply symbol [1]) (catch :default e (.-message e))) \
         (try (keyword nil 1) (catch :default e (.-name e))) \
         (try (symbol nil :k) (catch js/TypeError e :type) (catch :default e (.-message e)))]) \
         (js/console.log (pr-str (errors)))",
        "[\"Error\" \"no conversion to symbol\" \"Error\" \"symbol expects strings and an optional nil namespace\"]\n",
    );
    // A keyword converts to a symbol through its spelling.
    assert_native_matches_legacy(
        "(defmacro parts [] (let [s (symbol :a/b) t (symbol (keyword \"x\"))] \
         [(namespace s) (name s) (namespace t) (name t)])) (js/console.log (pr-str (parts)))",
        "[\"a\" \"b\" nil \"x\"]\n",
    );
    // `(keyword "" "x")` spells `/x`, whose symbol is the unqualified name
    // `/x` in the legacy runtimes; it has no native symbol, so it fails
    // instead of becoming namespace "" and name "x".
    let source = "(defmacro odd [] (name (symbol (keyword \"\" \"x\")))) (odd)";
    let mut host = MacroHost::new();
    host.use_native_macros();
    let Err(error) = analyze_with_macros(source, &parse(source).unwrap(), &mut host) else {
        panic!("expanded");
    };
    let message = error.to_string();
    assert!(message.contains("must split back"), "{message}");
}

#[test]
fn native_dynamic_set_literals_reject_duplicates_after_operand_effects() {
    // A small and a 1,025-member literal (number constants keep it short
    // enough for one macro unit); three members are calls of `w`, which
    // records and returns its operand, and the last of them equals another
    // member. Every operand runs once before the duplicate is rejected.
    // Literals past the 4,096-operand counted-constructor limit do not fit
    // one macro unit; `bbp_evaluator`'s tests cover their constructors.
    for size in [3, 1025] {
        let constants: String = (1..=size - 3).map(|k| format!("{k} ")).collect();
        let (big, dup) = (size + 1, if size > 3 { 1 } else { 0 });
        let set = |last: usize| format!("#{{{constants}(w 0) (w {big}) (w (+ 0 {last}))}}");
        let source = format!(
            "(defmacro dup [] (let [a (js/Array.) w (fn [x] (.push a x) x)] (try {} \
             (catch :default e [(.startsWith (.-message e) \"Duplicate key\") \
             (.join (.sort a (fn [x y] (- x y))) \",\")])))) \
             (js/console.log (pr-str (dup)))",
            set(dup),
        );
        assert_native_matches_legacy(&source, &format!("[true \"{},{dup},{big}\"]\n", 0));
        // Distinct members of the same sizes are accepted.
        let source = format!(
            "(defmacro ok [] (let [a (js/Array.) w (fn [x] (.push a x) x)] [(count {}) (.-length a)])) \
             (js/console.log (pr-str (ok)))",
            set(big + 1),
        );
        assert_native_matches_legacy(&source, &format!("[{size} 3]\n"));
    }
}

#[test]
fn native_literal_collections_keep_reader_distinct_members_or_fail_explicitly() {
    // Distinct after lowering: the same values as the legacy backends.
    assert_native_matches_legacy(
        "(defmacro fine [] [(count '#{:a :b \"c\"}) (contains? '#{\\a :b} \"a\") (count #{\\a \"b\"}) \
         (get '{\\a 1 \"b\" 2} \"a\")]) (js/console.log (pr-str (fine)))",
        "[3 true 2 1]\n",
    );
    // Reader-distinct members or keys that lower to equal values cannot
    // be native set members or map keys: an explicit error, never a
    // collection that silently lost one.
    for (source, expected) in [
        (
            "(defmacro literal-count [] (count '#{\\a \"a\"})) (literal-count)",
            "native literal set with reader-distinct equal members",
        ),
        (
            "(defmacro literal-count [] (count #{\\a \"a\"})) (literal-count)",
            "native literal set with reader-distinct equal members",
        ),
        (
            "(defmacro literal-count [] (count '#{1 1.0})) (literal-count)",
            "native literal set with reader-distinct equal members",
        ),
        (
            "(defmacro literal-count [] (count '{\\a 1 \"a\" 2})) (literal-count)",
            "native literal map with reader-distinct equal keys",
        ),
    ] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let message = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
            .err()
            .unwrap_or_else(|| panic!("{source}: expanded"))
            .to_string();
        assert!(message.contains(expected), "{source}: {message}");
    }
}

#[test]
fn collection_helpers_publish_once_at_one_and_two_slots() {
    // A set constant and two functions built only from lowered core
    // operations and literals: each value is initialized once and shared,
    // not initialized again in each consuming task.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/collections");
    for jobs in [1, 2] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let mut warnings = Vec::new();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
            "coll.app",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut warnings,
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                stats: Some(&mut stats),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("jobs {jobs}: {error}"));
        assert_eq!(
            program_output(&emit_js(&program)),
            "[{:op :get} 2] [{} 1]\n",
            "jobs {jobs}"
        );
        assert_eq!(stats.native_helpers.compiled, 3, "jobs {jobs}");
        assert_eq!(stats.native_helpers.published, 3, "jobs {jobs}");
        assert_eq!(stats.native_helpers.per_task, 0, "jobs {jobs}");
    }
}

#[test]
fn helper_namespace_output_is_deterministic_with_one_two_and_four_slots() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/pool");
    let mut reference = None;
    for jobs in [1, 2, 4] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let mut warnings = Vec::new();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
            "demand.app",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut warnings,
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                stats: Some(&mut stats),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("jobs {jobs}: {error}"));
        let js = emit_js(&program);
        assert_eq!(program_output(&js), "42 42 3\n");
        assert_eq!(stats.native_helpers.compiled, 1);
        // The helper builds its expansion with native core intrinsics, so its
        // value publishes once instead of initializing in each task.
        assert_eq!(stats.native_helpers.published, 1);
        assert_eq!(stats.native_helpers.per_task, 0);
        assert!(warnings.is_empty());
        if let Some(reference) = &reference {
            assert_eq!(&js, reference);
        } else {
            reference = Some(js);
        }
        if jobs > 1 {
            assert!(stats.deferred_units.pool_engine_workers > 0);
            assert!(stats.deferred_units.pool.helped > 0);
        }
    }
}

#[test]
fn helper_failures_have_the_same_source_diagnostic_at_one_two_and_four_slots() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/pool");
    let mut reference = None;
    for jobs in [1, 2, 4] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let error = light_cljs::compiler::analyze_namespace_entry_recorded(
            "demand.failure",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut Vec::new(),
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                ..Default::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("fixture helper failed"), "{error}");
        assert!(
            error.contains("failure.cljs:2:15") && error.contains("demand.macros/demand-failure"),
            "{error}"
        );
        if let Some(reference) = &reference {
            assert_eq!(&error, reference, "jobs {jobs}");
        } else {
            reference = Some(error);
        }
    }
}

/// `--macro-memory-limit` is the native runtime's aggregate accounted-byte
/// budget: a macro that retains more than it fails with the engine's
/// catchable resource error, a macro may catch that error, and the
/// compilation continues in the same runtime afterwards.
#[test]
fn the_macro_memory_limit_denies_allocation_catchably_and_the_build_recovers() {
    let hog = "(defmacro hog [] (js* \"(() => { const xs = []; try { for (;;) xs.push({v: xs.length}); } catch (e) { xs.length = 0; return e === `heap byte limit exceeded`; } })()\"))";
    let source =
        format!("{hog} (defmacro twice [x] (list '+ x x)) (js/console.log (hog) (twice 21) (hog))");
    let mut host = MacroHost::new();
    host.use_native_macros();
    host.evaluator_options.memory_limit = 16 << 20;
    let program = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host).unwrap();
    assert_eq!(program_output(&emit_js(&program)), "true 42 true\n");

    let source =
        "(defmacro hog! [] (js* \"(() => { const xs = []; for (;;) xs.push({}); })()\"))\n(hog!)";
    let mut host = MacroHost::new();
    host.use_native_macros();
    host.evaluator_options.memory_limit = 16 << 20;
    let message = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .err()
        .unwrap()
        .to_string();
    assert!(message.contains("heap byte limit exceeded"), "{message}");
    assert!(message.contains("cljs.user:2:1"), "{message}");
}

#[test]
fn a_macro_memory_limit_below_the_native_realm_fails_at_startup_with_a_diagnostic() {
    let source = "(defmacro twice [x] (list '+ x x)) (twice 21)";
    let mut host = MacroHost::new();
    host.use_native_macros();
    host.evaluator_options.memory_limit = 256 << 10;
    let message = analyze_with_macros(source, &parse(source).unwrap(), &mut host)
        .err()
        .unwrap()
        .to_string();
    assert!(
        message.contains(
            "macro memory limit of 262144 bytes cannot hold the native macro runtime's realm"
        ),
        "{message}"
    );
}

/// Macro code over compile-time protocols, types and records, which the
/// native unit runs on engine dispatch tables and native structs
/// (light-cljs-478; docs/native-macros.md, "Protocols and types"). Each
/// expands as the legacy compiler does and runs.
const PROTOCOL_FIXTURES: &[(&str, &str)] = &[
    // Records: protocol methods, field and extension lookup, map->R,
    // assoc, equality and the record hash.
    (
        "(defprotocol P (p [x]) (q [x y])) (defrecord R [a b] P (p [_] a) (q [this y] (+ b y))) \
         (defmacro m [] [(p (->R 1 2)) (q (R. 3 4) 10) (:a (->R 5 6)) (get (map->R {:a 7 :b 8 :c 9}) :c) \
         (count (assoc (->R 1 2) :z 3)) (= (->R 1 2) (->R 1 2)) (= (->R 1 2) (->R 1 3)) \
         (hash (->R 1 2)) (hash (assoc (->R 1 2) :c 3))]) (js/console.log (pr-str (m)))",
        "[1 14 5 9 3 true false -658793607 1908665455]\n",
    ),
    // Host-type extension, `nil`, `default` and `satisfies?`.
    (
        "(defprotocol P (p [x])) (extend-protocol P string (p [x] (str \"s\" x)) number (p [x] (inc x)) \
         nil (p [_] :nil) default (p [_] :default)) \
         (defmacro m [] [(p \"a\") (p 1) (p nil) (p :k) (satisfies? P 1) (satisfies? P :k)]) \
         (js/console.log (pr-str (m)))",
        "[\"sa\" 2 :nil :default true true]\n",
    ),
    // A deftype's fields, read in its methods and through interop.
    (
        "(defprotocol P (p [x])) (deftype T [a] P (p [this] (.-a this))) \
         (defmacro m [] [(p (T. 5)) (.-a (T. 6)) (= (T. 1) (T. 1))]) (js/console.log (pr-str (m)))",
        "[5 6 false]\n",
    ),
    // A mutable field keeps a task-owned JavaScript instance.
    (
        "(defprotocol P (p [x])) (deftype T [^:mutable a] P (p [_] (set! a (inc a)) a)) \
         (defmacro m [] (let [t (T. 1)] (p t) (p t))) (js/console.log (pr-str (m)))",
        "3\n",
    ),
    // Record map operations.
    (
        "(defrecord R [a b]) (defmacro m [] (let [r (->R 1 2)] (list 'quote [(seq r) (dissoc r :a) \
         (= r (dissoc (assoc r :c 3) :c)) (meta (with-meta r {:x 1})) (:d (conj r [:d 4])) \
         (contains? r :a) (keys r) (vals r) (record? r) (map? r) (map? (->R 1 2))]))) \
         (js/console.log (pr-str (m)))",
        "[([:a 1] [:b 2]) {:b 2} true {:x 1} 4 true (:a :b) (1 2) true true true]\n",
    ),
    // The protocol-extension frontier: an expansion before a later
    // extension sees the earlier one (`FRONTIER_REPLAY` in the compiler
    // macro suite, without its macro-expanding helper).
    (
        "(defprotocol P (p [x])) (defprotocol Q (q [x])) (deftype T []) (extend-type T P (p [_] 1)) \
         (extend-type T Q (q [_] 10)) (defmacro snapshot [] [(p (T.)) (q (T.))]) (def before (snapshot)) \
         (extend-type T P (p [_] 2)) (defmacro warm [] (q (T.)) (p (T.)) nil) (warm) \
         (js/console.log (pr-str [before (snapshot) (p (T.)) (q (T.))]))",
        "[[1 10] [2 10] 2 10]\n",
    ),
    // A macro that generates protocol, record and method declarations
    // (`generated_destructured_methods...` in the compiler macro suite).
    (
        "(defmacro bundle [] '(do (defprotocol P (run [this input])) \
         (defrecord R [a] P (run [_ {:keys [b]}] (+ a b))) (run (->R 2) {:b 3}))) \
         (js/console.log (bundle))",
        "5\n",
    ),
    // A record helper value read by a macro.
    (
        "(defprotocol P (p [x])) (defrecord R [a] P (p [_] a)) (def r (->R 41)) \
         (defmacro m [] (inc (p r))) (js/console.log (m))",
        "42\n",
    ),
    // A methodless deftype with a mutable field keeps a JavaScript
    // instance that interop assigns.
    (
        "(deftype T [^:mutable a]) (defmacro m [] (let [t (T. 1)] (set! (.-a t) 2) (.-a t))) \
         (js/console.log (m))",
        "2\n",
    ),
    // A record constructor's metadata and extension map.
    (
        "(defrecord R [a]) (defmacro m [] (let [r (R. 1 {:tag true} {:extra 2})] [(meta r) (:extra r) (count r)])) \
         (js/console.log (pr-str (m)))",
        "[{:tag true} 2 2]\n",
    ),
    // Object methods on struct and mutable instances.
    (
        "(deftype T [x] Object (value [_] x)) (deftype U [^:mutable y] Object (value [_] (set! y (inc y)) y)) \
         (defmacro m [] [(.value (T. 7)) (.value (U. 1))]) (js/console.log (pr-str (m)))",
        "[7 2]\n",
    ),
    // `instance?` of struct types: records, a deftype with methods and a
    // methodless one, directly, through an expression-valued class and
    // through a saved `instance?`.
    (
        "(defprotocol P (p [x])) (defrecord R []) (deftype T [a] P (p [_] a)) (deftype U [b]) \
         (defmacro m [] (let [i instance? c (if (= 1 (count [1])) T U)] [(instance? T (T. 1)) (instance? U (T. 1)) \
         (instance? U (U. 1)) (instance? R (->R)) (instance? T (->R)) (instance? c (T. 1)) (instance? c (U. 1)) \
         (i T (T. 1)) (i R (T. 1))])) (js/console.log (pr-str (m)))",
        "[true false true true false true false true false]\n",
    ),
    // Object methods follow later extensions, inside a macro too, for
    // struct and mutable instances.
    (
        "(deftype T [x] Object (f [_] 1)) (deftype U [^:mutable y] Object (f [_] 1)) \
         (extend-type T Object (g [_] :g)) \
         (defn install [] (extend-type T Object (f [_] 2)) (extend-type U Object (f [_] 2) (g [_] :u))) \
         (defmacro m [] (let [a [(.f (T. 0)) (.f (U. 0)) (.g (T. 0))]] (install) [a (.f (T. 0)) (.f (U. 0)) (.g (U. 0))])) \
         (js/console.log (pr-str (m)))",
        "[[1 1 :g] 2 2 :u]\n",
    ),
    // Object method overloads by arity, and punctuated member names.
    (
        "(deftype T [a] Object (f [_] 1) (f [_ x] [a x]) (foo-bar [_] 7)) \
         (deftype U [^:mutable a] Object (f [_] 1) (f [_ x] [a x]) (foo-bar [_] 8)) \
         (defmacro m [] [(.f (T. 0)) (.f (T. 0) 5) (.f (U. 0)) (.f (U. 0) 5) (.foo-bar (T. 0)) (.foo-bar (U. 0))]) \
         (js/console.log (pr-str (m)))",
        "[1 [0 5] 1 [0 5] 7 8]\n",
    ),
    // `implements?` of extended core collection types.
    (
        "(defprotocol P (p [x])) (extend-type PersistentVector P (p [_] 1)) (extend-type EmptyList P (p [_] 2)) \
         (extend-type Keyword P (p [_] 3)) (extend-protocol P string (p [_] 4)) \
         (defmacro m [] [(implements? P []) (satisfies? P []) (implements? P '()) (satisfies? P '()) \
         (implements? P '(1)) (satisfies? P '(1)) (implements? P :k) (implements? P \"s\") (satisfies? P \"s\")]) \
         (js/console.log (pr-str (m)))",
        "[true true true true false false true false true]\n",
    ),
    // An extension that macro code runs overrides the type's own.
    (
        "(defprotocol P (p [x])) (deftype T [] P (p [_] 1)) (defn install [] (extend-type T P (p [_] 2))) \
         (defmacro m [] (let [a (p (T.))] (install) [a (p (T.))])) (js/console.log (pr-str (m)))",
        "[1 2]\n",
    ),
    // `:extend-via-metadata`.
    (
        "(defprotocol P :extend-via-metadata true (p [x])) (extend-protocol P default (p [_] :default)) \
         (defmacro m [] [(p (with-meta [1] {`p (fn [_] :meta)})) (p [1])]) (js/console.log (pr-str (m)))",
        "[:meta :default]\n",
    ),
    // `implements?` sees the type's own implementations only.
    (
        "(defprotocol P (p [x])) (extend-protocol P string (p [x] 1)) (deftype T [] P (p [_] 2)) \
         (defmacro m [] [(implements? P \"x\") (satisfies? P \"x\") (implements? P (T.))]) \
         (js/console.log (pr-str (m)))",
        "[false true true]\n",
    ),
    // Variadic comparisons and identity through input occurrences.
    (
        "(defmacro m [x] [(< 1 3 2) (< 1) (<= 1 1 2) (> 3 2 1) (>= 1 2) (identical? x 1)]) \
         (js/console.log (pr-str (m 1)))",
        "[false true true true false true]\n",
    ),
    // `EmptyList` and `List` are distinct core types.
    (
        "(defprotocol P (p [x])) (extend-protocol P EmptyList (p [_] :empty) List (p [_] :list)) \
         (defmacro m [] [(p '()) (p '(1))]) (js/console.log (pr-str (m)))",
        "[:empty :list]\n",
    ),
    // One signature above arity three pads its arguments.
    (
        "(defprotocol P (p [x a b c])) (deftype T [] P (p [_ a b c] [a (nil? c)])) \
         (defmacro m [] (p (T.) 9)) (js/console.log (pr-str (m)))",
        "[9 true]\n",
    ),
];

#[test]
fn protocol_type_and_record_macros_match_the_legacy_compiler_and_run() {
    for (source, expected) in PROTOCOL_FIXTURES {
        assert_native_matches_legacy(source, expected);
    }
}

#[test]
fn record_helpers_publish_once_at_one_and_two_slots() {
    // A record built by a helper initializer and a function over records:
    // each value is initialized once and shared by the consuming tasks.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-helper-demand/records");
    for jobs in [1, 2] {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let mut stats = light_cljs::CompileStats::new();
        let mut warnings = Vec::new();
        let (program, _) = light_cljs::compiler::analyze_namespace_entry_recorded(
            "rec.app",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut warnings,
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                stats: Some(&mut stats),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("jobs {jobs}: {error}"));
        assert_eq!(
            program_output(&emit_js(&program)),
            "[8080 \"localhost\"] [9090 :tls]\n",
            "jobs {jobs}"
        );
        assert_eq!(stats.native_helpers.per_task, 0, "jobs {jobs}");
        assert!(stats.native_helpers.published >= 2, "jobs {jobs}");
    }
}
