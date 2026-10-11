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
            "(do (let* [earlier 1] earlier) (let* [second ",
            "earlier",
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
fn unresolved_bare_names_remain_strict_while_dotted_host_paths_resolve() {
    for (source, name) in [
        ("x", "x"),
        ("(let* [x x] x)", "x"),
        ("(let* [x y y 10] x)", "y"),
        ("(let* [f (fn* [] f)] (f))", "f"),
        ("(do (let* [x 1] x) x)", "x"),
        ("(do (fn* [x] x) x)", "x"),
        ("(do (fn* f [] f) f)", "f"),
        ("(let* [x 1] x) x", "x"),
        ("(if false unknown 0)", "unknown"),
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert_eq!(error.kind, Error::UnresolvedSymbol(name.into()), "{source}");
        assert_eq!(&source[error.span], name, "{source}");
    }
    let source = "(console.log 1)";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    assert_eq!(program.js_globals.len(), 1);
    assert_eq!(program.js_globals[0].path, ["console", "log"]);
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
            matches!(
                error.kind,
                Error::InvalidBinding | Error::Unsupported(_) | Error::InvalidForm(_)
            ),
            "{source}: {error}"
        );
    }
    let source = "(fn* [x x] x)";
    assert!(analyze(source, &parse(source).unwrap()).is_ok());
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
            "",
            "(fn* named ([x] 1) ([y] y))",
            "",
            Error::InvalidForm("fn* cannot have two overloads with the same arity"),
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
    for source in ["#?(:cljs 1)", "(ns app)"] {
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
fn numeric_literals_use_cljs_number_rounding_without_safe_integer_restrictions() {
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
        let program = analyze(source, &parse(source).unwrap()).unwrap();
        assert!(
            program
                .exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::Number(_))),
            "{source}"
        );
        let nested = format!("(let* [λ 0] (if false {source} λ))");
        compile(&nested).unwrap();
    }
}

