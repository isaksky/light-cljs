use light_cljs::interpreter::{
    Arity, Context, ErrorClass, EvalErrorKind, EvalPhase, HandleError, Name, NativeDefinition,
    NativeEffects, NativeError, NativeMetadata, ResourceLimits, RootedValue, ValueView, VarError,
};
use std::cell::RefCell;
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

fn wire(value: &RootedValue) -> protocol::Value {
    use protocol::Value as V;
    match value.inspect() {
        ValueView::LazySeq | ValueView::Delay => {
            panic!("deferred values must be projected by the evaluated fixture")
        }
        ValueView::NativeFunction(_)
        | ValueView::Function
        | ValueView::Var(_)
        | ValueView::Namespace(_)
        | ValueView::Unbound(_)
        | ValueView::Exception(_)
        | ValueView::Atom
        | ValueView::Input
        | ValueView::Output
        | ValueView::Volatile
        | ValueView::Reduced => {
            panic!("callables are outside fixture transport")
        }
        ValueView::Nil => V::Nil,
        ValueView::Boolean(v) => V::Bool(v),
        ValueView::Integer(v) => V::Int(v),
        ValueView::Double(v) => V::Float(if v.is_nan() {
            f64::NAN.to_bits()
        } else {
            v.to_bits()
        }),
        ValueView::Ratio {
            numerator,
            denominator,
        } => V::Ratio(numerator, denominator),
        ValueView::Character(v) => V::Char(v),
        ValueView::String(v) => V::String(v.to_vec()),
        ValueView::Symbol(v) => V::Symbol(v.namespace().map(<[u16]>::to_vec), v.name().to_vec()),
        ValueView::Keyword(v) => V::Keyword(v.namespace().map(<[u16]>::to_vec), v.name().to_vec()),
        ValueView::List(_) => V::List(value.elements().unwrap().iter().map(wire).collect()),
        ValueView::Seq(_) => V::Seq(value.elements().unwrap().iter().map(wire).collect()),
        ValueView::Vector(_) => V::Vector(value.elements().unwrap().iter().map(wire).collect()),
        ValueView::Map(_) => {
            let mut pairs: Vec<_> = value
                .entries()
                .unwrap()
                .iter()
                .map(|(k, v)| (wire(k), wire(v)))
                .collect();
            pairs.sort_by_key(|(k, v)| format!("(entry {} {})", k.wire(), v.wire()));
            V::Map(pairs)
        }
        ValueView::Set(_) => {
            let mut values: Vec<_> = value.elements().unwrap().iter().map(wire).collect();
            values.sort_by_key(V::wire);
            V::Set(values)
        }
    }
}

thread_local! { static TRACE: RefCell<Vec<protocol::Value>> = const { RefCell::new(Vec::new()) }; }
static OBSERVE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "observe",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |context, args| {
        TRACE.with(|trace| trace.borrow_mut().push(wire(&args[0])));
        context.collect_garbage();
        Ok(args[0].clone())
    },
};

#[test]
fn vars_match_pinned_sci_values_and_effects() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut failures = Vec::new();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-vars-")) {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|trace| trace.borrow_mut().clear());
        match c.eval_string(case.source) {
            Ok(value) => {
                let actual = protocol::Outcome::Returned(wire(&value));
                if actual != case.outcome {
                    failures.push(format!("{}: {:?} != {:?}", case.id, actual, case.outcome));
                }
            }
            Err(error) => {
                if !matches!(
                    error.kind,
                    EvalErrorKind::Native {
                        error: NativeError::Arity { .. },
                        ..
                    }
                ) || case.outcome
                    != (protocol::Outcome::Error {
                        phase: protocol::Phase::Execution,
                        category: protocol::Category::Arity,
                    })
                    || error.phase != EvalPhase::Execution
                {
                    failures.push(format!("{}: {error}", case.id));
                }
            }
        }
        TRACE.with(|t| {
            if *t.borrow() != case.effects {
                failures.push(format!(
                    "{} effects: {:?} != {:?}",
                    case.id,
                    t.borrow(),
                    case.effects
                ));
            }
        });
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        count += 1;
    }
    assert_eq!(count, 74);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn id(c: &Context, name: &str) -> light_cljs::interpreter::VarId {
    c.resolve_var(&Name::new(None, name))
        .unwrap()
        .unwrap()
        .var_id()
        .unwrap()
}
fn view(c: &mut Context, source: &str) -> ValueView {
    c.eval_string(source).unwrap().inspect()
}

