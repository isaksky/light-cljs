use light_cljs::interpreter::EvalErrorKind as E;
use light_cljs::interpreter::*;
use std::{
    cell::{Cell, RefCell},
    io::{self, Cursor},
    rc::Rc,
};
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
fn io_matches_pinned_values_errors_and_effects() {
    use protocol::{Category as C, Outcome as O, Phase as P};
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let mut count = 0;
    for case in cases.iter().filter(|c| {
        c.id.starts_with("sci-io-")
            || c.id == "sci-reader-eof"
            || ["sci-stdout", "sci-top-level-error", "sci-execution-error"].contains(&c.id)
    }) {
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
    assert!(count >= 40);
}

fn text(value: &RootedValue) -> Vec<u16> {
    match value.inspect() {
        ValueView::String(text) => text.to_vec(),
        other => panic!("{other:?}"),
    }
}
fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[test]
fn defaults_have_no_ambient_capabilities_and_streams_are_owner_checked() {
    let mut c = Context::new();
    assert_eq!(
        c.eval_string("[*in* *out* *err*]")
            .unwrap()
            .elements()
            .unwrap()
            .iter()
            .map(RootedValue::inspect)
            .collect::<Vec<_>>(),
        vec![ValueView::Nil; 3]
    );
    for source in [
        "(println 1)",
        "(read-line)",
        "(read)",
        "(flush)",
        "(slurp \"x\")",
        "(spit \"x\" \"y\")",
        "(clojure.java.shell/sh \"x\")",
    ] {
        assert!(c.eval_string(source).is_err(), "{source}");
    }
    let foreign = Context::new().output_stream(OutputBuffer::default());
    assert!(c.bind_io(None, Some(&foreign), None).is_err());
    assert_eq!(
        text(&c.eval_string("(with-out-str (println 42))").unwrap()),
        utf16("42\n")
    );
}

#[test]
fn channels_flush_and_bindings_restore_without_duplicate_writes() {
    struct Writer {
        writes: Rc<RefCell<Vec<Vec<u16>>>>,
        flushes: Rc<Cell<usize>>,
    }
    impl HostOutput for Writer {
        fn write(&mut self, text: &[u16]) -> io::Result<()> {
            self.writes.borrow_mut().push(text.to_vec());
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushes.set(self.flushes.get() + 1);
            Ok(())
        }
    }
    let mut c = Context::new();
    let writes = Rc::new(RefCell::new(Vec::new()));
    let flushes = Rc::new(Cell::new(0));
    let out = c.output_stream(Writer {
        writes: writes.clone(),
        flushes: flushes.clone(),
    });
    let err_capture = OutputBuffer::default();
    let err = c.output_stream(err_capture.clone());
    let guard = c.bind_io(None, Some(&out), Some(&err)).unwrap();
    c.eval_string("(print 1 2) (prn :three) (newline) (binding [*flush-on-newline* false] (println 4)) (binding [*out* *err*] (print :error)) (flush)").unwrap();
    assert_eq!(
        *writes.borrow(),
        [utf16("1 2"), utf16(":three\n"), utf16("\n"), utf16("4\n")]
    );
    assert_eq!(flushes.get(), 2);
    assert_eq!(err_capture.units(), utf16(":error"));
    c.collect_garbage();
    c.eval_string("(print :alive)").unwrap();
    drop(guard);
    assert_eq!(c.eval_string("*out*").unwrap().inspect(), ValueView::Nil);
}

#[test]
fn host_failures_are_propagated_once_and_not_catchable_as_language_errors() {
    let mut c = Context::new();
    let writes = Rc::new(Cell::new(0));
    let count = writes.clone();
    let out = c.output_stream(move |_: &[u16]| {
        count.set(count.get() + 1);
        Err(io::Error::other("sink failed after partial output"))
    });
    let _guard = c.bind_io(None, Some(&out), None).unwrap();
    let error = c
        .eval_string("(try (print :once) (catch Exception _ :hidden))")
        .unwrap_err();
    assert_eq!(error.class(), ErrorClass::Host);
    assert!(error.to_string().contains("sink failed"));
    assert_eq!(writes.get(), 1);
    let input = c.input_stream("broken", || Err(io::Error::other("input failed")));
    let _input = c.bind_io(Some(&input), None, None).unwrap();
    let error = c.eval_string("(read)").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Host);
    assert!(error.to_string().contains("input failed"));
}

