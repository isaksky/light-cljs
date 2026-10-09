use light_cljs::interpreter::{Context, HandleError, Name, ValueView};
use light_cljs::{NodeKind, parse};

#[path = "support/sci_protocol.rs"]
#[allow(dead_code)]
mod protocol;

#[test]
fn scalar_storage_matches_selected_pinned_sci_expectations() {
    // Storage only: these reference outputs are supplied to constructors. This
    // does not execute fixture source or claim reader/evaluator conformance.
    let cases = protocol::cases(include_str!("fixtures/sci-outcomes.tsv")).unwrap();
    let context = Context::new();
    for id in [
        "sci-scalars",
        "sci-integers",
        "sci-doubles",
        "sci-utf16",
        "sci-characters",
        "sci-symbols-keywords",
    ] {
        let case = cases.iter().find(|case| case.id == id).unwrap();
        let protocol::Outcome::Returned(protocol::Value::Vector(values)) = &case.outcome else {
            panic!("{id}")
        };
        for expected in values {
            use protocol::Value as V;
            let value = match expected {
                V::Nil => context.nil(),
                V::Bool(v) => context.boolean(*v),
                V::Int(v) => context.integer(*v),
                V::Float(v) => context.double(f64::from_bits(*v)),
                V::Char(v) => context.character(*v),
                V::String(v) => context.string(v.clone()),
                V::Symbol(ns, name) => context.symbol(Name::from_utf16(ns.as_deref(), name)),
                V::Keyword(ns, name) => context.keyword(Name::from_utf16(ns.as_deref(), name)),
                other => panic!("unexpected foundation fixture value: {other:?}"),
            };
            let actual = match value.inspect() {
                ValueView::Nil => V::Nil,
                ValueView::Boolean(v) => V::Bool(v),
                ValueView::Integer(v) => V::Int(v),
                ValueView::Double(v) => V::Float(v.to_bits()),
                ValueView::Character(v) => V::Char(v),
                ValueView::String(v) => V::String(v.to_vec()),
                ValueView::Symbol(v) => {
                    V::Symbol(v.namespace().map(<[u16]>::to_vec), v.name().to_vec())
                }
                ValueView::Keyword(v) => {
                    V::Keyword(v.namespace().map(<[u16]>::to_vec), v.name().to_vec())
                }
                other => panic!("unexpected scalar view: {other:?}"),
            };
            assert_eq!(&actual, expected, "{id}");
        }
    }
}

#[test]
fn values_and_unit_provenance_outlive_source_parser_storage_and_context() {
    let (value, unit) = {
        let context = Context::new();
        let original = String::from("; λ\r\n\"a\\uD800😀\"");
        let tree = parse(&original).unwrap();
        let node = &tree.nodes[tree.roots[0].0];
        let NodeKind::String(string) = &node.kind else {
            panic!("string")
        };
        let value = context.string(string.to_utf16(&original));
        let source = context.source("owned.clj", original.clone());
        let span = source.span(node.span.clone()).unwrap();
        assert_eq!(source.line_column(node.span.start), Some((2, 1)));
        let unit = context
            .executable_unit(std::slice::from_ref(&value), &[span])
            .unwrap();
        (value, unit)
    };
    let expected = ValueView::String(vec![0x61, 0xd800, 0xd83d, 0xde00].into());
    assert_eq!(value.inspect(), expected);
    let constant = unit.constant(0).unwrap();
    assert!(constant.same_identity(&value));
    assert_eq!(constant.inspect(), expected);
    let span = unit.span(0).unwrap();
    assert_eq!(span.text(), "\"a\\uD800😀\"");
    assert_eq!(span.source().name(), "owned.clj");
    assert_eq!(span.source().line_column(span.bytes().start), Some((2, 1)));
    assert_eq!(unit.constant_count(), 1);
    assert_eq!(unit.span_count(), 1);
    assert!(unit.constant(1).is_none());
    assert!(unit.span(1).is_none());
    drop((value, unit));
    assert_eq!(constant.inspect(), expected);
    assert_eq!(span.text(), "\"a\\uD800😀\"");
}

