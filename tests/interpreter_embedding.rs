use light_cljs::interpreter::*;
use std::{cell::Cell, collections::HashSet, rc::Rc};
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

#[test]
fn eval_and_context_semantics_match_pinned_sci() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-embedding-")) {
        let mut context = Context::new();
        let value = integer(
            context
                .eval_string(case.source)
                .unwrap_or_else(|e| panic!("{}: {e}", case.id)),
        );
        assert_eq!(
            case.outcome,
            protocol::Outcome::Returned(protocol::Value::Int(value)),
            "{}",
            case.id
        );
        assert!(case.effects.is_empty() && case.stdout.is_empty() && case.stderr.is_empty());
        count += 1;
    }
    assert_eq!(count, 11);
}

fn name(text: &str) -> Name {
    match text.split_once('/') {
        Some((ns, local)) => Name::new(Some(ns), local),
        None => Name::new(None, text),
    }
}
fn deny(c: &mut Context, names: &[&str]) {
    c.merge_opts(ContextOptions {
        policy: Some(NamePolicy {
            deny: names.iter().map(|s| name(s)).collect(),
            ..Default::default()
        }),
        ..Default::default()
    })
    .unwrap();
}
fn integer(value: RootedValue) -> i64 {
    let ValueView::Integer(n) = value.inspect() else {
        panic!("{value:?}")
    };
    n
}
fn rejected(c: &mut Context, source: &str) {
    let error = c.eval_string(source).expect_err(source);
    assert!(
        error.to_string().contains("PermissionDenied"),
        "{source}: {error}"
    );
}
static PLUS: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "increment",
        arity: Arity::Exact(1),
        effects: NativeEffects::Pure,
        errors: &[],
    },
    callback: |c, a| Ok(c.integer(integer(a[0].clone()) + 1)),
};
static TWICE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "twice",
        arity: Arity::Exact(3),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, a| Ok(c.list(&[c.symbol(name("do")), a[2].clone(), a[2].clone()])?),
};
static REENTER: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "reenter",
        arity: Arity::Exact(0),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, _| {
        c.eval_string("(host/increment 1)")
            .map_err(|e| NativeError::from(InvokeError::Execution(Box::new(e))))
    },
};
static RECONFIGURE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "reconfigure",
        arity: Arity::Exact(0),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, _| {
        assert!(matches!(c.fork(), Err(OptionsError::ActiveEvaluation)));
        assert_eq!(
            c.merge_opts(ContextOptions::default()),
            Err(OptionsError::ActiveEvaluation)
        );
        Ok(c.nil())
    },
};

#[test]
fn options_merge_preserves_values_vars_features_aliases_and_capabilities() {
    let mut c = Context::init(ContextOptions {
        natives: vec![&PLUS],
        macros: vec![&TWICE],
        features: Some(vec![name("cljs")]),
        aliases: vec![NamespaceAlias {
            namespace: name("user"),
            alias: name("h"),
            target: name("host"),
        }],
        ..Default::default()
    })
    .unwrap();
    c.eval_string("(def saved (atom 10)) (defn run [n] (h/increment n))")
        .unwrap();
    let before = c.resolve_var(&name("run")).unwrap().unwrap();
    let value = c.integer(42);
    c.merge_opts(ContextOptions {
        bindings: vec![(name("host/answer"), value)],
        ..Default::default()
    })
    .unwrap();
    c.merge_opts(ContextOptions::default()).unwrap();
    assert!(before.same_identity(&c.resolve_var(&name("run")).unwrap().unwrap()));
    assert_eq!(
        integer(
            c.eval_string("(host/twice (swap! saved inc)) @saved")
                .unwrap()
        ),
        12
    );
    assert_eq!(
        integer(c.eval_string("#?(:clj 1 :cljs (run host/answer))").unwrap()),
        43
    );
    let before = c.resolve_var(&name("host/answer")).unwrap().unwrap();
    c.merge_opts(ContextOptions {
        bindings: vec![(name("host/answer"), c.integer(5))],
        ..Default::default()
    })
    .unwrap();
    assert!(before.same_identity(&c.resolve_var(&name("host/answer")).unwrap().unwrap()));
    assert_eq!(integer(c.eval_string("host/answer").unwrap()), 5);
    deny(&mut c, &["host/twice"]);
    // Trusted registration must never unwrap a rejected resolution.
    c.merge_opts(ContextOptions {
        macros: vec![&TWICE],
        ..Default::default()
    })
    .unwrap();
    rejected(&mut c, "(host/twice 1)");
}

