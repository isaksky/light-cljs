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
        ("(js/alert 1)", "js/alert"),
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
        "(def x 1)",
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
        "0x20000000000000",
        "99999999999999999999999999999",
        "1e999",
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert_eq!(error.kind, Error::NumberOutOfRange, "{source}");
        assert_eq!(error.span, 0..source.len());
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