#[test]
fn additional_numeric_categories_compile_in_nested_contexts() {
    for (literal, _category) in [
        ("0N", "big integers"),
        ("-42N", "big integers"),
        ("+0xFFN", "big integers"),
        ("077N", "big integers"),
        ("1M", "decimal literals"),
        ("-1.25M", "decimal literals"),
        ("1e-10M", "decimal literals"),
        ("09M", "decimal literals"),
        ("1/1", "ratios"),
        ("##NaN", "symbolic numbers"),
        ("##Inf", "symbolic numbers"),
        ("##-Inf", "symbolic numbers"),
    ] {
        let prefix = "(let* [λ 0]\n  (do ";
        let source = format!("{prefix}{literal} λ))");
        compile(&source).unwrap();
    }
    let source = "(if false 1/2 1)";
    let CompileError::Analysis(error) = compile(source).unwrap_err() else {
        panic!()
    };
    assert_eq!(error.span, 10..13);
    assert_eq!(error.kind, Error::NumberOutOfRange);
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
fn definitions_and_interop_distinguish_valid_paths_from_malformed_shapes() {
    for source in [
        "(def)",
        "(def x 1 2)",
        "(def a/b 1)",
        "(def [x] 1)",
        "(.-length)",
        "(.-length \"x\" 2)",
        "(.slice)",
        "(js*/alert)",
        "(let* [x (def a)] x)",
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert!(!error.span.is_empty(), "{source}");
        assert!(source.get(error.span).is_some(), "{source}");
    }
    for source in ["(def a.b 1)", "(js/Math..sqrt 4)", "(.a/b js/lightHost)"] {
        analyze(source, &parse(source).unwrap())
            .unwrap_or_else(|error| panic!("{source}: {error}"));
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

#[test]
fn core_functions_resolve_after_locals_and_globals_and_by_explicit_qualifier() {
    use light_cljs::CoreFnId;
    let source = "vector cljs.core/get clojure.core/nth (let* [get 1] get) (def vector 2) vector";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let resolved: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            ExprKind::CoreFn(core) => Some(core),
            _ => None,
        })
        .collect();
    assert_eq!(resolved, [CoreFnId::Vector, CoreFnId::Get, CoreFnId::Nth]);
    let ExprKind::Do(body) = &program.exprs[program.root.0].kind else { panic!("program body") };
    assert!(matches!(program.exprs[body.last().unwrap().0].kind, ExprKind::Global(_)));
    for name in ["cljs.core/not-a-core-function", "other/vector"] {
        assert!(matches!(
            analyze(name, &parse(name).unwrap()).unwrap_err().kind,
            Error::UnresolvedSymbol(_)
        ));
    }
}

#[test]
fn keyword_literals_own_names_and_resolve_namespace_aliases_without_macro_execution() {
    use light_cljs::{NamespaceInput, analyze_namespaces};
    let program = {
        let source = "(ns owned.names) [:plain :explicit/x ::local ::a/item]".to_owned();
        let syntax = parse(&source).unwrap();
        analyze_namespaces(&[NamespaceInput {
            name: "owned.names",
            source: &source,
            syntax: &syntax,
            aliases: &[("a".into(), "library.data".into())],
            requires: &["library.data".into()],
        }])
        .unwrap()
    };
    let names: Vec<_> = program
        .named_literals
        .iter()
        .map(|name| {
            (
                name.namespace
                    .map(|id| String::from_utf16(&program.strings[id.0]).unwrap()),
                String::from_utf16(&program.strings[name.name.0]).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            (None, "plain".into()),
            (Some("explicit".into()), "x".into()),
            (Some("owned.names".into()), "local".into()),
            (Some("library.data".into()), "item".into()),
        ]
    );
    let standalone = analyze("::here", &parse("::here").unwrap()).unwrap();
    let ns = standalone.named_literals[0].namespace.unwrap();
    assert_eq!(
        String::from_utf16(&standalone.strings[ns.0]).unwrap(),
        "cljs.user"
    );
    let source = "(let* [λ 1] ::missing/x)";
    let error = analyze(source, &parse(source).unwrap()).unwrap_err();
    assert!(matches!(error.kind, Error::UnresolvedSymbol(_)));
    assert_eq!(&source[error.span], "::missing/x");
}

#[test]
fn literal_keyword_arity_rejections_are_located_even_in_dead_branches() {
    for call in ["(:x)", "(:x [] 1 2)"] {
        let prefix = "(if false ";
        let source = format!("{prefix}{call} nil)");
        let error = analyze(&source, &parse(&source).unwrap()).unwrap_err();
        assert_eq!(
            error.kind,
            Error::InvalidForm("literal keyword calls require one or two arguments")
        );
        assert_eq!(error.span, prefix.len()..prefix.len() + call.len());
    }
}

#[test]
fn runtime_inclusion_preserves_primitive_output_and_is_once_per_combined_unit() {
    assert_eq!(
        compile("42").unwrap(),
        "(() => {\n  const truth = (value) => value != null && value !== false;\n  return 42;\n})()\n"
    );
    for source in ["[1 2]", ":x", "vector", "(do :x [1 2] (vector :y))"] {
        let js = compile(source).unwrap();
        assert_eq!(js.matches("\n  const rt = ").count(), 1, "{source}");
    }
    let source = "(ns app.main (:require [cljs.core :as c])) 42";
    let syntax = parse(source).unwrap();
    let program = light_cljs::analyze_namespaces(&[light_cljs::NamespaceInput {
        name: "app.main",
        source,
        syntax: &syntax,
        aliases: &[("c".into(), "cljs.core".into())],
        requires: &["cljs.core".into()],
    }])
    .unwrap();
    assert!(!light_cljs::emit_js(&program).contains("const rt ="));
}

#[test]
fn quote_lowers_owned_data_without_runtime_resolution_or_macro_execution() {
    use light_cljs::ExprKind;
    let source = "'[(observe :bad) unresolved-name 'a/x () \"\\ud800\"]";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    assert!(program.locals.is_empty());
    assert!(program.globals.is_empty());
    assert!(program.js_globals.is_empty());
    assert!(!program.exprs.iter().any(|e| matches!(
        e.kind,
        ExprKind::Call { .. } | ExprKind::Local(_) | ExprKind::Global(_) | ExprKind::CoreFn(_)
    )));
    assert!(program.strings.contains(&vec![0xd800]));
    assert_eq!(program.named_literals.len(), 5);
    for expr in &program.exprs {
        assert!(source.get(expr.span.clone()).is_some());
    }
    for source in ["()", "'x", "(quote x)", "''x", "(quote (quote x))"] {
        assert!(compile(source).unwrap().contains("const rt ="));
    }
    for source in ["(quote)", "(quote x y)"] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert_eq!(
            error.kind,
            Error::InvalidForm("quote requires one argument")
        );
        assert_eq!(error.span, 0..source.len());
    }
}

#[test]
fn reader_data_compiles_without_execution_even_in_unselected_branches() {
    for datum in ["\\a", "^:m x", "#js []", "#:n{:x 1}", "`x"] {
        let prefix = "(if false '[ok ";
        let source = format!("{prefix}{datum}] nil)");
        compile(&source).unwrap();
    }
}

#[test]
fn repeated_quoted_literals_grow_linearly_without_duplicate_runtime_preludes() {
    let size = |n| {
        let source = format!("[{}]", "'[x (a/y 1) :k] ".repeat(n));
        let js = compile(&source).unwrap();
        assert_eq!(js.matches("\n  const rt = ").count(), 1);
        js.len()
    };
    let sizes = [size(1), size(10), size(100)];
    assert_eq!(sizes[2] - sizes[1], 10 * (sizes[1] - sizes[0]));
}

#[test]
fn map_literals_lower_in_source_order_and_reader_duplicates_keep_number_categories() {
    for (source, duplicate) in [
        ("{:a 1 :a 2}", ":a"),
        ("{[1 2] 1 (1 2) 2}", "(1 2)"),
        ("{{:a 1 :b 2} 1 {:b 2 :a 1} 2}", "{:b 2 :a 1}"),
        ("{1 1 0x1 2}", "0x1"),
        ("{1.0 1 1e0 2}", "1e0"),
        ("{'x 1 (quote x) 2}", "(quote x)"),
        ("{::x 1 :cljs.user/x 2}", ":cljs.user/x"),
        ("'{\"a\" 1 \"\\u0061\" 2}", "\"\\u0061\""),
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert_eq!(
            error.kind,
            Error::InvalidForm("duplicate map key"),
            "{source}"
        );
        assert_eq!(&source[error.span], duplicate);
    }
    for source in [
        "{1 :integer 1.0 :float}",
        "'{[1] :integer [1.0] :float}",
        "'{{1 2} :integer {1.0 2} :float}",
    ] {
        assert!(compile(source).is_ok(), "{source}");
    }
    let source =
        "{(js/parseInt \"1\") (js/parseInt \"2\") (js/parseInt \"3\") (js/parseInt \"4\")}";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let items = program
        .exprs
        .iter()
        .find_map(|expr| {
            if let ExprKind::Map(items) = &expr.kind {
                Some(items)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(items.len(), 4);
    let spans = items
        .iter()
        .map(|id| program.exprs[id.0].span.start)
        .collect::<Vec<_>>();
    assert!(spans.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn sets_lower_owned_values_and_reject_duplicate_readable_forms_with_spans() {
    for (source, duplicate) in [
        ("#{1 0x1}", "0x1"),
        ("#{1.0 1e0}", "1e0"),
        ("#{:a :a}", ":a"),
        ("#{nil nil}", "nil"),
        ("#{[1 2] [1 2]}", "[1 2]"),
        ("#{#{1 2} #{2 1}}", "#{2 1}"),
        ("#{ {:a 1 :b 2} {:b 2 :a 1}}", "{:b 2 :a 1}"),
        ("'#{[1 2] (1 2)}", "(1 2)"),
        ("'#{x x}", "x"),
        ("#{'x (quote x)}", "(quote x)"),
        ("#{::x :cljs.user/x}", ":cljs.user/x"),
        ("(let* [x 1] #{x x})", "x"),
        (
            "#{(js/parseInt \"1\") (js/parseInt \"1\")}",
            "(js/parseInt \"1\")",
        ),
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert_eq!(
            error.kind,
            Error::InvalidForm("duplicate set member"),
            "{source}"
        );
        assert_eq!(&source[error.span.clone()], duplicate);
        assert_eq!(error.span.start, source.rfind(duplicate).unwrap());
    }
    for source in [
        "#{1 1.0}",
        "'#{[1] [1.0]}",
        "(let* [x 1 y 1] #{x y})",
        "#{[1 2] (list 1 2)}",
        "'#{unresolved (not-a-call) {:a #{1}}}",
    ] {
        assert!(compile(source).is_ok(), "{source}");
    }
    let source = "#{(js/parseInt \"1\") (js/parseInt \"2\")}";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let items = program
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::Set(items) => Some(items),
            _ => None,
        })
        .unwrap();
    assert_eq!(items.len(), 2);
    assert!(program.exprs[items[0].0].span.start < program.exprs[items[1].0].span.start);
    let source = "{#{1 2} 1 #{2 1} 2}";
    assert_eq!(
        analyze(source, &parse(source).unwrap()).unwrap_err().kind,
        Error::InvalidForm("duplicate map key")
    );
}

#[test]
fn reduction_core_values_have_resolved_ids_without_native_macro_initialization() {
    use light_cljs::CoreFnId;
    let source =
        "reduce + reduced reduced? unreduced deref cljs.core/reduce clojure.core/+ reduce-kv";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let resolved: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            ExprKind::CoreFn(core) => Some(core),
            _ => None,
        })
        .collect();
    assert_eq!(
        resolved,
        [
            CoreFnId::Reduce,
            CoreFnId::Add,
            CoreFnId::Reduced,
            CoreFnId::IsReduced,
            CoreFnId::Unreduced,
            CoreFnId::Deref,
            CoreFnId::Reduce,
            CoreFnId::Add,
            CoreFnId::ReduceKv
        ]
    );
    for (name, expected) in [("transduce", CoreFnId::Transduce), ("completing", CoreFnId::Completing), ("ensure-reduced", CoreFnId::EnsureReduced)] {
        let program = analyze(name, &parse(name).unwrap()).unwrap();
        assert!(program.exprs.iter().any(|expr| expr.kind == ExprKind::CoreFn(expected)), "{name}");
    }
}

#[test]
fn recur_rejects_non_tail_targets_and_cross_try_with_located_errors() {
    for source in [
        "(recur)",
        "(loop* [x 0] (inc (recur 1)))",
        "(loop* [x 0] (if (recur 1) 2 3))",
        "(loop* [x 0] (do (recur 1) 2))",
        "(loop* [x 0] (let* [y (recur 1)] y))",
        "(loop* [x 0] (let* [y (let* [] (recur 1))] y))",
        "(loop* [x 0] [(recur 1)])",
        "(loop* [x 0] (def bad (recur 1)))",
        "(loop* [x 0] (recur (recur 1)))",
        "(loop* [] (try (recur)))",
        "(loop* [] (try (catch js/Error t (recur))))",
        "(loop* [] (try (catch :default t (recur))))",
        "(loop* [] (try (finally (recur))))",
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert_eq!(
            error.kind,
            Error::InvalidForm("Can't recur here"),
            "{source}: {error}"
        );
        assert!(source[error.span].starts_with("(recur"), "{source}");
    }
    for source in [
        "(loop* [x 1 y 2] (recur 3))",
        "(loop* [x 0] ((fn* [] (recur 1))))",
        "(fn* [n] (loop* [] (recur n)))",
        "(fn* [n & xs] (recur n))",
        "(fn* ([n] (recur n 1)) ([n x] x))",
        "(reify IFn (-invoke [self n] (recur self n)))",
        "(reify IFn (-invoke [self n] (recur)))",
    ] {
        assert!(
            compile(source)
                .unwrap_err()
                .to_string()
                .contains("recur argument count mismatch"),
            "{source}"
        );
    }
}

#[test]
fn recurrence_targets_are_distinct_and_macro_generated_spans_remain_at_the_call() {
    let source = "(fn* [x] (loop* [y x] (if y (recur false) (fn* [] (recur)))))";
    let p = analyze(source, &parse(source).unwrap()).unwrap();
    let targets = p
        .exprs
        .iter()
        .filter_map(|e| {
            if let ExprKind::Recur { target, .. } = e.kind {
                Some(target)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 2);
    assert_ne!(targets[0], targets[1]);
    assert_eq!(p.recur_targets[targets[0].0].params.len(), 1);
    assert_eq!(p.recur_targets[targets[1].0].params.len(), 0);
    let mut host = light_cljs::MacroHost::new();
    let source = "(defmacro again [] '(recur)) ; λ\n(again)";
    let CompileError::Analysis(error) =
        light_cljs::compile_with_macros(source, &mut host).unwrap_err()
    else {
        panic!()
    };
    assert_eq!(&source[error.span.clone()], "(again)");
    assert!(error.to_string().contains("recur"));
}

#[test]
fn letfn_allocates_group_identities_before_initializers_and_restores_scope() {
    let source =
        "(let* [g 9] (letfn* [f (fn* [] g) g (fn* [] f)] (letfn* [g (fn* [] g)] (g)) (f)) g)";
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    assert_eq!(
        program
            .locals
            .iter()
            .map(|l| l.name.as_str())
            .collect::<Vec<_>>(),
        ["g", "f", "g", "g"]
    );
    let refs: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|e| match e.kind {
            ExprKind::Local(local) => Some(local),
            _ => None,
        })
        .collect();
    assert_eq!(
        refs,
        [
            LocalId(2),
            LocalId(1),
            LocalId(3),
            LocalId(3),
            LocalId(1),
            LocalId(0)
        ]
    );
    let groups: Vec<_> = program
        .exprs
        .iter()
        .filter_map(|e| match &e.kind {
            ExprKind::LetFn { bindings, .. } => Some(bindings),
            _ => None,
        })
        .collect();
    assert_eq!(groups.len(), 2);
    assert_eq!(
        groups[1].iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [LocalId(1), LocalId(2)]
    );
    for local in &program.locals {
        assert_eq!(&source[local.span.clone()], local.name);
    }
}

#[test]
fn malformed_letfn_reports_the_binding_or_initializer_source_span() {
    for (before, rejected, after, message) in [
        ("", "(letfn*)", "", "binding vector"),
        ("(letfn* ", "()", " 1)", "binding vector"),
        ("(letfn* ", "[f]", " 1)", "even number"),
        ("(letfn* [", "a/b", " (fn* [] 1)] 1)", "binding must"),
        ("(letfn* [", "[f]", " (fn* [] 1)] 1)", "binding must"),
        ("(letfn* [f ", "1", "] 1)", "initializers must be functions"),
        (
            "(letfn* [f ",
            "(do (fn* [] 1))",
            "] 1)",
            "initializers must be functions",
        ),
        (
            "(letfn* [f ",
            "(if true (fn* [] 1) (fn* [] 2))",
            "] 1)",
            "initializers must be functions",
        ),
        (
            "(letfn* [f (fn* [] ",
            "missing",
            ")] (f))",
            "unresolved symbol",
        ),
    ] {
        let prefix = "; λ\n";
        let source = format!("{prefix}{before}{rejected}{after}");
        let error = analyze(&source, &parse(&source).unwrap()).unwrap_err();
        let start = prefix.len() + before.len();
        assert_eq!(
            error.span,
            start..start + rejected.len(),
            "{source}: {error}"
        );
        assert!(error.to_string().contains(message), "{source}: {error}");
    }
    for source in [
        "(letfn [x 1] x)",
        "(letfn [(42 [] 1)] nil)",
        "(letfn [(f [x &] x)] (f 1))",
    ] {
        let CompileError::Analysis(error) = compile(source).unwrap_err() else {
            panic!("{source}");
        };
        assert!(
            error.span.start < error.span.end && error.span.end <= source.len(),
            "{source}: {error}"
        );
        assert!(!error.to_string().contains("unsupported construct: letfn"));
    }
}

#[test]
fn malformed_destructuring_and_conditions_retain_source_locations() {
    for source in [
        "(let [[a &] [1]] a)",
        "(let [[a & xs extra] [1]] a)",
        "(let [[a :as] [1]] a)",
        "(let [{:keys x} {}] x)",
        "(let [{:keys [1]} {}] nil)",
        "(let [{:keys [x] :or 1} {}] x)",
        "(loop [[a & xs extra] []] a)",
        "(fn [[a & xs extra]] a)",
        "(defn f [{:as [x]}] x)",
        "(fn [x] {:post [true]} (recur x))",
        "(fn [x] {:pre 1} x)",
        "(reify IFn (-invoke [_ [x &]] x))",
        "(defprotocol P (run [this x])) (reify P (run [_ {:keys x}] x))",
        "(defprotocol P (run [this x])) (deftype T [a] P (run [_ [x &]] a))",
    ] {
        let error = light_cljs::compile(source).unwrap_err();
        let light_cljs::CompileError::Analysis(error) = error else {
            panic!("expected analysis diagnostic for {source}: {error}")
        };
        assert!(
            !error.span.is_empty() && error.span.end <= source.len(),
            "{source}: {error:?}"
        );
        assert!(
            source.is_char_boundary(error.span.start) && source.is_char_boundary(error.span.end)
        );
        let selected = &source[error.span.clone()];
        assert!(
            selected.contains('[') || selected.contains("recur") || selected.contains(":pre"),
            "{source}: {selected:?}"
        );
    }
}

#[test]
fn exception_forms_validate_clauses_and_preserve_recur_boundaries() {
    for (source, fragment) in [
        ("(throw)", "(throw)"),
        ("(throw 1 2)", "(throw 1 2)"),
        ("(try (catch js/Error))", "(catch js/Error)"),
        ("(try (catch :default))", "(catch :default)"),
        ("(try (catch js/Error bad/e))", "bad/e"),
        ("(try (catch :default [e]))", "[e]"),
        ("(try (catch js/Error e) 2)", "2"),
        ("(try (finally) 2)", "2"),
        ("(try (finally) (finally 1))", "(finally 1)"),
        ("(try (finally) (catch :default e))", "(catch :default e)"),
        (
            "(try (catch :default e) (catch js/Error f))",
            "(catch js/Error f)",
        ),
        ("(loop* [] (throw (recur)))", "(recur)"),
        ("(loop* [] (try (catch (recur) e)))", "(recur)"),
    ] {
        let CompileError::Analysis(error) = compile(source).unwrap_err() else {
            panic!("{source}")
        };
        assert_eq!(&source[error.span.clone()], fragment, "{source}: {error}");
    }
    for source in [
        "(try)",
        "(try (catch :default e))",
        "(try (finally))",
        "(try (catch :default e) (finally))",
        "(try (catch js/Error e) (catch :default e) (finally))",
        "(let [finally identity] (finally 1))",
        "(loop* [] (try (finally)) (recur))",
        "(try (fn* [x] (recur x)))",
        "(try (catch :default e (loop* [] (recur))))",
    ] {
        compile(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    }
}

#[test]
fn definitions_and_temporary_scope_validation_report_source_errors() {
    for (source, message) in [
        ("(def ^:const x 1) (set! x 2)", "Can't set! a constant"),
        ("(def ^:const x 1) (def x 2)", "Can't redefine a constant"),
        ("(let [x 1] (set! x 2))", "Can't set! local var"),
        (
            "(def x 1) (let [x 2] (binding [x 3] x))",
            "Can't set! local var",
        ),
        ("(set! 1 2)", "Can't set! local var"),
        ("(set! x)", "set! requires"),
        ("(binding)", "binding requires a vector"),
        ("(binding [x])", "even number"),
        ("(binding [1 2])", "global symbols"),
        ("(with-redefs [missing 2])", "unresolved symbol"),
        ("(exists? absent) absent", "unresolved symbol"),
        ("(resolve (quote absent)) (var absent)", "unresolved symbol"),
        ("(resolve x)", "quoted symbol"),
        ("(resolve 1)", "quoted symbol"),
        ("(var)", "one global symbol"),
        ("(var nil)", "one global symbol"),
        ("(defonce x)", "def requires"),
        ("(declare [x])", "binding must"),
        (
            "(def x 1) (loop [] (binding [x 2] (recur)))",
            "Can't recur here",
        ),
        (
            "(def x 1) (loop [] (with-redefs [x 2] (recur)))",
            "Can't recur here",
        ),
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert!(error.to_string().contains(message), "{source}: {error}");
        assert!(source.get(error.span.clone()).is_some());
    }
    for source in [
        "(declare f) (defn g [] (f)) (defn f [] 1) (g)",
        "(def x 1) (binding [x 2] (loop [n 1] (if (zero? n) x (recur (dec n)))))",
        "(def x 1) (def x \"doc\" 2) (set! x 3)",
    ] {
        assert!(compile(source).is_ok(), "{source}");
    }
    for source in [
        "(def x 1) x",
        "(defonce x 1) x",
        "(def x 1) (binding [x 2] x)",
    ] {
        assert!(
            !compile(source).unwrap().contains("const rt ="),
            "unused definition metadata must not pull in the collection runtime: {source}"
        );
    }
}

#[test]
fn accepted_nesting_analyzes_on_the_default_thread_stack() {
    // Exercise both sides of list/value dispatch and larger recursive handlers.
    // Do not increase this test's stack: the parser accepts all these sources.
    for (opening, closing) in [
        ("(let* [] ", ")"),
        ("(fn* [] ", ")"),
        ("(if true ", " nil)"),
        ("(identity ", ")"),
        ("(do ", ")"),
        ("[", "]"),
        ("{:value ", "}"),
        ("#{", "}"),
    ] {
        let source = format!("{}0{}", opening.repeat(127), closing.repeat(127));
        let syntax = parse(&source).unwrap();
        analyze(&source, &syntax).unwrap_or_else(|error| panic!("nested {opening}: {error}"));
    }

    let source = format!("{}missing{}", "(let* [] ".repeat(127), ")".repeat(127));
    let error = analyze(&source, &parse(&source).unwrap()).unwrap_err();
    assert_eq!(&source[error.span], "missing");
}
