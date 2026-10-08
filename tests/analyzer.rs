use light_cljs::{
    AnalysisErrorKind as Error, CompileError, ExprContext, ExprKind, LocalId, analyze, compile,
    parse,
};

#[test]
fn closures_resolve_to_distinct_persistent_binding_ids() {
    let source = "(let* [x 10 f (fn* [] x)] (let* [x 20] (f)))";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    assert_eq!(
        program
            .locals
            .iter()
            .map(|local| local.name.as_str())
            .collect::<Vec<_>>(),
        ["x", "f", "x"]
    );
    let references: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|expr| {
            if let ExprKind::Local(local) = expr.kind {
                Some(local)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(references, [LocalId(0), LocalId(1)]);
    assert!(
        program
            .exprs
            .iter()
            .all(|expr| source.get(expr.span.clone()).is_some())
    );
    for local in &program.locals {
        assert_eq!(&source[local.span.clone()], local.name);
    }
}

#[test]
fn initializer_references_the_previous_binding() {
    let source = "(let* [x 10 x x] x)";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let references: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|expr| {
            if let ExprKind::Local(local) = expr.kind {
                Some(local)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(references, [LocalId(0), LocalId(1)]);
}

#[test]
fn scope_exit_retains_binding_ids_and_restores_reference_targets() {
    let source = "(let* [x 1
        captured (fn* [] x)
        x (do (let* [x 2] x) x)]
        (let* [x 3] (fn* [x] x))
        (fn* [] x))";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    // Locate records by their source declarations, never their generated names
    // or an assumed arena allocation order. All records survive scope exit.
    let binding = |declaration: &str| {
        let start = source.find(declaration).unwrap();
        LocalId(
            program
                .locals
                .iter()
                .position(|local| local.span == (start..start + 1))
                .unwrap(),
        )
    };
    let outer = binding("x 1");
    let inner_initializer = binding("x 2");
    let rebound = binding("x (do");
    let inner_body = binding("x 3");
    let parameter = binding("x] x");
    let ids = [outer, inner_initializer, rebound, inner_body, parameter];
    assert_eq!(
        ids.into_iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        5
    );
    let references: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            ExprKind::Local(local) => {
                assert_eq!(&source[expr.span.clone()], "x");
                Some(local)
            }
            _ => None,
        })
        .collect();
    // Capture before rebinding; exit initializer scope; exit parameter/body
    // scopes; capture the most recent surviving binding.
    assert_eq!(
        references,
        [outer, inner_initializer, outer, parameter, rebound]
    );
    for local in &program.locals {
        assert_eq!(&source[local.span.clone()], local.name);
    }
}

#[test]
fn named_function_and_parameter_ids_restore_enclosing_local() {
    let source = "(let* [self 1 f (fn* self [self] self)] self)";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let self_bindings: Vec<_> = program
        .locals
        .iter()
        .enumerate()
        .filter(|(_, local)| local.name == "self")
        .map(|(id, _)| LocalId(id))
        .collect();
    assert_eq!(self_bindings.len(), 3);
    let references: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            ExprKind::Local(local) => Some(local),
            _ => None,
        })
        .collect();
    assert_eq!(references, [self_bindings[2], self_bindings[0]]);
}