#[test]
fn permission_policy_follows_var_and_native_identity_across_every_execution_route() {
    let mut c = Context::init(ContextOptions {
        natives: vec![&PLUS, &REENTER],
        macros: vec![&TWICE],
        ..Default::default()
    })
    .unwrap();
    c.eval_string("(alias 'h 'host) (def saved host/increment) (def concat-saved concat) (defmacro hidden [] '(host/increment 1))").unwrap();
    let plus = c.eval_string("host/increment").unwrap();
    let var = c.resolve_var(&name("host/increment")).unwrap().unwrap();
    let source = c.source("retained.clj", "(host/increment 1)");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let analyzed = c.analyze_form(&form).unwrap();
    let closure = c.eval_string("(fn [] (host/increment 1))").unwrap();
    let user = c
        .find_namespace(&name("user"))
        .unwrap()
        .namespace_id()
        .unwrap();
    c.refer_var(user, name("renamed"), var.var_id().unwrap())
        .unwrap();
    deny(&mut c, &["host/increment", "concat"]);
    for source in [
        "(host/increment 1)",
        "(h/increment 1)",
        "(renamed 1)",
        "#'host/increment",
        "(resolve 'h/increment)",
        "(ns-resolve 'host 'increment)",
        "(eval '(host/increment 1))",
        "(eval (read-string \"(host/increment 1)\"))",
        "(hidden)",
        "(host/reenter)",
        "(saved 1)",
        "(apply saved [1])",
        "(apply concat-saved [[1] [2] [3]])",
        "(deref (get (ns-publics 'host) 'increment))",
        "((get (ns-publics 'host) 'increment) 1)",
        "(alter-var-root (get (ns-publics 'host) 'increment) (fn [f] (f 1)))",
    ] {
        rejected(&mut c, source);
    }
    assert!(c.eval_analyzed(&analyzed).is_err());
    assert!(c.eval_form(&form).is_err());
    assert!(c.invoke(&closure, &[]).is_err());
    assert!(c.invoke(&plus, &[c.integer(1)]).is_err());
    assert!(c.invoke(&var, &[c.integer(1)]).is_err());
    assert!(c.invoke_native(&PLUS, &[c.integer(1)]).is_err());
    assert_eq!(integer(c.eval_string("(+ 2 3)").unwrap()), 5);
}

#[test]
fn allow_policy_canonicalizes_core_names_and_keeps_language_created_vars_usable() {
    let mut c = Context::init(ContextOptions {
        policy: Some(NamePolicy {
            allow: Some([name("int?"), name("def")].into()),
            ..Default::default()
        }),
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        c.eval_string("(clojure.core/int? 1)").unwrap().inspect(),
        ValueView::Boolean(true)
    ));
    assert_eq!(integer(c.eval_string("(def x 7) x").unwrap()), 7);
    rejected(&mut c, "(boolean? true)");
    rejected(&mut c, "(if true 1 2)");
    c.merge_opts(ContextOptions {
        policy: Some(NamePolicy {
            allow: Some(HashSet::new()),
            deny: [name("int?")].into(),
            unrestricted: true,
        }),
        ..Default::default()
    })
    .unwrap();
    assert!(c.eval_string("(int? 1)").is_ok());
    assert!(c.eval_string("(slurp \"/etc/passwd\")").is_err());
    assert!(c.eval_string("(println 1)").is_err());
    assert!(
        c.eval_string("(alter-var-root #'inc (constantly 1))")
            .is_err()
    );
}

#[test]
fn into_preserves_retained_native_var_and_interpreted_callback_permissions() {
    for callback in ["inc", "#'inc", "(fn [x] (inc x))"] {
        let mut c = Context::new();
        let into = c.eval_string("into").unwrap();
        let xf = c
            .eval_string(&format!(
                "(let [f {callback}] (fn [rf] (fn ([a] (rf a)) ([a x] (rf a (f x))))))"
            ))
            .unwrap();
        deny(&mut c, &["inc"]);
        let target = c.vector(&[]).unwrap();
        let source = c.vector(&[c.integer(1)]).unwrap();
        let error = c.invoke(&into, &[target.clone(), xf, source]).unwrap_err();
        assert!(error.to_string().contains("PermissionDenied"), "{error}");
        assert_eq!(target.count(), Some(0));
    }
}

#[test]
fn native_transforms_preserve_permissions_on_retained_callbacks() {
    for transform in ["map", "filter", "remove", "take-while", "mapcat"] {
        for callback in ["inc", "#'inc", "(fn [x] (inc x))"] {
            let mut c = Context::new();
            let xf = c.eval_string(&format!("({transform} {callback})")).unwrap();
            let rf = c.eval_string("(completing conj)").unwrap();
            let reducer = c.invoke(&xf, &[rf]).unwrap();
            deny(&mut c, &["inc"]);
            let error = c
                .invoke(&reducer, &[c.vector(&[]).unwrap(), c.integer(1)])
                .unwrap_err();
            assert!(error.to_string().contains("PermissionDenied"), "{error}");
        }
    }
}

#[test]
fn retained_transducer_adapters_preserve_callback_permissions_and_ambient_boundary() {
    for reducer in ["+", "#'+", "(fn [a x] (+ a x))"] {
        for transform in [
            "(fn [rf] rf)",
            "(take 1)",
            "(drop 0)",
            "(take-while pos?)",
            "cat",
            "(mapcat identity)",
            "(partition-all 1)",
            "(partition-all 3)",
        ] {
            let mut c = Context::new();
            let rf = c.eval_string(&format!("(completing {reducer})")).unwrap();
            let transduce = c.eval_string("transduce").unwrap();
            let xf = c.eval_string(transform).unwrap();
            deny(&mut c, &["+"]);
            let mut source = c.vector(&[c.integer(1)]).unwrap();
            if transform.contains("cat") {
                source = c.vector(&[source]).unwrap();
            }
            let error = c
                .invoke(&transduce, &[xf, rf, c.integer(0), source])
                .unwrap_err();
            assert!(error.to_string().contains("PermissionDenied"), "{error}");
        }
    }
    let mut c = Context::new();
    let rf = c.eval_string("(completing + inc)").unwrap();
    deny(&mut c, &["inc"]);
    assert!(
        c.invoke(&rf, &[c.integer(0)])
            .unwrap_err()
            .to_string()
            .contains("PermissionDenied")
    );
    for source in [
        "(transduce identity (completing (fn [a x] (slurp x))) nil [\"missing\"])",
        "(transduce identity (completing (fn [a x] (println x))) nil [1])",
    ] {
        assert!(c.eval_string(source).is_err(), "{source}");
    }
}