#[test]
fn host_binding_guards_root_values_restore_out_of_order_and_survive_reset() {
    let mut c = Context::new();
    c.eval_string("(def ^:dynamic x 0)").unwrap();
    let x = id(&c, "x");
    let first = c
        .bind_vars(&[(x, c.vector(&[c.integer(1)]).unwrap())])
        .unwrap();
    let second = c
        .bind_vars(&[(x, c.vector(&[c.integer(2)]).unwrap())])
        .unwrap();
    drop(first);
    c.collect_garbage();
    assert_eq!(view(&mut c, "(first x)"), ValueView::Integer(2));
    drop(second);
    assert_eq!(c.var_get(x).unwrap().inspect(), ValueView::Integer(0));
    let old_var = c.var_value(x).unwrap();
    let guard = c.bind_vars(&[(x, c.integer(3))]).unwrap();
    c.unmap_var(
        c.current_namespace().namespace_id().unwrap(),
        &Name::new(None, "x"),
    )
    .unwrap();
    c.collect_garbage();
    assert_eq!(c.var_get(x).unwrap().inspect(), ValueView::Integer(3));
    c.reset();
    assert_eq!(c.var_get(x).unwrap_err(), HandleError::WrongOwner);
    drop(guard);
    assert!(matches!(old_var.inspect(), ValueView::Var(_)));
    assert!(c.eval_string("x").is_err());
}

#[test]
fn bindings_and_redefinitions_restore_on_rust_errors_and_panics() {
    let mut c = Context::new();
    c.eval_string("(def ^:dynamic x 0)").unwrap();
    let x = id(&c, "x");
    fn failure(c: &mut Context, x: light_cljs::interpreter::VarId) -> Result<(), VarError> {
        let _guard = c.bind_vars(&[(x, c.integer(1))])?;
        Err(VarError::NoBinding)
    }
    assert_eq!(failure(&mut c, x), Err(VarError::NoBinding));
    assert_eq!(c.var_get(x).unwrap().inspect(), ValueView::Integer(0));
    assert_eq!(
        c.with_var_redefs(&[(x, c.integer(2))], |_| Err::<(), _>("body"))
            .unwrap(),
        Err("body")
    );
    assert_eq!(c.var_root(x).unwrap().inspect(), ValueView::Integer(0));
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = c.bind_vars(&[(x, c.integer(3))]).unwrap();
        c.with_var_redefs(&[(x, c.integer(4))], |_| panic!("body"))
            .unwrap();
    }));
    assert!(panic.is_err());
    assert_eq!(c.var_get(x).unwrap().inspect(), ValueView::Integer(0));
    assert_eq!(c.var_root(x).unwrap().inspect(), ValueView::Integer(0));
}

static REENTER: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "user",
        name: "reenter",
        arity: Arity::Exact(0),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |c, _| {
        c.collect_garbage();
        let mut independent = Context::new();
        assert!(independent.eval_string("x").is_err());
        c.eval_string("(in-ns 'nested) (binding [user/x 8] (set! user/x 9) user/x)")
            .map_err(|e| light_cljs::interpreter::InvokeError::Execution(Box::new(e)).into())
    },
};
#[test]
fn callbacks_reader_and_macro_reentry_share_dynamic_scope_without_leaks() {
    let mut c = Context::new();
    c.register_native(&REENTER);
    assert_eq!(
        wire(
            &c.eval_string("(def ^:dynamic x 1) [(binding [x 2] [(reenter) x]) x]")
                .unwrap()
        ),
        protocol::Value::Vector(vec![
            protocol::Value::Vector(vec![protocol::Value::Int(9), protocol::Value::Int(2)]),
            protocol::Value::Int(1)
        ])
    );
    assert_eq!(
        view(&mut c, "(ns-name *ns*)"),
        ValueView::Symbol(Name::new(None, "user"))
    );
    c.eval_string("(create-ns 'other)").unwrap();
    let ns = c.find_namespace(&Name::new(None, "other")).unwrap();
    let guard = c.bind_vars(&[(id(&c, "*ns*"), ns)]).unwrap();
    let source = c.source("dynamic-reader.cljc", "::x");
    let syntax = light_cljs::parse(source.text()).unwrap();
    let form = c.lower_form(&source, &syntax, syntax.roots[0]).unwrap();
    assert_eq!(
        form.data().inspect(),
        ValueView::Keyword(Name::new(Some("other"), "x"))
    );
    drop(guard);
    assert_eq!(
        view(&mut c, "(ns-name *ns*)"),
        ValueView::Symbol(Name::new(None, "user"))
    );
}