#[test]
fn different_spellings_have_distinct_ids_even_when_munging_would_collide() {
    let source =
        "(let* [a-b 1 a_b 2 + 3 _PLUS_ 4 class 5 class$ 6 C♯ 7] a-b a_b + _PLUS_ class class$ C♯)";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let references: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|expr| {
            if let ExprKind::Local(local) = expr.kind {
                let binding = &program.locals[local.0];
                assert_eq!(&source[expr.span.clone()], binding.name);
                assert_eq!(&source[binding.span.clone()], binding.name);
                Some(local)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(references.len(), 7);
    assert_eq!(
        references
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        7
    );
}

#[test]
fn namespace_and_alias_references_ignore_same_spelled_lexical_bindings() {
    use light_cljs::{GlobalId, NamespaceInput, analyze_namespaces};
    let sources = [
        "(ns lexical.static) (def value 10) (let* [value 11] (def captured (fn* [] value))) (def read-value (fn* [] value)) (def value 12)",
        "(ns lexical.main (:require [lexical.static :as lib])) (def value 30) (let* [value 40 lib 50] value lexical.main/value lib/value lexical.static/value) value",
    ];
    let syntax = sources.map(|source| parse(source).unwrap());
    let program = analyze_namespaces(&[
        NamespaceInput {
            name: "lexical.static",
            source: sources[0],
            syntax: &syntax[0],
            aliases: &[],
            requires: &[],
        },
        NamespaceInput {
            name: "lexical.main",
            source: sources[1],
            syntax: &syntax[1],
            aliases: &[("lib".into(), "lexical.static".into())],
            requires: &["lexical.static".into()],
        },
    ])
    .unwrap();
    let global = |namespace| {
        GlobalId(
            program
                .globals
                .iter()
                .position(|binding| binding.name == "value" && binding.namespace == namespace)
                .unwrap(),
        )
    };
    let local = |source_index| {
        LocalId(
            program
                .locals
                .iter()
                .position(|binding| binding.name == "value" && binding.source_index == source_index)
                .unwrap(),
        )
    };
    let dep = global("lexical.static");
    let main = global("lexical.main");
    assert_ne!(dep, main);
    assert_ne!(local(0), local(1));
    let references: Vec<_> = program
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::Local(_) | ExprKind::Global(_)))
        .map(|expr| {
            (
                expr.source_index,
                &sources[expr.source_index][expr.span.clone()],
                expr.kind.clone(),
            )
        })
        .collect();
    assert_eq!(
        references,
        [
            (0, "value", ExprKind::Local(local(0))),
            (0, "value", ExprKind::Global(dep)),
            (1, "value", ExprKind::Local(local(1))),
            (1, "lexical.main/value", ExprKind::Global(main)),
            (1, "lib/value", ExprKind::Global(dep)),
            (1, "lexical.static/value", ExprKind::Global(dep)),
            (1, "value", ExprKind::Global(main)),
        ]
    );
    for binding in &program.locals {
        assert_eq!(
            &sources[binding.source_index][binding.span.clone()],
            binding.name
        );
    }
}

#[test]
fn unresolved_initializers_and_exited_scopes_report_exact_byte_spans() {
    // Build each source around the particular rejected occurrence, including
    // multibyte text before it so byte spans cannot accidentally be columns.
    for (before, name, after) in [
        ("(let* [x ", "x", "] x)"),
        ("(let* [λ 1 x ", "later", " later 2] x)"),
        ("(let* [f (fn* [] ", "f", ")] (f))"),
        ("(let* [x (let* [inner 1] inner) y ", "inner", "] y)"),
        ("(let* [λ 1] (let* [x 2] x) ", "x", ")"),
        ("(let* [f (fn* [parameter] parameter)] ", "parameter", ")"),
        ("(let* [f (fn* named [] named)] ", "named", ")"),
        (
            "(let* [f (fn* [] (let* [hidden 1] hidden))] ",
            "hidden",
            ")",
        ),
        (
            "(do (let* [first 1] first) (let* [second ",
            "first",
            "] second))",
        ),
    ] {
        let source = format!("{before}{name}{after}");
        let error = analyze(&source, &parse(&source).unwrap()).unwrap_err();
        assert_eq!(error.kind, Error::UnresolvedSymbol(name.into()), "{source}");
        assert_eq!(
            error.span,
            before.len()..before.len() + name.len(),
            "{source}"
        );
    }
}

#[test]
fn unresolved_names_never_fall_back_to_javascript_globals() {
    for (source, name) in [
        ("x", "x"),
        ("(let* [x x] x)", "x"),
        ("(let* [x y y 10] x)", "y"),
        ("(let* [f (fn* [] f)] (f))", "f"),
        ("(do (let* [x 1] x) x)", "x"),
        ("(do (fn* [x] x) x)", "x"),
        ("(do (fn* f [] f) f)", "f"),
        ("(let* [x 1] x) x", "x"),
        ("(console.log 1)", "console.log"),
        ("(+ 1 2)", "+"),
        ("(if false unknown 0)", "unknown"),
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert_eq!(error.kind, Error::UnresolvedSymbol(name.into()), "{source}");
        assert_eq!(&source[error.span], name, "{source}");
    }
}

#[test]
fn invalid_special_forms_have_source_diagnostics() {
    for source in [
        "(if)",
        "(if true)",
        "(if true 1 2 3)",
        "(let*)",
        "(let* (x 1) x)",
        "(let* [x])",
        "(fn*)",
        "(fn* named)",
        "(fn* (x))",
        "(fn* ())",
        "(nil)",
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert!(
            matches!(error.kind, Error::InvalidForm(_)),
            "{source}: {error}"
        );
        assert!(!error.span.is_empty());
        assert!(source.get(error.span).is_some());
    }
    for source in [
        "(let* [a/b 1] 2)",
        "(let* [a.b 1] 2)",
        "(let* [& 1] 2)",
        "(let* [[x] 1] x)",
        "(let* [1 2] 3)",
        "(fn* [a/b] 1)",
        "(fn* [[x]] x)",
        "(fn* a/b [] 1)",
        "(fn* [&] 1)",
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert!(
            matches!(error.kind, Error::InvalidBinding | Error::Unsupported(_)),
            "{source}: {error}"
        );
    }
    let source = "(fn* [x x] x)";
    let error = analyze(source, &parse(source).unwrap()).unwrap_err();
    assert_eq!(error.kind, Error::DuplicateParameter("x".into()));
    assert_eq!(error.span, 8..9);
}