#[test]
fn source_spans_validate_utf8_boundaries_order_and_bounds() {
    let context = Context::new();
    let source = context.source("unicode.clj", "aλ\r\n😀\n");
    assert_eq!(source.span(1..3).unwrap().text(), "λ");
    assert_eq!(source.span(5..9).unwrap().text(), "😀");
    assert_eq!(source.span(10..10).unwrap().text(), "");
    for (start, end) in [(2, 3), (1, 2), (6, 9), (0, 11), (9, 5)] {
        assert_eq!(
            source.span(start..end).unwrap_err(),
            HandleError::InvalidSpan
        );
    }
    assert_eq!(source.line_column(5), Some((2, 1)));
    assert_eq!(source.line_column(10), Some((3, 1)));
    assert_eq!(source.line_column(11), None);
}

#[test]
fn identity_is_distinct_from_scalar_value_equality() {
    let a = Context::new();
    let b = Context::new();
    let left = a.string(vec![0xd800, 0, 0xdc00]);
    let right = a.string(vec![0xd800, 0, 0xdc00]);
    let foreign = b.string(vec![0xd800, 0, 0xdc00]);
    assert!(left.value_eq(&right));
    assert!(left.value_eq(&foreign));
    assert!(!left.same_identity(&right));
    assert!(!left.same_identity(&foreign));
    assert!(left.same_identity(&left.clone()));
    assert!(a.integer(i64::MAX).value_eq(&b.integer(i64::MAX)));
    assert!(!a.integer(1).value_eq(&a.double(1.0)));
    assert!(!a.integer(i64::MAX).value_eq(&a.double(i64::MAX as f64)));
    let minus_zero = a.double(-0.0);
    assert!(minus_zero.value_eq(&a.double(0.0)));
    assert!(!minus_zero.same_identity(&a.double(0.0)));
    let bits = 0xfff8_0000_0000_0042;
    let nan = a.double(f64::from_bits(bits));
    let ValueView::Double(actual) = nan.inspect() else {
        panic!("double")
    };
    assert_eq!(actual.to_bits(), bits);
    assert!(!nan.value_eq(&nan));
    assert!(nan.same_identity(&nan.clone()));
    assert_eq!(a.character(0xd800).inspect(), ValueView::Character(0xd800));
}

#[test]
fn interning_is_owner_scoped_and_keeps_symbol_keyword_categories_separate() {
    let context = Context::new();
    let name = Name::new(Some("alpha"), "x");
    let symbol = context.symbol(name.clone());
    let same = context.symbol(name.clone());
    let keyword = context.keyword(name.clone());
    let unqualified = context.symbol(Name::new(None, "x"));
    let foreign = Context::new().symbol(name);
    assert!(symbol.same_identity(&same));
    assert_eq!(symbol.symbol_id(), keyword.symbol_id());
    assert!(!symbol.value_eq(&keyword));
    assert!(!symbol.same_identity(&keyword));
    assert!(!symbol.value_eq(&unqualified));
    assert!(!symbol.same_identity(&foreign));
    assert!(symbol.value_eq(&foreign));
    assert_eq!(
        context
            .symbol_value(foreign.symbol_id().unwrap())
            .unwrap_err(),
        HandleError::WrongOwner
    );
    let unusual = Name::from_utf16(Some(&[0xd800]), &[0xdc00]);
    assert_eq!(
        context.symbol(unusual.clone()).inspect(),
        ValueView::Symbol(unusual)
    );
}