#[test]
fn owned_reader_distinguishes_nil_eof_and_captures_exact_original_bytes() {
    let mut c = Context::new();
    let source = c.source("owned.clj", "; λ\n#_ :ignored nil  ::x [1\n2]");
    let mut reader = c.source_reader(source.clone()).unwrap();
    let (nil, captured) = reader.read_form_with_source(&c).unwrap().unwrap();
    assert_eq!(nil.data().inspect(), ValueView::Nil);
    assert_eq!(captured.text(), "; λ\n#_ :ignored nil");
    c.eval_string("(ns changed)").unwrap();
    let keyword = reader.read_form(&c).unwrap().unwrap();
    assert_eq!(
        keyword.data().inspect(),
        ValueView::Keyword(Name::new(Some("changed"), "x"))
    );
    let form = reader.read_form(&c).unwrap().unwrap();
    assert_eq!(form.span().text(), "[1\n2]");
    assert_eq!(
        form.span().source().line_column(form.span().bytes().start),
        Some((2, 22))
    );
    assert!(reader.read_form(&c).unwrap().is_none());
    assert!(reader.read_form(&c).unwrap().is_none());
    c.reset();
    assert_eq!(nil.span().text(), "nil");
    assert_eq!(captured.source().name(), "owned.clj");
    assert!(reader.read_form(&c).is_err());
    assert_eq!(form.data().elements().unwrap().len(), 2);
}

#[test]
fn owned_reader_errors_fuse_and_preserve_input_and_provenance() {
    let c = Context::new();
    for source in ["1 [", "1 ]", "1 #=2"] {
        let mut reader = c.source_reader(c.source("bad.clj", source)).unwrap();
        assert_eq!(
            reader.read_form(&c).unwrap().unwrap().data().inspect(),
            ValueView::Integer(1)
        );
        let error = reader.read_form(&c).unwrap_err();
        assert_eq!(error.span.source().name(), "bad.clj");
        assert!(reader.read_form(&c).unwrap().is_none());
    }
}

#[test]
fn streamed_input_completes_multiline_forms_without_replaying_tag_handlers() {
    static TAG: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "tag",
            arity: Arity::Exact(1),
            effects: NativeEffects::Host,
            errors: &[],
        },
        callback: |c, args| c.invoke_native(&OBSERVE, args),
    };
    let mut c = Context::new();
    c.register_data_reader(Name::new(None, "tag"), &TAG);
    let input = c.input_stream(
        "stream.clj",
        Utf8Input(Cursor::new(
            b"#tag [1\n2]\nnil\n#?(:cljs :skip)\n42".to_vec(),
        )),
    );
    let _guard = c.bind_io(Some(&input), None, None).unwrap();
    TRACE.with(|t| t.borrow_mut().clear());
    let result = c
        .eval_string("[(read) (read-line) (read) (read) (read {:eof :end} *in*)]")
        .unwrap();
    assert_eq!(result.elements().unwrap()[0].elements().unwrap().len(), 2);
    assert_eq!(text(&result.elements().unwrap()[1]), utf16(""));
    assert_eq!(result.elements().unwrap()[2].inspect(), ValueView::Nil);
    assert_eq!(
        result.elements().unwrap()[3].inspect(),
        ValueView::Integer(42)
    );
    TRACE.with(|t| assert_eq!(t.borrow().len(), 1));
}

