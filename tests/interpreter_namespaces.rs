use light_cljs::interpreter::{
    Arity, Context, EvalErrorKind as E, EvalPhase, HandleError, Name, NativeDefinition,
    NativeEffects, NativeMetadata, RootedValue, ValueView,
};
use std::cell::RefCell;
#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

fn wire(value: &RootedValue) -> protocol::Value {
    use protocol::Value as V;
    match value.inspect() {
        ValueView::CompilerLiteral(_) => panic!("compiler-only reader value in native fixture"),
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
        | ValueView::Protocol(_)
        | ValueView::Reify
        | ValueView::Record(_)
        | ValueView::RecordType(_)
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
fn namespaces_execute_all_focused_pinned_sci_outcomes() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| {
        c.id.starts_with("sci-var-")
            || c.id.starts_with("sci-namespace-")
            || matches!(
                c.id,
                "sci-get-string" | "sci-contains-nil" | "sci-contains-string"
            )
    }) {
        let mut context = Context::new();
        context.register_native(&OBSERVE);
        TRACE.with(|trace| trace.borrow_mut().clear());
        let outcome = match context.eval_string(case.source) {
            Ok(value) => O::Returned(wire(&value)),
            Err(error) => {
                assert_eq!(error.phase, EvalPhase::Analysis, "{}: {error}", case.id);
                assert!(
                    matches!(error.kind, E::UnresolvedSymbol(_)),
                    "{}: {error}",
                    case.id
                );
                O::Error {
                    phase: P::Analysis,
                    category: C::UnresolvedSymbol,
                }
            }
        };
        assert_eq!(outcome, case.outcome, "{}: {}", case.id, case.source);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), case.effects, "{} effects", case.id));
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        count += 1;
    }
    assert_eq!(count, 37);
}

fn eval(c: &mut Context, source: &str) -> ValueView {
    c.eval_string(source).unwrap().inspect()
}

#[test]
fn persistent_contexts_keep_var_ids_and_resolve_lexicals_before_namespace_core() {
    let mut c = Context::new();
    let var = c.eval_string("(def x 1)").unwrap();
    let function = c.eval_string("(fn* [] x)").unwrap();
    let source = c.source("read.clj", "x");
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    let analyzed = c.analyze_form(&form).unwrap();
    let rebound = c.eval_string("(def x 2)").unwrap();
    assert!(var.same_identity(&rebound));
    assert_eq!(var.var_id(), rebound.var_id());
    assert_eq!(
        c.eval_analyzed(&analyzed).unwrap().inspect(),
        ValueView::Integer(2)
    );
    assert_eq!(
        c.invoke(&function, &[]).unwrap().inspect(),
        ValueView::Integer(2)
    );
    assert_eq!(
        eval(&mut c, "(let* [x 3] (def x x) x)"),
        ValueView::Integer(3)
    );
    assert_eq!(
        eval(&mut c, "(let* [inc (fn* [x] x)] (inc 8))"),
        ValueView::Integer(8)
    );
    assert!(matches!(
        Context::new().eval_string("x").unwrap_err().kind,
        E::UnresolvedSymbol(_)
    ));
    c.eval_string("(in-ns 'other) (def x 4)").unwrap();
    assert_eq!(eval(&mut c, "x"), ValueView::Integer(4));
    assert_eq!(eval(&mut c, "user/x"), ValueView::Integer(3));
    assert_eq!(
        c.eval_analyzed(&analyzed).unwrap().inspect(),
        ValueView::Integer(3)
    );
    assert_eq!(eval(&mut c, "(inc x)"), ValueView::Integer(5));
}

#[test]
fn unbound_values_and_var_call_cycles_fail_without_losing_identity() {
    let mut c = Context::new();
    let var = c.eval_string("(declare x) #'x").unwrap();
    assert_eq!(
        eval(&mut c, "x"),
        ValueView::Unbound(Name::new(Some("user"), "x"))
    );
    assert_eq!(eval(&mut c, "(if x true false)"), ValueView::Boolean(true));
    assert_eq!(c.eval_string("(x)").unwrap_err().kind, E::NotCallable);
    assert_eq!(c.eval_string("(#'x)").unwrap_err().kind, E::NotCallable);
    c.eval_string("(def x #'x)").unwrap();
    assert_eq!(c.eval_string("(x)").unwrap_err().kind, E::NotCallable);
    c.eval_string("(def x (fn* [] 9))").unwrap();
    assert_eq!(
        c.invoke(&var, &[]).unwrap().inspect(),
        ValueView::Integer(9)
    );
    assert_eq!(eval(&mut c, "(fn? #'x)"), ValueView::Boolean(false));
    assert_eq!(eval(&mut c, "(ifn? #'x)"), ValueView::Boolean(true));
}