#[test]
fn fixed_function_signature_boundaries_have_precise_diagnostics() {
    // The single-method spelling has the same binding restrictions as the
    // direct vector spelling. Arity-mismatched calls themselves are executable
    // fixtures: these errors concern declarations, not argument counts.
    for (before, invalid, after, kind) in [
        ("(fn* ", "a/b", " ([] 1))", Error::InvalidBinding),
        ("(fn* named ([", "a/b", "] 1))", Error::InvalidBinding),
        ("(fn* named ([", "a.b", "] 1))", Error::InvalidBinding),
        ("(fn* named ([", "[x]", "] x))", Error::InvalidBinding),
        ("(fn* named ([", "1", "] 1))", Error::InvalidBinding),
        (
            "(fn* named ([x ",
            "x",
            "] x))",
            Error::DuplicateParameter("x".into()),
        ),
        (
            "(fn* named ([x ",
            "&",
            " rest] x))",
            Error::Unsupported("variadic parameters".into()),
        ),
        (
            "",
            "(fn* named ([] 1) ([x] x))",
            "",
            Error::Unsupported("multiple function arities".into()),
        ),
        (
            "(fn* named (",
            "x",
            "))",
            Error::InvalidForm("fn* requires a parameter vector"),
        ),
        ("(", "nil", " 1)", Error::InvalidForm("cannot call nil")),
    ] {
        let source = format!("{before}{invalid}{after}");
        let error = analyze(&source, &parse(&source).unwrap()).unwrap_err();
        assert_eq!(error.kind, kind, "{source}");
        assert_eq!(
            error.span,
            before.len()..before.len() + invalid.len(),
            "{source}"
        );
    }
}

#[test]
fn unsupported_reader_forms_macros_and_language_forms_are_explicit() {
    for source in [
        "[]",
        "{}",
        "#{}",
        "()",
        ":key",
        "\\a",
        "#\"x\"",
        "##Inf",
        "1N",
        "1M",
        "1/2",
        "'x",
        "`x",
        "@x",
        "^:private x",
        "#js []",
        "#?(:cljs 1)",
        "#:a{:x 1}",
        "#(1)",
        "(let [x 1] x)",
        "(fn [] 1)",
        "(cljs.core/let [x 1] x)",
        "(ns app)",
        "(loop* [x 1] x)",
        "(recur)",
        "(quote x)",
        "(set! x 1)",
        "(js* \"1\")",
        "(fn* [x & xs] x)",
        "(fn* ([] 1) ([x] x))",
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert!(
            matches!(error.kind, Error::Unsupported(_)),
            "{source}: {error}"
        );
        assert!(source.get(error.span.clone()).is_some());
        assert!(error.to_string().contains("unsupported"));
    }
}

#[test]
fn numeric_limits_are_reported_instead_of_silently_losing_integer_precision() {
    for source in [
        "9007199254740992",
        "-9007199254740992",
        "+9007199254740993",
        "-9007199254740993",
        "0x20000000000000",
        "-0X20000000000000",
        "+0400000000000000000",
        "-0400000000000000000",
        "2r100000000000000000000000000000000000000000000000000000",
        "-2R100000000000000000000000000000000000000000000000000000",
        "36r2gosa7pa2gw",
        "-36R2gosa7pa2gw",
        "99999999999999999999999999999",
        "-99999999999999999999999999999",
        "1.7976931348623159e308",
        "-1.7976931348623159e308",
        "1e999",
        "-1E+999",
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert_eq!(error.kind, Error::NumberOutOfRange, "{source}");
        assert_eq!(error.span, 0..source.len());

        // Byte offsets must still identify the whole literal after non-ASCII
        // source and newlines, including an unselected branch.
        let prefix = "(let* [λ 0]\n  (if false ";
        let nested = format!("{prefix}{source} λ))");
        let CompileError::Analysis(error) = compile(&nested).unwrap_err() else {
            panic!("numeric boundary reached the wrong stage: {nested}")
        };
        assert_eq!(error.kind, Error::NumberOutOfRange, "{nested}");
        assert_eq!(error.span, prefix.len()..prefix.len() + source.len());
        assert!(
            error
                .to_string()
                .contains("safe integers and finite doubles")
        );
    }
}