#[test]
fn reader_and_loader_callbacks_inherit_current_permissions() {
    let mut c = Context::new();
    c.register_data_reader(name("increment"), &PLUS);
    let source = c.source("tag.clj", "#increment 1");
    let mut retained = c.reader(&source).unwrap();
    let loads = Rc::new(Cell::new(0));
    let counter = loads.clone();
    c.merge_opts(ContextOptions {
        source_provider: Some(Some(Rc::new(move |_: &LoadRequest| {
            counter.set(counter.get() + 1);
            Ok(Some(SourceInput::new(
                "loaded.clj",
                "(ns loaded) (host/increment 1)",
            )))
        }))),
        natives: vec![&PLUS],
        ..Default::default()
    })
    .unwrap();
    deny(&mut c, &["host/increment"]);
    assert!(
        retained
            .read_form()
            .unwrap_err()
            .to_string()
            .contains("PermissionDenied")
    );
    rejected(&mut c, "#increment 1");
    rejected(
        &mut c,
        "(clojure.edn/read-string {:readers {'x (get (ns-publics 'host) 'increment)}} \"#x 1\")",
    );
    rejected(&mut c, "(require 'loaded)");
    assert_eq!(loads.get(), 1);
    deny(&mut c, &["require"]);
    rejected(&mut c, "(require 'never)");
    assert_eq!(loads.get(), 1);
    c.merge_opts(ContextOptions {
        source_provider: Some(None),
        ..Default::default()
    })
    .unwrap();
}

#[test]
fn retained_primitive_code_and_callback_reconfiguration_cannot_bypass_policy() {
    let mut c = Context::new();
    c.register_native(&RECONFIGURE);
    c.eval_string("(host/reconfigure)").unwrap();
    let f = c.eval_string("(fn [] (if true 1 2))").unwrap();
    let s = c.source("old.clj", "(if true 1 2)");
    let form = c.reader(&s).unwrap().read_form().unwrap().unwrap();
    let code = c.analyze_form(&form).unwrap();
    deny(&mut c, &["if", "do"]);
    rejected(&mut c, "(do 1)");
    assert!(c.eval_analyzed(&code).is_err());
    assert!(c.invoke(&f, &[]).is_err());
}

#[test]
fn fork_copies_maps_but_shares_existing_vars_values_and_retained_code() {
    let mut parent = Context::new();
    parent.eval_string("(def x 1) (def state (atom 0)) (defn read-state [] @state) (defn where [] (ns-name *ns*))").unwrap();
    let x = parent.resolve_var(&name("x")).unwrap().unwrap();
    let mut child = parent.fork().unwrap();
    assert!(x.same_identity(&child.resolve_var(&name("x")).unwrap().unwrap()));
    child
        .eval_string(
            "(def x 2) (swap! state inc) (def child-only 10) (ns child-space) (def child-new 11)",
        )
        .unwrap();
    assert_eq!(integer(parent.eval_string("x").unwrap()), 2);
    assert_eq!(integer(parent.eval_string("(read-state)").unwrap()), 1);
    assert!(parent.eval_string("child-only").is_err());
    assert!(parent.find_namespace(&name("child-space")).is_none());
    assert!(
        matches!(parent.current_namespace().inspect(), ValueView::Namespace(n) if n == name("user"))
    );
    assert!(
        matches!(child.eval_string("(user/where)").unwrap().inspect(), ValueView::Symbol(n) if n == name("child-space"))
    );
    parent.eval_string("(def parent-only 12)").unwrap();
    child.eval_string("(in-ns 'user)").unwrap();
    assert!(child.eval_string("parent-only").is_err());
    // Shared metadata must not expose the original namespace's evolving map.
    assert!(
        child
            .eval_string("(binding [*ns* (:ns (meta #'x))] (eval 'parent-only))")
            .is_err()
    );
    child.eval_string("(ns-unmap (:ns (meta #'x)) 'x)").unwrap();
    assert_eq!(integer(parent.eval_string("x").unwrap()), 2);
    assert!(child.eval_string("x").is_err());
    child.eval_string("(def x 9)").unwrap();
    assert!(!x.same_identity(&child.resolve_var(&name("x")).unwrap().unwrap()));
    assert_eq!(integer(parent.eval_string("x").unwrap()), 2);
    parent.collect_garbage();
    child.collect_garbage();
    assert_eq!(integer(child.eval_string("(read-state)").unwrap()), 1);
    drop(parent);
    child.collect_garbage();
    assert_eq!(integer(child.eval_string("child-only").unwrap()), 10);
}

