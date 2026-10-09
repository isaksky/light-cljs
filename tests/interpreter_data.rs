use light_cljs::interpreter::EvalErrorKind as E;
use light_cljs::interpreter::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
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
fn data_libraries_match_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-data-")) {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        let stdout = OutputBuffer::default();
        let stderr = OutputBuffer::default();
        let out = c.output_stream(stdout.clone());
        let err = c.output_stream(stderr.clone());
        let _guard = c.bind_io(None, Some(&out), Some(&err)).unwrap();
        TRACE.with(|t| t.borrow_mut().clear());
        let actual = match c.eval_string(case.source) {
            Ok(v) => O::Returned(wire(&v)),
            Err(e) => {
                let phase = match e.phase {
                    EvalPhase::Execution | EvalPhase::Macroexpansion => P::Execution,
                    EvalPhase::Analysis => P::Analysis,
                    EvalPhase::Read => P::Read,
                };
                let category = match e.kind {
                    E::InvalidForm(_) => C::InvalidForm,
                    E::Read(_) => C::Reader,
                    E::UnresolvedSymbol(_) => C::UnresolvedSymbol,
                    E::Arity { .. }
                    | E::Native {
                        error: NativeError::Arity { .. },
                        ..
                    } => C::Arity,
                    E::NotCallable
                    | E::Native {
                        error: NativeError::Type { .. },
                        ..
                    } => C::Type,
                    E::Native {
                        error: NativeError::StringBounds { .. },
                        ..
                    } => C::Bounds,
                    E::Native {
                        error: NativeError::Collection(_),
                        ..
                    } => C::Type,
                    E::Native {
                        error: NativeError::Arithmetic(_),
                        ..
                    } => C::Arithmetic,
                    E::Thrown(ExceptionCategory::ExceptionInfo) => C::ExceptionInfo,
                    _ => panic!("{}: {e}", case.id),
                };
                O::Error { phase, category }
            }
        };
        assert_eq!(actual, case.outcome, "{}: {}", case.id, case.source);
        TRACE.with(|t| assert_eq!(*t.borrow(), case.effects, "{} effects", case.id));
        assert_eq!(stdout.units(), case.stdout, "{} stdout", case.id);
        assert_eq!(stderr.units(), case.stderr, "{} stderr", case.id);
        assert_eq!(
            c.eval_string("(+ 20 22)").unwrap().inspect(),
            ValueView::Integer(42)
        );
        count += 1;
    }
    assert_eq!(count, 178);
}

fn returned(c: &mut Context, source: &str) -> protocol::Value {
    wire(
        &c.eval_string(source)
            .unwrap_or_else(|error| panic!("{source}: {error}")),
    )
}
fn call(
    c: &mut Context,
    namespace: &str,
    name: &str,
    args: &[RootedValue],
) -> Result<RootedValue, InvokeError> {
    let callable = c.eval_string(&format!("{namespace}/{name}")).unwrap();
    c.invoke(&callable, args)
}

#[test]
fn every_export_is_registered_through_require_host_initialization_and_introspection() {
    for definitions in [set_definitions(), walk_definitions(), edn_definitions()] {
        let mut c = Context::new();
        let namespace = definitions[0].metadata.namespace;
        let exposed = c
            .eval_string(&format!("(keys (ns-publics '{namespace}))"))
            .unwrap();
        assert_eq!(exposed.elements().unwrap().len(), definitions.len());
        c.eval_string(&format!(
            "(require '[{namespace} :as lib :refer :all] :reload)"
        ))
        .unwrap();
        for definition in definitions {
            let direct = c
                .eval_string(&format!("{namespace}/{}", definition.metadata.name))
                .unwrap();
            let alias = c
                .eval_string(&format!("lib/{}", definition.metadata.name))
                .unwrap();
            let referred = c.eval_string(definition.metadata.name).unwrap();
            assert!(direct.same_identity(&alias));
            assert!(direct.same_identity(&referred));
            let host = c.register_native(definition);
            assert!(matches!(host.inspect(), ValueView::NativeFunction(_)));
        }
    }
}

