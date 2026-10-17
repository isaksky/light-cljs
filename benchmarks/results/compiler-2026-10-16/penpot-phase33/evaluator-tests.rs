    #[test]
    fn shared_macro_arguments_restore_provenance_after_environment_import() {
        use crate::compiler_data::{DataBuilder, Name, Origin, Source};
        let c = DataBuilder;
        let keyword = c.keyword(Name::new(None, "shared"));
        let cases = [
            (Datum::new(DatumKind::Ratio { numerator: 3, denominator: 2 }), Datum::new(DatumKind::Double(1.5))),
            (c.integer(9007199254740993), Datum::new(DatumKind::Double(9007199254740992.0))),
            (Datum::new(DatumKind::Character(97)), c.string(vec![97])),
            (keyword.clone(), keyword.with_metadata(c.map(&[(c.keyword(Name::new(None, "environment")), c.boolean(true))]).unwrap())),
        ];
        for (argument, environment) in cases {
            let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
            let source = Source::new("shared.cljs", "(identity value)");
            let data = c.list(&[c.symbol(Name::new(None, "identity")), argument.clone()]).unwrap();
            let form = Form::from_data(data, source.span(0..source.text().len()).unwrap(), Origin::Source);
            let limits = ConversionLimits::default();
            let input = evaluator.import_macro_form(&form, limits).unwrap();
            let env = evaluator.import_environment(&environment, limits).unwrap();
            let access = evaluator.evaluate_js("form => rt.core.nth(form, 1)").unwrap();
            let naive = evaluator.call(access, &[input]).unwrap();
            let stale = evaluator.export_data(naive, limits).unwrap();
            assert_ne!(stale.identity(), argument.identity(), "the naive shortcut must lose this input's provenance");
            evaluator.release(naive).unwrap();
            let arguments = evaluator.macro_arguments(input, &form, limits).unwrap();
            assert_eq!(arguments.len(), 1);
            let output = evaluator.export_data(arguments[0], limits).unwrap();
            assert_eq!(output.identity(), argument.identity(), "shared input must recover the exact original datum");
            let same = evaluator.evaluate_js("(form, argument) => rt.core.nth(form, 1) === argument").unwrap();
            let result = evaluator.call(same, &[input, arguments[0]]).unwrap();
            assert_eq!(evaluator.print(result).unwrap(), "true");
            for value in [input, env, access, arguments[0], same, result] { evaluator.release(value).unwrap(); }
        }
    }

    #[test]
    fn failed_macro_environments_release_pending_input_graphs() {
        use crate::compiler_data::{CompilerLiteralKind, DataBuilder, ExpansionOrigin, Name, Source};
        let evaluator = Evaluator::interactive(EvaluatorOptions::default()).unwrap();
        let function = evaluator.evaluate_js("(_form, _env, value) => value").unwrap();
        let mut host = crate::MacroHost::new();
        let form = host.read_compiler_form(&Source::new("pending.cljs", "(identity [some-name])")).unwrap().unwrap();
        let environment = Datum::new(DatumKind::CompilerLiteral {
            kind: CompilerLiteralKind::Uuid,
            payload: DataBuilder.string("not-valid".encode_utf16().collect::<Vec<_>>()),
        });
        let live = evaluator.bridge.borrow().live_provenance.clone();
        for _ in 0..25 {
            let error = host.expand_javascript(&evaluator, function, &form, &environment, ExpansionOrigin {
                macro_name: Name::new(Some("cljs.user"), "identity"), call: form.span(), definition: None, helpers: vec![],
            }).unwrap_err();
            assert!(error.contains("invalid UUID"), "{error}");
        }
        evaluator.collect_garbage();
        assert_eq!(live.get(), 1, "only the latest scalar provenance may remain");
        evaluator.release(function).unwrap();
        drop(evaluator);
        assert_eq!(live.get(), 0);
    }

    #[test]
    fn failed_argument_snapshots_drop_the_pending_graph_and_partial_results() {
        use crate::compiler_data::Source;
        let evaluator = Evaluator::interactive(EvaluatorOptions::default()).unwrap();
        let mut host = crate::MacroHost::new();
        let form = host.read_compiler_form(&Source::new("arguments.cljs", "(identity [first] #js [second])")).unwrap().unwrap();
        let mutate = evaluator.evaluate_js("form => { rt.core.nth(form, 2).push(() => null); return null; }").unwrap();
        let live = evaluator.bridge.borrow().live_provenance.clone();
        let limits = ConversionLimits::default();
        for _ in 0..25 {
            let input = evaluator.import_macro_form(&form, limits).unwrap();
            let result = evaluator.call(mutate, &[input]).unwrap();
            evaluator.release(result).unwrap();
            let error = evaluator.macro_arguments(input, &form, limits).unwrap_err();
            assert!(error.to_string().contains("Unsupported macro result function"), "{error}");
            evaluator.release(input).unwrap();
        }
        evaluator.collect_garbage();
        assert_eq!(live.get(), 0, "failed preparation must not retain snapshots or descriptor graphs");
        evaluator.release(mutate).unwrap();
    }