#[test]
fn fork_local_io_policy_aliases_and_reader_configuration_share_explicit_resources() {
    let mut parent = Context::new();
    let parent_output = OutputBuffer::default();
    parent
        .merge_opts(ContextOptions {
            output: Some(parent.output_stream(parent_output.clone())),
            natives: vec![&PLUS],
            ..Default::default()
        })
        .unwrap();
    parent
        .eval_string("(defn emit [] (println :ok)) (alias 'h 'host)")
        .unwrap();
    let function = parent.eval_string("emit").unwrap();
    let mut child = parent.fork().unwrap();
    child.eval_string("(emit)").unwrap();
    assert_eq!(
        parent_output.take(),
        ":ok\n".encode_utf16().collect::<Vec<_>>()
    );
    let child_output = OutputBuffer::default();
    child
        .merge_opts(ContextOptions {
            output: Some(child.output_stream(child_output.clone())),
            features: Some(vec![name("cljs")]),
            limits: Some(ResourceLimits {
                fuel: Some(100_000),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    child.invoke(&function, &[]).unwrap();
    assert!(parent_output.units().is_empty());
    assert_eq!(
        child_output.take(),
        ":ok\n".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(parent.resource_limits().fuel, Some(100_000));
    assert_eq!(integer(child.eval_string("#?(:clj 1 :cljs 2)").unwrap()), 2);
    assert_eq!(
        integer(parent.eval_string("#?(:clj 1 :cljs 2)").unwrap()),
        1
    );
    deny(&mut child, &["host/increment"]);
    rejected(&mut child, "(h/increment 1)");
    assert_eq!(integer(parent.eval_string("(h/increment 1)").unwrap()), 2);
    child.eval_string("(ns-unalias *ns* 'h)").unwrap();
    assert_eq!(integer(parent.eval_string("(h/increment 2)").unwrap()), 3);
}

#[test]
fn invalid_merge_is_atomic_and_owners_and_outcomes_are_explicit() {
    let mut c = Context::new();
    let foreign = Context::new();
    assert_eq!(
        c.merge_opts(ContextOptions {
            natives: vec![&PLUS],
            bindings: vec![(name("bad"), foreign.integer(1))],
            ..Default::default()
        }),
        Err(OptionsError::WrongOwner)
    );
    assert!(c.resolve_var(&name("host/increment")).unwrap().is_none());
    assert_eq!(
        c.merge_opts(ContextOptions {
            output: Some(c.integer(1)),
            ..Default::default()
        }),
        Err(OptionsError::ExpectedOutput)
    );
    let result = c
        .eval_string_plus("(ns result) (defn sum [x y] (+ x y)) sum")
        .unwrap();
    assert!(matches!(result.namespace.inspect(), ValueView::Namespace(n) if n == name("result")));
    assert_eq!(
        integer(
            c.invoke(&result.value, &[c.integer(2), c.integer(3)])
                .unwrap()
        ),
        5
    );
    assert!(
        c.invoke(&result.value, &[foreign.integer(2), c.integer(3)])
            .is_err()
    );
    let error = c.eval_string_plus("(ns failed) (/ 1 0)").unwrap_err();
    assert_eq!(error.diagnostic.phase, EvalPhase::Execution);
    assert!(matches!(error.namespace.inspect(), ValueView::Namespace(n) if n == name("failed")));
    assert_eq!(integer(c.eval_string("(+ 1 1)").unwrap()), 2);
    let s = c.source("data", "42");
    let form = c.reader(&s).unwrap().read_form().unwrap().unwrap();
    assert_eq!(integer(c.eval_form_plus(&form).unwrap().value), 42);
    c.reset();
    assert!(c.invoke(&result.value, &[]).is_err());
    assert_eq!(error.diagnostic.span.source().text(), "(ns failed) (/ 1 0)");
}

#[test]
fn synthetic_definitions_and_indirect_dynamic_binding_obey_policy() {
    let mut c = Context::new();
    deny(&mut c, &["defonce", "declare"]);
    for source in [
        "(defonce x 1)",
        "(declare x)",
        "(clojure.core/defonce x 1)",
        "(clojure.core/declare x)",
    ] {
        rejected(&mut c, source);
    }
    deny(&mut c, &["def"]);
    rejected(&mut c, "(defonce x 1)");
    rejected(&mut c, "(declare x)");
    deny(&mut c, &["*print-length*"]);
    rejected(
        &mut c,
        "(with-bindings* {(get (ns-map 'clojure.core) '*print-length*) 0} (fn [] (pr-str [1 2])))",
    );
    let ns = c.current_namespace().namespace_id().unwrap();
    let var = c.intern_var(ns, name("host-secret")).unwrap();
    c.set_var_root(var.var_id().unwrap(), &c.integer(42))
        .unwrap();
    c.merge_opts(ContextOptions {
        policy: Some(NamePolicy {
            allow: Some(HashSet::new()),
            ..Default::default()
        }),
        ..Default::default()
    })
    .unwrap();
    rejected(&mut c, "host-secret");
}

#[test]
fn stable_dynamic_var_remapping_survives_unmapping_collection_and_multiple_forks() {
    let mut c = Context::new();
    let f = c.eval_string("(fn [] *print-length*)").unwrap();
    let bound = c
        .eval_string("(fn [] (thread-bound? #'*print-length*))")
        .unwrap();
    let var = c
        .resolve_var(&name("*print-length*"))
        .unwrap()
        .unwrap()
        .var_id()
        .unwrap();
    c.set_var_root(var, &c.integer(7)).unwrap();
    let mut fork = c.fork().unwrap();
    let mut grandchild = fork.fork().unwrap();
    let copied = grandchild
        .resolve_var(&name("*print-length*"))
        .unwrap()
        .unwrap()
        .var_id()
        .unwrap();
    let guard = grandchild
        .bind_vars(&[(copied, grandchild.integer(9))])
        .unwrap();
    assert_eq!(integer(grandchild.invoke(&f, &[]).unwrap()), 9);
    assert!(matches!(
        grandchild.invoke(&bound, &[]).unwrap().inspect(),
        ValueView::Boolean(true)
    ));
    assert_eq!(integer(c.invoke(&f, &[]).unwrap()), 7);
    drop(guard);
    for context in [&mut c, &mut fork, &mut grandchild] {
        context
            .eval_string("(ns-unmap 'clojure.core '*print-length*)")
            .unwrap();
        context.collect_garbage();
        assert_eq!(integer(context.invoke(&f, &[]).unwrap()), 7);
    }
}

#[test]
fn transferring_code_for_an_absent_fork_namespace_reports_an_error() {
    let mut c = Context::new();
    let mut fork = c.fork().unwrap();
    let f = c.eval_string("(ns later) (fn [] (def x 1))").unwrap();
    let error = fork.invoke(&f, &[]).unwrap_err();
    assert!(error.to_string().contains("MissingNamespace"));
    assert!(fork.find_namespace(&name("later")).is_none());
}

#[test]
fn reset_in_callback_cannot_clear_policy_before_reentering() {
    static RESET: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "reset",
            arity: Arity::Exact(0),
            effects: NativeEffects::Context,
            errors: &[],
        },
        callback: |c, _| {
            c.reset();
            rejected(c, "(inc 1)");
            Ok(c.nil())
        },
    };
    let mut c = Context::init(ContextOptions {
        natives: vec![&RESET],
        ..Default::default()
    })
    .unwrap();
    deny(&mut c, &["inc"]);
    assert!(c.eval_string("(host/reset)").is_err());
    rejected(&mut c, "(inc 1)");
}

#[test]
fn forks_preserve_load_records_and_detached_current_namespace_after_failed_reload() {
    let loads = Rc::new(Cell::new(0));
    let fail = Rc::new(Cell::new(false));
    let counter = loads.clone();
    let failure = fail.clone();
    let mut c = Context::new();
    c.set_source_provider(move |_: &LoadRequest| {
        counter.set(counter.get() + 1);
        Ok(Some(SourceInput::new(
            "module.clj",
            if failure.get() {
                "(ns module) (/ 1 0)"
            } else {
                "(ns module) (def x 7)"
            },
        )))
    });
    c.eval_string("(require 'module)").unwrap();
    let mut fork = c.fork().unwrap();
    fork.eval_string("(require 'module)").unwrap();
    assert_eq!(loads.get(), 1);
    c.eval_string("(in-ns 'module)").unwrap();
    fail.set(true);
    assert!(c.eval_string("(require 'module :reload)").is_err());
    assert!(c.find_namespace(&name("module")).is_none());
    let mut detached = c.fork().unwrap();
    detached.collect_garbage();
    assert_eq!(integer(detached.eval_string("x").unwrap()), 7);
    assert!(detached.find_namespace(&name("module")).is_none());
    fail.set(false);
    assert_eq!(
        integer(detached.eval_string("(require 'module) x").unwrap()),
        7
    );
    assert!(c.find_namespace(&name("module")).is_none());
}

#[test]
fn merging_native_function_and_macro_replacements_preserves_var_identity() {
    static ORDINARY: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "twice",
            arity: Arity::Exact(1),
            effects: NativeEffects::Pure,
            errors: &[],
        },
        callback: |c, a| Ok(c.integer(integer(a[0].clone()) * 2)),
    };
    let mut c = Context::init(ContextOptions {
        macros: vec![&TWICE],
        ..Default::default()
    })
    .unwrap();
    let var = c.resolve_var(&name("host/twice")).unwrap().unwrap();
    c.merge_opts(ContextOptions {
        natives: vec![&ORDINARY],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(integer(c.eval_string("(host/twice 3)").unwrap()), 6);
    c.merge_opts(ContextOptions {
        macros: vec![&TWICE],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(integer(c.eval_string("(host/twice 3)").unwrap()), 3);
    assert!(var.same_identity(&c.resolve_var(&name("host/twice")).unwrap().unwrap()));
}

#[test]
fn core_repl_history_vars_are_dynamic_and_fork_local_without_automatic_rotation() {
    let mut parent = Context::new();
    let history = parent
        .resolve_var(&Name::new(Some("clojure.core"), "*1"))
        .unwrap()
        .unwrap()
        .var_id()
        .unwrap();
    assert!(parent.var_is_dynamic(history).unwrap());
    parent.set_var_root(history, &parent.integer(41)).unwrap();
    let mut child = parent.fork().unwrap();
    child.set_var_root(history, &child.integer(42)).unwrap();
    assert_eq!(
        parent.eval_string("*1").unwrap().inspect(),
        ValueView::Integer(41)
    );
    assert_eq!(
        child.eval_string("*1").unwrap().inspect(),
        ValueView::Integer(42)
    );
    assert_eq!(
        parent.eval_string("(binding [*1 9] *1)").unwrap().inspect(),
        ValueView::Integer(9)
    );
    assert_eq!(
        parent.eval_string("*1").unwrap().inspect(),
        ValueView::Integer(41)
    );
    assert_eq!(
        parent
            .eval_string("[*2 *3 *e]")
            .unwrap()
            .elements()
            .unwrap()
            .iter()
            .map(RootedValue::inspect)
            .collect::<Vec<_>>(),
        vec![ValueView::Nil; 3]
    );
}

#[test]
fn protocol_handles_forks_and_generated_helpers_obey_permissions() {
    let mut c = Context::new();
    c.eval_string("(defprotocol P (f [x]))").unwrap();
    let method = c.eval_string("f").unwrap();
    let protocol = c.eval_string("P").unwrap();
    let source = c.source("saved-protocol.clj", "(defprotocol P (f [x]))");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let code = c.analyze_form(&form).unwrap();
    let mut fork = c.fork().unwrap();
    drop((source, form));
    c.collect_garbage();
    assert!(fork.eval_string("P").unwrap().same_identity(&protocol));
    fork.eval_string("(def f (fn [_] 9))").unwrap();
    assert_eq!(integer(c.eval_string("(f nil)").unwrap()), 9);
    assert!(matches!(
        c.invoke(&method, &[c.nil()]),
        Err(InvokeError::Dispatch(
            EvalErrorKind::MissingProtocolImplementation { .. }
        ))
    ));
    deny(&mut fork, &["user/f"]);
    assert!(matches!(
        fork.invoke(&method, &[fork.nil()]),
        Err(InvokeError::Dispatch(EvalErrorKind::PermissionDenied(_)))
    ));
    deny(&mut c, &["clojure.core/defprotocol"]);
    assert!(matches!(
        c.eval_analyzed(&code).unwrap_err().kind,
        EvalErrorKind::PermissionDenied(_)
    ));
    let mut fresh = Context::new();
    assert!(fresh.eval_string("P").is_err());
    assert!(matches!(
        fresh.invoke(&method, &[fresh.nil()]),
        Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner))
    ));
    let satisfies = fresh.eval_string("satisfies?").unwrap();
    assert!(matches!(
        fresh.invoke(&satisfies, &[protocol, fresh.nil()]),
        Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner))
    ));
    let mut denied = Context::new();
    deny(&mut denied, &["user/f"]);
    rejected(&mut denied, "(defprotocol P (f [x]))");
    assert!(denied.resolve_var(&name("user/P")).unwrap().is_none());
}

#[test]
fn reify_retained_handles_survive_fork_and_reentrant_definition_replacement() {
    static REPLACE: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "replace-reify",
            arity: Arity::Exact(1),
            effects: NativeEffects::Context,
            errors: &[],
        },
        callback: |c, args| {
            assert_eq!(args[0].inspect(), ValueView::Reify);
            c.eval_string("(defprotocol P (f [x])) (def f (fn [_] 99))")
                .unwrap();
            c.collect_garbage();
            Ok(c.nil())
        },
    };
    let mut c = Context::new();
    c.register_native(&REPLACE);
    c.eval_string("(defprotocol P (f [x]))").unwrap();
    let method = c.eval_string("f").unwrap();
    let object = c
        .eval_string("(let [kept [7]] (reify P (f [this] (host/replace-reify this) (first kept))))")
        .unwrap();
    let mut fork = c.fork().unwrap();
    c.reset();
    assert!(matches!(
        c.invoke(&method, std::slice::from_ref(&object)),
        Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner))
    ));
    drop(c);
    fork.collect_garbage();
    assert_eq!(
        integer(fork.invoke(&method, std::slice::from_ref(&object)).unwrap()),
        7
    );
    let live = fork.eval_string("f").unwrap();
    assert_eq!(
        integer(fork.invoke(&live, std::slice::from_ref(&object)).unwrap()),
        99
    );
    // Saved protocol functions continue selecting their own object's captured method.
    assert_eq!(
        integer(fork.invoke(&method, std::slice::from_ref(&object)).unwrap()),
        7
    );
    let mut fresh = Context::new();
    assert!(matches!(
        fresh.invoke(&method, &[object]),
        Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner))
    ));
}

