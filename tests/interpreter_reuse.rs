use light_cljs::interpreter::*;
use std::{cell::Cell, collections::HashSet, rc::Rc};

fn form(c: &Context, text: &str) -> Form {
    let source = c.source("reuse.clj", text);
    c.reader(&source).unwrap().read_form().unwrap().unwrap()
}
fn value(c: &mut Context, form: &Form) -> ValueView {
    let code = c.analyze_form(form).unwrap();
    c.eval_analyzed(&code).unwrap().inspect()
}

#[test]
fn exact_form_reuses_owned_bodies_but_separate_source_identity_does_not() {
    let mut c = Context::new();
    let f = form(&c, "(let* [x 20 f (fn* [n] (+ x n))] (f 22))");
    let first = c.analyze_form(&f).unwrap();
    let second = c.analyze_form(&f).unwrap();
    assert_eq!(first.unit().id(), second.unit().id());
    assert_eq!(c.analysis_reuse_stats().hits, 1);
    assert_eq!(
        c.eval_analyzed(&second).unwrap().inspect(),
        ValueView::Integer(42)
    );
    let other = form(&c, "(let* [x 20 f (fn* [n] (+ x n))] (f 22))");
    assert_ne!(
        first.unit().id(),
        c.analyze_form(&other).unwrap().unit().id()
    );
    drop((f, other));
    c.collect_garbage();
    assert_eq!(c.analysis_reuse_stats().entries, 0);
    c.reset();
    assert_eq!(
        c.eval_analyzed(&first).unwrap_err().kind,
        EvalErrorKind::WrongOwner
    );
}

#[test]
fn mutable_roots_mapping_changes_and_fork_macro_changes_are_observed() {
    let mut c = Context::new();
    c.eval_string("(def x 1) (def f (fn* [a] a))").unwrap();
    let x = form(&c, "x");
    let old = c.analyze_form(&x).unwrap();
    assert_eq!(old.unit().id(), c.analyze_form(&x).unwrap().unit().id());
    c.eval_string("(def x 2)").unwrap();
    assert_eq!(
        c.eval_analyzed(&old).unwrap().inspect(),
        ValueView::Integer(2)
    );
    assert_ne!(old.unit().id(), c.analyze_form(&x).unwrap().unit().id());
    let ns = c.current_namespace().namespace_id().unwrap();
    c.unmap_var(ns, &Name::new(None, "x")).unwrap();
    assert!(c.analyze_form(&x).is_err());
    c.eval_string("(def x 3)").unwrap();
    assert_eq!(value(&mut c, &x), ValueView::Integer(3));
    assert_eq!(
        c.eval_analyzed(&old).unwrap().inspect(),
        ValueView::Integer(2)
    );

    let call = form(&c, "(f 9)");
    let before = c.analyze_form(&call).unwrap();
    let mut fork = c.fork().unwrap();
    assert_eq!(fork.analysis_reuse_stats().entries, 0);
    fork.eval_string("(defmacro f [a] 77)").unwrap();
    assert_ne!(
        before.unit().id(),
        c.analyze_form(&call).unwrap().unit().id()
    );
    assert_eq!(value(&mut c, &call), ValueView::Integer(77));
}

#[test]
fn namespace_bindings_and_aliases_invalidate_resolution() {
    let mut c = Context::new();
    c.eval_string("(ns one) (def x 1) (ns two) (def x 2) (in-ns 'user)")
        .unwrap();
    let one = c.find_namespace(&Name::new(None, "one")).unwrap();
    let two = c.find_namespace(&Name::new(None, "two")).unwrap();
    let user = c.current_namespace().namespace_id().unwrap();
    c.alias_namespace(user, Name::new(None, "a"), one.namespace_id().unwrap())
        .unwrap();
    let f = form(&c, "a/x");
    assert_eq!(value(&mut c, &f), ValueView::Integer(1));
    c.unalias_namespace(user, &Name::new(None, "a")).unwrap();
    c.alias_namespace(user, Name::new(None, "a"), two.namespace_id().unwrap())
        .unwrap();
    assert_eq!(value(&mut c, &f), ValueView::Integer(2));
    let ns = c
        .resolve_var(&Name::new(None, "*ns*"))
        .unwrap()
        .unwrap()
        .var_id()
        .unwrap();
    let x = form(&c, "x");
    let guard = c.bind_vars(&[(ns, one)]).unwrap();
    assert_eq!(value(&mut c, &x), ValueView::Integer(1));
    let inner = c.bind_vars(&[(ns, two)]).unwrap();
    assert_eq!(value(&mut c, &x), ValueView::Integer(2));
    drop(inner);
    assert_eq!(value(&mut c, &x), ValueView::Integer(1));
    drop(guard);
    assert!(c.analyze_form(&x).is_err());
}