#[test]
fn raw_ids_do_not_root_and_recycled_slots_reject_old_generations() {
    let mut context = Context::new();
    let value = context.string(vec![1]);
    let name = context.symbol(Name::new(None, "old"));
    let source = context.source("old", "old");
    let unit = context.executable_unit(&[], &[]).unwrap();
    let (heap_id, name_id, source_id, unit_id) = (
        value.heap_id().unwrap(),
        name.symbol_id().unwrap(),
        source.id(),
        unit.id(),
    );
    let slots = context.memory_stats().slots;
    drop((value, name, source, unit));
    let after = context.collect_garbage().after;
    assert_eq!(
        (
            after.heap_values,
            after.names,
            after.sources,
            after.executable_units,
            after.roots
        ),
        (0, 0, 0, 0, 0)
    );
    let new_value = context.string(vec![2]);
    let new_name = context.keyword(Name::new(None, "new"));
    let new_source = context.source("new", "new");
    let new_unit = context.executable_unit(&[], &[]).unwrap();
    assert_eq!(
        context.memory_stats().slots,
        slots,
        "vacant slots must be reused"
    );
    assert_ne!(new_value.heap_id().unwrap(), heap_id);
    assert_ne!(new_name.symbol_id().unwrap(), name_id);
    assert_ne!(new_source.id(), source_id);
    assert_ne!(new_unit.id(), unit_id);
    assert_eq!(
        context.heap_value(heap_id).unwrap_err(),
        HandleError::StaleId
    );
    assert_eq!(
        context.symbol_value(name_id).unwrap_err(),
        HandleError::StaleId
    );
    assert_eq!(
        context.keyword_value(name_id).unwrap_err(),
        HandleError::StaleId
    );
    assert_eq!(
        context.source_by_id(source_id).unwrap_err(),
        HandleError::StaleId
    );
    assert_eq!(
        context.unit_by_id(unit_id).unwrap_err(),
        HandleError::StaleId
    );
    assert!(
        context
            .heap_value(new_value.heap_id().unwrap())
            .unwrap()
            .same_identity(&new_value)
    );
    assert!(
        context
            .keyword_value(new_name.symbol_id().unwrap())
            .unwrap()
            .same_identity(&new_name)
    );
    assert_eq!(context.source_by_id(new_source.id()).unwrap().text(), "new");
    assert_eq!(
        context.unit_by_id(new_unit.id()).unwrap().id(),
        new_unit.id()
    );
}

#[test]
fn reset_preserves_host_roots_and_rejects_every_old_or_foreign_handle() {
    let mut context = Context::new();
    let value = context.string(vec![0xd800]);
    let name = context.symbol(Name::new(None, "name"));
    let source = context.source("old", "λ");
    let span = source.span(0..2).unwrap();
    let unit = context
        .executable_unit(std::slice::from_ref(&value), std::slice::from_ref(&span))
        .unwrap();
    let scalar = context.integer(42);
    context.reset();
    // Occupy matching slots in the new owner to make aliasing bugs observable.
    let fresh_value = context.string(vec![7]);
    let fresh_name = context.symbol(Name::new(None, "different"));
    let fresh_source = context.source("new", "new");
    let fresh_unit = context.executable_unit(&[], &[]).unwrap();
    assert_eq!(
        context.heap_value(value.heap_id().unwrap()).unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        context.symbol_value(name.symbol_id().unwrap()).unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        context.source_by_id(source.id()).unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        context.unit_by_id(unit.id()).unwrap_err(),
        HandleError::WrongOwner
    );
    let before = context.memory_stats();
    assert_eq!(
        context
            .executable_unit(std::slice::from_ref(&value), &[])
            .unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        context.executable_unit(&[scalar], &[]).unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        context.executable_unit(&[], &[span]).unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        context.memory_stats(),
        before,
        "validation must precede allocation"
    );
    assert_eq!(value.inspect(), ValueView::String(vec![0xd800].into()));
    assert!(unit.constant(0).unwrap().same_identity(&value));
    assert_eq!(unit.span(0).unwrap().text(), "λ");
    let foreign = Context::new();
    assert_eq!(
        foreign.executable_unit(&[fresh_value], &[]).unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        foreign
            .keyword_value(fresh_name.symbol_id().unwrap())
            .unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        foreign.source_by_id(fresh_source.id()).unwrap_err(),
        HandleError::WrongOwner
    );
    assert_eq!(
        foreign.unit_by_id(fresh_unit.id()).unwrap_err(),
        HandleError::WrongOwner
    );
}