#[test]
fn reify_construction_protocol_and_captured_callbacks_obey_current_permissions() {
    let mut c = Context::new();
    c.register_native(&PLUS);
    c.eval_string("(defprotocol P (f [x]))").unwrap();
    let method = c.eval_string("f").unwrap();
    let object = c
        .eval_string("(let [callback host/increment] (reify P (f [_] (callback 6))))")
        .unwrap();
    let source = c.source("reify.clj", "(reify P (f [_] 1))");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let code = c.analyze_form(&form).unwrap();
    assert_eq!(
        integer(c.invoke(&method, std::slice::from_ref(&object)).unwrap()),
        7
    );
    let mut denied_callback = c.fork().unwrap();
    deny(&mut denied_callback, &["host/increment"]);
    assert_eq!(
        denied_callback
            .invoke(&method, std::slice::from_ref(&object))
            .unwrap_err()
            .class(),
        ErrorClass::Diagnostic
    );
    let mut denied_protocol = c.fork().unwrap();
    deny(&mut denied_protocol, &["user/P"]);
    rejected(&mut denied_protocol, "(reify P (f [_] 1))");
    assert!(matches!(
        denied_protocol.eval_analyzed(&code).unwrap_err().kind,
        EvalErrorKind::PermissionDenied(_)
    ));
    deny(&mut c, &["clojure.core/reify"]);
    assert!(matches!(
        c.eval_analyzed(&code).unwrap_err().kind,
        EvalErrorKind::PermissionDenied(_)
    ));
    assert_eq!(integer(c.invoke(&method, &[object]).unwrap()), 7);
}

