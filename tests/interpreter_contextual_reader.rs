use light_cljs::interpreter::*;
use light_cljs::parser::ErrorKind;
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
static DATA: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "data",
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
static STR: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "str",
        arity: Arity::Exact(1),
        effects: NativeEffects::Pure,
        errors: &[],
    },
    callback: |context, args| {
        let ValueView::Integer(n) = args[0].inspect() else {
            return Err(NativeError::Type {
                argument: 0,
                expected: "integer test input",
            });
        };
        Ok(context.string(n.to_string().encode_utf16().collect::<Vec<_>>()))
    },
};
static FALLBACK: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "fallback",
        arity: Arity::Exact(2),
        effects: NativeEffects::Pure,
        errors: &[],
    },
    callback: |context, args| Ok(context.vector(args)?),
};
fn setup(profile: &str) -> Context {
    TRACE.with(|trace| trace.borrow_mut().clear());
    let mut c = Context::new();
    if profile == "reader-bb" {
        c.set_reader_features([Name::new(None, "bb")]);
    }
    if profile == "reader-tags" {
        c.register_data_reader(
            Name::new(Some("t"), "tag"),
            builtin_definitions()
                .iter()
                .find(|d| d.metadata.name == "inc")
                .unwrap(),
        );
        c.register_data_reader(Name::new(Some("x"), "str"), &STR);
        c.register_data_reader(Name::new(Some("t"), "data"), &DATA);
    }
    if profile == "reader-fallback" {
        c.set_default_data_reader(Some(&FALLBACK));
    }
    c
}
#[test]
fn contextual_reading_matches_focused_pinned_sci_values_errors_and_effects() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|case| {
        case.id.starts_with("sci-context-reader-") && case.profile != "reader-difference"
    }) {
        let mut c = setup(case.profile);
        let result = c.eval_string(case.source);
        let outcome = match result {
            Ok(value) => protocol::Outcome::Returned(wire(&value)),
            Err(error) => {
                assert_eq!(error.phase, EvalPhase::Read, "{}: {error}", case.id);
                protocol::Outcome::Error {
                    phase: protocol::Phase::Read,
                    category: protocol::Category::Reader,
                }
            }
        };
        assert_eq!(outcome, case.outcome, "{}: {}", case.id, case.source);
        TRACE.with(|trace| assert_eq!(*trace.borrow(), case.effects, "{} effects", case.id));
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        count += 1;
    }
    assert_eq!(count, 76);
}

fn read(c: &Context, text: &str) -> Form {
    let source = c.source("contextual.cljc", text);
    c.reader(&source).unwrap().read_form().unwrap().unwrap()
}

#[test]
fn existing_parser_restrictions_and_read_eval_stay_explicit() {
    for (source, kind) in [
        ("#?@(:clj [1 2])", ErrorKind::InvalidConditional),
        ("^#?(:clj :private) x", ErrorKind::InvalidMetadata),
        ("#?(:clj 42 :cljs {:a})", ErrorKind::OddMap),
        ("#?(:clj 42 :cljs #=(+ 1 2))", ErrorKind::ReadEvalDisabled),
        ("#_#=(+ 1 2)", ErrorKind::ReadEvalDisabled),
    ] {
        let c = Context::new();
        let source = c.source("differences.cljc", source);
        let mut reader = c.reader(&source).unwrap();
        assert_eq!(
            reader.read_form().unwrap_err().kind,
            ReadErrorKind::Syntax(kind)
        );
        assert!(reader.read_form().unwrap().is_none());
    }
}

#[test]
fn features_aliases_and_registry_updates_are_live_for_existing_readers() {
    let mut c = Context::new();
    let source = c.source(
        "live.cljc",
        "#?(:clj 1 :bb 2) #?(:clj 3 :bb 4) #?(:clj 5) #?(:default 6) ::a/x #::a{:x 7} #t/data 8",
    );
    let mut reader = c.reader(&source).unwrap();
    assert_eq!(
        reader.read_form().unwrap().unwrap().data().inspect(),
        ValueView::Integer(1)
    );
    c.set_reader_features([Name::new(None, "bb")]);
    assert_eq!(
        reader.read_form().unwrap().unwrap().data().inspect(),
        ValueView::Integer(4)
    );
    c.set_reader_features([]);
    assert_eq!(
        reader.read_form().unwrap().unwrap().data().inspect(),
        ValueView::Integer(6)
    );
    let target = c
        .create_namespace(Name::new(None, "target"))
        .unwrap()
        .namespace_id()
        .unwrap();
    c.alias_namespace(
        c.current_namespace().namespace_id().unwrap(),
        Name::new(None, "a"),
        target,
    )
    .unwrap();
    assert_eq!(
        reader.read_form().unwrap().unwrap().data().inspect(),
        ValueView::Keyword(Name::new(Some("target"), "x"))
    );
    let map = reader.read_form().unwrap().unwrap();
    assert_eq!(
        map.children()[0].data().inspect(),
        ValueView::Keyword(Name::new(Some("target"), "x"))
    );
    c.register_data_reader(Name::new(Some("t"), "data"), &DATA);
    assert_eq!(
        reader.read_form().unwrap().unwrap().data().inspect(),
        ValueView::Integer(8)
    );
    assert!(reader.read_form().unwrap().is_none());
    c.remove_data_reader(&Name::new(Some("t"), "data"));
    assert!(matches!(
        c.eval_string("#t/data 1").unwrap_err().kind,
        EvalErrorKind::Read(ReadErrorKind::Lower(LowerErrorKind::UnknownTag(_)))
    ));
    c.set_default_data_reader(Some(&FALLBACK));
    assert_eq!(read(&c, "#t/data 1").data().inspect(), ValueView::Vector(2));
    c.set_default_data_reader(None);
    assert!(c.eval_string("#t/data 1").is_err());
}

