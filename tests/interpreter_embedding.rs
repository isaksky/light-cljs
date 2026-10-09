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