#[test]
fn top_level_do_is_incremental_and_failed_def_analysis_still_interns() {
    let mut c = Context::new();
    assert_eq!(
        eval(
            &mut c,
            "(do (declare x) (do (def f (fn* [] x)) (in-ns 'n)) (def x 7) (def user-value (user/f)) user-value)"
        ),
        ValueView::Unbound(Name::new(Some("n"), "user-value"))
    );
    c.eval_string("(in-ns 'user)").unwrap();
    let error = c
        .eval_string("(do (def done 42) (def bad missing))")
        .unwrap_err();
    assert_eq!(error.phase, EvalPhase::Analysis);
    assert_eq!(eval(&mut c, "done"), ValueView::Integer(42));
    assert_eq!(eval(&mut c, "(bound? #'bad)"), ValueView::Boolean(false));
    assert_eq!(eval(&mut c, "(do (def x 3) (f))"), ValueView::Integer(3));
    c.eval_string("(def ^{:keep 1} x 8)").unwrap();
    let error = c.eval_string("(def ^{:fail (/ 1 0)} x 9)").unwrap_err();
    assert_eq!(error.phase, EvalPhase::Execution);
    assert_eq!(eval(&mut c, "x"), ValueView::Integer(8));
    assert_eq!(
        eval(&mut c, "(get (meta #'x) :keep)"),
        ValueView::Integer(1)
    );
    assert_eq!(
        eval(
            &mut c,
            "(def ^:ignored user/x 10) (get (meta #'x) :ignored)"
        ),
        ValueView::Nil
    );
    for text in [
        "(def)",
        "(def 1 2)",
        "(def x 1 2)",
        "(def x 1 2 3)",
        "(var 1)",
        "(var x y)",
        "(declare 2)",
        "(def other/x 2)",
    ] {
        assert_eq!(
            c.eval_string(text).unwrap_err().phase,
            EvalPhase::Analysis,
            "{text}"
        );
    }
}

#[test]
fn embedding_refers_introspection_metadata_and_owner_checks_are_explicit() {
    let mut c = Context::new();
    let src = c.create_namespace(Name::new(None, "source")).unwrap();
    let dst = c.create_namespace(Name::new(None, "destination")).unwrap();
    let var = c
        .intern_var(src.namespace_id().unwrap(), Name::new(None, "x"))
        .unwrap();
    c.set_var_root(var.var_id().unwrap(), &c.integer(13))
        .unwrap();
    c.refer_var(
        dst.namespace_id().unwrap(),
        Name::new(None, "renamed"),
        var.var_id().unwrap(),
    )
    .unwrap();
    c.set_current_namespace(dst.namespace_id().unwrap())
        .unwrap();
    assert_eq!(eval(&mut c, "renamed"), ValueView::Integer(13));
    assert_eq!(
        eval(&mut c, "(ns-name (get (meta #'renamed) :ns))"),
        ValueView::Symbol(Name::new(None, "source"))
    );
    assert_eq!(
        eval(&mut c, "(get (meta #'renamed) :name)"),
        ValueView::Symbol(Name::new(None, "x"))
    );
    assert_eq!(
        eval(&mut c, "(contains? (ns-refers *ns*) 'renamed)"),
        ValueView::Boolean(true)
    );
    assert_eq!(
        eval(&mut c, "(contains? (ns-interns *ns*) 'renamed)"),
        ValueView::Boolean(false)
    );
    assert!(matches!(
        c.eval_string("(def renamed 14)").unwrap_err().kind,
        E::Namespace(_)
    ));
    c.eval_string("(ns-unmap *ns* 'renamed) (def renamed 14)")
        .unwrap();
    assert_ne!(
        c.resolve_var(&Name::new(None, "renamed"))
            .unwrap()
            .unwrap()
            .var_id(),
        var.var_id()
    );
    c.eval_string("(in-ns 'source) (def ^:private hidden 2) (in-ns 'consumer) (refer 'source)")
        .unwrap();
    assert_eq!(eval(&mut c, "x"), ValueView::Integer(13));
    assert_eq!(eval(&mut c, "hidden"), ValueView::Integer(2));
    let foreign = Context::new();
    assert_eq!(
        foreign.var_value(var.var_id().unwrap()).unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        foreign
            .namespace_value(src.namespace_id().unwrap())
            .unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        c.set_var_root(var.var_id().unwrap(), &foreign.integer(1))
            .unwrap_err(),
        HandleError::WrongOwner
    );
    let source = c.source("location.clj", "\n (def located 1)");
    c.eval_source(&source).unwrap();
    assert_eq!(
        eval(
            &mut c,
            "[(get (meta #'located) :line) (get (meta #'located) :column)]"
        ),
        ValueView::Vector(2)
    );
    assert_eq!(
        eval(&mut c, "(get (meta #'located) :line)"),
        ValueView::Integer(2)
    );
    assert_eq!(
        eval(&mut c, "(get (meta #'located) :column)"),
        ValueView::Integer(2)
    );
}