#[test]
fn selected_forms_retain_exact_byte_spans_and_tag_generated_origin() {
    let mut c = setup("reader-tags");
    let form = read(&c, "[:λ #?@(:clj [1 2]) #:n{:x 3} #t/data [4 5]]");
    let parts = form.children();
    assert_eq!(
        parts
            .iter()
            .map(|f| f.span().text().to_owned())
            .collect::<Vec<_>>(),
        [":λ", "1", "2", "#:n{:x 3}", "#t/data [4 5]"]
    );
    assert_eq!(parts[3].children()[0].span().text(), ":x");
    assert_eq!(
        parts[4].node().origin,
        Origin::DataReader(std::rc::Rc::new(Name::new(Some("t"), "data")))
    );
    let data = form.data();
    c.reset();
    drop(c);
    assert_eq!(data.inspect(), ValueView::Vector(5));
    assert_eq!(parts[4].children()[0].span().text(), "#t/data [4 5]");
    let c = Context::new();
    let source = c.source("duplicate.cljc", "{:a 1 #?@(:clj [:a 2])}");
    let error = c.reader(&source).unwrap().read_form().unwrap_err();
    assert_eq!(error.span.text(), ":a");
    assert_eq!(error.related.unwrap().bytes(), 1..3);
}

static FAIL: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "fail",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[NativeErrorClass::Host],
    },
    callback: |_, _| Err(NativeError::Host("reader failed".into())),
};
static FOREIGN: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "foreign",
        arity: Arity::Exact(1),
        effects: NativeEffects::Host,
        errors: &[],
    },
    callback: |_, _| Ok(Context::new().integer(1)),
};
static RESET: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "reset",
        arity: Arity::Exact(1),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, _| {
        c.reset();
        Ok(c.integer(1))
    },
};
static REENTER: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "reenter",
        arity: Arity::Exact(1),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, _| {
        let source = c.source(
            "reader-body.clj",
            "(throw (ex-info \"reader body failed\" {:k 1}))",
        );
        c.eval_source(&source)
            .map_err(|e| InvokeError::Execution(Box::new(e)).into())
    },
};
static RECURSIVE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "recursive",
        arity: Arity::Exact(1),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, _| {
        c.eval_string("#recursive 0")
            .map_err(|e| InvokeError::Execution(Box::new(e)).into())
    },
};
static FUEL: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "fuel",
        arity: Arity::Exact(1),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, _| {
        c.eval_string("(loop [] (recur))")
            .map_err(|e| InvokeError::Execution(Box::new(e)).into())
    },
};
static ALLOCATE: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "host",
        name: "allocate",
        arity: Arity::Exact(1),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, _| {
        c.string(vec![97; 1000]);
        Ok(c.nil())
    },
};

#[test]
fn tag_callback_failures_keep_phase_origin_body_frames_and_owner_checks() {
    for (definition, expected) in [
        (&FAIL, NativeError::Host("reader failed".into())),
        (
            &FALLBACK,
            NativeError::Arity {
                expected: Arity::Exact(2),
                actual: 1,
            },
        ),
        (&FOREIGN, NativeError::WrongOwner),
        (&RESET, NativeError::WrongOwner),
    ] {
        let mut c = Context::new();
        c.register_data_reader(Name::new(None, "bad"), definition);
        let source = c.source("tag-errors.cljc", "[:λ #bad 1]");
        let mut reader = c.reader(&source).unwrap();
        let error = reader.read_form().unwrap_err();
        assert_eq!(
            error.kind,
            ReadErrorKind::Lower(LowerErrorKind::DataReader {
                tag: Box::new(Name::new(None, "bad")),
                error: Box::new(expected)
            })
        );
        assert_eq!(error.span.text(), "#bad 1");
        assert!(reader.read_form().unwrap().is_none());
        let error: EvalError = error.into();
        assert_eq!(error.phase, EvalPhase::Read);
        assert_eq!(
            error.origin,
            Origin::DataReader(std::rc::Rc::new(Name::new(None, "bad")))
        );
        assert_eq!(
            c.eval_string("42").unwrap().inspect(),
            ValueView::Integer(42)
        );
    }
    let mut c = Context::new();
    c.register_data_reader(Name::new(None, "body"), &REENTER);
    let error = c.eval_string("#body 0").unwrap_err();
    assert_eq!(error.phase, EvalPhase::Read);
    assert_eq!(error.span.text(), "#body 0");
    assert!(error.exception.is_some());
    assert!(
        error
            .frames
            .iter()
            .any(|frame| frame.span.source().name() == "reader-body.clj")
    );
    c.reset();
    drop(c);
    assert_eq!(error.span.text(), "#body 0");
    assert!(
        error
            .frames
            .iter()
            .any(|frame| frame.span.text().contains("throw"))
    );
}