#[test]
fn builtin_mutation_is_an_explicit_isolated_context_permission() {
    let mut c = Context::new();
    assert!(matches!(
        c.eval_string("(with-redefs [inc dec] (inc 2))")
            .unwrap_err()
            .kind,
        EvalErrorKind::Var(VarError::ReadOnly)
    ));
    c.allow_builtin_var_mutation(true);
    assert_eq!(
        view(&mut c, "(with-redefs [inc dec] (inc 2))"),
        ValueView::Integer(1)
    );
    assert_eq!(view(&mut c, "(inc 2)"), ValueView::Integer(3));
    assert_eq!(
        view(
            &mut c,
            "(alter-meta! #'inc assoc :custom 5) (:custom (meta #'inc))"
        ),
        ValueView::Integer(5)
    );
    assert_eq!(
        view(&mut c, "(alter-var-root #'inc (constantly dec)) (inc 2)"),
        ValueView::Integer(1)
    );
    let mut independent = Context::new();
    assert!(
        independent
            .eval_string("(alter-var-root #'inc (constantly dec))")
            .is_err()
    );
    c.reset();
    assert!(
        c.eval_string("(alter-var-root #'inc (constantly dec))")
            .is_err()
    );
}

#[test]
fn owner_validation_is_atomic_and_foreign_values_never_enter_scopes() {
    let mut c = Context::new();
    c.eval_string("(def ^:dynamic x 1) (def y 2)").unwrap();
    let foreign = Context::new();
    let x = id(&c, "x");
    let y = id(&c, "y");
    assert!(matches!(
        c.bind_vars(&[(x, foreign.integer(9))]),
        Err(VarError::Handle(HandleError::WrongOwner))
    ));
    assert!(matches!(
        c.bind_vars(&[(x, c.integer(9)), (y, c.integer(8))]),
        Err(VarError::NonDynamic)
    ));
    assert_eq!(c.var_get(x).unwrap().inspect(), ValueView::Integer(1));
    assert_eq!(
        c.with_var_redefs(&[(x, c.integer(9)), (y, foreign.integer(8))], |_| ()),
        Err(VarError::Handle(HandleError::WrongOwner))
    );
    assert_eq!(c.var_root(x).unwrap().inspect(), ValueView::Integer(1));
    assert_eq!(
        c.reset_var_metadata(x, &foreign.nil()),
        Err(VarError::Handle(HandleError::WrongOwner))
    );
    assert_eq!(
        c.set_var_binding(x, &c.integer(3)),
        Err(VarError::NoBinding)
    );
}

#[test]
fn scoped_control_failures_restore_values_without_using_cleanup_budget() {
    for program in [
        "(binding [x 2] (loop [] (recur)))",
        "(with-redefs [x 2] (loop [] (recur)))",
        "(binding [x 2] (with-redefs [x 3] (vec (repeat 1))))",
    ] {
        let mut c = Context::new();
        c.eval_string("(def ^:dynamic x 1)").unwrap();
        c.set_resource_limits(ResourceLimits {
            fuel: Some(10000),
            cleanup_fuel: 0,
            cleanup_allocation: 0,
            ..ResourceLimits::default()
        });
        let error = c.eval_string(program).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control, "{error}");
        let x = id(&c, "x");
        assert_eq!(c.var_get(x).unwrap().inspect(), ValueView::Integer(1));
        assert_eq!(c.var_root(x).unwrap().inspect(), ValueView::Integer(1));
        assert!(!c.var_is_thread_bound(x).unwrap());
    }
}

#[test]
fn invalid_scope_syntax_is_diagnostic_and_recur_cannot_cross_binding() {
    for source in [
        "(binding #{x 1})",
        "(binding [x])",
        "(binding [1 2])",
        "(loop [] (binding [x 2] (recur)))",
        "(let [x 1] (set! x 2))",
    ] {
        let mut c = Context::new();
        c.eval_string("(def ^:dynamic x 1)").unwrap();
        let error = c.eval_string(source).unwrap_err();
        assert!(
            matches!(error.phase, EvalPhase::Analysis | EvalPhase::Macroexpansion),
            "{source}: {error}"
        );
        assert_eq!(
            c.var_get(id(&c, "x")).unwrap().inspect(),
            ValueView::Integer(1)
        );
    }
}