#[test]
fn records_keep_host_roots_and_follow_shared_constructor_vars_across_forks() {
    let mut c = Context::new();
    let source = c.source("records.clj", "(defrecord R [x])");
    c.eval_source(&source).unwrap();
    let constructor = c.eval_string("->R").unwrap();
    let old_type = c.eval_string("R").unwrap();
    let old = c
        .invoke(&constructor, &[c.vector(&[c.integer(7)]).unwrap()])
        .unwrap();
    let mut fork = c.fork().unwrap();
    drop(source);
    fork.eval_string("(defrecord R [x])").unwrap();
    c.collect_garbage();
    let new = c
        .invoke(&constructor, &[c.vector(&[c.integer(7)]).unwrap()])
        .unwrap();
    assert!(!old.value_eq(&new));
    assert!(!old_type.same_identity(&fork.eval_string("R").unwrap()));
    c.reset();
    assert_eq!(
        old.get(&fork.keyword(Name::new(None, "x")))
            .unwrap()
            .unwrap()
            .count(),
        Some(1)
    );
    assert!(matches!(
        c.invoke(&constructor, &[c.nil()]),
        Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner))
    ));
    assert!(matches!(
        c.assoc(&old, &c.keyword(Name::new(None, "x")), &c.nil()),
        Err(CollectionError::WrongOwner)
    ));
    fork.collect_garbage();
    assert!(
        new.value_eq(
            &fork
                .invoke(&constructor, &[fork.vector(&[fork.integer(7)]).unwrap()])
                .unwrap()
        )
    );
    deny(&mut fork, &["user/R"]);
    assert!(matches!(
        fork.invoke(&constructor, &[fork.nil()]),
        Err(InvokeError::Execution(_))
    ));
}

