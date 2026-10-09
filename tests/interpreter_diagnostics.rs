use light_cljs::interpreter::{Context, Name, OutputBuffer, ValueView};

#[test]
fn reports_attribute_macro_definitions_and_repeated_generation_without_changing_effects() {
    let mut c = Context::new();
    c.set_diagnostics(true);
    let definitions = c.source("macros.clj", "(defmacro twice [x] `(+ ~x ~x))");
    c.eval_source(&definitions).unwrap();
    let source = c.source("consumer.clj", "(twice 21)");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let first = c.analyze_form(&form).unwrap();
    let second = c.analyze_form(&form).unwrap();
    for code in [&first, &second, &first] {
        assert_eq!(
            c.eval_analyzed(code).unwrap().inspect(),
            ValueView::Integer(42)
        );
    }
    let stats = c
        .macro_expansion_stats()
        .iter()
        .find(|s| s.name == Name::new(Some("user"), "twice"))
        .unwrap();
    assert_eq!(stats.expansions, 2);
    assert!(stats.generated_forms >= 6);
    let definition = stats.definition.as_ref().unwrap();
    assert_eq!(definition.source, "macros.clj");
    assert!(definition.text.contains("defmacro twice"));
    assert_eq!((definition.line, definition.column), (1, 1));
    let code = c
        .code_generation_stats()
        .iter()
        .find(|s| s.location.source == "consumer.clj")
        .unwrap();
    assert_eq!(code.location.text, "(twice 21)");
    assert_eq!(code.generations, 2);
    assert_eq!(
        code.generated_expressions,
        2 * first.expression_count() as u64
    );

    // A new definition at the same filename/offset gets a distinct report.
    let replacement = c.source("macros.clj", "(defmacro twice [x] `(* ~x 2))");
    c.eval_source(&replacement).unwrap();
    assert_eq!(
        c.eval_form(&form).unwrap().inspect(),
        ValueView::Integer(42)
    );
    assert_eq!(
        c.macro_expansion_stats()
            .iter()
            .filter(|s| s.name == Name::new(Some("user"), "twice"))
            .count(),
        2
    );
    let generations: u64 = c
        .code_generation_stats()
        .iter()
        .map(|s| s.generations)
        .sum();
    c.set_diagnostics(false);
    c.eval_form(&form).unwrap();
    assert_eq!(
        c.code_generation_stats()
            .iter()
            .map(|s| s.generations)
            .sum::<u64>(),
        generations
    );
    c.clear_diagnostics();
    assert!(c.macro_expansion_stats().is_empty());
    assert!(c.code_generation_stats().is_empty());
}

#[test]
fn diagnostics_preserve_output_errors_and_live_records_and_do_not_cache_effectful_macros() {
    fn run(
        enabled: bool,
    ) -> (
        String,
        ValueView,
        String,
        light_cljs::interpreter::MemoryStats,
    ) {
        let mut c = Context::new();
        let out = OutputBuffer::default();
        let stream = c.output_stream(out.clone());
        let _binding = c.bind_io(None, Some(&stream), None).unwrap();
        c.set_diagnostics(enabled);
        c.eval_string("(def n 0) (defmacro tick [] (def n (inc n)) (println n) n)")
            .unwrap();
        for _ in 0..2 {
            c.eval_string("(tick)").unwrap();
        }
        let value = c.eval_string("n").unwrap().inspect();
        let error = c
            .eval_string("(throw (ex-info \"same\" {}))")
            .unwrap_err()
            .to_string();
        c.collect_garbage();
        let memory = c.memory_stats();
        (String::from_utf16_lossy(&out.units()), value, error, memory)
    }
    let plain = run(false);
    let diagnosed = run(true);
    assert_eq!(plain, diagnosed);
    assert_eq!(plain.0, "1\n2\n");
    assert_eq!(plain.1, ValueView::Integer(2));

    let mut c = Context::new();
    c.eval_string("(+ 1 2)").unwrap();
    assert!(c.macro_expansion_stats().is_empty());
    assert!(c.code_generation_stats().is_empty());
    c.set_diagnostics(true);
    c.eval_string("(+ 1 2)").unwrap();
    assert!(!c.code_generation_stats().is_empty());
    c.reset();
    assert!(c.macro_expansion_stats().is_empty());
    assert!(c.code_generation_stats().is_empty());
}

#[test]
fn host_macro_can_disable_diagnostics_during_its_own_expansion() {
    use light_cljs::interpreter::{Arity, NativeDefinition, NativeEffects, NativeMetadata};
    static SWITCH: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "user",
            name: "diagnostic-switch",
            arity: Arity::Exact(3),
            effects: NativeEffects::Host,
            errors: &[],
        },
        callback: |c, args| {
            c.set_diagnostics(false);
            c.clear_diagnostics();
            Ok(args[2].clone())
        },
    };
    let mut c = Context::new();
    c.register_macro(&SWITCH);
    c.set_diagnostics(true);
    assert_eq!(
        c.eval_string("(diagnostic-switch 42)").unwrap().inspect(),
        ValueView::Integer(42)
    );
}

#[test]
fn diagnostics_do_not_consume_program_resource_budgets() {
    use light_cljs::interpreter::ResourceLimits;
    const PROGRAM: &str = "(defmacro twice [x] `(+ ~x ~x)) (twice 21)";
    let mut plain = Context::new();
    plain.eval_string(PROGRAM).unwrap();
    let required = plain.resource_usage();
    let mut diagnosed = Context::new();
    diagnosed.set_diagnostics(true);
    diagnosed.eval_string(PROGRAM).unwrap();
    assert_eq!(required, diagnosed.resource_usage());
    for allocation in [
        required.allocation - 1,
        required.allocation,
        required.allocation + 1,
    ] {
        let mut results = Vec::new();
        for enabled in [false, true] {
            let mut c = Context::new();
            c.set_diagnostics(enabled);
            c.set_resource_limits(ResourceLimits {
                allocation: Some(allocation),
                ..ResourceLimits::default()
            });
            results.push(
                c.eval_string(PROGRAM)
                    .map(|value| value.inspect())
                    .map_err(|error| error.class()),
            );
        }
        assert_eq!(results[0], results[1], "allocation budget {allocation}");
    }
}