#[test]
fn host_file_binding_flows_through_eval_reentry_and_definition_metadata() {
    let mut c = Context::new();
    assert_eq!(view(&mut c, "*file*"), ValueView::Nil);
    let file = id(&c, "*file*");
    let guard = c
        .bind_vars(&[(
            file,
            c.string("host.clj".encode_utf16().collect::<Vec<_>>()),
        )])
        .unwrap();
    assert_eq!(
        wire(&c.eval_string("[*file* (:file (meta (def x 1)))]").unwrap()),
        protocol::Value::Vector(vec![
            protocol::Value::String(
                "host.clj".encode_utf16().collect()
            );
            2
        ])
    );
    let named = c.source("named.clj", "[*file* (:file (meta (def y 1)))]");
    assert_eq!(
        wire(&c.eval_source(&named).unwrap()),
        protocol::Value::Vector(vec![
            protocol::Value::String(
                "named.clj".encode_utf16().collect()
            );
            2
        ])
    );
    assert_eq!(
        view(&mut c, "*file*"),
        ValueView::String("host.clj".encode_utf16().collect::<Vec<_>>().into())
    );
    drop(guard);
    assert_eq!(view(&mut c, "*file*"), ValueView::Nil);
}

#[test]
fn allocation_and_call_depth_failures_unwind_dynamic_slots_and_raw_roots() {
    for limits in [
        ResourceLimits {
            allocation: Some(1800),
            ..ResourceLimits::default()
        },
        ResourceLimits {
            call_depth: 6,
            ..ResourceLimits::default()
        },
    ] {
        let mut c = Context::new();
        c.eval_string("(def ^:dynamic x 1) (defn f [] (binding [x 2] (with-redefs [x 3] (f))))")
            .unwrap();
        c.set_resource_limits(limits);
        let error = c.eval_string("(f)").unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control, "{error}");
        assert_eq!(
            c.var_get(id(&c, "x")).unwrap().inspect(),
            ValueView::Integer(1)
        );
        assert_eq!(
            c.var_root(id(&c, "x")).unwrap().inspect(),
            ValueView::Integer(1)
        );
    }
}

#[test]
fn dynamic_metadata_cycles_and_saved_redefinition_roots_are_traced_and_reclaimed() {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    c.collect_garbage();
    let baseline = c.memory_stats();
    for _ in 0..20 {
        c.eval_string("(def ^:dynamic x (fn [] 1)) (alter-meta! #'x assoc :self #'x)")
            .unwrap();
        let x = id(&c, "x");
        let ns = c.current_namespace().namespace_id().unwrap();
        c.with_var_redefs(&[(x, c.nil())], |c| {
            c.unmap_var(ns, &Name::new(None, "x")).unwrap();
            c.collect_garbage();
            assert_eq!(c.var_root(x).unwrap().inspect(), ValueView::Nil);
        })
        .unwrap();
        let root = c.var_root(x).unwrap();
        assert_eq!(
            c.invoke(&root, &[]).unwrap().inspect(),
            ValueView::Integer(1)
        );
        drop(root);
        c.collect_garbage();
        assert_eq!(c.var_value(x).unwrap_err(), HandleError::StaleId);
        let after = c.memory_stats();
        assert_eq!(after.heap_values, baseline.heap_values);
        assert_eq!(after.vars, baseline.vars);
        assert_eq!(after.executable_units, baseline.executable_units);
    }
}

#[test]
fn macroexpansion_can_be_replayed_as_owned_data_and_keeps_cleanup() {
    let mut c = Context::new();
    c.eval_string("(def ^:dynamic x 1)").unwrap();
    let expanded = c.eval_string("(macroexpand '(binding [x 2] x))").unwrap();
    let source = c.source("owned-expansion.clj", "(binding [x 2] x)");
    let span = source.span(0..source.text().len()).unwrap();
    let form = c.form_from_data(&expanded, &span).unwrap();
    c.collect_garbage();
    assert_eq!(c.eval_form(&form).unwrap().inspect(), ValueView::Integer(2));
    assert_eq!(view(&mut c, "x"), ValueView::Integer(1));
}