#[test]
fn edn_is_a_restricted_mode_on_both_shared_reader_cursors() {
    let mut c = Context::new();
    let source = c.source("portable.edn", "nil [1 2] #:a{:x 3}");
    let mut reader = c.edn_reader(&source, EdnOptions::default()).unwrap();
    assert_eq!(
        reader.read_form().unwrap().unwrap().data().inspect(),
        ValueView::Nil
    );
    let form = reader.read_form().unwrap().unwrap();
    assert_eq!(form.span().text(), "[1 2]");
    assert_eq!(form.data().count(), Some(2));
    assert_eq!(reader.read_form().unwrap().unwrap().data().count(), Some(1));
    assert!(reader.read_form().unwrap().is_none());
    assert!(reader.read_form().unwrap().is_none());
    let mut cursor = c.edn_source_reader(source, EdnOptions::default()).unwrap();
    assert!(cursor.read_form(&c).unwrap().is_some());
    let (form, consumed) = cursor.read_form_with_source(&c).unwrap().unwrap();
    assert_eq!(consumed.text(), " [1 2]");
    c.reset();
    assert_eq!(form.span().text(), "[1 2]");
    assert_eq!(form.data().count(), Some(2));
    assert!(matches!(
        cursor.read_form(&c).unwrap_err().kind,
        ReadErrorKind::Lower(LowerErrorKind::WrongOwner)
    ));
}

#[test]
fn edn_explicitly_rejects_executable_syntax_numeric_extensions_and_host_values() {
    let c = Context::new();
    for text in [
        "'x",
        "`x",
        "~x",
        "~@x",
        "@x",
        "#'x",
        "#(inc %)",
        "#=1",
        "#?(:clj 1)",
        "#?@(:clj [1])",
        "::x",
        "#::{:x 1}",
        "#\"regex\"",
        "#inst \"2000-01-01\"",
        "#uuid \"00000000-0000-0000-0000-000000000000\"",
        "1N",
        "1M",
        "9223372036854775808",
        "1e999",
        "#_#(inc %) 1",
        "#_`x 1",
        "#!comment\n1",
        "[1 #!comment\n2]",
    ] {
        let source = c.source("invalid.edn", text);
        let mut reader = c.edn_reader(&source, EdnOptions::default()).unwrap();
        let error = reader.read_form().unwrap_err();
        assert_eq!(error.span.source().name(), "invalid.edn", "{text}");
        assert!(reader.read_form().unwrap().is_none(), "fused after {text}");
    }
    let mut c = c;
    for body in [
        "(fn [] 1)",
        "(atom 1)",
        "(delay 1)",
        "(map identity [1])",
        "(find-ns 'user)",
        "#'identity",
        "[{:x (atom 1)}]",
    ] {
        let error = c
            .eval_string(&format!(
                "(clojure.edn/read-string {{:readers {{'tag (fn [v] {body})}}}} \"#tag 1\")"
            ))
            .unwrap_err();
        assert!(
            matches!(
                error.kind,
                E::Read(ReadErrorKind::Lower(LowerErrorKind::UnsupportedEdnValue))
            ),
            "{body}: {error}"
        );
    }
}

#[test]
fn edn_options_are_local_owner_checked_and_preserve_callback_errors() {
    let mut c = Context::new();
    c.register_data_reader(Name::new(None, "tag"), &OBSERVE);
    c.set_default_data_reader(Some(&OBSERVE));
    assert!(
        c.eval_string("(clojure.edn/read-string \"#tag 1\")")
            .is_err()
    );
    assert_eq!(
        returned(
            &mut c,
            "(clojure.edn/read-string {:readers {'tag inc}} \"#tag 1\")"
        ),
        protocol::Value::Int(2)
    );
    assert!(
        c.eval_string("(clojure.edn/read-string \"#tag 1\")")
            .is_err()
    );
    assert_eq!(
        returned(
            &mut c,
            "(let [e (ex-info \"tag\" {:kind :reference/throw})] (try (clojure.edn/read-string {:readers {'tag (fn [v] (throw e))}} \"#tag 1\") (catch Exception x (identical? e x))))"
        ),
        protocol::Value::Bool(true)
    );
    let foreign = Context::new();
    let options = EdnOptions {
        default: Some(foreign.integer(1)),
        ..Default::default()
    };
    let source = c.source("wrong.edn", "1");
    assert!(matches!(
        c.edn_reader(&source, options),
        Err(HandleError::WrongOwner)
    ));
    for source in [
        "(clojure.edn/read-string {:features #{:clj}} \"1\")",
        "(clojure.edn/read-string {:readers 1} \"1\")",
        "(clojure.edn/read-string {:readers {:bad identity}} \"1\")",
        "(clojure.edn/read-string {} 3)",
    ] {
        assert!(c.eval_string(source).is_err(), "{source}");
    }
}