#[test]
fn namespace_roots_and_retained_code_trace_vars_and_reclaim_old_roots() {
    let mut c = Context::new();
    c.eval_string("(def x [\"retained\"]) (def f (fn* [] x))")
        .unwrap();
    c.collect_garbage();
    let function = c.eval_string("f").unwrap();
    assert_eq!(
        c.invoke(&function, &[])
            .unwrap()
            .nth(0)
            .unwrap()
            .unwrap()
            .inspect(),
        ValueView::String("retained".encode_utf16().collect::<Vec<_>>().into())
    );
    for _ in 0..3 {
        c.eval_string("(def x [\"new\"]) (def f (fn* [] x))")
            .unwrap();
        c.collect_garbage();
    }
    let baseline = c.memory_stats();
    for _ in 0..25 {
        c.eval_string("(def x [\"new\"]) (def f (fn* [] x))")
            .unwrap();
        c.collect_garbage();
        assert_eq!(c.memory_stats(), baseline);
    }
    let old_var = c.resolve_var(&Name::new(None, "x")).unwrap().unwrap();
    let old_id = old_var.var_id().unwrap();
    c.eval_string("(ns-unmap *ns* 'x) (def x 99)").unwrap();
    c.collect_garbage();
    assert_ne!(
        old_id,
        c.resolve_var(&Name::new(None, "x"))
            .unwrap()
            .unwrap()
            .var_id()
            .unwrap()
    );
    assert_eq!(c.invoke(&function, &[]).unwrap().count(), Some(1));
    c.reset();
    assert!(matches!(
        c.eval_string("x").unwrap_err().kind,
        E::UnresolvedSymbol(_)
    ));
    assert_eq!(c.var_value(old_id).unwrap_err(), HandleError::WrongOwner);
    assert_eq!(
        old_var.inspect(),
        ValueView::Var(Name::new(Some("user"), "x"))
    );
    drop(c);
    assert_eq!(function.inspect(), ValueView::Function);
    assert_eq!(old_var.metadata().unwrap().count(), Some(5));
}

#[test]
fn unmapped_recursive_var_cycles_are_reclaimed_and_raw_ids_become_stale() {
    let mut c = Context::new();
    let ns = c.current_namespace().namespace_id().unwrap();
    c.eval_string("").unwrap();
    c.collect_garbage();
    let baseline = c.memory_stats();
    let mut previous = None;
    for _ in 0..20 {
        let var = c.eval_string("(def f (fn* [] f))").unwrap();
        let id = var.var_id().unwrap();
        assert_ne!(previous, Some(id));
        drop(var);
        c.unmap_var(ns, &Name::new(None, "f")).unwrap();
        c.collect_garbage();
        assert_eq!(c.var_value(id).unwrap_err(), HandleError::StaleId);
        let stats = c.memory_stats();
        assert_eq!(
            (
                stats.heap_values,
                stats.vars,
                stats.executable_units,
                stats.names
            ),
            (
                baseline.heap_values,
                baseline.vars,
                baseline.executable_units,
                baseline.names
            )
        );
        previous = Some(id);
    }
}
#[test]
fn external_record_selectors_and_protocols_resolve_aliases_and_collisions() {
    let mut c = Context::new();
    let result = c.eval_string("(ns a) (defprotocol P (p [x])) (defrecord R [n]) (ns b) (defprotocol P (p [x])) (defrecord R [n]) (ns user (:require [a :as aa] [b :as bb])) (extend-type aa/R aa/P (p [x] (:n x)) bb/P (p [x] (* 2 (:n x)))) (extend-protocol aa/P bb/R (p [x] (* 3 (:n x)))) [(aa/p (aa/->R 2)) (bb/p (aa/->R 2)) (aa/p (bb/->R 2)) (satisfies? bb/P (bb/->R 2))]").unwrap();
    assert_eq!(
        result
            .elements()
            .unwrap()
            .iter()
            .map(RootedValue::inspect)
            .collect::<Vec<_>>(),
        vec![
            ValueView::Integer(2),
            ValueView::Integer(4),
            ValueView::Integer(6),
            ValueView::Boolean(false)
        ]
    );
}