#[test]
fn tag_callbacks_share_fuel_allocation_and_depth_scopes() {
    for (tag, handler, limits, expected) in [
        (
            "recursive",
            &RECURSIVE,
            ResourceLimits {
                call_depth: 4,
                ..ResourceLimits::default()
            },
            ResourceError::CallDepthExceeded,
        ),
        (
            "fuel",
            &FUEL,
            ResourceLimits {
                fuel: Some(120),
                ..ResourceLimits::default()
            },
            ResourceError::LimitExceeded(ResourceLimit::Fuel),
        ),
        (
            "allocate",
            &ALLOCATE,
            ResourceLimits {
                allocation: Some(200),
                ..ResourceLimits::default()
            },
            ResourceError::LimitExceeded(ResourceLimit::Allocation),
        ),
    ] {
        let mut c = Context::new();
        c.register_data_reader(Name::new(None, tag), handler);
        c.set_resource_limits(limits);
        let source = format!("#{tag} 0");
        let error = c.eval_string(&source).unwrap_err();
        assert_eq!(error.class(), ErrorClass::Control, "{error}");
        assert_eq!(error.phase, EvalPhase::Read);
        assert_eq!(error.span.text(), source);
        let actual = match error.kind {
            EvalErrorKind::Resource(e) => e,
            EvalErrorKind::CallDepthExceeded => ResourceError::CallDepthExceeded,
            other => panic!("unexpected resource error: {other:?}"),
        };
        assert_eq!(actual, expected);
        assert_eq!(
            c.eval_string("42").unwrap().inspect(),
            ValueView::Integer(42)
        );
    }
}

#[test]
fn inst_uuid_and_constructor_tags_require_explicit_host_implementations() {
    let mut c = Context::new();
    for source in [
        "#inst \"2020-01-01\"",
        "#uuid \"550e8400-e29b-41d4-a716-446655440000\"",
        "#java.lang.String \"hello\"",
    ] {
        let error = c.eval_string(source).unwrap_err();
        assert!(matches!(
            error.kind,
            EvalErrorKind::Read(ReadErrorKind::Lower(LowerErrorKind::UnknownTag(_)))
        ));
        assert_eq!(error.span.text(), source);
    }
    c.set_default_data_reader(Some(&FALLBACK));
    c.register_data_reader(
        Name::new(None, "known"),
        builtin_definitions()
            .iter()
            .find(|d| d.metadata.name == "inc")
            .unwrap(),
    );
    assert_eq!(
        c.eval_string("#known 1").unwrap().inspect(),
        ValueView::Integer(2)
    );
    assert_eq!(
        read(&c, "#unknown 1").data().inspect(),
        ValueView::Vector(2)
    );
}

#[test]
fn syntax_validation_precedes_all_callbacks_in_the_containing_top_level_form() {
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    for case in cases
        .iter()
        .filter(|case| case.id.starts_with("sci-context-reader-tag-syntax-error-"))
    {
        assert_eq!(case.effects, [protocol::Value::Int(1)]);
        let mut c = setup("reader-tags");
        assert_eq!(
            c.eval_string(case.source).unwrap_err().phase,
            EvalPhase::Read
        );
        TRACE.with(|trace| assert!(trace.borrow().is_empty()));
    }
}

#[test]
fn lowering_retained_syntax_uses_context_and_keeps_terminal_callback_provenance() {
    let mut c = Context::new();
    let source = c.source("retained.cljc", "#::{:a #?(:clj 1)}");
    let syntax = light_cljs::parse(source.text()).unwrap();
    let form = c.lower_form(&source, &syntax, syntax.roots[0]).unwrap();
    assert_eq!(
        form.children()[0].data().inspect(),
        ValueView::Keyword(Name::new(Some("user"), "a"))
    );
    c.register_data_reader(Name::new(None, "fuel"), &FUEL);
    c.set_resource_limits(ResourceLimits {
        // Include setup of the source dynamic Var before reaching recur.
        fuel: Some(5000),
        ..ResourceLimits::default()
    });
    let source = c.source("retained-tag.cljc", "#fuel 0");
    let syntax = light_cljs::parse(source.text()).unwrap();
    let error: EvalError = c
        .lower_form(&source, &syntax, syntax.roots[0])
        .unwrap_err()
        .into();
    assert_eq!(error.class(), ErrorClass::Control);
    assert_eq!(error.span.text(), "#fuel 0");
    assert_eq!(
        error.origin,
        Origin::DataReader(std::rc::Rc::new(Name::new(None, "fuel")))
    );
    assert!(
        error
            .frames
            .iter()
            .any(|frame| frame.span.text().contains("(recur)")),
        "{error:?}"
    );
}
