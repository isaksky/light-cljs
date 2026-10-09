use light_cljs::interpreter::{
    Arity, CancellationToken, Context, ErrorClass, EvalErrorKind as E, EvalPhase,
    ExceptionCategory, InvokeError, Name, NativeDefinition, NativeEffects, NativeError,
    NativeErrorClass, NativeMetadata, PrintOptions, ResourceLimits, RootedValue, ValueView,
    string_core_definitions, string_definitions,
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
fn strings_match_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| c.id.starts_with("sci-string-")) {
        let mut c = Context::new();
        c.register_native(&OBSERVE);
        TRACE.with(|t| t.borrow_mut().clear());
        let actual = match c.eval_string(case.source) {
            Ok(v) => O::Returned(wire(&v)),
            Err(e) => {
                let phase = match e.phase {
                    EvalPhase::Execution => P::Execution,
                    EvalPhase::Analysis => P::Analysis,
                    _ => panic!("{}: {e}", case.id),
                };
                let category = match e.kind {
                    E::InvalidForm(_) => C::InvalidForm,
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
        assert!(case.stdout.is_empty() && case.stderr.is_empty());
        assert_eq!(
            c.eval_string("(+ 20 22)").unwrap().inspect(),
            ValueView::Integer(42)
        );
        count += 1;
    }
    assert_eq!(count, 131);
}
fn text(v: &RootedValue) -> Vec<u16> {
    let ValueView::String(s) = v.inspect() else {
        panic!("expected string: {v:?}")
    };
    s.to_vec()
}
fn read(c: &Context, source: &str) -> RootedValue {
    let source = c.source("printed.clj", source);
    let mut reader = c.reader(&source).unwrap();
    let v = reader.read_form().unwrap().unwrap().data();
    assert!(reader.read_form().unwrap().is_none());
    v
}
fn call(
    c: &mut Context,
    namespace: &str,
    name: &str,
    args: &[RootedValue],
) -> Result<RootedValue, InvokeError> {
    let function = c
        .resolve_var(&Name::new(Some(namespace), name))
        .unwrap()
        .unwrap();
    c.invoke(&function, args)
}
fn pr(c: &mut Context, v: &RootedValue) -> RootedValue {
    c.print_value(v, Default::default()).unwrap()
}

#[test]
fn readable_data_round_trips_with_exact_types_units_and_metadata() {
    let mut c = Context::new();
    let strings = [
        vec![],
        vec![0],
        (0..=127).collect(),
        vec![0xd800],
        vec![0xdc00],
        vec![0xd800, 0xd800, 0xdc00, 0xdc00],
        "a😀λ\"\\\n".encode_utf16().collect(),
    ];
    for s in strings {
        let value = c.string(s.clone());
        let printed = String::from_utf16(&text(&pr(&mut c, &value))).unwrap();
        let roundtrip = read(&c, &printed);
        assert_eq!(text(&roundtrip), s);
    }
    for source in [
        "nil",
        "true",
        "false",
        "-9223372036854775808",
        "9223372036854775807",
        "1.0",
        "-0.0",
        "1e300",
        "1e-300",
        "4.9406564584124654e-324",
        "-2/3",
        "##Inf",
        "##-Inf",
        "\\u0000",
        "\\newline",
        "\\λ",
        "\\\\",
        "\\\"",
        "alpha",
        "n/x",
        "/",
        ":a",
        ":n/a",
        "[]",
        "()",
        "{}",
        "#{}",
        "[nil true false 1 1.0 2/3 \\tab \"a\" :a x {:z #{[1 2]}} (3 4)]",
    ] {
        let v = read(&c, source);
        let printed = String::from_utf16(&text(&pr(&mut c, &v))).unwrap();
        let result = read(&c, &printed);
        assert!(v.value_eq(&result), "{source} -> {printed}");
        assert_eq!(
            std::mem::discriminant(&v.inspect()),
            std::mem::discriminant(&result.inspect()),
            "{source}"
        );
        if let ValueView::Double(n) = v.inspect() {
            let ValueView::Double(other) = result.inspect() else {
                unreachable!()
            };
            assert_eq!(n.to_bits(), other.to_bits());
        }
    }
    let v = read(&c, "^{:flag true :note [1 2]} [3]");
    let printed = c
        .print_value(
            &v,
            PrintOptions {
                metadata: true,
                ..Default::default()
            },
        )
        .unwrap();
    let roundtrip = read(&c, &String::from_utf16(&text(&printed)).unwrap());
    assert!(v.value_eq(&roundtrip));
    assert!(
        v.metadata()
            .unwrap()
            .value_eq(&roundtrip.metadata().unwrap())
    );
    assert_eq!(v.value_hash(), roundtrip.value_hash());
    assert!(matches!(read(&c, "##NaN").inspect(), ValueView::Double(v) if v.is_nan()));
}

#[test]
fn utf16_operations_preserve_surrogates_and_named_components() {
    let mut c = Context::new();
    let s = c.string(vec![0xd800, 0x61, 0xdc00, 0xd83d, 0xde00, 0]);
    let high = c.string(vec![0xd800]);
    let replacement = c.string(vec![0xdc00]);
    assert_eq!(
        text(
            &call(
                &mut c,
                "clojure.string",
                "replace",
                &[s.clone(), high.clone(), replacement]
            )
            .unwrap()
        ),
        [0xdc00, 0x61, 0xdc00, 0xd83d, 0xde00, 0]
    );
    assert_eq!(
        text(
            &call(
                &mut c,
                "clojure.string",
                "upper-case",
                std::slice::from_ref(&s)
            )
            .unwrap()
        ),
        [0xd800, 0x41, 0xdc00, 0xd83d, 0xde00, 0]
    );
    let zero = c.integer(0);
    let one = c.integer(1);
    assert_eq!(
        text(&call(&mut c, "clojure.core", "subs", &[s.clone(), zero, one]).unwrap()),
        [0xd800]
    );
    let sym = call(
        &mut c,
        "clojure.core",
        "symbol",
        &[high.clone(), high.clone()],
    )
    .unwrap();
    assert_eq!(
        text(&call(&mut c, "clojure.core", "name", std::slice::from_ref(&sym)).unwrap()),
        [0xd800]
    );
    assert_eq!(
        text(
            &call(
                &mut c,
                "clojure.core",
                "namespace",
                std::slice::from_ref(&sym)
            )
            .unwrap()
        ),
        [0xd800]
    );
    let values = c.vector(&[s.clone(), high.clone()]).unwrap();
    assert_eq!(
        text(&call(&mut c, "clojure.string", "join", &[values]).unwrap()),
        [0xd800, 0x61, 0xdc00, 0xd83d, 0xde00, 0, 0xd800]
    );
    let character = c.character(0xd800);
    assert_eq!(
        text(
            &call(
                &mut c,
                "clojure.core",
                "str",
                std::slice::from_ref(&character)
            )
            .unwrap()
        ),
        [0xd800]
    );
    assert_eq!(
        text(&pr(&mut c, &character)),
        "\\ud800".encode_utf16().collect::<Vec<_>>()
    );
    // The existing source grammar (and SCI) rejects surrogate character literals.
    let source = c.source("host-character.clj", "\\ud800");
    assert!(c.reader(&source).unwrap().read_form().is_err());
    // The native profile explicitly uses Rust's context-free, locale-independent casing.
    assert_eq!(
        text(&c.eval_string("(clojure.string/lower-case \"ΟΣ\")").unwrap()),
        "οσ".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        text(&c.eval_string("(clojure.string/upper-case \"i\")").unwrap()),
        vec![0x49]
    );
}

#[test]
fn identity_cycles_and_all_supported_identity_types_print_without_forcing_delays() {
    let mut c = Context::new();
    let cycle = c
        .eval_string("(let [a (atom nil)] (reset! a a) a)")
        .unwrap();
    assert_eq!(
        String::from_utf16(&text(&pr(&mut c, &cycle))).unwrap(),
        "#<Atom #<cycle>>"
    );
    let mutual = c
        .eval_string("(let [a (atom nil) v (volatile! a)] (reset! a [v]) a)")
        .unwrap();
    assert_eq!(
        String::from_utf16(&text(&pr(&mut c, &mutual))).unwrap(),
        "#<Atom [#<Volatile #<cycle>>]>"
    );
    let delayed = c
        .eval_string("(delay (throw (ex-info \"must not run\" {})))")
        .unwrap();
    assert_eq!(
        String::from_utf16(&text(&pr(&mut c, &delayed))).unwrap(),
        "#<Delay Unrealized>"
    );
    for source in [
        "(fn [x] x)",
        "+",
        "#'map",
        "*ns*",
        "(do (declare x) x)",
        "(reduced [1])",
        "(ex-info \"x\" {:data 1})",
    ] {
        let value = c.eval_string(source).unwrap();
        assert!(!text(&pr(&mut c, &value)).is_empty());
    }
    let original = read(&c, "[1 2]");
    let shared = c.vector(&[original.clone(), original]).unwrap();
    assert_eq!(
        String::from_utf16(&text(&pr(&mut c, &shared))).unwrap(),
        "[[1 2] [1 2]]"
    );
    c.collect_garbage();
    let detached = pr(&mut c, &cycle);
    c.reset();
    assert_eq!(
        String::from_utf16(&text(&detached)).unwrap(),
        "#<Atom #<cycle>>"
    );
    assert!(matches!(
        c.print_value(&cycle, Default::default()),
        Err(NativeError::WrongOwner)
    ));
}

#[test]
fn printing_and_strings_observe_shared_fuel_allocation_output_depth_and_cancellation() {
    for (source, limits) in [
        (
            "(pr-str (range))",
            ResourceLimits {
                fuel: Some(1500),
                ..Default::default()
            },
        ),
        (
            "(str (range))",
            ResourceLimits {
                output: Some(100),
                ..Default::default()
            },
        ),
        (
            "(clojure.string/join (repeat \"x\"))",
            ResourceLimits {
                fuel: Some(2000),
                ..Default::default()
            },
        ),
        (
            "(clojure.string/replace \"abcdefgh\" \"\" \"xxxxxxxxxxxxxxxx\")",
            ResourceLimits {
                allocation: Some(300),
                ..Default::default()
            },
        ),
        (
            "(pr-str [1 2 3])",
            ResourceLimits {
                output: Some(4),
                ..Default::default()
            },
        ),
    ] {
        let mut c = Context::new();
        c.set_resource_limits(limits);
        assert_eq!(
            c.eval_string(source).unwrap_err().class(),
            ErrorClass::Control,
            "{source}"
        );
        c.set_resource_limits(Default::default());
        assert_eq!(
            c.eval_string("(+ 1 2)").unwrap().inspect(),
            ValueView::Integer(3)
        );
    }
    let mut c = Context::new();
    let infinite = c.eval_string("(range)").unwrap();
    assert_eq!(
        String::from_utf16(&text(
            &c.print_value(
                &infinite,
                PrintOptions {
                    length: Some(3),
                    ..Default::default()
                }
            )
            .unwrap()
        ))
        .unwrap(),
        "(0 1 2 ...)"
    );
    let mut nested = c.integer(1);
    for _ in 0..140 {
        nested = c.vector(&[nested]).unwrap();
    }
    assert_eq!(
        c.print_value(&nested, Default::default())
            .unwrap_err()
            .class(),
        NativeErrorClass::Invocation
    );
    assert!(
        c.print_value(
            &nested,
            PrintOptions {
                level: Some(2),
                ..Default::default()
            }
        )
        .is_ok()
    );
    let token = CancellationToken::default();
    token.cancel();
    c.set_cancellation_token(Some(token));
    assert!(c.print_value(&infinite, Default::default()).is_err());
    c.set_cancellation_token(None);
    // Allocation exhaustion while lazily materializing printer Vars must return an error.
    for budget in 0..100 {
        let mut tiny = Context::new();
        tiny.set_resource_limits(ResourceLimits {
            allocation: Some(budget),
            ..Default::default()
        });
        assert!(tiny.eval_string("(pr-str 1)").is_err());
    }
}

#[test]
fn every_registered_string_operation_checks_arity_and_types_without_effects() {
    let mut c = Context::new();
    let flags = c.eval_string("(mapv #(get (meta (get (ns-publics 'clojure.core) %)) :dynamic) '[*print-length* *print-level* *print-meta* *print-readably*])").unwrap();
    assert!(
        flags
            .elements()
            .unwrap()
            .iter()
            .all(|v| v.inspect() == ValueView::Boolean(true))
    );

    for definition in string_core_definitions().iter().chain(string_definitions()) {
        if !definition.metadata.arity.accepts(0) {
            assert!(
                matches!(
                    c.invoke_native(definition, &[]),
                    Err(NativeError::Arity { actual: 0, .. })
                ),
                "{}",
                definition.metadata.name
            );
        }
        let args = vec![c.nil(); 4];
        assert!(
            matches!(
                c.invoke_native(definition, &args),
                Err(NativeError::Arity { actual: 4, .. })
            ),
            "{}",
            definition.metadata.name
        );
    }
    for (name, args, argument) in [
        ("split-lines", vec![c.integer(1)], 0),
        ("trim", vec![c.integer(1)], 0),
        ("triml", vec![c.nil()], 0),
        ("trimr", vec![c.nil()], 0),
        ("trim-newline", vec![c.nil()], 0),
        ("blank?", vec![c.integer(1)], 0),
        ("reverse", vec![c.nil()], 0),
        ("starts-with?", vec![c.string(vec![]), c.integer(1)], 1),
        ("ends-with?", vec![c.nil(), c.string(vec![])], 0),
        ("includes?", vec![c.nil(), c.nil()], 0),
        (
            "index-of",
            vec![c.string(vec![]), c.string(vec![]), c.nil()],
            2,
        ),
        ("last-index-of", vec![c.string(vec![]), c.integer(1)], 1),
        ("replace", vec![c.string(vec![]), c.nil(), c.nil()], 1),
        (
            "replace-first",
            vec![c.string(vec![]), c.string(vec![]), c.nil()],
            2,
        ),
        ("upper-case", vec![c.nil()], 0),
        ("lower-case", vec![c.nil()], 0),
        ("capitalize", vec![c.nil()], 0),
        ("escape", vec![c.integer(1), c.nil()], 0),
    ] {
        let definition = string_definitions()
            .iter()
            .find(|d| d.metadata.name == name)
            .unwrap();
        assert!(
            matches!(c.invoke_native(definition, &args), Err(NativeError::Type { argument: i, .. }) if i == argument),
            "{name}"
        );
    }
    assert_eq!(
        c.eval_string("(try (subs \"abc\" -1) (catch Exception _ :bounds))")
            .unwrap()
            .inspect(),
        ValueView::Keyword(Name::new(None, "bounds"))
    );
    assert_eq!(c.eval_string("(require '[clojure.string :refer [join] :rename {join joined}] :reload) (joined \"-\" [1 2])").unwrap().inspect(), ValueView::String("1-2".encode_utf16().collect()));
    assert!(
        c.eval_string("(with-redefs [clojure.string/join identity] 1)")
            .is_err()
    );
    assert!(
        c.eval_string("(clojure.string/replace \"x\" #\"x\" \"y\")")
            .is_err()
    );
}

#[test]
fn hidden_lazy_values_are_not_forced_and_internal_metadata_depth_is_bounded() {
    let mut c = Context::new();
    assert_eq!(
        c.eval_string("(clojure.string/join (lazy-seq (throw (ex-info \"unused\" {}))) [])")
            .unwrap()
            .inspect(),
        ValueView::String(Vec::<u16>::new().into())
    );
    let lazy = c
        .eval_string("(lazy-seq (throw (ex-info \"hidden\" {})))")
        .unwrap();
    assert_eq!(
        String::from_utf16(&text(
            &c.print_value(
                &lazy,
                PrintOptions {
                    level: Some(0),
                    ..Default::default()
                }
            )
            .unwrap()
        ))
        .unwrap(),
        "#"
    );
    let mut metadata = c
        .map(&[(c.keyword(Name::new(None, "x")), c.integer(1))])
        .unwrap();
    for _ in 0..140 {
        let next = c
            .map(&[(c.keyword(Name::new(None, "x")), c.integer(1))])
            .unwrap();
        metadata = c.with_meta(&next, &metadata).unwrap();
    }
    assert!(
        c.print_value(
            &metadata,
            PrintOptions {
                metadata: true,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(
        c.eval_string("(in-ns 'clojure.string) (+ 1 2)")
            .unwrap()
            .inspect(),
        ValueView::Integer(3)
    );
}