#[test]
fn record_key_realization_and_walk_root_fields_across_callback_gc() {
    static GC: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "record-gc",
            arity: Arity::Exact(1),
            effects: NativeEffects::Context,
            errors: &[],
        },
        callback: |c, args| {
            c.collect_garbage();
            Ok(args[0].clone())
        },
    };
    let mut c = Context::new();
    c.register_native(&GC);
    assert_eq!(c.eval_string("(defrecord R [x]) (let [r (with-meta (->R (map host/record-gc [1 2 3])) {:m [9]})] [(= r (->R [1 2 3])) (get {r :ok} (->R [1 2 3])) (record? (clojure.walk/postwalk host/record-gc r))])").unwrap().elements().unwrap().iter().map(RootedValue::inspect).collect::<Vec<_>>(), vec![ValueView::Boolean(true), ValueView::Keyword(Name::new(None, "ok")), ValueView::Boolean(true)]);
}

#[test]
fn retained_record_helpers_check_current_type_and_definition_permissions() {
    let mut c = Context::new();
    c.eval_string("(defrecord R [x]) (def saved-type R) (def private-construct (first (nth (nth (nth (macroexpand-1 '(defrecord R [x])) 2) 2) 2)))").unwrap();
    let make = c.eval_string("private-construct").unwrap();
    let record_type = c.eval_string("saved-type").unwrap();
    let fields = c.vector(&[c.integer(7)]).unwrap();
    assert_eq!(
        c.invoke(&make, &[record_type.clone(), fields.clone()])
            .unwrap()
            .inspect(),
        ValueView::Record(1)
    );
    deny(&mut c, &["user/R"]);
    assert!(matches!(
        c.invoke(&make, &[record_type, fields]),
        Err(InvokeError::Dispatch(EvalErrorKind::PermissionDenied(_)))
    ));
}

#[test]
fn record_methods_retain_generation_fields_and_captures_across_reentrant_gc_and_context_drop() {
    static GC: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "record-method-gc",
            arity: Arity::Exact(1),
            effects: NativeEffects::Context,
            errors: &[],
        },
        callback: |c, a| {
            assert!(matches!(a[0].inspect(), ValueView::Record(_)));
            c.eval_string("(defrecord R [x] P (p [_] (+ x 100)))")
                .unwrap();
            c.collect_garbage();
            Ok(c.nil())
        },
    };
    let mut c = Context::new();
    c.register_native(&GC);
    c.eval_string("(defprotocol P (p [r]))").unwrap();
    let source = c.source("record-methods.clj", "(let [kept [7]] (defrecord R [x] P (p [this] (host/record-method-gc this) (+ x (first kept)))))");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    c.eval_form(&form).unwrap();
    let method = c.eval_string("p").unwrap();
    let saved = c.eval_string("->R").unwrap();
    let old = c.invoke(&saved, &[c.integer(2)]).unwrap();
    let mut fork = c.fork().unwrap();
    drop((source, form, c));
    fork.collect_garbage();
    assert_eq!(
        integer(fork.invoke(&method, std::slice::from_ref(&old)).unwrap()),
        9
    );
    let new = fork.invoke(&saved, &[fork.integer(2)]).unwrap();
    assert_eq!(integer(fork.invoke(&method, &[new]).unwrap()), 102);
    assert_eq!(
        integer(fork.invoke(&method, std::slice::from_ref(&old)).unwrap()),
        9
    );
    let mut other = Context::new();
    assert!(matches!(
        other.invoke(&method, &[old]),
        Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner))
    ));
}