#[test]
fn macro_effects_and_definition_file_capture_are_never_cached() {
    let mut c = Context::new();
    c.eval_string("(def n 0) (defmacro tick [] (def n (inc n)) n)")
        .unwrap();
    let f = form(&c, "(tick)");
    assert_eq!(value(&mut c, &f), ValueView::Integer(1));
    assert_eq!(value(&mut c, &f), ValueView::Integer(2));
    let source = c.source("<eval>", "(def x 1)");
    let f = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let file = c
        .resolve_var(&Name::new(None, "*file*"))
        .unwrap()
        .unwrap()
        .var_id()
        .unwrap();
    for name in ["first.clj", "second.clj"] {
        let guard = c
            .bind_vars(&[(file, c.string(name.encode_utf16().collect::<Vec<_>>()))])
            .unwrap();
        let before = c.analysis_reuse_stats().hits;
        value(&mut c, &f);
        value(&mut c, &f);
        assert_eq!(c.analysis_reuse_stats().hits, before);
        let actual = c.eval_string("(:file (meta #'x))").unwrap();
        assert_eq!(
            actual.inspect(),
            ValueView::String(name.encode_utf16().collect())
        );
        drop(guard);
    }
}

static TAG: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "tag",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |c, args| {
        c.eval_string("(def tag-calls (inc tag-calls))").unwrap();
        Ok(args[0].clone())
    },
};
#[test]
fn reader_features_callbacks_permissions_and_options_invalidate() {
    let mut c = Context::new();
    let f = form(&c, "(+ 20 22)");
    let first = c.analyze_form(&f).unwrap();
    assert_eq!(first.unit().id(), c.analyze_form(&f).unwrap().unit().id());
    c.set_reader_features([Name::new(None, "bb")]);
    assert_ne!(first.unit().id(), c.analyze_form(&f).unwrap().unit().id());
    assert_eq!(
        c.eval_string("#?(:clj 1 :bb 2)").unwrap().inspect(),
        ValueView::Integer(2)
    );
    c.eval_string("(def tag-calls 0)").unwrap();
    let before = c.analyze_form(&f).unwrap();
    c.register_data_reader(Name::new(None, "t"), &TAG);
    assert_ne!(before.unit().id(), c.analyze_form(&f).unwrap().unit().id());
    for _ in 0..2 {
        assert_eq!(
            c.eval_string("#t 7").unwrap().inspect(),
            ValueView::Integer(7)
        );
    }
    assert_eq!(
        c.eval_string("tag-calls").unwrap().inspect(),
        ValueView::Integer(2)
    );
    c.remove_data_reader(&Name::new(None, "t"));
    assert!(c.eval_string("#t 7").is_err());
    c.merge_opts(ContextOptions {
        policy: Some(NamePolicy {
            deny: HashSet::from([Name::new(None, "+")]),
            ..Default::default()
        }),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(c.analyze_form(&f).unwrap_err().phase, EvalPhase::Analysis);
    assert!(c.eval_analyzed(&first).is_err());
    c.merge_opts(ContextOptions {
        policy: Some(NamePolicy::default()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(value(&mut c, &f), ValueView::Integer(42));
}

#[test]
fn resource_limits_and_analysis_polling_bypass_reuse() {
    let mut c = Context::new();
    let f = form(&c, "(+ 20 22)");
    c.analyze_form(&f).unwrap();
    c.analyze_form(&f).unwrap();
    let before = c.analysis_reuse_stats().hits;
    c.set_resource_limits(ResourceLimits {
        allocation: Some(0),
        ..Default::default()
    });
    assert_eq!(c.analyze_form(&f).unwrap_err().class(), ErrorClass::Control);
    c.set_resource_limits(ResourceLimits {
        fuel: Some(0),
        ..Default::default()
    });
    assert_eq!(c.analyze_form(&f).unwrap_err().class(), ErrorClass::Control);
    c.set_resource_limits(ResourceLimits {
        evaluation_depth: 1,
        ..Default::default()
    });
    assert_eq!(c.analyze_form(&f).unwrap_err().class(), ErrorClass::Control);
    c.set_resource_limits(ResourceLimits::default());
    c.set_execution_poll(Some(|_| Ok(())));
    let first = c.analyze_form(&f).unwrap();
    assert_ne!(first.unit().id(), c.analyze_form(&f).unwrap().unit().id());
    assert_eq!(c.analysis_reuse_stats().hits, before);
}

#[test]
fn loaded_modules_reuse_initialization_but_reload_and_failure_recovery_execute() {
    let mut c = Context::new();
    c.set_diagnostics(true);
    c.eval_string("(def starts 0)").unwrap();
    let loads = Rc::new(Cell::new(0));
    let broken = Rc::new(Cell::new(false));
    // Install a source with visible effects and a feature-sensitive body.
    let (counter, failure) = (loads.clone(), broken.clone());
    c.set_source_provider(move |_: &LoadRequest| {
        counter.set(counter.get() + 1);
        Ok(Some(SourceInput::new(
            "module.cljc",
            if failure.get() {
                "(ns module) absent"
            } else {
                "(ns module) (alter-var-root #'user/starts inc) (def x #?(:clj 1 :bb 2))"
            },
        )))
    });
    c.eval_string("(require 'module)").unwrap();
    let module_generations = |c: &Context| {
        c.code_generation_stats()
            .iter()
            .filter(|s| s.location.source == "module.cljc")
            .map(|s| s.generations)
            .sum::<u64>()
    };
    let before = module_generations(&c);
    c.set_reader_features([Name::new(None, "bb")]);
    for _ in 0..5 {
        c.eval_string("(require 'module)").unwrap();
    }
    assert_eq!(loads.get(), 1);
    assert_eq!(module_generations(&c), before);
    assert_eq!(
        c.eval_string("[user/starts module/x]")
            .unwrap()
            .elements()
            .unwrap()
            .iter()
            .map(|v| v.inspect())
            .collect::<Vec<_>>(),
        vec![ValueView::Integer(1), ValueView::Integer(1)]
    );
    c.eval_string("(require 'module :reload)").unwrap();
    assert_eq!(loads.get(), 2);
    assert_eq!(
        c.eval_string("module/x").unwrap().inspect(),
        ValueView::Integer(2)
    );
    broken.set(true);
    assert!(c.eval_string("(require 'module :reload)").is_err());
    assert_eq!(
        c.namespace_load_state(&Name::new(None, "module")),
        LoadState::Failed
    );
    broken.set(false);
    c.eval_string("(require 'module)").unwrap();
    assert_eq!(loads.get(), 4);
    assert_eq!(
        c.eval_string("user/starts").unwrap().inspect(),
        ValueView::Integer(3)
    );
}

#[test]
fn retention_is_bounded_and_dead_inputs_and_failures_release_scratch_roots() {
    let mut c = Context::new();
    let forms: Vec<_> = (0..100).map(|i| form(&c, &format!("(+ {i} 1)"))).collect();
    for f in &forms {
        c.analyze_form(f).unwrap();
    }
    let stats = c.analysis_reuse_stats();
    assert!(stats.entries <= 32);
    assert!(stats.retained_nodes <= 8_192);
    assert!(stats.retained_source_bytes <= 256 * 1_024);
    let large = form(&c, &format!("\"{}\"", "x".repeat(70_000)));
    let before = c.analysis_reuse_stats().entries;
    c.analyze_form(&large).unwrap();
    assert_eq!(c.analysis_reuse_stats().entries, before);
    drop((forms, large));
    c.collect_garbage();
    assert_eq!(c.analysis_reuse_stats().entries, 0);
    let before = c.memory_stats();
    for _ in 0..10 {
        let f = form(&c, "[\"must be released\" (fn* [x] unresolved)]");
        assert!(c.analyze_form(&f).is_err());
    }
    let after = c.collect_garbage().after;
    assert_eq!(after.heap_values, before.heap_values);
    assert_eq!(after.sources, before.sources);
    assert_eq!(after.executable_units, before.executable_units);
}