#[test]
fn units_keep_constants_and_sources_live_without_original_host_roots() {
    let mut context = Context::new();
    let unit = {
        let value = context.string(vec![0xdfff]);
        let name = context.keyword(Name::new(None, "constant"));
        let source = context.source("constant", ":constant");
        context
            .executable_unit(&[value, name], &[source.span(0..9).unwrap()])
            .unwrap()
    };
    let live = context.collect_garbage().after;
    assert_eq!(
        (
            live.heap_values,
            live.names,
            live.sources,
            live.executable_units,
            live.roots
        ),
        (1, 1, 1, 1, 1)
    );
    let value = unit.constant(0).unwrap();
    let span = unit.span(0).unwrap();
    drop(unit);
    let live = context.collect_garbage().after;
    assert_eq!(
        (
            live.heap_values,
            live.names,
            live.sources,
            live.executable_units
        ),
        (1, 0, 1, 0)
    );
    assert_eq!(value.inspect(), ValueView::String(vec![0xdfff].into()));
    assert_eq!(span.text(), ":constant");
    drop((value, span));
    let live = context.collect_garbage().after;
    assert_eq!(
        (
            live.heap_values,
            live.names,
            live.sources,
            live.executable_units,
            live.roots
        ),
        (0, 0, 0, 0, 0)
    );
}

#[test]
fn repeated_repl_sized_allocations_do_not_accumulate_interned_names_or_roots() {
    let mut context = Context::new();
    for index in 0..1_000 {
        let value = context.symbol(Name::new(None, &format!("generated-{index}")));
        let clone = value.clone();
        assert_eq!(context.memory_stats().roots, 1);
        drop(value);
        assert_eq!(context.collect_garbage().after.names, 1);
        assert!(matches!(clone.inspect(), ValueView::Symbol(_)));
        drop(clone);
        let after = context.collect_garbage().after;
        assert_eq!((after.names, after.roots, after.slots), (0, 0, 1));
    }
}

#[test]
fn retained_scalar_source_matches_sci_parse_plus_string_assertions() {
    // SCI 5fe89056384418be50d232b4278344cb3a1a526b,
    // test/sci/parse_test.cljc, parse+string-test, lines 23–31.
    // Adapt only the first scalar/source pair; see reference/sci/NOTICE.md.
    let (unit, original_span) = {
        let context = Context::new();
        let input = String::from("1 {:a   1} [1  2 3]");
        let mut parser = light_cljs::Parser::new(&input);
        let first = parser.read_form().unwrap().unwrap();
        let node = &parser.nodes()[first.0];
        assert!(matches!(
            node.kind,
            NodeKind::Number(light_cljs::NumberKind::Integer)
        ));
        let source = context.source("parse+string", input.clone());
        let span = source.span(node.span.clone()).unwrap();
        let value = context.integer(1);
        (
            context.executable_unit(&[value], &[span]).unwrap(),
            node.span.clone(),
        )
    };
    assert_eq!(unit.constant(0).unwrap().inspect(), ValueView::Integer(1));
    assert_eq!(unit.span(0).unwrap().text(), "1");
    assert_eq!(unit.span(0).unwrap().bytes(), original_span);
}

#[test]
fn host_string_conversion_can_reenter_context_without_an_arena_borrow() {
    struct HostUnits<'a>(&'a Context);
    impl From<HostUnits<'_>> for std::rc::Rc<[u16]> {
        fn from(host: HostUnits<'_>) -> Self {
            let nested = host.0.string(vec![5]);
            assert_eq!(nested.inspect(), ValueView::String(vec![5].into()));
            vec![7].into()
        }
    }
    let context = Context::new();
    assert_eq!(
        context.string(HostUnits(&context)).inspect(),
        ValueView::String(vec![7].into())
    );
}