#[test]
fn unsupported_number_categories_have_precise_diagnostics() {
    for (literal, category) in [
        ("0N", "big integers"),
        ("-42N", "big integers"),
        ("+0xFFN", "big integers"),
        ("077N", "big integers"),
        ("1M", "decimal literals"),
        ("-1.25M", "decimal literals"),
        ("1e-10M", "decimal literals"),
        ("09M", "decimal literals"),
        ("1/2", "ratios"),
        ("1/1", "ratios"),
        ("-13/2", "ratios"),
        ("+01/002", "ratios"),
        ("##NaN", "symbolic numbers"),
        ("##Inf", "symbolic numbers"),
        ("##-Inf", "symbolic numbers"),
    ] {
        let prefix = "(let* [λ 0]\n  (do ";
        let source = format!("{prefix}{literal} λ))");
        let CompileError::Analysis(error) = compile(&source).unwrap_err() else {
            panic!("unsupported number reached the wrong stage: {source}")
        };
        assert_eq!(error.kind, Error::Unsupported(category.into()), "{literal}");
        assert_eq!(error.span, prefix.len()..prefix.len() + literal.len());
        assert!(error.to_string().contains(category));
    }
}

#[test]
fn context_tracks_discarded_returned_and_argument_values() {
    let source = "(fn* [f] (f 1) (if (f 2) (f 3) (f 4)))";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let contexts: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|expr| {
            if matches!(expr.kind, ExprKind::Call { .. }) {
                Some(expr.context)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        contexts,
        [
            ExprContext::Discard,
            ExprContext::Value,
            ExprContext::Return,
            ExprContext::Return
        ]
    );
}

#[test]
fn analysis_owns_its_values_and_compile_distinguishes_error_stages() {
    let program = {
        let source = String::from("(let* [λ \"\\uD800\"] λ)");
        analyze(&source, &parse(&source).unwrap()).unwrap()
    };
    assert_eq!(program.strings, [vec![0xd800]]);
    assert_eq!(program.locals[0].name, "λ");
    assert!(matches!(compile("["), Err(CompileError::Parse(_))));
    assert!(matches!(compile("x"), Err(CompileError::Analysis(_))));
}

#[test]
fn global_identity_is_stable_and_distinct_from_lexical_and_host_identity() {
    use light_cljs::{GlobalId, JsGlobalId};
    let source = "(def x 10) (def read (fn* [] x)) (def x 20) (let* [x 30] x) cljs.user/x js/Math.sqrt js/Math.sqrt";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    assert_eq!(program.globals.len(), 2);
    assert_eq!(program.globals[0].name, "x");
    assert_eq!(program.globals[0].namespace, "cljs.user");
    let definitions: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            ExprKind::Def { global, .. } => Some(global),
            _ => None,
        })
        .collect();
    assert_eq!(definitions, [GlobalId(0), GlobalId(1), GlobalId(0)]);
    assert!(
        program
            .exprs
            .iter()
            .any(|expr| expr.kind == ExprKind::Local(LocalId(0)))
    );
    assert_eq!(program.js_globals.len(), 1);
    assert!(
        program
            .exprs
            .iter()
            .any(|expr| expr.kind == ExprKind::JsGlobal(JsGlobalId(0)))
    );
}

#[test]
fn definitions_and_interop_reject_unsupported_shapes_with_spans() {
    for source in [
        "(def)",
        "(def x 1 2)",
        "(def a/b 1)",
        "(def a.b 1)",
        "(def [x] 1)",
        "(.-length)",
        "(.-length \"x\" 2)",
        "(.slice)",
        "(js/Math..sqrt 4)",
        "js/a-b",
        "(.-a-b js/lightHost)",
        "(.a/b js/lightHost)",
        "(js*/alert)",
        "(let* [x (def a)] x)",
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert!(!error.span.is_empty(), "{source}");
        assert!(source.get(error.span).is_some(), "{source}");
    }
}

#[test]
fn globals_are_source_ordered_and_definition_self_reference_is_registered_first() {
    for source in [
        "x (def x 1)",
        "(def f (fn* [] later)) (def later 1)",
        "other/x",
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert!(
            matches!(error.kind, Error::UnresolvedSymbol(_)),
            "{source}: {error}"
        );
    }
    let source = "(def f (fn* [done] (if done 7 (f true))))";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    assert!(
        program
            .exprs
            .iter()
            .any(|expr| expr.kind == ExprKind::Global(light_cljs::GlobalId(0)))
    );
}