#[test]
fn record_methods_recheck_permissions_and_retained_definition_protocols() {
    let mut c = Context::new();
    c.register_native(&PLUS);
    c.eval_string("(defprotocol P (p [r])) (let [callback host/increment] (defrecord R [x] P (p [_] (callback x))))").unwrap();
    let method = c.eval_string("p").unwrap();
    let object = c.eval_string("(->R 6)").unwrap();
    let source = c.source("record-policy.clj", "(defrecord R [x] P (p [_] x))");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let code = c.analyze_form(&form).unwrap();
    for denied_name in ["user/P", "user/R", "clojure.core/defrecord", "def"] {
        let mut denied = c.fork().unwrap();
        deny(&mut denied, &[denied_name]);
        assert!(
            matches!(
                denied.eval_analyzed(&code).unwrap_err().kind,
                EvalErrorKind::PermissionDenied(_)
            ),
            "{denied_name}"
        );
    }
    for denied_name in ["user/p", "host/increment"] {
        let mut denied = c.fork().unwrap();
        deny(&mut denied, &[denied_name]);
        assert_eq!(
            denied
                .invoke(&method, std::slice::from_ref(&object))
                .unwrap_err()
                .class(),
            ErrorClass::Diagnostic
        );
    }
    assert_eq!(integer(c.invoke(&method, &[object]).unwrap()), 7);
}
#[test]
fn extension_replacement_keeps_inflight_closure_rooted_and_releases_it_afterward() {
    static REPLACE: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "replace-extension",
            arity: Arity::Exact(1),
            effects: NativeEffects::Context,
            errors: &[],
        },
        callback: |c, a| {
            assert!(matches!(a[0].inspect(), ValueView::Record(_)));
            c.eval_string("(extend-type R P (p [r] (+ 100 (:n r))))")
                .unwrap();
            c.collect_garbage();
            Ok(c.nil())
        },
    };
    let mut c = Context::new();
    c.register_native(&REPLACE);
    c.eval_string("(defprotocol P (p [x])) (defrecord R [n])")
        .unwrap();
    let captured = c.eval_string("(let [kept [7]] (extend-type R P (p [r] (host/replace-extension r) (+ (:n r) (first kept)))) kept)").unwrap();
    let captured_id = captured.heap_id().unwrap();
    drop(captured);
    let object = c.eval_string("(->R 2)").unwrap();
    let method = c.eval_string("p").unwrap();
    let mut fork = c.fork().unwrap();
    drop(c);
    fork.collect_garbage();
    assert!(fork.heap_value(captured_id).is_ok());
    assert_eq!(
        integer(fork.invoke(&method, std::slice::from_ref(&object)).unwrap()),
        9
    );
    fork.collect_garbage();
    assert!(matches!(
        fork.heap_value(captured_id),
        Err(HandleError::StaleId)
    ));
    assert_eq!(integer(fork.invoke(&method, &[object]).unwrap()), 102);
}

#[test]
fn retained_extensions_recheck_target_protocol_and_macro_permissions() {
    let mut c = Context::new();
    c.register_native(&PLUS);
    c.eval_string("(defprotocol P (p [x])) (defrecord R [n])")
        .unwrap();
    for source in [
        "(extend-type R P (p [r] (:n r)))",
        "(extend-protocol P R (p [r] (:n r)))",
    ] {
        let form = c.source("extension-policy.clj", source);
        let form = c.reader(&form).unwrap().read_form().unwrap().unwrap();
        let code = c.analyze_form(&form).unwrap();
        for name in [
            "user/P",
            "user/R",
            if source.starts_with("(extend-type") {
                "clojure.core/extend-type"
            } else {
                "clojure.core/extend-protocol"
            },
        ] {
            let mut denied = c.fork().unwrap();
            deny(&mut denied, &[name]);
            assert!(
                matches!(
                    denied.eval_analyzed(&code).unwrap_err().kind,
                    EvalErrorKind::PermissionDenied(_)
                ),
                "{name}"
            );
        }
    }
    c.eval_string("(let [callback host/increment] (extend-type R P (p [r] (callback (:n r)))))")
        .unwrap();
    let method = c.eval_string("p").unwrap();
    let object = c.eval_string("(->R 6)").unwrap();
    for name in ["user/p", "host/increment"] {
        let mut denied = c.fork().unwrap();
        deny(&mut denied, &[name]);
        assert_eq!(
            denied
                .invoke(&method, std::slice::from_ref(&object))
                .unwrap_err()
                .class(),
            ErrorClass::Diagnostic
        );
    }
    assert_eq!(integer(c.invoke(&method, &[object]).unwrap()), 7);
    c.eval_string("(alter-meta! #'R assoc :sci/built-in true)")
        .unwrap();
    assert!(c.eval_string("(extend-type R P (p [_] 9))").is_err());
}