#[test]
fn edn_read_uses_explicit_io_and_keeps_exact_eof_and_error_boundaries() {
    let mut c = Context::new();
    assert!(c.eval_string("(clojure.edn/read)").is_err());
    assert_eq!(
        returned(
            &mut c,
            "(with-in-str \"nil 42\" [(clojure.edn/read) (clojure.edn/read) (clojure.edn/read {:eof false} *in*)])"
        ),
        protocol::Value::Vector(vec![
            protocol::Value::Nil,
            protocol::Value::Int(42),
            protocol::Value::Bool(false)
        ])
    );
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    let mut lines = ["#tag\n", "[1\n", "2]\n", "nil\n"].into_iter();
    let input = c.input_stream("stream.edn", move || {
        seen.set(seen.get() + 1);
        Ok(lines.next().map(|text| text.encode_utf16().collect()))
    });
    let _guard = c.bind_io(Some(&input), None, None).unwrap();
    TRACE.with(|t| t.borrow_mut().clear());
    c.register_native(&OBSERVE);
    assert_eq!(
        returned(&mut c, "(clojure.edn/read {:readers {'tag observe}} *in*)"),
        protocol::Value::Vector(vec![protocol::Value::Int(1), protocol::Value::Int(2)])
    );
    assert_eq!(calls.get(), 3);
    TRACE.with(|t| assert_eq!(t.borrow().len(), 1));
    assert_eq!(
        returned(&mut c, "(clojure.edn/read {:eof :done} *in*)"),
        protocol::Value::Nil
    );
    assert_eq!(
        returned(&mut c, "(clojure.edn/read {:eof :done} *in*)"),
        protocol::Value::Keyword(None, "done".encode_utf16().collect())
    );
}

#[test]
fn walk_gc_metadata_types_replacements_and_opaque_nodes() {
    let mut c = Context::new();
    c.register_native(&OBSERVE);
    assert_eq!(
        returned(
            &mut c,
            "(let [a (atom 1) d (delay (observe :bad))] (identical? a (clojure.walk/postwalk identity a)))"
        ),
        protocol::Value::Bool(true)
    );
    assert_eq!(
        returned(
            &mut c,
            "(let [d (delay (observe :bad))] (clojure.walk/prewalk identity d) (realized? d))"
        ),
        protocol::Value::Bool(false)
    );
    assert_eq!(
        returned(
            &mut c,
            "(let [x (with-meta (list 1 2) {:m true}) y (clojure.walk/postwalk observe x)] [(list? y) (meta y) y])"
        ),
        protocol::Value::Vector(vec![
            protocol::Value::Bool(true),
            protocol::Value::Map(vec![(
                protocol::Value::Keyword(None, vec![109]),
                protocol::Value::Bool(true)
            )]),
            protocol::Value::List(vec![protocol::Value::Int(1), protocol::Value::Int(2)])
        ])
    );
    assert_eq!(
        returned(&mut c, "(clojure.walk/postwalk-replace {[1 2] :hit} [1 2])"),
        protocol::Value::Keyword(None, "hit".encode_utf16().collect())
    );
    let value = c
        .eval_string("(clojure.walk/postwalk identity {:a [1 (list 2)]})")
        .unwrap();
    c.collect_garbage();
    c.reset();
    assert_eq!(value.count(), Some(1));
}

#[test]
fn all_new_paths_share_fuel_allocation_depth_and_cancellation() {
    let mut c = Context::new();
    let deep = (0..150).fold(c.integer(1), |v, _| c.vector(&[v]).unwrap());
    let identity = c.eval_string("identity").unwrap();
    let error = call(&mut c, "clojure.walk", "postwalk", &[identity, deep]).unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    for source in [
        "(clojure.walk/prewalk (fn [x] [x]) 1)",
        "(clojure.walk/postwalk identity (range))",
        "(clojure.set/project (repeat {:a 1}) [:a])",
        "(clojure.set/index (repeat {:a 1}) [:a])",
        "(clojure.set/join (repeat {:a 1}) #{{:a 1}})",
        "(clojure.edn/read-string {:readers {'tag (fn [v] (loop [] (recur)))}} \"#tag 1\")",
    ] {
        let mut c = Context::new();
        c.set_resource_limits(ResourceLimits {
            fuel: Some(5000),
            ..Default::default()
        });
        assert_eq!(
            c.eval_string(source).unwrap_err().class(),
            ErrorClass::Control,
            "{source}"
        );
    }
    for allocation in [false, true] {
        for source in [
            "(clojure.set/union #{1 2} #{3 4})",
            "(clojure.walk/postwalk identity [1 2])",
            "(clojure.edn/read-string \"[1 2]\")",
        ] {
            let mut c = Context::new();
            let form = c.source("budget.clj", source);
            let form = c.reader(&form).unwrap().read_form().unwrap().unwrap();
            let form = c.analyze_form(&form).unwrap();
            c.set_resource_limits(ResourceLimits {
                fuel: (!allocation).then_some(0),
                allocation: allocation.then_some(0),
                ..Default::default()
            });
            assert_eq!(
                c.eval_analyzed(&form).unwrap_err().class(),
                ErrorClass::Control,
                "{source}"
            );
        }
    }
    let mut c = Context::new();
    let token = CancellationToken::default();
    token.cancel();
    c.set_cancellation_token(Some(token));
    assert_eq!(
        c.eval_string("(clojure.walk/postwalk identity [1])")
            .unwrap_err()
            .class(),
        ErrorClass::Control
    );
}