#[test]
fn io_readers_use_context_features_tags_and_never_enable_read_eval() {
    let mut c = Context::new();
    c.set_reader_features([Name::new(None, "cljs")]);
    assert_eq!(
        c.eval_string("(read-string \"#?(:clj 1 :cljs 2)\")")
            .unwrap()
            .inspect(),
        ValueView::Integer(2)
    );
    for source in [
        "(read-string \"#unknown 1\")",
        "(read-string \"#=(+ 1 2)\")",
        "(read-string {:read-eval true} \"#=1\")",
        "(read-string {:features #{:clj}} \"1\")",
    ] {
        assert!(c.eval_string(source).is_err());
    }
    let error = c.eval_string("(read-string \"[1\")").unwrap_err();
    assert_eq!(error.phase, EvalPhase::Read);
    assert_eq!(error.span.source().name(), "<read-string>");
}

#[test]
fn output_limits_apply_before_effects_and_capture_is_not_charged_twice() {
    let mut c = Context::new();
    let capture = OutputBuffer::default();
    let out = c.output_stream(capture.clone());
    let _guard = c.bind_io(None, Some(&out), None).unwrap();
    c.set_resource_limits(ResourceLimits {
        output: Some(4),
        ..Default::default()
    });
    assert_eq!(
        text(&c.eval_string("(with-out-str (print \"ab\"))").unwrap()),
        utf16("ab")
    );
    assert_eq!(c.resource_usage().output, 4);
    let error = c.eval_string("(print \"abc\")").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    assert!(capture.units().is_empty());
    c.eval_string("(print \"ok\")").unwrap();
    assert_eq!(capture.units(), utf16("ok"));
}

#[test]
fn cancellation_inside_a_callback_restores_string_scope_and_preserves_effects_once() {
    let mut c = Context::new();
    let token = CancellationToken::default();
    let capture = OutputBuffer::default();
    let out = c.output_stream(capture.clone());
    let _guard = c.bind_io(None, Some(&out), None).unwrap();
    let cancelled = token.clone();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let input = c.input_stream("cancel", move || {
        count.set(count.get() + 1);
        cancelled.cancel();
        Ok(Some(utf16("42\n")))
    });
    let _input = c.bind_io(Some(&input), None, None).unwrap();
    c.set_cancellation_token(Some(token));
    let error = c
        .eval_string("(with-out-str (print :hidden) (read))")
        .unwrap_err();
    assert_eq!(error.class(), ErrorClass::Control);
    assert_eq!(calls.get(), 1);
    c.set_cancellation_token(None);
    c.eval_string("(print :restored)").unwrap();
    assert_eq!(capture.units(), utf16(":restored"));
}

#[test]
fn utf8_adapters_validate_without_replacing_surrogates() {
    let mut c = Context::new();
    let out = c.output_stream(Utf8Output(Vec::<u8>::new()));
    let _guard = c.bind_io(None, Some(&out), None).unwrap();
    let error = c.eval_string("(print \"\\uD800\")").unwrap_err();
    assert_eq!(error.class(), ErrorClass::Host);
    let input = c.input_stream("invalid utf8", Utf8Input(Cursor::new(vec![0xff])));
    let _input = c.bind_io(Some(&input), None, None).unwrap();
    assert_eq!(
        c.eval_string("(read-line)").unwrap_err().class(),
        ErrorClass::Host
    );
    let input = c.string_input("unpaired", vec![0xd800]);
    let _input = c.bind_io(Some(&input), None, None).unwrap();
    assert_eq!(
        c.eval_string("(read)").unwrap_err().class(),
        ErrorClass::Host
    );
}

#[test]
fn streaming_returns_completed_forms_before_fetching_later_lines() {
    for source in [
        "1 [\n",
        "#?(:clj 1) [\n",
        "[#?(:clj 1)] [\n",
        "'1 [\n",
        "' #?(:cljs :gone) 1 [\n",
        "^:x [] [\n",
    ] {
        let mut c = Context::new();
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        let first = utf16(source);
        let input = c.input_stream("incremental", move || {
            count.set(count.get() + 1);
            if count.get() == 1 {
                Ok(Some(first.clone()))
            } else {
                Err(io::Error::other("must not fetch a later form"))
            }
        });
        let _guard = c.bind_io(Some(&input), None, None).unwrap();
        c.eval_string("(read)")
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        assert_eq!(calls.get(), 1, "{source}");
    }
}

#[test]
fn streaming_semantic_prefixes_and_discards_complete_without_repeating_effects() {
    static TAG: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "host",
            name: "tag",
            arity: Arity::Exact(1),
            effects: NativeEffects::Host,
            errors: &[],
        },
        callback: |c, args| c.invoke_native(&OBSERVE, args),
    };
    for source in [
        "#_ #tag 0 ' #?(:cljs :skip)\n[1\n2]\n",
        "#_ #?(:cljs :skip) 42\n[1\n2]\n",
        "#tag #?(:cljs :skip)\n[1\n2]\n",
        "^:x #?(:cljs :skip)\n[1\n2]\n",
        "#_ #tag 0 #?(:cljs :skip)\n[1\n2]\n",
        "#_ #?(:cljs :skip)\n42\n99",
        "#_ ' #?(:cljs :skip)\n42\n99",
        "#_ #tag #?(:cljs :skip)\n42\n99",
    ] {
        let mut c = Context::new();
        c.register_data_reader(Name::new(None, "tag"), &TAG);
        TRACE.with(|t| t.borrow_mut().clear());
        let input = c.input_stream(
            "streamed",
            Utf8Input(Cursor::new(source.as_bytes().to_vec())),
        );
        let _guard = c.bind_io(Some(&input), None, None).unwrap();
        let actual = c
            .eval_string("(read)")
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        let count = TRACE.with(|t| t.borrow().len());
        TRACE.with(|t| t.borrow_mut().clear());
        let src = c.source("finite", source);
        let expected = c.reader(&src).unwrap().read_form().unwrap().unwrap().data();
        assert!(
            actual.value_eq(&expected),
            "{source}: {actual:?} != {expected:?}"
        );
        TRACE.with(|t| assert_eq!(count, t.borrow().len(), "{source}"));
    }
}

#[test]
fn stream_creation_and_input_buffering_share_allocation_limits() {
    static ALLOCATE: NativeDefinition = NativeDefinition {
        metadata: NativeMetadata {
            namespace: "user",
            name: "make-streams",
            arity: Arity::Exact(0),
            effects: NativeEffects::Host,
            errors: &[],
        },
        callback: |c, _| {
            for _ in 0..100 {
                c.output_stream(OutputBuffer::default());
            }
            Ok(c.nil())
        },
    };
    let mut c = Context::new();
    c.set_resource_limits(ResourceLimits {
        allocation: Some(10),
        ..Default::default()
    });
    let error = c.invoke_native(&ALLOCATE, &[]).unwrap_err();
    assert!(matches!(error, NativeError::Invocation(_)));
    c.set_resource_limits(ResourceLimits::default());
    let input = c.input_stream("large", || Ok(Some(vec![32; 5000])));
    let _guard = c.bind_io(Some(&input), None, None).unwrap();
    c.set_resource_limits(ResourceLimits {
        allocation: Some(2000),
        ..Default::default()
    });
    assert_eq!(
        c.eval_string("(read)").unwrap_err().class(),
        ErrorClass::Control
    );
}

#[test]
fn scoped_string_input_closes_escaped_handles_and_restores_after_control_failure() {
    let mut c = Context::new();
    let outer = c.string_input("outer", utf16("outside\n"));
    let _outer = c.bind_io(Some(&outer), None, None).unwrap();
    c.eval_string("(def escaped (with-in-str \"inside\" *in*))")
        .unwrap();
    assert_eq!(
        c.eval_string("(read escaped)").unwrap_err().class(),
        ErrorClass::Host
    );
    c.set_resource_limits(ResourceLimits {
        fuel: Some(1000),
        ..Default::default()
    });
    assert_eq!(
        c.eval_string("(with-in-str \"inner\" (loop [] (recur)))")
            .unwrap_err()
            .class(),
        ErrorClass::Control
    );
    c.set_resource_limits(ResourceLimits::default());
    assert_eq!(
        text(&c.eval_string("(read-line)").unwrap()),
        utf16("outside")
    );
}