#[test]
fn edn_callback_stream_reentry_is_explicit_and_other_streams_work() {
    let mut c = Context::new();
    assert_eq!(
        returned(
            &mut c,
            "(clojure.edn/read-string {:readers {'tag (fn [v] (+ v (with-in-str \"2\" (clojure.edn/read))))}} \"#tag 1\")"
        ),
        protocol::Value::Int(3)
    );
    let error=c.eval_string("(with-in-str \"#tag 1 2\" (clojure.edn/read {:readers {'tag (fn [v] (+ v (clojure.edn/read *in*)))}} *in*))").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Host);
    assert!(error.to_string().contains("input stream is already in use"));
}

#[test]
fn set_and_walk_types_errors_and_short_circuit_do_not_hide_failures() {
    let mut c = Context::new();
    for source in [
        "(clojure.set/union [1])",
        "(clojure.set/intersection 1 #{})",
        "(clojure.set/difference #{} [])",
        "(clojure.set/subset? 1 #{})",
        "(clojure.set/superset? #{} 1)",
        "(clojure.set/select identity [])",
        "(clojure.set/rename-keys 1 {})",
        "(clojure.set/project [1] [])",
        "(clojure.set/index [1] [])",
        "(clojure.set/join [{:a 1}] [2])",
        "(clojure.walk/prewalk 1 [2])",
        "(clojure.walk/postwalk-replace [] [1])",
    ] {
        assert_eq!(
            c.eval_string(source).unwrap_err().class(),
            ErrorClass::Language,
            "{source}"
        );
    }
    c.set_resource_limits(ResourceLimits {
        fuel: Some(5000),
        ..Default::default()
    });
    assert_eq!(
        returned(&mut c, "(clojure.set/join #{} (repeat {:a 1}))"),
        protocol::Value::Set(vec![])
    );
    assert_eq!(
        returned(&mut c, "(clojure.set/join (repeat {:a 1}) #{})"),
        protocol::Value::Set(vec![])
    );
}

#[test]
fn callback_reentry_cannot_restart_walk_depth_budget() {
    let mut c = Context::new();
    let error=c.eval_string("(let [v (reduce (fn [x _] [x]) 1 (range 40))] (letfn [(f [x] (if (number? x) (clojure.walk/postwalk f v) x))] (clojure.walk/postwalk f v)))").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    assert_eq!(returned(&mut c, "(+ 1 2)"), protocol::Value::Int(3));
}

#[test]
fn callback_reentry_cannot_restart_edn_depth_budget() {
    let mut c = Context::new();
    let error=c.eval_string("(let [text (str (apply str (repeat 120 \"[\")) \"1\" (apply str (repeat 120 \"]\")))] (clojure.edn/read-string {:readers {'tag (fn [v] (clojure.edn/read-string text))}} (str (apply str (repeat 40 \"[\")) \"#tag 1\" (apply str (repeat 40 \"]\")))))").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    let error=c.eval_string("(clojure.edn/read-string {:readers {'tag (fn [v] (reduce (fn [x _] [x]) v (range 100)))}} \"#tag 1\")").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    let error=c.eval_string("(let [text (str (apply str (repeat 40 \"[\")) \"#tag 1\" (apply str (repeat 40 \"]\")))] (letfn [(r [x] (clojure.edn/read-string {:readers {'tag r}} text))] (r 1)))").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    assert_eq!(returned(&mut c, "(+ 1 2)"), protocol::Value::Int(3));
}