#[test]
fn language_read_uses_the_same_first_form_semantics_as_the_shared_reader_corpus() {
    let definition = io_definitions()
        .iter()
        .find(|d| d.metadata.name == "read")
        .unwrap();
    for case in protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap() {
        // Syntax quote allocates fresh names on each reading by contract.
        if case.source.contains('`') {
            continue;
        }
        let mut c = Context::new();
        let source = c.source(case.id, case.source);
        let expected = c.reader(&source).unwrap().read_form();
        let input = c.string_input(case.id, utf16(case.source));
        let actual = c.invoke_native(
            definition,
            &[input, c.boolean(false), c.keyword(Name::new(None, "eof"))],
        );
        match (expected, actual) {
            (Ok(Some(form)), Ok(value)) => assert_eq!(
                wire(&form.data()),
                wire(&value),
                "{} first form differs",
                case.id
            ),
            (Ok(None), Ok(value)) => {
                assert_eq!(value.inspect(), ValueView::Keyword(Name::new(None, "eof")))
            }
            (Err(_), Err(_)) => (),
            (expected, actual) => panic!("{}: {expected:?} != {actual:?}", case.id),
        }
    }
}

#[test]
fn host_code_stream_recovery_preserves_cursor_diagnostics_and_owner_checks() {
    let mut c = Context::new();
    let stream = c.input_stream("session.clj", Utf8Input(Cursor::new("1 ]\n2\n")));
    let form = c.read_input_form(&stream).unwrap().unwrap();
    assert_eq!(c.eval_form(&form).unwrap().inspect(), ValueView::Integer(1));
    c.recover_input(&stream).unwrap(); // A healthy cursor is unchanged.
    let error = c.read_input_form(&stream).unwrap_err();
    assert_eq!(error.span.source().name(), "session.clj");
    assert_eq!(error.span.bytes(), 2..3);
    assert!(c.read_input_form(&stream).unwrap().is_none());
    c.recover_input(&stream).unwrap();
    let form = c.read_input_form(&stream).unwrap().unwrap();
    assert_eq!(
        form.span().source().line_column(form.span().bytes().start),
        Some((2, 1))
    );
    assert_eq!(c.eval_form(&form).unwrap().inspect(), ValueView::Integer(2));
    assert!(c.read_input_form(&stream).unwrap().is_none());
    let mut other = Context::new();
    assert_eq!(
        other.read_input_form(&stream).unwrap_err().kind.class(),
        ErrorClass::Host
    );
    assert!(matches!(
        other.recover_input(&stream),
        Err(NativeError::WrongOwner)
    ));
    assert!(c.read_input_form(&c.nil()).is_err());
}

#[test]
fn stream_compaction_preserves_retained_forms_and_mixed_newline_locations() {
    let mut c = Context::new();
    let stream = c.input_stream(
        "compact.clj",
        Utf8Input(Cursor::new("1\r\n2\r3\r\u{c}4\n[\"λ\"\n 5]\n")),
    );
    let mut forms = Vec::new();
    while let Some(form) = c.read_input_form(&stream).unwrap() {
        forms.push(form);
        c.compact_input(&stream).unwrap();
        c.collect_garbage();
    }
    for (index, form) in forms.iter().enumerate() {
        assert_eq!(
            form.span().source().line_column(form.span().bytes().start),
            Some((index + 1, 1))
        );
    }
    assert_eq!(
        c.eval_form(&forms[0]).unwrap().inspect(),
        ValueView::Integer(1)
    );
    assert_eq!(forms[4].span().text(), "[\"λ\"\n 5]");
    assert_eq!(
        forms[4]
            .span()
            .source()
            .line_column(forms[4].span().bytes().end),
        Some((6, 4))
    );
    assert!(forms[4].span().source().text().len() < 20);
    assert!(matches!(
        Context::new().compact_input(&stream),
        Err(NativeError::WrongOwner)
    ));
}
